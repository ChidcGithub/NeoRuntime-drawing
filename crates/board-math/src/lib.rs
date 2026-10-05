//! 有界的实数数学工具，不是通用计算机代数系统。
//! 三角函数使用弧度；log 为常用对数，ln 为自然对数。隐式乘法与 * 同级。
//! 接受全角 ASCII、×÷、Unicode −、π 和 ²；保留词元边界，不删除标识符内空白。
//! 可检测到的非零运算下溢返回数值错误，不将其当作精确零。
//! 输入最多 4096 字节、512 个词元，语法树深度最多 64；解析与求值 O(n)。
//! 多项式仅支持 x/y、正整数幂、常数除法，总次数最多 12；系数为 f64。
//! 为保留 0^0 的定义域限制，变量式的零次幂不作多项式化简。
//! 数值算法只处理指定有限区间，不保证发现全部根、交点或不连续点。
//! 根/交点去重另需 O(k²) 比较，k 受网格/种子上限约束；所有算法均非任意精度。

mod algebra;
mod exact;
mod numeric;
mod plot;
pub use algebra::{
    Polynomial, Solutions, SystemSolutions, antiderivative_polynomial, differentiate_polynomial,
    simplify, solve_equation, solve_linear_system,
};
pub use exact::{DisplayCalculation, MathDisplay, calculate_display_with_bounds};
pub use numeric::{
    Bounds2D, NumericOptions, Point, RootSearch, SampledCurve, SystemSearch, curve_intersections,
    derivative, integrate, roots, sample, solve_quadratic_system,
};
pub use plot::{
    CurveIntersection, IntersectionCurve, PlotKind, classify_plot, plot_expression, sample_plot,
};
use std::fmt;

pub const MAX_INPUT_BYTES: usize = 4096;
pub const MAX_TOKENS: usize = 512;
const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub enum MathError {
    Syntax(String),
    Domain(String),
    Unsupported(String),
    Limit(String),
    Numerical(String),
    MissingVariable(char),
}
impl fmt::Display for MathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(s) => write!(f, "语法错误：{s}"),
            Self::Domain(s) => write!(f, "定义域错误：{s}"),
            Self::Unsupported(s) => write!(f, "暂不支持：{s}"),
            Self::Limit(s) => write!(f, "超过计算限制：{s}"),
            Self::Numerical(s) => write!(f, "数值计算失败：{s}"),
            Self::MissingVariable(c) => write!(f, "变量 {c} 未赋值"),
        }
    }
}
impl std::error::Error for MathError {}
type Result<T> = std::result::Result<T, MathError>;
fn finite(v: f64) -> Result<f64> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(MathError::Domain("结果不是有限实数".into()))
    }
}

fn nonzero_result(value: f64, nonzero: bool) -> Result<f64> {
    if value == 0.0 && nonzero {
        Err(MathError::Numerical("非零结果下溢，不能当作零".into()))
    } else {
        finite(value)
    }
}
fn checked_ratio(value: f64, scale: f64) -> Result<f64> {
    nonzero_result(value / scale, value != 0.0)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}
