use super::{MathError, Node, Op, Result, finite, normalize, parse};
use std::collections::BTreeMap;
use std::fmt;

const DEGREE: u8 = 12;
// 二进制幂缩放不额外舍入常见整数系数，避免人为破坏重根和线性相关性。
fn scale_for(max: f64) -> f64 {
    2.0_f64.powf(max.log2().floor().clamp(-1074.0, 1023.0))
}
fn scaled(value: f64, scale: f64) -> Result<f64> {
    let out = finite(value / scale)?;
    if value != 0.0 && out == 0.0 {
        return Err(MathError::Numerical(
            "系数尺度差异过大，归一化会下溢".into(),
        ));
    }
    Ok(out)
}
fn product(a: f64, b: f64) -> Result<f64> {
    let value = finite(a * b)?;
    if a != 0.0 && b != 0.0 && value == 0.0 {
        return Err(MathError::Numerical("系数乘积下溢".into()));
    }
    Ok(value)
}
// 同时补偿两个乘积的舍入，避免把重根拆开或把近奇异系统误判成无穷多解。
fn difference_of_products(a: f64, b: f64, c: f64, d: f64) -> Result<f64> {
    product(a, b)?;
    let cd = product(c, d)?;
    finite(a.mul_add(b, -cd) - c.mul_add(d, -cd))
}
/// 稀疏二元实系数多项式；最多 91 项。不做有理式约分或三角恒等变换。
/// 乘法 O(t² log t)，每个幂最多 12 次乘法；t <= 91。
#[derive(Debug, Clone, PartialEq)]
pub struct Polynomial {
    terms: BTreeMap<(u8, u8), f64>,
}
impl Polynomial {
    pub fn parse(input: &str) -> Result<Self> {
        Self::from_node(&parse(input)?.node)
    }
    pub(crate) fn explicit_graph(input: &str) -> Result<Self> {
        Self::parse("y")?.add(&Self::parse(input)?, -1.0)
    }
    pub fn coefficient(&self, x_degree: u8, y_degree: u8) -> f64 {
        *self.terms.get(&(x_degree, y_degree)).unwrap_or(&0.0)
    }
    pub fn degree(&self) -> u8 {
        self.terms.keys().map(|(x, y)| x + y).max().unwrap_or(0)
    }
    pub fn eval(&self, x: f64, y: f64) -> Result<f64> {
        finite(x)?;
        finite(y)?;
        let mut sum = 0.0;
        for (&(i, j), &c) in &self.terms {
            sum = finite(sum + c * x.powi(i as i32) * y.powi(j as i32))?;
        }
        Ok(sum)
    }
    fn constant(v: f64) -> Self {
        let mut p = Self {
            terms: BTreeMap::new(),
        };
        if v != 0.0 {
            p.terms.insert((0, 0), v);
        }
        p
    }
    fn add_term(&mut self, key: (u8, u8), value: f64) -> Result<()> {
        let value = finite(value + self.coefficient(key.0, key.1))?;
        if value == 0.0 {
            self.terms.remove(&key);
        } else {
            self.terms.insert(key, value);
        }
        Ok(())
    }
    fn add(mut self, other: &Self, sign: f64) -> Result<Self> {
        for (&key, &v) in &other.terms {
            self.add_term(key, sign * v)?;
        }
        Ok(self)
    }
    fn scale(mut self, value: f64) -> Result<Self> {
        for c in self.terms.values_mut() {
            *c = product(*c, value)?;
        }
        self.terms.retain(|_, v| *v != 0.0);
        Ok(self)
    }
    fn mul(&self, other: &Self) -> Result<Self> {
        if self.degree() + other.degree() > DEGREE {
            return Err(MathError::Limit("多项式总次数最多 12".into()));
        }
        let mut out = Self::constant(0.0);
        for (&(i, j), &a) in &self.terms {
            for (&(k, l), &b) in &other.terms {
                out.add_term((i + k, j + l), product(a, b)?)?;
            }
        }
        Ok(out)
    }
    fn from_node(node: &Node) -> Result<Self> {
        match node {
            Node::Number(v) => Ok(Self::constant(*v)),
            Node::Variable(c) => {
                let mut p = Self::constant(0.0);
                p.terms.insert(if *c == 'x' { (1, 0) } else { (0, 1) }, 1.0);
                Ok(p)
            }
            Node::Neg(n) => Self::from_node(n)?.scale(-1.0),
            Node::Call(_, _) => Ok(Self::constant(node.eval(None, None).map_err(
                |e| match e {
                    MathError::MissingVariable(_) => {
                        MathError::Unsupported("含变量的函数不能化为多项式".into())
                    }
                    e => e,
                },
            )?)),
            Node::Binary(op, a, b) => {
                let a = Self::from_node(a)?;
                let b = Self::from_node(b)?;
                match op {
                    Op::Add => a.add(&b, 1.0),
                    Op::Sub => a.add(&b, -1.0),
                    Op::Mul => a.mul(&b),
                    Op::Div => {
                        if b.degree() != 0 {
                            return Err(MathError::Unsupported(
                                "多项式只允许除以非零常数，不约去变量分母".into(),
                            ));
                        }
                        let c = b.coefficient(0, 0);
                        if c == 0.0 {
                            return Err(MathError::Domain("除数为零".into()));
                        }
                        let mut p = a;
                        for v in p.terms.values_mut() {
                            *v = scaled(*v, c)?;
                        }
                        Ok(p)
                    }
                    Op::Pow => {
                        if b.degree() != 0 {
                            return Err(MathError::Unsupported("多项式指数必须是常数".into()));
                        }
                        let power = b.coefficient(0, 0);
                        if a.degree() == 0 {
                            let v = a.coefficient(0, 0);
                            if (v == 0.0 && power <= 0.0) || (v < 0.0 && power.fract() != 0.0) {
                                return Err(MathError::Domain("该幂没有实数值".into()));
                            }
                            let result = finite(v.powf(power))?;
                            if v != 0.0 && result == 0.0 {
                                return Err(MathError::Numerical("系数幂下溢".into()));
                            }
                            return Ok(Self::constant(result));
                        }
                        if power == 0.0 {
                            return Err(MathError::Unsupported(
                                "变量式的零次幂可能含 0^0，不消除其定义域限制".into(),
                            ));
                        }
                        if power < 0.0 || power.fract() != 0.0 {
                            return Err(MathError::Unsupported("多项式只支持正整数幂".into()));
                        }
                        if power > DEGREE as f64 {
                            return Err(MathError::Limit("多项式幂最多 12".into()));
                        }
                        let mut p = Self::constant(1.0);
                        for _ in 0..power as usize {
                            p = p.mul(&a)?;
                        }
                        Ok(p)
                    }
                }
            }
        }
    }
    pub(crate) fn normalized(&self) -> Result<Self> {
        let max = self.terms.values().fold(0.0_f64, |a, b| a.max(b.abs()));
        if max == 0.0 {
            return Ok(self.clone());
        }
        let scale = scale_for(max);
        let mut out = self.clone();
        for v in out.terms.values_mut() {
            *v = scaled(*v, scale)?;
        }
        Ok(out)
    }
    pub(crate) fn gradient(&self, x: f64, y: f64) -> (f64, f64) {
        let (mut dx, mut dy) = (0.0, 0.0);
        for (&(i, j), &c) in &self.terms {
            if i > 0 {
                dx += c * i as f64 * x.powi(i as i32 - 1) * y.powi(j as i32);
            }
            if j > 0 {
                dy += c * j as f64 * x.powi(i as i32) * y.powi(j as i32 - 1);
            }
        }
        (dx, dy)
    }
}
impl fmt::Display for Polynomial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.terms.is_empty() {
            return write!(f, "0");
        }
        let mut terms: Vec<_> = self.terms.iter().collect();
        terms.sort_by_key(|&(&(i, j), _)| std::cmp::Reverse((i + j, i, j)));
        for (index, &(&(i, j), &c)) in terms.iter().enumerate() {
            if index == 0 {
                if c < 0.0 {
                    write!(f, "-")?;
                }
            } else {
                write!(f, " {} ", if c < 0.0 { "-" } else { "+" })?;
            }
            let mut factors = Vec::new();
            if c.abs() != 1.0 || i + j == 0 {
                factors.push(c.abs().to_string());
            }
            if i > 0 {
                factors.push(if i == 1 { "x".into() } else { format!("x^{i}") });
            }
            if j > 0 {
                factors.push(if j == 1 { "y".into() } else { format!("y^{j}") });
            }
            write!(f, "{}", factors.join("*"))?;
        }
        Ok(())
    }
}
/// 展开并合并 x/y 多项式；表达式输入仍返回表达式。
/// 单个等式或分号分隔的等式组逐式移项为「左边 - 右边 = 0」，不求解、不除去公因子。
/// 系数沿用 f64，只有精确零项才删除；不支持变量分母或会丢失定义域的约分。
/// 整组共享 4096 字节、512 词元预算，每个多项式总次数最多 12。
pub fn simplify(input: &str) -> Result<String> {
    let input = normalize(input)?;
    super::statement_budget(&input, 0)?;
    if !input.contains(['=', ';']) {
        return Ok(Polynomial::parse(&input)?.to_string());
    }
    input
        .split(';')
        .map(|part| Ok(format!("{} = 0", equation(part)?)))
        .collect::<Result<Vec<_>>>()
        .map(|parts| parts.join("; "))
}

