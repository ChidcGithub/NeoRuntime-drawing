//! 符号计算内核（CAS）：精确有理数、自动规范化的表达式树、一元多项式引擎。
//!
//! 阶段 1（内核）：不改变任何公开 API 与既有输出；阶段 2-4 逐步把解方程、
//! 微积分、求点接到本内核上，接线完成后移除底部的 `allow(dead_code)`。
//!
//! 设计约定：
//! - 系数用 i128 精确有理数，溢出映射为 `MathError::Limit`（与 exact.rs 同风格）；
//! - 浮点字面量按最短往返十进制转成精确十进制有理数（0.1 → 1/10，不引入二进制噪声）；
//! - `Add`/`Mul` 构造即规范化：折叠常量、合并同类项/同底幂、规范排序；
//! - 多项式视图把树投影为 (分子, 分母) 一元多项式对，供解方程、化简、部分分式使用；
//! - 实根求解：精确有理根（有理根定理 + 降幂）→ 二次稳定公式 → 其余次数经
//!   导数临界点递归隔离 + 二分（多项式实根落在柯西界内，隔离是完备的，不再依赖采样运气）。

use super::{Function, MathError, Node, Op, Result};
use std::fmt;

/// 有理数上限：分子分母与中间量超过即报 Limit，与 exact.rs 的 i128 预算一致。
const RAT_LIMIT: &str = "精确计算超出 i128、128 次幂、32 项或运算预算";
/// 符号幂与多项式次数上限；高于现有代数路径的 12，覆盖高次方程教学场景。
const MAX_POWER: i64 = 128;
const MAX_DEGREE: usize = 24;

fn limit() -> MathError {
    MathError::Limit(RAT_LIMIT.into())
}

fn gcd(mut a: i128, mut b: i128) -> i128 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn checked(value: Option<i128>) -> Result<i128> {
    value.filter(|v| *v != i128::MIN).ok_or_else(limit)
}

/// 精确有理数；分母恒为正且与分子互素。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Rat {
    num: i128,
    den: i128,
}

impl Rat {
    pub(crate) fn new(num: i128, den: i128) -> Result<Self> {
        if den == 0 {
            return Err(MathError::Domain("分母为零".into()));
        }
        let (num, den) = if den < 0 {
            (checked(num.checked_neg())?, checked(den.checked_neg())?)
        } else {
            (num, den)
        };
        let g = gcd(num, den);
        let (num, den) = if g > 1 {
            (num / g, den / g)
        } else {
            (num, den)
        };
        Ok(Self { num, den })
    }

    pub(crate) fn zero() -> Self {
        Self { num: 0, den: 1 }
    }

    pub(crate) fn one() -> Self {
        Self { num: 1, den: 1 }
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.num == 0
    }

    pub(crate) fn num(&self) -> i128 {
        self.num
    }

    pub(crate) fn den(&self) -> i128 {
        self.den
    }

    /// 浮点字面量 → 精确十进制有理数：按最短往返十进制解析（0.1 → 1/10）。
    pub(crate) fn from_f64(v: f64) -> Result<Self> {
        if !v.is_finite() {
            return Err(MathError::Domain("结果不是有限实数".into()));
        }
        let text = format!("{v}");
        if text.contains(['e', 'E']) {
            return Err(limit());
        }
        let (sign, digits) = match text.strip_prefix('-') {
            Some(rest) => (-1_i128, rest),
            None => (1_i128, text.as_str()),
        };
        let (int_part, frac_part) = match digits.split_once('.') {
            Some((a, b)) => (a, b),
            None => (digits, ""),
        };
        if int_part.len() + frac_part.len() > 30 || frac_part.len() > 18 {
            return Err(limit());
        }
        let mut num: i128 = 0;
        for c in int_part.chars().chain(frac_part.chars()) {
            let digit = c
                .to_digit(10)
                .ok_or_else(|| MathError::Syntax("系数不是十进制数".into()))?
                as i128;
            num = checked(checked(num.checked_mul(10))?.checked_add(digit))?;
        }
        let den = 10_i128
            .checked_pow(frac_part.len() as u32)
            .ok_or_else(limit)?;
        Self::new(checked(sign.checked_mul(num))?, den)
    }

    pub(crate) fn to_f64(&self) -> f64 {
        self.num as f64 / self.den as f64
    }

    pub(crate) fn add(self, other: Self) -> Result<Self> {
        // 先约去分母公因子，缩小交叉乘法的溢出风险。
        let g = gcd(self.den, other.den).max(1);
        let self_den = self.den / g;
        let den = checked(self_den.checked_mul(other.den))?;
        let num = checked(self.num.checked_mul(other.den / g))?
            .checked_add(checked(other.num.checked_mul(self_den))?);
        Self::new(checked(num)?, den)
    }

    pub(crate) fn neg(self) -> Result<Self> {
        Self::new(checked(self.num.checked_neg())?, self.den)
    }

    pub(crate) fn sub(self, other: Self) -> Result<Self> {
        self.add(other.neg()?)
    }

    pub(crate) fn mul(self, other: Self) -> Result<Self> {
        // 交叉约分后再乘，尽量避免中间量溢出。
        let g1 = gcd(self.num, other.den).max(1);
        let g2 = gcd(other.num, self.den).max(1);
        let num =
            checked(self.num.checked_div(g1))?.checked_mul(checked(other.num.checked_div(g2))?);
        let den =
            checked(self.den.checked_div(g2))?.checked_mul(checked(other.den.checked_div(g1))?);
        Self::new(checked(num)?, checked(den)?)
    }

    pub(crate) fn inv(self) -> Result<Self> {
        if self.num == 0 {
            return Err(MathError::Domain("除数为零".into()));
        }
        Self::new(self.den, self.num)
    }

    pub(crate) fn div(self, other: Self) -> Result<Self> {
        self.mul(other.inv()?)
    }

