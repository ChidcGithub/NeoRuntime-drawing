//! 无窗口共享渲染。页面坐标直接对应 egui 逻辑点或导出像素，左上为原点，向下为正。
//! 界面字体由应用安装到 egui；PNG 字体和图片通过 RenderResources 显式注入。

mod handwriting_font;
mod resources;
use ab_glyph::{Font, ScaleFont};
pub use handwriting_font::HandwritingFont;
pub use resources::{MAX_FONT_BYTES, MAX_RESOURCE_BYTES, MAX_RESOURCE_PIXELS, RenderResources};

use board_core::{BoardObject, Color, MathLayout, ObjectKind, Page, Point, Style};
use egui::{Color32, Pos2, Rect, Vec2};
use std::fmt::{self, Write as _};

pub const MAX_EXPORT_DIMENSION: u32 = 8192;
pub const MAX_EXPORT_PIXELS: u64 = 16_777_216;
const MAX_OBJECTS: usize = board_core::MAX_DOCUMENT_OBJECTS;
const MAX_PRIMITIVES: usize = 2_000_000;
const MAX_INPUT_POINTS: usize = board_core::MAX_DOCUMENT_POINTS;
const MAX_TEXT_BYTES: usize = board_core::MAX_DOCUMENT_BYTES;
const MAX_SVG_BYTES: usize = 32 * 1024 * 1024;
const MAX_COORD: f32 = 1_000_000.0;
const AXIS_COLOR: Color = Color {
    r: 128,
    g: 146,
    b: 154,
    a: 255,
};

#[derive(Debug)]
pub enum RenderError {
    InvalidObject(String),
    InvalidDimensions,
    ResourceLimit(&'static str),
    Geometry(board_ink::InkError),
    Math(board_math::MathError),
    MissingResource(String),
    UnsupportedText,
    MissingFont,
    InvalidFont,
    MissingGlyph(char),
    Decoding(String),
    Encoding(String),
}
impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidObject(s) => write!(f, "无效绘画对象：{s}"),
            Self::InvalidDimensions => {
                write!(f, "尺寸须为 1..8192，且总像素不超过 {MAX_EXPORT_PIXELS}")
            }
            Self::ResourceLimit(s) => write!(f, "渲染资源超限：{s}"),
            Self::Geometry(e) => write!(f, "几何生成失败：{e:?}"),
            Self::Math(e) => e.fmt(f),
            Self::MissingResource(s) => write!(f, "图片资源未提供，无法导出：{s}"),
            Self::UnsupportedText | Self::MissingFont => {
                write!(f, "PNG 文字渲染需要显式注入 TTF/TTC 字体")
            }
            Self::InvalidFont => write!(f, "无效 TTF/TTC 字体或字体索引"),
            Self::MissingGlyph(c) => write!(f, "注入字体缺少字符 U+{:04X}（{c}）", *c as u32),
            Self::Decoding(s) => write!(f, "PNG 解码失败：{s}"),
            Self::Encoding(s) => write!(f, "PNG 编码失败：{s}"),
        }
    }
}
impl std::error::Error for RenderError {}
impl From<board_ink::InkError> for RenderError {
    fn from(e: board_ink::InkError) -> Self {
        Self::Geometry(e)
    }
}
impl From<board_math::MathError> for RenderError {
    fn from(e: board_math::MathError) -> Self {
        Self::Math(e)
    }
}
pub type Result<T> = std::result::Result<T, RenderError>;

/// 仅解析调用方已加载的纹理；asset_ref 是不透明资源引用，不是路径或 URL。
pub trait ImageResources {
    fn texture_id(&self, asset_ref: &str) -> Option<egui::TextureId>;
}
impl<F: Fn(&str) -> Option<egui::TextureId>> ImageResources for F {
    fn texture_id(&self, asset_ref: &str) -> Option<egui::TextureId> {
        self(asset_ref)
    }
}
struct NoResources;
impl ImageResources for NoResources {
    fn texture_id(&self, _: &str) -> Option<egui::TextureId> {
        None
    }
}

#[derive(Clone, Debug)]
enum Primitive {
    Stroke(std::sync::Arc<StrokeGeometry>),
    Line(Pos2, Pos2, f32, Color),
    Disk(Pos2, f32, Color),
    Text(Pos2, String, f32, Color),
    Image(Rect, String),
    Clip(Rect),
    EndClip,
}
#[derive(Clone, Debug)]
struct StrokeGeometry {
    points: Vec<Pos2>,
    widths: Vec<f32>,
    color: Color,
    bounds: Rect,
}
impl StrokeGeometry {
    fn normal(&self, i: usize) -> Vec2 {
        let last = self.points.len() - 1;
        let before = if i > 0 {
            (self.points[i] - self.points[i - 1]).normalized()
        } else {
            (self.points[1] - self.points[0]).normalized()
        };
        let after = if i < last {
            (self.points[i + 1] - self.points[i]).normalized()
        } else {
            before
        };
        let tangent = (before + after).normalized();
        if tangent.length_sq() < 0.5 {
            return before.rot90();
        }
        // Bounded joins: no unbounded miters at reversals or sharp corners.
        tangent.rot90() / tangent.dot(before).max(std::f32::consts::FRAC_1_SQRT_2)
    }

    fn outline(&self) -> Vec<Pos2> {
        let mut outline = Vec::with_capacity(self.points.len() * 2 + 16);
        for i in 0..self.points.len() {
            outline.push(self.points[i] + self.normal(i) * (self.widths[i] / 2.0));
        }
        let last = self.points.len() - 1;
        for i in [last, 0] {
            let normal = self.normal(i) * if i == last { 1.0 } else { -1.0 };
            let angle = normal.y.atan2(normal.x);
            for step in 1..8 {
                outline.push(
                    self.points[i]
                        + Vec2::angled(angle + step as f32 * std::f32::consts::PI / 8.0)
                            * (self.widths[i] / 2.0),
                );
            }
            if i == last {
                for j in (0..self.points.len()).rev() {
                    outline.push(self.points[j] - self.normal(j) * (self.widths[j] / 2.0));
                }
            }
        }
        outline
    }

    fn display_indices(&self, pixel_scale: f32) -> Vec<usize> {
        let n = self.points.len();
        if n <= 4 || !pixel_scale.is_finite() || pixel_scale <= 0.0 {
            return (0..n).collect();
        }
        let mut keep = vec![false; n];
        // Preserve cap tangents, extrema, reversals, sharp corners and width discontinuities.
        for i in [0, 1, n - 2, n - 1] {
            keep[i] = true;
        }
        for (i, triple) in self.points.windows(3).enumerate() {
            let a = triple[1] - triple[0];
            let b = triple[2] - triple[1];
            let widths = &self.widths[i..i + 3];
            let dw0 = widths[1] - widths[0];
            let dw1 = widths[2] - widths[1];
            if a.x * b.x < 0.0
                || a.y * b.y < 0.0
                || a.dot(b) <= std::f32::consts::FRAC_1_SQRT_2 * a.length() * b.length()
                || dw0 * dw1 < 0.0
            {
                keep[i + 1] = true;
            }
            for j in 0..2 {
                if (widths[j + 1] - widths[j]).abs() > widths[j + 1].max(widths[j]) * 0.1 {
                    keep[i + j] = true;
                    keep[i + j + 1] = true;
                }
            }
        }
        let anchors: Vec<_> = (0..n).filter(|&i| keep[i]).collect();
        let mut pending: Vec<_> = anchors.windows(2).map(|w| (w[0], w[1])).collect();
        // Iterative RDP, at most 32 input scans in total. Unchecked ranges retain all samples.
        let mut work = n.saturating_mul(32);
        let scale = f64::from(pixel_scale);
        while let Some((start, end)) = pending.pop() {
            let count = end - start - 1;
            if count == 0 {
                continue;
            }
            if count > work {
                keep[start..=end].fill(true);
                continue;
            }
            work -= count;
            let a = self.points[start];
            let b = self.points[end];
            let dx = f64::from(b.x) - f64::from(a.x);
            let dy = f64::from(b.y) - f64::from(a.y);
            let length_sq = dx * dx + dy * dy;
            let mut worst = 1.0;
            let mut split = None;
            let mut previous_t = 0.0;
            for i in start + 1..end {
                let px = f64::from(self.points[i].x) - f64::from(a.x);
                let py = f64::from(self.points[i].y) - f64::from(a.y);
                let t = if length_sq > 0.0 {
                    ((px * dx + py * dy) / length_sq).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                // Distance alone can erase a traversal that doubles back on the fitted segment.
                if t < previous_t {
                    split = Some(i - 1);
                    break;
                }
                previous_t = t;
                let distance = (px - t * dx).hypot(py - t * dy) * scale;
                let width = f64::from(self.widths[start])
                    + t * (f64::from(self.widths[end]) - f64::from(self.widths[start]));
                let width_error = (f64::from(self.widths[i]) - width).abs() * scale;
                let error = (distance / 0.2499).max(width_error / 0.0999);
                if error > worst {
                    worst = error;
                    split = Some(i);
                }
            }
            if let Some(i) = split {
                keep[i] = true;
                pending.push((start, i));
                pending.push((i, end));
            }
        }
        (0..n).filter(|&i| keep[i]).collect()
    }

    fn mesh(
        &self,
        transform: egui::emath::TSTransform,
        pixels_per_point: f32,
        feather: f32,
        mesh: &mut egui::Mesh,
    ) {
        let indices = self.display_indices(pixels_per_point * transform.scaling.abs());
        if indices.len() == self.points.len() {
            self.mesh_full(transform, feather, mesh);
        } else {
            let display = Self {
                points: indices.iter().map(|&i| self.points[i]).collect(),
                widths: indices.iter().map(|&i| self.widths[i]).collect(),
                color: self.color,
                bounds: self.bounds,
            };
            display.mesh_full(transform, feather, mesh);
        }
    }

    fn mesh_full(&self, transform: egui::emath::TSTransform, feather: f32, mesh: &mut egui::Mesh) {
        let base = mesh.vertices.len() as u32;
        let color = rgba(self.color);
        let mut vertex = |p, c| mesh.colored_vertex(p, c);
        for i in 0..self.points.len() {
            let center = transform * self.points[i];
            let normal = self.normal(i);
            let radius = self.widths[i] * transform.scaling / 2.0;
            let inner = (radius - feather / 2.0).max(0.0);
            let outer = radius + feather / 2.0;
            let color = if feather > 0.0 {
                color.linear_multiply((2.0 * radius / feather).min(1.0))
            } else {
                color
            };
            vertex(center + normal * outer, Color32::TRANSPARENT);
            vertex(center + normal * inner, color);
            vertex(center - normal * inner, color);
            vertex(center - normal * outer, Color32::TRANSPARENT);
        }
        for i in 0..self.points.len() - 1 {
            let a = base + i as u32 * 4;
            for j in 0..3 {
                mesh.add_triangle(a + j, a + j + 1, a + j + 4);
                mesh.add_triangle(a + j + 1, a + j + 5, a + j + 4);
            }
        }
        for (i, sign) in [(0, -1.0), (self.points.len() - 1, 1.0)] {
            let center = transform * self.points[i];
            let radius = self.widths[i] * transform.scaling / 2.0;
            let inner = (radius - feather / 2.0).max(0.0);
            let outer = radius + feather / 2.0;
            let color = if feather > 0.0 {
                color.linear_multiply((2.0 * radius / feather).min(1.0))
            } else {
                color
            };
            let center_index = mesh.vertices.len() as u32;
            mesh.colored_vertex(center, color);
            let normal = self.normal(i);
            let angle = normal.y.atan2(normal.x);
            for step in 0..=8 {
                let direction =
                    Vec2::angled(angle + sign * step as f32 * std::f32::consts::PI / 8.0);
                mesh.colored_vertex(center + direction * inner, color);
                mesh.colored_vertex(center + direction * outer, Color32::TRANSPARENT);
                if step > 0 {
                    let a = center_index + 1 + (step - 1) * 2;
                    mesh.add_triangle(center_index, a, a + 2);
                    mesh.add_triangle(a, a + 1, a + 2);
                    mesh.add_triangle(a + 1, a + 3, a + 2);
                }
            }
        }
    }
}

#[derive(Default)]
struct Scene {
    items: Vec<Primitive>,
    merge_start: usize,
    stroke_points: usize,
}
impl Scene {
    fn stroke(&mut self, points: Vec<Pos2>, widths: Vec<f32>, color: Color) -> Result<()> {
        if points.is_empty() {
            return Ok(());
        }
        // Charge generated samples before any deduplication or display simplification.
        self.stroke_points = self.stroke_points.saturating_add(points.len() + 10);
        if self.stroke_points > MAX_INPUT_POINTS * 2 {
            return Err(RenderError::ResourceLimit("笔迹细分点数超限"));
        }
        let mut centers: Vec<Pos2> = Vec::with_capacity(points.len());
        let mut sizes: Vec<f32> = Vec::with_capacity(points.len());
        for (p, width) in points.into_iter().zip(widths) {
            if centers.last() == Some(&p) {
                let last = sizes.last_mut().unwrap();
                *last = last.max(width);
                continue;
            }
            centers.push(p);
            sizes.push(width);
        }
        if centers.len() == 1 {
            return self.push(Primitive::Disk(centers[0], sizes[0] / 2.0, color));
        }
        let bounds = Rect::from_points(&centers)
            .expand(sizes.iter().copied().fold(0.0, f32::max) * std::f32::consts::FRAC_1_SQRT_2);
        self.push(Primitive::Stroke(std::sync::Arc::new(StrokeGeometry {
            points: centers,
            widths: sizes,
            color,
            bounds,
        })))
    }

