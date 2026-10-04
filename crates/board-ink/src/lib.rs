//! 无窗口笔迹与几何算法。坐标单位由调用方决定，时间统一使用秒。

pub use board_core::{Point, ShapeKind, StrokePoint, Style};
use std::f64::consts::PI;

pub const MAX_POINTS: usize = 100_000;
const MAX_COORD: f32 = 1_000_000.0;
const EPS: f64 = 16.0 * f64::EPSILON;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InkError {
    InvalidInput,
    TooManyPoints,
    UnsupportedVertexEdit,
    InvalidVertex,
}

fn valid_point(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.x.abs() <= MAX_COORD && p.y.abs() <= MAX_COORD
}

fn check_points(points: &[Point]) -> Result<(), InkError> {
    if points.len() > MAX_POINTS {
        return Err(InkError::TooManyPoints);
    }
    if points.iter().any(|p| !valid_point(*p)) {
        return Err(InkError::InvalidInput);
    }
    Ok(())
}

fn position(p: &StrokePoint) -> Point {
    Point { x: p.x, y: p.y }
}

fn check_stroke(points: &[StrokePoint]) -> Result<(), InkError> {
    if points.len() > MAX_POINTS {
        return Err(InkError::TooManyPoints);
    }
    if points.iter().any(|p| {
        !valid_point(position(p))
            || !p.time.is_finite()
            || p.time.abs() > 1e12
            || !p.pressure.is_finite()
    }) {
        return Err(InkError::InvalidInput);
    }
    Ok(())
}

fn distance(a: Point, b: Point) -> f64 {
    (a.x as f64 - b.x as f64).hypot(a.y as f64 - b.y as f64)
}

fn lerp(a: Point, b: Point, t: f64) -> Point {
    // Preserve endpoints when subtracting very different coordinate scales loses the smaller one.
    if t == 0.0 {
        return a;
    }
    if t == 1.0 {
        return b;
    }
    Point {
        x: (a.x as f64 + (b.x as f64 - a.x as f64) * t) as f32,
        y: (a.y as f64 + (b.y as f64 - a.y as f64) * t) as f32,
    }
}

fn interpolate(a: &StrokePoint, b: &StrokePoint, t: f64) -> StrokePoint {
    if t == 0.0 {
        return *a;
    }
    if t == 1.0 {
        return *b;
    }
    let p = lerp(position(a), position(b), t);
    StrokePoint {
        x: p.x,
        y: p.y,
        time: a.time + (b.time - a.time) * t,
        pressure: (a.pressure as f64 + (b.pressure as f64 - a.pressure as f64) * t) as f32,
    }
}

// 重复位置合并；倒序时间提升至上一采样时刻，避免负速度和无穷宽度。
fn clean_stroke(points: &[StrokePoint]) -> Result<Vec<StrokePoint>, InkError> {
    check_stroke(points)?;
    let mut clean: Vec<StrokePoint> = Vec::with_capacity(points.len());
    for p in points {
        let mut p = *p;
        p.pressure = p.pressure.clamp(0.0, 1.0);
        if let Some(last) = clean.last_mut() {
            p.time = p.time.max(last.time);
            if position(last) == position(&p) {
                *last = p;
                continue;
            }
        }
        clean.push(p);
    }
    Ok(clean)
}

// 弦长参数的局部导数；短邻段不会把长邻段的控制柄放大。
fn curve_turn(a: Point, b: Point, c: Point) -> f64 {
    let u = (b.x as f64 - a.x as f64, b.y as f64 - a.y as f64);
    let v = (c.x as f64 - b.x as f64, c.y as f64 - b.y as f64);
    (u.0 * v.1 - u.1 * v.0).atan2(u.0 * v.0 + u.1 * v.1)
}

fn curve_derivative(a: Point, b: Point, c: Point) -> (f64, f64) {
    let left = distance(a, b);
    let right = distance(b, c);
    if left == 0.0 || right == 0.0 {
        return (0.0, 0.0);
    }
    let weight = right / (left + right);
    (
        (b.x as f64 - a.x as f64) / left * weight
            + (c.x as f64 - b.x as f64) / right * (1.0 - weight),
        (b.y as f64 - a.y as f64) / left * weight
            + (c.y as f64 - b.y as f64) / right * (1.0 - weight),
    )
}

