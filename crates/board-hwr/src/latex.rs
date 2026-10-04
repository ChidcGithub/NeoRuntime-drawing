//! TexTeller 输出的保守 LaTeX 子集转换器；不执行 TeX，也不计算表达式。
//! ln 为自然对数，log 为常用对数，三角函数使用弧度，与 board-math 一致。

const MAX_BYTES: usize = 4096;
const MAX_TOKENS: usize = 512;
// 输出为显式带括号的表达式，保守留出 board-math 64 层解析预算。
const MAX_DEPTH: usize = 32;

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(String),
    Name(String),
    Command(String),
    Plus,
    Minus,
    Star,
    Slash,
    Power,
    Equal,
    Open(char),
    Close(char),
    EscapedBrace,
    Amp,
    Row,
    Dot,
}

fn unsupported(message: &str) -> String {
    format!("unsupported: {message}")
}

fn function(name: &str) -> bool {
    matches!(name, "sin" | "cos" | "tan" | "exp" | "ln" | "log")
}

fn unwrap_math(input: &str) -> Result<&str> {
    let input = input.trim();
    for (open, close) in [("$$", "$$"), ("$", "$"), (r"\[", r"\]"), (r"\(", r"\)")] {
        if let Some(body) = input.strip_prefix(open) {
            return body
                .strip_suffix(close)
                .ok_or_else(|| "syntax: 数学定界符未闭合".into());
        }
    }
    Ok(input)
}

fn lex(input: &str) -> Result<Vec<Token>> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    let mut count = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        count += 1;
        if count > MAX_TOKENS {
            return Err("limit: LaTeX 最多 512 个词元（含排版命令）".into());
        }
        let token = match bytes[i] {
            b'\\' => {
                i += 1;
                let start = i;
                if i == bytes.len() {
                    return Err("syntax: 不完整的 LaTeX 命令".into());
                }
                if bytes[i].is_ascii_alphabetic() {
                    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                } else {
                    // 非 ASCII 命令不会参与切片，避免落在 UTF-8 字符中间。
                    if !bytes[i].is_ascii() {
                        return Err(unsupported("非 ASCII 命令"));
                    }
                    i += 1;
                }
                let name = &input[start..i];
                match name {
                    "," | "!" | ":" | ";" | " " | "quad" | "qquad" => continue,
                    "\\" => Token::Row,
                    "{" => Token::EscapedBrace,
                    "times" | "cdot" => Token::Star,
                    "div" => Token::Slash,
                    "pi" => Token::Name("pi".into()),
                    "frac" | "dfrac" | "tfrac" | "sqrt" | "left" | "right" | "begin" | "end"
                    | "mathrm" | "operatorname" => Token::Command(name.into()),
                    name if function(name) => Token::Name(name.into()),
                    _ => return Err(unsupported(&format!("命令 \\{name}"))),
                }
            }
            b'0'..=b'9' | b'.' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                    i += 1;
                }
                if matches!(bytes.get(i), Some(b'e' | b'E'))
                    && matches!(bytes.get(i + 1), Some(b'0'..=b'9' | b'+' | b'-'))
                {
                    return Err(unsupported("科学计数法请写成显式乘以 10 的幂"));
                }
                let literal = &input[start..i];
                if literal == "." {
                    Token::Dot
                } else {
                    let value = literal
                        .parse::<f64>()
                        .map_err(|_| "syntax: 无效数字".to_string())?;
                    if !value.is_finite()
                        || (value == 0.0 && literal.bytes().any(|b| matches!(b, b'1'..=b'9')))
                    {
                        return Err("limit: 数字溢出或下溢".into());
                    }
                    if matches!(tokens.last(), Some(Token::Number(_))) {
                        return Err(unsupported("空白分隔的相邻数字需要显式运算符"));
                    }
                    Token::Number(literal.into())
                }
            }
            b'x' | b'y' => {
                let name = (bytes[i] as char).to_string();
                i += 1;
                Token::Name(name)
            }
            b'a'..=b'z' | b'A'..=b'Z' => {
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
                Token::Name(input[start..i].into())
            }
            c => {
                i += 1;
                match c {
                    b'+' => Token::Plus,
                    b'-' => Token::Minus,
                    b'*' => Token::Star,
                    b'/' => Token::Slash,
                    b'^' => Token::Power,
                    b'=' => Token::Equal,
                    b'(' | b'[' | b'{' => Token::Open(c as char),
                    b')' | b']' | b'}' => Token::Close(c as char),
                    b'&' => Token::Amp,
                    b';' => Token::Row,
                    _ => return Err(unsupported(&format!("第 {i} 字节的字符"))),
                }
            }
        };
        tokens.push(token);
    }
    Ok(tokens)
}

