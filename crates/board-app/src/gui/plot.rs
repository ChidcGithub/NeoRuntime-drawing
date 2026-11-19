// 函数图控件与求点覆盖层：图例、缩放平移、候选标记。从 gui.rs 拆出（纯移动）。
use super::*;

impl BoardApp {
    pub(super) fn selected_object(&self) -> Option<BoardObject> {
        self.session
            .document
            .current_page()
            .objects
            .iter()
            .find(|o| Some(&o.id) == self.selected.as_ref())
            .cloned()
    }
    pub(super) fn ink_action_busy(&self) -> bool {
        self.hwr_worker.busy() || self.math_worker.busy() || self.model_loader.busy()
    }

    #[cfg(test)]
    pub(super) fn ink_action(&self) -> Option<(Rect, &'static str, &'static str)> {
        if let Some(bounds) = self.calculating_ink
            && self.math_worker.busy()
            && self.session.effective_visible()
            && self
                .ink_context
                .as_ref()
                .is_some_and(|context| *context == ContextToken::capture(&self.session.document))
        {
            return Some((bounds, "=", "计算中"));
        }
        let request = self.calculation.as_ref()?;
        if self.ink_math_mode == InkMathMode::Off
            || request.context != ContextToken::capture(&self.session.document)
            || !self.session.effective_visible()
            || self.ink_ticket.is_some()
        {
            return None;
        }
        if self.recognition.is_none()
            || self
                .neural_raw
                .as_ref()
                .is_some_and(|raw| latex_to_expression(&raw.latex).is_err())
        {
            // 无法识别算式：不显示计算/纠正按钮。
            return None;
        }
        let (icon, hover) = if features::function_expression(&self.expression).is_some() {
            ("f", "生成函数图像")
        } else {
            ("=", "计算")
        };
        Some((features::ink_bounds(&request.strokes), icon, hover))
    }

    pub(super) fn ink_function(&self) -> Option<(Rect, String)> {
        let request = self.calculation.as_ref()?;
        if self.ink_math_mode == InkMathMode::Off
            || request.context != ContextToken::capture(&self.session.document)
            || !self.session.effective_visible()
        {
            return None;
        }
        Some((
            features::ink_bounds(&request.strokes),
            features::function_expression(&self.expression)?,
        ))
    }

    pub(super) fn plot_ink_function(&mut self) {
        if self.ink_action_busy() || self.gesture.is_some() {
            return;
        }
        self.sync_ink_cache();
        self.save_candidate_edit();
        let Some(id) = self.active_candidate.clone() else {
            return;
        };
        if !self.activate_candidate(&id) {
            return;
        }
        if let Some((bounds, expression)) = self.ink_function() {
            let object = features::plot(
                expression,
                Point {
                    x: bounds.left(),
                    y: bounds.bottom() + 20.0,
                },
            );
            let _ = self.add_candidate_result(&id, object.kind);
        }
    }

    pub(super) fn sync_plot_state(&mut self) {
        if self.plot_points.as_ref().is_some_and(|state| {
            state.context != ContextToken::capture(&self.session.document)
                || self.selected_object().as_ref() != Some(&state.object)
                || self
                    .preview()
                    .is_some_and(|preview| preview != state.object)
        }) {
            self.plot_worker.cancel();
            self.plot_points = None;
        }
    }

