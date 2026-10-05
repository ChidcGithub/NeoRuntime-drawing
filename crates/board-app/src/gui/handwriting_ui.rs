use crate::{features::Background, handwriting::Profile};
use board_core::{Color, HandwritingStroke, ObjectKind, Point, StrokePoint, Style};
use board_render::HandwritingFont;
use egui::{Color32, PointerButton, Pos2, Rect, Sense, Stroke, Vec2};
use std::path::PathBuf;

const CANVAS: f32 = 128.0;
const DISPLAY: f32 = 256.0;
const MAX_STROKES: usize = 16;
const MAX_POINTS: usize = 2048;

pub(super) struct HandwritingUi {
    pub(super) enabled: bool,
    pub(super) profile: Profile,
    label: String,
    draft: Draft,
    last_frame: Option<u64>,
    confirm_delete: bool,
    confirm_reset: bool,
    path: String,
    confirm_load: bool,
    confirm_overwrite: bool,
    preview_text: String,
    preview: Option<Preview>,
    worker: Background<WorkResult>,
    pending: Option<Pending>,
    sampling_frame: Option<u64>,
    font_present: bool,
    message: String,
    file_notice: String,
}

impl Default for HandwritingUi {
    fn default() -> Self {
        Self {
            enabled: true,
            profile: Profile::default(),
            label: String::new(),
            draft: Draft::default(),
            last_frame: None,
            confirm_delete: false,
            confirm_reset: false,
            path: String::new(),
            confirm_load: false,
            confirm_overwrite: false,
            preview_text: "0123".into(),
            preview: None,
            worker: Background::default(),
            pending: None,
            sampling_frame: None,
            font_present: false,
            message: String::new(),
            file_notice: String::new(),
        }
    }
}

// A bounded snapshot, captured only on an explicit action, not on every frame.
struct Source {
    profile: Profile,
    text: String,
    path: String,
    enabled: bool,
    font_present: bool,
}

impl Source {
    fn capture(ui: &HandwritingUi) -> Self {
        Self {
            profile: ui.profile.clone(),
            text: ui.preview_text.clone(),
            path: ui.path.clone(),
            enabled: ui.enabled,
            font_present: ui.font_present,
        }
    }

    fn matches(&self, ui: &HandwritingUi) -> bool {
        self.profile == ui.profile
            && self.text == ui.preview_text
            && self.path == ui.path
            && self.enabled == ui.enabled
            && self.font_present == ui.font_present
    }
}

#[derive(Clone, Copy, PartialEq)]
enum WorkKind {
    Preview,
    Load,
    Save,
}

struct Pending {
    ctx: egui::Context,
    kind: WorkKind,
    source: Source,
    stale: bool,
}

enum WorkResult {
    Preview(Result<PreviewPaint, String>),
    Load(Result<Profile, String>),
    Save(Result<(), String>),
}

struct Preview {
    source: Source,
    paint: PreviewPaint,
}

struct PreviewPaint {
    bounds: Rect,
    thumbnail: board_render::PageThumbnail,
}

impl PreviewPaint {
    fn new(kind: ObjectKind) -> Result<Self, String> {
        let mut object = board_core::BoardObject {
            id: "handwriting-preview".into(),
            kind,
        };
        let bounds = board_render::object_bounds(&object);
        if !bounds.is_finite() || !bounds.is_positive() {
            return Err("预览没有有效笔迹范围".into());
        }
        let ObjectKind::Handwritten { position, .. } = &mut object.kind else {
            return Err("预览不是个人笔迹对象".into());
        };
        position.x -= bounds.min.x;
        position.y -= bounds.min.y;
        // This is an isolated render page, never a session document or transaction.
        let page = board_core::Page {
            id: "handwriting-preview".into(),
            objects: vec![object],
        };
        Ok(Self {
            bounds,
            thumbnail: board_render::PageThumbnail::new(&page, ("handwriting-preview", 0)),
        })
    }

    fn paint(&self, ui: &mut egui::Ui) {
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width().clamp(1.0, DISPLAY), 112.0),
            Sense::hover(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::WHITE);
        let inner = rect.shrink(6.0);
        if !inner.is_positive() {
            return;
        }
        let scale = (inner.width() / self.bounds.width())
            .min(inner.height() / self.bounds.height())
            .min(2.0);
        let target = Rect::from_min_size(inner.min, self.bounds.size() * scale);
        // PageThumbnail caches the pressure-aware board renderer's mesh. Idle frames
        // reuse it; only the viewport/pixel scale changing rebuilds paint geometry.
        if let Err(error) = self.thumbnail.paint(
            &painter,
            target,
            self.bounds.size(),
            false,
            &board_render::RenderResources::default(),
        ) {
            ui.label(format!("预览绘制失败：{error}"));
        }
    }
}

fn render_preview(
    profile: &Profile,
    text: &str,
    font: Option<&HandwritingFont>,
) -> Result<ObjectKind, String> {
    let kind = ObjectKind::Text {
        position: Point::default(),
        text: text.into(),
        size: 26.0,
        color: Color::default(),
    };
    profile.render_adaptive(&kind, text, |ch| {
        crate::handwriting_fallback::sample(ch, font)
    })
}

#[derive(Default)]
struct Draft {
    strokes: Vec<HandwritingStroke>,
    active: Option<HandwritingStroke>,
    started: Option<f64>,
}