    pub(crate) fn pow(self, exponent: i64) -> Result<Self> {
        if exponent.abs() > MAX_POWER {
            return Err(limit());
        }
        if exponent < 0 {
            return self.inv()?.pow(-exponent);
        }
        // 平方乘法，途中溢出即报 Limit。
        let mut result = Self::one();
        let mut base = self;
        let mut e = exponent;
        while e > 0 {
            if e & 1 == 1 {
                result = result.mul(base)?;
            }
            e >>= 1;
            if e > 0 {
                base = base.mul(base)?;
            }
        }
        Ok(result)
    }
}

impl fmt::Display for Rat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            return write!(f, "{}", self.num);
        }
        // 分母只含 2/5 因子时按有限小数显示（与输入书写形式一致），否则最简分数。
        let (mut rest, mut twos, mut fives) = (self.den, 0u32, 0u32);
        while rest % 2 == 0 {
            rest /= 2;
            twos += 1;
        }
        while rest % 5 == 0 {
            rest /= 5;
            fives += 1;
        }
        if rest != 1 {
            return write!(f, "{}/{}", self.num, self.den);
        }
        let digits = (twos.max(fives)) as usize;
        if digits > 18 {
            return write!(f, "{}/{}", self.num, self.den);
        }
        let scale = 10_i128.pow(digits as u32);
        let scaled = checked(
            self.num
                .checked_mul(scale.checked_div(self.den).ok_or_else(|| fmt::Error)?),
        )
        .map_err(|_| fmt::Error)?;
        let sign = if scaled < 0 { "-" } else { "" };
        let magnitude = scaled.abs();
        let int_part = magnitude / scale;
        let frac_part = magnitude % scale;
        if frac_part == 0 {
            return write!(f, "{sign}{int_part}");
        }
        write!(f, "{sign}{int_part}.{frac_part:0digits$}")
    }
}

/// 符号表达式树；`Add`/`Mul` 构造即规范化（见模块注释）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Sym {
    Rat(Rat),
    Var(char),
    /// 至少两项；至多一个有理常数（排在首位）；同类项已合并。
    Add(Vec<Sym>),
    /// 至少两个因子；至多一个有理常数（排在首位）；同底幂已合并。
    Mul(Vec<Sym>),
    /// 底数非 Rat；指数为非零整数，|指数| ≤ 128。
    Pow(Box<Sym>, i64),
    Func(Function, Box<Sym>),
}

/// 规范排序用的确定性序列化键：子节点已排序，故整棵树的键是规范的。
fn key(sym: &Sym) -> String {
    match sym {
        Sym::Rat(r) => format!("#{}%{}", r.num(), r.den()),
        Sym::Var(c) => format!("@{c}"),
        Sym::Add(terms) => {
            let inner: Vec<_> = terms.iter().map(key).collect();
            format!("(+{})", inner.join(","))
        }
        Sym::Mul(factors) => {
            let inner: Vec<_> = factors.iter().map(key).collect();
            format!("(*{})", inner.join(","))
        }
        Sym::Pow(base, exponent) => format!("(^{};{exponent})", key(base)),
        Sym::Func(f, arg) => format!("({}{}))", func_name(f), key(arg)),
    }
}

fn func_name(f: &Function) -> &'static str {
    match f {
        Function::Sin => "sin",
        Function::Cos => "cos",
        Function::Tan => "tan",
        Function::Asin => "asin",
        Function::Acos => "acos",
        Function::Atan => "atan",
        Function::Ln => "ln",
        Function::Log => "log",
        Function::Sqrt => "sqrt",
        Function::Abs => "abs",
        Function::Exp => "exp",
        Function::Sinh => "sinh",
        Function::Cosh => "cosh",
        Function::Tanh => "tanh",
        Function::Asinh => "asinh",
        Function::Acosh => "acosh",
        Function::Atanh => "atanh",
    }
}

/// 把一个乘积项拆成（有理系数，剩余因子）；剩余为 None 表示纯常数。
fn split_coefficient(sym: &Sym) -> Result<(Rat, Option<Sym>)> {
    match sym {
        Sym::Rat(r) => Ok((*r, None)),
        Sym::Mul(factors) => {
            let mut coefficient = Rat::one();
            let mut rest = Vec::new();
            for factor in factors {
                match factor {
                    Sym::Rat(r) => coefficient = coefficient.mul(*r)?,
                    other => rest.push(other.clone()),
                }
            }
            let core = match rest.len() {
                0 => None,
                1 => Some(rest.remove(0)),
                _ => Some(Sym::Mul(rest)),
            };
            Ok((coefficient, core))
        }
        other => Ok((Rat::one(), Some(other.clone()))),
    }
}

/// 规范化求和：扁平化、折叠常数、按同类项合并、规范排序。
pub(crate) fn add_terms(terms: Vec<Sym>) -> Result<Sym> {
    let mut flat = Vec::new();
    for term in terms {
        match term {
            Sym::Add(inner) => flat.extend(inner),
            other => flat.push(other),
        }
    }
    let mut constant = Rat::zero();
    // key(剩余因子) → (系数, 剩余因子)
    let mut merged: Vec<(String, Rat, Sym)> = Vec::new();
    for term in flat {
        let (coefficient, core) = split_coefficient(&term)?;
        match core {
            None => constant = constant.add(coefficient)?,
            Some(core) => {
                let k = key(&core);
                if let Some(entry) = merged.iter_mut().find(|(existing, _, _)| *existing == k) {
                    entry.1 = entry.1.add(coefficient)?;
                } else {
                    merged.push((k, coefficient, core));
                }
            }
        }
    }
    merged.retain(|(_, coefficient, _)| !coefficient.is_zero());
    let mut terms: Vec<Sym> = Vec::new();
    if !constant.is_zero() {
        terms.push(Sym::Rat(constant));
    }
    merged.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, coefficient, core) in merged {
        if coefficient == Rat::one() {
            terms.push(core);
        } else {
            terms.push(mul_terms(vec![Sym::Rat(coefficient), core])?);
        }
    }
    Ok(match terms.len() {
        0 => Sym::Rat(Rat::zero()),
        1 => terms.remove(0),
        _ => Sym::Add(terms),
    })
}

