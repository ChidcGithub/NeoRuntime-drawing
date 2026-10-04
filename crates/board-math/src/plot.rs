use super::{
    Bounds2D, Expr, MathError, Node, NumericOptions, Point, Polynomial, Result, SampledCurve,
    algebra, checked_ratio, finite, normalize, parse, sample, statement_budget,
};

/// 手动绘图的存储形式：显函数为右端表达式，隐式曲线为完整单个等式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlotKind {
    Explicit(String),
    Implicit(String),
}

fn depends_on(node: &Node, variable: char) -> bool {
    match node {
        Node::Variable(c) => *c == variable,
        Node::Neg(a) | Node::Call(_, a) => depends_on(a, variable),
        Node::Binary(_, a, b) => depends_on(a, variable) || depends_on(b, variable),
        Node::Number(_) => false,
    }
}

fn function_label(input: &str) -> bool {
    input.split_whitespace().collect::<String>() == "f(x)"
}

/// 分类手动绘图输入。允许纯 f(x) 表达式、y=f(x)、f(x)=... 和二次以内隐式方程。
/// 不求解、不改变 calculate/solve 的语义；非多项式隐式关系返回 Unsupported。
pub fn classify_plot(input: &str) -> Result<PlotKind> {
    let normalized = normalize(input)?;
    let input = normalized.trim();
    if input.contains(';') {
        return Err(MathError::Unsupported("绘图需要单个表达式或方程".into()));
    }
    let Some((left, right)) = input.split_once('=') else {
        let expression = parse(input)?;
        if depends_on(&expression.node, 'y') {
            return Err(MathError::Unsupported("含 y 的隐式曲线必须提供等号".into()));
        }
        return Ok(PlotKind::Explicit(input.into()));
    };
    // f(x) 是绘图标签而非解析器函数；仍将其词元计入整条输入预算。
    let label = function_label(left);
    if label {
        statement_budget(right, 5)?;
    } else {
        statement_budget(input, 0)?;
    }
    let rhs = parse(right)?;
    let lhs = if label { None } else { Some(parse(left)?) };
    if (label
        || lhs
            .as_ref()
            .is_some_and(|e| matches!(e.node, Node::Variable('y'))))
        && !depends_on(&rhs.node, 'y')
    {
        return Ok(PlotKind::Explicit(right.trim().into()));
    }
    let p = algebra::equation(input)?;
    if p.degree() > 2 {
        return Err(MathError::Unsupported(
            "隐式绘图仅支持总次数不超过 2 的多项式".into(),
        ));
    }
    Ok(PlotKind::Implicit(input.into()))
}

/// 自动候选分类：仅显式函数声明或含 y 的二次以内单方程成为绘图候选。
/// x=2、x^2=2、2x+3=x-1 及普通表达式仍留给原计算路径。
/// 返回值可直接存入 FunctionPlot.expressions；无实曲线的合法等式也保留。
pub fn plot_expression(input: &str) -> Option<String> {
    let normalized = normalize(input).ok()?;
    let (left, right) = normalized.split_once('=')?;
    match classify_plot(&normalized).ok()? {
        PlotKind::Explicit(expression) => Some(expression),
        PlotKind::Implicit(equation) => (depends_on(&parse(left).ok()?.node, 'y')
            || depends_on(&parse(right).ok()?.node, 'y'))
        .then_some(equation),
    }
}

/// 有界绘图采样。steps 要求 8..=4096，实际分段数钳制到 512..=2048。
/// 边界沿用数值 API：两轴递增、有限且位于 [-1e6,1e6]。
/// 显函数复用 sample 的不连续性检测；隐式二次曲线解析参数化，分支分别存储。
/// 椭圆首尾点完全相同；孤立点为单点 segment；空集无 segment；全平面 Unsupported。
/// 不裁剪矩形：闭合椭圆完整输出，无界曲线按矩形投影选择参数范围，由渲染端裁剪。
/// complete 恒为 false（有限折线不承诺几何误差界），最多 2050 个隐式采样点。
pub fn sample_plot(input: &str, bounds: Bounds2D, steps: usize) -> Result<SampledCurve> {
    if !(8..=4096).contains(&steps) {
        return Err(MathError::Limit("绘图网格须为 8..4096".into()));
    }
    super::numeric::interval(bounds.x_min, bounds.x_max)?;
    super::numeric::interval(bounds.y_min, bounds.y_max)?;
    let steps = steps.clamp(512, 2048).div_ceil(4) * 4;
    match classify_plot(input)? {
        PlotKind::Explicit(expression) => sample(
            &expression,
            bounds.x_min,
            bounds.x_max,
            NumericOptions {
                steps,
                ..NumericOptions::default()
            },
        ),
        PlotKind::Implicit(equation) => {
            let p = algebra::equation(&equation)?;
            let (left, right) = equation.split_once('=').unwrap();
            let original = (parse(left)?, parse(right)?);
            let segments = conic(&p, &original, bounds, steps)?;
            Ok(SampledCurve {
                segments,
                skipped_intervals: 0,
                complete: false,
            })
        }
    }
}