    fn push(&mut self, primitive: Primitive) -> Result<()> {
        if self.items.len() >= MAX_PRIMITIVES {
            return Err(RenderError::ResourceLimit("绘制图元过多"));
        }
        self.items.push(primitive);
        Ok(())
    }
    fn line(&mut self, a: Pos2, b: Pos2, width: f32, color: Color) -> Result<()> {
        if a == b {
            self.push(Primitive::Disk(a, width / 2.0, color))
        } else {
            if self.items.len() > self.merge_start
                && let Some(Primitive::Line(start, end, w, c)) = self.items.last_mut()
            {
                let u = *end - *start;
                let v = b - a;
                if *end == a
                    && *w == width
                    && *c == color
                    && u.x * v.y == u.y * v.x
                    && u.dot(v) > 0.0
                {
                    *end = b;
                    return Ok(());
                }
            }
            self.push(Primitive::Line(a, b, width, color))
        }
    }
    fn border(&mut self, rect: Rect, color: Color) -> Result<()> {
        let corners = [
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
            rect.left_top(),
        ];
        for pair in corners.windows(2) {
            self.line(pair[0], pair[1], 1.0, color)?;
        }
        Ok(())
    }
}
fn pos(p: Point) -> Pos2 {
    egui::pos2(p.x, p.y)
}
fn rgba(c: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}
fn size_rect(p: Point, w: f32, h: f32) -> Rect {
    Rect::from_min_size(pos(p), egui::vec2(w, h))
}
fn check_coord(v: f32) -> bool {
    v.is_finite() && v.abs() <= MAX_COORD
}

fn validate_object(object: &BoardObject) -> Result<()> {
    // 在逐点验证之前限制输入大小，避免恶意输入拖慢界面。
    let allowed = match &object.kind {
        ObjectKind::Stroke { points, .. } => points.len() <= MAX_INPUT_POINTS,
        ObjectKind::Shape { points, .. } => points.len() <= 192,
        ObjectKind::Text { text, .. } => text.len() <= MAX_TEXT_BYTES,
        ObjectKind::Image { asset_ref, .. } => asset_ref.len() <= 4096,
        ObjectKind::FunctionPlot { expressions, .. } => {
            expressions.len() <= 16
                && expressions
                    .iter()
                    .all(|s| s.len() <= board_math::MAX_INPUT_BYTES)
        }
        _ => true,
    };
    if !allowed {
        return Err(RenderError::ResourceLimit("对象输入过大"));
    }
    object
        .validate()
        .map_err(|e| RenderError::InvalidObject(e.to_string()))?;
    let point_ok = |p: Point| check_coord(p.x) && check_coord(p.y);
    let rect_ok = |p: Point, w: f32, h: f32| {
        point_ok(p)
            && check_coord(w)
            && check_coord(h)
            && check_coord(p.x + w)
            && check_coord(p.y + h)
    };
    let valid = match &object.kind {
        ObjectKind::Stroke { points, .. } => points
            .iter()
            .all(|p| check_coord(p.x) && check_coord(p.y) && p.time <= 1e12),
        // Core validates local and translated coordinates, time, pressure and styles.
        ObjectKind::Handwritten { .. } => true,
        ObjectKind::Shape { points, .. } => points.iter().copied().all(point_ok),
        ObjectKind::Text {
            position,
            size,
            text,
            ..
        } => point_ok(*position) && *size <= 4096.0 && text.chars().all(xml_char),
        ObjectKind::Math {
            position,
            layout,
            size,
            ..
        } => {
            let measured = MathBox::new(layout, *size)?;
            *size <= 4096.0 && rect_ok(*position, measured.width, measured.height)
        }
        ObjectKind::Image {
            position,
            width,
            height,
            ..
        } => rect_ok(*position, *width, *height),
        ObjectKind::CoordinateSystem { origin, scale } => rect_ok(
            Point {
                x: origin.x - 5.0 * scale,
                y: origin.y - 5.0 * scale,
            },
            10.0 * scale,
            10.0 * scale,
        ),
        ObjectKind::FunctionPlot {
            position,
            width,
            height,
            x_min,
            x_max,
            y_min,
            y_max,
            ..
        } => {
            rect_ok(*position, *width, *height)
                && [x_min, x_max, y_min, y_max].iter().all(|v| v.abs() <= 1e6)
                && (*x_max - *x_min).is_finite()
                && (*y_max - *y_min).is_finite()
        }
    };
    if valid {
        Ok(())
    } else {
        Err(RenderError::InvalidObject(
            "坐标、时间、字号或 XML 字符超出支持范围".into(),
        ))
    }
}
fn xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}
fn geometry(shape: board_core::ShapeKind, points: &[Point]) -> Result<board_ink::ShapeGeometry> {
    let mut g = board_ink::shape_geometry(shape, points[0], points[1])?;
    if points.len() > 2 {
        if points.len() != g.vertices.len() {
            return Err(RenderError::InvalidObject(
                "编辑后顶点数与几何拓扑不匹配".into(),
            ));
        }
        g.vertices.clone_from_slice(points);
    }
    Ok(g)
}

// 虚线相位沿折线累计，不能在每个重采样点重新开始。
fn styled_line(
    scene: &mut Scene,
    a: Pos2,
    b: Pos2,
    width: f32,
    style: Style,
    phase: &mut f64,
) -> Result<()> {
    if !style.dashed || a == b {
        return scene.line(a, b, width, style.color);
    }
    let length = f64::from(a.distance(b));
    let dash = f64::from((style.width * 3.0).max(4.0));
    let period = dash * 1.6;
    let mut cursor = 0.0;
    let mut iterations = 0;
    while cursor < length {
        iterations += 1;
        if iterations > MAX_PRIMITIVES {
            return Err(RenderError::ResourceLimit("虚线细分过多"));
        }
        let at = (*phase + cursor).rem_euclid(period);
        let visible = at < dash;
        let step = (if visible { dash - at } else { period - at })
            .max(1e-6)
            .min(length - cursor);
        if visible {
            scene.line(
                a.lerp(b, (cursor / length) as f32),
                a.lerp(b, ((cursor + step) / length) as f32),
                width,
                style.color,
            )?;
        }
        cursor += step;
    }
    *phase = (*phase + length).rem_euclid(period);
    Ok(())
}
fn stroke_widths(points: &[board_core::StrokePoint], style: &Style) -> Vec<f32> {
    // 与 ink 的压感/速度公式一致，但不受其交互采样点数上限约束。
    let mut time = points.first().map_or(0.0, |p| p.time);
    let mut speed = 0.0;
    points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let next_time = p.time.max(time);
            let dt = next_time - time;
            if i > 0 && dt > 1e-6 {
                speed = (f64::from(p.x) - f64::from(points[i - 1].x))
                    .hypot(f64::from(p.y) - f64::from(points[i - 1].y))
                    / dt;
            }
            time = next_time;
            let velocity = 0.25 + 0.75 / (1.0 + speed / 1200.0);
            let weight = f64::from(0.7_f32);
            let pressure = 1.0 - weight + weight * (0.2 + 0.8 * f64::from(p.pressure));
            (f64::from(style.width) * velocity * pressure).max(0.01) as f32
        })
        .collect()
}

// All backends and hit testing use the same font-independent layout metrics.
struct MathBox {
    width: f32,
    height: f32,
    baseline: f32,
    size: f32,
    text: Option<String>,
    children: Vec<(Vec2, MathBox)>,
    lines: Vec<(Pos2, Pos2)>,
}
impl MathBox {
    fn new(layout: &MathLayout, size: f32) -> Result<Self> {
        let mut result = Self {
            width: 0.0,
            height: size * 1.5,
            baseline: size,
            size,
            text: None,
            children: Vec::new(),
            lines: Vec::new(),
        };
        match layout {
            MathLayout::Text(text) => {
                if !text.chars().all(xml_char) {
                    return Err(RenderError::InvalidObject(
                        "数学文字包含无效 XML 字符".into(),
                    ));
                }
                let text = text
                    .replace("\r\n", "\n")
                    .replace('\r', "")
                    .replace('\t', "    ");
                result.width = text
                    .split('\n')
                    .map(|line| line.chars().count())
                    .max()
                    .unwrap_or(0) as f32
                    * size
                    * 1.25;
                result.height = text.split('\n').count() as f32 * size * 1.5;
                result.text = Some(text);
            }
            MathLayout::Row(children) => {
                let boxes = children
                    .iter()
                    .map(|child| Self::new(child, size))
                    .collect::<Result<Vec<_>>>()?;
                result.baseline = boxes
                    .iter()
                    .map(|child| child.baseline)
                    .fold(size, f32::max);
                let descent = boxes
                    .iter()
                    .map(|child| child.height - child.baseline)
                    .fold(size * 0.5, f32::max);
                result.height = result.baseline + descent;
                for child in boxes {
                    let offset = egui::vec2(result.width, result.baseline - child.baseline);
                    result.width += child.width;
                    result.children.push((offset, child));
                }
            }
            MathLayout::Fraction(numerator, denominator) => {
                let numerator = Self::new(numerator, size * 0.9)?;
                let denominator = Self::new(denominator, size * 0.9)?;
                let padding = size * 0.2;
                let gap = size * 0.15;
                let stroke = size * 0.06;
                result.width = numerator.width.max(denominator.width) + padding * 2.0;
                let bar_y = numerator.height + gap + stroke / 2.0;
                let denominator_y = bar_y + stroke / 2.0 + gap;
                result.height = denominator_y + denominator.height;
                result.baseline = bar_y + size * 0.3;
                result.lines.push((
                    egui::pos2(stroke / 2.0, bar_y),
                    egui::pos2(result.width - stroke / 2.0, bar_y),
                ));
                result.children.push((
                    egui::vec2((result.width - numerator.width) / 2.0, 0.0),
                    numerator,
                ));
                result.children.push((
                    egui::vec2((result.width - denominator.width) / 2.0, denominator_y),
                    denominator,
                ));
            }
            MathLayout::Radical(child) => {
                let child = Self::new(child, size)?;
                let padding = size * 0.18;
                let left = size * 0.65;
                let stroke = size * 0.06;
                result.width = left + child.width + padding;
                result.height = child.height + padding;
                result.baseline = child.baseline + padding;
                let points = [
                    egui::pos2(stroke / 2.0, result.height * 0.55),
                    egui::pos2(size * 0.18, result.height * 0.48),
                    egui::pos2(size * 0.35, result.height - stroke / 2.0),
                    egui::pos2(left - padding * 0.3, stroke / 2.0),
                    egui::pos2(result.width - stroke / 2.0, stroke / 2.0),
                ];
                result
                    .lines
                    .extend(points.windows(2).map(|pair| (pair[0], pair[1])));
                result.children.push((egui::vec2(left, padding), child));
            }
        }
        Ok(result)
    }
    fn append(&self, scene: &mut Scene, position: Pos2, color: Color) -> Result<()> {
        if let Some(text) = &self.text
            && !text.is_empty()
        {
            scene.push(Primitive::Text(position, text.clone(), self.size, color))?;
        }
        for (a, b) in &self.lines {
            scene.line(
                position + a.to_vec2(),
                position + b.to_vec2(),
                self.size * 0.06,
                color,
            )?;
        }
        for (offset, child) in &self.children {
            child.append(scene, position + *offset, color)?;
        }
        Ok(())
    }
    fn text_bytes(&self) -> usize {
        self.text.as_ref().map_or(0, String::len)
            + self
                .children
                .iter()
                .map(|(_, child)| child.text_bytes())
                .sum::<usize>()
    }
}

