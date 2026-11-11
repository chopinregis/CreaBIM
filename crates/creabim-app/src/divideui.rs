//! Divide (MU34 B2, reworked MU38): a slim bar over the views, not a
//! window over the drawing. Along its length, up its height or both (a
//! grid); by a count, or by a size with the rest put at the ends, the
//! start, the end or spread; making cuts, guides or copies. Every view
//! shows the divisions and each part's size while the bar is open; Enter
//! makes them in one undo step. Divided parts remember how, so Divide on
//! one of them opens as it was left.

use super::*;
use creabim_core::divide::{self, By, Make, Rest, Span};

/// Which way a division runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Way {
    Along,
    Up,
    Grid,
}

/// The settings of a division, kept between frames and per element divided.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub way: Way,
    pub by_size: bool,
    pub parts: usize,
    pub parts_up: usize,
    pub size: f64,
    pub size_up: f64,
    pub rest: Rest,
    pub make: Make,
    pub gap: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            way: Way::Along,
            by_size: false,
            parts: 4,
            parts_up: 3,
            size: 1.2,
            size_up: 1.0,
            rest: Rest::Ends,
            make: Make::Cuts,
            gap: 0.0,
        }
    }
}

/// What the Divide bar holds between frames.
#[derive(Clone, Debug, Default)]
pub struct DivideUi {
    pub open: bool,
    /// The element being divided, fixed when the bar opens.
    pub host: Option<ElementId>,
    pub s: Settings,
    /// How each divided element was divided, for this session.
    pub memory: HashMap<ElementId, Settings>,
    /// Where the bar was dragged to, from its place at the top of the
    /// view (MU39: it felt stuck).
    pub offset: egui::Vec2,
}

impl Settings {
    fn by(&self, up: bool) -> By {
        match (self.by_size, up) {
            (false, false) => By::Count(self.parts),
            (false, true) => By::Count(self.parts_up),
            (true, false) => By::Size(self.size, self.rest),
            (true, true) => By::Size(self.size_up, self.rest),
        }
    }

    fn gap(&self) -> f64 {
        if self.make == Make::Cuts {
            self.gap
        } else {
            0.0
        }
    }

    /// The parts along and up `s`, for the ways divided.
    #[allow(clippy::type_complexity)]
    fn split(&self, s: &Span) -> (Option<Vec<(f64, f64)>>, Option<Vec<(f64, f64)>>) {
        let along = matches!(self.way, Way::Along | Way::Grid)
            .then(|| divide::split(s.len, self.by(false), self.gap()))
            .flatten();
        let up = matches!(self.way, Way::Up | Way::Grid)
            .then(|| divide::split(s.z1 - s.z0, self.by(true), self.gap()))
            .flatten();
        (along, up)
    }
}

/// A division mark: a line in model coordinates, and whether plans show it
/// (the marks along a length; the ones up its height only show standing).
pub type Mark = (DVec3, DVec3, bool);

impl CreaBimApp {
    /// Open Divide for the element selected first.
    pub(super) fn open_divide(&mut self) {
        let host = self
            .selection
            .all()
            .first()
            .copied()
            .filter(|h| divide::span(self.doc.model(), *h).is_some());
        let Some(h) = host else {
            self.status =
                "Divide: select a wall, a beam, a guide or a part drawn from a plan outline first."
                    .into();
            return;
        };
        if let Some(s) = self.divide.memory.get(&h) {
            self.divide.s = *s;
        }
        self.divide.host = Some(h);
        self.divide.open = true;
        self.status = "Divide: + and − (or scroll over the number) change the parts, Enter divides, Esc leaves it.".into();
    }