    pub(super) fn plot_controls(&mut self, ctx: &egui::Context) {
        self.sync_ink_cache();
        self.save_candidate_edit();
        let actions: Vec<_> = self
            .ink_cache
            .entries
            .iter()
            .filter(|entry| {
                entry.request.context.page_id == self.session.document.current_page().id
                    && !entry.result_present
                    && self.ink_math_mode != InkMathMode::Off
                    && self.session.effective_visible()
                    && !self.allow_close
                    && !self.confirm_close
            })
            .filter_map(|entry| {
                let (icon, hover) = if self
                    .running_candidate
                    .as_ref()
                    .is_some_and(|(id, _)| id == &entry.id)
                {
                    (Icon::Calculator, "计算中")
                } else if entry.recognition.is_none() && entry.expression.is_empty() {
                    // 无法识别算式：不显示计算/纠正按钮。
                    return None;
                } else if features::function_expression(&entry.expression).is_some() {
                    (Icon::Plot, "生成函数图像")
                } else {
                    (Icon::Calculator, "计算")
                };
                Some((entry.id.clone(), entry.bounds, icon, hover))
            })
            .collect();
        let mut occupied = self.controls.clone();
        for (id, bounds, icon, hover) in actions {
            let Some(rect) =
                features::ink_action_rect_avoiding(bounds, ctx.content_rect(), &occupied)
            else {
                continue;
            };
            occupied.push(rect);
            self.controls.push(rect);
            egui::Area::new(egui::Id::new(("ink_action", &id)))
                .order(egui::Order::Foreground)
                .fixed_pos(rect.min)
                .default_size(rect.size())
                .movable(false)
                .show(ctx, |ui| {
                    ui.set_min_size(rect.size());
                    ui.set_max_size(rect.size());
                    if icons::button(
                        ui,
                        icon,
                        hover,
                        rect.width(),
                        false,
                        !self.ink_action_busy() && self.gesture.is_none(),
                    )
                    .clicked()
                        && self.activate_candidate(&id)
                    {
                        match icon {
                            Icon::Plot => self.plot_ink_function(),
                            Icon::Calculator => self.confirm_calculation(ctx),
                            _ => self.math = true,
                        }
                    }
                });
        }
        self.sync_plot_state();
        let objects: Vec<_> = self
            .session
            .document
            .current_page()
            .objects
            .iter()
            .filter(|object| {
                matches!(
                    object.kind,
                    ObjectKind::Text { .. }
                        | ObjectKind::Image { .. }
                        | ObjectKind::FunctionPlot { .. }
                )
            })
            .cloned()
            .collect();
        for object in &objects {
            // The selected image's corners belong to resize; its center still opens Agent.
            if !(self.tool == Tool::Select && self.selected.as_ref() == Some(&object.id))
                && let Some(rect) = features::image_agent_rect(object, ctx.content_rect())
            {
                let area = egui::Area::new(egui::Id::new(("image_agent", &object.id)))
                    .fixed_pos(rect.min)
                    .movable(false)
                    .show(ctx, |ui| {
                        if ui
                            .add_sized(rect.size(), egui::Button::new("询问 Agent"))
                            .on_hover_text("仅 hosted 宿主服务：打开这张图片的授权，不自动发送；选择工具单击图片也可打开，拖动仍移动")
                            .clicked()
                        {
                            self.open_image_agent(object);
                            self.tool = Tool::Pen;
                        }
                    });
                self.controls.push(area.response.rect);
            }
            if let ObjectKind::Text { text, .. } = &object.kind
                && let Some(expression) = features::function_expression(text)
            {
                let bounds = board_render::object_bounds(object);
                let area = egui::Area::new(egui::Id::new(("function", &object.id)))
                    .fixed_pos(bounds.right_top() + Vec2::new(6.0, -24.0))
                    .movable(false)
                    .show(ctx, |ui| {
                        if ActionButton::secondary(egui::Id::new("plot.create"), "绘图")
                            .show(ui)
                            .clicked()
                        {
                            self.apply(vec![Operation::Add {
                                object: features::plot(
                                    expression,
                                    Point {
                                        x: bounds.left(),
                                        y: bounds.bottom() + 20.0,
                                    },
                                ),
                            }]);
                        }
                    });
                self.controls.push(area.response.rect);
            }
        }
        if let Some(object) = self.selected_object() {
            if let ObjectKind::FunctionPlot {
                expressions,
                position,
                width,
                ..
            } = &object.kind
            {
                let area = egui::Area::new("plot_legend".into())
                    .fixed_pos(Pos2::new(position.x, position.y - 30.0))
                    .movable(false)
                    .show(ctx, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            if ActionButton::secondary(egui::Id::new("plot.legend.reset"), "复原")
                                .show(ui)
                                .clicked()
                            {
                                let mut reset = object.clone();
                                if let ObjectKind::FunctionPlot {
                                    x_min,
                                    x_max,
                                    y_min,
                                    y_max,
                                    ..
                                } = &mut reset.kind
                                {
                                    *x_min = -10.0;
                                    *x_max = 10.0;
                                    *y_min = -7.0;
                                    *y_max = 7.0;
                                }
                                self.apply(vec![Operation::Update { object: reset }]);
                            }
                            for (index, expression) in expressions.iter().take(16).enumerate() {
                                let response = ui.add(
                                    egui::Label::new(features::plot_text(expression))
                                        .sense(Sense::drag()),
                                );
                                if expressions.len() > 1 && response.drag_started() {
                                    self.split_drag = Some((
                                        object.id.clone(),
                                        index,
                                        ContextToken::capture(&self.session.document),
                                    ));
                                }
                                response
                                    .on_hover_text("拖出图框分离函数；拖动整个图到另一个图合并");
                            }
                            if expressions.len() > 1
                                && ActionButton::secondary(
                                    egui::Id::new("plot.legend.split"),
                                    "分离最后函数",
                                )
                                .show(ui)
                                .clicked()
                                && let Some(ops) = features::split_plot(
                                    &object,
                                    expressions.len() - 1,
                                    Point {
                                        x: position.x + width + 20.0,
                                        y: position.y,
                                    },
                                )
                            {
                                self.apply(ops);
                            }
                        });
                    });
                self.controls.push(area.response.rect);
                self.sync_plot_state();
                if self.plot_points.is_none()
                    && !self.plot_worker.busy()
                    && self.preview().is_none_or(|preview| preview == object)
                    && self.selected_object().as_ref() == Some(&object)
                {
                    let snapshot = object.clone();
                    let context = ContextToken::capture(&self.session.document);
                    self.plot_points = Some(PlotState {
                        context: context.clone(),
                        object: object.clone(),
                        report: None,
                        coordinate: None,
                    });
                    self.plot_worker.start(ctx.clone(), move || {
                        let report = features::intersections(&snapshot);
                        (context, snapshot, report)
                    });
                }
                let bounds = board_render::object_bounds(&object);
                let area = egui::Area::new("plot_search_status".into())
                    .fixed_pos(bounds.left_bottom() + Vec2::new(0.0, 8.0))
                    .movable(false)
                    .show(ctx, |ui| {
                        ui.set_max_width(440.0);
                        match self
                            .plot_points
                            .as_ref()
                            .and_then(|state| state.report.as_ref())
                        {
                            None => {
                                ui.label("交点计算中（旧任务未排空时等待）…");
                            }
                            Some(Err(error)) => {
                                ui.label(format!("部分搜索失败；非完备：{error}"));
                            }
                            Some(Ok(report)) => {
                                ui.label(report.summary());
                                for diagnostic in &report.diagnostics {
                                    let color = match diagnostic.issue {
                                        features::IntersectionIssue::Failed
                                        | features::IntersectionIssue::Limit => Color32::LIGHT_RED,
                                        _ => Color32::LIGHT_YELLOW,
                                    };
                                    ui.colored_label(color, &diagnostic.message);
                                }
                                if report.omitted_diagnostics > 0 {
                                    ui.label(format!(
                                        "另有 {} 条诊断未展开",
                                        report.omitted_diagnostics
                                    ));
                                }
                            }
                        }
                        if let Some(label) = self
                            .plot_points
                            .as_ref()
                            .and_then(|state| state.coordinate.as_ref())
                        {
                            ui.label(label);
                        }
                    });
                self.controls.push(area.response.rect);
            } else if let ObjectKind::CoordinateSystem {
                origin, three_d, ..
            } = &object.kind
            {
                // 选中坐标系：一键切换三维/二维（与函数图复原按钮同构）。
                let center = Pos2::new(
                    origin.x * self.view_zoom + self.view_pan.x,
                    origin.y * self.view_zoom + self.view_pan.y - 30.0,
                );
                let area = egui::Area::new(egui::Id::new(format!("coordinate_mode:{}", object.id)))
                    .fixed_pos(center)
                    .movable(false)
                    .show(ctx, |ui| {
                        if ui
                            .button(if *three_d {
                                "切换为二维"
                            } else {
                                "切换为三维"
                            })
                            .clicked()
                        {
                            let mut toggled = object.clone();
                            if let ObjectKind::CoordinateSystem { three_d, .. } = &mut toggled.kind
                            {
                                *three_d = !*three_d;
                            }
                            self.apply(vec![Operation::Update { object: toggled }]);
                        }
                    });
                self.controls.push(area.response.rect);
            } else {
                self.plot_points = None;
            }
        } else {
            self.plot_points = None;
        }
        if ctx.input(|i| i.pointer.any_released())
            && let Some((id, index, context)) = self.split_drag.take()
            && context == ContextToken::capture(&self.session.document)
            && let (Some(source), Some(pos)) = (
                objects.iter().find(|o| o.id == id),
                ctx.input(|i| i.pointer.interact_pos()),
            )
            && !board_render::object_bounds(source).contains(pos)
            && !self.controls.iter().any(|r| r.contains(pos))
            && let Some(ops) = features::split_plot(source, index, Point { x: pos.x, y: pos.y })
        {
            self.apply(ops);
        }
    }
}
