// Keep includes in the same module to preserve gui::tests paths and shared helpers.
use super::*;

include!("tests/gpu_offscreen.rs");

include!("tests/performance.rs");

fn app() -> BoardApp {
    let mut session = Session::new(AppMode::Blackboard.kind());
    session.handle(Request::new("neo:setup", "configure", serde_json::json!({"classroom_safe": true, "desktop_capture_allowed": false, "agent_allowed": false})).unwrap());
    BoardApp::new(
        AppMode::Blackboard,
        session,
        false,
        None,
        Default::default(),
    )
}

fn frame(
    app: &mut BoardApp,
    ctx: &egui::Context,
    size: Vec2,
    events: Vec<egui::Event>,
    toolbar: bool,
) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
        events,
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| {
        if toolbar {
            app.toolbar(ctx);
        }
        app.canvas(ui);
    });
    output.textures_delta.clear();
    output
}

fn pointer(pos: Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

fn drag_plot(id: &str, x: f32, expressions: &[&str]) -> BoardObject {
    BoardObject {
        id: id.into(),
        kind: ObjectKind::FunctionPlot {
            position: Point { x, y: 100.0 },
            width: 100.0,
            height: 100.0,
            expressions: expressions.iter().map(|s| (*s).into()).collect(),
            x_min: -10.0,
            x_max: 10.0,
            y_min: -7.0,
            y_max: 7.0,
        },
    }
}

fn plot_drag_app(objects: Vec<BoardObject>) -> (BoardApp, egui::Context) {
    let mut app = app();
    app.tool = Tool::Select;
    app.session.document.pages[0].objects = objects;
    app.session.history = board_core::History::new(&app.session.document);
    (app, egui::Context::default())
}

fn plot_frame(app: &mut BoardApp, ctx: &egui::Context, events: Vec<egui::Event>) -> String {
    app.controls.clear();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
            events,
            ..Default::default()
        },
        |ui| {
            app.plot_controls(ctx);
            app.canvas(ui);
        },
    );
    output.textures_delta.clear();
    format!("{:?}", output.shapes)
}

include!("tests/intersections.rs");

fn drag_to(app: &mut BoardApp, ctx: &egui::Context, start: Pos2, end: Pos2) -> egui::FullOutput {
    let size = Vec2::new(800.0, 600.0);
    frame(app, ctx, size, vec![], false);
    frame(
        app,
        ctx,
        size,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
        false,
    );
    assert!(app.gesture.is_some());
    frame(app, ctx, size, vec![egui::Event::PointerMoved(end)], false)
}

fn edit_shape(shape: ShapeKind, a: Point, b: Point) -> BoardObject {
    BoardObject {
        id: "shape".into(),
        kind: ObjectKind::Shape {
            shape,
            points: vec![a, b],
            style: Style::default(),
        },
    }
}

fn shape_points(object: &BoardObject) -> &[Point] {
    let ObjectKind::Shape { points, .. } = &object.kind else {
        panic!("shape expected")
    };
    points
}

fn release(app: &mut BoardApp, ctx: &egui::Context, end: Pos2) {
    frame(
        app,
        ctx,
        Vec2::new(800.0, 600.0),
        vec![pointer(end, false)],
        false,
    );
}

fn reverse_drag_objects() -> Vec<BoardObject> {
    let shape = edit_shape(
        ShapeKind::Rectangle,
        Point { x: 100.0, y: 100.0 },
        Point { x: 200.0, y: 200.0 },
    );
    let mut line = edit_shape(
        ShapeKind::Line,
        Point { x: 300.0, y: 100.0 },
        Point { x: 450.0, y: 250.0 },
    );
    line.id = "line".into();
    vec![shape, line]
}

fn bind(app: &mut BoardApp, line: &str, endpoint: usize, target: &str, index: usize) {
    let d = &mut app.session.document;
    let page = d.current_page().id.clone();
    d.connect(
        &page,
        d.revision,
        board_core::Connection {
            id: new_id(),
            page_id: page.clone(),
            line_id: line.into(),
            line_endpoint: endpoint,
            target_id: target.into(),
            target: board_core::Anchor::Vertex { index },
        },
    )
    .unwrap();
    app.session.history = board_core::History::new(d);
}

include!("tests/vertex_connections.rs");

include!("tests/curve_resize.rs");

include!("tests/frame_resize.rs");

include!("tests/plot_drag.rs");

include!("tests/gestures.rs");

fn text_position(output: &egui::FullOutput, text: &str) -> Pos2 {
    fn locate(shape: &egui::Shape, text: &str) -> Option<Pos2> {
        match shape {
            egui::Shape::Text(t) if t.galley.text() == text => Some(t.pos + t.galley.size() / 2.0),
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| locate(s, text)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|s| locate(&s.shape, text))
        .unwrap_or_else(|| panic!("missing button {text}"))
}

fn board_frame(
    app: &mut BoardApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 720.0))),
            events,
            ..Default::default()
        },
        |ui| app.board_ui(ui, ctx),
    );
    output.textures_delta.clear();
    output
}

