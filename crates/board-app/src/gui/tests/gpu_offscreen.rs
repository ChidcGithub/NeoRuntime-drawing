// #region debug-point E:F:G:H:gpu-offscreen
mod gpu_offscreen_debug {
    use super::*;
    use eframe::{egui_wgpu, wgpu};
    use std::{
        future::Future,
        sync::Arc,
        task::{Context, Poll, Wake, Waker},
    };

    fn report(data: serde_json::Value) {
        let body = serde_json::json!({
            "sessionId": "ink-white-lag", "runId": "gpu-offscreen-v1",
            "hypothesisId": "E:F:G:H", "location": "gui.rs:gpu_offscreen_debug",
            "msg": "[DEBUG] synthetic GPU texture summary", "data": data,
            "ts": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis(),
        }).to_string();
        eprintln!("{body}");
    }

    fn block_on<T>(future: impl Future<Output = T>) -> T {
        struct ThreadWake(std::thread::Thread);
        impl Wake for ThreadWake {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
            fn wake_by_ref(self: &Arc<Self>) {
                self.0.unpark();
            }
        }
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut cx = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
                return value;
            }
            assert!(Instant::now() < deadline, "GPU future timed out");
            std::thread::park_timeout(Duration::from_millis(10));
        }
    }

    fn run(backend: wgpu::Backends) {
        let result = std::panic::catch_unwind(|| probe(backend));
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                report(
                    serde_json::json!({"backend": format!("{backend:?}"), "status": "unavailable_or_failed", "error": error}),
                );
                panic!("offscreen GPU diagnostic failed: {error}");
            }
            Err(error) => {
                let message = error
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| error.downcast_ref::<&str>().copied())
                    .unwrap_or("non-string panic");
                report(
                    serde_json::json!({"backend": format!("{backend:?}"), "status": "panic", "error": message}),
                );
                std::panic::resume_unwind(error);
            }
        }
    }

    fn probe(backend: wgpu::Backends) -> std::result::Result<(), String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: backend,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            ..Default::default()
        }))
        .map_err(|e| format!("request_adapter: {e}"))?;
        let info = adapter.get_info();
        report(
            serde_json::json!({"status": "adapter", "backend": format!("{:?}", info.backend),
            "device_type": format!("{:?}", info.device_type), "name": info.name,
            "driver": info.driver, "driver_info": info.driver_info,
            "surface_created": false, "format": "Bgra8Unorm", "ppp": 2, "dithering": false}),
        );
        let (device, queue) =
            block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| format!("request_device: {e}"))?;
        let mut failures = Vec::new();
        for scene in [
            "clear",
            "black_rect",
            "black_shadow",
            "black_stroke",
            "black_cached_stroke",
            "red_stroke",
            "drawing_ui",
        ] {
            let ctx = egui::Context::default();
            let full_ui = scene == "drawing_ui";
            let (width, height) = if full_ui { (1280, 960) } else { (256, 256) };
            let mut input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(width as f32 / 2.0, height as f32 / 2.0),
                )),
                ..Default::default()
            };
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .unwrap()
                .native_pixels_per_point = Some(2.0);
            let mut drawing = app();
            drawing.mode = AppMode::Drawing;
            let mut output = egui::FullOutput::default();
            for _ in 0..if full_ui { 3 } else { 1 } {
                output.append(ctx.run_ui(input.clone(), |ui| {
                    if full_ui {
                        drawing.board_ui(ui, &ctx);
                        return;
                    }
                    let painter = ui.painter();
                    let rect =
                        Rect::from_min_max(egui::pos2(32.0, 32.0), egui::pos2(96.0, 96.0));
                    match scene {
                        "clear" => {}
                        "black_rect" => {
                            painter.rect_filled(rect, 0.0, Color32::from_black_alpha(128));
                        }
                        "black_shadow" => {
                            painter.add(
                                egui::Shadow {
                                    offset: [0, 4],
                                    blur: 12,
                                    spread: 0,
                                    color: Color32::from_black_alpha(96),
                                }
                                .as_shape(rect, 6),
                            );
                        }
                        _ => {
                            let object = BoardObject {
                                id: "synthetic".into(),
                                kind: ObjectKind::Stroke {
                                    points: (0..24)
                                        .map(|i| StrokePoint {
                                            x: 24.0 + i as f32 * 3.0,
                                            y: 64.0 + (i as f32 * 0.3).sin() * 16.0,
                                            time: i as f64 * 0.01,
                                            pressure: 1.0,
                                        })
                                        .collect(),
                                    style: Style {
                                        width: 8.0,
                                        dashed: false,
                                        color: Color {
                                            r: if scene == "red_stroke" { 255 } else { 0 },
                                            g: 0,
                                            b: 0,
                                            a: 255,
                                        },
                                    },
                                },
                            };
                            if scene == "black_cached_stroke" {
                                let page = board_core::Page {
                                    id: "synthetic-page".into(),
                                    objects: vec![object],
                                };
                                board_render::PageRenderer::new()
                                    .paint_page_at_revision(
                                        painter,
                                        &page,
                                        ("synthetic-document", 0),
                                        false,
                                        &|_: &str| None,
                                    )
                                    .unwrap();
                            } else {
                                board_render::paint_object(painter, &object);
                            }
                        }
                    }
                }));
            }
            // Own the deltas before fallible GPU work, so a diagnostic failure cannot
            // double-panic in TexturesDelta::drop and hide the original error.
            let texture_sets = std::mem::take(&mut output.textures_delta.set);
            let texture_frees = std::mem::take(&mut output.textures_delta.free);
            assert_eq!(output.pixels_per_point, 2.0);
            let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
            let mut input_bad_rgb = 0_u64;
            let mut vertices = 0_u64;
            for job in &jobs {
                if let egui::epaint::Primitive::Mesh(mesh) = &job.primitive {
                    for v in &mesh.vertices {
                        vertices += 1;
                        input_bad_rgb += u64::from(
                            v.color.g() != 0
                                || v.color.b() != 0
                                || (scene != "red_stroke" && v.color.r() != 0),
                        );
                    }
                }
            }
            let mut renderer = egui_wgpu::Renderer::new(
                &device,
                wgpu::TextureFormat::Bgra8Unorm,
                egui_wgpu::RendererOptions {
                    dithering: false,
                    ..Default::default()
                },
            );
            for (id, deltas) in &texture_sets {
                for delta in deltas {
                    renderer.update_texture(&device, &queue, *id, delta);
                }
            }
            let size = wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            };
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("debug synthetic offscreen only"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("debug synthetic readback"),
                size: u64::from(width) * u64::from(height) * 4,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            let screen = egui_wgpu::ScreenDescriptor {
                size_in_pixels: [width, height],
                pixels_per_point: 2.0,
            };
            let mut commands =
                renderer.update_buffers(&device, &queue, &mut encoder, &jobs, &screen);
            {
                let attachments = [Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })];
                let mut pass = encoder
                    .begin_render_pass(&wgpu::RenderPassDescriptor {
                        color_attachments: &attachments,
                        ..Default::default()
                    })
                    .forget_lifetime();
                renderer.render(&mut pass, &jobs, &screen);
            }
            encoder.copy_texture_to_buffer(
                texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(width * 4),
                        rows_per_image: Some(height),
                    },
                },
                size,
            );
            commands.push(encoder.finish());
            let submission = queue.submit(commands);
            let (tx, rx) = mpsc::channel();
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = tx.send(result);
                });
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(submission),
                    timeout: Some(Duration::from_secs(30)),
                })
                .map_err(|e| format!("{scene} poll: {e}"))?;
            rx.recv_timeout(Duration::from_secs(5))
                .map_err(|e| format!("{scene} map callback: {e}"))?
                .map_err(|e| format!("{scene} map: {e}"))?;
            let mapped = buffer
                .slice(..)
                .get_mapped_range()
                .map_err(|e| format!("{scene} mapped range: {e}"))?;
            let mut bad_rgb = 0_u64;
            let mut white = 0_u64;
            let mut clear = 0_u64;
            let mut partial_alpha = 0_u64;
            let mut nonzero_alpha = 0_u64;
            let mut rgb_at_zero_alpha = 0_u64;
            let mut nonpremultiplied = 0_u64;
            let mut max_rgba = [0_u8; 4];
            let mut min_alpha = 255_u8;
            let mut border_bad = 0_u64;
            let mut rect_interior_bad_alpha = 0_u64;
            let mut canvas_nonclear = 0_u64;
            for (i, p) in mapped.chunks_exact(4).enumerate() {
                let rgba = [p[2], p[1], p[0], p[3]];
                let [r, g, b, a] = rgba;
                for c in 0..4 {
                    max_rgba[c] = max_rgba[c].max(rgba[c]);
                }
                min_alpha = min_alpha.min(a);
                bad_rgb += u64::from(g != 0 || b != 0 || (scene != "red_stroke" && r != 0));
                white += u64::from(r == 255 && g == 255 && b == 255);
                clear += u64::from(rgba == [0; 4]);
                nonzero_alpha += u64::from(a != 0);
                partial_alpha += u64::from(a > 0 && a < 255);
                rgb_at_zero_alpha += u64::from(a == 0 && (r != 0 || g != 0 || b != 0));
                nonpremultiplied += u64::from(r.max(g).max(b) > a.saturating_add(1));
                let (x, y) = (i % width as usize, i / width as usize);
                if full_ui && y < height as usize / 2 {
                    canvas_nonclear += u64::from(rgba != [0; 4]);
                }
                if x < 8 || y < 8 || x >= width as usize - 8 || y >= height as usize - 8 {
                    border_bad += u64::from(rgba != [0; 4]);
                }
                if scene == "black_rect" && (70..186).contains(&x) && (70..186).contains(&y) {
                    rect_interior_bad_alpha += u64::from(a != 128);
                }
            }
            drop(mapped);
            buffer.unmap();
            for id in &texture_frees {
                renderer.free_texture(id);
            }
            let cap = match scene {
                "clear" => 0,
                "black_rect" => 128,
                "black_shadow" => 96,
                _ => 255,
            };
            let alpha_ok = min_alpha == 0
                && max_rgba[3] <= cap
                && border_bad == 0
                && rect_interior_bad_alpha == 0
                && if scene == "clear" {
                    clear == 65536
                } else {
                    nonzero_alpha > 0 && partial_alpha > 0
                };
            let ok = if full_ui {
                canvas_nonclear == 0 && nonzero_alpha > 0
            } else {
                input_bad_rgb == 0 && bad_rgb == 0 && alpha_ok
            } && rgb_at_zero_alpha == 0
                && nonpremultiplied == 0;
            report(
                serde_json::json!({"backend": format!("{:?}", info.backend), "scene": scene,
                "status": if ok { "pass" } else { "unexpected_pixels" }, "pixel_count": width * height,
                "canvas_nonclear_pixels": canvas_nonclear,
                "input_vertices": vertices, "input_bad_rgb_vertices": (!full_ui).then_some(input_bad_rgb),
                "texture_uploads": texture_sets.len(), "forbidden_rgb_pixels": (!full_ui).then_some(bad_rgb),
                "white_pixels": white, "clear_pixels": clear, "nonzero_alpha_pixels": nonzero_alpha,
                "partial_alpha_pixels": partial_alpha, "rgb_at_zero_alpha_pixels": rgb_at_zero_alpha,
                "nonpremultiplied_pixels": nonpremultiplied,
                "max_rgba": max_rgba, "min_alpha": min_alpha, "expected_alpha_max": cap,
                "border_nonclear_pixels": border_bad, "rect_interior_bad_alpha_pixels": rect_interior_bad_alpha,
                "alpha_bounds_ok": (!full_ui).then_some(alpha_ok)}),
            );
            if !ok {
                failures.push(scene);
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(format!("unexpected synthetic pixels: {failures:?}"))
        }
    }

    #[test]
    #[ignore = "explicit GPU diagnostic; stderr summaries only; no window or screen capture"]
    fn gpu_offscreen_dx12() {
        run(wgpu::Backends::DX12);
    }

    #[test]
    #[ignore = "explicit GPU diagnostic; stderr summaries only; no window or screen capture"]
    fn gpu_offscreen_vulkan() {
        run(wgpu::Backends::VULKAN);
    }
}
// #endregion
