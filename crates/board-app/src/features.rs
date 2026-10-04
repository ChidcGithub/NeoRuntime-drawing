use board_core::{BoardObject, ObjectKind, Operation, Page, Point, StrokePoint, new_id};
use egui::{Pos2, Rect};

pub struct Background<T> {
    receiver: Option<std::sync::mpsc::Receiver<Result<T, String>>>,
    cancelled: bool,
}

impl<T: Send + 'static> Default for Background<T> {
    fn default() -> Self {
        Self {
            receiver: None,
            cancelled: false,
        }
    }
}

impl<T: Send + 'static> Background<T> {
    pub fn busy(&self) -> bool {
        self.receiver.is_some()
    }

    // 单槽、无排队；取消后也先排空旧工作，避免连续取消产生无界线程。
    pub fn start(&mut self, ctx: egui::Context, work: impl FnOnce() -> T + Send + 'static) {
        if self.busy() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.receiver = Some(rx);
        self.cancelled = false;
        let worker_tx = tx.clone();
        if let Err(error) = std::thread::Builder::new().spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                .map_err(|_| "后台计算异常结束".to_string());
            // 接收端随应用关闭销毁时允许发送失败，不重试、不重启任务。
            let _ = worker_tx.send(result);
            ctx.request_repaint();
        }) {
            let _ = tx.send(Err(format!("无法启动后台计算：{error}")));
        }
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    pub fn take(&mut self) -> Option<Result<T, String>> {
        let result = match self.receiver.as_ref()?.try_recv() {
            Ok(value) => value,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("后台计算异常结束".into()),
            Err(std::sync::mpsc::TryRecvError::Empty) => return None,
        };
        self.receiver = None;
        if self.cancelled { None } else { Some(result) }
    }
}

pub fn image_agent_rect(object: &BoardObject, viewport: Rect) -> Option<Rect> {
    let ObjectKind::Image {
        position,
        width,
        height,
        ..
    } = &object.kind
    else {
        return None;
    };
    let image = Rect::from_min_size(
        egui::pos2(position.x, position.y),
        egui::vec2(*width, *height),
    );
    let visible = image.intersect(viewport);
    if !visible.is_positive() {
        return None;
    }
    let size = egui::vec2(88.0, 26.0).min(visible.size());
    Some(Rect::from_min_size(
        visible.right_top() - egui::vec2(size.x, 0.0),
        size,
    ))
}

pub fn ink_bounds(strokes: &[Vec<StrokePoint>]) -> Rect {
    Rect::from_points(
        &strokes
            .iter()
            .flatten()
            .map(|p| Pos2::new(p.x, p.y))
            .collect::<Vec<_>>(),
    )
}

pub fn ink_action_rect(bounds: Rect, viewport: Rect) -> Option<Rect> {
    let size = egui::vec2(26.0, 26.0);
    if !bounds.is_finite() || viewport.width() < size.x || viewport.height() < size.y {
        return None;
    }
    let ink = bounds.expand(6.0);
    [
        egui::pos2(ink.right(), ink.top() - size.y),
        ink.right_top(),
        egui::pos2(ink.left() - size.x, ink.top()),
        egui::pos2(ink.right() - size.x, ink.bottom()),
    ]
    .into_iter()
    .map(|pos| Rect::from_min_size(pos.clamp(viewport.min, viewport.max - size), size))
    .find(|rect| !rect.intersect(ink).is_positive())
}

pub fn ink_action_rect_avoiding(bounds: Rect, viewport: Rect, occupied: &[Rect]) -> Option<Rect> {
    let preferred = ink_action_rect(bounds, viewport)?;
    if !occupied
        .iter()
        .any(|rect| rect.expand(2.0).intersects(preferred))
    {
        return Some(preferred);
    }
    let size = preferred.size();
    // Bounded viewport grid fallback; never overlap another candidate's control.
    let columns = (viewport.width() / 30.0) as usize;
    let rows = (viewport.height() / 30.0) as usize;
    let mut best: Option<(f32, Rect)> = None;
    for row in 0..rows.min(256) {
        for column in 0..columns.min(256) {
            let rect = Rect::from_min_size(
                viewport.min + egui::vec2(column as f32 * 30.0, row as f32 * 30.0),
                size,
            );
            if bounds.expand(6.0).intersects(rect)
                || occupied.iter().any(|old| old.expand(2.0).intersects(rect))
            {
                continue;
            }
            let distance = rect.center().distance_sq(preferred.center());
            if best.is_none_or(|(old, _)| distance < old) {
                best = Some((distance, rect));
            }
        }
    }
    best.map(|(_, rect)| rect)
}