/// 对 x/y 多项式按指定变量求符号偏导，另一个变量视为常量。
/// 输入总次数最多 12，沿用解析预算和 f64 系数；不支持非多项式函数。
pub fn differentiate_polynomial(input: &str, variable: char) -> Result<String> {
    polynomial_calculus(input, variable, false)
}

/// 返回 x/y 多项式关于指定变量的一个原函数，积分常数（或另一变量的函数）取零。
/// 输入和结果总次数均不得超过 12；系数使用 f64，除法可能舍入，不是精确有理数积分。
pub fn antiderivative_polynomial(input: &str, variable: char) -> Result<String> {
    polynomial_calculus(input, variable, true)
}

fn polynomial_calculus(input: &str, variable: char, integral: bool) -> Result<String> {
    if !matches!(variable, 'x' | 'y') {
        return Err(MathError::Unsupported("符号微积分变量仅支持 x 或 y".into()));
    }
    let p = Polynomial::parse(input)?;
    let mut out = Polynomial::constant(0.0);
    for (&(i, j), &coefficient) in &p.terms {
        let degree = if variable == 'x' { i } else { j };
        let (new_degree, value) = if integral {
            if i + j >= DEGREE {
                return Err(MathError::Limit("原函数总次数最多 12".into()));
            }
            (degree + 1, scaled(coefficient, f64::from(degree + 1))?)
        } else {
            if degree == 0 {
                continue;
            }
            (degree - 1, product(coefficient, f64::from(degree))?)
        };
        let key = if variable == 'x' {
            (new_degree, j)
        } else {
            (i, new_degree)
        };
        out.add_term(key, value)?;
    }
    Ok(out.to_string())
}

