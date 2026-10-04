use super::*;

fn plot_bounds() -> Bounds2D {
    Bounds2D {
        x_min: -10.0,
        x_max: 10.0,
        y_min: -10.0,
        y_max: 10.0,
    }
}

fn checked_plot(equation: &str) -> SampledCurve {
    let result = sample_plot(equation, plot_bounds(), 512).unwrap();
    let (lhs, rhs) = equation.split_once('=').unwrap();
    let (lhs, rhs) = (parse(lhs).unwrap(), parse(rhs).unwrap());
    for point in result.segments.iter().flatten() {
        assert!(point.x.is_finite() && point.y.is_finite());
        let a = lhs.eval_xy(point.x, point.y).unwrap();
        let b = rhs.eval_xy(point.x, point.y).unwrap();
        assert!(
            (a - b).abs() <= 1e-9 * (a.abs() + b.abs()).max(1.0),
            "{equation}: {point:?}: {a} != {b}"
        );
    }
    assert!(result.segments.iter().map(Vec::len).sum::<usize>() <= 2050);
    result
}

#[test]
fn plot_classification_preserves_storage_and_calculation_semantics() {
    for input in [
        "y^2=x",
        "(y^2)=x",
        "y-x=1",
        "y=y+1",
        "((x-1)^2/9)+((y+2)^2/4)=1",
    ] {
        assert_eq!(plot_expression(input), Some(input.into()));
        assert_eq!(
            classify_plot(input).unwrap(),
            PlotKind::Implicit(input.into())
        );
    }
    for (input, rhs) in [
        ("y = sin(x)", "sin(x)"),
        ("f(x)=cos(x)", "cos(x)"),
        ("(y)=2", "2"),
    ] {
        assert_eq!(plot_expression(input), Some(rhs.into()));
        assert_eq!(
            classify_plot(input).unwrap(),
            PlotKind::Explicit(rhs.into())
        );
    }
    for input in [
        "x=2", "x^2=2", "2x+3=x-1", "sin(x)", "sin(y)=x", "y^3=x", "y=sin(y)", "y=x; x=2",
    ] {
        assert_eq!(plot_expression(input), None, "{input}");
    }
    assert_eq!(calculate("x=2").unwrap(), "x = 2");
    assert_eq!(calculate("2x+3=x-1").unwrap(), "x = -4");
    let Solutions::Finite(roots) = solve_equation("x^2=2").unwrap() else {
        panic!("expected roots")
    };
    assert_eq!(roots.len(), 2);
    close(roots[0], -2.0_f64.sqrt());
    close(roots[1], 2.0_f64.sqrt());
    assert!(
        !sample_plot("sin(x)", plot_bounds(), 512)
            .unwrap()
            .segments
            .is_empty()
    );
    assert!(
        sample_plot("y=y+1", plot_bounds(), 512)
            .unwrap()
            .segments
            .is_empty()
    );
}

#[test]
fn implicit_parabolas_keep_vertex_and_both_arms() {
    for equation in ["y^2=x", "(y^2)=x", "(y-2)^2=x-1", "(x+y)^2=x-y"] {
        let curve = checked_plot(equation);
        assert_eq!(curve.segments.len(), 1);
        assert!(curve.segments[0].len() >= 513);
    }
    let curve = checked_plot("y^2=x");
    let points = &curve.segments[0];
    assert!(points.contains(&Point { x: 0.0, y: 0.0 }));
    assert!(points.iter().any(|p| p.y < -1.0));
    assert!(points.iter().any(|p| p.y > 1.0));
    assert!(points.iter().all(|p| p.x >= 0.0));
}

#[test]
fn implicit_ellipses_close_and_cover_shifted_axes() {
    for equation in [
        "(x-1)^2+(y+2)^2=4",
        "((x-1)^2/9)+((y+2)^2/4)=1",
        "2*(x-1)^2+2*(x-1)*(y+2)+3*(y+2)^2=1",
        "1e-100*((x-1)^2+(y+2)^2)=4e-100",
    ] {
        let curve = checked_plot(equation);
        assert_eq!(curve.segments.len(), 1);
        let points = &curve.segments[0];
        assert_eq!(points.first(), points.last());
        assert!(points.iter().any(|p| p.x < 1.0));
        assert!(points.iter().any(|p| p.x > 1.0));
        assert!(points.iter().any(|p| p.y < -2.0));
        assert!(points.iter().any(|p| p.y > -2.0));
    }
    let curve = checked_plot("((x-1)^2/9)+((y+2)^2/4)=1");
    let points = &curve.segments[0];
    for target in [
        Point { x: -2.0, y: -2.0 },
        Point { x: 4.0, y: -2.0 },
        Point { x: 1.0, y: -4.0 },
        Point { x: 1.0, y: 0.0 },
    ] {
        assert!(
            points
                .iter()
                .any(|p| (p.x - target.x).hypot(p.y - target.y) < 1e-12)
        );
    }
}

#[test]
fn tiny_visible_circle_is_not_dropped_by_coefficient_tolerance() {
    for equation in [
        "x^2+y^2=1e-24",
        "(x-1)^2+(y-2)^2=1e-20",
        "((x-0.0001)^2+(y+0.0002)^2)=1e-24",
    ] {
        let curve = checked_plot(equation);
        assert_eq!(curve.segments.len(), 1);
        let points = &curve.segments[0];
        assert_eq!(points.len(), 513);
        assert_eq!(points.first(), points.last());
        let (lhs, rhs) = equation.split_once('=').unwrap();
        let lhs = parse(lhs).unwrap();
        let r2 = evaluate(rhs).unwrap();
        for p in points {
            assert!(
                (lhs.eval_xy(p.x, p.y).unwrap() / r2 - 1.0).abs() < 1e-5,
                "{equation}: {p:?}"
            );
        }
    }
}

#[test]
fn hyperbola_branches_stay_separate() {
    for equation in [
        "x^2-y^2=1",
        "y^2-x^2=1",
        "x*y=1",
        "(x-2)*(y+3)=1",
        "(x+y)^2-4*(x-y)^2=1",
    ] {
        let curve = checked_plot(equation);
        assert_eq!(curve.segments.len(), 2, "{equation}");
    }
    let curve = checked_plot("x^2-y^2=1");
    for segment in curve.segments {
        assert!(segment.iter().all(|p| p.x >= 1.0) || segment.iter().all(|p| p.x <= -1.0));
        assert!(segment.iter().any(|p| p.y == 0.0));
    }
    let curve = checked_plot("x*y=1");
    for segment in curve.segments {
        let sign = segment[0].x.signum();
        assert!(
            segment
                .iter()
                .all(|p| p.x.signum() == sign && p.y.signum() == sign)
        );
    }
}