fn click_text(app: &mut BoardApp, ctx: &egui::Context, text: &str) {
    board_frame(app, ctx, vec![]);
    let out = board_frame(app, ctx, vec![]);
    let pos = text_position(&out, text);
    board_frame(
        app,
        ctx,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
    );
    board_frame(app, ctx, vec![pointer(pos, false)]);
}

fn click_toolbar_icon(app: &mut BoardApp, ctx: &egui::Context, label: &str) {
    board_frame(app, ctx, vec![]);
    board_frame(app, ctx, vec![]);
    let rect = ctx
        .data(|data| data.get_temp::<Rect>(egui::Id::new(("icon_rect", label))))
        .unwrap();
    let pos = rect.center();
    assert!(
        app.controls.iter().any(|r| r.contains(pos)),
        "{label}: {rect:?}"
    );
    board_frame(
        app,
        ctx,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
    );
    board_frame(app, ctx, vec![pointer(pos, false)]);
    assert!(app.gesture.is_none(), "toolbar click must not draw");
}

fn open_toolbar_menu(app: &mut BoardApp, ctx: &egui::Context, label: &str) {
    click_toolbar_icon(app, ctx, label);
    if matches!(label, "画笔" | "橡皮") {
        assert!(!egui::Popup::is_any_open(ctx), "first tool click: {label}");
        click_toolbar_icon(app, ctx, label);
    }
    assert!(egui::Popup::is_any_open(ctx), "open menu: {label}");
}

include!("tests/toolbar_interaction.rs");

include!("tests/toolbar_layout.rs");

include!("tests/eraser_menu.rs");

include!("tests/capture_and_window.rs");

include!("tests/local_capture_flow.rs");

include!("tests/authorization.rs");

fn write_sample(app: &mut BoardApp, now: Instant) -> usize {
    write_sample_at(app, now, Vec2::ZERO)
}

fn write_sample_at(app: &mut BoardApp, now: Instant, offset: Vec2) -> usize {
    // Explicit, slightly imperfect upright pen trajectories for 1+1, not injected text.
    for points in [
        vec![(80.0, 80.0), (80.3, 100.0), (80.0, 120.0)],
        vec![(110.0, 100.0), (130.0, 100.2), (150.0, 100.0)],
        vec![(130.0, 80.0), (130.1, 100.0), (130.0, 120.0)],
        vec![(180.0, 80.0), (180.2, 100.0), (180.0, 120.0)],
    ] {
        app.ink_started();
        app.gesture = Some(Gesture {
            document: app.session.document.id.clone(),
            page: app.session.document.current_page().id.clone(),
            revision: app.session.document.revision,
            points: points
                .into_iter()
                .enumerate()
                .map(|(i, (x, y))| StrokePoint {
                    x: x + offset.x,
                    y: y + offset.y,
                    time: i as f64 * 0.1,
                    pressure: 0.6,
                })
                .collect(),
            original: None,
            vertex: None,
            resize: None,
            erasing: None,
        });
        app.finish_gesture_at(now);
    }
    app.session.document.current_page().objects.len()
}

fn recognize_sample_at(app: &mut BoardApp, ctx: &egui::Context, offset: Vec2) -> String {
    app.tool = Tool::Pen;
    app.set_ink_math_mode(InkMathMode::Confirm);
    let now = Instant::now();
    write_sample_at(app, now, offset);
    app.poll_backend_ink(ctx, now + AutoCalculate::IDLE_DELAY);
    drain_workers(app);
    assert_eq!(app.recognition.as_ref().unwrap().text, "1+1");
    app.active_candidate.clone().unwrap()
}

include!("tests/ink_cache.rs");

fn click_ink_icon(app: &mut BoardApp, ctx: &egui::Context) {
    plot_frame(app, ctx, vec![]);
    plot_frame(app, ctx, vec![]);
    let rect = app.controls[0];
    assert!(ctx.content_rect().contains_rect(rect));
    let pos = rect.center();
    let count = app.session.document.current_page().objects.len();
    plot_frame(
        app,
        ctx,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
    );
    assert!(app.gesture.is_none());
    assert_eq!(app.session.document.current_page().objects.len(), count);
    plot_frame(app, ctx, vec![pointer(pos, false)]);
    assert!(app.gesture.is_none());
}

fn candidate(text: &str, confidence: f32) -> Recognition {
    Recognition {
        text: text.into(),
        confidence,
        candidates: vec![board_hwr::Candidate {
            text: text.into(),
            confidence,
        }],
        requires_confirmation: true,
        backend: "test-injection".into(),
    }
}

fn mock_neural(latex: &str, finished: bool, lp: f64) -> HwrResult {
    let mut result = neural_result(NeuralFormula {
        latex: latex.into(),
        finished,
        mean_log_probability: lp,
        generated_tokens: 6,
    });
    // Synthetic pixels only exercise worker delivery; tensor provenance is tested in board-hwr.
    result.preview = Some(board_hwr::NeuralInputPreview {
        size: [448, 448],
        gray: vec![243; 448 * 448],
    });
    result
}

include!("tests/ink_preview.rs");

include!("tests/neural_recognition.rs");

include!("tests/model_lifecycle.rs");

include!("tests/ink_math.rs");

include!("tests/math_workers.rs");

include!("tests/eraser_rendering.rs");

include!("tests/background_cancellation.rs");
