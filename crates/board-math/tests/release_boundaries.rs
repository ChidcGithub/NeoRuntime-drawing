use board_math::{MathError, NumericOptions, integrate};

#[test]
fn nonzero_integral_underflow_is_not_reported_as_exact_zero() {
    for (a, b) in [(0.0, 1e-30), (1e-30, 0.0)] {
        let result = integrate("1e-300", a, b, NumericOptions::default());
        assert!(
            matches!(result, Err(MathError::Numerical(_))),
            "{a}..{b}: {result:?}"
        );
    }
}

#[test]
fn small_representable_integrals_and_true_zero_remain_supported() {
    let options = NumericOptions::default();
    for (a, b, expected) in [(0.0, 1e-10, 1e-310), (1e-10, 0.0, -1e-310)] {
        let value = integrate("1e-300", a, b, options).unwrap();
        assert!((value / expected - 1.0).abs() < 1e-10, "{value}");
    }
    assert_eq!(integrate("0", 0.0, 1e-30, options).unwrap(), 0.0);
    assert_eq!(integrate("x", -1.0, 1.0, options).unwrap(), 0.0);
    assert_eq!(integrate("1e-300", 0.0, 0.0, options).unwrap(), 0.0);
}