#[test]
fn conic_degeneracies_do_not_draw_fake_lines() {
    for (equation, count) in [
        ("y-x=1", 1),
        ("x=2", 1),
        ("y-2=0", 1),
        ("(x-1)^2=0", 1),
        ("(y+2)^2=0", 1),
        ("(x+y)^2=0", 1),
        ("(x+y-1)^2=0", 1),
        ("(x+y-1)^2=1", 2),
        ("(x-1)^2=4", 2),
        ("x^2-y^2=0", 2),
        ("x*y=0", 2),
        ("(x-1)^2+(y+2)^2=0", 1),
        ("x^2+y^2=-1", 0),
        ("y=y+1", 0),
        ("(x-1)^2=-1", 0),
    ] {
        assert_eq!(checked_plot(equation).segments.len(), count, "{equation}");
    }
    let point = checked_plot("(x-1)^2+(y+2)^2=0");
    assert_eq!(point.segments, vec![vec![Point { x: 1.0, y: -2.0 }]]);
    assert!(matches!(
        sample_plot("y=y", plot_bounds(), 512),
        Err(MathError::Unsupported(_))
    ));
    assert!(matches!(
        sample_plot("sin(y)=x", plot_bounds(), 512),
        Err(MathError::Unsupported(_))
    ));
}

#[test]
fn plot_rejects_invalid_bounds_steps_and_statement_budgets() {
    for steps in [0, 7, 4097, usize::MAX] {
        assert!(matches!(
            sample_plot("y^2=x", plot_bounds(), steps),
            Err(MathError::Limit(_))
        ));
    }
    for bounds in [
        Bounds2D {
            x_min: 1.0,
            x_max: 1.0,
            ..plot_bounds()
        },
        Bounds2D {
            y_min: f64::NAN,
            ..plot_bounds()
        },
        Bounds2D {
            y_max: 1e7,
            ..plot_bounds()
        },
    ] {
        assert!(sample_plot("y^2=x", bounds, 512).is_err());
        assert!(sample_plot("sin(x)", bounds, 512).is_err());
    }
    let long = format!("y={}1", "(".repeat(4096));
    assert!(matches!(classify_plot(&long), Err(MathError::Limit(_))));
    let part = (0..60).map(|_| "(1+1)").collect::<Vec<_>>().join("+");
    assert!(matches!(
        classify_plot(&format!("{part}+y={part}")),
        Err(MathError::Limit(_))
    ));
    assert!(classify_plot("y=x=1").is_err());
    let curve = sample_plot("x*y=1", plot_bounds(), 4096).unwrap();
    assert!(curve.segments.iter().map(Vec::len).sum::<usize>() <= 2050);
}

fn display_calculation(input: &str) -> Result<DisplayCalculation> {
    calculate_display_with_bounds(
        input,
        Bounds2D {
            x_min: -2.0,
            x_max: 2.0,
            y_min: -2.0,
            y_max: 2.0,
        },
        options(),
    )
}

#[test]
fn exact_constants_keep_literals_and_simplify_radicals() {
    for (input, expected) in [
        ("1/2+1/3", "5/6"),
        ("0.1+0.2", "3/10"),
        ("1.20e-2 + 3E-3", "3/200"),
        ("001.2000", "6/5"),
        ("1e3+2E2", "1200"),
        (".50", "1/2"),
        ("9007199254740993-9007199254740992", "1"),
        ("sqrt(1)+sqrt(3)", "1 + sqrt(3)"),
        ("sqrt(8)", "2*sqrt(2)"),
        ("sqrt(8)+sqrt(18)-sqrt(2)", "4*sqrt(2)"),
        ("sqrt(2)*sqrt(3)", "sqrt(6)"),
        ("sqrt(6)*sqrt(10)", "2*sqrt(15)"),
        ("sqrt(2)*sqrt(2)", "2"),
        ("sqrt(2/3)", "sqrt(6)/3"),
        ("sqrt(4/9)", "2/3"),
        ("1/sqrt(2)", "sqrt(2)/2"),
        ("1/(1+sqrt(2))", "-1 + sqrt(2)"),
        ("1/(sqrt(2)+sqrt(3))", "-sqrt(2) + sqrt(3)"),
        ("(1+sqrt(2))/(1-sqrt(2))", "-3 - 2*sqrt(2)"),
        ("(1/2)/(3/4)", "2/3"),
        ("1/(-2)", "-1/2"),
        ("-sqrt(2)/3", "-sqrt(2)/3"),
        ("(1+sqrt(3))/2", "(1 + sqrt(3))/2"),
        ("(sqrt(2)+sqrt(3))^2", "5 + 2*sqrt(6)"),
        ("(1+sqrt(2))^-1", "-1 + sqrt(2)"),
        ("4^0.5", "2"),
        ("abs(-1/3)", "1/3"),
        ("-2^-2", "-1/4"),
        ("2^3^2", "512"),
        ("8/2(2+2)", "16"),
        ("（１÷２）＋１÷３", "5/6"),
        ("sqrt(0)", "0"),
        (
            "sqrt(1000000000000000000000000000000000000)",
            "1000000000000000000",
        ),
        ("sqrt(2)/(-sqrt(8))", "-1/2"),
        ("1/(sqrt(2)/2+sqrt(3)/3)", "3*sqrt(2) - 2*sqrt(3)"),
        ("0/-2", "0"),
    ] {
        let result = display_calculation(input).unwrap();
        assert_eq!(result.text, expected, "{input}");
        assert_eq!(calculate(input).unwrap(), expected, "{input}");
        assert!(!result.approximate, "{input}");
        assert!(result.display.is_some(), "{input}");
        if !input.starts_with("9007199254740993") {
            close(evaluate(&result.text).unwrap(), evaluate(input).unwrap());
        }
    }
}

#[test]
fn exact_display_uses_real_fraction_and_radical_nodes() {
    use MathDisplay::{Fraction, Radical, Row, Text};
    let text = |s: &str| Text(s.into());
    assert_eq!(
        display_calculation("1/2+1/3").unwrap().display,
        Some(Fraction(Box::new(text("5")), Box::new(text("6"))))
    );
    assert_eq!(
        display_calculation("-sqrt(8)/4").unwrap().display,
        Some(Fraction(
            Box::new(Row(vec![text("-"), Radical(Box::new(text("2")))])),
            Box::new(text("2"))
        ))
    );
    assert_eq!(
        display_calculation("(1+sqrt(3))/2").unwrap().display,
        Some(Fraction(
            Box::new(Row(vec![
                text("1"),
                text(" + "),
                Radical(Box::new(text("3")))
            ])),
            Box::new(text("2"))
        ))
    );
}

