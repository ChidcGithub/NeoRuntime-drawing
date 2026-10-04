//! TexTeller 的本地 CPU 推理；输出原始 LaTeX，不执行、不纠正识别结果。
use board_core::StrokePoint;
use image::{GrayImage, imageops};
use ort::{
    execution_providers::CPUExecutionProvider,
    session::{
        Session,
        builder::GraphOptimizationLevel,
        run_options::{OutputSelector, RunOptions},
    },
    tensor::TensorElementType,
    value::{Tensor, ValueType},
};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};
use tiny_skia::{Paint, PathBuilder, Pixmap, Stroke, Transform};
use tokenizers::Tokenizer;

const SIDE: usize = 448;
const MEAN: f32 = 0.9545467;
const STD: f32 = 0.15394445;
const MAX_TOKENS: usize = 256;
const TIME_BUDGET: Duration = Duration::from_secs(60);
// 所有实例共用一个非排队推理槽；超时只在 ORT 调用之间检查，不强杀调用。
static INFERENCE_SLOT: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone)]
pub struct NeuralFormula {
    pub latex: String,
    /// 实际贪心选择词元（含 EOS、不含起始词元）的平均 log-softmax，非准确概率。
    pub mean_log_probability: f64,
    /// 仅真实生成 EOS 才为 true；达到时间或词元预算返回未完成结果。
    pub finished: bool,
    pub generated_tokens: usize,
}

/// 由实际 encoder 输入张量逆归一化得到，仅驻留内存，不参与序列化。
#[derive(Debug, Clone)]
pub struct NeuralInputPreview {
    pub size: [usize; 2],
    pub gray: Vec<u8>,
}

struct PreprocessedInk {
    pixels: Vec<f32>,
}

impl PreprocessedInk {
    fn new(strokes: &[Vec<StrokePoint>]) -> Result<Self, String> {
        Ok(Self {
            pixels: normalize(&rasterize(strokes)?)?,
        })
    }

    fn preview(&self) -> NeuralInputPreview {
        NeuralInputPreview {
            size: [SIDE, SIDE],
            // 包括归一化后的零填充：逆变换后为 MEAN（约 243），不是白色。
            gray: self
                .pixels
                .iter()
                .map(|p| ((p * STD + MEAN) * 255.0).round().clamp(0.0, 255.0) as u8)
                .collect(),
        }
    }

    fn into_tensor(self) -> Result<Tensor<f32>, String> {
        Tensor::from_array(([1, 1, SIDE, SIDE], self.pixels)).map_err(error)
    }
}