const PLOT_SAMPLE_STEPS: usize = 512;
const MAX_PLOT_SAMPLE_BYTES: usize = 16 * 1024 * 1024;
const MAX_PLOT_SAMPLE_ENTRIES: usize = 128;

#[derive(PartialEq)]
struct PlotSamplesKey {
    expressions: Vec<String>,
    bounds: [u64; 4],
    steps: usize,
}
impl PlotSamplesKey {
    fn bounds(bounds: board_math::Bounds2D) -> [u64; 4] {
        [bounds.x_min, bounds.x_max, bounds.y_min, bounds.y_max].map(f64::to_bits)
    }

    fn matches(&self, expressions: &[String], bounds: board_math::Bounds2D, steps: usize) -> bool {
        self.expressions == expressions
            && self.bounds == Self::bounds(bounds)
            && self.steps == steps
    }

    fn matches_object(&self, object: &BoardObject) -> bool {
        match &object.kind {
            ObjectKind::FunctionPlot {
                expressions,
                x_min,
                x_max,
                y_min,
                y_max,
                ..
            } => self.matches(
                expressions,
                board_math::Bounds2D {
                    x_min: *x_min,
                    x_max: *x_max,
                    y_min: *y_min,
                    y_max: *y_max,
                },
                PLOT_SAMPLE_STEPS,
            ),
            _ => false,
        }
    }
}

struct PlotSamplesEntry {
    key: PlotSamplesKey,
    curves: Vec<board_math::SampledCurve>,
    bytes: usize,
}

/// Mathematical samples only: screen transforms, clipping and DPI never enter the key.
#[derive(Default)]
struct PlotSamplesCache {
    entries: Vec<PlotSamplesEntry>,
    #[cfg(any(test, feature = "test-support"))]
    sample_calls: u64,
}
impl PlotSamplesCache {
    fn retain_page(&mut self, page: &Page) {
        self.entries.retain(|entry| {
            page.objects
                .iter()
                .any(|object| entry.key.matches_object(object))
        });
    }

    fn samples(
        &mut self,
        expressions: &[String],
        bounds: board_math::Bounds2D,
        steps: usize,
    ) -> Result<&[board_math::SampledCurve]> {
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.key.matches(expressions, bounds, steps))
        {
            return Ok(&self.entries[index].curves);
        }
        if self.entries.len() >= MAX_PLOT_SAMPLE_ENTRIES {
            return Err(RenderError::ResourceLimit("函数采样缓存项数超限"));
        }
        let used = self.entries.iter().map(|entry| entry.bytes).sum::<usize>();
        let mut bytes = std::mem::size_of::<PlotSamplesEntry>()
            + std::mem::size_of_val(expressions)
            + expressions.iter().map(String::len).sum::<usize>();
        let mut curves = Vec::with_capacity(expressions.len());
        bytes += curves.capacity() * std::mem::size_of::<board_math::SampledCurve>();
        for expression in expressions {
            if used + bytes > MAX_PLOT_SAMPLE_BYTES {
                return Err(RenderError::ResourceLimit("函数采样缓存内存超限"));
            }
            #[cfg(any(test, feature = "test-support"))]
            {
                self.sample_calls += 1;
            }
            let curve = board_math::sample_plot(expression, bounds, steps)?;
            bytes += curve.segments.capacity() * std::mem::size_of::<Vec<board_math::Point>>()
                + curve
                    .segments
                    .iter()
                    .map(|segment| segment.capacity() * std::mem::size_of::<board_math::Point>())
                    .sum::<usize>();
            if used + bytes > MAX_PLOT_SAMPLE_BYTES {
                return Err(RenderError::ResourceLimit("函数采样缓存内存超限"));
            }
            curves.push(curve);
        }
        self.entries.push(PlotSamplesEntry {
            key: PlotSamplesKey {
                expressions: expressions.to_vec(),
                bounds: PlotSamplesKey::bounds(bounds),
                steps,
            },
            curves,
            bytes,
        });
        Ok(&self.entries.last().unwrap().curves)
    }
}

fn append_stroke(
    scene: &mut Scene,
    points: &[board_core::StrokePoint],
    style: &Style,
    offset: Point,
) -> Result<()> {
    // Widths use the frozen local samples so moving an answer cannot change velocity.
    let widths = stroke_widths(points, style);
    let centers: Vec<_> = points.iter().map(|p| egui::pos2(p.x, p.y)).collect();
    // Split dashes locally too: translation must not change the frozen geometry.
    let emit = |scene: &mut Scene, mut centers: Vec<Pos2>, widths: Vec<f32>| {
        for center in &mut centers {
            *center += egui::vec2(offset.x, offset.y);
        }
        scene.stroke(centers, widths, style.color)
    };
    if !style.dashed || centers.len() == 1 {
        emit(scene, centers, widths)?;
    } else {
        let dash = f64::from((style.width * 3.0).max(4.0));
        let period = dash * 1.6;
        let mut phase = 0.0_f64;
        let mut run = Vec::new();
        let mut sizes = Vec::new();
        let mut steps = 0;
        for (i, pair) in centers.windows(2).enumerate() {
            let length = f64::from(pair[0].distance(pair[1]));
            let mut cursor = 0.0;
            while cursor < length {
                steps += 1;
                if steps > MAX_INPUT_POINTS * 2 {
                    return Err(RenderError::ResourceLimit("虚线细分过多"));
                }
                let visible = phase < dash;
                let step = (if visible {
                    dash - phase
                } else {
                    period - phase
                })
                .max(1e-6)
                .min(length - cursor);
                if visible {
                    for distance in [cursor, cursor + step] {
                        let t = (distance / length) as f32;
                        run.push(pair[0].lerp(pair[1], t));
                        sizes.push(widths[i] + (widths[i + 1] - widths[i]) * t);
                    }
                } else if !run.is_empty() {
                    emit(scene, std::mem::take(&mut run), std::mem::take(&mut sizes))?;
                }
                cursor += step;
                phase = (phase + step).rem_euclid(period);
            }
        }
        if centers.iter().all(|p| *p == centers[0]) {
            emit(scene, centers, widths)?;
        } else {
            emit(scene, run, sizes)?;
        }
    }
    Ok(())
}

fn append_object(
    scene: &mut Scene,
    object: &BoardObject,
    samples: &mut PlotSamplesCache,
) -> Result<()> {
    validate_object(object)?;
    scene.merge_start = scene.items.len();
    match &object.kind {
        ObjectKind::Stroke { points, style } => {
            // The input already carries ink's Hermite sampling; never smooth it a second time.
            append_stroke(scene, points, style, Point::default())?;
        }
        ObjectKind::Handwritten {
            position, strokes, ..
        } => {
            for stroke in strokes {
                append_stroke(scene, &stroke.points, &stroke.style, *position)?;
            }
        }
        ObjectKind::Shape {
            shape,
            points,
            style,
        } => {
            let g = geometry(*shape, points)?;
            let mut phase = 0.0;
            let mut previous = None;
            for [a, b] in g.edges {
                if previous != Some(a) {
                    phase = 0.0;
                }
                styled_line(
                    scene,
                    pos(g.vertices[a]),
                    pos(g.vertices[b]),
                    style.width,
                    *style,
                    &mut phase,
                )?;
                previous = Some(b);
            }
        }
        ObjectKind::Text {
            position,
            text,
            size,
            color,
        } => {
            if !text.is_empty() {
                let text = text
                    .replace("\r\n", "\n")
                    .replace('\r', "")
                    .replace('\t', "    ");
                scene.push(Primitive::Text(pos(*position), text, *size, *color))?;
            }
        }
        ObjectKind::Math {
            position,
            layout,
            size,
            color,
        } => {
            MathBox::new(layout, *size)?.append(scene, pos(*position), *color)?;
        }
        ObjectKind::Image {
            position,
            width,
            height,
            asset_ref,
        } => {
            scene.push(Primitive::Image(
                size_rect(*position, *width, *height),
                asset_ref.clone(),
            ))?;
        }
        ObjectKind::CoordinateSystem { origin, scale } => {
            let o = pos(*origin);
            let r = 5.0 * scale;
            scene.line(o - Vec2::X * r, o + Vec2::X * r, 1.5, AXIS_COLOR)?;
            scene.line(o - Vec2::Y * r, o + Vec2::Y * r, 1.5, AXIS_COLOR)?;
            for i in -5..=5 {
                if i == 0 {
                    continue;
                }
                let d = i as f32 * scale;
                scene.line(
                    o + egui::vec2(d, -3.0),
                    o + egui::vec2(d, 3.0),
                    1.0,
                    AXIS_COLOR,
                )?;
                scene.line(
                    o + egui::vec2(-3.0, d),
                    o + egui::vec2(3.0, d),
                    1.0,
                    AXIS_COLOR,
                )?;
            }
            for sign in [-1.0, 1.0] {
                scene.line(
                    o + egui::vec2(r, 0.0),
                    o + egui::vec2(r - 7.0, sign * 4.0),
                    1.5,
                    AXIS_COLOR,
                )?;
                scene.line(
                    o + egui::vec2(0.0, -r),
                    o + egui::vec2(sign * 4.0, -r + 7.0),
                    1.5,
                    AXIS_COLOR,
                )?;
            }
        }
        ObjectKind::FunctionPlot {
            position,
            width,
            height,
            expressions,
            x_min,
            x_max,
            y_min,
            y_max,
        } => {
            let rect = size_rect(*position, *width, *height);
            scene.border(rect, AXIS_COLOR)?;
            let map = |x: f64, y: f64| {
                egui::pos2(
                    position.x + ((x - x_min) / (x_max - x_min)) as f32 * width,
                    position.y + ((y_max - y) / (y_max - y_min)) as f32 * height,
                )
            };
            if *x_min <= 0.0 && *x_max >= 0.0 {
                scene.line(map(0.0, *y_min), map(0.0, *y_max), 1.0, AXIS_COLOR)?;
            }
            if *y_min <= 0.0 && *y_max >= 0.0 {
                scene.line(map(*x_min, 0.0), map(*x_max, 0.0), 1.0, AXIS_COLOR)?;
            }
            let colors = [
                Color {
                    r: 52,
                    g: 145,
                    b: 236,
                    a: 255,
                },
                Color {
                    r: 232,
                    g: 104,
                    b: 87,
                    a: 255,
                },
                Color {
                    r: 92,
                    g: 184,
                    b: 115,
                    a: 255,
                },
            ];
            scene.push(Primitive::Clip(rect))?;
            let bounds = board_math::Bounds2D {
                x_min: *x_min,
                x_max: *x_max,
                y_min: *y_min,
                y_max: *y_max,
            };
            for (i, curve) in samples
                .samples(expressions, bounds, PLOT_SAMPLE_STEPS)?
                .iter()
                .enumerate()
            {
                let color = colors[i % colors.len()];
                // 每个片段独立绘制，裁剪后的不连续片段也不能互相连接。
                for segment in &curve.segments {
                    if let [point] = segment.as_slice() {
                        if point.x >= *x_min
                            && point.x <= *x_max
                            && point.y >= *y_min
                            && point.y <= *y_max
                        {
                            scene.push(Primitive::Disk(map(point.x, point.y), 1.0, color))?;
                        }
                        continue;
                    }
                    let mut points = Vec::new();
                    for pair in segment.windows(2) {
                        // 先在 f64 数学空间裁剪两轴，避免极大函数值转换成 f32 无穷。
                        let clipped = clip_plot_segment(pair[0], pair[1], bounds)
                            .map(|(a, b)| (map(a.x, a.y), map(b.x, b.y)));
                        if !points.is_empty()
                            && clipped.is_none_or(|(a, _)| points.last() != Some(&a))
                        {
                            let widths = vec![2.0; points.len()];
                            scene.stroke(std::mem::take(&mut points), widths, color)?;
                        }
                        if let Some((a, b)) = clipped {
                            if points.is_empty() {
                                points.push(a);
                            }
                            points.push(b);
                        }
                    }
                    let widths = vec![2.0; points.len()];
                    scene.stroke(points, widths, color)?;
                }
            }
            scene.push(Primitive::EndClip)?;
        }
    }
    Ok(())
}
fn clip_plot_segment(
    a: board_math::Point,
    b: board_math::Point,
    bounds: board_math::Bounds2D,
) -> Option<(board_math::Point, board_math::Point)> {
    let swap = |p: board_math::Point| board_math::Point { x: p.y, y: p.x };
    let (a, b) = clip_y(a, b, bounds.y_min, bounds.y_max)?;
    let (a, b) = clip_y(swap(a), swap(b), bounds.x_min, bounds.x_max)?;
    Some((swap(a), swap(b)))
}