// 只收集本轮相邻书写，避免把整页板书送入同一个表达式。
pub fn joins_expression(strokes: &[Vec<StrokePoint>], next: &[StrokePoint]) -> bool {
    if strokes.is_empty() {
        return true;
    }
    let bounds = ink_bounds(strokes);
    let next = ink_bounds(&[next.to_vec()]);
    bounds.expand2(egui::vec2(100.0, 28.0)).intersects(next)
}

pub fn passthrough_at(local: Pos2, viewport: Rect, controls: &[Rect]) -> bool {
    local.is_finite()
        && viewport.contains(local)
        && !controls.is_empty()
        && !controls
            .iter()
            .any(|rect| rect.expand(16.0).contains(local))
}

pub fn cursor_logical(screen: (i32, i32), origin: (i32, i32), pixels_per_point: f32) -> Pos2 {
    Pos2::new(
        (screen.0 as f64 - origin.0 as f64) as f32 / pixels_per_point,
        (screen.1 as f64 - origin.1 as f64) as f32 / pixels_per_point,
    )
}

pub fn result_position(bounds: Rect, equation: bool) -> Point {
    if equation {
        Point {
            x: bounds.left(),
            y: bounds.bottom() + 14.0,
        }
    } else {
        Point {
            x: bounds.right() + 16.0,
            y: bounds.top(),
        }
    }
}

pub fn calculation_output(
    text: &str,
    bounds: board_math::Bounds2D,
    answer_prompt: bool,
) -> Result<(String, bool), board_math::MathError> {
    calculation_display(text, bounds, answer_prompt).map(|output| (output.text, output.equation))
}

pub struct CalculationOutput {
    pub text: String,
    pub layout: Option<board_core::MathLayout>,
    pub equation: bool,
}

impl CalculationOutput {
    pub fn into_kind(self, position: Point, color: board_core::Color) -> ObjectKind {
        match self.layout {
            Some(layout) => ObjectKind::Math {
                position,
                layout,
                size: 26.0,
                color,
            },
            None => ObjectKind::Text {
                position,
                text: self.text,
                size: 26.0,
                color,
            },
        }
    }
}

fn math_layout(display: board_math::MathDisplay) -> board_core::MathLayout {
    use board_core::MathLayout;
    use board_math::MathDisplay;
    match display {
        MathDisplay::Text(text) => MathLayout::Text(text),
        MathDisplay::Row(children) => {
            MathLayout::Row(children.into_iter().map(math_layout).collect())
        }
        MathDisplay::Fraction(numerator, denominator) => MathLayout::Fraction(
            Box::new(math_layout(*numerator)),
            Box::new(math_layout(*denominator)),
        ),
        MathDisplay::Radical(child) => MathLayout::Radical(Box::new(math_layout(*child))),
    }
}

pub fn calculation_display(
    text: &str,
    bounds: board_math::Bounds2D,
    answer_prompt: bool,
) -> Result<CalculationOutput, board_math::MathError> {
    let text = text.trim();
    let prompt = if text.ends_with('=') {
        Some(board_hwr::latex_to_calculation(text).map_err(board_math::MathError::Syntax)?)
    } else {
        None
    };
    let input = prompt
        .as_ref()
        .map_or(text, |parsed| parsed.expression.as_str());
    let equation = input.contains('=');
    let result = board_math::calculate_display_with_bounds(input, bounds, Default::default())?;
    let prefix = if equation
        || answer_prompt
        || result.approximate
        || prompt.is_some_and(|parsed| parsed.answer_prompt)
    {
        ""
    } else {
        "= "
    };
    let layout = result.display.map(math_layout).map(|layout| {
        if prefix.is_empty() {
            layout
        } else {
            board_core::MathLayout::Row(vec![board_core::MathLayout::Text(prefix.into()), layout])
        }
    });
    Ok(CalculationOutput {
        text: format!("{prefix}{}", result.text),
        layout,
        equation,
    })
}

pub fn function_expression(text: &str) -> Option<String> {
    board_math::plot_expression(text)
}

pub fn plot(expression: String, position: Point) -> BoardObject {
    BoardObject {
        id: new_id(),
        kind: ObjectKind::FunctionPlot {
            position,
            width: 500.0,
            height: 350.0,
            expressions: vec![expression],
            x_min: -10.0,
            x_max: 10.0,
            y_min: -7.0,
            y_max: 7.0,
        },
    }
}