// 限制 Bezier 控制柄沿弦投影至半弦，法向至半弦，避免短段回折/巨大过冲。
fn limited_tangent(tangent: (f64, f64), chord: (f64, f64), length: f64) -> (f64, f64) {
    let u = (chord.0 / length, chord.1 / length);
    let along = (tangent.0 * u.0 + tangent.1 * u.1).clamp(0.0, 1.5 * length);
    let normal = (-tangent.0 * u.1 + tangent.1 * u.0).clamp(-0.5 * length, 0.5 * length);
    (along * u.0 - normal * u.1, along * u.1 + normal * u.0)
}

/// 受限弦长 Hermite 局部插值，spacing 为输出最大段长（允许 f32 量化误差）。
/// strength=0 为线性；0..=1 连续增加曲线插值与密集交替抖动的降噪强度。
/// corner_degrees 为孤立转角阈值，0 禁止曲线；连续同向曲率可越过该阈值，
/// 但 >=80 度的转角始终保留。稀疏样本不移动，端点保留清洗后的 time/pressure。
/// 仅数值重合的首尾可共享平滑切线，不自动封闭近邻端点或消除闭合处尖角。
/// O(输入+输出)，最多 MAX_POINTS 点；不保证反复处理幂等，已重采样数据应直接渲染。
pub fn smooth_resample(
    points: &[StrokePoint],
    spacing: f32,
    strength: f32,
    corner_degrees: f32,
) -> Result<Vec<StrokePoint>, InkError> {
    if !spacing.is_finite()
        || spacing <= 0.0
        || !strength.is_finite()
        || !(0.0..=1.0).contains(&strength)
        || !corner_degrees.is_finite()
        || !(0.0..=180.0).contains(&corner_degrees)
    {
        return Err(InkError::InvalidInput);
    }
    let clean = clean_stroke(points)?;
    let n = clean.len();
    if n < 2 {
        return Ok(clean);
    }
    let mut turns = vec![0.0; n];
    for i in 1..n - 1 {
        turns[i] = curve_turn(
            position(&clean[i - 1]),
            position(&clean[i]),
            position(&clean[i + 1]),
        );
    }
    let closed = n >= 4
        && distance(position(&clean[0]), position(&clean[n - 1]))
            <= 1e-6
                * distance(position(&clean[0]), position(&clean[1]))
                    .min(distance(position(&clean[n - 2]), position(&clean[n - 1])));
    if closed {
        turns[0] = curve_turn(
            position(&clean[n - 2]),
            position(&clean[0]),
            position(&clean[1]),
        );
        turns[n - 1] = turns[0];
    }
    let mut smooth = clean.clone();
    for i in 2..n.saturating_sub(2) {
        let a = position(&clean[i - 1]);
        let b = position(&clean[i]);
        let c = position(&clean[i + 1]);
        let left = distance(a, b);
        let right = distance(b, c);
        let target = lerp(a, c, left / (left + right));
        // 只削弱密集的交替高频偏移；同向曲率（包括再次输入的圆弧）不做均值收缩。
        if turns[i].abs().to_degrees() < (corner_degrees as f64).min(80.0)
            && turns[i] * turns[i - 1] < 0.0
            && turns[i] * turns[i + 1] < 0.0
            && left.max(right) <= 4.0 * spacing as f64
            && distance(b, target) <= 2.0 * spacing as f64
        {
            let p = lerp(b, target, strength as f64 * 0.5);
            smooth[i].x = p.x;
            smooth[i].y = p.y;
        }
    }
    let mut tangents = vec![None; n];
    let blend = 1.0 - (1.0 - strength as f64).powi(3);
    if blend > 0.0 && corner_degrees > 0.0 {
        for i in 0..n {
            if !closed && (i == 0 || i == n - 1) {
                continue;
            }
            let before = if i == 0 { n - 2 } else { i - 1 };
            let after = if i == n - 1 { 1 } else { i + 1 };
            let angle = turns[i].abs().to_degrees();
            let coherent = [turns[before], turns[after]].iter().any(|neighbor| {
                turns[i] * neighbor > 0.0
                    && neighbor.abs() >= turns[i].abs() * 0.4
                    && neighbor.abs() <= turns[i].abs() * 2.5
            });
            if angle < 80.0 && (angle < corner_degrees as f64 || coherent) {
                tangents[i] = Some(curve_derivative(
                    position(&smooth[before]),
                    position(&smooth[i]),
                    position(&smooth[after]),
                ));
            }
        }
        // 单边二阶端切线；相邻点为尖角时沿端弦，不能借尖角另一边改变形状。
        if !closed && n >= 3 {
            for (end, neighbor) in [(0, 1), (n - 1, n - 2)] {
                if let Some(t) = tangents[neighbor] {
                    let (a, b) = if end == 0 { (0, 1) } else { (n - 2, n - 1) };
                    let length = distance(position(&smooth[a]), position(&smooth[b]));
                    if length > 0.0 {
                        tangents[end] = Some((
                            2.0 * (smooth[b].x as f64 - smooth[a].x as f64) / length - t.0,
                            2.0 * (smooth[b].y as f64 - smooth[a].y as f64) / length - t.1,
                        ));
                    }
                }
            }
        }
    }
    let mut output = Vec::with_capacity(n);
    output.push(smooth[0]);
    let mut work = 1;
    for (index, pair) in smooth.windows(2).enumerate() {
        let a = position(&pair[0]);
        let b = position(&pair[1]);
        let length = distance(a, b);
        if length == 0.0 {
            *output.last_mut().unwrap() = pair[1];
            continue;
        }
        let chord = (b.x as f64 - a.x as f64, b.y as f64 - a.y as f64);
        let tangent = |i: usize| {
            let Some(t) = tangents[i] else {
                return chord;
            };
            let t = limited_tangent((t.0 * length, t.1 * length), chord, length);
            (
                chord.0 + blend * (t.0 - chord.0),
                chord.1 + blend * (t.1 - chord.1),
            )
        };
        let m0 = tangent(index);
        let m1 = tangent(index + 1);
        // Bezier 导数是三个控制边的凸组合，最大范数给出可靠的段长上界。
        let bound =
            m0.0.hypot(m0.1)
                .max(m1.0.hypot(m1.1))
                .max((3.0 * chord.0 - m0.0 - m1.0).hypot(3.0 * chord.1 - m0.1 - m1.1));
        let steps = (bound / spacing as f64).ceil().max(1.0);
        if steps > (MAX_POINTS - work) as f64 {
            return Err(InkError::TooManyPoints);
        }
        work += steps as usize;
        for i in 1..=steps as usize {
            let t = i as f64 / steps;
            let mut sample = interpolate(&pair[0], &pair[1], t);
            if i != steps as usize {
                let h0 = t * (1.0 - t) * (1.0 - t);
                let h1 = -t * t * (1.0 - t);
                sample.x =
                    (a.x as f64 + chord.0 * t + h0 * (m0.0 - chord.0) + h1 * (m1.0 - chord.0))
                        .clamp(-MAX_COORD as f64, MAX_COORD as f64) as f32;
                sample.y =
                    (a.y as f64 + chord.1 * t + h0 * (m0.1 - chord.1) + h1 * (m1.1 - chord.1))
                        .clamp(-MAX_COORD as f64, MAX_COORD as f64) as f32;
            }
            let last = output.last_mut().unwrap();
            if position(last) != position(&sample) {
                output.push(sample);
            } else if output.len() > 1 {
                // f32 量化重合时保留较晚元数据，首点除外。
                *output.last_mut().unwrap() = sample;
            }
        }
    }
    Ok(output)
}