impl Draft {
    fn points(&self) -> usize {
        self.strokes.iter().map(|s| s.points.len()).sum::<usize>()
            + self.active.as_ref().map_or(0, |s| s.points.len())
    }

    fn begin(&mut self, point: Pos2, now: f64) -> Result<(), &'static str> {
        if self.strokes.len() >= MAX_STROKES {
            return Err("每份样本最多 16 笔；未开始新笔划，请撤回或清空草稿。");
        }
        if self.points() >= MAX_POINTS {
            return Err("每份样本最多 2048 点；未开始新笔划，请撤回或清空草稿。");
        }
        self.started.get_or_insert(now);
        self.active = Some(HandwritingStroke {
            points: Vec::new(),
            style: Style::default(),
        });
        self.push(point, now)
    }

    fn push(&mut self, point: Pos2, now: f64) -> Result<(), &'static str> {
        let Some(active) = &self.active else {
            return Ok(());
        };
        if active
            .points
            .last()
            .is_some_and(|p| p.x == point.x && p.y == point.y)
        {
            return Ok(());
        }
        if self.points() >= MAX_POINTS {
            self.active = None;
            return Err("超过 2048 点，已取消整条当前笔划（未截断）；之前的笔划仍保留。");
        }
        let active = self.active.as_mut().unwrap();
        let previous = active.points.last().map_or(0.0, |p| p.time);
        active.points.push(StrokePoint {
            x: point.x,
            y: point.y,
            time: (now - self.started.unwrap_or(now)).max(previous),
            pressure: 1.0,
        });
        Ok(())
    }

    fn finish(&mut self) {
        if let Some(stroke) = self.active.take() {
            self.strokes.push(stroke);
        }
    }

    fn undo(&mut self) {
        if self.active.take().is_none() {
            self.strokes.pop();
        }
        if self.strokes.is_empty() {
            self.started = None;
        }
    }
}

fn single_label(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let label = chars.next()?;
    (chars.next().is_none() && !label.is_whitespace() && !label.is_control()).then_some(label)
}

impl HandwritingUi {
    /// Parent must call on panel close, document/page/source changes, or font
    /// replacement (including Some -> Some: HandwritingFont has no identity API).
    /// Authorized file writes are not interrupted; their result remains reportable.
    pub(super) fn invalidate_context(&mut self) {
        self.invalidate_work();
        self.sampling_frame = None;
        self.cancel_stroke("采样上下文已改变，已取消当前笔划。");
    }

    fn invalidate_work(&mut self) {
        self.preview = None;
        if let Some(pending) = &mut self.pending {
            pending.stale = true;
        }
    }

    /// Call before the parent's global Ctrl-Z handler, while this panel is open.
    pub(super) fn wants_sampling_undo(&self, ctx: &egui::Context) -> bool {
        self.sampling_frame
            .is_some_and(|last| ctx.cumulative_frame_nr().saturating_sub(last) <= 1)
            && (self.draft.active.is_some() || !self.draft.strokes.is_empty())
            && !ctx.egui_wants_keyboard_input()
            && ctx.input(|i| i.focused)
    }

    pub(super) fn undo_sampling(&mut self) {
        self.draft.undo();
    }

    fn check_source(&mut self) {
        if self
            .pending
            .as_ref()
            .is_some_and(|p| !p.source.matches(self))
        {
            self.invalidate_work();
        }
        if self
            .preview
            .as_ref()
            .is_some_and(|p| !p.source.matches(self))
        {
            self.preview = None;
        }
    }

    fn start_work(&mut self, ctx: &egui::Context, kind: WorkKind, font: Option<&HandwritingFont>) {
        if self.worker.busy() {
            return;
        }
        let source = Source::capture(self);
        let profile = source.profile.clone();
        let text = source.text.clone();
        let path = PathBuf::from(source.path.trim());
        let font = font.cloned();
        self.pending = Some(Pending {
            ctx: ctx.clone(),
            kind,
            source,
            stale: false,
        });
        self.worker.start(ctx.clone(), move || match kind {
            WorkKind::Preview => WorkResult::Preview(
                render_preview(&profile, &text, font.as_ref()).and_then(PreviewPaint::new),
            ),
            WorkKind::Load => WorkResult::Load(Profile::load(&path)),
            WorkKind::Save => WorkResult::Save(profile.save(&path)),
        });
        match kind {
            WorkKind::Preview => {
                self.preview = None;
                self.message = "正在后台生成预览…".into();
            }
            WorkKind::Load => {
                self.file_notice = "正在后台加载；仅在来源仍有效时替换内存档案。".into();
            }
            WorkKind::Save => {
                self.file_notice =
                    "正在后台保存点击时的档案快照；关闭面板或修改档案不会取消已授权的写入。".into();
            }
        }
    }

