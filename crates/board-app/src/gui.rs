#[path = "gui/capture.rs"]
mod capture;
#[path = "gui/handwriting_ui.rs"]
mod handwriting_ui;
#[path = "icons.rs"]
mod icons;
use crate::{AppMode, Result, editing, emit, features, transport_event};
use board_core::{
    BoardObject, Color, ObjectKind, Operation, Point, ShapeKind, StrokePoint, Style, new_id,
};
use board_hwr::{
    AutoCalculate, CalculationRequest, ContextToken, InkRecognizer, NeuralFormula,
    NeuralRecognizer, Recognition, latex_to_expression,
};
use board_protocol::{Event, Message, Request, read_message};
use board_session::Session;
use egui::{Align2, Color32, Key, Pos2, Rect, Sense, Stroke, Vec2, ViewportCommand};
use icons::Icon;
use std::collections::HashMap;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

fn toolbar_scale(viewport: Vec2, compact: bool) -> f32 {
    let relative = (viewport.x / 1920.0).min(viewport.y / 1080.0);
    let fit = if compact {
        viewport.x / 120.0
    } else {
        viewport.x / 540.0
    };
    (relative.clamp(0.8, 1.5) * 1.1)
        .min(fit)
        .min(viewport.y / 120.0)
        .max(0.001)
}

fn toolbar_style(ui: &mut egui::Ui, scale: f32) {
    let style = ui.style_mut();
    for font in style.text_styles.values_mut() {
        font.size = (font.size * scale).max(12.0);
    }
    style.spacing.item_spacing *= scale;
    style.spacing.button_padding *= scale;
    style.spacing.interact_size = Vec2::splat(42.0 * scale);
}

// #region debug-point B:benchmark
pub(crate) mod debug_ink {
    #[cfg(test)]
    use std::{
        cell::RefCell,
        time::{Instant, SystemTime, UNIX_EPOCH},
    };

    #[derive(Clone, Copy)]
    pub struct Stamp(#[cfg(test)] Instant);
    pub fn enabled() -> bool {
        #[cfg(test)]
        if CAPTURE.with(|capture| capture.borrow().is_some()) {
            return true;
        }
        false
    }
    // #region debug-point C:backend-selection
    pub fn backend_from_env(
        debug: Option<&str>,
        backend: Option<&str>,
    ) -> Option<eframe::wgpu::Backends> {
        match (debug, backend) {
            (Some("1"), Some("dx12")) => Some(eframe::wgpu::Backends::DX12),
            (Some("1"), Some("vulkan")) => Some(eframe::wgpu::Backends::VULKAN),
            _ => None,
        }
    }
    // #endregion
    pub fn start() -> Option<Stamp> {
        #[cfg(test)]
        if enabled() {
            return Some(Stamp(Instant::now()));
        }
        None
    }
    pub fn micros(_start: Option<Stamp>) -> u64 {
        #[cfg(test)]
        if let Some(start) = _start {
            return start.0.elapsed().as_micros() as u64;
        }
        0
    }
    pub fn event(_hypothesis: &'static str, _location: &'static str, _data: serde_json::Value) {}

    // #region debug-point B:stage-timers
    #[cfg(test)]
    pub struct Stage {
        slot: usize,
        name: &'static str,
        started: Option<Stamp>,
    }
    #[cfg(test)]
    impl Stage {
        pub fn begin(slot: usize, name: &'static str) -> Self {
            Self {
                slot,
                name,
                started: start(),
            }
        }
    }
    #[cfg(test)]
    impl Drop for Stage {
        fn drop(&mut self) {
            let us = micros(self.started);
            record(self.slot, self.name, us);
        }
    }
    #[cfg(test)]
    pub fn record(_slot: usize, name: &'static str, us: u64) {
        CAPTURE.with(|capture| {
            let mut capture = capture.borrow_mut();
            if let Some(stages) = capture.as_mut() {
                let values = stages.entry(name).or_default();
                if values.len() < 2048 {
                    values.push(us);
                }
            }
        });
    }
    #[cfg(test)]
    thread_local! {
        static CAPTURE: RefCell<Option<std::collections::BTreeMap<&'static str, Vec<u64>>>> = const { RefCell::new(None) };
    }
    #[cfg(test)]
    pub fn capture_begin() {
        CAPTURE.with(|capture| *capture.borrow_mut() = Some(Default::default()));
    }
    #[cfg(test)]
    pub fn distribution(mut values: Vec<u64>) -> serde_json::Value {
        values.sort_unstable();
        let n = values.len();
        if n == 0 {
            return serde_json::json!({"n": 0});
        }
        let q = |p: usize| values[(n * p).div_ceil(100).saturating_sub(1)];
        serde_json::json!({
            "n": n, "p50_us": q(50), "p95_us": q(95), "p99_us": q(99), "max_us": values[n-1],
            "budget_us": 1_000_000.0 / 165.0,
            "over_budget_165hz": values.iter().filter(|&&us| us as f64 > 1_000_000.0 / 165.0).count(),
        })
    }
    #[cfg(test)]
    pub fn capture_end() -> serde_json::Value {
        CAPTURE.with(|capture| {
            let stages = capture.borrow_mut().take().unwrap();
            stages
                .into_iter()
                .map(|(name, values)| (name.to_owned(), distribution(values)))
                .collect::<serde_json::Map<_, _>>()
                .into()
        })
    }
    #[cfg(test)]
    pub fn report_benchmark(data: serde_json::Value) {
        let body = serde_json::json!({
            "sessionId": "fullscreen-ink-performance",

            "hypothesisId": "B", "location": "gui.rs:fullscreen_ink_performance_baseline",
            "msg": "[DEBUG] headless release phase summary", "data": data,
            "ts": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis(),
        })
        .to_string();
        eprintln!("{body}");
    }
    // #endregion

    #[cfg(test)]
    thread_local! {
        static LAST_FRAME: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
    }
    pub fn frame_gap() -> u64 {
        #[cfg(test)]
        if enabled() {
            let now = Instant::now();
            return LAST_FRAME
                .replace(Some(now))
                .map_or(0, |last| now.duration_since(last).as_micros() as u64);
        }
        0
    }
    pub fn sample(
        _slot: usize,
        _hypothesis: &'static str,
        _location: &'static str,
        _names: [&'static str; 4],
        _values: [u64; 4],
        _data: impl FnOnce() -> serde_json::Value,
    ) {
        #[cfg(test)]
        CAPTURE.with(|capture| {
            let mut capture = capture.borrow_mut();
            if let Some(stages) = capture.as_mut() {
                for (name, value) in _names.iter().zip(_values) {
                    let key = match *name {
                        "preview" => "canvas_preview",
                        "page_clone_apply" => "canvas_page_clone_apply",
                        "textures" => "canvas_textures",
                        "render" => "canvas_render",
                        _ => continue,
                    };
                    let values = stages.entry(key).or_default();
                    if values.len() < 2048 {
                        values.push(value);
                    }
                }
            }
        });
    }

    pub fn counts(page: &board_core::Page) -> serde_json::Value {
        let points: usize = page
            .objects
            .iter()
            .map(|o| match &o.kind {
                board_core::ObjectKind::Stroke { points, .. } => points.len(),
                board_core::ObjectKind::Shape { points, .. } => points.len(),
                _ => 0,
            })
            .sum();
        serde_json::json!({ "objects": page.objects.len(), "points": points })
    }

    pub struct Commit {
        start: Option<Stamp>,
        data: serde_json::Value,
    }
    impl Commit {
        pub(super) fn begin(app: &super::BoardApp) -> Option<Self> {
            if !enabled() || app.gesture.is_none() {
                return None;
            }
            let data = serde_json::json!({
                "page_before": counts(app.session.document.current_page()),
                "gesture_points": app.gesture.as_ref().map_or(0, |g| g.points.len()),
                "revision_before": app.session.document.revision,
                "can_undo_before": app.session.history.can_undo(),
                "can_redo_before": app.session.history.can_redo(),
            });
            Some(Self {
                start: start(),
                data,
            })
        }
    }
    impl Drop for Commit {
        fn drop(&mut self) {
            self.data["finish_gesture_us"] = micros(self.start).into();
            event(
                "B",
                "gui.rs:finish_gesture_at",
                std::mem::take(&mut self.data),
            );
        }
    }
}
// #endregion

// #region debug-point A:C:output
#[derive(Default)]
struct DebugInkOutput {
    last: Option<Instant>,
}
impl egui::Plugin for DebugInkOutput {
    fn debug_name(&self) -> &'static str {
        "ink-white-lag-output"
    }
    fn output_hook(&mut self, ctx: &egui::Context, output: &mut egui::FullOutput) {
        if self
            .last
            .is_some_and(|t| t.elapsed() < Duration::from_millis(500))
        {
            return;
        }
        self.last = Some(Instant::now());
        let screen = ctx.content_rect();
        let mut counts = [0_usize; 7];
        let mut max_alpha = 0_u8;
        fn inspect(
            shape: &egui::Shape,
            clip: Rect,
            screen: Rect,
            c: &mut [usize; 7],
            alpha: &mut u8,
        ) {
            c[0] += 1;
            match shape {
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        inspect(shape, clip, screen, c, alpha);
                    }
                }
                egui::Shape::Rect(rect) => {
                    c[1] += 1;
                    let coverage = rect.rect.intersect(clip).intersect(screen).area();
                    if coverage >= screen.area() * 0.95 && screen.area() > 0.0 {
                        c[2] += 1;
                        *alpha = (*alpha).max(rect.fill.a());
                        c[3] += usize::from(rect.fill.a() == 255);
                        c[4] += usize::from(rect.fill == Color32::WHITE);
                    }
                }
                egui::Shape::Mesh(mesh) => {
                    c[5] += mesh.vertices.len();
                    c[6] += mesh.indices.len();
                }
                _ => {}
            }
        }
        for clipped in &output.shapes {
            inspect(
                &clipped.shape,
                clipped.clip_rect,
                screen,
                &mut counts,
                &mut max_alpha,
            );
        }
        debug_ink::event(
            "A,C",
            "gui.rs:egui_output_hook",
            serde_json::json!({
                "shape_nodes": counts[0], "rects": counts[1], "screen_covering_rects": counts[2],
                "screen_covering_opaque_rects": counts[3], "screen_covering_white_rects": counts[4],
                "screen_covering_rect_max_alpha": max_alpha,
                "pre_tessellation_mesh_vertices": counts[5], "pre_tessellation_mesh_indices": counts[6],
                "texture_sets": output.textures_delta.set.len(), "texture_frees": output.textures_delta.free.len(),
                "scope": "final_egui_shapes_before_backend_tessellation_rect_alpha_only",
            }),
        );
    }
}
// #endregion

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Pen,
    Eraser,
    Select,
    Shape,
    Coordinates,
    Mouse,
}

#[derive(Clone, Copy, PartialEq)]
enum InkMathMode {
    Off,
    Confirm,
}

#[derive(Clone, Copy, PartialEq)]
enum HwrBackend {
    Template,
    TexTeller,
}

#[path = "cache.rs"]
mod cache;

type SharedNeural = Arc<Mutex<NeuralRecognizer>>;
type ModelLoad = std::result::Result<SharedNeural, String>;

fn release_model<T: Send + 'static>(
    loader: &mut features::Background<std::result::Result<T, String>>,
    model: &mut Option<T>,
    loaded_dir: &mut String,
) {
    // Cancellation keeps the receiver busy until the old result has been dropped.
    loader.cancel();
    *model = None;
    loaded_dir.clear();
}

fn start_model_load<T: Send + 'static>(
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

fn poll_model_load<T: Send + 'static>(
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

struct HwrResult {
    recognition: std::result::Result<Recognition, String>,
    neural: Option<NeuralFormula>,
    preview: Option<board_hwr::NeuralInputPreview>,
}

struct InkDiagnostic {
    request_id: String,
    context: ContextToken,
    stroke_points: Vec<usize>,
    excluded: (usize, usize),
    texture: Option<egui::TextureHandle>,
    error: Option<String>,
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

fn neural_result(raw: NeuralFormula) -> HwrResult {
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

const INT8_MODEL_SUGGESTION_FILES: [&str; 6] = [
    "encoder_model.onnx",
    "decoder_model.onnx",
    "config.json",
    "generation_config.json",
    "tokenizer.json",
    "optimization.json",
];

fn suggested_model_dir(cwd: Option<&Path>, exe_dir: Option<&Path>) -> Option<PathBuf> {
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

fn default_model_dir() -> String {
    let cwd = std::env::current_dir().ok();
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));
    suggested_model_dir(cwd.as_deref(), exe.as_deref())
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

struct Gesture {
    document: String,
    page: String,
    revision: u64,
    points: Vec<StrokePoint>,
    original: Option<BoardObject>,
    vertex: Option<usize>,
    resize: Option<usize>,
    erasing: Option<editing::ErasePreview>,
}

const PLOT_MERGE_LIMIT_HINT: &str = "合并超过 16 个不同表达式，仅移动并保留两图";

type MathResult = std::result::Result<(String, Option<ObjectKind>), String>;
type PersonalizedMathResult =
    std::result::Result<(String, Option<ObjectKind>, Option<ObjectKind>), String>;
type MathWork = (String, ContextToken, PersonalizedMathResult);
type PlotWork = (ContextToken, BoardObject, features::IntersectionReport);

fn personalize_math(
    result: MathResult,
    profile: Option<&crate::handwriting::Profile>,
    font: Option<&board_render::HandwritingFont>,
) -> PersonalizedMathResult {
    let (text, object) = result?;
    let Some(profile) = profile else {
        return Ok((text, object, None));
    };
    let Some(kind) = object else {
        return Ok((text, None, None));
    };
    if !matches!(kind, ObjectKind::Text { .. } | ObjectKind::Math { .. }) {
        return Ok((text, Some(kind), None));
    }
    match profile.render_adaptive(&kind, &text, |ch| {
        crate::handwriting_fallback::sample(ch, font)
    }) {
        Ok(handwritten) => Ok((text, Some(handwritten), Some(kind))),
        Err(error) => Ok((
            format!("{text}\n个人笔迹未应用，已保留标准字体：{error}"),
            Some(kind),
            None,
        )),
    }
}

struct PlotState {
    context: ContextToken,
    object: BoardObject,
    report: Option<std::result::Result<features::IntersectionReport, String>>,
    coordinate: Option<String>,
}

#[derive(Clone, PartialEq)]
struct MathInput {
    expression: String,
    numbers: [u64; 5],
}

struct CaptureContext {
    context: ContextToken,
    canvas_size: Vec2,
    stale: bool,
}

#[derive(Clone, Copy)]
struct WindowGeometry {
    size: Vec2,
    position: Option<Pos2>,
    maximized: bool,
}

struct BoardApp {
    mode: AppMode,
    session: Session,
    hosted: bool,
    incoming: Option<mpsc::Receiver<Message>>,
    tool: Tool,
    shape: ShapeKind,
    style: Style,
    eraser: f32,
    gesture: Option<Gesture>,
    selected: Option<String>,
    collapsed: bool,
    toolbar_hidden: bool,
    armed_toolbar_menu: Option<egui::Id>,
    thumbnails: HashMap<String, board_render::PageThumbnail>,
    expanded_geometry: Option<WindowGeometry>,
    files: bool,
    math: bool,
    path: String,
    export_path: String,
    discard_load: bool,
    status: String,
    expression: String,
    math_result: String,
    math_bounds: board_math::Bounds2D,
    derivative_at: f64,
    template_label: String,
    template_path: String,
    handwriting: handwriting_ui::HandwritingUi,
    handwriting_context: Option<(ContextToken, MathInput)>,
    ink_math_mode: InkMathMode,
    ink_ticket: Option<String>,
    calculating_ink: Option<Rect>,
    auto_gate: AutoCalculate,
    recognizer: InkRecognizer,
    hwr_backend: HwrBackend,
    model_dir: String,
    loaded_model_dir: String,
    model_status: String,
    neural_recognizer: Option<SharedNeural>,
    model_loader: features::Background<ModelLoad>,
    neural_raw: Option<NeuralFormula>,
    ink_diagnostic: Option<InkDiagnostic>,
    ink_excluded: (usize, usize),
    ink: Vec<Vec<StrokePoint>>,
    ink_sources: Vec<BoardObject>,
    ink_cache: cache::Cache,
    active_candidate: Option<String>,
    running_candidate: Option<(String, u64)>,
    recognition: Option<Recognition>,
    calculation: Option<CalculationRequest>,
    ink_context: Option<ContextToken>,
    authorization: Option<bool>,
    agent_prompt: String,
    authorize_assets: bool,
    authorize_write_back: bool,
    agent_image: Option<(ContextToken, String, String)>,
    jobs: Vec<(String, Instant)>,
    hwr_worker: features::Background<(CalculationRequest, HwrResult)>,
    plot_worker: features::Background<PlotWork>,
    math_worker: features::Background<MathWork>,
    math_ticket: Option<String>,
    math_input: Option<MathInput>,
    textures: HashMap<String, egui::TextureHandle>,
    plot_points: Option<PlotState>,
    split_drag: Option<(String, usize, ContextToken)>,
    image_path: String,
    captured_asset: Option<String>,
    capture_requests: HashMap<String, CaptureContext>,
    capture_jobs: HashMap<String, CaptureContext>,
    local_capture: Option<capture::LocalCapture>,
    capture_hide_window: bool,
    renderer: board_render::PageRenderer,
    export_resources: board_render::RenderResources,
    confirm_close: bool,
    allow_close: bool,
    controls: Vec<Rect>,
    canvas_size: Vec2,
    passthrough: bool,
    applied_window_request: Option<String>,
    #[cfg(test)]
    emitted: Vec<Message>,
}

pub fn run(mode: AppMode, hosted: bool) -> Result {
    let render_diagnostics = crate::render_diagnostics::install_from_env();
    let renderer = crate::render_diagnostics::select_renderer(
        mode,
        std::env::var("NEO_DRAW_RENDERER").ok().as_deref(),
    )?;
    let mut session = Session::new(mode.kind());
    session.attach_window();
    if hosted {
        // eframe 首次呈现会自动显示根窗口，因此 configure 前根本不创建窗口。
        match crate::hosted_startup::run(
            &mut session,
            &mut std::io::stdin().lock(),
            &mut std::io::stdout().lock(),
            &crate::recovery_directory(),
        )? {
            crate::hosted_startup::StartupOutcome::Configured => {}
            crate::hosted_startup::StartupOutcome::Closed
            | crate::hosted_startup::StartupOutcome::Disconnected => return Ok(()),
        }
    }
    let options = eframe::NativeOptions {
        renderer,
        viewport: egui::ViewportBuilder::default()
            .with_title(mode.title())
            .with_transparent(mode == AppMode::Drawing)
            .with_decorations(false)
            .with_maximized(true)
            .with_always_on_top()
            .with_visible(!hosted),
        ..Default::default()
    };
    // #region debug-point C:backend-override
    let debug_backend = debug_ink::backend_from_env(
        std::env::var("NEO_DRAW_DEBUG").ok().as_deref(),
        std::env::var("NEO_DRAW_DEBUG_BACKEND").ok().as_deref(),
    );
    let options = {
        let mut options = options;
        if let Some(backend) = debug_backend
            && let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) =
                &mut options.wgpu_options.wgpu_setup
        {
            setup.instance_descriptor.backends = backend;
        }
        // 只作单变量颜色抖动对照，不关闭抗锯齿或阴影。
        if std::env::var("NEO_DRAW_DEBUG").as_deref() == Ok("1")
            && std::env::var("NEO_DRAW_DEBUG_DITHERING").as_deref() == Ok("off")
        {
            options.dithering = false;
        }
        options
    };
    // #endregion
    // #region debug-point C:startup
    let debug_dithering = options.dithering;
    if render_diagnostics {
        eprintln!(
            "[render] app={mode:?} renderer={} transparent={:?} dithering={debug_dithering} backend_override={debug_backend:?}",
            options.renderer, options.viewport.transparent
        );
        if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &options.wgpu_options.wgpu_setup {
            eprintln!(
                "[render] backends={:?} dx12_presentation={:?}",
                setup.instance_descriptor.backends,
                setup
                    .instance_descriptor
                    .backend_options
                    .dx12
                    .presentation_system
            );
        }
    }
    if debug_ink::enabled() {
        debug_ink::event(
            "C",
            "gui.rs:run_native_options",
            serde_json::json!({
                "build": "ink-white-lag-instrumentation-v2-mesh-cache", "version": env!("CARGO_PKG_VERSION"),
                "debug_assertions": cfg!(debug_assertions), "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "mode": if mode == AppMode::Drawing { "drawing" } else { "blackboard" },
                "hosted": hosted, "renderer": options.renderer.to_string(),
                "transparent_requested": options.viewport.transparent,
                "decorations": options.viewport.decorations, "multisampling": options.multisampling,
                "dithering": debug_dithering,
                "debug_backend_override": debug_backend.map(|backend| format!("{backend:?}")),
                "dx12_presentation_system": match &options.wgpu_options.wgpu_setup {
                    eframe::egui_wgpu::WgpuSetup::CreateNew(setup) => Some(format!("{:?}", setup.instance_descriptor.backend_options.dx12.presentation_system)),
                    _ => None,
                },
            }),
        );
    }
    // #endregion
    eframe::run_native(mode.title(), options, Box::new(move |cc| {
        if render_diagnostics {
            eprintln!("[render] active_context: glow={} wgpu={}", cc.gl.is_some(), cc.wgpu_render_state.is_some());
        }
        if render_diagnostics && let Some(state) = &cc.wgpu_render_state {
            eprintln!("[render] adapter={:?} target_format={:?} pixels_per_point={} clear_rgba=[0,0,0,0]", state.adapter.get_info(), state.target_format, cc.egui_ctx.pixels_per_point());
        }
        // #region debug-point C:startup
        if debug_ink::enabled() {
            let adapter = cc.wgpu_render_state.as_ref().map(|state| state.adapter.get_info());
            let target_format = cc.wgpu_render_state.as_ref().map(|state| state.target_format);
            let visuals = &cc.egui_ctx.global_style().visuals;
            debug_ink::event("C", "gui.rs:creation_context", serde_json::json!({
                "wgpu_active": cc.wgpu_render_state.is_some(),
                "target_format": target_format.map(|format| format!("{format:?}")),
                "target_format_is_srgb": target_format.map(|format| format.is_srgb()),
                "window_shadow_rgba": visuals.window_shadow.color.to_array(),
                "popup_shadow_rgba": visuals.popup_shadow.color.to_array(),
                "shadow_rgba_encoding": "srgba_premultiplied_u8",
                "pixels_per_point": cc.egui_ctx.pixels_per_point(),
                "dithering": debug_dithering,
                "backend": adapter.as_ref().map(|a| format!("{:?}", a.backend)),
                "device_type": adapter.as_ref().map(|a| format!("{:?}", a.device_type)),
                "native_transparent": "not_exposed_by_winit",
                "native_window_present": cc.winit_window().is_some(),
            }));
            cc.egui_ctx.add_plugin(DebugInkOutput::default());
        }
        // #endregion
        let export_resources = load_font(&cc.egui_ctx);
        if !hosted {
            let request = Request::new("runtime:standalone", "configure", serde_json::json!({
                "classroom_safe": true, "desktop_capture_allowed": false, "agent_allowed": false,
            }))?;
            session.handle(request);
        }
        let incoming = if hosted {
            let (tx, rx) = mpsc::sync_channel(128);
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                let mut reader = std::io::stdin().lock();
                loop {
                    let message = match read_message(&mut reader) {
                        Ok(Some(message)) => message,
                        Ok(None) => break,
                        Err(error) => {
                            let fatal = matches!(error, board_protocol::TransportError::Io(_));
                            if tx.send(transport_event(&error)).is_err() { break; }
                            ctx.request_repaint();
                            if fatal { break; }
                            continue;
                        }
                    };
                    if tx.send(message).is_err() { break; }
                    ctx.request_repaint();
                }
            });
            Some(rx)
        } else { None };
        Ok(Box::new(BoardApp::new(mode, session, hosted, incoming, export_resources)))
    })).map_err(|e| e.to_string().into())
}

