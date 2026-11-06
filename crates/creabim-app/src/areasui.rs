//! View areas in the app (brief 02, 2.6): each area's header with its
//! view's controls, the dividers between areas, maximising, and the
//! workspaces. The layout itself is in [`crate::areas`].

use super::*;
use crate::areas::{self, Node, Shows, Workspace};
use crate::ui_model::Item;
use serde_json::{json, Value};
use shell::icon_name;

/// The height of an area's header, in points.
pub(super) const HEADER: f32 = 30.0;

/// The areas as they are now.
#[derive(Clone, Debug)]
pub(super) struct AreaState {
    pub root: Node,
    /// The area tools, menus and Properties work in: the last one clicked.
    pub focus: usize,
    /// The area shown alone (Ctrl+Space), if one is.
    pub max: Option<usize>,
    /// The drawing open at the end of last frame, to see one opened since
    /// (from the project browser, a menu or a view mark).
    pub seen: Option<ElementId>,
    /// The workspace last picked, by name.
    pub workspace: String,
    /// Each area's rectangle (body only) as drawn last frame.
    pub rects: Vec<Rect>,
    /// The area under the pointer last frame.
    pub hovered: Option<usize>,
    /// Save current layout: the name being typed.
    pub naming: Option<String>,
    /// The last drawing (not a sheet) worked in, for workspaces.
    pub drawing: Option<ElementId>,
}

impl Default for AreaState {
    fn default() -> Self {
        Self {
            root: Node::split(
                true,
                0.5,
                Node::Area(Shows::Model),
                Node::Area(Shows::Current),
            ),
            focus: 1,
            max: None,
            seen: None,
            workspace: "Drawing".into(),
            rects: vec![],
            hovered: None,
            naming: None,
            drawing: None,
        }
    }
}

/// What a click in a header asks for.
enum Ask {
    Run(String),
    Popup(String, Rect),
    Number(f64),
}

impl CreaBimApp {
    /// Every workspace: the built-in ones, then the ones saved.
    pub(super) fn workspaces(&self) -> Vec<Workspace> {
        let mut all = areas::built_in();
        all.extend(self.shell.settings.workspaces.iter().cloned());
        all
    }

    /// The sheet `view` is placed on, if any.
    fn sheet_holding(&self, view: Option<ElementId>) -> Option<ElementId> {
        let view = view?;
        if self.is_sheet(view) {
            return Some(view);
        }
        self.doc.model().iter().find_map(|e| match &e.kind {
            ElementKind::Viewport(vp) if vp.view == view => Some(vp.sheet),
            _ => None,
        })
    }

    /// Open workspace `name`: its areas, with its open views filled in.
    pub(super) fn open_workspace(&mut self, name: &str) {
        let Some(w) = self.workspaces().into_iter().find(|w| w.name == name) else {
            return;
        };
        let current = self.current_drawing();
        let sheet = self
            .sheet_holding(current)
            .or_else(|| self.sheet_list().first().map(|s| s.0));
        let mut root = w.root.clone();
        root.resolve(current, sheet);
        let model = self.doc.model();
        root.forget(&|id| model.get(id).is_some(), current);
        self.areas.root = root;
        self.areas.max = None;
        // the drawing area takes the tools
        self.areas.focus = self
            .areas
            .root
            .areas()
            .iter()
            .position(|s| matches!(s, Shows::View(_)))
            .unwrap_or(0);
        self.areas.workspace = w.name.clone();
        self.shell.settings.workspace = w.name.clone();
        self.store_areas();
        self.status = format!("Workspace {}. {}.", w.name, w.hint);
    }

    /// The drawing (not a sheet) open now, for a workspace's open areas:
    /// the focused area's, else the first area's that shows one, else the
    /// last one open, else the first floor plan.
    fn current_drawing(&self) -> Option<ElementId> {
        let shows = self.areas.root.areas();
        let of = |s: &Shows| match s {
            Shows::View(v) if !self.is_sheet(*v) => Some(*v),
            _ => None,
        };
        shows
            .get(self.areas.focus)
            .and_then(of)
            .or_else(|| shows.iter().find_map(of))
            .or(self
                .areas
                .drawing
                .filter(|v| self.doc.model().get(*v).is_some()))
            .or(self.active_view.filter(|v| !self.is_sheet(*v)))
            .or_else(|| {
                crate::browser::view_groups(self.doc.model())
                    .into_iter()
                    .find_map(|(_, ids)| ids.first().copied())
            })
    }

    /// The drawing open now: the focused area's, else the first area's
    /// that shows one, else the one last open.
    fn drawing_open(&self) -> Option<ElementId> {
        let shows = self.areas.root.areas();
        let of = |s: &Shows| match s {
            Shows::View(v) => Some(*v),
            _ => None,
        };
        shows
            .get(self.areas.focus)
            .and_then(of)
            .or_else(|| shows.iter().find_map(of))
            .or(self.active_view)
    }

    fn store_areas(&mut self) {
        self.shell.settings.areas = Some(self.areas.root.clone());
        self.shell.settings.store();
    }

    /// Step to the next (`1`) or previous (`-1`) workspace.
    pub(super) fn step_workspace(&mut self, by: i32) {
        let all = self.workspaces();
        let i = all
            .iter()
            .position(|w| w.name == self.areas.workspace)
            .unwrap_or(0) as i32;
        let n = all.len() as i32;
        let next = &all[((i + by).rem_euclid(n)) as usize];
        self.open_workspace(&next.name.clone());
    }

