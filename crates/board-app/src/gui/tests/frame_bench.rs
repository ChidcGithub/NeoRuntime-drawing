// Headless 帧基准：`cargo test -p board-app --locked --offline --release frame_bench -- --ignored --nocapture`
// 只测量，不改变行为；#[ignore] 避免进入常规套件。

fn bench_content_app() -> (BoardApp, egui::Context) {
    let mut app = app();
    let ctx = egui::Context::default();
    // 500 笔 × 100 点：书写后的真实黑板量级。
    for stroke_index in 0..500u32 {
        let base_x = (stroke_index % 25) as f32 * 62.0 + 20.0;
        let base_y = (stroke_index / 25) as f32 * 44.0 + 20.0;
        let points: Vec<StrokePoint> = (0..100u32)
            .map(|i| {
                let t = i as f32;
                StrokePoint {
                    x: base_x + t * 0.55,
                    y: base_y + (t * 0.21).sin() * 14.0,
                    time: (t * 0.008) as f64,
                    pressure: 1.0,
                }
            })
            .collect();
        app.add(ObjectKind::Stroke {
            points,
            style: Style {
                color: Color {
                    r: 20,
                    g: 20,
                    b: 30,
                    a: 255,
                },
                width: 2.5,
                dashed: false,
            },
        });
    }
    app.add(ObjectKind::Text {
        position: Point { x: 40.0, y: 4.0 },
        text: "二次方程 x²−5x+6=0 的根为 x=2 与 x=3".into(),
        size: 26.0,
        color: Color {
            r: 20,
            g: 20,
            b: 30,
            a: 255,
        },
    });
    app.add(ObjectKind::FunctionPlot {
        position: Point { x: 900.0, y: 60.0 },
        width: 560.0,
        height: 380.0,
        expressions: vec!["x^2+1".into(), "sin(x)*2".into()],
        x_min: -10.0,
        x_max: 10.0,
        y_min: -7.0,
        y_max: 7.0,
    });
    (app, ctx)
}

fn bench_summary(name: &str, frames: &[f64]) {
    let mut sorted = frames.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let mean = frames.iter().sum::<f64>() / frames.len() as f64;
    let p50 = sorted[sorted.len() / 2];
    let p95 = sorted[(sorted.len() as f64 * 0.95) as usize % sorted.len()];
    let worst = sorted[sorted.len() - 1];
    eprintln!(
        "[bench] {name}: n={} mean={mean:.0}µs p50={p50:.0}µs p95={p95:.0}µs max={worst:.0}µs",
        frames.len()
    );
}

fn bench_run(
    app: &mut BoardApp,
    ctx: &egui::Context,
    frames: usize,
    mut per_frame: impl FnMut(&mut BoardApp, usize),
) -> Vec<f64> {
    let size = egui::Vec2::new(1600.0, 900.0);
    // 预热：网格缓存、纹理、galley 缓存全部就位。
    for _ in 0..40 {
        frame(app, ctx, size, vec![], false);
    }
    let mut samples = Vec::with_capacity(frames);
    for index in 0..frames {
        per_frame(app, index);
        let start = Instant::now();
        frame(app, ctx, size, vec![], false);
        samples.push(start.elapsed().as_secs_f64() * 1e6);
    }
    samples
}

#[test]
#[ignore = "headless 帧基准，--release --ignored 手动运行"]
fn frame_bench_idle() {
    let (mut app, ctx) = bench_content_app();
    let samples = bench_run(&mut app, &ctx, 300, |_, _| {});
    bench_summary("idle(缓存命中)", &samples);
}

#[test]
#[ignore = "headless 帧基准，--release --ignored 手动运行"]
fn frame_bench_ink_preview() {
    let (mut app, ctx) = bench_content_app();
    let context = ContextToken::capture(&app.session.document);
    let mut length = 1.0f32;
    let samples = bench_run(&mut app, &ctx, 300, |app, index| {
        length += 1.2;
        app.gesture = Some(Gesture {
            document: context.document_id.clone(),
            page: context.page_id.clone(),
            revision: context.revision,
            points: (0..(index + 1).min(400))
                .map(|i| StrokePoint {
                    x: 60.0 + i as f32 * 2.6,
                    y: 300.0 + (i as f32 * 0.15).sin() * 60.0,
                    time: i as f64 * 0.008,
                    pressure: 1.0,
                })
                .collect(),
            original: None,
            vertex: None,
            resize: None,
            erasing: None,
        });
    });
    app.gesture = None;
    bench_summary("ink_preview(笔尖预览)", &samples);
}

#[test]
#[ignore = "headless 帧基准，--release --ignored 手动运行"]
fn frame_bench_pan() {
    let (mut app, ctx) = bench_content_app();
    let samples = bench_run(&mut app, &ctx, 300, |app, _| {
        app.view_pan += egui::Vec2::new(4.0, 2.0);
    });
    bench_summary("pan(平移)", &samples);
}

#[test]
#[ignore = "headless 帧基准，--release --ignored 手动运行"]
fn frame_bench_edit() {
    let (mut app, ctx) = bench_content_app();
    let samples = bench_run(&mut app, &ctx, 300, |app, index| {
        if index % 10 == 0 {
            // 每 10 帧一次落笔提交：revision 变化 -> 逐对象缓存增量重镶嵌。
            app.add(ObjectKind::Stroke {
                points: (0..100u32)
                    .map(|i| StrokePoint {
                        x: 40.0 + i as f32 * 0.5,
                        y: 800.0 + index as f32,
                        time: i as f64 * 0.008,
                        pressure: 1.0,
                    })
                    .collect(),
                style: Style {
                    color: Color {
                        r: 20,
                        g: 20,
                        b: 30,
                        a: 255,
                    },
                    width: 2.5,
                    dashed: false,
                },
            });
        }
    });
    bench_summary("edit(提交笔画)", &samples);
}
