use super::*;

fn converts(input: &str, expected: &str) {
    assert_eq!(latex_to_expression(input).unwrap(), expected, "{input}");
}

#[test]
fn numeric_answer_prompts_require_one_complete_top_level_expression() {
    for (input, expected) in [
        (r"\[1+1=\]", "(1+1)"),
        (r"\(\frac{1}{2}+\frac{1}{2}=\)", "((1/2)+(1/2))"),
        (r"\sqrt{4}=", "sqrt(4)"),
        (r"\sin(\pi)+\ln(e)=", "(sin(pi)+ln(e))"),
        (r"\operatorname{cos}(0)+\mathrm{e}=", "(cos(0)+e)"),
        (r"-2^{2}=\,", "(-(2^2))"),
    ] {
        let parsed = latex_to_calculation(input).unwrap();
        assert!(parsed.answer_prompt, "{input}");
        assert_eq!(parsed.expression, expected);
        converts(input, expected);
    }
    for (input, expected) in [("1+1", "(1+1)"), ("1+1=2", "(1+1)=2")] {
        let parsed = latex_to_calculation(input).unwrap();
        assert!(!parsed.answer_prompt);
        assert_eq!(parsed.expression, expected);
    }
    for input in [
        "x+1=",
        "y=",
        "f(x)=",
        "x-x+1=",
        r"\mathrm{x}+1=",
        r"\frac{1}{y}=",
        r"\sin(x)=",
        "z+1=",
        "(1+1=)",
        "{1+1=}",
        r"\frac{1=}{2}",
        "1+(2=)",
        "1+1==",
        "1+1=2=",
        "1+=",
        "=",
        "1+1=;",
        "1=1;2=",
        r"1=1\\2=",
        r"1+1=\\",
        "1+1=&",
        "1+1=)",
        r"1+1=\unknown",
        r"1+\unknown{1}=",
        "1+1=@",
        "1+1=garbage",
        r"\[1+1=\]x",
        r"\begin{aligned}1+1&=\end{aligned}",
        r"\begin{cases}x=1\\y=\end{cases}",
        r"\begin{array}{l}1+1=\end{array}",
    ] {
        assert!(latex_to_calculation(input).is_err(), "must reject {input}");
    }
}

#[test]
fn brace_diagnostics_use_tokens_without_repairing_model_output() {
    for (input, position) in [
        (r"\[\frac{3+2 \sqrt{3}}}{2}=\]", 11),
        (r"\frac{1}}{2}", 5),
        ("1}", 2),
        ("(1}", 3),
        ("{(1} )", 4),
        (r"\[\,\frac{3+2\quad\sqrt{3}}}{2}=\]", 11),
    ] {
        assert_eq!(
            latex_to_calculation(input).unwrap_err(),
            format!("syntax: LaTeX 右花括号多余/不匹配，位置：第 {position} 个词元"),
            "{input}"
        );
    }
    assert_eq!(
        latex_to_calculation(r"\frac{1}{2").unwrap_err(),
        "syntax: LaTeX 左花括号未闭合，位置：第 5 个词元"
    );
    // 转义花括号不参与分组；不支持的转义命令仍拒绝，而不是删去后计算。
    for input in [r"\{1", r"1\}"] {
        let error = latex_to_calculation(input).unwrap_err();
        assert!(!error.contains("花括号"), "{error}");
    }
    converts(
        r"\left\{\begin{aligned}x&=1\\y&=2\end{aligned}\right.",
        "x=1;y=2",
    );
    let parsed = latex_to_calculation(r"\[\frac{3+2 \sqrt{3}}{2}=\]").unwrap();
    assert_eq!(parsed.expression, "((3+(2*sqrt(3)))/2)");
    assert!(parsed.answer_prompt);
}

#[test]
fn fractions_roots_and_powers() {
    converts(r"\frac{1}{2}", "(1/2)");
    converts(r"\dfrac{x+1}{\tfrac{2}{3}}", "((x+1)/(2/3))");
    converts(r"\sqrt{\frac{x^{2}+1}{4}}", "sqrt((((x^2)+1)/4))");
    converts(r"(x+1)^{2}+x^(y+1)", "(((x+1)^2)+(x^(y+1)))");
    converts(r"-x^{2}", "(-(x^2))");
    converts(r"(-x)^2", "((-x)^2)");
    converts(r"x^{-2}", "(x^(-2))");
    converts(r"x^{y^{2}}", "(x^(y^2))");
    converts(r"2\left(x+1\right)\sqrt{9}", "((2*(x+1))*sqrt(9))");
}

