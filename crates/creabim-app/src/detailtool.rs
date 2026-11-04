//! Detailing in the 2D panes: the Callout tool, which draws a box on a
//! plan or section and makes a larger-scale detail view of it, and the
//! Detail tool, which drafts detail components (flashings, membranes,
//! insulation, blocking, brick courses, hatched regions, break lines) in
//! the view. Also the Properties fields of a detail component and of a
//! junction rule.

use crate::planview::{PlanState, Xf};
use crate::selection::Selection;
use creabim_core::{
    DetailItem, DetailKind, Document, Element, ElementId, ElementKind, Hatch, JoinRule, JoinWinner,
    Model, ModelError, Transaction, View,
};
use eframe::egui::{self, Color32, Stroke};
use glam::DVec2;

/// What the Detail and Callout tools draw next, and the Detail tool's
/// points so far.
#[derive(Clone, Debug)]
pub struct DetailSettings {
    pub kind: DetailKind,
    /// Width of the next component; set from the kind when it is picked.
    pub width: f64,
    /// Points clicked so far, and the view they are in.
    pub path: Vec<DVec2>,
    pub view: Option<ElementId>,
    /// Scale of the next detail view, 1:`callout_scale`.
    pub callout_scale: f64,
}

impl Default for DetailSettings {
    fn default() -> Self {
        Self {
            kind: DetailKind::Batt,
            width: DetailKind::Batt.default_width(),
            path: vec![],
            view: None,
            callout_scale: 10.0,
        }
    }
}

/// The detail scales the Callout tool offers.
pub const CALLOUT_SCALES: [f64; 4] = [2.0, 5.0, 10.0, 20.0];

impl DetailSettings {
    /// Pick what the Detail tool draws, at its usual width.
    pub fn pick(&mut self, kind: DetailKind) {
        self.kind = kind;
        self.width = kind.default_width();
    }
}

fn view_of(model: &Model, id: ElementId) -> Option<&View> {
    match &model.get(id)?.kind {
        ElementKind::View(v) => Some(v),
        _ => None,
    }
}

/// The next free "Detail N" name.
fn next_detail_name(model: &Model) -> String {
    (1..)
        .map(|n| format!("Detail {n}"))
        .find(|n| !model.iter().any(|e| &e.name == n))
        .expect("unbounded")
}

/// A click with the Callout tool in `view`: the box's first corner, then
/// its second, which makes the detail view. Returns the new view's id.
pub fn callout_click(
    doc: &mut Document,
    view: ElementId,
    p: DVec2,
    state: &mut PlanState,
    selection: &mut Selection,
) -> Result<Option<ElementId>, ModelError> {
    let Some(parent) = view_of(doc.model(), view).cloned() else {
        return Ok(None);
    };
    if !matches!(
        parent.kind,
        creabim_core::ViewKind::Plan { .. } | creabim_core::ViewKind::Section { .. }
    ) {
        state.status = Some("Callouts are drawn on plans and sections.".into());
        return Ok(None);
    }
    let Some(a) = state.pending.take() else {
        state.pending = Some(p);
        state.status = Some("Click the opposite corner of the callout's box.".into());
        return Ok(None);
    };
    let size = (p - a).abs();
    if size.x < 0.05 || size.y < 0.05 {
        state.pending = Some(a);
        return Ok(None);
    }
    let name = next_detail_name(doc.model());
    let detail = View::callout(view, &parent, a, p, state.details.callout_scale);
    let e = Element::new(name.clone(), ElementKind::View(detail));
    let id = e.id;
    doc.commit(Transaction::new(format!("Call out {name}")).insert(e))?;
    selection.only(id);
    state.status = Some(format!(
        "{name} at 1:{:.0}: it shows what is inside the box, and follows the model. Draw detail components in it with the Detail tool; place it on a sheet like any view.",
        state.details.callout_scale
    ));
    Ok(Some(id))
}