pub fn merge_plots(source: &BoardObject, target: &BoardObject) -> Option<Vec<Operation>> {
    if source.id == target.id {
        return None;
    }
    let ObjectKind::FunctionPlot {
        expressions: incoming,
        ..
    } = &source.kind
    else {
        return None;
    };
    let mut target = target.clone();
    let ObjectKind::FunctionPlot { expressions, .. } = &mut target.kind else {
        return None;
    };
    let mut unique = Vec::new();
    for expression in expressions.iter().chain(incoming) {
        if !unique.contains(expression) {
            unique.push(expression.clone());
        }
    }
    *expressions = unique;
    if expressions.len() > 16 {
        return None;
    }
    Some(vec![
        Operation::Update { object: target },
        Operation::Delete {
            id: source.id.clone(),
        },
    ])
}

pub fn split_plot(source: &BoardObject, index: usize, position: Point) -> Option<Vec<Operation>> {
    let mut source = source.clone();
    let ObjectKind::FunctionPlot { expressions, .. } = &mut source.kind else {
        return None;
    };
    if expressions.len() < 2 || index >= expressions.len() {
        return None;
    }
    let expression = expressions.remove(index);
    let mut separated = source.clone();
    separated.id = new_id();
    if let ObjectKind::FunctionPlot {
        expressions,
        position: p,
        ..
    } = &mut separated.kind
    {
        *expressions = vec![expression];
        *p = position;
    }
    Some(vec![
        Operation::Update { object: source },
        Operation::Add { object: separated },
    ])
}

pub struct ConnectionHit {
    pub object: BoardObject,
    pub anchor: board_core::Anchor,
    pub point: Point,
    distance: f32,
}

pub fn connection_hit(page: &Page, query: Point, excluded: Option<&str>) -> Option<ConnectionHit> {
    connection_hits(page, query, excluded).into_iter().next()
}

pub fn connection_hits(page: &Page, query: Point, excluded: Option<&str>) -> Vec<ConnectionHit> {
    let mut hits = Vec::new();
    for object in &page.objects {
        if Some(object.id.as_str()) == excluded {
            continue;
        }
        let ObjectKind::Shape { shape, points, .. } = &object.kind else {
            continue;
        };
        if *shape == board_core::ShapeKind::Line && points.len() == 2 {
            let mut endpoint_hit = false;
            for (index, &point) in points.iter().enumerate() {
                let distance = (point.x - query.x).hypot(point.y - query.y);
                if distance <= 12.0 {
                    hits.push(ConnectionHit {
                        object: object.clone(),
                        anchor: board_core::Anchor::Vertex { index },
                        point,
                        distance,
                    });
                    endpoint_hit = true;
                }
            }
            if endpoint_hit {
                continue;
            }
        }
        let geometry = if points.len() == 2 {
            board_ink::shape_geometry(*shape, points[0], points[1]).ok()
        } else {
            // 展开后的立体形状保留其几何拓扑，不把投影顶点误当作多边形。
            let edges =
                board_ink::shape_geometry(*shape, Point::default(), Point { x: 100.0, y: 100.0 })
                    .ok()
                    .filter(|g| g.vertices.len() == points.len())
                    .map(|g| g.edges)
                    .unwrap_or_else(|| {
                        (0..points.len())
                            .map(|i| [i, (i + 1) % points.len()])
                            .collect()
                    });
            Some(board_ink::ShapeGeometry {
                kind: *shape,
                vertices: points.clone(),
                edges,
            })
        };
        if let Some(geometry) = geometry
            && let Ok(Some(hit)) = board_ink::nearest_connection(&geometry, query, 12.0, None)
        {
            let anchor = match hit.site {
                board_ink::ConnectionSite::Vertex(index) => board_core::Anchor::Vertex { index },
                board_ink::ConnectionSite::Edge(index) => {
                    let [start, end] = geometry.edges[index];
                    board_core::Anchor::Edge {
                        start,
                        end,
                        t: hit.t,
                    }
                }
            };
            let mut object = object.clone();
            if let ObjectKind::Shape { points, .. } = &mut object.kind {
                *points = geometry.vertices;
            }
            hits.push(ConnectionHit {
                object,
                anchor,
                point: hit.point,
                distance: hit.distance,
            });
        }
    }
    // 同距离优先线端点，避免已绑定端点被重合目标的几何吸附掩盖；其余保持页面顺序。
    let endpoint = |hit: &ConnectionHit| {
        matches!(
            hit.object.kind,
            ObjectKind::Shape {
                shape: board_core::ShapeKind::Line,
                ..
            }
        ) && matches!(hit.anchor, board_core::Anchor::Vertex { .. })
    };
    hits.sort_by(|a, b| {
        a.distance
            .total_cmp(&b.distance)
            .then_with(|| endpoint(b).cmp(&endpoint(a)))
    });
    hits
}