#[test]
fn only_unsupported_exact_operations_fall_back_to_marked_approximation() {
    for input in [
        "pi",
        "e",
        "2pi",
        "pi-pi",
        "sin(0)",
        "ln(2)",
        "sqrt(1+sqrt(2))",
        "2^(1/3)",
        "1/(1+sqrt(2)+sqrt(3))",
    ] {
        let result = display_calculation(input).unwrap();
        assert!(result.approximate, "{input}");
        assert!(result.text.starts_with("≈ "), "{input}");
        assert_eq!(calculate(input).unwrap(), result.text);
        assert_eq!(result.display, Some(MathDisplay::Text(result.text.clone())));
        close(
            result.text.trim_start_matches("≈ ").parse().unwrap(),
            evaluate(input).unwrap(),
        );
    }
    for input in [
        "0/0",
        "sqrt(-1)",
        "0^0",
        "0^-1",
        "(-2)^0.5",
        "1/(sqrt(8)-2sqrt(2))",
        "sin(0)+0^0",
        "sin(0)/0",
        "ln(-1)",
        "sqrt(-1)+pi",
        "pi+sqrt(-1)",
        "0*sqrt(-1)",
    ] {
        assert!(
            matches!(display_calculation(input), Err(MathError::Domain(_))),
            "{input}"
        );
    }
    for input in [
        "1e38*10",
        "1e-39",
        "2^129",
        "1^1000000000",
        "sqrt(10000000019)",
        "pi+2^129",
        "170141183460469231731687303715884105728",
    ] {
        assert!(
            matches!(display_calculation(input), Err(MathError::Limit(_))),
            "{input}"
        );
    }
    let many_terms = [2, 3, 5, 7, 11, 13]
        .map(|p| format!("(1+sqrt({p}))"))
        .join("*");
    assert!(matches!(
        display_calculation(&many_terms),
        Err(MathError::Limit(_))
    ));
    for input in ["1e-999", "1e999", "sin(1)+", "sqrt(1", "1..2", ""] {
        assert!(display_calculation(input).is_err(), "{input}");
    }
}

#[test]
fn exact_display_keeps_existing_equation_and_symbolic_outputs() {
    for input in [
        "x^2-5x+6=0",
        "x^2=2",
        "x=1;y=0",
        "x^2+y^2=1;y=x",
        "simplify(x/2+x/3)",
        "diff(x^3,x)",
        "x+x",
        "0.1*x+0.2*x",
    ] {
        let result = display_calculation(input).unwrap();
        let expected = calculate_with_bounds(
            input,
            Bounds2D {
                x_min: -2.0,
                x_max: 2.0,
                y_min: -2.0,
                y_max: 2.0,
            },
            options(),
        )
        .unwrap();
        assert_eq!(result.text, expected, "{input}");
        assert_eq!(result.display, None);
        assert!(!result.approximate);
    }
    assert_eq!(calculate("x^2-5x+6=0").unwrap(), "x = 2；x = 3");
    assert_eq!(calculate("x+y=3;x-y=1").unwrap(), "x = 2, y = 1");
}

#[test]
fn simplify_equations_and_systems_without_solving() {
    for (input, expected) in [
        ("2x+3x=10", "5*x - 10 = 0"),
        ("（ｘ＋１）²＝ｘ²＋２ｘ＋１", "0 = 0"),
        ("x=x+1", "-1 = 0"),
        ("0=1", "-1 = 0"),
        ("x/2+y/4=3/4", "0.5*x + 0.25*y - 0.75 = 0"),
        ("x*(x-1)=0", "x^2 - x = 0"),
        ("x^12=x^11", "x^12 - x^11 = 0"),
        (
            "x+x=2；y+y=4；x+y=3",
            "2*x - 2 = 0; 2*y - 4 = 0; x + y - 3 = 0",
        ),
        ("(x+1)^2-x^2+2y-y", "2*x + y + 1"),
        ("2+3*4", "14"),
    ] {
        assert_eq!(simplify(input).unwrap(), expected, "{input}");
        assert_eq!(calculate(&format!("simplify({input})")).unwrap(), expected);
        assert_eq!(simplify(expected).unwrap(), expected);
    }
    let tiny = simplify("1e-20*x=1e-20").unwrap();
    assert_eq!(
        tiny,
        "0.00000000000000000001*x - 0.00000000000000000001 = 0"
    );
    assert_eq!(solve_equation(&tiny).unwrap(), Solutions::Finite(vec![1.0]));
    assert_eq!(
        solve_equation(&simplify("x^2=x").unwrap()).unwrap(),
        Solutions::Finite(vec![0.0, 1.0])
    );
    assert_eq!(calculate("2x=6").unwrap(), "x = 3");
}

#[test]
fn equation_simplification_keeps_domain_and_error_boundaries() {
    for input in ["x/x=1", "0*(1/x)=0", "x^0=1", "x^-1=0", "sin(x)=0"] {
        assert!(
            matches!(simplify(input), Err(MathError::Unsupported(_))),
            "{input}"
        );
    }
    for input in [
        "x/0=1",
        "1/(x-x)=0",
        "exp(1000)*x=0",
        "10^1000*x=0",
        "0^0=1",
    ] {
        assert!(
            matches!(simplify(input), Err(MathError::Domain(_))),
            "{input}"
        );
    }
    for input in [
        "x==1", "x=1=2", "=1", "x=", "x=1;", ";x=1", "x=1;;y=2", "x=1;y", "x;y", "",
    ] {
        assert!(
            matches!(simplify(input), Err(MathError::Syntax(_))),
            "{input}"
        );
    }
    for input in ["1e-999*x=0", "1e-200*x/1e200=0", "(1e-200*x)^2=0"] {
        assert!(
            matches!(simplify(input), Err(MathError::Numerical(_))),
            "{input}"
        );
    }
    assert!(matches!(simplify("x^13=0"), Err(MathError::Limit(_))));
    assert!(matches!(
        simplify(&vec!["x=0"; 129].join(";")),
        Err(MathError::Limit(_))
    ));
    assert!(matches!(
        simplify(&" ".repeat(4097)),
        Err(MathError::Limit(_))
    ));
}