    /// A drawing opened from elsewhere goes into the focused area when that
    /// shows a drawing, else into the first area that does; with only the
    /// 3D view open, the area splits to show it beside the model.
    fn take_opened_view(&mut self) {
        if self.active_view == self.areas.seen {
            return;
        }
        self.areas.seen = self.active_view;
        let Some(v) = self.active_view else { return };
        let shows = self.areas.root.areas();
        if shows.contains(&Shows::View(v)) {
            self.areas.focus = shows.iter().position(|s| *s == Shows::View(v)).unwrap_or(0);
            return;
        }
        let target = if matches!(shows.get(self.areas.focus), Some(Shows::View(_))) {
            Some(self.areas.focus)
        } else {
            shows.iter().position(|s| matches!(s, Shows::View(_)))
        };
        match target {
            Some(i) => {
                if let Some(s) = self.areas.root.get_mut(i) {
                    *s = Shows::View(v);
                }
                self.areas.focus = i;
            }
            None => {
                let i = self.areas.focus.min(shows.len() - 1);
                self.areas.focus = self.areas.root.divide(i, true, Shows::View(v));
                self.areas.max = None;
            }
        }
        self.store_areas();
    }

    /// The window body: its areas, each with its header, and the dividers.
    pub(super) fn show_areas(
        &mut self,
        ui: &mut egui::Ui,
        full: Rect,
        frame: &eframe::Frame,
        background: Color32,
    ) {
        // a workspace's open views are what is open now
        let current = self.active_view;
        let sheet = self
            .sheet_holding(current)
            .or_else(|| self.sheet_list().first().map(|s| s.0));
        self.areas.root.resolve(current, sheet);
        self.take_opened_view();
        let current = self.active_view;
        {
            let model = self.doc.model();
            self.areas
                .root
                .forget(&|id| model.get(id).is_some(), current);
        }
        let n = self.areas.root.len();
        self.areas.focus = self.areas.focus.min(n - 1);
        self.areas.max = self.areas.max.filter(|m| *m < n);
        let (mut rects, dividers) = self.areas.root.layout(full);
        if let Some(m) = self.areas.max {
            for (i, r) in rects.iter_mut().enumerate() {
                *r = if i == m { full } else { Rect::NOTHING };
            }
        } else {
            self.dividers(ui, &dividers);
        }
        let pointer = ui.input(|i| i.pointer.hover_pos());
        // a press on a window floating over the areas is the window's
        // (MU18): it neither focuses nor reaches the area under it
        let over_window = pointer.is_some_and(|p| {
            ui.ctx()
                .layer_id_at(p)
                .is_some_and(|l| l.order != egui::Order::Background)
        });
        let pressed = ui.input(|i| i.pointer.any_pressed()) && !over_window;
        self.areas.hovered = pointer.and_then(|p| rects.iter().position(|r| r.contains(p)));
        if pressed {
            if let Some(h) = self.areas.hovered {
                self.areas.focus = h;
            }
        }
        let shows = self.areas.root.areas();
        let model_area = shows.iter().position(|s| *s == Shows::Model);
        self.rect_3d = None;
        self.rect_2d = None;
        let mut bodies = vec![];
        let mut focus_rect = None;
        // how long each area took, logged while something is dragged, to
        // find what makes a drag slow (MU18)
        let mut timings: Vec<String> = vec![];
        for (i, (r, s)) in rects.iter().zip(&shows).enumerate() {
            if !r.is_positive() {
                bodies.push(Rect::NOTHING);
                continue;
            }
            let head = Rect::from_min_max(r.min, egui::pos2(r.max.x, r.min.y + HEADER));
            let body = Rect::from_min_max(egui::pos2(r.min.x, head.max.y), r.max);
            bodies.push(body);
            match s {
                Shows::Model if model_area == Some(i) => {
                    let t = std::time::Instant::now();
                    self.show_3d(ui, body, frame);
                    timings.push(format!("3D {:.1}", t.elapsed().as_secs_f64() * 1000.0));
                }
                Shows::View(v) => {
                    self.active_view = Some(*v);
                    let keep = self.rect_2d;
                    // two areas may show the same drawing
                    let t = std::time::Instant::now();
                    ui.push_id(("area", i), |ui| self.show_plan(ui, body, background));
                    timings.push(format!(
                        "area {i} {:.1}",
                        t.elapsed().as_secs_f64() * 1000.0
                    ));
                    // the pane tools act in: the focused one, else the last
                    if i != self.areas.focus && keep.is_some() {
                        self.rect_2d = keep;
                    }
                    if i == self.areas.focus {
                        focus_rect = Some(body);
                    }
                }
                _ => {
                    let p = self.shell.theme.palette();
                    ui.painter().rect_filled(body, 0.0, p.panel2);
                    ui.painter().with_clip_rect(body).text(
                        body.center(),
                        egui::Align2::CENTER_CENTER,
                        "The 3D view is open in another area.\nPick a drawing for this one above.",
                        egui::FontId::proportional(13.0),
                        p.muted,
                    );
                }
            }
            self.area_header(ui, i, head, *s);
        }
        if let Some(r) = focus_rect {
            self.rect_2d = Some(r);
        }
        if self.handle_drag.is_some() || self.plan.modal.is_some() {
            crate::actlog::line(format!("areas: {}", timings.join(" | ")));
        }
        if self.areas.max.is_none() {
            self.corners(ui, &rects);
        }
        // the focused area's drawing is the one menus and Properties use
        self.active_view = match shows.get(self.areas.focus) {
            Some(Shows::View(v)) => Some(*v),
            _ => shows
                .iter()
                .find_map(|s| match s {
                    Shows::View(v) => Some(*v),
                    _ => None,
                })
                .or(current),
        };
        self.areas.seen = self.active_view;
        if let Some(v) = self.active_view.filter(|v| !self.is_sheet(*v)) {
            self.areas.drawing = Some(v);
        }
        // the area under the pointer is outlined: keys and tools act there
        if n > 1 {
            if let Some(r) = self.areas.hovered.and_then(|h| rects.get(h)) {
                let p = self.shell.theme.palette();
                ui.painter().rect_stroke(
                    r.shrink(0.5),
                    0.0,
                    egui::Stroke::new(1.0, p.accent.gamma_multiply(0.8)),
                    egui::StrokeKind::Inside,
                );
            }
        }
        self.areas.rects = bodies;
    }