// 使用词元而非原始字符，避免把 \\left\\{ 等排版定界符当作分组括号。
fn validate_delimiters(tokens: &[Token]) -> Result<()> {
    let mut opens = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let position = index + 1;
        match token {
            Token::Open(open) => {
                if opens.len() == MAX_DEPTH {
                    return Err("limit: LaTeX 嵌套最多 32 层".into());
                }
                opens.push((*open, position));
            }
            Token::Close(close) => {
                let expected = match close {
                    ')' => '(',
                    ']' => '[',
                    _ => '{',
                };
                if opens.pop().is_none_or(|(open, _)| open != expected) {
                    let label = if *close == '}' {
                        "右花括号"
                    } else {
                        "右括号"
                    };
                    return Err(format!(
                        "syntax: LaTeX {label}多余/不匹配，位置：第 {position} 个词元"
                    ));
                }
            }
            _ => {}
        }
    }
    if let Some((open, position)) = opens.last() {
        let label = if *open == '{' {
            "左花括号"
        } else {
            "左括号"
        };
        return Err(format!(
            "syntax: LaTeX {label}未闭合，位置：第 {position} 个词元"
        ));
    }
    Ok(())
}

struct Expression {
    text: String,
    tokens: usize,
    height: usize,
}

impl Expression {
    fn leaf(text: String) -> Self {
        Self {
            text,
            tokens: 1,
            height: 1,
        }
    }

    fn build(parts: &[&str], tokens: usize, height: usize) -> Result<Self> {
        if parts.iter().map(|part| part.len()).sum::<usize>() > MAX_BYTES {
            return Err("limit: 转换结果最多 4096 字节".into());
        }
        if tokens > MAX_TOKENS || height > MAX_DEPTH {
            return Err("limit: 转换结果词元或语法树深度超限".into());
        }
        Ok(Self {
            text: parts.concat(),
            tokens,
            height,
        })
    }

    fn binary(self, op: &str, rhs: Self) -> Result<Self> {
        Self::build(
            &["(", &self.text, op, &rhs.text, ")"],
            self.tokens + rhs.tokens + 3,
            self.height.max(rhs.height) + 1,
        )
    }