    /// Deliver results only after all frame input (including board_ui) and the
    /// parent's sync_handwriting_context. On true, cancel math before poll_workers.
    /// Also call while hidden to drain stale work and report authorized snapshot
    /// saves; invalidate_context must first reflect closure/source/font changes.
    pub(super) fn poll_after_ui(&mut self) -> bool {
        self.check_source();
        let Some(result) = self.worker.take() else {
            return false;
        };
        let Some(pending) = self.pending.take() else {
            return false;
        };
        pending.ctx.request_repaint();
        if pending.kind != WorkKind::Save && pending.stale {
            if pending.kind == WorkKind::Load {
                self.file_notice = "加载结果已过期，未替换内存档案。".into();
            } else {
                self.message = "预览结果已过期，未应用。".into();
            }
            return false;
        }
        match result {
            Ok(WorkResult::Preview(Ok(paint))) => {
                self.preview = Some(Preview {
                    source: pending.source,
                    paint,
                });
                self.message = "已生成风格预览；未修改文档或学习统计。".into();
            }
            Ok(WorkResult::Preview(Err(error))) => {
                self.message = format!("预览失败：{error}");
            }
            Ok(WorkResult::Load(Ok(profile))) => {
                let changed = self.profile != profile;
                self.profile = profile;
                self.profile_changed();
                self.file_notice = "已加载指定档案；启用状态不变。".into();
                return changed;
            }
            Ok(WorkResult::Load(Err(error))) => {
                self.file_notice = format!("加载失败，原档案保留：{error}");
            }
            Ok(WorkResult::Save(Ok(()))) => {
                let newer = if pending.stale || pending.source.profile != self.profile {
                    "；当前来源或档案已有新变化，未包含在此快照中"
                } else {
                    "；不代表之后的变化会自动保存"
                };
                self.file_notice = format!(
                    "已保存点击时的档案快照：{}{newer}。",
                    pending.source.path.trim()
                );
            }
            Ok(WorkResult::Save(Err(error))) => {
                self.file_notice = format!("保存快照失败，内存档案保留：{error}");
            }
            Err(error) => {
                if pending.kind == WorkKind::Save {
                    self.file_notice =
                        format!("后台保存异常，写入状态未确认，请检查目标文件：{error}");
                } else if pending.kind == WorkKind::Load {
                    self.file_notice = format!("后台加载失败，原档案保留：{error}");
                } else {
                    self.message = format!("后台预览失败：{error}");
                }
            }
        }
        false
    }

    /// Returns immediate control changes affecting answer generation. Does not
    /// deliver worker results: the parent must call poll_after_ui after all input.
    pub(super) fn ui(&mut self, ui: &mut egui::Ui, font: Option<&HandwritingFont>) -> bool {
        let frame = ui.ctx().cumulative_frame_nr();
        if self.last_frame.is_some_and(|last| frame > last + 1) {
            self.invalidate_context();
        }
        self.last_frame = Some(frame);
        self.font_present = font.is_some();
        self.check_source();
        let mut changed = false;
        let mut sampling_shown = false;
        let mut panel_shown = false;
        let panel = ui.collapsing("个人笔迹答案（实验性）", |ui| {
            panel_shown = true;
            changed |= ui
                .checkbox(&mut self.enabled, "自动学习并使用个人笔迹（默认开启）")
                .changed();
            ui.label("启用时自动从本次新提交的本地画笔笔划学习风格统计，无需标签或确认，也无需先采样。");
            ui.small("不学习导入内容、生成答案或旧文档笔迹；只保留本地统计，不识别文字、不进行神经生成、不训练或下载模型、不联网。");
            ui.small("自动学习仅保存在本次会话内存，不自动读写文件；重启后启用设置恢复默认、内存档案不保留，可按需手动保存/加载档案。");
            ui.small("未采样字符使用模板或字体骨架加个人风格近似，不是本人字形的精确克隆；精确字形采样仅为可选补充。");
            ui.small("分数横线、根号及顶横线由结构化数学布局程序生成，不是个人采样笔迹。");
            ui.small("文档、恢复文件与导出会包含答案实际使用的字形和风格笔迹，不包含整个档案；分享前请留意个人笔迹信息。");
            ui.label(format!("已自动学习：{} 笔", self.profile.learned_strokes()));
            ui.label(self.profile.style_summary());
            let (labels, variants) = self.profile.counts();
            ui.label(format!("精确字形：{labels} 个字符 / {variants} 份样本"));
            ui.collapsing("可选：精确字形采样", |ui| {
                sampling_shown = true;
                changed |= self.exact_sampling_ui(ui);
            });
            ui.separator();
            ui.small("以下确认仅用于防止误删或覆盖，不是自动学习的前置条件。");
            ui.checkbox(&mut self.confirm_reset, "确认清空整个个人笔迹档案（含风格统计）");
            if ui
                .add_enabled(self.can_reset(), egui::Button::new("清空全部样本与风格"))
                .clicked()
            {
                self.profile.clear();
                self.profile_changed();
                self.message = "已清空内存档案；磁盘文件未改动。自动学习启用时，新笔划会继续积累风格。".into();
                changed = true;
            }
            ui.separator();
            self.files_ui(ui);
            ui.separator();
            self.preview_ui(ui, font);
            if self.worker.busy() {
                ui.small("后台任务处理中（单槽，无排队）；旧工作结束前不能提交新任务。");
                if self.pending.as_ref().is_some_and(|p| p.kind != WorkKind::Save && !p.stale)
                    && ui.button("丢弃本次结果（等待后台结束）").clicked()
                {
                    self.invalidate_work();
                }
            }
            if !self.file_notice.is_empty() {
                ui.label(&self.file_notice);
            }
            if !self.message.is_empty() {
                ui.label(&self.message);
            }
        });
        if !sampling_shown {
            self.sampling_frame = None;
            self.cancel_stroke("采样区域已折叠，已取消当前笔划。");
        }
        // The body can still run during a collapse animation. Invalidate on the
        // header click as well, before accepting a result from that same frame.
        if !panel_shown || panel.header_response.clicked() {
            self.invalidate_context();
        }
        changed
    }