pub struct NeuralRecognizer {
    encoder: Session,
    decoder: Session,
    tokenizer: Tokenizer,
    model_dir: PathBuf,
    start_id: i64,
    eos_id: i64,
    vocab_size: usize,
}

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl NeuralRecognizer {
    pub fn load(model_dir: &Path) -> Result<Self, String> {
        let model_dir = model_dir
            .canonicalize()
            .map_err(|e| format!("模型目录不可用：{e}"))?;
        let config = read_json(&model_dir.join("config.json"), 64 * 1024)?;
        let generation = read_json(&model_dir.join("generation_config.json"), 64 * 1024)?;
        if config["encoder"]["image_size"] != 448 || config["encoder"]["num_channels"] != 1 {
            return Err("仅支持 TexTeller 448×448 单通道模型".into());
        }
        let token_id = |name: &str| -> Result<i64, String> {
            generation[name]
                .as_i64()
                .or_else(|| config[name].as_i64())
                .or_else(|| config["decoder"][name].as_i64())
                .ok_or_else(|| format!("模型配置缺少 {name}"))
        };
        let start_id = token_id("decoder_start_token_id")?;
        let eos_id = token_id("eos_token_id")?;
        let bos_id = token_id("bos_token_id")?;
        let pad_id = token_id("pad_token_id")?;
        let vocab_size = config["decoder"]["vocab_size"]
            .as_u64()
            .filter(|v| *v > 0 && *v <= 100_000)
            .ok_or("词表大小无效")? as usize;
        let tokenizer_path = model_dir.join("tokenizer.json");
        if fs::metadata(&tokenizer_path).map_err(error)?.len() > 8 * 1024 * 1024 {
            return Err("tokenizer.json 超过大小预算".into());
        }
        let tokenizer = Tokenizer::from_file(tokenizer_path).map_err(error)?;
        for (name, id) in [("<s>", bos_id), ("</s>", eos_id), ("<pad>", pad_id)] {
            if id < 0 || id as usize >= vocab_size || tokenizer.token_to_id(name) != Some(id as u32)
            {
                return Err(format!("配置与 tokenizer 的 {name} 不一致"));
            }
        }
        if start_id < 0 || start_id as usize >= vocab_size {
            return Err("decoder_start_token_id 越界".into());
        }
        let make_session = |name: &str| -> Result<Session, String> {
            Session::builder()
                .map_err(error)?
                .with_execution_providers([CPUExecutionProvider::default().build()])
                .map_err(error)?
                .with_intra_threads(4)
                .map_err(error)?
                .with_inter_threads(1)
                .map_err(error)?
                .with_optimization_level(GraphOptimizationLevel::Level3)
                .map_err(error)?
                .commit_from_file(model_dir.join(name))
                .map_err(|e| format!("加载 {name} 失败：{e}"))
        };
        let encoder = make_session("encoder_model.onnx")?;
        let decoder = make_session("decoder_model.onnx")?;
        // 不接受 merged / with_past 等不同签名，避免把输入错误误当识别结果。
        check_inputs(
            &encoder,
            &[(
                "pixel_values",
                TensorElementType::Float32,
                &[1, 1, 448, 448],
            )],
        )?;
        check_inputs(
            &decoder,
            &[
                ("input_ids", TensorElementType::Int64, &[1, -1]),
                (
                    "encoder_hidden_states",
                    TensorElementType::Float32,
                    &[1, 785, 768],
                ),
            ],
        )?;
        check_output(&encoder, "last_hidden_state", &[1, 785, 768])?;
        check_output(&decoder, "logits", &[1, -1, vocab_size as i64])?;
        Ok(Self {
            encoder,
            decoder,
            tokenizer,
            model_dir,
            start_id,
            eos_id,
            vocab_size,
        })
    }

    pub fn model_dir(&self) -> &Path {
        &self.model_dir
    }

    pub fn recognize(&mut self, strokes: &[Vec<StrokePoint>]) -> Result<NeuralFormula, String> {
        self.recognize_inner(strokes, None)
    }

    /// 即使推理失败也保留已准备的输入；预处理前失败则无预览。
    pub fn recognize_with_preview(
        &mut self,
        strokes: &[Vec<StrokePoint>],
    ) -> (Result<NeuralFormula, String>, Option<NeuralInputPreview>) {
        let mut preview = None;
        let result = self.recognize_inner(strokes, Some(&mut preview));
        (result, preview)
    }

    fn recognize_inner(
        &mut self,
        strokes: &[Vec<StrokePoint>],
        preview: Option<&mut Option<NeuralInputPreview>>,
    ) -> Result<NeuralFormula, String> {
        let _slot = INFERENCE_SLOT
            .try_lock()
            .map_err(|_| "神经识别槽正在使用，请等待当前任务完成".to_string())?;
        let started = Instant::now();
        let prepared = PreprocessedInk::new(strokes)?;
        if let Some(preview) = preview {
            *preview = Some(prepared.preview());
        }
        let input = prepared.into_tensor()?;
        let encoded = self
            .encoder
            .run(ort::inputs!["pixel_values" => input])
            .map_err(error)?;
        let hidden = encoded.get("last_hidden_state").ok_or("encoder 缺少输出")?;
        let (shape, _) = hidden.try_extract_tensor::<f32>().map_err(error)?;
        if shape.as_ref() != [1, 785, 768] {
            return Err(format!("encoder 输出形状错误：{shape:?}"));
        }
        let options = RunOptions::new()
            .map_err(error)?
            .with_outputs(OutputSelector::no_default().with("logits"));
        let mut ids = vec![self.start_id];
        let mut log_probability = 0.0;
        let mut finished = false;
        for _ in 0..MAX_TOKENS {
            if started.elapsed() >= TIME_BUDGET {
                break;
            }
            let input_ids = Tensor::from_array(([1, ids.len()], ids.clone())).map_err(error)?;
            // 每次提交完整前缀；复用 encoder 输出，不使用 past/key-value cache。
            let outputs = self
                .decoder
                .run_with_options(
                    ort::inputs![
                        "input_ids" => input_ids,
                        "encoder_hidden_states" => hidden,
                    ],
                    &options,
                )
                .map_err(error)?;
            let (shape, logits) = outputs["logits"]
                .try_extract_tensor::<f32>()
                .map_err(error)?;
            if shape.as_ref() != [1, ids.len() as i64, self.vocab_size as i64] {
                return Err(format!("decoder 输出形状错误：{shape:?}"));
            }
            let row = &logits[(ids.len() - 1) * self.vocab_size..];
            let (next, lp) = greedy(row)?;
            ids.push(next as i64);
            log_probability += lp;
            if next as i64 == self.eos_id {
                finished = true;
                break;
            }
        }
        let generated_tokens = ids.len() - 1;
        if generated_tokens == 0 {
            return Err("encoder 已耗尽识别时间预算，尚未生成词元".into());
        }
        let tokens: Vec<u32> = ids[1..].iter().map(|v| *v as u32).collect();
        let latex = self.tokenizer.decode(&tokens, true).map_err(error)?;
        Ok(NeuralFormula {
            latex,
            mean_log_probability: log_probability / generated_tokens as f64,
            finished,
            generated_tokens,
        })
    }
}