#[derive(Debug, Clone, Copy, PartialEq)]
enum Function {
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Ln,
    Log,
    Sqrt,
    Abs,
    Exp,
    Sinh,
    Cosh,
    Tanh,
    Asinh,
    Acosh,
    Atanh,
}
#[derive(Debug, Clone)]
enum Node {
    Number(f64),
    Variable(char),
    Neg(Box<Node>),
    Binary(Op, Box<Node>, Box<Node>),
    Call(Function, Box<Node>),
}
/// 通过 parse 构建的受限 AST；内部节点不允许外部绕过深度限制。
#[derive(Debug, Clone)]
pub struct Expr {
    node: Node,
    size: usize,
}
impl Expr {
    pub fn eval(&self, x: Option<f64>, y: Option<f64>) -> Result<f64> {
        if let Some(v) = x {
            finite(v)?;
        }
        if let Some(v) = y {
            finite(v)?;
        }
        self.node.eval(x, y)
    }
    pub fn eval_xy(&self, x: f64, y: f64) -> Result<f64> {
        self.eval(Some(x), Some(y))
    }
}
impl Node {
    fn eval(&self, x: Option<f64>, y: Option<f64>) -> Result<f64> {
        let value = match self {
            Self::Number(v) => *v,
            Self::Variable(c) => match c {
                'x' => x,
                _ => y,
            }
            .ok_or(MathError::MissingVariable(*c))?,
            Self::Neg(a) => -a.eval(x, y)?,
            Self::Binary(op, a, b) => {
                let a = a.eval(x, y)?;
                let b = b.eval(x, y)?;
                match op {
                    Op::Add => a + b,
                    Op::Sub => a - b,
                    Op::Mul => nonzero_result(a * b, a != 0.0 && b != 0.0)?,
                    Op::Div => {
                        if b == 0.0 {
                            return Err(MathError::Domain("除数为零".into()));
                        }
                        checked_ratio(a, b)?
                    }
                    Op::Pow => {
                        if (a == 0.0 && b <= 0.0) || (a < 0.0 && b.fract() != 0.0) {
                            return Err(MathError::Domain(
                                "该幂在实数范围内未定义（包括 0^0）".into(),
                            ));
                        }
                        nonzero_result(a.powf(b), a != 0.0)?
                    }
                }
            }
            Self::Call(fun, a) => {
                let a = a.eval(x, y)?;
                match fun {
                    Function::Sin => a.sin(),
                    Function::Cos => a.cos(),
                    Function::Tan => {
                        if a.cos().abs() < 1e-14 {
                            return Err(MathError::Domain("正切函数极点".into()));
                        }
                        a.tan()
                    }
                    Function::Asin => a.asin(),
                    Function::Acos => a.acos(),
                    Function::Atan => a.atan(),
                    Function::Ln => a.ln(),
                    Function::Log => a.log10(),
                    Function::Sqrt => a.sqrt(),
                    Function::Abs => a.abs(),
                    Function::Exp => nonzero_result(a.exp(), true)?,
                    Function::Sinh => a.sinh(),
                    Function::Cosh => a.cosh(),
                    Function::Tanh => a.tanh(),
                    Function::Asinh => a.asinh(),
                    Function::Acosh => a.acosh(),
                    Function::Atanh => a.atanh(),
                }
            }
        };
        finite(value)
    }
}
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64, String),
    Variable(char),
    Fun(Function),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Left,
    Right,
    End,
}
fn normalize(input: &str) -> Result<std::borrow::Cow<'_, str>> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(MathError::Limit("表达式最多 4096 字节".into()));
    }
    if input.is_ascii() {
        return Ok(std::borrow::Cow::Borrowed(input));
    }
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '！'..='～' => out.push(char::from_u32(c as u32 - 0xfee0).unwrap()),
            '×' => out.push('*'),
            '÷' => out.push('/'),
            '−' => out.push('-'),
            'π' => out.push_str(" pi "),
            '²' => out.push_str("^2"),
            c if c.is_whitespace() => out.push(' '),
            c => out.push(c),
        }
    }
    if out.len() > MAX_INPUT_BYTES {
        return Err(MathError::Limit("规范化表达式最多 4096 字节".into()));
    }
    Ok(std::borrow::Cow::Owned(out))
}
fn display_number(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else {
        value.to_string()
    }
}
fn lex(input: &str) -> Result<Vec<Token>> {
    let input = normalize(input)?;
    let b = input.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if out.len() >= MAX_TOKENS {
            return Err(MathError::Limit("最多 512 个词元".into()));
        }
        let token = match b[i] {
            b'+' => {
                i += 1;
                Token::Plus
            }
            b'-' => {
                i += 1;
                Token::Minus
            }
            b'*' => {
                i += 1;
                Token::Star
            }
            b'/' => {
                i += 1;
                Token::Slash
            }
            b'^' => {
                i += 1;
                Token::Caret
            }
            b'(' => {
                i += 1;
                Token::Left
            }
            b')' => {
                i += 1;
                Token::Right
            }
            b'0'..=b'9' | b'.' => {
                let start = i;
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                    i += 1;
                }
                // e 仅在后接指数数字时作为科学计数法，否则是欧拉常数。
                if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
                    let mut j = i + 1;
                    if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                        j += 1;
                    }
                    if j < b.len() && b[j].is_ascii_digit() {
                        i = j + 1;
                        while i < b.len() && b[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                }
                let literal = &input[start..i];
                let v = literal
                    .parse::<f64>()
                    .map_err(|_| MathError::Syntax("数字格式错误".into()))?;
                if v == 0.0
                    && literal
                        .split(['e', 'E'])
                        .next()
                        .unwrap()
                        .bytes()
                        .any(|c| matches!(c, b'1'..=b'9'))
                {
                    return Err(MathError::Numerical("数字下溢，不能当作零".into()));
                }
                Token::Number(finite(v)?, literal.to_owned())
            }
            b'x' | b'y' => {
                let c = b[i] as char;
                i += 1;
                Token::Variable(c)
            }
            b'a'..=b'z' | b'A'..=b'Z' => {
                let start = i;
                while i < b.len() && b[i].is_ascii_alphabetic() {
                    i += 1;
                }
                match &input[start..i] {
                    "pi" => Token::Number(std::f64::consts::PI, "pi".into()),
                    "e" => Token::Number(std::f64::consts::E, "e".into()),
                    name => Token::Fun(match name {
                        "sin" => Function::Sin,
                        "cos" => Function::Cos,
                        "tan" => Function::Tan,
                        "asin" | "arcsin" => Function::Asin,
                        "acos" | "arccos" => Function::Acos,
                        "atan" | "arctan" => Function::Atan,
                        "ln" => Function::Ln,
                        "log" => Function::Log,
                        "sqrt" => Function::Sqrt,
                        "abs" => Function::Abs,
                        "exp" => Function::Exp,
                        "sinh" => Function::Sinh,
                        "cosh" => Function::Cosh,
                        "tanh" => Function::Tanh,
                        "asinh" => Function::Asinh,
                        "acosh" => Function::Acosh,
                        "atanh" => Function::Atanh,
                        _ => return Err(MathError::Syntax(format!("未知名称 {name}"))),
                    }),
                }
            }
            _ => {
                return Err(MathError::Syntax(format!(
                    "第 {} 字节存在不支持的字符",
                    i + 1
                )));
            }
        };
        out.push(token);
    }
    out.push(Token::End);
    Ok(out)
}
struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}
impl Parser {
    fn expr(&mut self, min: u8, depth: usize) -> Result<(Node, usize)> {
        if depth > MAX_DEPTH {
            return Err(MathError::Limit("语法嵌套最多 64 层".into()));
        }
        let token = self.tokens[self.pos].clone();
        self.pos += 1;
        let (mut node, mut height) = match token {
            Token::Number(v, _) => (Node::Number(v), 1),
            Token::Variable(c) => (Node::Variable(c), 1),
            Token::Minus | Token::Plus => {
                let (n, h) = self.expr(5, depth + 1)?;
                (
                    if token == Token::Minus {
                        Node::Neg(Box::new(n))
                    } else {
                        n
                    },
                    h + 1,
                )
            }
            Token::Left => {
                let n = self.expr(0, depth + 1)?;
                self.close()?;
                n
            }
            Token::Fun(f) => {
                if self.tokens[self.pos] != Token::Left {
                    return Err(MathError::Syntax("函数参数必须加括号".into()));
                }
                self.pos += 1;
                let (n, h) = self.expr(0, depth + 1)?;
                self.close()?;
                (Node::Call(f, Box::new(n)), h + 1)
            }
            _ => return Err(MathError::Syntax("此处需要数字、变量或括号表达式".into())),
        };
        loop {
            let (op, l, r, implicit) = match self.tokens[self.pos] {
                Token::Plus => (Op::Add, 1, 2, false),
                Token::Minus => (Op::Sub, 1, 2, false),
                Token::Star => (Op::Mul, 3, 4, false),
                Token::Slash => (Op::Div, 3, 4, false),
                Token::Caret => (Op::Pow, 7, 6, false),
                Token::Number(..) | Token::Variable(_) | Token::Fun(_) | Token::Left => {
                    (Op::Mul, 3, 4, true)
                }
                _ => break,
            };
            if l < min {
                break;
            }
            if !implicit {
                self.pos += 1;
            }
            let (rhs, h) = self.expr(r, depth + 1)?;
            height = height.max(h) + 1;
            if height > MAX_DEPTH {
                return Err(MathError::Limit("语法树最多 64 层".into()));
            }
            node = Node::Binary(op, Box::new(node), Box::new(rhs));
        }
        if height > MAX_DEPTH {
            return Err(MathError::Limit("语法树最多 64 层".into()));
        }
        Ok((node, height))
    }
    fn close(&mut self) -> Result<()> {
        if self.tokens[self.pos] != Token::Right {
            return Err(MathError::Syntax("缺少右括号".into()));
        }
        self.pos += 1;
        Ok(())
    }
}
pub fn parse(input: &str) -> Result<Expr> {
    let tokens = lex(input)?;
    let size = tokens.len();
    let mut p = Parser { tokens, pos: 0 };
    let (node, _) = p.expr(0, 0)?;
    if p.tokens[p.pos] != Token::End {
        return Err(MathError::Syntax("表达式末尾有多余内容".into()));
    }
    Ok(Expr { node, size })
}
pub fn evaluate(input: &str) -> Result<f64> {
    parse(input)?.eval(None, None)
}
pub fn eval_at(input: &str, x: f64) -> Result<f64> {
    parse(input)?.eval(Some(x), None)
}
fn statement_budget(input: &str, extra: usize) -> Result<()> {
    let mut count = extra;
    for (index, part) in input.split(['=', ';']).enumerate() {
        count += lex(part)?.len() - 1 + usize::from(index != 0);
        if count > MAX_TOKENS {
            return Err(MathError::Limit("整条输入最多 512 个词元".into()));
        }
    }
    Ok(())
}

