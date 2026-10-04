// Only synthetic pixels/rectangles/messages: no desktop capture or GUI calls.
use super::*;

fn desktop() -> Rect {
    Rect::desktop(-1920, -1080, 5760, 3240).unwrap()
}
fn point(x: i32, y: i32) -> Point {
    Point { x, y }
}

#[test]
fn negative_desktop_reverse_drag_and_cross_monitor_selection() {
    let mut selection = Selection::new(desktop());
    selection.input(Input::Down(point(2000, 1200)));
    selection.input(Input::Move(point(-1800, -900)));
    let expected = Rect {
        left: -1800,
        top: -900,
        right: 2000,
        bottom: 1200,
    };
    assert_eq!(selection.preview(), Some(expected));
    selection.input(Input::Up(point(-1800, -900)));
    assert_eq!(selection.result, Some(Ok(expected)));
    assert_eq!(
        expected.dimensions().unwrap(),
        (3800, 2100, 3800 * 2100 * 4)
    );
}

#[test]
fn all_drag_directions_and_outside_points_are_clipped() {
    let bounds = Rect::desktop(-10, -20, 30, 40).unwrap();
    for (a, b) in [
        (point(-100, -100), point(100, 100)),
        (point(100, 100), point(-100, -100)),
        (point(100, -100), point(-100, 100)),
        (point(-100, 100), point(100, -100)),
    ] {
        assert_eq!(bounds.between(a, b), bounds);
    }
}

#[test]
fn wide_virtual_desktop_does_not_truncate_to_signed_16_bit() {
    let bounds = Rect::desktop(-40000, -20000, 80000, 40000).unwrap();
    let rect = bounds.between(point(35000, 0), point(35010, 20));
    assert_eq!(rect.dimensions().unwrap(), (10, 20, 800));
    assert_eq!(rect.left, 35000);
}

#[test]
fn invalid_or_overflowing_virtual_desktop_is_rejected() {
    for args in [
        (0, 0, 0, 1),
        (0, 0, 1, -1),
        (i32::MAX, 0, 1, 1),
        (0, i32::MAX, 1, 1),
    ] {
        assert!(Rect::desktop(args.0, args.1, args.2, args.3).is_err());
    }
    assert!(
        Rect {
            left: i32::MIN,
            top: 0,
            right: i32::MAX,
            bottom: 1
        }
        .dimensions()
        .is_err()
    );
}

#[test]
fn empty_drag_is_an_error_and_mouse_up_without_down_is_ignored() {
    let mut selection = Selection::new(desktop());
    selection.input(Input::Up(point(1, 1)));
    assert!(selection.result.is_none());
    selection.input(Input::Down(point(1, 1)));
    selection.input(Input::Up(point(1, 20)));
    assert_eq!(selection.result, Some(Err(End::Empty)));
}

#[test]
fn escape_right_click_focus_loss_and_display_changes_are_terminal() {
    for (input, expected) in [
        (Input::Cancel, End::Cancelled),
        (Input::LostCapture, End::Cancelled),
        (Input::DisplayChanged, End::DisplayChanged),
    ] {
        let mut selection = Selection::new(desktop());
        selection.input(Input::Down(point(0, 0)));
        selection.input(input);
        selection.input(Input::Up(point(200, 200)));
        assert_eq!(selection.result, Some(Err(expected)));
    }
}

#[test]
fn capture_loss_after_mouse_up_does_not_erase_completed_selection() {
    let mut selection = Selection::new(desktop());
    selection.input(Input::LostCapture);
    assert!(selection.result.is_none());
    selection.input(Input::Down(point(-5, -5)));
    selection.input(Input::Up(point(5, 5)));
    let result = selection.result;
    selection.input(Input::LostCapture);
    selection.input(Input::Cancel);
    assert_eq!(selection.result, result);
}

#[test]
fn cancel_and_timeout_boundaries() {
    assert!(check_abort(false, TIMEOUT - Duration::from_nanos(1)).is_ok());
    assert_eq!(check_abort(false, TIMEOUT).unwrap_err(), TIMED_OUT);
    assert_eq!(check_abort(true, Duration::ZERO).unwrap_err(), CANCELLED);
    assert_eq!(check_abort(true, TIMEOUT).unwrap_err(), CANCELLED);
}

#[test]
fn dimensions_and_checked_rgba_budget_are_enforced() {
    assert_eq!(rgba_len(8192, 1024).unwrap(), MAX_RGBA_BYTES);
    assert_eq!(rgba_len(1024, 8192).unwrap(), MAX_RGBA_BYTES);
    assert_eq!(rgba_len(1, 8192).unwrap(), 32768);
    for (w, h) in [
        (0, 1),
        (1, 0),
        (8193, 1),
        (1, 8193),
        (8192, 1025),
        (8192, 8192),
        (u32::MAX, u32::MAX),
    ] {
        assert!(rgba_len(w, h).is_err());
    }
}

