use crate::*;
use std::collections::{HashMap, HashSet, VecDeque};

/// Whole-document budgets, independent of the 500-page navigation limit.
pub const MAX_DOCUMENT_OBJECTS: usize = 100_000;
pub const MAX_DOCUMENT_POINTS: usize = 1_000_000;
pub const MAX_DOCUMENT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_JSON_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_CONNECTIONS: usize = 100_000;
pub const MAX_OPERATIONS: usize = 10_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Anchor {
    Vertex { index: usize },
    Edge { start: usize, end: usize, t: f32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub id: String,
    pub page_id: String,
    pub line_id: String,
    pub line_endpoint: usize,
    pub target_id: String,
    pub target: Anchor,
}

fn invalid(reason: &str) -> Error {
    Error::InvalidDocument(format!("连接无效：{reason}"))
}

#[derive(Default)]
pub(crate) struct Budget {
    objects: usize,
    points: usize,
    bytes: usize,
}
impl Budget {
    pub(crate) fn bytes(&mut self, bytes: usize) -> Result<()> {
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > MAX_DOCUMENT_BYTES {
            return Err(Error::InvalidDocument("文档内存预算超过 32 MiB".into()));
        }
        Ok(())
    }
    pub(crate) fn object(&mut self, object: &BoardObject) -> Result<()> {
        self.objects += 1;
        if self.objects > MAX_DOCUMENT_OBJECTS {
            return Err(Error::InvalidDocument("对象数量超过 100000".into()));
        }
        self.bytes(std::mem::size_of::<BoardObject>() + object.id.len())?;
        let (points, size) = match &object.kind {
            ObjectKind::Stroke { points, .. } => (points.len(), std::mem::size_of::<StrokePoint>()),
            ObjectKind::Shape { points, .. } => (points.len(), std::mem::size_of::<Point>()),
            ObjectKind::Handwritten {
                text,
                layout,
                strokes,
                ..
            } => {
                if text.len() > MAX_HANDWRITING_TEXT_BYTES
                    || strokes.is_empty()
                    || strokes.len() > MAX_HANDWRITING_STROKES
                {
                    return Err(Error::InvalidDocument("手写答案文字或笔画数超限".into()));
                }
                let points = strokes.iter().fold(0usize, |total, stroke| {
                    total.saturating_add(stroke.points.len())
                });
                if points > MAX_HANDWRITING_POINTS {
                    return Err(Error::InvalidDocument("手写答案点数超过 32768".into()));
                }
                self.bytes(text.len())?;
                self.bytes(
                    strokes
                        .len()
                        .saturating_mul(std::mem::size_of::<HandwritingStroke>()),
                )?;
                if let Some(layout) = layout {
                    let (nodes, bytes) = layout.measure()?;
                    self.bytes(nodes.saturating_mul(std::mem::size_of::<MathLayout>()))?;
                    self.bytes(bytes)?;
                }
                (points, std::mem::size_of::<StrokePoint>())
            }
            ObjectKind::Text { text, .. } => {
                self.bytes(text.len())?;
                (0, 0)
            }
            ObjectKind::Math { layout, .. } => {
                let (nodes, bytes) = layout.measure()?;
                self.bytes(nodes.saturating_mul(std::mem::size_of::<MathLayout>()))?;
                self.bytes(bytes)?;
                (0, 0)
            }
            ObjectKind::Image { asset_ref, .. } => {
                self.bytes(asset_ref.len())?;
                (0, 0)
            }
            ObjectKind::FunctionPlot { expressions, .. } => {
                self.bytes(
                    expressions
                        .len()
                        .saturating_mul(std::mem::size_of::<String>()),
                )?;
                for expression in expressions {
                    self.bytes(expression.len())?;
                }
                (0, 0)
            }
            ObjectKind::CoordinateSystem { .. } => (0, 0),
        };
        self.points = self.points.saturating_add(points);
        if self.points > MAX_DOCUMENT_POINTS {
            return Err(Error::InvalidDocument("点数超过 1000000".into()));
        }
        self.bytes(points.saturating_mul(size))
    }
}

pub(crate) fn check_budget(document: &Document) -> Result<()> {
    measure_document(document).map(|_| ())
}

pub(crate) fn document_bytes(document: &Document) -> usize {
    measure_document(document).expect("history snapshots have been validated")
}

fn measure_document(document: &Document) -> Result<usize> {
    if document.connections.len() > MAX_CONNECTIONS {
        return Err(invalid("连接数量超过 100000"));
    }
    let mut budget = Budget::default();
    budget.bytes(document.id.len())?;
    for page in &document.pages {
        budget.bytes(std::mem::size_of::<Page>() + page.id.len())?;
        for object in &page.objects {
            budget.object(object)?;
        }
    }
    for connection in &document.connections {
        budget.bytes(std::mem::size_of::<Connection>())?;
        for value in [
            &connection.id,
            &connection.page_id,
            &connection.line_id,
            &connection.target_id,
        ] {
            budget.bytes(value.len())?;
        }
    }
    Ok(budget.bytes)
}

fn vertices(object: &BoardObject) -> Result<&[Point]> {
    match &object.kind {
        ObjectKind::Shape { shape, points, .. }
            if (*shape == ShapeKind::Line && points.len() == 2)
                || (*shape != ShapeKind::Line && points.len() >= 3) =>
        {
            Ok(points)
        }
        _ => Err(invalid(
            "目标必须为显式顶点形状；非线形状的双点 bbox 必须先展开",
        )),
    }
}

fn anchor_point(object: &BoardObject, anchor: Anchor) -> Result<Point> {
    let points = vertices(object)?;
    let get = |index: usize| {
        points
            .get(index)
            .copied()
            .ok_or_else(|| invalid("锚点索引越界"))
    };
    let point = match anchor {
        Anchor::Vertex { index } => get(index)?,
        Anchor::Edge { start, end, t } => {
            if start == end || !t.is_finite() || !(0.0..=1.0).contains(&t) {
                return Err(invalid("线段参数无效"));
            }
            let a = get(start)?;
            let b = get(end)?;
            // f64 avoids overflow for finite, opposite f32 extremes.
            let interpolate = |a: f32, b: f32| {
                ((1.0 - f64::from(t)) * f64::from(a) + f64::from(t) * f64::from(b)) as f32
            };
            Point {
                x: interpolate(a.x, b.x),
                y: interpolate(a.y, b.y),
            }
        }
    };
    if !finite_point(&point) {
        return Err(invalid("锚点坐标不是有限数"));
    }
    Ok(point)
}

fn dimension(object: &BoardObject) -> u8 {
    match &object.kind {
        ObjectKind::Shape {
            shape: ShapeKind::Line,
            ..
        } => 0,
        ObjectKind::Shape {
            shape:
                ShapeKind::Cube
                | ShapeKind::Cuboid
                | ShapeKind::Cylinder
                | ShapeKind::Cone
                | ShapeKind::Sphere,
            ..
        } => 2,
        _ => 1,
    }
}

struct Link {
    connection: usize,
    page: usize,
    line: usize,
    target: usize,
}

impl Document {
    // O(objects + connections), iterative even for deep chains. Returns dependency order.
    fn connection_plan(&self, check_positions: bool) -> Result<Vec<Link>> {
        if self.connections.is_empty() {
            return Ok(Vec::new());
        }
        let mut objects = HashMap::new();
        let mut locations = Vec::new();
        for (p, page) in self.pages.iter().enumerate() {
            for (o, object) in page.objects.iter().enumerate() {
                objects.insert(object.id.as_str(), locations.len());
                locations.push((p, o));
            }
        }
        let mut ids = HashSet::new();
        let mut endpoints = HashSet::new();
        let mut outgoing = vec![Vec::new(); locations.len()];
        let mut neighbors = vec![Vec::new(); locations.len()];
        let mut incoming = vec![0usize; locations.len()];
        let mut links = Vec::with_capacity(self.connections.len());
        for (index, connection) in self.connections.iter().enumerate() {
            if connection.id.trim().is_empty() || !ids.insert(&connection.id) {
                return Err(invalid("连接 ID 为空或重复"));
            }
            if connection.line_id == connection.target_id {
                return Err(invalid("禁止自连接"));
            }
            if connection.line_endpoint > 1
                || !endpoints.insert((&connection.line_id, connection.line_endpoint))
            {
                return Err(invalid("线端点越界或已被连接"));
            }
            let line = *objects
                .get(connection.line_id.as_str())
                .ok_or_else(|| Error::ObjectNotFound(connection.line_id.clone()))?;
            let target = *objects
                .get(connection.target_id.as_str())
                .ok_or_else(|| Error::ObjectNotFound(connection.target_id.clone()))?;
            let (p, l) = locations[line];
            let (tp, t) = locations[target];
            if p != tp || self.pages[p].id != connection.page_id {
                return Err(invalid("连接对象必须位于指定的同一页"));
            }
            let ObjectKind::Shape {
                shape: ShapeKind::Line,
                points,
                ..
            } = &self.pages[p].objects[l].kind
            else {
                return Err(invalid("连接源必须是 Line"));
            };
            if points.len() != 2 {
                return Err(invalid("Line 必须恰好有两个点"));
            }
            let point = anchor_point(&self.pages[p].objects[t], connection.target)?;
            if check_positions && points[connection.line_endpoint] != point {
                return Err(invalid("端点位置与锚点不一致"));
            }
            outgoing[target].push((line, index));
            neighbors[line].push(target);
            neighbors[target].push(line);
            incoming[line] += 1;
            links.push(Some(Link {
                connection: index,
                page: p,
                line: l,
                target: t,
            }));
        }
        let mut visited = vec![false; locations.len()];
        for root in 0..locations.len() {
            if visited[root] || neighbors[root].is_empty() {
                continue;
            }
            let mut stack = vec![root];
            visited[root] = true;
            let mut dimensions = 0;
            while let Some(node) = stack.pop() {
                let (p, o) = locations[node];
                dimensions |= dimension(&self.pages[p].objects[o]);
                if dimensions == 3 {
                    return Err(invalid("同一连通分量不能混接 2D 与 3D"));
                }
                for &next in &neighbors[node] {
                    if !visited[next] {
                        visited[next] = true;
                        stack.push(next);
                    }
                }
            }
        }
        let mut queue: VecDeque<_> = incoming
            .iter()
            .enumerate()
            .filter_map(|(i, &n)| (n == 0).then_some(i))
            .collect();
        let mut plan = Vec::with_capacity(links.len());
        while let Some(node) = queue.pop_front() {
            for &(next, link) in &outgoing[node] {
                plan.push(links[link].take().expect("each connection has one target"));
                incoming[next] -= 1;
                if incoming[next] == 0 {
                    queue.push_back(next);
                }
            }
        }
        if plan.len() != self.connections.len() {
            return Err(invalid("连接包含依赖环"));
        }
        Ok(plan)
    }

    pub(crate) fn validate_connections(&self) -> Result<()> {
        self.connection_plan(true).map(|_| ())
    }

    pub(crate) fn propagate_connections(&mut self) -> Result<()> {
        self.validate_content()?;
        for link in self.connection_plan(false)? {
            let connection = &self.connections[link.connection];
            let point = anchor_point(
                &self.pages[link.page].objects[link.target],
                connection.target,
            )?;
            if let ObjectKind::Shape { points, .. } =
                &mut self.pages[link.page].objects[link.line].kind
            {
                points[connection.line_endpoint] = point;
            }
        }
        Ok(())
    }

    /// Adds a persistent anchor and snaps its endpoint, atomically advancing revision once.
    pub fn connect(
        &mut self,
        page_id: &str,
        expected_revision: u64,
        connection: Connection,
    ) -> Result<()> {
        self.check_revision(expected_revision)?;
        self.validate()?;
        if !self.pages.iter().any(|p| p.id == page_id) {
            return Err(Error::PageNotFound(page_id.into()));
        }
        if connection.page_id != page_id {
            return Err(invalid("连接 page_id 与请求不一致"));
        }
        let mut candidate = self.clone();
        candidate.connections.push(connection);
        candidate.propagate_connections()?;
        candidate.revision = self.next_revision()?;
        *self = candidate;
        Ok(())
    }

    /// Removes a connection without moving the line; unknown IDs are rejected.
    pub fn disconnect(
        &mut self,
        page_id: &str,
        expected_revision: u64,
        connection_id: &str,
    ) -> Result<()> {
        self.check_revision(expected_revision)?;
        self.validate()?;
        if !self.pages.iter().any(|p| p.id == page_id) {
            return Err(Error::PageNotFound(page_id.into()));
        }
        let index = self
            .connections
            .iter()
            .position(|c| c.id == connection_id && c.page_id == page_id)
            .ok_or_else(|| invalid("连接不存在于指定页面"))?;
        let revision = self.next_revision()?;
        self.connections.remove(index);
        self.revision = revision;
        Ok(())
    }
}