#[derive(Clone, Copy)]
struct Frame {
    center: Point,
    axis: Point,
}
impl Frame {
    fn point(self, u: f64, v: f64) -> Result<Point> {
        Ok(Point {
            x: finite(
                self.axis
                    .x
                    .mul_add(u, (-self.axis.y).mul_add(v, self.center.x)),
            )?,
            y: finite(
                self.axis
                    .y
                    .mul_add(u, self.axis.x.mul_add(v, self.center.y)),
            )?,
        })
    }
    fn range(self, bounds: Bounds2D, second: bool) -> (f64, f64) {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for x in [bounds.x_min, bounds.x_max] {
            for y in [bounds.y_min, bounds.y_max] {
                let x = x - self.center.x;
                let y = y - self.center.y;
                let value = if second {
                    -self.axis.y * x + self.axis.x * y
                } else {
                    self.axis.x * x + self.axis.y * y
                };
                lo = lo.min(value);
                hi = hi.max(value);
            }
        }
        (lo, hi)
    }
}

// 使用稳定大特征值与补偿行列式求小特征值；绝不按绝对 epsilon 抹掉小圆/小轴。
fn eigen(a: f64, b: f64, c: f64) -> Result<(f64, f64, Point)> {
    if b == 0.0 {
        return Ok(if a.abs() >= c.abs() {
            (a, c, Point { x: 1.0, y: 0.0 })
        } else {
            (c, a, Point { x: 0.0, y: 1.0 })
        });
    }
    let q = a.abs().max(b.abs()).max(c.abs());
    let (a, b, c) = (
        checked_ratio(a, q)?,
        checked_ratio(b, q)?,
        checked_ratio(c, q)?,
    );
    let half_trace = (a + c) * 0.5;
    let radius = ((a - c) * 0.5).hypot(b);
    let first = half_trace + radius.copysign(half_trace);
    let bb = super::nonzero_result(b * b, b != 0.0)?;
    super::nonzero_result(a * c, a != 0.0 && c != 0.0)?;
    let det = a.mul_add(c, -bb) - b.mul_add(b, -bb);
    if det != 0.0 && det.abs() < 32.0 * f64::EPSILON * (a * c).abs().max(bb) {
        return Err(MathError::Numerical("二次型接近奇异，无法可靠分类".into()));
    }
    let (x, y) = if (first - c).abs() >= (first - a).abs() {
        (first - c, b)
    } else {
        (b, first - a)
    };
    let norm = x.hypot(y);
    Ok((
        finite(first * q)?,
        finite((det / first) * q)?,
        Point {
            x: x / norm,
            y: y / norm,
        },
    ))
}

fn parameters(
    range: (f64, f64),
    steps: usize,
    f: impl Fn(f64) -> Result<Point>,
) -> Result<Vec<Point>> {
    let (lo, hi) = range;
    finite(lo)?;
    finite(hi)?;
    // 两段参数网格共享精确零，确保抛物线顶点和双曲线顶点不被跨过。
    (0..=steps)
        .map(|i| {
            let t = if lo < 0.0 && hi > 0.0 {
                if i <= steps / 2 {
                    lo * (1.0 - i as f64 / (steps / 2) as f64)
                } else {
                    hi * ((i - steps / 2) as f64 / (steps - steps / 2) as f64)
                }
            } else {
                lo + (hi - lo) * i as f64 / steps as f64
            };
            f(t)
        })
        .collect()
}

fn line(center: Point, direction: Point, bounds: Bounds2D) -> Result<Vec<Point>> {
    let norm = direction.x.hypot(direction.y);
    let frame = Frame {
        center,
        axis: Point {
            x: direction.x / norm,
            y: direction.y / norm,
        },
    };
    let (lo, hi) = frame.range(bounds, false);
    Ok(vec![frame.point(lo, 0.0)?, frame.point(hi, 0.0)?])
}