    fn call(name: &str, arg: Self) -> Result<Self> {
        Self::build(&[name, "(", &arg.text, ")"], arg.tokens + 3, arg.height + 1)
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    answer_prompt: bool,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn is_command(&self, name: &str) -> bool {
        matches!(self.peek(), Some(Token::Command(value)) if value == name)
    }

    fn eat(&mut self, token: &Token) -> bool {
        if self.peek() == Some(token) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, token: &Token) -> Result<()> {
        if self.eat(token) {
            Ok(())
        } else {
            Err(format!("syntax: 需要 {token:?}，位于词元 {}", self.pos + 1))
        }
    }

    fn depth(depth: usize) -> Result<()> {
        if depth > MAX_DEPTH {
            Err("limit: LaTeX 嵌套最多 32 层".into())
        } else {
            Ok(())
        }
    }

    fn starts_atom(&self) -> bool {
        match self.peek() {
            Some(Token::Number(_) | Token::Name(_) | Token::Open(_)) => true,
            Some(Token::Command(name)) => matches!(
                name.as_str(),
                "frac" | "dfrac" | "tfrac" | "sqrt" | "left" | "mathrm" | "operatorname"
            ),
            _ => false,
        }
    }

    // Pratt 优先级与 board-math 相同；所有二元结果加括号，禁止词元拼接改变含义。
    fn expression(&mut self, min: u8, depth: usize) -> Result<Expression> {
        Self::depth(depth)?;
        let mut lhs = if self.eat(&Token::Plus) {
            self.expression(5, depth + 1)?
        } else if self.eat(&Token::Minus) {
            let arg = self.expression(5, depth + 1)?;
            Expression::build(&["(-", &arg.text, ")"], arg.tokens + 3, arg.height + 1)?
        } else {
            self.atom(depth + 1)?
        };
        let mut superscript = false;
        loop {
            if self.peek() == Some(&Token::Power) {
                if min > 7 {
                    break;
                }
                if superscript {
                    return Err(unsupported("重复上标，请用分组明确幂的含义"));
                }
                self.pos += 1;
                let rhs = self.script(depth + 1)?;
                lhs = lhs.binary("^", rhs)?;
                superscript = true;
                continue;
            }
            let (op, left, right, implicit) = match self.peek() {
                Some(Token::Plus) => ("+", 1, 2, false),
                Some(Token::Minus) => ("-", 1, 2, false),
                Some(Token::Star) => ("*", 3, 4, false),
                Some(Token::Slash) => ("/", 3, 4, false),
                _ if self.starts_atom() => ("*", 3, 4, true),
                _ => break,
            };
            if left < min {
                break;
            }
            if !implicit {
                self.pos += 1;
            }
            let rhs = self.expression(right, depth + 1)?;
            lhs = lhs.binary(op, rhs)?;
            superscript = false;
        }
        Ok(lhs)
    }

    fn group(&mut self, open: char, scaled: bool, depth: usize) -> Result<Expression> {
        Self::depth(depth)?;
        self.expect(&Token::Open(open))?;
        let expression = self.expression(0, depth + 1)?;
        if scaled {
            if !self.is_command("right") {
                return Err("syntax: 缺少 \\right".into());
            }
            self.pos += 1;
        }
        let close = match open {
            '(' => ')',
            '[' => ']',
            _ => '}',
        };
        self.expect(&Token::Close(close))?;
        Ok(expression)
    }

    fn script(&mut self, depth: usize) -> Result<Expression> {
        Self::depth(depth)?;
        match self.peek() {
            Some(Token::Open(open)) => {
                let open = *open;
                self.group(open, false, depth + 1)
            }
            Some(Token::Number(value)) if value.len() == 1 => self.atom(depth + 1),
            Some(Token::Name(name)) if matches!(name.as_str(), "x" | "y" | "e" | "pi") => {
                self.atom(depth + 1)
            }
            _ => Err(unsupported(
                "上标必须为单个数字/变量，或显式括号/花括号分组",
            )),
        }
    }

    fn named_group(&mut self) -> Result<String> {
        self.expect(&Token::Open('{'))?;
        let name = match self.peek().cloned() {
            Some(Token::Name(name)) => name,
            _ => return Err(unsupported("此处仅允许白名单名称")),
        };
        self.pos += 1;
        self.expect(&Token::Close('}'))?;
        Ok(name)
    }

    fn function_arg(&mut self, name: &str, depth: usize) -> Result<Expression> {
        Self::depth(depth)?;
        if self.peek() == Some(&Token::Power) {
            return Err(unsupported(
                "函数名上标含义不明确（例如 sin^{-1}），请显式改写",
            ));
        }
        let grouped = matches!(self.peek(), Some(Token::Open(_))) || self.is_command("left");
        if !self.starts_atom() {
            return Err("syntax: 函数缺少参数".into());
        }
        let arg = if grouped {
            self.atom(depth + 1)?
        } else {
            let arg = self.expression(5, depth + 1)?;
            if self.starts_atom() {
                return Err(unsupported("无括号函数的复合参数有歧义，请加括号"));
            }
            arg
        };
        Expression::call(name, arg)
    }

    fn atom(&mut self, depth: usize) -> Result<Expression> {
        Self::depth(depth)?;
        match self.peek().cloned() {
            Some(Token::Number(value)) => {
                self.pos += 1;
                Ok(Expression::leaf(value))
            }
            Some(Token::Name(name)) => {
                self.pos += 1;
                if matches!(name.as_str(), "x" | "y" | "e" | "pi") {
                    Ok(Expression::leaf(name))
                } else if function(&name) {
                    self.function_arg(&name, depth + 1)
                } else {
                    Err(unsupported(&format!("名称 {name}（变量仅支持 x/y）")))
                }
            }
            Some(Token::Open(open)) => self.group(open, false, depth + 1),
            Some(Token::Command(name)) => {
                self.pos += 1;
                match name.as_str() {
                    "frac" | "dfrac" | "tfrac" => {
                        let numerator = self.group('{', false, depth + 1)?;
                        let denominator = self.group('{', false, depth + 1)?;
                        numerator.binary("/", denominator)
                    }
                    "sqrt" => {
                        if self.peek() == Some(&Token::Open('[')) {
                            return Err(unsupported("带根指数的根式"));
                        }
                        let arg = self.group('{', false, depth + 1)?;
                        Expression::call("sqrt", arg)
                    }
                    "left" => match self.peek() {
                        Some(Token::Open(open @ ('(' | '['))) => {
                            let open = *open;
                            self.group(open, true, depth + 1)
                        }
                        _ => Err(unsupported("此处的 \\left 定界符")),
                    },
                    "mathrm" | "operatorname" => {
                        let value = self.named_group()?;
                        if function(&value) {
                            self.function_arg(&value, depth + 1)
                        } else if name == "mathrm" && matches!(value.as_str(), "x" | "y" | "e") {
                            Ok(Expression::leaf(value))
                        } else {
                            Err(unsupported("排版名称不在变量/函数白名单中"))
                        }
                    }
                    _ => Err(unsupported("环境或定界符不能作为算术子表达式")),
                }
            }
            _ => Err(format!("syntax: 第 {} 个词元处缺少表达式", self.pos + 1)),
        }
    }

    fn statement(
        &mut self,
        alignment: Option<usize>,
        allow_function: bool,
    ) -> Result<(Expression, bool)> {
        let is_function = self.peek() == Some(&Token::Name("f".into()));
        let lhs = if is_function {
            if !allow_function {
                return Err(unsupported("方程组中不能使用 f(x) 定义"));
            }
            self.pos += 1;
            let scaled = self.is_command("left");
            if scaled {
                self.pos += 1;
            }
            self.expect(&Token::Open('('))?;
            self.expect(&Token::Name("x".into()))?;
            if scaled {
                if !self.is_command("right") {
                    return Err("syntax: f(x) 缺少 \\right".into());
                }
                self.pos += 1;
            }
            self.expect(&Token::Close(')'))?;
            Expression {
                text: "f(x)".into(),
                tokens: 4,
                height: 1,
            }
        } else {
            self.expression(0, 0)?
        };
        let mut amps = usize::from(self.eat(&Token::Amp));
        if !self.eat(&Token::Equal) {
            if is_function || amps != 0 || alignment.is_some() {
                return Err("syntax: 此处需要一个完整等式".into());
            }
            return Ok((lhs, false));
        }
        amps += usize::from(self.eat(&Token::Amp));
        match alignment {
            None if amps != 0 => return Err(unsupported("环境之外不能使用 &")),
            Some(max) if amps > max => return Err(unsupported("& 仅允许在等号两侧对齐")),
            _ => {}
        }
        // Only a complete top-level numeric expression may ask to fill in an answer.
        // Successful expression parsing has already rejected unknown names/commands.
        if self.peek().is_none()
            && !is_function
            && alignment.is_none()
            && allow_function
            && !self.tokens.iter().any(|token| {
                matches!(token, Token::Row)
                    || matches!(token, Token::Name(name) if matches!(name.as_str(), "x" | "y"))
            })
            && self
                .tokens
                .iter()
                .filter(|token| **token == Token::Equal)
                .count()
                == 1
        {
            self.answer_prompt = true;
            return Ok((lhs, false));
        }
        let rhs = self.expression(0, 0)?;
        let output = Expression::build(
            &[&lhs.text, "=", &rhs.text],
            lhs.tokens + rhs.tokens + 1,
            lhs.height.max(rhs.height),
        )?;
        Ok((output, true))
    }

    fn document(&mut self) -> Result<String> {
        let wrapped = self.is_command("left");
        // 只将包裹整个方程组的左大括号视为排版，绝不删除任意定界符。
        if wrapped && self.tokens.get(self.pos + 1) == Some(&Token::EscapedBrace) {
            self.pos += 2;
            if !self.is_command("begin") {
                return Err(unsupported("左大括号只能包裹方程组环境"));
            }
        }
        let wrapped = wrapped && self.pos == 2;
        let mut columns = None;
        let environment = if self.is_command("begin") {
            self.pos += 1;
            let name = self.named_group()?;
            if !matches!(name.as_str(), "aligned" | "array" | "cases") {
                return Err(unsupported("仅支持 aligned/array/cases 方程组，不支持矩阵"));
            }
            if name == "array" {
                let format = self.named_group()?;
                if format.is_empty()
                    || format.len() > 3
                    || !format.bytes().all(|c| matches!(c, b'l' | b'c' | b'r'))
                {
                    return Err(unsupported("array 列格式仅允许一至三个 l/c/r"));
                }
                columns = Some(format.len());
            }
            Some(name)
        } else {
            None
        };
        let mut rows = Vec::new();
        let mut equations = Vec::new();
        loop {
            if rows.len() == 2 {
                return Err(unsupported("现有引擎最多支持两个方程"));
            }
            let start = self.pos;
            let alignment = environment
                .as_ref()
                .map(|_| columns.map_or(1, |count| count - 1));
            let (row, equation) =
                self.statement(alignment, environment.is_none() && rows.is_empty())?;
            if let Some(columns) = columns {
                let amps = self.tokens[start..self.pos]
                    .iter()
                    .filter(|t| **t == Token::Amp)
                    .count();
                if amps + 1 != columns {
                    return Err("syntax: array 的列数与 & 数量不匹配".into());
                }
            }
            rows.push(row);
            equations.push(equation);
            if !self.eat(&Token::Row) {
                break;
            }
            if environment.is_some() && self.is_command("end") {
                break;
            }
        }
        if rows.len() > 1
            && (equations.iter().any(|equation| !equation)
                || rows.iter().any(|row| row.text.starts_with("f(x)=")))
        {
            return Err(unsupported("多行必须是两个完整的普通方程"));
        }
        if let Some(name) = environment {
            if !self.is_command("end") {
                return Err("syntax: 方程组环境缺少 \\end".into());
            }
            self.pos += 1;
            if self.named_group()? != name {
                return Err("syntax: 方程组环境起止名称不一致".into());
            }
        }
        if wrapped {
            if !self.is_command("right") {
                return Err("syntax: 方程组缺少 \\right.".into());
            }
            self.pos += 1;
            self.expect(&Token::Dot)?;
        }
        if self.pos != self.tokens.len() {
            return Err(format!(
                "syntax: 第 {} 个词元起有未消费的内容",
                self.pos + 1
            ));
        }
        let token_count = rows.iter().map(|row| row.tokens).sum::<usize>() + rows.len() - 1;
        let parts: Vec<_> = rows.iter().map(|row| row.text.as_str()).collect();
        let text = parts.join(";");
        if text.len() > MAX_BYTES || token_count > MAX_TOKENS {
            return Err("limit: 整条转换结果超过字节或词元预算".into());
        }
        Ok(text)
    }
}

/// 将受限 LaTeX 完整转换为现有数学文本；返回错误时不得拿部分结果计算。
/// 输入/输出最多 4096 字节、512 词元，嵌套/输出树深最多 32 层。
/// 保留 y= / f(x)=；后者供 GUI 函数入口消费，而非直接交给 board_math::parse。
/// 拒绝未知命令、函数名上标、下标、微积分、矩阵、分段条件和歧义参数。
/// 只保证语法转换，不承诺定义域有效、可多项式化、可求解或识别置信度。
pub fn latex_to_expression(input: &str) -> std::result::Result<String, String> {
    latex_to_calculation(input).map(|parsed| parsed.expression)
}

#[derive(Debug, PartialEq)]
pub struct LatexCalculation {
    pub expression: String,
    pub answer_prompt: bool,
}

/// 完整转换，并标记唯一顶层纯数值算式末尾的补答案等号；不放行残缺方程。
pub fn latex_to_calculation(input: &str) -> std::result::Result<LatexCalculation, String> {
    if input.len() > MAX_BYTES {
        return Err("limit: LaTeX 输入最多 4096 字节".into());
    }
    let tokens = lex(unwrap_math(input)?)?;
    validate_delimiters(&tokens)?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        answer_prompt: false,
    };
    let expression = parser.document()?;
    Ok(LatexCalculation {
        expression,
        answer_prompt: parser.answer_prompt,
    })
}

#[cfg(test)]
#[path = "latex_tests.rs"]
mod tests;
