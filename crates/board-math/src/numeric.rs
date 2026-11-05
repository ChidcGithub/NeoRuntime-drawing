use super::{Expr, MathError, Result, SystemSolutions, algebra, checked_ratio, finite, parse};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
#[derive(Debug, Clone, Copy)]
pub struct NumericOptions {
    /// 网格分段数，8..=4096。
    pub steps: usize,
    /// 网格幅值归一化后的残差/积分误差容限，1e-12..=1e-3；不是严格误差证明。
    pub tolerance: f64,
    /// 单次调用最多求值次数，32..=100000；另有 500 万词元计算预算。
    pub max_evaluations: usize,
}
impl Default for NumericOptions {
    fn default() -> Self {
        Self {
            steps: 512,
            tolerance: 1e-8,
            max_evaluations: 30000,
        }
    }
}
impl NumericOptions {
    pub(crate) fn validate(self) -> Result<Self> {
        if !(8..=4096).contains(&self.steps)
            || !(32..=100000).contains(&self.max_evaluations)
            || !self.tolerance.is_finite()
            || !(1e-12..=1e-3).contains(&self.tolerance)
        {
            return Err(MathError::Limit(
                "数值参数要求：网格 8..4096，容限 1e-12..1e-3，求值预算 32..100000".into(),
            ));
        }
        Ok(self)
    }
}
pub(crate) fn interval(a: f64, b: f64) -> Result<()> {
    if !a.is_finite() || !b.is_finite() || a >= b || a.abs() > 1e6 || b.abs() > 1e6 {
        return Err(MathError::Domain(
            "搜索/积分区间须递增且位于 [-1000000,1000000]".into(),
        ));
    }
    Ok(())
}
struct Budget {
    left: usize,
}
impl Budget {
    fn new(o: NumericOptions, size: usize) -> Self {
        Self {
            left: o.max_evaluations.min(5_000_000 / size.max(1)),
        }
    }
    fn take(&mut self) -> Result<()> {
        if self.left == 0 {
            return Err(MathError::Limit(
                "数值计算预算已耗尽，请缩小范围或减少网格".into(),
            ));
        }
        self.left -= 1;
        Ok(())
    }
}
struct Function1 {
    first: Expr,
    second: Option<Expr>,
    budget: Budget,
    scale: f64,
}
impl Function1 {
    fn new(first: Expr, second: Option<Expr>, o: NumericOptions) -> Self {
        let size = first.size + second.as_ref().map_or(0, |e| e.size);
        Self {
            first,
            second,
            budget: Budget::new(o, size),
            scale: 1.0,
        }
    }
    fn eval(&mut self, x: f64) -> Result<f64> {
        self.budget.take()?;
        let a = self.first.eval(Some(x), None)?;
        checked_ratio(
            finite(
                a - match &self.second {
                    Some(e) => e.eval(Some(x), None)?,
                    None => 0.0,
                },
            )?,
            self.scale,
        )
    }
    fn optional(&mut self, x: f64) -> Result<Option<f64>> {
        match self.eval(x) {
            Ok(v) => Ok(Some(v)),
            Err(MathError::Domain(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
/// 中心差分及半步长校验，O(n) 且最多 5 次求值；检测明显尖点，不证明可微性。
pub fn derivative(input: &str, x: f64) -> Result<f64> {
    if !x.is_finite() || x.abs() > 1e6 {
        return Err(MathError::Domain("微分位置须在 [-1000000,1000000]".into()));
    }
    let e = parse(input)?;
    let f = |v| e.eval(Some(v), None);
    // 大横坐标不意味着函数变化缓慢；二进制步长避免 x±h 的额外舍入。
    let local = if x == 0.0 { 1.0 } else { x.abs().min(1.0) };
    let h = 2.0_f64.powf((1e-4 * local).log2().floor());
    if h == 0.0 || x + h / 2.0 == x || x - h / 2.0 == x {
        return Err(MathError::Numerical("差分步长无法表示".into()));
    }
    let mut v = [
        f(x)?,
        f(x - h)?,
        f(x + h)?,
        f(x - h / 2.0)?,
        f(x + h / 2.0)?,
    ];
    let scale = v.iter().fold(0.0_f64, |a, b| a.max(b.abs()));
    if scale == 0.0 {
        return Ok(0.0);
    }
    for value in &mut v {
        *value = checked_ratio(*value, scale)?;
    }
    let [center, left, right, left_half, right_half] = v;
    let dl = center - left;
    let dr = right - center;
    let dl2 = 2.0 * (center - left_half);
    let dr2 = 2.0 * (right_half - center);
    let jump = (dr - dl).abs();
    let jump2 = (dr2 - dl2).abs();
    let slope_scale = dl.abs().max(dr.abs()).max(dl2.abs()).max(dr2.abs());
    let noise = 32.0 * f64::EPSILON;
    if jump2 > (1e-5 * slope_scale).max(noise) && jump2 > 0.8 * jump {
        return Err(MathError::Numerical("该点可能不可微或差分未收敛".into()));
    }
    let d1 = (right - left) / 2.0;
    let d2 = right_half - left_half;
    let extrapolated = (4.0 * d2 - d1) / 3.0;
    if (d2 - d1).abs() > (1e-5 * slope_scale).max(noise) && extrapolated.abs() > noise {
        return Err(MathError::Numerical("差分未收敛".into()));
    }
    finite((extrapolated / h) * scale)
}
/// 自适应 Simpson，O(预算*n)，深度最多 20；仅普通定积分，不计算主值/反常积分。
/// 有限采样不保证发现窄尖峰或所有奇点；遇到定义域错误、深度/预算耗尽会失败。
pub fn integrate(input: &str, a: f64, b: f64, options: NumericOptions) -> Result<f64> {
    let o = options.validate()?;
    finite(a)?;
    finite(b)?;
    if a == b {
        if a.abs() > 1e6 {
            return Err(MathError::Domain("积分边界超限".into()));
        }
        parse(input)?.eval(Some(a), None)?;
        return Ok(0.0);
    }
    let (lo, hi, sign) = if a < b { (a, b, 1.0) } else { (b, a, -1.0) };
    interval(lo, hi)?;
    let mut f = Function1::new(parse(input)?, None, o);
    let mut total = 0.0;
    let mut grid = Vec::with_capacity(o.steps);
    let mut scale = 0.0_f64;
    // 同一网格同时提供幅值基准和预分段，不因整体缩放而放过奇点。
    for i in 0..o.steps {
        let l = lo + (hi - lo) * i as f64 / o.steps as f64;
        let r = lo + (hi - lo) * (i + 1) as f64 / o.steps as f64;
        let m = l + (r - l) / 2.0;
        let values = [f.eval(l)?, f.eval(m)?, f.eval(r)?];
        for v in values {
            scale = scale.max(v.abs());
        }
        grid.push((l, r, values));
    }
    f.scale = if scale == 0.0 { 1.0 } else { scale };
    for (l, r, mut values) in grid {
        for v in &mut values {
            *v = checked_ratio(*v, f.scale)?;
        }
        let whole = simpson(l, r, values)?;
        total = finite(
            total + adaptive(&mut f, l, r, values, whole, o.tolerance / o.steps as f64, 0)?,
        )?;
    }
    super::nonzero_result(sign * total * f.scale, total != 0.0)
}
fn simpson(a: f64, b: f64, v: [f64; 3]) -> Result<f64> {
    finite((b - a) * (v[0] / 6.0 + v[1] * (2.0 / 3.0) + v[2] / 6.0))
}
fn adaptive(
    f: &mut Function1,
    a: f64,
    b: f64,
    v: [f64; 3],
    whole: f64,
    tol: f64,
    depth: usize,
) -> Result<f64> {
    let m = a + (b - a) / 2.0;
    let l = f.eval(a + (m - a) / 2.0)?;
    let r = f.eval(m + (b - m) / 2.0)?;
    let lv = [v[0], l, v[1]];
    let rv = [v[1], r, v[2]];
    let ls = simpson(a, m, lv)?;
    let rs = simpson(m, b, rv)?;
    let delta = finite(ls + rs - whole)?;
    if delta.abs() <= 15.0 * tol {
        return finite(ls + rs + delta / 15.0);
    }
    if depth >= 20 || m == a || m == b {
        return Err(MathError::Numerical("积分未收敛，可能存在奇点".into()));
    }
    finite(
        adaptive(f, a, m, lv, ls, tol / 2.0, depth + 1)?
            + adaptive(f, m, b, rv, rs, tol / 2.0, depth + 1)?,
    )
}
#[derive(Debug, Clone)]
pub struct SampledCurve {
    pub segments: Vec<Vec<Point>>,
    /// 被舍弃的网格连接数量；不等于已证明的不连续点数。
    pub skipped_intervals: usize,
    pub complete: bool,
}
/// 每格检查端点、中点和四分点；偏離弦线过大则断开连接，O(steps*n)。
/// 这是保守绘图启发式，complete 恒为 false（不承诺识别全部不连续点）。
pub fn sample(input: &str, a: f64, b: f64, options: NumericOptions) -> Result<SampledCurve> {
    let o = options.validate()?;
    interval(a, b)?;
    let mut f = Function1::new(parse(input)?, None, o);
    let mut segments = Vec::new();
    let mut current = Vec::new();
    let mut skipped = 0;
    let mut left = f.optional(a)?;
    for i in 0..o.steps {
        let x = a + (b - a) * i as f64 / o.steps as f64;
        let right_x = a + (b - a) * (i + 1) as f64 / o.steps as f64;
        let right = f.optional(right_x)?;
        let probes = [
            f.optional(x + (right_x - x) * 0.25)?,
            f.optional(x + (right_x - x) * 0.5)?,
            f.optional(x + (right_x - x) * 0.75)?,
        ];
        let safe = match (left, right, probes) {
            (Some(l), Some(r), [Some(q), Some(m), Some(t)]) => {
                let scale = 1.0 + l.abs().min(r.abs());
                (q - (l * 0.75 + r * 0.25)).abs() < 0.1 * scale
                    && (m - (l * 0.5 + r * 0.5)).abs() < 0.1 * scale
                    && (t - (l * 0.25 + r * 0.75)).abs() < 0.1 * scale
            }
            _ => false,
        };
        if safe {
            if current.is_empty() {
                current.push(Point {
                    x,
                    y: left.unwrap(),
                });
            }
            current.push(Point {
                x: right_x,
                y: right.unwrap(),
            });
        } else {
            skipped += 1;
            if !current.is_empty() {
                segments.push(std::mem::take(&mut current));
            }
        }
        left = right;
    }
    if !current.is_empty() {
        segments.push(current);
    }
    Ok(SampledCurve {
        segments,
        skipped_intervals: skipped,
        complete: false,
    })
}
#[derive(Debug, Clone)]
pub struct RootSearch {
    pub roots: Vec<f64>,
    pub interval: (f64, f64),
    pub complete: bool,
}
fn add_root(values: &mut Vec<f64>, x: f64, tol: f64) {
    if !values.iter().any(|v| (v - x).abs() <= tol) {
        values.push(x);
    }
}
/// 网格括根 + 二分（64 次）+ 局部极小值处有界 Newton（32 次）。
/// O(steps*96*n)，受总预算约束；网格幅值归一化残差 <= tolerance 的候选才会返回。
/// 偶重根、密集根可能漏检；恒零函数不枚举，complete 恒为 false。
pub fn roots(input: &str, a: f64, b: f64, options: NumericOptions) -> Result<RootSearch> {
    let o = options.validate()?;
    interval(a, b)?;
    search_roots(&mut Function1::new(parse(input)?, None, o), a, b, o)
}
fn search_roots(f: &mut Function1, a: f64, b: f64, o: NumericOptions) -> Result<RootSearch> {
    let mut grid = Vec::with_capacity(o.steps + 1);
    let mut values = Vec::new();
    let merge_tol = (o.tolerance * 0.1).max(1e-10);
    for i in 0..=o.steps {
        let x = a + (b - a) * i as f64 / o.steps as f64;
        grid.push((x, f.optional(x)?));
    }
    if grid.iter().all(|(_, v)| *v == Some(0.0)) {
        return Err(MathError::Unsupported(
            "所有网格值均为零，可能为恒零函数，不能枚举离散根".into(),
        ));
    }
    let scale = grid
        .iter()
        .filter_map(|(_, v)| *v)
        .fold(0.0_f64, |a, b| a.max(b.abs()));
    f.scale = if scale == 0.0 { 1.0 } else { scale };
    for (_, v) in &mut grid {
        if let Some(value) = v {
            *value = checked_ratio(*value, f.scale)?;
        }
    }
    for &(x, v) in &grid {
        if v == Some(0.0) {
            add_root(&mut values, x, merge_tol);
        }
    }
    for pair in grid.windows(2) {
        let ((mut l, lv), (mut r, rv)) = (pair[0], pair[1]);
        let (Some(mut fl), Some(fr)) = (lv, rv) else {
            continue;
        };
        if fl == 0.0 || fr == 0.0 || fl.signum() == fr.signum() {
            continue;
        }
        for _ in 0..64 {
            let m = l + (r - l) / 2.0;
            let Some(fm) = f.optional(m)? else {
                break;
            };
            // 只在括区同时收缩时接受小残差，避免极点两侧远处的小函数值。
            if fm == 0.0 || ((r - l) <= merge_tol && fm.abs() <= o.tolerance) {
                add_root(&mut values, m, merge_tol);
                break;
            }
            if m == l || m == r {
                break;
            }
            if fm.signum() == fl.signum() {
                l = m;
                fl = fm;
            } else {
                r = m;
            }
        }
    }
    for triple in grid.windows(3) {
        let (Some(l), Some(m), Some(r)) = (triple[0].1, triple[1].1, triple[2].1) else {
            continue;
        };
        if m.abs() > l.abs() || m.abs() > r.abs() {
            continue;
        }
        let (lo, hi) = (triple[0].0, triple[2].0);
        let mut x = triple[1].0;
        for _ in 0..32 {
            let Some(fx) = f.optional(x)? else {
                break;
            };
            if fx == 0.0 {
                add_root(&mut values, x, merge_tol);
                break;
            }
            let h = ((hi - lo) * 1e-4).max(1e-8 * (1.0 + x.abs()));
            let (Some(fl), Some(fr)) = (f.optional(x - h)?, f.optional(x + h)?) else {
                break;
            };
            let d = (fr - fl) / (2.0 * h);
            if d == 0.0 || !d.is_finite() {
                break;
            }
            let next = x - fx / d;
            if !next.is_finite() || next < lo || next > hi {
                break;
            }
            if (next - x).abs() <= merge_tol {
                if let Some(v) = f.optional(next)?
                    && v.abs() <= o.tolerance
                {
                    add_root(&mut values, next, merge_tol * 4.0);
                }
                break;
            }
            x = next;
        }
    }
    values.sort_by(f64::total_cmp);
    Ok(RootSearch {
        roots: values,
        interval: (a, b),
        complete: false,
    })
}
/// 返回 y=f(x)、y=g(x) 的有限区间数值交点；继承 roots 的非完备性。
pub fn curve_intersections(
    first: &str,
    second: &str,
    a: f64,
    b: f64,
    options: NumericOptions,
) -> Result<Vec<Point>> {
    let o = options.validate()?;
    interval(a, b)?;
    let mut f = Function1::new(parse(first)?, Some(parse(second)?), o);
    let found = search_roots(&mut f, a, b, o)?;
    found
        .roots
        .into_iter()
        .map(|x| {
            f.budget.take()?;
            Ok(Point {
                x,
                y: f.first.eval(Some(x), None)?,
            })
        })
        .collect()
}
#[derive(Debug, Clone, Copy)]
pub struct Bounds2D {
    pub x_min: f64,
    pub x_max: f64,
    pub y_min: f64,
    pub y_max: f64,
}
#[derive(Debug, Clone)]
pub struct SystemSearch {
    pub points: Vec<Point>,
    pub bounds: Bounds2D,
    pub complete: bool,
    /// 网格 Newton 中没有满足残差/步长条件的种子数量。
    pub unconverged_seeds: usize,
}
/// 总次数 ≤ 2 的两式在矩形内求全部交点：结式消元化为一元多项式，实根由符号
/// 引擎完备隔离（不再依赖采样与 Newton 收敛运气），再回代求 y；线性系统走
/// 解析接口。重合或含公共曲线成分的系统不能枚举为点集，返回 Unsupported。
pub fn solve_quadratic_system(
    first: &str,
    second: &str,
    bounds: Bounds2D,
    options: NumericOptions,
) -> Result<SystemSearch> {
    let o = options.validate()?;
    interval(bounds.x_min, bounds.x_max)?;
    interval(bounds.y_min, bounds.y_max)?;
    let p = algebra::equation(first)?;
    let q = algebra::equation(second)?;
    if p.degree() > 2 || q.degree() > 2 {
        return Err(MathError::Unsupported(
            "仅支持总次数不超过 2 的二元多项式方程".into(),
        ));
    }
    if p.degree() <= 1 && q.degree() <= 1 {
        let points = match algebra::linear(&p, &q)? {
            SystemSolutions::None => Vec::new(),
            SystemSolutions::Infinite => {
                return Err(MathError::Unsupported(
                    "系统有无穷多解，不能枚举为点集".into(),
                ));
            }
            SystemSolutions::Unique { x, y } => {
                if inside(bounds, x, y) {
                    vec![Point { x, y }]
                } else {
                    Vec::new()
                }
            }
        };
        return Ok(SystemSearch {
            points,
            bounds,
            complete: true,
            unconverged_seeds: 0,
        });
    }
    let p = p.normalized()?;
    let q = q.normalized()?;
    if p == q || p.degree() == 0 || q.degree() == 0 {
        return Err(MathError::Unsupported(
            "退化或重合的二次系统请单独分析，不能保证离散解".into(),
        ));
    }
    let mut budget = Budget::new(o, 12);
    // 结式消元：两式视为 y 的多项式，Sylvester 结式是 x 的一元多项式，
    // 其实根恰为存在公共 y 解的 x；回代后取两式在该 x 处的公共 y 根。
    let resultant = sylvester_resultant(&p, &q)?;
    budget.take()?;
    let x_poly = cleaned(resultant)?
        .into_iter()
        .map(super::symbolic::Rat::from_f64)
        .collect::<Result<Vec<_>>>()?;
    let x_roots = super::symbolic::real_roots(&super::symbolic::Poly::from_coefficients(x_poly))?;
    let mut points: Vec<Point> = Vec::new();
    for x in x_roots {
        budget.take()?;
        let py = y_slice(&p, x)?;
        let qy = y_slice(&q, x)?;
        // 退化：某个 x 上两式同时恒零 → 公共线成分，无法枚举离散点。
        if py.iter().all(|v| v.abs() <= o.tolerance) && qy.iter().all(|v| v.abs() <= o.tolerance) {
            return Err(MathError::Unsupported(
                "系统含公共曲线成分，不能枚举为点集".into(),
            ));
        }
        for y in common_roots(&py, &qy, o.tolerance) {
            if inside(bounds, x, y)
                && !points
                    .iter()
                    .any(|p| (p.x - x).hypot(p.y - y) < o.tolerance * 16.0)
            {
                points.push(Point { x, y });
            }
        }
    }
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    Ok(SystemSearch {
        points,
        bounds,
        complete: true,
        unconverged_seeds: 0,
    })
}

/// y^j 的系数是 x 的多项式：layers[j][i] = x^i·y^j 的系数（总次数 ≤ 2）。
fn y_layers(p: &algebra::Polynomial) -> [[f64; 3]; 3] {
    let mut layers = [[0.0_f64; 3]; 3];
    for (j, row) in layers.iter_mut().enumerate() {
        for (i, cell) in row.iter_mut().enumerate() {
            *cell = p.coefficient(i as u8, j as u8);
        }
    }
    layers
}

/// Sylvester 结式：把 p、q 视为 y 的多项式，返回 x 的一元多项式系数
/// （下标 = x 次数）。两式有公共成分时结式恒为零，由 cleaned 拦截。
fn sylvester_resultant(p: &algebra::Polynomial, q: &algebra::Polynomial) -> Result<Vec<f64>> {
    let a = y_layers(p);
    let b = y_layers(q);
    let da = (0..3)
        .rev()
        .find(|&j| a[j].iter().any(|c| *c != 0.0))
        .ok_or_else(|| MathError::Unsupported("方程退化为恒等式".into()))?;
    let db = (0..3)
        .rev()
        .find(|&j| b[j].iter().any(|c| *c != 0.0))
        .ok_or_else(|| MathError::Unsupported("方程退化为恒等式".into()))?;
    let size = da + db;
    let mut matrix = vec![vec![Vec::new(); size]; size];
    for row in 0..db {
        for col in 0..size {
            let shift = col as isize - row as isize;
            let index = da as isize - shift;
            if (0..=da as isize).contains(&index) {
                matrix[row][col] = a[index as usize].to_vec();
            }
        }
    }
    for row in db..size {
        let r = row - db;
        for col in 0..size {
            let shift = col as isize - r as isize;
            let index = db as isize - shift;
            if (0..=db as isize).contains(&index) {
                matrix[row][col] = b[index as usize].to_vec();
            }
        }
    }
    poly_determinant(&matrix)
}

/// 多项式行列式（元素为 x 的系数向量，规模 ≤ 4，按余子式展开）。
fn poly_determinant(matrix: &[Vec<Vec<f64>>]) -> Result<Vec<f64>> {
    if matrix.len() == 1 {
        return Ok(matrix[0][0].clone());
    }
    let mut total = vec![0.0];
    for col in 0..matrix.len() {
        let mut minor = Vec::with_capacity(matrix.len() - 1);
        for row in 1..matrix.len() {
            let mut r = Vec::with_capacity(matrix.len() - 1);
            for (c, cell) in matrix[row].iter().enumerate() {
                if c != col {
                    r.push(cell.clone());
                }
            }
            minor.push(r);
        }
        let term = poly_mul(&matrix[0][col], &poly_determinant(&minor)?)?;
        let signed = if col % 2 == 0 { term } else { poly_neg(&term) };
        total = poly_add(&total, &signed);
    }
    Ok(total)
}

fn poly_add(a: &[f64], b: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; a.len().max(b.len())];
    for (i, v) in out.iter_mut().enumerate() {
        *v = a.get(i).copied().unwrap_or(0.0) + b.get(i).copied().unwrap_or(0.0);
    }
    out
}

fn poly_neg(a: &[f64]) -> Vec<f64> {
    a.iter().map(|v| -v).collect()
}

fn poly_mul(a: &[f64], b: &[f64]) -> Result<Vec<f64>> {
    if a.is_empty() || b.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = vec![0.0; a.len() + b.len() - 1];
    for (i, &x) in a.iter().enumerate() {
        for (j, &y) in b.iter().enumerate() {
            out[i + j] += x * y;
        }
    }
    Ok(out)
}

/// 去掉结式的数值噪声：相对幅值 1e-10 以下的系数视为零并裁掉尾部零项；
/// 结式恒为零说明两式有公共成分，无法枚举离散解。
fn cleaned(mut poly: Vec<f64>) -> Result<Vec<f64>> {
    if poly.iter().any(|v| !v.is_finite()) {
        return Err(MathError::Numerical("结式系数溢出".into()));
    }
    let max = poly.iter().fold(0.0_f64, |acc, v| acc.max(v.abs()));
    if max == 0.0 {
        return Err(MathError::Unsupported(
            "两式有公共成分，不能枚举离散解".into(),
        ));
    }
    for v in poly.iter_mut() {
        if v.abs() < max * 1e-10 {
            *v = 0.0;
        }
    }
    while poly.last().is_some_and(|v| *v == 0.0) {
        poly.pop();
    }
    if poly.is_empty() {
        return Err(MathError::Unsupported(
            "两式有公共成分，不能枚举离散解".into(),
        ));
    }
    Ok(poly)
}

/// 固定 x 后两式关于 y 的系数（Horner 求值 x 部分）。
fn y_slice(p: &algebra::Polynomial, x: f64) -> Result<[f64; 3]> {
    let mut out = [0.0_f64; 3];
    let layers = y_layers(p);
    for (j, row) in layers.iter().enumerate() {
        let mut acc = 0.0;
        for c in row.iter().rev() {
            acc = acc * x + c;
        }
        if !acc.is_finite() {
            return Err(MathError::Numerical("回代求值溢出".into()));
        }
        out[j] = acc;
    }
    Ok(out)
}

/// 固定 x 处两式的公共 y 根：两式各自根的并集中，残差都在容差内的点。
fn common_roots(py: &[f64; 3], qy: &[f64; 3], tolerance: f64) -> Vec<f64> {
    let mut candidates = Vec::new();
    candidates.extend(quadratic_roots(py));
    candidates.extend(quadratic_roots(qy));
    candidates.sort_by(f64::total_cmp);
    candidates.dedup_by(|a, b| (*a - *b).abs() <= tolerance.max(1e-9) * 16.0);
    candidates
        .into_iter()
        .filter(|y| eval_y(py, *y).abs() <= tolerance && eval_y(qy, *y).abs() <= tolerance)
        .collect()
}

fn eval_y(coefficients: &[f64; 3], y: f64) -> f64 {
    (coefficients[2] * y + coefficients[1]) * y + coefficients[0]
}

/// 稳定二次求根（归一化 + q 公式，与代数路径同风格）；退化自动降次。
fn quadratic_roots(coefficients: &[f64; 3]) -> Vec<f64> {
    let (a, b, c) = (coefficients[2], coefficients[1], coefficients[0]);
    let scale = a.abs().max(b.abs()).max(c.abs());
    if !(scale > 0.0) || !scale.is_finite() {
        return Vec::new();
    }
    let (a, b, c) = (a / scale, b / scale, c / scale);
    if a.abs() < 1e-12 {
        if b.abs() < 1e-12 {
            return Vec::new();
        }
        return vec![-c / b];
    }
    let d = b * b - 4.0 * a * c;
    if d < 0.0 {
        return Vec::new();
    }
    if d == 0.0 {
        return vec![-b / (2.0 * a)];
    }
    let s = d.sqrt();
    let q = -0.5 * (b + s.copysign(b));
    let mut values = vec![q / a, c / q];
    values.sort_by(f64::total_cmp);
    values
}
fn inside(b: Bounds2D, x: f64, y: f64) -> bool {
    x >= b.x_min && x <= b.x_max && y >= b.y_min && y <= b.y_max
}
