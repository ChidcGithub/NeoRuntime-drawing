use super::*;
use board_core::{ShapeKind, StrokePoint};

fn append_object(scene: &mut Scene, object: &BoardObject) -> Result<()> {
    super::append_object(scene, object, &mut PlotSamplesCache::default())
}

fn math_kind(layout: MathLayout) -> ObjectKind {
    ObjectKind::Math {
        position: p(20.0, 20.0),
        layout,
        size: 24.0,
        color: Color::default(),
    }
}
fn fraction() -> MathLayout {
    MathLayout::Fraction(
        Box::new(MathLayout::Text("-1".into())),
        Box::new(MathLayout::Text("(2)".into())),
    )
}

#[test]
fn thumbnail_reuses_real_scene_with_scaled_math_ink_plot_and_image_clip() {
    let mut page = Page::new();
    page.objects = vec![
        object(math_kind(fraction())),
        object(ObjectKind::Stroke {
            points: vec![
                StrokePoint {
                    x: 10.0,
                    y: 100.0,
                    time: 0.0,
                    pressure: 1.0,
                },
                StrokePoint {
                    x: 100.0,
                    y: 130.0,
                    time: 0.1,
                    pressure: 1.0,
                },
            ],
            style: Style::default(),
        }),
        object(ObjectKind::Image {
            position: p(100.0, 10.0),
            width: 40.0,
            height: 30.0,
            asset_ref: "asset:preview".into(),
        }),
        object(ObjectKind::FunctionPlot {
            position: p(150.0, 100.0),
            width: 200.0,
            height: 160.0,
            expressions: vec!["x".into()],
            x_min: -5.0,
            x_max: 5.0,
            y_min: -5.0,
            y_max: 5.0,
        }),
    ];
    let before = page.clone();
    let preview = PageThumbnail::new(&page, ("doc", 4));
    assert!(preview.matches(("doc", 4)));
    assert!(!preview.matches(("doc", 5)));
    assert!(!preview.matches(("other", 4)));
    let target = Rect::from_min_size(egui::pos2(50.0, 60.0), egui::vec2(200.0, 150.0));
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        assert!(
            preview
                .paint(
                    ui.painter(),
                    target,
                    egui::vec2(400.0, 300.0),
                    true,
                    &NoResources
                )
                .is_err()
        );
        preview
            .paint(
                ui.painter(),
                target,
                egui::vec2(400.0, 300.0),
                true,
                &|_: &str| Some(egui::TextureId::User(17)),
            )
            .unwrap();
    });
    output.textures_delta.clear();
    assert_eq!(page, before);
    assert!(output.shapes.len() >= 7);
    assert!(
        output
            .shapes
            .iter()
            .all(|s| target.contains_rect(s.clip_rect))
    );
    assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Mesh(m) if m.texture_id == egui::TextureId::User(17) && m.vertices.iter().all(|v| target.contains(v.pos)))));
    assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "-1" && t.pos.x >= target.left() && t.galley.job.sections[0].format.font_id.size < 24.0)));
    assert!(
        output
            .shapes
            .iter()
            .any(|s| s.clip_rect.width() < target.width())
    );
}

#[test]
fn fraction_has_vertical_text_and_shared_bounds_and_translation() {
    let mut object = object(math_kind(fraction()));
    let mut scene = Scene::default();
    append_object(&mut scene, &object).unwrap();
    let texts: Vec<_> = scene
        .items
        .iter()
        .filter_map(|item| match item {
            Primitive::Text(p, text, size, _) => Some((*p, text.as_str(), *size)),
            _ => None,
        })
        .collect();
    assert_eq!(texts.len(), 2);
    assert_eq!(texts[0].1, "-1");
    assert_eq!(texts[1].1, "(2)");
    let Primitive::Line(a, b, width, _) = scene.items[0] else {
        panic!("fraction bar missing")
    };
    assert_eq!(a.y, b.y);
    assert!(texts[0].0.y + texts[0].2 * 1.5 < a.y - width / 2.0);
    assert!(texts[1].0.y > a.y + width / 2.0);
    let bounds = object_bounds(&object);
    assert_eq!(bounds.min, egui::pos2(20.0, 20.0));
    for (position, text, size) in texts {
        assert!(bounds.contains_rect(Rect::from_min_size(
            position,
            egui::vec2(text.len() as f32 * size * 1.25, size * 1.5)
        )));
    }
    let delta = egui::vec2(13.0, -7.0);
    if let ObjectKind::Math { position, .. } = &mut object.kind {
        position.x += delta.x;
        position.y += delta.y;
    }
    assert_eq!(object_bounds(&object), bounds.translate(delta));
    let mut moved = Scene::default();
    append_object(&mut moved, &object).unwrap();
    for (old, new) in scene.items.iter().zip(&moved.items) {
        match (old, new) {
            (Primitive::Text(a, ta, sa, _), Primitive::Text(b, tb, sb, _)) => {
                assert_eq!(*a + delta, *b);
                assert_eq!(ta, tb);
                assert_eq!(sa, sb);
            }
            (Primitive::Line(a, b, _, _), Primitive::Line(c, d, _, _)) => {
                assert_eq!(*a + delta, *c);
                assert_eq!(*b + delta, *d);
            }
            _ => panic!("translated math must preserve primitives"),
        }
    }
}

#[test]
fn nested_radical_fraction_rows_preserve_sign_parentheses_and_baselines() {
    let layout = MathLayout::Row(vec![
        MathLayout::Text("-(".into()),
        MathLayout::Radical(Box::new(fraction())),
        MathLayout::Text(")".into()),
    ]);
    let measured = MathBox::new(&layout, 24.0).unwrap();
    for (offset, child) in &measured.children {
        assert!((offset.y + child.baseline - measured.baseline).abs() < 0.001);
    }
    let object = object(math_kind(layout));
    let mut scene = Scene::default();
    append_object(&mut scene, &object).unwrap();
    let bounds = object_bounds(&object).expand(0.001);
    let lines: Vec<_> = scene
        .items
        .iter()
        .filter_map(|item| match item {
            Primitive::Line(a, b, width, _) => Some((*a, *b, *width)),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 5); // Four radical segments plus the fraction bar.
    assert!(lines[0].0.x < lines[0].1.x);
    assert!(lines[1].0.y < lines[1].1.y);
    assert!(lines[2].0.y > lines[2].1.y);
    assert_eq!(lines[3].0.y, lines[3].1.y);
    assert!(lines[3].0.y < lines[4].0.y);
    for (a, b, width) in lines {
        assert!(bounds.contains_rect(Rect::from_two_pos(a, b).expand(width / 2.0)));
    }
    let text: String = scene
        .items
        .iter()
        .filter_map(|item| match item {
            Primitive::Text(_, text, _, _) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "-(-1(2))");
}

#[test]
fn math_exports_share_scene_and_plain_text_never_guesses_formulas() {
    let page = page(math_kind(MathLayout::Radical(Box::new(fraction()))));
    let svg = export_svg(&page, 200, 200, false).unwrap();
    assert_eq!(svg.matches("stroke-linecap").count(), 5);
    assert_eq!(svg.matches("<text ").count(), 2);
    assert!(svg.contains("-1") && svg.contains("(2)"));
    assert!(matches!(
        export_png(&page, 200, 200, false),
        Err(RenderError::MissingFont)
    ));
    let mut scene = Scene::default();
    append_object(
        &mut scene,
        &object(ObjectKind::Text {
            position: Point::default(),
            text: "-sqrt(2)/3".into(),
            size: 24.0,
            color: Color::default(),
        }),
    )
    .unwrap();
    assert!(matches!(&scene.items[..], [Primitive::Text(_, text, _, _)] if text == "-sqrt(2)/3"));
    let context = egui::Context::default();
    let mut output = context.run_ui(egui::RawInput::default(), |ui| {
        paint_page(ui.painter(), &page, false);
    });
    output.textures_delta.clear();
    assert!(!output.shapes.is_empty());
}

#[test]
fn invalid_math_layout_is_rejected_before_rendering_or_bounds() {
    for layout in [
        MathLayout::Row(vec![
            MathLayout::Text("x".into());
            board_core::MAX_MATH_NODES
        ]),
        MathLayout::Text("x".repeat(board_core::MAX_MATH_TEXT_BYTES + 1)),
        MathLayout::Text("\u{0}".into()),
    ] {
        let page = page(math_kind(layout));
        assert_eq!(object_bounds(&page.objects[0]), Rect::NOTHING);
        assert!(page_scene(&page).is_err());
        assert!(export_svg(&page, 100, 100, false).is_err());
        assert!(export_png(&page, 100, 100, false).is_err());
    }
}

#[cfg(windows)]
#[test]
fn math_png_and_svg_with_same_font_include_numerator_denominator_and_root() {
    let bytes = match std::fs::read(r"C:\Windows\Fonts\msyh.ttc") {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("SKIP 数学字体回归：微软雅黑 TTC 不可用：{error}");
            return;
        }
    };
    let mut resources = RenderResources::new();
    resources.set_font(bytes, 0).unwrap();
    let page = page(math_kind(MathLayout::Radical(Box::new(fraction()))));
    let svg = export_svg_with_resources(&page, 200, 200, false, &resources).unwrap();
    assert!(svg.contains("aria-label=\"-1\"") && svg.contains("aria-label=\"(2)\""));
    assert!(!svg.contains("<text "));
    assert_eq!(svg.matches("stroke-linecap").count(), 5);
    let png = export_png_with_resources(&page, 200, 200, false, &resources).unwrap();
    let image = tiny_skia::Pixmap::decode_png(&png).unwrap();
    for item in page_scene(&page).unwrap().items {
        match item {
            Primitive::Text(position, text, size, _) => {
                let right = (position.x + text.len() as f32 * size * 1.25).ceil() as u32;
                let bottom = (position.y + size * 1.5).ceil() as u32;
                assert!((position.y as u32..bottom).any(|y| {
                    (position.x as u32..right).any(|x| image.pixel(x, y).unwrap().red() < 128)
                }));
            }
            Primitive::Line(a, b, _, _) => {
                let middle = a.lerp(b, 0.5);
                assert!(image.pixel(middle.x as u32, middle.y as u32).unwrap().red() < 250);
            }
            _ => panic!("unexpected math primitive"),
        }
    }
}

// #region debug-point A: deterministic fullscreen CPU baseline, no native window/GPU
#[test]
#[ignore = "explicit release CPU baseline; reports to stderr"]
fn fullscreen_ink_performance_phase1() {
    use std::time::Instant;

    const WARMUP: usize = 5;
    const SAMPLES: usize = 100;
    const SEED: u32 = 0x83a4_7349;
    const STAGES: [&str; 8] = [
        "cpu_total_us",
        "page_entry_us",
        "revision_update_us",
        "mesh_build_us",
        "cached_paint_arc_clone_us",
        "preview_us",
        "egui_tessellate_us",
        "cpu_drop_us",
    ];
    if cfg!(debug_assertions) {
        panic!("run this ignored benchmark with --release");
    }

    fn distribution(values: &mut [f64]) -> String {
        values.sort_by(f64::total_cmp);
        let percentile = |percent: usize| values[(values.len() * percent).div_ceil(100) - 1];
        format!(
            r#"{{"p50":{:.3},"p95":{:.3},"p99":{:.3},"max":{:.3}}}"#,
            percentile(50),
            percentile(95),
            percentile(99),
            values[values.len() - 1]
        )
    }

    for strokes in [100, 500, 1500] {
        for points_per_stroke in [128, 512] {
            let mut seed = SEED;
            let mut next = || {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 8) as f32 / 16_777_215.0
            };
            let mut objects = Vec::with_capacity(strokes + 1);
            for stroke in 0..=strokes {
                let x = 20.0 + next() * 1720.0;
                let y = 40.0 + next() * 1000.0;
                let length = 80.0 + next() * 100.0;
                let phase = next() * std::f32::consts::TAU;
                objects.push(BoardObject {
                    id: format!("fullscreen-stroke-{stroke}"),
                    kind: ObjectKind::Stroke {
                        points: (0..points_per_stroke)
                            .map(|i| {
                                let t = i as f32 / (points_per_stroke - 1) as f32;
                                StrokePoint {
                                    x: x + t * length,
                                    y: y + (t * std::f32::consts::TAU + phase).sin() * 18.0,
                                    time: i as f64 / 120.0,
                                    pressure: 0.55 + (t * 8.0 + phase).sin() * 0.35,
                                }
                            })
                            .collect(),
                        style: Style {
                            width: 3.0,
                            color: Color {
                                r: 255,
                                g: 255,
                                b: 255,
                                a: 255,
                            },
                            dashed: false,
                        },
                    },
                });
            }
            let new_stroke = objects.pop().unwrap();
            let base = Page {
                id: "fullscreen-seeded".into(),
                objects,
            };
            let clip = Rect::from_min_size(Pos2::ZERO, egui::vec2(1920.0, 1080.0));
            for scenario in [
                "hot_cache",
                "new_stroke_preview",
                "single_stroke_revision_add_undo",
            ] {
                let context = egui::Context::default();
                context.set_pixels_per_point(1.0);
                let mut renderer = PageRenderer::new();
                let mut page = base.clone();
                let mut revision = 1;
                let mut samples: [Vec<f64>; 8] =
                    std::array::from_fn(|_| Vec::with_capacity(SAMPLES));
                let mut counts = RenderDebugMetrics::default();
                let mut cold_total_us = 0.0;
                let mut final_vertices = 0;
                let mut final_triangles = 0;
                let mut cached_vertices = 0;
                let mut cached_triangles = 0;
                let mut scene_primitives = 0;
                let mut final_shapes = 0;
                for frame in 0..WARMUP + SAMPLES {
                    // Fixture mutation is outside timing; no Document/history costs are measured.
                    if scenario == "single_stroke_revision_add_undo" {
                        if page.objects.len() == strokes {
                            page.objects.push(new_stroke.clone());
                        } else {
                            page.objects.pop();
                        }
                        revision += 1;
                    }
                    let mut preview = new_stroke.clone();
                    if let ObjectKind::Stroke { points, .. } = &mut preview.kind {
                        let length = 8 + (points_per_stroke - 8) * frame / (WARMUP + SAMPLES - 1);
                        points.truncate(length);
                    }
                    RENDER_DEBUG_METRICS.with(|metrics| metrics.set(RenderDebugMetrics::default()));
                    let mut page_entry_us = 0.0;
                    let mut preview_us = 0.0;
                    let total_start = Instant::now();
                    let mut output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(clip),
                            ..Default::default()
                        },
                        |ui| {
                            let painter = ui
                                .ctx()
                                .layer_painter(egui::LayerId::background())
                                .with_clip_rect(clip);
                            let start = Instant::now();
                            renderer
                                .paint_page_at_document_revision(
                                    &painter,
                                    &page,
                                    ("fullscreen-bench", revision),
                                    true,
                                    &NoResources,
                                )
                                .unwrap();
                            page_entry_us = start.elapsed().as_secs_f64() * 1e6;
                            if scenario == "new_stroke_preview" {
                                let start = Instant::now();
                                paint_object(&painter, &preview);
                                preview_us = start.elapsed().as_secs_f64() * 1e6;
                            }
                        },
                    );
                    let shapes = output.shapes.len();
                    output.textures_delta.clear();
                    let start = Instant::now();
                    let primitives = context.tessellate(output.shapes, output.pixels_per_point);
                    let tessellate_us = start.elapsed().as_secs_f64() * 1e6;
                    let metrics = RENDER_DEBUG_METRICS.with(|metrics| metrics.get());
                    // Geometry enumeration is excluded from CPU timing.
                    let total_before_counts = total_start.elapsed().as_secs_f64() * 1e6;
                    if frame == WARMUP + SAMPLES - 1 {
                        final_shapes = shapes;
                        for primitive in &primitives {
                            if let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive {
                                final_vertices += mesh.vertices.len();
                                final_triangles += mesh.indices.len() / 3;
                            }
                        }
                        for entry in renderer.objects.values() {
                            if let Some(meshes) = &entry.meshes {
                                for (_, _, mesh) in &meshes.batches {
                                    cached_vertices += mesh.vertices.len();
                                    cached_triangles += mesh.indices.len() / 3;
                                }
                            }
                            scene_primitives += entry.scene.items.len();
                        }
                    }
                    let drop_start = Instant::now();
                    drop(primitives);
                    let drop_us = drop_start.elapsed().as_secs_f64() * 1e6;
                    let total_us = total_before_counts + drop_us;
                    if frame == 0 {
                        cold_total_us = total_us;
                    }
                    if frame >= WARMUP {
                        for (values, value) in samples.iter_mut().zip([
                            total_us,
                            page_entry_us,
                            metrics.scene_update_us,
                            metrics.mesh_build_us,
                            metrics.paint_clone_us,
                            preview_us,
                            tessellate_us,
                            drop_us,
                        ]) {
                            values.push(value);
                        }
                        counts.revision_hits += metrics.revision_hits;
                        counts.revision_misses += metrics.revision_misses;
                        counts.object_hits += metrics.object_hits;
                        counts.object_misses += metrics.object_misses;
                        counts.culled_objects += metrics.culled_objects;
                        counts.mesh_hits += metrics.mesh_hits;
                        counts.mesh_misses += metrics.mesh_misses;
                        counts.paint_arc_clones += metrics.paint_arc_clones;
                        counts.built_vertices += metrics.built_vertices;
                        counts.built_triangles += metrics.built_triangles;
                        counts.batch_rebuilds += metrics.batch_rebuilds;
                        counts.batch_buffer_allocations += metrics.batch_buffer_allocations;
                        counts.batch_arc_allocations += metrics.batch_arc_allocations;
                        counts.batch_copied_vertices += metrics.batch_copied_vertices;
                        counts.batch_copied_indices += metrics.batch_copied_indices;
                        counts.content_compare_skips += metrics.content_compare_skips;
                    }
                }
                if scenario == "single_stroke_revision_add_undo" {
                    assert_eq!(counts.revision_misses, SAMPLES as u64);
                    assert_eq!(counts.object_misses, (SAMPLES / 2) as u64);
                    assert_eq!(counts.mesh_misses, counts.object_misses);
                } else {
                    assert_eq!(counts.revision_hits, SAMPLES as u64);
                    assert_eq!(counts.object_misses, 0);
                    assert_eq!(counts.mesh_misses, 0);
                }
                if scenario == "single_stroke_revision_add_undo" {
                    assert_eq!(counts.object_hits, (SAMPLES * strokes) as u64);
                    assert_eq!(counts.content_compare_skips, 0);
                } else {
                    assert_eq!(counts.object_hits, 0);
                    assert_eq!(counts.content_compare_skips, SAMPLES as u64);
                    assert_eq!(counts.batch_buffer_allocations, 0);
                    assert_eq!(counts.batch_copied_vertices, 0);
                }
                assert_eq!(
                    counts.mesh_hits + counts.culled_objects,
                    (SAMPLES * strokes) as u64
                );
                assert!(cached_vertices > 0 && final_vertices >= cached_vertices);
                let stages = STAGES
                    .iter()
                    .zip(samples.iter_mut())
                    .map(|(name, values)| format!("\"{name}\":{}", distribution(values)))
                    .collect::<Vec<_>>()
                    .join(",");
                let run = serde_json::to_string(
                    &std::env::var("NEO_DRAW_DEBUG_RUN").unwrap_or_else(|_| "pre-fix".into()),
                )
                .unwrap();
                let batch_metrics = serde_json::json!({
                    "batch_rebuilds": counts.batch_rebuilds,
                    "batch_buffer_allocations": counts.batch_buffer_allocations,
                    "batch_arc_allocations": counts.batch_arc_allocations,
                    "batch_copied_vertices": counts.batch_copied_vertices,
                    "batch_copied_indices": counts.batch_copied_indices,
                    "content_compare_skips": counts.content_compare_skips,
                    "aggregate_bytes": renderer.chunks.iter().map(|c| c.bytes).sum::<usize>(),
                    "chunk_objects": STROKE_BATCH_OBJECTS,
                    "allocation_scope": "aggregate mesh buffers and Arc only; not global allocator"
                });
                let body = format!(
                    r#"{{"sessionId":"fullscreen-ink-performance","runId":{run},"hypothesisId":"A","location":"board-render:fullscreen_ink_performance_phase1","msg":"[DEBUG] fullscreen headless CPU stage summary; not FPS/GPU","data":{{"scenario":"{scenario}","seed":{SEED},"width":1920,"height":1080,"pixels_per_point":1,"blackboard":true,"base_strokes":{strokes},"points_per_stroke":{points_per_stroke},"input_points":{},"final_strokes":{},"warmup":{WARMUP},"samples":{SAMPLES},"debug_assertions":false,"cold_total_us":{cold_total_us:.3},"revision_hits":{},"revision_misses":{},"metrics_schema":3,"mesh_cache_unit":"visible_object_plus_64_object_stroke_chunk","batch_metrics":{batch_metrics},"object_hits":{},"object_misses":{},"culled_objects":{},"mesh_hits":{},"mesh_misses":{},"paint_arc_clones":{},"built_vertices_sum":{},"built_triangles_sum":{},"cached_vertices":{cached_vertices},"cached_triangles":{cached_triangles},"final_vertices":{final_vertices},"final_triangles":{final_triangles},"scene_primitives":{scene_primitives},"final_shapes":{final_shapes},"alloc_counters":null,"fixture_and_http_timed":false,"gpu_measured":false,"stages":{{{stages}}}}}}}"#,
                    strokes * points_per_stroke,
                    page.objects.len(),
                    counts.revision_hits,
                    counts.revision_misses,
                    counts.object_hits,
                    counts.object_misses,
                    counts.culled_objects,
                    counts.mesh_hits,
                    counts.mesh_misses,
                    counts.paint_arc_clones,
                    counts.built_vertices,
                    counts.built_triangles,
                );
                eprintln!("{body}");
            }
        }
    }
}
// #endregion