fn clip_y(
    a: board_math::Point,
    b: board_math::Point,
    lo: f64,
    hi: f64,
) -> Option<(board_math::Point, board_math::Point)> {
    if (a.y < lo && b.y < lo) || (a.y > hi && b.y > hi) {
        return None;
    }
    let clip = |p: board_math::Point, q: board_math::Point| {
        let y = p.y.clamp(lo, hi);
        if y == p.y {
            p
        } else {
            // 缩放后计算比例，规避两个极大异号数相减溢出。
            let scale = p.y.abs().max(q.y.abs()).max(1.0);
            let t = (y / scale - p.y / scale) / (q.y / scale - p.y / scale);
            board_math::Point {
                x: p.x + (q.x - p.x) * t,
                y,
            }
        }
    };
    Some((clip(a, b), clip(b, a)))
}
fn validate_page_budget(page: &Page) -> Result<()> {
    if page.objects.len() > MAX_OBJECTS {
        return Err(RenderError::ResourceLimit("页面对象过多"));
    }
    let mut points = 0usize;
    let mut text = 0usize;
    let mut expressions = 0usize;
    for object in &page.objects {
        match &object.kind {
            ObjectKind::Stroke { points: p, .. } => points = points.saturating_add(p.len()),
            ObjectKind::Shape { points: p, .. } => points = points.saturating_add(p.len()),
            ObjectKind::Text { text: t, .. } => text = text.saturating_add(t.len()),
            ObjectKind::Handwritten { strokes, .. } => {
                validate_object(object)?;
                for stroke in strokes {
                    points = points.saturating_add(stroke.points.len());
                }
            }
            ObjectKind::Math { layout, size, .. } => {
                validate_object(object)?;
                text = text.saturating_add(MathBox::new(layout, *size)?.text_bytes());
            }
            ObjectKind::FunctionPlot { expressions: e, .. } => {
                expressions = expressions.saturating_add(e.len())
            }
            _ => (),
        }
    }
    if points > MAX_INPUT_POINTS || text > MAX_TEXT_BYTES || expressions > 32 {
        return Err(RenderError::ResourceLimit("页面点数、文字或函数预算超限"));
    }
    Ok(())
}
fn page_scene(page: &Page) -> Result<Scene> {
    validate_page_budget(page)?;
    let mut scene = Scene::default();
    let mut samples = PlotSamplesCache::default();
    for object in &page.objects {
        append_object(&mut scene, object, &mut samples)?;
    }
    Ok(scene)
}

/// 无字体上下文的保守文本包围盒；精确文字选择范围应使用应用的 egui Galley。
pub fn object_bounds(object: &BoardObject) -> Rect {
    if validate_object(object).is_err() {
        return Rect::NOTHING;
    }
    match &object.kind {
        ObjectKind::Shape {
            shape,
            points,
            style,
        } => geometry(*shape, points)
            .map(|g| {
                Rect::from_points(&g.vertices.into_iter().map(pos).collect::<Vec<_>>())
                    .expand(style.width / 2.0)
            })
            .unwrap_or(Rect::NOTHING),
        ObjectKind::Stroke { points, style } => {
            let mut bounds = Rect::NOTHING;
            for point in points {
                bounds.extend_with(egui::pos2(point.x, point.y));
            }
            bounds.expand(style.width / 2.0)
        }
        ObjectKind::Handwritten {
            position, strokes, ..
        } => {
            let mut bounds = Rect::NOTHING;
            for stroke in strokes {
                let mut stroke_bounds = Rect::NOTHING;
                for point in &stroke.points {
                    stroke_bounds
                        .extend_with(egui::pos2(point.x + position.x, point.y + position.y));
                }
                // Pressure/velocity never exceed the pen width. Bounded joins extend
                // up to width / sqrt(2); dash samples stay inside the input bbox.
                // Selection must not tessellate arbitrarily long dashed segments.
                bounds = bounds.union(
                    stroke_bounds.expand(stroke.style.width * std::f32::consts::FRAC_1_SQRT_2),
                );
            }
            bounds
        }
        ObjectKind::Text {
            position,
            text,
            size,
            ..
        } => {
            let columns = text
                .split('\n')
                .map(|s| {
                    s.chars()
                        .map(|c| if c == '\t' { 4 } else { 1 })
                        .sum::<usize>()
                })
                .max()
                .unwrap_or(0);
            size_rect(
                *position,
                columns as f32 * size * 1.25,
                text.split('\n').count() as f32 * size * 1.5,
            )
        }
        ObjectKind::Math {
            position,
            layout,
            size,
            ..
        } => MathBox::new(layout, *size)
            .map(|measured| size_rect(*position, measured.width, measured.height))
            .unwrap_or(Rect::NOTHING),
        ObjectKind::Image {
            position,
            width,
            height,
            ..
        } => size_rect(*position, *width, *height),
        ObjectKind::FunctionPlot {
            position,
            width,
            height,
            ..
        } => size_rect(*position, *width, *height).expand(1.0),
        ObjectKind::CoordinateSystem { origin, scale } => {
            Rect::from_center_size(pos(*origin), Vec2::splat(10.0 * scale)).expand(8.0)
        }
    }
}
fn background(blackboard: bool) -> Color32 {
    if blackboard {
        Color32::from_rgb(24, 52, 43)
    } else {
        Color32::WHITE
    }
}
fn texture_lines(rect: Rect) -> impl Iterator<Item = (Pos2, Pos2)> {
    // 确定性细纹，间距随视口增大，始终最多 1024 条。
    let step = (rect.height() / 1024.0).max(7.0);
    let count = (rect.height() / step).ceil().clamp(0.0, 1024.0) as usize;
    (0..count).map(move |i| {
        let y = rect.top() + i as f32 * step;
        (
            egui::pos2(rect.left(), y),
            egui::pos2(rect.right(), y + 0.7),
        )
    })
}
fn paint_error(painter: &egui::Painter, position: Pos2, message: &str) {
    painter.text(
        position,
        egui::Align2::LEFT_TOP,
        message,
        egui::FontId::proportional(14.0),
        Color32::LIGHT_RED,
    );
}
fn capsule(a: Pos2, b: Pos2, width: f32, color: Color) -> egui::Shape {
    let angle = (b.y - a.y).atan2(b.x - a.x);
    let mut points = Vec::with_capacity(18);
    for (center, start) in [
        (b, angle - std::f32::consts::FRAC_PI_2),
        (a, angle + std::f32::consts::FRAC_PI_2),
    ] {
        for i in 0..=8 {
            let t = start + i as f32 * std::f32::consts::PI / 8.0;
            points.push(center + Vec2::angled(t) * (width / 2.0));
        }
    }
    egui::Shape::convex_polygon(points, rgba(color), egui::Stroke::NONE)
}

// #region debug-point A: fullscreen render CPU telemetry (test-only)
#[cfg(test)]
#[derive(Clone, Copy, Default)]
struct RenderDebugMetrics {
    revision_hits: u64,
    revision_misses: u64,
    object_hits: u64,
    object_misses: u64,
    culled_objects: u64,
    mesh_hits: u64,
    mesh_misses: u64,
    mesh_build_us: f64,
    scene_update_us: f64,
    paint_clone_us: f64,
    paint_arc_clones: u64,
    built_vertices: usize,
    built_triangles: usize,
    batch_rebuilds: u64,
    batch_buffer_allocations: u64,
    batch_arc_allocations: u64,
    batch_copied_vertices: usize,
    batch_copied_indices: usize,
    content_compare_skips: u64,
}
#[cfg(test)]
std::thread_local! {
    static RENDER_DEBUG_METRICS: std::cell::Cell<RenderDebugMetrics> =
        std::cell::Cell::new(RenderDebugMetrics::default());
}
// #endregion

struct LineMeshes {
    pixels_per_point: f32,
    options: egui::epaint::TessellationOptions,
    clip: Rect,
    transform: egui::emath::TSTransform,
    batches: Vec<(usize, usize, std::sync::Arc<egui::Mesh>)>,
}
fn mesh_buffer_bytes(mesh: &egui::Mesh) -> usize {
    mesh.vertices
        .capacity()
        .saturating_mul(std::mem::size_of::<egui::epaint::Vertex>())
        .saturating_add(
            mesh.indices
                .capacity()
                .saturating_mul(std::mem::size_of::<u32>()),
        )
}

impl LineMeshes {
    fn buffer_bytes(&self) -> usize {
        self.batches.iter().fold(0usize, |total, (_, _, mesh)| {
            total.saturating_add(mesh_buffer_bytes(mesh))
        })
    }