    /// Each area's corner grip, at the left end of its header: dragged into the area it
    /// splits it there (across or down, as the drag goes), dragged into a
    /// neighbour it closes the neighbour, whose room this one takes.
    fn corners(&mut self, ui: &mut egui::Ui, rects: &[Rect]) {
        let p = self.shell.theme.palette();
        let mut done: Option<(usize, egui::Pos2, egui::Pos2)> = None;
        for (i, r) in rects.iter().enumerate() {
            if !r.is_positive() {
                continue;
            }
            // in the header's corner, where no drawing takes the drag
            let grip = Rect::from_min_size(r.min, egui::vec2(12.0, HEADER));
            let resp = ui
                .interact(grip, ui.id().with(("area-corner", i)), Sense::drag())
                .on_hover_text(
                    "Drag into this area to split it, or into the one beside it to join the two",
                );
            let hot = resp.hovered() || resp.dragged();
            let c = if hot {
                p.accent
            } else {
                p.muted.gamma_multiply(0.7)
            };
            // three dots down the corner: something to take hold of
            for k in 0..3 {
                ui.painter().circle_filled(
                    egui::pos2(grip.center().x + 1.0, grip.top() + 9.0 + 6.0 * k as f32),
                    1.3,
                    c,
                );
            }
            if hot {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            if resp.dragged() {
                if let (Some(start), Some(at)) = (
                    resp.interact_pointer_pos().map(|_| grip.center()),
                    ui.input(|i| i.pointer.hover_pos()),
                ) {
                    self.corner_preview(ui, rects, i, start, at);
                }
            }
            if resp.drag_stopped() {
                if let Some(at) = ui
                    .input(|i| i.pointer.hover_pos())
                    .or(resp.interact_pointer_pos())
                {
                    done = Some((i, grip.center(), at));
                }
            }
        }
        let Some((i, start, at)) = done else { return };
        let r = rects[i];
        match rects.iter().position(|o| o.contains(at)) {
            Some(t) if t == i => {
                let d = at - start;
                if d.length() < 24.0 {
                    return;
                }
                let across = d.x.abs() >= (-d.y).abs();
                let frac = if across {
                    (at.x - r.left()) / r.width()
                } else {
                    (at.y - r.top()) / r.height()
                };
                let new = match self.areas.root.areas().get(i) {
                    Some(Shows::View(v)) => Shows::View(*v),
                    _ if !self.areas.root.has_model() => Shows::Model,
                    _ => self.drawing_open().map_or(Shows::Model, Shows::View),
                };
                self.areas.focus = self.areas.root.divide_at(i, across, new, frac);
                self.status = "Split the area: pick what the new one shows from its header.".into();
            }
            Some(t) => {
                if self.areas.root.close(t) {
                    self.areas.focus = if t < i { i - 1 } else { i };
                    self.status = "Joined the two areas.".into();
                }
            }
            None => return,
        }
        self.store_areas();
    }

    /// While a corner is dragged: where the split goes, or which area the
    /// join closes.
    fn corner_preview(
        &self,
        ui: &egui::Ui,
        rects: &[Rect],
        i: usize,
        start: egui::Pos2,
        at: egui::Pos2,
    ) {
        let p = self.shell.theme.palette();
        let r = rects[i];
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            ui.id().with("area-preview"),
        ));
        // a pill behind the words keeps them readable over the drawing
        let label = |c: egui::Pos2, text: &str| {
            let g =
                painter.layout_no_wrap(text.to_string(), egui::FontId::proportional(14.0), p.text);
            let b = Rect::from_center_size(c, g.size() + egui::vec2(20.0, 10.0));
            painter.rect(
                b,
                6.0,
                p.panel,
                egui::Stroke::new(1.0, p.accent),
                egui::StrokeKind::Inside,
            );
            painter.galley(b.min + egui::vec2(10.0, 5.0), g, p.text);
        };
        match rects.iter().position(|o| o.contains(at)) {
            Some(t) if t == i => {
                let d = at - start;
                if d.length() < 24.0 {
                    return;
                }
                // the new area, tinted, beyond the line it would split at
                let (line, new) = if d.x.abs() >= (-d.y).abs() {
                    (
                        [egui::pos2(at.x, r.top()), egui::pos2(at.x, r.bottom())],
                        Rect::from_min_max(egui::pos2(at.x, r.top()), r.max),
                    )
                } else {
                    (
                        [egui::pos2(r.left(), at.y), egui::pos2(r.right(), at.y)],
                        Rect::from_min_max(egui::pos2(r.left(), at.y), r.max),
                    )
                };
                painter.rect_filled(new, 0.0, p.accent.gamma_multiply(0.12));
                painter.line_segment(line, egui::Stroke::new(3.0, p.accent));
                label(new.center(), "Let go to split here");
            }
            Some(t) => {
                painter.rect_filled(rects[t], 0.0, p.accent.gamma_multiply(0.18));
                label(rects[t].center(), "Let go to join: this area closes");
            }
            None => {}
        }
    }

    /// Drag a divider to resize the areas beside it; double-click evens
    /// them out.
    fn dividers(&mut self, ui: &mut egui::Ui, dividers: &[areas::Divider]) {
        let p = self.shell.theme.palette();
        let mut changed = false;
        for d in dividers {
            let id = ui.id().with(("area-divider", d.path.clone()));
            let r = ui.interact(d.strip.expand(2.0), id, Sense::click_and_drag());
            let hot = r.hovered() || r.dragged();
            ui.painter().rect_filled(
                d.strip,
                0.0,
                if hot {
                    p.accent.gamma_multiply(0.6)
                } else {
                    p.border
                },
            );
            if hot {
                ui.ctx().set_cursor_icon(if d.across {
                    egui::CursorIcon::ResizeHorizontal
                } else {
                    egui::CursorIcon::ResizeVertical
                });
            }
            if r.dragged() {
                if let Some(at) = r.interact_pointer_pos() {
                    let at = if d.across { at.x } else { at.y };
                    self.areas.root.drag(&d.path, d.whole, at);
                    changed = true;
                }
            }
            if r.double_clicked() {
                if let Some(Node::Split { frac, .. }) = self.areas.root.split_at(&d.path) {
                    *frac = 0.5;
                }
                changed = true;
            }
            if r.drag_stopped() {
                self.store_areas();
            }
        }
        if changed {
            ui.ctx().request_repaint();
        }
    }

    /// The controls of the 3D view's header, most used first.
    fn model_controls(&self) -> Vec<Value> {
        let s = &self.shell.settings;
        vec![
            json!({"kind": "menu", "key": "render3d", "icon": s.render_3d.icon(), "label": "Shading",
                   "hint": format!("How the 3D view draws: {}. Shaded, with edges, hidden line or wireframe", s.render_3d.label())}),
            json!({"kind": "toggle", "id": "nav.ortho", "icon": if self.camera.ortho { "box" } else { "cuboid" },
                   "label": "Projection", "on": self.camera.ortho,
                   "hint": if self.camera.ortho { "Orthographic: click for perspective" } else { "Perspective: click for orthographic" }}),
            json!({"kind": "button", "id": "crop.add", "icon": "crop-box", "label": "Section box",
                   "hint": "Add a crop box to the 3D view: it crops everything outside it"}),
            json!({"kind": "menu", "key": "vis3d", "icon": if self.vis3d.is_empty() { "eye" } else { "eye-off" },
                   "label": "Show", "hint": "What the 3D view shows: grids, annotation, guides, sketches and every category"}),
            json!({"kind": "button", "id": "frame", "icon": "frame-all", "label": "Frame all", "shortcut": "Home",
                   "hint": "Frame everything"}),
            json!({"kind": "button", "id": "nav.fitsel", "icon": "scan", "label": "Frame the selection", "shortcut": ".",
                   "hint": "Zoom to what is selected", "off": self.selection.is_empty()}),
            json!({"kind": "toggle", "id": "xray", "icon": "xray", "label": "X-ray", "on": s.xray, "shortcut": "Alt+Z",
                   "hint": "See through walls, slabs and roofs to what is inside them"}),
            json!({"kind": "menu", "key": "look", "icon": "rotate-3d", "label": "Look from",
                   "hint": "Look from the top, a side or the corner"}),
            json!({"kind": "toggle", "id": "ground.grid", "icon": "grid-3x3", "label": "Ground grid", "on": s.ground_grid,
                   "hint": "The grid on the working level (or the workplane)"}),
            json!({"kind": "toggle", "id": "nav.orbitsel", "icon": "rotate-3d", "label": "Orbit around the selection",
                   "on": self.orbit_selection, "hint": "Orbiting swings round the selected elements"}),
        ]
    }

    /// One area's header: what it shows, which one, its view's controls,
    /// then More (⋯) and maximise. What doesn't fit goes in More, least
    /// used first.
    fn area_header(&mut self, ui: &mut egui::Ui, i: usize, head: Rect, shows: Shows) {
        let p = self.shell.theme.palette();
        ui.painter().rect_filled(head, 0.0, p.panel);
        ui.painter().line_segment(
            [head.left_bottom(), head.right_bottom()],
            egui::Stroke::new(1.0, p.border),
        );
        let focused = i == self.areas.focus && self.areas.root.len() > 1;
        if focused {
            ui.painter().line_segment(
                [
                    head.left_top() + egui::vec2(0.0, 1.0),
                    head.right_top() + egui::vec2(0.0, 1.0),
                ],
                egui::Stroke::new(2.0, p.accent),
            );
        }
        let controls: Vec<Value> = match shows {
            Shows::Model => self.model_controls(),
            Shows::View(_) => self
                .view_options()
                .into_iter()
                .filter(|c| c["kind"] != "sep")
                .collect(),
            _ => vec![],
        };
        let (kind_icon, kind_label, name) = match shows {
            Shows::View(v) => {
                let e = self.doc.model().get(v);
                let icon = e.map_or("map", |e| icon_name(Icon::of(e)));
                let group = self.group_title(v);
                (icon, group, e.map_or(String::new(), |e| e.name.clone()))
            }
            _ => ("box", "3D view", "Model".to_string()),
        };
        let mut asks: Vec<Ask> = vec![];
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("area-head", i))
                .max_rect(Rect::from_min_max(
                    head.min + egui::vec2(14.0, 3.0),
                    head.max - egui::vec2(6.0, 3.0),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 2.0;
        let room = head.width() - 20.0;
        // what shows: the kind, then which one
        let r = pill(&mut child, kind_icon, "", true, &p).on_hover_text(format!(
            "{kind_label}: click to show another kind of view here"
        ));
        if r.clicked() {
            asks.push(Ask::Popup("area.kind".into(), r.rect));
        }
        let name_w = (room * 0.32).clamp(70.0, 220.0);
        let r = name_button(&mut child, &name, name_w, &p).on_hover_text(format!(
            "{name}: click to show another of the {}",
            kind_label.to_lowercase()
        ));
        if r.clicked() {
            asks.push(Ask::Popup(
                if shows == Shows::Model {
                    "area.kind".into()
                } else {
                    "area.views".into()
                },
                r.rect,
            ));
        }
        child.add_space(6.0);
        // its controls, as many as fit before More and maximise
        let used = 34.0 + name_w + 10.0 + 2.0 * 26.0 + 8.0;
        let mut left = room - used;
        let mut over = vec![];
        for c in &controls {
            let w = control_width(c);
            if w > left {
                // a smaller one further on may still fit
                over.push(c.clone());
                continue;
            }
            left -= w;
            if let Some(a) = header_control(&mut child, c, &p) {
                asks.push(a);
            }
        }
        child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let maxed = self.areas.max == Some(i);
            let r = icon_btn(
                ui,
                if maxed { "square" } else { "maximize-2" },
                maxed,
                true,
                &p,
            )
            .on_hover_text(if maxed {
                "Back to the other areas (Ctrl+Space)"
            } else {
                "Show this area alone (Ctrl+Space)"
            });
            if r.clicked() {
                asks.push(Ask::Run(format!("area.max:{i}")));
            }
            let r = icon_btn(ui, "ellipsis", false, true, &p).on_hover_text(if over.is_empty() {
                "Split, join or close this area".to_string()
            } else {
                format!(
                    "{} more {} that don't fit, and split, join or close this area",
                    over.len(),
                    if over.len() == 1 {
                        "control"
                    } else {
                        "controls"
                    }
                )
            });
            if r.clicked() {
                asks.push(Ask::Popup("area.more".into(), r.rect));
            }
        });
        if !asks.is_empty() {
            // a header click works in its own area
            self.areas.focus = i;
            if let Shows::View(v) = shows {
                self.active_view = Some(v);
                self.areas.seen = Some(v);
            }
        }
        self.areas_over = if i == self.areas.focus {
            over
        } else {
            std::mem::take(&mut self.areas_over)
        };
        let ctx = ui.ctx().clone();
        for a in asks {
            match a {
                Ask::Run(id) => self.run_action(&ctx, &id),
                Ask::Popup(key, r) => self.toggle_popup(&key, r),
                Ask::Number(v) => self.set_view_number(v),
            }
        }
    }

    /// The group a view is listed in ("Floor plans", "Sheets"…).
    fn group_title(&self, v: ElementId) -> &'static str {
        if self.is_sheet(v) {
            return "Sheets";
        }
        crate::browser::view_groups(self.doc.model())
            .into_iter()
            .find(|(_, ids)| ids.contains(&v))
            .map_or("Views", |(g, _)| g.title())
    }

    /// The menus of an area's header, by key.
    pub(super) fn area_menu(&self, key: &str) -> Option<Vec<Item>> {
        let i = self.areas.focus;
        let shows = self.areas.root.areas();
        let here = shows.get(i).copied().unwrap_or(Shows::Model);
        Some(match key {
            "area.kind" => {
                let model_elsewhere = shows
                    .iter()
                    .enumerate()
                    .any(|(k, s)| k != i && *s == Shows::Model);
                let mut items = vec![Item::new("area.show:model", "3D view")
                    .icon("box")
                    .checked(here == Shows::Model)
                    .enabled(!model_elsewhere)
                    .hint(if model_elsewhere {
                        "The 3D view is open in another area"
                    } else {
                        "The model in 3D"
                    })];
                let title = match here {
                    Shows::View(v) => self.group_title(v),
                    _ => "",
                };
                for (g, ids) in crate::browser::view_groups(self.doc.model()) {
                    if let Some(first) = ids.first() {
                        items.push(
                            Item::new(format!("area.show:{}", first.0), g.title())
                                .checked(title == g.title())
                                .icon(group_icon(g.title())),
                        );
                    }
                }
                if let Some((s, _)) = self.sheet_list().first() {
                    items.push(
                        Item::new(format!("area.show:{}", s.0), "Sheets")
                            .checked(title == "Sheets")
                            .icon("file-text"),
                    );
                }
                items
            }
            "area.views" => {
                let Shows::View(v) = here else {
                    return Some(vec![]);
                };
                let ids: Vec<(ElementId, String)> = if self.is_sheet(v) {
                    self.sheet_list()
                } else {
                    crate::browser::view_groups(self.doc.model())
                        .into_iter()
                        .find(|(_, ids)| ids.contains(&v))
                        .map(|(_, ids)| ids)
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|id| Some((id, self.doc.model().get(id)?.name.clone())))
                        .collect()
                };
                ids.into_iter()
                    .map(|(id, name)| {
                        Item::new(format!("area.show:{}", id.0), name).checked(id == v)
                    })
                    .collect()
            }
            "area.more" => {
                let mut items = vec![];
                for c in &self.areas_over {
                    let label = c["label"]
                        .as_str()
                        .filter(|l| !l.is_empty())
                        .or(c["value"].as_str())
                        .unwrap_or("")
                        .to_string();
                    let icon = crate::svgicon::named(c["icon"].as_str().unwrap_or(""));
                    match c["kind"].as_str() {
                        Some("toggle") => items.push(
                            Item::new(c["id"].as_str().unwrap_or(""), label)
                                .checked(c["on"].as_bool().unwrap_or(false))
                                .icon(icon),
                        ),
                        Some("button") => {
                            items.push(Item::new(c["id"].as_str().unwrap_or(""), label).icon(icon))
                        }
                        Some("menu") => {
                            let key = c["key"].as_str().unwrap_or("");
                            let sub = self.menu_items(key);
                            if !sub.is_empty() {
                                items.push(Item::sub(label, sub).icon(icon));
                            }
                        }
                        _ => {}
                    }
                }
                if !items.is_empty() {
                    items.push(Item::Separator);
                }
                items.extend(self.area_layout_items());
                items
            }
            "workspaces" => {
                let mut items: Vec<Item> = self
                    .workspaces()
                    .into_iter()
                    .map(|w| {
                        Item::new(format!("workspace:{}", w.name), w.name.clone())
                            .checked(w.name == self.areas.workspace)
                            .hint(w.hint.clone())
                    })
                    .collect();
                items.push(Item::Separator);
                items.push(
                    Item::new("workspace.save", "Save current layout…")
                        .icon("save")
                        .hint("Keep these areas as a workspace of your own"),
                );
                if self
                    .shell
                    .settings
                    .workspaces
                    .iter()
                    .any(|w| w.name == self.areas.workspace)
                {
                    items.push(
                        Item::new(
                            "workspace.delete",
                            format!("Delete {}", self.areas.workspace),
                        )
                        .icon("trash-2"),
                    );
                }
                items.push(Item::Separator);
                items.extend(self.area_layout_items());
                items
            }
            _ => return None,
        })
    }

    /// Split, close and maximise, for the focused area.
    pub(super) fn area_layout_items(&self) -> Vec<Item> {
        let i = self.areas.focus;
        let n = self.areas.root.len();
        vec![
            Item::new(format!("area.split:{i}:1"), "Split side by side").icon("columns-2"),
            Item::new(format!("area.split:{i}:0"), "Split one above the other")
                .icon("panel-bottom"),
            Item::new(format!("area.close:{i}"), "Close this area")
                .icon("x")
                .enabled(n > 1)
                .hint("The area beside it takes its room"),
            Item::new(format!("area.max:{i}"), "Show this area alone")
                .key("Ctrl+Space")
                .checked(self.areas.max == Some(i))
                .icon("maximize-2"),
        ]
    }

    /// Run an area or workspace action; false when `head` is not one.
    pub(super) fn area_action(&mut self, head: &str, arg: &str) -> bool {
        let nums: Vec<usize> = arg.split(':').filter_map(|s| s.parse().ok()).collect();
        match head {
            "area.max" => {
                let i = nums.first().copied().unwrap_or(self.areas.focus);
                self.areas.max = if self.areas.max == Some(i) {
                    None
                } else {
                    Some(i)
                };
                self.areas.focus = i;
            }
            "area.split" => {
                let (i, across) = (nums.first().copied().unwrap_or(0), nums.get(1) != Some(&0));
                // the new half shows a drawing, or the 3D view if none is open
                let new = match self.areas.root.areas().get(i) {
                    Some(Shows::View(v)) => Shows::View(*v),
                    _ if !self.areas.root.has_model() => Shows::Model,
                    _ => self.drawing_open().map_or(Shows::Model, Shows::View),
                };
                self.areas.max = None;
                self.areas.focus = self.areas.root.divide(i, across, new);
                self.store_areas();
            }
            "area.close" => {
                let i = nums.first().copied().unwrap_or(self.areas.focus);
                if self.areas.root.close(i) {
                    self.areas.max = None;
                    self.areas.focus = i.saturating_sub(1).min(self.areas.root.len() - 1);
                    self.store_areas();
                }
            }
            "area.show" => {
                let i = self.areas.focus;
                let new = if arg == "model" {
                    Shows::Model
                } else {
                    match self.doc.model().iter().find(|e| e.id.0.to_string() == arg) {
                        Some(e) => Shows::View(e.id),
                        None => return true,
                    }
                };
                if new == Shows::Model && self.areas.root.has_model() {
                    self.status = "The 3D view is open in another area already.".into();
                    return true;
                }
                if let Some(s) = self.areas.root.get_mut(i) {
                    *s = new;
                }
                if let Shows::View(v) = new {
                    self.active_view = Some(v);
                    self.areas.seen = Some(v);
                }
                self.store_areas();
            }
            "workspace" => self.open_workspace(arg),
            "workspace.save" => {
                let n = self.shell.settings.workspaces.len() + 1;
                self.areas.naming = Some(format!("My layout {n}"));
            }
            "workspace.delete" => {
                let name = self.areas.workspace.clone();
                self.shell.settings.workspaces.retain(|w| w.name != name);
                self.open_workspace("Drawing");
            }
            _ => return false,
        }
        true
    }

    /// Save current layout: a small window to name it.
    pub(super) fn workspace_prompt(&mut self, ctx: &egui::Context) {
        let Some(mut name) = self.areas.naming.take() else {
            return;
        };
        let (mut open, mut done) = (true, false);
        egui::Window::new("Save current layout")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("A name for these areas, to pick from Workspace at the top right:");
                let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(220.0));
                r.request_focus();
                done = ui.input(|i| i.key_pressed(egui::Key::Enter)) || ui.button("Save").clicked();
            });
        let name = name.trim().to_string();
        if done && !name.is_empty() {
            // views stay as they are: a saved layout opens the same drawings
            let w = Workspace {
                name: name.clone(),
                root: self.areas.root.clone(),
                hint: format!("Your layout of {} areas", self.areas.root.len()),
            };
            self.shell.settings.workspaces.retain(|o| o.name != name);
            if areas::built_in().iter().any(|b| b.name == name) {
                self.status =
                    format!("{name} is one of the built-in workspaces: pick another name.");
                self.areas.naming = Some(name);
                return;
            }
            self.shell.settings.workspaces.push(w);
            self.areas.workspace = name.clone();
            self.shell.settings.workspace = name.clone();
            self.store_areas();
            self.status =
                format!("Saved the layout as {name}: it is in Workspace at the top right.");
        } else if open && !done {
            self.areas.naming = Some(name);
        }
    }

    /// Ctrl+Space, Ctrl+PgUp/PgDn and `/`, over the areas.
    pub(super) fn area_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (max, prev, next, iso) = ctx.input(|i| {
            (
                with_command(i, egui::Key::Space, true),
                with_command(i, egui::Key::PageUp, true),
                with_command(i, egui::Key::PageDown, true),
                with_command(i, egui::Key::Slash, false),
            )
        });
        if max {
            let i = self.areas.hovered.unwrap_or(self.areas.focus);
            self.area_action("area.max", &i.to_string());
        }
        if prev {
            self.step_workspace(-1);
        }
        if next {
            self.step_workspace(1);
        }
        if iso {
            self.toggle_isolate();
        }
    }
}

