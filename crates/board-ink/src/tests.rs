use super::*;

// #region debug-point C
fn report_synthetic_bench(data: &str) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let run = serde_json::to_string(
        &std::env::var("NEO_DRAW_DEBUG_RUN").unwrap_or_else(|_| "pre-fix".into()),
    )
    .unwrap();
    let body = format!(
        r#"{{"sessionId":"fullscreen-ink-performance","runId":{run},"hypothesisId":"C","location":"board-ink/src/tests.rs","msg":"[DEBUG] synthetic ink benchmark","ts":{ts},"data":{data}}}"#
    );
    eprintln!("{body}");
}
// #endregion

// #region debug-point C: synthetic-only ignored benchmarks
fn synthetic_curve(kind: &str, t: f64) -> Point {
    let angle = match kind {
        "arc" | "uneven_arc" => PI * t,
        _ => 2.0 * PI * t,
    };
    match kind {
        "arc" | "circle" | "uneven_arc" => {
            p((100.0 * angle.cos()) as f32, (100.0 * angle.sin()) as f32)
        }
        "curve" => p((200.0 * t) as f32, (40.0 * (2.0 * PI * t).sin()) as f32),
        "corner90" if t <= 0.5 => p((200.0 * t) as f32, 0.0),
        "corner90" => p(100.0, (200.0 * (t - 0.5)) as f32),
        _ => p((200.0 * t) as f32, 0.0),
    }
}

fn synthetic_input(kind: &str, count: usize) -> Vec<StrokePoint> {
    let mut input = Vec::new();
    for i in 0..count {
        let time = i as f64 / (count - 1) as f64;
        let t = if kind == "uneven_arc" {
            time * time
        } else {
            time
        };
        let mut point = synthetic_curve(kind, t);
        if kind == "jitter" && i > 0 && i + 1 < count {
            point.y += if i % 2 == 0 { 1.5 } else { -1.5 };
        }
        input.push(StrokePoint {
            x: point.x,
            y: point.y,
            time,
            pressure: (0.2 + 0.8 * time) as f32,
        });
        if kind == "duplicates" {
            input.push(*input.last().unwrap());
        }
    }
    input
}

fn polyline_distance(point: Point, line: &[StrokePoint]) -> f64 {
    line.windows(2)
        .map(|pair| project(point, position(&pair[0]), position(&pair[1]), true).distance as f64)
        .fold(f64::INFINITY, f64::min)
}

#[test]
#[ignore = "synthetic quality evidence; reports to stderr"]
fn bench_low_sampling_quality() {
    for kind in [
        "arc",
        "circle",
        "curve",
        "line",
        "uneven_arc",
        "jitter",
        "duplicates",
        "corner90",
    ] {
        let counts: &[usize] = if kind == "corner90" {
            &[3, 9, 17, 33]
        } else {
            &[8, 16, 32]
        };
        for &count in counts {
            let input = synthetic_input(kind, count);
            for profile in ["linear", "live", "commit", "commit_render"] {
                let output = match profile {
                    "linear" => smooth_resample(&input, 2.0, 0.0, 75.0).unwrap(),
                    "live" => smooth_resample(&input, 2.0, 0.65, 75.0).unwrap(),
                    "commit" => smooth_resample(&input, 2.0, 0.45, 38.0).unwrap(),
                    _ => smooth_resample(
                        &smooth_resample(&input, 2.0, 0.45, 38.0).unwrap(),
                        2.0,
                        0.65,
                        75.0,
                    )
                    .unwrap(),
                };
                let mut squared = 0.0;
                let mut maximum = 0.0_f64;
                // Uniform reference parameters avoid making the RMS depend on output density.
                for i in 0..=2048 {
                    let error =
                        polyline_distance(synthetic_curve(kind, i as f64 / 2048.0), &output);
                    squared += error * error;
                    maximum = maximum.max(error);
                }
                let bounds = input.iter().fold(
                    [
                        f32::INFINITY,
                        f32::NEG_INFINITY,
                        f32::INFINITY,
                        f32::NEG_INFINITY,
                    ],
                    |b, q| [b[0].min(q.x), b[1].max(q.x), b[2].min(q.y), b[3].max(q.y)],
                );
                let mut bbox_overshoot = 0.0_f64;
                let mut off_input_polyline = 0.0_f64;
                let mut radius_squared = 0.0;
                let mut radius_max = 0.0_f64;
                let mut outward_radius = 0.0_f64;
                let mut probes = 0;
                for pair in output.windows(2) {
                    for t in [0.0, 0.5, 1.0] {
                        let q = lerp(position(&pair[0]), position(&pair[1]), t);
                        bbox_overshoot = bbox_overshoot.max(
                            (bounds[0] - q.x)
                                .max(q.x - bounds[1])
                                .max(bounds[2] - q.y)
                                .max(q.y - bounds[3])
                                .max(0.0) as f64,
                        );
                        off_input_polyline = off_input_polyline.max(polyline_distance(q, &input));
                        let radial = distance(q, p(0.0, 0.0)) - 100.0;
                        radius_squared += radial * radial;
                        radius_max = radius_max.max(radial.abs());
                        outward_radius = outward_radius.max(radial);
                        probes += 1;
                    }
                }
                let corner_deviation = if kind == "corner90" {
                    polyline_distance(p(100.0, 0.0), &output)
                } else {
                    0.0
                };
                let endpoint_error =
                    distance(position(&input[0]), position(&output[0])).max(distance(
                        position(input.last().unwrap()),
                        position(output.last().unwrap()),
                    ));
                assert_eq!(endpoint_error, 0.0);
                assert!(output.windows(2).all(|q| q[0].time <= q[1].time));
                assert!(output.iter().all(|q| (0.0..=1.0).contains(&q.pressure)));
                if profile == "linear" {
                    assert!(off_input_polyline < 0.0001);
                }
                if kind == "corner90" {
                    assert!(corner_deviation < 0.0001);
                }
                let radial = matches!(kind, "arc" | "circle" | "uneven_arc");
                let radius_rms = if radial {
                    format!("{}", (radius_squared / probes as f64).sqrt())
                } else {
                    "null".into()
                };
                let radius_max = if radial {
                    format!("{radius_max}")
                } else {
                    "null".into()
                };
                report_synthetic_bench(&format!(
                    r#"{{"experiment":"quality","fixture":"{kind}","profile":"{profile}","synthetic":true,"samples":{count},"input_points":{},"nominal_hz":{},"output_points":{},"reference_probes":2049,"rms_error":{},"max_error":{maximum},"radius_rms":{radius_rms},"radius_max":{radius_max},"radial_outward_overshoot":{},"bbox_overshoot":{bbox_overshoot},"endpoint_error":{endpoint_error},"corner_deviation":{corner_deviation},"off_input_polyline_max":{off_input_polyline}}}"#,
                    input.len(),
                    count - 1,
                    output.len(),
                    (squared / 2049.0).sqrt(),
                    if radial { outward_radius } else { 0.0 }
                ));
            }
        }
    }
}