fn symbolic_command(input: &str) -> Option<Result<String>> {
    let input = input.trim();
    let open = input.find('(')?;
    let name = input[..open].trim_end();
    if !matches!(name, "simplify" | "diff" | "integrate") {
        return None;
    }
    Some((|| {
        let body = input[open + 1..]
            .strip_suffix(')')
            .ok_or_else(|| MathError::Syntax("符号命令必须以右括号结束".into()))?;
        let mut depth = 0usize;
        let mut comma = None;
        for (index, c) in body.char_indices() {
            match c {
                '(' => {
                    depth += 1;
                    if depth >= MAX_DEPTH {
                        return Err(MathError::Limit("语法嵌套最多 64 层".into()));
                    }
                }
                ')' => {
                    depth = depth
                        .checked_sub(1)
                        .ok_or_else(|| MathError::Syntax("符号命令后有多余内容".into()))?;
                }
                ',' if depth == 0 && comma.replace(index).is_some() => {
                    return Err(MathError::Syntax("符号微积分需要表达式和一个变量".into()));
                }
                _ => {}
            }
        }
        if depth != 0 {
            return Err(MathError::Syntax("缺少右括号".into()));
        }
        if name == "simplify" {
            statement_budget(body, 3)?;
            return simplify(body);
        }
        let comma = comma.ok_or_else(|| {
            MathError::Syntax("格式为 diff(多项式,x) 或 integrate(多项式,x)，变量也可为 y".into())
        })?;
        let expression = &body[..comma];
        let variable = match body[comma + 1..].trim() {
            "x" => 'x',
            "y" => 'y',
            _ => return Err(MathError::Unsupported("符号微积分变量仅支持 x 或 y".into())),
        };
        statement_budget(expression, 5)?;
        if name == "diff" {
            differentiate_polynomial(expression, variable)
        } else {
            antiderivative_polynomial(expression, variable)
        }
    })())
}