// #region debug-point B:ink-cpu-benchmark
#[test]
#[ignore = "explicit headless CPU sample; optional local debug server"]
fn ink_cpu_benchmark() {
    use std::io::{Read as _, Write as _};
    use std::time::Instant;
    let run = std::env::var("INK_CPU_RUN").unwrap_or_else(|_| "cpu-baseline".into());
    assert!(matches!(run.as_str(), "cpu-baseline" | "cpu-optimized"));
    let page = ink_benchmark_page();
    for preview in [false, true] {
        let context = egui::Context::default();
        let mut renderer = PageRenderer::new();
        let mut baseline: Option<(Option<Page>, Scene)> = None;
        let mut times = Vec::new();
        let mut paints = Vec::new();
        let mut final_shapes = 0;
        let mut vertices = 0;
        let mut cold_us = 0;
        for frame in 0..33 {
            let start = Instant::now();
            let mut paint_us = 0;
            let mut output = context.run_ui(egui::RawInput::default(), |ui| {
                let painter = ui
                    .ctx()
                    .layer_painter(egui::LayerId::background())
                    .with_clip_rect(Rect::from_min_size(Pos2::ZERO, egui::vec2(1200.0, 800.0)));
                let paint_start = Instant::now();
                if preview {
                    let mut stroke = page.objects[0].clone();
                    stroke.id = "preview".into();
                    if let ObjectKind::Stroke { points, .. } = &mut stroke.kind {
                        points.truncate(50 + frame);
                    }
                    if run == "cpu-baseline" {
                        let mut temporary = page.clone();
                        temporary.objects.push(stroke);
                        if !baseline
                            .as_ref()
                            .is_some_and(|(previous, _)| previous.as_ref() == Some(&temporary))
                        {
                            baseline =
                                Some((Some(temporary.clone()), page_scene(&temporary).unwrap()));
                        }
                        paint_scene(&painter, &baseline.as_ref().unwrap().1, &NoResources);
                    } else {
                        renderer
                            .paint_page_at_revision(
                                &painter,
                                &page,
                                ("bench", 6),
                                false,
                                &NoResources,
                            )
                            .unwrap();
                        paint_object(&painter, &stroke);
                    }
                } else if run == "cpu-baseline" {
                    if baseline.is_none() {
                        baseline = Some((None, page_scene(&page).unwrap()));
                    }
                    paint_scene(&painter, &baseline.as_ref().unwrap().1, &NoResources);
                } else {
                    renderer
                        .paint_page_at_revision(&painter, &page, ("bench", 6), false, &NoResources)
                        .unwrap();
                }
                paint_us = paint_start.elapsed().as_micros();
            });
            output.textures_delta.clear();
            final_shapes = output.shapes.len();
            let primitives = context.tessellate(output.shapes, output.pixels_per_point);
            vertices = primitives
                .iter()
                .map(|p| match &p.primitive {
                    egui::epaint::Primitive::Mesh(mesh) => mesh.vertices.len(),
                    _ => 0,
                })
                .sum::<usize>();
            let elapsed = start.elapsed().as_micros();
            if frame == 0 {
                cold_us = elapsed;
            }
            if frame >= 3 {
                times.push(elapsed);
                paints.push(paint_us);
            }
        }
        times.sort_unstable();
        paints.sort_unstable();
        let body = format!(
            r#"{{"sessionId":"ink-white-lag","runId":"{run}","hypothesisId":"B","location":"render:ink_cpu_benchmark","msg":"[DEBUG] synthetic headless CPU","data":{{"preview":{preview},"points":10200,"strokes":6,"samples":30,"debug_assertions":{},"pixels_per_point":1,"cold_us":{cold_us},"median_total_us":{},"median_paint_us":{},"mean_total_us":{},"shapes":{final_shapes},"vertices":{vertices}}}}}"#,
            cfg!(debug_assertions),
            times[15],
            paints[15],
            times.iter().sum::<u128>() / 30
        );
        if std::env::var_os("INK_CPU_REPORT").is_some() {
            let mut stream = std::net::TcpStream::connect_timeout(
                &"127.0.0.1:7777".parse().unwrap(),
                std::time::Duration::from_secs(2),
            )
            .unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            write!(stream, "POST /event HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            let mut response = String::new();
            std::io::BufRead::read_line(
                &mut std::io::BufReader::new(stream.take(1024)),
                &mut response,
            )
            .unwrap();
            assert!(response.ends_with("\r\n"), "incomplete HTTP status line");
            assert!(
                response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.0 200"),
                "{response}"
            );
        }
        assert!(vertices > 0);
        if run == "cpu-optimized" {
            assert!(
                final_shapes < 200,
                "completed strokes must not emit per-segment shapes"
            );
        }
    }
}
// #endregion

fn assert_display_error(stroke: &StrokeGeometry, scale: f32) -> Vec<usize> {
    let kept = stroke.display_indices(scale);
    assert_eq!(kept.first(), Some(&0));
    assert_eq!(kept.last(), Some(&(stroke.points.len() - 1)));
    for pair in kept.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        let a = stroke.points[start];
        let b = stroke.points[end];
        let dx = f64::from(b.x) - f64::from(a.x);
        let dy = f64::from(b.y) - f64::from(a.y);
        let length_sq = dx * dx + dy * dy;
        let mut previous_t = 0.0;
        for i in start..=end {
            let x = f64::from(stroke.points[i].x) - f64::from(a.x);
            let y = f64::from(stroke.points[i].y) - f64::from(a.y);
            let t = if length_sq == 0.0 {
                0.0
            } else {
                ((x * dx + y * dy) / length_sq).clamp(0.0, 1.0)
            };
            if end > start + 1 {
                assert!(t >= previous_t, "traversal reversed at {i}");
                previous_t = t;
                let distance = (x - t * dx).hypot(y - t * dy) * f64::from(scale);
                let width = f64::from(stroke.widths[start])
                    + t * f64::from(stroke.widths[end] - stroke.widths[start]);
                let width_error = (f64::from(stroke.widths[i]) - width).abs() * f64::from(scale);
                assert!(distance <= 0.25, "sample {i}: {distance}px");
                assert!(width_error <= 0.1, "sample {i}: {width_error}px width");
            }
        }
    }
    kept
}

#[test]
fn display_simplification_bounds_long_paths_corners_pressure_near_points_and_loops() {
    let cases: Vec<Vec<Pos2>> = vec![
        (0..10_000)
            .map(|i| {
                let t = i as f32 / 9999.0;
                egui::pos2(t * 1000.0, (t * 24.0).sin() * 20.0)
            })
            .collect(),
        (0..200)
            .map(|i| {
                if i < 100 {
                    egui::pos2(i as f32 * 0.01, 0.0)
                } else {
                    egui::pos2(0.99, (i - 99) as f32 * 0.01)
                }
            })
            .collect(),
        (0..200)
            .map(|i| egui::pos2(i as f32 * 0.00001, 0.0))
            .collect(),
        (0..1001)
            .map(|i| {
                if i == 1000 {
                    return egui::pos2(20.0, 0.0);
                }
                let t = i as f32 * std::f32::consts::TAU / 1000.0;
                egui::pos2(t.cos() * 20.0, t.sin() * 20.0)
            })
            .collect(),
        (0..1001)
            .map(|i| {
                let t = i as f32 * std::f32::consts::TAU / 1000.0;
                egui::pos2(t.sin() * 20.0, (2.0 * t).sin() * 0.05)
            })
            .collect(),
        vec![
            egui::pos2(0.0, 0.0),
            egui::pos2(1.0, 0.0),
            egui::pos2(9.0, 0.0),
            egui::pos2(2.0, 0.0),
            egui::pos2(10.0, 0.0),
            egui::pos2(11.0, 0.0),
        ],
    ];
    for (case, points) in cases.into_iter().enumerate() {
        let n = points.len();
        let stroke = StrokeGeometry {
            bounds: Rect::from_points(&points),
            points,
            widths: (0..n)
                .map(|i| if case == 2 && i >= n / 2 { 6.0 } else { 2.0 })
                .collect(),
            color: Color {
                a: 128,
                ..Color::default()
            },
        };
        for scale in [0.01, 0.1, 1.0, 2.5, 16.0] {
            let kept = assert_display_error(&stroke, scale);
            if case == 0 {
                assert!(kept.len() < n / 5);
            }
            if case == 1 {
                assert!(kept.contains(&99));
            }
            if case == 2 {
                assert!(kept.contains(&99) && kept.contains(&100));
            }
            if case == 3 {
                for i in [250, 500, 750] {
                    assert!(kept.contains(&i));
                }
                assert_eq!(stroke.points[kept[0]], stroke.points[*kept.last().unwrap()]);
            }
            if case == 5 {
                assert!(kept.contains(&2) && kept.contains(&3));
            }
            let mut mesh = egui::Mesh::default();
            stroke.mesh(
                egui::emath::TSTransform::from_scaling(scale),
                1.0,
                1.0,
                &mut mesh,
            );
            assert!(mesh.is_valid());
            assert!(
                mesh.vertices
                    .iter()
                    .all(|v| v.pos.is_finite() && v.color.a() <= 128)
            );
        }
    }
}