fn bench_distribution(mut action: impl FnMut()) -> (f64, f64, f64) {
    for _ in 0..3 {
        action();
    }
    let mut samples = Vec::with_capacity(31);
    for _ in 0..31 {
        let clock = std::time::Instant::now();
        action();
        samples.push(clock.elapsed().as_secs_f64() * 1e6);
    }
    samples.sort_by(f64::total_cmp);
    (samples[15], samples[29], samples[30])
}

#[test]
#[ignore = "synthetic CPU benchmark; reports to stderr"]
fn bench_long_stroke_cost() {
    use std::hint::black_box;
    for count in [100, 1000, 10_000] {
        for extent in ["fixed", "growing"] {
            let input: Vec<_> = (0..count)
                .map(|i| {
                    let x = if extent == "fixed" {
                        i as f32 * 1000.0 / (count - 1) as f32
                    } else {
                        i as f32
                    };
                    s(x, (x * 0.02).sin() * 30.0, i as f64 / 120.0)
                })
                .collect();
            let output = smooth_resample(&input, 2.0, 0.65, 75.0).unwrap();
            let clean = bench_distribution(|| {
                black_box(clean_stroke(black_box(&input)).unwrap());
            });
            let smooth = bench_distribution(|| {
                black_box(smooth_resample(black_box(&input), 2.0, 0.65, 75.0).unwrap());
            });
            let width = bench_distribution(|| {
                black_box(
                    stroke_widths(black_box(&output), &Style::default(), 1200.0, 0.7).unwrap(),
                );
            });
            let clock = std::time::Instant::now();
            let mut visited = 0_usize;
            let mut emitted = 0_usize;
            // Synthetic 120 Hz samples / 60 Hz refresh; no GUI or actual frame claims.
            for prefix in (2..=count).step_by(2) {
                let points = smooth_resample(black_box(&input[..prefix]), 2.0, 0.65, 75.0).unwrap();
                visited += prefix;
                emitted += points.len();
                black_box(stroke_widths(&points, &Style::default(), 1200.0, 0.7).unwrap());
            }
            let prefixes_us = clock.elapsed().as_secs_f64() * 1e6;
            report_synthetic_bench(&format!(
                r#"{{"experiment":"long_cost","synthetic":true,"extent":"{extent}","input_points":{count},"output_points":{},"repetitions":31,"clean_us_p50":{},"clean_us_p95":{},"clean_us_max":{},"smooth_total_us_p50":{},"smooth_total_us_p95":{},"smooth_total_us_max":{},"width_us_p50":{},"width_us_p95":{},"width_us_max":{},"prefix_calls":{},"prefix_input_visited":{visited},"prefix_output_emitted":{emitted},"prefix_smooth_width_total_us":{prefixes_us},"mesh_included":false}}"#,
                output.len(),
                clean.0,
                clean.1,
                clean.2,
                smooth.0,
                smooth.1,
                smooth.2,
                width.0,
                width.1,
                width.2,
                count / 2
            ));
        }
    }
}

#[test]
#[ignore = "synthetic metadata and budget evidence; reports to stderr"]
fn bench_metadata_budget_and_source_changes() {
    let input = [
        StrokePoint {
            x: 0.0,
            y: 0.0,
            time: 3.0,
            pressure: -0.5,
        },
        StrokePoint {
            x: 0.0,
            y: 0.0,
            time: 1.0,
            pressure: 0.4,
        },
        StrokePoint {
            x: 10.0,
            y: 0.0,
            time: 2.0,
            pressure: 1.5,
        },
        StrokePoint {
            x: 20.0,
            y: 0.0,
            time: 4.0,
            pressure: 0.8,
        },
    ];
    let clean = clean_stroke(&input).unwrap();
    let output = smooth_resample(&input, 2.0, 0.65, 75.0).unwrap();
    assert_eq!(clean.len(), 3);
    assert_eq!(clean[0].time, 3.0);
    assert_eq!(clean[0].pressure, 0.4);
    assert!(output.windows(2).all(|q| q[0].time <= q[1].time));
    assert!(output.iter().all(|q| (0.0..=1.0).contains(&q.pressure)));
    let duplicate_endpoint_metadata_changed = output.first() != input.first();
    let base = synthetic_input("arc", 16);
    let original = smooth_resample(&base, 2.0, 0.65, 75.0).unwrap();
    let mut changes = Vec::new();
    for field in ["position", "time", "pressure"] {
        let mut changed = base.clone();
        match field {
            "position" => changed[8].y += 3.0,
            "time" => changed[8].time += 0.01,
            _ => changed[8].pressure = 0.1,
        }
        changes.push(smooth_resample(&changed, 2.0, 0.65, 75.0).unwrap() != original);
    }
    assert!(changes.iter().all(|changed| *changed));
    let huge = [s(0.0, 0.0, 0.0), s(1_000_000.0, 0.0, 1.0)];
    let rejected = smooth_resample(&huge, 2.0, 0.65, 75.0) == Err(InkError::TooManyPoints);
    assert!(rejected);
    let midpoint = interpolate(
        &s(0.0, 0.0, 0.0),
        &StrokePoint {
            pressure: 0.0,
            ..s(10.0, 0.0, 2.0)
        },
        0.5,
    );
    assert_eq!(midpoint.time, 1.0);
    assert_eq!(midpoint.pressure, 0.5);
    report_synthetic_bench(&format!(
        r#"{{"experiment":"metadata_budget_source","synthetic":true,"input_points":4,"clean_points":{},"output_points":{},"duplicate_endpoint_metadata_changed":{duplicate_endpoint_metadata_changed},"time_monotonic":true,"pressure_clamped":true,"time_pressure_linear_interpolation":true,"same_length_position_change_visible":{},"same_length_time_change_visible":{},"same_length_pressure_change_visible":{},"point_budget":{MAX_POINTS},"oversized_resample_rejected":{rejected}}}"#,
        clean.len(),
        output.len(),
        changes[0],
        changes[1],
        changes[2]
    ));
}
// #endregion