    fn new(painter: &egui::Painter, scene: &Scene) -> Self {
        Self::transformed(painter, scene, egui::emath::TSTransform::IDENTITY)
    }
    fn transformed(
        painter: &egui::Painter,
        scene: &Scene,
        transform: egui::emath::TSTransform,
    ) -> Self {
        let pixels_per_point = painter.ctx().pixels_per_point();
        let options = painter.ctx().tessellation_options(|options| *options);
        let clip = painter.clip_rect();
        // 只缓存纯色笔迹/线条，不保存字形图集 UV 或图片纹理 ID。
        // 连续笔迹共享边界，不在每段连接处重复覆盖半透明颜色。
        let mut tessellator =
            egui::epaint::Tessellator::new(pixels_per_point, options, [1, 1], Vec::new());
        let mut batches = Vec::new();
        let mut current_clip = clip;
        let mut index = 0;
        while index < scene.items.len() {
            match &scene.items[index] {
                Primitive::Clip(rect) => current_clip = transform.mul_rect(*rect).intersect(clip),
                Primitive::EndClip => current_clip = clip,
                Primitive::Line(..) | Primitive::Stroke(_) => {
                    let start = index;
                    let mut mesh = egui::Mesh::default();
                    tessellator.set_clip_rect(current_clip);
                    while let Some(item) = scene.items.get(index) {
                        match item {
                            Primitive::Line(a, b, width, color) => {
                                let (a, b, width) =
                                    (transform * *a, transform * *b, width * transform.scaling);
                                if current_clip
                                    .intersects(Rect::from_two_pos(a, b).expand(width / 2.0))
                                {
                                    tessellator
                                        .tessellate_shape(capsule(a, b, width, *color), &mut mesh);
                                }
                            }
                            Primitive::Stroke(stroke) => {
                                let feather = stroke_feather(options, pixels_per_point);
                                if current_clip
                                    .intersects(transform.mul_rect(stroke.bounds).expand(feather))
                                {
                                    stroke.mesh(transform, pixels_per_point, feather, &mut mesh);
                                }
                            }
                            _ => break,
                        }
                        index += 1;
                    }
                    batches.push((start, index, std::sync::Arc::new(mesh)));
                    continue;
                }
                _ => {}
            }
            index += 1;
        }
        Self {
            pixels_per_point,
            options,
            clip,
            transform,
            batches,
        }
    }
    fn matches(&self, painter: &egui::Painter) -> bool {
        self.pixels_per_point == painter.ctx().pixels_per_point()
            && self.clip == painter.clip_rect()
            && self.options == painter.ctx().tessellation_options(|options| *options)
    }
}

fn stroke_feather(options: egui::epaint::TessellationOptions, pixels_per_point: f32) -> f32 {
    if options.feathering {
        options.feathering_size_in_pixels / pixels_per_point
    } else {
        0.0
    }
}

fn paint_scene(painter: &egui::Painter, scene: &Scene, resources: &dyn ImageResources) {
    paint_scene_with_meshes(painter, scene, resources, None);
}
fn paint_scene_with_meshes(
    painter: &egui::Painter,
    scene: &Scene,
    resources: &dyn ImageResources,
    meshes: Option<&LineMeshes>,
) {
    paint_scene_transformed(
        painter,
        scene,
        resources,
        meshes,
        egui::emath::TSTransform::IDENTITY,
    );
}