#[test]
fn display_scale_uses_dpi_and_transform_without_mutating_source_or_exports() {
    let page = page(ObjectKind::Stroke {
        points: (0..1000)
            .map(|i| {
                let t = i as f32 / 999.0;
                StrokePoint {
                    x: 10.0 + t * 100.0,
                    y: 30.0 + (t * 8.0).sin() * 10.0,
                    time: i as f64 / 120.0,
                    pressure: if i < 500 { 0.2 } else { 1.0 },
                }
            })
            .collect(),
        style: Style {
            width: 4.0,
            ..Default::default()
        },
    });
    let source = serde_json::to_vec(&page).unwrap();
    let svg = export_svg(&page, 128, 64, false).unwrap();
    let png = export_png(&page, 128, 64, false).unwrap();
    let scene = page_scene(&page).unwrap();
    let Primitive::Stroke(stroke) = &scene.items[0] else {
        panic!("stroke expected")
    };
    assert_eq!(stroke.points.len(), 1000);
    let mut counts = Vec::new();
    for (scale, dpi) in [(0.1, 1.0), (1.0, 1.0), (1.0, 4.0), (2.0, 2.0)] {
        let indices = assert_display_error(stroke, scale * dpi);
        assert!(indices.contains(&499) && indices.contains(&500));
        let mut mesh = egui::Mesh::default();
        stroke.mesh(
            egui::emath::TSTransform::from_scaling(scale),
            dpi,
            0.0,
            &mut mesh,
        );
        assert_eq!(mesh.vertices.len(), indices.len() * 4 + 38);
        counts.push(mesh.vertices.len());
    }
    assert!(counts[0] < counts[1] && counts[1] < counts[2]);
    assert_eq!(counts[2], counts[3]);
    assert_eq!(source, serde_json::to_vec(&page).unwrap());
    assert_eq!(svg, export_svg(&page, 128, 64, false).unwrap());
    assert_eq!(png, export_png(&page, 128, 64, false).unwrap());
}

#[test]
fn display_simplification_keeps_dense_translucent_strip_single_coverage() {
    let stroke = StrokeGeometry {
        points: (0..1000)
            .map(|i| egui::pos2(20.0 + i as f32 * 0.1, 20.0))
            .collect(),
        widths: vec![6.0; 1000],
        bounds: Rect::EVERYTHING,
        color: Color {
            a: 128,
            ..Color::default()
        },
    };
    let mut mesh = egui::Mesh::default();
    stroke.mesh(egui::emath::TSTransform::IDENTITY, 1.0, 0.0, &mut mesh);
    assert_eq!(mesh.vertices.len(), 4 * 4 + 38);
    let cross = |a: Vec2, b: Vec2| a.x * b.y - a.y * b.x;
    for x in 18..122 {
        for y in 16..24 {
            let p = egui::pos2(x as f32 + 0.37, y as f32 + 0.23);
            let coverage = mesh
                .indices
                .chunks_exact(3)
                .filter(|t| {
                    let a = mesh.vertices[t[0] as usize].pos;
                    let b = mesh.vertices[t[1] as usize].pos;
                    let c = mesh.vertices[t[2] as usize].pos;
                    let signs = [
                        cross(b - a, p - a),
                        cross(c - b, p - b),
                        cross(a - c, p - c),
                    ];
                    signs.iter().all(|s| *s > 1e-5) || signs.iter().all(|s| *s < -1e-5)
                })
                .count();
            assert!(coverage <= 1, "alpha overlap at {p:?}");
        }
    }
}

#[test]
fn generated_geometry_budget_counts_samples_before_deduplication() {
    for repeated in [true, false] {
        let mut scene = Scene {
            stroke_points: MAX_INPUT_POINTS * 2 - 12,
            ..Default::default()
        };
        let points = (0..4)
            .map(|i| egui::pos2(if repeated { 0.0 } else { i as f32 }, 0.0))
            .collect();
        assert!(matches!(
            scene.stroke(points, vec![2.0; 4], Color::default()),
            Err(RenderError::ResourceLimit(_))
        ));
        assert!(scene.items.is_empty());
    }
}

#[test]
fn benchmark_http_status_does_not_wait_for_body_or_eof() {
    use std::io::{BufRead, BufReader, Read};
    struct KeepAlive {
        status: std::io::Cursor<&'static [u8]>,
    }
    impl Read for KeepAlive {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.status.read(buf)?;
            if n == 0 {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            Ok(n)
        }
    }
    let socket = KeepAlive {
        status: std::io::Cursor::new(b"HTTP/1.1 200 OK\r\n"),
    };
    let mut status = String::new();
    BufReader::new(socket.take(1024))
        .read_line(&mut status)
        .unwrap();
    assert_eq!(status, "HTTP/1.1 200 OK\r\n");
}

#[test]
fn compact_strokes_reduce_1500_by_128_geometry() {
    let mut seed = 0x83a4_7349_u32;
    let mut next = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / 16_777_215.0
    };
    let mut scene = Scene::default();
    for stroke in 0..1500 {
        let x = 20.0 + next() * 1720.0;
        let y = 40.0 + next() * 1000.0;
        let length = 80.0 + next() * 100.0;
        let phase = next() * std::f32::consts::TAU;
        append_object(
            &mut scene,
            &BoardObject {
                id: format!("fullscreen-stroke-{stroke}"),
                kind: ObjectKind::Stroke {
                    points: (0..128)
                        .map(|i| {
                            let t = i as f32 / 127.0;
                            StrokePoint {
                                x: x + t * length,
                                y: y + (t * std::f32::consts::TAU + phase).sin() * 18.0,
                                time: i as f64 / 120.0,
                                pressure: 0.55 + (t * 8.0 + phase).sin() * 0.35,
                            }
                        })
                        .collect(),
                    style: Style {
                        width: 3.0,
                        ..Default::default()
                    },
                },
            },
        )
        .unwrap();
    }
    assert_eq!(scene.items.len(), 1500);
    let mut vertices = 0;
    let mut triangles = 0;
    let mut full_vertices = 0;
    let mut thumbnail_vertices = 0;
    for item in &scene.items {
        let Primitive::Stroke(stroke) = item else {
            panic!("compact stroke expected")
        };
        let mut mesh = egui::Mesh::default();
        stroke.mesh(egui::emath::TSTransform::IDENTITY, 1.0, 1.0, &mut mesh);
        assert!(mesh.is_valid());
        assert!(mesh.vertices.iter().all(|v| v.pos.is_finite()));
        vertices += mesh.vertices.len();
        triangles += mesh.indices.len() / 3;
        assert_display_error(stroke, 1.0);
        assert_display_error(stroke, 0.1);
        full_vertices += stroke.points.len() * 4 + 38;
        let mut thumbnail = egui::Mesh::default();
        stroke.mesh(
            egui::emath::TSTransform::from_scaling(0.1),
            1.0,
            1.0,
            &mut thumbnail,
        );
        thumbnail_vertices += thumbnail.vertices.len();
    }
    assert_eq!(full_vertices, 825_000);
    assert!(vertices < full_vertices / 3);
    assert!(thumbnail_vertices < vertices);
    eprintln!(
        "display full={full_vertices}, simplified={vertices}, reduction={:.2}%, thumbnail_0.1={thumbnail_vertices}",
        100.0 * (1.0 - vertices as f64 / full_vertices as f64)
    );
    let old_vertices = 1500 * 127 * 36;
    let old_triangles = 1500 * 127 * 52;
    assert!(vertices < old_vertices / 5);
    assert!(triangles < old_triangles / 5);
    eprintln!(
        "1500x128: vertices={vertices} (capsules={old_vertices}), triangles={triangles} (capsules={old_triangles})"
    );
}

#[test]
fn variable_strip_preserves_centers_pressure_caps_and_export_alpha() {
    let input: Vec<_> = (0..5)
        .map(|i| StrokePoint {
            x: 20.0 + i as f32 * 20.0,
            y: 30.0,
            time: i as f64,
            pressure: if i % 2 == 0 { 1.0 } else { 0.1 },
        })
        .collect();
    let page = page(ObjectKind::Stroke {
        points: input.clone(),
        style: Style {
            width: 12.0,
            color: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 128,
            },
            dashed: false,
        },
    });
    let scene = page_scene(&page).unwrap();
    let [Primitive::Stroke(stroke)] = scene.items.as_slice() else {
        panic!("one stroke expected")
    };
    assert_eq!(
        stroke.points,
        input
            .iter()
            .map(|p| egui::pos2(p.x, p.y))
            .collect::<Vec<_>>()
    );
    assert!(stroke.widths[2] > stroke.widths[1] * 1.5);
    let outline = stroke.outline();
    assert!(outline.iter().any(|p| p.x < 15.0));
    assert!(outline.iter().any(|p| p.x > 105.0));
    let image = tiny_skia::Pixmap::decode_png(&export_png(&page, 128, 64, false).unwrap()).unwrap();
    for x in 18..103 {
        assert!(
            (126..=128).contains(&image.pixel(x, 30).unwrap().red()),
            "alpha at {x}"
        );
    }
    assert!(image.pixel(60, 34).unwrap().red() < 200);
    assert_eq!(image.pixel(40, 34).unwrap().red(), 255);
    let svg = export_svg(&page, 128, 64, false).unwrap();
    assert_eq!(svg.matches("<path ").count(), 1);
    assert!(svg.contains("fill-opacity=\"0.5019608\""));
    assert!(!svg.contains("stroke-width"));
    let mut mesh = egui::Mesh::default();
    stroke.mesh(egui::emath::TSTransform::IDENTITY, 1.0, 0.0, &mut mesh);
    for i in 0..input.len() {
        assert!(
            (mesh.vertices[i * 4 + 1]
                .pos
                .distance(mesh.vertices[i * 4 + 2].pos)
                - stroke.widths[i])
                .abs()
                < 1e-4
        );
    }
}

#[test]
fn pressure_one_still_keeps_velocity_width_and_strip_has_no_join_overdraw() {
    let points = vec![
        StrokePoint {
            x: 20.0,
            y: 20.0,
            time: 0.0,
            pressure: 1.0,
        },
        StrokePoint {
            x: 80.0,
            y: 20.0,
            time: 1.0,
            pressure: 1.0,
        },
        StrokePoint {
            x: 80.0,
            y: 80.0,
            time: 1.01,
            pressure: 1.0,
        },
    ];
    let scene = page_scene(&page(ObjectKind::Stroke {
        points,
        style: Style {
            width: 10.0,
            color: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 128,
            },
            dashed: false,
        },
    }))
    .unwrap();
    let Primitive::Stroke(stroke) = &scene.items[0] else {
        panic!("stroke expected")
    };
    assert!(stroke.widths[1] > stroke.widths[2] * 2.0);
    let mut mesh = egui::Mesh::default();
    stroke.mesh(egui::emath::TSTransform::IDENTITY, 1.0, 0.0, &mut mesh);
    let cross = |a: Vec2, b: Vec2| a.x * b.y - a.y * b.x;
    let coverage = |p: Pos2| {
        mesh.indices
            .chunks_exact(3)
            .filter(|triangle| {
                let a = mesh.vertices[triangle[0] as usize].pos;
                let b = mesh.vertices[triangle[1] as usize].pos;
                let c = mesh.vertices[triangle[2] as usize].pos;
                let signs = [
                    cross(b - a, p - a),
                    cross(c - b, p - b),
                    cross(a - c, p - c),
                ];
                signs.iter().all(|s| *s > 1e-4) || signs.iter().all(|s| *s < -1e-4)
            })
            .count()
    };
    for y in 15..28 {
        for x in 73..86 {
            assert!(coverage(egui::pos2(x as f32 + 0.37, y as f32 + 0.23)) <= 1);
        }
    }
}

#[test]
fn sharp_duplicate_and_reversing_stroke_geometry_is_bounded() {
    for points in [
        vec![
            egui::pos2(20.0, 20.0),
            egui::pos2(40.0, 20.0),
            egui::pos2(20.0, 20.001),
        ],
        vec![
            egui::pos2(20.0, 20.0),
            egui::pos2(40.0, 20.0),
            egui::pos2(40.0, 20.0),
            egui::pos2(40.0, 40.0),
        ],
    ] {
        let mut scene = Scene::default();
        scene
            .stroke(points.clone(), vec![6.0; points.len()], Color::default())
            .unwrap();
        let Primitive::Stroke(stroke) = &scene.items[0] else {
            panic!("stroke expected")
        };
        let mut mesh = egui::Mesh::default();
        stroke.mesh(egui::emath::TSTransform::IDENTITY, 1.0, 1.0, &mut mesh);
        assert!(mesh.is_valid());
        assert!(
            mesh.vertices
                .iter()
                .all(|v| v.pos.is_finite() && stroke.bounds.expand(1.0).contains(v.pos))
        );
    }
    let mut scene = Scene::default();
    scene
        .stroke(vec![Pos2::ZERO; 3], vec![1.0, 4.0, 2.0], Color::default())
        .unwrap();
    assert!(matches!(
        scene.items.as_slice(),
        [Primitive::Disk(_, 2.0, _)]
    ));
}