    fn can_reset(&self) -> bool {
        self.confirm_reset && (self.profile.counts().0 > 0 || self.profile.learned_strokes() > 0)
    }

    fn exact_sampling_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.small(
            "仅在需要精确复用某个字形时采样；每字符最多 3 个变体，不影响无需标签的自动风格学习。",
        );
        ui.label(format!("已有字符：{}", self.profile.labels()));
        ui.label("单字符标签（修改标签会清空草稿）");
        if ui
            .add(egui::TextEdit::singleline(&mut self.label).desired_width(120.0))
            .changed()
        {
            self.label_changed();
        }
        let label = single_label(&self.label);
        if let Some(label) = label {
            ui.small(format!(
                "「{label}」已有 {} / 3 个变体",
                self.profile.sample_count(label)
            ));
        } else {
            ui.small("请输入且仅输入一个非空白、非控制字符；不自动识别标签。");
        }
        ui.small("采样逻辑坐标 128×128，显示 256×256；顶线 y=24，基线 y=96。");
        ui.small("鼠标或触笔的主指针输入；压力固定为 1，不提供原生触笔压感支持。");
        self.sampling_ui(ui, label.is_some());
        ui.small(format!(
            "草稿：{} 笔 / {} 点（上限 16 笔 / 2048 点）",
            self.draft.strokes.len() + usize::from(self.draft.active.is_some()),
            self.draft.points()
        ));
        ui.horizontal(|ui| {
            if ui.button("撤回一笔").clicked() {
                self.draft.undo();
            }
            if ui.button("清空草稿").clicked() {
                self.draft = Draft::default();
            }
        });
        let can_add = label.is_some_and(|ch| self.profile.sample_count(ch) < 3)
            && self.draft.active.is_none()
            && !self.draft.strokes.is_empty();
        if ui
            .add_enabled(can_add, egui::Button::new("加入精确字形"))
            .clicked()
        {
            changed |= self.add_draft();
        }
        ui.checkbox(&mut self.confirm_delete, "确认删除当前标签的全部变体");
        if ui
            .add_enabled(
                self.confirm_delete && label.is_some_and(|ch| self.profile.sample_count(ch) > 0),
                egui::Button::new("删除当前标签"),
            )
            .clicked()
        {
            self.profile.remove(label.unwrap());
            self.profile_changed();
            self.message = "已删除该字符的全部变体；尚未保存到文件。".into();
            changed = true;
        }
        changed
    }

    fn label_changed(&mut self) {
        self.draft = Draft::default();
        self.confirm_delete = false;
        self.message = "标签已修改，草稿已清空；请重新采样。".into();
    }

    fn profile_changed(&mut self) {
        self.draft = Draft::default();
        self.confirm_delete = false;
        self.confirm_reset = false;
        self.confirm_load = false;
        self.confirm_overwrite = false;
        self.invalidate_work();
    }

    fn add_draft(&mut self) -> bool {
        let Some(label) = single_label(&self.label) else {
            return false;
        };
        if self.draft.active.is_some() || self.draft.strokes.is_empty() {
            return false;
        }
        match self.profile.add_sample(label, self.draft.strokes.clone()) {
            Ok(()) => {
                self.profile_changed();
                self.message = "样本已加入内存档案；可继续采集变体，请按需显式保存。".into();
                true
            }
            Err(error) => {
                self.message = format!("未添加样本：{error}");
                false
            }
        }
    }

    fn cancel_stroke(&mut self, reason: &str) {
        if self.draft.active.take().is_some() {
            self.message = reason.into();
        }
    }

    fn sampling_ui(&mut self, ui: &mut egui::Ui, valid_label: bool) {
        self.sampling_frame = Some(ui.ctx().cumulative_frame_nr());
        if self.wants_sampling_undo(ui.ctx())
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z))
        {
            self.undo_sampling();
        }
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(DISPLAY), Sense::drag());
        let visible = rect.intersect(ui.clip_rect());
        let (events, now, focused, down) = ui.input(|i| {
            (
                i.events.clone(),
                i.time,
                i.focused,
                i.pointer.button_down(PointerButton::Primary),
            )
        });
        if !focused || !valid_label || !ui.is_enabled() {
            self.cancel_stroke("失焦或标签无效，已取消当前笔划。");
        } else {
            // Raw events retain press/release in one frame, including a single-point dot.
            let can_start = response.contains_pointer()
                || response.is_pointer_button_down_on()
                || response.drag_started_by(PointerButton::Primary);
            for event in events {
                match event {
                    egui::Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed: true,
                        ..
                    } if can_start && visible.contains(pos) => {
                        let local = Pos2::ZERO + (pos - rect.min) * (CANVAS / DISPLAY);
                        if let Err(error) = self.draft.begin(local, now) {
                            self.message = error.into();
                        }
                    }
                    egui::Event::PointerMoved(pos)
                    | egui::Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed: false,
                        ..
                    } => {
                        if !visible.contains(pos) {
                            self.cancel_stroke("指针离开采样区域，已取消整条当前笔划。");
                        } else {
                            let local = Pos2::ZERO + (pos - rect.min) * (CANVAS / DISPLAY);
                            if let Err(error) = self.draft.push(local, now) {
                                self.message = error.into();
                            }
                            if matches!(event, egui::Event::PointerButton { pressed: false, .. }) {
                                self.draft.finish();
                            }
                        }
                    }
                    egui::Event::PointerGone | egui::Event::WindowFocused(false) => {
                        self.cancel_stroke("指针丢失或窗口失焦，已取消当前笔划。");
                    }
                    _ => {}
                }
            }
            if !down {
                self.cancel_stroke("未收到有效松手事件，已取消当前笔划。");
            }
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::WHITE);
        for (y, name) in [(24.0, "顶线 24"), (96.0, "基线 96")] {
            let y = rect.top() + y * (DISPLAY / CANVAS);
            painter.line_segment(
                [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
                Stroke::new(1.0, Color32::LIGHT_BLUE),
            );
            painter.text(
                Pos2::new(rect.left() + 3.0, y - 2.0),
                egui::Align2::LEFT_BOTTOM,
                name,
                egui::FontId::proportional(11.0),
                Color32::GRAY,
            );
        }
        for stroke in self.draft.strokes.iter().chain(self.draft.active.iter()) {
            paint_stroke(&painter, stroke, rect.min, DISPLAY / CANVAS);
        }
    }

    fn files_ui(&mut self, ui: &mut egui::Ui) {
        ui.label("个人笔迹档案 JSON 的明确文件路径（上限 8 MiB）");
        if ui.text_edit_singleline(&mut self.path).changed() {
            self.confirm_overwrite = false;
            self.confirm_load = false;
            self.invalidate_work();
        }
        ui.small("不自动读写、不扫描目录。加载替换内存档案；保存不改变启用状态。");
        ui.checkbox(&mut self.confirm_load, "确认加载此路径并替换当前内存档案");
        if ui
            .add_enabled(
                !self.worker.busy() && !self.path.trim().is_empty() && self.confirm_load,
                egui::Button::new("加载指定档案"),
            )
            .clicked()
        {
            self.confirm_load = false;
            self.start_work(ui.ctx(), WorkKind::Load, None);
        }
        // Require consent even for a new path: an existence check alone races with writers.
        ui.checkbox(
            &mut self.confirm_overwrite,
            "确认保存到此路径，并允许覆盖该路径已有文件",
        );
        if ui
            .add_enabled(
                !self.worker.busy() && !self.path.trim().is_empty() && self.confirm_overwrite,
                egui::Button::new("保存到指定路径"),
            )
            .clicked()
        {
            self.confirm_overwrite = false;
            self.start_work(ui.ctx(), WorkKind::Save, None);
        }
    }

    fn preview_ui(&mut self, ui: &mut egui::Ui, font: Option<&HandwritingFont>) {
        ui.label("个人风格预览（无需精确字形样本；不写入文档；非最终渲染效果）");
        if ui
            .add(
                egui::TextEdit::singleline(&mut self.preview_text)
                    .desired_width(180.0)
                    .char_limit(64),
            )
            .changed()
        {
            self.invalidate_work();
        }
        if ui
            .add_enabled(!self.worker.busy(), egui::Button::new("生成风格预览"))
            .clicked()
        {
            self.start_work(ui.ctx(), WorkKind::Preview, font);
        }
        self.check_source();
        if let Some(preview) = &self.preview {
            preview.paint.paint(ui);
        }
    }
}