/// 规范化乘积：扁平化、折叠常数、合并同底幂、规范排序；不对和式展开。
pub(crate) fn mul_terms(factors: Vec<Sym>) -> Result<Sym> {
    let mut flat = Vec::new();
    for factor in factors {
        match factor {
            Sym::Mul(inner) => flat.extend(inner),
            other => flat.push(other),
        }
    }
    let mut constant = Rat::one();
    // 底数键 → (底数, 指数和)
    let mut merged: Vec<(String, Sym, i64)> = Vec::new();
    for factor in flat {
        match factor {
            Sym::Rat(r) => {
                if r.is_zero() {
                    return Ok(Sym::Rat(Rat::zero()));
                }
                constant = constant.mul(r)?;
            }
            Sym::Pow(base, exponent) => {
                if exponent.abs() > MAX_POWER {
                    return Err(limit());
                }
                let k = key(&base);
                if let Some(entry) = merged.iter_mut().find(|(existing, _, _)| *existing == k) {
                    entry.2 = entry
                        .2
                        .checked_add(exponent)
                        .filter(|t| t.abs() <= MAX_POWER)
                        .ok_or_else(limit)?;
                } else {
                    merged.push((k, *base, exponent));
                }
            }
            other => {
                let k = key(&other);
                if let Some(entry) = merged.iter_mut().find(|(existing, _, _)| *existing == k) {
                    entry.2 = entry.2.checked_add(1).ok_or_else(limit)?;
                } else {
                    merged.push((k, other, 1));
                }
            }
        }
    }
    let mut factors: Vec<Sym> = Vec::new();
    if constant != Rat::one() {
        factors.push(Sym::Rat(constant));
    }
    merged.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, base, exponent) in merged {
        match exponent {
            0 => continue,
            1 => factors.push(base),
            _ => factors.push(Sym::Pow(Box::new(base), exponent)),
        }
    }
    Ok(match factors.len() {
        0 => Sym::Rat(Rat::one()),
        1 => factors.remove(0),
        _ => Sym::Mul(factors),
    })
}

fn pow_sym(base: Sym, exponent: i64) -> Result<Sym> {
    if exponent.abs() > MAX_POWER {
        return Err(limit());
    }
    match base {
        Sym::Rat(r) => Ok(Sym::Rat(r.pow(exponent)?)),
        other => {
            // 幂的幂折叠：(b^a)^e = b^(a*e)；b == 0 且 e <= 0 的定义域限制
            // 由求值路径负责，符号层面按 CAS 约定折叠。
            let folded = match &other {
                Sym::Pow(inner, e) => Some((
                    (**inner).clone(),
                    e.checked_mul(exponent)
                        .filter(|t| t.abs() <= MAX_POWER)
                        .ok_or_else(limit)?,
                )),
                _ => None,
            };
            match folded {
                Some((_inner, 0)) => Ok(Sym::Rat(Rat::one())),
                Some((inner, 1)) => Ok(inner),
                Some((Sym::Rat(r), total)) => Ok(Sym::Rat(r.pow(total)?)),
                Some((inner, total)) => Ok(Sym::Pow(Box::new(inner), total)),
                None if exponent == 0 => Ok(Sym::Rat(Rat::one())),
                None => Ok(Sym::Pow(Box::new(other), exponent)),
            }
        }
    }
}

/// 从既有解析 AST 构建（沿用预算与深度限制）。
pub(crate) fn sym_from_node(node: &Node) -> Result<Sym> {
    match node {
        Node::Number(v) => Ok(Sym::Rat(Rat::from_f64(*v)?)),
        Node::Variable(c) => Ok(Sym::Var(*c)),
        Node::Neg(n) => mul_terms(vec![Sym::Rat(Rat::new(-1, 1)?), sym_from_node(n)?]),
        Node::Call(f, arg) => Ok(Sym::Func(*f, Box::new(sym_from_node(arg)?))),
        Node::Binary(op, a, b) => {
            let a = sym_from_node(a)?;
            let b = sym_from_node(b)?;
            match op {
                Op::Add => add_terms(vec![a, b]),
                Op::Sub => add_terms(vec![a, mul_terms(vec![Sym::Rat(Rat::new(-1, 1)?), b])?]),
                Op::Mul => mul_terms(vec![a, b]),
                Op::Div => match b {
                    Sym::Rat(r) => mul_terms(vec![a, Sym::Rat(r.inv()?)]),
                    other => mul_terms(vec![a, pow_sym(other, -1)?]),
                },
                Op::Pow => match b {
                    // 指数必须是有理常数且为整数；无理/分式指数保持不支持，
                    // 与现有求值路径的实数幂语义一致。
                    Sym::Rat(r) if r.den() == 1 => {
                        pow_sym(a, r.num().try_into().map_err(|_| limit())?)
                    }
                    Sym::Rat(_) => Err(MathError::Unsupported(
                        "符号引擎指数仅支持整数；无理指数请用数值路径".into(),
                    )),
                    other => Err(MathError::Unsupported(format!(
                        "符号引擎指数必须是常数，不支持 {other:?}"
                    ))),
                },
            }
        }
    }
}

/// 一元多项式（有理系数），下标即次数，末尾无零系数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Poly {
    coefficients: Vec<Rat>,
}

impl Poly {
    pub(crate) fn zero() -> Self {
        Self {
            coefficients: Vec::new(),
        }
    }

    pub(crate) fn one() -> Self {
        Self {
            coefficients: vec![Rat::one()],
        }
    }

    pub(crate) fn constant(v: Rat) -> Self {
        if v.is_zero() {
            Self::zero()
        } else {
            Self {
                coefficients: vec![v],
            }
        }
    }