impl BoardApp {
    fn new(
        mode: AppMode,
        session: Session,
        hosted: bool,
        incoming: Option<mpsc::Receiver<Message>>,
        export_resources: board_render::RenderResources,
    ) -> Self {
        let mut handwriting = handwriting_ui::HandwritingUi::default();
        handwriting.enabled = mode == AppMode::Blackboard;
        let mut app = Self {
            mode,
            session,
            hosted,
            incoming,
            tool: Tool::Pen,
            shape: ShapeKind::Rectangle,
            style: Style {
                color: if mode == AppMode::Blackboard {
                    Color {
                        r: 245,
                        g: 245,
                        b: 235,
                        a: 255,
                    }
                } else {
                    Color {
                        r: 230,
                        g: 45,
                        b: 45,
                        a: 255,
                    }
                },
                ..Style::default()
            },
            eraser: 18.0,
            gesture: None,
            selected: None,
            collapsed: false,
            toolbar_hidden: false,
            armed_toolbar_menu: None,
            thumbnails: HashMap::new(),
            expanded_geometry: None,
            files: false,
            math: false,
            path: String::new(),
            export_path: String::new(),
            discard_load: false,
            status: "课堂安全模式：不采集桌面，不上传内容".into(),
            expression: String::new(),
            math_result: String::new(),
            ink_math_mode: InkMathMode::Off,
            ink_ticket: None,
            calculating_ink: None,
            math_bounds: board_math::Bounds2D {
                x_min: -10.0,
                x_max: 10.0,
                y_min: -10.0,
                y_max: 10.0,
            },
            derivative_at: 0.0,
            template_label: String::new(),
            template_path: String::new(),
            handwriting,
            handwriting_context: None,
            auto_gate: AutoCalculate::new(),
            recognizer: InkRecognizer::default(),
            hwr_backend: HwrBackend::Template,
            model_dir: default_model_dir(),
            loaded_model_dir: String::new(),
            model_status: "尚未加载；选择模型后点击后台加载".into(),
            neural_recognizer: None,
            model_loader: Default::default(),
            neural_raw: None,
            ink_diagnostic: None,
            ink_excluded: (0, 0),
            ink: Vec::new(),
            ink_sources: Vec::new(),
            ink_cache: Default::default(),
            active_candidate: None,
            running_candidate: None,
            recognition: None,
            calculation: None,
            ink_context: None,
            authorization: None,
            agent_prompt: String::new(),
            authorize_assets: false,
            authorize_write_back: false,
            agent_image: None,
            jobs: Vec::new(),
            hwr_worker: Default::default(),
            plot_worker: Default::default(),
            math_worker: Default::default(),
            math_ticket: None,
            math_input: None,
            textures: HashMap::new(),
            plot_points: None,
            split_drag: None,
            image_path: String::new(),
            renderer: board_render::PageRenderer::new(),
            export_resources: Default::default(),
            captured_asset: None,
            capture_requests: HashMap::new(),
            capture_jobs: HashMap::new(),
            local_capture: None,
            capture_hide_window: true,
            confirm_close: false,
            allow_close: false,
            controls: Vec::new(),
            canvas_size: Vec2::new(1280.0, 720.0),
            passthrough: false,
            applied_window_request: None,
            #[cfg(test)]
            emitted: Vec::new(),
        };
        app.replace_export_resources(export_resources);
        app
    }

    fn replace_export_resources(&mut self, resources: board_render::RenderResources) {
        // Font snapshots have no identity API, including Some -> Some replacement.
        self.handwriting.invalidate_context();
        self.cancel_math();
        self.export_resources = resources;
    }
}

fn read_font(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > board_render::MAX_FONT_BYTES as u64 {
        return Err(std::io::Error::other("字体超过 64 MiB"));
    }
    let mut bytes = Vec::new();
    file.take(board_render::MAX_FONT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > board_render::MAX_FONT_BYTES {
        return Err(std::io::Error::other("字体超过 64 MiB"));
    }
    Ok(bytes)
}

fn install_font(
    ctx: &egui::Context,
    resources: &mut board_render::RenderResources,
    bytes: Vec<u8>,
) -> board_render::Result<()> {
    // Validate before cloning or handing untrusted font bytes to egui.
    if bytes.len() > board_render::MAX_FONT_BYTES {
        return Err(board_render::RenderError::ResourceLimit("字体超过 64 MiB"));
    }
    resources.set_font(bytes.clone(), 0)?;
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "system-chinese".into(),
        egui::FontData::from_owned(bytes).into(),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "system-chinese".into());
    }
    ctx.set_fonts(fonts);
    Ok(())
}

fn load_font(ctx: &egui::Context) -> board_render::RenderResources {
    let mut resources = board_render::RenderResources::new();
    for path in [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
    ] {
        if let Ok(bytes) = read_font(std::path::Path::new(path)) {
            match install_font(ctx, &mut resources, bytes) {
                Ok(()) => return resources,
                Err(e) => eprintln!("导出字体不可用：{e}"),
            }
        }
    }
    eprintln!("未找到指定系统中文字体；界面使用内置字体。");
    resources
}

impl BoardApp {
    fn messages(&mut self, messages: Vec<Message>) {
        self.invalidate_captures();
        let mut captures = Vec::new();
        for message in messages {
            match &message {
                Message::Response(response) if response.id.starts_with("runtime:gui:") => {
                    if response.ok
                        && let Some(result) = &response.result
                        && result["status"] == "pending"
                        && let Some(id) = result["job_id"].as_str()
                    {
                        self.jobs.push((id.to_owned(), Instant::now()));
                    }
                    if let Some(capture) = self.capture_requests.remove(&response.id)
                        && response.ok
                        && let Some(id) =
                            response.result.as_ref().and_then(|r| r["job_id"].as_str())
                    {
                        self.capture_jobs.insert(id.to_owned(), capture);
                    }
                    self.status = if response.ok {
                        format!(
                            "请求已受理：{}",
                            response.result.as_ref().unwrap_or(&serde_json::Value::Null)
                        )
                    } else {
                        format!("请求被拒绝：{:?}", response.error)
                    };
                }
                Message::Event(event) if event.event == "document_changed" => {
                    // The renderer synchronizes changed/deleted objects by content and revision.
                    // Keep mathematical samples across move/resize commits and undo/redo;
                    // document replacement paths explicitly clear even for the same document ID.
                    self.thumbnails.clear();
                    self.textures.clear();
                    self.plot_worker.cancel();
                    self.plot_points = None;
                    self.cancel_authorization();
                }
                Message::Event(event) if event.event == "job.finished" => {
                    self.jobs
                        .retain(|(id, _)| Some(id.as_str()) != event.data["job_id"].as_str());
                    self.status = format!("任务响应：{}", event.data);
                    if let Some(capture) = event.data["job_id"]
                        .as_str()
                        .and_then(|id| self.capture_jobs.remove(id))
                        && event.data["ok"] == true
                        && let Some(asset) = event.data["result"]["asset_ref"].as_str()
                        && self.session.resources.get(asset).is_some()
                    {
                        captures.push((capture, asset.to_owned()));
                    }
                }
                _ => {}
            }
            #[cfg(test)]
            if !matches!(&message, Message::Response(r) if r.id.starts_with("runtime:gui:")) {
                self.emitted.push(message.clone());
            }
            if self.hosted
                && !matches!(&message, Message::Response(r) if r.id.starts_with("runtime:gui:"))
                && let Err(error) = emit(&message)
            {
                eprintln!("协议输出失败：{error}");
                self.disconnected();
            }
        }
        // Finish emitting the host batch before changing revision: never interleave newer
        // document events with the session's older response/state_changed snapshots.
        for (capture, asset) in captures {
            self.insert_capture(capture, asset);
        }
    }

    fn invalidate_captures(&mut self) {
        self.invalidate_local_capture(false);
        let context = ContextToken::capture(&self.session.document);
        for capture in self
            .capture_jobs
            .values_mut()
            .chain(self.capture_requests.values_mut())
        {
            capture.stale |= capture.context != context;
        }
    }

    fn insert_capture(&mut self, capture: CaptureContext, asset_ref: String) {
        self.captured_asset = Some(asset_ref.clone());
        if capture.stale || capture.context != ContextToken::capture(&self.session.document) {
            self.status =
                "截图已返回，但原页面/版本已变化；请在文件面板手动插入截图（不上传）".into();
            return;
        }
        let Some(resource) = self.session.resources.get(&asset_ref) else {
            return;
        };
        let area = (capture.canvas_size - Vec2::splat(80.0)).max(Vec2::splat(1.0));
        let factor = (area.x / resource.width as f32)
            .min(area.y / resource.height as f32)
            .min(1.0);
        let width = resource.width as f32 * factor;
        let height = resource.height as f32 * factor;
        let object = BoardObject {
            id: new_id(),
            kind: ObjectKind::Image {
                position: Point {
                    x: ((capture.canvas_size.x - width) / 2.0).max(0.0),
                    y: ((capture.canvas_size.y - height) / 2.0).max(0.0),
                },
                width,
                height,
                asset_ref,
            },
        };
        let image_id = object.id.clone();
        let s = &mut self.session;
        let page = s.document.current_page().id.clone();
        let revision = s.document.revision;
        match s.history.apply(
            &mut s.document,
            &page,
            revision,
            &[Operation::Add { object }],
        ) {
            Ok(()) => {
                self.captured_asset = None;
                self.gesture = None;
                self.split_drag = None;
                self.changed();
                self.tool = Tool::Select;
                self.selected = Some(image_id);
                self.status = "截图已插入并选中；单击图片可询问 Agent，拖动可移动，可一次撤销；未发送给 Agent".into();
            }
            Err(error) => self.status = format!("截图自动插入失败：{error}；可在文件面板手动插入"),
        }
    }

    fn disconnected(&mut self) {
        self.hosted = false;
        self.incoming = None;
        self.session.host_disconnected();
        self.capture_requests.clear();
        self.capture_jobs.clear();
        self.jobs.clear();
        self.cancel_authorization();
        self.plot_worker.cancel();
        self.plot_points = None;
        self.gesture = None;
        self.cancel_ink();
        self.status = "宿主已断开：可本地保存/退出；未确认停止的截图仍保持隐藏".into();
        match crate::recover_document(&mut self.session, &crate::recovery_directory()) {
            Ok(path) => {
                if let Some(path) = path {
                    self.status
                        .push_str(&format!("；恢复包：{}", path.display()));
                }
                // 隐藏租约期间绝不显示板书/确认窗口；保存恢复包后可安全退出本 app。
                if !self.session.effective_visible() {
                    self.allow_close = true;
                }
            }
            Err(e) => {
                self.status.push_str(&format!("；恢复失败：{e}"));
                eprintln!("{}", self.status);
            }
        }
    }
    fn incoming_message(&mut self, message: Message) {
        let before = ContextToken::capture(&self.session.document);
        let mut replaced = false;
        let mut capture_selection = None;
        match message {
            Message::Request(request) => {
                let replacing = matches!(request.method.as_str(), "document.open" | "document.new");
                let out = self.session.handle(request);
                replaced = replacing
                    && out.iter().any(
                        |message| matches!(message, Message::Response(response) if response.ok),
                    );
                self.messages(out);
            }
            Message::Response(response) => {
                let out = self.session.handle_response(response);
                let revision = self.session.document.revision;
                self.messages(out);
                if self.session.document.revision != revision {
                    capture_selection = self.selected.clone();
                }
            }
            Message::Event(event) if event.event == "protocol_error" => {
                self.messages(vec![event.into()]);
                if self.session.state()["pending_jobs"].as_u64().unwrap_or(0) > 0 {
                    self.disconnected();
                }
            }
            Message::Event(event) => {
                let out = self.session.handle_event(event);
                let revision = self.session.document.revision;
                self.messages(out);
                if self.session.document.revision != revision {
                    capture_selection = self.selected.clone();
                }
            }
        }
        self.invalidate_local_capture(replaced);
        if self.session.closed || self.session.state()["close_pending"] == true {
            self.cancel_local_capture();
        }
        if replaced {
            self.handwriting.invalidate_context();
            self.ink_cache.clear();
            self.textures.clear();
            self.renderer.clear();
            self.thumbnails.clear();
            self.captured_asset = None;
        }
        if replaced
            || before != ContextToken::capture(&self.session.document)
            || !self.session.effective_visible()
        {
            self.plot_worker.cancel();
            self.gesture = None;
            self.split_drag = None;
            // Only messages()' successful capture insertion changes revision here;
            // preserve its new selection, not a selection predating the host result.
            self.selected = capture_selection;
            self.plot_points = None;
            self.cancel_authorization();
            self.cancel_ink();
        }
        self.sync_ink_cache();
    }