#[test]
fn cached_strokes_match_uncached_triangles_across_dpi_clip_and_alpha() {
    let mut page = ink_benchmark_page();
    for (i, object) in page.objects.iter_mut().enumerate() {
        if let ObjectKind::Stroke { points, style } = &mut object.kind {
            points.truncate(40);
            points[20].y += 20.0;
            points[21] = points[20];
            style.dashed = i % 2 == 0;
            style.color.a = if i % 2 == 0 { 255 } else { 128 };
        }
    }
    page.objects.insert(
        2,
        object(ObjectKind::Image {
            position: p(50.0, 50.0),
            width: 40.0,
            height: 40.0,
            asset_ref: "asset:test".into(),
        }),
    );
    page.objects.insert(
        4,
        object(ObjectKind::Text {
            position: p(50.0, 100.0),
            text: "Z-order".into(),
            size: 18.0,
            color: Color::default(),
        }),
    );
    let mut invisible = page.objects[0].clone();
    if let ObjectKind::Stroke { points, .. } = &mut invisible.kind {
        for point in points {
            point.x += 5000.0;
        }
    }
    page.objects.insert(1, invisible);
    page.objects.push(object(ObjectKind::FunctionPlot {
        position: p(80.0, 80.0),
        width: 100.0,
        height: 80.0,
        expressions: vec!["x".into()],
        x_min: -2.0,
        x_max: 2.0,
        y_min: -1.0,
        y_max: 1.0,
    }));
    for (i, object) in page.objects.iter_mut().enumerate() {
        object.id = format!("mixed-{i}");
    }
    let context = egui::Context::default();
    let mut renderer = PageRenderer::new();
    for dpi in [1.0, 2.0, 1.25] {
        context.set_pixels_per_point(dpi);
        for clip in [
            Rect::from_min_max(Pos2::ZERO, egui::pos2(1200.0, 800.0)),
            Rect::from_min_max(egui::pos2(55.0, 85.0), egui::pos2(160.0, 220.0)),
        ] {
            for texture in [egui::TextureId::User(1), egui::TextureId::User(2)] {
                for feathering in [true, false] {
                    context.tessellation_options_mut(|options| options.feathering = feathering);
                    let resources = |_: &str| Some(texture);
                    let mut draw = |cached: bool| {
                        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
                            let painter = ui
                                .ctx()
                                .layer_painter(egui::LayerId::background())
                                .with_clip_rect(clip);
                            if cached {
                                renderer
                                    .paint_page_at_revision(
                                        &painter,
                                        &page,
                                        ("doc", 1),
                                        false,
                                        &resources,
                                    )
                                    .unwrap();
                            } else {
                                paint_page_with_resources(&painter, &page, false, &resources);
                            }
                        });
                        output.textures_delta.clear();
                        context.tessellate(output.shapes, output.pixels_per_point)
                    };
                    // Warm up fonts/DPI before comparing final, post-tessellation triangles.
                    draw(false);
                    let expected = draw(false);
                    let actual = draw(true);
                    assert_eq!(
                        format!("{actual:?}"),
                        format!("{expected:?}"),
                        "dpi={dpi}, clip={clip:?}"
                    );
                    let mesh = renderer.objects[&page.objects[0].id]
                        .meshes
                        .as_ref()
                        .unwrap()
                        .batches[0]
                        .2
                        .clone();
                    let mut output = context.run_ui(egui::RawInput::default(), |ui| {
                        let painter = ui
                            .ctx()
                            .layer_painter(egui::LayerId::background())
                            .with_clip_rect(clip);
                        renderer
                            .paint_page_at_revision(&painter, &page, ("doc", 1), false, &resources)
                            .unwrap();
                        paint_object(&painter, &page.objects[0]);
                    });
                    output.textures_delta.clear();
                    assert!(std::sync::Arc::ptr_eq(
                        &mesh,
                        &renderer.objects[&page.objects[0].id]
                            .meshes
                            .as_ref()
                            .unwrap()
                            .batches[0]
                            .2
                    ));
                }
            }
        }
    }
}

#[test]
fn revision_mesh_cache_is_reused_and_cannot_leak_across_documents_or_undo() {
    let context = egui::Context::default();
    let page = ink_benchmark_page();
    let empty = Page {
        id: page.id.clone(),
        objects: Vec::new(),
    };
    let mut renderer = PageRenderer::new();
    let mut output = context.run_ui(egui::RawInput::default(), |ui| {
        let painter = ui
            .ctx()
            .layer_painter(egui::LayerId::background())
            .with_clip_rect(Rect::from_min_size(Pos2::ZERO, egui::vec2(1200.0, 800.0)));
        renderer
            .paint_page_at_revision(&painter, &page, ("doc", 6), false, &NoResources)
            .unwrap();
        assert_eq!(renderer.objects.len(), page.objects.len());
        let meshes = renderer.objects[&page.objects[0].id]
            .meshes
            .as_ref()
            .unwrap();
        assert_eq!(meshes.batches.len(), 1);
        assert_eq!(meshes.batches[0].1, 1);
        assert!(meshes.batches[0].2.vertices.len() > 1_000);
        let mesh = meshes.batches[0].2.clone();
        renderer
            .paint_page_at_revision(&painter, &page, ("doc", 6), false, &NoResources)
            .unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &mesh,
            &renderer.objects[&page.objects[0].id]
                .meshes
                .as_ref()
                .unwrap()
                .batches[0]
                .2
        ));
        renderer
            .paint_page_at_revision(&painter, &empty, ("other-doc", 6), false, &NoResources)
            .unwrap();
        assert!(renderer.objects.is_empty());
        renderer
            .paint_page_at_revision(&painter, &page, ("other-doc", 7), false, &NoResources)
            .unwrap();
        renderer
            .paint_page_at_revision(&painter, &empty, ("other-doc", 8), false, &NoResources)
            .unwrap();
        assert!(renderer.objects.is_empty());
    });
    assert!(output.shapes.len() < 30);
    output.textures_delta.clear();
}

fn ink_benchmark_page() -> Page {
    Page {
        id: "synthetic".into(),
        objects: (0..6)
            .map(|stroke| BoardObject {
                id: format!("stroke-{stroke}"),
                kind: ObjectKind::Stroke {
                    points: (0..1700)
                        .map(|i| StrokePoint {
                            x: 40.0 + i as f32 * 0.6,
                            y: 80.0 + stroke as f32 * 100.0 + (i as f32 * 0.09).sin() * 35.0,
                            time: i as f64 * 0.002,
                            pressure: 0.5 + (i as f32 * 0.013).sin() * 0.45,
                        })
                        .collect(),
                    style: Style {
                        width: 5.0,
                        color: Color {
                            r: 255,
                            g: 255,
                            b: 255,
                            a: 128,
                        },
                        dashed: false,
                    },
                },
            })
            .collect(),
    }
}

fn object(kind: ObjectKind) -> BoardObject {
    BoardObject {
        id: "test".into(),
        kind,
    }
}
fn page(kind: ObjectKind) -> Page {
    Page {
        id: "page".into(),
        objects: vec![object(kind)],
    }
}
fn shape(kind: ShapeKind, points: Vec<Point>) -> ObjectKind {
    ObjectKind::Shape {
        shape: kind,
        points,
        style: Style::default(),
    }
}
fn p(x: f32, y: f32) -> Point {
    Point { x, y }
}

#[test]
fn million_point_core_document_and_large_object_page_render() {
    let points = (0..board_core::MAX_DOCUMENT_POINTS)
        .map(|i| StrokePoint {
            x: 2.0 + i as f32 / 32768.0,
            y: 8.0,
            time: 0.0,
            pressure: 1.0,
        })
        .collect();
    let page = page(ObjectKind::Stroke {
        points,
        style: Style::default(),
    });
    let mut document = board_core::Document::new();
    document.pages = vec![page];
    document.validate().unwrap();
    let page = &document.pages[0];
    assert_eq!(page_scene(page).unwrap().items.len(), 1);
    assert!(export_svg(page, 40, 16, false).unwrap().contains("<path"));
    let image = tiny_skia::Pixmap::decode_png(&export_png(page, 40, 16, false).unwrap()).unwrap();
    assert!(image.pixel(16, 8).unwrap().red() < 10);
    let many = Page {
        id: "many".into(),
        objects: (0..10_001)
            .map(|i| BoardObject {
                id: format!("o{i}"),
                kind: shape(ShapeKind::Line, vec![p(2.0, 2.0), p(4.0, 4.0)]),
            })
            .collect(),
    };
    assert_eq!(page_scene(&many).unwrap().items.len(), 10_001);
}

#[test]
fn random_geometry_bbox_and_vertices_are_identical() {
    use ShapeKind::*;
    let mut seed = 0x83a4_7349_u32;
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
        for _ in 0..12 {
            let mut next = || {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed % 50) as f32 + 5.0
            };
            let a = p(next(), next());
            let b = p(next(), next());
            let bbox = page(shape(kind, vec![a, b]));
            let g = board_ink::shape_geometry(kind, a, b).unwrap();
            let explicit = page(shape(kind, g.vertices));
            assert_eq!(
                export_svg(&bbox, 64, 64, false).unwrap(),
                export_svg(&explicit, 64, 64, false).unwrap()
            );
            assert_eq!(
                export_png(&bbox, 64, 64, false).unwrap(),
                export_png(&explicit, 64, 64, false).unwrap()
            );
        }
    }
}

#[test]
fn connected_3d_line_endpoint_matches_rendered_vertex_after_edit() {
    for kind in [
        ShapeKind::Cube,
        ShapeKind::Cuboid,
        ShapeKind::Cylinder,
        ShapeKind::Cone,
        ShapeKind::Sphere,
    ] {
        let mut document = board_core::Document::new();
        let page_id = document.pages[0].id.clone();
        let g = board_ink::shape_geometry(kind, p(10.0, 10.0), p(50.0, 50.0)).unwrap();
        document.pages[0].objects = vec![
            BoardObject {
                id: "target".into(),
                kind: shape(kind, g.vertices.clone()),
            },
            BoardObject {
                id: "line".into(),
                kind: shape(ShapeKind::Line, vec![p(0.0, 0.0), p(60.0, 60.0)]),
            },
        ];
        document
            .connect(
                &page_id,
                0,
                board_core::Connection {
                    id: "connection".into(),
                    page_id: page_id.clone(),
                    line_id: "line".into(),
                    line_endpoint: 0,
                    target_id: "target".into(),
                    target: board_core::Anchor::Vertex { index: 0 },
                },
            )
            .unwrap();
        let mut vertices = g.vertices;
        vertices[0].x += 3.0;
        let expected = pos(vertices[0]);
        document
            .apply(
                &page_id,
                document.revision,
                &[board_core::Operation::Update {
                    object: BoardObject {
                        id: "target".into(),
                        kind: shape(kind, vertices),
                    },
                }],
            )
            .unwrap();
        let scene = page_scene(&document.pages[0]).unwrap();
        assert!(matches!(scene.items.last(), Some(Primitive::Line(a, _, _, _)) if *a == expected));
        assert!(matches!(scene.items.first(), Some(Primitive::Line(a, _, _, _)) if *a == expected));
        assert!(export_svg(&document.pages[0], 64, 64, false).is_ok());
    }
}

#[test]
fn one_pixel_empty_alpha_dashes_and_round_caps() {
    let empty = export_png(&Page::new(), 1, 1, false).unwrap();
    assert_eq!(
        tiny_skia::Pixmap::decode_png(&empty)
            .unwrap()
            .pixel(0, 0)
            .unwrap()
            .red(),
        255
    );
    let dot = page(shape(ShapeKind::Line, vec![p(0.5, 0.5), p(0.5, 0.5)]));
    assert!(
        tiny_skia::Pixmap::decode_png(&export_png(&dot, 1, 1, false).unwrap())
            .unwrap()
            .pixel(0, 0)
            .unwrap()
            .red()
            < 10
    );
    let page = page(ObjectKind::Shape {
        shape: ShapeKind::Line,
        points: vec![p(5.0, 8.0), p(60.0, 8.0)],
        style: Style {
            width: 4.0,
            dashed: true,
            color: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 128,
            },
        },
    });
    let image = tiny_skia::Pixmap::decode_png(&export_png(&page, 64, 16, false).unwrap()).unwrap();
    assert!((126..=128).contains(&image.pixel(10, 8).unwrap().red()));
    assert_eq!(image.pixel(21, 8).unwrap().red(), 255);
    assert!(image.pixel(4, 8).unwrap().red() < 255);
    let context = egui::Context::default();
    let mut output = context.run_ui(egui::RawInput::default(), |ui| {
        paint_object(ui.painter(), &page.objects[0])
    });
    output.textures_delta.clear();
    assert!(output.shapes.iter().any(
        |s| matches!(&s.shape, egui::Shape::Path(path) if path.closed && path.fill.a() == 128)
    ));
}

#[test]
fn clip_stroke_coverage_and_extreme_expressions() {
    let mut plot = page(ObjectKind::FunctionPlot {
        position: p(10.0, 10.0),
        width: 20.0,
        height: 20.0,
        expressions: vec!["1".into()],
        x_min: -1.0,
        x_max: 1.0,
        y_min: -1.0,
        y_max: 1.0,
    });
    let image = tiny_skia::Pixmap::decode_png(&export_png(&plot, 40, 40, false).unwrap()).unwrap();
    let above = image.pixel(20, 9).unwrap();
    assert!(
        above.red() > 128,
        "曲线圆帽不能越过函数框，框线抗锯齿仍可见"
    );
    assert!(
        export_svg(&plot, 40, 40, false)
            .unwrap()
            .contains("clip-path=\"url(#plot-")
    );
    if let ObjectKind::FunctionPlot { expressions, .. } = &mut plot.objects[0].kind {
        *expressions = vec![format!("{}x{}", "(".repeat(2000), ")".repeat(2000))];
    }
    assert!(matches!(page_scene(&plot), Err(RenderError::Math(_))));
    let a = board_math::Point {
        x: -1.0,
        y: -f64::MAX,
    };
    let b = board_math::Point {
        x: 1.0,
        y: f64::MAX,
    };
    let (a, b) = clip_y(a, b, -1.0, 1.0).unwrap();
    assert!(a.x.is_finite() && b.x.is_finite());
    assert_eq!((a.y, b.y), (-1.0, 1.0));
}

#[test]
fn signatures_and_software_pixels() {
    let page = page(shape(
        ShapeKind::Rectangle,
        vec![p(5.0, 5.0), p(25.0, 25.0)],
    ));
    let svg = export_svg(&page, 32, 32, false).unwrap();
    assert!(svg.starts_with("<svg xmlns="));
    assert!(svg.ends_with("</svg>"));
    let png = export_png(&page, 32, 32, false).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let decoded = tiny_skia::Pixmap::decode_png(&png).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (32, 32));
    assert_eq!(decoded.pixel(0, 0).unwrap().red(), 255);
    assert!(decoded.pixel(5, 15).unwrap().red() < 50);
}