    /// 变量本身：x。
    pub(crate) fn variable() -> Self {
        Self {
            coefficients: vec![Rat::zero(), Rat::one()],
        }
    }

    /// 由系数向量构造（下标 = 次数），裁掉尾部零项。
    pub(crate) fn from_coefficients(coefficients: Vec<Rat>) -> Self {
        Self { coefficients }.trim()
    }

    pub(crate) fn degree(&self) -> usize {
        self.coefficients.len().checked_sub(1).unwrap_or(0)
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.coefficients.is_empty()
    }

    pub(crate) fn coefficient(&self, degree: usize) -> Rat {
        self.coefficients
            .get(degree)
            .copied()
            .unwrap_or(Rat::zero())
    }

    fn trim(mut self) -> Self {
        while self.coefficients.last().is_some_and(|c| c.is_zero()) {
            self.coefficients.pop();
        }
        self
    }

    pub(crate) fn add(&self, other: &Self) -> Result<Self> {
        let len = self.coefficients.len().max(other.coefficients.len());
        if len > MAX_DEGREE + 1 {
            return Err(limit());
        }
        let mut out = Vec::with_capacity(len);
        for index in 0..len {
            out.push(self.coefficient(index).add(other.coefficient(index))?);
        }
        Ok(Self { coefficients: out }.trim())
    }

    pub(crate) fn neg(&self) -> Result<Self> {
        let mut out = Vec::with_capacity(self.coefficients.len());
        for c in &self.coefficients {
            out.push(c.neg()?);
        }
        Ok(Self { coefficients: out }.trim())
    }

    pub(crate) fn sub(&self, other: &Self) -> Result<Self> {
        self.add(&other.neg()?)
    }

    pub(crate) fn mul(&self, other: &Self) -> Result<Self> {
        if !self.is_zero() && !other.is_zero() && self.degree() + other.degree() > MAX_DEGREE {
            return Err(limit());
        }
        let mut out = vec![Rat::zero(); self.coefficients.len() + other.coefficients.len()];
        for (i, &a) in self.coefficients.iter().enumerate() {
            for (j, &b) in other.coefficients.iter().enumerate() {
                out[i + j] = out[i + j].add(a.mul(b)?)?;
            }
        }
        Ok(Self { coefficients: out }.trim())
    }

    pub(crate) fn pow(&self, exponent: usize) -> Result<Self> {
        if self.degree() * exponent > MAX_DEGREE {
            return Err(limit());
        }
        let mut out = Self::one();
        for _ in 0..exponent {
            out = out.mul(self)?;
        }
        Ok(out)
    }

    /// 商与余式（余式次数 < 除式次数）；除零多项式报定义域错误。
    pub(crate) fn divmod(&self, other: &Self) -> Result<(Self, Self)> {
        if other.is_zero() {
            return Err(MathError::Domain("多项式除以零".into()));
        }
        let mut quotient = vec![Rat::zero(); self.degree() + 1];
        let mut remainder = self.clone();
        let lead = other.coefficient(other.degree());
        while !remainder.is_zero() && remainder.degree() >= other.degree() {
            let shift = remainder.degree() - other.degree();
            let factor = remainder.coefficient(remainder.degree()).div(lead)?;
            quotient[shift] = factor;
            let mut sub = vec![Rat::zero(); shift + 1];
            sub[shift] = factor;
            remainder = remainder.sub(&other.mul(&Self { coefficients: sub })?)?;
        }
        Ok((
            Self {
                coefficients: quotient,
            }
            .trim(),
            remainder.trim(),
        ))
    }

    /// 首一化的最大公因式（欧几里得）；两个零多项式的 gcd 为零。
    pub(crate) fn gcd(&self, other: &Self) -> Result<Self> {
        let mut a = self.clone();
        let mut b = other.clone();
        while !b.is_zero() {
            let (_, remainder) = a.divmod(&b)?;
            a = b;
            b = remainder;
        }
        if a.is_zero() {
            return Ok(Self::zero());
        }
        // 首一化，保证规范输出。
        let lead = a.coefficient(a.degree());
        let mut out = Vec::with_capacity(a.coefficients.len());
        for c in &a.coefficients {
            out.push(c.div(lead)?);
        }
        Ok(Self { coefficients: out }.trim())
    }

    /// d/dx。
    pub(crate) fn derivative(&self) -> Result<Self> {
        if self.coefficients.len() <= 1 {
            return Ok(Self::zero());
        }
        let mut out = Vec::with_capacity(self.coefficients.len() - 1);
        for (index, c) in self.coefficients.iter().enumerate().skip(1) {
            out.push(c.mul(Rat::new(index as i128, 1)?)?);
        }
        Ok(Self { coefficients: out }.trim())
    }

    pub(crate) fn eval_rat(&self, x: Rat) -> Result<Rat> {
        let mut acc = Rat::zero();
        for c in self.coefficients.iter().rev() {
            acc = acc.mul(x)?.add(*c)?;
        }
        Ok(acc)
    }

    pub(crate) fn eval_f64(&self, x: f64) -> f64 {
        let mut acc = 0.0;
        for c in self.coefficients.iter().rev() {
            acc = acc * x + c.to_f64();
        }
        acc
    }
}

