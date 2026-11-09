//! The profile library (MU27 block 5, approved option A): the presets and
//! the project's own profiles, listed in Manage › Profiles and in an area
//! of its own (Profiles in an area's view menu, as Notes). A project
//! profile is edited there: its corners dragged, its insertion point (the
//! origin) shown, open or closed. The Sweep and Extrusion tools pick from
//! it; a sweep's Properties change its profile or save it as one.

use super::*;
use crate::ui_model::Item;
use creabim_core::profiles::{self, NamedProfile, PRESETS};
use glam::DVec2;

/// The library's state between frames.
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
    frame: Option<(DVec2, f32)>,
}

/// A row of a list: its name from the left, lit when chosen.
fn list_row(ui: &mut egui::Ui, on: bool, text: &str, w: f32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 22.0), egui::Sense::click());
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
    let g = ui.painter().layout(text.to_string(), egui::FontId::proportional(13.0), ink, f32::INFINITY);
    let clip = ui.painter().with_clip_rect(r.shrink2(Vec2::new(6.0, 0.0)));
    clip.galley(r.left_center() + Vec2::new(8.0, -g.size().y / 2.0), g, ink);
    resp.on_hover_text(text)
}

/// What the library asks for once drawn.
enum Ask {
    Pick(String),
    Write(Vec<NamedProfile>, &'static str, bool),
    New,
    FromSelection,
    Copy,
    Use,
}

impl CreaBimApp {
    /// Write the project's profiles: merged into one undo step while one
    /// edit goes on (a drag), else a step of its own.
    fn put_profiles(&mut self, all: Vec<NamedProfile>, label: &str, merge: bool) {
        let model = self.doc.model();
        let before = profiles::project_profiles(model);
        let mut tx = profiles::set_profiles(model, all.clone(), label);
        // the sweeps made with a profile follow its edits, in the same step
        for p in &all {
            if before.iter().find(|b| b.name == p.name) != Some(p) {
                tx = profiles::follow(model, &p.name, &p.outline(), tx);
            }
        }
        let r = if merge && model.project_info().is_some() {
            self.doc.commit_merged(tx)
        } else {
            self.doc.commit(tx)
        };
        if let Err(e) = r {
            self.status = format!("Couldn't do that: {e}");
        }
    }

    /// A new project profile from the selected shape: a sketch drawn on
    /// any plane (open or closed, as drawn), a slab or an extrusion, its
    /// insertion point the middle of its bottom.
    pub(super) fn profile_from_selection(&mut self) {
        let model = self.doc.model();
        let Some((id, (pts, open))) = self
            .selection
            .all()
            .into_iter()
            .rev()
            .find_map(|id| Some((id, profiles::from_shape(model, id)?)))
        else {
            self.status = "Select a sketch (drawn on any plane, open or closed), a slab or an extrusion first: its shape becomes the profile.".into();
            return;
        };
        let base = model.get(id).map_or("Profile".to_string(), |e| format!("{} profile", e.name));
        let name = profiles::free_name(model, &base);
        let mut all = profiles::project_profiles(model);
        all.push(NamedProfile { name: name.clone(), points: pts, open, thickness: None });
        self.put_profiles(all, "Profile from a shape", false);
        self.open_profile(&name);
        self.status = format!(
            "Profile {name} made from the selection{}: pick it in the Sweep tool's Profile list, or on a sweep in Properties.",
            if open { " (open: swept as a 20 mm surface; set its thickness in Profiles)" } else { "" }
        );
    }

    fn new_profile(&mut self, from: Option<NamedProfile>, base: &str) {
        let model = self.doc.model();
        let name = profiles::free_name(model, base);
        let mut all = profiles::project_profiles(model);
        let mut p = from.unwrap_or(NamedProfile { points: creabim_core::parts::rectangle(0.1, 0.05), ..Default::default() });
        p.name = name.clone();
        all.push(p);
        self.put_profiles(all, "New profile", false);
        self.open_profile(&name);
    }