#[test]
fn top_down_bgra_is_opaque_rgba_without_reversing_rows() {
    let image = bgra_to_rgba(
        2,
        2,
        &[3, 2, 1, 0, 6, 5, 4, 123, 9, 8, 7, 255, 12, 11, 10, 0],
        || Ok(()),
    )
    .unwrap();
    assert_eq!(
        image.pixels,
        [1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255]
    );
    for length in [0, 3, 5, 8] {
        assert!(bgra_to_rgba(1, 1, &vec![0; length], || Ok(())).is_err());
    }
}

#[test]
fn pixel_conversion_checks_cancellation_before_allocation_and_each_row() {
    assert_eq!(
        bgra_to_rgba(0, 0, &[], || Err(CANCELLED.into())).unwrap_err(),
        CANCELLED
    );
    let mut calls = 0;
    let result = bgra_to_rgba(1, 3, &[0; 12], || {
        calls += 1;
        if calls == 3 {
            Err(CANCELLED.into())
        } else {
            Ok(())
        }
    });
    assert_eq!(result.unwrap_err(), CANCELLED);
    assert_eq!(calls, 3);
}

#[test]
fn png_round_trip_is_static_rgba_and_has_no_trailing_data() {
    let image = bgra_to_rgba(2, 1, &[3, 2, 1, 0, 6, 5, 4, 0], || Ok(())).unwrap();
    let bytes = encode_png(image, || Ok(())).unwrap();
    assert!(bytes.len() <= MAX_PNG_BYTES);
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(&bytes[bytes.len() - 12..], b"\0\0\0\0IEND\xaeB`\x82");
    let decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
    let mut reader = decoder.read_info().unwrap();
    assert!(reader.info().animation_control.is_none());
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).unwrap();
    assert_eq!((info.width, info.height), (2, 1));
    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    assert_eq!(&pixels[..info.buffer_size()], &[1, 2, 3, 255, 4, 5, 6, 255]);
    reader.finish().unwrap();
}

#[test]
fn bounded_png_sink_accepts_exact_limit_and_rejects_extra_without_copying() {
    let mut sink = PngSink {
        bytes: vec![0; MAX_PNG_BYTES - 1],
        check: || Ok(()),
    };
    assert_eq!(sink.write(&[7]).unwrap(), 1);
    assert_eq!(sink.bytes.len(), MAX_PNG_BYTES);
    assert!(sink.write(&[8]).unwrap_err().to_string().contains("8 MiB"));
    assert_eq!(sink.bytes.len(), MAX_PNG_BYTES);
    assert_eq!(sink.bytes.last(), Some(&7));
}

#[test]
fn cancelled_png_writes_do_not_copy_and_deadlines_propagate() {
    let mut sink = PngSink {
        bytes: Vec::new(),
        check: || Err(CANCELLED.into()),
    };
    assert!(
        sink.write(&[1, 2, 3])
            .unwrap_err()
            .to_string()
            .contains("取消")
    );
    assert!(sink.bytes.is_empty());
    let image = bgra_to_rgba(1, 1, &[0; 4], || Ok(())).unwrap();
    assert_eq!(
        encode_png(image, || Err(TIMED_OUT.into())).unwrap_err(),
        TIMED_OUT
    );
}

#[test]
fn malformed_rgba_is_rejected_before_encoding() {
    for (width, height, pixels) in [
        (0, 1, vec![]),
        (1, 0, vec![]),
        (1, 1, vec![0; 3]),
        (8193, 1, vec![]),
    ] {
        assert!(
            encode_png(
                RgbaImage {
                    width,
                    height,
                    pixels
                },
                || Ok(())
            )
            .is_err()
        );
    }
}

#[test]
fn incompressible_synthetic_image_hits_png_limit_during_encoding() {
    let mut pixels = vec![0; 2048 * 1536 * 4];
    let mut state = 0x1234_5678u32;
    for pixel in pixels.chunks_exact_mut(4) {
        for channel in &mut pixel[..3] {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *channel = state as u8;
        }
        pixel[3] = 255;
    }
    let error = encode_png(
        RgbaImage {
            width: 2048,
            height: 1536,
            pixels,
        },
        || Ok(()),
    )
    .unwrap_err();
    assert!(error.contains("8 MiB"), "{error}");
}
