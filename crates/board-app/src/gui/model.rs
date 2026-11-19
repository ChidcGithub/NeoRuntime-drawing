// 模型加载/卸载与数学输入后端选择；后台任务助手。从 gui.rs 拆出（纯移动）。
use super::*;

impl BoardApp {
    pub(super) fn current_math_input(&self) -> MathInput {
        MathInput {
            expression: self.expression.clone(),
            numbers: [
                self.math_bounds.x_min,
                self.math_bounds.x_max,
                self.math_bounds.y_min,
                self.math_bounds.y_max,
                self.derivative_at,
            ]
            .map(f64::to_bits),
        }
    }

    pub(super) fn set_hwr_backend(&mut self, backend: HwrBackend) {
        if self.hwr_backend != backend {
            self.cancel_ink();
            self.expression.clear();
            self.hwr_backend = backend;
            if backend == HwrBackend::Template {
                self.unload_model();
            }
        }
    }

    pub(super) fn unload_model(&mut self) {
        self.cancel_ink();
        if self.neural_recognizer.is_some() {
            self.logger.info("model unloaded", &[]);
        }
        self.expression.clear();
        release_model(
            &mut self.model_loader,
            &mut self.neural_recognizer,
            &mut self.loaded_model_dir,
        );
        self.model_status = "未加载模型（CPU）".into();
    }

    pub(super) fn model_load_ready(&self) -> bool {
        self.hwr_backend == HwrBackend::TexTeller
            && !self.model_loader.busy()
            && !self.hwr_worker.busy()
    }

    pub(super) fn load_model(&mut self, ctx: &egui::Context) {
        if self.hwr_backend != HwrBackend::TexTeller || self.model_loader.busy() {
            return;
        }
        self.logger.info("model load started", &[]);
        self.cancel_ink();
        self.expression.clear();
        if !self.model_load_ready() {
            self.model_status = "等待旧识别任务排空后再加载（CPU）".into();
            return;
        }
        let path = PathBuf::from(self.model_dir.trim());
        if !path.is_absolute() {
            self.model_status = "请输入明确的绝对模型目录；不会扫描目录".into();
            return;
        }
        self.loaded_model_dir = self.model_dir.trim().to_owned();
        self.model_status = format!("后台加载中（CPU）：{}", path.display());
        start_model_load(
            &mut self.model_loader,
            &mut self.neural_recognizer,
            ctx,
            move || NeuralRecognizer::load(&path).map(|model| Arc::new(Mutex::new(model))),
        );
    }
}

pub(super) fn release_model<T: Send + 'static>(
    loader: &mut features::Background<std::result::Result<T, String>>,
    model: &mut Option<T>,
    loaded_dir: &mut String,
) {
    // Cancellation keeps the receiver busy until the old result has been dropped.
    loader.cancel();
    *model = None;
    loaded_dir.clear();
}

pub(super) fn start_model_load<T: Send + 'static>(
    loader: &mut features::Background<std::result::Result<T, String>>,
    model: &mut Option<T>,
    ctx: &egui::Context,
    load: impl FnOnce() -> std::result::Result<T, String> + Send + 'static,
) {
    let old = model.take();
    loader.start(ctx.clone(), move || {
        // Caller has drained hwr_worker, including its model Arc, before entering.
        drop(old);
        load()
    });
}

pub(super) fn poll_model_load<T: Send + 'static>(
    loader: &mut features::Background<std::result::Result<T, String>>,
    model: &mut Option<T>,
    loaded_dir: &mut String,
    model_dir: &str,
    backend: HwrBackend,
    status: &mut String,
) {
    if let Some(result) = loader.take() {
        if backend != HwrBackend::TexTeller
            || loaded_dir.is_empty()
            || loaded_dir != model_dir.trim()
        {
            *model = None;
            loaded_dir.clear();
            *status = "已丢弃过期加载结果；请重新加载（CPU）".into();
            return;
        }
        match result.and_then(|model| model) {
            Ok(loaded) => {
                *model = Some(loaded);
                *status = format!("模型已加载（CPU）：{loaded_dir}");
            }
            Err(error) => {
                *model = None;
                loaded_dir.clear();
                *status = format!("模型加载失败（CPU）：{error}");
            }
        }
    }
    // A cancelled result is consumed by take() without being returned.
    if !loader.busy() && model.is_none() {
        loaded_dir.clear();
    }
}

pub(super) struct HwrResult {
    pub(super) recognition: std::result::Result<Recognition, String>,
    pub(super) neural: Option<NeuralFormula>,
    pub(super) preview: Option<board_hwr::NeuralInputPreview>,
}

pub(super) struct InkDiagnostic {
    pub(super) request_id: String,
    pub(super) context: ContextToken,
    pub(super) stroke_points: Vec<usize>,
    pub(super) excluded: (usize, usize),
    pub(super) texture: Option<egui::TextureHandle>,
    pub(super) error: Option<String>,
}

impl From<std::result::Result<Recognition, String>> for HwrResult {
    fn from(recognition: std::result::Result<Recognition, String>) -> Self {
        Self {
            recognition,
            neural: None,
            preview: None,
        }
    }
}

pub(super) fn neural_result(raw: NeuralFormula) -> HwrResult {
    let recognition = latex_to_expression(&raw.latex).map(|text| Recognition {
        candidates: vec![board_hwr::Candidate {
            text: text.clone(),
            confidence: 0.0,
        }],
        text,
        confidence: 0.0,
        requires_confirmation: true,
        backend: "texteller-local".into(),
    });
    HwrResult {
        recognition: recognition.map_err(|error| {
            format!("LaTeX 转换失败：{error}；请在表达式输入框人工纠错后确认计算，原始 LaTeX 保留且不会自动修复")
        }),
        neural: Some(raw),
        preview: None,
    }
}

pub(super) const INT8_MODEL_SUGGESTION_FILES: [&str; 6] = [
    "encoder_model.onnx",
    "decoder_model.onnx",
    "config.json",
    "generation_config.json",
    "tokenizer.json",
    "optimization.json",
];

pub(super) fn suggested_model_dir(cwd: Option<&Path>, exe_dir: Option<&Path>) -> Option<PathBuf> {
    // Only fixed paths under these two locations; no traversal, scanning or loading.
    // Existence is a suggestion heuristic, not content/hash validation.
    for base in [cwd, exe_dir].into_iter().flatten() {
        let quantized = base.join("models").join("texteller-int8");
        if INT8_MODEL_SUGGESTION_FILES
            .iter()
            .all(|name| quantized.join(name).exists())
        {
            return Some(quantized);
        }
    }
    let relative = PathBuf::from("models").join("texteller");
    let base = cwd
        .filter(|p| p.join(&relative).is_dir())
        .or_else(|| exe_dir.filter(|p| p.join(&relative).is_dir()))
        .or(cwd)
        .or(exe_dir);
    base.map(|p| p.join(relative))
}

pub(super) fn default_model_dir() -> String {
    let cwd = std::env::current_dir().ok();
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));
    suggested_model_dir(cwd.as_deref(), exe.as_deref())
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}
