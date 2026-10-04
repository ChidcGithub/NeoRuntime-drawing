use super::*;

fn frame(image: bool) -> BoardObject {
    BoardObject {
        id: "frame".into(),
        kind: if image {
            ObjectKind::Image {
                position: Point { x: 200.0, y: 180.0 },
                width: 200.0,
                height: 100.0,
                asset_ref: "asset:screenshot".into(),
            }
        } else {
            ObjectKind::FunctionPlot {
                position: Point { x: 200.0, y: 180.0 },
                width: 200.0,
                height: 100.0,
                expressions: vec!["x".into(), "y^2=x".into()],
                x_min: -13.0,
                x_max: 7.0,
                y_min: -2.0,
                y_max: 8.0,
            }
        },
    }
}

#[test]
fn frame_resize_all_corners_grow_shrink_and_preserve_payload() {
    for image in [false, true] {
        for index in 0..4 {
            for amount in [-40.0, 60.0] {
                let original = frame(image);
                let anchor = resize_handles(&original)[(index + 2) % 4];
                let mut resized = original.clone();
                resize_object(
                    &mut resized,
                    index,
                    Point {
                        x: amount * if index == 0 || index == 3 { -1.0 } else { 1.0 },
                        y: amount * if index < 2 { -1.0 } else { 1.0 },
                    },
                );
                let (position, width, height) = frame_geometry(&resized).unwrap();
                assert_eq!(resize_handles(&resized)[(index + 2) % 4], anchor);
                if image {
                    assert_eq!(width / height, 2.0);
                    assert!((width - (200.0 + amount * 1.2)).abs() < 0.001);
                } else {
                    assert_eq!((width, height), (200.0 + amount, 100.0 + amount));
                }
                let mut expected = original;
                match &mut expected.kind {
                    ObjectKind::Image {
                        position: p,
                        width: w,
                        height: h,
                        ..
                    }
                    | ObjectKind::FunctionPlot {
                        position: p,
                        width: w,
                        height: h,
                        ..
                    } => {
                        *p = position;
                        *w = width;
                        *h = height;
                    }
                    _ => unreachable!(),
                }
                assert_eq!(resized, expected);
                resized.validate().unwrap();
                assert!(vertices(&resized).is_empty());
            }
        }
    }
}

#[test]
fn frame_resize_minimum_crossing_noop_and_overflow_are_safe() {
    for image in [false, true] {
        for index in 0..4 {
            let original = frame(image);
            for (handle, delta) in [
                (index, Point::default()),
                (4, Point { x: 10.0, y: 20.0 }),
                (
                    index,
                    Point {
                        x: f32::NAN,
                        y: 0.0,
                    },
                ),
                (
                    index,
                    Point {
                        x: 0.0,
                        y: f32::INFINITY,
                    },
                ),
                (
                    index,
                    Point {
                        x: f32::MAX * if index == 0 || index == 3 { -1.0 } else { 1.0 },
                        y: 0.0,
                    },
                ),
            ] {
                let mut resized = original.clone();
                resize_object(&mut resized, handle, delta);
                assert_eq!(resized, original);
            }
            let mut resized = original.clone();
            let anchor = resize_handles(&original)[(index + 2) % 4];
            resize_object(
                &mut resized,
                index,
                Point {
                    x: 1000.0 * if index == 0 || index == 3 { 1.0 } else { -1.0 },
                    y: 1000.0 * if index < 2 { 1.0 } else { -1.0 },
                },
            );
            let (_, width, height) = frame_geometry(&resized).unwrap();
            assert_eq!(
                (width, height),
                if image { (64.0, 32.0) } else { (32.0, 32.0) }
            );
            assert_eq!(resize_handles(&resized)[(index + 2) % 4], anchor);
            resized.validate().unwrap();
        }
    }
}

#[test]
fn frame_resize_one_axis_and_tiny_imports() {
    for image in [false, true] {
        for delta in [Point { x: 50.0, y: 0.0 }, Point { x: 0.0, y: 50.0 }] {
            let mut object = frame(image);
            resize_object(&mut object, 2, delta);
            let (_, w, h) = frame_geometry(&object).unwrap();
            if image {
                assert_eq!(w / h, 2.0);
                assert!(w > 200.0 && h > 100.0);
            } else {
                assert_eq!((w, h), (200.0 + delta.x, 100.0 + delta.y));
            }
        }
        let mut tiny = frame(image);
        match &mut tiny.kind {
            ObjectKind::Image { width, height, .. }
            | ObjectKind::FunctionPlot { width, height, .. } => {
                *width = 4.0;
                *height = 2.0;
            }
            _ => unreachable!(),
        }
        let original = tiny.clone();
        resize_object(&mut tiny, 2, Point::default());
        assert_eq!(tiny, original);
        resize_object(&mut tiny, 2, Point { x: 1.0, y: 1.0 });
        let (position, w, h) = frame_geometry(&tiny).unwrap();
        assert_eq!(position, frame_geometry(&original).unwrap().0);
        assert!(w >= 32.0 && h >= 32.0);
    }
}
