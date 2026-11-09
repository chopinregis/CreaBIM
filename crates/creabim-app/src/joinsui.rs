//! Junctions set by hand, in the app: with two elements selected that are
//! cut in drawings (walls, slabs, roofs, columns, beams, footings,
//! ceilings), the right-click menu switches which one cuts through the
//! other, turns the junction's cleanup off or on, or gives it back to the
//! automatic join by layer priority; an element's Properties list the
//! junctions set for it. Every change is one transaction, so it undoes.

use super::*;
use creabim_core::joins::{self, JoinRule, JoinWinner};

impl CreaBimApp {
    /// The two selected elements a junction rule can be set between.
    fn join_pair(&self) -> Option<(ElementId, ElementId)> {
        let model = self.doc.model();
        let ids: Vec<ElementId> = self
            .selection
            .all()
            .into_iter()
            .filter(|id| model.get(*id).is_some_and(|e| joins::joinable(&e.kind)))
            .collect();
        match (ids.as_slice(), self.selection.len()) {
            ([a, b], 2) => Some((*a, *b)),
            _ => None,
        }
    }

    /// The right-click menu's junction items, for two selected elements.
    pub(super) fn join_menu_items(&self) -> Vec<crate::ui_model::Item> {
        use crate::ui_model::Item;
        let Some((a, b)) = self.join_pair() else {
            return vec![];
        };
        let rule = joins::rule_for(self.doc.model(), a, b).map(|r| r.1);
        let mut items = vec![Item::new("join.switch", "Switch join order")
            .icon("join")
            .hint("The other element cuts through where the two overlap in plans and sections")];
        let off = rule.is_some_and(|r| !r.cleanup);
        items.push(
            Item::new(
                "join.cleanup",
                if off {
                    "Clean up the junction"
                } else {
                    "Don't clean up the junction"
                },
            )
            .icon("join")
            .hint("Off: both are drawn whole where they overlap, each with its own outline"),
        );
        if rule.is_some() {
            items.push(
                Item::new("join.auto", "Join automatically")
                    .icon("join")
                    .hint("Back to the automatic join: layer by layer, by priority"),
            );
        }
        items.push(Item::Separator);
        items
    }

    /// Run a junction item picked from a menu; false when `head` is not one.
    pub(super) fn join_action(&mut self, head: &str, _arg: &str) -> bool {
        if !matches!(head, "join.switch" | "join.cleanup" | "join.auto") {
            return false;
        }
        let Some((a, b)) = self.join_pair() else {
            self.status = "Select the two elements whose junction to set.".into();
            return true;
        };
        let model = self.doc.model();
        let (tx, ok) = match head {
            "join.switch" => {
                let tx = joins::switch_order(model, a, b);
                (
                    tx,
                    "Switched: the other element now cuts through. Ctrl+Z to undo.",
                )
            }
            "join.cleanup" => {
                let on = joins::rule_for(model, a, b).is_some_and(|r| !r.1.cleanup);
                let tx = joins::set_rule(model, a, b, "Junction cleanup", |r| r.cleanup = on);
                (
                    tx,
                    if on {
                        "The junction is cleaned up again."
                    } else {
                        "The junction is left as it is: both drawn whole."
                    },
                )
            }
            _ => {
                let tx = joins::set_rule(model, a, b, "Join automatically", |r| {
                    *r = JoinRule::new(r.a, r.b)
                });
                (
                    tx,
                    "The junction joins automatically again, by layer priority.",
                )
            }
        };
        let r = self.doc.commit(tx);
        self.report(r, ok);
        true
    }