/// A click with the Detail tool in `view`: each click adds a point; a
/// click on the last point again (or Enter) finishes the path, and for a
/// filled region a click on the first point closes it. A break line ends
/// at its second point.
pub fn detail_click(
    doc: &mut Document,
    view: ElementId,
    p: DVec2,
    state: &mut PlanState,
    selection: &mut Selection,
) -> Result<(), ModelError> {
    let d = &mut state.details;
    if d.view != Some(view) {
        d.path.clear();
        d.view = Some(view);
    }
    let near = |a: DVec2| a.distance(p) < 0.004_f64.max(view_scale(doc.model(), view) * 0.0015);
    let done = match (d.path.first(), d.path.last()) {
        (Some(first), _) if d.kind.is_area() && d.path.len() >= 3 && near(*first) => true,
        (_, Some(last)) if near(*last) => true,
        _ => {
            d.path.push(p);
            matches!(d.kind, DetailKind::BreakLine) && d.path.len() == 2
        }
    };
    if done {
        return finish(doc, state, selection);
    }
    state.status = Some(match (d.kind.is_area(), d.path.len()) {
        (true, n) if n >= 3 => "Click the next corner, or the first to close the region.".into(),
        (true, _) => "Click the region's next corner.".into(),
        (false, 1) => "Click the next point.".into(),
        _ => "Click the next point, or the last one again (Enter) to finish.".into(),
    });
    Ok(())
}

fn view_scale(model: &Model, view: ElementId) -> f64 {
    view_of(model, view).map_or(100.0, |v| v.scale)
}

/// Make the component from the points clicked so far.
pub fn finish(
    doc: &mut Document,
    state: &mut PlanState,
    selection: &mut Selection,
) -> Result<(), ModelError> {
    let d = &mut state.details;
    let (Some(view), path) = (d.view, std::mem::take(&mut d.path)) else {
        return Ok(());
    };
    let need = if d.kind.is_area() { 3 } else { 2 };
    if path.len() < need {
        state.status = Some(format!(
            "A {} needs {need} points or more.",
            d.kind.label().to_lowercase()
        ));
        return Ok(());
    }
    let mut item = DetailItem::new(view, d.kind, path);
    item.width = d.width;
    let label = d.kind.label();
    let e = Element::new(label, ElementKind::Detail(item));
    selection.only(e.id);
    state.status = Some(format!(
        "{label} drawn. Its width and kind are in Properties; it shows in this view only."
    ));
    doc.commit(Transaction::new(format!("Draw {}", label.to_lowercase())).insert(e))
}

/// The Callout box or the Detail path being drawn, under the pointer.
pub fn preview(
    painter: &egui::Painter,
    xf: &Xf,
    tool: crate::planview::Tool,
    state: &PlanState,
    view: ElementId,
    hover: Option<DVec2>,
    mm: f64,
) {
    let accent = Stroke::new(1.5, Color32::from_rgb(230, 120, 30));
    match tool {
        crate::planview::Tool::Callout => {
            if let (Some(a), Some(b)) = (state.pending, hover) {
                let r = egui::Rect::from_two_pos(xf.to_screen(a), xf.to_screen(b));
                painter.rect_stroke(r, 0.0, accent, egui::StrokeKind::Middle);
            }
        }
        crate::planview::Tool::Detail => {
            let d = &state.details;
            if d.view != Some(view) || d.path.is_empty() {
                return;
            }
            let mut pts = d.path.clone();
            pts.extend(hover);
            let mut item = DetailItem::new(view, d.kind, pts);
            item.width = d.width;
            for it in creabim_geom::detail::detail_items(ElementId::new(), &item) {
                crate::planview::draw_item(painter, xf, &it, true, mm);
            }
        }
        _ => {}
    }
}

/// A component's settings in Properties; returns the edit's name when one
/// changed.
pub fn detail_fields(ui: &mut egui::Ui, d: &mut DetailItem) -> &'static str {
    let mut label = "";
    egui::ComboBox::from_id_salt("detail.kind")
        .selected_text(d.kind.label())
        .show_ui(ui, |ui| {
            for k in DetailKind::ALL {
                if ui.selectable_label(d.kind.same(k), k.label()).clicked() && !d.kind.same(k) {
                    d.kind = k;
                    if d.width <= 0.0 {
                        d.width = k.default_width();
                    }
                    label = "Detail kind";
                }
            }
        });
    if !matches!(
        d.kind,
        DetailKind::Membrane | DetailKind::Region { .. } | DetailKind::BreakLine
    ) {
        ui.horizontal(|ui| {
            ui.label("Width");
            if ui
                .add(crate::units::drag(&mut d.width).range(0.0..=5.0))
                .changed()
            {
                label = "Detail width";
            }
        });
    }
    match &mut d.kind {
        DetailKind::Repeat { module, joint } => {
            ui.horizontal(|ui| {
                ui.label("Unit");
                if ui
                    .add(crate::units::drag(module).range(0.005..=5.0))
                    .changed()
                {
                    label = "Detail unit";
                }
                ui.label("Joint");
                if ui.add(crate::units::drag(joint).range(0.0..=0.5)).changed() {
                    label = "Detail joint";
                }
            });
        }
        DetailKind::Region { hatch } => {
            egui::ComboBox::from_id_salt("detail.hatch")
                .selected_text(hatch.label())
                .show_ui(ui, |ui| {
                    for h in Hatch::ALL {
                        if ui.selectable_value(hatch, h, h.label()).changed() {
                            label = "Detail hatch";
                        }
                    }
                });
        }
        _ => {}
    }
    ui.label(
        egui::RichText::new(format!(
            "{} points, {} long. Detail components are drafting: they show in this view only, never in 3D, schedules or IFC.",
            d.points.len(),
            crate::units::len(d.length())
        ))
        .weak(),
    );
    label
}