fn p(x: f32, y: f32) -> Point {
    Point { x, y }
}
fn s(x: f32, y: f32, time: f64) -> StrokePoint {
    StrokePoint {
        x,
        y,
        time,
        pressure: 1.0,
    }
}
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-4, "{a} != {b}");
}

#[test]
fn duplicate_points_and_reversed_time_are_finite() {
    let input = [
        s(0.0, 0.0, 3.0),
        s(0.0, 0.0, 1.0),
        s(5.0, 0.0, 2.0),
        s(10.0, 0.0, 4.0),
    ];
    let output = smooth_resample(&input, 1.0, 0.5, 45.0).unwrap();
    assert!(output.windows(2).all(|p| p[0].time <= p[1].time));
    assert!(
        output
            .iter()
            .all(|p| p.x.is_finite() && p.y.is_finite() && p.time.is_finite())
    );
    let widths = stroke_widths(&input, &Style::default(), 100.0, 0.5).unwrap();
    assert_eq!(widths.len(), input.len());
    assert!(widths.iter().all(|w| w.is_finite() && *w > 0.0));
    assert_eq!(
        smooth_resample(&input[..2], 1.0, 1.0, 45.0).unwrap().len(),
        1
    );
}

#[test]
fn low_sample_corner_survives_smoothing() {
    let input = [s(0.0, 0.0, 0.0), s(10.0, 0.0, 0.1), s(10.0, 10.0, 0.2)];
    let output = smooth_resample(&input, 2.0, 1.0, 45.0).unwrap();
    assert!(output.iter().any(|p| p.x == 10.0 && p.y == 0.0));
    assert!(output.iter().all(|p| p.y == 0.0 || p.x == 10.0));
    assert!(
        output
            .windows(2)
            .all(|p| distance(position(&p[0]), position(&p[1])) <= 2.0001)
    );
    assert_eq!(output.first(), input.first());
    assert_eq!(output.last(), input.last());
}

#[test]
fn shallow_turn_is_interpolated_without_moving_samples() {
    let input = [s(0.0, 0.0, 0.0), s(5.0, 1.0, 1.0), s(10.0, 0.0, 2.0)];
    let output = smooth_resample(&input, 0.5, 1.0, 45.0).unwrap();
    assert!(output.contains(&input[1]));
    assert!(output.iter().all(|q| q.y >= 0.0 && q.y <= 1.0));
    assert!(
        output
            .iter()
            .any(|q| polyline_distance(position(q), &input) > 0.1)
    );
}

#[test]
fn sparse_circles_are_curves_not_shrunken_or_linearly_densified() {
    for count in [8, 16, 32] {
        let input = synthetic_input("circle", count);
        let linear = smooth_resample(&input, 2.0, 0.0, 75.0).unwrap();
        let error = |line: &[StrokePoint]| {
            (0..=512)
                .map(|i| polyline_distance(synthetic_curve("circle", i as f64 / 512.0), line))
                .fold(0.0_f64, f64::max)
        };
        let baseline = error(&linear);
        for (strength, corner) in [(0.65, 75.0), (0.45, 38.0), (1.0, 38.0)] {
            let curve = smooth_resample(&input, 2.0, strength, corner).unwrap();
            let maximum = error(&curve);
            eprintln!(
                "circle {count} strength={strength}: linear={baseline:.4}, curve={maximum:.4}"
            );
            assert!(maximum < baseline * 0.4, "{count}: {maximum} >= {baseline}");
            assert_eq!(curve.first(), input.first());
            assert_eq!(curve.last(), input.last());
            assert!(input.iter().all(|q| curve.contains(q)));
            assert!(
                curve
                    .iter()
                    .all(|q| distance(position(q), p(0.0, 0.0)) <= 100.0001)
            );
            assert!(
                curve
                    .windows(2)
                    .all(|w| distance(position(&w[0]), position(&w[1])) <= 2.0001)
            );
            let again = smooth_resample(&curve, 2.0, 0.65, 75.0).unwrap();
            let drift = again
                .iter()
                .map(|q| polyline_distance(position(q), &curve))
                .fold(0.0_f64, f64::max);
            assert!(drift < 0.05, "resampled circle drift {drift}");
        }
    }
}

#[test]
fn corner_threshold_still_controls_isolated_turns_and_strength_zero_is_linear() {
    let input = [s(0.0, 0.0, 0.0), s(10.0, 0.0, 1.0), s(15.0, 8.660254, 2.0)];
    for (strength, threshold) in [(1.0, 38.0), (1.0, 0.0), (0.0, 180.0)] {
        let output = smooth_resample(&input, 0.5, strength, threshold).unwrap();
        assert!(
            output
                .iter()
                .all(|q| polyline_distance(position(q), &input) < 1e-5)
        );
    }
    let output = smooth_resample(&input, 0.5, 1.0, 75.0).unwrap();
    assert!(
        output
            .iter()
            .any(|q| polyline_distance(position(q), &input) > 0.1)
    );
    assert!(output.contains(&input[1]));
}