fn conic(
    original_p: &Polynomial,
    original: &(Expr, Expr),
    bounds: Bounds2D,
    steps: usize,
) -> Result<Vec<Vec<Point>>> {
    let p = original_p.normalized()?;
    let (a, b, c, d, e, f) = (
        p.coefficient(2, 0),
        p.coefficient(1, 1) * 0.5,
        p.coefficient(0, 2),
        p.coefficient(1, 0),
        p.coefficient(0, 1),
        p.coefficient(0, 0),
    );
    if a == 0.0 && b == 0.0 && c == 0.0 {
        if d == 0.0 && e == 0.0 {
            return if f == 0.0 {
                Err(MathError::Unsupported(
                    "恒等式表示全平面，不能绘为曲线".into(),
                ))
            } else {
                Ok(Vec::new())
            };
        }
        let center = if d.abs() >= e.abs() {
            Point {
                x: checked_ratio(-f, d)?,
                y: 0.0,
            }
        } else {
            Point {
                x: 0.0,
                y: checked_ratio(-f, e)?,
            }
        };
        return Ok(vec![line(center, Point { x: -e, y: d }, bounds)?]);
    }
    let (l1, l2, axis) = eigen(a, b, c)?;
    let du = d * axis.x + e * axis.y;
    let dv = -d * axis.y + e * axis.x;
    let u0 = checked_ratio(-du * 0.5, l1)?;
    let v0 = if l2 == 0.0 {
        0.0
    } else {
        checked_ratio(-dv * 0.5, l2)?
    };
    let mut frame = Frame {
        center: Point { x: 0.0, y: 0.0 },
        axis,
    };
    frame.center = frame.point(u0, v0)?;
    if l2 == 0.0 && dv == 0.0 {
        // 平行/二重直线可沿零空间平移中心；轴截距避免旋转往返导致假空集。
        frame.center = if a.abs() >= c.abs() {
            Point {
                x: checked_ratio(-d * 0.5, a)?,
                y: 0.0,
            }
        } else {
            Point {
                x: 0.0,
                y: checked_ratio(-e * 0.5, c)?,
            }
        };
    }
    // 在原 AST 上计算平移常数，避免展开 (x-h)^2-r^2 时丢失可见的微小半径。
    let raw_k = finite(
        original.0.eval_xy(frame.center.x, frame.center.y)?
            - original.1.eval_xy(frame.center.x, frame.center.y)?,
    )?;
    // 原式与 normalized 系数共享同一个二进制缩放因子。
    let scale = if a != 0.0 {
        original_p.coefficient(2, 0) / a
    } else if c != 0.0 {
        original_p.coefficient(0, 2) / c
    } else {
        original_p.coefficient(1, 1) / (2.0 * b)
    };
    let k = checked_ratio(raw_k, scale)?;
    if l2 == 0.0 {
        if dv != 0.0 {
            if dv.abs() < 32.0 * f64::EPSILON * d.abs().max(e.abs()) {
                return Err(MathError::Numerical("退化二次曲线的线性项接近零".into()));
            }
            let shift = checked_ratio(-k, dv)?;
            frame.center = frame.point(0.0, shift)?;
            return Ok(vec![parameters(frame.range(bounds, false), steps, |u| {
                frame.point(u, -(l1 / dv) * u * u)
            })?]);
        }
        let radius2 = checked_ratio(-k, l1)?;
        return if radius2 < 0.0 {
            Ok(Vec::new())
        } else {
            let radius = radius2.sqrt();
            let direction = Point {
                x: -axis.y,
                y: axis.x,
            };
            let mut lines = vec![line(frame.point(radius, 0.0)?, direction, bounds)?];
            if radius != 0.0 {
                lines.push(line(frame.point(-radius, 0.0)?, direction, bounds)?);
            }
            Ok(lines)
        };
    }
    if l1.signum() == l2.signum() {
        let ru = checked_ratio(-k, l1)?;
        if ru < 0.0 {
            return Ok(Vec::new());
        }
        if ru == 0.0 {
            return Ok(vec![vec![frame.center]]);
        }
        let (ru, rv) = (ru.sqrt(), checked_ratio(-k, l2)?.sqrt());
        let mut points = parameters((0.0, std::f64::consts::TAU), steps, |t| {
            frame.point(ru * t.cos(), rv * t.sin())
        })?;
        points[steps] = points[0];
        return Ok(vec![points]);
    }
    if k == 0.0 {
        let u = l2.abs().sqrt();
        let v = l1.abs().sqrt();
        return [1.0, -1.0]
            .into_iter()
            .map(|sign| {
                line(
                    frame.center,
                    Point {
                        x: axis.x * u - axis.y * v * sign,
                        y: axis.y * u + axis.x * v * sign,
                    },
                    bounds,
                )
            })
            .collect();
    }
    let along_u = (-k).signum() == l1.signum();
    let (ru, rv) = (
        checked_ratio(k, l1)?.abs().sqrt(),
        checked_ratio(k, l2)?.abs().sqrt(),
    );
    let extent = frame.range(bounds, along_u);
    let transverse = if along_u { rv } else { ru };
    let range = (
        (extent.0 / transverse).asinh(),
        (extent.1 / transverse).asinh(),
    );
    [1.0, -1.0]
        .into_iter()
        .map(|sign| {
            parameters(range, steps / 2, |t| {
                if along_u {
                    frame.point(sign * ru * t.cosh(), rv * t.sinh())
                } else {
                    frame.point(ru * t.sinh(), sign * rv * t.cosh())
                }
            })
        })
        .collect()
}