#[test]
fn xml_escaping_and_png_text_is_explicit() {
    let page = page(ObjectKind::Text {
        position: p(0.0, 0.0),
        text: "中文<&>\"'\n第二行".into(),
        size: 16.0,
        color: Color::default(),
    });
    let svg = export_svg(&page, 128, 128, false).unwrap();
    assert!(svg.contains("中文&lt;&amp;&gt;&quot;&apos;"));
    assert_eq!(svg.matches("<tspan").count(), 2);
    assert!(matches!(
        export_png(&page, 128, 128, false),
        Err(RenderError::MissingFont)
    ));
    let invalid = page_with_invalid_xml();
    assert!(matches!(
        export_svg(&invalid, 32, 32, false),
        Err(RenderError::InvalidObject(_))
    ));
}
fn page_with_invalid_xml() -> Page {
    page(ObjectKind::Text {
        position: p(0.0, 0.0),
        text: "\0".into(),
        size: 16.0,
        color: Color::default(),
    })
}

#[test]
fn bounds_use_generated_or_edited_vertices() {
    let square = object(shape(
        ShapeKind::Square,
        vec![p(10.0, 20.0), p(110.0, 70.0)],
    ));
    assert_eq!(
        object_bounds(&square),
        Rect::from_min_max(egui::pos2(8.5, 18.5), egui::pos2(61.5, 71.5))
    );
    let edited = object(shape(
        ShapeKind::Rectangle,
        vec![p(-20.0, 10.0), p(40.0, 20.0), p(50.0, 60.0), p(0.0, 80.0)],
    ));
    assert_eq!(
        object_bounds(&edited),
        Rect::from_min_max(egui::pos2(-21.5, 8.5), egui::pos2(51.5, 81.5))
    );
    let mut scene = Scene::default();
    append_object(&mut scene, &edited).unwrap();
    assert_eq!(scene.items.len(), 4);
    assert!(
        matches!(scene.items.last().unwrap(), Primitive::Line(a, b, _, _) if *a == egui::pos2(0.0, 80.0) && *b == egui::pos2(-20.0, 10.0))
    );
}

#[test]
fn wireframes_keep_topology_after_editing() {
    for kind in [
        ShapeKind::Cube,
        ShapeKind::Cuboid,
        ShapeKind::Cylinder,
        ShapeKind::Cone,
        ShapeKind::Sphere,
    ] {
        let mut geometry = board_ink::shape_geometry(kind, p(0.0, 0.0), p(80.0, 80.0)).unwrap();
        geometry.vertices[0].x -= 4.0;
        let obj = object(shape(kind, geometry.vertices));
        let mut scene = Scene::default();
        append_object(&mut scene, &obj).unwrap();
        assert_eq!(scene.items.len(), geometry.edges.len());
    }
    let invalid = page(shape(
        ShapeKind::Cube,
        vec![p(0.0, 0.0), p(1.0, 1.0), p(2.0, 2.0)],
    ));
    assert!(export_svg(&invalid, 32, 32, false).is_err());
}

fn plot_page(expression: &str) -> Page {
    page(ObjectKind::FunctionPlot {
        position: p(20.0, 20.0),
        width: 200.0,
        height: 200.0,
        expressions: vec![expression.into()],
        x_min: -5.0,
        x_max: 5.0,
        y_min: -5.0,
        y_max: 5.0,
    })
}

fn plot_strokes(scene: &Scene) -> Vec<&StrokeGeometry> {
    scene
        .items
        .iter()
        .filter_map(|item| match item {
            Primitive::Stroke(stroke) => Some(stroke.as_ref()),
            _ => None,
        })
        .collect()
}

#[test]
fn plot_samples_reuse_math_geometry_through_transforms_clip_dpi_and_reorder() {
    let ctx = egui::Context::default();
    let mut renderer = PageRenderer::new();
    let mut page = plot_page("x");
    for (i, expression) in [
        "y^2=x",
        "x^2+y^2=9",
        "(x-1)^2/9+(y+1)^2/4=1",
        "x*y=1",
        "(x-1)^2+(y+2)^2=0",
        "x^2+y^2=-1",
    ]
    .iter()
    .enumerate()
    {
        let mut object = plot_page(expression).objects.remove(0);
        object.id = format!("plot-{i}");
        page.objects.push(object);
    }
    let expected_calls = page.objects.len() as u64;
    for frame in 0..12 {
        if frame > 0 {
            for object in &mut page.objects {
                if let ObjectKind::FunctionPlot {
                    position,
                    width,
                    height,
                    ..
                } = &mut object.kind
                {
                    position.x += 3.0;
                    position.y += 2.0;
                    *width += 2.0;
                    *height += 1.0;
                }
            }
            page.objects.reverse();
        }
        ctx.set_pixels_per_point(if frame % 2 == 0 { 1.0 } else { 2.0 });
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.painter().with_clip_rect(Rect::from_min_size(
                Pos2::new(frame as f32, 0.0),
                Vec2::new(600.0, 500.0),
            ));
            if frame == 0 || frame == 11 {
                renderer
                    .paint_page_at_revision(&painter, &page, ("doc", frame), false, &NoResources)
                    .unwrap();
            } else {
                renderer
                    .paint_page_with_resources(&painter, &page, false, &NoResources)
                    .unwrap();
            }
        });
        output.textures_delta.clear();
        assert_eq!(renderer.plot_sample_calls(), expected_calls);
        for object in &page.objects {
            let expected = page_scene(&Page {
                objects: vec![object.clone()],
                ..page.clone()
            })
            .unwrap();
            let actual = &renderer.objects[&object.id];
            assert_eq!(
                format!("{:?}", actual.scene.items),
                format!("{:?}", expected.items)
            );
            assert!(actual.bounds.contains(object_bounds(object).center()));
        }
    }
    assert_eq!(renderer.plot_samples.entries.len(), expected_calls as usize);
    assert!(
        renderer
            .plot_samples
            .entries
            .iter()
            .map(|entry| entry.bytes)
            .sum::<usize>()
            < MAX_PLOT_SAMPLE_BYTES
    );
}

#[test]
fn plot_samples_invalidate_sources_bounds_errors_deletion_and_document_reset() {
    let ctx = egui::Context::default();
    let mut renderer = PageRenderer::new();
    let mut page = plot_page("x");
    if let ObjectKind::FunctionPlot { expressions, .. } = &mut page.objects[0].kind {
        expressions.push("x^2".into());
    }
    let paint = |renderer: &mut PageRenderer, page: &Page, document: &str| {
        let mut result = Ok(());
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            result = renderer.paint_page_at_revision(
                ui.painter(),
                page,
                (document, 1),
                false,
                &NoResources,
            );
        });
        output.textures_delta.clear();
        result
    };
    paint(&mut renderer, &page, "doc").unwrap();
    assert_eq!(renderer.plot_sample_calls(), 2);
    if let ObjectKind::FunctionPlot { expressions, .. } = &mut page.objects[0].kind {
        expressions.reverse();
    }
    paint(&mut renderer, &page, "doc").unwrap();
    assert_eq!(renderer.plot_sample_calls(), 4);
    for axis in 0..4 {
        if let ObjectKind::FunctionPlot {
            x_min,
            x_max,
            y_min,
            y_max,
            ..
        } = &mut page.objects[0].kind
        {
            *[x_min, x_max, y_min, y_max][axis] += 0.25;
        }
        paint(&mut renderer, &page, "doc").unwrap();
        assert_eq!(renderer.plot_sample_calls(), 6 + axis as u64 * 2);
    }
    let valid = page.clone();
    if let ObjectKind::FunctionPlot { expressions, .. } = &mut page.objects[0].kind {
        expressions[0] = "sin(y)=x".into();
    }
    assert!(matches!(
        paint(&mut renderer, &page, "doc"),
        Err(RenderError::Math(_))
    ));
    assert!(renderer.objects.is_empty());
    assert!(renderer.plot_samples.entries.is_empty());
    page = valid;
    paint(&mut renderer, &page, "doc").unwrap();
    assert_eq!(renderer.plot_sample_calls(), 15);
    let saved = page.objects.clone();
    page.objects.clear();
    paint(&mut renderer, &page, "doc").unwrap();
    assert!(renderer.plot_samples.entries.is_empty());
    page.objects = saved;
    paint(&mut renderer, &page, "doc").unwrap();
    assert_eq!(renderer.plot_sample_calls(), 17);
    paint(&mut renderer, &page, "other-doc").unwrap();
    assert_eq!(renderer.plot_sample_calls(), 19);
    renderer.clear();
    assert!(renderer.plot_samples.entries.is_empty());
    paint(&mut renderer, &page, "other-doc").unwrap();
    assert_eq!(renderer.plot_sample_calls(), 21);
}

#[test]
fn plot_sample_cache_steps_and_limits_are_bounded() {
    let mut cache = PlotSamplesCache::default();
    let bounds = board_math::Bounds2D {
        x_min: -5.0,
        x_max: 5.0,
        y_min: -5.0,
        y_max: 5.0,
    };
    cache.samples(&["x".into()], bounds, 512).unwrap();
    cache.samples(&["x".into()], bounds, 1024).unwrap();
    assert_eq!(cache.sample_calls, 2);
    for i in 2..MAX_PLOT_SAMPLE_ENTRIES {
        cache.samples(&[i.to_string()], bounds, 512).unwrap();
    }
    assert!(matches!(
        cache.samples(&["x+1".into()], bounds, 512),
        Err(RenderError::ResourceLimit(_))
    ));
    assert_eq!(cache.entries.len(), MAX_PLOT_SAMPLE_ENTRIES);
    cache.entries.truncate(1);
    cache.entries[0].bytes = MAX_PLOT_SAMPLE_BYTES;
    let before = cache.sample_calls;
    assert!(matches!(
        cache.samples(&["x+1".into()], bounds, 512),
        Err(RenderError::ResourceLimit(_))
    ));
    assert_eq!(cache.sample_calls, before);
    assert_eq!(cache.entries.len(), 1);
}

#[test]
fn implicit_plot_scene_preserves_full_equations_and_closed_conics() {
    for (equation, center, radii) in [
        ("x^2+y^2=9", (0.0, 0.0), (3.0, 3.0)),
        ("(x-1)^2/9+(y+1)^2/4=1", (1.0, -1.0), (3.0, 2.0)),
    ] {
        let scene = page_scene(&plot_page(equation)).unwrap();
        let strokes = plot_strokes(&scene);
        assert_eq!(strokes.len(), 1);
        let points = &strokes[0].points;
        assert_eq!(points.first(), points.last());
        assert!(points.len() >= 512);
        let bounds = Rect::from_points(points);
        assert!((bounds.left() - (120.0 + 20.0 * (center.0 - radii.0))).abs() < 0.01);
        assert!((bounds.right() - (120.0 + 20.0 * (center.0 + radii.0))).abs() < 0.01);
        assert!((bounds.top() - (120.0 - 20.0 * (center.1 + radii.1))).abs() < 0.01);
        assert!((bounds.bottom() - (120.0 - 20.0 * (center.1 - radii.1))).abs() < 0.01);
        for point in points {
            let x = (point.x - 120.0) / 20.0;
            let y = (120.0 - point.y) / 20.0;
            assert!(
                (((x - center.0) / radii.0).powi(2) + ((y - center.1) / radii.1).powi(2) - 1.0)
                    .abs()
                    < 0.0001
            );
        }
    }
    let scene = page_scene(&plot_page("y^2=x")).unwrap();
    let strokes = plot_strokes(&scene);
    let points: Vec<_> = strokes.iter().flat_map(|s| &s.points).collect();
    assert!(points.iter().any(|p| p.y < 85.0));
    assert!(points.iter().any(|p| p.y > 155.0));
    for point in points {
        let x = (point.x - 120.0) / 20.0;
        let y = (120.0 - point.y) / 20.0;
        assert!((y * y - x).abs() < 0.001);
    }
    assert!(strokes.len() <= 2, "曲线使用紧凑 Stroke 而非逐小线图元");
    for expression in ["y-x=1", "x+1", " y = x+1 ", "f(x)=x+1"] {
        let scene = page_scene(&plot_page(expression)).unwrap();
        let strokes = plot_strokes(&scene);
        assert_eq!(strokes.len(), 1);
        for point in &strokes[0].points {
            assert!((point.x + point.y - 220.0).abs() < 0.001);
        }
    }
}

#[test]
fn implicit_plot_exports_preserve_branches_and_clip_without_connectors() {
    for (expression, visible, gap) in [
        ("y^2=x", vec![(200, 80), (200, 160)], (200, 120)),
        (
            "x^2+y^2=9",
            vec![(180, 120), (60, 120), (120, 60), (120, 180)],
            (120, 120),
        ),
        (
            "(x-1)^2/9+(y+1)^2/4=1",
            vec![(200, 140), (80, 140), (140, 100), (140, 180)],
            (140, 140),
        ),
        ("x*y=1", vec![(140, 100), (100, 140)], (120, 120)),
        (
            "x^2+y^2=36",
            vec![(216, 48), (24, 48), (216, 192), (24, 192)],
            (120, 20),
        ),
    ] {
        let page = plot_page(expression);
        let scene = page_scene(&page).unwrap();
        let strokes = plot_strokes(&scene);
        let rect = Rect::from_min_max(egui::pos2(20.0, 20.0), egui::pos2(220.0, 220.0));
        assert!(
            strokes
                .iter()
                .flat_map(|s| &s.points)
                .all(|p| rect.contains(*p))
        );
        if expression == "x*y=1" {
            assert_eq!(strokes.len(), 2);
        }
        if expression == "x^2+y^2=36" {
            assert!(strokes.len() >= 4, "出框后重新进入必须断开");
        }
        let svg = export_svg(&page, 240, 240, false).unwrap();
        assert!(svg.contains("clip-path=\"url(#plot-"));
        assert_eq!(svg.matches("fill=\"#3491ec\"").count(), strokes.len());
        let image =
            tiny_skia::Pixmap::decode_png(&export_png(&page, 240, 240, false).unwrap()).unwrap();
        let blue_near = |x: u32, y: u32| {
            (x - 2..=x + 2).any(|px| {
                (y - 2..=y + 2).any(|py| {
                    let pixel = image.pixel(px, py).unwrap();
                    i16::from(pixel.blue()) - i16::from(pixel.red()) > 100
                })
            })
        };
        for (x, y) in visible {
            assert!(blue_near(x, y), "{expression}: 缺少 ({x},{y}) 曲线");
        }
        assert!(!blue_near(gap.0, gap.1), "{expression}: 夹带分支连线");
        for y in 0..240 {
            for x in 0..240 {
                if !(20..220).contains(&x) || !(20..220).contains(&y) {
                    let pixel = image.pixel(x, y).unwrap();
                    assert!(i16::from(pixel.blue()) - i16::from(pixel.red()) < 100);
                }
            }
        }
    }
}