fn name_of(model: &Model, id: ElementId) -> String {
    model
        .get(id)
        .map_or("(missing)".to_string(), |e| e.name.clone())
}

/// What a junction rule's winner is called, for `a` and `b`.
pub fn winner_label(model: &Model, rule: &JoinRule, w: JoinWinner) -> String {
    match w {
        JoinWinner::Auto => "Automatic (by layer priority)".into(),
        JoinWinner::First => format!("{} cuts through", name_of(model, rule.a)),
        JoinWinner::Second => format!("{} cuts through", name_of(model, rule.b)),
    }
}

/// A junction rule in Properties.
pub fn join_fields(ui: &mut egui::Ui, model: &Model, j: &mut JoinRule) -> &'static str {
    let mut label = "";
    ui.label(format!(
        "How {} and {} join where they meet in plans and sections.",
        name_of(model, j.a),
        name_of(model, j.b)
    ));
    if crate::props::switch(ui, &mut j.cleanup, "Clean up the junction")
        .on_hover_text("Off: both are drawn whole where they overlap, with their own outlines.")
        .changed()
    {
        label = "Junction cleanup";
    }
    ui.add_enabled_ui(j.cleanup, |ui| {
        let rule = *j;
        for w in JoinWinner::ALL {
            if ui
                .radio_value(&mut j.winner, w, winner_label(model, &rule, w))
                .changed()
            {
                label = "Junction winner";
            }
        }
    });
    label
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planview::Tool;

    fn doc_with_section() -> (Document, ElementId) {
        let mut doc = Document::new();
        let v = Element::new(
            "Section A",
            ElementKind::View(View::section(DVec2::ZERO, DVec2::new(5.0, 0.0), 3.0, true)),
        );
        let id = v.id;
        doc.commit(Transaction::new("setup").insert(v)).unwrap();
        (doc, id)
    }

    #[test]
    fn a_callout_makes_a_detail_view_of_its_box() {
        let (mut doc, section) = doc_with_section();
        let mut state = PlanState::default();
        state.tool = Tool::Callout;
        let mut sel = Selection::default();
        let p = DVec2::new;
        assert_eq!(
            callout_click(&mut doc, section, p(2.0, 1.0), &mut state, &mut sel).unwrap(),
            None
        );
        let id = callout_click(&mut doc, section, p(1.0, 0.0), &mut state, &mut sel)
            .unwrap()
            .unwrap();
        let e = doc.model().get(id).unwrap();
        assert_eq!(e.name, "Detail 1");
        let ElementKind::View(v) = &e.kind else {
            panic!()
        };
        let c = v.callout.unwrap();
        assert_eq!((c.parent, c.lo, c.hi), (section, p(1.0, 0.0), p(2.0, 1.0)));
        assert_eq!(v.scale, 10.0);
        // undo takes it away again
        assert!(doc.undo());
        assert!(doc.model().get(id).is_none());
    }

    #[test]
    fn the_detail_tool_draws_a_path_and_finishes_on_the_last_point() {
        let (mut doc, section) = doc_with_section();
        let mut state = PlanState::default();
        state.details.pick(DetailKind::Membrane);
        let mut sel = Selection::default();
        let p = DVec2::new;
        for q in [p(0.0, 0.0), p(1.0, 0.0), p(1.0, 0.5), p(1.0, 0.5)] {
            detail_click(&mut doc, section, q, &mut state, &mut sel).unwrap();
        }
        let items: Vec<&DetailItem> = doc
            .model()
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::Detail(d) => Some(d),
                _ => None,
            })
            .collect();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].points.len(), 3);
        assert_eq!(items[0].view, section);
        assert!(state.details.path.is_empty());
    }
}
