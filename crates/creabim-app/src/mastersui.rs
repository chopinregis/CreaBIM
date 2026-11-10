//! Masters (MU40): the sheet templates, drawn in a view of their own as
//! profiles are. The header picks a master and makes, duplicates, brings
//! in, writes out and deletes them; its tools draw lines, rectangles,
//! text and the sheet's fields as chips, snapping to a millimetre grid,
//! the paper's edges and margins and what is drawn already. Properties
//! sets the master's paper, border and own fields, and what is picked.
//!
//! A master is the frame; a sheet uses one. Change a master and every
//! sheet on it changes.

use super::*;
use crate::ui_model::Item;
use creabim_core::titleblock::{self as tb, Corner, SheetTemplate, TextAlign, TitleItem};
use glam::DVec2;

/// What the Masters view's pointer does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MTool {
    /// Pick, drag and nudge what is drawn.
    #[default]
    Select,
    /// Click corners; Enter, a right-click or the first corner again ends.
    Line,
    /// Two corners.
    Rect,
    /// Click where text goes.
    Text,
    /// Click where a field goes: the sheet fills it in.
    Field,
}

/// The Masters view between frames.
#[derive(Clone, Debug, Default)]
pub struct MastersUi {
    /// The master shown.
    pub current: Option<ElementId>,
    pub tool: MTool,
    /// Centre (paper mm) and points per mm.
    camera: Option<(DVec2, f32)>,
    /// The item picked.
    pub sel: Option<usize>,
    /// A drag of the picked item: where it was grabbed (mm), how far it
    /// has gone; shown moved, committed on release.
    drag: Option<(DVec2, DVec2)>,
    /// Corners of a line or rectangle being drawn (paper mm).
    drawing: Vec<DVec2>,
    /// The field the Field tool places.
    pub field: String,
    /// The pass the pointer was last over the view.
    hovered_pass: u64,
    /// The view's size last frame: a new size fits the sheet again.
    seen: egui::Vec2,
}

/// Sheet sizes, mm, landscape.
const PAPERS: [(&str, f64, f64); 5] = [
    ("A4", 297.0, 210.0),
    ("A3", 420.0, 297.0),
    ("A2", 594.0, 420.0),
    ("A1", 841.0, 594.0),
    ("A0", 1189.0, 841.0),
];

/// The points of an item, paper mm from the master's corner.
fn item_points(it: &TitleItem) -> Vec<DVec2> {
    match it {
        TitleItem::Line { points, .. } | TitleItem::Fill { points, .. } => points.clone(),
        TitleItem::Text { at, .. } => vec![*at],
    }
}

fn shift(it: &mut TitleItem, d: DVec2) {
    match it {
        TitleItem::Line { points, .. } | TitleItem::Fill { points, .. } => {
            points.iter_mut().for_each(|p| *p += d)
        }
        TitleItem::Text { at, .. } => *at += d,
    }
}

/// A text's box, roughly, mm from the corner: for picking.
fn text_box(at: DVec2, size: f64, text: &str, align: TextAlign) -> (DVec2, DVec2) {
    let w = size * 0.62 * text.chars().count().max(1) as f64;
    let x0 = match align {
        TextAlign::Start => at.x,
        TextAlign::Middle => at.x - w / 2.0,
        TextAlign::End => at.x - w,
    };
    (
        DVec2::new(x0, at.y - size * 0.7),
        DVec2::new(x0 + w, at.y + size * 0.7),
    )
}

fn seg_dist(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let t = if ab.length_squared() < 1e-12 {
        0.0
    } else {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    };
    p.distance(a + ab * t)
}

/// How far `p` is from an item, mm (0 inside a text's box).
fn item_dist(it: &TitleItem, p: DVec2) -> f64 {
    match it {
        TitleItem::Line { points, .. } | TitleItem::Fill { points, .. } => points
            .windows(2)
            .map(|w| seg_dist(p, w[0], w[1]))
            .fold(f64::MAX, f64::min),
        TitleItem::Text {
            at,
            size,
            text,
            align,
            ..
        } => {
            let (lo, hi) = text_box(*at, *size, text, *align);
            let q = p.clamp(lo, hi);
            p.distance(q)
        }
    }
}

impl CreaBimApp {
    /// The masters of the project, by name.
    fn masters(&self) -> Vec<(ElementId, String)> {
        super::drafting::templates(self.doc.model())
    }

    /// The master shown: the one picked, else the first.
    fn master(&self) -> Option<(ElementId, String, SheetTemplate)> {
        let model = self.doc.model();
        let get = |id: ElementId| match model.get(id).map(|e| (&e.kind, &e.name)) {
            Some((ElementKind::SheetTemplate(t), n)) => Some((id, n.clone(), t.clone())),
            _ => None,
        };
        self.masters
            .current
            .and_then(get)
            .or_else(|| self.masters().first().and_then(|(id, _)| get(*id)))
    }

    /// Change the master shown, as one undo step.
    fn edit_master(&mut self, label: &str, f: impl FnOnce(&mut SheetTemplate)) {
        let Some((id, _, mut t)) = self.master() else {
            return;
        };
        f(&mut t);
        if t.validate().is_err() {
            self.status = "That would make the master unreadable: not changed.".into();
            return;
        }
        let r = self
            .doc
            .edit_merged(id, label, move |e| e.kind = ElementKind::SheetTemplate(t));
        if let Err(e) = r {
            self.status = format!("Couldn't change the master: {e}");
        }
    }

    /// Show master `id` in a Masters view: one there, else the focused
    /// area turns into one.
    pub(super) fn open_master(&mut self, id: Option<ElementId>) {
        if id.is_some() {
            self.masters.current = id;
        }
        self.masters.sel = None;
        self.masters.camera = None;
        self.masters.drawing.clear();
        self.prop_tab = PropTab::Master;
        if !self
            .areas
            .root
            .areas()
            .contains(&crate::areas::Shows::Masters)
        {
            let i = self.areas.focus;
            if let Some(s) = self.areas.root.get_mut(i) {
                *s = crate::areas::Shows::Masters;
            }
            self.store_areas();
        }
    }