#[test]
fn symbolic_polynomial_calculus_uses_calculate_and_bounded_entry() {
    let bounds = Bounds2D {
        x_min: -1.0,
        x_max: 1.0,
        y_min: -1.0,
        y_max: 1.0,
    };
    for (input, expected) in [
        ("diff(x^3+2x+1,x)", "3*x^2 + 2"),
        ("diff(x^2*y+3y^2,y)", "x^2 + 6*y"),
        ("diff(7,x)", "0"),
        ("diff(y,x)", "0"),
        ("ｄｉｆｆ（ｘ²，ｘ）", "2*x"),
        ("integrate(3x^2+2,x)", "x^3 + 2*x"),
        ("integrate(2*x*y,y)", "x*y^2"),
        ("integrate(y,x)", "x*y"),
        ("integrate(0,y)", "0"),
        ("diff ( (x+1)^2 / 2 , x )", "x + 1"),
        ("simplify(x=x;y=y)", "0 = 0; 0 = 0"),
    ] {
        assert_eq!(calculate(input).unwrap(), expected, "{input}");
        assert_eq!(
            calculate_with_bounds(input, bounds, options()).unwrap(),
            expected
        );
    }
    assert_eq!(differentiate_polynomial("x^12", 'x').unwrap(), "12*x^11");
    assert_eq!(antiderivative_polynomial("12*x^11", 'x').unwrap(), "x^12");
    for input in ["x^2+2x+3", "x*y^2+3*y", "0", "2*x^3*y^2"] {
        for variable in ['x', 'y'] {
            let primitive = antiderivative_polynomial(input, variable).unwrap();
            let restored = differentiate_polynomial(&primitive, variable).unwrap();
            let original = Polynomial::parse(input).unwrap();
            let restored = Polynomial::parse(&restored).unwrap();
            for x in [-2.0, 0.0, 0.5, 3.0] {
                close(
                    original.eval(x, 0.75).unwrap(),
                    restored.eval(x, 0.75).unwrap(),
                );
            }
        }
    }
}

#[test]
fn symbolic_commands_are_bounded_not_general_cas() {
    for input in [
        "diff(sin(x),x)",
        "integrate(1/x,x)",
        "diff(x/x,x)",
        "diff(x^0,x)",
        "diff(x,z)",
        "diff(x)",
        "diff(x,x,y)",
        "diff(x,)",
        "diff(,x)",
        "diff(x,x)+1",
        "diff(x,x)(1)",
        "diff(x,x",
        "diff((x,x)",
        "diff(x=1,x)",
        "diff(diff(x,x),x)",
        "integrate(x,0,1)",
        "simplify(x=1);y=2",
        "simplify(x=1,y=2)",
    ] {
        assert!(calculate(input).is_err(), "{input}");
    }
    for input in ["diff(x^13,x)", "integrate(x^12,x)", "integrate(y^12,x)"] {
        assert!(
            matches!(calculate(input), Err(MathError::Limit(_))),
            "{input}"
        );
    }
    assert!(matches!(
        calculate("diff(1e308*x^2,x)"),
        Err(MathError::Domain(_))
    ));
    assert!(calculate("integrate(5e-324*x,y)").is_ok());
    assert!(matches!(
        calculate("integrate(5e-324*x,x)"),
        Err(MathError::Numerical(_))
    ));
    assert!(matches!(
        calculate(&format!("simplify({})", vec!["x=0"; 128].join(";"))),
        Err(MathError::Limit(_))
    ));
}

#[test]
fn handwriting_normalization_and_minimal_output() {
    assert_eq!(calculate("（２＋３）×４÷２−１").unwrap(), "9");
    assert_eq!(calculate("ｘ²−５ｘ＋６＝０").unwrap(), "x = 2；x = 3");
    assert_eq!(calculate("x＝１；y＝０").unwrap(), "x = 1, y = 0");
    close(
        evaluate("２π＋sin（π÷２）").unwrap(),
        2.0 * std::f64::consts::PI + 1.0,
    );
    close(eval_at("πx²", 2.0).unwrap(), 4.0 * std::f64::consts::PI);
    for input in ["s in(0)", "sｉ n(0)", "sinπ", "p i", "ex p(1)"] {
        assert!(parse(input).is_err(), "{input}");
    }
    for input in ["-0", "0/-2"] {
        assert_eq!(calculate(input).unwrap(), "0");
    }
    assert_eq!(calculate("x^2=0").unwrap(), "x = 0");
    assert_eq!(calculate("x=0;y=0").unwrap(), "x = 0, y = 0");
    for input in ["1e-999*x=0", "(1e-200)^2*x=0", "1e-200*1e-200*x=0"] {
        assert!(calculate(input).is_err(), "{input}");
    }
}

#[test]
fn generated_quadratics_use_known_roots_not_solver_residuals() {
    let mut state = 0x615a_9017_u64;
    for _ in 0..80 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let r = ((state >> 32) % 257) as f64 / 8.0 - 16.0;
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let s = r + 0.25 + ((state >> 32) % 128) as f64 / 8.0;
        for scale in ["1e-200", "1", "1e200"] {
            let input = format!("{scale}*(x^2-({})*x+({}))=0", r + s, r * s);
            let Solutions::Finite(actual) = solve_equation(&input).unwrap() else {
                panic!("{input}")
            };
            assert_eq!(actual.len(), 2, "{input}");
            assert!((actual[0] - r).abs() < 1e-10, "{input}: {actual:?}");
            assert!((actual[1] - s).abs() < 1e-10, "{input}: {actual:?}");
        }
        let input = format!("x^2-({})*x+({})=0", 2.0 * r, r * r);
        assert_eq!(solve_equation(&input).unwrap(), Solutions::Finite(vec![r]));
        assert_eq!(
            solve_equation(&format!("0*x^2+8*x={}", 8.0 * r)).unwrap(),
            Solutions::Finite(vec![r])
        );
    }
}

#[test]
fn generated_linear_systems_use_known_solutions_and_row_scaling() {
    for i in -12..=12 {
        let x = i as f64 / 4.0;
        let y = (i * i - 7) as f64 / 8.0;
        for (s, t) in [("1", "1"), ("1e-200", "1e200"), ("-1e200", "1e-200")] {
            let p = format!("{s}*(3x+2y-({}))=0", 3.0 * x + 2.0 * y);
            let q = format!("{t}*(5x-7y-({}))=0", 5.0 * x - 7.0 * y);
            let SystemSolutions::Unique { x: a, y: b } = solve_linear_system(&p, &q).unwrap()
            else {
                panic!()
            };
            assert!((a - x).abs() < 1e-12 && (b - y).abs() < 1e-12);
        }
    }
    for (p, q, expected) in [
        ("0=1", "0=0", SystemSolutions::None),
        ("x+y=1", "2x+2y=3", SystemSolutions::None),
        ("x+y=1", "-2x-2y=-2", SystemSolutions::Infinite),
    ] {
        assert_eq!(solve_linear_system(p, q).unwrap(), expected);
    }
}