/// 把符号树投影为一元有理函数 (分子, 分母)；含函数符号或其他变量时返回 None。
pub(crate) fn rational_function(sym: &Sym, variable: char) -> Result<Option<(Poly, Poly)>> {
    match sym {
        Sym::Rat(r) => Ok(Some((Poly::constant(*r), Poly::one()))),
        Sym::Var(c) => {
            if *c == variable {
                Ok(Some((Poly::variable(), Poly::one())))
            } else {
                Ok(None)
            }
        }
        Sym::Add(terms) => {
            let mut num = Poly::zero();
            let mut den = Poly::one();
            for term in terms {
                let Some((tn, td)) = rational_function(term, variable)? else {
                    return Ok(None);
                };
                // num = num*td + tn*den；den = den*td
                num = num.mul(&td)?.add(&tn.mul(&den)?)?;
                den = den.mul(&td)?;
                if den.degree() > MAX_DEGREE || num.degree() > MAX_DEGREE {
                    return Err(limit());
                }
            }
            Ok(Some((num, den)))
        }
        Sym::Mul(factors) => {
            let mut num = Poly::one();
            let mut den = Poly::one();
            for factor in factors {
                let Some((fn_, fd)) = rational_function(factor, variable)? else {
                    return Ok(None);
                };
                num = num.mul(&fn_)?;
                den = den.mul(&fd)?;
                if den.degree() > MAX_DEGREE || num.degree() > MAX_DEGREE {
                    return Err(limit());
                }
            }
            Ok(Some((num, den)))
        }
        Sym::Pow(base, exponent) => {
            let Some((bn, bd)) = rational_function(base, variable)? else {
                return Ok(None);
            };
            let (num, den) = if *exponent >= 0 {
                (bn.pow(*exponent as usize)?, bd.pow(*exponent as usize)?)
            } else {
                // 负指数：分子分母互换；零分子报定义域错误。
                if bn.is_zero() {
                    return Err(MathError::Domain("零的负次幂没有定义".into()));
                }
                (bd.pow((-exponent) as usize)?, bn.pow((-exponent) as usize)?)
            };
            Ok(Some((num, den)))
        }
        Sym::Func(_, _) => Ok(None),
    }
}

/// 一元多项式的全部实根（去重、升序）。
/// 精确有理根先行（有理根定理 + 降幂）；其余次数：1 次精确、2 次稳定二次公式、
/// ≥3 次经导数临界点递归隔离 + 二分。多项式实根落在柯西界内，隔离是完备的。
pub(crate) fn real_roots(polynomial: &Poly) -> Result<Vec<f64>> {
    let mut roots: Vec<f64> = Vec::new();
    let mut remaining = polynomial.clone();
    // 1) 精确有理根：±p/q（p | 常数项，q | 首项），精确求值判定后降幂。
    loop {
        if remaining.is_zero() || remaining.degree() <= 2 {
            break;
        }
        let Some(root) = rational_root(&remaining)? else {
            break;
        };
        roots.push(root.to_f64());
        // 综合除法降幂：P(x) = (x - root) * Q(x)。
        let mut next = vec![Rat::zero(); remaining.degree()];
        let mut carry = Rat::zero();
        for index in (0..remaining.degree()).rev() {
            carry = remaining.coefficient(index + 1).add(carry.mul(root)?)?;
            next[index] = carry;
        }
        remaining = Poly { coefficients: next }.trim();
    }
    // 2) 剩余低次：1 次精确；2 次稳定二次公式（判别式精确比较）。
    match remaining.degree() {
        0 => {}
        1 => {
            let root = remaining
                .coefficient(0)
                .neg()?
                .div(remaining.coefficient(1))?;
            roots.push(root.to_f64());
        }
        2 => {
            let (c, b, a) = (
                remaining.coefficient(0),
                remaining.coefficient(1),
                remaining.coefficient(2),
            );
            // 稳定二次公式（与现有代数路径同风格）：q = -(b + sign(b)√d)/2。
            let discriminant = b.mul(b)?.sub(a.mul(c)?.mul(Rat::new(4, 1)?)?)?;
            if discriminant.is_zero() {
                roots.push(b.neg()?.div(a.mul(Rat::new(2, 1)?)?)?.to_f64());
            } else if discriminant.num() > 0 {
                let s = discriminant.to_f64().sqrt();
                let bf = b.to_f64();
                let q = -0.5 * (bf + s.copysign(bf));
                let af = a.to_f64();
                let cf = c.to_f64();
                let mut values = vec![q / af, cf / q];
                values.sort_by(f64::total_cmp);
                roots.extend(values);
            }
            // 判别式 < 0：无实根
        }
        _ => {
            // 3) 完备实根隔离：临界点（导数实根）划分单调区间，符号变化二分，
            //    临界点处 |f| 相对极小视为偶数重根。
            let bound = cauchy_bound(&remaining);
            let critical = real_roots(&remaining.derivative()?)?;
            let mut points: Vec<f64> = Vec::with_capacity(critical.len() + 2);
            points.push(-bound);
            points.extend(critical.iter().copied().filter(|c| c.abs() < bound));
            points.push(bound);
            points.sort_by(f64::total_cmp);
            points.dedup_by(|a, b| (*a - *b).abs() < 1e-12 * a.abs().max(1.0));
            for pair in points.windows(2) {
                let (lo, hi) = (pair[0], pair[1]);
                let mut flo = remaining.eval_f64(lo);
                let fhi = remaining.eval_f64(hi);
                if flo == 0.0 {
                    push_root(&mut roots, lo);
                }
                if flo * fhi < 0.0 {
                    let (mut a, mut b) = (lo, hi);
                    for _ in 0..200 {
                        let mid = 0.5 * (a + b);
                        if mid == a || mid == b {
                            break;
                        }
                        let fm = remaining.eval_f64(mid);
                        if fm == 0.0 {
                            a = mid;
                            break;
                        }
                        if flo * fm < 0.0 {
                            b = mid;
                        } else {
                            a = mid;
                            flo = fm;
                        }
                    }
                    push_root(&mut roots, 0.5 * (a + b));
                }
            }
            let last = points.last().copied().unwrap_or(bound);
            if remaining.eval_f64(last) == 0.0 {
                push_root(&mut roots, last);
            }
            // 偶数重根在临界点处不改号：|f| 相对极小即视为根。
            let scale = remaining
                .coefficients
                .iter()
                .fold(0.0_f64, |acc, c| acc.max(c.to_f64().abs()))
                .max(1.0);
            for c in &critical {
                if c.abs() < bound && remaining.eval_f64(*c).abs() <= scale * 1e-12 {
                    push_root(&mut roots, *c);
                }
            }
        }
    }
    roots.sort_by(f64::total_cmp);
    roots.dedup_by(|a, b| (*a - *b).abs() < 1e-9 * a.abs().max(1.0));
    Ok(roots)
}