#[test]
fn implicit_plot_isolated_points_empty_sets_and_errors_are_explicit() {
    let page = plot_page("(x-1)^2+(y+2)^2=0");
    let scene = page_scene(&page).unwrap();
    assert!(plot_strokes(&scene).is_empty());
    assert!(scene.items.iter().any(|item| matches!(item,
        Primitive::Disk(center, radius, _) if *center == egui::pos2(140.0, 160.0) && *radius == 1.0)));
    assert!(
        export_svg(&page, 240, 240, false)
            .unwrap()
            .contains("<circle cx=\"140\" cy=\"160\"")
    );
    let image =
        tiny_skia::Pixmap::decode_png(&export_png(&page, 240, 240, false).unwrap()).unwrap();
    assert!(image.pixel(140, 160).unwrap().red() < 200);
    for expression in ["x^2+y^2=-1", "y=y+1", "(x-10)^2+y^2=0"] {
        let page = plot_page(expression);
        let scene = page_scene(&page).unwrap();
        assert!(scene.items.iter().all(|item| match item {
            Primitive::Line(_, _, _, color) => *color == AXIS_COLOR,
            Primitive::Clip(_) | Primitive::EndClip => true,
            _ => false,
        }));
        assert!(
            !export_svg(&page, 240, 240, false)
                .unwrap()
                .contains("#3491ec")
        );
    }
    for expression in ["sin(y)=x", "y=y", "y=x=1"] {
        let page = plot_page(expression);
        assert!(matches!(page_scene(&page), Err(RenderError::Math(_))));
        assert!(matches!(
            export_svg(&page, 240, 240, false),
            Err(RenderError::Math(_))
        ));
        assert!(matches!(
            export_png(&page, 240, 240, false),
            Err(RenderError::Math(_))
        ));
    }
}

#[test]
fn implicit_plot_thumbnail_reuses_clipped_meshes() {
    let page = plot_page("x^2+y^2=36");
    let thumbnail = PageThumbnail::new(&page, ("doc", 1));
    let context = egui::Context::default();
    let target = Rect::from_min_size(Pos2::ZERO, egui::vec2(120.0, 120.0));
    let mut previous = None;
    for _ in 0..2 {
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            thumbnail
                .paint(
                    ui.painter(),
                    target,
                    egui::vec2(240.0, 240.0),
                    false,
                    &NoResources,
                )
                .unwrap();
            let meshes = thumbnail.meshes.borrow();
            let mesh = meshes.as_ref().unwrap().batches.last().unwrap().2.clone();
            assert!(!mesh.vertices.is_empty());
            if let Some(previous) = &previous {
                assert!(std::sync::Arc::ptr_eq(previous, &mesh));
            }
            previous = Some(mesh);
        });
        output.textures_delta.clear();
        let plot_clip = Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(110.0, 110.0));
        assert!(
            output
                .shapes
                .iter()
                .any(|s| s.clip_rect == plot_clip && matches!(s.shape, egui::Shape::Mesh(_)))
        );
    }
    assert!(thumbnail.matches(("doc", 1)));
    assert!(!thumbnail.matches(("doc", 2)));
}

#[test]
fn function_discontinuities_are_not_connected_and_are_clipped() {
    for expression in ["1/x", "1/(x-0.003)"] {
        let plot = object(ObjectKind::FunctionPlot {
            position: p(0.0, 0.0),
            width: 200.0,
            height: 100.0,
            expressions: vec![expression.into()],
            x_min: -1.0,
            x_max: 1.0,
            y_min: -10.0,
            y_max: 10.0,
        });
        let mut scene = Scene::default();
        append_object(&mut scene, &plot).unwrap();
        let mut count = 0;
        for item in scene.items {
            if let Primitive::Stroke(stroke) = item {
                for pair in stroke.points.windows(2) {
                    let (a, b) = (pair[0], pair[1]);
                    count += 1;
                    assert!(!(a.x < 100.0 && b.x > 100.3));
                    assert!((0.0..=100.0).contains(&a.y));
                    assert!((0.0..=100.0).contains(&b.y));
                    assert!((a.y - b.y).abs() < 50.0);
                }
            }
        }
        assert!(count > 10);
    }
}

#[test]
fn strokes_have_pressure_width_and_continuous_dash_phase() {
    let stroke = object(ObjectKind::Stroke {
        points: vec![
            StrokePoint {
                x: 0.0,
                y: 5.0,
                time: 0.0,
                pressure: 1.0,
            },
            StrokePoint {
                x: 80.0,
                y: 5.0,
                time: 1.0,
                pressure: 0.1,
            },
        ],
        style: Style {
            width: 4.0,
            dashed: true,
            ..Default::default()
        },
    });
    let mut scene = Scene::default();
    append_object(&mut scene, &stroke).unwrap();
    let mut widths = Vec::new();
    let mut segments = Vec::new();
    for item in scene.items {
        if let Primitive::Stroke(stroke) = item {
            widths.extend_from_slice(&stroke.widths);
            segments.push((stroke.points[0].x, stroke.points.last().unwrap().x));
        }
    }
    assert!(widths.first().unwrap() > widths.last().unwrap());
    assert!(segments.windows(2).any(|s| s[1].0 - s[0].1 > 5.0));
    assert!(object_bounds(&stroke).contains(egui::pos2(-2.0, 3.0)));
}

#[test]
fn missing_images_never_become_external_links() {
    let page = page(ObjectKind::Image {
        position: p(0.0, 0.0),
        width: 20.0,
        height: 20.0,
        asset_ref: "https://example.invalid/image.png".into(),
    });
    assert!(matches!(
        export_svg(&page, 32, 32, false),
        Err(RenderError::MissingResource(_))
    ));
    assert!(matches!(
        export_png(&page, 32, 32, false),
        Err(RenderError::MissingResource(_))
    ));
    let resolver = |reference: &str| (reference == "asset:1").then_some(egui::TextureId::User(7));
    assert_eq!(
        resolver.texture_id("asset:1"),
        Some(egui::TextureId::User(7))
    );
    assert_eq!(resolver.texture_id("unknown"), None);
}

#[test]
fn budgets_and_invalid_geometry_are_rejected() {
    let page = Page::new();
    for (width, height) in [(0, 1), (1, 0), (8193, 1), (8192, 8192)] {
        assert!(matches!(
            export_svg(&page, width, height, false),
            Err(RenderError::InvalidDimensions)
        ));
        assert!(matches!(
            export_png(&page, width, height, false),
            Err(RenderError::InvalidDimensions)
        ));
    }
    let obj = object(shape(
        ShapeKind::Line,
        vec![p(f32::NAN, 0.0), p(10.0, 10.0)],
    ));
    assert_eq!(object_bounds(&obj), Rect::NOTHING);
    let obj = object(shape(ShapeKind::Line, vec![]));
    assert_eq!(object_bounds(&obj), Rect::NOTHING);
}

#[test]
fn blackboard_has_deterministic_texture() {
    let page = Page::new();
    let a = export_png(&page, 32, 32, true).unwrap();
    assert_eq!(a, export_png(&page, 32, 32, true).unwrap());
    assert_ne!(a, export_png(&page, 32, 32, false).unwrap());
    let svg = export_svg(&page, 32, 32, true).unwrap();
    assert!(svg.contains("#18342b"));
    assert!(svg.contains("#bed2be"));
}

#[test]
fn screen_backgrounds_preserve_transparency_and_white_ink_across_all_page_entries() {
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0));
    for entry in ["plain", "resources", "content_cache", "revision_cache"] {
        for ink_alpha in [None, Some(255), Some(128)] {
            let page = match ink_alpha {
                None => Page::new(),
                Some(alpha) => page(ObjectKind::Stroke {
                    points: vec![
                        StrokePoint {
                            x: 20.0,
                            y: 50.0,
                            time: 0.0,
                            pressure: 1.0,
                        },
                        StrokePoint {
                            x: 80.0,
                            y: 50.0,
                            time: 1.0,
                            pressure: 1.0,
                        },
                    ],
                    style: Style {
                        color: Color {
                            r: 255,
                            g: 255,
                            b: 255,
                            a: alpha,
                        },
                        width: 4.0,
                        dashed: false,
                    },
                }),
            };
            let context = egui::Context::default();
            let mut renderer = PageRenderer::new();
            let mut cached_pointer = None;
            for blackboard in [false, false, true, true, false] {
                let mut output = context.run_ui(egui::RawInput::default(), |ui| {
                    let painter = ui
                        .ctx()
                        .layer_painter(egui::LayerId::background())
                        .with_clip_rect(rect);
                    match entry {
                        "plain" => paint_page(&painter, &page, blackboard),
                        "resources" => {
                            paint_page_with_resources(&painter, &page, blackboard, &NoResources)
                        }
                        "content_cache" => renderer
                            .paint_page_with_resources(&painter, &page, blackboard, &NoResources)
                            .unwrap(),
                        "revision_cache" => renderer
                            .paint_page_at_revision(
                                &painter,
                                &page,
                                ("doc", 0),
                                blackboard,
                                &NoResources,
                            )
                            .unwrap(),
                        _ => unreachable!(),
                    }
                });
                output.textures_delta.clear();
                if let Some(cached) = page
                    .objects
                    .first()
                    .and_then(|object| renderer.objects.get(&object.id))
                {
                    let pointer = cached.scene.items.as_ptr();
                    if let Some(previous) = cached_pointer {
                        assert_eq!(previous, pointer, "{entry}: 模式切换应复用几何缓存");
                    }
                    cached_pointer = Some(pointer);
                }
                let mut shapes = output
                    .shapes
                    .iter()
                    .map(|clipped| &clipped.shape)
                    .filter(|shape| !matches!(shape, egui::Shape::Noop));
                if blackboard {
                    assert!(
                        matches!(
                            shapes.next(),
                            Some(egui::Shape::Rect(bg))
                                if bg.rect == rect && bg.fill == Color32::from_rgb(24, 52, 43)
                                    && bg.fill.a() == 255
                        ),
                        "{entry}: 黑板必须保留不透明底色"
                    );
                    for (a, b) in texture_lines(rect) {
                        assert!(
                            matches!(
                                shapes.next(),
                                Some(egui::Shape::LineSegment { points, stroke })
                                    if *points == [a, b] && stroke.color.a() == 10
                            ),
                            "{entry}: 黑板必须保留粗糙细纹"
                        );
                    }
                }
                let ink: Vec<_> = shapes.collect();
                if let Some(alpha) = ink_alpha {
                    assert!(!ink.is_empty(), "{entry}: 白色笔迹不能被过滤");
                    assert!(
                        ink.iter().all(|shape| match shape {
                            egui::Shape::Path(path) =>
                                path.closed
                                    && path.fill
                                        == Color32::from_rgba_unmultiplied(255, 255, 255, alpha),
                            egui::Shape::Mesh(mesh) => mesh.vertices.iter().all(|vertex| vertex
                                .color
                                == Color32::TRANSPARENT
                                || vertex.color
                                    == Color32::from_rgba_unmultiplied(255, 255, 255, alpha)),
                            _ => false,
                        }),
                        "{entry}: 仅绘制保留原始 alpha 的白色笔迹，不得补背景"
                    );
                } else {
                    assert!(ink.is_empty(), "{entry}: 空白 Drawing 页面不得产生背景图元");
                }
            }
        }
    }
}

#[test]
fn drawing_exports_keep_opaque_white_background() {
    let page = Page::new();
    let png = export_png(&page, 32, 32, false).unwrap();
    let image = tiny_skia::Pixmap::decode_png(&png).unwrap();
    assert!(image.pixels().iter().all(|pixel| {
        (pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()) == (255, 255, 255, 255)
    }));
    let svg = export_svg(&page, 32, 32, false).unwrap();
    assert!(svg.contains("<rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>"));
}

#[test]
fn painter_api_runs_headlessly() {
    let context = egui::Context::default();
    let mut output = context.run_ui(egui::RawInput::default(), |ui| {
        let painter = ui
            .ctx()
            .layer_painter(egui::LayerId::background())
            .with_clip_rect(Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0)));
        let page = page(shape(ShapeKind::Cube, vec![p(10.0, 10.0), p(50.0, 50.0)]));
        paint_page(&painter, &page, true);
        paint_object(&painter, &page.objects[0]);
        let image = object(ObjectKind::Image {
            position: p(0.0, 0.0),
            width: 20.0,
            height: 20.0,
            asset_ref: "asset:1".into(),
        });
        paint_object_with_resources(&painter, &image, &|_: &str| Some(egui::TextureId::User(1)));
        paint_object(&painter, &image);
    });
    output.textures_delta.clear();
    assert!(!output.shapes.is_empty());
}

#[test]
fn math_bounds_and_degenerate_dashes_are_visible() {
    let plot = object(ObjectKind::FunctionPlot {
        position: p(12.0, 24.0),
        width: 120.0,
        height: 80.0,
        expressions: vec!["sin(x)".into()],
        x_min: -3.0,
        x_max: 3.0,
        y_min: -1.0,
        y_max: 1.0,
    });
    assert_eq!(
        object_bounds(&plot),
        Rect::from_min_max(egui::pos2(11.0, 23.0), egui::pos2(133.0, 105.0))
    );
    let mut scene = Scene::default();
    styled_line(
        &mut scene,
        egui::pos2(8.0, 8.0),
        egui::pos2(8.0, 8.0),
        6.0,
        Style {
            dashed: true,
            ..Default::default()
        },
        &mut 0.0,
    )
    .unwrap();
    assert!(matches!(scene.items[0], Primitive::Disk(_, 3.0, _)));
}

fn red_png(alpha: u8) -> Vec<u8> {
    let mut image = tiny_skia::Pixmap::new(2, 2).unwrap();
    image.fill(tiny_skia::Color::from_rgba8(255, 0, 0, alpha));
    image.encode_png().unwrap()
}

#[test]
fn injected_png_svg_alpha_and_texture_lifecycle() {
    let mut resources = RenderResources::new();
    resources.insert_png("asset:1", &red_png(128)).unwrap();
    let page = page(ObjectKind::Image {
        position: p(4.0, 4.0),
        width: 16.0,
        height: 16.0,
        asset_ref: "asset:1".into(),
    });
    let svg = export_svg_with_resources(&page, 32, 32, false, &resources).unwrap();
    assert!(svg.contains("href=\"data:image/png;base64,iVBORw0KGgo"));
    assert!(svg.contains("preserveAspectRatio=\"none\""));
    assert!(!svg.contains("asset:1"));
    let output = export_png_with_resources(&page, 32, 32, false, &resources).unwrap();
    let decoded = tiny_skia::Pixmap::decode_png(&output).unwrap();
    let pixel = decoded.pixel(12, 12).unwrap();
    assert_eq!(pixel.red(), 255);
    assert!((126..=128).contains(&pixel.green()));
    assert_eq!(pixel.alpha(), 255);
    assert_eq!(decoded.pixel(0, 0).unwrap().green(), 255);
    let context = egui::Context::default();
    resources.prepare_textures(&context);
    let texture = resources.texture_id("asset:1").unwrap();
    resources.prepare_textures(&context);
    assert_eq!(resources.texture_id("asset:1"), Some(texture));
    let mut output = context.run_ui(egui::RawInput::default(), |ui| {
        paint_page_with_resources(ui.painter(), &page, false, &resources);
    });
    output.textures_delta.clear();
    assert!(
        output
            .shapes
            .iter()
            .any(|s| matches!(&s.shape, egui::Shape::Mesh(m) if m.texture_id == texture))
    );
    resources.insert_png("asset:1", &red_png(0)).unwrap();
    assert!(resources.texture_id("asset:1").is_none());
    let transparent = export_png_with_resources(&page, 32, 32, false, &resources).unwrap();
    assert_eq!(
        tiny_skia::Pixmap::decode_png(&transparent)
            .unwrap()
            .pixel(12, 12)
            .unwrap()
            .green(),
        255
    );
    assert!(resources.remove_image("asset:1"));
    assert!(export_png_with_resources(&page, 32, 32, false, &resources).is_err());
}