#[test]
fn root_amplitude_and_discontinuity_metamorphisms() {
    for scale in ["1e-200", "1", "1e200"] {
        let found = roots(&format!("{scale}*(x^2-2)"), -2.0, 2.0, options())
            .unwrap()
            .roots;
        assert_eq!(found.len(), 2, "{scale}: {found:?}");
        assert!((found[0] + 2.0_f64.sqrt()).abs() < 1e-8);
        assert!((found[1] - 2.0_f64.sqrt()).abs() < 1e-8);
        let found = roots(&format!("{scale}*(x-0.12345)^2"), -1.0, 1.0, options())
            .unwrap()
            .roots;
        assert_eq!(found.len(), 1, "{scale}: {found:?}");
        assert!((found[0] - 0.12345).abs() < 1e-8);
        assert_eq!(
            roots(&format!("{scale}*(x^2-1)"), -1.0, 1.0, options())
                .unwrap()
                .roots,
            vec![-1.0, 1.0]
        );
        for input in [
            format!("{scale}*(x-0.12345)/abs(x-0.12345)"),
            format!("{scale}/(x-0.12345)"),
        ] {
            assert!(
                roots(&input, -1.0, 1.0, options())
                    .unwrap()
                    .roots
                    .is_empty(),
                "{input}"
            );
        }
    }
    let found = roots("tanh(1e8*(x-0.12345))", -1.0, 1.0, options())
        .unwrap()
        .roots;
    assert_eq!(found.len(), 1);
    assert!((found[0] - 0.12345).abs() < 1e-12);
}

#[test]
fn integral_amplitude_and_orientation_metamorphisms() {
    let c: f64 = 0.12345;
    let exact = (c * c + (1.0 - c).powi(2)) / 2.0;
    for scale in [1e-200, 1.0, 1e200] {
        let input = format!("{scale}*abs(x-{c})");
        let forward = integrate(&input, 0.0, 1.0, options()).unwrap() / scale;
        let backward = integrate(&input, 1.0, 0.0, options()).unwrap() / scale;
        assert!((forward - exact).abs() < 1e-8, "{scale}: {forward}");
        assert!((backward + exact).abs() < 1e-8);
        for power in [1, 2] {
            let input = format!("{scale}/(x-{c})^{power}");
            assert!(integrate(&input, 0.0, 1.0, options()).is_err(), "{input}");
        }
    }
}

#[test]
fn derivative_relative_accuracy_and_scaled_cusps() {
    for scale in [1e-200, 1.0, 1e200] {
        for (input, x, expected) in [
            ("sin(x)", 1e6, 1e6_f64.cos()),
            ("exp(x)", 3.0, 3.0_f64.exp()),
            ("x^3", 2.0, 12.0),
            ("ln(x)", 0.5, 2.0),
        ] {
            let actual = derivative(&format!("{scale}*({input})"), x).unwrap() / scale;
            assert!(
                (actual / expected - 1.0).abs() < 1e-7,
                "{scale} {input}: {actual} != {expected}"
            );
        }
        assert!(derivative(&format!("{scale}*abs(x)"), 0.0).is_err());
    }
}

#[test]
fn quadratic_systems_match_cartesian_known_roots_under_scaling() {
    let b = Bounds2D {
        x_min: -2.0,
        x_max: 2.0,
        y_min: -2.0,
        y_max: 2.0,
    };
    let o = NumericOptions {
        steps: 64,
        max_evaluations: 100000,
        ..options()
    };
    for (s, t) in [("1", "1"), ("1e-200", "1e200")] {
        let actual = solve_quadratic_system(
            &format!("{s}*(x+0.75)*(x-1.25)=0"),
            &format!("{t}*(y+1.5)*(y-0.5)=0"),
            b,
            o,
        )
        .unwrap();
        assert_eq!(actual.points.len(), 4);
        for x in [-0.75, 1.25] {
            for y in [-1.5, 0.5] {
                assert!(
                    actual
                        .points
                        .iter()
                        .any(|p| (p.x - x).hypot(p.y - y) < 1e-7)
                );
            }
        }
    }
}

#[test]
fn underflow_is_not_an_identity_or_a_sampled_root() {
    for input in ["exp(-1000)*x=0", "(1e-200)^2=0", "1e-200/1e200=0"] {
        assert!(calculate(input).is_err(), "{input}");
    }
    for input in ["1e-200*1e-200", "1e-200/1e200", "(1e-200)^2", "exp(-1000)"] {
        assert!(evaluate(input).is_err(), "{input}");
    }
    // exp 在 [-460,460] 内非零；归一化后两端跨越 f64 可表示范围。
    assert!(matches!(
        roots("exp(x)", -460.0, 460.0, options()),
        Err(MathError::Numerical(_))
    ));
    for input in ["x^2", "x^3", "cos(x)", "1+x^3"] {
        let actual = derivative(input, 0.0).unwrap();
        assert!(actual.abs() < 1e-12, "{input}: {actual}");
    }
    for scale in [1e-200, 1.0, 1e200] {
        for (input, expected) in [("x^2", 2e-8), ("x^3", 3e-16)] {
            let actual = derivative(&format!("{scale}*{input}"), 1e-8).unwrap() / scale;
            assert!((actual / expected - 1.0).abs() < 1e-7, "{input}: {actual}");
        }
    }
}

#[test]
fn repeated_decimal_quadratic_and_near_singular_linear() {
    assert_eq!(
        solve_equation("1.1*x^2+2.2*x+1.1=0").unwrap(),
        Solutions::Finite(vec![-1.0])
    );
    assert!(matches!(
        solve_linear_system("134217729x+134217728y=1", "134217728x+134217727y=1"),
        Err(MathError::Numerical(_))
    ));
}

#[test]
fn polynomial_underflow_must_not_erase_terms() {
    for input in ["(1e-200*x)*(1e-200*x)", "1e-200*(1e-200*x)"] {
        assert!(
            matches!(simplify(input), Err(MathError::Numerical(_))),
            "{input}"
        );
    }
}

#[test]
fn intersection_output_consumes_evaluation_budget() {
    let o = NumericOptions {
        steps: 32,
        max_evaluations: 33,
        ..options()
    };
    assert_eq!(roots("x", 0.0, 1.0, o).unwrap().roots, vec![0.0]);
    assert!(matches!(
        curve_intersections("x", "0", 0.0, 1.0, o),
        Err(MathError::Limit(_))
    ));
}

#[test]
fn integral_avoids_intermediate_weight_overflow() {
    let actual = integrate("1e308", 0.0, 1e-10, options()).unwrap();
    assert!((actual / 1e298 - 1.0).abs() < 1e-12);
}

#[test]
fn powers_keep_unary_signs_and_polynomial_parity() {
    for (input, expected) in [
        ("-2^-2", -0.25),
        ("(-2)^-3", -0.125),
        ("2^--3", 8.0),
        ("-2^3^2", -512.0),
        ("2^-3*4", 0.5),
        ("2^(-3*2)", 1.0 / 64.0),
    ] {
        assert_eq!(evaluate(input).unwrap(), expected, "{input}");
    }
    for input in ["-x^2", "(-x)^2", "-(-x)^3", "(x+y)^4", "(x-1)^2/2"] {
        let expr = parse(input).unwrap();
        let poly = Polynomial::parse(input).unwrap();
        for x in [-3.0, -0.5, 0.0, 2.0] {
            close(expr.eval_xy(x, 0.25).unwrap(), poly.eval(x, 0.25).unwrap());
        }
    }
}

