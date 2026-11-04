//! One rule for every row of Properties and the settings windows: the
//! label on the left, the control on the right, every control lined up in
//! one column. On and off settings are switches; choices of four or more
//! are drop-downs; section headers are full-width rows with a chevron at
//! the right.

use eframe::egui::{self, Color32, Response, Sense, Ui, Vec2};

/// The label column's width for a row `width` wide: wide enough for most
/// labels, never more than 40% of the row.
pub fn label_width(width: f32) -> f32 {
    (width * 0.4).clamp(64.0, 128.0)
}

/// A row: `label` at the left, in the label column, then `add`'s controls
/// filling the rest. A label too long for its column is cut with … and
/// shows whole when hovered.
pub fn row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let lw = label_width(ui.available_width());
    ui.horizontal(|ui| {
        let h = ui.spacing().interact_size.y;
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(lw, h), Sense::hover());
        let muted = ui.visuals().widgets.noninteractive.fg_stroke.color;
        let galley = ui.painter().layout(
            label.to_string(),
            egui::TextStyle::Body.resolve(ui.style()),
            muted,
            f32::INFINITY,
        );
        if galley.size().x > lw - 6.0 {
            // cut to fit, with the whole label on hover
            let mut job = egui::text::LayoutJob::single_section(
                label.to_string(),
                egui::TextFormat::simple(egui::TextStyle::Body.resolve(ui.style()), muted),
            );
            job.wrap = egui::text::TextWrapping::truncate_at_width(lw - 6.0);
            let g = ui.painter().layout_job(job);
            ui.painter().galley(
                rect.left_center() - Vec2::new(0.0, g.size().y / 2.0),
                g,
                muted,
            );
            resp.on_hover_text(label);
        } else {
            ui.painter().galley(
                rect.left_center() - Vec2::new(0.0, galley.size().y / 2.0),
                galley,
                muted,
            );
        }
        add(ui)
    })
    .inner
}

/// Add a control at the full width left in the row, as fields and boxes
/// line up in one column.
pub fn fill(ui: &mut Ui, w: impl egui::Widget) -> Response {
    let h = ui.spacing().interact_size.y;
    ui.add_sized([ui.available_width().max(40.0), h], w)
}

/// A drop-down list entry with a small picture before its label, painted
/// by `paint` in a square of the text's height.
pub fn picture_option(
    ui: &mut Ui,
    selected: bool,
    label: &str,
    paint: impl FnOnce(&egui::Painter, egui::Rect, egui::Stroke),
) -> Response {
    let h = ui.spacing().interact_size.y + 4.0;
    let w = ui.available_width().max(140.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    if ui.is_rect_visible(rect) {
        let v = ui.visuals();
        if selected || resp.hovered() {
            let fill = if selected {
                v.selection.bg_fill
            } else {
                v.widgets.hovered.weak_bg_fill
            };
            ui.painter().rect_filled(rect, 5.0, fill);
        }
        let ink = v.text_color();
        let pic = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 6.0, rect.top() + 3.0),
            Vec2::splat(h - 6.0),
        );
        paint(ui.painter(), pic, egui::Stroke::new(1.5, ink));
        ui.painter().text(
            egui::pos2(pic.right() + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::TextStyle::Body.resolve(ui.style()),
            ink,
        );
    }
    resp
}

/// A plain heading inside a window, for grouping rows.
pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(title).strong());
    ui.add_space(2.0);
}

/// An on/off switch in the theme's accent colour, the label at the left
/// and the switch at the right. It answers like a checkbox: `.changed()`
/// when clicked.
pub fn switch(ui: &mut Ui, on: &mut bool, label: impl Into<egui::WidgetText>) -> Response {
    let label: egui::WidgetText = label.into();
    let text = label.text().to_string();
    let w = ui.available_width();
    let h = ui.spacing().interact_size.y;
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let enabled = ui.is_enabled();
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, *on, &text)
    });
    if ui.is_rect_visible(rect) {
        let v = ui.visuals();
        let ink = if enabled {
            v.text_color()
        } else {
            v.weak_text_color()
        };
        // the label, cut to leave the switch its room
        let sw = Vec2::new(30.0, 16.0);
        let mut job = egui::text::LayoutJob::single_section(
            text.clone(),
            egui::TextFormat::simple(egui::TextStyle::Body.resolve(ui.style()), ink),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width((w - sw.x - 12.0).max(10.0));
        let g = ui.painter().layout_job(job);
        ui.painter().galley(
            rect.left_center() - Vec2::new(0.0, g.size().y / 2.0),
            g,
            ink,
        );
        let r = egui::Rect::from_center_size(
            egui::pos2(rect.right() - sw.x / 2.0 - 2.0, rect.center().y),
            sw,
        );
        paint_switch(ui, r, *on, resp.hovered() && enabled, enabled);
    }
    resp
}