/// 常量求值；含变量时多项式化简；一个等号解 x 方程；分号隔开的两式解线性系统。
/// 顶层命令 simplify(表达式或等式组) 返回化简结果，不求解。
/// diff(多项式,x) / integrate(多项式,x) 返回符号偏导 / 一个原函数（积分常数取零），也支持 y。
/// 符号命令不嵌套、不作为算术子表达式；仅支持 x/y 多项式，不是通用符号微积分。
/// 二元二次请调用带显式边界的 calculate_with_bounds 或 solve_quadratic_system。
pub fn calculate(input: &str) -> Result<String> {
    let normalized = normalize(input)?;
    let input = normalized.as_ref();
    if let Some(result) = symbolic_command(input) {
        return result;
    }
    statement_budget(input, 0)?;
    if input.contains(';') {
        let parts: Vec<_> = input.split(';').collect();
        if parts.len() != 2 {
            return Err(MathError::Syntax("方程组需要两个方程，用分号分隔".into()));
        }
        return Ok(match solve_linear_system(parts[0], parts[1])? {
            SystemSolutions::None => "无解".into(),
            SystemSolutions::Infinite => "无穷多解".into(),
            SystemSolutions::Unique { x, y } => {
                format!("x = {}, y = {}", display_number(x), display_number(y))
            }
        });
    }
    if input.contains('=') {
        return Ok(match solve_equation(input)? {
            Solutions::None => "无实数解".into(),
            Solutions::Infinite => "任意实数 x 均为解".into(),
            Solutions::Finite(v) => v
                .iter()
                .map(|x| format!("x = {}", display_number(*x)))
                .collect::<Vec<_>>()
                .join("；"),
        });
    }
    if !lex(input)?.iter().any(|t| matches!(t, Token::Variable(_))) {
        return Ok(exact::constant(input)?.text);
    }
    match evaluate(input) {
        Ok(v) => Ok(display_number(v)),
        Err(MathError::MissingVariable(_)) => simplify(input),
        Err(e) => Err(e),
    }
}

