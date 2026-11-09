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
                    crate::props::note(ui, "Select a wall, a beam, a guide or a part drawn from a plan outline to divide. For Copies, then Shift+click what goes in each part.");
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
                crate::props::choice(
                    ui,
                    "Make",
                    &mut d.make,
                    &[
                        (Make::Cuts, "Cuts", "The element itself cut into the parts (a part only)"),
                        (Make::Guides, "Guides", "A guide at each division, to snap to"),
                        (Make::Copies, "Copies", "What you Shift+clicked, one in each part"),
                    ],
                );
                if d.make == Make::Cuts {
                    crate::props::row(ui, "Gap", |ui| {
                        crate::props::fill(ui, crate::units::drag(&mut d.gap).range(0.0..=1.0));
                    });
                }
                let gap = if d.make == Make::Cuts { d.gap } else { 0.0 };
                let size = (total - gap * (d.parts as f64 - 1.0)) / d.parts as f64;
                crate::props::note(
                    ui,
                    format!(
                        "{} ÷ {} = {} each{}",
                        crate::units::len(total),
                        d.parts,
                        crate::units::len(size.max(0.0)),
                        match (d.make, item) {
                            (Make::Copies, Some(i)) => format!(" · copies {}", model.get(i).map_or("", |e| e.name.as_str())),
                            (Make::Copies, None) => " · Shift+click what to copy".into(),
                            _ => String::new(),
                        }
                    ),
                );
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
