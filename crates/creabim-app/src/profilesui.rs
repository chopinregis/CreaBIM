//! The profile library (MU27 block 5, approved option A): Manage › Profiles
//! lists the presets and the project's own profiles; a project profile is
//! edited in a small editor, its corners dragged, with its insertion point
//! (the origin) marked. The Sweep tool's Profile list picks from it.

use super::*;
use crate::ui_model::Item;
use creabim_core::profiles::{self, NamedProfile, PRESETS};

/// The Profiles window between frames.
#[derive(Clone, Debug, Default)]
pub struct ProfilesUi {
    pub open: bool,
    /// The profile shown: a preset's or the project's name.
    pub current: Option<String>,
    /// The corner being dragged in the editor.
    drag: Option<usize>,
    /// The name being typed.
    name: String,
    /// The editor's centre and scale, held while a corner is dragged.
    frame: Option<(glam::DVec2, f32)>,
}

/// A row of a list: its name from the left, lit when chosen.
fn list_row(ui: &mut egui::Ui, on: bool, text: &str) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(160.0, 22.0), egui::Sense::click());
    let v = ui.visuals();
    let fill = if on {
        v.selection.bg_fill
    } else if resp.hovered() {
        v.widgets.hovered.weak_bg_fill
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(r, 5.0, fill);
    let ink = if on { v.selection.stroke.color } else { v.text_color() };
    ui.painter().text(r.left_center() + Vec2::new(8.0, 0.0), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(13.0), ink);
    resp
}

impl CreaBimApp {
    /// Write the project's profiles: merged into one undo step while one
    /// edit goes on (a drag), else a step of its own.
    fn put_profiles(&mut self, all: Vec<NamedProfile>, label: &str, merge: bool) {
        let model = self.doc.model();
        let r = match model.project_info().map(|(id, _)| id) {
            Some(id) if merge => self.doc.edit_merged(id, label, |e| {
                if let ElementKind::Project(p) = &mut e.kind {
                    p.profiles = all.clone();
                }
            }),
            _ => {
                let tx = profiles::set_profiles(model, all, label);
                self.doc.commit(tx)
            }
        };
        if let Err(e) = r {
            self.status = format!("Couldn't do that: {e}");
        }
    }

    /// A new project profile from the selected shape (a closed sketch, a
    /// slab or a part's outline), its insertion point the middle of its
    /// bottom.
    fn profile_from_selection(&mut self) {
        let model = self.doc.model();
        let Some((id, pts)) = self
            .selection
            .all()
            .into_iter()
            .find_map(|id| Some((id, profiles::from_shape(model, id)?)))
        else {
            self.status = "Select a closed sketch, a slab or an extrusion first: its outline becomes the profile.".into();
            return;
        };
        let base = model.get(id).map_or("Profile".to_string(), |e| format!("{} profile", e.name));
        let name = profiles::free_name(model, &base);
        let mut all = profiles::project_profiles(model);
        all.push(NamedProfile { name: name.clone(), points: pts });
        self.put_profiles(all, "Profile from a shape", false);
        self.open_profile(&name);
        self.status = format!("Profile {name} made from the selection: pick it in the Sweep tool's Profile list.");
    }

    fn new_profile(&mut self, from: Option<Vec<glam::DVec2>>, base: &str) {
        let model = self.doc.model();
        let name = profiles::free_name(model, base);
        let mut all = profiles::project_profiles(model);
        all.push(NamedProfile {
            name: name.clone(),
            points: from.unwrap_or_else(|| creabim_core::parts::rectangle(0.1, 0.05)),
        });
        self.put_profiles(all, "New profile", false);
        self.open_profile(&name);
    }

    fn open_profile(&mut self, name: &str) {
        self.profiles.open = true;
        self.profiles.current = Some(name.to_string());
        self.profiles.name = name.to_string();
    }

    /// Profile actions from menus, the browser and the Manage tab.
    pub(super) fn profiles_action(&mut self, head: &str, arg: &str) -> bool {
        match head {
            "profiles.browser" => self.profiles.open = true,
            "profiles.new" => self.new_profile(None, "Profile"),
            "profiles.fromsel" => self.profile_from_selection(),
            "profiles.open" => self.open_profile(arg),
            "sweep.lib" => {
                self.plan.parts.library = (!arg.is_empty()).then(|| arg.to_string());
                let tool = if self.plan.tool == Tool::Sweep { "" } else { " (pick Sweep in the Model tab)" };
                self.status = match &self.plan.parts.library {
                    Some(n) => format!("The Sweep tool{tool} uses {n}: click a wall to run it along, or click a path."),
                    None => "Sweep with the tool's own profiles.".into(),
                };
            }
            _ => return false,
        }
        true
    }

