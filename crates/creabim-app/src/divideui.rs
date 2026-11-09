//! The Divide window (MU34 B2): how many parts, which way, and what to
//! make in them, for the element selected first.

use super::*;
use creabim_core::divide::{self, Dir, Make};

/// What the Divide window holds between frames.
#[derive(Clone, Debug)]
pub struct DivideUi {
    pub open: bool,
    pub parts: usize,
    pub dir: Dir,
    pub make: Make,
    pub gap: f64,
}

impl Default for DivideUi {
    fn default() -> Self {
        Self { open: false, parts: 4, dir: Dir::Along, make: Make::Cuts, gap: 0.0 }
    }
}

impl CreaBimApp {
    pub(super) fn divide_window(&mut self, ctx: &egui::Context) {
        if !self.divide.open {
            return;
        }
        let all = self.selection.all();
        let host = all.first().copied().filter(|h| divide::span(self.doc.model(), *h).is_some());
        let item = (all.len() > 1).then(|| all.last().copied()).flatten();
        let mut open = true;
        let mut go = false;
        let d = &mut self.divide;
        let model = self.doc.model();
        let area = self.mat.area.unwrap_or_else(|| ctx.content_rect());
        crate::app::shell::window("Divide")
            .open(&mut open)
            .default_pos(area.min + Vec2::new(60.0, 90.0))
            .constrain_to(area)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.set_width(330.0);
                let Some(h) = host else {
                    // full size, with the panel's own text (MU35: small and squeezed)
                    ui.set_min_width(320.0);
                    ui.label("Select what to divide: a wall, a beam, a guide, or an extrusion drawn from a plan outline.");
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("For Copies, then Shift+click what goes in each part. A sketch? Extrude it first, or draw a guide along it.").weak());
                    return;
                };
                let s = divide::span(model, h).expect("has a span");
                let name = model.get(h).map_or(String::new(), |e| e.name.clone());
                crate::props::row(ui, "Divides", |ui| ui.label(name));
                crate::props::choice(
                    ui,
                    "Which way",
                    &mut d.dir,
                    &[(Dir::Along, "Along", "Its length, in plan"), (Dir::Up, "Up", "Its height: shelves, courses, bands")],
                );
                let total = match d.dir {
                    Dir::Along => s.len,
                    Dir::Up => s.z1 - s.z0,
                };
                crate::props::row(ui, "Parts", |ui| {
                    crate::props::fill(ui, egui::DragValue::new(&mut d.parts).range(2..=200).speed(0.1));
                });
                // only what applies (MU36: guides going up ran off endlessly,
                // a wall can't be cut into parts)
                let cuttable = matches!(model.get(h).map(|e| &e.kind), Some(ElementKind::Part(_) | ElementKind::Wall(_)));
                let mut opts: Vec<(Make, &str, &str)> = vec![];
                if cuttable {
                    opts.push((Make::Cuts, "Cuts", "Cut into parts: a wall into walls side by side or stacked (a base course, bands), a part into bays or shelves"));
                }
                opts.push((Make::Guides, "Guides", if d.dir == Dir::Up { "A level guide along its face at each division, as long as it is" } else { "A guide across it at each division, to snap to" }));
                opts.push((Make::Copies, "Copies", "What you Shift+clicked, one in each part"));
                if !opts.iter().any(|o| o.0 == d.make) {
                    d.make = opts[0].0;
                }
                crate::props::choice(ui, "Make", &mut d.make, &opts);
                if d.make == Make::Cuts {
                    crate::props::row(ui, "Gap", |ui| {
                        crate::props::fill(ui, crate::units::drag(&mut d.gap).range(0.0..=1.0));
                    });
                }
                let gap = if d.make == Make::Cuts { d.gap } else { 0.0 };
                let size = (total - gap * (d.parts as f64 - 1.0)) / d.parts as f64;
                ui.add_space(4.0);
                // the sum, plain and readable, in the field column
                crate::props::row(
                    ui,
                    "Each",
                    |ui| ui.label(egui::RichText::new(format!(
                        "{} ÷ {} = {}{}",
                        crate::units::len(total),
                        d.parts,
                        crate::units::len(size.max(0.0)),
                        match (d.make, item) {
                            (Make::Copies, Some(i)) => format!(" · copies {}", model.get(i).map_or("", |e| e.name.as_str())),
                            (Make::Copies, None) => " · Shift+click what to copy".into(),
                            _ => String::new(),
                        }
                    )).strong()),
                );
                crate::props::note(ui, "The orange marks in the 3D view show where the divisions fall.");
                if crate::props::action(ui, "", "Divide", "One undo step", true).clicked() {
                    go = true;
                }
            });
        self.divide.open = open;
        if let (true, Some(h)) = (go, host) {
            let d = self.divide.clone();
            match divide::divide(self.doc.model(), h, d.parts, d.dir, d.make, d.gap, item.filter(|_| d.make == Make::Copies)) {
                Ok(tx) => {
                    let r = self.doc.commit(tx);
                    self.report(r, format!("Divided in {}. Ctrl+Z puts it back.", d.parts));
                }
                Err(e) => self.status = e,
            }
        }
    }
}