    fn open_profile(&mut self, name: &str) {
        // in its area when one shows the library, else its window
        if !self.areas.root.areas().contains(&crate::areas::Shows::Profiles) {
            self.profiles.open = true;
        }
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
            "sweep.lib" | "extrusion.lib" => {
                let lib = (!arg.is_empty()).then(|| arg.to_string());
                let (tool, what) = if head == "sweep.lib" {
                    self.plan.parts.library = lib.clone();
                    (Tool::Sweep, "Sweep")
                } else {
                    self.plan.parts.extrusion_library = lib.clone();
                    (Tool::Extrusion, "Extrusion")
                };
                let pick = if self.plan.tool == tool { String::new() } else { format!(" (pick {what} in the Model tab)") };
                self.status = match (lib, tool) {
                    (Some(n), Tool::Sweep) => format!("The Sweep tool{pick} uses {n}: click a wall to run it along, or click a path."),
                    (Some(n), _) => format!("The Extrusion tool{pick} places {n}: click where its insertion point goes; it is pulled up by the Height."),
                    (None, _) => format!("The {what} tool draws its own shape again."),
                };
            }
            _ => return false,
        }
        true
    }

    /// The Sweep (or Extrusion) tool's Profile list.
    pub(super) fn library_items(&self, head: &str, now: Option<String>, own_label: &str) -> Vec<Item> {
        let mut items = vec![Item::Header("This project".into())];
        let own = profiles::project_profiles(self.doc.model());
        if own.is_empty() {
            items.push(Item::new("profiles.new", "None yet: New profile…").enabled(true));
        }
        for p in &own {
            items.push(Item::new(format!("{head}:{}", p.name), &p.name).checked(now.as_deref() == Some(p.name.as_str())));
        }
        items.push(Item::Header("Presets".into()));
        for p in PRESETS {
            items.push(Item::new(format!("{head}:{p}"), p).checked(now.as_deref() == Some(p)));
        }
        items.push(Item::Separator);
        items.push(Item::new(format!("{head}:"), own_label).checked(now.is_none()));
        items.push(
            Item::new("profiles.fromsel", "From the selection")
                .icon("pipette")
                .hint("A sketch on any plane (open or closed), a slab or an extrusion: its shape becomes a profile of the project"),
        );
        items.push(Item::new("profiles.new", "New profile…").icon("plus"));
        items.push(Item::new("profiles.browser", "Profiles…").icon("profile"));
        items
    }

    pub(super) fn sweep_library_items(&self) -> Vec<Item> {
        self.library_items("sweep.lib", self.plan.parts.library.clone(), "The tool's own (Coping, Plinth, Bar, Steps)")
    }

    pub(super) fn extrusion_library_items(&self) -> Vec<Item> {
        self.library_items("extrusion.lib", self.plan.parts.extrusion_library.clone(), "Draw its outline (no profile)")
    }

