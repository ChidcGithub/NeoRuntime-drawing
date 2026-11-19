#[path = "gui/capture.rs"]
mod capture;
#[path = "gui/handwriting_ui.rs"]
mod handwriting_ui;

#[path = "gui/ink_flow.rs"]
mod ink_flow;

use model::*;
#[cfg(test)]
use toolbar::toolbar_scale;

#[path = "gui/model.rs"]
mod model;

#[path = "gui/app_frame.rs"]
mod app_frame;

#[path = "gui/gesture.rs"]
mod gesture;

#[path = "gui/canvas.rs"]
mod canvas;

#[path = "gui/plot.rs"]
mod plot;

#[path = "gui/panels.rs"]
mod panels;

#[path = "icons.rs"]
mod icons;
#[path = "gui/messaging.rs"]
mod messaging;
#[path = "gui/toolbar.rs"]
mod toolbar;
use crate::{AppMode, Result, editing, efficiency, emit, features, transport_event};
use board_core::{
    BoardObject, Color, ObjectKind, Operation, Point, ShapeKind, StrokePoint, Style, new_id,
};
use board_hwr::{
    AutoCalculate, CalculationRequest, ContextToken, InkRecognizer, NeuralFormula,
    NeuralRecognizer, Recognition, latex_to_expression,
};
use board_protocol::{Event, Message, ProtocolError, Request, Response, read_message};
use board_session::Session;
use egui::{Align2, Color32, Key, Pos2, Rect, Sense, Stroke, Vec2, ViewportCommand};
use egui_surface::actions::ActionButton;
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

/// NeoUI 全量主题：深/浅两个主题槽都换成 NeoUI 品牌预设（跟随系统明暗），
/// 触摸距离 1.2（Comfortable 基线 -> 主按钮高约 40pt，希沃式易点但不大）。
fn apply_neoui_themes(ctx: &egui::Context) {
    apply_theme_seed(ctx, None);
}

/// 单个主题槽的预设：宿主给定种子色时用 from_seed 动态生成，否则用品牌预设。
fn themed_preset(
    mode: neo_ui_adapter::ThemeMode,
    seed: Option<Color32>,
) -> neo_ui_adapter::NeoStyle {
    let base = match seed {
        Some(seed) => neo_ui_adapter::NeoStyle::from_seed(seed, mode),
        None => neo_ui_adapter::NeoStyle::new(mode),
    };
    base.with_density(neo_ui_adapter::Density::Comfortable)
        .with_distance(1.2)
}

/// 动态取色：把种子色应用到深/浅两个主题槽（跟随系统明暗），
/// 密度与触摸距离不变；None 恢复内置品牌预设。
pub(crate) fn apply_theme_seed(ctx: &egui::Context, seed: Option<Color32>) {
    for (slot, mode) in [
        (egui::Theme::Dark, neo_ui_adapter::ThemeMode::Dark),
        (egui::Theme::Light, neo_ui_adapter::ThemeMode::Light),
    ] {
        let preset = themed_preset(mode, seed);
        ctx.style_mut_of(slot, |style| preset.apply_to(style));
    }
    ctx.request_repaint();
}

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
        crate::handwriting_fallback::sample_with_corner(ch, font, profile.style_corner())
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

/// 选择模式双指捏合：锚定起始中点下的板面点，缩放按距离比、平移随中点（跟手）。
#[derive(Clone, Copy)]
struct PinchGesture {
    start_dist: f32,
    start_zoom: f32,
    anchor: Point,
}

/// 选择模式空白处拖动（单指/鼠标）：跟手平移，松手按平滑末速惯性滑行。
#[derive(Clone, Copy)]
struct ViewDrag {
    last: Pos2,
    last_time: f64,
    velocity: egui::Vec2,
}

/// 缩放比例提示状态：appeared=首次出现时刻，last_change=最后一次缩放时刻。
/// 显示值实时取自 view_zoom，不在状态里存副本。
#[derive(Clone, Copy)]
struct ZoomHud {
    appeared: f64,
    last_change: f64,
}

/// 五指手势方向（锁向后本触摸序列内不换向）。
#[derive(Clone, Copy, PartialEq)]
enum SwipeAxis {
    Down,
    Left,
    Right,
}

/// 五指手势锁定态：进度实时跟随手指，p≥1 弹跳提示松手执行，返回可撤销。
#[derive(Clone, Copy)]
struct SwipeGesture {
    axis: SwipeAxis,
    progress: f32,
    popped: bool,
    pop_time: f64,
}

