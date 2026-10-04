use board_ink::{
    Point, StrokePoint, point_segment_distance, point_stroke_distance, project_to_segment,
};

#[test]
fn segment_projection_preserves_exact_endpoints_across_coordinate_scales() {
    let a = Point { x: 1e6, y: -1e6 };
    let b = Point {
        x: 1e-20,
        y: -1e-20,
    };
    for (start, end) in [(a, b), (b, a)] {
        for (query, t) in [(start, 0.0), (end, 1.0)] {
            let projection = project_to_segment(query, start, end).unwrap();
            assert_eq!(projection.t, t);
            assert_eq!(projection.point, query);
            assert_eq!(projection.distance, 0.0);
            assert_eq!(point_segment_distance(query, start, end).unwrap(), 0.0);
        }
    }
}

#[test]
fn stroke_distance_is_zero_at_small_terminal_sample() {
    let stroke = [
        StrokePoint {
            x: 1e6,
            y: 0.0,
            time: 0.0,
            pressure: 0.5,
        },
        StrokePoint {
            x: 1e-20,
            y: 0.0,
            time: 1.0,
            pressure: 0.5,
        },
    ];
    assert_eq!(
        point_stroke_distance(
            Point {
                x: stroke[1].x,
                y: 0.0
            },
            &stroke
        )
        .unwrap(),
        Some(0.0)
    );
}