    /// The Sweep tool's Profile list.
    pub(super) fn sweep_library_items(&self) -> Vec<Item> {
        let now = self.plan.parts.library.clone();
        let mut items = vec![Item::Header("Presets".into())];
        for p in PRESETS {
            items.push(Item::new(format!("sweep.lib:{p}"), p).checked(now.as_deref() == Some(p)));
        }
        let own = profiles::project_profiles(self.doc.model());
        items.push(Item::Header("This project".into()));
        if own.is_empty() {
            items.push(Item::new("profiles.new", "None yet").enabled(false));
        }
        for p in &own {
            items.push(Item::new(format!("sweep.lib:{}", p.name), &p.name).checked(now.as_deref() == Some(p.name.as_str())));
        }
        items.push(Item::Separator);
        items.push(Item::new("sweep.lib:", "The tool's own (Coping, Plinth, Bar, Steps)").checked(now.is_none()));
        items.push(
            Item::new("profiles.fromsel", "From the selection")
                .icon("pipette")
                .hint("A closed sketch, a slab or an extrusion selected: its outline becomes a profile of the project"),
        );
        items.push(Item::new("profiles.new", "New profile…").icon("plus"));
        items.push(Item::new("profiles.browser", "Profiles…").icon("profile"));
        items
    }

    /// The browser's Profiles group: the project's own, then a way to see
    /// the presets.
    pub(super) fn profiles_group(&self) -> serde_json::Value {
        let own = profiles::project_profiles(self.doc.model());
        let rows: Vec<serde_json::Value> = own
            .iter()
            .map(|p| {
                serde_json::json!({
                    "id": format!("profile:{}", p.name), "name": p.name, "icon": "profile",
                    "sel": self.profiles.open && self.profiles.current.as_deref() == Some(p.name.as_str()),
                    "hint": format!("{}: click to edit it", p.name),
                })
            })
            .collect();
        serde_json::json!({
            "key": "profiles", "title": "Profiles", "icon": "profile", "open": !rows.is_empty(),
            "rows": rows,
            "note": if own.is_empty() { format!("{} presets", PRESETS.len()) } else { String::new() },
            "acts": [
                {"id": "profiles.new", "icon": "plus", "hint": "A new profile of the project, to edit"},
                {"id": "profiles.browser", "icon": "ellipsis", "hint": "Profiles: the presets and the project's own; edit, copy, rename, delete"},
            ],
        })
    }