    fn current_math_input(&self) -> MathInput {
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

    fn set_hwr_backend(&mut self, backend: HwrBackend) {
        if self.hwr_backend != backend {
            self.cancel_ink();
            self.expression.clear();
            self.hwr_backend = backend;
            if backend == HwrBackend::Template {
                self.unload_model();
            }
        }
    }

    fn unload_model(&mut self) {
        self.cancel_ink();
        self.expression.clear();
        release_model(
            &mut self.model_loader,
            &mut self.neural_recognizer,
            &mut self.loaded_model_dir,
        );
        self.model_status = "未加载模型（CPU）".into();
    }

    fn model_load_ready(&self) -> bool {
        self.hwr_backend == HwrBackend::TexTeller
            && !self.model_loader.busy()
            && !self.hwr_worker.busy()
    }

    fn load_model(&mut self, ctx: &egui::Context) {
        if self.hwr_backend != HwrBackend::TexTeller || self.model_loader.busy() {
            return;
        }
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

    fn poll_backend_ink(&mut self, ctx: &egui::Context, now: Instant) {
        match self.hwr_backend {
            HwrBackend::Template => {
                let recognizer = self.recognizer.clone();
                self.poll_ink(ctx, now, move |strokes| {
                    recognizer.recognize(strokes).map_err(|e| e.to_string())
                });
            }
            HwrBackend::TexTeller => {
                let Some(model) = self.neural_recognizer.clone() else {
                    return;
                };
                if self.model_loader.busy() || self.model_dir.trim() != self.loaded_model_dir {
                    return;
                }
                self.poll_ink_result(ctx, now, move |strokes| {
                    let Ok(mut model) = model.try_lock() else {
                        return Err("模型忙或锁异常，请稍后重新书写".to_string()).into();
                    };
                    let (result, preview) = model.recognize_with_preview(strokes);
                    let mut result = match result {
                        Ok(raw) => neural_result(raw),
                        Err(error) => Err(format!(
                            "本地模型推理未完成，请检查输入预览或重新识别：{error}"
                        ))
                        .into(),
                    };
                    result.preview = preview;
                    result
                });
            }
        }
    }

    fn set_ink_math_mode(&mut self, mode: InkMathMode) {
        if self.ink_math_mode != mode {
            self.cancel_ink();
            self.ink_math_mode = mode;
            self.auto_gate.set_enabled(mode != InkMathMode::Off);
        }
    }

    fn cancel_math(&mut self) {
        self.running_candidate = None;
        self.calculating_ink = None;
        self.math_ticket = None;
        self.math_input = None;
        self.math_worker.cancel();
    }

    fn start_math(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce() -> MathResult + Send + 'static,
    ) {
        if self.math_worker.busy() {
            self.math_result = "上一项计算仍在排空，请稍后重试".into();
            return;
        }
        let ticket = new_id();
        self.math_ticket = Some(ticket.clone());
        self.math_input = Some(self.current_math_input());
        let context = ContextToken::capture(&self.session.document);
        self.math_result = "后台计算中；修改输入、换页或取消将丢弃结果".into();
        // Freeze local style and font once; later learning cannot mutate this answer.
        // Profile validation bounds this clone to 65,536 sample points. The output
        // alphabet is only known inside work(), so filtering here would lose glyphs.
        let profile = self
            .handwriting
            .enabled
            .then(|| self.handwriting.profile.clone());
        let font = self.export_resources.handwriting_font();
        self.math_worker.start(ctx.clone(), move || {
            let result = personalize_math(work(), profile.as_ref(), font.as_ref());
            (ticket, context, result)
        });
    }

    fn poll_ink(
        &mut self,
        ctx: &egui::Context,
        now: Instant,
        recognize: impl FnOnce(&[Vec<StrokePoint>]) -> std::result::Result<Recognition, String>
        + Send
        + 'static,
    ) {
        self.poll_ink_result(ctx, now, move |strokes| recognize(strokes).into());
    }

    fn poll_ink_result(
        &mut self,
        ctx: &egui::Context,
        now: Instant,
        recognize: impl FnOnce(&[Vec<StrokePoint>]) -> HwrResult + Send + 'static,
    ) {
        let context = ContextToken::capture(&self.session.document);
        if !self.session.effective_visible()
            || self.ink_context.as_ref().is_some_and(|old| old != &context)
        {
            self.cancel_ink();
            return;
        }
        if self.ink_math_mode == InkMathMode::Off {
            // Off is the default steady state, not a user cancellation event.
            // Manual math is independent of recognition and must survive polling.
            self.cancel_recognition();
            return;
        }
        if !self.math_worker.busy()
            && !self.hwr_worker.busy()
            && !ctx.input(|i| i.pointer.any_down())
            && self.gesture.is_none()
            && let Some(request) = self.auto_gate.poll(now, &context)
        {
            self.save_candidate_edit();
            self.active_candidate = None;
            self.expression.clear();
            self.recognition = None;
            self.ink_ticket = Some(request.id().to_owned());
            let work = request.clone();
            self.neural_raw = None;
            self.ink_diagnostic =
                (self.hwr_backend == HwrBackend::TexTeller).then(|| InkDiagnostic {
                    request_id: request.id().to_owned(),
                    context: request.context.clone(),
                    stroke_points: request.strokes.iter().map(Vec::len).collect(),
                    excluded: self.ink_excluded,
                    texture: None,
                    error: None,
                });
            let (max_strokes, max_points) = match self.hwr_backend {
                HwrBackend::Template => (32, 8192),
                HwrBackend::TexTeller => (128, 32768),
            };
            self.hwr_worker.start(ctx.clone(), move || {
                let result = if work.strokes.len() > max_strokes
                    || work.strokes.iter().map(Vec::len).sum::<usize>() > max_points
                {
                    Err(format!(
                        "本次识别超过 GUI 预算（{max_strokes} 笔 / {max_points} 点），请分段书写"
                    ))
                    .into()
                } else {
                    recognize(&work.strokes)
                };
                (work, result)
            });
            self.math_result = "后台识别中；识别完成后点击笔迹右上角按钮才计算或绘图".into();
            self.status = self.math_result.clone();
            self.calculation = Some(request);
        }
    }

    fn poll_workers(&mut self, ctx: &egui::Context) {
        self.sync_ink_cache();
        if !self.session.effective_visible()
            || self.compact_entry()
            || self.allow_close
            || self.confirm_close
            || self
                .ink_context
                .as_ref()
                .is_some_and(|context| *context != ContextToken::capture(&self.session.document))
            || self.calculation.as_ref().is_some_and(|request| {
                request.context != ContextToken::capture(&self.session.document)
            })
        {
            self.cancel_ink();
        }
        if !self.session.effective_visible()
            || self.compact_entry()
            || self.allow_close
            || self.confirm_close
        {
            self.plot_worker.cancel();
            self.plot_points = None;
        }
        poll_model_load(
            &mut self.model_loader,
            &mut self.neural_recognizer,
            &mut self.loaded_model_dir,
            &self.model_dir,
            self.hwr_backend,
            &mut self.model_status,
        );

        if self
            .math_input
            .as_ref()
            .is_some_and(|input| input != &self.current_math_input())
        {
            self.cancel_math();
        }
        if let Some(result) = self.math_worker.take() {
            match result {
                Ok((ticket, context, result))
                    if self.math_ticket.as_ref() == Some(&ticket)
                        && context == ContextToken::capture(&self.session.document)
                        && self.session.effective_visible() =>
                {
                    self.math_ticket = None;
                    match result {
                        Ok((mut text, object, fallback)) => {
                            if let Some(kind) = object {
                                let candidate = self.running_candidate.take();
                                let insert = |app: &mut Self, kind| {
                                    if let Some((id, epoch)) = &candidate {
                                        if *epoch != app.ink_cache.epoch {
                                            return None;
                                        }
                                        app.add_candidate_result(id, kind)
                                    } else {
                                        let revision = app.session.document.revision;
                                        app.add(kind);
                                        Some(app.session.document.revision != revision)
                                    }
                                };
                                match insert(self, kind) {
                                    Some(true) => {}
                                    Some(false) => {
                                        let error = self.status.clone();
                                        // History failures are atomic; retry only the original
                                        // standard answer, using the same candidate identity.
                                        text = if let Some(standard) = fallback {
                                            match insert(self, standard) {
                                                Some(true) => format!(
                                                    "{text}\n个人笔迹写入失败，已改用标准字体：{error}"
                                                ),
                                                Some(false) => format!(
                                                    "个人笔迹写入失败：{error}\n标准答案写入失败：{}",
                                                    self.status
                                                ),
                                                None => format!(
                                                    "个人笔迹写入失败：{error}\n候选上下文已失效或结果已存在，未重试写入"
                                                ),
                                            }
                                        } else {
                                            error
                                        };
                                    }
                                    None => {
                                        text = "候选上下文已失效或结果已存在，未写入".into();
                                    }
                                }
                            }
                            self.status = text.clone();
                            self.math_result = text;
                        }
                        Err(error) => {
                            self.status = error.clone();
                            self.math_result = error;
                        }
                    }
                }
                Err(error) => self.math_result = error,
                _ => {}
            }
        }
        if !self.math_worker.busy() {
            self.running_candidate = None;
            self.calculating_ink = None;
            self.math_ticket = None;
            self.math_input = None;
        }
        if let Some(result) = self.hwr_worker.take() {
            match result {
                Ok((request, result))
                    if self.session.effective_visible()
                        && self.ink_math_mode != InkMathMode::Off
                        && self.ink_ticket.as_deref() == Some(request.id())
                        && self.calculation.as_ref() == Some(&request)
                        && request.context == ContextToken::capture(&self.session.document) =>
                {
                    self.ink_ticket = None;
                    if let Some(diagnostic) = &mut self.ink_diagnostic
                        && diagnostic.request_id == request.id()
                        && diagnostic.context == request.context
                    {
                        diagnostic.error = result.recognition.as_ref().err().cloned();
                        diagnostic.texture = result.preview.map(|preview| {
                            ctx.load_texture(
                                format!("texteller-input-{}", request.id()),
                                egui::ColorImage::from_gray(preview.size, &preview.gray),
                                egui::TextureOptions::NEAREST,
                            )
                        });
                    }
                    self.neural_raw = result.neural;
                    match result.recognition {
                        Ok(recognition) => {
                            self.expression = recognition.text.clone();
                            self.math_result = if features::function_expression(&self.expression)
                                .is_some()
                            {
                                "函数候选：点击笔迹右上角曲线坐标图标生成函数图像，也可在数学/设置中纠错"
                                    .into()
                            } else {
                                "候选已识别：点击笔迹右上角计算器图标计算，也可在数学/设置中纠错"
                                    .into()
                            };
                            self.recognition = Some(recognition);
                        }
                        Err(error) => {
                            self.math_result = format!(
                                "识别失败，未写入，请点击纠错图标查看原输入并手动纠错：{error}"
                            )
                        }
                    }
                    self.active_candidate = self.ink_cache.insert(
                        &self.session.document,
                        request,
                        self.ink_sources.clone(),
                        self.recognition.clone(),
                        self.neural_raw.clone(),
                        self.hwr_backend,
                    );
                    if self.active_candidate.is_none() {
                        self.calculation = None;
                        self.math_result =
                            "候选未缓存：缺少合法原笔迹或达到内存安全预算；已有候选仍保留".into();
                    }
                    self.status = self.math_result.clone();
                }
                Err(error) => self.math_result = error,
                _ => {}
            }
        }
        self.sync_plot_state();
        if let Some(result) = self.plot_worker.take() {
            match result {
                Ok((context, object, report))
                    if context == ContextToken::capture(&self.session.document)
                        && self.selected_object().as_ref() == Some(&object) =>
                {
                    if let Some(state) = &mut self.plot_points {
                        state.report = Some(Ok(report));
                    }
                }
                Err(error) => {
                    if let Some(state) = &mut self.plot_points {
                        state.report = Some(Err(features::plot_text(&error)));
                    }
                }
                _ => {}
            }
        }
    }

    fn cancel_job(&mut self, id: &str) {
        self.cancel_authorization();
        if let Ok(request) = Request::new(
            format!("runtime:gui:{}", new_id()),
            "jobs.cancel",
            serde_json::json!({"job_id": id}),
        ) {
            let out = self.session.handle(request);
            self.messages(out);
        }
    }

    fn save_candidate_edit(&mut self) {
        if let Some(entry) = self
            .active_candidate
            .as_deref()
            .and_then(|id| self.ink_cache.get_mut(id))
            && entry.expression != self.expression
            && self.expression.len() <= 4096
        {
            entry.expression = self.expression.clone();
        }
    }

    fn sync_ink_cache(&mut self) {
        self.ink_cache.sync(&self.session.document);
        if self.active_candidate.as_deref().is_some_and(|id| {
            self.ink_cache.get(id).is_none_or(|entry| {
                entry.request.context.page_id != self.session.document.current_page().id
            })
        }) {
            self.cancel_ink();
        }
    }

    fn activate_candidate(&mut self, id: &str) -> bool {
        self.sync_ink_cache();
        self.save_candidate_edit();
        let Some(entry) = self.ink_cache.get(id) else {
            return false;
        };
        if entry.request.context.page_id != self.session.document.current_page().id {
            return false;
        }
        self.expression = entry.expression.clone();
        self.recognition = entry.recognition.clone();
        self.neural_raw = entry.neural.clone();
        let mut request = entry.request.clone();
        request.context = ContextToken::capture(&self.session.document);
        self.ink_context = Some(request.context.clone());
        self.calculation = Some(request);
        self.active_candidate = Some(id.to_owned());
        true
    }

    // None means no insertion was attempted, not a transaction failure to retry.
    fn add_candidate_result(&mut self, id: &str, kind: ObjectKind) -> Option<bool> {
        self.sync_ink_cache();
        let entry = self.ink_cache.get(id)?;
        if entry.result_present
            || entry.request.context.page_id != self.session.document.current_page().id
        {
            return None;
        }
        // This identity is already excluded from local validation before changed() runs.
        let result_id = entry.result_id.clone();
        let revision = self.session.document.revision;
        self.apply(vec![Operation::Add {
            object: BoardObject {
                id: result_id,
                kind,
            },
        }]);
        Some(self.session.document.revision != revision)
    }

    fn ink_started(&mut self) {
        self.save_candidate_edit();
        self.active_candidate = None;
        self.auto_gate.input_started();
        self.ink_ticket = None;
        self.cancel_math();
        self.hwr_worker.cancel();
        self.recognition = None;
        self.neural_raw = None;
        self.ink_diagnostic = None;
        self.calculation = None;
    }

    fn cancel_ink(&mut self) {
        self.cancel_math();
        self.cancel_recognition();
    }

    fn cancel_recognition(&mut self) {
        self.save_candidate_edit();
        self.active_candidate = None;
        self.ink_sources.clear();
        self.ink_ticket = None;
        self.hwr_worker.cancel();
        self.auto_gate.cancel();
        self.ink.clear();
        self.ink_excluded = (0, 0);
        self.ink_diagnostic = None;
        self.ink_context = None;
        self.recognition = None;
        self.neural_raw = None;
        self.calculation = None;
    }
    fn changed(&mut self) {
        self.cancel_ink();
        self.sync_ink_cache();
        self.plot_worker.cancel();
        self.plot_points = None;
        self.messages(vec![
            Event::new("document_changed", self.session.state()).into(),
            Event::new("state_changed", self.session.state()).into(),
        ]);
    }
    fn apply(&mut self, operations: Vec<Operation>) {
        if operations.is_empty() {
            return;
        }
        let s = &mut self.session;
        let page = s.document.current_page().id.clone();
        let revision = s.document.revision;
        // #region debug-point D:history
        let debug_started = debug_ink::start();
        // #endregion
        let result = s
            .history
            .apply(&mut s.document, &page, revision, &operations);
        // #region debug-point D:history
        let debug_us = debug_ink::micros(debug_started);
        // #region debug-point B:history
        #[cfg(test)]
        debug_ink::record(7, "history_apply", debug_us);
        // #endregion
        if debug_started.is_some() {
            debug_ink::event(
                "B",
                "gui.rs:history.apply",
                serde_json::json!({
                    "history_us": debug_us, "operations": operations.len(), "ok": result.is_ok(),
                    "revision_before": revision, "revision_after": s.document.revision,
                    "page_after": debug_ink::counts(s.document.current_page()),
                    "can_undo": s.history.can_undo(), "can_redo": s.history.can_redo(),
                }),
            );
        }
        // #endregion
        match result {
            Ok(()) => self.changed(),
            Err(error) => self.status = error.to_string(),
        }
    }
    fn commit_line(&mut self, object: BoardObject, adding: bool, endpoints: &[usize]) {
        let s = &mut self.session;
        match editing::commit_line(&mut s.document, &mut s.history, object, adding, endpoints) {
            Ok(()) => self.changed(),
            Err(e) => self.status = e.to_string(),
        }
    }
    fn add(&mut self, kind: ObjectKind) {
        self.apply(vec![Operation::Add {
            object: BoardObject { id: new_id(), kind },
        }]);
    }
    fn undo(&mut self, redo: bool) {
        self.cancel_ink();
        self.gesture = None;
        let s = &mut self.session;
        let result = if redo {
            s.history.redo(&mut s.document)
        } else {
            s.history.undo(&mut s.document)
        };
        match result {
            Ok(true) => self.changed(),
            Ok(false) => {}
            Err(e) => self.status = e.to_string(),
        }
    }
    fn save(&mut self) -> bool {
        if self.path.trim().is_empty() {
            self.files = true;
            self.status = "请先输入保存路径".into();
            return false;
        }
        match self.session.save_document(self.path.trim()) {
            Ok(()) => {
                self.status = "已保存".into();
                self.changed();
                true
            }
            Err(e) => {
                self.status = e.message;
                false
            }
        }
    }
    fn compact_entry(&self) -> bool {
        self.collapsed && self.mode == AppMode::Blackboard
    }

    fn set_collapsed(&mut self, ctx: &egui::Context, collapsed: bool) {
        if self.collapsed == collapsed {
            return;
        }
        self.collapsed = collapsed;
        self.toolbar_hidden = false;
        self.enforce_tool_mode();
        self.gesture = None;
        self.split_drag = None;
        self.cancel_ink();
        self.cancel_authorization();
        self.plot_worker.cancel();
        self.plot_points = None;
        self.files = false;
        self.math = false;
        self.confirm_close = false;
        egui::Popup::close_all(ctx);
        if self.mode == AppMode::Blackboard {
            if collapsed {
                self.expanded_geometry = Some(ctx.input(|i| {
                    WindowGeometry {
                        size: i
                            .viewport()
                            .inner_rect
                            .map_or(self.canvas_size, |r| r.size()),
                        position: i.viewport().outer_rect.map(|r| r.min),
                        maximized: i.viewport().maximized.unwrap_or(false),
                    }
                }));
                ctx.send_viewport_cmd(ViewportCommand::Maximized(false));
                if let Some(position) = ctx.input(|i| i.viewport().inner_rect.map(|r| r.min)) {
                    ctx.send_viewport_cmd(ViewportCommand::OuterPosition(position));
                }
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::new(220.0, 64.0)));
                ctx.send_viewport_cmd(ViewportCommand::Resizable(false));
            } else if let Some(geometry) = self.expanded_geometry.take() {
                ctx.send_viewport_cmd(ViewportCommand::Resizable(true));
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(geometry.size));
                if let Some(position) = geometry.position {
                    ctx.send_viewport_cmd(ViewportCommand::OuterPosition(position));
                }
                ctx.send_viewport_cmd(ViewportCommand::Maximized(geometry.maximized));
            }
        }
        ctx.request_repaint();
    }

    fn entry(&mut self, ctx: &egui::Context) {
        self.controls.clear();
        // Retry after unmaximizing has reached the native window (some WMs defer it).
        if ctx.input(|i| {
            i.viewport().maximized == Some(false)
                && i.viewport()
                    .inner_rect
                    .is_some_and(|r| r.size() != Vec2::new(220.0, 64.0))
        }) {
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::new(220.0, 64.0)));
        }
        let entry = egui::Area::new("blackboard_entry".into())
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("展开黑板").clicked() {
                            self.set_collapsed(ctx, false);
                        }
                        if ui.button("退出").clicked() {
                            self.request_close(ctx);
                        }
                    });
                });
            });
        self.controls.push(entry.response.rect);
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        self.enforce_tool_mode();
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.armed_toolbar_menu = None;
            self.toolbar_hidden = false;
            self.tool = Tool::Pen;
            self.set_collapsed(ctx, false);
            self.gesture = None;
            self.confirm_close = false;
            self.files = false;
            self.math = false;
            self.cancel_authorization();
            self.split_drag = None;
            self.cancel_ink();
            self.handwriting.invalidate_context();
            self.plot_worker.cancel();
            self.plot_points = None;
            // 原生命中检测由 logic 统一恢复，避免 egui 命令与 passthrough 缓存不同步。
            ctx.request_repaint();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::S)) {
            self.save();
        }
        if !ctx.egui_wants_keyboard_input() {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::Z)) {
                if self.math && self.handwriting.wants_sampling_undo(ctx) {
                    self.handwriting.undo_sampling();
                } else {
                    self.undo(false);
                }
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::Y)) {
                self.undo(true);
            }
        }
    }
    fn toolbar(&mut self, ctx: &egui::Context) {
        // #region debug-point B:toolbar
        #[cfg(test)]
        let _debug_stage = debug_ink::Stage::begin(3, "toolbar");
        // #endregion
        if self.compact_entry() {
            self.entry(ctx);
            return;
        }
        self.controls.clear();
        self.enforce_tool_mode();
        let content = ctx.content_rect();
        let narrow = content.width() < 320.0;
        let compact = self.collapsed || self.toolbar_hidden || narrow;
        if compact {
            self.armed_toolbar_menu = None;
        }
        // 10% larger than the 1920×1080 baseline; viewport fitting still takes priority.
        let scale = toolbar_scale(content.size(), compact);
        let size = 42.0 * scale;
        let margin = (6.0 * scale).round() as i8;
        let frame_width = 2.0 * (f32::from(margin) + scale);
        let spacing = 4.0 * scale;
        let edge = 8.0 * scale;
        let gap = 12.0 * scale;
        let left_width = size + frame_width;
        let right_width = size * 2.0 + 56.0 * scale + spacing * 2.0 + frame_width;
        let width = if compact {
            if self.collapsed {
                size * 2.0 + spacing
            } else {
                size
            }
        } else {
            (content.width() - 2.0 * (edge + left_width.max(right_width) + gap) - frame_width)
                .clamp(1.0, 502.0 * scale)
        };
        let lower_switch = self.mode == AppMode::Drawing && !compact;
        let bottom = content.bottom()
            - if lower_switch {
                (58.0 * scale).max(52.0)
            } else {
                edge
            };
        let bar = egui::Area::new("tools".into())
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_BOTTOM, [0.0, bottom - content.bottom()])
            .movable(false)
            .default_size(Vec2::new(width + frame_width, size + frame_width))
            .show(ctx, |ui| {
                toolbar_style(ui, scale);
                egui::Frame::popup(ui.style())
                    .inner_margin(margin)
                    .stroke(egui::Stroke::new(scale, ui.visuals().window_stroke.color))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.set_height(size);
                        ui.spacing_mut().item_spacing.x = spacing;
                        ui.spacing_mut().interact_size = Vec2::splat(size);
                        if compact {
                            ui.horizontal(|ui| {
                                if icons::button(ui, Icon::Expand, "显示工具栏", size, false, true)
                                    .clicked()
                                {
                                    self.toolbar_hidden = false;
                                    self.set_collapsed(ctx, false);
                                }
                                if self.collapsed
                                    && icons::button(
                                        ui,
                                        Icon::Exit,
                                        "退出（未保存时确认）",
                                        size,
                                        false,
                                        true,
                                    )
                                    .clicked()
                                {
                                    self.request_close(ctx);
                                }
                            });
                            return;
                        }
                        // 更多与隐藏固定在中岛最右侧，其余工具可横向滚动。
                        let row = Rect::from_min_size(ui.cursor().min, Vec2::new(width, size));
                        ui.scope_builder(
                            egui::UiBuilder::new().max_rect(Rect::from_min_size(
                                Pos2::new(row.right() - size, row.top()),
                                Vec2::splat(size),
                            )),
                            |ui| {
                                if icons::button(
                                    ui,
                                    Icon::Collapse,
                                    "隐藏工具栏（保留板书）",
                                    size,
                                    false,
                                    true,
                                )
                                .clicked()
                                {
                                    self.toolbar_hidden = true;
                                    self.gesture = None;
                                    self.tool = Tool::Pen;
                                    egui::Popup::close_all(ctx);
                                }
                            },
                        );

                        ui.scope_builder(
                            egui::UiBuilder::new().max_rect(Rect::from_min_size(
                                Pos2::new(row.right() - size * 2.0 - spacing, row.top()),
                                Vec2::splat(size),
                            )),
                            |ui| {
                                let response =
                                    icons::button(ui, Icon::More, "更多 / 状态", size, false, true);
                                self.upward_popup(&response, false, |app, ui| app.more_menu(ui));
                            },
                        );
                        let left = Rect::from_min_max(
                            row.min,
                            Pos2::new(row.right() - (size + spacing) * 2.0, row.bottom()),
                        );
                        ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
                            egui::ScrollArea::horizontal()
                                .id_salt("toolbar_row")
                                .min_scrolled_width(1.0)
                                .max_width(left.width().max(1.0))
                                .max_height(size)
                                .auto_shrink([false, false])
                                .scroll_bar_visibility(
                                    egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                                )
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        self.toolbar_menu(ui, Icon::Pen, "画笔", |app, ui| {
                                            app.brush_menu(ui)
                                        });
                                        self.toolbar_menu(ui, Icon::Eraser, "橡皮", |app, ui| {
                                            app.eraser_menu(ui)
                                        });
                                        if icons::button(
                                            ui,
                                            Icon::Select,
                                            "选择 / 顶点编辑",
                                            size,
                                            self.tool == Tool::Select,
                                            true,
                                        )
                                        .clicked()
                                        {
                                            self.tool = Tool::Select;
                                        }
                                        if icons::button(
                                            ui,
                                            Icon::Line,
                                            "线段",
                                            size,
                                            self.tool == Tool::Shape
                                                && self.shape == ShapeKind::Line,
                                            true,
                                        )
                                        .clicked()
                                        {
                                            self.tool = Tool::Shape;
                                            self.shape = ShapeKind::Line;
                                        }
                                        self.toolbar_menu(
                                            ui,
                                            Icon::Shapes,
                                            "其他形状",
                                            |app, ui| app.shape_menu(ui),
                                        );
                                        for (redo, icon, label, enabled) in [
                                            (
                                                false,
                                                Icon::Undo,
                                                "撤销",
                                                self.session.history.can_undo(),
                                            ),
                                            (
                                                true,
                                                Icon::Redo,
                                                "重做",
                                                self.session.history.can_redo(),
                                            ),
                                        ] {
                                            if icons::button(ui, icon, label, size, false, enabled)
                                                .clicked()
                                            {
                                                self.undo(redo);
                                            }
                                        }
                                        self.toolbar_menu(
                                            ui,
                                            Icon::File,
                                            "文件 / 保存 / 载入 / 导出",
                                            |app, ui| app.file_menu(ui),
                                        );
                                        self.toolbar_menu(
                                            ui,
                                            Icon::Settings,
                                            "数学 / 模型 / 截图 / AI",
                                            |app, ui| app.settings_menu(ui),
                                        );
                                    })
                                });
                        });
                        ui.allocate_rect(row, Sense::hover());
                    });
            });
        self.controls.insert(0, bar.response.rect);
        if !self.collapsed && !narrow {
            for (id, anchor, x, island_width) in [
                ("board_management", Align2::LEFT_BOTTOM, edge, left_width),
                ("board_pages", Align2::RIGHT_BOTTOM, -edge, right_width),
            ] {
                let island = egui::Area::new(id.into())
                    .order(egui::Order::Foreground)
                    .anchor(anchor, [x, bottom - content.bottom()])
                    .movable(false)
                    .default_size(Vec2::new(island_width, size + frame_width))
                    .show(ctx, |ui| {
                        toolbar_style(ui, scale);
                        egui::Frame::popup(ui.style())
                            .inner_margin(margin)
                            .stroke(egui::Stroke::new(scale, ui.visuals().window_stroke.color))
                            .show(ui, |ui| {
                                ui.set_width(island_width - frame_width);
                                ui.set_height(size);
                                ui.set_clip_rect(ui.max_rect().intersect(content));
                                ui.spacing_mut().item_spacing.x = spacing;
                                ui.horizontal(|ui| {
                                    if id == "board_management" {
                                        let label = if self.mode == AppMode::Blackboard {
                                            "黑板菜单"
                                        } else {
                                            "画板菜单"
                                        };
                                        let response =
                                            icons::button(ui, Icon::More, label, size, false, true);
                                        self.upward_popup(&response, false, |app, ui| {
                                            app.management_menu(ui)
                                        });
                                    } else {
                                        self.page_controls(ui, size);
                                    }
                                });
                            });
                    });
                self.controls.push(island.response.rect);
                #[cfg(test)]
                ctx.data_mut(|d| d.insert_temp(egui::Id::new(id), island.response.rect));
            }
        }
        if lower_switch {
            let area = egui::Area::new("drawing_passthrough".into())
                .order(egui::Order::Foreground)
                .anchor(Align2::CENTER_BOTTOM, [0.0, -4.0])
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style())
                        .inner_margin(2)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let active = self.tool == Tool::Mouse;
                                let icon = icons::button(
                                    ui,
                                    Icon::Mouse,
                                    "鼠标穿透：点击退出；Esc 恢复画笔",
                                    36.0,
                                    active,
                                    cfg!(windows),
                                );
                                let label = ui.add_enabled(
                                    cfg!(windows),
                                    egui::Button::new(if active {
                                        "退出鼠标穿透"
                                    } else {
                                        "鼠标穿透"
                                    })
                                    .selected(active),
                                );
                                if icon.clicked() || label.clicked() {
                                    self.set_mouse_passthrough(!active);
                                }
                            });
                        });
                });
            self.controls.push(area.response.rect);
        }
    }

    fn enforce_tool_mode(&mut self) {
        if self.mode == AppMode::Blackboard && self.tool == Tool::Mouse {
            self.tool = Tool::Pen;
        }
    }

    fn set_mouse_passthrough(&mut self, enabled: bool) {
        self.tool = if enabled && self.mode == AppMode::Drawing && cfg!(windows) {
            Tool::Mouse
        } else {
            Tool::Pen
        };
        self.gesture = None;
        if self.tool == Tool::Mouse {
            self.files = false;
            self.math = false;
        }
    }

    fn management_menu(&mut self, ui: &mut egui::Ui) {
        let collapse = if self.mode == AppMode::Blackboard {
            "收起黑板"
        } else {
            "收起画板工具"
        };
        if ui.button(collapse).clicked() {
            self.set_collapsed(ui.ctx(), true);
            ui.close();
        }
        let exit = if self.mode == AppMode::Blackboard {
            "退出黑板"
        } else {
            "退出画板"
        };
        if ui.button(exit).clicked() {
            self.request_close(ui.ctx());
            ui.close();
        }
    }

    fn more_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("画笔", |ui| self.brush_menu(ui));
        ui.menu_button("其他形状", |ui| self.shape_menu(ui));
        if ui.button("选择 / 顶点编辑").clicked() {
            self.tool = Tool::Select;
            ui.close();
        }
        ui.menu_button("橡皮", |ui| {
            if ui.button("使用橡皮").clicked() {
                self.tool = Tool::Eraser;
                ui.close();
            }
            self.eraser_menu(ui);
        });
        ui.menu_button("文件 / 保存 / 载入 / 导出", |ui| self.file_menu(ui));
        ui.menu_button("数学 / 模型 / 截图 / AI", |ui| self.settings_menu(ui));
        if ui
            .add_enabled(self.session.history.can_undo(), egui::Button::new("撤销"))
            .clicked()
        {
            self.undo(false);
            ui.close();
        }
        if ui
            .add_enabled(self.session.history.can_redo(), egui::Button::new("重做"))
            .clicked()
        {
            self.undo(true);
            ui.close();
        }
        ui.menu_button("状态与任务", |ui| {
            ui.label(&self.status);
            ui.label("仅覆盖当前窗口所在显示器；跨屏请先恢复画笔。");
            ui.label("模板识别32笔/8192点；交点16曲线，最多136次搜索，每次2048求值；宿主任务60秒请求取消。");
            for (id, _) in self.jobs.clone() { if ui.button(format!("取消任务 {id}")).clicked() { self.cancel_job(&id); } }
        });
    }

    fn page_controls(&mut self, ui: &mut egui::Ui, size: f32) {
        let current = self.session.document.current_page;
        let count = self.session.document.pages.len();
        if icons::button(ui, Icon::Previous, "上一页", size, false, current > 0).clicked() {
            self.select_page(current - 1);
        }
        let response = ui
            .add_sized(
                [56.0 * size / 42.0, size],
                egui::Button::new(
                    egui::RichText::new(format!("{}/{}", current + 1, count))
                        .size(12.0 * size / 42.0),
                ),
            )
            .on_hover_text("页面管理：预览所有页面");
        #[cfg(test)]
        ui.ctx()
            .data_mut(|data| data.insert_temp(egui::Id::new("page_count_rect"), response.rect));
        let command = self.toolbar_popup_command(&response, false);
        if let Some(popup) = egui::Popup::menu(&response)
            .open_memory(command)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .align(egui::emath::RectAlign::TOP_END)
            .align_alternatives(&[])
            .width((ui.ctx().content_rect().width() - 32.0).clamp(1.0, 280.0))
            .show(|ui| self.pages_menu(ui))
        {
            self.controls.push(popup.response.rect);
        }
        let last = current + 1 == count;
        if icons::button(
            ui,
            if last { Icon::Plus } else { Icon::Next },
            if last { "添加页面" } else { "下一页" },
            size,
            false,
            !last || count < board_core::MAX_PAGES,
        )
        .clicked()
        {
            if last {
                let s = &mut self.session;
                match s.history.edit(&mut s.document, |d| d.add_page()) {
                    Ok(_) => {
                        self.gesture = None;
                        self.selected = None;
                        self.changed();
                    }
                    Err(e) => self.status = e.to_string(),
                }
            } else {
                self.select_page(current + 1);
            }
        }
    }

    fn select_page(&mut self, page: usize) {
        if self.session.document.set_current_page(page).is_ok() {
            self.gesture = None;
            self.selected = None;
            self.changed();
        }
    }

    fn toolbar_popup_command(
        &mut self,
        response: &egui::Response,
        requires_second_click: bool,
    ) -> Option<egui::SetOpenCommand> {
        let popup_id = egui::Popup::default_response_id(response);
        if response.clicked() {
            if !requires_second_click
                || egui::Popup::is_id_open(&response.ctx, popup_id)
                || self.armed_toolbar_menu == Some(response.id)
            {
                self.armed_toolbar_menu = None;
                return Some(egui::SetOpenCommand::Toggle);
            }
            egui::Popup::close_all(&response.ctx);
            self.armed_toolbar_menu = Some(response.id);
        } else if self.armed_toolbar_menu == Some(response.id)
            && (response.clicked_elsewhere() || response.ctx.input(|i| i.key_pressed(Key::Escape)))
        {
            self.armed_toolbar_menu = None;
        }
        None
    }

    fn upward_popup(
        &mut self,
        response: &egui::Response,
        requires_second_click: bool,
        content: impl FnOnce(&mut Self, &mut egui::Ui),
    ) {
        let command = self.toolbar_popup_command(response, requires_second_click);
        if let Some(popup) = egui::Popup::menu(response)
            .open_memory(command)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .align(egui::emath::RectAlign::TOP_START)
            .align_alternatives(&[])
            .width((response.ctx.content_rect().width() - 32.0).clamp(1.0, 280.0))
            .show(|ui| {
                let max_height =
                    (response.rect.top() - ui.ctx().content_rect().top() - 24.0).max(1.0);
                egui::ScrollArea::vertical()
                    .max_height(max_height)
                    .show(ui, |ui| content(self, ui));
            })
        {
            self.controls.push(popup.response.rect);
        }
    }

    fn toolbar_menu(
        &mut self,
        ui: &mut egui::Ui,
        icon: Icon,
        label: &str,
        content: impl FnOnce(&mut Self, &mut egui::Ui),
    ) {
        ui.push_id(label, |ui| {
            let tool = match icon {
                Icon::Pen => Some(Tool::Pen),
                Icon::Eraser => Some(Tool::Eraser),
                _ => None,
            };
            let size = ui.spacing().interact_size.y;
            let response = icons::button(ui, icon, label, size, tool == Some(self.tool), true);
            if response.clicked()
                && let Some(tool) = tool
            {
                self.tool = tool;
            }
            self.upward_popup(&response, tool.is_some(), content);
        });
    }

    fn shape_menu(&mut self, ui: &mut egui::Ui) {
        for shape in SHAPES {
            if ui
                .selectable_label(
                    self.tool == Tool::Shape && self.shape == *shape,
                    shape_name(*shape),
                )
                .clicked()
            {
                self.shape = *shape;
                self.tool = Tool::Shape;
                ui.close();
            }
        }
        if self.mode == AppMode::Blackboard
            && ui
                .selectable_label(self.tool == Tool::Coordinates, "坐标系")
                .clicked()
        {
            self.tool = Tool::Coordinates;
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(
                self.selected.is_some(),
                egui::Button::new("断开所选直线的连接"),
            )
            .clicked()
            && let Some(id) = self.selected.clone()
        {
            let s = &mut self.session;
            match editing::disconnect_line(&mut s.document, &mut s.history, &id) {
                Ok(()) => self.changed(),
                Err(e) => self.status = e.to_string(),
            }
        }
        ui.label("端点吸附连接；拖角调整圆/椭圆")
            .on_hover_text("直线连接后随目标移动，拖端点可重新连接；不支持二维/三维混接。\n选择圆/椭圆后，拖角手柄调整大小，拖轮廓移动；固定对角，圆等比、椭圆分轴。");
    }

    fn brush_menu(&mut self, ui: &mut egui::Ui) {
        ui.label("画笔颜色");
        ui.horizontal(|ui| {
            for (color, name) in [
                (Color32::RED, "红色"),
                (Color32::BLACK, "黑色"),
                (Color32::WHITE, "白色"),
                (Color32::YELLOW, "黄色"),
                (Color32::GREEN, "绿色"),
                (Color32::BLUE, "蓝色"),
                (Color32::from_rgb(170, 70, 230), "紫色"),
            ] {
                let response = ui
                    .add_sized(
                        [24.0, 24.0],
                        egui::Button::new("")
                            .fill(color)
                            .selected(self.style.color == core_color(color)),
                    )
                    .on_hover_text(name);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name)
                });
                if response.clicked() {
                    self.style.color = core_color(color);
                }
            }
        });
        let mut color = egui_color(self.style.color);
        if ui.color_edit_button_srgba(&mut color).changed() {
            self.style.color = core_color(color);
        }
        ui.add(egui::Slider::new(&mut self.style.width, 0.5..=40.0).text("粗细"));
        ui.checkbox(&mut self.style.dashed, "虚线");
    }

    fn eraser_menu(&mut self, ui: &mut egui::Ui) {
        ui.add(egui::Slider::new(&mut self.eraser, 2.0..=80.0).text("橡皮半径"));
        if ui
            .button("全部擦除")
            .on_hover_text("清除当前页全部对象（可撤销），保留其他页面")
            .clicked()
        {
            self.clear_current_page();
            ui.close();
        }
    }

    fn clear_current_page(&mut self) {
        if self.session.document.current_page().objects.is_empty() {
            return;
        }
        let s = &mut self.session;
        // A page can exceed MAX_OPERATIONS; clear objects and connections in one transaction.
        match s.history.edit(&mut s.document, |d| {
            let page = &mut d.pages[d.current_page];
            page.objects.clear();
            d.connections
                .retain(|connection| connection.page_id != page.id);
            Ok(())
        }) {
            Ok(()) => {
                self.gesture = None;
                self.selected = None;
                self.changed();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn pages_menu(&mut self, ui: &mut egui::Ui) {
        // #region debug-point B:thumbnails
        #[cfg(test)]
        let _debug_stage = debug_ink::Stage::begin(6, "pages_menu_thumbnails");
        // #endregion
        let current = self.session.document.current_page;
        let count = self.session.document.pages.len();
        ui.label(format!("第 {} / {} 页", current + 1, count));
        let mut selected = None;
        let mut visible = Vec::new();
        let height = (ui.ctx().content_rect().height() - 210.0).clamp(60.0, 420.0);
        egui::ScrollArea::vertical()
            .id_salt("page_previews")
            .max_height(height)
            .show_rows(ui, 112.0, count, |ui, rows| {
                for index in rows {
                    let page = &self.session.document.pages[index];
                    visible.push(page.id.clone());
                    let (rect, response) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 112.0),
                        Sense::click(),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Button,
                            true,
                            current == index,
                            format!("第 {} 页", index + 1),
                        )
                    });
                    if ui.is_rect_visible(rect) {
                        let target = Rect::from_min_max(
                            rect.min + Vec2::new(4.0, 4.0),
                            rect.max - Vec2::new(4.0, 24.0),
                        );
                        for object in &page.objects {
                            if let ObjectKind::Image { asset_ref, .. } = &object.kind
                                && !self.textures.contains_key(asset_ref)
                                && let Some(resource) = self.session.resources.get(asset_ref)
                                && let Ok(image) = features::decode_png(&resource.bytes)
                            {
                                self.textures.insert(
                                    asset_ref.clone(),
                                    ui.ctx().load_texture(
                                        asset_ref,
                                        image,
                                        egui::TextureOptions::LINEAR,
                                    ),
                                );
                            }
                        }
                        let version = (
                            self.session.document.id.as_str(),
                            self.session.document.revision,
                        );
                        if !self
                            .thumbnails
                            .get(&page.id)
                            .is_some_and(|p| p.matches(version))
                        {
                            self.thumbnails.insert(
                                page.id.clone(),
                                board_render::PageThumbnail::new(page, version),
                            );
                        }
                        let resources =
                            |asset: &str| self.textures.get(asset).map(egui::TextureHandle::id);
                        if self.thumbnails[&page.id]
                            .paint(
                                ui.painter(),
                                target,
                                self.canvas_size,
                                self.mode == AppMode::Blackboard,
                                &resources,
                            )
                            .is_err()
                        {
                            ui.painter()
                                .rect_filled(target, 0.0, Color32::from_gray(40));
                            ui.painter().text(
                                target.center(),
                                Align2::CENTER_CENTER,
                                "预览失败",
                                egui::FontId::proportional(14.0),
                                Color32::LIGHT_RED,
                            );
                        }
                        ui.painter().rect_stroke(
                            rect.shrink(1.0),
                            3.0,
                            Stroke::new(
                                if current == index { 2.0 } else { 1.0 },
                                if current == index {
                                    ui.visuals().selection.stroke.color
                                } else {
                                    ui.visuals().widgets.noninteractive.bg_stroke.color
                                },
                            ),
                            egui::StrokeKind::Inside,
                        );
                        ui.painter().text(
                            Pos2::new(rect.center().x, rect.bottom() - 12.0),
                            Align2::CENTER_CENTER,
                            format!("第 {} 页", index + 1),
                            egui::FontId::proportional(14.0),
                            ui.visuals().text_color(),
                        );
                    }
                    if response.clicked() {
                        selected = Some(index);
                    }
                }
            });
        self.thumbnails.retain(|id, _| visible.contains(id));
        if let Some(index) = selected {
            self.select_page(index);
            ui.close();
        }
        if ui.button("清屏（可撤销）").clicked() {
            self.clear_current_page();
        }
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        if ui.button("保存到当前路径").clicked() {
            self.save();
            ui.close();
        }
        if ui.button("文件面板：路径 / 载入 / 图片 / 导出").clicked() {
            self.files = !self.files;
            self.tool = Tool::Pen;
            ui.close();
        }
    }

    fn settings_menu(&mut self, ui: &mut egui::Ui) {
        if self.mode == AppMode::Blackboard && ui.button("数学 / 手写识别 / 模型设置").clicked()
        {
            self.math = !self.math;
            self.tool = Tool::Pen;
            ui.close();
        }
        if self.hosted {
            let mut required = true;
            ui.add_enabled(false, egui::Checkbox::new(&mut required, "截图时隐藏窗口"))
                .on_disabled_hover_text(
                    "宿主截图协议要求隐藏全部所属窗口；不隐藏选项仅用于独立模式本地截图。",
                );
        } else {
            ui.add_enabled(
                self.local_capture.is_none(),
                egui::Checkbox::new(&mut self.capture_hide_window, "截图时隐藏窗口"),
            )
            .on_hover_text("默认隐藏；取消勾选会保留窗口和笔迹，它们可能进入截图。本次运行有效。");
        }
        if ui.add_enabled(self.local_capture.is_none(), egui::Button::new("截图（本次授权）"))
            .on_hover_text(if self.hosted {
                "点击授权本次宿主框选，成功自动上板、不发送 Agent；仍须宿主允许截图且关闭课堂安全模式。"
            } else { capture::LOCAL_HINT }).clicked()
        {
            self.screenshot_clicked();
            ui.close();
        }
        if ui
            .button("询问 AI / Agent（本次授权）")
            .on_hover_text("仅 hosted 宿主服务可用；打开授权面板，不自动发送图片或允许写回")
            .clicked()
        {
            self.cancel_authorization();
            self.authorization = Some(false);
            self.tool = Tool::Pen;
            ui.close();
        }
    }
    fn request_close(&mut self, ctx: &egui::Context) {
        self.cancel_local_capture();
        self.cancel_ink();
        if self.session.history.is_dirty(&self.session.document) {
            self.set_collapsed(ctx, false);
            self.confirm_close = true;
            self.tool = Tool::Pen;
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        } else {
            self.allow_close = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
    fn sync_handwriting_context(&mut self) {
        let context = (self.math
            && self.mode == AppMode::Blackboard
            && self.session.effective_visible()
            && !self.compact_entry())
        .then(|| {
            (
                ContextToken::capture(&self.session.document),
                self.current_math_input(),
            )
        });
        if self.handwriting_context != context {
            self.handwriting.invalidate_context();
            self.handwriting_context = context;
        }
    }

    fn panels(&mut self, ctx: &egui::Context) {
        self.sync_handwriting_context();
        if self.files {
            let mut open = true;
            egui::Window::new("文件：明确路径，不扫描个人文件")
                .default_width((ctx.content_rect().width() - 40.0).clamp(80.0, 480.0))
                .max_size((ctx.content_rect().size() - Vec2::splat(40.0)).max(Vec2::splat(80.0)))
                .scroll([true, true])
                .constrain_to(ctx.content_rect())
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("板书路径")
                        .on_hover_text("兼容旧版 JSON；图片随板书资源包保存。");
                    ui.text_edit_singleline(&mut self.path);
                    ui.horizontal(|ui| {
                        if ui.button("保存").clicked() {
                            self.save();
                        }
                        if ui.button("载入").clicked() {
                            if self.session.history.is_dirty(&self.session.document)
                                && !self.discard_load
                            {
                                self.status = "当前有未保存内容，请先保存或勾选确认丢弃".into();
                            } else {
                                match self
                                    .session
                                    .open_document(self.path.trim(), self.discard_load)
                                {
                                    Ok(messages) => {
                                        self.handwriting.invalidate_context();
                                        self.ink_cache.clear();
                                        self.messages(messages);
                                        self.selected = None;
                                        self.gesture = None;
                                        self.discard_load = false;
                                        self.textures.clear();
                                        self.renderer.clear();
                                        self.thumbnails.clear();
                                        self.captured_asset = None;
                                        self.split_drag = None;
                                        self.agent_image = None;
                                        self.changed();
                                        self.status = "已载入板书及图片资源".into();
                                    }
                                    Err(e) => self.status = e.message,
                                }
                            }
                        }
                        ui.checkbox(&mut self.discard_load, "确认载入时丢弃未保存内容");
                    });
                    ui.separator();
                    ui.label("PNG 图片路径")
                        .on_hover_text("只读取指定路径，不扫描文件；图片随板书保存。");
                    ui.text_edit_singleline(&mut self.image_path);
                    if ui.button("导入图片").clicked() {
                        use std::io::Read;
                        let imported = (|| -> std::result::Result<String, String> {
                            let mut bytes = Vec::new();
                            std::fs::File::open(self.image_path.trim())
                                .map_err(|e| e.to_string())?
                                .take(16 * 1024 * 1024 + 1)
                                .read_to_end(&mut bytes)
                                .map_err(|e| e.to_string())?;
                            self.session
                                .resources
                                .import_png(bytes)
                                .map_err(|e| e.message)
                        })();
                        match imported {
                            Ok(asset_ref) => {
                                let resource = self.session.resources.get(&asset_ref).unwrap();
                                let factor = (600.0 / resource.width as f32)
                                    .min(400.0 / resource.height as f32)
                                    .min(1.0);
                                self.add(ObjectKind::Image {
                                    position: Point { x: 80.0, y: 80.0 },
                                    width: resource.width as f32 * factor,
                                    height: resource.height as f32 * factor,
                                    asset_ref,
                                });
                            }
                            Err(e) => self.status = e,
                        }
                    }
                    if self.captured_asset.is_some()
                        && ui.button("将已返回截图插入当前页（不上传）").clicked()
                        && let Some(asset_ref) = self.captured_asset.take()
                        && let Some(resource) = self.session.resources.get(&asset_ref)
                    {
                        let factor = (600.0 / resource.width as f32)
                            .min(400.0 / resource.height as f32)
                            .min(1.0);
                        self.add(ObjectKind::Image {
                            position: Point { x: 80.0, y: 80.0 },
                            width: resource.width as f32 * factor,
                            height: resource.height as f32 * factor,
                            asset_ref,
                        });
                    }
                    ui.label("导出路径（当前页）").on_hover_text("不含工具条。");
                    ui.text_edit_singleline(&mut self.export_path);
                    ui.horizontal(|ui| {
                        if ui.button("导出 SVG").clicked() {
                            self.export(false);
                        }
                        if ui.button("导出 PNG").clicked() {
                            self.export(true);
                        }
                    });
                    ui.label(&self.status);
                });
            self.files = open;
        }
        if self.math && self.mode == AppMode::Blackboard {
            let mut open = true;
            egui::Window::new("数学输入与识别候选")
                .default_width((ctx.content_rect().width() - 40.0).clamp(80.0, 480.0))
                .max_size((ctx.content_rect().size() - Vec2::splat(40.0)).max(Vec2::splat(80.0)))
                .scroll([true, true])
                .constrain_to(ctx.content_rect())
                .open(&mut open)
                .show(ctx, |ui| {
                    let mut mode = self.ink_math_mode;
                    ui.label("手写识别（默认关闭）")
                        .on_hover_text("启用后停笔 2.5 秒触发识别；仅本次运行有效，不自动计算或绘图。");
                    ui.radio_value(&mut mode, InkMathMode::Off, "关闭手写计算/识别")
                        .on_hover_text("关闭识别仍保留已加载模型；释放内存请点击“卸载模型”或切换模板后端。");
                    ui.radio_value(&mut mode, InkMathMode::Confirm, "启用手写识别（点击图标才计算/绘图）");
                    self.set_ink_math_mode(mode);
                    let mut backend = self.hwr_backend;
                    ui.horizontal(|ui| {
                        ui.label("识别后端");
                        ui.radio_value(&mut backend, HwrBackend::Template, "模板（默认）");
                        ui.radio_value(&mut backend, HwrBackend::TexTeller, "TexTeller 本地模型");
                    });
                    self.set_hwr_backend(backend);
                    if backend == HwrBackend::TexTeller {
                        ui.label("模型目录（绝对路径）")
                            .on_hover_text("手动加载，不扫描或下载；每次最多 128 笔 / 32768 点。");
                        ui.add_enabled_ui(!self.model_loader.busy(), |ui| {
                            if ui.text_edit_singleline(&mut self.model_dir).changed() {
                                self.cancel_ink();
                                self.model_status = "目录已修改；请点击后台加载 / 重载模型".into();
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.add_enabled(self.model_load_ready(), egui::Button::new("后台加载 / 重载模型")).clicked() {
                                self.load_model(ctx);
                            }
                            if ui.add_enabled(self.neural_recognizer.is_some() || self.model_loader.busy() || !self.loaded_model_dir.is_empty(), egui::Button::new("卸载模型")).clicked() {
                                self.unload_model();
                            }
                        });
                        ui.label(&self.model_status);
                        if self.model_loader.busy() || self.hwr_worker.busy() {
                            ui.small("后台任务未排空；取消/卸载不强杀线程，排空前不能重载。");
                        }
                        ui.small("关闭识别仍保留模型；卸载或切模板会释放，进行中的任务排空后完成释放。");
                        ui.small("默认优先建议文件齐全的 models/texteller-int8；仅检查存在，不保证内容或哈希有效，仍须点击加载后校验。");
                        ui.colored_label(Color32::YELLOW, "低内存设备建议手动加载独立量化目录（models/texteller-int8）。FP32 模型内存占用高，不保证 3GB 内运行；目录建议不代表已量化或整机内存达标。");
                        ui.colored_label(Color32::YELLOW, "模型可能看错，请核对后点击计算/绘图。");
                    }
                    if let Some(diagnostic) = &self.ink_diagnostic
                        && diagnostic.context == ContextToken::capture(&self.session.document)
                        && self.calculation.as_ref().is_some_and(|request| request.id() == diagnostic.request_id)
                    {
                        ui.separator();
                        ui.collapsing("本次识别输入", |ui| {
                            ui.small(format!("请求：{}", diagnostic.request_id));
                            ui.label(format!("快照：{} 笔 / {} 点；逐笔点数（书写顺序）：{:?}", diagnostic.stroke_points.len(), diagnostic.stroke_points.iter().sum::<usize>(), diagnostic.stroke_points));
                            ui.label(format!("未纳入的非相邻笔迹：{} 笔 / {} 点（仍保留在板书中）", diagnostic.excluded.0, diagnostic.excluded.1));
                            if let Some(texture) = &diagnostic.texture {
                                let side = ui.available_width().clamp(1.0, 448.0);
                                ui.image((texture.id(), Vec2::splat(side)));
                                ui.small("448×448 模型输入预览")
                                    .on_hover_text("实际预处理张量逆归一化：灰度 = (值 × 0.15394445 + 0.9545467) × 255；右/下补零约为 243 灰度。推理失败时仍保留已准备的输入。");
                            } else {
                                ui.label(if self.hwr_worker.busy() { "等待本次输入预览…" } else { "本次未生成预览（未进入预处理或预处理失败）" });
                            }
                            ui.small("仅本地笔迹预览，不保存或上传。")
                                .on_hover_text("不是桌面截图；继续书写、切换上下文/后端或取消后清除。");
                        });
                        if let Some(error) = &diagnostic.error {
                            ui.colored_label(Color32::YELLOW, error);
                        }
                    }
                    if let Some(raw) = &self.neural_raw {
                        ui.collapsing("查看原始 LaTeX / 模型分数（非准确概率）", |ui| {
                            ui.label(&raw.latex);
                            ui.label(format!("finished={}，tokens={}，mean logprob={:.6}", raw.finished, raw.generated_tokens, raw.mean_log_probability));
                        });
                    }
                    ui.label("识别结果须核对/纠错后点击计算；分数不是正确概率。")
                        .on_hover_text("离线模板仅适合分离直立符号及简单上下标/分数，不支持任意手写或所有高数。");
                    if let Some(recognition) = &self.recognition {
                        for candidate in &recognition.candidates {
                            let backend = self.active_candidate.as_deref().and_then(|id| self.ink_cache.get(id)).map_or(self.hwr_backend, |entry| entry.backend);
                            let label = if backend == HwrBackend::TexTeller {
                                format!("{}  （模型候选，未校准）", candidate.text)
                            } else {
                                format!("{}  相似度 {:.0}%", candidate.text, candidate.confidence * 100.0)
                            };
                            if ui.selectable_label(self.expression == candidate.text, label).clicked() {
                                self.expression = candidate.text.clone();
                                self.math_ticket = None;
                                self.math_input = None;
                                self.math_worker.cancel();
                                self.hwr_worker.cancel();
                            }
                        }
                    }
                    if self.calculation.is_some() && ui.add_enabled(!self.ink_action_busy() && !self.expression.trim().is_empty(), egui::Button::new("确认纠错并计算写回 / 生成函数图像")).clicked() {
                        self.confirm_calculation(ctx);
                    }
                    if ui.button("取消候选 / 开始新表达式").clicked() { self.cancel_ink(); }
                    ui.collapsing("学习个人符号模板（只在本机）", |ui| {
                        ui.label("启用识别，单独写一个 1–4 笔符号；停笔后填写正确符号。")
                            .on_hover_text("只学习个人符号模板，不训练通用手写模型。");
                        ui.text_edit_singleline(&mut self.template_label);
                        if ui.button("学习当前候选笔迹").clicked() {
                            self.math_result = match self.calculation.as_ref() {
                                Some(request) if request.context == ContextToken::capture(&self.session.document) => {
                                    match self.recognizer.register_template(self.template_label.trim(), &request.strokes) {
                                        Ok(()) => "已加入本次识别器；请明确选择路径保存以供下次加载".into(),
                                        Err(e) => e.to_string(),
                                    }
                                }
                                _ => "请先单独书写符号并等待候选；当前候选为空或已过期".into(),
                            };
                        }
                        ui.label("模板 JSON 路径（保存会覆盖该文件）")
                            .on_hover_text("仅使用指定路径，不自动扫描或加载。");
                        ui.text_edit_singleline(&mut self.template_path);
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!self.template_path.trim().is_empty(), egui::Button::new("保存模板到指定路径")).clicked() {
                                self.math_result = self.recognizer.save(self.template_path.trim())
                                    .map(|()| format!("模板已保存：{}", self.template_path.trim()))
                                    .unwrap_or_else(|e| e.to_string());
                            }
                            if ui.add_enabled(!self.template_path.trim().is_empty(), egui::Button::new("加载指定模板")).clicked() {
                                match InkRecognizer::load(self.template_path.trim()) {
                                    Ok(recognizer) => { self.cancel_ink(); self.expression.clear(); self.recognizer = recognizer; self.math_result = "已加载模板，将用于后续识别".into(); }
                                    Err(e) => self.math_result = e.to_string(),
                                }
                            }
                        });
                    });
                    self.sync_handwriting_context();
                    let handwriting_font = self.export_resources.handwriting_font();
                    if self.handwriting.ui(ui, handwriting_font.as_ref()) {
                        self.cancel_math();
                    }
                    if let Some(ObjectKind::Handwritten { text, .. }) = self.selected.as_ref().and_then(|id| {
                        self.session.document.current_page().objects.iter().find(|object| &object.id == id).map(|object| &object.kind)
                    }) {
                        ui.label("所选手写答案的标准文字：");
                        ui.label(text);
                        if ui.button("复制所选答案标准文字").clicked() { ui.ctx().copy_text(text.clone()); }
                    }
                    if ui.text_edit_singleline(&mut self.expression).changed() { self.cancel_math(); self.hwr_worker.cancel(); }
                    if self.math_worker.busy() && ui.button("取消后台计算（不写回）").clicked() { self.cancel_math(); self.math_result = "已取消；旧工作排空前不启动新任务".into(); }
                    ui.label("二元二次求解仅给范围内数值候选，不保证全解。")
                        .on_hover_text("两式用英文分号分隔；每式仅支持总次数 ≤2 的 x/y 多项式。未找到候选不等于无解。");
                    ui.horizontal(|ui| {
                        ui.label("x 范围 / 积分上下限");
                        let mut changed = ui.add(egui::DragValue::new(&mut self.math_bounds.x_min)).changed();
                        changed |= ui.add(egui::DragValue::new(&mut self.math_bounds.x_max)).changed();
                        ui.label("y 范围");
                        changed |= ui.add(egui::DragValue::new(&mut self.math_bounds.y_min)).changed();
                        changed |= ui.add(egui::DragValue::new(&mut self.math_bounds.y_max)).changed();
                        if changed { self.cancel_math(); }
                    });
                    ui.horizontal(|ui| {
                        ui.label("数值微分点 x");
                        if ui.add(egui::DragValue::new(&mut self.derivative_at)).changed() { self.cancel_math(); }
                        if ui.button("数值微分").clicked() {
                            let expression = self.expression.clone();
                            let at = self.derivative_at;
                            self.start_math(ctx, move || board_math::derivative(&expression, at)
                                .map(|v| (format!("局部导数近似 ≈ {v}"), None)).map_err(|e| e.to_string()));
                        }
                        if ui.button("数值定积分").clicked() {
                            let expression = self.expression.clone();
                            let bounds = self.math_bounds;
                            self.start_math(ctx, move || board_math::integrate(&expression, bounds.x_min, bounds.x_max, Default::default())
                                .map(|v| (format!("指定有限区间积分近似 ≈ {v}"), None)).map_err(|e| e.to_string()));
                        }
                    });
                    ui.label("数值微分/积分仅为近似，不支持广义积分。")
                        .on_hover_text("请输入 f(x) 表达式，而非等式。");
                    ui.collapsing("数学语法与限制", |ui| {
                        ui.label("输入后点击计算：simplify(x+1=2)、diff(x^3,x)、integrate(3*x^2,x)。");
                        ui.label("符号运算仅支持有限 x/y 多项式；原函数积分常数取 0，不表示完整通解。");
                        ui.small("数学任务单槽无排队；默认数值预算512网格/30000次求值/500万词元计算。取消只丢弃结果，旧工作排空前不再启动；不承诺硬实时截止。");
                    });
                    ui.horizontal(|ui| {
                        for label in ["计算", "化简", "解方程"] {
                            if ui.button(label).clicked() {
                                let expression = self.expression.clone();
                                let bounds = self.math_bounds;
                                self.start_math(ctx, move || {
                                    let result = if label == "化简" { board_math::simplify(&expression) } else { features::calculation_output(&expression, bounds, true).map(|(text, _)| text) };
                                    result.map(|text| (text, None)).map_err(|e| e.to_string())
                                });
                            }
                        }
                        if ui.add_enabled(self.calculation.is_none(), egui::Button::new("绘制函数")).clicked() {
                            match board_math::classify_plot(&self.expression) {
                                Ok(board_math::PlotKind::Explicit(expression) | board_math::PlotKind::Implicit(expression)) => self.add(ObjectKind::FunctionPlot {
                                    position: Point { x: 100.0, y: 80.0 },
                                    width: 500.0,
                                    height: 350.0,
                                    expressions: vec![expression],
                                    x_min: -10.0,
                                    x_max: 10.0,
                                    y_min: -7.0,
                                    y_max: 7.0,
                                }),
                                Err(e) => self.math_result = e.to_string(),
                            }
                        }
                    });
                    ui.label(&self.math_result);
                    if ui.add_enabled(self.calculation.is_none(), egui::Button::new("将手动输入计算后写到板上")).clicked()
                        && !self.expression.trim().is_empty()
                    {
                        let expression = self.expression.clone();
                        let bounds = self.math_bounds;
                        let color = self.style.color;
                        self.start_math(ctx, move || {
                            let output = features::calculation_display(&expression, bounds, true).map_err(|e| e.to_string())?;
                            let result = output.text.clone();
                            let kind = output.into_kind(Point { x: 80.0, y: 80.0 }, color);
                            Ok((result, Some(kind)))
                        });
                    }
                    if ui.button("复原所选图像/坐标")
                        .on_hover_text("选择函数图后可滚轮缩放、拖动平移；复原恢复默认范围。")
                        .clicked()
                        && let Some(mut object) = self.selected_object()
                        && matches!(object.kind, ObjectKind::FunctionPlot { .. } | ObjectKind::CoordinateSystem { .. })
                    {
                        match &mut object.kind {
                            ObjectKind::FunctionPlot { x_min, x_max, y_min, y_max, .. } => {
                                *x_min = -10.0; *x_max = 10.0; *y_min = -7.0; *y_max = 7.0;
                            }
                            ObjectKind::CoordinateSystem { scale, .. } => *scale = 40.0,
                            _ => {}
                        }
                        self.apply(vec![Operation::Update { object }]);
                    }
                });
            self.math = open;
            self.sync_handwriting_context();
        }
        if self.confirm_close {
            egui::Window::new("有未保存的板书")
                .default_width((ctx.content_rect().width() - 40.0).clamp(80.0, 400.0))
                .max_size((ctx.content_rect().size() - Vec2::splat(40.0)).max(Vec2::splat(80.0)))
                .scroll([true, true])
                .constrain_to(ctx.content_rect())
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("退出将丢失未保存内容。请保存或明确选择丢弃。");
                    ui.text_edit_singleline(&mut self.path);
                    ui.horizontal(|ui| {
                        if ui.button("保存并退出").clicked() && self.save() {
                            self.allow_close = true;
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                        if ui.button("丢弃并退出").clicked() {
                            self.allow_close = true;
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                        if ui.button("取消").clicked() {
                            self.confirm_close = false;
                        }
                    });
                    ui.label(&self.status);
                });
        }
    }
    fn confirm_calculation(&mut self, ctx: &egui::Context) {
        if self.expression.len() > 4096 {
            self.math_result = "表达式超过 4096 字节，请缩短后重试".into();
            return;
        }
        if self.ink_action_busy()
            || self.expression.trim().is_empty()
            || self.ink_math_mode == InkMathMode::Off
            || !self.session.effective_visible()
            || self.gesture.is_some()
        {
            return;
        }
        self.sync_ink_cache();
        self.save_candidate_edit();
        let Some(id) = self.active_candidate.clone() else {
            return;
        };
        let Some(entry) = self.ink_cache.get(&id) else {
            return;
        };
        if entry.result_present {
            return;
        }
        if !self.activate_candidate(&id) {
            return;
        }
        if features::function_expression(&self.expression).is_some() {
            self.plot_ink_function();
            return;
        }
        let request = self.calculation.as_ref().unwrap().clone();
        self.calculate_ink(ctx, &request);
        self.running_candidate = Some((id, self.ink_cache.epoch));
        self.calculating_ink = Some(features::ink_bounds(&request.strokes));
    }

    fn calculate_ink(&mut self, ctx: &egui::Context, request: &CalculationRequest) {
        let bounds = features::ink_bounds(&request.strokes);
        let expression = self.expression.clone();
        // Revalidate raw metadata against the current input, including manual corrections.
        let answer_prompt = self.neural_raw.as_ref().is_some_and(|raw| {
            board_hwr::latex_to_calculation(&raw.latex)
                .is_ok_and(|parsed| parsed.answer_prompt && parsed.expression == expression)
        });
        let math_bounds = self.math_bounds;
        let color = self.style.color;
        self.start_math(ctx, move || {
            let output = features::calculation_display(&expression, math_bounds, answer_prompt)
                .map_err(|e| e.to_string())?;
            let position = features::result_position(bounds, output.equation);
            let result = output.text.clone();
            Ok((result, Some(output.into_kind(position, color))))
        });
    }
    fn authorized_asset_refs(&self) -> Vec<String> {
        if !self.authorize_assets {
            return Vec::new();
        }
        if let Some((_, _, asset)) = &self.agent_image {
            return vec![asset.clone()];
        }
        let mut assets = Vec::new();
        for object in &self.session.document.current_page().objects {
            if let ObjectKind::Image { asset_ref, .. } = &object.kind
                && !assets.contains(asset_ref)
            {
                assets.push(asset_ref.clone());
            }
        }
        assets
    }

    fn cancel_authorization(&mut self) {
        self.authorization = None;
        self.agent_image = None;
        self.authorize_assets = false;
        self.authorize_write_back = false;
    }

    fn take_host_request(&mut self, capture: bool) -> std::result::Result<Request, String> {
        if !capture && let Some((context, id, asset)) = &self.agent_image
            && (context != &ContextToken::capture(&self.session.document)
                || !self.session.document.current_page().objects.iter().any(|o| &o.id == id && matches!(&o.kind, ObjectKind::Image { asset_ref, .. } if asset_ref == asset)))
        {
            self.cancel_authorization();
            return Err("图片或页面已变化，请从图片按钮重新授权".into());
        }
        let mut params = serde_json::json!({
            "document_id": self.session.document.id,
            "page_id": self.session.document.current_page().id,
            "expected_revision": self.session.document.revision,
            "user_authorized": true,
        });
        if !capture {
            params["prompt"] = serde_json::json!(self.agent_prompt.trim());
            params["write_back"] = serde_json::json!(self.authorize_write_back);
            params["asset_refs"] = serde_json::json!(self.authorized_asset_refs());
        }
        self.cancel_authorization();
        let request = Request::new(
            format!("runtime:gui:{}", new_id()),
            if capture {
                "capture.request"
            } else {
                "agent.request"
            },
            params,
        )
        .map_err(|e| e.to_string())?;
        if capture {
            self.capture_requests.insert(
                request.id.clone(),
                CaptureContext {
                    context: ContextToken::capture(&self.session.document),
                    canvas_size: self.canvas_size,
                    stale: false,
                },
            );
        }
        Ok(request)
    }

    fn request_host(&mut self, capture: bool) {
        if !self.hosted {
            self.cancel_authorization();
            self.status = "独立模式没有宿主；请求未发送".into();
            return;
        }
        match self.take_host_request(capture) {
            Ok(request) => {
                let out = self.session.handle(request);
                self.messages(out);
            }
            Err(e) => self.status = e,
        }
    }
    fn authorization_panel(&mut self, ctx: &egui::Context) {
        let Some(capture) = self.authorization else {
            return;
        };
        let mut open = true;
        let response = egui::Window::new(if capture {
            "本次截图授权"
        } else {
            "本次 Agent 授权"
        })
        .default_width((ctx.content_rect().width() - 40.0).clamp(80.0, 400.0))
        .max_size((ctx.content_rect().size() - Vec2::splat(40.0)).max(Vec2::splat(80.0)))
        .scroll([true, true])
        .constrain_to(ctx.content_rect())
        .open(&mut open)
        .show(ctx, |ui| {
            let state = self.session.state();
            let safe = state["permissions"]["classroom_safe"] == true;
            let permitted = if capture {
                !safe && state["permissions"]["desktop_capture_allowed"] == true
            } else {
                state["permissions"]["agent_allowed"] == true
            };
            if capture {
                ui.label("宿主确认所有窗口隐藏后框选；成功截图自动插入原页面（可撤销），不会发送给 Agent。页面或版本变化时不自动插入。");
                if safe {
                    ui.colored_label(Color32::YELLOW, "课堂安全模式：拒绝截图，不会采集桌面。");
                }
            } else {
                ui.label("Agent 仅 hosted 宿主服务可用；独立模式不会发送。只发送本次明确输入的提示；图片与板书写入分别授权。默认只显示回答。");
                ui.text_edit_multiline(&mut self.agent_prompt);
                ui.checkbox(&mut self.authorize_write_back, "允许本次回答写入板书");
                ui.checkbox(
                    &mut self.authorize_assets,
                    if self.agent_image.is_some() {
                        "授权仅发送这张图片（不包含其他图片）"
                    } else {
                        "授权发送当前页图片资源（单独授权）"
                    },
                );
            }
            if !permitted {
                ui.label("宿主权限未允许，本次请求将拒绝。");
            }
            if ui
                .add_enabled(
                    permitted
                        && self.hosted
                        && (capture || !self.agent_prompt.trim().is_empty())
                        && (self.agent_image.is_none() || self.authorize_assets || capture),
                    egui::Button::new("明确授权本次请求"),
                )
                .clicked()
            {
                self.request_host(capture);
            }
            ui.label(&self.status);
        });
        if let Some(response) = response {
            self.controls.push(response.response.rect);
        }
        if !open {
            self.cancel_authorization();
        }
    }
    fn export(&mut self, png: bool) {
        if self.export_path.trim().is_empty() {
            self.status = "请输入导出文件路径".into();
            return;
        }
        let page = self.session.document.current_page();
        let w = self.canvas_size.x.ceil().clamp(1.0, 8192.0) as u32;
        let h = self.canvas_size.y.ceil().clamp(1.0, 8192.0) as u32;
        let blackboard = self.mode == AppMode::Blackboard;
        for object in &page.objects {
            if let ObjectKind::Image { asset_ref, .. } = &object.kind {
                if let Some(resource) = self.session.resources.get(asset_ref) {
                    if let Err(e) = self.export_resources.insert_png(asset_ref, &resource.bytes) {
                        self.status = e.to_string();
                        return;
                    }
                } else {
                    self.export_resources.remove_image(asset_ref);
                }
            }
        }
        let bytes = if png {
            board_render::export_png_with_resources(page, w, h, blackboard, &self.export_resources)
                .map_err(|e| e.to_string())
        } else {
            board_render::export_svg_with_resources(page, w, h, blackboard, &self.export_resources)
                .map(String::into_bytes)
                .map_err(|e| e.to_string())
        };
        self.status = match bytes.and_then(|data| {
            std::fs::write(self.export_path.trim(), data).map_err(|e| e.to_string())
        }) {
            Ok(()) => "已导出当前页".into(),
            Err(e) => e,
        };
    }
    fn selected_object(&self) -> Option<BoardObject> {
        self.session
            .document
            .current_page()
            .objects
            .iter()
            .find(|o| Some(&o.id) == self.selected.as_ref())
            .cloned()
    }
    fn ink_action_busy(&self) -> bool {
        self.hwr_worker.busy() || self.math_worker.busy() || self.model_loader.busy()
    }

    #[cfg(test)]
    fn ink_action(&self) -> Option<(Rect, &'static str, &'static str)> {
        if let Some(bounds) = self.calculating_ink
            && self.math_worker.busy()
            && self.session.effective_visible()
            && self
                .ink_context
                .as_ref()
                .is_some_and(|context| *context == ContextToken::capture(&self.session.document))
        {
            return Some((bounds, "=", "计算中"));
        }
        let request = self.calculation.as_ref()?;
        if self.ink_math_mode == InkMathMode::Off
            || request.context != ContextToken::capture(&self.session.document)
            || !self.session.effective_visible()
            || self.ink_ticket.is_some()
        {
            return None;
        }
        let (icon, hover) = if self.recognition.is_none()
            || self
                .neural_raw
                .as_ref()
                .is_some_and(|raw| latex_to_expression(&raw.latex).is_err())
        {
            ("?", "查看原输入并纠错")
        } else if features::function_expression(&self.expression).is_some() {
            ("f", "生成函数图像")
        } else {
            ("=", "计算")
        };
        Some((features::ink_bounds(&request.strokes), icon, hover))
    }

    fn ink_function(&self) -> Option<(Rect, String)> {
        let request = self.calculation.as_ref()?;
        if self.ink_math_mode == InkMathMode::Off
            || request.context != ContextToken::capture(&self.session.document)
            || !self.session.effective_visible()
        {
            return None;
        }
        Some((
            features::ink_bounds(&request.strokes),
            features::function_expression(&self.expression)?,
        ))
    }

    fn plot_ink_function(&mut self) {
        if self.ink_action_busy() || self.gesture.is_some() {
            return;
        }
        self.sync_ink_cache();
        self.save_candidate_edit();
        let Some(id) = self.active_candidate.clone() else {
            return;
        };
        if !self.activate_candidate(&id) {
            return;
        }
        if let Some((bounds, expression)) = self.ink_function() {
            let object = features::plot(
                expression,
                Point {
                    x: bounds.left(),
                    y: bounds.bottom() + 20.0,
                },
            );
            let _ = self.add_candidate_result(&id, object.kind);
        }
    }

    fn sync_plot_state(&mut self) {
        if self.plot_points.as_ref().is_some_and(|state| {
            state.context != ContextToken::capture(&self.session.document)
                || self.selected_object().as_ref() != Some(&state.object)
                || self
                    .preview()
                    .is_some_and(|preview| preview != state.object)
        }) {
            self.plot_worker.cancel();
            self.plot_points = None;
        }
    }

    fn plot_controls(&mut self, ctx: &egui::Context) {
        self.sync_ink_cache();
        self.save_candidate_edit();
        let actions: Vec<_> = self
            .ink_cache
            .entries
            .iter()
            .filter(|entry| {
                entry.request.context.page_id == self.session.document.current_page().id
                    && !entry.result_present
                    && self.ink_math_mode != InkMathMode::Off
                    && self.session.effective_visible()
                    && !self.allow_close
                    && !self.confirm_close
            })
            .map(|entry| {
                let (icon, hover) = if self
                    .running_candidate
                    .as_ref()
                    .is_some_and(|(id, _)| id == &entry.id)
                {
                    (Icon::Calculator, "计算中")
                } else if entry.recognition.is_none() && entry.expression.is_empty() {
                    (Icon::Correct, "查看原输入并纠错")
                } else if features::function_expression(&entry.expression).is_some() {
                    (Icon::Plot, "生成函数图像")
                } else {
                    (Icon::Calculator, "计算")
                };
                (entry.id.clone(), entry.bounds, icon, hover)
            })
            .collect();
        let mut occupied = self.controls.clone();
        for (id, bounds, icon, hover) in actions {
            let Some(rect) =
                features::ink_action_rect_avoiding(bounds, ctx.content_rect(), &occupied)
            else {
                continue;
            };
            occupied.push(rect);
            self.controls.push(rect);
            egui::Area::new(egui::Id::new(("ink_action", &id)))
                .order(egui::Order::Foreground)
                .fixed_pos(rect.min)
                .default_size(rect.size())
                .movable(false)
                .show(ctx, |ui| {
                    ui.set_min_size(rect.size());
                    ui.set_max_size(rect.size());
                    if icons::button(
                        ui,
                        icon,
                        hover,
                        rect.width(),
                        false,
                        !self.ink_action_busy() && self.gesture.is_none(),
                    )
                    .clicked()
                        && self.activate_candidate(&id)
                    {
                        match icon {
                            Icon::Plot => self.plot_ink_function(),
                            Icon::Calculator => self.confirm_calculation(ctx),
                            _ => self.math = true,
                        }
                    }
                });
        }
        self.sync_plot_state();
        let objects: Vec<_> = self
            .session
            .document
            .current_page()
            .objects
            .iter()
            .filter(|object| {
                matches!(
                    object.kind,
                    ObjectKind::Text { .. }
                        | ObjectKind::Image { .. }
                        | ObjectKind::FunctionPlot { .. }
                )
            })
            .cloned()
            .collect();
        for object in &objects {
            // The selected image's corners belong to resize; its center still opens Agent.
            if !(self.tool == Tool::Select && self.selected.as_ref() == Some(&object.id))
                && let Some(rect) = features::image_agent_rect(object, ctx.content_rect())
            {
                let area = egui::Area::new(egui::Id::new(("image_agent", &object.id)))
                    .fixed_pos(rect.min)
                    .movable(false)
                    .show(ctx, |ui| {
                        if ui
                            .add_sized(rect.size(), egui::Button::new("询问 Agent"))
                            .on_hover_text("仅 hosted 宿主服务：打开这张图片的授权，不自动发送；选择工具单击图片也可打开，拖动仍移动")
                            .clicked()
                        {
                            self.open_image_agent(object);
                            self.tool = Tool::Pen;
                        }
                    });
                self.controls.push(area.response.rect);
            }
            if let ObjectKind::Text { text, .. } = &object.kind
                && let Some(expression) = features::function_expression(text)
            {
                let bounds = board_render::object_bounds(object);
                let area = egui::Area::new(egui::Id::new(("function", &object.id)))
                    .fixed_pos(bounds.right_top() + Vec2::new(6.0, -24.0))
                    .movable(false)
                    .show(ctx, |ui| {
                        if ui.button("绘图").clicked() {
                            self.apply(vec![Operation::Add {
                                object: features::plot(
                                    expression,
                                    Point {
                                        x: bounds.left(),
                                        y: bounds.bottom() + 20.0,
                                    },
                                ),
                            }]);
                        }
                    });
                self.controls.push(area.response.rect);
            }
        }
        if let Some(object) = self.selected_object() {
            if let ObjectKind::FunctionPlot {
                expressions,
                position,
                width,
                ..
            } = &object.kind
            {
                let area = egui::Area::new("plot_legend".into())
                    .fixed_pos(Pos2::new(position.x, position.y - 30.0))
                    .movable(false)
                    .show(ctx, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("复原").clicked() {
                                let mut reset = object.clone();
                                if let ObjectKind::FunctionPlot {
                                    x_min,
                                    x_max,
                                    y_min,
                                    y_max,
                                    ..
                                } = &mut reset.kind
                                {
                                    *x_min = -10.0;
                                    *x_max = 10.0;
                                    *y_min = -7.0;
                                    *y_max = 7.0;
                                }
                                self.apply(vec![Operation::Update { object: reset }]);
                            }
                            for (index, expression) in expressions.iter().take(16).enumerate() {
                                let response = ui.add(
                                    egui::Label::new(features::plot_text(expression))
                                        .sense(Sense::drag()),
                                );
                                if expressions.len() > 1 && response.drag_started() {
                                    self.split_drag = Some((
                                        object.id.clone(),
                                        index,
                                        ContextToken::capture(&self.session.document),
                                    ));
                                }
                                response
                                    .on_hover_text("拖出图框分离函数；拖动整个图到另一个图合并");
                            }
                            if expressions.len() > 1
                                && ui.button("分离最后函数").clicked()
                                && let Some(ops) = features::split_plot(
                                    &object,
                                    expressions.len() - 1,
                                    Point {
                                        x: position.x + width + 20.0,
                                        y: position.y,
                                    },
                                )
                            {
                                self.apply(ops);
                            }
                        });
                    });
                self.controls.push(area.response.rect);
                self.sync_plot_state();
                if self.plot_points.is_none()
                    && !self.plot_worker.busy()
                    && self.preview().is_none_or(|preview| preview == object)
                    && self.selected_object().as_ref() == Some(&object)
                {
                    let snapshot = object.clone();
                    let context = ContextToken::capture(&self.session.document);
                    self.plot_points = Some(PlotState {
                        context: context.clone(),
                        object: object.clone(),
                        report: None,
                        coordinate: None,
                    });
                    self.plot_worker.start(ctx.clone(), move || {
                        let report = features::intersections(&snapshot);
                        (context, snapshot, report)
                    });
                }
                let bounds = board_render::object_bounds(&object);
                let area = egui::Area::new("plot_search_status".into())
                    .fixed_pos(bounds.left_bottom() + Vec2::new(0.0, 8.0))
                    .movable(false)
                    .show(ctx, |ui| {
                        ui.set_max_width(440.0);
                        match self
                            .plot_points
                            .as_ref()
                            .and_then(|state| state.report.as_ref())
                        {
                            None => {
                                ui.label("交点计算中（旧任务未排空时等待）…");
                            }
                            Some(Err(error)) => {
                                ui.label(format!("部分搜索失败；非完备：{error}"));
                            }
                            Some(Ok(report)) => {
                                ui.label(report.summary());
                                for diagnostic in &report.diagnostics {
                                    let color = match diagnostic.issue {
                                        features::IntersectionIssue::Failed
                                        | features::IntersectionIssue::Limit => Color32::LIGHT_RED,
                                        _ => Color32::LIGHT_YELLOW,
                                    };
                                    ui.colored_label(color, &diagnostic.message);
                                }
                                if report.omitted_diagnostics > 0 {
                                    ui.label(format!(
                                        "另有 {} 条诊断未展开",
                                        report.omitted_diagnostics
                                    ));
                                }
                            }
                        }
                        if let Some(label) = self
                            .plot_points
                            .as_ref()
                            .and_then(|state| state.coordinate.as_ref())
                        {
                            ui.label(label);
                        }
                    });
                self.controls.push(area.response.rect);
            } else {
                self.plot_points = None;
            }
        } else {
            self.plot_points = None;
        }
        if ctx.input(|i| i.pointer.any_released())
            && let Some((id, index, context)) = self.split_drag.take()
            && context == ContextToken::capture(&self.session.document)
            && let (Some(source), Some(pos)) = (
                objects.iter().find(|o| o.id == id),
                ctx.input(|i| i.pointer.interact_pos()),
            )
            && !board_render::object_bounds(source).contains(pos)
            && !self.controls.iter().any(|r| r.contains(pos))
            && let Some(ops) = features::split_plot(source, index, Point { x: pos.x, y: pos.y })
        {
            self.apply(ops);
        }
    }
    fn sync_erase_preview(&mut self) {
        let compact = self.compact_entry();
        let Some(gesture) = &mut self.gesture else {
            return;
        };
        let Some(erasing) = &mut gesture.erasing else {
            return;
        };
        if self.tool != Tool::Eraser
            || compact
            || gesture.document != self.session.document.id
            || gesture.page != self.session.document.current_page().id
            || gesture.revision != self.session.document.revision
        {
            self.gesture = None;
            return;
        }
        erasing.update(&gesture.points);
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        self.sync_erase_preview();
        if self.compact_entry() {
            return;
        }
        let rect = ui.max_rect();
        self.canvas_size = rect.size();
        let response = ui.interact(rect, "board_canvas".into(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let over_overlay = ui.input(|i| i.pointer.interact_pos()).is_some_and(|pos| {
            self.controls.iter().any(|r| r.contains(pos))
                || ui
                    .ctx()
                    .layer_id_at(pos)
                    .is_some_and(|layer| layer != ui.layer_id())
        });
        let blocked = self.tool == Tool::Mouse
            || self.confirm_close
            || self.authorization.is_some()
            || self.split_drag.is_some()
            || egui::Popup::is_any_open(ui.ctx())
            || over_overlay;
        if blocked {
            if ui.input(|i| i.pointer.any_released()) {
                self.gesture = None;
            }
        } else if let Some(pos) = response.interact_pointer_pos() {
            if self.gesture.is_none()
                && ui.input(|i| i.pointer.primary_pressed())
                && (response.is_pointer_button_down_on() || response.clicked())
            {
                let mut original = None;
                let mut vertex = None;
                let mut resize = None;
                if self.tool == Tool::Select {
                    if let Some(object) = self.selected_object() {
                        let nearest = |handles: Vec<Point>| {
                            handles
                                .iter()
                                .enumerate()
                                .map(|(index, p)| (index, Pos2::new(p.x, p.y).distance(pos)))
                                .filter(|(_, distance)| *distance < 12.0)
                                .min_by(|a, b| a.1.total_cmp(&b.1))
                                .map(|(index, _)| index)
                        };
                        vertex = nearest(editing::vertices(&object));
                        resize = nearest(editing::resize_handles(&object));
                        if vertex.is_some() || resize.is_some() {
                            original = Some(object);
                        }
                    }
                    if original.is_none() {
                        original = self
                            .session
                            .document
                            .current_page()
                            .objects
                            .iter()
                            .rev()
                            .find(|o| editing::hit_test(o, pos, 8.0))
                            .cloned();
                    }
                    self.selected = original.as_ref().map(|o| o.id.clone());
                }
                let start = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
                self.gesture = Some(Gesture {
                    document: self.session.document.id.clone(),
                    page: self.session.document.current_page().id.clone(),
                    revision: self.session.document.revision,
                    points: vec![StrokePoint {
                        x: start.x,
                        y: start.y,
                        time: ui.input(|i| i.time),
                        pressure: 1.0,
                    }],
                    original,
                    vertex,
                    resize,
                    erasing: (self.tool == Tool::Eraser)
                        .then(|| editing::ErasePreview::new(&self.session.document, self.eraser)),
                });
                self.ink_started();
            }
            if (response.is_pointer_button_down_on() || response.drag_stopped())
                && let Some(gesture) = &mut self.gesture
                && gesture.points.len() < 8000
                && gesture
                    .points
                    .last()
                    .is_none_or(|p| (p.x - pos.x).hypot(p.y - pos.y) >= 1.0)
            {
                gesture.points.push(StrokePoint {
                    x: pos.x,
                    y: pos.y,
                    time: ui.input(|i| i.time),
                    pressure: 1.0,
                });
            }
        }
        // #region debug-point B:canvas
        let debug_preview_start = debug_ink::start();
        // #endregion
        self.sync_erase_preview();
        self.sync_plot_state();
        let mut preview = self.preview();
        // 新画笔笔迹独立叠加，不能作为临时页面内容替换真实 revision 缓存。
        let pen_preview = if self.tool == Tool::Pen {
            preview.take()
        } else {
            None
        };
        let plot_preview_changed = preview.as_ref().is_some_and(|object| {
            self.plot_points
                .as_ref()
                .is_some_and(|state| state.object != *object)
        });
        let merge_hint = if blocked {
            None
        } else {
            preview.as_ref().and_then(|source| {
                self.plot_merge_target(source).map(|target| {
                    (
                        board_render::object_bounds(target),
                        features::merge_plots(source, target).is_some(),
                    )
                })
            })
        };
        let vertex_edit = self.polygon_vertex_edit();
        let erased = self
            .gesture
            .as_ref()
            .and_then(|gesture| gesture.erasing.as_ref())
            .and_then(|erasing| erasing.result.as_ref())
            .and_then(|result| result.as_ref().ok())
            .map(|(document, _)| document.current_page());
        let previewing = preview.is_some() || erased.is_some() || vertex_edit.is_some();
        // #region debug-point B:canvas
        let debug_preview_us = debug_ink::micros(debug_preview_start);
        let debug_clone_start = debug_ink::start();
        // #endregion
        let mut page = std::borrow::Cow::Borrowed(
            erased.unwrap_or_else(|| self.session.document.current_page()),
        );
        if let Some(Ok(edit)) = &vertex_edit {
            let mut document = self.session.document.clone();
            if edit.apply(&mut document).is_ok() {
                page = std::borrow::Cow::Owned(document.current_page().clone());
            }
        } else if let Some(preview) = preview {
            if page.objects.iter().any(|o| o.id == preview.id) {
                // 预览也通过 core 传播；不改真实文档或历史。
                let mut document = self.session.document.clone();
                if let Some(endpoint) = self.gesture.as_ref().and_then(|g| g.vertex) {
                    let ids: Vec<_> = document
                        .connections
                        .iter()
                        .filter(|c| c.line_id == preview.id && c.line_endpoint == endpoint)
                        .map(|c| c.id.clone())
                        .collect();
                    for id in ids {
                        let _ = document.disconnect(&page.id, document.revision, &id);
                    }
                }
                if document
                    .apply(
                        &page.id,
                        document.revision,
                        &[Operation::Update { object: preview }],
                    )
                    .is_ok()
                {
                    page = std::borrow::Cow::Owned(document.current_page().clone());
                }
            } else {
                page.to_mut().objects.push(preview);
            }
        }
        // #region debug-point B:canvas
        let debug_clone_us = debug_ink::micros(debug_clone_start);
        let debug_textures_start = debug_ink::start();
        // #endregion
        self.textures
            .retain(|asset, _| self.session.resources.get(asset).is_some());
        for object in &page.objects {
            if let ObjectKind::Image { asset_ref, .. } = &object.kind
                && !self.textures.contains_key(asset_ref)
                && let Some(resource) = self.session.resources.get(asset_ref)
            {
                match features::decode_png(&resource.bytes) {
                    Ok(image) => {
                        self.textures.insert(
                            asset_ref.clone(),
                            ui.ctx()
                                .load_texture(asset_ref, image, egui::TextureOptions::LINEAR),
                        );
                    }
                    Err(e) => self.status = format!("图片解码失败：{e}"),
                }
            }
        }
        let resources = |asset: &str| self.textures.get(asset).map(egui::TextureHandle::id);
        // #region debug-point B:canvas
        let debug_textures_us = debug_ink::micros(debug_textures_start);
        let debug_render_start = debug_ink::start();
        // #endregion
        let rendered = if previewing {
            // 临时内容不共享文档 revision；旧入口清除 revision 键，取消后会重建真实页。
            self.renderer.paint_page_with_resources(
                &painter,
                &page,
                self.mode == AppMode::Blackboard,
                &resources,
            )
        } else {
            self.renderer.paint_page_at_document_revision(
                &painter,
                &page,
                (&self.session.document.id, self.session.document.revision),
                self.mode == AppMode::Blackboard,
                &resources,
            )
        };
        if let Some(stroke) = &pen_preview {
            board_render::paint_object_with_resources(&painter, stroke, &resources);
        }
        // #region debug-point B:canvas
        let debug_render_us = debug_ink::micros(debug_render_start);
        debug_ink::sample(
            0,
            "A,B",
            "gui.rs:canvas",
            ["preview", "page_clone_apply", "textures", "render"],
            [
                debug_preview_us,
                debug_clone_us,
                debug_textures_us,
                debug_render_us,
            ],
            || {
                serde_json::json!({
                    "page": debug_ink::counts(&page),
                    "gesture_points": self.gesture.as_ref().map_or(0, |g| g.points.len()),
                    "page_owned": matches!(page, std::borrow::Cow::Owned(_)),
                    "previewing": previewing || pen_preview.is_some(), "revision": self.session.document.revision,
                    "render_entry": if previewing { "content_cache" } else if pen_preview.is_some() { "revision_cache_plus_pen" } else { "revision_cache" },
                    "cache_hit": "not_exposed", "render_ok": rendered.is_ok(),
                    "blackboard_background_requested": self.mode == AppMode::Blackboard,
                })
            },
        );
        // #endregion
        if let Err(e) = rendered {
            if let board_render::RenderError::Math(error) = e {
                painter.text(
                    rect.left_top() + Vec2::splat(8.0),
                    Align2::LEFT_TOP,
                    format!(
                        "函数图绘制失败：{}",
                        features::plot_text(&error.to_string())
                    ),
                    egui::FontId::proportional(14.0),
                    Color32::LIGHT_RED,
                );
            } else {
                self.status = e.to_string();
            }
        }
        if let Some(result) = &vertex_edit {
            let hint = match result {
                Ok(edit) => edit.hint.to_string(),
                Err(error) => format!("无法连接，本次拖动取消：{error}"),
            };
            if !hint.is_empty() {
                painter.text(
                    rect.left_top() + Vec2::new(8.0, 32.0),
                    Align2::LEFT_TOP,
                    hint,
                    egui::FontId::proportional(16.0),
                    Color32::YELLOW,
                );
            }
        }
        if !plot_preview_changed
            && let Some(state) = &mut self.plot_points
            && let Some(Ok(report)) = &state.report
        {
            for (pos, _) in &report.candidates {
                painter.circle_filled(*pos, 4.0, Color32::YELLOW);
            }
            if self.tool == Tool::Select
                && !blocked
                && response.clicked()
                && let Some(click) = response.interact_pointer_pos()
                && let Some((_, label)) = report
                    .candidates
                    .iter()
                    .filter(|(pos, _)| click.distance(*pos) <= 10.0)
                    .min_by(|(a, _), (b, _)| click.distance(*a).total_cmp(&click.distance(*b)))
            {
                state.coordinate = Some(label.clone());
            }
        }
        if let Some(object) = self
            .selected
            .as_ref()
            .and_then(|id| page.objects.iter().find(|o| &o.id == id))
        {
            painter.rect_stroke(
                board_render::object_bounds(object).expand(4.0),
                0.0,
                Stroke::new(1.0, Color32::LIGHT_BLUE),
                egui::StrokeKind::Outside,
            );
            for p in editing::vertices(object) {
                painter.circle_filled(Pos2::new(p.x, p.y), 5.0, Color32::LIGHT_BLUE);
            }
            for p in editing::resize_handles(object) {
                painter.rect_filled(
                    Rect::from_center_size(Pos2::new(p.x, p.y), Vec2::splat(10.0)),
                    0.0,
                    Color32::LIGHT_BLUE,
                );
            }
        }
        if let Some((bounds, allowed)) = merge_hint {
            let color = if allowed {
                Color32::LIGHT_GREEN
            } else {
                Color32::YELLOW
            };
            painter.rect_stroke(
                bounds.expand(4.0),
                0.0,
                Stroke::new(3.0, color),
                egui::StrokeKind::Outside,
            );
            painter.text(
                bounds.left_top() + Vec2::splat(8.0),
                Align2::LEFT_TOP,
                if allowed {
                    "松手合并函数图"
                } else {
                    PLOT_MERGE_LIMIT_HINT
                },
                egui::FontId::proportional(16.0),
                color,
            );
        }
        if blocked {
            return;
        }
        if self.tool == Tool::Eraser
            && let Some(pos) = response.hover_pos()
        {
            painter.circle_stroke(pos, self.eraser, Stroke::new(1.0, Color32::LIGHT_BLUE));
        }
        if response.clicked()
            && !response.drag_stopped()
            && let Some(object) = response
                .interact_pointer_pos()
                .and_then(|pos| self.clicked_image(pos))
        {
            self.open_image_agent(&object);
        } else if response.drag_stopped() || response.clicked() {
            self.finish_gesture();
        }
        if response.hovered() && self.tool == Tool::Select && self.gesture.is_none() {
            let scroll: f32 = ui.input(|i| {
                i.raw
                    .events
                    .iter()
                    .filter_map(|event| {
                        if let egui::Event::MouseWheel { delta, .. } = event {
                            Some(delta.y)
                        } else {
                            None
                        }
                    })
                    .sum()
            });
            if scroll != 0.0
                && let Some(mut object) = self.selected_object()
                && matches!(
                    object.kind,
                    ObjectKind::FunctionPlot { .. } | ObjectKind::CoordinateSystem { .. }
                )
                && response
                    .hover_pos()
                    .is_some_and(|pos| board_render::object_bounds(&object).contains(pos))
            {
                editing::scale_plot(&mut object, if scroll > 0.0 { 0.9 } else { 1.1 });
                self.apply(vec![Operation::Update { object }]);
            }
        }
    }
    fn polygon_vertex_edit(&self) -> Option<board_core::Result<editing::VertexEdit>> {
        let gesture = self.gesture.as_ref()?;
        let original = gesture.original.as_ref()?;
        let index = gesture.vertex?;
        if self.tool != Tool::Select
            || gesture.resize.is_some()
            || gesture.document != self.session.document.id
            || gesture.page != self.session.document.current_page().id
            || gesture.revision != self.session.document.revision
            || !matches!(original.kind, ObjectKind::Shape { shape, .. } if shape != ShapeKind::Line)
        {
            return None;
        }
        let first = gesture.points.first()?;
        let last = gesture.points.last()?;
        let query = if first.x == last.x && first.y == last.y {
            *editing::vertices(original).get(index)?
        } else {
            Point {
                x: last.x,
                y: last.y,
            }
        };
        Some(editing::polygon_vertex_edit(
            &self.session.document,
            original,
            index,
            query,
        ))
    }

    fn preview(&self) -> Option<BoardObject> {
        if let Some(edit) = self.polygon_vertex_edit() {
            return edit.ok().map(|edit| edit.object);
        }
        let gesture = self.gesture.as_ref()?;
        if gesture.document != self.session.document.id
            || gesture.page != self.session.document.current_page().id
            || gesture.revision != self.session.document.revision
        {
            return None;
        }
        let first = gesture.points.first()?;
        let last = gesture.points.last()?;
        let mut start = Point {
            x: first.x,
            y: first.y,
        };
        let mut end = Point {
            x: last.x,
            y: last.y,
        };
        let kind = match self.tool {
            Tool::Pen => ObjectKind::Stroke {
                points: gesture.points.clone(),
                style: self.style,
            },
            Tool::Shape => {
                if self.shape == ShapeKind::Line {
                    start =
                        features::snap_endpoint(self.session.document.current_page(), start, None);
                    end = board_ink::snap_angle(start, end, 6.0).unwrap_or(end);
                    end = features::snap_endpoint(self.session.document.current_page(), end, None);
                }
                ObjectKind::Shape {
                    shape: self.shape,
                    points: vec![start, end],
                    style: self.style,
                }
            }
            Tool::Coordinates => ObjectKind::CoordinateSystem {
                origin: start,
                scale: (end.x - start.x).abs().clamp(40.0, 200.0),
            },
            Tool::Select => {
                let mut object = gesture.original.clone()?;
                if let Some(index) = gesture.resize {
                    editing::resize_object(
                        &mut object,
                        index,
                        Point {
                            x: end.x - start.x,
                            y: end.y - start.y,
                        },
                    );
                } else if let Some(index) = gesture.vertex {
                    // A handle's hit area is larger than its endpoint; clicking is not a drag.
                    if start == end {
                        return Some(object);
                    }
                    end = editing::snap_vertex_angle(&object, index, end);
                    end = features::snap_endpoint(
                        self.session.document.current_page(),
                        end,
                        Some(&object.id),
                    );
                    editing::move_vertex(&mut object, index, end);
                } else {
                    editing::translate(
                        &mut object,
                        Point {
                            x: end.x - start.x,
                            y: end.y - start.y,
                        },
                    );
                }
                return Some(object);
            }
            _ => return None,
        };
        Some(BoardObject {
            id: "local-preview".into(),
            kind,
        })
    }
    fn plot_merge_target(&self, moved: &BoardObject) -> Option<&BoardObject> {
        // 非函数选择手势在扫描页面之前返回。
        if self.tool != Tool::Select || !matches!(moved.kind, ObjectKind::FunctionPlot { .. }) {
            return None;
        }
        let gesture = self.gesture.as_ref()?;
        let original = gesture.original.as_ref()?;
        if gesture.vertex.is_some()
            || gesture.resize.is_some()
            || !matches!(original.kind, ObjectKind::FunctionPlot { .. })
            || gesture.document != self.session.document.id
            || gesture.page != self.session.document.current_page().id
            || gesture.revision != self.session.document.revision
        {
            return None;
        }
        // 渲染 bounds 含描边外扩；合并只计算 position/width/height 定义的图框。
        let frame = |object: &BoardObject| {
            if let ObjectKind::FunctionPlot {
                position,
                width,
                height,
                ..
            } = &object.kind
            {
                Rect::from_min_size(
                    Pos2::new(position.x, position.y),
                    Vec2::new(*width, *height),
                )
            } else {
                unreachable!("only function plots reach overlap testing")
            }
        };
        let before = frame(original);
        let after = frame(moved);
        let overlap = |a: Rect, b: Rect| {
            let intersection = a.intersect(b);
            f64::from(intersection.width().max(0.0)) * f64::from(intersection.height().max(0.0))
        };
        self.session
            .document
            .current_page()
            .objects
            .iter()
            .rev()
            .find(|other| {
                if other.id == moved.id || !matches!(other.kind, ObjectKind::FunctionPlot { .. }) {
                    return false;
                }
                let target = frame(other);
                let area = overlap(after, target);
                // 以按下时为基准；边接触、面积不变及拖离均不合并。
                area > 0.0 && area > overlap(before, target)
            })
    }

    fn finish_gesture(&mut self) {
        self.finish_gesture_at(Instant::now());
    }

    fn finish_gesture_at(&mut self, now: Instant) {
        // #region debug-point B:commit
        #[cfg(test)]
        let _debug_stage = debug_ink::Stage::begin(8, "finish_gesture");
        // #endregion
        // #region debug-point D:commit
        let _debug_commit = debug_ink::Commit::begin(self);
        // #endregion
        self.sync_erase_preview();
        let vertex_edit = self.polygon_vertex_edit();
        let preview = self.preview();
        let merge = preview.as_ref().and_then(|source| {
            self.plot_merge_target(source)
                .map(|target| (target.id.clone(), features::merge_plots(source, target)))
        });
        let Some(gesture) = self.gesture.take() else {
            return;
        };
        if gesture.document != self.session.document.id
            || gesture.page != self.session.document.current_page().id
            || gesture.revision != self.session.document.revision
        {
            self.status = "绘制期间文档已更新，本次操作已取消".into();
            return;
        }
        if let Some(result) = vertex_edit {
            match result {
                Ok(edit) if gesture.original.as_ref() != Some(&edit.object) => {
                    let s = &mut self.session;
                    match s.history.edit(&mut s.document, |d| edit.apply(d)) {
                        Ok(()) => {
                            self.changed();
                            if !edit.hint.is_empty() {
                                self.status = edit.hint.replace("松手后", "已连接，");
                            }
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
                Ok(_) => {}
                Err(error) => self.status = format!("无法连接，本次拖动取消：{error}"),
            }
            return;
        }
        if self.tool == Tool::Eraser {
            if let Some(result) = gesture.erasing.and_then(|erasing| erasing.result) {
                match result {
                    Ok((_, operations)) => self.apply(operations),
                    Err(error) => self.status = error,
                }
            }
        } else if let Some(mut object) = preview {
            if self.tool == Tool::Select {
                if gesture.original.as_ref() != Some(&object) {
                    if let Some((target, operations)) = merge {
                        if let Some(operations) = operations {
                            self.selected = Some(target);
                            self.apply(operations);
                        } else {
                            let revision = self.session.document.revision;
                            self.apply(vec![Operation::Update { object }]);
                            if self.session.document.revision != revision {
                                self.status = PLOT_MERGE_LIMIT_HINT.into();
                            }
                        }
                    } else if matches!(
                        object.kind,
                        ObjectKind::Shape {
                            shape: ShapeKind::Line,
                            ..
                        }
                    ) && let Some(endpoint) = gesture.vertex
                    {
                        self.commit_line(object, false, &[endpoint]);
                    } else {
                        self.apply(vec![Operation::Update { object }]);
                    }
                }
            } else {
                if let ObjectKind::Stroke { points, .. } = &mut object.kind {
                    *points = board_ink::smooth_resample(points, 2.0, 0.45, 38.0)
                        .unwrap_or_else(|_| points.clone());
                }
                let stroke = if let ObjectKind::Stroke { points, .. } = &object.kind {
                    Some(points.clone())
                } else {
                    None
                };
                let mut ink = self.ink.clone();
                let mut sources = self.ink_sources.clone();
                let mut excluded = self.ink_excluded;
                let previous_revision = self.session.document.revision;
                object.id = new_id();
                let source = stroke.as_ref().map(|_| object.clone());
                if matches!(
                    object.kind,
                    ObjectKind::Shape {
                        shape: ShapeKind::Line,
                        ..
                    }
                ) {
                    self.commit_line(object, true, &[0, 1]);
                } else {
                    self.apply(vec![Operation::Add { object }]);
                }
                // Only successful fresh local pen commits teach style. Imports, replay,
                // host operations, generated answers and moving old ink never enter here.
                if self.mode == AppMode::Blackboard
                    && self.tool == Tool::Pen
                    && self.handwriting.enabled
                    && self.session.document.revision != previous_revision
                    && let Some(BoardObject {
                        kind: ObjectKind::Stroke { points, style },
                        ..
                    }) = &source
                {
                    self.handwriting.profile.observe_stroke(points, *style);
                }
                if self.ink_math_mode != InkMathMode::Off
                    && self.session.document.revision != previous_revision
                    && let Some(stroke) = stroke
                {
                    if !features::joins_expression(&ink, &stroke) {
                        excluded.0 += ink.len();
                        excluded.1 += ink.iter().map(Vec::len).sum::<usize>();
                        ink.clear();
                        sources.clear();
                    }
                    ink.push(stroke);
                    sources.push(source.unwrap());
                    let context = ContextToken::capture(&self.session.document);
                    match self.auto_gate.strokes_finished(now, context.clone(), &ink) {
                        Ok(()) => {
                            self.ink = ink;
                            self.ink_sources = sources;
                            self.ink_excluded = excluded;
                            self.ink_context = Some(context);
                        }
                        Err(e) => self.status = e.to_string(),
                    }
                }
            }
        }
    }
}

impl eframe::App for BoardApp {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // #region debug-point D:logic
        let debug_logic_start = debug_ink::start();
        let mut debug_incoming = 0_u32;
        // #endregion
        for _ in 0..64 {
            let message = self.incoming.as_ref().map(|rx| rx.try_recv());
            let message = match message {
                Some(Ok(message)) => message,
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    self.disconnected();
                    break;
                }
                _ => break,
            };
            // #region debug-point D:logic
            debug_incoming += u32::from(debug_logic_start.is_some());
            // #endregion
            self.incoming_message(message);
        }
        // #region debug-point D:logic
        let debug_incoming_us = debug_ink::micros(debug_logic_start);
        // #endregion
        if let Some(request) = self.session.pending_window_request().cloned()
            && let Some(window) = frame.winit_window()
        {
            let applied =
                self.applied_window_request.as_deref() == Some(request.request_id.as_str());
            let confirmed = applied
                && window.is_visible() == Some(request.visible)
                && self.session.state()["owned_window_count"] == 1;
            window.set_visible(request.visible);
            self.applied_window_request = Some(request.request_id.clone());
            if confirmed {
                let out = self
                    .session
                    .acknowledge_window(&request.request_id, request.visible);
                self.messages(out);
            }
        }
        self.poll_local_capture();
        if self.allow_close
            || (self.session.closed && self.session.pending_window_request().is_none())
        {
            self.cancel_local_capture();
            self.allow_close = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        self.enforce_tool_mode();
        if let Some(window) = frame.winit_window() {
            let pass = self.mode == AppMode::Drawing
                && self.tool == Tool::Mouse
                && !self.compact_entry()
                && self.session.effective_visible()
                && !self.controls.is_empty()
                && !self.files
                && !self.math
                && !self.confirm_close
                && self.authorization.is_none()
                && !egui::Popup::is_any_open(ctx)
                && !ctx.input(|i| i.pointer.any_down())
                && cursor_position()
                    .and_then(|(x, y)| {
                        window.inner_position().ok().map(|p| {
                            let scale = ctx.pixels_per_point();
                            let local = features::cursor_logical((x, y), (p.x, p.y), scale);
                            features::passthrough_at(local, ctx.content_rect(), &self.controls)
                        })
                    })
                    .unwrap_or(false);
            if pass != self.passthrough {
                if window.set_cursor_hittest(!pass).is_ok() {
                    self.passthrough = pass;
                } else {
                    self.tool = Tool::Pen;
                    self.status = "系统不支持鼠标穿透，已恢复画笔".into();
                }
            }
        }
        // #region debug-point D:logic
        let debug_workers_start = debug_ink::start();
        // #endregion
        self.prepare_workers(ctx, Instant::now());
        // #region debug-point D:logic
        let debug_workers_us = debug_ink::micros(debug_workers_start);
        let debug_ink_start = debug_ink::start();
        // #endregion
        // GUI 发起的任务 60 秒后请求取消；隐藏租约仍只能由宿主停止确认释放。
        for id in self
            .jobs
            .iter()
            .filter(|(_, started)| started.elapsed() >= Duration::from_secs(60))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>()
        {
            self.cancel_job(&id);
        }
        ctx.request_repaint_after(Duration::from_millis(30));
        // #region debug-point D:logic
        let debug_ink_us = debug_ink::micros(debug_ink_start);
        let debug_logic_us = debug_ink::micros(debug_logic_start);
        debug_ink::sample(
            1,
            "D",
            "gui.rs:logic",
            ["logic", "incoming", "poll_workers", "poll_ink_jobs"],
            [
                debug_logic_us,
                debug_incoming_us,
                debug_workers_us,
                debug_ink_us,
            ],
            || {
                serde_json::json!({
                    "incoming_messages": debug_incoming, "jobs": self.jobs.len(),
                    "hwr_busy": self.hwr_worker.busy(), "math_busy": self.math_worker.busy(),
                    "plot_busy": self.plot_worker.busy(), "gesture_active": self.gesture.is_some(),
                    "visible": self.session.effective_visible(), "revision": self.session.document.revision,
                    "native_visible": frame.winit_window().and_then(|w| w.is_visible()),
                    "previous_frame_cpu_seconds": frame.info().cpu_usage,
                })
            },
        );
        // #endregion
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // #region debug-point B:C:frame
        let debug_frame_gap_us = debug_ink::frame_gap();
        let debug_ui_start = debug_ink::start();
        // #endregion
        let ctx = ui.ctx().clone();
        self.board_ui(ui, &ctx);
        // #region debug-point B:C:frame
        let debug_ui_us = debug_ink::micros(debug_ui_start);
        debug_ink::sample(
            2,
            "B,C",
            "gui.rs:ui",
            ["ui", "frame_gap", "", ""],
            [debug_ui_us, debug_frame_gap_us, 0, 0],
            || {
                serde_json::json!({
                    "page": debug_ink::counts(self.session.document.current_page()),
                    "gesture_active": self.gesture.is_some(), "revision": self.session.document.revision,
                    "clear_alpha": <Self as eframe::App>::clear_color(self, ui.visuals())[3],
                    "window_fill_alpha": ui.visuals().window_fill.a(),
                    "panel_fill_alpha": ui.visuals().panel_fill.a(),
                    "scope": "app_ui_cpu_not_gpu_present",
                })
            },
        );
        // #endregion
    }
}

impl BoardApp {
    // The native logic phase and headless frame tests share this exact preparation
    // path. It may submit HWR, but visible results are accepted only after UI input.
    fn prepare_workers(&mut self, ctx: &egui::Context, now: Instant) {
        let context = ContextToken::capture(&self.session.document);
        if !self.session.effective_visible()
            || self.compact_entry()
            || self.allow_close
            || self.ink_context.as_ref().is_some_and(|old| old != &context)
        {
            self.cancel_ink();
        }
        if !self.session.effective_visible() || self.compact_entry() || self.allow_close {
            self.finish_worker_frame(ctx);
            return;
        }
        if !self.confirm_close {
            self.poll_backend_ink(ctx, now);
        }
    }

    fn board_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.board_ui_input(ui, ctx);
        self.finish_worker_frame(ctx);
    }

    fn finish_worker_frame(&mut self, ctx: &egui::Context) {
        self.sync_handwriting_context();
        if self.handwriting.poll_after_ui() {
            self.cancel_math();
        }
        // egui consumes keyboard events while building widgets. Poll unconditionally
        // afterwards, not behind an input gate that can starve on held keys/pointers.
        self.poll_workers(ctx);
    }

    fn board_ui_input(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.sync_handwriting_context();
        self.enforce_tool_mode();
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            self.request_close(ctx);
        }
        if !self.session.effective_visible() {
            return;
        }
        if self.compact_entry() {
            self.entry(ctx);
            return;
        }
        self.shortcuts(ctx);
        let previous_tool = self.tool;
        self.toolbar(ctx);
        if self.tool != previous_tool {
            self.gesture = None;
            self.cancel_ink();
        }
        if self.compact_entry() {
            return;
        }
        self.panels(ctx);
        self.authorization_panel(ctx);
        self.plot_controls(ctx);
        self.canvas(ui);
    }
}

#[cfg(windows)]
fn cursor_position() -> Option<(i32, i32)> {
    #[repr(C)]
    struct WinPoint {
        x: i32,
        y: i32,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetCursorPos(point: *mut WinPoint) -> i32;
    }
    let mut point = WinPoint { x: 0, y: 0 };
    // 仅查询指针坐标用于命中工具条，不采集屏幕或其他用户数据。
    (unsafe { GetCursorPos(&mut point) } != 0).then_some((point.x, point.y))
}
#[cfg(not(windows))]
fn cursor_position() -> Option<(i32, i32)> {
    None
}

fn core_color(c: Color32) -> Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Color { r, g, b, a }
}
fn egui_color(c: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}
const SHAPES: &[ShapeKind] = &[
    ShapeKind::Line,
    ShapeKind::Rectangle,
    ShapeKind::Square,
    ShapeKind::Triangle,
    ShapeKind::RightTriangle,
    ShapeKind::EquilateralTriangle,
    ShapeKind::Parallelogram,
    ShapeKind::Rhombus,
    ShapeKind::Ellipse,
    ShapeKind::Circle,
    ShapeKind::Cube,
    ShapeKind::Cuboid,
    ShapeKind::Cylinder,
    ShapeKind::Cone,
    ShapeKind::Sphere,
];
fn shape_name(shape: ShapeKind) -> &'static str {
    match shape {
        ShapeKind::Line => "线段",
        ShapeKind::Rectangle => "长方形",
        ShapeKind::Square => "正方形",
        ShapeKind::Triangle => "三角形",
        ShapeKind::RightTriangle => "直角三角形",
        ShapeKind::EquilateralTriangle => "等边三角形",
        ShapeKind::Parallelogram => "平行四边形",
        ShapeKind::Rhombus => "菱形",
        ShapeKind::Ellipse => "椭圆",
        ShapeKind::Circle => "圆",
        ShapeKind::Cube => "立方体",
        ShapeKind::Cuboid => "长方体",
        ShapeKind::Cylinder => "圆柱",
        ShapeKind::Cone => "圆锥",
        ShapeKind::Sphere => "球体",
    }
}

#[cfg(test)]
mod tests;