    /// The marks and part sizes every view shows while Divide is open.
    pub(super) fn divide_marks(&self) -> (Vec<Mark>, Vec<(DVec3, String)>) {
        let (mut marks, mut labels) = (vec![], vec![]);
        let model = self.doc.model();
        let Some(h) = self.divide.host.filter(|_| self.divide.open) else {
            return (marks, labels);
        };
        let Some(s) = divide::span(model, h) else {
            return (marks, labels);
        };
        let lift = s
            .level
            .and_then(|l| model.level(l))
            .map_or(0.0, |l| l.elevation);
        let (z0, z1) = (lift + s.z0, lift + s.z1.max(s.z0 + 0.3));
        // across it: a wall is centred on its line, a part runs to its left
        let (c0, c1) = if matches!(model.get(h).map(|e| &e.kind), Some(ElementKind::Wall(_))) {
            (-s.width / 2.0, s.width / 2.0)
        } else {
            (0.0, s.width)
        };
        let mid = s.o + s.v * ((c0 + c1) / 2.0);
        let (along, up) = self.divide.s.split(&s);
        let gap = self.divide.s.gap();
        let edges = |ps: &[(f64, f64)]| -> Vec<f64> {
            let mut e = vec![];
            for (k, (t, size)) in ps.iter().enumerate() {
                if k > 0 {
                    e.push(*t);
                }
                if gap > 0.0 && k + 1 < ps.len() {
                    e.push(t + size);
                }
            }
            e
        };
        // guides change nothing: they show as the guides they will be,
        // dashed, running past it, the element left as it is (MU39)
        if self.divide.s.make == Make::Guides {
            let reach = s.width.max(1.0);
            if let Some(ps) = &along {
                for part in &ps[1..] {
                    let p = s.o + s.u * part.0;
                    let (a, b) = (p - s.v * 0.5, p + s.v * (reach + 0.5));
                    marks.push((a.extend(z0), b.extend(z0), true));
                }
            }
            if let Some(ps) = &up {
                for part in &ps[1..] {
                    let z = lift + s.z0 + part.0;
                    let (a, b) = (s.o - s.u * 0.5, s.o + s.u * (s.len + 0.5));
                    marks.push((a.extend(z), b.extend(z), false));
                }
            }
            return (marks, labels);
        }
        if let Some(ps) = &along {
            for t in edges(ps) {
                let p = s.o + s.u * t;
                // a tick across it, for plans, and a line up its height
                marks.push((
                    (p + s.v * (c0 - 0.15)).extend(z0),
                    (p + s.v * (c1 + 0.15)).extend(z0),
                    true,
                ));
                let m = mid + s.u * t;
                marks.push((m.extend(z0), m.extend(z1), false));
            }
            for (t, size) in ps {
                let p = s.o + s.u * (t + size / 2.0) + s.v * (c1 + 0.45);
                labels.push((p.extend(z0), crate::units::len(*size)));
            }
        }
        if let Some(ps) = &up {
            for t in edges(ps) {
                let z = lift + s.z0 + t;
                marks.push((mid.extend(z), (mid + s.u * s.len).extend(z), false));
            }
            for (t, size) in ps {
                let p = s.o + s.v * (c1 + 0.45);
                labels.push((
                    p.extend(lift + s.z0 + t + size / 2.0),
                    crate::units::len(*size),
                ));
            }
        }
        (marks, labels)
    }