fn paint_scene_transformed(
    painter: &egui::Painter,
    scene: &Scene,
    resources: &dyn ImageResources,
    meshes: Option<&LineMeshes>,
    transform: egui::emath::TSTransform,
) {
    let original_clip = painter.clip_rect();
    let mut painter = painter.clone();
    let mut batches = meshes.into_iter().flat_map(|m| m.batches.iter()).peekable();
    let mut items = scene.items.iter().enumerate();
    while let Some((index, item)) = items.next() {
        if let Some((start, end, mesh)) = batches.peek()
            && *start == index
        {
            if !mesh.is_empty() {
                // #region debug-point A: paint clone is an Arc clone, not a vertex copy
                #[cfg(test)]
                RENDER_DEBUG_METRICS.with(|metrics| {
                    let mut value = metrics.get();
                    value.paint_arc_clones += 1;
                    metrics.set(value);
                });
                // #endregion
                painter.add(egui::Shape::Mesh(std::sync::Arc::clone(mesh)));
            }
            let skip = end - start - 1;
            if skip > 0 {
                items.nth(skip - 1);
            }
            batches.next();
            continue;
        }
        match item {
            Primitive::Clip(rect) => {
                painter.set_clip_rect(transform.mul_rect(*rect).intersect(original_clip))
            }
            Primitive::EndClip => painter.set_clip_rect(original_clip),
            Primitive::Stroke(stroke) => {
                let feather = stroke_feather(
                    painter.ctx().tessellation_options(|options| *options),
                    painter.ctx().pixels_per_point(),
                );
                if painter
                    .clip_rect()
                    .intersects(transform.mul_rect(stroke.bounds).expand(feather))
                {
                    let mut mesh = egui::Mesh::default();
                    stroke.mesh(
                        transform,
                        painter.ctx().pixels_per_point(),
                        feather,
                        &mut mesh,
                    );
                    painter.add(mesh);
                }
            }
            Primitive::Line(a, b, width, color) => {
                let (a, b, width) = (transform * *a, transform * *b, width * transform.scaling);
                if !painter
                    .clip_rect()
                    .intersects(Rect::from_two_pos(a, b).expand(width / 2.0))
                {
                    continue;
                }
                // 单个凸胶囊避免 egui 方端点及叠加圆帽造成半透明颜色加深。
                painter.add(capsule(a, b, width, *color));
            }
            Primitive::Disk(center, radius, color) => {
                painter.circle_filled(
                    transform * *center,
                    radius * transform.scaling,
                    rgba(*color),
                );
            }
            Primitive::Text(p, text, size, color) => {
                let size = size * transform.scaling;
                for (i, line) in text.split('\n').enumerate() {
                    painter.text(
                        transform * *p + egui::vec2(0.0, i as f32 * size * 1.5),
                        egui::Align2::LEFT_TOP,
                        line,
                        egui::FontId::proportional(size),
                        rgba(*color),
                    );
                }
            }
            Primitive::Image(rect, asset_ref) => {
                let rect = transform.mul_rect(*rect);
                if let Some(texture) = resources.texture_id(asset_ref) {
                    painter.image(
                        texture,
                        rect,
                        Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                } else {
                    let clipped = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
                    clipped.rect_filled(rect, 0.0, Color32::from_gray(80));
                    clipped.line_segment(
                        [rect.left_top(), rect.right_bottom()],
                        egui::Stroke::new(2.0, Color32::LIGHT_RED),
                    );
                    clipped.line_segment(
                        [rect.right_top(), rect.left_bottom()],
                        egui::Stroke::new(2.0, Color32::LIGHT_RED),
                    );
                    paint_error(&clipped, rect.min, "缺失图片资源");
                }
            }
        }
    }
}
pub fn paint_page(painter: &egui::Painter, page: &Page, blackboard: bool) {
    paint_page_with_resources(painter, page, blackboard, &NoResources);
}
pub fn paint_page_with_resources(
    painter: &egui::Painter,
    page: &Page,
    blackboard: bool,
    resources: &dyn ImageResources,
) {
    paint_background(painter, blackboard);
    match page_scene(page) {
        Ok(scene) => paint_scene(painter, &scene, resources),
        Err(e) => paint_error(painter, painter.clip_rect().min, &e.to_string()),
    }
}

/// 只包含页面 Scene 的缩略图；不持有或修改 Document，不包含界面控件。
pub struct PageThumbnail {
    version: (String, u64),
    scene: Result<Scene>,
    meshes: std::cell::RefCell<Option<LineMeshes>>,
}
impl PageThumbnail {
    pub fn new(page: &Page, version: (&str, u64)) -> Self {
        Self {
            version: (version.0.into(), version.1),
            scene: page_scene(page),
            meshes: std::cell::RefCell::new(None),
        }
    }

    pub fn matches(&self, version: (&str, u64)) -> bool {
        self.version.0 == version.0 && self.version.1 == version.1
    }

    pub fn paint(
        &self,
        painter: &egui::Painter,
        target: Rect,
        canvas_size: Vec2,
        blackboard: bool,
        resources: &dyn ImageResources,
    ) -> Result<()> {
        let scene = self
            .scene
            .as_ref()
            .map_err(|e| RenderError::InvalidObject(e.to_string()))?;
        for item in &scene.items {
            if let Primitive::Image(_, asset) = item
                && resources.texture_id(asset).is_none()
            {
                return Err(RenderError::MissingResource(asset.clone()));
            }
        }
        let scale =
            (target.width() / canvas_size.x.max(1.0)).min(target.height() / canvas_size.y.max(1.0));
        let rect = Rect::from_center_size(target.center(), canvas_size * scale);
        let painter = painter.with_clip_rect(target.intersect(painter.clip_rect()));
        painter.rect_filled(target, 0.0, background(blackboard));
        let transform = egui::emath::TSTransform::new(rect.min.to_vec2(), scale);
        let mut meshes = self.meshes.borrow_mut();
        if !meshes
            .as_ref()
            .is_some_and(|meshes| meshes.matches(&painter) && meshes.transform == transform)
        {
            *meshes = Some(LineMeshes::transformed(&painter, scene, transform));
        }
        paint_scene_transformed(&painter, scene, resources, meshes.as_ref(), transform);
        Ok(())
    }
}

struct CachedObject {
    source: BoardObject,
    scene: Scene,
    bounds: Rect,
    meshes: Option<LineMeshes>,
}

const STROKE_BATCH_OBJECTS: usize = 64;
// All retained page mesh vertex/index buffers share this cap, including aggregates.
// Source snapshots, scenes, resources and egui's submitted frame Arcs are separate.
const MAX_RETAINED_MESH_BYTES: usize = 64 * 1024 * 1024;
const MAX_STROKE_BATCH_BYTES: usize = 32 * 1024 * 1024;
const MAX_SOURCE_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;

fn source_snapshot_bytes(page: &Page) -> usize {
    fn layout_bytes(layout: &MathLayout) -> usize {
        std::mem::size_of::<MathLayout>()
            + match layout {
                MathLayout::Text(text) => text.len(),
                MathLayout::Row(children) => children.iter().map(layout_bytes).sum(),
                MathLayout::Fraction(a, b) => layout_bytes(a) + layout_bytes(b),
                MathLayout::Radical(inner) => layout_bytes(inner),
            }
    }
    page.objects.iter().fold(page.id.len(), |total, object| {
        let payload = match &object.kind {
            ObjectKind::Stroke { points, .. } => {
                points.len() * std::mem::size_of::<board_core::StrokePoint>()
            }
            ObjectKind::Shape { points, .. } => points.len() * std::mem::size_of::<Point>(),
            ObjectKind::Text { text, .. } => text.len(),
            ObjectKind::Math { layout, .. } => layout_bytes(layout),
            ObjectKind::Handwritten {
                text,
                layout,
                strokes,
                ..
            } => strokes.iter().fold(
                text.len()
                    .saturating_add(layout.as_ref().map_or(0, layout_bytes)),
                |bytes, stroke| {
                    bytes
                        .saturating_add(std::mem::size_of::<board_core::HandwritingStroke>())
                        .saturating_add(
                            stroke
                                .points
                                .len()
                                .saturating_mul(std::mem::size_of::<board_core::StrokePoint>()),
                        )
                },
            ),
            ObjectKind::Image { asset_ref, .. } => asset_ref.len(),
            ObjectKind::FunctionPlot { expressions, .. } => expressions
                .iter()
                .map(|s| s.len() + std::mem::size_of::<String>())
                .sum(),
            ObjectKind::CoordinateSystem { .. } => 0,
        };
        total
            .saturating_add(std::mem::size_of::<BoardObject>())
            .saturating_add(object.id.len().saturating_mul(3))
            .saturating_add(payload)
    })
}

enum PagePaintCommand {
    Mesh(std::sync::Arc<egui::Mesh>),
    Object(usize),
}

#[derive(Default)]
struct StrokeChunk {
    // Arc identities are enough: object meshes are immutable and clip/DPI keyed.
    sources: Vec<Option<std::sync::Arc<egui::Mesh>>>,
    commands: Vec<PagePaintCommand>,
    bytes: usize,
}

#[derive(Clone, PartialEq)]
struct PageMeshKey {
    pixels_per_point: f32,
    options: egui::epaint::TessellationOptions,
    clip: Rect,
}
impl PageMeshKey {
    fn new(painter: &egui::Painter) -> Self {
        Self {
            pixels_per_point: painter.ctx().pixels_per_point(),
            options: painter.ctx().tessellation_options(|options| *options),
            clip: painter.clip_rect(),
        }
    }
}

/// 仅保留当前页的对象快照及几何；revision 改变不丢弃未修改对象。
#[derive(Default)]
pub struct PageRenderer {
    plot_samples: PlotSamplesCache,
    objects: std::collections::HashMap<String, CachedObject>,
    page_id: Option<String>,
    revision: Option<(String, String, u64)>,
    preview: Option<()>,
    order: Vec<String>,
    chunks: Vec<StrokeChunk>,
    mesh_key: Option<PageMeshKey>,
    batches_dirty: bool,
    visible_objects: u64,
    culled_objects: u64,
    object_mesh_bytes: usize,
}
impl PageRenderer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of actual sample_plot invocations, including failed attempts.
    #[cfg(any(test, feature = "test-support"))]
    pub fn plot_sample_calls(&self) -> u64 {
        self.plot_samples.sample_calls
    }

    pub fn clear(&mut self) {
        self.plot_samples.entries.clear();
        self.objects.clear();
        self.page_id = None;
        self.revision = None;
        self.preview = None;
        self.order.clear();
        self.chunks.clear();
        self.mesh_key = None;
        self.batches_dirty = true;
        self.visible_objects = 0;
        self.culled_objects = 0;
        self.object_mesh_bytes = 0;
    }

    fn update_objects(&mut self, page: &Page) -> Result<()> {
        validate_page_budget(page)?;
        if source_snapshot_bytes(page) > MAX_SOURCE_SNAPSHOT_BYTES {
            return Err(RenderError::ResourceLimit("页面源快照预算超限"));
        }
        let ids: std::collections::HashSet<_> =
            page.objects.iter().map(|o| o.id.as_str()).collect();
        if ids.len() != page.objects.len() {
            return Err(RenderError::InvalidObject("页面对象 ID 重复".into()));
        }
        if !self
            .order
            .iter()
            .map(String::as_str)
            .eq(page.objects.iter().map(|o| o.id.as_str()))
        {
            self.order = page.objects.iter().map(|o| o.id.clone()).collect();
            self.chunks.clear();
            self.batches_dirty = true;
        }
        self.objects.retain(|id, _| ids.contains(id.as_str()));
        self.plot_samples.retain_page(page);
        let mut primitives = 0usize;
        let mut stroke_points = 0usize;
        for (object_index, object) in page.objects.iter().enumerate() {
            let hit = self
                .objects
                .get(&object.id)
                .is_some_and(|entry| entry.source == *object);
            #[cfg(test)]
            RENDER_DEBUG_METRICS.with(|metrics| {
                let mut value = metrics.get();
                if hit {
                    value.object_hits += 1;
                } else {
                    value.object_misses += 1;
                }
                metrics.set(value);
            });
            if !hit {
                self.batches_dirty = true;
                // Release stale source Arcs as well as the aggregate before replacing
                // object meshes, so the shared budget never misses hidden old buffers.
                if let Some(chunk) = self.chunks.get_mut(object_index / STROKE_BATCH_OBJECTS) {
                    *chunk = StrokeChunk::default();
                }
                let mut scene = Scene::default();
                append_object(&mut scene, object, &mut self.plot_samples)?;
                let mut bounds = object_bounds(object);
                // Stroke joins can extend past the input-point bbox by more than half a width.
                for item in &scene.items {
                    if let Primitive::Stroke(stroke) = item {
                        bounds = bounds.union(stroke.bounds);
                    }
                }
                self.objects.insert(
                    object.id.clone(),
                    CachedObject {
                        source: object.clone(),
                        scene,
                        bounds,
                        meshes: None,
                    },
                );
            }
            let scene = &self.objects[&object.id].scene;
            primitives = primitives.saturating_add(scene.items.len());
            stroke_points = stroke_points.saturating_add(scene.stroke_points);
            if primitives > MAX_PRIMITIVES || stroke_points > MAX_INPUT_POINTS * 2 {
                return Err(RenderError::ResourceLimit("页面绘制图元或笔迹细分点数超限"));
            }
        }
        self.page_id = Some(page.id.clone());
        Ok(())
    }

    fn prepare_meshes(&mut self, painter: &egui::Painter) {
        self.prepare_meshes_with_budget(painter, MAX_RETAINED_MESH_BYTES);
    }

    fn prepare_meshes_with_budget(&mut self, painter: &egui::Painter, budget: usize) {
        // Key changes clear chunks in paint_cached. Discard obsolete offscreen
        // meshes too: they must not accumulate as the viewport moves.
        for entry in self.objects.values_mut() {
            if entry.meshes.as_ref().is_some_and(|m| !m.matches(painter)) {
                entry.meshes = None;
            }
        }
        self.object_mesh_bytes = self
            .objects
            .values()
            .filter_map(|entry| entry.meshes.as_ref())
            .map(LineMeshes::buffer_bytes)
            .sum();
        let aggregate_bytes: usize = self.chunks.iter().map(|c| c.bytes).sum();
        if self.object_mesh_bytes.saturating_add(aggregate_bytes) > budget {
            self.chunks.clear();
            for entry in self.objects.values_mut() {
                entry.meshes = None;
            }
            self.object_mesh_bytes = 0;
        }
        let aggregate_bytes: usize = self.chunks.iter().map(|c| c.bytes).sum();
        let feather = stroke_feather(
            painter.ctx().tessellation_options(|options| *options),
            painter.ctx().pixels_per_point(),
        );
        self.visible_objects = 0;
        self.culled_objects = 0;
        for id in &self.order {
            let entry = self.objects.get_mut(id).unwrap();
            if !painter.clip_rect().intersects(entry.bounds.expand(feather)) {
                self.culled_objects += 1;
                #[cfg(test)]
                RENDER_DEBUG_METRICS.with(|metrics| {
                    let mut value = metrics.get();
                    value.culled_objects += 1;
                    metrics.set(value);
                });
                continue;
            }
            self.visible_objects += 1;
            if !entry
                .meshes
                .as_ref()
                .is_some_and(|meshes| meshes.matches(painter))
            {
                #[cfg(test)]
                let start = std::time::Instant::now();
                let meshes = LineMeshes::new(painter, &entry.scene);
                let bytes = meshes.buffer_bytes();
                if self
                    .object_mesh_bytes
                    .saturating_add(aggregate_bytes)
                    .saturating_add(bytes)
                    > budget
                {
                    // Keep the scene; paint_scene_with_meshes(None) renders it uncached.
                    // Never omit geometry just because retaining its mesh is too costly.
                    continue;
                }
                self.object_mesh_bytes += bytes;
                entry.meshes = Some(meshes);
                #[cfg(test)]
                RENDER_DEBUG_METRICS.with(|metrics| {
                    let mut value = metrics.get();
                    value.mesh_misses += 1;
                    value.mesh_build_us += start.elapsed().as_secs_f64() * 1e6;
                    for (_, _, mesh) in &entry.meshes.as_ref().unwrap().batches {
                        value.built_vertices += mesh.vertices.len();
                        value.built_triangles += mesh.indices.len() / 3;
                    }
                    metrics.set(value);
                });
            } else {
                #[cfg(test)]
                RENDER_DEBUG_METRICS.with(|metrics| {
                    let mut value = metrics.get();
                    value.mesh_hits += 1;
                    metrics.set(value);
                });
            }
        }
    }

    fn prepare_chunks(&mut self, painter: &egui::Painter) {
        self.prepare_chunks_with_budget(painter, MAX_RETAINED_MESH_BYTES);
    }

    fn prepare_chunks_with_budget(&mut self, painter: &egui::Painter, budget: usize) {
        let aggregate_budget =
            MAX_STROKE_BATCH_BYTES.min(budget.saturating_sub(self.object_mesh_bytes));
        let feather = stroke_feather(
            painter.ctx().tessellation_options(|options| *options),
            painter.ctx().pixels_per_point(),
        );
        self.chunks
            .truncate(self.order.len().div_ceil(STROKE_BATCH_OBJECTS));
        let mut bytes: usize = self.chunks.iter().map(|chunk| chunk.bytes).sum();
        for (chunk_index, ids) in self.order.chunks(STROKE_BATCH_OBJECTS).enumerate() {
            if chunk_index == self.chunks.len() {
                self.chunks.push(StrokeChunk::default());
            }
            let source = |id: &String| {
                let entry = &self.objects[id];
                if matches!(entry.source.kind, ObjectKind::Stroke { .. })
                    && painter.clip_rect().intersects(entry.bounds.expand(feather))
                    && entry
                        .scene
                        .items
                        .iter()
                        .all(|item| matches!(item, Primitive::Stroke(_) | Primitive::Line(..)))
                {
                    entry.meshes.as_ref().and_then(|meshes| {
                        (meshes.batches.len() == 1).then(|| &meshes.batches[0].2)
                    })
                } else {
                    None
                }
            };
            let chunk = &mut self.chunks[chunk_index];
            if chunk.sources.len() == ids.len()
                && ids
                    .iter()
                    .zip(&chunk.sources)
                    .all(|(id, old)| match (source(id), old) {
                        (Some(new), Some(old)) => std::sync::Arc::ptr_eq(new, old),
                        (None, None) => true,
                        _ => false,
                    })
            {
                continue;
            }
            bytes -= chunk.bytes;
            // Drop the old aggregate before allocating its replacement; sources only share Arcs.
            chunk.commands.clear();
            chunk.bytes = 0;
            chunk.sources = ids.iter().map(|id| source(id).cloned()).collect();
            let mut index = 0;
            while index < ids.len() {
                if chunk.sources[index].is_none() {
                    chunk.commands.push(PagePaintCommand::Object(
                        chunk_index * STROKE_BATCH_OBJECTS + index,
                    ));
                    index += 1;
                    continue;
                }
                let start = index;
                while index < ids.len() && chunk.sources[index].is_some() {
                    index += 1;
                }
                let meshes = &chunk.sources[start..index];
                let vertices: usize = meshes
                    .iter()
                    .flatten()
                    .map(|mesh| mesh.vertices.len())
                    .sum();
                let indices: usize = meshes.iter().flatten().map(|mesh| mesh.indices.len()).sum();
                let required = vertices * std::mem::size_of::<egui::epaint::Vertex>()
                    + indices * std::mem::size_of::<u32>();
                if meshes.len() == 1 || bytes.saturating_add(required) > aggregate_budget {
                    for mesh in meshes.iter().flatten().filter(|mesh| !mesh.is_empty()) {
                        chunk.commands.push(PagePaintCommand::Mesh(mesh.clone()));
                    }
                    continue;
                }
                if indices == 0 {
                    continue;
                }
                let mut merged = egui::Mesh {
                    vertices: Vec::with_capacity(vertices),
                    indices: Vec::with_capacity(indices),
                    ..Default::default()
                };
                for mesh in meshes.iter().flatten() {
                    merged.append_ref(mesh);
                }
                let allocated = mesh_buffer_bytes(&merged);
                if bytes.saturating_add(allocated) > aggregate_budget {
                    for mesh in meshes.iter().flatten().filter(|mesh| !mesh.is_empty()) {
                        chunk.commands.push(PagePaintCommand::Mesh(mesh.clone()));
                    }
                    continue;
                }
                bytes += allocated;
                chunk.bytes += allocated;
                chunk
                    .commands
                    .push(PagePaintCommand::Mesh(std::sync::Arc::new(merged)));
                #[cfg(test)]
                RENDER_DEBUG_METRICS.with(|metrics| {
                    let mut value = metrics.get();
                    value.batch_rebuilds += 1;
                    value.batch_buffer_allocations += 2;
                    value.batch_arc_allocations += 1;
                    value.batch_copied_vertices += vertices;
                    value.batch_copied_indices += indices;
                    metrics.set(value);
                });
            }
        }
    }

    fn paint_cached(&mut self, painter: &egui::Painter, resources: &dyn ImageResources) {
        let key = PageMeshKey::new(painter);
        let key_changed = self.mesh_key.as_ref() != Some(&key);
        if key_changed || self.batches_dirty {
            if key_changed {
                self.chunks.clear();
            }
            self.prepare_meshes(painter);
            self.prepare_chunks(painter);
            self.mesh_key = Some(key.clone());
            self.batches_dirty = false;
        } else {
            #[cfg(test)]
            RENDER_DEBUG_METRICS.with(|metrics| {
                let mut value = metrics.get();
                value.mesh_hits += self.visible_objects;
                value.culled_objects += self.culled_objects;
                metrics.set(value);
            });
        }
        #[cfg(test)]
        let start = std::time::Instant::now();
        let feather = stroke_feather(key.options, key.pixels_per_point);
        for chunk in &self.chunks {
            for command in &chunk.commands {
                match command {
                    PagePaintCommand::Mesh(mesh) => {
                        painter.add(egui::Shape::Mesh(mesh.clone()));
                        #[cfg(test)]
                        RENDER_DEBUG_METRICS.with(|metrics| {
                            let mut value = metrics.get();
                            value.paint_arc_clones += 1;
                            metrics.set(value);
                        });
                    }
                    PagePaintCommand::Object(index) => {
                        let entry = &self.objects[&self.order[*index]];
                        if painter.clip_rect().intersects(entry.bounds.expand(feather)) {
                            paint_scene_with_meshes(
                                painter,
                                &entry.scene,
                                resources,
                                entry.meshes.as_ref(),
                            );
                        }
                    }
                }
            }
        }
        #[cfg(test)]
        RENDER_DEBUG_METRICS.with(|metrics| {
            let mut value = metrics.get();
            value.paint_clone_us += start.elapsed().as_secs_f64() * 1e6;
            metrics.set(value);
        });
    }

    /// document/page 切换清空；对象 ID 与完整快照共同校验，亦检测同 revision 直接编辑。
    pub fn paint_page_at_revision(
        &mut self,
        painter: &egui::Painter,
        page: &Page,
        version: (&str, u64),
        blackboard: bool,
        resources: &dyn ImageResources,
    ) -> Result<()> {
        paint_background(painter, blackboard);
        #[cfg(test)]
        let start = std::time::Instant::now();
        #[cfg(test)]
        RENDER_DEBUG_METRICS.with(|metrics| {
            let mut value = metrics.get();
            if self.revision.as_ref().is_some_and(|(doc, id, revision)| {
                doc == version.0 && id == &page.id && *revision == version.1
            }) {
                value.revision_hits += 1;
            } else {
                value.revision_misses += 1;
            }
            metrics.set(value);
        });
        if !self
            .revision
            .as_ref()
            .is_some_and(|(doc, id, _)| doc == version.0 && id == &page.id)
        {
            self.clear();
        }
        if let Err(error) = self.update_objects(page) {
            self.clear();
            paint_error(painter, painter.clip_rect().min, &error.to_string());
            return Err(error);
        }
        self.preview = None;
        self.revision = Some((version.0.into(), page.id.clone(), version.1));
        #[cfg(test)]
        RENDER_DEBUG_METRICS.with(|metrics| {
            let mut value = metrics.get();
            value.scene_update_us += start.elapsed().as_secs_f64() * 1e6;
            metrics.set(value);
        });
        self.paint_cached(painter, resources);
        Ok(())
    }

    /// Fast path for callers guaranteeing that every content/order change advances the
    /// document revision. A replacement document must have a new ID or call `clear()`.
    /// Unlike `paint_page_at_revision`, this does not detect same-revision direct mutation.
    /// Preview rendering invalidates the guarantee until the next full synchronization.
    pub fn paint_page_at_document_revision(
        &mut self,
        painter: &egui::Painter,
        page: &Page,
        version: (&str, u64),
        blackboard: bool,
        resources: &dyn ImageResources,
    ) -> Result<()> {
        if self.preview.is_none()
            && self.revision.as_ref().is_some_and(|(doc, id, revision)| {
                doc == version.0 && id == &page.id && *revision == version.1
            })
        {
            paint_background(painter, blackboard);
            #[cfg(test)]
            RENDER_DEBUG_METRICS.with(|metrics| {
                let mut value = metrics.get();
                value.revision_hits += 1;
                value.content_compare_skips += 1;
                metrics.set(value);
            });
            self.paint_cached(painter, resources);
            Ok(())
        } else {
            self.paint_page_at_revision(painter, page, version, blackboard, resources)
        }
    }

    pub fn paint_page_with_resources(
        &mut self,
        painter: &egui::Painter,
        page: &Page,
        blackboard: bool,
        resources: &dyn ImageResources,
    ) -> Result<()> {
        paint_background(painter, blackboard);
        if self.page_id.as_ref() != Some(&page.id) {
            self.clear();
        }
        if let Err(error) = self.update_objects(page) {
            self.clear();
            paint_error(painter, painter.clip_rect().min, &error.to_string());
            return Err(error);
        }
        self.preview = Some(());
        self.paint_cached(painter, resources);
        Ok(())
    }
}