#[test]
fn uneven_arcs_keep_metadata_and_improve_on_linear() {
    for count in [8, 16, 32] {
        let input = synthetic_input("uneven_arc", count);
        let curve = smooth_resample(&input, 2.0, 0.65, 75.0).unwrap();
        let linear = smooth_resample(&input, 2.0, 0.0, 75.0).unwrap();
        let error = |line: &[StrokePoint]| {
            (0..=512)
                .map(|i| polyline_distance(synthetic_curve("arc", i as f64 / 512.0), line))
                .fold(0.0_f64, f64::max)
        };
        assert!(error(&curve) < error(&linear) * 0.6);
        assert_eq!(curve.first(), input.first());
        assert_eq!(curve.last(), input.last());
        assert!(
            curve
                .windows(2)
                .all(|w| w[0].time <= w[1].time && w[0].pressure <= w[1].pressure)
        );
        for q in &curve {
            assert!((q.pressure as f64 - (0.2 + 0.8 * q.time)).abs() < 1e-6);
        }
        assert!(
            curve
                .iter()
                .all(|q| distance(position(q), p(0.0, 0.0)) <= 100.1)
        );
    }
}

#[test]
fn dense_jitter_is_reduced_but_right_angles_are_not_rounded() {
    let mut input = synthetic_input("jitter", 64);
    for q in &mut input {
        q.y *= 0.3;
    }
    let curve = smooth_resample(&input, 2.0, 0.65, 75.0).unwrap();
    let linear = smooth_resample(&input, 2.0, 0.0, 75.0).unwrap();
    let energy = |line: &[StrokePoint]| {
        line.iter().map(|q| (q.y as f64).powi(2)).sum::<f64>() / line.len() as f64
    };
    assert!(energy(&curve) < energy(&linear) * 0.5);
    for threshold in [0.0, 38.0, 75.0, 180.0] {
        let corner = synthetic_input("corner90", 9);
        let result = smooth_resample(&corner, 2.0, 1.0, threshold).unwrap();
        assert!(result.iter().all(|q| q.y == 0.0 || q.x == 100.0));
        assert!(result.contains(&corner[4]));
    }
}

#[test]
fn closed_seam_and_intentional_loop_corner_are_distinct() {
    let circle = synthetic_input("circle", 8);
    let curve = smooth_resample(&circle, 1.0, 1.0, 38.0).unwrap();
    let a = position(&curve[curve.len() - 2]);
    let b = position(&curve[0]);
    let c = position(&curve[1]);
    assert!(curve_turn(a, b, c).abs().to_degrees() < 3.0);
    let square = [
        s(0.0, 0.0, 0.0),
        s(10.0, 0.0, 1.0),
        s(10.0, 10.0, 2.0),
        s(0.0, 10.0, 3.0),
        s(0.0, 0.0, 4.0),
    ];
    let output = smooth_resample(&square, 1.0, 1.0, 75.0).unwrap();
    assert!(
        output
            .iter()
            .all(|q| q.x == 0.0 || q.x == 10.0 || q.y == 0.0 || q.y == 10.0)
    );
    let mut open = circle.clone();
    open.last_mut().unwrap().y = -0.1;
    let output = smooth_resample(&open, 1.0, 1.0, 38.0).unwrap();
    assert_eq!(output.first(), open.first());
    assert_eq!(output.last(), open.last());
}

#[test]
fn extreme_length_ratios_duplicates_and_quantization_stay_bounded() {
    for offset in [0.0, 999_000.0] {
        let input = [
            s(offset, offset, 0.0),
            s(offset + 0.125, offset, 1.0),
            s(offset + 0.125, offset, 1.0),
            s(offset + 200.0, offset + 1.0, 2.0),
            s(offset + 200.125, offset + 1.0, 3.0),
        ];
        let curve = smooth_resample(&input, 0.01, 1.0, 75.0).unwrap();
        assert_eq!(curve.first(), input.first());
        assert_eq!(curve.last(), input.last());
        assert!(
            curve
                .windows(2)
                .all(|w| position(&w[0]) != position(&w[1]) && w[0].time <= w[1].time)
        );
        assert!(curve.iter().all(|q| valid_point(position(q))
            && q.x >= offset
            && q.x <= offset + 200.125
            && q.y >= offset - 0.1
            && q.y <= offset + 1.1));
    }
    let path: Vec<_> = (0..10_000)
        .map(|i| s(i as f32, (i as f32 * 0.01).sin() * 10.0, i as f64))
        .collect();
    let curve = smooth_resample(&path, 2.0, 0.65, 75.0).unwrap();
    assert!(curve.len() <= 2 * path.len());
    assert_eq!(curve.first(), path.first());
    assert_eq!(curve.last(), path.last());
    assert_eq!(
        smooth_resample(&path, 1e-10, 1.0, 75.0),
        Err(InkError::TooManyPoints)
    );
}

#[test]
fn faster_and_lighter_strokes_are_thinner() {
    let style = Style {
        width: 8.0,
        ..Style::default()
    };
    let slow = [s(0.0, 0.0, 0.0), s(10.0, 0.0, 1.0)];
    let fast = [s(0.0, 0.0, 0.0), s(10.0, 0.0, 0.01)];
    let slow_width = stroke_widths(&slow, &style, 100.0, 1.0).unwrap()[1];
    let fast_width = stroke_widths(&fast, &style, 100.0, 1.0).unwrap()[1];
    assert!(slow_width > fast_width);
    let mut light = slow;
    light[1].pressure = 0.1;
    assert!(stroke_widths(&light, &style, 100.0, 1.0).unwrap()[1] < slow_width);
}