impl CreaBimApp {
    /// `/`: the selection alone, and back again.
    fn toggle_isolate(&mut self) {
        if !self.hidden.is_empty() {
            self.hidden.clear();
            self.hidden_rev += 1;
            self.status = "Everything shown again.".into();
        } else if self.selection.is_empty() {
            self.status =
                "Select something first: / shows it alone, and / again brings the rest back."
                    .into();
        } else {
            self.isolate_selected();
            self.status = "Showing the selection alone: / again brings the rest back.".into();
        }
    }
}

/// Was `key` pressed this frame with Ctrl (`command`) held, or without it?
/// Read from the key's own event: keys passed on from a web pane carry
/// their modifiers there.
pub(super) fn with_command(i: &egui::InputState, key: egui::Key, command: bool) -> bool {
    i.events.iter().any(|e| {
        matches!(e, egui::Event::Key { key: k, pressed: true, modifiers, .. }
            if *k == key && (modifiers.command || i.modifiers.command) == command
                && !modifiers.shift && !modifiers.alt)
    })
}

/// The icon for a group of views.
fn group_icon(title: &str) -> &'static str {
    match title {
        "Floor plans" => "map",
        "3D views" => "box",
        "Sections" => "scissors-line-dashed",
        "Elevations" => "elevation",
        "Details" => "callout",
        "Schedules" => "table-2",
        _ => "file-text",
    }
}