#[test]
fn quadratic_scales_cancellation_and_exact_degeneracy() {
    for scale in ["1e-200", "1", "1e200"] {
        let Solutions::Finite(r) = solve_equation(&format!("{scale}*(x^2-5x+6)=0")).unwrap() else {
            panic!()
        };
        assert_eq!(r.len(), 2);
        close(r[0], 2.0);
        close(r[1], 3.0);
    }
    for b in [-1e12, 1e12] {
        let Solutions::Finite(r) = solve_equation(&format!("x^2+({b})x+1=0")).unwrap() else {
            panic!()
        };
        assert_eq!(r.len(), 2);
        assert!((r[0] * r[1] - 1.0).abs() < 1e-14);
        assert!(((r[0] + r[1]) / -b - 1.0).abs() < 1e-14);
    }
    assert_eq!(
        solve_equation("1e-200*x^2=0").unwrap(),
        Solutions::Finite(vec![0.0])
    );
    assert_eq!(
        solve_equation("x^2-2x+1.0000000000000002=0").unwrap(),
        Solutions::None
    );
    assert!(matches!(
        solve_equation("1e-200*x^2+x+1e-200=0"),
        Err(MathError::Numerical(_))
    ));
    assert!(matches!(
        solve_linear_system("1e-200*x=1", "1e-200*y=1"),
        Err(MathError::Numerical(_))
    ));
}

#[test]
fn singular_integrals_and_tangent_intersections() {
    for input in ["1/(x-0.12345)", "1/(x-0.125)^2", "1/sqrt(x)", "ln(x)"] {
        assert!(integrate(input, 0.0, 1.0, options()).is_err(), "{input}");
    }
    assert_eq!(integrate("sin(x)", 0.5, 0.5, options()).unwrap(), 0.0);
    assert!(integrate("1/x", 0.0, 0.0, options()).is_err());
    let p = curve_intersections("(x-0.12345)^2", "0", -1.0, 1.0, options()).unwrap();
    assert_eq!(p.len(), 1);
    close(p[0].x, 0.12345);
    assert!(p[0].y.abs() <= options().tolerance);
    assert!(
        roots("1e-12/(x-0.12345)", -1.0, 1.0, options())
            .unwrap()
            .roots
            .is_empty()
    );
}

#[test]
fn sampling_preserves_endpoints_domain_and_budget() {
    let c = sample("2x+1", -3.0, 2.0, options()).unwrap();
    assert_eq!(c.skipped_intervals, 0);
    assert_eq!(c.segments[0].len(), options().steps + 1);
    assert_eq!(c.segments[0][0], Point { x: -3.0, y: -5.0 });
    assert_eq!(*c.segments[0].last().unwrap(), Point { x: 2.0, y: 5.0 });
    let c = sample("sqrt(x)", -1.0, 1.0, options()).unwrap();
    assert!(
        c.segments
            .iter()
            .flatten()
            .all(|p| p.x >= 0.0 && p.y.is_finite())
    );
    let c = sample("sqrt(-1)", -1.0, 1.0, options()).unwrap();
    assert!(c.segments.is_empty());
    assert_eq!(c.skipped_intervals, options().steps);
    let o = NumericOptions {
        steps: 8,
        max_evaluations: 32,
        ..options()
    };
    assert!(matches!(sample("x", 0.0, 1.0, o), Err(MathError::Limit(_))));
    assert!(
        sample(
            "x",
            0.0,
            1.0,
            NumericOptions {
                max_evaluations: 33,
                ..o
            }
        )
        .is_ok()
    );
    assert!(matches!(
        integrate("x", 0.0, 1.0, o),
        Err(MathError::Limit(_))
    ));
    assert!(
        integrate(
            "x",
            0.0,
            1.0,
            NumericOptions {
                max_evaluations: 40,
                ..o
            }
        )
        .is_ok()
    );
    assert!(matches!(
        roots("x", 0.0, 1.0, NumericOptions { steps: 32, ..o }),
        Err(MathError::Limit(_))
    ));
}

#[test]
fn hyperbolic_functions_domains_and_calculus() {
    for (input, expected) in [
        ("sinh(0)", 0.0),
        ("cosh(0)", 1.0),
        ("tanh(0)", 0.0),
        ("asinh(0)", 0.0),
        ("acosh(1)", 0.0),
        ("atanh(0)", 0.0),
        ("sinh(ln(2))", 0.75),
        ("cosh(ln(2))", 1.25),
        ("tanh(ln(2))", 0.6),
        ("tanh(1000)", 1.0),
    ] {
        close(evaluate(input).unwrap(), expected);
    }
    for x in [-10.0, -0.1, 0.0, 0.1, 10.0] {
        close(eval_at("asinh(sinh(x))", x).unwrap(), x);
        close(eval_at("acosh(cosh(x))", x).unwrap(), x.abs());
    }
    close(evaluate("atanh(tanh(0.5))").unwrap(), 0.5);
    for input in [
        "acosh(0.5)",
        "atanh(1)",
        "atanh(-1)",
        "atanh(2)",
        "sinh(1000)",
        "cosh(1000)",
    ] {
        assert!(
            matches!(evaluate(input), Err(MathError::Domain(_))),
            "{input}"
        );
    }
    close(derivative("sinh(x)", 1.0).unwrap(), 1.0_f64.cosh());
    close(
        integrate("cosh(x)", 0.0, 1.0, options()).unwrap(),
        1.0_f64.sinh(),
    );
    assert_eq!(simplify("cosh(0)*x").unwrap(), "x");
    assert!(matches!(
        simplify("sinh(x)"),
        Err(MathError::Unsupported(_))
    ));
}

#[test]
fn bounded_calculation_reports_search_not_proof() {
    let b = Bounds2D {
        x_min: -2.0,
        x_max: 2.0,
        y_min: -2.0,
        y_max: 2.0,
    };
    let o = NumericOptions {
        steps: 64,
        max_evaluations: 100000,
        ..options()
    };
    let text = calculate_with_bounds("x^2+y^2=1;y=x", b, o).unwrap();
    assert!(text.contains("数值搜索不保证完备"));
    assert!(text.contains("x = "));
    let text = calculate_with_bounds("x^2+y^2=-1;y=x", b, o).unwrap();
    assert!(text.contains("未找到候选解"));
    assert!(text.contains("未找到不等于无解"));
    assert_eq!(calculate_with_bounds("2+3", b, o).unwrap(), "5");
    let text = calculate_with_bounds("x=1;y=0", b, o).unwrap();
    assert!(text.contains("x = 1, y = "));
    assert!(!text.contains("不保证完备"));
    assert!(
        calculate_with_bounds("x=3;y=0", b, o)
            .unwrap()
            .contains("范围")
    );
    for input in ["x=1;y=1;x=2", "x=1;", "x^3=1;y=0", "0=0;x=1"] {
        assert!(calculate_with_bounds(input, b, o).is_err(), "{input}");
    }
    assert!(calculate("x^2=1;y=0").is_err());
    assert!(matches!(
        calculate_with_bounds(&" ".repeat(4097), b, o),
        Err(MathError::Limit(_))
    ));
    assert!(calculate_with_bounds("x^2=1;y=0", Bounds2D { x_min: 3.0, ..b }, o).is_err());
    assert!(matches!(
        solve_quadratic_system(
            "x^2+y^2=1",
            "y=x",
            b,
            NumericOptions {
                max_evaluations: 32,
                ..o
            }
        ),
        Err(MathError::Limit(_))
    ));
}