    /// The Divide bar over the views, while it is open.
    pub(super) fn divide_window(&mut self, ctx: &egui::Context) {
        if !self.divide.open {
            return;
        }
        let model = self.doc.model();
        let host = self
            .divide
            .host
            .filter(|h| divide::span(model, *h).is_some());
        let Some(h) = host else {
            self.divide.open = false;
            return;
        };
        let span = divide::span(model, h).expect("has a span");
        let name = model.get(h).map_or(String::new(), |e| e.name.clone());
        let all = self.selection.all();
        let item = all.iter().copied().rfind(|i| *i != h);
        let item_name = item.and_then(|i| model.get(i)).map(|e| e.name.clone());
        let cuttable = matches!(
            model.get(h).map(|e| &e.kind),
            Some(ElementKind::Part(_) | ElementKind::Wall(_))
        );
        let area = self.mat.area.unwrap_or_else(|| ctx.content_rect());
        let mut go = false;
        let mut close = false;
        let p = crate::theme::Palette::of(ctx);
        let typing = ctx.egui_wants_keyboard_input();
        // keys: + and − change the count (or size), Enter divides, Esc leaves
        let (plus, minus, enter, esc) = ctx.input(|i| {
            (
                !typing
                    && (i.key_pressed(egui::Key::Plus)
                        || i.key_pressed(egui::Key::Equals)
                        || i.key_pressed(egui::Key::CloseBracket)),
                !typing
                    && (i.key_pressed(egui::Key::Minus) || i.key_pressed(egui::Key::OpenBracket)),
                i.key_pressed(egui::Key::Enter),
                !typing && i.key_pressed(egui::Key::Escape),
            )
        });
        let self_offset = self.divide.offset;
        let mut moved_by = egui::Vec2::ZERO;
        let moved = &mut moved_by;
        let d = &mut self.divide.s;
        if !cuttable && d.make == Make::Cuts {
            d.make = Make::Guides;
        }
        fn step(d: &mut Settings, up: bool, k: i32) {
            match (d.by_size, up) {
                (false, false) => d.parts = (d.parts as i32 + k).clamp(2, 200) as usize,
                (false, true) => d.parts_up = (d.parts_up as i32 + k).clamp(2, 200) as usize,
                (true, false) => d.size = (d.size + 0.1 * k as f64).max(0.05),
                (true, true) => d.size_up = (d.size_up + 0.1 * k as f64).max(0.05),
            }
        }
        if plus {
            step(d, d.way == Way::Up, 1);
        }
        if minus {
            step(d, d.way == Way::Up, -1);
        }
        egui::Area::new(egui::Id::new("divide bar"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_TOP)
            .fixed_pos(egui::pos2(area.center().x, area.top() + 44.0) + self.divide.offset)
            .show(ctx, |ui| {
                ui.set_max_width((area.width() - 40.0).max(260.0));
                egui::Frame::new()
                    .fill(ui.visuals().window_fill)
                    .stroke(egui::Stroke::new(1.0, p.border))
                    .corner_radius(18.0)
                    .shadow(ui.visuals().popup_shadow)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            // a grip to carry the bar out of the way
                            let (gr, grip) =
                                ui.allocate_exact_size(egui::vec2(12.0, 20.0), egui::Sense::drag());
                            let dot = if grip.hovered() || grip.dragged() {
                                p.text
                            } else {
                                p.muted
                            };
                            for (x, y) in [
                                (-2.5, -5.0),
                                (2.5, -5.0),
                                (-2.5, 0.0),
                                (2.5, 0.0),
                                (-2.5, 5.0),
                                (2.5, 5.0),
                            ] {
                                ui.painter().circle_filled(
                                    gr.center() + egui::vec2(x, y),
                                    1.3,
                                    dot,
                                );
                            }
                            if grip.dragged() {
                                *moved += grip.drag_delta();
                            }
                            if grip.double_clicked() {
                                *moved = -self_offset;
                            }
                            grip.on_hover_cursor(egui::CursorIcon::Grab)
                                .on_hover_text("Drag to move the bar; double-click puts it back");
                            let (ir, _) = ui
                                .allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                            crate::svgicon::paint(ui.painter(), ir, "mod-array", p.accent);
                            ui.label(egui::RichText::new("Divide").strong());
                            ui.add(egui::Label::new(egui::RichText::new(&name).weak()).truncate())
                                .on_hover_text(
                                    "What is divided: the element selected when Divide opened",
                                );
                            ui.separator();
                            for (w, t, tip) in [
                                (Way::Along, "Along", "Along its length"),
                                (Way::Up, "Up", "Up its height: courses, bands, shelves"),
                                (Way::Grid, "Grid", "Both: panels"),
                            ] {
                                ui.selectable_value(&mut d.way, w, t).on_hover_text(tip);
                            }
                            ui.separator();
                            ui.selectable_value(&mut d.by_size, false, "Count")
                                .on_hover_text("A number of equal parts");
                            ui.selectable_value(&mut d.by_size, true, "Size")
                                .on_hover_text("Parts of a size; the rest goes where you say");
                            let number =
                                |ui: &mut egui::Ui, d: &mut Settings, up: bool, tag: &str| {
                                    if !tag.is_empty() {
                                        ui.label(egui::RichText::new(tag).weak());
                                    }
                                    if ui.small_button("−").on_hover_text("Fewer (−)").clicked()
                                    {
                                        step(d, up, -1);
                                    }
                                    let r = match (d.by_size, up) {
                                        (false, false) => ui.add(
                                            egui::DragValue::new(&mut d.parts)
                                                .range(2..=200)
                                                .speed(0.05),
                                        ),
                                        (false, true) => ui.add(
                                            egui::DragValue::new(&mut d.parts_up)
                                                .range(2..=200)
                                                .speed(0.05),
                                        ),
                                        (true, false) => ui.add(
                                            crate::units::drag(&mut d.size).range(0.05..=1000.0),
                                        ),
                                        (true, true) => ui.add(
                                            crate::units::drag(&mut d.size_up).range(0.05..=1000.0),
                                        ),
                                    };
                                    // the wheel over the number steps it
                                    if r.hovered() {
                                        let wheel: f32 = ui.input(|i| {
                                            i.raw
                                                .events
                                                .iter()
                                                .filter_map(|e| match e {
                                                    egui::Event::MouseWheel { delta, .. } => {
                                                        Some(delta.y)
                                                    }
                                                    _ => None,
                                                })
                                                .sum()
                                        });
                                        if wheel.abs() > 0.5 {
                                            step(d, up, wheel.signum() as i32);
                                        }
                                    }
                                    r.on_hover_text("Type a number, drag, or scroll over it");
                                    if ui.small_button("+").on_hover_text("More (+)").clicked() {
                                        step(d, up, 1);
                                    }
                                };
                            match d.way {
                                Way::Along => number(ui, d, false, ""),
                                Way::Up => number(ui, d, true, ""),
                                Way::Grid => {
                                    number(ui, d, false, "along");
                                    number(ui, d, true, "up");
                                }
                            }
                            if d.by_size {
                                egui::ComboBox::from_id_salt("divide.rest")
                                    .icon(crate::props::combo_icon)
                                    .selected_text(match d.rest {
                                        Rest::Ends => "Rest at both ends",
                                        Rest::Start => "Rest at the start",
                                        Rest::End => "Rest at the end",
                                        Rest::Spread => "Spread evenly",
                                    })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut d.rest,
                                            Rest::Ends,
                                            "Rest at both ends",
                                        )
                                        .on_hover_text(
                                            "Full parts centred, a short one at each end",
                                        );
                                        ui.selectable_value(
                                            &mut d.rest,
                                            Rest::Start,
                                            "Rest at the start",
                                        );
                                        ui.selectable_value(
                                            &mut d.rest,
                                            Rest::End,
                                            "Rest at the end",
                                        );
                                        ui.selectable_value(
                                            &mut d.rest,
                                            Rest::Spread,
                                            "Spread evenly",
                                        )
                                        .on_hover_text(
                                            "As many parts as are nearest the size, all equal",
                                        );
                                    });
                            }
                            ui.separator();
                            if cuttable {
                                ui.selectable_value(&mut d.make, Make::Cuts, "Cuts")
                                    .on_hover_text("Cut it into separate elements");
                            }
                            ui.selectable_value(&mut d.make, Make::Guides, "Guides")
                                .on_hover_text("Guides at the divisions, to snap to");
                            ui.selectable_value(&mut d.make, Make::Copies, "Copies")
                                .on_hover_text("What you Shift+click, one in each part");
                            if d.make == Make::Cuts {
                                ui.label(egui::RichText::new("gap").weak());
                                ui.add(crate::units::drag(&mut d.gap).range(0.0..=1.0))
                                    .on_hover_text("Left between the parts: a joint");
                            }
                            ui.separator();
                            let (along, up) = d.split(&span);
                            let count = along.as_ref().map_or(1, |a| a.len())
                                * up.as_ref().map_or(1, |u| u.len());
                            let ready = count >= 2 && (d.make != Make::Copies || item.is_some());
                            if ui
                                .add_enabled(
                                    ready,
                                    egui::Button::new(
                                        egui::RichText::new(format!("Divide · {count}")).strong(),
                                    ),
                                )
                                .on_hover_text(format!(
                                    "Divide into {count}, one undo step (Enter)"
                                ))
                                .on_disabled_hover_text(if count < 2 {
                                    "Two parts or more"
                                } else {
                                    "Shift+click what to copy into each part first"
                                })
                                .clicked()
                            {
                                go = true;
                            }
                            if crate::props::icon_button(ui, "x", "Leave Divide (Esc)").clicked() {
                                close = true;
                            }
                        });
                        if d.make == Make::Copies {
                            ui.label(
                                egui::RichText::new(match &item_name {
                                    Some(n) => format!("Copies {n} into each part."),
                                    None => "Shift+click what goes in each part.".into(),
                                })
                                .small()
                                .weak(),
                            );
                        }
                    });
            });
        let s = self.divide.s;
        let (along, up) = s.split(&span);
        let count = along.as_ref().map_or(1, |a| a.len()) * up.as_ref().map_or(1, |u| u.len());
        if enter && !typing && count >= 2 && (s.make != Make::Copies || item.is_some()) {
            go = true;
        }
        if esc || close {
            self.divide.open = false;
            self.status = "Divide left.".into();
            return;
        }
        self.divide.offset += moved_by;
        if go {
            let item = item.filter(|_| s.make == Make::Copies);
            match divide::divide_grid(
                self.doc.model(),
                h,
                along.as_deref(),
                up.as_deref(),
                s.make,
                s.gap(),
                item,
            ) {
                Ok(tx) => {
                    // the parts remember how they were divided
                    let made: Vec<ElementId> = tx
                        .ops
                        .iter()
                        .filter_map(|o| match o {
                            creabim_core::ops::Op::Insert(e) => Some(e.id),
                            creabim_core::ops::Op::Replace { after, .. } => Some(after.id),
                            _ => None,
                        })
                        .collect();
                    let r = self.doc.commit(tx);
                    if r.is_ok() {
                        for id in made {
                            self.divide.memory.insert(id, s);
                        }
                        self.divide.open = false;
                    }
                    self.report(r, format!("Divided in {count}. Ctrl+Z puts it back."));
                }
                Err(e) => self.status = e,
            }
        }
    }
}