fn paint_background(painter: &egui::Painter, blackboard: bool) {
    // 屏幕画板透出桌面；导出仍使用独立的白底路径。
    if !blackboard {
        return;
    }
    let rect = painter.clip_rect();
    painter.rect_filled(rect, 0.0, background(blackboard));
    if rect.is_finite() {
        for (a, b) in texture_lines(rect) {
            painter.line_segment(
                [a, b],
                egui::Stroke::new(0.5, Color32::from_rgba_unmultiplied(190, 210, 190, 10)),
            );
        }
    }
}
pub fn paint_object(painter: &egui::Painter, object: &BoardObject) {
    paint_object_with_resources(painter, object, &NoResources);
}
pub fn paint_object_with_resources(
    painter: &egui::Painter,
    object: &BoardObject,
    resources: &dyn ImageResources,
) {
    let mut scene = Scene::default();
    match append_object(&mut scene, object, &mut PlotSamplesCache::default()) {
        Ok(()) => paint_scene(painter, &scene, resources),
        Err(e) => paint_error(painter, painter.clip_rect().min, &e.to_string()),
    }
}
fn export_rect(width: u32, height: u32) -> Result<Rect> {
    if width == 0
        || height == 0
        || width > MAX_EXPORT_DIMENSION
        || height > MAX_EXPORT_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_EXPORT_PIXELS
    {
        return Err(RenderError::InvalidDimensions);
    }
    Ok(Rect::from_min_size(
        Pos2::ZERO,
        egui::vec2(width as f32, height as f32),
    ))
}
fn escape_xml(text: &str) -> String {
    let mut output = String::new();
    for c in text.chars() {
        match c {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&apos;"),
            _ => output.push(c),
        }
    }
    output
}
fn svg_color(color: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}

pub fn export_svg(page: &Page, width: u32, height: u32, blackboard: bool) -> Result<String> {
    export_svg_with_resources(page, width, height, blackboard, &RenderResources::default())
}

pub fn export_svg_with_resources(
    page: &Page,
    width: u32,
    height: u32,
    blackboard: bool,
    resources: &RenderResources,
) -> Result<String> {
    let rect = export_rect(width, height)?;
    let scene = page_scene(page)?;
    let bg = background(blackboard);
    let mut output = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\"><rect width=\"100%\" height=\"100%\" fill=\"#{:02x}{:02x}{:02x}\"/>",
        bg.r(),
        bg.g(),
        bg.b()
    );
    if blackboard {
        for (a, b) in texture_lines(rect) {
            let _ = write!(
                output,
                "<path d=\"M {} {} L {} {}\" stroke=\"#bed2be\" stroke-opacity=\"0.039216\" stroke-width=\"0.5\"/>",
                a.x, a.y, b.x, b.y
            );
        }
    }
    for (index, item) in scene.items.into_iter().enumerate() {
        match item {
            Primitive::Clip(r) => {
                let _ = write!(
                    output,
                    "<defs><clipPath id=\"plot-{index}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath></defs><g clip-path=\"url(#plot-{index})\">",
                    r.min.x,
                    r.min.y,
                    r.width(),
                    r.height()
                );
            }
            Primitive::EndClip => output.push_str("</g>"),
            Primitive::Stroke(stroke) => {
                output.push_str("<path d=\"");
                for (i, p) in stroke.outline().iter().enumerate() {
                    let command = if i == 0 { 'M' } else { 'L' };
                    let _ = write!(output, "{command} {} {} ", p.x, p.y);
                    if output.len() > MAX_SVG_BYTES {
                        return Err(RenderError::ResourceLimit("SVG 输出超过 32 MiB"));
                    }
                }
                let _ = write!(
                    output,
                    "Z\" fill=\"{}\" fill-opacity=\"{}\"/>",
                    svg_color(stroke.color),
                    f32::from(stroke.color.a) / 255.0
                );
            }
            Primitive::Line(a, b, w, c) => {
                let _ = write!(
                    output,
                    "<path d=\"M {} {} L {} {}\" fill=\"none\" stroke=\"{}\" stroke-opacity=\"{}\" stroke-width=\"{w}\" stroke-linecap=\"round\"/>",
                    a.x,
                    a.y,
                    b.x,
                    b.y,
                    svg_color(c),
                    f32::from(c.a) / 255.0
                );
            }
            Primitive::Disk(p, r, c) => {
                let _ = write!(
                    output,
                    "<circle cx=\"{}\" cy=\"{}\" r=\"{r}\" fill=\"{}\" fill-opacity=\"{}\"/>",
                    p.x,
                    p.y,
                    svg_color(c),
                    f32::from(c.a) / 255.0
                );
            }
            Primitive::Text(p, text, size, c) => {
                if resources.font.is_some() {
                    svg_text(&mut output, p, &text, size, c, resources)?;
                    continue;
                }
                let _ = write!(
                    output,
                    "<text font-family=\"sans-serif\" font-size=\"{size}\" fill=\"{}\" fill-opacity=\"{}\" xml:space=\"preserve\">",
                    svg_color(c),
                    f32::from(c.a) / 255.0
                );
                for (i, line) in text.split('\n').enumerate() {
                    let _ = write!(
                        output,
                        "<tspan x=\"{}\" y=\"{}\">{}</tspan>",
                        p.x,
                        p.y + size * (1.0 + i as f32 * 1.5),
                        escape_xml(line)
                    );
                }
                output.push_str("</text>");
            }
            Primitive::Image(rect, asset_ref) => {
                let image = resources
                    .images
                    .get(&asset_ref)
                    .ok_or(RenderError::MissingResource(asset_ref))?;
                if output.len() + image.png.len().div_ceil(3) * 4 + 256 > MAX_SVG_BYTES {
                    return Err(RenderError::ResourceLimit("SVG 输出超过 32 MiB"));
                }
                let _ = write!(
                    output,
                    "<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" href=\"data:image/png;base64,{}\"/>",
                    rect.min.x,
                    rect.min.y,
                    rect.width(),
                    rect.height(),
                    resources::base64(&image.png)
                );
            }
        }
        if output.len() > MAX_SVG_BYTES {
            return Err(RenderError::ResourceLimit("SVG 输出超过 32 MiB"));
        }
    }
    output.push_str("</svg>");
    Ok(output)
}
fn svg_text(
    output: &mut String,
    position: Pos2,
    text: &str,
    size: f32,
    color: Color,
    resources: &RenderResources,
) -> Result<()> {
    let font = resources.check_text(text)?;
    let scaled = font.as_scaled(size);
    let factor = scaled.scale_factor();
    let mut cursor = ab_glyph::point(position.x, position.y + scaled.ascent());
    let mut previous = None;
    let _ = write!(
        output,
        "<g aria-label=\"{}\" fill=\"{}\" fill-opacity=\"{}\">",
        escape_xml(text),
        svg_color(color),
        f32::from(color.a) / 255.0
    );
    for c in text.chars() {
        if c == '\n' {
            cursor.x = position.x;
            cursor.y += size * 1.5;
            previous = None;
            continue;
        }
        let id = font.glyph_id(c);
        if let Some(last) = previous {
            cursor.x += scaled.kern(last, id);
        }
        if let Some(outline) = font.outline(id) {
            let map = |p: ab_glyph::Point| {
                (
                    cursor.x + p.x * factor.horizontal,
                    cursor.y - p.y * factor.vertical,
                )
            };
            output.push_str("<path d=\"");
            let mut end = None;
            for curve in outline.curves {
                use ab_glyph::OutlineCurve::*;
                let start = match curve {
                    Line(a, _) | Quad(a, _, _) | Cubic(a, _, _, _) => a,
                };
                if end != Some(start) {
                    if end.is_some() {
                        output.push_str(" Z ");
                    }
                    let (x, y) = map(start);
                    let _ = write!(output, "M {x} {y} ");
                }
                match curve {
                    Line(_, b) => {
                        let (x, y) = map(b);
                        let _ = write!(output, "L {x} {y} ");
                        end = Some(b);
                    }
                    Quad(_, b, c) => {
                        let (x, y) = map(b);
                        let (u, v) = map(c);
                        let _ = write!(output, "Q {x} {y} {u} {v} ");
                        end = Some(c);
                    }
                    Cubic(_, b, c, d) => {
                        let (x, y) = map(b);
                        let (u, v) = map(c);
                        let (s, t) = map(d);
                        let _ = write!(output, "C {x} {y} {u} {v} {s} {t} ");
                        end = Some(d);
                    }
                }
                if output.len() > MAX_SVG_BYTES {
                    return Err(RenderError::ResourceLimit("SVG 输出超过 32 MiB"));
                }
            }
            output.push_str("Z\"/>");
        } else if !c.is_whitespace() {
            return Err(RenderError::MissingGlyph(c));
        }
        cursor.x += scaled.h_advance(id);
        previous = Some(id);
    }
    output.push_str("</g>");
    if output.len() > MAX_SVG_BYTES {
        return Err(RenderError::ResourceLimit("SVG 输出超过 32 MiB"));
    }
    Ok(())
}