#[test]
fn options_and_expression_work_budgets_are_enforced() {
    let input = vec!["sin(x)"; 62].join("+");
    assert!(parse(&input).is_ok());
    let o = NumericOptions {
        steps: 4096,
        max_evaluations: 100000,
        ..options()
    };
    assert!(matches!(
        sample(&input, 0.0, 1.0, o),
        Err(MathError::Limit(_))
    ));
    for o in [
        NumericOptions {
            tolerance: f64::NAN,
            ..options()
        },
        NumericOptions {
            tolerance: 0.0,
            ..options()
        },
        NumericOptions {
            steps: 7,
            ..options()
        },
        NumericOptions {
            max_evaluations: 100001,
            ..options()
        },
    ] {
        assert!(matches!(roots("x", 0.0, 1.0, o), Err(MathError::Limit(_))));
        assert!(matches!(sample("x", 0.0, 1.0, o), Err(MathError::Limit(_))));
        assert!(matches!(
            integrate("x", 0.0, 0.0, o),
            Err(MathError::Limit(_))
        ));
    }
    for (a, b) in [
        (1.0, 0.0),
        (0.0, 0.0),
        (f64::NAN, 1.0),
        (0.0, f64::INFINITY),
        (-1000001.0, 0.0),
    ] {
        assert!(roots("x", a, b, options()).is_err());
        assert!(sample("x", a, b, options()).is_err());
    }
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6 * (1.0 + b.abs()), "{a} != {b}");
}
fn options() -> NumericOptions {
    NumericOptions {
        steps: 128,
        ..NumericOptions::default()
    }
}