#[test]
fn png_decode_rejects_corruption_and_oversize_without_losing_existing_resource() {
    let mut resources = RenderResources::new();
    let valid = red_png(255);
    resources.insert_png("one", &valid).unwrap();
    assert!(resources.insert_png("one", b"not a PNG").is_err());
    assert!(
        resources
            .insert_png("one", &valid[..valid.len() - 10])
            .is_err()
    );
    let mut corrupt = valid.clone();
    corrupt[29] ^= 1;
    assert!(resources.insert_png("one", &corrupt).is_err());
    assert_eq!(resources.images.len(), 1);
    assert_eq!(resources.images["one"].pixmap.width(), 2);
    let mut large_header = Vec::new();
    let mut encoder = png::Encoder::new(&mut large_header, 8193, 1);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().finish().unwrap();
    assert!(resources.insert_png("large", &large_header).is_err());
    assert!(resources.set_font(vec![1, 2, 3], 0).is_err());
    assert_eq!(resources::base64(b""), "");
    assert_eq!(resources::base64(b"f"), "Zg==");
    assert_eq!(resources::base64(b"fo"), "Zm8=");
    assert_eq!(resources::base64(b"foo"), "Zm9v");
    for i in 1..256 {
        resources.insert_png(format!("image:{i}"), &valid).unwrap();
    }
    assert!(matches!(
        resources.insert_png("overflow", &valid),
        Err(RenderError::ResourceLimit(_))
    ));
    resources.insert_png("one", &valid).unwrap();
    assert!(resources.remove_image("one"));
    resources.insert_png("replacement", &valid).unwrap();
}

#[cfg(windows)]
#[test]
fn real_system_ttc_renders_chinese_multiline_and_reports_missing_glyphs() {
    let bytes = match std::fs::read(r"C:\Windows\Fonts\msyh.ttc") {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("SKIP 中文字体回归：微软雅黑 TTC 不可用：{error}");
            return;
        }
    };
    let mut resources = RenderResources::new();
    resources.set_font(bytes, 0).unwrap();
    let mut page = page(ObjectKind::Text {
        position: p(2.0, 2.0),
        text: "中文 A\n第二行".into(),
        size: 24.0,
        color: Color {
            r: 0,
            g: 0,
            b: 0,
            a: 128,
        },
    });
    let png = export_png_with_resources(&page, 128, 90, false, &resources).unwrap();
    let image = tiny_skia::Pixmap::decode_png(&png).unwrap();
    for (top, bottom) in [(2, 32), (38, 75)] {
        assert!((top..bottom).any(|y| (2..100).any(|x| image.pixel(x, y).unwrap().red() < 200)));
    }
    assert!(
        image
            .pixels()
            .iter()
            .all(|p| p.red() >= 126 && p.alpha() == 255)
    );
    if let ObjectKind::Text { text, .. } = &mut page.objects[0].kind {
        *text = "\u{10ffff}".into();
    }
    assert!(matches!(
        export_png_with_resources(&page, 128, 90, false, &resources),
        Err(RenderError::MissingGlyph(_))
    ));
    let svg = export_svg_with_resources(&page, 128, 90, false, &resources);
    assert!(matches!(svg, Err(RenderError::MissingGlyph(_))));
    if let ObjectKind::Text { text, .. } = &mut page.objects[0].kind {
        *text = "ASCII".into();
    }
    page.objects.push(object(ObjectKind::Image {
        position: p(70.0, 40.0),
        width: 20.0,
        height: 20.0,
        asset_ref: "mixed".into(),
    }));
    resources.insert_png("mixed", &red_png(255)).unwrap();
    page.objects.push(object(shape(
        ShapeKind::Circle,
        vec![p(4.0, 40.0), p(24.0, 60.0)],
    )));
    let mixed = export_png_with_resources(&page, 128, 90, false, &resources).unwrap();
    assert_eq!(
        tiny_skia::Pixmap::decode_png(&mixed)
            .unwrap()
            .pixel(80, 50)
            .unwrap()
            .green(),
        0
    );
    let svg = export_svg_with_resources(&page, 128, 90, false, &resources).unwrap();
    assert!(svg.contains("ASCII") && svg.contains("data:image/png;base64,"));
    resources.clear_font();
    assert!(matches!(
        export_png_with_resources(&page, 128, 90, false, &resources),
        Err(RenderError::MissingFont)
    ));
}

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut chunk = (data.len() as u32).to_be_bytes().to_vec();
    chunk.extend_from_slice(kind);
    chunk.extend_from_slice(data);
    let mut crc = !0u32;
    for byte in &chunk[4..] {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    chunk.extend_from_slice(&(!crc).to_be_bytes());
    chunk
}

#[test]
fn png_truncations_ancillary_crc_apng_and_decompression_bombs() {
    let mut resources = RenderResources::new();
    let valid = red_png(255);
    resources.insert_png("keep", &valid).unwrap();
    for end in 0..valid.len() {
        assert!(
            resources.insert_png("keep", &valid[..end]).is_err(),
            "截断 {end}"
        );
    }
    let mut trailing = valid.clone();
    trailing.push(0);
    assert!(resources.insert_png("keep", &trailing).is_err());
    let mut ancillary = png_chunk(b"tEXt", b"key\0value");
    *ancillary.last_mut().unwrap() ^= 1;
    let mut corrupt = valid.clone();
    corrupt.splice(33..33, ancillary);
    assert!(resources.insert_png("keep", &corrupt).is_err());
    for kind in [b"acTL", b"fcTL", b"fdAT"] {
        let mut animation = valid.clone();
        animation.splice(33..33, png_chunk(kind, &[0; 8]));
        assert!(resources.insert_png("keep", &animation).is_err());
    }
    // 声明 1x1，却包含较大图像的压缩 IDAT：必须拒绝额外的解压像素。
    let mut large = tiny_skia::Pixmap::new(512, 512)
        .unwrap()
        .encode_png()
        .unwrap();
    let mut ihdr = large[16..29].to_vec();
    ihdr[..4].copy_from_slice(&1u32.to_be_bytes());
    ihdr[4..8].copy_from_slice(&1u32.to_be_bytes());
    large.splice(8..33, png_chunk(b"IHDR", &ihdr));
    assert!(resources.insert_png("keep", &large).is_err());
    let mut offset = 8;
    let mut bad_adler = valid.clone();
    while &valid[offset + 4..offset + 8] != b"IDAT" {
        offset += u32::from_be_bytes(valid[offset..offset + 4].try_into().unwrap()) as usize + 12;
    }
    let length = u32::from_be_bytes(valid[offset..offset + 4].try_into().unwrap()) as usize;
    let mut idat = valid[offset + 8..offset + 8 + length].to_vec();
    *idat.last_mut().unwrap() ^= 1;
    bad_adler.splice(offset..offset + length + 12, png_chunk(b"IDAT", &idat));
    assert!(resources.insert_png("keep", &bad_adler).is_err());
    assert_eq!(resources.images.len(), 1);
    assert_eq!(
        resources.images["keep"].pixmap.pixel(0, 0).unwrap().red(),
        255
    );
}

#[test]
fn strict_png_accepts_packed_palette_gray_16bit_and_adam7() {
    for (color, depth, bytes) in [
        (png::ColorType::Grayscale, png::BitDepth::One, vec![0]),
        (
            png::ColorType::GrayscaleAlpha,
            png::BitDepth::Eight,
            vec![0, 255],
        ),
        (
            png::ColorType::Rgb,
            png::BitDepth::Sixteen,
            vec![0, 0, 0, 0, 0, 0],
        ),
        (png::ColorType::Indexed, png::BitDepth::One, vec![0]),
    ] {
        let mut encoded = Vec::new();
        let mut encoder = png::Encoder::new(&mut encoded, 1, 1);
        encoder.set_color(color);
        encoder.set_depth(depth);
        if color == png::ColorType::Indexed {
            encoder.set_palette(vec![0, 0, 0]);
        }
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&bytes)
            .unwrap();
        for interlaced in [false, true] {
            let mut image = encoded.clone();
            if interlaced {
                // 1x1 的 Adam7 仅第一 pass 有像素，与普通扫描行相同。
                let mut header = image[16..29].to_vec();
                header[12] = 1;
                image.splice(8..33, png_chunk(b"IHDR", &header));
            }
            let mut resources = RenderResources::new();
            resources.insert_png("one", &image).unwrap();
            let pixel = resources.images["one"].pixmap.pixel(0, 0).unwrap();
            assert_eq!(
                (pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()),
                (0, 0, 0, 255)
            );
        }
    }
}

#[test]
fn textures_are_freed_on_replace_clear_remove_and_drop() {
    let context = egui::Context::default();
    let mut resources = RenderResources::new();
    resources.insert_png("one", &red_png(255)).unwrap();
    resources.prepare_textures(&context);
    let first = resources.texture_id("one").unwrap();
    resources.insert_png("one", &red_png(128)).unwrap();
    resources.prepare_textures(&context);
    let second = resources.texture_id("one").unwrap();
    resources.clear_textures();
    resources.prepare_textures(&context);
    let third = resources.texture_id("one").unwrap();
    resources.remove_image("one");
    resources.insert_png("two", &red_png(255)).unwrap();
    resources.prepare_textures(&context);
    let fourth = resources.texture_id("two").unwrap();
    drop(resources);
    let mut delta = context.tex_manager().write().take_delta();
    for id in [first, second, third, fourth] {
        assert!(delta.free.contains(&id));
    }
    delta.clear();
}

#[test]
fn revision_cache_has_no_page_clone_and_failures_are_visible() {
    let mut renderer = PageRenderer::new();
    let context = egui::Context::default();
    let mut page = page(shape(ShapeKind::Line, vec![p(1.0, 1.0), p(5.0, 5.0)]));
    let mut output = context.run_ui(egui::RawInput::default(), |ui| {
        renderer
            .paint_page_at_revision(ui.painter(), &page, ("doc", 0), false, &NoResources)
            .unwrap();
        assert!(renderer.preview.is_none());
        let pointer = renderer.objects[&page.objects[0].id].scene.items.as_ptr();
        renderer
            .paint_page_at_revision(ui.painter(), &page, ("doc", 0), false, &NoResources)
            .unwrap();
        assert_eq!(
            pointer,
            renderer.objects[&page.objects[0].id].scene.items.as_ptr()
        );
        page.objects.clear();
        renderer
            .paint_page_at_revision(ui.painter(), &page, ("doc", 1), false, &NoResources)
            .unwrap();
        assert!(renderer.objects.is_empty());
        page.objects.push(object(shape(
            ShapeKind::Line,
            vec![p(f32::NAN, 0.0), p(1.0, 1.0)],
        )));
        assert!(
            renderer
                .paint_page_at_revision(ui.painter(), &page, ("other-doc", 1), false, &NoResources)
                .is_err()
        );
        assert!(renderer.objects.is_empty());
        page.objects = vec![object(ObjectKind::FunctionPlot {
            position: p(0.0, 0.0),
            width: 1.0,
            height: 1.0,
            expressions: vec!["x".into(); 33],
            x_min: 0.0,
            x_max: 1.0,
            y_min: 0.0,
            y_max: 1.0,
        })];
        assert!(matches!(
            renderer.paint_page_at_revision(ui.painter(), &page, ("doc", 3), false, &NoResources),
            Err(RenderError::ResourceLimit(_))
        ));
    });
    output.textures_delta.clear();
    assert!(
        output
            .shapes
            .iter()
            .any(|shape| matches!(shape.shape, egui::Shape::Text(_)))
    );
}

#[test]
fn stroke_width_formula_and_normalized_newlines_match() {
    let points: Vec<_> = (0..100)
        .map(|i| StrokePoint {
            x: i as f32,
            y: (i % 3) as f32,
            time: i as f64 / 60.0,
            pressure: (i % 10) as f32 / 10.0,
        })
        .collect();
    assert_eq!(
        stroke_widths(&points, &Style::default()),
        board_ink::stroke_widths(&points, &Style::default(), 1200.0, 0.7).unwrap()
    );
    let text = page(ObjectKind::Text {
        position: p(0.0, 0.0),
        text: "a\r\n\tb\rc".into(),
        size: 12.0,
        color: Color::default(),
    });
    assert!(
        matches!(&page_scene(&text).unwrap().items[0], Primitive::Text(_, text, _, _) if text == "a\n    bc")
    );
    let context = egui::Context::default();
    let mut output = context.run_ui(egui::RawInput::default(), |ui| {
        paint_page(ui.painter(), &text, false)
    });
    output.textures_delta.clear();
    let positions: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                Some(t.pos.y)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(positions[1] - positions[0], 18.0);
}

