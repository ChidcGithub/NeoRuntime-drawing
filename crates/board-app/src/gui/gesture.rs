// 手势提交：合并目标判定、收笔落板与延迟提交。从 gui.rs 拆出（纯移动）。
use super::*;

impl BoardApp {
    pub(super) fn plot_merge_target(&self, moved: &BoardObject) -> Option<&BoardObject> {
        // 非函数选择手势在扫描页面之前返回。
        if self.tool != Tool::Select || !matches!(moved.kind, ObjectKind::FunctionPlot { .. }) {
            return None;
        }
        let gesture = self.gesture.as_ref()?;
        let original = gesture.original.as_ref()?;
        if gesture.vertex.is_some()
            || gesture.resize.is_some()
            || !matches!(original.kind, ObjectKind::FunctionPlot { .. })
            || gesture.document != self.session.document.id
            || gesture.page != self.session.document.current_page().id
            || gesture.revision != self.session.document.revision
        {
            return None;
        }
        // 渲染 bounds 含描边外扩；合并只计算 position/width/height 定义的图框。
        let frame = |object: &BoardObject| {
            if let ObjectKind::FunctionPlot {
                position,
                width,
                height,
                ..
            } = &object.kind
            {
                Rect::from_min_size(
                    Pos2::new(position.x, position.y),
                    Vec2::new(*width, *height),
                )
            } else {
                unreachable!("only function plots reach overlap testing")
            }
        };
        let before = frame(original);
        let after = frame(moved);
        let overlap = |a: Rect, b: Rect| {
            let intersection = a.intersect(b);
            f64::from(intersection.width().max(0.0)) * f64::from(intersection.height().max(0.0))
        };
        self.session
            .document
            .current_page()
            .objects
            .iter()
            .rev()
            .find(|other| {
                if other.id == moved.id || !matches!(other.kind, ObjectKind::FunctionPlot { .. }) {
                    return false;
                }
                let target = frame(other);
                let area = overlap(after, target);
                // 以按下时为基准；边接触、面积不变及拖离均不合并。
                area > 0.0 && area > overlap(before, target)
            })
    }

    pub(super) fn finish_gesture(&mut self) {
        // 指针路径提交时保护仍在书写的附加指手势：先取走、提交后放回
        // （revision 同步为提交后的文档状态）。
        let others: Vec<(egui::TouchId, Gesture)> = self.multi_gestures.drain(..).collect();
        self.finish_gesture_at(Instant::now());
        let new_revision = self.session.document.revision;
        for (touch_id, mut gesture) in others {
            gesture.revision = new_revision;
            self.multi_gestures.push((touch_id, gesture));
        }
    }