#[test]
fn full_expressions_and_functions() {
    converts(
        r"y=\frac{1}{2}x^{2}-3x+\sqrt{4}",
        "y=((((1/2)*(x^2))-(3*x))+sqrt(4))",
    );
    converts(
        r"f\left(x\right)=\sin\left(\frac{\pi x}{2}\right)+\cos(x)",
        "f(x)=(sin(((pi*x)/2))+cos(x))",
    );
    converts(
        r"\sin x^{2}+\cos{y}+\tan(0)",
        "((sin((x^2))+cos(y))+tan(0))",
    );
    converts(r"\exp(1)+\ln(e)+\log(100)", "((exp(1)+ln(e))+log(100))");
    converts(r"\sin(x)^2", "(sin(x)^2)");
    converts(r"\mathrm{x}\cdot\mathrm{y}\div 2", "((x*y)/2)");
    converts(
        r"\operatorname{sin}(x)+\mathrm{log}(10)",
        "(sin(x)+log(10))",
    );
    converts(r"2\times 3+4\div 2", "((2*3)+(4/2))");
    converts(r"\pi x+2 e", "((pi*x)+(2*e))");
    converts(".5x+1.25", "((.5*x)+1.25)");
}

#[test]
fn wrappers_and_spacing_do_not_join_tokens() {
    for input in [r"$x+1$", r"$$x+1$$", r"\(x+1\)", r"\[x+1\]"] {
        converts(input, "(x+1)");
    }
    converts(r"x\,y\!+\quad 2", "((x*y)+2)");
    converts(r"\left[x+1\right]", "(x+1)");
    for input in [
        r"1\,2",
        "1 2",
        r"$x+1",
        r"$x$garbage",
        r"x$",
        r"\left(x+1)",
        r"(x\right)",
        r"\left(x\right]",
    ] {
        assert!(latex_to_expression(input).is_err(), "{input}");
    }
}

#[test]
fn equation_systems_and_alignment() {
    let expected = "(x+y)=3;(x-y)=1";
    for input in [
        r"\begin{aligned}x+y&=3\\x-y&=1\end{aligned}",
        r"\begin{aligned}x+y=&3\\x-y=&1\\\end{aligned}",
        r"\begin{array}{rcl}x+y&=&3\\x-y&=&1\end{array}",
        r"\begin{array}{ll}x+y&=3\\x-y&=1\end{array}",
        r"\left\{\begin{array}{l}x+y=3\\x-y=1\end{array}\right.",
        r"\begin{cases}x+y=3\\x-y=1\end{cases}",
        r"x+y=3\\x-y=1",
        "x+y=3;x-y=1",
    ] {
        converts(input, expected);
    }
    converts("x=1", "x=1");
    converts("y=x", "y=x");
    converts("f(x)=x", "f(x)=x");
}

#[test]
fn never_drop_unknown_or_ambiguous_content() {
    for input in [
        r"\input{secret}",
        r"\write18{calc}",
        r"\def\x{1}x",
        r"\href{url}{x}",
        r"\text{x}",
        r"\mathrm{hello}",
        r"\operatorname{system}(x)",
        r"\mathrm{xy}",
        r"\sin^{-1}(x)",
        r"\sin^{2}x",
        r"\log_{2}(8)",
        r"x_1",
        r"\sqrt[3]{8}",
        r"\int_0^1 x\,dx",
        r"\frac{d}{dx}x^2",
        r"\frac{\mathrm{d}}{\mathrm{d}x}x^2",
        r"\sin 2x",
        r"x^23",
        r"x^2^3",
        r"x^-2",
        "z+1",
        "1e3",
        "NaN",
        "inf",
        r"\begin{matrix}1&2\\3&4\end{matrix}",
        r"\begin{array}{|l|}x=1\\y=2\end{array}",
        r"\begin{array}{p{1cm}}x=1\\y=2\end{array}",
        r"\begin{array}{llll}x=1\\y=2\end{array}",
        r"\begin{array}{rcl}x&=1\\y&=2\end{array}",
        r"\begin{aligned}x&+y=1\\y=2\end{aligned}",
        r"\begin{aligned}x&=&1\\y=2\end{aligned}",
        r"\begin{cases}x&x>0\\0&x=0\end{cases}",
        r"y=\begin{cases}x=1\\x=2\end{cases}",
        r"\begin{aligned}x=1\\y=2\end{cases}",
        r"\begin{aligned}x=1\\y=2\\x=3\end{aligned}",
        r"\begin{aligned}x=1\\y=2\end{aligned}x",
        "x&=1",
        "x=1&",
        "x&y",
    ] {
        assert!(latex_to_expression(input).is_err(), "must reject {input}");
    }
    assert!(
        latex_to_expression(r"\sin^{-1}(x)")
            .unwrap_err()
            .starts_with("unsupported:")
    );
}