fn push_root(roots: &mut Vec<f64>, value: f64) {
    if value.is_finite() {
        roots.push(value);
    }
}

/// 多项式实根的柯西界：1 + max(|a_i / a_n|)。
fn cauchy_bound(polynomial: &Poly) -> f64 {
    let lead = polynomial.coefficient(polynomial.degree()).to_f64();
    if lead == 0.0 {
        return 1.0;
    }
    let max = polynomial
        .coefficients
        .iter()
        .take(polynomial.degree())
        .fold(0.0_f64, |acc, c| acc.max(c.to_f64().abs() / lead.abs()));
    1.0 + max
}

/// 有理根定理：候选 ±p/q（p | 常数项，q | 首项），精确求值判定。
/// 常数项为 0 ⇒ 0 是根（由调用方降幂迭代捕获）；量级超出交给数值隔离。
fn rational_root(polynomial: &Poly) -> Result<Option<Rat>> {
    let degree = polynomial.degree();
    let constant = polynomial.coefficient(0);
    let lead = polynomial.coefficient(degree);
    if constant.is_zero() {
        return Ok(Some(Rat::zero()));
    }
    if constant.num().abs() > 10_i128.pow(6) || lead.num().abs() > 10_i128.pow(6) {
        return Ok(None);
    }
    let numerators = divisors(constant.num())?;
    let denominators = divisors(lead.num())?;
    for &q in &denominators {
        for &p in &numerators {
            for candidate in [Rat::new(p, q)?, Rat::new(-p, q)?] {
                if polynomial.eval_rat(candidate)? == Rat::zero() {
                    return Ok(Some(candidate));
                }
            }
        }
    }
    Ok(None)
}

/// 非零整数的全部正因子（含自身），升序。
fn divisors(value: i128) -> Result<Vec<i128>> {
    let v = value.abs();
    if v > 10_i128.pow(6) {
        return Err(limit());
    }
    let mut out = Vec::new();
    let mut index = 1_i128;
    while index * index <= v {
        if v % index == 0 {
            out.push(index);
            if index != v / index {
                out.push(v / index);
            }
        }
        index += 1;
    }
    out.sort_unstable();
    Ok(out)
}

// ============================================================
// 通用微积分（阶段 3）：任意初等函数求导、标准形积分、规范打印。
// 多项式输入仍走既有代数路径（输出逐字节不变）；这里只接管多项式路径
// 不支持的表达式（函数符号、变量分母、次数超过 12 等）。
// ============================================================

/// 通用符号求导 d/dvariable（另一变量视为常量）。
pub(crate) fn differentiate(input: &str, variable: char) -> Result<String> {
    if !matches!(variable, 'x' | 'y') {
        return Err(MathError::Unsupported("符号微积分变量仅支持 x 或 y".into()));
    }
    let sym = sym_from_node(&crate::parse(input)?.node)?;
    print_sym(&derivative_sym(&sym, variable)?)
}

fn derivative_sym(sym: &Sym, variable: char) -> Result<Sym> {
    match sym {
        Sym::Rat(_) => Ok(Sym::Rat(Rat::zero())),
        Sym::Var(c) => Ok(Sym::Rat(if *c == variable {
            Rat::one()
        } else {
            Rat::zero()
        })),
        Sym::Add(terms) => {
            let mut out = Vec::with_capacity(terms.len());
            for term in terms {
                out.push(derivative_sym(term, variable)?);
            }
            add_terms(out)
        }
        Sym::Mul(factors) => {
            // n 元乘积法则：Σ_i (∏_{j≠i} f_j)·f_i'。
            let mut out = Vec::with_capacity(factors.len());
            for i in 0..factors.len() {
                let mut term = Vec::with_capacity(factors.len());
                for (j, factor) in factors.iter().enumerate() {
                    if j != i {
                        term.push(factor.clone());
                    }
                }
                term.push(derivative_sym(&factors[i], variable)?);
                out.push(mul_terms(term)?);
            }
            add_terms(out)
        }
        Sym::Pow(base, e) => {
            // d(b^e) = e·b^(e-1)·b'（整数幂，负指数同样成立）。
            let mut factors = vec![Sym::Rat(Rat::new(i128::from(*e), 1)?)];
            if *e != 1 {
                factors.push(pow_sym((**base).clone(), e - 1)?);
            }
            factors.push(derivative_sym(base, variable)?);
            mul_terms(factors)
        }
        Sym::Func(f, u) => {
            let du = derivative_sym(u, variable)?;
            let outer = outer_derivative(*f, u)?;
            mul_terms(vec![outer, du])
        }
    }
}