    /// Is a Masters view the area in front?
    pub(super) fn masters_in_front(&self) -> bool {
        matches!(
            self.areas.root.areas().get(self.areas.focus),
            Some(crate::areas::Shows::Masters)
        )
    }

    /// Is the pointer over a Masters view (its keys win there)?
    pub(super) fn masters_view_hovered(&self, ctx: &egui::Context) -> bool {
        self.masters.hovered_pass + 1 >= ctx.cumulative_pass_nr()
            && self
                .areas
                .root
                .areas()
                .contains(&crate::areas::Shows::Masters)
    }

    /// The master's name for the header.
    pub(super) fn master_title(&self) -> String {
        self.master()
            .map_or_else(|| "Pick a master".to_string(), |(_, n, _)| n)
    }

    /// The buttons beside the name: new, duplicate, bring in, write out,
    /// a sheet from it, delete.
    pub(super) fn master_buttons(&self) -> Vec<(&'static str, &'static str, String, bool)> {
        let cur = self.master();
        let name = cur.as_ref().map(|(_, n, _)| n.clone());
        let has = cur.is_some();
        vec![
            (
                "master.newmenu",
                "plus",
                "New master: from Standard, Minimal or blank".into(),
                true,
            ),
            (
                "master.duplicate",
                "copy",
                match &name {
                    Some(n) => format!("Duplicate {n}: a copy to change"),
                    None => "Duplicate: pick a master first".into(),
                },
                has,
            ),
            (
                "master.import",
                "download",
                "Bring in a frame drawn elsewhere: an SVG or DXF file".into(),
                true,
            ),
            (
                "master.export",
                "upload",
                "Write it out as SVG, to finish in Inkscape or any drawing program".into(),
                has,
            ),
            (
                "master.sheet",
                "file-plus",
                "A new sheet on this master".into(),
                has,
            ),
            (
                "master.delete",
                "trash-2",
                match &name {
                    Some(n) => format!("Delete {n}: its sheets go back to the built-in block"),
                    None => "Delete: pick a master first".into(),
                },
                has,
            ),
        ]
    }

    /// The tools, after the buttons.
    pub(super) fn master_controls(&self) -> Vec<serde_json::Value> {
        use serde_json::json;
        let t = self.masters.tool;
        let field = if self.masters.field.is_empty() {
            "project"
        } else {
            self.masters.field.as_str()
        };
        vec![
            json!({"kind": "toggle", "id": "master.tool:select", "icon": "mouse-pointer-2", "label": "Select", "on": t == MTool::Select,
                   "hint": "Pick and drag what is drawn; arrows nudge it 1 mm (Shift 5 mm); Delete removes it"}),
            json!({"kind": "toggle", "id": "master.tool:line", "icon": "slash", "label": "Line", "on": t == MTool::Line,
                   "hint": "Lines: click corners; Enter, a right-click or the first corner again ends"}),
            json!({"kind": "toggle", "id": "master.tool:rect", "icon": "square", "label": "Rectangle", "on": t == MTool::Rect,
                   "hint": "A rectangle: two corners"}),
            json!({"kind": "toggle", "id": "master.tool:text", "icon": "text-tool", "label": "Text", "on": t == MTool::Text,
                   "hint": "Text: click where it goes, then type it in Properties"}),
            json!({"kind": "menu", "key": "master.field", "icon": "field", "label": "Field", "value": format!("{{{field}}}"),
                   "hint": "A field: what each sheet fills in (project, sheet, number, scale, date, or one of the master's own). Pick one, then click where it goes"}),
        ]
    }

    /// The Masters view's header menus.
    pub(super) fn master_menu(&self, key: &str) -> Option<Vec<Item>> {
        Some(match key {
            "master.pick" => {
                let cur = self.master().map(|(id, _, _)| id);
                let mut v: Vec<Item> = self
                    .masters()
                    .into_iter()
                    .map(|(id, n)| {
                        Item::new(format!("master.open:{}", id.0), n)
                            .icon("sheet-template")
                            .checked(Some(id) == cur)
                    })
                    .collect();
                if !v.is_empty() {
                    v.push(Item::Separator);
                }
                v.extend(self.master_new_items());
                v
            }
            "master.newmenu" => self.master_new_items(),
            "master.field" => {
                let own: Vec<(String, String)> =
                    self.master().map(|(_, _, t)| t.fields).unwrap_or_default();
                let mut v: Vec<Item> = tb::BUILT_IN_FIELDS
                    .iter()
                    .map(|(f, what)| {
                        Item::new(format!("master.field:{f}"), format!("{{{f}}}")).hint(*what)
                    })
                    .collect();
                if !own.is_empty() {
                    v.push(Item::Separator);
                    for (f, _) in own {
                        v.push(
                            Item::new(format!("master.field:{f}"), format!("{{{f}}}"))
                                .hint("One of the master's own fields"),
                        );
                    }
                }
                v
            }
            _ => return None,
        })
    }

    fn master_new_items(&self) -> Vec<Item> {
        let mut v: Vec<Item> = tb::styles()
            .into_iter()
            .map(|(n, _)| {
                Item::new(format!("master.new:{n}"), format!("New from {n}")).icon("plus")
            })
            .collect();
        v.push(Item::new("master.new:", "New blank master").icon("plus"));
        v.push(
            Item::new("master.import", "Bring in an SVG or DXF…")
                .icon("download")
                .hint("A frame drawn in Inkscape, AutoCAD or any drawing program"),
        );
        v
    }

    /// The browser's Masters group: the frames sheets are drawn on, apart
    /// from the sheets themselves.
    pub(super) fn masters_group(&self) -> serde_json::Value {
        let cur = self.master().map(|(id, _, _)| id);
        let model = self.doc.model();
        let rows: Vec<serde_json::Value> = self
            .masters()
            .into_iter()
            .map(|(id, n)| {
                let users = super::drafting::template_users(model, id).len();
                serde_json::json!({
                    "id": format!("master:{}", id.0), "name": n, "icon": "sheet-template",
                    "sel": Some(id) == cur && self.areas.root.areas().contains(&crate::areas::Shows::Masters),
                    "hint": format!("{n}: {users} sheet(s) on it. Click to open it in a Masters view"),
                })
            })
            .collect();
        serde_json::json!({
            "key": "masters", "title": "Masters", "icon": "sheet-template", "open": !rows.is_empty(),
            "rows": rows,
            "note": if rows.is_empty() { "The built-in title block".to_string() } else { String::new() },
            "acts": [
                {"id": "master.new:Standard A3", "icon": "plus", "hint": "A new master from the Standard strip, in a Masters view"},
                {"id": "master.view", "icon": "ellipsis", "hint": "Open the Masters view"},
            ],
        })
    }