struct CaptureContext {
    context: ContextToken,
    canvas_size: Vec2,
    stale: bool,
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
    multi_gestures: Vec<(egui::TouchId, Gesture)>,
    touch_writing: bool,
    // 无限黑板视图（板面→屏幕：screen = board × zoom + pan），按页记忆；不写入文档。
    view_zoom: f32,
    view_pan: egui::Vec2,
    page_views: HashMap<String, (f32, egui::Vec2)>,
    // 视图归属页：与当前页不一致（任何路径翻页）时存取切换。
    view_page_id: String,
    // 选择模式：双指捏合（跟手）；空白处单指拖动平移（带惯性）。
    pinch: Option<PinchGesture>,
    select_touches: Vec<(egui::TouchId, Pos2)>,
    // 触摸驱动的手势（形状/橡皮/坐标系）归属触点；指针手势不设置。
    touch_gesture: Option<egui::TouchId>,
    view_drag: Option<ViewDrag>,
    fling: egui::Vec2,
    // 五指手势：下滑最小化 / 左滑上一页 / 右滑下一页；中央图标随进度反馈。
    swipe: Option<SwipeGesture>,
    swipe_touches: Vec<(egui::TouchId, Pos2, Pos2)>,
    swipe_speed: f32,
    swipe_last: Option<(Pos2, f64)>,
    swipe_cooldown: bool,
    gestures_enabled: bool,
    gesture_threshold: f32,
    // 窗口首帧初始化：None→全屏+临时置顶；Some(false)→次帧回正常层。
    window_started: Option<bool>,
    // 缩放比例提示：缩放时在工具栏上方淡入，停止缩放 1 秒后淡出。
    zoom_hud: Option<ZoomHud>,
    selected: Option<String>,
    collapsed: bool,
    toolbar_hidden: bool,
    armed_toolbar_menu: Option<egui::Id>,
    thumbnails: HashMap<String, board_render::PageThumbnail>,
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
    texture_prune_revision: Option<u64>,
    plot_points: Option<PlotState>,
    split_drag: Option<(String, usize, ContextToken)>,
    image_path: String,
    captured_asset: Option<String>,
    capture_requests: HashMap<String, CaptureContext>,
    capture_jobs: HashMap<String, CaptureContext>,
    local_capture: Option<capture::LocalCapture>,
    capture_hide_window: bool,
    logger: crate::board_log::BoardLog,
    pending_protocol_error: Option<Message>,
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
    let mut logger = crate::board_log::BoardLog::new(hosted);
    if hosted {
        // eframe 首次呈现会自动显示根窗口，因此 configure 前根本不创建窗口。
        match crate::hosted_startup::run(
            &mut session,
            &mut std::io::stdin().lock(),
            &mut std::io::stdout().lock(),
            &crate::recovery_directory(),
        )? {
            crate::hosted_startup::StartupOutcome::Configured => {
                logger.info("hosted configured", &[]);
                for message in logger.drain() {
                    let _ = crate::emit(&message);
                }
            }
            outcome @ (crate::hosted_startup::StartupOutcome::Closed
            | crate::hosted_startup::StartupOutcome::Disconnected) => {
                logger.warn(
                    "hosted startup ended before window",
                    &[(
                        "outcome",
                        match outcome {
                            crate::hosted_startup::StartupOutcome::Closed => "closed",
                            _ => "disconnected",
                        },
                    )],
                );
                for message in logger.drain() {
                    let _ = crate::emit(&message);
                }
                return Ok(());
            }
        }
    }
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(mode.title())
        .with_transparent(mode == AppMode::Drawing)
        .with_decorations(false)
        // 黑板首帧转全屏（覆盖任务栏）；画板保持最大化透明浮层。
        .with_maximized(mode == AppMode::Drawing)
        .with_visible(!hosted);
    if mode == AppMode::Drawing {
        // 画板是透明标注浮层：持续置顶才能盖在桌面上。
        viewport = viewport.with_always_on_top();
    }
    let mut options = eframe::NativeOptions {
        renderer,
        viewport,
        ..Default::default()
    };
    // #region debug-point C:backend-override
    let debug_backend = debug_ink::backend_from_env(
        std::env::var("NEO_DRAW_DEBUG").ok().as_deref(),
        std::env::var("NEO_DRAW_DEBUG_BACKEND").ok().as_deref(),
    );
    // wgpu 后端探测：以子进程重新拉起自身，依次初始化候选后端（实例+适配器+
    // 设备）。驱动内的硬崩溃只杀死探测子进程，主进程按退出码/超时回退下一个
    // 候选或 Glow。显式强制 NEO_DRAW_DEBUG_BACKEND 时跳过探测——诊断场景需要
    // 复现原崩溃。
    if renderer == eframe::Renderer::Wgpu && debug_backend.is_none() {
        let probe = std::env::current_exe().ok().and_then(|exe| {
            crate::renderer_probe::probe_wgpu_backend_via_process(
                &exe,
                &crate::renderer_probe::candidates(None),
                crate::renderer_probe::PROBE_TIMEOUT,
                &mut logger,
            )
        });
        match probe {
            Some(backends) => {
                if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) =
                    &mut options.wgpu_options.wgpu_setup
                {
                    setup.instance_descriptor.backends = backends;
                }
                eprintln!("[render] wgpu 探测选定后端 {backends:?}");
            }
            None => {
                eprintln!("[render] wgpu 后端探测全部失败，回退 Glow 渲染器");
                options.renderer = eframe::Renderer::Glow;
            }
        }
        for message in logger.drain() {
            let _ = crate::emit(&message);
        }
    }
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
        apply_neoui_themes(&cc.egui_ctx);
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
            multi_gestures: Vec::new(),
            touch_writing: false,
            view_zoom: 1.0,
            view_pan: egui::Vec2::ZERO,
            page_views: HashMap::new(),
            view_page_id: String::new(),
            pinch: None,
            select_touches: Vec::new(),
            touch_gesture: None,
            view_drag: None,
            fling: egui::Vec2::ZERO,
            swipe: None,
            swipe_touches: Vec::new(),
            swipe_speed: 0.0,
            swipe_last: None,
            swipe_cooldown: false,
            gestures_enabled: true,
            gesture_threshold: 120.0,
            window_started: None,
            zoom_hud: None,
            selected: None,
            collapsed: false,
            toolbar_hidden: false,
            armed_toolbar_menu: None,
            thumbnails: HashMap::new(),
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
            texture_prune_revision: None,
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
            logger: crate::board_log::BoardLog::new(hosted),
            pending_protocol_error: None,
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
                self.logger.info("document saved", &[]);
                self.status = "已保存".into();
                self.changed();
                true
            }
            Err(e) => {
                self.logger.warn("document save failed", &[]);
                self.status = e.message;
                false
            }
        }
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
