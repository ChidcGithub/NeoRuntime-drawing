// 画布：输入手势分发、预览、纹理同步与页面渲染调用。从 gui.rs 拆出（纯移动）。
use super::*;

impl BoardApp {
    pub(super) fn sync_erase_preview(&mut self) {
        let compact = self.compact_entry();
        let Some(gesture) = &mut self.gesture else {
            return;
        };
        let Some(erasing) = &mut gesture.erasing else {
            return;
        };
        if self.tool != Tool::Eraser
            || compact
            || gesture.document != self.session.document.id
            || gesture.page != self.session.document.current_page().id
            || gesture.revision != self.session.document.revision
        {
            self.gesture = None;
            return;
        }
        erasing.update(&gesture.points);
    }

    pub(super) fn canvas(&mut self, ui: &mut egui::Ui) {
        self.sync_erase_preview();
        if self.compact_entry() {
            return;
        }
        let rect = ui.max_rect();
        self.canvas_size = rect.size();
        let response = ui.interact(rect, "board_canvas".into(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let over_overlay = ui.input(|i| i.pointer.interact_pos()).is_some_and(|pos| {
            self.controls.iter().any(|r| r.contains(pos))
                || ui
                    .ctx()
                    .layer_id_at(pos)
                    .is_some_and(|layer| layer != ui.layer_id())
        });
        let blocked = self.tool == Tool::Mouse
            || self.confirm_close
            || self.authorization.is_some()
            || self.split_drag.is_some()
            || over_overlay;
        // 工具栏二级菜单不阻塞画布：按在工具栏/菜单以外时，这一次按下既收起
        // 菜单也照常落笔（菜单项自身的点击由 Popup 的 CloseOnClick 处理）。
        if !over_overlay && ui.input(|i| i.pointer.any_pressed()) {
            egui::Popup::close_all(ui.ctx());
            self.armed_toolbar_menu = None;
        }
        // 按页记忆视图：当前页与视图归属页不一致（本地翻页或宿主翻页）时存取切换。
        let current_page_id = self.session.document.current_page().id.clone();
        if self.view_page_id != current_page_id {
            let previous = std::mem::replace(&mut self.view_page_id, current_page_id);
            if !previous.is_empty() {
                self.page_views
                    .insert(previous, (self.view_zoom, self.view_pan));
            }
            let (zoom, pan) = self
                .page_views
                .get(&self.view_page_id)
                .copied()
                .unwrap_or((1.0, egui::Vec2::ZERO));
            self.view_zoom = zoom;
            self.view_pan = pan;
            self.pinch = None;
            self.select_touches.clear();
            self.view_drag = None;
            self.fling = egui::Vec2::ZERO;
        }
        // 画板是透明标注浮层：视图恒为 1:1，无缩放平移（防御性强制）。
        if self.mode != AppMode::Blackboard {
            self.view_zoom = 1.0;
            self.view_pan = egui::Vec2::ZERO;
            self.pinch = None;
            self.view_drag = None;
            self.fling = egui::Vec2::ZERO;
        }
        // 滚轮平移；Ctrl+滚轮以指针为锚缩放（10%–500%）；中键拖动平移。
        // 直接读原始滚轮事件：不走 egui 平滑，画布平移需要即时跟手。
        // 触控书写路径绝不触发缩放/平移（多指只写字）。
        let mut scroll = egui::Vec2::ZERO;
        let mut ctrl_scroll = false;
        ui.input(|i| {
            for event in &i.raw.events {
                if let egui::Event::MouseWheel {
                    delta, modifiers, ..
                } = event
                {
                    scroll += *delta;
                    ctrl_scroll |= modifiers.ctrl;
                }
            }
        });
        // 无限黑板仅黑板模式：画板是透明标注浮层，恒为 1:1（板面=屏幕）。
        if !blocked && self.mode == AppMode::Blackboard && scroll != egui::Vec2::ZERO {
            if ctrl_scroll {
                if let Some(cursor) = ui.input(|i| i.pointer.hover_pos()) {
                    let anchor = self.screen_to_board(cursor);
                    self.view_zoom =
                        (self.view_zoom * (scroll.y * 0.0015f32).exp()).clamp(0.1, 5.0);
                    self.view_pan =
                        cursor.to_vec2() - egui::Vec2::new(anchor.x, anchor.y) * self.view_zoom;
                    self.note_zoom_changed(ui.input(|i| i.time));
                }
            } else {
                self.view_pan += scroll;
            }
        }
        if !blocked && self.mode == AppMode::Blackboard && ui.input(|i| i.pointer.middle_down()) {
            let delta = ui.input(|i| i.pointer.delta());
            // 中键拖拽跟手：内容跟随指针。
            self.view_pan += delta;
            self.fling = egui::Vec2::ZERO;
        }
        // 惯性滑行：无按下时按指数摩擦衰减；任何按下立即停止。
        let any_down = ui.input(|i| i.pointer.any_down())
            || !self.select_touches.is_empty()
            || self.view_drag.is_some()
            || self.pinch.is_some();
        if any_down {
            self.fling = egui::Vec2::ZERO;
        } else if self.fling != egui::Vec2::ZERO {
            let dt = ui.input(|i| i.stable_dt).max(1.0 / 240.0);
            // 惯性沿松手速度方向继续（跟手语义）。
            self.view_pan += self.fling * dt;
            self.fling *= (-dt / 0.15f32).exp();
            if self.fling.length() < 20.0 {
                self.fling = egui::Vec2::ZERO;
            }
        }
        // 触控多指书写：全部手指走 Touch 事件路径（每指独立手势，上限 5）；
        // 无指按下时的指针按下回到鼠标/触控笔模式；触控模式下 egui 合成的
        // 指针事件只服务工具栏等 UI，不参与手势。
        // 仅存在触控事件时才收集：绝大多数帧（鼠标/触控笔）零分配。
        let has_touch = ui.input(|i| {
            i.raw
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Touch { .. }))
        });
        let touch_events: Vec<egui::Event> = if has_touch {
            ui.input(|i| {
                i.raw
                    .events
                    .iter()
                    .filter(|event| matches!(event, egui::Event::Touch { .. }))
                    .cloned()
                    .collect()
            })
        } else {
            Vec::new()
        };
        let frame_touch_start = touch_events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Touch {
                    phase: egui::TouchPhase::Start,
                    ..
                }
            )
        });
        for event in touch_events {
            let egui::Event::Touch { id, phase, pos, .. } = event else {
                continue;
            };
            match phase {
                egui::TouchPhase::Start => {
                    self.touch_writing = true;
                    if blocked {
                        continue;
                    }
                    let board = self.screen_to_board(pos);
                    // 五指手势触点追踪（与书写/捏合并行，锁定时才接管）。
                    self.swipe_touches.push((id, pos, pos));
                    if self.tool == Tool::Pen {
                        // 书写模式：每指独立手势，多指只写字。
                        if self.multi_gestures.len() >= 5 {
                            continue;
                        }
                        self.multi_gestures.push((
                            id,
                            Gesture {
                                document: self.session.document.id.clone(),
                                page: self.session.document.current_page().id.clone(),
                                revision: self.session.document.revision,
                                points: vec![StrokePoint {
                                    x: board.x,
                                    y: board.y,
                                    time: ui.input(|i| i.time),
                                    pressure: 1.0,
                                }],
                                original: None,
                                vertex: None,
                                resize: None,
                                erasing: None,
                            },
                        ));
                        self.ink_started();
                    } else if self.tool == Tool::Select {
                        // 选择模式：黑板触点参与捏合/平移；画板仅选择对象。
                        let blackboard = self.mode == AppMode::Blackboard;
                        if let Some(slot) = self
                            .select_touches
                            .iter_mut()
                            .find(|(touch_id, _)| *touch_id == id)
                        {
                            slot.1 = pos;
                        } else {
                            self.select_touches.push((id, pos));
                        }
                        if blackboard && self.select_touches.len() == 2 {
                            // 双指捏合接管：取消进行中的选择拖动。
                            let (a, b) = (self.select_touches[0].1, self.select_touches[1].1);
                            let mid = Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
                            self.pinch = Some(PinchGesture {
                                start_dist: a.distance(b).max(1.0),
                                start_zoom: self.view_zoom,
                                anchor: self.screen_to_board(mid),
                            });
                            self.gesture = None;
                            self.touch_gesture = None;
                            self.view_drag = None;
                        } else if let Some((original, vertex, resize)) = self.select_pick(board) {
                            self.selected = Some(original.id.clone());
                            self.gesture = Some(Gesture {
                                document: self.session.document.id.clone(),
                                page: self.session.document.current_page().id.clone(),
                                revision: self.session.document.revision,
                                points: vec![StrokePoint {
                                    x: board.x,
                                    y: board.y,
                                    time: ui.input(|i| i.time),
                                    pressure: 1.0,
                                }],
                                original: Some(original),
                                vertex,
                                resize,
                                erasing: None,
                            });
                            self.touch_gesture = Some(id);
                        } else if blackboard {
                            self.selected = None;
                            self.view_drag = Some(ViewDrag {
                                last: pos,
                                last_time: ui.input(|i| i.time),
                                velocity: egui::Vec2::ZERO,
                            });
                        } else {
                            // 画板：空白处触摸不平移。
                            self.selected = None;
                        }
                    } else if matches!(self.tool, Tool::Shape | Tool::Eraser | Tool::Coordinates) {
                        // 形状/橡皮/坐标系：首触点创建手势（与指针路径同构，
                        // 线段吸附/衰减/交点在触屏同样生效）。
                        if self.gesture.is_none() {
                            self.gesture = Some(Gesture {
                                document: self.session.document.id.clone(),
                                page: self.session.document.current_page().id.clone(),
                                revision: self.session.document.revision,
                                points: vec![StrokePoint {
                                    x: board.x,
                                    y: board.y,
                                    time: ui.input(|i| i.time),
                                    pressure: 1.0,
                                }],
                                original: None,
                                vertex: None,
                                resize: None,
                                erasing: (self.tool == Tool::Eraser).then(|| {
                                    editing::ErasePreview::new(&self.session.document, self.eraser)
                                }),
                            });
                            self.touch_gesture = Some(id);
                            self.ink_started();
                        }
                    }
                }
                egui::TouchPhase::Move => {
                    if blocked {
                        continue;
                    }
                    if let Some(entry) = self
                        .swipe_touches
                        .iter_mut()
                        .find(|(touch_id, _, _)| *touch_id == id)
                    {
                        entry.1 = pos;
                    }
                    let board = self.screen_to_board(pos);
                    let spacing = 1.0 / self.view_zoom.max(0.05);
                    if let Some(slot) = self
                        .select_touches
                        .iter_mut()
                        .find(|(touch_id, _)| *touch_id == id)
                    {
                        slot.1 = pos;
                        if self.select_touches.len() >= 2
                            && let Some(pinch) = self.pinch
                        {
                            // 双指捏合跟手：锚点板面点始终钉在触点中点下。
                            let (a, b) = (self.select_touches[0].1, self.select_touches[1].1);
                            let dist = a.distance(b).max(1.0);
                            let mid = Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
                            self.view_zoom =
                                (pinch.start_zoom * dist / pinch.start_dist).clamp(0.1, 5.0);
                            self.view_pan = mid.to_vec2()
                                - egui::Vec2::new(pinch.anchor.x, pinch.anchor.y) * self.view_zoom;
                            self.fling = egui::Vec2::ZERO;
                            self.note_zoom_changed(ui.input(|i| i.time));
                        } else if let Some(drag) = &mut self.view_drag {
                            let delta = pos - drag.last;
                            // 拖拽跟手：内容跟随手指。
                            self.view_pan += delta;
                            let dt =
                                ((ui.input(|i| i.time) - drag.last_time) as f32).max(1.0 / 240.0);
                            drag.velocity = drag.velocity * 0.7 + (delta / dt) * 0.3;
                            drag.last = pos;
                            drag.last_time = ui.input(|i| i.time);
                        } else if self.gesture.is_some()
                            && let Some(gesture) = self.gesture.as_mut()
                            && gesture.document == self.session.document.id
                            && gesture.page == self.session.document.current_page().id
                            && gesture.revision == self.session.document.revision
                            && gesture.points.len() < 8000
                            && gesture
                                .points
                                .last()
                                .is_none_or(|p| (p.x - board.x).hypot(p.y - board.y) >= spacing)
                        {
                            // 触摸选择拖动：推进手势点。此前本分支直接 continue，
                            // 手势只有起点，预览/提交均为零位移（选中却无法移动）。
                            gesture.points.push(StrokePoint {
                                x: board.x,
                                y: board.y,
                                time: ui.input(|i| i.time),
                                pressure: 1.0,
                            });
                        }
                        continue;
                    }
                    if let Some((_, gesture)) = self
                        .multi_gestures
                        .iter_mut()
                        .find(|(touch_id, _)| *touch_id == id)
                    {
                        if gesture.document == self.session.document.id
                            && gesture.page == self.session.document.current_page().id
                            && gesture.revision == self.session.document.revision
                            && gesture.points.len() < 8000
                            && gesture
                                .points
                                .last()
                                .is_none_or(|p| (p.x - board.x).hypot(p.y - board.y) >= spacing)
                        {
                            gesture.points.push(StrokePoint {
                                x: board.x,
                                y: board.y,
                                time: ui.input(|i| i.time),
                                pressure: 1.0,
                            });
                        }
                    } else if self.gesture.is_some()
                        && matches!(
                            self.tool,
                            Tool::Select | Tool::Shape | Tool::Eraser | Tool::Coordinates
                        )
                        && let Some(gesture) = &mut self.gesture
                        && gesture.document == self.session.document.id
                        && gesture.page == self.session.document.current_page().id
                        && gesture.revision == self.session.document.revision
                        && gesture.points.len() < 8000
                        && gesture
                            .points
                            .last()
                            .is_none_or(|p| (p.x - board.x).hypot(p.y - board.y) >= spacing)
                    {
                        // 触控拖动对象：选择手势与指针路径同构（板面坐标）。
                        gesture.points.push(StrokePoint {
                            x: board.x,
                            y: board.y,
                            time: ui.input(|i| i.time),
                            pressure: 1.0,
                        });
                    }
                }
                egui::TouchPhase::End => {
                    self.swipe_touches
                        .retain(|(touch_id, _, _)| *touch_id != id);
                    self.select_touches.retain(|(touch_id, _)| *touch_id != id);
                    if self.select_touches.len() < 2 {
                        self.pinch = None;
                    }
                    if let Some(drag) = self.view_drag.take() {
                        // 平移松手：按平滑末速惯性滑行。
                        self.fling = if drag.velocity.length() > 60.0 {
                            drag.velocity
                        } else {
                            egui::Vec2::ZERO
                        };
                    } else if self.gesture.is_some()
                        && matches!(
                            self.tool,
                            Tool::Select | Tool::Shape | Tool::Eraser | Tool::Coordinates
                        )
                        && self.touch_gesture == Some(id)
                        && !ui.input(|i| i.pointer.any_down())
                    {
                        // 触控手势松手：与指针路径同一提交管线。
                        self.touch_gesture = None;
                        self.finish_gesture_at(Instant::now());
                    }
                    if let Some(index) = self
                        .multi_gestures
                        .iter()
                        .position(|(touch_id, _)| *touch_id == id)
                    {
                        let (_, gesture) = self.multi_gestures.remove(index);
                        // 同帧内自身提交会触发 document_changed（并可能清理手势
                        // 队列）：先取走其余手指的手势，提交后以新 revision 放回，
                        // 并发书写不被自家提交打断。外部替换文档时仍全部取消。
                        let others: Vec<(egui::TouchId, Gesture)> =
                            self.multi_gestures.drain(..).collect();
                        if gesture.document != self.session.document.id
                            || gesture.page != self.session.document.current_page().id
                            || gesture.revision != self.session.document.revision
                        {
                            self.status = "绘制期间文档已更新，本次触控已取消".into();
                        } else if let Some(object) = self.preview_for(&gesture) {
                            self.commit_pen_object(object, Instant::now());
                            let new_revision = self.session.document.revision;
                            for (touch_id, mut other) in others {
                                other.revision = new_revision;
                                self.multi_gestures.push((touch_id, other));
                            }
                        } else {
                            for other in others {
                                self.multi_gestures.push(other);
                            }
                        }
                    }
                }
                egui::TouchPhase::Cancel => {
                    self.swipe_touches
                        .retain(|(touch_id, _, _)| *touch_id != id);
                    self.select_touches.retain(|(touch_id, _)| *touch_id != id);
                    if self.select_touches.len() < 2 {
                        self.pinch = None;
                    }
                    self.view_drag = None;
                    self.multi_gestures.retain(|(touch_id, _)| *touch_id != id);
                }
            }
        }
        if frame_touch_start {
            self.touch_writing = true;
        } else if self.multi_gestures.is_empty() && ui.input(|i| i.pointer.primary_pressed()) {
            // 无指按下时的指针按下：回到鼠标/触控笔模式。
            self.touch_writing = false;
        }
        // 五指手势（下滑最小化 / 左右滑翻页）：中央图标随进度反馈。
        if self.gestures_enabled && !blocked {
            self.update_swipe_watcher(ui.ctx(), ui.input(|i| i.time));
        } else {
            self.swipe = None;
            self.swipe_touches.clear();
        }
        if blocked {
            if ui.input(|i| i.pointer.any_released()) {
                self.gesture = None;
            }
            self.multi_gestures.clear();
            self.pinch = None;
            self.select_touches.clear();
            self.view_drag = None;
            self.fling = egui::Vec2::ZERO;
            self.swipe = None;
            self.swipe_touches.clear();
            self.touch_gesture = None;
        } else if !self.touch_writing
            && let Some(pos) = response.interact_pointer_pos()
        {
            if self.gesture.is_none()
                && self.view_drag.is_none()
                && ui.input(|i| i.pointer.primary_pressed())
                && (response.is_pointer_button_down_on() || response.clicked())
            {
                let board = self.screen_to_board(pos);
                let mut original = None;
                let mut vertex = None;
                let mut resize = None;
                if self.tool == Tool::Select {
                    if let Some((picked, picked_vertex, picked_resize)) = self.select_pick(board) {
                        self.selected = Some(picked.id.clone());
                        original = Some(picked);
                        vertex = picked_vertex;
                        resize = picked_resize;
                    } else {
                        // 选择工具在空白处按下：拖动平移黑板（松手带轻微惯性）。
                        self.selected = None;
                        self.view_drag = Some(ViewDrag {
                            last: pos,
                            last_time: ui.input(|i| i.time),
                            velocity: egui::Vec2::ZERO,
                        });
                    }
                }
                if self.view_drag.is_none() {
                    let start = ui
                        .input(|i| i.pointer.press_origin())
                        .map(|origin| self.screen_to_board(origin))
                        .unwrap_or(board);
                    self.gesture = Some(Gesture {
                        document: self.session.document.id.clone(),
                        page: self.session.document.current_page().id.clone(),
                        revision: self.session.document.revision,
                        points: vec![StrokePoint {
                            x: start.x,
                            y: start.y,
                            time: ui.input(|i| i.time),
                            pressure: 1.0,
                        }],
                        original,
                        vertex,
                        resize,
                        erasing: (self.tool == Tool::Eraser).then(|| {
                            editing::ErasePreview::new(&self.session.document, self.eraser)
                        }),
                    });
                    self.ink_started();
                }
            }
            let board = self.screen_to_board(pos);
            let spacing = 1.0 / self.view_zoom.max(0.05);
            if let Some(drag) = &mut self.view_drag {
                let delta = pos - drag.last;
                // 拖拽跟手：内容跟随手指（板面原点随手指同向移动）。
                self.view_pan += delta;
                let dt = ((ui.input(|i| i.time) - drag.last_time) as f32).max(1.0 / 240.0);
                drag.velocity = drag.velocity * 0.7 + (delta / dt) * 0.3;
                drag.last = pos;
                drag.last_time = ui.input(|i| i.time);
            } else if (response.is_pointer_button_down_on() || response.drag_stopped())
                && let Some(gesture) = &mut self.gesture
                && gesture.points.len() < 8000
                && gesture
                    .points
                    .last()
                    .is_none_or(|p| (p.x - board.x).hypot(p.y - board.y) >= spacing)
            {
                gesture.points.push(StrokePoint {
                    x: board.x,
                    y: board.y,
                    time: ui.input(|i| i.time),
                    pressure: 1.0,
                });
            }
            if ui.input(|i| i.pointer.any_released())
                && let Some(drag) = self.view_drag.take()
            {
                self.fling = if drag.velocity.length() > 60.0 {
                    drag.velocity
                } else {
                    egui::Vec2::ZERO
                };
            }
        }
        // #region debug-point B:canvas
        let debug_preview_start = debug_ink::start();
        // #endregion
        self.sync_erase_preview();
        self.sync_plot_state();
        let mut preview = self.preview();
        // 新画笔笔迹独立叠加，不能作为临时页面内容替换真实 revision 缓存。
        let pen_preview = if self.tool == Tool::Pen {
            preview.take()
        } else {
            None
        };
        let multi_previews: Vec<BoardObject> = self
            .multi_gestures
            .iter()
            .filter(|(_, gesture)| {
                gesture.document == self.session.document.id
                    && gesture.page == self.session.document.current_page().id
                    && gesture.revision == self.session.document.revision
            })
            .filter_map(|(_, gesture)| self.preview_for(gesture))
            .collect();
        let plot_preview_changed = preview.as_ref().is_some_and(|object| {
            self.plot_points
                .as_ref()
                .is_some_and(|state| state.object != *object)
        });
        let merge_hint = if blocked {
            None
        } else {
            preview.as_ref().and_then(|source| {
                self.plot_merge_target(source).map(|target| {
                    (
                        board_render::object_bounds(target),
                        features::merge_plots(source, target).is_some(),
                    )
                })
            })
        };
        let vertex_edit = self.polygon_vertex_edit();
        let erased = self
            .gesture
            .as_ref()
            .and_then(|gesture| gesture.erasing.as_ref())
            .and_then(|erasing| erasing.result.as_ref())
            .and_then(|result| result.as_ref().ok())
            .map(|(document, _)| document.current_page());
        let previewing = preview.is_some() || erased.is_some() || vertex_edit.is_some();
        // #region debug-point B:canvas
        let debug_preview_us = debug_ink::micros(debug_preview_start);
        let debug_clone_start = debug_ink::start();
        // #endregion
        let mut page = std::borrow::Cow::Borrowed(
            erased.unwrap_or_else(|| self.session.document.current_page()),
        );
        if let Some(Ok(edit)) = &vertex_edit {
            let mut document = self.session.document.clone();
            if edit.apply(&mut document).is_ok() {
                page = std::borrow::Cow::Owned(document.current_page().clone());
            }
        } else if let Some(preview) = preview {
            if page.objects.iter().any(|o| o.id == preview.id) {
                // 预览也通过 core 传播；不改真实文档或历史。
                let mut document = self.session.document.clone();
                if let Some(endpoint) = self.gesture.as_ref().and_then(|g| g.vertex) {
                    let ids: Vec<_> = document
                        .connections
                        .iter()
                        .filter(|c| c.line_id == preview.id && c.line_endpoint == endpoint)
                        .map(|c| c.id.clone())
                        .collect();
                    for id in ids {
                        let _ = document.disconnect(&page.id, document.revision, &id);
                    }
                }
                if document
                    .apply(
                        &page.id,
                        document.revision,
                        &[Operation::Update { object: preview }],
                    )
                    .is_ok()
                {
                    page = std::borrow::Cow::Owned(document.current_page().clone());
                }
            } else {
                page.to_mut().objects.push(preview);
            }
        }
        // #region debug-point B:canvas
        let debug_clone_us = debug_ink::micros(debug_clone_start);
        let debug_textures_start = debug_ink::start();
        // #endregion
        // 每帧轻量剪枝：资源被释放的纹理立即清除。
        self.textures
            .retain(|asset, _| self.session.resources.get(asset).is_some());
        // 按 revision 门控的引用剪枝：图片对象被删除（含撤销恢复）后
        // 不再被任何页引用的纹理立即释放；恢复引用时同一路径重建。
        // 纯渲染缓存语义：文档数据不受影响。
        if self.texture_prune_revision != Some(self.session.document.revision) {
            self.texture_prune_revision = Some(self.session.document.revision);
            let referenced: std::collections::HashSet<&str> = self
                .session
                .document
                .pages
                .iter()
                .flat_map(|page| page.objects.iter())
                .filter_map(|object| match &object.kind {
                    ObjectKind::Image { asset_ref, .. } => Some(asset_ref.as_str()),
                    _ => None,
                })
                .collect();
            self.textures.retain(|asset, _| {
                self.session.resources.get(asset).is_some() && referenced.contains(asset.as_str())
            });
        }
        for object in &page.objects {
            if let ObjectKind::Image { asset_ref, .. } = &object.kind
                && !self.textures.contains_key(asset_ref)
                && let Some(resource) = self.session.resources.get(asset_ref)
            {
                match features::decode_png(&resource.bytes) {
                    Ok(image) => {
                        self.textures.insert(
                            asset_ref.clone(),
                            ui.ctx()
                                .load_texture(asset_ref, image, egui::TextureOptions::LINEAR),
                        );
                    }
                    Err(e) => self.status = format!("图片解码失败：{e}"),
                }
            }
        }
        let resources = |asset: &str| self.textures.get(asset).map(egui::TextureHandle::id);
        // #region debug-point B:canvas
        let debug_textures_us = debug_ink::micros(debug_textures_start);
        let debug_render_start = debug_ink::start();
        // #endregion
        // 视图换算（本帧输入处理后的最终视图）；画布叠加与渲染共用同一变换。
        let view = self.view_transform();
        let (zoom, pan) = (self.view_zoom, self.view_pan);
        let to_screen = |p: Pos2| Pos2::new(p.x * zoom + pan.x, p.y * zoom + pan.y);
        let rendered = if previewing {
            // 临时内容不共享文档 revision；旧入口清除 revision 键，取消后会重建真实页。
            self.renderer.paint_page_with_resources_viewed(
                &painter,
                &page,
                self.mode == AppMode::Blackboard,
                &resources,
                view,
            )
        } else {
            self.renderer.paint_page_at_document_revision_viewed(
                &painter,
                &page,
                (&self.session.document.id, self.session.document.revision),
                self.mode == AppMode::Blackboard,
                &resources,
                view,
            )
        };
        if let Some(stroke) = &pen_preview {
            board_render::paint_object_with_resources_viewed(&painter, stroke, &resources, view);
        }
        for object in &multi_previews {
            board_render::paint_object_with_resources_viewed(&painter, object, &resources, view);
        }
        // #region debug-point B:canvas
        let debug_render_us = debug_ink::micros(debug_render_start);
        debug_ink::sample(
            0,
            "A,B",
            "gui.rs:canvas",
            ["preview", "page_clone_apply", "textures", "render"],
            [
                debug_preview_us,
                debug_clone_us,
                debug_textures_us,
                debug_render_us,
            ],
            || {
                serde_json::json!({
                    "page": debug_ink::counts(&page),
                    "gesture_points": self.gesture.as_ref().map_or(0, |g| g.points.len()),
                    "page_owned": matches!(page, std::borrow::Cow::Owned(_)),
                    "previewing": previewing || pen_preview.is_some(), "revision": self.session.document.revision,
                    "render_entry": if previewing { "content_cache" } else if pen_preview.is_some() { "revision_cache_plus_pen" } else { "revision_cache" },
                    "cache_hit": "not_exposed", "render_ok": rendered.is_ok(),
                    "blackboard_background_requested": self.mode == AppMode::Blackboard,
                })
            },
        );
        // #endregion
        if let Err(e) = rendered {
            if let board_render::RenderError::Math(error) = e {
                painter.text(
                    rect.left_top() + Vec2::splat(8.0),
                    Align2::LEFT_TOP,
                    format!(
                        "函数图绘制失败：{}",
                        features::plot_text(&error.to_string())
                    ),
                    egui::FontId::proportional(14.0),
                    Color32::LIGHT_RED,
                );
            } else {
                self.status = e.to_string();
            }
        }
        if let Some(result) = &vertex_edit {
            let hint = match result {
                Ok(edit) => edit.hint.to_string(),
                Err(error) => format!("无法连接，本次拖动取消：{error}"),
            };
            if !hint.is_empty() {
                painter.text(
                    rect.left_top() + Vec2::new(8.0, 32.0),
                    Align2::LEFT_TOP,
                    hint,
                    egui::FontId::proportional(16.0),
                    Color32::YELLOW,
                );
            }
        }
        if !plot_preview_changed
            && let Some(state) = &mut self.plot_points
            && let Some(Ok(report)) = &state.report
        {
            for (pos, _) in &report.candidates {
                painter.circle_filled(to_screen(*pos), 4.0, Color32::YELLOW);
            }
            if self.tool == Tool::Select
                && !blocked
                && response.clicked()
                && let Some(click) = response.interact_pointer_pos()
                && let Some((_, label)) = report
                    .candidates
                    .iter()
                    .filter(|(pos, _)| click.distance(to_screen(*pos)) <= 10.0)
                    .min_by(|(a, _), (b, _)| click.distance(*a).total_cmp(&click.distance(*b)))
            {
                state.coordinate = Some(label.clone());
            }
        }
        if let Some(object) = self
            .selected
            .as_ref()
            .and_then(|id| page.objects.iter().find(|o| &o.id == id))
        {
            painter.rect_stroke(
                view.mul_rect(board_render::object_bounds(object))
                    .expand(4.0),
                0.0,
                Stroke::new(1.0, Color32::LIGHT_BLUE),
                egui::StrokeKind::Outside,
            );
            for p in editing::vertices(object) {
                painter.circle_filled(to_screen(Pos2::new(p.x, p.y)), 5.0, Color32::LIGHT_BLUE);
            }
            for p in editing::resize_handles(object) {
                painter.rect_filled(
                    Rect::from_center_size(to_screen(Pos2::new(p.x, p.y)), Vec2::splat(10.0)),
                    0.0,
                    Color32::LIGHT_BLUE,
                );
            }
        }
        if let Some((bounds, allowed)) = merge_hint {
            let color = if allowed {
                Color32::LIGHT_GREEN
            } else {
                Color32::YELLOW
            };
            painter.rect_stroke(
                view.mul_rect(bounds).expand(4.0),
                0.0,
                Stroke::new(3.0, color),
                egui::StrokeKind::Outside,
            );
            painter.text(
                to_screen(bounds.left_top()) + Vec2::splat(8.0),
                Align2::LEFT_TOP,
                if allowed {
                    "松手合并函数图"
                } else {
                    PLOT_MERGE_LIMIT_HINT
                },
                egui::FontId::proportional(16.0),
                color,
            );
        }
        // 五指手势中央图标：随进度变大变实，达阈值弹跳提示松手执行。
        if let Some(swipe) = &self.swipe {
            let progress = swipe.progress.min(1.0);
            let center = Pos2::new(rect.center().x, rect.center().y);
            let base = (rect.width().min(rect.height()) * 0.09).clamp(28.0, 64.0);
            let pop = if swipe.popped {
                let dt = ((ui.input(|i| i.time) - swipe.pop_time) as f32).max(0.0);
                1.0 + 0.35 * (-dt / 0.09f32).exp()
            } else {
                1.0
            };
            let radius = base * (0.55 + 0.85 * progress) * pop;
            let alpha = (70.0 + 150.0 * progress) as u8;
            let tint = Color32::from_rgba_unmultiplied(255, 255, 255, alpha);
            painter.circle_filled(
                center,
                radius,
                Color32::from_rgba_unmultiplied(255, 255, 255, alpha / 3),
            );
            painter.circle_stroke(center, radius, Stroke::new(2.0, tint));
            let dir = match swipe.axis {
                SwipeAxis::Down => egui::vec2(0.0, 1.0),
                SwipeAxis::Left => egui::vec2(-1.0, 0.0),
                SwipeAxis::Right => egui::vec2(1.0, 0.0),
            };
            let shaft = radius * 0.55;
            let head = radius * 0.3;
            let tip = center + dir * shaft;
            let tail = center - dir * shaft;
            let side = egui::vec2(-dir.y, dir.x);
            let stroke = Stroke::new(3.0 * pop, tint);
            painter.line_segment([tail, tip], stroke);
            painter.line_segment([tip, tip - dir * head + side * head], stroke);
            painter.line_segment([tip, tip - dir * head - side * head], stroke);
            if swipe.popped {
                painter.text(
                    center + egui::vec2(0.0, radius + 18.0),
                    Align2::CENTER_TOP,
                    match swipe.axis {
                        SwipeAxis::Down => "松手最小化",
                        SwipeAxis::Left => "松手上一页",
                        SwipeAxis::Right => "松手下一页",
                    },
                    egui::FontId::proportional(16.0),
                    tint,
                );
            }
        }
        // 缩放比例提示：缩放时淡入，停止缩放 1 秒后淡出（工具栏上方居中）。
        let mut hud_expired = false;
        if let Some(hud) = &self.zoom_hud {
            let now = ui.input(|i| i.time);
            let hold_end = hud.last_change + 1.0;
            // 过期只按时间判定：淡入首帧 alpha 为 0 不算淡出完成。
            if now > hold_end + 0.3 {
                hud_expired = true;
            } else {
                let fade_in = (((now - hud.appeared) / 0.15) as f32).clamp(0.0, 1.0);
                let fade_out = if now > hold_end {
                    (1.0 - ((now - hold_end) / 0.3) as f32).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let alpha = fade_in * fade_out;
                let text = format!("{}%", (self.view_zoom * 100.0).round() as i32);
                let font = egui::FontId::proportional(22.0);
                let text_size = painter
                    .layout_no_wrap(text.clone(), font.clone(), Color32::WHITE)
                    .size();
                // 工具栏锚定底部居中：提示放在其上方。
                let center = Pos2::new(rect.center().x, rect.bottom() - 96.0);
                let pill = Rect::from_center_size(center, text_size + egui::vec2(30.0, 16.0));
                painter.rect_filled(
                    pill,
                    pill.height() / 2.0,
                    Color32::from_rgba_unmultiplied(28, 28, 30, (alpha * 190.0) as u8),
                );
                painter.text(
                    pill.center(),
                    Align2::CENTER_CENTER,
                    text,
                    font,
                    Color32::from_rgba_unmultiplied(255, 255, 255, (alpha * 235.0) as u8),
                );
            }
            // 淡入/淡出动画期间保持重绘。
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
        }
        if hud_expired {
            self.zoom_hud = None;
        }
        if blocked {
            return;
        }
        if self.tool == Tool::Eraser
            && let Some(pos) = response.hover_pos()
        {
            // 橡皮半径为板面单位，屏幕半径按缩放折算。
            painter.circle_stroke(
                pos,
                self.eraser * zoom,
                Stroke::new(1.0, Color32::LIGHT_BLUE),
            );
        }
        if response.clicked()
            && !response.drag_stopped()
            && let Some(object) = response
                .interact_pointer_pos()
                .map(|pos| self.screen_to_board(pos))
                .and_then(|board| self.clicked_image(Pos2::new(board.x, board.y)))
        {
            self.open_image_agent(&object);
        } else if response.drag_stopped() || response.clicked() {
            self.finish_gesture();
        }
        if response.hovered() && self.tool == Tool::Select && self.gesture.is_none() {
            let scroll: f32 = ui.input(|i| {
                i.raw
                    .events
                    .iter()
                    .filter_map(|event| {
                        if let egui::Event::MouseWheel { delta, .. } = event {
                            Some(delta.y)
                        } else {
                            None
                        }
                    })
                    .sum()
            });
            if scroll != 0.0
                && let Some(mut object) = self.selected_object()
                && matches!(
                    object.kind,
                    ObjectKind::FunctionPlot { .. } | ObjectKind::CoordinateSystem { .. }
                )
                && response.hover_pos().is_some_and(|pos| {
                    view.mul_rect(board_render::object_bounds(&object))
                        .contains(pos)
                })
            {
                editing::scale_plot(&mut object, if scroll > 0.0 { 0.9 } else { 1.1 });
                self.apply(vec![Operation::Update { object }]);
            }
        }
    }
    pub(super) fn polygon_vertex_edit(&self) -> Option<board_core::Result<editing::VertexEdit>> {
        let gesture = self.gesture.as_ref()?;
        let original = gesture.original.as_ref()?;
        let index = gesture.vertex?;
        if self.tool != Tool::Select
            || gesture.resize.is_some()
            || gesture.document != self.session.document.id
            || gesture.page != self.session.document.current_page().id
            || gesture.revision != self.session.document.revision
            || !matches!(original.kind, ObjectKind::Shape { shape, .. } if shape != ShapeKind::Line)
        {
            return None;
        }
        let first = gesture.points.first()?;
        let last = gesture.points.last()?;
        let query = if first.x == last.x && first.y == last.y {
            *editing::vertices(original).get(index)?
        } else {
            Point {
                x: last.x,
                y: last.y,
            }
        };
        Some(editing::polygon_vertex_edit(
            &self.session.document,
            original,
            index,
            query,
        ))
    }

    pub(super) fn preview(&self) -> Option<BoardObject> {
        if let Some(edit) = self.polygon_vertex_edit() {
            return edit.ok().map(|edit| edit.object);
        }
        self.preview_for(self.gesture.as_ref()?)
    }

    /// Per-gesture preview: the pointer path passes the primary gesture and the
    /// touch path passes each extra-finger gesture; same object construction.
    pub(super) fn preview_for(&self, gesture: &Gesture) -> Option<BoardObject> {
        if gesture.document != self.session.document.id
            || gesture.page != self.session.document.current_page().id
            || gesture.revision != self.session.document.revision
        {
            return None;
        }
        let first = gesture.points.first()?;
        let last = gesture.points.last()?;
        // 吸附容差随拖动时长指数衰减（板面单位，按缩放折算屏幕像素）。
        let elapsed = (last.time - first.time).max(0.0) as f32;
        let snap_radius = features::drag_snap_radius(elapsed) / self.view_zoom.max(f32::EPSILON);
        let mut start = Point {
            x: first.x,
            y: first.y,
        };
        let mut end = Point {
            x: last.x,
            y: last.y,
        };
        let kind = match self.tool {
            Tool::Pen => ObjectKind::Stroke {
                points: gesture.points.clone(),
                style: self.style,
            },
            Tool::Shape => {
                if self.shape == ShapeKind::Line {
                    start = features::snap_endpoint_with_radius(
                        self.session.document.current_page(),
                        start,
                        None,
                        snap_radius,
                    );
                    // 线身穿交点吸附最优先（教师场景：画线精确穿过已有线段的
                    // 交点）；命中则跳过角度/端点吸附——角度吸附先执行会把线
                    // 拉到 15° 倍数方向上，导致穿交点时有时无。
                    if let Some(snapped) = features::snap_segment_through_intersections(
                        self.session.document.current_page(),
                        start,
                        end,
                        None,
                        snap_radius,
                    ) {
                        end = snapped;
                    } else {
                        // 特定度数吸附容差同样随拖动时长指数衰减（6° → 1°，
                        // 角度容差与缩放无关，不做 zoom 折算）。
                        let angle_tolerance =
                            6.0 * (features::drag_snap_radius(elapsed) / features::SNAP_RADIUS_MAX);
                        end = board_ink::snap_angle(start, end, angle_tolerance).unwrap_or(end);
                        end = features::snap_endpoint_with_radius(
                            self.session.document.current_page(),
                            end,
                            None,
                            snap_radius,
                        );
                    }
                }
                ObjectKind::Shape {
                    shape: self.shape,
                    points: vec![start, end],
                    style: self.style,
                }
            }
            Tool::Coordinates => ObjectKind::CoordinateSystem {
                origin: start,
                scale: (end.x - start.x).abs().clamp(40.0, 200.0),
                // 轴线匹配当前笔的粗细再细一点点。
                axis_width: Some(
                    (self.style.width * 0.8f32)
                        .clamp(board_core::MIN_BRUSH_WIDTH, board_core::MAX_BRUSH_WIDTH),
                ),
                three_d: false,
            },
            Tool::Select => {
                let mut object = gesture.original.clone()?;
                if let Some(index) = gesture.resize {
                    editing::resize_object(
                        &mut object,
                        index,
                        Point {
                            x: end.x - start.x,
                            y: end.y - start.y,
                        },
                    );
                } else if let Some(index) = gesture.vertex {
                    // A handle's hit area is larger than its endpoint; clicking is not a drag.
                    if start == end {
                        return Some(object);
                    }
                    end = editing::snap_vertex_angle(&object, index, end);
                    let endpoint_snapped = features::snap_endpoint_with_radius(
                        self.session.document.current_page(),
                        end,
                        Some(&object.id),
                        snap_radius,
                    );
                    // 端点吸附未命中时：拖动线段端点经过交点则吸附为经过之
                    // （仅 Line 的端点拖动；锚点为线的另一端）。
                    end = if endpoint_snapped == end {
                        let anchor = match (&object.kind, index) {
                            (
                                ObjectKind::Shape {
                                    shape: ShapeKind::Line,
                                    points,
                                    ..
                                },
                                i @ 0..=1,
                            ) => points.get(1 - i).copied(),
                            _ => None,
                        };
                        match anchor {
                            Some(anchor) => features::snap_segment_through_intersections(
                                self.session.document.current_page(),
                                anchor,
                                end,
                                Some(&object.id),
                                snap_radius,
                            )
                            .unwrap_or(end),
                            None => end,
                        }
                    } else {
                        endpoint_snapped
                    };
                    editing::move_vertex(&mut object, index, end);
                } else {
                    editing::translate(
                        &mut object,
                        Point {
                            x: end.x - start.x,
                            y: end.y - start.y,
                        },
                    );
                }
                return Some(object);
            }
            _ => return None,
        };
        Some(BoardObject {
            id: "local-preview".into(),
            kind,
        })
    }
}
