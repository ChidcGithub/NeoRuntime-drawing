//! Authored mathematical centerlines first, then an explicitly supplied local font.
//! Neither source is a user example. Generated samples never enter the saved profile.
use crate::handwriting::MATH_AXIS;
use board_core::{HandwritingStroke, StrokePoint, Style};

// Synthesis metrics, deliberately separate from recognition's normalized shapes.
const X_HEIGHT: f32 = 0.65;
use std::sync::OnceLock;

pub(crate) fn sample_with_corner(
    ch: char,
    font: Option<&board_render::HandwritingFont>,
    corner: f32,
) -> Result<Vec<HandwritingStroke>, String> {
    static TEMPLATES: OnceLock<Vec<board_hwr::InkTemplate>> = OnceLock::new();
    let templates =
        TEMPLATES.get_or_init(|| board_hwr::InkRecognizer::default().templates().to_vec());
    let label = match ch {
        'π' => "pi".to_owned(),
        '*' => "×".to_owned(),
        '−' => "-".to_owned(),
        _ => ch.to_string(),
    };
    let custom: Option<Vec<Vec<(f32, f32)>>> = match ch {
        // Curved, hooked lowercase x remains distinct even if viewed at the same
        // normalized size as the straight multiplication cross.
        'x' => Some(vec![
            vec![
                (0.0, 0.08),
                (0.15, 0.0),
                (0.35, 0.18),
                (0.65, 0.82),
                (0.85, 1.0),
                (1.0, 0.92),
            ],
            vec![(0.95, 0.0), (0.05, 1.0)],
        ]),
        '/' => Some(vec![vec![(0.8, 0.0), (0.0, 1.0)]]),
        ',' => Some(vec![vec![(0.2, 0.9), (0.1, 1.08)]]),
        ':' => Some(vec![vec![(0.2, 0.4)], vec![(0.2, 0.9)]]),
        '[' => Some(vec![vec![(0.4, 0.0), (0.0, 0.0), (0.0, 1.0), (0.4, 1.0)]]),
        ']' => Some(vec![vec![(0.0, 0.0), (0.4, 0.0), (0.4, 1.0), (0.0, 1.0)]]),
        '±' => Some(vec![
            vec![(0.0, 0.35), (0.8, 0.35)],
            vec![(0.4, 0.05), (0.4, 0.65)],
            vec![(0.0, 0.85), (0.8, 0.85)],
        ]),
        '≈' => Some(vec![
            vec![(0.0, 0.35), (0.2, 0.23), (0.6, 0.47), (0.8, 0.35)],
            vec![(0.0, 0.7), (0.2, 0.58), (0.6, 0.82), (0.8, 0.7)],
        ]),
        _ => None,
    };
    let lines = custom.or_else(|| {
        templates.iter().find(|t| t.text == label).map(|t| {
            t.strokes
                .iter()
                .map(|stroke| {
                    stroke
                        .iter()
                        .map(|p| {
                            let y = match ch {
                                '.' => 0.96,
                                '-' | '−' => 1.0 - MATH_AXIS,
                                ';' => 0.45 + p.y * 0.6,
                                _ => p.y,
                            };
                            (p.x, y)
                        })
                        .collect()
                })
                .collect()
        })
    });
    if let Some(lines) = lines {
        return Ok(lines
            .into_iter()
            .map(|line| {
                let mut time = 0.0;
                let mut previous: Option<(f32, f32)> = None;
                let points = line
                    .into_iter()
                    .map(|(x, y)| {
                        let (x, y) = match ch {
                            // Ascenders retain cap height; x-height bodies sit on
                            // the baseline, and descenders extend below it.
                            'a' | 'c' | 'e' | 'n' | 'o' | 's' | 'x' | 'g' => {
                                (x * X_HEIGHT, 1.0 - X_HEIGHT + y * X_HEIGHT)
                            }
                            'y' => (x * X_HEIGHT, 1.0 - X_HEIGHT + y * 0.85),
                            '+' | '×' | '*' | '÷' | '=' => (x, y + 0.5 - MATH_AXIS),
                            '±' => (x, y + 0.55 - MATH_AXIS),
                            '≈' => (x, y + 0.475 - MATH_AXIS),
                            _ => (x, y),
                        };
                        let (x, y) = (28.0 + x * 60.0, 24.0 + y * 72.0);
                        if let Some((px, py)) = previous {
                            time += f64::from((x - px).hypot(y - py)) / 600.0;
                        }
                        previous = Some((x, y));
                        StrokePoint {
                            x,
                            y,
                            time,
                            pressure: 1.0,
                        }
                    })
                    .collect();
                HandwritingStroke {
                    points,
                    style: Style::default(),
                }
            })
            .collect());
    }
    let strokes = font
        .ok_or_else(|| format!("没有字符「{ch}」的基础字形；本机字体不可用"))?
        .sample(ch)
        .map_err(|e| e.to_string())?;
    Ok(smooth_font_strokes(strokes, corner))
}

