use board_core::{BoardObject, ObjectKind, Operation, Point, ShapeKind, new_id};

pub struct ErasePreview {
    source: board_core::Document,
    radius: f32,
    bounds: Vec<egui::Rect>,
    path: Vec<Point>,
    operations: Vec<Vec<Operation>>,
    pub result: Option<Result<(board_core::Document, Vec<Operation>), String>>,
}

impl ErasePreview {
    pub fn new(document: &board_core::Document, radius: f32) -> Self {
        // #region debug-point B:erase-source-clone
        #[cfg(test)]
        let _debug_stage = crate::gui::debug_ink::Stage::begin(12, "erase_source_clone");
        // #endregion
        Self {
            source: document.clone(),
            radius,
            bounds: document
                .current_page()
                .objects
                .iter()
                .map(board_render::object_bounds)
                .collect(),
            path: Vec::new(),
            operations: vec![Vec::new(); document.current_page().objects.len()],
            result: None,
        }
    }

    pub fn update(&mut self, points: &[board_core::StrokePoint]) {
        // #region debug-point B:erase-preview
        #[cfg(test)]
        let _debug_stage = crate::gui::debug_ink::Stage::begin(5, "erase_preview_update");
        // #endregion
        // 手势点只追加；停留帧复用结果（包括错误），不生成新的分段 ID。
        if self.path.len() == points.len() {
            return;
        }
        let reset =
            points.len() < self.path.len() || self.result.as_ref().is_some_and(Result::is_err);
        if reset {
            self.path.clear();
            self.operations.iter_mut().for_each(Vec::clear);
        }
        // Include the previous endpoint so a new segment cannot jump over an object.
        let start = self.path.len().saturating_sub(1);
        self.path.extend(
            points[self.path.len()..]
                .iter()
                .map(|p| Point { x: p.x, y: p.y }),
        );
        let added_bounds = path_bounds(&self.path[start..]).expand(self.radius);
        self.result = Some((|| {
            // #region debug-point B:erase-operations
            #[cfg(test)]
            let debug_operations = crate::gui::debug_ink::Stage::begin(9, "erase_operations");
            // #endregion
            for ((object, bounds), cached) in self
                .source
                .current_page()
                .objects
                .iter()
                .zip(&self.bounds)
                .zip(&mut self.operations)
            {
                if added_bounds.intersects(*bounds) {
                    *cached = erase_object_operations(object, *bounds, &self.path, self.radius)?;
                }
            }
            let operations: Vec<_> = self.operations.iter().flatten().cloned().collect();
            // #region debug-point B:erase-clone-apply
            #[cfg(test)]
            drop(debug_operations);
            #[cfg(test)]
            let _debug_clone_apply = crate::gui::debug_ink::Stage::begin(10, "erase_clone_apply");
            // #endregion
            let mut document = self.source.clone();
            if !operations.is_empty() {
                document
                    .apply(
                        &self.source.current_page().id,
                        self.source.revision,
                        &operations,
                    )
                    .map_err(|error| error.to_string())?;
            }
            Ok((document, operations))
        })());
    }
}

#[cfg(test)]
pub fn erase_operations(
    page: &board_core::Page,
    path: &[Point],
    radius: f32,
) -> Result<Vec<Operation>, String> {
    let mut operations = Vec::new();
    let path_bounds = path_bounds(path).expand(radius);
    for object in &page.objects {
        // object_bounds already includes half the pen width.
        let bounds = board_render::object_bounds(object);
        if path_bounds.intersects(bounds) {
            operations.extend(erase_object_operations(object, bounds, path, radius)?);
        }
    }
    Ok(operations)
}

fn path_bounds(path: &[Point]) -> egui::Rect {
    let mut bounds = egui::Rect::NOTHING;
    for point in path {
        bounds.extend_with(egui::pos2(point.x, point.y));
    }
    bounds
}

fn erase_object_operations(
    object: &BoardObject,
    bounds: egui::Rect,
    path: &[Point],
    radius: f32,
) -> Result<Vec<Operation>, String> {
    let mut operations = Vec::new();
    if let ObjectKind::Stroke { points, style } = &object.kind {
        let radius = radius + style.width / 2.0;
        if !board_ink::stroke_hit(points, path, radius).map_err(|e| format!("橡皮：{e:?}"))? {
            return Ok(operations);
        }
        let pieces =
            board_ink::erase_stroke(points, path, radius).map_err(|e| format!("橡皮：{e:?}"))?;
        operations.push(Operation::Delete {
            id: object.id.clone(),
        });
        operations.extend(pieces.into_iter().map(|points| Operation::Add {
            object: BoardObject {
                id: new_id(),
                kind: ObjectKind::Stroke {
                    points,
                    style: *style,
                },
            },
        }));
    } else if path
        .iter()
        .any(|p| bounds.expand(radius).contains(egui::pos2(p.x, p.y)))
    {
        operations.push(Operation::Delete {
            id: object.id.clone(),
        });
    }
    Ok(operations)
}