/// Paint a switch in `r`: a rounded track, accent when on, with its knob.
pub fn paint_switch(ui: &Ui, r: egui::Rect, on: bool, hovered: bool, enabled: bool) {
    let v = ui.visuals();
    let t = ui
        .ctx()
        .animate_bool_responsive(ui.id().with(r.min.x as i32).with(r.min.y as i32), on);
    let accent = v.selection.stroke.color;
    let off = v.widgets.inactive.bg_fill;
    let mut track = lerp(off, accent, t);
    if !enabled {
        track = track.gamma_multiply(0.5);
    } else if hovered {
        track = lerp(track, v.text_color(), 0.12);
    }
    let rad = r.height() / 2.0;
    ui.painter().rect(
        r,
        rad,
        track,
        egui::Stroke::new(1.0, lerp(v.widgets.inactive.bg_stroke.color, accent, t)),
        egui::StrokeKind::Inside,
    );
    let x = egui::lerp((r.left() + rad)..=(r.right() - rad), t);
    ui.painter()
        .circle_filled(egui::pos2(x, r.center().y), rad - 2.5, Color32::WHITE);
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        m(a.r(), b.r()),
        m(a.g(), b.g()),
        m(a.b(), b.b()),
        m(a.a(), b.a()),
    )
}

/// A section header of Properties: a full-width row with the title in
/// semibold, a chevron at the right that turns as it opens, and a tint
/// under the pointer. `indent` draws a sub-section's header, set in.
/// Returns the row's response and the rect left for a ? button beside the
/// title.
pub fn header(ui: &mut Ui, title: &str, open: bool, indent: bool) -> (Response, egui::Rect) {
    let w = ui.available_width();
    let h = 28.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    if ui.is_rect_visible(rect) {
        let v = ui.visuals();
        let fill = if resp.hovered() {
            v.widgets.hovered.weak_bg_fill
        } else if indent {
            Color32::TRANSPARENT
        } else {
            v.widgets.inactive.weak_bg_fill.gamma_multiply(0.55)
        };
        ui.painter().rect_filled(rect, 6.0, fill);
        if !indent {
            // a top-level section reads as a row of its own
            ui.painter().rect_stroke(
                rect,
                6.0,
                v.widgets.noninteractive.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        let left = rect.left() + if indent { 18.0 } else { 8.0 };
        let font = egui::FontId::proportional(if indent { 13.0 } else { 13.5 });
        let g = ui.painter().layout(
            title.to_string(),
            font,
            v.strong_text_color(),
            (w - 60.0).max(10.0),
        );
        let tw = g.size().x;
        ui.painter().galley(
            egui::pos2(left, rect.center().y - g.size().y / 2.0),
            g,
            v.strong_text_color(),
        );
        // the chevron: right when shut, down when open, turning between
        let t = ui.ctx().animate_bool_responsive(resp.id, open);
        let c = egui::pos2(rect.right() - 14.0, rect.center().y);
        let ang = t * std::f32::consts::FRAC_PI_2;
        let rot = |p: Vec2| {
            let (s, co) = ang.sin_cos();
            c + Vec2::new(p.x * co - p.y * s, p.x * s + p.y * co)
        };
        let pts = [
            rot(Vec2::new(-2.5, -5.0)),
            rot(Vec2::new(2.5, 0.0)),
            rot(Vec2::new(-2.5, 5.0)),
        ];
        let stroke = egui::Stroke::new(1.6, v.weak_text_color());
        ui.painter().line_segment([pts[0], pts[1]], stroke);
        ui.painter().line_segment([pts[1], pts[2]], stroke);
        let help = egui::Rect::from_min_size(
            egui::pos2(left + tw + 6.0, rect.center().y - 8.0),
            Vec2::splat(16.0),
        );
        return (resp, help);
    }
    (resp, egui::Rect::NOTHING)
}

/// A labelled drop-down that draws as a Properties row: the label at the
/// left, the box filling the rest. It is made like an egui `ComboBox`.
pub struct Combo {
    label: String,
    inner: egui::ComboBox,
    width: Option<f32>,
}

impl Combo {
    pub fn new(label: &str) -> Self {
        Self {
            label: label.to_string(),
            inner: egui::ComboBox::from_id_salt(("props.combo", label.to_string())),
            width: None,
        }
    }

    pub fn selected_text(mut self, t: impl Into<egui::WidgetText>) -> Self {
        self.inner = self.inner.selected_text(t);
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    pub fn height(mut self, h: f32) -> Self {
        self.inner = self.inner.height(h);
        self
    }

    pub fn show_ui<R>(
        self,
        ui: &mut Ui,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> egui::InnerResponse<Option<R>> {
        let Combo {
            label,
            inner,
            width,
        } = self;
        row(ui, &label, |ui| {
            let w = width.unwrap_or(f32::INFINITY).min(ui.available_width());
            inner.width(w).show_ui(ui, add)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_label_column_is_at_most_two_fifths_of_the_row() {
        assert_eq!(label_width(300.0), 120.0);
        assert_eq!(label_width(1000.0), 128.0);
        assert_eq!(label_width(100.0), 64.0);
    }
}