    pub(super) fn finish_gesture_at(&mut self, now: Instant) {
        // #region debug-point B:commit
        #[cfg(test)]
        let _debug_stage = debug_ink::Stage::begin(8, "finish_gesture");
        // #endregion
        // #region debug-point D:commit
        let _debug_commit = debug_ink::Commit::begin(self);
        // #endregion
        self.sync_erase_preview();
        let vertex_edit = self.polygon_vertex_edit();
        let preview = self.preview();
        let merge = preview.as_ref().and_then(|source| {
            self.plot_merge_target(source)
                .map(|target| (target.id.clone(), features::merge_plots(source, target)))
        });
        let Some(gesture) = self.gesture.take() else {
            return;
        };
        if gesture.document != self.session.document.id
            || gesture.page != self.session.document.current_page().id
            || gesture.revision != self.session.document.revision
        {
            self.status = "绘制期间文档已更新，本次操作已取消".into();
            return;
        }
        if let Some(result) = vertex_edit {
            match result {
                Ok(edit) if gesture.original.as_ref() != Some(&edit.object) => {
                    let s = &mut self.session;
                    match s.history.edit(&mut s.document, |d| edit.apply(d)) {
                        Ok(()) => {
                            self.changed();
                            if !edit.hint.is_empty() {
                                self.status = edit.hint.replace("松手后", "已连接，");
                            }
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
                Ok(_) => {}
                Err(error) => self.status = format!("无法连接，本次拖动取消：{error}"),
            }
            return;
        }
        if self.tool == Tool::Eraser {
            if let Some(result) = gesture.erasing.and_then(|erasing| erasing.result) {
                match result {
                    Ok((_, operations)) => self.apply(operations),
                    Err(error) => self.status = error,
                }
            }
        } else if let Some(object) = preview {
            if self.tool == Tool::Select {
                if gesture.original.as_ref() != Some(&object) {
                    if let Some((target, operations)) = merge {
                        if let Some(operations) = operations {
                            self.selected = Some(target);
                            self.apply(operations);
                        } else {
                            let revision = self.session.document.revision;
                            self.apply(vec![Operation::Update { object }]);
                            if self.session.document.revision != revision {
                                self.status = PLOT_MERGE_LIMIT_HINT.into();
                            }
                        }
                    } else if matches!(
                        object.kind,
                        ObjectKind::Shape {
                            shape: ShapeKind::Line,
                            ..
                        }
                    ) && let Some(endpoint) = gesture.vertex
                    {
                        self.commit_line(object, false, &[endpoint]);
                    } else {
                        self.apply(vec![Operation::Update { object }]);
                    }
                }
            } else {
                self.commit_pen_object(object, now);
            }
        }
        // 多指并发：本次提交推进的 revision 同步给仍在书写的附加指手势，
        // 避免它们被当作"文档已更新"而取消。
        let new_revision = self.session.document.revision;
        for (_, gesture) in &mut self.multi_gestures {
            gesture.revision = new_revision;
        }
    }

    /// 笔类预览对象的统一提交：平滑、入库、个人笔迹学习与墨迹候选流。
    /// 主指针路径与触控附加指路径共用，保证两条路径行为一致。
    pub(super) fn commit_pen_object(&mut self, mut object: BoardObject, now: Instant) {
        if let ObjectKind::Stroke { points, .. } = &mut object.kind {
            *points = board_ink::smooth_resample(points, 2.0, 0.45, 38.0)
                .unwrap_or_else(|_| points.clone());
        }
        let stroke = if let ObjectKind::Stroke { points, .. } = &object.kind {
            Some(points.clone())
        } else {
            None
        };
        let mut ink = self.ink.clone();
        let mut sources = self.ink_sources.clone();
        let mut excluded = self.ink_excluded;
        let previous_revision = self.session.document.revision;
        object.id = new_id();
        let source = stroke.as_ref().map(|_| object.clone());
        if matches!(
            object.kind,
            ObjectKind::Shape {
                shape: ShapeKind::Line,
                ..
            }
        ) {
            self.commit_line(object, true, &[0, 1]);
        } else {
            self.apply(vec![Operation::Add { object }]);
        }
        // Only successful fresh local pen commits teach style. Imports, replay,
        // host operations, generated answers and moving old ink never enter here.
        if self.mode == AppMode::Blackboard
            && self.tool == Tool::Pen
            && self.handwriting.enabled
            && self.session.document.revision != previous_revision
            && let Some(BoardObject {
                kind: ObjectKind::Stroke { points, style },
                ..
            }) = &source
        {
            self.handwriting.profile.observe_stroke(points, *style);
        }
        if self.ink_math_mode != InkMathMode::Off
            && self.session.document.revision != previous_revision
            && let Some(stroke) = stroke
        {
            if !features::joins_expression(&ink, &stroke) {
                excluded.0 += ink.len();
                excluded.1 += ink.iter().map(Vec::len).sum::<usize>();
                ink.clear();
                sources.clear();
            }
            ink.push(stroke);
            sources.push(source.unwrap());
            let context = ContextToken::capture(&self.session.document);
            match self.auto_gate.strokes_finished(now, context.clone(), &ink) {
                Ok(()) => {
                    self.ink = ink;
                    self.ink_sources = sources;
                    self.ink_excluded = excluded;
                    self.ink_context = Some(context);
                }
                Err(e) => self.status = e.to_string(),
            }
        }
    }
}