#[test]
fn precedence_and_negative_exponents() {
    for (s, v) in [
        ("1+2*3", 7.0),
        ("-2^2", -4.0),
        ("(-2)^2", 4.0),
        ("2^-3", 0.125),
        ("2^3^2", 512.0),
        ("2^-2^2", 0.0625),
        ("2--3", 5.0),
        ("8/2(2+2)", 16.0),
        ("2(3+4)", 14.0),
        ("(2+1)(4-1)", 9.0),
        ("1e-3+2E2", 200.001),
    ] {
        close(evaluate(s).unwrap(), v);
    }
    close(eval_at("2x(x+1)", 3.0).unwrap(), 24.0);
    close(parse("xy+2y").unwrap().eval_xy(3.0, 4.0).unwrap(), 20.0);
}
#[test]
fn constants_and_functions() {
    close(evaluate("sin(pi/2)+cos(0)+tan(0)").unwrap(), 2.0);
    close(
        evaluate("ln(e)+log(100)+sqrt(9)+abs(-2)+exp(0)").unwrap(),
        9.0,
    );
    close(
        evaluate("asin(1)+acos(0)+atan(1)").unwrap(),
        std::f64::consts::PI * 1.25,
    );
    close(evaluate("2pi").unwrap(), std::f64::consts::PI * 2.0);
    assert!(matches!(
        evaluate("x"),
        Err(MathError::MissingVariable('x'))
    ));
}
#[test]
fn domain_and_syntax_errors() {
    for s in [
        "1/0",
        "sqrt(-1)",
        "ln(0)",
        "log(-1)",
        "asin(2)",
        "acos(-2)",
        "(-2)^0.5",
        "0^-2",
        "0^0",
        "tan(pi/2)",
        "exp(1000)",
        "1e999",
    ] {
        assert!(matches!(evaluate(s), Err(MathError::Domain(_))), "{s}");
    }
    for s in [
        "",
        "(",
        "1+",
        "sin(",
        "1)",
        "unknown(1)",
        "sin x",
        "1..2",
        "x=2",
    ] {
        assert!(evaluate(s).is_err(), "{s}");
    }
    assert!(eval_at("x", f64::NAN).is_err());
}
#[test]
fn bounded_input_and_depth() {
    assert!(matches!(
        evaluate(&" ".repeat(4097)),
        Err(MathError::Limit(_))
    ));
    assert!(matches!(
        evaluate(&format!("{}1{}", "(".repeat(80), ")".repeat(80))),
        Err(MathError::Limit(_))
    ));
    assert!(matches!(
        evaluate(&vec!["1"; 100].join("+")),
        Err(MathError::Limit(_))
    ));
    assert!(matches!(simplify("(x+y)^13"), Err(MathError::Limit(_))));
    assert!(matches!(
        roots(
            "x",
            -1.0,
            1.0,
            NumericOptions {
                steps: usize::MAX,
                ..options()
            }
        ),
        Err(MathError::Limit(_))
    ));
    assert!(matches!(
        sample(
            "x",
            -1.0,
            1.0,
            NumericOptions {
                max_evaluations: 32,
                ..options()
            }
        ),
        Err(MathError::Limit(_))
    ));
}
#[test]
fn polynomial_scope_and_calculate() {
    assert_eq!(simplify("(x+1)^2-x^2+2y-y").unwrap(), "2*x + y + 1");
    assert_eq!(simplify("x-x").unwrap(), "0");
    assert_eq!(calculate("2+3*4").unwrap(), "14");
    assert_eq!(calculate("x+x").unwrap(), "2*x");
    for s in ["x/x", "sin(x)", "x^-1", "x^0", "x^0.5", "1/(x-x)", "x^x"] {
        assert!(simplify(s).is_err(), "{s}");
    }
    let p = Polynomial::parse("(x+y)^3").unwrap();
    close(p.eval(2.0, 3.0).unwrap(), 125.0);
    assert_eq!(p.coefficient(2, 1), 3.0);
    assert_eq!(p.degree(), 3);
    assert_eq!(simplify("1e-20*x").unwrap(), "0.00000000000000000001*x");
}
#[test]
fn equations_and_degeneracy() {
    assert_eq!(
        solve_equation("2x+4=0").unwrap(),
        Solutions::Finite(vec![-2.0])
    );
    assert_eq!(
        solve_equation("x^2-5x+6=0").unwrap(),
        Solutions::Finite(vec![2.0, 3.0])
    );
    assert_eq!(
        solve_equation("x^2+2x+1=0").unwrap(),
        Solutions::Finite(vec![-1.0])
    );
    assert_eq!(solve_equation("x^2+1=0").unwrap(), Solutions::None);
    assert_eq!(solve_equation("x=x").unwrap(), Solutions::Infinite);
    assert_eq!(solve_equation("x=x+1").unwrap(), Solutions::None);
    assert_eq!(
        solve_equation("0x^2+2x=4").unwrap(),
        Solutions::Finite(vec![2.0])
    );
    assert_eq!(
        solve_equation("1e-20*x=1e-20").unwrap(),
        Solutions::Finite(vec![1.0])
    );
    let Solutions::Finite(r) = solve_equation("x^2+1e8*x+1=0").unwrap() else {
        panic!()
    };
    assert!((r[1] + 1e-8).abs() < 1e-20);
    assert!(solve_equation("y=1").is_err());
    assert!(solve_equation("x^3=0").is_err());
    assert!(solve_equation("x=1=2").is_err());
    assert_eq!(calculate("2x=6").unwrap(), "x = 3");
}
#[test]
fn linear_systems() {
    assert_eq!(
        solve_linear_system("x+y=3", "x-y=1").unwrap(),
        SystemSolutions::Unique { x: 2.0, y: 1.0 }
    );
    assert_eq!(
        solve_linear_system("x+y=3", "2x+2y=6").unwrap(),
        SystemSolutions::Infinite
    );
    assert_eq!(
        solve_linear_system("x+y=3", "x+y=4").unwrap(),
        SystemSolutions::None
    );
    assert_eq!(
        solve_linear_system("0=0", "x=1").unwrap(),
        SystemSolutions::Infinite
    );
    assert_eq!(
        solve_linear_system("0=1", "0=0").unwrap(),
        SystemSolutions::None
    );
    assert_eq!(
        solve_linear_system("0=0", "0=0").unwrap(),
        SystemSolutions::Infinite
    );
    assert!(solve_linear_system("x^2=1", "y=0").is_err());
    assert_eq!(calculate("x+y=3;x-y=1").unwrap(), "x = 2, y = 1");
}
#[test]
fn differentiation_and_integration() {
    close(derivative("x^3", 2.0).unwrap(), 12.0);
    close(derivative("sin(x)", 0.0).unwrap(), 1.0);
    assert!(derivative("abs(x)", 0.0).is_err());
    assert!(derivative("1/x", 0.0).is_err());
    close(integrate("x^2", 0.0, 1.0, options()).unwrap(), 1.0 / 3.0);
    close(
        integrate("sin(x)", 0.0, std::f64::consts::PI, options()).unwrap(),
        2.0,
    );
    close(integrate("x", 1.0, 0.0, options()).unwrap(), -0.5);
    assert!(integrate("1/x", -1.0, 1.0, options()).is_err());
    assert!(integrate("1/(x-0.12345)^2", 0.0, 1.0, options()).is_err());
    assert!(integrate("sqrt(x)", -1.0, 1.0, options()).is_err());
}
#[test]
fn discontinuities_are_not_connected() {
    for expression in ["1/x", "1/(x-0.12345)", "tan(x)"] {
        let c = sample(expression, -2.0, 2.0, options()).unwrap();
        assert!(!c.complete);
        assert!(c.skipped_intervals > 0, "{expression}");
        let pole = if expression == "1/x" {
            0.0
        } else if expression == "tan(x)" {
            std::f64::consts::FRAC_PI_2
        } else {
            0.12345
        };
        for s in c.segments {
            assert!(
                !(s.first().unwrap().x < pole && s.last().unwrap().x > pole),
                "{expression}"
            );
        }
    }
    assert_eq!(sample("x", -2.0, 2.0, options()).unwrap().segments.len(), 1);
    assert!(
        !sample("sqrt(x)", -1.0, 1.0, options())
            .unwrap()
            .segments
            .is_empty()
    );
}
#[test]
fn numerical_roots_and_intersections() {
    let r = roots("x^2-2", -2.0, 2.0, options()).unwrap();
    assert!(!r.complete);
    assert_eq!(r.roots.len(), 2);
    close(r.roots[0], -2.0_f64.sqrt());
    close(r.roots[1], 2.0_f64.sqrt());
    let r = roots("(x-0.12345)^2", -1.0, 1.0, options()).unwrap();
    assert_eq!(r.roots.len(), 1);
    close(r.roots[0], 0.12345);
    assert!(roots("1/x", -1.0, 1.0, options()).unwrap().roots.is_empty());
    assert!(
        roots("1/(x-0.12345)", -1.0, 1.0, options())
            .unwrap()
            .roots
            .is_empty()
    );
    assert!(
        roots("x^2+1", -1.0, 1.0, options())
            .unwrap()
            .roots
            .is_empty()
    );
    assert!(roots("0", -1.0, 1.0, options()).is_err());
    assert!(roots("y", -1.0, 1.0, options()).is_err());
    let p = curve_intersections("x^2", "1", -2.0, 2.0, options()).unwrap();
    assert_eq!(p.len(), 2);
    close(p[0].x, -1.0);
    close(p[1].y, 1.0);
}
#[test]
fn quadratic_system_search_is_bounded_and_incomplete() {
    let b = Bounds2D {
        x_min: -2.0,
        x_max: 2.0,
        y_min: -2.0,
        y_max: 2.0,
    };
    let o = NumericOptions {
        steps: 64,
        max_evaluations: 100000,
        ..options()
    };
    let s = solve_quadratic_system("x^2+y^2=1", "y=x", b, o).unwrap();
    assert!(!s.complete);
    assert_eq!(s.points.len(), 2);
    for p in s.points {
        close(p.x, p.y);
        close(p.x * p.x + p.y * p.y, 1.0);
    }
    let s = solve_quadratic_system("x^2+y^2=1", "(x-1)^2+y^2=1", b, o).unwrap();
    assert_eq!(s.points.len(), 2);
    for p in s.points {
        close(p.x, 0.5);
        close(p.y.abs(), 3.0_f64.sqrt() / 2.0);
    }
    let s = solve_quadratic_system("x^2=1", "y^2=1", b, o).unwrap();
    assert_eq!(s.points.len(), 4);
    assert!(solve_quadratic_system("x^2+y^2=1", "x^2+y^2=1", b, o).is_err());
    assert!(solve_quadratic_system("x^3=1", "y=0", b, o).is_err());
    let s = solve_quadratic_system("x=1", "y=0", b, o).unwrap();
    assert!(s.complete);
    assert_eq!(s.points, vec![Point { x: 1.0, y: 0.0 }]);
}
