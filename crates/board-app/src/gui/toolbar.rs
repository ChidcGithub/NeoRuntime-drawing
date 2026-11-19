// 工具栏与全部二级菜单：入口浮层、快捷键、页控、视图手势入口与弹层。
use super::*;

impl BoardApp {
    pub(super) fn compact_entry(&self) -> bool {
        self.collapsed && self.mode == AppMode::Blackboard
    }

    pub(super) fn set_collapsed(&mut self, ctx: &egui::Context, collapsed: bool) {
        if self.collapsed == collapsed {
            return;
        }
        self.collapsed = collapsed;
        self.toolbar_hidden = false;
        self.enforce_tool_mode();
        self.gesture = None;
        self.split_drag = None;
        self.cancel_ink();
        self.cancel_authorization();
        self.plot_worker.cancel();
        self.plot_points = None;
        self.files = false;
        self.math = false;
        self.confirm_close = false;
        egui::Popup::close_all(ctx);
        if self.mode == AppMode::Blackboard {
            if collapsed {
                // 收起 = 直接最小化窗口，并开启进程效率模式（EcoQoS）；
                // 用户点回任务栏恢复时由 logic() 展开并退出效率模式。
                ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
                efficiency::set_enabled(true);
            } else {
                // 恢复：退出效率模式并回到默认全屏。
                efficiency::set_enabled(false);
                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
            }
        }
        ctx.request_repaint();
    }