#[test]
fn malformed_expressions_and_equalities() {
    for input in [
        "",
        " ",
        "{}",
        "()",
        "x+",
        "*x",
        "x/",
        "x^^2",
        "x^{}",
        "x)",
        "(x",
        "(x]",
        "=1",
        "x=",
        "x==1",
        "x=1=2",
        "(x=1)",
        "x=1;",
        ";x=1",
        "x=1;;y=2",
        "x;y",
        "x=1;y",
        "x=1;y=2;x=3",
        "f(y)=1",
        "f(x)",
        "f(x)=1;y=2",
        r"\frac{x}",
        r"\frac12",
        r"\sqrt{}",
        r"\sin",
        r"\begin{aligned}x=1",
        "1..2",
        r"\left\{x=1\right.",
        r"\begin{cases}\end{cases}",
        "\\",
        "你好",
        "x\0",
    ] {
        assert!(latex_to_expression(input).is_err(), "must reject {input:?}");
    }
}

#[test]
fn resource_limits_cover_input_output_tokens_and_depth() {
    assert!(
        latex_to_expression(&" ".repeat(MAX_BYTES + 1))
            .unwrap_err()
            .starts_with("limit:")
    );
    assert!(
        latex_to_expression(&format!("{}x", r"\,".repeat(MAX_TOKENS)))
            .unwrap_err()
            .starts_with("limit:")
    );
    assert!(
        latex_to_expression(&format!("{}x{}", "{".repeat(40), "}".repeat(40)))
            .unwrap_err()
            .starts_with("limit:")
    );
    assert!(
        latex_to_expression(&format!("{}x", "-".repeat(100)))
            .unwrap_err()
            .starts_with("limit:")
    );
    assert!(
        latex_to_expression(&vec!["x"; 100].join("+"))
            .unwrap_err()
            .starts_with("limit:")
    );
    let mut fraction = "x".to_string();
    for _ in 0..40 {
        fraction = format!(r"\frac{{1}}{{{fraction}}}");
    }
    assert!(
        latex_to_expression(&fraction)
            .unwrap_err()
            .starts_with("limit:")
    );
    let mut terms = vec!["x".to_string(); 128];
    while terms.len() > 1 {
        terms = terms
            .chunks(2)
            .map(|pair| format!("({}{})", pair[0], pair[1]))
            .collect();
    }
    let expanded = format!("{}x", terms[0]);
    assert!(lex(&expanded).unwrap().len() < MAX_TOKENS);
    assert!(
        latex_to_expression(&expanded)
            .unwrap_err()
            .starts_with("limit:")
    );
    let large = vec!["0".repeat(500); 8].join("+");
    assert!(large.len() < MAX_BYTES);
    assert!(
        latex_to_expression(&format!("{large}+{}", "0".repeat(80)))
            .unwrap_err()
            .starts_with("limit:")
    );
    assert!(latex_to_expression(&"9".repeat(400)).is_err());
    assert!(latex_to_expression(&format!("0.{}1", "0".repeat(400))).is_err());
}

#[test]
fn conversion_does_not_evaluate_or_invent_confidence() {
    converts(r"\frac{1}{0}", "(1/0)");
    converts(r"\sqrt{-1}", "sqrt((-1))");
    converts(r"\ln(-2)", "ln((-2))");
    converts(r"x^{100}", "(x^100)");
}