fn paint_stroke(painter: &egui::Painter, stroke: &HandwritingStroke, origin: Pos2, scale: f32) {
    let color = stroke.style.color;
    let color = Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a);
    let width = stroke.style.width * scale;
    let points: Vec<_> = stroke
        .points
        .iter()
        .map(|p| origin + Vec2::new(p.x, p.y) * scale)
        .collect();
    if points.len() == 1 {
        painter.circle_filled(points[0], width / 2.0, color);
    } else if let (Some(first), Some(last)) = (points.first(), points.last()) {
        painter.circle_filled(*first, width / 2.0, color);
        painter.circle_filled(*last, width / 2.0, color);
        painter.add(egui::Shape::line(points, Stroke::new(width, color)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("handwriting-ui-{}", board_core::new_id()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn sampled_profile() -> Profile {
        let mut profile = Profile::default();
        let mut draft = Draft::default();
        draft.begin(Pos2::new(24.0, 24.0), 0.0).unwrap();
        draft.push(Pos2::new(30.0, 96.0), 0.5).unwrap();
        draft.finish();
        profile.add_sample('1', draft.strokes).unwrap();
        profile
    }

    fn drain(state: &mut HandwritingUi) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut changed = false;
        while state.worker.busy() {
            changed |= state.poll_after_ui();
            assert!(Instant::now() < deadline, "worker did not drain");
            std::thread::sleep(Duration::from_millis(1));
        }
        changed
    }

    // The gate makes invalidation deterministic, independent of disk/CPU timing.
    fn blocked(
        state: &mut HandwritingUi,
        kind: WorkKind,
        work: impl FnOnce() -> WorkResult + Send + 'static,
    ) -> mpsc::SyncSender<()> {
        let (tx, rx) = mpsc::sync_channel(1);
        state.pending = Some(Pending {
            ctx: egui::Context::default(),
            kind,
            source: Source::capture(state),
            stale: false,
        });
        state.worker.start(egui::Context::default(), move || {
            rx.recv_timeout(Duration::from_secs(10)).unwrap();
            work()
        });
        tx
    }

    #[test]
    fn stale_blocked_workers_remain_single_slot_and_never_replace_profile() {
        for kind in [WorkKind::Load, WorkKind::Preview] {
            for change in 0..8 {
                let ctx = egui::Context::default();
                let mut state = HandwritingUi::default();
                let tx = blocked(&mut state, kind, move || match kind {
                    WorkKind::Load => WorkResult::Load(Ok(sampled_profile())),
                    _ => WorkResult::Preview(
                        render_preview(&Profile::default(), "0123", None)
                            .and_then(PreviewPaint::new),
                    ),
                });
                match change {
                    0 => state.profile = sampled_profile(), // parent's automatic learning path
                    1 => state.preview_text.push('4'),
                    2 => state.font_present = true,
                    3 => state.profile_changed(), // reset invalidates even an equal value
                    4 => state.invalidate_context(), // panel/source/font replacement
                    5 => state.path = "different.json".into(),
                    6 => state.enabled = false,
                    _ => {
                        // Editing away and back still invalidates the request.
                        state.preview_text.push('4');
                        state.check_source();
                        state.preview_text.pop();
                    }
                }
                let expected = state.profile.clone();
                assert!(!state.poll_after_ui());
                assert!(state.worker.busy());
                state.start_work(&ctx, WorkKind::Save, None);
                assert_eq!(state.pending.as_ref().unwrap().kind as u8, kind as u8);
                tx.send(()).unwrap();
                assert!(!drain(&mut state));
                assert_eq!(state.profile, expected);
                assert!(state.preview.is_none());
            }
        }
    }

    #[test]
    fn file_failures_preserve_memory_and_failed_replace_preserves_target() {
        let temp = TempDirectory::new();
        let path = temp.0.join("invalid.json");
        std::fs::write(&path, "not json").unwrap();
        let ctx = egui::Context::default();
        let mut state = HandwritingUi {
            profile: sampled_profile(),
            path: path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let before = state.profile.clone();
        state.start_work(&ctx, WorkKind::Load, None);
        assert!(!drain(&mut state));
        assert_eq!(state.profile, before);
        assert!(state.file_notice.contains("加载失败"));
        let target = temp.0.join("existing-directory");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("sentinel"), "keep").unwrap();
        state.path = target.to_string_lossy().into_owned();
        state.start_work(&ctx, WorkKind::Save, None);
        assert!(!drain(&mut state));
        assert_eq!(state.profile, before);
        assert!(state.file_notice.contains("保存快照失败"));
        assert_eq!(
            std::fs::read_to_string(target.join("sentinel")).unwrap(),
            "keep"
        );
        assert_eq!(std::fs::read_dir(&temp.0).unwrap().count(), 2);
    }

    #[test]
    fn authorized_snapshot_save_finishes_after_changes_and_panel_close() {
        let temp = TempDirectory::new();
        let path = temp.0.join("snapshot.json");
        let original = sampled_profile();
        let mut state = HandwritingUi {
            profile: original.clone(),
            path: path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let snapshot = original.clone();
        let destination = path.clone();
        let tx = blocked(&mut state, WorkKind::Save, move || {
            WorkResult::Save(snapshot.save(&destination))
        });
        state.profile.clear();
        state.profile_changed();
        state.invalidate_context();
        assert!(!state.poll_after_ui());
        assert!(state.worker.busy());
        tx.send(()).unwrap();
        assert!(!drain(&mut state));
        assert_eq!(Profile::load(&path).unwrap(), original);
        assert_eq!(state.profile, Profile::default());
        assert!(state.file_notice.contains("点击时的档案快照"));
        assert!(state.file_notice.contains("未包含在此快照"));
    }

    #[test]
    fn ready_load_waits_for_post_ui_context_validation() {
        let temp = TempDirectory::new();
        let path = temp.0.join("ready-profile.json");
        let loaded = sampled_profile();
        loaded.save(&path).unwrap();
        for invalidate_after_ui in [false, true] {
            let ctx = egui::Context::default();
            let mut state = HandwritingUi {
                path: path.to_string_lossy().into_owned(),
                ..Default::default()
            };
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let panel = ui.collapsing("个人笔迹答案（实验性）", |_| {});
                let mut open = egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    panel.header_response.id,
                    true,
                );
                open.set_open(true);
                open.store(ui.ctx());
            });
            output.textures_delta.clear();
            // Background requests repaint only after sending its result. Waiting
            // for that callback proves the load is ready before ui(), without sleeps.
            let (tx, rx) = mpsc::channel();
            // A fresh context avoids coalescing with the panel's animation repaint.
            let worker_ctx = egui::Context::default();
            worker_ctx.set_request_repaint_callback(move |_| {
                let _ = tx.send(());
            });
            state.start_work(&worker_ctx, WorkKind::Load, None);
            rx.recv_timeout(Duration::from_secs(10)).unwrap();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                assert!(!state.ui(ui, None));
            });
            output.textures_delta.clear();
            assert_eq!(state.profile, Profile::default());
            assert!(state.worker.busy(), "ui must not consume a ready result");
            assert!(!state.pending.as_ref().unwrap().stale);
            if invalidate_after_ui {
                // Models a later expression edit / board input and the parent's
                // sync_handwriting_context, before any worker delivery.
                state.invalidate_context();
            }
            assert_eq!(state.poll_after_ui(), !invalidate_after_ui);
            assert!(!state.worker.busy());
            if invalidate_after_ui {
                assert_eq!(state.profile, Profile::default());
                assert!(state.file_notice.contains("加载结果已过期"));
            } else {
                assert_eq!(state.profile, loaded);
            }
            assert!(!state.poll_after_ui());
        }
    }

    #[test]
    fn accepted_load_returns_change_only_once_and_unchanged_load_returns_false() {
        let temp = TempDirectory::new();
        let path = temp.0.join("profile.json");
        let profile = sampled_profile();
        profile.save(&path).unwrap();
        let ctx = egui::Context::default();
        let mut state = HandwritingUi {
            path: path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        state.start_work(&ctx, WorkKind::Load, None);
        assert!(drain(&mut state));
        assert_eq!(state.profile, profile);
        assert!(!state.poll_after_ui());
        state.start_work(&ctx, WorkKind::Load, None);
        assert!(!drain(&mut state));
    }

    #[test]
    fn preview_equals_answer_render_without_mutating_profile_or_document() {
        let profile = sampled_profile();
        let before = profile.clone();
        let text = "0123 + 1";
        let kind = ObjectKind::Text {
            position: Point::default(),
            text: text.into(),
            size: 26.0,
            color: Color::default(),
        };
        let expected = profile
            .render_adaptive(&kind, text, |ch| {
                crate::handwriting_fallback::sample(ch, None)
            })
            .unwrap();
        assert_eq!(render_preview(&profile, text, None).unwrap(), expected);
        let paint = PreviewPaint::new(expected.clone()).unwrap();
        let object = board_core::BoardObject {
            id: "expected".into(),
            kind: expected,
        };
        assert_eq!(paint.bounds, board_render::object_bounds(&object));
        let ctx = egui::Context::default();
        for _ in 0..3 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| paint.paint(ui));
            assert!(!output.shapes.is_empty());
            output.textures_delta.clear();
        }
        assert_eq!(profile, before);
        let mut state = HandwritingUi {
            profile,
            ..Default::default()
        };
        state.start_work(&ctx, WorkKind::Preview, None);
        assert!(!drain(&mut state));
        assert!(state.preview.is_some());
        assert_eq!(state.profile, before);
        state.profile = Profile::default();
        state.check_source();
        assert!(state.preview.is_none());
    }

    #[test]
    fn sampling_undo_is_scoped_and_does_not_change_profile() {
        let ctx = egui::Context::default();
        let mut state = HandwritingUi::default();
        state.draft.begin(Pos2::new(20.0, 24.0), 0.0).unwrap();
        state.draft.finish();
        assert!(!state.wants_sampling_undo(&ctx));
        sampling_frame(&ctx, &mut state, vec![], true);
        assert!(state.wants_sampling_undo(&ctx));
        state.undo_sampling();
        assert_eq!(state.draft.points(), 0);
        assert!(!state.wants_sampling_undo(&ctx));
        assert_eq!(state.profile, Profile::default());
        state.invalidate_context();
        assert!(!state.wants_sampling_undo(&ctx));
    }

    fn pointer(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn sampling_frame(
        ctx: &egui::Context,
        state: &mut HandwritingUi,
        events: Vec<egui::Event>,
        focused: bool,
    ) {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(600.0))),
                events,
                focused,
                ..Default::default()
            },
            |ui| state.sampling_ui(ui, true),
        );
        assert!(!state.poll_after_ui());
        output.textures_delta.clear();
    }

    #[test]
    fn pointer_dot_outside_and_focus_loss() {
        let ctx = egui::Context::default();
        let mut state = HandwritingUi::default();
        let pos = Pos2::new(80.0, 80.0);
        sampling_frame(&ctx, &mut state, vec![], true);
        sampling_frame(
            &ctx,
            &mut state,
            vec![
                egui::Event::PointerMoved(pos),
                pointer(pos, true),
                pointer(pos, false),
            ],
            true,
        );
        assert_eq!(state.draft.strokes.len(), 1);
        assert_eq!(state.draft.strokes[0].points.len(), 1);
        sampling_frame(&ctx, &mut state, vec![pointer(pos, true)], true);
        assert!(state.draft.active.is_some());
        sampling_frame(
            &ctx,
            &mut state,
            vec![egui::Event::PointerMoved(Pos2::new(500.0, 500.0))],
            true,
        );
        assert!(state.draft.active.is_none());
        sampling_frame(
            &ctx,
            &mut state,
            vec![egui::Event::PointerMoved(pos), pointer(pos, false)],
            true,
        );
        assert_eq!(state.draft.strokes.len(), 1);
        sampling_frame(&ctx, &mut state, vec![pointer(pos, true)], true);
        assert!(state.draft.active.is_some());
        sampling_frame(
            &ctx,
            &mut state,
            vec![egui::Event::WindowFocused(false)],
            false,
        );
        assert!(state.draft.active.is_none());
        assert_eq!(state.draft.strokes.len(), 1);
    }

    #[test]
    fn idle_ui_does_not_invalidate_math() {
        let ctx = egui::Context::default();
        let mut state = HandwritingUi::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| assert!(!state.ui(ui, None)));
        assert!(!state.poll_after_ui());
        output.textures_delta.clear();
    }

    #[test]
    fn defaults_and_single_character_labels() {
        let state = HandwritingUi::default();
        assert!(state.enabled);
        assert_eq!(state.profile.counts(), (0, 0));
        assert_eq!(state.profile.learned_strokes(), 0);
        assert!(!state.can_reset());
        assert_eq!(state.preview_text, "0123");
        for text in ["", "12", " ", "\n", " 1", "1 "] {
            assert_eq!(single_label(text), None);
        }
        assert_eq!(single_label("中"), Some('中'));
    }

    #[test]
    fn dot_and_relative_time_survive_submission_without_confirmation() {
        let mut state = HandwritingUi {
            label: ".".into(),
            ..Default::default()
        };
        state.draft.begin(Pos2::new(30.0, 94.0), 100.0).unwrap();
        state.draft.finish();
        assert!(state.add_draft());
        assert_eq!(state.profile.sample_count('.'), 1);
        assert_eq!(state.profile.learned_strokes(), 0);
        assert!(!state.can_reset());
        state.confirm_reset = true;
        assert!(state.can_reset());
        let mut draft = Draft::default();
        draft.begin(Pos2::new(1.0, 2.0), 100.0).unwrap();
        draft.push(Pos2::new(2.0, 3.0), 100.25).unwrap();
        draft.finish();
        assert_eq!(draft.strokes[0].points[0].time, 0.0);
        assert_eq!(draft.strokes[0].points[1].time, 0.25);
        assert_eq!(draft.strokes[0].points[1].pressure, 1.0);
    }

    #[test]
    fn reset_accepts_style_only_profile_but_requires_destructive_confirmation() {
        let mut state = HandwritingUi::default();
        state.confirm_reset = true;
        assert!(!state.can_reset());
        let points = [
            StrokePoint {
                x: 20.0,
                y: 24.0,
                time: 0.0,
                pressure: 1.0,
            },
            StrokePoint {
                x: 56.0,
                y: 96.0,
                time: 0.8,
                pressure: 1.0,
            },
        ];
        assert!(state.profile.observe_stroke(&points, Style::default()));
        assert_eq!(state.profile.counts(), (0, 0));
        assert_eq!(state.profile.learned_strokes(), 1);
        assert!(state.can_reset());
        state.confirm_reset = false;
        assert!(!state.can_reset());
        state.confirm_reset = true;
        state.profile.clear();
        state.profile_changed();
        assert_eq!(state.profile.learned_strokes(), 0);
        assert!(!state.confirm_reset);
        assert!(!state.can_reset());
        assert!(state.enabled);
    }

    #[test]
    fn manual_sample_still_requires_valid_label_and_finished_ink() {
        let mut state = HandwritingUi {
            label: "1".into(),
            ..Default::default()
        };
        assert!(!state.add_draft());
        state.draft.begin(Pos2::new(20.0, 24.0), 0.0).unwrap();
        assert!(!state.add_draft());
        state.draft.finish();
        state.label = "12".into();
        assert!(!state.add_draft());
        assert_eq!(state.draft.strokes.len(), 1);
        state.label = "1".into();
        assert!(state.add_draft());
    }

    #[test]
    fn changing_label_discards_active_and_finished_ink() {
        let mut state = HandwritingUi::default();
        state.draft.begin(Pos2::new(1.0, 2.0), 1.0).unwrap();
        state.draft.finish();
        state.draft.begin(Pos2::new(2.0, 2.0), 2.0).unwrap();
        state.confirm_delete = true;
        state.label = "2".into();
        state.label_changed();
        assert_eq!(state.draft.points(), 0);
        assert!(state.draft.active.is_none());
        assert!(!state.confirm_delete);
        assert!(!state.add_draft());
    }

    #[test]
    fn limits_reject_instead_of_truncating() {
        let mut draft = Draft::default();
        for n in 0..MAX_STROKES {
            draft.begin(Pos2::new(1.0, 1.0), n as f64).unwrap();
            draft.finish();
        }
        assert!(draft.begin(Pos2::ZERO, 20.0).is_err());
        assert_eq!(draft.strokes.len(), MAX_STROKES);
        draft.undo();
        assert_eq!(draft.strokes.len(), MAX_STROKES - 1);
        let mut draft = Draft::default();
        draft.begin(Pos2::ZERO, 0.0).unwrap();
        draft.finish();
        draft.begin(Pos2::ZERO, 1.0).unwrap();
        for n in 2..MAX_POINTS {
            draft
                .push(Pos2::new((n % 128) as f32, 1.0), n as f64)
                .unwrap();
        }
        assert_eq!(draft.points(), MAX_POINTS);
        assert!(draft.push(Pos2::new(128.0, 128.0), 3000.0).is_err());
        assert!(draft.active.is_none());
        assert_eq!(draft.points(), 1);
    }

    #[test]
    fn fourth_variant_is_rejected_without_losing_draft() {
        let mut state = HandwritingUi {
            label: "1".into(),
            ..Default::default()
        };
        for _ in 0..3 {
            state.draft.begin(Pos2::new(20.0, 24.0), 0.0).unwrap();
            state.draft.finish();
            assert!(state.add_draft());
        }
        state.draft.begin(Pos2::new(20.0, 24.0), 0.0).unwrap();
        state.draft.finish();
        assert!(!state.add_draft());
        assert_eq!(state.profile.sample_count('1'), 3);
        assert_eq!(state.draft.strokes.len(), 1);
    }
}