/// How wide a control is in a header.
fn control_width(c: &Value) -> f32 {
    match c["kind"].as_str() {
        Some("number") => 132.0,
        Some("menu") if c["icon"].as_str().is_none_or(|i| i.is_empty()) => 90.0,
        Some("menu") => 36.0,
        _ => 26.0,
    }
}

/// One control of a header.
fn header_control(ui: &mut egui::Ui, c: &Value, p: &crate::theme::Palette) -> Option<Ask> {
    let label = c["label"]
        .as_str()
        .filter(|l| !l.is_empty())
        .or(c["value"].as_str())
        .unwrap_or("");
    let mut tip = label.to_string();
    if let Some(k) = c["shortcut"].as_str() {
        tip += &format!("  ({k})");
    }
    if let Some(v) = c["value"].as_str().filter(|v| *v != label) {
        tip += &format!(": {v}");
    }
    if let Some(h) = c["hint"].as_str() {
        tip += &format!("\n{h}");
    }
    let icon = c["icon"].as_str().unwrap_or("");
    match c["kind"].as_str()? {
        "toggle" | "button" => {
            let on = c["on"].as_bool().unwrap_or(false);
            let enabled = !c["off"].as_bool().unwrap_or(false);
            let r = icon_btn(
                ui,
                if icon.is_empty() { "circle-dot" } else { icon },
                on,
                enabled,
                p,
            )
            .on_hover_text(tip);
            r.clicked()
                .then(|| Ask::Run(c["id"].as_str().unwrap_or("").into()))
        }
        "menu" => {
            let r = if icon.is_empty() {
                name_button(ui, c["value"].as_str().unwrap_or(label), 86.0, p)
            } else {
                pill(ui, icon, "", false, p)
            }
            .on_hover_text(tip);
            r.clicked()
                .then(|| Ask::Popup(c["key"].as_str().unwrap_or("").into(), r.rect))
        }
        "number" => {
            let mut v = c["value"].as_f64().unwrap_or(0.0);
            let unit = c["unit"].as_str().unwrap_or("");
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.set_width(126.0);
                ui.label(egui::RichText::new(label).size(12.0).color(p.muted))
                    .on_hover_text(&tip);
                let w = if unit == "m" {
                    ui.add_sized([64.0, 20.0], crate::units::drag(&mut v))
                } else {
                    ui.add_sized(
                        [64.0, 20.0],
                        egui::DragValue::new(&mut v)
                            .speed(c["step"].as_f64().unwrap_or(1.0))
                            .suffix(unit),
                    )
                };
                changed = w.changed();
            });
            changed.then_some(Ask::Number(v))
        }
        _ => None,
    }
}

