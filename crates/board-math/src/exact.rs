use super::{
    Bounds2D, Function, MathError, Node, NumericOptions, Op, Parser, Result, Token,
    calculate_with_bounds, display_number, lex, normalize,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathDisplay {
    Text(String),
    Row(Vec<MathDisplay>),
    Fraction(Box<MathDisplay>, Box<MathDisplay>),
    Radical(Box<MathDisplay>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayCalculation {
    pub text: String,
    pub display: Option<MathDisplay>,
    /// 仅标记常量计算的近似回退；旧方程/符号输出的精度说明仍在 text 中。
    pub approximate: bool,
}

/// 常量优先返回精确有理数/平方根结构；其余输入保持原有求解和符号命令输出。
/// 仅不支持的精确运算回退到带 ≈ 的数值结果，定义域和预算错误不回退。
pub fn calculate_display_with_bounds(
    input: &str,
    bounds: Bounds2D,
    options: NumericOptions,
) -> Result<DisplayCalculation> {
    let normalized = normalize(input)?;
    let input = normalized.as_ref();
    let command = input.split('(').next().unwrap_or("").trim();
    if input.contains(['=', ';']) || matches!(command, "simplify" | "diff" | "integrate") {
        return legacy(input, bounds, options);
    }
    if lex(input)?.iter().any(|t| matches!(t, Token::Variable(_))) {
        return legacy(input, bounds, options);
    }
    constant(input)
}

fn legacy(input: &str, bounds: Bounds2D, options: NumericOptions) -> Result<DisplayCalculation> {
    Ok(DisplayCalculation {
        text: calculate_with_bounds(input, bounds, options)?,
        display: None,
        approximate: false,
    })
}

fn limit() -> MathError {
    MathError::Limit("精确计算超出 i128、128 次幂、32 项或运算预算".into())
}
fn checked(value: Option<i128>) -> Result<i128> {
    value.filter(|v| *v != i128::MIN).ok_or_else(limit)
}
fn gcd(mut a: i128, mut b: i128) -> i128 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

#[derive(Clone, Copy, Debug)]
struct Rational {
    n: i128,
    d: i128,
}
impl Rational {
    fn new(n: i128, d: i128) -> Result<Self> {
        if d == 0 {
            return Err(MathError::Domain("除数为零".into()));
        }
        let n = checked(Some(n))?;
        let d = checked(Some(d))?;
        let g = gcd(n, d);
        Ok(Self {
            n: n / g * d.signum(),
            d: d.abs() / g,
        })
    }
    fn integer(n: i128) -> Self {
        Self { n, d: 1 }
    }
    fn neg(self) -> Self {
        Self { n: -self.n, ..self }
    }
    fn add(self, b: Self) -> Result<Self> {
        let g = gcd(self.d, b.d);
        let a_num = checked(self.n.checked_mul(b.d / g))?;
        let b_num = checked(b.n.checked_mul(self.d / g))?;
        Self::new(
            checked(a_num.checked_add(b_num))?,
            checked(self.d.checked_mul(b.d / g))?,
        )
    }
    fn mul(self, b: Self) -> Result<Self> {
        let g = gcd(self.n, b.d);
        let h = gcd(b.n, self.d);
        Self::new(
            checked((self.n / g).checked_mul(b.n / h))?,
            checked((self.d / h).checked_mul(b.d / g))?,
        )
    }
    fn div(self, b: Self) -> Result<Self> {
        self.mul(Self::new(b.d, b.n)?)
    }
    fn literal(s: &str) -> Result<Self> {
        let (mantissa, exponent) = match s.split_once(['e', 'E']) {
            Some((m, e)) => (m, e.parse::<i32>().map_err(|_| limit())?),
            None => (s, 0),
        };
        let fractional = mantissa.split_once('.').map_or(0, |(_, s)| s.len());
        let raw_digits = mantissa.replace('.', "");
        let digits = raw_digits.trim_start_matches('0').trim_end_matches('0');
        if digits.is_empty() {
            return Ok(Self::integer(0));
        }
        let trailing = raw_digits.len() - raw_digits.trim_end_matches('0').len();
        let power = exponent
            .checked_sub(fractional as i32)
            .and_then(|v| v.checked_add(trailing as i32))
            .ok_or_else(limit)?;
        if power.unsigned_abs() > 38 || digits.len() > 39 {
            return Err(limit());
        }
        let n = digits.parse::<i128>().map_err(|_| limit())?;
        let scale = checked(10_i128.checked_pow(power.unsigned_abs()))?;
        if power >= 0 {
            Self::new(checked(n.checked_mul(scale))?, 1)
        } else {
            Self::new(n, scale)
        }
    }
}

struct Budget {
    operations: usize,
    factors: usize,
}
impl Budget {
    fn take(&mut self) -> Result<()> {
        self.operations = self.operations.checked_sub(1).ok_or_else(limit)?;
        Ok(())
    }
    fn square_parts(&mut self, mut n: i128) -> Result<(i128, i128)> {
        let square = n.isqrt();
        if square * square == n {
            return Ok((square, 1));
        }
        let (mut outside, mut inside, mut p) = (1_i128, 1_i128, 2_i128);
        while p <= n / p {
            self.factors = self.factors.checked_sub(1).ok_or_else(limit)?;
            let mut count = 0;
            while n % p == 0 {
                n /= p;
                count += 1;
            }
            for _ in 0..count / 2 {
                outside = checked(outside.checked_mul(p))?;
            }
            if count % 2 != 0 {
                inside = checked(inside.checked_mul(p))?;
            }
            p = if p == 2 { 3 } else { p + 2 };
        }
        Ok((outside, checked(inside.checked_mul(n))?))
    }
}

// 每项为有理系数乘以正的无平方因子整数的平方根；键 1 表示有理数。
#[derive(Clone, Debug)]
struct Exact(BTreeMap<i128, Rational>);
impl Exact {
    fn rational(r: Rational) -> Self {
        if r.n == 0 {
            Self(BTreeMap::new())
        } else {
            Self(BTreeMap::from([(1, r)]))
        }
    }
    fn as_rational(&self) -> Option<Rational> {
        if self.0.is_empty() {
            Some(Rational::integer(0))
        } else if self.0.len() == 1 {
            self.0.get(&1).copied()
        } else {
            None
        }
    }
    fn insert(&mut self, root: i128, coefficient: Rational) -> Result<()> {
        let sum = match self.0.get(&root) {
            Some(old) => old.add(coefficient)?,
            None => coefficient,
        };
        if sum.n == 0 {
            self.0.remove(&root);
        } else {
            self.0.insert(root, sum);
        }
        if self.0.len() > 32 {
            return Err(limit());
        }
        Ok(())
    }
    fn neg(mut self) -> Self {
        for c in self.0.values_mut() {
            *c = c.neg();
        }
        self
    }
    fn add(mut self, b: Self) -> Result<Self> {
        for (root, c) in b.0 {
            self.insert(root, c)?;
        }
        Ok(self)
    }
    fn mul(&self, b: &Self, budget: &mut Budget) -> Result<Self> {
        let mut result = Self(BTreeMap::new());
        for (&r, &a) in &self.0 {
            for (&s, &b) in &b.0 {
                budget.take()?;
                let common = gcd(r, s);
                let root = checked((r / common).checked_mul(s / common))?;
                let c = a.mul(b)?.mul(Rational::integer(common))?;
                result.insert(root, c)?;
            }
        }
        Ok(result)
    }
    fn inverse(&self, budget: &mut Budget) -> Result<Option<Self>> {
        if self.0.is_empty() {
            return Err(MathError::Domain("除数为零".into()));
        }
        if self.0.len() == 1 {
            let (&root, &c) = self.0.first_key_value().unwrap();
            let coefficient = Rational::integer(1).div(c)?.div(Rational::integer(root))?;
            return Ok(Some(Self(BTreeMap::from([(root, coefficient)]))));
        }
        if self.0.len() == 2 {
            let mut conjugate = self.clone();
            let (_, c) = conjugate.0.last_key_value().unwrap();
            let last = *conjugate.0.last_key_value().unwrap().0;
            conjugate.0.insert(last, c.neg());
            let norm = self.mul(&conjugate, budget)?.as_rational().unwrap();
            let scale = Self::rational(Rational::integer(1).div(norm)?);
            return Ok(Some(conjugate.mul(&scale, budget)?));
        }
        Ok(None)
    }
    fn sqrt(&self, budget: &mut Budget) -> Result<Option<Self>> {
        let Some(r) = self.as_rational() else {
            return Ok(None);
        };
        if r.n < 0 {
            return Err(MathError::Domain("负数没有实平方根".into()));
        }
        if r.n == 0 {
            return Ok(Some(self.clone()));
        }
        let (a, b) = budget.square_parts(r.n)?;
        let (c, d) = budget.square_parts(r.d)?;
        let root = checked(b.checked_mul(d))?;
        let coefficient = Rational::new(a, checked(c.checked_mul(d))?)?;
        Ok(Some(Self(BTreeMap::from([(root, coefficient)]))))
    }
    fn pow(&self, exponent: Rational, budget: &mut Budget) -> Result<Option<Self>> {
        if self.0.is_empty() && exponent.n <= 0 {
            return Err(MathError::Domain(
                "该幂在实数范围内未定义（包括 0^0）".into(),
            ));
        }
        if exponent.d != 1 {
            if self.as_rational().is_some_and(|r| r.n < 0) {
                return Err(MathError::Domain(
                    "负底数的非整数幂不在支持的实数定义域".into(),
                ));
            }
            if exponent.n == 1 && exponent.d == 2 {
                return self.sqrt(budget);
            }
            return Ok(None);
        }
        if exponent.n.abs() > 128 {
            return Err(limit());
        }
        let mut power = exponent.n.unsigned_abs();
        let mut base = if exponent.n < 0 {
            let Some(inverse) = self.inverse(budget)? else {
                return Ok(None);
            };
            inverse
        } else {
            self.clone()
        };
        let mut result = Self::rational(Rational::integer(1));
        while power != 0 {
            if power % 2 == 1 {
                result = result.mul(&base, budget)?;
            }
            power /= 2;
            if power != 0 {
                base = base.mul(&base, budget)?;
            }
        }
        Ok(Some(result))
    }
    fn display(&self) -> Result<(String, MathDisplay)> {
        if self.0.is_empty() {
            return Ok(("0".into(), text("0")));
        }
        let mut denominator = 1_i128;
        for c in self.0.values() {
            denominator = checked((denominator / gcd(denominator, c.d)).checked_mul(c.d))?;
        }
        let mut pieces = Vec::new();
        let mut plain = String::new();
        for (&root, &c) in &self.0 {
            let n = checked(c.n.checked_mul(denominator / c.d))?;
            let sign = if n < 0 {
                if pieces.is_empty() { "-" } else { " - " }
            } else if pieces.is_empty() {
                ""
            } else {
                " + "
            };
            if !sign.is_empty() {
                pieces.push(text(sign));
                plain.push_str(sign);
            }
            if root == 1 || n.abs() != 1 {
                pieces.push(text(n.abs().to_string()));
                plain.push_str(&n.abs().to_string());
                if root != 1 {
                    plain.push('*');
                }
            }
            if root != 1 {
                pieces.push(MathDisplay::Radical(Box::new(text(root.to_string()))));
                plain.push_str(&format!("sqrt({root})"));
            }
        }
        let numerator = if pieces.len() == 1 {
            pieces.pop().unwrap()
        } else {
            MathDisplay::Row(pieces)
        };
        if denominator == 1 {
            return Ok((plain, numerator));
        }
        if self.0.len() > 1 {
            plain = format!("({plain})");
        }
        Ok((
            format!("{plain}/{denominator}"),
            MathDisplay::Fraction(Box::new(numerator), Box::new(text(denominator.to_string()))),
        ))
    }
}
fn text(s: impl Into<String>) -> MathDisplay {
    MathDisplay::Text(s.into())
}

fn eval_exact<'a>(
    node: &Node,
    literals: &mut impl Iterator<Item = &'a str>,
    budget: &mut Budget,
) -> Result<Option<Exact>> {
    budget.take()?;
    match node {
        Node::Number(_) => {
            let literal = literals.next().unwrap();
            if matches!(literal, "pi" | "e") {
                return Ok(None);
            }
            Ok(Some(Exact::rational(Rational::literal(literal)?)))
        }
        Node::Variable(c) => Err(MathError::MissingVariable(*c)),
        Node::Neg(a) => Ok(eval_exact(a, literals, budget)?.map(Exact::neg)),
        Node::Call(f, a) => {
            let value = eval_exact(a, literals, budget)?;
            let Some(value) = value else {
                return Ok(None);
            };
            match f {
                Function::Sqrt => value.sqrt(budget),
                Function::Abs => Ok(value
                    .as_rational()
                    .map(|r| Exact::rational(Rational { n: r.n.abs(), ..r }))),
                _ => Ok(None),
            }
        }
        Node::Binary(op, a, b) => {
            // 即便另一分支不支持精确计算，也遍历全部子式以保留预算和定义域错误。
            let a = eval_exact(a, literals, budget)?;
            let b = eval_exact(b, literals, budget)?;
            if matches!(op, Op::Div) && b.as_ref().is_some_and(|b| b.0.is_empty()) {
                return Err(MathError::Domain("除数为零".into()));
            }
            let (Some(a), Some(b)) = (a, b) else {
                return Ok(None);
            };
            match op {
                Op::Add => Ok(Some(a.add(b)?)),
                Op::Sub => Ok(Some(a.add(b.neg())?)),
                Op::Mul => Ok(Some(a.mul(&b, budget)?)),
                Op::Div => match b.inverse(budget)? {
                    Some(inverse) => Ok(Some(a.mul(&inverse, budget)?)),
                    None => Ok(None),
                },
                Op::Pow => match b.as_rational() {
                    Some(exponent) => a.pow(exponent, budget),
                    None => Ok(None),
                },
            }
        }
    }
}

pub(super) fn constant(input: &str) -> Result<DisplayCalculation> {
    let tokens = lex(input)?;
    let mut parser = Parser {
        tokens: tokens.clone(),
        pos: 0,
    };
    let (node, _) = parser.expr(0, 0)?;
    if parser.tokens[parser.pos] != Token::End {
        return Err(MathError::Syntax("表达式末尾有多余内容".into()));
    }
    let mut literals = tokens.iter().filter_map(|t| match t {
        Token::Number(_, literal) => Some(literal.as_str()),
        _ => None,
    });
    let mut budget = Budget {
        operations: 10_000,
        factors: 20_000,
    };
    match eval_exact(&node, &mut literals, &mut budget)? {
        Some(value) => {
            let (text, display) = value.display()?;
            Ok(DisplayCalculation {
                text,
                display: Some(display),
                approximate: false,
            })
        }
        None => {
            let text = format!("≈ {}", display_number(node.eval(None, None)?));
            Ok(DisplayCalculation {
                display: Some(MathDisplay::Text(text.clone())),
                text,
                approximate: true,
            })
        }
    }
}