fn read_json(path: &Path, max_bytes: u64) -> Result<Value, String> {
    if fs::metadata(path).map_err(error)?.len() > max_bytes {
        return Err("模型配置超过大小预算".into());
    }
    serde_json::from_slice(&fs::read(path).map_err(error)?).map_err(error)
}

fn tensor_matches(value: &ValueType, kind: TensorElementType, expected: &[i64]) -> bool {
    matches!(value, ValueType::Tensor { ty, shape, .. } if *ty == kind
        && shape.len() == expected.len()
        && shape.iter().zip(expected).all(|(a,b)| *a == -1 || *b == -1 || a == b))
}

fn check_inputs(
    session: &Session,
    expected: &[(&str, TensorElementType, &[i64])],
) -> Result<(), String> {
    if session.inputs.len() != expected.len() {
        return Err(format!("不支持的 ONNX 输入：{:?}", session.inputs));
    }
    for (name, kind, shape) in expected {
        if !session
            .inputs
            .iter()
            .any(|v| v.name == *name && tensor_matches(&v.input_type, *kind, shape))
        {
            return Err(format!(
                "ONNX 输入签名不符：{name}；实际 {:?}",
                session.inputs
            ));
        }
    }
    Ok(())
}

fn check_output(session: &Session, name: &str, shape: &[i64]) -> Result<(), String> {
    if session.outputs.iter().any(|v| {
        v.name == name && tensor_matches(&v.output_type, TensorElementType::Float32, shape)
    }) {
        Ok(())
    } else {
        Err(format!("ONNX 输出签名不符：{name}"))
    }
}

fn greedy(logits: &[f32]) -> Result<(usize, f64), String> {
    if logits.is_empty() || logits.iter().any(|v| !v.is_finite()) {
        return Err("模型产生无效 logits".into());
    }
    let mut best = 0;
    for i in 1..logits.len() {
        if logits[i] > logits[best] {
            best = i;
        }
    }
    let max = logits[best] as f64;
    let sum: f64 = logits.iter().map(|v| (*v as f64 - max).exp()).sum();
    Ok((best, -sum.ln()))
}