    pub(super) fn profiles_window(&mut self, ctx: &egui::Context) {
        if !self.profiles.open {
            return;
        }
        let model = self.doc.model();
        let own = profiles::project_profiles(model);
        let mut open = true;
        let mut pick: Option<String> = None;
        let mut write: Option<(Vec<NamedProfile>, &'static str, bool)> = None;
        let mut ask: Option<&'static str> = None;
        let current = self.profiles.current.clone();
        let is_own = current.as_deref().is_some_and(|c| own.iter().any(|p| p.name == c));
        let points = current.as_deref().and_then(|c| profiles::find(model, c));
        let area = self.mat.area.unwrap_or_else(|| ctx.content_rect());
        let pr = &mut self.profiles;
        crate::app::shell::window("Profiles")
            .open(&mut open)
            .default_pos(area.min + Vec2::new(80.0, 80.0))
            .constrain_to(area)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    // the list: the project's own, then the presets
                    ui.vertical(|ui| {
                        ui.set_width(170.0);
                        egui::ScrollArea::vertical().max_height(330.0).show(ui, |ui| {
                            ui.label(egui::RichText::new("This project").weak().small());
                            if own.is_empty() {
                                ui.label(egui::RichText::new("None yet").weak());
                            }
                            for p in &own {
                                if list_row(ui, current.as_deref() == Some(p.name.as_str()), &p.name).clicked() {
                                    pick = Some(p.name.clone());
                                }
                            }
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("Presets").weak().small());
                            for p in PRESETS {
                                if list_row(ui, current.as_deref() == Some(p), p).clicked() {
                                    pick = Some(p.to_string());
                                }
                            }
                        });
                        ui.add_space(4.0);
                        if ui.add_sized([160.0, 24.0], egui::Button::new("New")).on_hover_text("A new profile of the project: a 100 × 50 mm bar to reshape").clicked() {
                            ask = Some("new");
                        }
                        if ui.add_sized([160.0, 24.0], egui::Button::new("From the selection")).on_hover_text("A closed sketch, a slab or an extrusion selected: its outline becomes a profile").clicked() {
                            ask = Some("fromsel");
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_width(300.0);
                        let Some(pts) = points.clone() else {
                            ui.add_space(8.0);
                            ui.label("Pick a profile to see it.");
                            ui.label(egui::RichText::new("Presets can be copied into the project to edit.").weak());
                            return;
                        };
                        // its name, editable for the project's own
                        if is_own {
                            crate::props::row(ui, "Name", |ui| {
                                let r = crate::props::fill(ui, egui::TextEdit::singleline(&mut pr.name));
                                if r.lost_focus() && !pr.name.trim().is_empty() && Some(pr.name.trim()) != current.as_deref() {
                                    let mut all = own.clone();
                                    if let Some(p) = all.iter_mut().find(|p| Some(p.name.as_str()) == current.as_deref()) {
                                        p.name = pr.name.trim().to_string();
                                    }
                                    write = Some((all, "Rename profile", false));
                                    pr.current = Some(pr.name.trim().to_string());
                                }
                            });
                        } else {
                            crate::props::row(ui, "Preset", |ui| ui.label(current.clone().unwrap_or_default()));
                        }
                        let lo = pts.iter().fold(glam::DVec2::splat(f64::MAX), |a, p| a.min(*p)).min(glam::DVec2::ZERO);
                        let hi = pts.iter().fold(glam::DVec2::splat(f64::MIN), |a, p| a.max(*p)).max(glam::DVec2::ZERO);
                        crate::props::row(ui, "Size", |ui| {
                            ui.label(format!("{} × {}", crate::units::len(hi.x - lo.x), crate::units::len(hi.y - lo.y)))
                        });
                        // the editor: the outline, its corners, the
                        // insertion point
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(300.0, 240.0), egui::Sense::click_and_drag());
                        let pal = crate::theme::Palette::of(ui.ctx());
                        ui.painter().rect_filled(rect, 8.0, pal.field);
                        let fit = {
                            let span = (hi - lo).max_element().max(1e-3);
                            ((lo + hi) / 2.0, (rect.width().min(rect.height()) - 40.0) / span as f32)
                        };
                        // held still while a corner is dragged (MU35: the
                        // view refitting under the pointer ran away)
                        let (mid, k) = match (pr.drag, pr.frame) {
                            (Some(_), Some(f)) => f,
                            _ => fit,
                        };
                        let to_s = |p: glam::DVec2| rect.center() + Vec2::new(((p.x - mid.x) as f32) * k, -((p.y - mid.y) as f32) * k);
                        let to_w = |q: egui::Pos2| mid + glam::DVec2::new(((q.x - rect.center().x) / k) as f64, (-(q.y - rect.center().y) / k) as f64);
                        let scr: Vec<egui::Pos2> = pts.iter().map(|p| to_s(*p)).collect();
                        let accent = pal.accent;
                        ui.painter().add(egui::Shape::closed_line(scr.clone(), egui::Stroke::new(1.6, pal.text)));
                        let o = to_s(glam::DVec2::ZERO);
                        ui.painter().line_segment([o - Vec2::new(8.0, 0.0), o + Vec2::new(8.0, 0.0)], egui::Stroke::new(1.2, accent));
                        ui.painter().line_segment([o - Vec2::new(0.0, 8.0), o + Vec2::new(0.0, 8.0)], egui::Stroke::new(1.2, accent));
                        ui.painter().text(o + Vec2::new(6.0, 4.0), egui::Align2::LEFT_TOP, "insertion", egui::FontId::proportional(10.0), accent);
                        if is_own {
                            let hover = resp.hover_pos();
                            let near = |q: egui::Pos2| scr.iter().position(|s| s.distance(q) < 7.0);
                            for (i, s) in scr.iter().enumerate() {
                                let lit = hover.is_some_and(|h| h.distance(*s) < 7.0) || pr.drag == Some(i);
                                ui.painter().circle(*s, if lit { 5.0 } else { 3.5 }, egui::Color32::WHITE, egui::Stroke::new(1.4, accent));
                            }
                            if resp.drag_started() {
                                pr.drag = ui.input(|i| i.pointer.press_origin()).and_then(near);
                                pr.frame = Some(fit);
                            }
                            if let (Some(i), Some(q)) = (pr.drag, resp.interact_pointer_pos()) {
                                if resp.dragged() {
                                    // to the millimetre
                                    let w = to_w(q);
                                    let w = glam::DVec2::new((w.x * 1000.0).round() / 1000.0, (w.y * 1000.0).round() / 1000.0);
                                    let mut all = own.clone();
                                    if let Some(p) = all.iter_mut().find(|p| Some(p.name.as_str()) == current.as_deref()) {
                                        if let Some(c) = p.points.get_mut(i) {
                                            *c = w;
                                        }
                                    }
                                    write = Some((all, "Shape profile", true));
                                }
                            }
                            if resp.drag_stopped() {
                                pr.drag = None;
                                pr.frame = None;
                            }
                            // right-click a corner: remove it; double-click
                            // an edge: a corner there
                            if let Some(q) = resp.interact_pointer_pos() {
                                if resp.secondary_clicked() && pts.len() > 3 {
                                    if let Some(i) = near(q) {
                                        let mut all = own.clone();
                                        if let Some(p) = all.iter_mut().find(|p| Some(p.name.as_str()) == current.as_deref()) {
                                            p.points.remove(i);
                                        }
                                        write = Some((all, "Remove profile corner", false));
                                    }
                                }
                                if resp.double_clicked() && near(q).is_none() {
                                    let n = scr.len();
                                    let edge = (0..n).min_by(|a, b| {
                                        let d = |i: usize| {
                                            let (s0, s1) = (scr[i], scr[(i + 1) % n]);
                                            let t = ((q - s0).dot(s1 - s0) / (s1 - s0).length_sq().max(1e-6)).clamp(0.0, 1.0);
                                            (s0 + (s1 - s0) * t).distance(q)
                                        };
                                        d(*a).total_cmp(&d(*b))
                                    });
                                    if let Some(e) = edge {
                                        let mut all = own.clone();
                                        if let Some(p) = all.iter_mut().find(|p| Some(p.name.as_str()) == current.as_deref()) {
                                            p.points.insert(e + 1, to_w(q));
                                        }
                                        write = Some((all, "Add profile corner", false));
                                    }
                                }
                            }
                            crate::props::note(ui, "Drag a corner; double-click an edge for a new one; right-click a corner to remove it. The cross is where it sits on the path.");
                        }
                        let (use_it, copy) = ui
                            .horizontal(|ui| {
                                let w = (ui.available_width() - 6.0) / 2.0;
                                let a = ui.add_sized([w, 24.0], egui::Button::new("Use for Sweep")).on_hover_text("The Sweep tool takes this profile").clicked();
                                let b = ui
                                    .add_sized([w, 24.0], egui::Button::new(if is_own { "Duplicate" } else { "Copy to project" }))
                                    .on_hover_text("A copy of it in the project, to edit")
                                    .clicked();
                                (a, b)
                            })
                            .inner;
                        if use_it {
                            ask = Some("use");
                        }
                        if copy {
                            ask = Some("copy");
                        }
                        if is_own && ui.add_sized([ui.available_width(), 24.0], egui::Button::new("Delete")).on_hover_text("Sweeps made with it keep their shape").clicked() {
                            let all: Vec<NamedProfile> = own.iter().filter(|p| Some(p.name.as_str()) != current.as_deref()).cloned().collect();
                            write = Some((all, "Delete profile", false));
                            pr.current = None;
                        }
                    });
                });
            });
        self.profiles.open = open;
        if let Some(p) = pick {
            self.open_profile(&p);
        }
        if let Some((all, label, merge)) = write {
            self.put_profiles(all, label, merge);
        }
        match ask {
            Some("new") => self.new_profile(None, "Profile"),
            Some("fromsel") => self.profile_from_selection(),
            Some("copy") => {
                let base = current.clone().unwrap_or_default();
                self.new_profile(points, &base);
            }
            Some("use") => {
                let n = current.unwrap_or_default();
                self.profiles_action("sweep.lib", &n);
            }
            _ => {}
        }
    }
}