#[test]
fn object_cache_only_rebuilds_changed_snapshots_and_prunes_removed_objects() {
    let ctx = egui::Context::default();
    let mut renderer = PageRenderer::new();
    let mut page = ink_benchmark_page();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let painter = ui
            .painter()
            .with_clip_rect(Rect::from_min_size(Pos2::ZERO, egui::vec2(1200.0, 800.0)));
        renderer
            .paint_page_at_revision(&painter, &page, ("doc", 1), false, &NoResources)
            .unwrap();
        let id = page.objects[0].id.clone();
        let mesh = renderer.objects[&id].meshes.as_ref().unwrap().batches[0]
            .2
            .clone();
        let scene = renderer.objects[&id].scene.items.as_ptr();
        let mut extra = page.objects[1].clone();
        extra.id = "extra".into();
        page.objects.push(extra);
        RENDER_DEBUG_METRICS.with(|m| m.set(RenderDebugMetrics::default()));
        renderer
            .paint_page_at_revision(&painter, &page, ("doc", 2), false, &NoResources)
            .unwrap();
        let metrics = RENDER_DEBUG_METRICS.with(|m| m.get());
        assert_eq!(
            (
                metrics.object_hits,
                metrics.object_misses,
                metrics.mesh_hits,
                metrics.mesh_misses
            ),
            (6, 1, 6, 1)
        );
        assert_eq!(scene, renderer.objects[&id].scene.items.as_ptr());
        assert!(std::sync::Arc::ptr_eq(
            &mesh,
            &renderer.objects[&id].meshes.as_ref().unwrap().batches[0].2
        ));
        if let ObjectKind::Stroke { points, style } = &mut page.objects[1].kind {
            points[0].pressure = 0.2;
            style.color.a = 100;
        }
        RENDER_DEBUG_METRICS.with(|m| m.set(RenderDebugMetrics::default()));
        renderer
            .paint_page_at_revision(&painter, &page, ("doc", 2), false, &NoResources)
            .unwrap();
        let metrics = RENDER_DEBUG_METRICS.with(|m| m.get());
        assert_eq!((metrics.object_misses, metrics.mesh_misses), (1, 1));
        page.objects.pop();
        page.objects.reverse();
        RENDER_DEBUG_METRICS.with(|m| m.set(RenderDebugMetrics::default()));
        renderer
            .paint_page_at_revision(&painter, &page, ("doc", 3), false, &NoResources)
            .unwrap();
        let metrics = RENDER_DEBUG_METRICS.with(|m| m.get());
        assert_eq!((metrics.object_misses, metrics.mesh_misses), (0, 0));
        assert_eq!(renderer.objects.len(), page.objects.len());
        assert!(!renderer.objects.contains_key("extra"));
        for (doc, page_id) in [("other", page.id.clone()), ("other", "other-page".into())] {
            page.id = page_id;
            RENDER_DEBUG_METRICS.with(|m| m.set(RenderDebugMetrics::default()));
            renderer
                .paint_page_at_revision(&painter, &page, (doc, 3), false, &NoResources)
                .unwrap();
            assert_eq!(RENDER_DEBUG_METRICS.with(|m| m.get().object_misses), 6);
            assert!(!std::sync::Arc::ptr_eq(
                &mesh,
                &renderer.objects[&id].meshes.as_ref().unwrap().batches[0].2
            ));
        }
    });
    output.textures_delta.clear();
}

#[test]
fn modified_preview_keeps_unchanged_objects_and_offscreen_meshes_are_lazy() {
    let ctx = egui::Context::default();
    let mut renderer = PageRenderer::new();
    let mut page = ink_benchmark_page();
    page.objects.truncate(2);
    if let ObjectKind::Stroke { points, .. } = &mut page.objects[1].kind {
        for point in points {
            point.x += 5000.0;
            point.y += 5000.0;
        }
    }
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let painter = ui
            .painter()
            .with_clip_rect(Rect::from_min_size(Pos2::ZERO, egui::vec2(1200.0, 800.0)));
        renderer
            .paint_page_at_revision(&painter, &page, ("doc", 1), false, &NoResources)
            .unwrap();
        let id = page.objects[0].id.clone();
        let mesh = renderer.objects[&id].meshes.as_ref().unwrap().batches[0]
            .2
            .clone();
        assert!(renderer.objects[&page.objects[1].id].meshes.is_none());
        let mut preview = page.clone();
        let mut extra = page.objects[0].clone();
        extra.id = "preview".into();
        preview.objects.push(extra);
        renderer
            .paint_page_with_resources(&painter, &preview, false, &NoResources)
            .unwrap();
        if let ObjectKind::Stroke { points, .. } = &mut preview.objects[2].kind {
            points.truncate(20);
        }
        renderer
            .paint_page_with_resources(&painter, &preview, false, &NoResources)
            .unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &mesh,
            &renderer.objects[&id].meshes.as_ref().unwrap().batches[0].2
        ));
        renderer
            .paint_page_at_revision(&painter, &page, ("doc", 1), false, &NoResources)
            .unwrap();
        assert_eq!(renderer.objects.len(), 2);
        assert!(std::sync::Arc::ptr_eq(
            &mesh,
            &renderer.objects[&id].meshes.as_ref().unwrap().batches[0].2
        ));
        let mut moved_clip = painter.clone();
        moved_clip.set_clip_rect(Rect::from_min_size(
            egui::pos2(4900.0, 4900.0),
            egui::vec2(1200.0, 800.0),
        ));
        RENDER_DEBUG_METRICS.with(|m| m.set(RenderDebugMetrics::default()));
        renderer
            .paint_page_at_revision(&moved_clip, &page, ("doc", 1), false, &NoResources)
            .unwrap();
        let metrics = RENDER_DEBUG_METRICS.with(|m| m.get());
        assert_eq!((metrics.mesh_misses, metrics.culled_objects), (1, 1));
        assert!(renderer.objects[&page.objects[1].id].meshes.is_some());
    });
    output.textures_delta.clear();
}

#[test]
fn reordered_translucent_objects_match_uncached_painter_order() {
    let ctx = egui::Context::default();
    let mut page = ink_benchmark_page();
    page.objects.truncate(2);
    for (i, object) in page.objects.iter_mut().enumerate() {
        if let ObjectKind::Stroke { points, style } = &mut object.kind {
            points.truncate(30);
            for point in points {
                point.y = 50.0 + (point.x / 10.0).sin();
            }
            style.color = Color {
                r: (i * 255) as u8,
                g: 0,
                b: 255,
                a: 100,
            };
        }
    }
    let mut renderer = PageRenderer::new();
    for _ in 0..3 {
        page.objects.reverse();
        let mut draw = |cached| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                if cached {
                    renderer
                        .paint_page_at_revision(
                            ui.painter(),
                            &page,
                            ("doc", 1),
                            false,
                            &NoResources,
                        )
                        .unwrap();
                } else {
                    paint_page(ui.painter(), &page, false);
                }
            });
            output.textures_delta.clear();
            ctx.tessellate(output.shapes, output.pixels_per_point)
        };
        let expected = draw(false);
        let actual = draw(true);
        assert_eq!(format!("{expected:?}"), format!("{actual:?}"));
    }
}

#[test]
fn stroke_chunks_reuse_idle_and_only_copy_one_chunk_on_edit() {
    let ctx = egui::Context::default();
    let mut renderer = PageRenderer::new();
    let mut page = ink_benchmark_page();
    let mut template = page.objects.remove(0);
    if let ObjectKind::Stroke { points, style } = &mut template.kind {
        points.truncate(128);
        style.color.a = 128;
    }
    page.objects = (0..1500)
        .map(|i| {
            let mut object = template.clone();
            object.id = format!("batch-{i}");
            object
        })
        .collect();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let painter = ui.painter().with_clip_rect(Rect::from_min_size(Pos2::ZERO, egui::vec2(1200.0, 800.0)));
        RENDER_DEBUG_METRICS.with(|m| m.set(RenderDebugMetrics::default()));
        renderer.paint_page_at_document_revision(&painter, &page, ("doc", 1), false, &NoResources).unwrap();
        let cold = RENDER_DEBUG_METRICS.with(|m| m.get());
        assert_eq!(cold.batch_rebuilds, 24);
        assert_eq!(cold.batch_buffer_allocations, 48);
        assert_eq!(cold.batch_arc_allocations, 24);
        assert_eq!(cold.paint_arc_clones, 24);
        assert!(renderer.chunks.iter().map(|c| c.bytes).sum::<usize>() <= MAX_STROKE_BATCH_BYTES);
        // The exact ordered vertices (including UV/AA/color) and reindexed triangles
        // must be identical to egui's old per-object append_ref path.
        let mut expected = egui::Mesh::default();
        for object in &page.objects {
            for (_, _, mesh) in &renderer.objects[&object.id].meshes.as_ref().unwrap().batches {
                expected.append_ref(mesh);
            }
        }
        let mut actual = egui::Mesh::default();
        for chunk in &renderer.chunks {
            for command in &chunk.commands {
                let PagePaintCommand::Mesh(mesh) = command else { panic!("pure stroke batch") };
                actual.append_ref(mesh);
            }
        }
        assert_eq!(expected.vertices, actual.vertices);
        assert_eq!(expected.indices, actual.indices);
        let unchanged = match &renderer.chunks[0].commands[0] {
            PagePaintCommand::Mesh(mesh) => mesh.clone(),
            _ => unreachable!(),
        };
        for legacy in [false, true] {
            RENDER_DEBUG_METRICS.with(|m| m.set(RenderDebugMetrics::default()));
            if legacy {
                renderer.paint_page_at_revision(&painter, &page, ("doc", 1), false, &NoResources).unwrap();
            } else {
                renderer.paint_page_at_document_revision(&painter, &page, ("doc", 1), false, &NoResources).unwrap();
            }
            let hot = RENDER_DEBUG_METRICS.with(|m| m.get());
            assert_eq!(hot.paint_arc_clones, 24);
            assert_eq!(hot.batch_buffer_allocations + hot.batch_arc_allocations, 0);
            assert_eq!(hot.batch_copied_vertices + hot.batch_copied_indices, 0);
            assert_eq!(hot.content_compare_skips, u64::from(!legacy));
        }
        if let ObjectKind::Stroke { style, .. } = &mut page.objects[80].kind {
            style.color.r = 71;
        }
        RENDER_DEBUG_METRICS.with(|m| m.set(RenderDebugMetrics::default()));
        // Preserve the legacy same-revision mutation guarantee.
        renderer.paint_page_at_revision(&painter, &page, ("doc", 1), false, &NoResources).unwrap();
        let edit = RENDER_DEBUG_METRICS.with(|m| m.get());
        assert_eq!((edit.object_misses, edit.mesh_misses, edit.batch_rebuilds), (1, 1, 1));
        assert_eq!((edit.batch_buffer_allocations, edit.batch_arc_allocations), (2, 1));
        let PagePaintCommand::Mesh(current) = &renderer.chunks[0].commands[0] else { unreachable!() };
        assert!(std::sync::Arc::ptr_eq(&unchanged, current));
        assert!(edit.batch_copied_vertices < cold.batch_copied_vertices / 10);
        eprintln!("stroke chunks: 1500 -> {} submissions; cold buffers={} arcs={}; idle allocations=0; edit buffers={} arcs={}", cold.paint_arc_clones, cold.batch_buffer_allocations, cold.batch_arc_allocations, edit.batch_buffer_allocations, edit.batch_arc_allocations);
    });
    output.textures_delta.clear();
}

#[test]
fn document_revision_fast_path_resynchronizes_after_preview_and_clip_changes() {
    let ctx = egui::Context::default();
    let mut renderer = PageRenderer::new();
    let page = ink_benchmark_page();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let painter = ui
            .painter()
            .with_clip_rect(Rect::from_min_size(Pos2::ZERO, egui::vec2(1200.0, 800.0)));
        renderer
            .paint_page_at_document_revision(&painter, &page, ("doc", 1), false, &NoResources)
            .unwrap();
        let mut preview = page.clone();
        preview.objects.remove(0);
        renderer
            .paint_page_with_resources(&painter, &preview, false, &NoResources)
            .unwrap();
        renderer
            .paint_page_at_document_revision(&painter, &page, ("doc", 1), false, &NoResources)
            .unwrap();
        assert_eq!(renderer.objects.len(), page.objects.len());
        assert!(renderer.preview.is_none());
        let outside = painter.with_clip_rect(Rect::from_min_size(
            egui::pos2(5000.0, 5000.0),
            egui::vec2(100.0, 100.0),
        ));
        RENDER_DEBUG_METRICS.with(|m| m.set(RenderDebugMetrics::default()));
        renderer
            .paint_page_at_document_revision(&outside, &page, ("doc", 1), false, &NoResources)
            .unwrap();
        assert_eq!(RENDER_DEBUG_METRICS.with(|m| m.get().paint_arc_clones), 0);
        renderer
            .paint_page_at_document_revision(&painter, &page, ("doc", 1), false, &NoResources)
            .unwrap();
        assert_eq!(renderer.chunks.len(), 1);
    });
    output.textures_delta.clear();
}

#[test]
fn thumbnail_mesh_cache_reuses_and_invalidates_transform_clip_dpi_and_options() {
    let page = ink_benchmark_page();
    let thumbnail = PageThumbnail::new(&page, ("doc", 1));
    let ctx = egui::Context::default();
    let target = Rect::from_min_size(Pos2::ZERO, egui::vec2(300.0, 200.0));
    let canvas = egui::vec2(1200.0, 800.0);
    let mut previous = None;
    for step in 0..7 {
        if step == 4 {
            ctx.set_pixels_per_point(2.0);
        }
        if step == 5 {
            ctx.tessellation_options_mut(|options| options.feathering = false);
        }
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let clip = if step >= 3 {
                target.shrink(5.0)
            } else {
                target
            };
            let target = if step >= 2 {
                target.translate(egui::vec2(2.0, 2.0))
            } else {
                target
            };
            // Same target and clip, changed canvas scale: zoom must not reuse old simplified mesh.
            let canvas = if step == 6 { canvas * 0.5 } else { canvas };
            let painter = ui.painter().with_clip_rect(clip);
            thumbnail
                .paint(&painter, target, canvas, false, &NoResources)
                .unwrap();
            let mesh = thumbnail.meshes.borrow().as_ref().unwrap().batches[0]
                .2
                .clone();
            if let Some(previous) = &previous {
                assert_eq!(std::sync::Arc::ptr_eq(previous, &mesh), step == 1);
            }
            previous = Some(mesh);
        });
        output.textures_delta.clear();
    }
}

#[test]
fn cached_page_invalidates_on_edits_and_circle_bounds_stay_round() {
    let mut renderer = PageRenderer::new();
    let context = egui::Context::default();
    let mut page = page(shape(ShapeKind::Circle, vec![p(10.0, 10.0), p(90.0, 50.0)]));
    let bounds = object_bounds(&page.objects[0]);
    assert_eq!(bounds.width(), bounds.height());
    assert_eq!(bounds.width(), 43.0);
    let mut output = context.run_ui(egui::RawInput::default(), |ui| {
        renderer
            .paint_page_with_resources(ui.painter(), &page, false, &NoResources)
            .unwrap();
        let pointer = renderer.objects[&page.objects[0].id].scene.items.as_ptr();
        renderer
            .paint_page_with_resources(ui.painter(), &page, true, &NoResources)
            .unwrap();
        assert_eq!(
            pointer,
            renderer.objects[&page.objects[0].id].scene.items.as_ptr()
        );
        page.objects[0].kind = shape(ShapeKind::Line, vec![p(1.0, 1.0), p(5.0, 5.0)]);
        renderer
            .paint_page_with_resources(ui.painter(), &page, false, &NoResources)
            .unwrap();
        assert_eq!(renderer.objects[&page.objects[0].id].scene.items.len(), 1);
        page.objects.clear();
        renderer
            .paint_page_with_resources(ui.painter(), &page, false, &NoResources)
            .unwrap();
        assert!(renderer.objects.is_empty());
    });
    output.textures_delta.clear();
    renderer.clear();
    assert!(renderer.objects.is_empty());
}