    /// The junctions set by hand for element `id`, in its Properties: each
    /// with its winner and cleanup, and a button back to automatic.
    /// A wall's corners: for each end that meets one other wall, how the
    /// two join there, as a preset (3.7). The walls met, so their rules
    /// are not listed again.
    fn corner_rows(&mut self, ui: &mut egui::Ui, id: ElementId) -> Vec<ElementId> {
        let model = self.doc.model();
        if !matches!(model.get(id).map(|e| &e.kind), Some(ElementKind::Wall(_))) {
            return vec![];
        }
        let mut met = vec![];
        let mut pick = None;
        for (at_end, which) in [(false, "Start"), (true, "End")] {
            let Some(other) = creabim_core::corner_partner(model, id, at_end) else {
                continue;
            };
            met.push(other);
            let rule = joins::rule_for(model, id, other).map(|(_, r)| r);
            let now = creabim_core::CornerPreset::of(rule.as_ref(), id);
            let name = model
                .get(other)
                .map_or("(missing)".into(), |e| e.name.clone());
            // the end in the label column, the wall it meets on hover, the
            // choice filling the field column (MU29: it ran off the panel)
            crate::props::row(ui, which, |ui| {
                egui::ComboBox::from_id_salt(("corner", id, at_end)).icon(crate::props::combo_icon)
                    .selected_text(now.label())
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        for p in creabim_core::CornerPreset::ALL {
                            if ui.selectable_label(p == now, p.label()).clicked() && p != now {
                                pick = Some((other, p));
                            }
                        }
                    })
                    .response
                    .on_hover_text(format!("With {name}. How the two walls' layers meet at this corner. Auto mitres every layer; Cores lap runs the stronger wall's core on past the other's; a wall running through has every layer of the other stop at it; Not joined leaves both ends square."))
            });
        }
        if let Some((other, p)) = pick {
            let tx = joins::set_rule(self.doc.model(), id, other, "Corner join", |r| {
                p.apply(r, id)
            });
            let res = self.doc.commit(tx);
            self.report(
                res,
                format!("Corner: {}. Ctrl+Z to undo.", p.label().to_lowercase()),
            );
        }
        met
    }

    pub(super) fn joins_section(&mut self, ui: &mut egui::Ui, id: ElementId) {
        let corners = self.corner_rows(ui, id);
        let model = self.doc.model();
        let mine: Vec<(ElementId, JoinRule)> = model
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::Join(r) if r.a == id || r.b == id => Some((e.id, *r)),
                _ => None,
            })
            // a corner's rule is set by its preset above
            .filter(|(_, r)| !corners.iter().any(|o| r.joins(id, *o)))
            .collect();
        if mine.is_empty() && !corners.is_empty() {
            return;
        }
        if mine.is_empty() {
            ui.label(
                egui::RichText::new(
                    "Joins automatically: layer by layer, the structure through finishes. Select it with another element and right-click to switch the join order or turn cleanup off.",
                )
                .weak(),
            );
            return;
        }
        let mut edit: Option<(ElementId, JoinRule, &'static str)> = None;
        for (rid, mut r) in mine {
            let other = if r.a == id { r.b } else { r.a };
            let name = model
                .get(other)
                .map_or("(missing)".into(), |e| e.name.clone());
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("With {name}"));
                let now = crate::detailtool::winner_label(model, &r, r.winner);
                let shown = if r.cleanup {
                    now
                } else {
                    "Not cleaned up".into()
                };
                egui::ComboBox::from_id_salt(("join", rid))
                    .icon(crate::props::combo_icon)
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        let rule = r;
                        for w in JoinWinner::ALL {
                            let text = crate::detailtool::winner_label(model, &rule, w);
                            if ui
                                .selectable_label(r.cleanup && r.winner == w, text)
                                .clicked()
                            {
                                r.winner = w;
                                r.cleanup = true;
                                edit = Some((rid, r, "Junction winner"));
                            }
                        }
                        if ui.selectable_label(!r.cleanup, "Not cleaned up").clicked() {
                            r.cleanup = false;
                            edit = Some((rid, r, "Junction cleanup"));
                        }
                    });
            });
        }
        if let Some((_, r, label)) = edit {
            let tx = joins::set_rule(self.doc.model(), r.a, r.b, label, |x| *x = r);
            let res = self.doc.commit(tx);
            self.report(res, "Junction changed. Ctrl+Z to undo.");
        }
    }
}