pub fn hit_test(object: &BoardObject, pos: egui::Pos2, tolerance: f32) -> bool {
    if !board_render::object_bounds(object)
        .expand(tolerance)
        .contains(pos)
    {
        return false;
    }
    match &object.kind {
        ObjectKind::Stroke { points, style } => board_ink::stroke_hit(
            points,
            &[Point { x: pos.x, y: pos.y }],
            tolerance + style.width / 2.0,
        )
        .unwrap_or(false),
        ObjectKind::Shape {
            shape,
            points,
            style,
        } => {
            let geometry = if points.len() == 2 {
                board_ink::shape_geometry(*shape, points[0], points[1]).ok()
            } else {
                board_ink::shape_geometry(*shape, Point::default(), Point { x: 100.0, y: 100.0 })
                    .ok()
                    .map(|mut g| {
                        g.vertices = points.clone();
                        g
                    })
            };
            geometry.is_some_and(|g| {
                g.edges.iter().any(|[a, b]| {
                    let (Some(a), Some(b)) = (g.vertices.get(*a), g.vertices.get(*b)) else {
                        return false;
                    };
                    let a = egui::pos2(a.x, a.y);
                    let b = egui::pos2(b.x, b.y);
                    let delta = b - a;
                    let t = if delta.length_sq() > 0.0 {
                        ((pos - a).dot(delta) / delta.length_sq()).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    pos.distance(a + delta * t) <= tolerance + style.width / 2.0
                })
            })
        }
        _ => true,
    }
}

pub fn translate(object: &mut BoardObject, delta: Point) {
    let shift = |p: &mut Point| {
        p.x += delta.x;
        p.y += delta.y;
    };
    match &mut object.kind {
        ObjectKind::Stroke { points, .. } => {
            for p in points {
                p.x += delta.x;
                p.y += delta.y;
            }
        }
        ObjectKind::Shape { points, .. } => {
            for p in points {
                shift(p);
            }
        }
        ObjectKind::Text { position, .. }
        | ObjectKind::Math { position, .. }
        | ObjectKind::Handwritten { position, .. }
        | ObjectKind::Image { position, .. }
        | ObjectKind::FunctionPlot { position, .. } => shift(position),
        ObjectKind::CoordinateSystem { origin, .. } => shift(origin),
    }
}

pub fn vertices(object: &BoardObject) -> Vec<Point> {
    if let ObjectKind::Shape { shape, points, .. } = &object.kind {
        if !board_ink::supports_vertex_edit(*shape) {
            return Vec::new();
        }
        if points.len() == 2 && *shape != ShapeKind::Line {
            return board_ink::shape_geometry(*shape, points[0], points[1])
                .map(|g| g.vertices)
                .unwrap_or_default();
        }
        return points.clone();
    }
    Vec::new()
}

pub fn move_vertex(object: &mut BoardObject, index: usize, target: Point) {
    let mut handles = vertices(object);
    if let Some(vertex) = handles.get_mut(index) {
        if *vertex == target {
            return;
        }
        *vertex = target;
        if let ObjectKind::Shape { points, .. } = &mut object.kind {
            *points = handles;
        }
    }
}

// 尺寸手柄不是几何顶点，不能参与连接锚点或自由顶点编辑。
fn curve_vertices(object: &BoardObject) -> Option<Vec<Point>> {
    let ObjectKind::Shape { shape, points, .. } = &object.kind else {
        return None;
    };
    if !matches!(shape, ShapeKind::Circle | ShapeKind::Ellipse) {
        return None;
    }
    if points.len() == 2 {
        board_ink::shape_geometry(*shape, points[0], points[1])
            .ok()
            .map(|g| g.vertices)
    } else {
        Some(points.clone())
    }
}

fn curve_bounds(object: &BoardObject) -> Option<egui::Rect> {
    let points = curve_vertices(object)?;
    let mut bounds = egui::Rect::NOTHING;
    for p in points {
        bounds.extend_with(egui::pos2(p.x, p.y));
    }
    bounds.is_finite().then_some(bounds)
}

fn frame_geometry(object: &BoardObject) -> Option<(Point, f32, f32)> {
    let (position, width, height) = match object.kind {
        ObjectKind::Image {
            position,
            width,
            height,
            ..
        }
        | ObjectKind::FunctionPlot {
            position,
            width,
            height,
            ..
        } => (position, width, height),
        _ => return None,
    };
    (position.x.is_finite()
        && position.y.is_finite()
        && width.is_finite()
        && height.is_finite()
        && width > 0.0
        && height > 0.0
        && (position.x + width).is_finite()
        && (position.y + height).is_finite())
    .then_some((position, width, height))
}

pub fn resize_handles(object: &BoardObject) -> Vec<Point> {
    let bounds = frame_geometry(object)
        .map(|(p, w, h)| egui::Rect::from_min_size(egui::pos2(p.x, p.y), egui::vec2(w, h)))
        .or_else(|| curve_bounds(object));
    let Some(bounds) = bounds else {
        return Vec::new();
    };
    // 小图形将手柄向外展开，保留中间的选择/平移区域。
    let bounds =
        egui::Rect::from_center_size(bounds.center(), bounds.size().max(egui::vec2(32.0, 32.0)));
    [
        bounds.left_top(),
        bounds.right_top(),
        bounds.right_bottom(),
        bounds.left_bottom(),
    ]
    .map(|p| Point { x: p.x, y: p.y })
    .to_vec()
}

pub fn resize_object(object: &mut BoardObject, index: usize, delta: Point) {
    if matches!(
        object.kind,
        ObjectKind::Image { .. } | ObjectKind::FunctionPlot { .. }
    ) {
        resize_frame(object, index, delta);
    } else {
        resize_curve(object, index, delta);
    }
}

fn resize_frame(object: &mut BoardObject, index: usize, delta: Point) {
    if index >= 4 || !delta.x.is_finite() || !delta.y.is_finite() || delta == Point::default() {
        return;
    }
    let Some((position, width, height)) = frame_geometry(object) else {
        return;
    };
    let (w, h) = (f64::from(width), f64::from(height));
    let left = index == 0 || index == 3;
    let top = index < 2;
    let dx = f64::from(delta.x) * if left { -1.0 } else { 1.0 };
    let dy = f64::from(delta.y) * if top { -1.0 } else { 1.0 };
    let (new_w, new_h) = if matches!(object.kind, ObjectKind::Image { .. }) {
        // Project the pointer onto the original diagonal: either axis can resize,
        // without stretching or reflecting the screenshot when crossing the anchor.
        let scale = (1.0 + (dx * w + dy * h) / (w * w + h * h)).max((32.0 / w).max(32.0 / h));
        (w * scale, h * scale)
    } else {
        ((w + dx).max(32.0), (h + dy).max(32.0))
    };
    let x = f64::from(position.x) + if left { w - new_w } else { 0.0 };
    let y = f64::from(position.y) + if top { h - new_h } else { 0.0 };
    // Match the editing coordinate budget, and validate before changing any field.
    if [new_w, new_h, x, y, x + new_w, y + new_h]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
    {
        return;
    }
    if let ObjectKind::Image {
        position,
        width,
        height,
        ..
    }
    | ObjectKind::FunctionPlot {
        position,
        width,
        height,
        ..
    } = &mut object.kind
    {
        *position = Point {
            x: x as f32,
            y: y as f32,
        };
        *width = new_w as f32;
        *height = new_h as f32;
    }
}

pub fn resize_curve(object: &mut BoardObject, index: usize, delta: Point) {
    if index >= 4
        || !delta.x.is_finite()
        || !delta.y.is_finite()
        || (delta.x == 0.0 && delta.y == 0.0)
    {
        return;
    }
    let Some(bounds) = curve_bounds(object) else {
        return;
    };
    let Some(mut transformed) = curve_vertices(object) else {
        return;
    };
    let corners = [
        bounds.left_top(),
        bounds.right_top(),
        bounds.right_bottom(),
        bounds.left_bottom(),
    ];
    let corner = corners[index];
    let anchor = corners[(index + 2) % 4];
    let dx = f64::from(corner.x) - f64::from(anchor.x);
    let dy = f64::from(corner.y) - f64::from(anchor.y);
    // 已坍缩轴无法从显式采样恢复；保持该轴，不猜测/重新生成顶点。
    let mut sx = if dx == 0.0 {
        1.0
    } else {
        (dx + f64::from(delta.x)) / dx
    };
    let mut sy = if dy == 0.0 {
        1.0
    } else {
        (dy + f64::from(delta.y)) / dy
    };
    if matches!(
        object.kind,
        ObjectKind::Shape {
            shape: ShapeKind::Circle,
            ..
        }
    ) {
        if dx == 0.0 || dy == 0.0 {
            return;
        }
        let scale = sx.abs().min(sy.abs());
        sx = scale * sx.signum();
        sy = scale * sy.signum();
    }
    for p in &mut transformed {
        p.x = (f64::from(anchor.x) + (f64::from(p.x) - f64::from(anchor.x)) * sx) as f32;
        p.y = (f64::from(anchor.y) + (f64::from(p.y) - f64::from(anchor.y)) * sy) as f32;
        if !p.x.is_finite()
            || !p.y.is_finite()
            || p.x.abs() > 1_000_000.0
            || p.y.abs() > 1_000_000.0
        {
            return;
        }
    }
    if sx == 1.0 && sy == 1.0 {
        return;
    }
    if let ObjectKind::Shape { points, .. } = &mut object.kind {
        *points = transformed;
    }
}

pub fn snap_vertex_angle(object: &BoardObject, index: usize, target: Point) -> Point {
    let ObjectKind::Shape { shape, .. } = &object.kind else {
        return target;
    };
    let handles = vertices(object);
    let Ok(geometry) =
        board_ink::shape_geometry(*shape, Point::default(), Point { x: 100.0, y: 100.0 })
    else {
        return target;
    };
    let mut neighbors: Vec<_> = geometry
        .edges
        .iter()
        .filter_map(|&[a, b]| {
            if a == index {
                Some(b)
            } else if b == index {
                Some(a)
            } else {
                None
            }
        })
        .collect();
    neighbors.sort_unstable();
    neighbors.dedup();
    let mut best = target;
    let mut distance = f64::INFINITY;
    for neighbor in neighbors {
        let Some(&anchor) = handles.get(neighbor) else {
            continue;
        };
        let Ok(candidate) = board_ink::snap_angle(anchor, target, 45.0) else {
            continue;
        };
        let angle = (f64::from(target.y) - f64::from(anchor.y))
            .atan2(f64::from(target.x) - f64::from(anchor.x));
        let snapped = (f64::from(candidate.y) - f64::from(anchor.y))
            .atan2(f64::from(candidate.x) - f64::from(anchor.x));
        let difference = (snapped - angle + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
            - std::f64::consts::PI;
        let displacement = (f64::from(candidate.x) - f64::from(target.x))
            .hypot(f64::from(candidate.y) - f64::from(target.y));
        // 仅真实邻边、6 度内；最小位移优先，相同位移按邻点索引。
        if difference.abs() <= 6.0_f64.to_radians() && displacement < distance {
            best = candidate;
            distance = displacement;
        }
    }
    best
}

pub struct VertexEdit {
    pub object: BoardObject,
    pub connection: Option<board_core::Connection>,
    pub hint: &'static str,
}

impl VertexEdit {
    pub fn apply(&self, document: &mut board_core::Document) -> board_core::Result<()> {
        let page = document.current_page().id.clone();
        document.apply(
            &page,
            document.revision,
            &[Operation::Update {
                object: self.object.clone(),
            }],
        )?;
        if let Some(connection) = &self.connection {
            document.connect(&page, document.revision, connection.clone())?;
        }
        Ok(())
    }
}

pub fn polygon_vertex_edit(
    document: &board_core::Document,
    original: &BoardObject,
    index: usize,
    query: Point,
) -> board_core::Result<VertexEdit> {
    let ObjectKind::Shape { shape, .. } = original.kind else {
        return Err(board_core::Error::InvalidDocument("不是多边形顶点".into()));
    };
    let handles = vertices(original);
    if shape == ShapeKind::Line || index >= handles.len() {
        return Err(board_core::Error::InvalidDocument(
            "不是多边形真实顶点".into(),
        ));
    }
    let mut edit = VertexEdit {
        object: original.clone(),
        connection: None,
        hint: "",
    };
    if handles[index] == query {
        return Ok(edit);
    }
    let mut rejected = None;
    for hit in crate::features::connection_hits(document.current_page(), query, Some(&original.id))
    {
        let mut candidate = VertexEdit {
            object: original.clone(),
            connection: None,
            hint: "仅几何吸附：当前连接模型不支持形状顶点跟随目标边或顶点",
        };
        move_vertex(&mut candidate.object, index, hit.point);
        if candidate.object == *original {
            return Ok(edit);
        }
        if matches!(
            hit.object.kind,
            ObjectKind::Shape {
                shape: ShapeKind::Line,
                ..
            }
        ) {
            if let board_core::Anchor::Vertex { index: endpoint } = hit.anchor {
                if document
                    .connections
                    .iter()
                    .any(|c| c.line_id == hit.object.id && c.line_endpoint == endpoint)
                {
                    rejected.get_or_insert_with(|| {
                        board_core::Error::InvalidDocument(
                            "线端点已有绑定，不会替换；本次拖动已取消".into(),
                        )
                    });
                    continue;
                }
                candidate.connection = Some(board_core::Connection {
                    id: new_id(),
                    page_id: document.current_page().id.clone(),
                    line_id: hit.object.id,
                    line_endpoint: endpoint,
                    target_id: original.id.clone(),
                    target: board_core::Anchor::Vertex { index },
                });
                candidate.hint = "持久端点连接：松手后线端点跟随形状顶点";
            } else {
                candidate.hint = "仅几何吸附：形状顶点不能持久绑定线段内部";
            }
        }
        if rejected.is_some() && candidate.connection.is_none() {
            continue;
        }
        // 使用真实 core 传播及全连通分量/环校验；失败候选不修改真实文档。
        match candidate.apply(&mut document.clone()) {
            Ok(()) => return Ok(candidate),
            Err(error) => {
                rejected.get_or_insert(error);
            }
        }
    }
    if let Some(error) = rejected {
        return Err(error);
    }
    move_vertex(
        &mut edit.object,
        index,
        snap_vertex_angle(original, index, query),
    );
    Ok(edit)
}

pub fn commit_line(
    document: &mut board_core::Document,
    history: &mut board_core::History,
    object: BoardObject,
    adding: bool,
    endpoints: &[usize],
) -> board_core::Result<()> {
    history.edit(document, |d| {
        let page = d.current_page().id.clone();
        let old: Vec<_> = d
            .connections
            .iter()
            .filter(|c| c.line_id == object.id && endpoints.contains(&c.line_endpoint))
            .map(|c| c.id.clone())
            .collect();
        for id in old {
            d.disconnect(&page, d.revision, &id)?;
        }
        let operation = if adding {
            Operation::Add {
                object: object.clone(),
            }
        } else {
            Operation::Update {
                object: object.clone(),
            }
        };
        d.apply(&page, d.revision, &[operation])?;
        if let ObjectKind::Shape {
            shape: ShapeKind::Line,
            points,
            ..
        } = &object.kind
        {
            for &endpoint in endpoints {
                if let Some(hit) = crate::features::connection_hit(
                    d.current_page(),
                    points[endpoint],
                    Some(&object.id),
                ) {
                    // materialize bbox 与 connect 同一事务；维度/环/连通分量约束交给 core。
                    d.apply(
                        &page,
                        d.revision,
                        &[Operation::Update {
                            object: hit.object.clone(),
                        }],
                    )?;
                    d.connect(
                        &page,
                        d.revision,
                        board_core::Connection {
                            id: new_id(),
                            page_id: page.clone(),
                            line_id: object.id.clone(),
                            line_endpoint: endpoint,
                            target_id: hit.object.id,
                            target: hit.anchor,
                        },
                    )?;
                }
            }
        }
        Ok(())
    })
}

pub fn disconnect_line(
    document: &mut board_core::Document,
    history: &mut board_core::History,
    line: &str,
) -> board_core::Result<()> {
    history.edit(document, |d| {
        let page = d.current_page().id.clone();
        let ids: Vec<_> = d
            .connections
            .iter()
            .filter(|c| c.line_id == line)
            .map(|c| c.id.clone())
            .collect();
        for id in ids {
            d.disconnect(&page, d.revision, &id)?;
        }
        Ok(())
    })
}

pub fn scale_plot(object: &mut BoardObject, factor: f64) {
    match &mut object.kind {
        ObjectKind::FunctionPlot {
            x_min,
            x_max,
            y_min,
            y_max,
            ..
        } => {
            let cx = (*x_min + *x_max) / 2.0;
            let cy = (*y_min + *y_max) / 2.0;
            let rx = ((*x_max - *x_min) / 2.0 * factor).clamp(0.0001, 1e6);
            let ry = ((*y_max - *y_min) / 2.0 * factor).clamp(0.0001, 1e6);
            *x_min = cx - rx;
            *x_max = cx + rx;
            *y_min = cy - ry;
            *y_max = cy + ry;
        }
        ObjectKind::CoordinateSystem { scale, .. } => {
            *scale = (*scale / factor as f32).clamp(5.0, 500.0);
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "editing_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "editing_resize_tests.rs"]
mod resize_tests;