    pub(super) fn entry(&mut self, ctx: &egui::Context) {
        self.controls.clear();
        // Retry after unmaximizing has reached the native window (some WMs defer it).
        if ctx.input(|i| {
            i.viewport().maximized == Some(false)
                && i.viewport()
                    .inner_rect
                    .is_some_and(|r| r.size() != Vec2::new(220.0, 64.0))
        }) {
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::new(220.0, 64.0)));
        }
        let entry = egui::Area::new("blackboard_entry".into())
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ActionButton::secondary(egui::Id::new("top.expand"), "展开黑板")
                            .show(ui)
                            .clicked()
                        {
                            self.set_collapsed(ctx, false);
                        }
                        if ActionButton::secondary(egui::Id::new("top.exit"), "退出")
                            .show(ui)
                            .clicked()
                        {
                            self.request_close(ctx);
                        }
                    });
                });
            });
        self.controls.push(entry.response.rect);
    }

    pub(super) fn shortcuts(&mut self, ctx: &egui::Context) {
        self.enforce_tool_mode();
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.armed_toolbar_menu = None;
            self.toolbar_hidden = false;
            self.tool = Tool::Pen;
            self.set_collapsed(ctx, false);
            self.gesture = None;
            self.confirm_close = false;
            self.files = false;
            self.math = false;
            self.cancel_authorization();
            self.split_drag = None;
            self.cancel_ink();
            self.handwriting.invalidate_context();
            self.plot_worker.cancel();
            self.plot_points = None;
            // 原生命中检测由 logic 统一恢复，避免 egui 命令与 passthrough 缓存不同步。
            ctx.request_repaint();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::S)) {
            self.save();
        }
        if !ctx.egui_wants_keyboard_input() {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::Z)) {
                if self.math && self.handwriting.wants_sampling_undo(ctx) {
                    self.handwriting.undo_sampling();
                } else {
                    self.undo(false);
                }
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::Y)) {
                self.undo(true);
            }
        }
    }
    pub(super) fn toolbar(&mut self, ctx: &egui::Context) {
        // #region debug-point B:toolbar
        #[cfg(test)]
        let _debug_stage = debug_ink::Stage::begin(3, "toolbar");
        // #endregion
        if self.compact_entry() {
            self.entry(ctx);
            return;
        }
        self.controls.clear();
        self.enforce_tool_mode();
        let content = ctx.content_rect();
        let narrow = content.width() < 320.0;
        let compact = self.collapsed || self.toolbar_hidden || narrow;
        if compact {
            self.armed_toolbar_menu = None;
        }
        // 10% larger than the 1920×1080 baseline; viewport fitting still takes priority.
        let scale = toolbar_scale(content.size(), compact);
        let size = 42.0 * scale;
        let margin = (6.0 * scale).round() as i8;
        let frame_width = 2.0 * (f32::from(margin) + scale);
        let spacing = 4.0 * scale;
        let edge = 8.0 * scale;
        let gap = 12.0 * scale;
        let left_width = size + frame_width;
        let right_width = size * 2.0 + 56.0 * scale + spacing * 2.0 + frame_width;
        let width = if compact {
            if self.collapsed {
                size * 2.0 + spacing
            } else {
                size
            }
        } else {
            (content.width() - 2.0 * (edge + left_width.max(right_width) + gap) - frame_width)
                .clamp(1.0, 502.0 * scale)
        };
        let lower_switch = self.mode == AppMode::Drawing && !compact;
        let bottom = content.bottom()
            - if lower_switch {
                (58.0 * scale).max(52.0)
            } else {
                edge
            };
        let bar = egui::Area::new("tools".into())
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_BOTTOM, [0.0, bottom - content.bottom()])
            .movable(false)
            .default_size(Vec2::new(width + frame_width, size + frame_width))
            .show(ctx, |ui| {
                toolbar_style(ui, scale);
                egui::Frame::popup(ui.style())
                    .inner_margin(margin)
                    .stroke(egui::Stroke::new(scale, ui.visuals().window_stroke.color))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.set_height(size);
                        ui.spacing_mut().item_spacing.x = spacing;
                        ui.spacing_mut().interact_size = Vec2::splat(size);
                        if compact {
                            ui.horizontal(|ui| {
                                if icons::button(ui, Icon::Expand, "显示工具栏", size, false, true)
                                    .clicked()
                                {
                                    self.toolbar_hidden = false;
                                    self.set_collapsed(ctx, false);
                                }
                                if self.collapsed
                                    && icons::button(
                                        ui,
                                        Icon::Exit,
                                        "退出（未保存时确认）",
                                        size,
                                        false,
                                        true,
                                    )
                                    .clicked()
                                {
                                    self.request_close(ctx);
                                }
                            });
                            return;
                        }
                        // 更多与隐藏固定在中岛最右侧，其余工具可横向滚动。
                        let row = Rect::from_min_size(ui.cursor().min, Vec2::new(width, size));
                        ui.scope_builder(
                            egui::UiBuilder::new().max_rect(Rect::from_min_size(
                                Pos2::new(row.right() - size, row.top()),
                                Vec2::splat(size),
                            )),
                            |ui| {
                                if icons::button(
                                    ui,
                                    Icon::Collapse,
                                    "隐藏工具栏（保留板书）",
                                    size,
                                    false,
                                    true,
                                )
                                .clicked()
                                {
                                    self.toolbar_hidden = true;
                                    self.gesture = None;
                                    self.tool = Tool::Pen;
                                    egui::Popup::close_all(ctx);
                                }
                            },
                        );

                        ui.scope_builder(
                            egui::UiBuilder::new().max_rect(Rect::from_min_size(
                                Pos2::new(row.right() - size * 2.0 - spacing, row.top()),
                                Vec2::splat(size),
                            )),
                            |ui| {
                                let response =
                                    icons::button(ui, Icon::More, "更多 / 状态", size, false, true);
                                self.upward_popup(&response, false, |app, ui| app.more_menu(ui));
                            },
                        );
                        let left = Rect::from_min_max(
                            row.min,
                            Pos2::new(row.right() - (size + spacing) * 2.0, row.bottom()),
                        );
                        ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
                            egui::ScrollArea::horizontal()
                                .id_salt("toolbar_row")
                                .min_scrolled_width(1.0)
                                .max_width(left.width().max(1.0))
                                .max_height(size)
                                .auto_shrink([false, false])
                                .scroll_bar_visibility(
                                    egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                                )
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        self.toolbar_menu(ui, Icon::Pen, "画笔", |app, ui| {
                                            app.brush_menu(ui)
                                        });
                                        self.toolbar_menu(ui, Icon::Eraser, "橡皮", |app, ui| {
                                            app.eraser_menu(ui)
                                        });
                                        if icons::button(
                                            ui,
                                            Icon::Select,
                                            "选择 / 顶点编辑",
                                            size,
                                            self.tool == Tool::Select,
                                            true,
                                        )
                                        .clicked()
                                        {
                                            self.tool = Tool::Select;
                                        }
                                        if icons::button(
                                            ui,
                                            Icon::Line,
                                            "线段",
                                            size,
                                            self.tool == Tool::Shape
                                                && self.shape == ShapeKind::Line,
                                            true,
                                        )
                                        .clicked()
                                        {
                                            self.tool = Tool::Shape;
                                            self.shape = ShapeKind::Line;
                                        }
                                        self.toolbar_menu(
                                            ui,
                                            Icon::Shapes,
                                            "其他形状",
                                            |app, ui| app.shape_menu(ui),
                                        );
                                        for (redo, icon, label, enabled) in [
                                            (
                                                false,
                                                Icon::Undo,
                                                "撤销",
                                                self.session.history.can_undo(),
                                            ),
                                            (
                                                true,
                                                Icon::Redo,
                                                "重做",
                                                self.session.history.can_redo(),
                                            ),
                                        ] {
                                            if icons::button(ui, icon, label, size, false, enabled)
                                                .clicked()
                                            {
                                                self.undo(redo);
                                            }
                                        }
                                        self.toolbar_menu(
                                            ui,
                                            Icon::File,
                                            "文件 / 保存 / 载入 / 导出",
                                            |app, ui| app.file_menu(ui),
                                        );
                                        self.toolbar_menu(
                                            ui,
                                            Icon::Settings,
                                            "数学 / 模型 / 截图 / AI",
                                            |app, ui| app.settings_menu(ui),
                                        );
                                    })
                                });
                        });
                        ui.allocate_rect(row, Sense::hover());
                    });
            });
        self.controls.insert(0, bar.response.rect);
        if !self.collapsed && !narrow {
            for (id, anchor, x, island_width) in [
                ("board_management", Align2::LEFT_BOTTOM, edge, left_width),
                ("board_pages", Align2::RIGHT_BOTTOM, -edge, right_width),
            ] {
                let island = egui::Area::new(id.into())
                    .order(egui::Order::Foreground)
                    .anchor(anchor, [x, bottom - content.bottom()])
                    .movable(false)
                    .default_size(Vec2::new(island_width, size + frame_width))
                    .show(ctx, |ui| {
                        toolbar_style(ui, scale);
                        egui::Frame::popup(ui.style())
                            .inner_margin(margin)
                            .stroke(egui::Stroke::new(scale, ui.visuals().window_stroke.color))
                            .show(ui, |ui| {
                                ui.set_width(island_width - frame_width);
                                ui.set_height(size);
                                ui.set_clip_rect(ui.max_rect().intersect(content));
                                ui.spacing_mut().item_spacing.x = spacing;
                                ui.horizontal(|ui| {
                                    if id == "board_management" {
                                        let label = if self.mode == AppMode::Blackboard {
                                            "黑板菜单"
                                        } else {
                                            "画板菜单"
                                        };
                                        let response =
                                            icons::button(ui, Icon::More, label, size, false, true);
                                        self.upward_popup(&response, false, |app, ui| {
                                            app.management_menu(ui)
                                        });
                                    } else {
                                        self.page_controls(ui, size);
                                    }
                                });
                            });
                    });
                self.controls.push(island.response.rect);
                #[cfg(test)]
                ctx.data_mut(|d| d.insert_temp(egui::Id::new(id), island.response.rect));
            }
        }
        if lower_switch {
            let area = egui::Area::new("drawing_passthrough".into())
                .order(egui::Order::Foreground)
                .anchor(Align2::CENTER_BOTTOM, [0.0, -4.0])
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style())
                        .inner_margin(2)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let active = self.tool == Tool::Mouse;
                                let icon = icons::button(
                                    ui,
                                    Icon::Mouse,
                                    "鼠标穿透：点击退出；Esc 恢复画笔",
                                    36.0,
                                    active,
                                    cfg!(windows),
                                );
                                let label = ActionButton::secondary(
                                    egui::Id::new("passthrough.toggle"),
                                    if active {
                                        "退出鼠标穿透"
                                    } else {
                                        "鼠标穿透"
                                    },
                                )
                                .selected(active)
                                .enabled(cfg!(windows))
                                .show(ui);
                                if icon.clicked() || label.clicked() {
                                    self.set_mouse_passthrough(!active);
                                }
                            });
                        });
                });
            self.controls.push(area.response.rect);
        }
    }

    pub(super) fn enforce_tool_mode(&mut self) {
        if self.mode == AppMode::Blackboard && self.tool == Tool::Mouse {
            self.tool = Tool::Pen;
        }
    }

    pub(super) fn set_mouse_passthrough(&mut self, enabled: bool) {
        self.tool = if enabled && self.mode == AppMode::Drawing && cfg!(windows) {
            Tool::Mouse
        } else {
            Tool::Pen
        };
        self.gesture = None;
        if self.tool == Tool::Mouse {
            self.files = false;
            self.math = false;
        }
    }

    pub(super) fn management_menu(&mut self, ui: &mut egui::Ui) {
        let collapse = if self.mode == AppMode::Blackboard {
            "收起黑板"
        } else {
            "收起画板工具"
        };
        if ActionButton::secondary(egui::Id::new("menu.mgmt.collapse"), collapse)
            .show(ui)
            .clicked()
        {
            self.set_collapsed(ui.ctx(), true);
            ui.close();
        }
        let exit = if self.mode == AppMode::Blackboard {
            "退出黑板"
        } else {
            "退出画板"
        };
        if ActionButton::secondary(egui::Id::new("menu.mgmt.exit"), exit)
            .show(ui)
            .clicked()
        {
            self.request_close(ui.ctx());
            ui.close();
        }
    }

    pub(super) fn more_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("画笔", |ui| self.brush_menu(ui));
        ui.menu_button("其他形状", |ui| self.shape_menu(ui));
        if ActionButton::secondary(egui::Id::new("menu.more.select"), "选择 / 顶点编辑")
            .show(ui)
            .clicked()
        {
            self.tool = Tool::Select;
            ui.close();
        }
        ui.menu_button("橡皮", |ui| {
            if ActionButton::secondary(egui::Id::new("menu.eraser.use"), "使用橡皮")
                .show(ui)
                .clicked()
            {
                self.tool = Tool::Eraser;
                ui.close();
            }
            self.eraser_menu(ui);
        });
        ui.menu_button("文件 / 保存 / 载入 / 导出", |ui| self.file_menu(ui));
        ui.menu_button("数学 / 模型 / 截图 / AI", |ui| self.settings_menu(ui));
        if ActionButton::secondary(egui::Id::new("menu.more.undo"), "撤销")
            .enabled(self.session.history.can_undo())
            .show(ui)
            .clicked()
        {
            self.undo(false);
            ui.close();
        }
        if ActionButton::secondary(egui::Id::new("menu.more.redo"), "重做")
            .enabled(self.session.history.can_redo())
            .show(ui)
            .clicked()
        {
            self.undo(true);
            ui.close();
        }
        ui.menu_button("状态与任务", |ui| {
            ui.label(&self.status);
            ui.label("仅覆盖当前窗口所在显示器；跨屏请先恢复画笔。");
            ui.label("模板识别32笔/8192点；交点16曲线，最多136次搜索，每次2048求值；宿主任务60秒请求取消。");
            for (id, _) in self.jobs.clone() {
                if ActionButton::danger(egui::Id::new(("jobs.cancel", &id)), format!("取消任务 {id}"))
                    .show(ui)
                    .clicked()
                {
                    self.cancel_job(&id);
                }
            }
        });
    }

    pub(super) fn page_controls(&mut self, ui: &mut egui::Ui, size: f32) {
        let current = self.session.document.current_page;
        let count = self.session.document.pages.len();
        if icons::button(ui, Icon::Previous, "上一页", size, false, current > 0).clicked() {
            self.select_page(current - 1);
        }
        let response = ui
            .add_sized(
                [56.0 * size / 42.0, size],
                egui::Button::new(
                    egui::RichText::new(format!("{}/{}", current + 1, count))
                        .size(12.0 * size / 42.0),
                ),
            )
            .on_hover_text("页面管理：预览所有页面");
        #[cfg(test)]
        ui.ctx()
            .data_mut(|data| data.insert_temp(egui::Id::new("page_count_rect"), response.rect));
        let command = self.toolbar_popup_command(&response, false);
        if let Some(popup) = egui::Popup::menu(&response)
            .open_memory(command)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .align(egui::emath::RectAlign::TOP_END)
            .align_alternatives(&[])
            .width((ui.ctx().content_rect().width() - 32.0).clamp(1.0, 280.0))
            .show(|ui| self.pages_menu(ui))
        {
            self.controls.push(popup.response.rect);
        }
        let last = current + 1 == count;
        if icons::button(
            ui,
            if last { Icon::Plus } else { Icon::Next },
            if last { "添加页面" } else { "下一页" },
            size,
            false,
            !last || count < board_core::MAX_PAGES,
        )
        .clicked()
        {
            if last {
                let s = &mut self.session;
                match s.history.edit(&mut s.document, |d| d.add_page()) {
                    Ok(_) => {
                        self.gesture = None;
                        self.selected = None;
                        self.changed();
                    }
                    Err(e) => self.status = e.to_string(),
                }
            } else {
                self.select_page(current + 1);
            }
        }
    }

    // #region 无限黑板视图（板面→屏幕）
    pub(super) fn screen_to_board(&self, pos: Pos2) -> Point {
        Point {
            x: (pos.x - self.view_pan.x) / self.view_zoom,
            y: (pos.y - self.view_pan.y) / self.view_zoom,
        }
    }

    pub(super) fn view_transform(&self) -> egui::emath::TSTransform {
        egui::emath::TSTransform {
            scaling: self.view_zoom,
            translation: self.view_pan,
        }
    }

    /// 缩放发生：显示/刷新工具栏上方的比例提示（淡入，停止缩放 1 秒后淡出）。
    pub(super) fn note_zoom_changed(&mut self, time: f64) {
        match &mut self.zoom_hud {
            Some(hud) => hud.last_change = time,
            None => {
                self.zoom_hud = Some(ZoomHud {
                    appeared: time,
                    last_change: time,
                });
            }
        }
    }

    /// 五指手势监视：5 触点同向滑动锁向（速度门槛防书写误触发），
    /// 进度实时跟随手指；释放时 p≥1 才执行；手返回进度回落即撤销。
    pub(super) fn update_swipe_watcher(&mut self, ctx: &egui::Context, time: f64) {
        if self.swipe_touches.len() != 5 {
            // 执行后待全部手指抬起再武装。
            if self.swipe_touches.is_empty() {
                self.swipe_cooldown = false;
            }
            if let Some(swipe) = self.swipe.take()
                && self.swipe_touches.is_empty()
                && swipe.progress >= 1.0
            {
                self.execute_swipe(ctx, swipe.axis);
            }
            self.swipe_speed = 0.0;
            self.swipe_last = None;
            return;
        }
        if self.swipe_cooldown {
            return;
        }
        let threshold = self.gesture_threshold.max(60.0);
        let dead_zone = 30.0_f32;
        let mut mean = egui::Vec2::ZERO;
        for (_, current, start) in &self.swipe_touches {
            mean += *current - *start;
        }
        mean /= 5.0;
        // 中心速度（平滑）：滑动是快速挥动，缓慢移动按书写处理不锁向。
        let speed = match self.swipe_last {
            Some((last, last_time)) => {
                let dt = ((time - last_time) as f32).max(1.0 / 240.0);
                self.swipe_speed * 0.6 + ((mean - last.to_vec2()).length() / dt) * 0.4
            }
            None => self.swipe_speed,
        };
        self.swipe_speed = speed;
        self.swipe_last = Some((Pos2::new(mean.x, mean.y), time));
        // 方向锁：平均位移超死区后按主导轴锁定，本触摸序列内不换向。
        let axis = if let Some(swipe) = self.swipe {
            swipe.axis
        } else {
            if speed < 400.0 {
                return;
            }
            if mean.y.abs() >= mean.x.abs() {
                if mean.y > dead_zone {
                    SwipeAxis::Down
                } else {
                    return;
                }
            } else if mean.x <= -dead_zone {
                SwipeAxis::Left
            } else if mean.x >= dead_zone {
                SwipeAxis::Right
            } else {
                return;
            }
        };
        if self.swipe.is_none() {
            // 锁向前逐指校验：沿轴位移过半死区、横摆受锥角约束。
            for (_, current, start) in &self.swipe_touches {
                let d = *current - *start;
                let (along, cross) = match axis {
                    SwipeAxis::Down => (d.y, d.x.abs()),
                    SwipeAxis::Left => (-d.x, d.y.abs()),
                    SwipeAxis::Right => (d.x, d.y.abs()),
                };
                if along < dead_zone * 0.5 || cross > along * 0.6 {
                    return;
                }
            }
        }
        let along = match axis {
            SwipeAxis::Down => mean.y,
            SwipeAxis::Left => -mean.x,
            SwipeAxis::Right => mean.x,
        };
        let progress = (along / threshold).max(0.0);
        match &mut self.swipe {
            Some(swipe) => {
                swipe.progress = progress;
                if swipe.popped {
                    if progress < 0.85 {
                        // 迟滞退出弹跳态：手已明显返回。
                        swipe.popped = false;
                    }
                } else if progress >= 1.0 {
                    swipe.popped = true;
                    swipe.pop_time = time;
                }
            }
            None => {
                if progress >= dead_zone / threshold {
                    // 锁向即接管：手势优先，进行中的书写/捏合/拖动作废。
                    // 锁向瞬间已过阈值（快速挥动）则立即进入弹跳态。
                    let popped = progress >= 1.0;
                    self.swipe = Some(SwipeGesture {
                        axis,
                        progress,
                        popped,
                        pop_time: if popped { time } else { 0.0 },
                    });
                    self.multi_gestures.clear();
                    self.gesture = None;
                    self.touch_gesture = None;
                    self.pinch = None;
                    self.view_drag = None;
                    self.fling = egui::Vec2::ZERO;
                }
            }
        }
    }

    pub(super) fn execute_swipe(&mut self, ctx: &egui::Context, axis: SwipeAxis) {
        self.swipe_cooldown = true;
        match axis {
            SwipeAxis::Down => {
                self.status = "五指下滑：已最小化黑板".into();
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            SwipeAxis::Left => {
                let current = self.session.document.current_page;
                if current > 0 {
                    self.status = "五指左滑：上一页".into();
                    self.select_page(current - 1);
                }
            }
            SwipeAxis::Right => {
                let current = self.session.document.current_page;
                if current + 1 < self.session.document.pages.len() {
                    self.status = "五指右滑：下一页".into();
                    self.select_page(current + 1);
                }
            }
        }
    }

    /// 选择工具拾取：手柄/顶点（容差按缩放折算屏幕 12px）与对象命中（屏幕 8px）。
    pub(super) fn select_pick(
        &self,
        board: Point,
    ) -> Option<(BoardObject, Option<usize>, Option<usize>)> {
        let board_pos = Pos2::new(board.x, board.y);
        let handle_tolerance = 12.0 / self.view_zoom.max(f32::EPSILON);
        let hit_tolerance = 8.0 / self.view_zoom.max(f32::EPSILON);
        let mut original = None;
        let mut vertex = None;
        let mut resize = None;
        if let Some(object) = self.selected_object() {
            let nearest = |handles: Vec<Point>| {
                handles
                    .iter()
                    .enumerate()
                    .map(|(index, p)| (index, Pos2::new(p.x, p.y).distance(board_pos)))
                    .filter(|(_, distance)| *distance < handle_tolerance)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(index, _)| index)
            };
            vertex = nearest(editing::vertices(&object));
            resize = nearest(editing::resize_handles(&object));
            if vertex.is_some() || resize.is_some() {
                original = Some(object);
            }
        }
        if original.is_none() {
            original = self
                .session
                .document
                .current_page()
                .objects
                .iter()
                .rev()
                .find(|o| editing::hit_test(o, board_pos, hit_tolerance))
                .cloned();
        }
        let original = original?;
        Some((original, vertex, resize))
    }
    // #endregion

    pub(super) fn select_page(&mut self, page: usize) {
        if self.session.document.set_current_page(page).is_ok() {
            self.gesture = None;
            self.selected = None;
            self.changed();
        }
    }

    pub(super) fn toolbar_popup_command(
        &mut self,
        response: &egui::Response,
        requires_second_click: bool,
    ) -> Option<egui::SetOpenCommand> {
        let popup_id = egui::Popup::default_response_id(response);
        if response.clicked() {
            if !requires_second_click
                || egui::Popup::is_id_open(&response.ctx, popup_id)
                || self.armed_toolbar_menu == Some(response.id)
            {
                self.armed_toolbar_menu = None;
                return Some(egui::SetOpenCommand::Toggle);
            }
            egui::Popup::close_all(&response.ctx);
            self.armed_toolbar_menu = Some(response.id);
        } else if self.armed_toolbar_menu == Some(response.id)
            && (response.clicked_elsewhere() || response.ctx.input(|i| i.key_pressed(Key::Escape)))
        {
            self.armed_toolbar_menu = None;
        }
        None
    }

    pub(super) fn upward_popup(
        &mut self,
        response: &egui::Response,
        requires_second_click: bool,
        content: impl FnOnce(&mut Self, &mut egui::Ui),
    ) {
        let command = self.toolbar_popup_command(response, requires_second_click);
        if let Some(popup) = egui::Popup::menu(response)
            .open_memory(command)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .align(egui::emath::RectAlign::TOP_START)
            .align_alternatives(&[])
            .width((response.ctx.content_rect().width() - 32.0).clamp(1.0, 280.0))
            .show(|ui| {
                let max_height =
                    (response.rect.top() - ui.ctx().content_rect().top() - 24.0).max(1.0);
                egui::ScrollArea::vertical()
                    .max_height(max_height)
                    .show(ui, |ui| content(self, ui));
            })
        {
            self.controls.push(popup.response.rect);
        }
    }

    pub(super) fn toolbar_menu(
        &mut self,
        ui: &mut egui::Ui,
        icon: Icon,
        label: &str,
        content: impl FnOnce(&mut Self, &mut egui::Ui),
    ) {
        ui.push_id(label, |ui| {
            let tool = match icon {
                Icon::Pen => Some(Tool::Pen),
                Icon::Eraser => Some(Tool::Eraser),
                _ => None,
            };
            let size = ui.spacing().interact_size.y;
            let response = icons::button(ui, icon, label, size, tool == Some(self.tool), true);
            if response.clicked()
                && let Some(tool) = tool
            {
                self.tool = tool;
            }
            self.upward_popup(&response, tool.is_some(), content);
        });
    }

    pub(super) fn shape_menu(&mut self, ui: &mut egui::Ui) {
        for shape in SHAPES {
            if ActionButton::quiet(
                egui::Id::new(("shape.pick", shape_name(*shape))),
                shape_name(*shape),
            )
            .selected(self.tool == Tool::Shape && self.shape == *shape)
            .show(ui)
            .clicked()
            {
                self.shape = *shape;
                self.tool = Tool::Shape;
                ui.close();
            }
        }
        if self.mode == AppMode::Blackboard
            && ActionButton::quiet(egui::Id::new("shape.coordinates"), "坐标系")
                .selected(self.tool == Tool::Coordinates)
                .show(ui)
                .clicked()
        {
            self.tool = Tool::Coordinates;
            ui.close();
        }
        ui.separator();
        if ActionButton::danger(egui::Id::new("shape.disconnect"), "断开所选直线的连接")
            .enabled(self.selected.is_some())
            .show(ui)
            .clicked()
            && let Some(id) = self.selected.clone()
        {
            let s = &mut self.session;
            match editing::disconnect_line(&mut s.document, &mut s.history, &id) {
                Ok(()) => self.changed(),
                Err(e) => self.status = e.to_string(),
            }
        }
        ui.label("端点吸附连接；拖角调整圆/椭圆")
            .on_hover_text("直线连接后随目标移动，拖端点可重新连接；不支持二维/三维混接。\n选择圆/椭圆后，拖角手柄调整大小，拖轮廓移动；固定对角，圆等比、椭圆分轴。");
    }

    pub(super) fn brush_menu(&mut self, ui: &mut egui::Ui) {
        ui.label("画笔颜色");
        ui.horizontal(|ui| {
            for (color, name) in [
                (Color32::RED, "红色"),
                (Color32::BLACK, "黑色"),
                (Color32::WHITE, "白色"),
                (Color32::YELLOW, "黄色"),
                (Color32::GREEN, "绿色"),
                (Color32::BLUE, "蓝色"),
                (Color32::from_rgb(170, 70, 230), "紫色"),
            ] {
                let response = ui
                    .add_sized(
                        [24.0, 24.0],
                        egui::Button::new("")
                            .fill(color)
                            .selected(self.style.color == core_color(color)),
                    )
                    .on_hover_text(name);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name)
                });
                if response.clicked() {
                    self.style.color = core_color(color);
                }
            }
        });
        let mut color = egui_color(self.style.color);
        if ui.color_edit_button_srgba(&mut color).changed() {
            self.style.color = core_color(color);
        }
        ui.add(egui::Slider::new(&mut self.style.width, 0.5..=40.0).text("粗细"));
        ui.checkbox(&mut self.style.dashed, "虚线");
    }

    pub(super) fn eraser_menu(&mut self, ui: &mut egui::Ui) {
        ui.add(egui::Slider::new(&mut self.eraser, 2.0..=80.0).text("橡皮半径"));
        if ui
            .button("全部擦除")
            .on_hover_text("清除当前页全部对象（可撤销），保留其他页面")
            .clicked()
        {
            self.clear_current_page();
            ui.close();
        }
    }

    pub(super) fn clear_current_page(&mut self) {
        if self.session.document.current_page().objects.is_empty() {
            return;
        }
        let s = &mut self.session;
        // A page can exceed MAX_OPERATIONS; clear objects and connections in one transaction.
        match s.history.edit(&mut s.document, |d| {
            let page = &mut d.pages[d.current_page];
            page.objects.clear();
            d.connections
                .retain(|connection| connection.page_id != page.id);
            Ok(())
        }) {
            Ok(()) => {
                self.gesture = None;
                self.selected = None;
                self.changed();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    pub(super) fn pages_menu(&mut self, ui: &mut egui::Ui) {
        // #region debug-point B:thumbnails
        #[cfg(test)]
        let _debug_stage = debug_ink::Stage::begin(6, "pages_menu_thumbnails");
        // #endregion
        let current = self.session.document.current_page;
        let count = self.session.document.pages.len();
        ui.label(format!("第 {} / {} 页", current + 1, count));
        let mut selected = None;
        let mut visible = Vec::new();
        let height = (ui.ctx().content_rect().height() - 210.0).clamp(60.0, 420.0);
        egui::ScrollArea::vertical()
            .id_salt("page_previews")
            .max_height(height)
            .show_rows(ui, 112.0, count, |ui, rows| {
                for index in rows {
                    let page = &self.session.document.pages[index];
                    visible.push(page.id.clone());
                    let (rect, response) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 112.0),
                        Sense::click(),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Button,
                            true,
                            current == index,
                            format!("第 {} 页", index + 1),
                        )
                    });
                    if ui.is_rect_visible(rect) {
                        let target = Rect::from_min_max(
                            rect.min + Vec2::new(4.0, 4.0),
                            rect.max - Vec2::new(4.0, 24.0),
                        );
                        for object in &page.objects {
                            if let ObjectKind::Image { asset_ref, .. } = &object.kind
                                && !self.textures.contains_key(asset_ref)
                                && let Some(resource) = self.session.resources.get(asset_ref)
                                && let Ok(image) = features::decode_png(&resource.bytes)
                            {
                                self.textures.insert(
                                    asset_ref.clone(),
                                    ui.ctx().load_texture(
                                        asset_ref,
                                        image,
                                        egui::TextureOptions::LINEAR,
                                    ),
                                );
                            }
                        }
                        let version = (
                            self.session.document.id.as_str(),
                            self.session.document.revision,
                        );
                        if !self
                            .thumbnails
                            .get(&page.id)
                            .is_some_and(|p| p.matches(version))
                        {
                            self.thumbnails.insert(
                                page.id.clone(),
                                board_render::PageThumbnail::new(page, version),
                            );
                        }
                        // 缩略图按页内容包围盒缩放（无限画布：书写范围任意
                        // 大小与位置），留 5% 边距；空页回退固定画布尺寸。
                        let mut content = page
                            .objects
                            .iter()
                            .map(board_render::object_bounds)
                            .fold(Rect::NOTHING, Rect::union);
                        if content.is_positive() {
                            let margin = (content.width().max(content.height()) * 0.05).max(24.0);
                            content = content.expand(margin);
                        } else {
                            content = Rect::from_min_size(Pos2::ZERO, self.canvas_size);
                        }
                        let resources =
                            |asset: &str| self.textures.get(asset).map(egui::TextureHandle::id);
                        if self.thumbnails[&page.id]
                            .paint_with_bounds(
                                ui.painter(),
                                target,
                                content,
                                self.mode == AppMode::Blackboard,
                                &resources,
                            )
                            .is_err()
                        {
                            ui.painter()
                                .rect_filled(target, 0.0, Color32::from_gray(40));
                            ui.painter().text(
                                target.center(),
                                Align2::CENTER_CENTER,
                                "预览失败",
                                egui::FontId::proportional(14.0),
                                Color32::LIGHT_RED,
                            );
                        }
                        ui.painter().rect_stroke(
                            rect.shrink(1.0),
                            3.0,
                            Stroke::new(
                                if current == index { 2.0 } else { 1.0 },
                                if current == index {
                                    ui.visuals().selection.stroke.color
                                } else {
                                    ui.visuals().widgets.noninteractive.bg_stroke.color
                                },
                            ),
                            egui::StrokeKind::Inside,
                        );
                        ui.painter().text(
                            Pos2::new(rect.center().x, rect.bottom() - 12.0),
                            Align2::CENTER_CENTER,
                            format!("第 {} 页", index + 1),
                            egui::FontId::proportional(14.0),
                            ui.visuals().text_color(),
                        );
                    }
                    if response.clicked() {
                        selected = Some(index);
                    }
                }
            });
        self.thumbnails.retain(|id, _| visible.contains(id));
        if let Some(index) = selected {
            self.select_page(index);
            ui.close();
        }
        if ActionButton::danger(egui::Id::new("menu.page.clear"), "清屏（可撤销）")
            .show(ui)
            .clicked()
        {
            self.clear_current_page();
        }
    }

    pub(super) fn file_menu(&mut self, ui: &mut egui::Ui) {
        if ActionButton::primary(egui::Id::new("menu.file.save.current"), "保存到当前路径")
            .show(ui)
            .clicked()
        {
            self.save();
            ui.close();
        }
        if ActionButton::secondary(
            egui::Id::new("menu.file.panel"),
            "文件面板：路径 / 载入 / 图片 / 导出",
        )
        .show(ui)
        .clicked()
        {
            self.files = !self.files;
            self.tool = Tool::Pen;
            ui.close();
        }
    }

    pub(super) fn settings_menu(&mut self, ui: &mut egui::Ui) {
        if self.mode == AppMode::Blackboard
            && ActionButton::secondary(
                egui::Id::new("menu.settings.math"),
                "数学 / 手写识别 / 模型设置",
            )
            .show(ui)
            .clicked()
        {
            self.math = !self.math;
            self.tool = Tool::Pen;
            ui.close();
        }
        if self.hosted {
            let mut required = true;
            ui.add_enabled(false, egui::Checkbox::new(&mut required, "截图时隐藏窗口"))
                .on_disabled_hover_text(
                    "宿主截图协议要求隐藏全部所属窗口；不隐藏选项仅用于独立模式本地截图。",
                );
        } else {
            ui.add_enabled(
                self.local_capture.is_none(),
                egui::Checkbox::new(&mut self.capture_hide_window, "截图时隐藏窗口"),
            )
            .on_hover_text("默认隐藏；取消勾选会保留窗口和笔迹，它们可能进入截图。本次运行有效。");
        }
        if ActionButton::primary(egui::Id::new("capture.once"), "截图（本次授权）")
            .enabled(self.local_capture.is_none())
            .show(ui)
            .on_hover_text(if self.hosted {
                "点击授权本次宿主框选，成功自动上板、不发送 Agent；仍须宿主允许截图且关闭课堂安全模式。"
            } else { capture::LOCAL_HINT })
            .clicked()
        {
            self.screenshot_clicked();
            ui.close();
        }
        if ui
            .button("询问 AI / Agent（本次授权）")
            .on_hover_text("仅 hosted 宿主服务可用；打开授权面板，不自动发送图片或允许写回")
            .clicked()
        {
            self.cancel_authorization();
            self.authorization = Some(false);
            self.tool = Tool::Pen;
            ui.close();
        }
        ui.separator();
        ui.heading("手势");
        ui.checkbox(
            &mut self.gestures_enabled,
            "五指手势：下滑最小化 / 左右滑翻页",
        )
        .on_hover_text("五指同向快速滑动触发；黑板中部图标随进度变大，松手执行，手返回可撤销。");
        ui.add_enabled(
            self.gestures_enabled,
            egui::Slider::new(&mut self.gesture_threshold, 80.0..=300.0).text("触发距离（像素）"),
        );
    }
    pub(super) fn request_close(&mut self, ctx: &egui::Context) {
        self.cancel_local_capture();
        self.cancel_ink();
        if self.session.history.is_dirty(&self.session.document) {
            self.set_collapsed(ctx, false);
            self.confirm_close = true;
            self.tool = Tool::Pen;
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        } else {
            self.allow_close = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
    pub(super) fn sync_handwriting_context(&mut self) {
        let context = (self.math
            && self.mode == AppMode::Blackboard
            && self.session.effective_visible()
            && !self.compact_entry())
        .then(|| {
            (
                ContextToken::capture(&self.session.document),
                self.current_math_input(),
            )
        });
        if self.handwriting_context != context {
            self.handwriting.invalidate_context();
            self.handwriting_context = context;
        }
    }
}

pub(super) fn toolbar_scale(viewport: Vec2, compact: bool) -> f32 {
    let relative = (viewport.x / 1920.0).min(viewport.y / 1080.0);
    let fit = if compact {
        viewport.x / 120.0
    } else {
        viewport.x / 540.0
    };
    (relative.clamp(0.8, 1.5) * 1.1)
        .min(fit)
        .min(viewport.y / 120.0)
        .max(0.001)
}

pub(super) fn toolbar_style(ui: &mut egui::Ui, scale: f32) {
    let style = ui.style_mut();
    for font in style.text_styles.values_mut() {
        font.size = (font.size * scale).max(12.0);
    }
    style.spacing.item_spacing *= scale;
    style.spacing.button_padding *= scale;
    style.spacing.interact_size = Vec2::splat(42.0 * scale);
}

// #region debug-point B:benchmark