    /// The browser's Profiles group.
    pub(super) fn profiles_group(&self) -> serde_json::Value {
        let own = profiles::project_profiles(self.doc.model());
        let rows: Vec<serde_json::Value> = own
            .iter()
            .map(|p| {
                serde_json::json!({
                    "id": format!("profile:{}", p.name), "name": p.name, "icon": "profile",
                    "sel": self.profiles.current.as_deref() == Some(p.name.as_str()),
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

    /// The library: its list beside the editor, `rect` wide (a window or
    /// an area).
    fn profiles_body(&mut self, ui: &mut egui::Ui, big: bool) -> Option<Ask> {
        let model = self.doc.model();
        let own = profiles::project_profiles(model);
        let mut ask = None;
        let current = self.profiles.current.clone();
        let mine = current.as_deref().and_then(|c| own.iter().find(|p| p.name == c).cloned());
        let shown: Option<NamedProfile> = mine.clone().or_else(|| {
            let n = current.clone()?;
            Some(NamedProfile { points: profiles::preset(&n)?, name: n, ..Default::default() })
        });
        let pr = &mut self.profiles;
        let list_w = 170.0;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(list_w);
                let h = if big { (ui.available_height() - 70.0).max(120.0) } else { 330.0 };
                egui::ScrollArea::vertical().max_height(h).show(ui, |ui| {
                    ui.label(egui::RichText::new("This project").weak().small());
                    if own.is_empty() {
                        ui.label(egui::RichText::new("None yet").weak());
                    }
                    for p in &own {
                        if list_row(ui, current.as_deref() == Some(p.name.as_str()), &p.name, list_w - 10.0).clicked() {
                            ask = Some(Ask::Pick(p.name.clone()));
                        }
                    }
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Presets").weak().small());
                    for p in PRESETS {
                        if list_row(ui, current.as_deref() == Some(p), p, list_w - 10.0).clicked() {
                            ask = Some(Ask::Pick(p.to_string()));
                        }
                    }
                });
                ui.add_space(4.0);
                if ui.add_sized([list_w - 10.0, 24.0], egui::Button::new("New")).on_hover_text("A new profile of the project: a 100 × 50 mm bar to reshape").clicked() {
                    ask = Some(Ask::New);
                }
                if ui.add_sized([list_w - 10.0, 24.0], egui::Button::new("From the selection")).on_hover_text("A sketch on any plane (open or closed), a slab or an extrusion: its shape becomes a profile").clicked() {
                    ask = Some(Ask::FromSelection);
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                let w = if big { ui.available_width().max(260.0) } else { 300.0 };
                ui.set_width(w);
                let Some(p) = shown.clone() else {
                    ui.add_space(8.0);
                    ui.label("Pick a profile to see it.");
                    ui.label(egui::RichText::new("Presets can be copied into the project to edit.").weak());
                    return;
                };
                let is_own = mine.is_some();
                let edit = |f: &mut dyn FnMut(&mut NamedProfile)| -> Vec<NamedProfile> {
                    let mut all = own.clone();
                    if let Some(x) = all.iter_mut().find(|x| Some(x.name.as_str()) == current.as_deref()) {
                        f(x);
                    }
                    all
                };
                if is_own {
                    crate::props::row(ui, "Name", |ui| {
                        let r = crate::props::fill(ui, egui::TextEdit::singleline(&mut pr.name).vertical_align(egui::Align::Center));
                        if r.lost_focus() && !pr.name.trim().is_empty() && Some(pr.name.trim()) != current.as_deref() {
                            let n = pr.name.trim().to_string();
                            ask = Some(Ask::Write(edit(&mut |x| x.name = n.clone()), "Rename profile", false));
                            pr.current = Some(pr.name.trim().to_string());
                        }
                    });
                } else {
                    crate::props::row(ui, "Preset", |ui| ui.label(p.name.clone()));
                }
                let pts = p.points.clone();
                let lo = pts.iter().fold(DVec2::splat(f64::MAX), |a, q| a.min(*q)).min(DVec2::ZERO);
                let hi = pts.iter().fold(DVec2::splat(f64::MIN), |a, q| a.max(*q)).max(DVec2::ZERO);
                crate::props::row(ui, "Size", |ui| {
                    ui.label(format!("{} × {}", crate::units::len(hi.x - lo.x), crate::units::len(hi.y - lo.y)))
                });
                if is_own {
                    let mut open = p.open;
                    if crate::props::choice(
                        ui,
                        "Shape",
                        &mut open,
                        &[(false, "Closed", "An outline: swept, a solid"), (true, "Open", "A line: swept, a surface as thick as below")],
                    ) {
                        ask = Some(Ask::Write(edit(&mut |x| x.open = open), "Profile open or closed", false));
                    }
                    if p.open {
                        let mut t = p.thickness.unwrap_or(profiles::OPEN_THICKNESS);
                        crate::props::row(ui, "Thickness", |ui| {
                            if crate::props::fill(ui, crate::units::drag(&mut t).range(0.001..=1.0)).changed() {
                                ask = Some(Ask::Write(edit(&mut |x| x.thickness = Some(t)), "Profile thickness", true));
                            }
                        });
                    }
                }
                // the editor: the shape, its corners, its insertion point
                let eh = if big { (ui.available_height() - 110.0).clamp(200.0, 900.0) } else { 240.0 };
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, eh), egui::Sense::click_and_drag());
                let pal = crate::theme::Palette::of(ui.ctx());
                ui.painter().rect_filled(rect, 8.0, pal.field);
                let fit = {
                    let span = (hi - lo).max_element().max(1e-3);
                    ((lo + hi) / 2.0, (rect.width().min(rect.height()) - 40.0) / span as f32)
                };
                // held still while a corner is dragged (MU35)
                let (mid, k) = match (pr.drag, pr.frame) {
                    (Some(_), Some(f)) => f,
                    _ => fit,
                };
                let to_s = |q: DVec2| rect.center() + Vec2::new(((q.x - mid.x) as f32) * k, -((q.y - mid.y) as f32) * k);
                let to_w = |q: egui::Pos2| mid + DVec2::new(((q.x - rect.center().x) / k) as f64, (-(q.y - rect.center().y) / k) as f64);
                let scr: Vec<egui::Pos2> = pts.iter().map(|q| to_s(*q)).collect();
                let accent = pal.accent;
                // an open line drawn open, its thickness shown faint (MU36)
                if p.open {
                    ui.painter().add(egui::Shape::line(scr.clone(), egui::Stroke::new(1.6, pal.text)));
                    let solid: Vec<egui::Pos2> = p.outline().iter().map(|q| to_s(*q)).collect();
                    ui.painter().add(egui::Shape::closed_line(solid, egui::Stroke::new(1.0, pal.muted)));
                } else {
                    ui.painter().add(egui::Shape::closed_line(scr.clone(), egui::Stroke::new(1.6, pal.text)));
                }
                let o = to_s(DVec2::ZERO);
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
                    if let (Some(i), Some(q), true) = (pr.drag, resp.interact_pointer_pos(), resp.dragged()) {
                        let w = to_w(q);
                        let w = DVec2::new((w.x * 1000.0).round() / 1000.0, (w.y * 1000.0).round() / 1000.0);
                        ask = Some(Ask::Write(edit(&mut |x| {
                            if let Some(c) = x.points.get_mut(i) {
                                *c = w;
                            }
                        }), "Shape profile", true));
                    }
                    if resp.drag_stopped() {
                        pr.drag = None;
                        pr.frame = None;
                    }
                    if let Some(q) = resp.interact_pointer_pos() {
                        if resp.secondary_clicked() && pts.len() > if p.open { 2 } else { 3 } {
                            if let Some(i) = near(q) {
                                ask = Some(Ask::Write(edit(&mut |x| {
                                    x.points.remove(i);
                                }), "Remove profile corner", false));
                            }
                        }
                        if resp.double_clicked() && near(q).is_none() && scr.len() >= 2 {
                            let n = scr.len();
                            let edges = if p.open { n - 1 } else { n };
                            let edge = (0..edges).min_by(|a, b| {
                                let d = |i: usize| {
                                    let (s0, s1) = (scr[i], scr[(i + 1) % n]);
                                    let t = ((q - s0).dot(s1 - s0) / (s1 - s0).length_sq().max(1e-6)).clamp(0.0, 1.0);
                                    (s0 + (s1 - s0) * t).distance(q)
                                };
                                d(*a).total_cmp(&d(*b))
                            });
                            if let Some(e) = edge {
                                let at = to_w(q);
                                ask = Some(Ask::Write(edit(&mut |x| x.points.insert(e + 1, at)), "Add profile corner", false));
                            }
                        }
                    }
                    crate::props::note(ui, "Drag a corner; double-click an edge for a new one; right-click a corner to remove it. The cross is where it sits on the path.");
                }
                let bw = (ui.available_width() - 6.0) / 2.0;
                ui.horizontal(|ui| {
                    if ui.add_sized([bw, 24.0], egui::Button::new("Use for Sweep")).on_hover_text("The Sweep tool takes this profile").clicked() {
                        ask = Some(Ask::Use);
                    }
                    if ui
                        .add_sized([bw, 24.0], egui::Button::new(if is_own { "Duplicate" } else { "Copy to project" }))
                        .on_hover_text("A copy of it in the project, to edit")
                        .clicked()
                    {
                        ask = Some(Ask::Copy);
                    }
                });
                if is_own && ui.add_sized([ui.available_width(), 24.0], egui::Button::new("Delete")).on_hover_text("Sweeps made with it keep their shape").clicked() {
                    let all: Vec<NamedProfile> = own.iter().filter(|x| Some(x.name.as_str()) != current.as_deref()).cloned().collect();
                    ask = Some(Ask::Write(all, "Delete profile", false));
                    pr.current = None;
                }
            });
        });
        ask
    }

    fn run_profile_ask(&mut self, ask: Option<Ask>) {
        let current = self.profiles.current.clone();
        match ask {
            Some(Ask::Pick(p)) => self.open_profile(&p),
            Some(Ask::Write(all, label, merge)) => self.put_profiles(all, label, merge),
            Some(Ask::New) => self.new_profile(None, "Profile"),
            Some(Ask::FromSelection) => self.profile_from_selection(),
            Some(Ask::Copy) => {
                let model = self.doc.model();
                let base = current.clone().unwrap_or_default();
                let from = profiles::project_profiles(model)
                    .into_iter()
                    .find(|p| Some(&p.name) == current.as_ref())
                    .or_else(|| Some(NamedProfile { points: profiles::preset(&base)?, ..Default::default() }));
                self.new_profile(from, &base);
            }
            Some(Ask::Use) => {
                let n = current.unwrap_or_default();
                self.profiles_action("sweep.lib", &n);
            }
            None => {}
        }
    }

    pub(super) fn profiles_window(&mut self, ctx: &egui::Context) {
        if !self.profiles.open {
            return;
        }
        let mut open = true;
        let mut ask = None;
        let area = self.mat.area.unwrap_or_else(|| ctx.content_rect());
        crate::app::shell::window("Profiles")
            .open(&mut open)
            .default_pos(area.min + Vec2::new(80.0, 80.0))
            .constrain_to(area)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| ask = self.profiles_body(ui, false));
        self.profiles.open = open;
        self.run_profile_ask(ask);
    }

    /// The library as an area (MU36: a profile as its own view): list and
    /// a large editor filling the area.
    pub(super) fn profiles_area(&mut self, ui: &mut egui::Ui, body: Rect) {
        let p = self.shell.theme.palette();
        ui.painter().rect_filled(body, 0.0, p.panel);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body.shrink(12.0)));
        let ask = self.profiles_body(&mut child, true);
        self.run_profile_ask(ask);
    }

    /// A sweep's Properties: its profile changed for another from the
    /// library, or saved as one (MU36).
    pub(super) fn sweep_profile_rows(&mut self, ui: &mut egui::Ui, id: ElementId) {
        let model = self.doc.model();
        let Some(ElementKind::Part(part)) = model.get(id).map(|e| &e.kind) else { return };
        let creabim_core::parts::Form::Sweep { profile, .. } = &part.form else { return };
        let profile = profile.clone();
        let link = part.profile.clone();
        let mut all: Vec<(String, Vec<DVec2>)> = profiles::project_profiles(model).into_iter().map(|p| (p.name.clone(), p.outline())).collect();
        all.extend(PRESETS.iter().filter_map(|n| Some((n.to_string(), profiles::preset(n)?))));
        // the profile it was made with, else one the same shape
        let now = link.clone().or_else(|| all.iter().find(|(_, p)| *p == profile).map(|(n, _)| n.clone()));
        let stale = now.as_deref().and_then(|n| all.iter().find(|(m, _)| m == n)).is_some_and(|(_, p)| *p != profile);
        let mut pick: Option<(String, Vec<DVec2>)> = None;
        let mut save = false;
        if !self.prop_header(ui, "sweep.profile", "Profile", true, None) {
            return;
        }
        crate::props::row(ui, "Profile", |ui| {
            let w = ui.available_width();
            egui::ComboBox::from_id_salt(("sweep.profile", id))
                .icon(crate::props::combo_icon)
                .width(w)
                .truncate()
                .selected_text(now.clone().unwrap_or_else(|| "Its own".into()))
                .show_ui(ui, |ui| {
                    for (n, p) in &all {
                        if ui.selectable_label(now.as_deref() == Some(n.as_str()), n).clicked() {
                            pick = Some((n.clone(), p.clone()));
                        }
                    }
                });
        });
        if stale {
            crate::props::note(ui, "Its profile has been edited since: Update takes the new shape.");
        }
        let (a, b) = crate::props::actions2(
            ui,
            "",
            ("Update", "Take its library profile's shape again", stale),
            ("Save as a profile", "Its shape, kept in the project's library to reuse or edit", true),
        );
        if a {
            save = true;
        }
        if b {
            let name = self.doc.model().get(id).map_or("Profile".into(), |e| e.name.clone());
            self.new_profile(Some(NamedProfile { points: profile.clone(), ..Default::default() }), &name);
        }
        if let Some((n, p)) = pick {
            let r = self.doc.edit(id, "Change sweep profile", |e| {
                if let ElementKind::Part(q) = &mut e.kind {
                    if let creabim_core::parts::Form::Sweep { profile, .. } = &mut q.form {
                        *profile = p.clone();
                    }
                    q.profile = Some(n.clone());
                }
            });
            self.report(r, format!("The sweep takes {n} now: editing {n} in Profiles updates it. Ctrl+Z puts it back."));
        }
        if let (true, Some(n)) = (save, now) {
            if let Some(p) = profiles::find(self.doc.model(), &n) {
                let r = self.doc.edit(id, "Update sweep profile", |e| {
                    if let ElementKind::Part(q) = &mut e.kind {
                        if let creabim_core::parts::Form::Sweep { profile, .. } = &mut q.form {
                            *profile = p.clone();
                        }
                        q.profile = Some(n.clone());
                    }
                });
                self.report(r, format!("Updated from {n}."));
            }
        }
    }
}
