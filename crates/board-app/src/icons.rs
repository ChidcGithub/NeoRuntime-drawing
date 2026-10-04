use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Icon {
    Pen,
    Eraser,
    Select,
    Line,
    Shapes,
    Previous,
    Next,
    Plus,
    Mouse,
    File,
    Settings,
    More,
    Undo,
    Redo,
    Collapse,
    Expand,
    Exit,
    Calculator,
    Plot,
    Correct,
}

pub(super) fn button(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    size: f32,
    selected: bool,
    enabled: bool,
) -> Response {
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
            response.widget_info(|| {
                WidgetInfo::selected(WidgetType::Button, ui.is_enabled(), selected, label)
            });
            if ui.is_rect_visible(rect) {
                let visuals = ui.style().interact_selectable(&response, selected);
                ui.painter().rect(
                    rect.shrink(size / 42.0),
                    5.0 * size / 42.0,
                    visuals.bg_fill,
                    visuals.bg_stroke,
                    egui::StrokeKind::Inside,
                );
                let color = if ui.is_enabled() {
                    visuals.fg_stroke.color
                } else {
                    ui.visuals().weak_text_color().gamma_multiply(0.5)
                };
                paint(
                    ui.painter(),
                    Rect::from_center_size(rect.center(), Vec2::splat(size * 26.0 / 42.0)),
                    icon,
                    color,
                );
            }
            response
        })
        .inner;
    #[cfg(test)]
    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::new(("icon_rect", label)), response.rect));
    response.on_hover_text(label).on_disabled_hover_text(label)
}

fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let p = |x: f32, y: f32| {
        Pos2::new(
            rect.left() + x * rect.width() / 24.0,
            rect.top() + y * rect.height() / 24.0,
        )
    };
    let stroke = Stroke::new(1.8 * rect.width() / 26.0, color);
    let line = |points: &[(f32, f32)]| {
        painter.add(egui::Shape::line(
            points.iter().map(|&(x, y)| p(x, y)).collect(),
            stroke,
        ));
    };
    let box_at = |x, y, w, h| {
        painter.rect_stroke(
            Rect::from_min_max(p(x, y), p(x + w, y + h)),
            1.5,
            stroke,
            egui::StrokeKind::Inside,
        );
    };
    match icon {
        Icon::Pen => {
            line(&[
                (3., 21.),
                (5., 14.),
                (17., 2.),
                (22., 7.),
                (10., 19.),
                (3., 21.),
            ]);
            line(&[(14., 5.), (19., 10.)]);
        }
        Icon::Eraser => {
            line(&[
                (2., 14.),
                (13., 3.),
                (22., 12.),
                (13., 21.),
                (8., 21.),
                (2., 14.),
            ]);
            line(&[(7., 9.), (17., 18.)]);
            line(&[(8., 21.), (23., 21.)]);
        }
        Icon::Select => {
            line(&[(4., 2.), (19., 13.), (12., 14.), (9., 21.), (4., 2.)]);
        }
        Icon::Line => {
            line(&[(3., 21.), (21., 3.)]);
            painter.circle_filled(p(3., 21.), rect.width() / 13.0, color);
            painter.circle_filled(p(21., 3.), rect.width() / 13.0, color);
        }
        Icon::Shapes => {
            box_at(2., 2., 12., 12.);
            painter.circle_stroke(p(16., 16.), rect.width() / 3.4, stroke);
        }
        Icon::Previous | Icon::Next => {
            let x = |v| if icon == Icon::Next { 24. - v } else { v };
            line(&[(x(14.), 4.), (x(6.), 12.), (x(14.), 20.)]);
            line(&[(x(6.), 12.), (x(22.), 12.)]);
        }
        Icon::Plus => {
            line(&[(3., 12.), (21., 12.)]);
            line(&[(12., 3.), (12., 21.)]);
        }
        Icon::Mouse => {
            box_at(5., 1., 14., 22.);
            line(&[(12., 1.), (12., 10.)]);
            line(&[(5., 10.), (19., 10.)]);
        }
        Icon::File => {
            line(&[
                (2., 20.),
                (2., 4.),
                (10., 4.),
                (13., 7.),
                (22., 7.),
                (22., 20.),
                (2., 20.),
            ]);
        }
        Icon::Settings => {
            for (y, x) in [(5., 8.), (12., 16.), (19., 10.)] {
                line(&[(2., y), (22., y)]);
                painter.circle_filled(p(x, y), 3.0 * rect.width() / 26.0, color);
            }
        }
        Icon::More => {
            for x in [4., 12., 20.] {
                painter.circle_filled(p(x, 12.), rect.width() / 13.0, color);
            }
        }
        Icon::Undo | Icon::Redo => {
            let flip = |x| if icon == Icon::Redo { 24. - x } else { x };
            line(&[(flip(9.), 3.), (flip(3.), 9.), (flip(9.), 15.)]);
            line(&[
                (flip(3.), 9.),
                (flip(15.), 9.),
                (flip(20.), 13.),
                (flip(20.), 19.),
                (flip(16.), 22.),
            ]);
        }
        Icon::Collapse => {
            line(&[(3., 8.), (12., 17.), (21., 8.)]);
        }
        Icon::Expand => {
            line(&[(3., 17.), (12., 8.), (21., 17.)]);
        }
        Icon::Exit => {
            line(&[(10., 3.), (3., 3.), (3., 21.), (10., 21.)]);
            line(&[(8., 12.), (22., 12.), (16., 6.)]);
            line(&[(22., 12.), (16., 18.)]);
        }
        Icon::Calculator => {
            box_at(3., 1., 18., 22.);
            box_at(6., 4., 12., 5.);
            for x in [7., 12., 17.] {
                for y in [13., 18.] {
                    painter.circle_filled(p(x, y), rect.width() / 20.0, color);
                }
            }
        }
        Icon::Plot => {
            line(&[(3., 2.), (3., 21.), (23., 21.)]);
            line(&[
                (5., 17.),
                (8., 10.),
                (11., 8.),
                (14., 12.),
                (17., 14.),
                (20., 5.),
                (22., 3.),
            ]);
        }
        Icon::Correct => {
            box_at(2., 2., 13., 19.);
            line(&[(5., 6.), (11., 6.)]);
            line(&[(5., 10.), (9., 10.)]);
            line(&[
                (10., 21.),
                (12., 15.),
                (20., 7.),
                (23., 10.),
                (15., 18.),
                (10., 21.),
            ]);
        }
    }
}