#[test]
fn distance_handles_lines_segments_and_degenerate_points() {
    close(
        point_line_distance(p(20.0, 3.0), p(0.0, 0.0), p(10.0, 0.0)).unwrap(),
        3.0,
    );
    close(
        point_segment_distance(p(20.0, 0.0), p(0.0, 0.0), p(10.0, 0.0)).unwrap(),
        10.0,
    );
    close(
        point_segment_distance(p(3.0, 4.0), p(0.0, 0.0), p(0.0, 0.0)).unwrap(),
        5.0,
    );
    let projection = project_to_segment(p(4.0, 3.0), p(0.0, 0.0), p(10.0, 0.0)).unwrap();
    close(projection.t, 0.4);
    assert_eq!(projection.point, p(4.0, 0.0));
    assert_eq!(point_stroke_distance(p(0.0, 0.0), &[]).unwrap(), None);
}

#[test]
fn circle_has_constant_radius_and_closed_edges() {
    let circle = shape_geometry(ShapeKind::Circle, p(0.0, 0.0), p(20.0, 10.0)).unwrap();
    assert_eq!(circle.vertices.len(), 64);
    assert_eq!(circle.edges.len(), 64);
    assert_eq!(circle.edges.last(), Some(&[63, 0]));
    for vertex in circle.vertices {
        close(distance(vertex, p(5.0, 5.0)) as f32, 5.0);
    }
    let triangle =
        shape_geometry(ShapeKind::EquilateralTriangle, p(0.0, 0.0), p(20.0, 10.0)).unwrap();
    let v = triangle.vertices;
    close(distance(v[0], v[1]) as f32, distance(v[1], v[2]) as f32);
}

#[test]
fn every_shape_has_finite_valid_edges_even_when_degenerate() {
    use ShapeKind::*;
    for kind in [
        Line,
        Rectangle,
        Square,
        Triangle,
        RightTriangle,
        EquilateralTriangle,
        Parallelogram,
        Rhombus,
        Ellipse,
        Circle,
        Cube,
        Cuboid,
        Cylinder,
        Cone,
        Sphere,
    ] {
        for end in [p(100.0, 80.0), p(0.0, 0.0)] {
            let g = shape_geometry(kind, p(0.0, 0.0), end).unwrap();
            assert!(!g.edges.is_empty());
            assert!(g.vertices.iter().all(|p| valid_point(*p)));
            assert!(g.edges.iter().flatten().all(|i| *i < g.vertices.len()));
        }
    }
}

#[test]
fn sparse_stroke_is_split_at_intersections_not_deleted_whole() {
    let stroke = [s(0.0, 0.0, 0.0), s(10.0, 0.0, 1.0)];
    let path = [p(5.0, 0.0)];
    assert!(stroke_hit(&stroke, &path, 1.0).unwrap());
    let pieces = erase_stroke(&stroke, &path, 1.0).unwrap();
    assert_eq!(pieces.len(), 2);
    close(pieces[0].last().unwrap().x, 4.0);
    close(pieces[1][0].x, 6.0);
    assert!((pieces[0].last().unwrap().time - 0.4).abs() < 1e-8);
}

#[test]
fn continuous_eraser_crossing_without_endpoint_hits_splits() {
    let stroke = [s(0.0, 0.0, 0.0), s(20.0, 0.0, 2.0)];
    let path = [p(10.0, -10.0), p(10.0, 10.0)];
    let pieces = erase_stroke(&stroke, &path, 2.0).unwrap();
    assert_eq!(pieces.len(), 2);
    close(pieces[0].last().unwrap().x, 8.0);
    close(pieces[1][0].x, 12.0);
    let second: Vec<_> = pieces
        .iter()
        .flat_map(|part| erase_stroke(part, &[p(4.0, 0.0)], 1.0).unwrap())
        .collect();
    assert_eq!(second.len(), 3);
    close(second[0].last().unwrap().x, 3.0);
    close(second[1][0].x, 5.0);
    close(second[2][0].x, 12.0);
}