/// 常用初等函数的外层导数 f'(u)；链式内层导数由调用方乘上。
fn outer_derivative(f: Function, u: &Sym) -> Result<Sym> {
    let neg_one = Rat::new(-1, 1)?;
    let one = Rat::one();
    match f {
        Function::Sin => Ok(Sym::Func(Function::Cos, Box::new(u.clone()))),
        Function::Cos => mul_terms(vec![
            Sym::Rat(neg_one),
            Sym::Func(Function::Sin, Box::new(u.clone())),
        ]),
        Function::Tan => pow_sym(Sym::Func(Function::Cos, Box::new(u.clone())), -2),
        Function::Exp => Ok(Sym::Func(Function::Exp, Box::new(u.clone()))),
        Function::Ln => pow_sym(u.clone(), -1),
        Function::Log => {
            // d(log u)/du = 1/(u·ln 10)。
            let inner = mul_terms(vec![
                u.clone(),
                Sym::Func(Function::Ln, Box::new(Sym::Rat(Rat::new(10, 1)?))),
            ])?;
            pow_sym(inner, -1)
        }
        Function::Sqrt => mul_terms(vec![
            Sym::Rat(Rat::new(1, 2)?),
            pow_sym(Sym::Func(Function::Sqrt, Box::new(u.clone())), -1)?,
        ]),
        Function::Asin | Function::Acos => {
            // 1/sqrt(1 - u^2)；acos 取负。
            let inside = add_terms(vec![
                Sym::Rat(one),
                mul_terms(vec![Sym::Rat(neg_one), pow_sym(u.clone(), 2)?])?,
            ])?;
            let d = pow_sym(Sym::Func(Function::Sqrt, Box::new(inside)), -1)?;
            if f == Function::Asin {
                Ok(d)
            } else {
                mul_terms(vec![Sym::Rat(neg_one), d])
            }
        }
        Function::Atan => {
            let inside = add_terms(vec![Sym::Rat(one), pow_sym(u.clone(), 2)?])?;
            pow_sym(inside, -1)
        }
        Function::Sinh => Ok(Sym::Func(Function::Cosh, Box::new(u.clone()))),
        Function::Cosh => Ok(Sym::Func(Function::Sinh, Box::new(u.clone()))),
        Function::Tanh => pow_sym(Sym::Func(Function::Cosh, Box::new(u.clone())), -2),
        Function::Asinh | Function::Acosh => {
            let sign = if f == Function::Asinh { one } else { neg_one };
            let inside = add_terms(vec![
                Sym::Rat(one),
                mul_terms(vec![Sym::Rat(sign), pow_sym(u.clone(), 2)?])?,
            ])?;
            pow_sym(Sym::Func(Function::Sqrt, Box::new(inside)), -1)
        }
        Function::Atanh => {
            let inside = add_terms(vec![
                Sym::Rat(one),
                mul_terms(vec![Sym::Rat(neg_one), pow_sym(u.clone(), 2)?])?,
            ])?;
            pow_sym(inside, -1)
        }
        Function::Abs => Err(MathError::Unsupported(
            "abs 不是处处可导的函数，不提供符号导数".into(),
        )),
    }
}

/// 标准形不定积分（积分常数取零）：幂、指数、对数、三角、sqrt 的线性复合，
/// 逐项处理；乘积需拆出系数后只剩一个核心因子（分部积分/有理分式暂不支持）。
pub(crate) fn integrate(input: &str, variable: char) -> Result<String> {
    if !matches!(variable, 'x' | 'y') {
        return Err(MathError::Unsupported("符号微积分变量仅支持 x 或 y".into()));
    }
    let sym = sym_from_node(&crate::parse(input)?.node)?;
    print_sym(&antiderivative_sym(&sym, variable)?)
}

fn antiderivative_sym(sym: &Sym, variable: char) -> Result<Sym> {
    let unsupported = || {
        MathError::Unsupported(
            "符号积分仅支持幂、指数、对数、三角与 sqrt 的线性复合；乘积（分部积分）与有理分式暂不支持"
                .into(),
        )
    };
    match sym {
        Sym::Rat(r) => {
            if r.is_zero() {
                Ok(Sym::Rat(Rat::zero()))
            } else {
                mul_terms(vec![Sym::Rat(*r), Sym::Var(variable)])
            }
        }
        Sym::Var(c) => {
            if *c == variable {
                // ∫x dx = x²/2。
                mul_terms(vec![
                    Sym::Rat(Rat::new(1, 2)?),
                    pow_sym(Sym::Var(variable), 2)?,
                ])
            } else {
                // ∫y dx = y·x（y 视为常量）。
                mul_terms(vec![sym.clone(), Sym::Var(variable)])
            }
        }
        Sym::Add(terms) => {
            let mut out = Vec::with_capacity(terms.len());
            for term in terms {
                out.push(antiderivative_sym(term, variable)?);
            }
            add_terms(out)
        }
        Sym::Mul(_) | Sym::Pow(_, _) | Sym::Func(_, _) => {
            let (coefficient, core) = split_coefficient(sym)?;
            if coefficient.is_zero() {
                return Ok(Sym::Rat(Rat::zero()));
            }
            let Some(core) = core else {
                return mul_terms(vec![Sym::Rat(coefficient), Sym::Var(variable)]);
            };
            let Some((scale, inner)) = antiderivative_core(&core, variable)? else {
                return Err(unsupported());
            };
            // ∫a·f(b·x+c)dx = (a/scale)·F(b·x+c)；scale 已并入斜率与幂次因子。
            mul_terms(vec![Sym::Rat(coefficient.div(scale)?), inner])
        }
    }
}

/// 核心因子的原函数 F(u) 与总缩放 scale（∫a·core dx = (a/scale)·F）；
/// None = 不是支持的标准形。
fn antiderivative_core(core: &Sym, variable: char) -> Result<Option<(Rat, Sym)>> {
    let neg_one = Rat::new(-1, 1)?;
    match core {
        Sym::Func(f, u) => {
            let u: &Sym = u;
            let Some((b, _)) = linear_parts(u, variable)? else {
                return Ok(None);
            };
            let big_f = match f {
                Function::Exp => Sym::Func(Function::Exp, Box::new(u.clone())),
                Function::Sin => mul_terms(vec![
                    Sym::Rat(neg_one),
                    Sym::Func(Function::Cos, Box::new(u.clone())),
                ])?,
                Function::Cos => Sym::Func(Function::Sin, Box::new(u.clone())),
                Function::Tan => mul_terms(vec![
                    Sym::Rat(neg_one),
                    Sym::Func(
                        Function::Ln,
                        Box::new(Sym::Func(Function::Cos, Box::new(u.clone()))),
                    ),
                ])?,
                Function::Ln => add_terms(vec![
                    mul_terms(vec![
                        u.clone(),
                        Sym::Func(Function::Ln, Box::new(u.clone())),
                    ])?,
                    mul_terms(vec![Sym::Rat(neg_one), u.clone()])?,
                ])?,
                Function::Sqrt => mul_terms(vec![
                    Sym::Rat(Rat::new(2, 3)?),
                    Sym::Func(Function::Sqrt, Box::new(pow_sym(u.clone(), 3)?)),
                ])?,
                _ => return Ok(None),
            };
            Ok(Some((b, big_f)))
        }
        Sym::Pow(base, e) => {
            // 幂：n ≠ -1 → u^(n+1)/(n+1)；n = -1 → ln(u)。
            let Some((b, _)) = linear_parts(base, variable)? else {
                return Ok(None);
            };
            if *e == -1 {
                return Ok(Some((
                    b,
                    Sym::Func(Function::Ln, Box::new((**base).clone())),
                )));
            }
            let next = e
                .checked_add(1)
                .filter(|n| n.abs() <= MAX_POWER)
                .ok_or_else(limit)?;
            let big_f = pow_sym((**base).clone(), next)?;
            let scale = b.mul(Rat::new(i128::from(next), 1)?)?;
            Ok(Some((scale, big_f)))
        }
        _ => Ok(None),
    }
}