fn icon_btn(
    ui: &mut egui::Ui,
    icon: &str,
    on: bool,
    enabled: bool,
    p: &crate::theme::Palette,
) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(
        egui::vec2(24.0, 24.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let fill = if on {
        p.accent.gamma_multiply(0.22)
    } else if resp.hovered() && enabled {
        p.raise
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(r, 6.0, fill);
    let mut c = if on { p.accent } else { p.text };
    if !enabled {
        c = c.gamma_multiply(0.35);
    }
    crate::svgicon::paint(ui.painter(), r.shrink(4.0), icon, c);
    resp
}

/// An icon with a small chevron: a menu.
fn pill(
    ui: &mut egui::Ui,
    icon: &str,
    _label: &str,
    strong: bool,
    p: &crate::theme::Palette,
) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(34.0, 24.0), Sense::click());
    let fill = if resp.hovered() {
        p.raise2
    } else if strong {
        p.raise
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(r, 6.0, fill);
    let ir = Rect::from_min_size(r.min + egui::vec2(3.0, 3.0), egui::vec2(18.0, 18.0));
    crate::svgicon::paint(ui.painter(), ir, icon, p.text);
    let cr = Rect::from_min_size(
        egui::pos2(r.right() - 12.0, r.center().y - 4.0),
        egui::vec2(8.0, 8.0),
    );
    crate::svgicon::paint(ui.painter(), cr, "chevron-down", p.muted);
    resp
}

/// A name with a chevron, as wide as `w` at most.
fn name_button(ui: &mut egui::Ui, name: &str, w: f32, p: &crate::theme::Palette) -> egui::Response {
    let font = egui::FontId::proportional(12.5);
    let galley = ui
        .painter()
        .layout_no_wrap(name.to_string(), font.clone(), p.text);
    let width = (galley.size().x + 26.0).min(w);
    let (r, resp) = ui.allocate_exact_size(egui::vec2(width, 24.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, 6.0, p.raise);
    }
    let clip = Rect::from_min_max(r.min, egui::pos2(r.right() - 16.0, r.bottom()));
    ui.painter().with_clip_rect(clip).galley(
        egui::pos2(r.left() + 6.0, r.center().y - galley.size().y / 2.0),
        galley,
        p.text,
    );
    let cr = Rect::from_min_size(
        egui::pos2(r.right() - 13.0, r.center().y - 4.0),
        egui::vec2(8.0, 8.0),
    );
    crate::svgicon::paint(ui.painter(), cr, "chevron-down", p.muted);
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspaces_fill_in_the_drawing_and_its_sheet() {
        let mut app = crate::app::test_app();
        let plan = app
            .doc
            .model()
            .iter()
            .find(|e| matches!(&e.kind, ElementKind::View(v) if matches!(v.kind, ViewKind::Plan { .. })))
            .map(|e| e.id)
            .unwrap();
        app.active_view = Some(plan);
        app.areas.drawing = Some(plan);
        app.open_workspace("Document");
        let shows = app.areas.root.areas();
        assert_eq!(shows.len(), 3);
        assert_eq!(shows[0], Shows::Model);
        assert_eq!(shows[1], Shows::View(plan));
        // the third is a sheet (the plan's, or the first)
        match shows[2] {
            Shows::View(s) => assert!(app.is_sheet(s)),
            other => panic!("{other:?}"),
        }
        // the Model workspace, then a drawing opened from the browser: the
        // 3D area makes room for it
        app.open_workspace("Model");
        assert_eq!(app.areas.root.areas(), [Shows::Model]);
        app.areas.seen = None;
        app.take_opened_view();
        assert_eq!(app.areas.root.areas(), [Shows::Model, Shows::View(plan)]);
        assert_eq!(app.areas.focus, 1);
        // Ctrl+Space shows it alone, and again brings the other back
        assert!(app.area_action("area.max", "1"));
        assert_eq!(app.areas.max, Some(1));
        assert!(app.area_action("area.max", "1"));
        assert_eq!(app.areas.max, None);
        // a second 3D area is refused
        app.areas.focus = 1;
        app.area_action("area.show", "model");
        assert_eq!(app.areas.root.areas()[1], Shows::View(plan));
        // stepping goes round the workspaces
        app.step_workspace(1);
        assert_eq!(app.areas.workspace, "Drawing");
        app.step_workspace(-2);
        assert_eq!(app.areas.workspace, "Sheets");
    }
}