    /// Masters actions from the header, menus and the browser.
    pub(super) fn masters_action(&mut self, head: &str, arg: &str) -> bool {
        match head {
            "master.view" => {
                let id = self.master().map(|(id, _, _)| id);
                self.open_master(id);
            }
            "master.open" => {
                let id = self
                    .doc
                    .model()
                    .iter()
                    .find(|e| e.id.0.to_string() == arg)
                    .map(|e| e.id);
                self.open_master(id);
            }
            "master.new" => {
                let (base, t) = match tb::styles().into_iter().find(|(n, _)| *n == arg) {
                    Some((n, t)) => (n.to_string(), t),
                    None => (
                        "Master".to_string(),
                        SheetTemplate {
                            corner: Corner::BottomRight,
                            items: vec![],
                            border: true,
                            fields: vec![],
                            paper: Some(DVec2::new(420.0, 297.0)),
                        },
                    ),
                };
                let name = self.master_name(&base);
                self.add_master(name, t);
            }
            "master.duplicate" => {
                if let Some((_, n, t)) = self.master() {
                    let name = self.master_name(&format!("{n} copy"));
                    self.add_master(name, t);
                }
            }
            "master.delete" => {
                if let Some((id, n, _)) = self.master() {
                    self.delete_template(id);
                    self.masters.current = None;
                    self.masters.sel = None;
                    self.status = format!(
                        "{n} deleted; its sheets have the built-in block. Ctrl+Z brings it back."
                    );
                }
            }
            "master.sheet" => {
                if let Some((id, n, _)) = self.master() {
                    self.new_sheet_from(id);
                    self.status = format!("A new sheet on {n}. It is open in the sheet views.");
                }
            }
            "master.import" => {
                let Some(p) = rfd::FileDialog::new()
                    .set_title("Bring in a frame or title block")
                    .add_filter("Drawings", &["svg", "dxf"])
                    .pick_file()
                else {
                    return true;
                };
                let path = p.to_string_lossy().to_string();
                match creabim_exchange::titleblock::read(&path) {
                    Ok(t) => {
                        let stem = p
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("Master")
                            .to_string();
                        let name = self.master_name(&stem);
                        self.add_master(name, t);
                    }
                    Err(e) => self.status = format!("Couldn't bring it in: {e}"),
                }
            }
            "master.export" => {
                let Some((_, n, t)) = self.master() else {
                    return true;
                };
                let Some(p) = rfd::FileDialog::new()
                    .set_title("Write the master out as SVG")
                    .add_filter("SVG drawing", &["svg"])
                    .set_file_name(format!("{n}.svg"))
                    .save_file()
                else {
                    return true;
                };
                self.status = match std::fs::write(&p, creabim_exchange::titleblock::to_svg(&t)) {
                    Ok(()) => format!(
                        "{n} written to {}. Change it anywhere, then bring it back in.",
                        p.display()
                    ),
                    Err(e) => format!("Couldn't write {}: {e}", p.display()),
                };
            }
            "master.tool" => {
                self.masters.tool = match arg {
                    "line" => MTool::Line,
                    "rect" => MTool::Rect,
                    "text" => MTool::Text,
                    "field" => MTool::Field,
                    _ => MTool::Select,
                };
                self.masters.drawing.clear();
                self.status = match self.masters.tool {
                    MTool::Select => "Select: pick and drag what is drawn.",
                    MTool::Line => "Line: click corners; Enter or a right-click ends.",
                    MTool::Rect => "Rectangle: click two corners.",
                    MTool::Text => "Text: click where it goes.",
                    MTool::Field => "Field: click where it goes.",
                }
                .into();
            }
            "master.field" => {
                self.masters.field = arg.to_string();
                self.masters.tool = MTool::Field;
                self.status = format!("Click where {{{arg}}} goes: each sheet fills it in.");
            }
            _ => return false,
        }
        true
    }

    /// `base`, or `base 2`, `base 3`…, whichever no element has (a name
    /// ending in a number, "Standard A3", keeps it).
    fn master_name(&self, base: &str) -> String {
        let model = self.doc.model();
        let taken = |n: &str| model.iter().any(|e| e.name == n);
        if !taken(base) {
            return base.to_string();
        }
        (2..)
            .map(|k| format!("{base} {k}"))
            .find(|n| !taken(n))
            .expect("unbounded")
    }

    fn add_master(&mut self, name: String, t: SheetTemplate) {
        let e = Element::new(name.clone(), ElementKind::SheetTemplate(t));
        let id = e.id;
        let r = self.doc.commit(Transaction::new("New master").insert(e));
        self.report(
            r,
            format!("{name} made. Draw on it here; a sheet uses it from its Properties or the + sheet button."),
        );
        self.open_master(Some(id));
    }