/// Font skeletons are pixel-thinned polylines; smooth them so synthesized ink
/// does not inherit axis-aligned jaggies. Templates and manual samples never
/// pass through here. Strength follows the learned corner sharpness: writers
/// with sharp strokes keep more of the skeleton's corners. Deterministic, and
/// the 2048-point sample budget is never exceeded (oversized input is kept).
fn smooth_font_strokes(strokes: Vec<HandwritingStroke>, corner: f32) -> Vec<HandwritingStroke> {
    let corner = if corner.is_finite() {
        corner.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let strength = 0.6 * (1.0 - corner) + 0.1;
    let total_points = |strokes: &[HandwritingStroke]| {
        strokes
            .iter()
            .map(|stroke| stroke.points.len())
            .sum::<usize>()
    };
    if total_points(&strokes) > crate::handwriting::MAX_SAMPLE_POINTS {
        return strokes;
    }
    let mut candidate: Vec<HandwritingStroke> = strokes
        .iter()
        .map(|stroke| {
            let mut stroke = stroke.clone();
            let length: f32 = stroke
                .points
                .windows(2)
                .map(|pair| (pair[1].x - pair[0].x).hypot(pair[1].y - pair[0].y))
                .sum();
            if stroke.points.len() >= 3 && length >= 16.0 {
                let spacing = (length / 1600.0).clamp(2.5, 6.0);
                if let Ok(points) =
                    board_ink::smooth_resample(&stroke.points, spacing, strength, 75.0)
                {
                    stroke.points = points;
                }
            }
            stroke
        })
        .collect();
    if total_points(&candidate) > crate::handwriting::MAX_SAMPLE_POINTS {
        return strokes;
    }
    // Deterministic re-timing: cumulative distance at a fixed 600 units/second
    // pace; style application later re-times with the learned speed profile.
    for stroke in &mut candidate {
        let mut time = 0.0;
        let mut previous: Option<(f32, f32)> = None;
        for point in &mut stroke.points {
            if let Some((px, py)) = previous {
                time += f64::from((point.x - px).hypot(point.y - py)) / 600.0;
            }
            point.time = time;
            previous = Some((point.x, point.y));
        }
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synthesis_confusables_keep_lowercase_size_and_distinct_x_shape() {
        let bounds = |ink: &[HandwritingStroke]| {
            let mut bounds = egui::Rect::NOTHING;
            for p in ink.iter().flat_map(|s| &s.points) {
                bounds.extend_with(egui::pos2(p.x, p.y));
            }
            bounds
        };
        for size in [0.01, 26.0, 72.0, 144.0] {
            let profile = crate::handwriting::Profile::default();
            let render = |ch: char| {
                let text = ch.to_string();
                let kind = board_core::ObjectKind::Text {
                    position: board_core::Point::default(),
                    text: text.clone(),
                    size,
                    color: board_core::Color::default(),
                };
                let board_core::ObjectKind::Handwritten { strokes, .. } = profile
                    .render_adaptive(&kind, &text, |ch| sample_with_corner(ch, None, 0.5))
                    .unwrap()
                else {
                    panic!("Expected ink")
                };
                strokes
            };
            for (lower, upper) in [('o', '0'), ('x', '×')] {
                let a = render(lower);
                let b = render(upper);
                assert_ne!(a, b);
                assert!((bounds(&a).height() / bounds(&b).height() - X_HEIGHT).abs() < 0.0001);
                assert!(bounds(&a).width() < bounds(&b).width());
            }
            assert!(render('x')[0].points.len() > render('×')[0].points.len());
            assert_eq!(render('*'), render('×'));
        }
        let o = bounds(&sample_with_corner('o', None, 0.5).unwrap());
        assert_eq!(o.max.y, 96.0);
        assert!(o.min.y > 24.0);
        // Recognition retains its deliberately ambiguous cross templates.
        let recognizer = board_hwr::InkRecognizer::default();
        let templates = recognizer.templates();
        assert_eq!(
            templates.iter().find(|t| t.text == "x").unwrap().strokes,
            templates.iter().find(|t| t.text == "×").unwrap().strokes
        );
    }

    #[test]
    fn mathematical_fallback_is_bounded_deterministic_and_keeps_punctuation_position() {
        let mut profile = crate::handwriting::Profile::default();
        for ch in "0123456789+-−*/×÷=xy().,;:[]±≈π^".chars() {
            let ink = sample_with_corner(ch, None, 0.5).unwrap();
            assert_eq!(ink, sample_with_corner(ch, None, 0.5).unwrap());
            profile.add_sample(ch, ink).unwrap();
        }
        assert!(sample_with_corner('.', None, 0.5).unwrap()[0].points[0].y > 90.0);
        assert_eq!(
            sample_with_corner('-', None, 0.5).unwrap()[0].points[0].y,
            60.0
        );
        assert!(sample_with_corner('漢', None, 0.5).is_err());
    }

    #[test]
    fn font_skeleton_smoothing_is_bounded_deterministic_and_corner_aware() {
        let jag = HandwritingStroke {
            // Shallow ~67° zigzag: pixel-jitter-like wobble that smoothing
            // should damp, unlike sharp ≥75° corners that must survive.
            points: (0..48)
                .map(|i| StrokePoint {
                    x: 20.0 + i as f32 * 3.0,
                    y: if i % 2 == 0 { 63.0 } else { 65.0 },
                    time: i as f64 * 0.01,
                    pressure: 1.0,
                })
                .collect(),
            style: Style::default(),
        };
        // smooth_resample is a single damped pass whose anti-jitter loop skips
        // the second and second-to-last samples, so compare mean wobble
        // amplitude rather than the max (which stays at those two samples).
        let wobble = |stroke: &HandwritingStroke| {
            let sum: f32 = stroke
                .points
                .iter()
                .map(|point| (point.y - 64.0).abs())
                .sum();
            sum / stroke.points.len() as f32
        };
        let soft = smooth_font_strokes(vec![jag.clone()], 0.0);
        let soft_again = smooth_font_strokes(vec![jag.clone()], 0.0);
        let sharp = smooth_font_strokes(vec![jag.clone()], 1.0);
        assert_eq!(soft, soft_again, "deterministic");
        assert_ne!(soft[0].points, jag.points, "jagged skeletons are smoothed");
        assert!(
            wobble(&soft[0]) < wobble(&jag) - 0.1,
            "round writers smooth away zigzag wobble"
        );
        assert!(wobble(&sharp[0]) > wobble(&soft[0]) + 0.1);
        assert!(soft[0].points.len() <= crate::handwriting::MAX_SAMPLE_POINTS);
        // Oversized input (beyond the 2048-point sample budget) is kept
        // verbatim instead of being smoothed or re-timed.
        let mut huge = jag.clone();
        let last = *huge.points.last().unwrap();
        huge.points
            .resize(crate::handwriting::MAX_SAMPLE_POINTS + 1, last);
        let kept = smooth_font_strokes(vec![huge.clone()], 0.0);
        assert_eq!(kept[0].points, huge.points);
    }
}