fn raster_paint(color: Color) -> tiny_skia::Paint<'static> {
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
    paint.anti_alias = true;
    paint
}
fn raster_line(pixmap: &mut tiny_skia::Pixmap, a: Pos2, b: Pos2, width: f32, color: Color) {
    raster_line_clipped(pixmap, a, b, width, color, None);
}
fn raster_line_clipped(
    pixmap: &mut tiny_skia::Pixmap,
    a: Pos2,
    b: Pos2,
    width: f32,
    color: Color,
    mask: Option<&tiny_skia::Mask>,
) {
    let mut path = tiny_skia::PathBuilder::new();
    path.move_to(a.x, a.y);
    path.line_to(b.x, b.y);
    if let Some(path) = path.finish() {
        pixmap.stroke_path(
            &path,
            &raster_paint(color),
            &tiny_skia::Stroke {
                width,
                line_cap: tiny_skia::LineCap::Round,
                ..Default::default()
            },
            tiny_skia::Transform::identity(),
            mask,
        );
    }
}
pub fn export_png(page: &Page, width: u32, height: u32, blackboard: bool) -> Result<Vec<u8>> {
    export_png_with_resources(page, width, height, blackboard, &RenderResources::default())
}

pub fn export_png_with_resources(
    page: &Page,
    width: u32,
    height: u32,
    blackboard: bool,
    resources: &RenderResources,
) -> Result<Vec<u8>> {
    let rect = export_rect(width, height)?;
    let scene = page_scene(page)?;
    // 预检在分配像素缓冲前失败，绝不生成缺少文字或图片的“成功”文件。
    let mut raster_work = 0.0_f64;
    for item in &scene.items {
        let bounds = match item {
            Primitive::Clip(_) => {
                raster_work += f64::from(width) * f64::from(height);
                Rect::NOTHING
            }
            Primitive::EndClip => continue,
            Primitive::Text(p, text, size, _) => {
                resources.check_text(text)?;
                // 包含离屏字形的光栅开销，避免巨字号/长文本消耗不受输出尺寸限制。
                raster_work += text.chars().count() as f64 * f64::from(*size).powi(2) * 4.0;
                Rect::from_min_size(*p, Vec2::ZERO)
            }
            Primitive::Image(bounds, asset_ref) => {
                if !resources.images.contains_key(asset_ref) {
                    return Err(RenderError::MissingResource(asset_ref.clone()));
                }
                *bounds
            }
            Primitive::Stroke(stroke) => {
                raster_work += stroke.points.len() as f64 * 16.0;
                stroke.bounds.expand(1.0)
            }
            Primitive::Line(a, b, w, _) => Rect::from_two_pos(*a, *b).expand(*w / 2.0 + 1.0),
            Primitive::Disk(p, r, _) => Rect::from_center_size(*p, Vec2::splat(2.0 * r + 2.0)),
        }
        .intersect(rect);
        if bounds.is_positive() {
            raster_work += f64::from(bounds.width()) * f64::from(bounds.height());
        }
        if raster_work > 200_000_000.0 {
            return Err(RenderError::ResourceLimit("软件光栅化覆盖预算超限"));
        }
    }
    let mut pixmap = tiny_skia::Pixmap::new(width, height)
        .ok_or(RenderError::ResourceLimit("像素缓冲分配失败"))?;
    let bg = background(blackboard);
    pixmap.fill(tiny_skia::Color::from_rgba8(bg.r(), bg.g(), bg.b(), 255));
    if blackboard {
        for (a, b) in texture_lines(rect) {
            raster_line(
                &mut pixmap,
                a,
                b,
                0.5,
                Color {
                    r: 190,
                    g: 210,
                    b: 190,
                    a: 10,
                },
            );
        }
    }
    let mut mask = None;
    for item in scene.items {
        match item {
            Primitive::Clip(r) => {
                let mut clip = tiny_skia::Mask::new(width, height)
                    .ok_or(RenderError::ResourceLimit("裁剪缓冲分配失败"))?;
                if let Some(rect) =
                    tiny_skia::Rect::from_xywh(r.min.x, r.min.y, r.width(), r.height())
                {
                    clip.fill_path(
                        &tiny_skia::PathBuilder::from_rect(rect),
                        tiny_skia::FillRule::Winding,
                        true,
                        tiny_skia::Transform::identity(),
                    );
                }
                mask = Some(clip);
            }
            Primitive::EndClip => mask = None,
            Primitive::Stroke(stroke) => {
                let mut path = tiny_skia::PathBuilder::new();
                for (i, p) in stroke.outline().iter().enumerate() {
                    if i == 0 {
                        path.move_to(p.x, p.y);
                    } else {
                        path.line_to(p.x, p.y);
                    }
                }
                path.close();
                if let Some(path) = path.finish() {
                    pixmap.fill_path(
                        &path,
                        &raster_paint(stroke.color),
                        tiny_skia::FillRule::Winding,
                        tiny_skia::Transform::identity(),
                        mask.as_ref(),
                    );
                }
            }
            Primitive::Line(a, b, w, c) => {
                raster_line_clipped(&mut pixmap, a, b, w, c, mask.as_ref())
            }
            Primitive::Disk(p, r, c) => {
                if let Some(path) = tiny_skia::PathBuilder::from_circle(p.x, p.y, r) {
                    pixmap.fill_path(
                        &path,
                        &raster_paint(c),
                        tiny_skia::FillRule::Winding,
                        tiny_skia::Transform::identity(),
                        mask.as_ref(),
                    );
                }
            }
            Primitive::Text(p, text, size, color) => {
                raster_text(&mut pixmap, p, &text, size, color, resources)?;
            }
            Primitive::Image(rect, asset_ref) => {
                let image = &resources.images[&asset_ref].pixmap;
                pixmap.draw_pixmap(
                    0,
                    0,
                    image.as_ref(),
                    &tiny_skia::PixmapPaint {
                        quality: tiny_skia::FilterQuality::Bilinear,
                        ..Default::default()
                    },
                    tiny_skia::Transform::from_row(
                        rect.width() / image.width() as f32,
                        0.0,
                        0.0,
                        rect.height() / image.height() as f32,
                        rect.min.x,
                        rect.min.y,
                    ),
                    None,
                );
            }
        }
    }
    pixmap
        .encode_png()
        .map_err(|e| RenderError::Encoding(e.to_string()))
}

fn raster_text(
    pixmap: &mut tiny_skia::Pixmap,
    position: Pos2,
    text: &str,
    size: f32,
    color: Color,
    resources: &RenderResources,
) -> Result<()> {
    let font = resources.check_text(text)?;
    let scaled = font.as_scaled(size);
    let mut cursor = ab_glyph::point(position.x, position.y + scaled.ascent());
    let mut previous = None;
    for c in text.chars() {
        match c {
            '\n' => {
                cursor.x = position.x;
                cursor.y += size * 1.5;
                previous = None;
                continue;
            }
            '\r' => continue,
            '\t' => {
                cursor.x += scaled.h_advance(font.glyph_id(' ')) * 4.0;
                previous = None;
                continue;
            }
            _ => (),
        }
        let id = font.glyph_id(c);
        if let Some(last) = previous {
            cursor.x += scaled.kern(last, id);
        }
        let glyph = id.with_scale_and_position(size, cursor);
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            if f64::from(bounds.width()) * f64::from(bounds.height()) > 16_777_216.0 {
                return Err(RenderError::ResourceLimit("单字形光栅预算超限"));
            }
            let width = pixmap.width();
            let height = pixmap.height();
            if bounds.max.x > 0.0
                && bounds.max.y > 0.0
                && bounds.min.x < width as f32
                && bounds.min.y < height as f32
            {
                outlined.draw(|x, y, coverage| {
                    let x = bounds.min.x as i64 + i64::from(x);
                    let y = bounds.min.y as i64 + i64::from(y);
                    if x < 0 || y < 0 || x >= i64::from(width) || y >= i64::from(height) {
                        return;
                    }
                    let alpha = (coverage * f32::from(color.a)).round() as u32;
                    let pixel = &mut pixmap.pixels_mut()[y as usize * width as usize + x as usize];
                    let blend = |source: u8, destination: u8| {
                        ((u32::from(source) * alpha + u32::from(destination) * (255 - alpha) + 127)
                            / 255) as u8
                    };
                    *pixel = tiny_skia::PremultipliedColorU8::from_rgba(
                        blend(color.r, pixel.red()),
                        blend(color.g, pixel.green()),
                        blend(color.b, pixel.blue()),
                        (alpha + (u32::from(pixel.alpha()) * (255 - alpha) + 127) / 255) as u8,
                    )
                    .expect("source-over 保持预乘颜色不变量");
                });
            }
        } else if !c.is_whitespace() {
            return Err(RenderError::MissingGlyph(c));
        }
        cursor.x += scaled.h_advance(id);
        previous = Some(id);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