#[test]
fn overlapping_eraser_capsules_merge_and_repeated_erasing_is_stable() {
    let stroke = [s(0.0, 0.0, 0.0), s(5.0, 0.0, 0.5), s(10.0, 0.0, 1.0)];
    let path = [p(4.0, 0.0), p(6.0, 0.0), p(4.0, 0.0)];
    let pieces = erase_stroke(&stroke, &path, 1.0).unwrap();
    assert_eq!(pieces.len(), 2);
    close(pieces[0].last().unwrap().x, 3.0);
    close(pieces[1][0].x, 7.0);
    let repeated: Vec<_> = pieces
        .iter()
        .flat_map(|part| erase_stroke(part, &path, 1.0).unwrap())
        .collect();
    assert_eq!(pieces, repeated);
    assert!(
        erase_stroke(&stroke, &[p(5.0, 0.0)], 20.0)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn erasing_across_multiple_sample_segments_never_reconnects_gaps() {
    let stroke: Vec<_> = (0..=10).map(|i| s(i as f32, 0.0, i as f64)).collect();
    let pieces = erase_stroke(&stroke, &[p(5.0, 0.0)], 2.5).unwrap();
    assert_eq!(pieces.len(), 2);
    close(pieces[0].last().unwrap().x, 2.5);
    close(pieces[1][0].x, 7.5);
    assert_eq!(
        erase_stroke(&[s(0.0, 0.0, 0.0)], &[p(0.0, 0.0)], 1.0)
            .unwrap()
            .len(),
        0
    );
    assert!(!stroke_hit(&[], &[p(0.0, 0.0)], 1.0).unwrap());
}

#[test]
fn angle_snapping_preserves_length_and_respects_tolerance() {
    let target = p(10.0, 0.2);
    let snapped = snap_angle(p(0.0, 0.0), target, 3.0).unwrap();
    close(snapped.y, 0.0);
    close(
        distance(p(0.0, 0.0), snapped) as f32,
        distance(p(0.0, 0.0), target) as f32,
    );
    assert_eq!(
        snap_angle(p(0.0, 0.0), p(10.0, 2.0), 1.0).unwrap(),
        p(10.0, 2.0)
    );
}

#[test]
fn endpoint_priority_projection_and_vertex_edit() {
    let mut rectangle = shape_geometry(ShapeKind::Rectangle, p(0.0, 0.0), p(10.0, 10.0)).unwrap();
    let endpoint = nearest_connection(&rectangle, p(0.1, 0.1), 0.5, None)
        .unwrap()
        .unwrap();
    assert_eq!(endpoint.site, ConnectionSite::Vertex(0));
    let edge = nearest_connection(&rectangle, p(5.0, 0.1), 0.5, None)
        .unwrap()
        .unwrap();
    assert_eq!(edge.site, ConnectionSite::Edge(0));
    close(edge.t, 0.5);
    edit_vertex(&mut rectangle, 0, p(1.0, 1.0)).unwrap();
    assert_eq!(rectangle.vertices[0], p(1.0, 1.0));
    let mut circle = shape_geometry(ShapeKind::Circle, p(0.0, 0.0), p(10.0, 10.0)).unwrap();
    assert_eq!(
        edit_vertex(&mut circle, 0, p(1.0, 1.0)),
        Err(InkError::UnsupportedVertexEdit)
    );
}

#[test]
fn connections_isolate_2d_and_3d_even_through_lines() {
    use ShapeKind::*;
    assert!(connections_compatible(&[Line, Line, Cube, Sphere]));
    assert!(connections_compatible(&[Rectangle, Line, Circle]));
    assert!(!connections_compatible(&[Rectangle, Line, Line, Cube]));
    let cube = shape_geometry(Cube, p(0.0, 0.0), p(10.0, 10.0)).unwrap();
    assert!(
        nearest_connection(&cube, cube.vertices[0], 1.0, Some(Dimension::TwoD))
            .unwrap()
            .is_none()
    );
    assert!(
        nearest_connection(&cube, cube.vertices[0], 1.0, Some(Dimension::ThreeD))
            .unwrap()
            .is_some()
    );
}

#[test]
fn invalid_inputs_and_excessive_resampling_are_rejected() {
    assert_eq!(
        shape_geometry(ShapeKind::Circle, p(f32::NAN, 0.0), p(0.0, 0.0)).unwrap_err(),
        InkError::InvalidInput
    );
    let stroke = [s(0.0, 0.0, 0.0), s(10.0, 0.0, 1.0)];
    assert_eq!(
        smooth_resample(&stroke, 0.0, 1.0, 45.0).unwrap_err(),
        InkError::InvalidInput
    );
    assert_eq!(
        smooth_resample(&stroke, 1e-8, 1.0, 45.0).unwrap_err(),
        InkError::TooManyPoints
    );
    assert_eq!(
        erase_stroke(&stroke, &[p(0.0, 0.0)], f32::INFINITY).unwrap_err(),
        InkError::InvalidInput
    );
    let mut bad = stroke;
    bad[0].pressure = f32::NAN;
    assert_eq!(
        stroke_widths(&bad, &Style::default(), 100.0, 1.0).unwrap_err(),
        InkError::InvalidInput
    );
    assert!(smooth_resample(&[], 1.0, 0.5, 45.0).unwrap().is_empty());
}

// 固定种子保证随机回归可复现，不引入额外依赖。
fn random(seed: &mut u64) -> f32 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    (*seed >> 40) as f32 / (1_u32 << 24) as f32
}

fn erase_again(pieces: &[Vec<StrokePoint>], path: &[Point], radius: f32) -> Vec<Vec<StrokePoint>> {
    pieces
        .iter()
        .flat_map(|piece| erase_stroke(piece, path, radius).unwrap())
        .collect()
}

#[test]
fn tiny_strokes_and_erasers_do_not_collapse() {
    let stroke = [s(0.0, 0.0, 0.0), s(1e-10, 0.0, 1.0)];
    let smooth = smooth_resample(&stroke, 1e-11, 0.5, 45.0).unwrap();
    assert_eq!(smooth.first(), stroke.first());
    assert_eq!(smooth.last(), stroke.last());
    assert!(smooth.len() >= 11);
    let projection = project_to_segment(p(5e-11, 2e-11), p(0.0, 0.0), p(1e-10, 0.0)).unwrap();
    close(projection.t, 0.5);
    let pieces = erase_stroke(&stroke, &[p(5e-11, 0.0)], 1e-11).unwrap();
    assert_eq!(pieces.len(), 2);
    assert_eq!(pieces, erase_again(&pieces, &[p(5e-11, 0.0)], 1e-11));
}

#[test]
fn tiny_disk_on_long_segment_is_not_lost_to_cancellation() {
    let stroke = [s(-1_000_000.0, 0.0, 0.0), s(1_000_000.0, 0.0, 2.0)];
    let pieces = erase_stroke(&stroke, &[p(0.0, 0.0)], 0.0001).unwrap();
    assert_eq!(pieces.len(), 2);
    assert!((pieces[0].last().unwrap().x + 0.0001).abs() < 1e-7);
    assert!((pieces[1][0].x - 0.0001).abs() < 1e-7);
    assert!(!stroke_hit(&stroke, &[p(0.0, 0.001)], 0.0001).unwrap());
    assert_eq!(pieces, erase_again(&pieces, &[p(0.0, 0.0)], 0.0001));
}

#[test]
fn random_eraser_results_are_exactly_idempotent() {
    let mut seed = 0x20261002;
    for case in 0..5000 {
        let offset = match case % 3 {
            0 => 900_000.0,
            1 => -900_000.0,
            _ => 0.0,
        };
        let stroke: Vec<_> = (0..12)
            .map(|i| {
                let mut point = s(
                    offset + i as f32 * 8.0,
                    offset + random(&mut seed) * 80.0,
                    i as f64 / 120.0,
                );
                point.pressure = random(&mut seed);
                point
            })
            .collect();
        let path: Vec<_> = (0..3)
            .map(|_| {
                p(
                    offset + random(&mut seed) * 90.0,
                    offset + random(&mut seed) * 80.0,
                )
            })
            .collect();
        let radius = 0.1 + random(&mut seed) * 15.0;
        let pieces = erase_stroke(&stroke, &path, radius).unwrap();
        assert_eq!(
            pieces,
            erase_again(&pieces, &path, radius),
            "随机擦除 case={case}"
        );
        for piece in &pieces {
            assert!(piece.windows(2).all(|pair| pair[0].time <= pair[1].time));
            assert!(
                piece
                    .iter()
                    .all(|point| valid_point(position(point))
                        && (0.0..=1.0).contains(&point.pressure))
            );
        }
    }
}

#[test]
fn random_smoothing_preserves_endpoints_and_seconds() {
    let mut seed = 49;
    for case in 0..200 {
        let offset = if case % 2 == 0 { 999_000.0 } else { 0.0 };
        let stroke: Vec<_> = (0..100)
            .map(|i| {
                s(
                    offset + i as f32 * 2.0,
                    offset + random(&mut seed) * 10.0,
                    1700.0 + i as f64 / 120.0,
                )
            })
            .collect();
        let smooth = smooth_resample(&stroke, 2.0, 0.45, 38.0).unwrap();
        assert_eq!(smooth.first(), stroke.first());
        assert_eq!(smooth.last(), stroke.last());
        assert!(smooth.windows(2).all(|pair| pair[0].time <= pair[1].time));
        // 大坐标处两个坐标分量量化为 f32，各允许半个 ULP 的误差。
        assert!(
            smooth
                .windows(2)
                .all(|pair| distance(position(&pair[0]), position(&pair[1])) <= 2.09)
        );
    }
    let stroke = [s(0.0, 0.0, 1799.0), s(120.0, 0.0, 1800.0)];
    let style = Style {
        width: 8.0,
        ..Style::default()
    };
    let widths = stroke_widths(&stroke, &style, 120.0, 0.0).unwrap();
    close(widths[1], 5.0);
    let smooth = smooth_resample(&stroke, 1.0, 0.5, 45.0).unwrap();
    assert!((smooth[60].time - 1799.5).abs() < 1e-10);
}

#[test]
fn constrained_shapes_stay_anchored_when_dragging_backwards() {
    for kind in [
        ShapeKind::Square,
        ShapeKind::Circle,
        ShapeKind::Cube,
        ShapeKind::Sphere,
        ShapeKind::EquilateralTriangle,
    ] {
        for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
            let start = p(50.0, 50.0);
            let geometry =
                shape_geometry(kind, start, p(50.0 + sx * 40.0, 50.0 + sy * 20.0)).unwrap();
            let left = geometry
                .vertices
                .iter()
                .map(|v| v.x)
                .fold(f32::INFINITY, f32::min);
            let right = geometry
                .vertices
                .iter()
                .map(|v| v.x)
                .fold(f32::NEG_INFINITY, f32::max);
            let top = geometry
                .vertices
                .iter()
                .map(|v| v.y)
                .fold(f32::INFINITY, f32::min);
            let bottom = geometry
                .vertices
                .iter()
                .map(|v| v.y)
                .fold(f32::NEG_INFINITY, f32::max);
            close(if sx > 0.0 { left } else { right }, start.x);
            close(if sy > 0.0 { top } else { bottom }, start.y);
            if kind == ShapeKind::Circle {
                let center = p((left + right) / 2.0, (top + bottom) / 2.0);
                for vertex in &geometry.vertices {
                    close(distance(center, *vertex) as f32, 10.0);
                    let anchor = nearest_connection(&geometry, *vertex, 0.0, None)
                        .unwrap()
                        .unwrap();
                    assert_eq!(anchor.point, *vertex);
                    close(distance(center, anchor.point) as f32, 10.0);
                }
            }
        }
    }
}