/// 返回与输入一一对应的宽度；无压感设备传 pressure=1。速度越快越细。
/// speed_scale 是宽度明显衰减时的坐标单位/秒，pressure_weight 范围 0..=1。
pub fn stroke_widths(
    points: &[StrokePoint],
    style: &Style,
    speed_scale: f32,
    pressure_weight: f32,
) -> Result<Vec<f32>, InkError> {
    check_stroke(points)?;
    if !style.width.is_finite()
        || !(0.0..=10_000.0).contains(&style.width)
        || style.width == 0.0
        || !speed_scale.is_finite()
        || speed_scale <= 0.0
        || !pressure_weight.is_finite()
        || !(0.0..=1.0).contains(&pressure_weight)
    {
        return Err(InkError::InvalidInput);
    }
    let mut previous_time = points.first().map_or(0.0, |p| p.time);
    let mut previous_speed = 0.0;
    let mut result = Vec::with_capacity(points.len());
    for (i, p) in points.iter().enumerate() {
        let time = p.time.max(previous_time);
        let dt = time - previous_time;
        let speed = if i == 0 {
            0.0
        } else if dt > 1e-6 {
            distance(position(&points[i - 1]), position(p)) / dt
        } else {
            previous_speed
        };
        let velocity = 0.25 + 0.75 / (1.0 + speed / speed_scale as f64);
        let pressure = 1.0 - pressure_weight as f64
            + pressure_weight as f64 * (0.2 + 0.8 * p.pressure.clamp(0.0, 1.0) as f64);
        result.push((style.width as f64 * velocity * pressure).max(0.01) as f32);
        previous_time = time;
        previous_speed = speed;
    }
    Ok(result)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projection {
    pub point: Point,
    pub t: f32,
    pub distance: f32,
}

fn project(p: Point, a: Point, b: Point, segment: bool) -> Projection {
    let dx = b.x as f64 - a.x as f64;
    let dy = b.y as f64 - a.y as f64;
    let length2 = dx * dx + dy * dy;
    let mut t = if length2 == 0.0 {
        0.0
    } else {
        ((p.x as f64 - a.x as f64) * dx + (p.y as f64 - a.y as f64) * dy) / length2
    };
    if segment {
        t = t.clamp(0.0, 1.0);
    }
    let point = lerp(a, b, t);
    Projection {
        point,
        t: t as f32,
        distance: distance(p, point) as f32,
    }
}

pub fn project_to_segment(p: Point, a: Point, b: Point) -> Result<Projection, InkError> {
    check_points(&[p, a, b])?;
    Ok(project(p, a, b, true))
}

pub fn point_segment_distance(p: Point, a: Point, b: Point) -> Result<f32, InkError> {
    Ok(project_to_segment(p, a, b)?.distance)
}

/// 退化直线按单点处理。
pub fn point_line_distance(p: Point, a: Point, b: Point) -> Result<f32, InkError> {
    check_points(&[p, a, b])?;
    Ok(project(p, a, b, false).distance)
}

/// 点到折线笔迹中心线的最短距离，空笔迹返回 None。
pub fn point_stroke_distance(p: Point, stroke: &[StrokePoint]) -> Result<Option<f32>, InkError> {
    check_points(&[p])?;
    check_stroke(stroke)?;
    if stroke.is_empty() {
        return Ok(None);
    }
    let mut best = distance(p, position(&stroke[0])) as f32;
    for pair in stroke.windows(2) {
        best = best.min(project(p, position(&pair[0]), position(&pair[1]), true).distance);
    }
    Ok(Some(best))
}

// 交点写回 f32 后允许一个量化误差带；容差随局部坐标精度缩放。
fn coordinate_resolution(p: Point) -> f64 {
    let x = (p.x.next_up() as f64 - p.x as f64).abs();
    let y = (p.y.next_up() as f64 - p.y as f64).abs();
    x.hypot(y)
}

// 胶囊由两个端圆和中间矩形组成；求与笔迹线段相交的参数区间，而非仅检测采样点。
fn disk_interval(a: Point, b: Point, center: Point, radius: f64) -> Option<(f64, f64)> {
    let dx = b.x as f64 - a.x as f64;
    let dy = b.y as f64 - a.y as f64;
    let ox = a.x as f64 - center.x as f64;
    let oy = a.y as f64 - center.y as f64;
    let length = dx.hypot(dy);
    if length == 0.0 {
        return (ox.hypot(oy) <= radius).then_some((0.0, 1.0));
    }
    // 用垂距求半弦长，避免大坐标二次方判别式相减吞掉小半径。
    let perpendicular = (ox * dy - oy * dx).abs() / length;
    if perpendicular > radius {
        return None;
    }
    let center_t = -(ox * dx + oy * dy) / (length * length);
    let half = ((radius - perpendicular) * (radius + perpendicular)).sqrt() / length;
    let lo = (center_t - half).max(0.0);
    let hi = (center_t + half).min(1.0);
    (lo <= hi).then_some((lo, hi))
}

fn capsule_intervals(
    a: Point,
    b: Point,
    c: Point,
    d: Point,
    radius: f64,
    out: &mut Vec<(f64, f64)>,
) {
    let base = out.len();
    raw_capsule_intervals(a, b, c, d, radius, out);
    if a == b || out.len() == base {
        return;
    }
    let dx = d.x as f64 - c.x as f64;
    let dy = d.y as f64 - c.y as f64;
    let length2 = dx * dx + dy * dy;
    let boundary = |p: Point| {
        let px = p.x as f64 - c.x as f64;
        let py = p.y as f64 - c.y as f64;
        let t = if length2 == 0.0 {
            0.0
        } else {
            ((px * dx + py * dy) / length2).clamp(0.0, 1.0)
        };
        let nx = px - dx * t;
        let ny = py - dy * t;
        let separation = nx.hypot(ny);
        let near = separation > 0.0 && (separation - radius).abs() <= coordinate_resolution(p);
        let direction = nx * (b.x as f64 - a.x as f64) + ny * (b.y as f64 - a.y as f64);
        (near, direction)
    };
    let (a_near, a_direction) = boundary(a);
    let (b_near, b_direction) = boundary(b);
    // 对胶囊距离是凸函数；从边界向外行进时，不应再次裁掉舍入误差带。
    if (a_near && a_direction >= 0.0) || (b_near && b_direction <= 0.0) {
        out.truncate(base);
    }
}

fn raw_capsule_intervals(
    a: Point,
    b: Point,
    c: Point,
    d: Point,
    radius: f64,
    out: &mut Vec<(f64, f64)>,
) {
    if let Some(interval) = disk_interval(a, b, c, radius) {
        out.push(interval);
    }
    let length = distance(c, d);
    if length == 0.0 {
        return;
    }
    if let Some(interval) = disk_interval(a, b, d, radius) {
        out.push(interval);
    }
    let ux = (d.x as f64 - c.x as f64) / length;
    let uy = (d.y as f64 - c.y as f64) / length;
    let ax = a.x as f64 - c.x as f64;
    let ay = a.y as f64 - c.y as f64;
    let dx = b.x as f64 - a.x as f64;
    let dy = b.y as f64 - a.y as f64;
    let mut lo: f64 = 0.0;
    let mut hi: f64 = 1.0;
    for (origin, delta, min, max) in [
        (ax * ux + ay * uy, dx * ux + dy * uy, 0.0, length),
        (-ax * uy + ay * ux, -dx * uy + dy * ux, -radius, radius),
    ] {
        if delta == 0.0 {
            if origin < min || origin > max {
                return;
            }
        } else {
            let t1 = (min - origin) / delta;
            let t2 = (max - origin) / delta;
            lo = lo.max(t1.min(t2));
            hi = hi.min(t1.max(t2));
            if lo > hi {
                return;
            }
        }
    }
    out.push((lo, hi));
}

fn erase_intervals(a: Point, b: Point, path: &[Point], radius: f32) -> Vec<(f64, f64)> {
    let mut intervals = Vec::new();
    if path.len() == 1 {
        capsule_intervals(a, b, path[0], path[0], radius as f64, &mut intervals);
    }
    for pair in path.windows(2) {
        capsule_intervals(a, b, pair[0], pair[1], radius as f64, &mut intervals);
    }
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (lo, hi) in intervals {
        if let Some(last) = merged.last_mut()
            && lo <= last.1 + EPS
        {
            last.1 = last.1.max(hi);
            continue;
        }
        merged.push((lo, hi));
    }
    merged
}

fn check_eraser(stroke: &[StrokePoint], path: &[Point], radius: f32) -> Result<(), InkError> {
    check_stroke(stroke)?;
    check_points(path)?;
    if !radius.is_finite() || radius <= 0.0 || radius > MAX_COORD {
        return Err(InkError::InvalidInput);
    }
    if stroke.len().saturating_mul(path.len()) > 2_000_000 {
        return Err(InkError::TooManyPoints);
    }
    Ok(())
}

/// radius 为有效擦除半径，渲染有宽度时由调用方加上半笔宽。
/// path 的相邻点构成连续扫过的胶囊，即使采样稀疏也不会漏擦。
/// 非退化线段仅相切不算命中；边界单点仍可擦除，与片段裁剪语义一致。
pub fn stroke_hit(stroke: &[StrokePoint], path: &[Point], radius: f32) -> Result<bool, InkError> {
    check_eraser(stroke, path, radius)?;
    if stroke.len() == 1 {
        return Ok(
            !erase_intervals(position(&stroke[0]), position(&stroke[0]), path, radius).is_empty(),
        );
    }
    Ok(stroke.windows(2).any(|p| {
        let a = position(&p[0]);
        let b = position(&p[1]);
        erase_intervals(a, b, path, radius)
            .iter()
            .any(|(lo, hi)| a == b || hi - lo > EPS)
    }))
}

/// 返回擦除后的独立片段，交点插值保留 time/pressure；不会把间隙重新连接。
pub fn erase_stroke(
    stroke: &[StrokePoint],
    path: &[Point],
    radius: f32,
) -> Result<Vec<Vec<StrokePoint>>, InkError> {
    check_eraser(stroke, path, radius)?;
    let clean = clean_stroke(stroke)?;
    if clean.is_empty() {
        return Ok(Vec::new());
    }
    if path.is_empty() {
        return Ok(vec![clean]);
    }
    if clean.len() == 1 {
        return Ok(if stroke_hit(&clean, path, radius)? {
            vec![]
        } else {
            vec![clean]
        });
    }
    let mut result = Vec::new();
    let mut current: Vec<StrokePoint> = Vec::new();
    let mut count = 0;
    for pair in clean.windows(2) {
        let intervals = erase_intervals(position(&pair[0]), position(&pair[1]), path, radius);
        let mut start = 0.0;
        let mut visible = Vec::new();
        for (lo, hi) in intervals {
            // 相切不切出零长片段。
            if hi - lo <= EPS {
                continue;
            }
            if lo > start + EPS {
                visible.push((start, lo));
            }
            start = start.max(hi);
        }
        if start < 1.0 - EPS {
            visible.push((start, 1.0));
        }
        for (lo, hi) in visible {
            if lo > EPS && !current.is_empty() {
                result.push(std::mem::take(&mut current));
            }
            let begin = interpolate(&pair[0], &pair[1], lo);
            let end = interpolate(&pair[0], &pair[1], hi);
            if current
                .last()
                .is_none_or(|p| position(p) != position(&begin))
            {
                current.push(begin);
                count += 1;
            }
            current.push(end);
            count += 1;
            if count > MAX_POINTS {
                return Err(InkError::TooManyPoints);
            }
            if hi < 1.0 - EPS {
                result.push(std::mem::take(&mut current));
            }
        }
        if start >= 1.0 - EPS && !current.is_empty() {
            result.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    // 相邻交点可能量化为同一点，首次输出即归一化，避免二次擦除改变结构。
    for piece in &mut result {
        piece.dedup_by(|later, earlier| {
            if position(later) == position(earlier) {
                *earlier = *later;
                true
            } else {
                false
            }
        });
    }
    result.retain(|piece| {
        piece.len() > 1
            || erase_intervals(position(&piece[0]), position(&piece[0]), path, radius).is_empty()
    });
    Ok(result)
}

/// 吸附到 0、30、45、60、90 度及其对称角，保持线段长度；容差以度计。
pub fn snap_angle(anchor: Point, target: Point, tolerance_degrees: f32) -> Result<Point, InkError> {
    check_points(&[anchor, target])?;
    if !tolerance_degrees.is_finite() || !(0.0..=45.0).contains(&tolerance_degrees) {
        return Err(InkError::InvalidInput);
    }
    let length = distance(anchor, target);
    if length <= EPS {
        return Ok(target);
    }
    let angle = (target.y as f64 - anchor.y as f64).atan2(target.x as f64 - anchor.x as f64);
    let mut best = angle;
    let mut difference = f64::INFINITY;
    for degrees in [
        0.0_f64, 30.0, 45.0, 60.0, 90.0, 120.0, 135.0, 150.0, 180.0, 210.0, 225.0, 240.0, 270.0,
        300.0, 315.0, 330.0,
    ] {
        let candidate = degrees.to_radians();
        let delta = (candidate - angle + PI).rem_euclid(2.0 * PI) - PI;
        if delta.abs() < difference {
            difference = delta.abs();
            best = candidate;
        }
    }
    if difference > (tolerance_degrees as f64).to_radians() {
        return Ok(target);
    }
    let p = Point {
        x: (anchor.x as f64 + length * best.cos()) as f32,
        y: (anchor.y as f64 + length * best.sin()) as f32,
    };
    if !valid_point(p) {
        return Err(InkError::InvalidInput);
    }
    Ok(p)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dimension {
    TwoD,
    ThreeD,
}

/// 直线不自带维度，连接后继承所在连通分量的维度。
pub fn shape_dimension(kind: ShapeKind) -> Option<Dimension> {
    use ShapeKind::*;
    match kind {
        Line => None,
        Cube | Cuboid | Cylinder | Cone | Sphere => Some(Dimension::ThreeD),
        _ => Some(Dimension::TwoD),
    }
}

/// 必须传入连接合并后的整个连通分量（包括经由其他线间接相连的形状）。
pub fn connections_compatible(kinds: &[ShapeKind]) -> bool {
    let mut dimension = None;
    for kind in kinds {
        if let Some(next) = shape_dimension(*kind) {
            if dimension.is_some_and(|current| current != next) {
                return false;
            }
            dimension = Some(next);
        }
    }
    true
}

#[derive(Clone, Debug)]
pub struct ShapeGeometry {
    pub kind: ShapeKind,
    pub vertices: Vec<Point>,
    pub edges: Vec<[usize; 2]>,
}

fn polygon(geometry: &mut ShapeGeometry, points: &[Point], closed: bool) {
    let base = geometry.vertices.len();
    geometry.vertices.extend_from_slice(points);
    for i in 1..points.len() {
        geometry.edges.push([base + i - 1, base + i]);
    }
    if closed && points.len() > 2 {
        geometry.edges.push([base + points.len() - 1, base]);
    }
}

fn ellipse(geometry: &mut ShapeGeometry, cx: f64, cy: f64, rx: f64, ry: f64) {
    let points: Vec<_> = (0..64)
        .map(|i| {
            let angle = i as f64 * PI / 32.0;
            Point {
                x: (cx + rx * angle.cos()) as f32,
                y: (cy + ry * angle.sin()) as f32,
            }
        })
        .collect();
    polygon(geometry, &points, true);
}

/// start/end 为拖拽包围框；正方形、圆、立方体使用较小边长。
/// 3D 图形输出屏幕空间线框，所有曲线离散为 64 段闭合折线。
pub fn shape_geometry(
    kind: ShapeKind,
    start: Point,
    end: Point,
) -> Result<ShapeGeometry, InkError> {
    check_points(&[start, end])?;
    let mut geometry = ShapeGeometry {
        kind,
        vertices: vec![],
        edges: vec![],
    };
    use ShapeKind::*;
    if kind == Line {
        polygon(&mut geometry, &[start, end], false);
        return Ok(geometry);
    }
    let mut w = (end.x as f64 - start.x as f64).abs();
    let mut h = (end.y as f64 - start.y as f64).abs();
    if matches!(kind, Square | Circle | Cube | Sphere) {
        w = w.min(h);
        h = w;
    }
    if kind == EquilateralTriangle {
        w = w.min(h * 2.0 / 3.0_f64.sqrt());
        h = w * 3.0_f64.sqrt() / 2.0;
    }
    // 先约束尺寸再定位反向拖拽的包围框，起点始终固定，形状不倒置。
    let x = if end.x < start.x {
        start.x as f64 - w
    } else {
        start.x as f64
    };
    let y = if end.y < start.y {
        start.y as f64 - h
    } else {
        start.y as f64
    };
    let p = |u: f64, v: f64| Point {
        x: (x + w * u) as f32,
        y: (y + h * v) as f32,
    };
    match kind {
        Line => unreachable!(),
        Rectangle | Square => polygon(
            &mut geometry,
            &[p(0.0, 0.0), p(1.0, 0.0), p(1.0, 1.0), p(0.0, 1.0)],
            true,
        ),
        Triangle | EquilateralTriangle => polygon(
            &mut geometry,
            &[p(0.5, 0.0), p(1.0, 1.0), p(0.0, 1.0)],
            true,
        ),
        RightTriangle => polygon(
            &mut geometry,
            &[p(0.0, 0.0), p(1.0, 1.0), p(0.0, 1.0)],
            true,
        ),
        Parallelogram => polygon(
            &mut geometry,
            &[p(0.25, 0.0), p(1.0, 0.0), p(0.75, 1.0), p(0.0, 1.0)],
            true,
        ),
        Rhombus => polygon(
            &mut geometry,
            &[p(0.5, 0.0), p(1.0, 0.5), p(0.5, 1.0), p(0.0, 0.5)],
            true,
        ),
        Ellipse | Circle => ellipse(&mut geometry, x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0),
        Cube | Cuboid => {
            polygon(
                &mut geometry,
                &[p(0.0, 0.25), p(0.75, 0.25), p(0.75, 1.0), p(0.0, 1.0)],
                true,
            );
            polygon(
                &mut geometry,
                &[p(0.25, 0.0), p(1.0, 0.0), p(1.0, 0.75), p(0.25, 0.75)],
                true,
            );
            for i in 0..4 {
                geometry.edges.push([i, i + 4]);
            }
        }
        Cylinder => {
            ellipse(&mut geometry, x + w / 2.0, y + h * 0.15, w / 2.0, h * 0.15);
            ellipse(&mut geometry, x + w / 2.0, y + h * 0.85, w / 2.0, h * 0.15);
            geometry.edges.extend([[0, 64], [32, 96]]);
        }
        Cone => {
            ellipse(&mut geometry, x + w / 2.0, y + h * 0.85, w / 2.0, h * 0.15);
            geometry.vertices.push(p(0.5, 0.0));
            geometry.edges.extend([[64, 0], [64, 32]]);
        }
        Sphere => {
            ellipse(&mut geometry, x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
            ellipse(&mut geometry, x + w / 2.0, y + h / 2.0, w / 2.0, h * 0.15);
            ellipse(&mut geometry, x + w / 2.0, y + h / 2.0, w * 0.15, h / 2.0);
        }
    }
    check_points(&geometry.vertices)?;
    Ok(geometry)
}

pub fn supports_vertex_edit(kind: ShapeKind) -> bool {
    !matches!(
        kind,
        ShapeKind::Ellipse
            | ShapeKind::Circle
            | ShapeKind::Cylinder
            | ShapeKind::Cone
            | ShapeKind::Sphere
    )
}

/// 自由顶点微调，不维持等边/直角等约束；圆弧应修改包围框后重新生成。
pub fn edit_vertex(
    geometry: &mut ShapeGeometry,
    index: usize,
    target: Point,
) -> Result<(), InkError> {
    check_points(&[target])?;
    if !supports_vertex_edit(geometry.kind) {
        return Err(InkError::UnsupportedVertexEdit);
    }
    let vertex = geometry
        .vertices
        .get_mut(index)
        .ok_or(InkError::InvalidVertex)?;
    *vertex = target;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionSite {
    Vertex(usize),
    Edge(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Connection {
    pub site: ConnectionSite,
    pub point: Point,
    pub t: f32,
    pub distance: f32,
}

/// component_dimension 必须来自待连接线所在的整个连通分量，防止间接混连。
/// 容差内优先顶点，否则取最近线段投影；维度不兼容时返回 None。
pub fn nearest_connection(
    geometry: &ShapeGeometry,
    query: Point,
    tolerance: f32,
    component_dimension: Option<Dimension>,
) -> Result<Option<Connection>, InkError> {
    check_points(&geometry.vertices)?;
    check_points(&[query])?;
    if geometry.edges.len() > MAX_POINTS {
        return Err(InkError::TooManyPoints);
    }
    if !tolerance.is_finite() || !(0.0..=MAX_COORD).contains(&tolerance) {
        return Err(InkError::InvalidInput);
    }
    if geometry
        .edges
        .iter()
        .flatten()
        .any(|i| *i >= geometry.vertices.len())
    {
        return Err(InkError::InvalidVertex);
    }
    if let (Some(a), Some(b)) = (shape_dimension(geometry.kind), component_dimension)
        && a != b
    {
        return Ok(None);
    }
    let mut best: Option<Connection> = None;
    for (i, p) in geometry.vertices.iter().enumerate() {
        let d = distance(query, *p) as f32;
        if d <= tolerance && best.is_none_or(|b| d < b.distance) {
            best = Some(Connection {
                site: ConnectionSite::Vertex(i),
                point: *p,
                t: 0.0,
                distance: d,
            });
        }
    }
    if best.is_some() {
        return Ok(best);
    }
    for (i, edge) in geometry.edges.iter().enumerate() {
        let p = project(
            query,
            geometry.vertices[edge[0]],
            geometry.vertices[edge[1]],
            true,
        );
        if p.distance <= tolerance && best.is_none_or(|b| p.distance < b.distance) {
            best = Some(Connection {
                site: ConnectionSite::Edge(i),
                point: p.point,
                t: p.t,
                distance: p.distance,
            });
        }
    }
    Ok(best)
}

#[cfg(test)]
mod tests;