pub fn snap_endpoint(page: &Page, query: Point, excluded: Option<&str>) -> Point {
    connection_hit(page, query, excluded).map_or(query, |hit| hit.point)
}

const MAX_PLOT_CURVES: usize = 16;
const MAX_PLOT_DIAGNOSTICS: usize = 6;

pub fn plot_text(text: &str) -> String {
    let mut chars = text.chars();
    let short: String = chars.by_ref().take(64).collect();
    if chars.next().is_some() {
        format!("{short}…")
    } else {
        short
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IntersectionIssue {
    Unsupported,
    Limit,
    Failed,
    DomainGap,
    NonDiscrete,
    PossiblyNonDiscrete,
}

#[derive(Debug)]
pub struct IntersectionDiagnostic {
    pub issue: IntersectionIssue,
    pub message: String,
}

#[derive(Debug)]
pub struct IntersectionReport {
    pub candidates: Vec<(Pos2, String)>,
    pub diagnostics: Vec<IntersectionDiagnostic>,
    pub omitted_diagnostics: usize,
    pub noncomplete: bool,
    pub failed_searches: usize,
    pub non_discrete_searches: usize,
}

impl Default for IntersectionReport {
    fn default() -> Self {
        Self {
            candidates: Vec::new(),
            diagnostics: Vec::new(),
            omitted_diagnostics: 0,
            noncomplete: true,
            failed_searches: 0,
            non_discrete_searches: 0,
        }
    }
}

impl IntersectionReport {
    fn diagnose(&mut self, issue: IntersectionIssue, scope: &str, message: &str) {
        if matches!(issue, IntersectionIssue::Limit | IntersectionIssue::Failed) {
            self.failed_searches += 1;
        }
        if matches!(
            issue,
            IntersectionIssue::NonDiscrete | IntersectionIssue::PossiblyNonDiscrete
        ) {
            self.non_discrete_searches += 1;
        }
        if self.diagnostics.len() < MAX_PLOT_DIAGNOSTICS {
            self.diagnostics.push(IntersectionDiagnostic {
                issue,
                message: format!("{}：{}", plot_text(scope), plot_text(message)),
            });
        } else {
            self.omitted_diagnostics += 1;
        }
    }

    fn error(&mut self, scope: &str, error: board_math::MathError) {
        let issue = match &error {
            board_math::MathError::Limit(_) => IntersectionIssue::Limit,
            board_math::MathError::Domain(_) => IntersectionIssue::DomainGap,
            // 库仅检查网格，不是恒等式证明；不能把采样近似相同宣称为重合。
            board_math::MathError::Unsupported(message) if message.contains("不能枚举离散根") =>
            {
                self.diagnose(
                    IntersectionIssue::PossiblyNonDiscrete,
                    scope,
                    "可能存在非离散交点；网格均为零，不证明曲线重合",
                );
                return;
            }
            _ => IntersectionIssue::Failed,
        };
        self.diagnose(issue, scope, &error.to_string());
    }

    pub fn summary(&self) -> String {
        let mut text = if self.candidates.is_empty() {
            if self
                .diagnostics
                .iter()
                .any(|d| d.issue == IntersectionIssue::Unsupported)
            {
                "暂不支持隐式曲线或显隐式混合图的交点搜索；未执行搜索".to_string()
            } else if self.non_discrete_searches > 0 {
                "存在或可能存在非离散交点，不能枚举为候选列表".to_string()
            } else {
                "未找到候选；非证明无交点".to_string()
            }
        } else {
            format!("找到 {} 候选", self.candidates.len())
        };
        if self.noncomplete {
            text.push_str("；非完备（有限区间搜索）");
        }
        if self.non_discrete_searches > 0 && !self.candidates.is_empty() {
            text.push_str("；含非离散交点或其可能情形");
        }
        if self.failed_searches > 0 {
            text.push_str("；部分搜索失败");
        }
        text
    }
}

// 只显示有限搜索区间内通过残差校验的候选，不能宣称枚举所有交点。
pub fn intersections(object: &BoardObject) -> IntersectionReport {
    intersections_with_options(
        object,
        board_math::NumericOptions {
            steps: 128,
            max_evaluations: 2048,
            ..Default::default()
        },
    )
}

fn intersections_with_options(
    object: &BoardObject,
    options: board_math::NumericOptions,
) -> IntersectionReport {
    let mut report = IntersectionReport::default();
    let ObjectKind::FunctionPlot {
        position,
        width,
        height,
        expressions,
        x_min,
        x_max,
        y_min,
        y_max,
    } = &object.kind
    else {
        return report;
    };
    // 最多处理前 16 条曲线；超限明确诊断，仍保留已搜索曲线的成功候选。
    if expressions.len() > MAX_PLOT_CURVES {
        report.diagnose(
            IntersectionIssue::Limit,
            "曲线数量",
            "超过 16 条曲线上限，仅搜索前 16 条",
        );
    }
    let expressions = &expressions[..expressions.len().min(MAX_PLOT_CURVES)];
    if expressions.iter().any(|expression| {
        matches!(
            board_math::classify_plot(expression),
            Ok(board_math::PlotKind::Implicit(_))
        )
    }) {
        report.diagnose(
            IntersectionIssue::Unsupported,
            "交点搜索",
            "暂不支持隐式曲线或显隐式混合图；未执行搜索",
        );
        return report;
    }
    let mut points = Vec::new();
    for (i, expression) in expressions.iter().enumerate() {
        let scope = format!("函数{} {} 与 x 轴", i + 1, plot_text(expression));
        match board_math::roots(expression, *x_min, *x_max, options) {
            Ok(roots) => points.extend(roots.roots.into_iter().map(|x| (x, 0.0))),
            Err(error) => {
                if expression.trim() == "0" {
                    report.diagnose(
                        IntersectionIssue::NonDiscrete,
                        &scope,
                        "恒零函数与 x 轴重合：非离散交点",
                    );
                } else {
                    report.error(&scope, error);
                }
            }
        }
        if *x_min <= 0.0 && *x_max >= 0.0 {
            match board_math::eval_at(expression, 0.0) {
                Ok(y) => points.push((0.0, y)),
                Err(error) => report.error(&format!("函数{} 与 y 轴", i + 1), error),
            }
        }
        for (j, other) in expressions.iter().enumerate().skip(i + 1) {
            let scope = format!("函数{} 与函数{}", i + 1, j + 1);
            if expression == other && board_math::parse(expression).is_ok() {
                report.diagnose(
                    IntersectionIssue::NonDiscrete,
                    &scope,
                    "完全相同表达式：共同定义域上重合，非离散交点（定义域可能为空）",
                );
                continue;
            }
            match board_math::curve_intersections(expression, other, *x_min, *x_max, options) {
                Ok(found) => points.extend(found.into_iter().map(|p| (p.x, p.y))),
                Err(error) => report.error(&scope, error),
            }
        }
    }
    points.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    points.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6);
    report.candidates = points
        .into_iter()
        .filter(|(x, y)| x.is_finite() && y.is_finite() && *y >= *y_min && *y <= *y_max)
        .map(|(x, y)| {
            (
                Pos2::new(
                    position.x + ((x - x_min) / (x_max - x_min)) as f32 * width,
                    position.y + ((y_max - y) / (y_max - y_min)) as f32 * height,
                ),
                format!("交点约 ({x:.6}, {y:.6})；有限区间数值候选"),
            )
        })
        .collect();
    report
}

pub fn decode_png(bytes: &[u8]) -> Result<egui::ColorImage, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let info = reader.info();
    if u64::from(info.width) * u64::from(info.height) > 16_777_216 {
        return Err("图片像素过多".into());
    }
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).map_err(|e| e.to_string())?;
    let bytes = &buffer[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => bytes.to_vec(),
        png::ColorType::Rgb => bytes
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Grayscale => bytes.iter().flat_map(|p| [*p, *p, *p, 255]).collect(),
        png::ColorType::GrayscaleAlpha => bytes
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        _ => return Err("不支持的图片颜色格式".into()),
    };
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [info.width as usize, info.height as usize],
        &rgba,
    ))
}

#[cfg(test)]
pub(crate) fn layout_text(layout: &board_core::MathLayout) -> String {
    use board_core::MathLayout;
    match layout {
        MathLayout::Text(text) => text.clone(),
        MathLayout::Row(children) => children.iter().map(layout_text).collect(),
        MathLayout::Fraction(numerator, denominator) => {
            format!("{}/{}", layout_text(numerator), layout_text(denominator))
        }
        MathLayout::Radical(child) => format!("sqrt({})", layout_text(child)),
    }
}

#[cfg(test)]
#[path = "features_tests.rs"]
mod tests;