/// u 是否为 variable 的一次式 b·variable + c（b ≠ 0）；返回 (b, c)。
fn linear_parts(u: &Sym, variable: char) -> Result<Option<(Rat, Rat)>> {
    match rational_function(u, variable)? {
        Some((num, den)) if den.degree() == 0 && num.degree() <= 1 => {
            let lead = den.coefficient(0);
            let b = num.coefficient(1).div(lead)?;
            if b.is_zero() {
                return Ok(None);
            }
            Ok(Some((b, num.coefficient(0).div(lead)?)))
        }
        _ => Ok(None),
    }
}

/// 规范输出：乘除沿用多项式记法（*、^），负指数折进分母，函数按名字(参数)，
/// 和式按「非负项在前、负项在后」以贴近多项式书写习惯。
pub(crate) fn print_sym(sym: &Sym) -> Result<String> {
    match sym {
        Sym::Rat(r) => Ok(r.to_string()),
        Sym::Var(c) => Ok(c.to_string()),
        Sym::Func(f, arg) => Ok(format!("{}({})", func_name(f), print_sym(arg)?)),
        Sym::Pow(base, e) => {
            if *e < 0 {
                let denominator = power_text(base, -*e)?;
                Ok(format!("1/{denominator}"))
            } else {
                power_text(base, *e)
            }
        }
        Sym::Mul(factors) => {
            let mut coefficient = Rat::one();
            let mut numerator: Vec<String> = Vec::new();
            let mut denominator: Vec<String> = Vec::new();
            for factor in factors {
                match factor {
                    Sym::Rat(r) => coefficient = coefficient.mul(*r)?,
                    Sym::Pow(base, e) if *e < 0 => {
                        denominator.push(power_text(base, -*e)?);
                    }
                    other => numerator.push(print_sym(other)?),
                }
            }
            if coefficient.is_zero() {
                return Ok("0".into());
            }
            let mut num_text = String::new();
            if coefficient == Rat::one() {
                // 分子非空时省略 1。
                if numerator.is_empty() {
                    num_text.push_str(&coefficient.to_string());
                }
            } else if coefficient == Rat::new(-1, 1)? && !numerator.is_empty() {
                num_text.push('-');
            } else {
                num_text.push_str(&coefficient.to_string());
                if !numerator.is_empty() {
                    num_text.push('*');
                }
            }
            num_text.push_str(&numerator.join("*"));
            if denominator.is_empty() {
                return Ok(num_text);
            }
            let den_text = if denominator.len() == 1 {
                denominator.remove(0)
            } else {
                format!("({})", denominator.join("*"))
            };
            if num_text.is_empty() || num_text == "-" {
                Ok(format!("1/{den_text}"))
            } else {
                Ok(format!("{num_text}/{den_text}"))
            }
        }
        Sym::Add(terms) => {
            let mut parts: Vec<(bool, String)> = Vec::with_capacity(terms.len());
            for term in terms {
                parts.push(signed_term(term)?);
            }
            let (negatives, positives): (Vec<_>, Vec<_>) =
                parts.into_iter().partition(|(negative, _)| *negative);
            let mut ordered = positives;
            ordered.extend(negatives);
            let mut out = String::new();
            for (index, (negative, text)) in ordered.iter().enumerate() {
                if index == 0 {
                    if *negative {
                        out.push('-');
                    }
                    out.push_str(text);
                } else {
                    out.push_str(if *negative { " - " } else { " + " });
                    out.push_str(text);
                }
            }
            Ok(out)
        }
    }
}

/// Add 层的符号提取：负系数项拆成（负号，系数取绝对值后的文本）。
fn signed_term(term: &Sym) -> Result<(bool, String)> {
    match term {
        Sym::Rat(r) => Ok((r.num() < 0, Rat::new(r.num().abs(), r.den())?.to_string())),
        Sym::Mul(factors)
            if factors
                .first()
                .is_some_and(|f| matches!(f, Sym::Rat(r) if r.num() < 0)) =>
        {
            let mut flipped = Vec::with_capacity(factors.len());
            for (index, factor) in factors.iter().enumerate() {
                if index == 0 {
                    if let Sym::Rat(r) = factor {
                        flipped.push(Sym::Rat(Rat::new(r.num().abs(), r.den())?));
                        continue;
                    }
                }
                flipped.push(factor.clone());
            }
            Ok((true, print_sym(&mul_terms(flipped)?)?))
        }
        other => Ok((false, print_sym(other)?)),
    }
}

fn power_text(base: &Sym, exponent: i64) -> Result<String> {
    if exponent == 1 {
        print_sym(base)
    } else {
        Ok(format!("{}^{exponent}", pow_base_text(base)?))
    }
}

/// 幂底数的打印：变量与函数不加括号（x^2、cos(x)^2），其余加括号。
fn pow_base_text(base: &Sym) -> Result<String> {
    match base {
        Sym::Var(_) | Sym::Func(_, _) => print_sym(base),
        _ => Ok(format!("({})", print_sym(base)?)),
    }
}