#[test]
fn common_shapes_remain_upright_for_all_drag_directions() {
    use ShapeKind::*;
    for kind in [
        Rectangle,
        Triangle,
        RightTriangle,
        Parallelogram,
        Rhombus,
        Ellipse,
        Cuboid,
        Cylinder,
        Cone,
    ] {
        let reference = shape_geometry(kind, p(0.0, 0.0), p(40.0, 20.0)).unwrap();
        for (start, end) in [
            (p(40.0, 20.0), p(0.0, 0.0)),
            (p(40.0, 0.0), p(0.0, 20.0)),
            (p(0.0, 20.0), p(40.0, 0.0)),
        ] {
            let geometry = shape_geometry(kind, start, end).unwrap();
            assert_eq!(geometry.vertices, reference.vertices);
            assert_eq!(geometry.edges, reference.edges);
        }
    }
}

#[test]
fn dashed_style_does_not_change_widths_or_reconnect_erased_pieces() {
    let stroke = [s(0.0, 0.0, 0.0), s(100.0, 0.0, 1.0)];
    let solid = Style::default();
    let dashed = Style {
        dashed: true,
        ..solid
    };
    assert_eq!(
        stroke_widths(&stroke, &solid, 100.0, 0.5),
        stroke_widths(&stroke, &dashed, 100.0, 0.5)
    );
    // 本层擦除连续中心线；虚线的空白与相位由渲染层处理。
    let path = [p(50.0, 0.0)];
    let pieces = erase_stroke(&stroke, &path, 5.0 + dashed.width / 2.0).unwrap();
    assert_eq!(pieces.len(), 2);
    let smooth: Vec<_> = pieces
        .iter()
        .map(|piece| smooth_resample(piece, 2.0, 0.65, 75.0).unwrap())
        .collect();
    assert!(smooth[0].last().unwrap().x < smooth[1][0].x);
    assert_eq!(
        smooth,
        erase_again(&smooth, &path, 5.0 + dashed.width / 2.0)
    );
}