fn rasterize(strokes: &[Vec<StrokePoint>]) -> Result<GrayImage, String> {
    if strokes.is_empty() || strokes.len() > super::MAX_STROKES {
        return Err("笔画数量须为 1..128".into());
    }
    let mut count = 0usize;
    let (mut x0, mut y0, mut x1, mut y1) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for stroke in strokes {
        count = count.checked_add(stroke.len()).ok_or("笔迹点数溢出")?;
        if stroke.is_empty() || count > super::MAX_POINTS {
            return Err("笔画不能为空，总点数不能超过 32768".into());
        }
        for p in stroke {
            if !p.x.is_finite()
                || !p.y.is_finite()
                || !p.time.is_finite()
                || p.time < 0.0
                || !p.pressure.is_finite()
                || !(0.0..=1.0).contains(&p.pressure)
            {
                return Err("笔迹包含非有限值或无效时间/压力".into());
            }
            x0 = x0.min(p.x as f64);
            y0 = y0.min(p.y as f64);
            x1 = x1.max(p.x as f64);
            y1 = y1.max(p.y as f64);
        }
    }
    // 用 f64 计算包围盒，极端但有限的 f32 坐标也不会相减溢出。
    let extent = (x1 - x0).max(y1 - y0).max(1e-6);
    let scale = 880.0 / extent;
    let width = ((x1 - x0) * scale).ceil() as u32 + 16;
    let height = ((y1 - y0) * scale).ceil() as u32 + 16;
    let mut pixmap = Pixmap::new(width, height).ok_or("无法创建笔迹图像")?;
    pixmap.fill(tiny_skia::Color::WHITE);
    let mut paint = Paint::default();
    paint.set_color_rgba8(0, 0, 0, 255);
    paint.anti_alias = true;
    let pen = Stroke {
        width: (((y1 - y0) * scale * 0.035) as f32).clamp(2.5, 10.0),
        line_cap: tiny_skia::LineCap::Round,
        line_join: tiny_skia::LineJoin::Round,
        ..Stroke::default()
    };
    let xy = |p: &StrokePoint| {
        (
            ((p.x as f64 - x0) * scale + 8.0) as f32,
            ((p.y as f64 - y0) * scale + 8.0) as f32,
        )
    };
    for stroke in strokes {
        let mut path = PathBuilder::new();
        let (x, y) = xy(&stroke[0]);
        path.move_to(x, y);
        // tiny-skia 的零长度路径不会画点，为点笔画添加亚像素短线。
        path.line_to(x + 0.01, y);
        for p in &stroke[1..] {
            let (x, y) = xy(p);
            path.line_to(x, y);
        }
        let path = path.finish().ok_or("笔迹路径无效")?;
        pixmap.stroke_path(&path, &paint, &pen, Transform::identity(), None);
    }
    let gray: Vec<u8> = pixmap.data().chunks_exact(4).map(|p| p[0]).collect();
    GrayImage::from_raw(width, height, gray).ok_or_else(|| "灰度图像大小错误".into())
}

fn normalize(image: &GrayImage) -> Result<Vec<f32>, String> {
    // 对照官方 texteller/utils/image.py：白边阈值 15，双三次抗锯齿，
    // Resize(short=447,max=448)，先归一化再仅在右侧/下侧填数值零。
    let (mut x0, mut y0, mut x1, mut y1) = (image.width(), image.height(), 0, 0);
    for (x, y, p) in image.enumerate_pixels() {
        if p[0] < 240 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x0 > x1 || y0 > y1 {
        return Err("笔迹图像为空白".into());
    }
    let cropped = imageops::crop_imm(image, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image();
    let (w, h) = cropped.dimensions();
    let (short, long) = (w.min(h) as u64, w.max(h) as u64);
    let mut new_short = 447u64;
    let mut new_long = 447 * long / short;
    if new_long > 448 {
        new_short = (448 * new_short / new_long).max(1);
        new_long = 448;
    }
    let (nw, nh) = if w <= h {
        (new_short, new_long)
    } else {
        (new_long, new_short)
    };
    let resized = imageops::resize(
        &cropped,
        nw as u32,
        nh as u32,
        imageops::FilterType::CatmullRom,
    );
    let mut result = vec![0.0; SIDE * SIDE];
    for (x, y, p) in resized.enumerate_pixels() {
        result[y as usize * SIDE + x as usize] = (p[0] as f32 / 255.0 - MEAN) / STD;
    }
    Ok(result)
}

#[cfg(test)]
#[path = "neural_tests.rs"]
mod tests;