    /// The Masters view: the master's sheet, its drawing and the tools.
    pub(super) fn masters_area(&mut self, ui: &mut egui::Ui, body: Rect) {
        if ui.rect_contains_pointer(body) {
            self.masters.hovered_pass = ui.ctx().cumulative_pass_nr();
        }
        let pal = self.shell.theme.palette();
        let painter = ui.painter_at(body);
        painter.rect_filled(body, 0.0, pal.panel2);
        let response = ui.interact(
            body,
            ui.id().with("masters.view"),
            egui::Sense::click_and_drag(),
        );
        let Some((_id, _name, t)) = self.master() else {
            painter.text(
                body.center(),
                egui::Align2::CENTER_CENTER,
                "No masters yet. + in the header makes one from Standard, Minimal or blank,\nor brings in a frame drawn elsewhere.",
                egui::FontId::proportional(14.0),
                pal.muted,
            );
            return;
        };
        let paper = t.paper.unwrap_or(DVec2::new(420.0, 297.0));
        let o = t.corner.at(paper.x, paper.y);
        // a new size (maximised, a divider dragged) fits the sheet again
        if (self.masters.seen - body.size()).length() > 1.0 {
            self.masters.seen = body.size();
            self.masters.camera = None;
        }
        // the camera: the sheet fitted, then wheel and right-drag
        let fit = || {
            let k = ((body.width() - 120.0).max(80.0) as f64 / paper.x)
                .min((body.height() - 60.0).max(60.0) as f64 / paper.y) as f32;
            (paper / 2.0 + DVec2::new(14.0 / k as f64, 0.0), k.max(0.2))
        };
        let (mut center, mut scale) = self.masters.camera.unwrap_or_else(fit);
        if response.hovered() {
            let (pan, scroll) = crate::planview::wheel(ui);
            if pan != egui::Vec2::ZERO {
                center -= DVec2::new(pan.x as f64, -pan.y as f64) / scale as f64;
            }
            if scroll != 0.0 {
                if let Some(h) = response.hover_pos() {
                    let k = (1.0 + scroll / 300.0).clamp(0.5, 2.0);
                    let off = DVec2::new(
                        (h.x - body.center().x) as f64,
                        -(h.y - body.center().y) as f64,
                    );
                    let w = center + off / scale as f64;
                    scale = (scale * k).clamp(0.2, 200.0);
                    center = w - off / scale as f64;
                }
            }
        }
        if response.dragged_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Middle)
        {
            let d = response.drag_delta();
            center -= DVec2::new(d.x as f64, -d.y as f64) / scale as f64;
        }
        let picked_fit = self.masters.sel.and_then(|i| t.items.get(i)).map(|it| {
            let pts: Vec<DVec2> = item_points(it).iter().map(|p| o + *p).collect();
            let lo = pts.iter().fold(DVec2::splat(f64::MAX), |a, p| a.min(*p));
            let hi = pts.iter().fold(DVec2::splat(f64::MIN), |a, p| a.max(*p));
            let span = (hi - lo).max(DVec2::splat(10.0));
            let k = ((body.width() - 160.0).max(60.0) as f64 / span.x)
                .min((body.height() - 120.0).max(60.0) as f64 / span.y) as f32;
            ((lo + hi) / 2.0, k.clamp(0.2, 200.0))
        });
        let (nav, _) =
            crate::planview::nav_input(ui, body, ElementId::NIL, fit, picked_fit, center, scale);
        if let Some((c, k)) = nav {
            center = c;
            scale = k;
        }
        self.masters.camera = Some((center, scale));
        let to_s = |p: DVec2| {
            body.center()
                + egui::vec2(
                    ((p.x - center.x) * scale as f64) as f32,
                    (-(p.y - center.y) * scale as f64) as f32,
                )
        };
        let to_w = |q: egui::Pos2| {
            center
                + DVec2::new(
                    (q.x - body.center().x) as f64,
                    -(q.y - body.center().y) as f64,
                ) / scale as f64
        };
        // the sheet, white, with a soft shadow
        let sheet = Rect::from_two_pos(to_s(DVec2::ZERO), to_s(paper));
        painter.rect_filled(
            sheet.translate(egui::vec2(0.0, 3.0)),
            2.0,
            egui::Color32::from_black_alpha(40),
        );
        painter.rect_filled(sheet, 0.0, egui::Color32::WHITE);
        // its millimetre grid, the finest step at least 8 px apart
        let step = [1.0, 5.0, 10.0, 50.0]
            .into_iter()
            .find(|g| g * scale as f64 >= 8.0)
            .unwrap_or(100.0);
        let faint = egui::Stroke::new(1.0, egui::Color32::from_gray(238));
        let strong = egui::Stroke::new(1.0, egui::Color32::from_gray(222));
        let mut x = step;
        while x < paper.x {
            let s = if ((x / step).round() as i64) % 5 == 0 {
                strong
            } else {
                faint
            };
            painter.line_segment([to_s(DVec2::new(x, 0.0)), to_s(DVec2::new(x, paper.y))], s);
            x += step;
        }
        let mut y = step;
        while y < paper.y {
            let s = if ((y / step).round() as i64) % 5 == 0 {
                strong
            } else {
                faint
            };
            painter.line_segment([to_s(DVec2::new(0.0, y)), to_s(DVec2::new(paper.x, y))], s);
            y += step;
        }
        let ink = egui::Color32::from_gray(25);
        let pen = |w: f64| egui::Stroke::new(((w * scale as f64) as f32).max(1.0), ink);
        // the margins, dashed, and the border if it has one
        let (blo, bhi) = (DVec2::new(20.0, 10.0), paper - DVec2::splat(10.0));
        let margin = [
            blo,
            DVec2::new(bhi.x, blo.y),
            bhi,
            DVec2::new(blo.x, bhi.y),
            blo,
        ];
        if t.border {
            painter.add(egui::Shape::line(
                margin.iter().map(|p| to_s(*p)).collect(),
                pen(0.5),
            ));
        } else {
            let m: Vec<egui::Pos2> = margin.iter().map(|p| to_s(*p)).collect();
            painter.extend(egui::Shape::dashed_line(
                &m,
                egui::Stroke::new(1.0, egui::Color32::from_gray(190)),
                5.0,
                4.0,
            ));
        }
        // a picked item being dragged shows where it goes
        let mut items = t.items.clone();
        if let (Some(i), Some((_, d))) = (self.masters.sel, self.masters.drag) {
            if let Some(it) = items.get_mut(i) {
                shift(it, d);
            }
        }
        let accent = crate::theme::interaction();
        for (i, it) in items.iter().enumerate() {
            match it {
                TitleItem::Line { points, width } => {
                    painter.add(egui::Shape::line(
                        points.iter().map(|p| to_s(o + *p)).collect(),
                        pen(*width),
                    ));
                }
                TitleItem::Fill { points, gray } => {
                    let g = (gray.clamp(0.0, 1.0) * 255.0) as u8;
                    painter.add(egui::Shape::convex_polygon(
                        points.iter().map(|p| to_s(o + *p)).collect(),
                        egui::Color32::from_gray(g),
                        egui::Stroke::NONE,
                    ));
                }
                TitleItem::Text {
                    at,
                    size,
                    text,
                    align,
                    angle,
                } => {
                    // as sheets draw it: the size is the font's, in mm
                    let px = (*size as f32 * scale).max(4.0);
                    let field = text.contains('{');
                    let colour = if field {
                        egui::Color32::from_rgb(30, 90, 170)
                    } else {
                        ink
                    };
                    let g = painter.layout_no_wrap(
                        text.clone(),
                        egui::FontId::proportional(px),
                        colour,
                    );
                    let sz = g.size();
                    let dx = match align {
                        TextAlign::Start => 0.0,
                        TextAlign::Middle => sz.x / 2.0,
                        TextAlign::End => sz.x,
                    };
                    let a = -(*angle as f32);
                    let rot = |v: egui::Vec2| {
                        let (s, c) = a.sin_cos();
                        egui::vec2(v.x * c - v.y * s, v.x * s + v.y * c)
                    };
                    let p = to_s(o + *at);
                    let top_left = p + rot(egui::vec2(-dx, -sz.y / 2.0));
                    // a field is a chip: what each sheet fills in
                    if field && a == 0.0 {
                        painter.rect_filled(
                            Rect::from_min_size(top_left, sz).expand(1.5),
                            3.0,
                            egui::Color32::from_rgba_unmultiplied(40, 110, 220, 26),
                        );
                    }
                    painter.add(egui::epaint::TextShape::new(top_left, g, colour).with_angle(a));
                }
            }
            if Some(i) == self.masters.sel {
                let pts: Vec<DVec2> = match it {
                    TitleItem::Text {
                        at,
                        size,
                        text,
                        align,
                        ..
                    } => {
                        let (lo, hi) = text_box(*at, *size, text, *align);
                        vec![lo, hi]
                    }
                    _ => item_points(it),
                };
                let lo = pts.iter().fold(DVec2::splat(f64::MAX), |a, p| a.min(*p));
                let hi = pts.iter().fold(DVec2::splat(f64::MIN), |a, p| a.max(*p));
                let r = Rect::from_two_pos(to_s(o + lo), to_s(o + hi)).expand(3.0);
                painter.rect_stroke(
                    r,
                    2.0,
                    egui::Stroke::new(1.5, accent),
                    egui::StrokeKind::Middle,
                );
                for c in [
                    r.left_top(),
                    r.right_top(),
                    r.left_bottom(),
                    r.right_bottom(),
                ] {
                    painter.rect_filled(
                        Rect::from_center_size(c, egui::vec2(5.0, 5.0)),
                        0.0,
                        accent,
                    );
                }
            }
        }
        // snapping: what is drawn, the paper's edges and margins, then the
        // millimetre grid; Shift lets go of all but the grid
        let shift_held = ui.input(|i| i.modifiers.shift);
        let tol = 8.0 / scale as f64;
        let mut targets: Vec<DVec2> = vec![
            DVec2::ZERO,
            DVec2::new(paper.x, 0.0),
            paper,
            DVec2::new(0.0, paper.y),
        ];
        targets.extend(margin.iter().take(4));
        for (i, it) in t.items.iter().enumerate() {
            if Some(i) == self.masters.sel && self.masters.drag.is_some() {
                continue;
            }
            let pts = item_points(it);
            targets.extend(pts.iter().map(|p| o + *p));
            if let TitleItem::Line { points, .. } = it {
                targets.extend(points.windows(2).map(|w| o + (w[0] + w[1]) / 2.0));
            }
        }
        targets.extend(self.masters.drawing.iter().copied());
        let xs: Vec<f64> = [0.0, 20.0, paper.x - 10.0, paper.x]
            .into_iter()
            .chain(targets.iter().map(|p| p.x))
            .collect();
        let ys: Vec<f64> = [0.0, 10.0, paper.y - 10.0, paper.y]
            .into_iter()
            .chain(targets.iter().map(|p| p.y))
            .collect();
        let grid_step = if scale >= 12.0 { 0.5 } else { 1.0 };
        let snap = |w: DVec2| -> (DVec2, bool) {
            if !shift_held {
                if let Some(p) = targets
                    .iter()
                    .filter(|p| p.distance(w) < tol)
                    .min_by(|a, b| a.distance(w).total_cmp(&b.distance(w)))
                {
                    return (*p, true);
                }
            }
            let near = |v: f64, all: &[f64]| {
                all.iter()
                    .filter(|a| (*a - v).abs() < tol * 0.75)
                    .min_by(|a, b| (*a - v).abs().total_cmp(&(*b - v).abs()))
                    .copied()
            };
            let gx = (w.x / grid_step).round() * grid_step;
            let gy = (w.y / grid_step).round() * grid_step;
            if shift_held {
                return (DVec2::new(gx, gy), false);
            }
            let x = near(w.x, &xs);
            let y = near(w.y, &ys);
            (
                DVec2::new(x.unwrap_or(gx), y.unwrap_or(gy)),
                x.is_some() || y.is_some(),
            )
        };
        let here = response.hover_pos().map(|h| snap(to_w(h)));
        if let Some((w, on)) = here {
            if on {
                painter.circle_stroke(to_s(w), 5.0, egui::Stroke::new(1.5, accent));
            }
            if self.masters.tool != MTool::Select || self.masters.drag.is_some() {
                // where the pointer is, mm from the master's corner
                let rel = w - o;
                painter.text(
                    to_s(w) + egui::vec2(12.0, 14.0),
                    egui::Align2::LEFT_TOP,
                    format!("{:.1}, {:.1} mm", rel.x, rel.y),
                    egui::FontId::proportional(11.0),
                    pal.muted,
                );
            }
        }
        let typing = ui.ctx().egui_wants_keyboard_input();
        let hovered = response.hovered();
        match self.masters.tool {
            MTool::Select => {
                let picked_at = |p: DVec2| {
                    t.items
                        .iter()
                        .enumerate()
                        .map(|(i, it)| (i, item_dist(it, p - o)))
                        .filter(|(_, d)| *d < 6.0 / scale as f64)
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(i, _)| i)
                };
                if response.drag_started_by(egui::PointerButton::Primary) {
                    if let Some(p) = response.interact_pointer_pos() {
                        let w = to_w(p);
                        if let Some(i) = picked_at(w) {
                            self.masters.sel = Some(i);
                            self.masters.drag = Some((w, DVec2::ZERO));
                        }
                    }
                }
                if let (Some((from, _)), Some((w, _))) = (self.masters.drag, here) {
                    // the grabbed point snaps; the item follows it
                    if let Some(i) = self.masters.sel {
                        let first = t
                            .items
                            .get(i)
                            .map(item_points)
                            .and_then(|v| v.first().copied());
                        let d = match first {
                            Some(f) => {
                                snap(
                                    o + f + (to_w(response.hover_pos().unwrap_or_default()) - from),
                                )
                                .0 - (o + f)
                            }
                            None => w - from,
                        };
                        self.masters.drag = Some((from, d));
                    }
                }
                if response.drag_stopped_by(egui::PointerButton::Primary) {
                    if let (Some(i), Some((_, d))) = (self.masters.sel, self.masters.drag.take()) {
                        if d.length() > 1e-9 {
                            self.edit_master("Move", |t| {
                                if let Some(it) = t.items.get_mut(i) {
                                    shift(it, d);
                                }
                            });
                        }
                    }
                }
                if response.clicked() {
                    self.masters.sel = response
                        .interact_pointer_pos()
                        .and_then(|p| picked_at(to_w(p)));
                }
                // the keys over the view: nudge, delete, let go
                if hovered && !typing {
                    if let Some(i) = self.masters.sel {
                        let step = if shift_held { 5.0 } else { 1.0 };
                        let (l, r, u, dn, del, esc) = ui.input(|k| {
                            (
                                k.key_pressed(egui::Key::ArrowLeft),
                                k.key_pressed(egui::Key::ArrowRight),
                                k.key_pressed(egui::Key::ArrowUp),
                                k.key_pressed(egui::Key::ArrowDown),
                                k.key_pressed(egui::Key::Delete)
                                    || k.key_pressed(egui::Key::Backspace),
                                k.key_pressed(egui::Key::Escape),
                            )
                        });
                        let d =
                            DVec2::new((r as i32 - l as i32) as f64, (u as i32 - dn as i32) as f64)
                                * step;
                        if d != DVec2::ZERO {
                            self.edit_master("Nudge", |t| {
                                if let Some(it) = t.items.get_mut(i) {
                                    shift(it, d);
                                }
                            });
                        }
                        if del {
                            self.edit_master("Delete from master", |t| {
                                if i < t.items.len() {
                                    t.items.remove(i);
                                }
                            });
                            self.masters.sel = None;
                            self.status = "Deleted. Ctrl+Z brings it back.".into();
                        }
                        if esc {
                            self.masters.sel = None;
                        }
                    }
                }
            }
            MTool::Line | MTool::Rect => {
                let rect_tool = self.masters.tool == MTool::Rect;
                if let (Some(last), Some((w, _))) = (self.masters.drawing.last(), here) {
                    if rect_tool {
                        let a = *last;
                        let pts = [a, DVec2::new(w.x, a.y), w, DVec2::new(a.x, w.y), a];
                        painter.add(egui::Shape::line(
                            pts.iter().map(|p| to_s(*p)).collect(),
                            egui::Stroke::new(1.5, accent),
                        ));
                    } else {
                        let mut pts: Vec<egui::Pos2> =
                            self.masters.drawing.iter().map(|p| to_s(*p)).collect();
                        pts.push(to_s(w));
                        painter.add(egui::Shape::line(pts, egui::Stroke::new(1.5, accent)));
                    }
                }
                let mut finish = false;
                if response.clicked() {
                    if let Some((w, _)) = here {
                        let d = &mut self.masters.drawing;
                        if rect_tool && d.len() == 1 {
                            let a = d[0];
                            d.clear();
                            if (w - a).abs().min_element() > 0.1 {
                                let (lo, hi) = (a.min(w) - o, a.max(w) - o);
                                self.edit_master("Rectangle", |t| {
                                    t.items.push(TitleItem::Line {
                                        points: vec![
                                            lo,
                                            DVec2::new(hi.x, lo.y),
                                            hi,
                                            DVec2::new(lo.x, hi.y),
                                            lo,
                                        ],
                                        width: 0.35,
                                    })
                                });
                            }
                        } else if !rect_tool && d.len() >= 2 && d[0].distance(w) < 1e-6 {
                            d.push(w);
                            finish = true;
                        } else if !rect_tool && d.last().is_some_and(|q| q.distance(w) < 1e-6) {
                            finish = true;
                        } else {
                            d.push(w);
                        }
                    }
                }
                let (enter, esc) = if hovered && !typing {
                    ui.input(|k| {
                        (
                            k.key_pressed(egui::Key::Enter),
                            k.key_pressed(egui::Key::Escape),
                        )
                    })
                } else {
                    (false, false)
                };
                if enter || response.secondary_clicked() {
                    finish = true;
                }
                if esc {
                    self.masters.drawing.clear();
                }
                if finish {
                    let pts: Vec<DVec2> = std::mem::take(&mut self.masters.drawing)
                        .into_iter()
                        .map(|p| p - o)
                        .collect();
                    if pts.len() >= 2 {
                        self.edit_master("Line", |t| {
                            t.items.push(TitleItem::Line {
                                points: pts,
                                width: 0.25,
                            })
                        });
                        let n = t.items.len();
                        self.masters.sel = Some(n);
                    }
                }
            }
            MTool::Text | MTool::Field => {
                if response.clicked() {
                    if let Some((w, _)) = here {
                        let words = if self.masters.tool == MTool::Field {
                            let f = if self.masters.field.is_empty() {
                                "project".to_string()
                            } else {
                                self.masters.field.clone()
                            };
                            format!("{{{f}}}")
                        } else {
                            "Text".to_string()
                        };
                        let n = t.items.len();
                        self.edit_master("Text", |t| {
                            t.items.push(TitleItem::Text {
                                at: w - o,
                                size: 3.0,
                                text: words,
                                align: TextAlign::Start,
                                angle: 0.0,
                            })
                        });
                        self.masters.sel = Some(n);
                        self.prop_tab = PropTab::Master;
                        self.status =
                            "Placed. Its words, size and alignment are in Properties.".into();
                    }
                }
            }
        }
        // what the pointer does, at the foot of the view
        let hint = match self.masters.tool {
            MTool::Select => {
                "Click to pick, drag to move; arrows nudge 1 mm, Shift 5 mm; Delete removes"
            }
            MTool::Line => {
                "Click corners; Enter, a right-click or the first corner again ends; Esc drops it"
            }
            MTool::Rect => "Click two corners",
            MTool::Text => "Click where the text goes",
            MTool::Field => "Click where the field goes: each sheet fills it in",
        };
        painter.text(
            body.left_bottom() + egui::vec2(12.0, -10.0),
            egui::Align2::LEFT_BOTTOM,
            format!("{hint}. Snaps to what is drawn, the margins and a {grid_step} mm grid; Shift: the grid only."),
            egui::FontId::proportional(11.5),
            pal.muted,
        );
    }

    /// The Master tab of Properties while a Masters view is in front.
    pub(super) fn master_props(&mut self, ui: &mut egui::Ui) {
        let Some((id, name, t)) = self.master() else {
            crate::props::section(ui, "Master");
            crate::props::note(
                ui,
                "No masters yet: + in the Masters view's header makes one.",
            );
            return;
        };
        let users = super::drafting::template_users(self.doc.model(), id);
        if self.prop_header(ui, "master.about", "Master", true, None) {
            let mut n = name.clone();
            let r = crate::props::row(ui, "Name", |ui| {
                crate::props::fill(ui, egui::TextEdit::singleline(&mut n))
            });
            if r.lost_focus() && !n.trim().is_empty() && n.trim() != name {
                let new = n.trim().to_string();
                let res = self.doc.edit(id, "Rename master", |e| e.name = new.clone());
                self.report(res, format!("Renamed {new}."));
            }
            let size = t.paper.unwrap_or(DVec2::new(420.0, 297.0));
            let now = PAPERS
                .iter()
                .find(|(_, w, h)| {
                    (size - DVec2::new(*w, *h)).length() < 0.5
                        || (size - DVec2::new(*h, *w)).length() < 0.5
                })
                .map_or("Custom", |p| p.0);
            let portrait = size.y > size.x;
            let mut pick: Option<DVec2> = None;
            crate::props::row(ui, "Paper", |ui| {
                egui::ComboBox::from_id_salt(("master.paper", id))
                    .icon(crate::props::combo_icon)
                    .width(ui.available_width())
                    .selected_text(format!("{now}{}", if portrait { ", portrait" } else { "" }))
                    .show_ui(ui, |ui| {
                        for (n, w, h) in PAPERS {
                            if ui.selectable_label(n == now && !portrait, n).clicked() {
                                pick = Some(DVec2::new(w, h));
                            }
                            if ui
                                .selectable_label(n == now && portrait, format!("{n}, portrait"))
                                .clicked()
                            {
                                pick = Some(DVec2::new(h, w));
                            }
                        }
                    });
            });
            if let Some(p) = pick {
                self.edit_master("Master paper", |t| t.paper = Some(p));
                self.masters.camera = None;
            }
            let mut corner = t.corner;
            crate::props::row(ui, "Measured from", |ui| {
                egui::ComboBox::from_id_salt(("master.corner", id))
                    .icon(crate::props::combo_icon)
                    .width(ui.available_width())
                    .selected_text(corner.label())
                    .show_ui(ui, |ui| {
                        for c in Corner::ALL {
                            ui.selectable_value(&mut corner, c, c.label());
                        }
                    })
            })
            .response
            .on_hover_text(
                "The sheet corner its drawing is measured from: it stays there on any paper",
            );
            if corner != t.corner {
                // the drawing stays where it is on the sheet
                let d = t.corner.at(size.x, size.y) - corner.at(size.x, size.y);
                self.edit_master("Master corner", |t| {
                    t.corner = corner;
                    t.items.iter_mut().for_each(|it| shift(it, d));
                });
            }
            let mut border = t.border;
            if crate::props::check(ui, "Border", &mut border, "Drawn").changed() {
                self.edit_master("Master border", |t| t.border = border);
            }
            crate::props::row(ui, "Sheets", |ui| {
                ui.label(match users.len() {
                    0 => "None on it yet".to_string(),
                    1 => "1 sheet".to_string(),
                    n => format!("{n} sheets"),
                })
            });
            let all_sheets: Vec<ElementId> = creabim_geom::sheet::sheets(self.doc.model())
                .into_iter()
                .map(|(i, _)| i)
                .collect();
            let new_sheet = crate::props::action(
                ui,
                "",
                "New sheet on it",
                "A new sheet using this master",
                true,
            )
            .clicked();
            let every = crate::props::action(
                ui,
                "",
                "Use on every sheet",
                "Every sheet of the project takes this master (one undo step)",
                !all_sheets.is_empty() && users.len() < all_sheets.len(),
            )
            .clicked();
            if new_sheet {
                self.new_sheet_from(id);
            }
            if every {
                self.set_sheet_template(&all_sheets, Some(id));
            }
        }
        // its own fields: what each sheet can fill in beyond the built-in
        // ones, and what shows until it does
        if self.prop_header(ui, "master.fields", "Its own fields", true, None) {
            let mut fields = t.fields.clone();
            let mut changed = false;
            let mut remove = None;
            for (k, (f, v)) in fields.iter_mut().enumerate() {
                crate::props::row(ui, &format!("{{{f}}}"), |ui| {
                    ui.horizontal(|ui| {
                        let w = ui.available_width() - 26.0;
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(v)
                                    .desired_width(w)
                                    .hint_text("Until a sheet fills it"),
                            )
                            .lost_focus();
                        if crate::props::icon_button(ui, "trash-2", "Remove this field").clicked() {
                            remove = Some(k);
                        }
                    });
                });
            }
            if let Some(k) = remove {
                fields.remove(k);
                changed = true;
            }
            let key = egui::Id::new(("master.newfield", id));
            let mut new = ui
                .ctx()
                .data(|d| d.get_temp::<String>(key))
                .unwrap_or_default();
            let add = crate::props::row(ui, "New field", |ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut new)
                        .desired_width(ui.available_width())
                        .hint_text("client, drawn by, revision…"),
                );
                r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
            });
            let clean: String = new
                .trim()
                .to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '_' })
                .collect();
            if add && !clean.is_empty() && !fields.iter().any(|(f, _)| *f == clean) {
                fields.push((clean.clone(), String::new()));
                changed = true;
                new.clear();
                self.masters.field = clean.clone();
                self.status = format!("{{{clean}}} added: the Field tool places it now.");
            }
            ui.ctx().data_mut(|d| d.insert_temp(key, new));
            if changed {
                self.edit_master("Master fields", |t| t.fields = fields);
            }
            crate::props::note(
                ui,
                "Every sheet fills in {sheet}, {number}, {project}, {scale} and {date} itself; a sheet's own values for these are in its Properties.",
            );
        }
        // what is picked
        let Some(i) = self.masters.sel.filter(|i| *i < t.items.len()) else {
            return;
        };
        let it = t.items[i].clone();
        let title = match &it {
            TitleItem::Text { text, .. } if text.contains('{') => "Picked field",
            TitleItem::Text { .. } => "Picked text",
            TitleItem::Line { .. } => "Picked line",
            TitleItem::Fill { .. } => "Picked fill",
        };
        if !self.prop_header(ui, "master.picked", title, true, None) {
            return;
        }
        let mut edited = it.clone();
        let mut label = None;
        match &mut edited {
            TitleItem::Text {
                at,
                size,
                text,
                align,
                angle,
            } => {
                let r = crate::props::row(ui, "Words", |ui| {
                    crate::props::fill(ui, egui::TextEdit::singleline(text))
                });
                if r.lost_focus() {
                    label = Some("Master text");
                }
                if crate::props::row(ui, "Size", |ui| {
                    crate::props::fill(
                        ui,
                        egui::DragValue::new(size)
                            .range(0.5..=60.0)
                            .speed(0.1)
                            .suffix(" mm"),
                    )
                })
                .changed()
                {
                    label = Some("Text size");
                }
                if crate::props::choice(
                    ui,
                    "Align",
                    align,
                    &[
                        (TextAlign::Start, "Left", "Text runs right from its point"),
                        (TextAlign::Middle, "Centre", "Text centred on its point"),
                        (TextAlign::End, "Right", "Text ends at its point"),
                    ],
                ) {
                    label = Some("Text alignment");
                }
                let mut deg = angle.to_degrees();
                if crate::props::row(ui, "Turned", |ui| {
                    crate::props::fill(ui, egui::DragValue::new(&mut deg).speed(1.0).suffix("°"))
                })
                .changed()
                {
                    *angle = deg.to_radians();
                    label = Some("Text angle");
                }
                let (mut x, mut y) = (at.x, at.y);
                let cx = crate::props::row(ui, "X", |ui| {
                    crate::props::fill(ui, egui::DragValue::new(&mut x).speed(0.5).suffix(" mm"))
                });
                let cy = crate::props::row(ui, "Y", |ui| {
                    crate::props::fill(ui, egui::DragValue::new(&mut y).speed(0.5).suffix(" mm"))
                });
                if cx.changed() || cy.changed() {
                    *at = DVec2::new(x, y);
                    label = Some("Move text");
                }
            }
            TitleItem::Line { points, width } => {
                if crate::props::row(ui, "Pen", |ui| {
                    crate::props::fill(
                        ui,
                        egui::DragValue::new(width)
                            .range(0.05..=3.0)
                            .speed(0.01)
                            .suffix(" mm"),
                    )
                })
                .changed()
                {
                    label = Some("Line pen");
                }
                let len: f64 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
                crate::props::row(ui, "Length", |ui| ui.label(format!("{len:.1} mm")));
            }
            TitleItem::Fill { gray, .. } => {
                let mut g = *gray * 100.0;
                if crate::props::row(ui, "Grey", |ui| {
                    crate::props::fill(
                        ui,
                        egui::DragValue::new(&mut g).range(0.0..=100.0).suffix(" %"),
                    )
                })
                .changed()
                {
                    *gray = g / 100.0;
                    label = Some("Fill grey");
                }
            }
        }
        if crate::props::action(ui, "", "Delete it", "Take it off the master (Delete)", true)
            .clicked()
        {
            self.edit_master("Delete from master", |t| {
                if i < t.items.len() {
                    t.items.remove(i);
                }
            });
            self.masters.sel = None;
            return;
        }
        if let Some(l) = label {
            if edited != it {
                self.edit_master(l, |t| {
                    if let Some(x) = t.items.get_mut(i) {
                        *x = edited;
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_and_lines_are_picked_near_them() {
        let line = TitleItem::Line {
            points: vec![DVec2::ZERO, DVec2::new(10.0, 0.0)],
            width: 0.25,
        };
        assert!(item_dist(&line, DVec2::new(5.0, 0.5)) < 0.6);
        assert!(item_dist(&line, DVec2::new(5.0, 5.0)) > 4.0);
        let text = TitleItem::Text {
            at: DVec2::new(0.0, 0.0),
            size: 3.0,
            text: "{project}".into(),
            align: TextAlign::End,
            angle: 0.0,
        };
        // right-aligned: it runs leftward from its point
        assert_eq!(item_dist(&text, DVec2::new(-5.0, 0.0)), 0.0);
        assert!(item_dist(&text, DVec2::new(5.0, 0.0)) > 4.0);
        let mut t = text.clone();
        shift(&mut t, DVec2::new(1.0, 2.0));
        assert_eq!(item_points(&t), vec![DVec2::new(1.0, 2.0)]);
    }
}