pub(crate) fn equation(input: &str) -> Result<Polynomial> {
    let input = normalize(input)?;
    super::statement_budget(&input, 0)?;
    let mut parts = input.split('=');
    let left = parts.next().unwrap_or("");
    let right = parts
        .next()
        .ok_or_else(|| MathError::Syntax("方程必须含一个等号".into()))?;
    if parts.next().is_some() {
        return Err(MathError::Syntax("方程只能含一个等号".into()));
    }
    Polynomial::parse(left)?.add(&Polynomial::parse(right)?, -1.0)
}
#[derive(Debug, Clone, PartialEq)]
pub enum Solutions {
    None,
    Infinite,
    Finite(Vec<f64>),
}
/// 求一元 x 一次/二次方程的实根；退化为恒等式或矛盾式时分别返回 Infinite/None。
pub fn solve_equation(input: &str) -> Result<Solutions> {
    let p = equation(input)?;
    if p.degree() > 2 || p.terms.keys().any(|(_, j)| *j != 0) {
        return Err(MathError::Unsupported(
            "解析求解仅支持变量 x 的一次/二次方程".into(),
        ));
    }
    quadratic(
        p.coefficient(2, 0),
        p.coefficient(1, 0),
        p.coefficient(0, 0),
    )
}
fn quadratic(a: f64, b: f64, c: f64) -> Result<Solutions> {
    if a == 0.0 {
        return Ok(if b == 0.0 {
            if c == 0.0 {
                Solutions::Infinite
            } else {
                Solutions::None
            }
        } else {
            Solutions::Finite(vec![scaled(-c, b)?])
        });
    }
    // 归一化和稳定的 q 公式减少溢出以及小根的相消误差；不把近零系数擅自退化为零。
    let scale = scale_for(a.abs().max(b.abs()).max(c.abs()));
    let (a, b, c) = (scaled(a, scale)?, scaled(b, scale)?, scaled(c, scale)?);
    let d = difference_of_products(b, b, 4.0 * a, c)?;
    if d < 0.0 {
        return Ok(Solutions::None);
    }
    if d == 0.0 {
        if (b != 0.0 && b * b == 0.0) || (c != 0.0 && a * c == 0.0) {
            return Err(MathError::Numerical("判别式计算下溢".into()));
        }
        return Ok(Solutions::Finite(vec![scaled(-b, 2.0 * a)?]));
    }
    let q = -0.5 * (b + d.sqrt().copysign(b));
    let mut values = vec![scaled(q, a)?, scaled(c, q)?];
    values.sort_by(f64::total_cmp);
    Ok(Solutions::Finite(values))
}
#[derive(Debug, Clone, PartialEq)]
pub enum SystemSolutions {
    None,
    Infinite,
    Unique { x: f64, y: f64 },
}
/// 二元一次系统，使用行归一化和行列式；病态而非严格退化时返回数值错误。
pub fn solve_linear_system(first: &str, second: &str) -> Result<SystemSolutions> {
    let p = equation(first)?;
    let q = equation(second)?;
    if p.degree() > 1 || q.degree() > 1 {
        return Err(MathError::Unsupported("该接口只支持二元一次系统".into()));
    }
    linear(&p, &q)
}
pub(crate) fn linear(p: &Polynomial, q: &Polynomial) -> Result<SystemSolutions> {
    let p = p.normalized()?;
    let q = q.normalized()?;
    let (a, b, c) = (
        p.coefficient(1, 0),
        p.coefficient(0, 1),
        -p.coefficient(0, 0),
    );
    let (d, e, f) = (
        q.coefficient(1, 0),
        q.coefficient(0, 1),
        -q.coefficient(0, 0),
    );
    if (a == 0.0 && b == 0.0 && c != 0.0) || (d == 0.0 && e == 0.0 && f != 0.0) {
        return Ok(SystemSolutions::None);
    }
    let det = difference_of_products(a, e, b, d)?;
    let x_numerator = difference_of_products(c, e, b, f)?;
    let y_numerator = difference_of_products(a, f, c, d)?;
    if det == 0.0 {
        return Ok(if y_numerator == 0.0 && x_numerator == 0.0 {
            SystemSolutions::Infinite
        } else {
            SystemSolutions::None
        });
    }
    if det.abs() < 1e-14 * (a * e).abs().max((b * d).abs()) {
        return Err(MathError::Numerical(
            "线性系统接近奇异，无法可靠求解".into(),
        ));
    }
    Ok(SystemSolutions::Unique {
        x: scaled(x_numerator, det)?,
        y: scaled(y_numerator, det)?,
    })
}