/// calculate 的有界便捷入口；分号分隔的两式支持总次数不超过 2 的系统。
/// 二次系统仅返回矩形内数值候选，文字明确提示非完备；未找到不等于无解。
/// 非方程组输入沿用 calculate，不使用边界与数值参数；结构化结果请用 solve_quadratic_system。
pub fn calculate_with_bounds(
    input: &str,
    bounds: Bounds2D,
    options: NumericOptions,
) -> Result<String> {
    let normalized = normalize(input)?;
    let input = normalized.as_ref();
    if let Some(result) = symbolic_command(input) {
        return result;
    }
    if !input.contains(';') {
        return calculate(input);
    }
    statement_budget(input, 0)?;
    let parts: Vec<_> = input.split(';').collect();
    if parts.len() != 2 {
        return Err(MathError::Syntax("方程组需要两个方程，用分号分隔".into()));
    }
    let result = solve_quadratic_system(parts[0], parts[1], bounds, options)?;
    let points = result
        .points
        .iter()
        .map(|p| format!("x = {}, y = {}", display_number(p.x), display_number(p.y)))
        .collect::<Vec<_>>()
        .join("；");
    let region = format!(
        "x∈[{}, {}]，y∈[{}, {}]",
        bounds.x_min, bounds.x_max, bounds.y_min, bounds.y_max
    );
    if result.complete {
        return Ok(if points.is_empty() {
            format!("指定范围（{region}）内无解")
        } else {
            format!("指定范围（{region}）内：{points}")
        });
    }
    let description = if points.is_empty() {
        "未找到候选解"
    } else {
        &points
    };
    Ok(format!(
        "指定范围（{region}）内：{description}；数值搜索不保证完备，未找到不等于无解；未收敛种子：{}",
        result.unconverged_seeds
    ))
}

#[cfg(test)]
mod tests;