#[test]
fn limits_reject_excessive_work_without_truncating_valid_strokes() {
    let stroke: Vec<_> = (0..MAX_POINTS)
        .map(|i| s(i as f32, 0.0, i as f64 / 120.0))
        .collect();
    assert_eq!(smooth_resample(&stroke, 1.0, 0.0, 45.0).unwrap(), stroke);
    assert_eq!(
        erase_stroke(&stroke, &[p(0.0, 100.0)], 1.0).unwrap(),
        vec![stroke.clone()]
    );
    assert_eq!(
        erase_stroke(&stroke, &[p(0.0, 100.0); 21], 1.0),
        Err(InkError::TooManyPoints)
    );
    let mut excess = stroke;
    excess.push(s(100_000.0, 0.0, 1000.0));
    assert_eq!(
        smooth_resample(&excess, 1.0, 0.0, 45.0),
        Err(InkError::TooManyPoints)
    );
    for spacing in [f32::NAN, f32::INFINITY, -1.0] {
        assert_eq!(
            smooth_resample(&[], spacing, 0.0, 45.0),
            Err(InkError::InvalidInput)
        );
    }
}

#[test]
fn random_shapes_and_extreme_bounds_remain_valid() {
    use ShapeKind::*;
    let mut seed = 849;
    for kind in [
        Line,
        Rectangle,
        Square,
        Triangle,
        RightTriangle,
        EquilateralTriangle,
        Parallelogram,
        Rhombus,
        Ellipse,
        Circle,
        Cube,
        Cuboid,
        Cylinder,
        Cone,
        Sphere,
    ] {
        for case in 0..200 {
            let (start, end) = match case {
                0 => (p(-MAX_COORD, -MAX_COORD), p(MAX_COORD, MAX_COORD)),
                1 => (p(MAX_COORD, MAX_COORD), p(-MAX_COORD, -MAX_COORD)),
                2 => (p(0.0, 0.0), p(1e-20, 2e-20)),
                _ => (
                    p(
                        (random(&mut seed) - 0.5) * 2.0 * MAX_COORD,
                        (random(&mut seed) - 0.5) * 2.0 * MAX_COORD,
                    ),
                    p(
                        (random(&mut seed) - 0.5) * 2.0 * MAX_COORD,
                        (random(&mut seed) - 0.5) * 2.0 * MAX_COORD,
                    ),
                ),
            };
            let geometry = shape_geometry(kind, start, end).unwrap();
            assert!(geometry.vertices.iter().all(|v| valid_point(*v)));
            assert!(
                geometry
                    .edges
                    .iter()
                    .flatten()
                    .all(|i| *i < geometry.vertices.len())
            );
            for vertex in &geometry.vertices {
                assert!(vertex.x >= start.x.min(end.x) && vertex.x <= start.x.max(end.x));
                assert!(vertex.y >= start.y.min(end.y) && vertex.y <= start.y.max(end.y));
            }
        }
    }
}

#[test]
fn eraser_boundary_keeps_outgoing_edges_but_removes_interior_and_dots() {
    let path = [p(0.0, 0.0)];
    let outgoing = [s(1.0, 0.0, 0.0), s(2.0, 0.0, 1.0)];
    assert_eq!(
        erase_stroke(&outgoing, &path, 1.0).unwrap(),
        vec![outgoing.to_vec()]
    );
    assert!(!stroke_hit(&outgoing, &path, 1.0).unwrap());
    assert!(erase_stroke(&outgoing[..1], &path, 1.0).unwrap().is_empty());
    let large = [s(900_000.0, 900_000.0, 0.0), s(900_001.0, 900_000.0, 1.0)];
    assert!(
        erase_stroke(
            &large,
            &[p(900_000.0, 900_000.0), p(900_001.0, 900_000.0)],
            0.001
        )
        .unwrap()
        .is_empty()
    );
    let diameter = [s(-1.0, 0.0, 0.0), s(1.0, 0.0, 1.0)];
    assert!(erase_stroke(&diameter, &path, 1.0).unwrap().is_empty());
    let tangent = [s(-2.0, 1.0, 0.0), s(2.0, 1.0, 1.0)];
    assert!(!stroke_hit(&tangent, &path, 1.0).unwrap());
    assert_eq!(
        erase_stroke(&tangent, &path, 1.0).unwrap(),
        vec![tangent.to_vec()]
    );
}

#[test]
fn thirty_minute_sampling_workload() {
    let clock = std::time::Instant::now();
    let mut sample_count = 0;
    let mut output_count = 0;
    // 模拟 30 分钟、120 Hz，每秒一笔；保留全会话数据，随后擦除整页。
    let mut strokes = Vec::with_capacity(1800);
    for second in 0..1800 {
        let stroke: Vec<_> = (0..120)
            .map(|i| {
                s(
                    (second % 30) as f32 * 40.0 + i as f32 / 4.0,
                    (second / 30) as f32 * 12.0 + (i as f32 * 0.1).sin() * 4.0,
                    second as f64 + i as f64 / 120.0,
                )
            })
            .collect();
        sample_count += stroke.len();
        // 60 帧/秒的当前笔迹预览，模拟调用方反复处理增长中的采样数组。
        for visible in (2..=stroke.len()).step_by(2) {
            let preview = smooth_resample(&stroke[..visible], 2.0, 0.65, 75.0).unwrap();
            assert_eq!(
                stroke_widths(&preview, &Style::default(), 100.0, 0.5)
                    .unwrap()
                    .len(),
                preview.len()
            );
        }
        let smooth = smooth_resample(&stroke, 2.0, 0.45, 38.0).unwrap();
        let widths = stroke_widths(&smooth, &Style::default(), 100.0, 0.5).unwrap();
        assert_eq!(widths.len(), smooth.len());
        output_count += smooth.len();
        strokes.push(smooth);
    }
    let drawing_time = clock.elapsed();
    let path = [p(600.0, -10.0), p(600.0, 740.0)];
    let mut hit_count = 0;
    for stroke in &strokes {
        if stroke_hit(stroke, &path, 8.0).unwrap() {
            hit_count += 1;
            let pieces = erase_stroke(stroke, &path, 8.0).unwrap();
            assert_eq!(pieces, erase_again(&pieces, &path, 8.0));
        }
    }
    assert_eq!(sample_count, 216_000);
    assert!(hit_count > 0);
    eprintln!(
        "30 分钟模拟：输入 {sample_count} 点，输出 {output_count} 点，平滑/笔宽 {drawing_time:?}，含整页擦除 {:?}",
        clock.elapsed()
    );
}
