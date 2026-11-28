//! Edits that follow the pointer: the Move and Rotate tools, which take a
//! base point and then a destination, as in Revit or AutoCAD; Blender-style
//! move (G), rotate (R) and scale (S) from the keyboard; and dragging grips
//! and elements in plans. The same code serves the 3D view, plans and
//! sections; each pane only says how its pointer maps onto the model and
//! where it snaps.
//!
//! The Move tool goes: pick what to move (if nothing is selected), click a
//! base point (it snaps), then the selection follows the pointer with a
//! rubber band from the base point until a click places it. Nothing moves
//! before the base point is picked. Esc or right-click steps back a stage.
//!
//! Move works on whole elements, or (switched on its panel, or with F) on
//! one face: the face under the pointer lights up, and after the base
//! click it is pushed or pulled along its normal, SketchUp style, the
//! element's own parameters following (see `creabim_core::pushpull`).

use crate::handles::{Constraint, HandleKind, SnapFn};
use crate::snaps::{Snap, SnapKind};
use creabim_core::{
    pushpull::{self, FaceRef},
    transform::{self, Xform},
    Axis, Document, Element, ElementId, ElementKind, ModelError, Transaction, Workplane,
};
use eframe::egui::{self, Color32, Pos2, Stroke};
use glam::{DVec2, DVec3};

/// An edit remade from the same starting elements every frame, so it can
/// follow the pointer and still undo, or cancel, as one step.
#[derive(Debug, Default)]
pub struct LiveEdit {
    originals: Vec<Element>,
    label: String,
    applied: bool,
    /// Nothing else follows: a wall end dragged free of its joins.
    free: bool,
    /// What the last [`Self::update`] was asked to show, and whether free:
    /// asked the same again (the pointer at rest), nothing is redone.
    shown: Option<(Vec<Element>, bool)>,
}

impl LiveEdit {
    pub fn new(label: impl Into<String>, originals: Vec<Element>) -> Self {
        Self {
            originals,
            label: label.into(),
            applied: false,
            free: false,
            shown: None,
        }
    }

    /// With `free`, nothing joined to the edited elements follows them.
    pub fn free(mut self, free: bool) -> Self {
        self.free = free;
        self
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    /// Call the edit something else from now on (Inset turned Outset).
    pub fn relabel(&mut self, label: impl Into<String>) {
        self.label = label.into();
    }

    pub fn originals(&self) -> &[Element] {
        &self.originals
    }

    /// Show `changed` (the originals edited, same ids) in place of the
    /// previous frame's version.
    pub fn update(&mut self, doc: &mut Document, changed: Vec<Element>) -> Result<(), ModelError> {
        // the same as the frame before, still in place: the model, and
        // with it every drawing and the 3D scene, stays as it is (a move
        // held still redid everything 15 times a second)
        if self.applied
            && doc.undo_label() == Some(self.label.as_str())
            && self
                .shown
                .as_ref()
                .is_some_and(|(c, free)| *free == self.free && *c == changed)
        {
            return Ok(());
        }
        self.cancel(doc);
        self.shown = Some((changed.clone(), self.free));
        let mut changed = changed;
        // walls joined to a moved wall stretch to stay joined
        let follow = if self.free {
            vec![]
        } else {
            creabim_core::joins::follow(doc.model(), &mut changed)
        };
        let mut tx = Transaction::new(self.label.clone());
        for (o, n) in self.originals.iter().zip(changed) {
            if *o != n {
                tx = tx.replace(o.clone(), n);
            }
        }
        for (o, n) in follow {
            tx = tx.replace(o, n);
        }
        if tx.ops.is_empty() {
            return Ok(());
        }
        doc.commit(tx)?;
        self.applied = true;
        Ok(())
    }

    /// Put everything back as it was.
    pub fn cancel(&mut self, doc: &mut Document) {
        if self.applied && doc.undo_label() == Some(self.label.as_str()) {
            doc.discard_last();
        }
        self.applied = false;
    }

    pub fn applied(&self) -> bool {
        self.applied
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Move,
    Rotate,
    Scale,
    /// Mirror across a line drawn with two clicks, on the view's plane (in
    /// 3D the workplane, or the level).
    Mirror,
    /// Offset a line or an outline by a distance, to the pointer's side.
    Offset,
    /// One grip of one element, following the pointer from where it is:
    /// an outline's edge offset, or a corner just inserted (from the edge
    /// palette). Set up with [`Modal::with_grip`].
    Grip,
    /// Pull (MU43): the face under the pointer, out or in along its
    /// normal. A building element's own values follow; a flat shape
    /// becomes a solid. It needs no selection.
    Pull,
    /// Inset the picked faces of a shape, following the pointer: a
    /// thickness, and with Ctrl a depth (MU49). Set up with
    /// [`Modal::with_inset`].
    Inset,
}

impl Mode {
    pub fn verb(self) -> &'static str {
        match self {
            Mode::Move => "Move",
            Mode::Rotate => "Rotate",
            Mode::Scale => "Scale",
            Mode::Mirror => "Mirror",
            Mode::Offset => "Offset",
            Mode::Grip => "Edit",
            Mode::Inset => "Inset",
            Mode::Pull => "Pull",
        }
    }

    /// Does it make copies by default (Ctrl keeps the original instead)?
    fn copies_by_default(self) -> bool {
        matches!(self, Mode::Mirror | Mode::Offset)
    }
}

/// What rotating and scaling turn about, as in Blender.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pivot {
    /// The middle of the selected elements' own pivots.
    #[default]
    Median,
    /// The centre of the box round them.
    Bounds,
    /// Each element about itself.
    Individual,
    /// The active element (the last one picked).
    Active,
    /// The pivot cursor, placed with Shift+right-click.
    Cursor,
}

impl Pivot {
    pub const ALL: [Pivot; 5] = [
        Pivot::Median,
        Pivot::Bounds,
        Pivot::Individual,
        Pivot::Active,
        Pivot::Cursor,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Pivot::Median => "Median point",
            Pivot::Bounds => "Bounding box centre",
            Pivot::Individual => "Individual origins",
            Pivot::Active => "Active element",
            Pivot::Cursor => "Pivot cursor",
        }
    }
}

/// Where a pointer edit runs, which decides the plane things move in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Space {
    /// The 3D view: things slide on the level they stand on, or up and down
    /// with the Z axis.
    Model,
    /// A plan: things slide in plan.
    Plan,
    /// A section or elevation: things slide in the section plane.
    Section { start: DVec2, end: DVec2 },
}

/// What Move works on: whole elements, or one face of an element.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Target {
    #[default]
    Object,
    Face,
}

/// A face under the pointer, as a pane picks it.
#[derive(Clone, Debug)]
pub struct FacePick {
    pub id: ElementId,
    pub face: FaceRef,
    /// Its triangles in model coordinates, for the highlight.
    pub tris: Vec<[DVec3; 3]>,
}

/// A face being pushed or pulled.
#[derive(Debug)]
struct Push {
    pick: FacePick,
    edit: LiveEdit,
    /// How far it went out along its normal (in when negative).
    distance: f64,
    /// What to say with it: an open line pulled is a surface, not a solid.
    note: String,
}

/// How a pane maps its pointer onto the model, for one frame.
pub struct PaneMap<'a> {
    /// The pointer as a model point on the plane through `pivot` that
    /// things move in (vertical when constrained to Z).
    pub to_model: &'a dyn Fn(Pos2, Option<Axis>, DVec3) -> Option<DVec3>,
    /// A model point on screen.
    pub project: &'a dyn Fn(DVec3) -> Option<Pos2>,
    /// The direction the pane looks in, into the screen.
    pub look: DVec3,
    /// The best snap near the pointer, as a model point. With a start
    /// point (the base point) it also tracks the axes through it; without
    /// one it lands on what is under the pointer (a face in 3D).
    pub snap: &'a SnapFn<'a>,
    /// The face under the pointer, in panes that can pick faces (3D).
    pub pick_face: Option<&'a dyn Fn(Pos2) -> Option<FacePick>>,
    /// A line of the drawing under the pointer (an edge, a wall face, a
    /// grid or centre line), as its two ends in the model: Mirror's picked
    /// line (MU142), in panes that draw lines.
    pub pick_line: Option<&'a dyn Fn(Pos2) -> Option<(DVec3, DVec3)>>,
}

/// How the edit was started, which decides how it takes clicks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Flow {
    /// From a key over a view (G, R, S), Blender style: it follows the
    /// pointer at once, from the point under it.
    #[default]
    Keys,
    /// From the Move or Rotate tool (a button, a menu or M): click a base
    /// point first, then where it goes.
    Tool,
}

/// Where a pointer edit is in its clicks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Waiting for the base point (or a rotation's centre). Nothing has
    /// moved yet.
    Base,
    /// A rotation: centre picked, waiting for the direction to turn from.
    Reference,
    /// Following the pointer from the base point.
    Follow,
}

/// One frame's input for an edit, as read from the pane.
#[derive(Clone, Debug, Default)]
pub struct Events {
    /// The pointer over the pane, if it is there.
    pub pointer: Option<Pos2>,
    /// A left click (not the end of a drag) in the pane.
    pub click: bool,
    /// A right click (not the end of a pan) in the pane.
    pub right: bool,
    pub enter: bool,
    pub escape: bool,
    pub backspace: bool,
    /// Ctrl went down this frame.
    pub ctrl_pressed: bool,
    /// Ctrl is held.
    pub ctrl: bool,
    /// Shift is held: keep square to the pane's axes.
    pub shift: bool,
    /// Alt is held: Scale from the centre, not the opposite corner.
    pub alt: bool,
    /// Keys typed: x, y, z, digits.
    pub text: String,
}

impl Events {
    /// Read the frame's keys, with the pane's pointer and clicks.
    pub fn read(
        ctx: &egui::Context,
        pointer: Option<Pos2>,
        click: bool,
        right: bool,
        ctrl_before: &mut bool,
    ) -> Self {
        let unfocus = egui::Id::new("modal.unfocus");
        if ctx
            .data_mut(|d| d.remove_temp::<bool>(unfocus))
            .unwrap_or(false)
        {
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
        }
        let typing = ctx.egui_wants_keyboard_input();
        let mut ev = ctx.input(|i| Events {
            pointer,
            click,
            right,
            enter: !typing && i.key_pressed(egui::Key::Enter),
            escape: i.key_pressed(egui::Key::Escape),
            backspace: !typing && i.key_pressed(egui::Key::Backspace),
            ctrl_pressed: false,
            ctrl: i.modifiers.ctrl || i.modifiers.mac_cmd,
            shift: i.modifiers.shift,
            alt: i.modifiers.alt,
            text: if typing {
                String::new()
            } else {
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::Text(t) => Some(t.as_str()),
                        _ => None,
                    })
                    .collect()
            },
        });
        ev.ctrl_pressed = ev.ctrl && !*ctrl_before;
        *ctrl_before = ev.ctrl;
        ev
    }
}

/// Let go of the keyboard focus as the next edit reads its keys: a button
/// that started it keeps the focus otherwise, and typed values go to it.
pub fn unfocus_soon(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("modal.unfocus"), true));
}

/// An element changed as an edit changes it, or `None` when it can't be.
type Change<'a> = dyn Fn(&creabim_core::Model, &Element) -> Option<Element> + 'a;

/// Copies following the pointer, for Move with Ctrl: each click leaves one
/// behind, as its own undo step.
#[derive(Debug)]
struct CopyPreview {
    label: String,
    /// Fresh copies, where the originals are.
    copies: Vec<Element>,
    /// Which copies move (a door in a copied wall goes with its wall).
    moves: Vec<bool>,
    applied: bool,
}

impl CopyPreview {
    fn new(doc: &Document, originals: &[Element]) -> Option<Self> {
        let model = doc.model();
        let ids: Vec<ElementId> = originals.iter().map(|e| e.id).collect();
        let set = transform::copy_set(model, &ids);
        let elements: Vec<Element> = set
            .iter()
            .filter_map(|id| model.get(*id))
            .cloned()
            .collect();
        let (mut copies, _) = transform::fresh_copies(model, &elements);
        if copies.len() != elements.len() || copies.is_empty() {
            return None;
        }
        let copied: Vec<ElementId> = copies.iter().map(|c| c.id).collect();
        let mut moves = Vec::with_capacity(copies.len());
        for (c, o) in copies.iter_mut().zip(&elements) {
            if ids.contains(&o.id) {
                c.name = format!("{} copy", c.name);
            }
            moves.push(match &c.kind {
                ElementKind::Opening(op) => !copied.contains(&op.host),
                ElementKind::Cut(_) => false,
                _ => true,
            });
        }
        let what = match ids.len() {
            1 => originals[0].name.clone(),
            n => format!("{n} elements"),
        };
        Some(Self {
            label: format!("Copy {what}"),
            copies,
            moves,
            applied: false,
        })
    }

    fn update(&mut self, doc: &mut Document, f: &Change) -> Result<(), String> {
        self.cancel(doc);
        let mut tx = Transaction::new(self.label.clone());
        for (c, moves) in self.copies.iter().zip(&self.moves) {
            let c = if *moves {
                f(doc.model(), c).ok_or_else(|| format!("{} can't be copied there", c.name))?
            } else {
                c.clone()
            };
            tx = tx.insert(c);
        }
        doc.commit(tx).map_err(|e| e.to_string())?;
        self.applied = true;
        Ok(())
    }

    fn cancel(&mut self, doc: &mut Document) {
        if self.applied && doc.undo_label() == Some(self.label.as_str()) {
            doc.discard_last();
        }
        self.applied = false;
    }
}

/// The elements attached to any of `ids` (they refer to one), each once,
/// in the model's order: what `Model::dependents` finds for every id, in
/// one pass over the model. Asked every frame of a move, so a pass for
/// each of 180 moving elements was most of the frame.
pub fn attached_to(model: &creabim_core::Model, ids: &[ElementId]) -> Vec<ElementId> {
    let picked: std::collections::HashSet<ElementId> = ids.iter().copied().collect();
    model
        .iter()
        .filter(|e| e.references().iter().any(|r| picked.contains(r)))
        .map(|e| e.id)
        .collect()
}

/// How far the pointer must go from where a move began before anything
/// moves, in points: a click or a hand's tremor nudges nothing.
pub const STILL: f32 = 4.0;

/// A move, rotate or scale in progress, from a key or a tool.
static BAR_BY_POINTER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Where the edit bar was dragged to, from its own place.
fn bar_offset(ctx: &egui::Context) -> egui::Vec2 {
    ctx.data_mut(|m| {
        *m.get_persisted_mut_or_default::<egui::Vec2>(egui::Id::new("modal_bar_offset"))
    })
}

/// Show the edit bar by the pointer (true) or over the top of the view.
pub fn set_bar_by_pointer(on: bool) {
    BAR_BY_POINTER.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Rotate's step, degrees: 0 for none (Free), else 15, 45 or 90, picked in
/// its tool row (MU142). For the session.
static ROTATE_STEP: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn rotate_step() -> f64 {
    ROTATE_STEP.load(std::sync::atomic::Ordering::Relaxed) as f64
}

pub fn set_rotate_step(deg: f64) {
    ROTATE_STEP.store(deg.max(0.0) as u32, std::sync::atomic::Ordering::Relaxed);
}

/// What the protractor and the axes count from (MU142), as Blender's
/// Transform Orientations; picked in Rotate's row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orient {
    /// The plan's axes: 0° is east.
    Global,
    /// The object's own, as it stood when picked up: 0° on its front.
    Local,
    Normal,
    View,
    Cursor,
    Host,
}

impl Orient {
    pub const ALL: [Orient; 6] = [
        Orient::Global,
        Orient::Local,
        Orient::Normal,
        Orient::View,
        Orient::Cursor,
        Orient::Host,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Orient::Global => "Global",
            Orient::Local => "Local",
            Orient::Normal => "Normal",
            Orient::View => "View",
            Orient::Cursor => "Cursor",
            Orient::Host => "Host",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Orient::Global => "The plan's axes: 0° is east",
            Orient::Local => "Its own: 0° on its front, as it stood when picked up",
            Orient::Normal => "A picked face's own way, in Edit mode: 0° out of a wall-like face, along the first edge of a flat one",
            Orient::View => "The screen's: 0° to the right of the view",
            Orient::Cursor => "From the pivot cursor: 0° from where it stands towards what turns",
            Orient::Host => "Along the wall a door or window is in, or the wall nearest an object",
        }
    }

    /// All six are built (MU142).
    pub fn ready(self) -> bool {
        true
    }
}

/// Rotate's orientation for the session (MU142: Local to start with).
static ROTATE_ORIENT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(1);

pub fn rotate_orient() -> Orient {
    Orient::ALL
        .get(ROTATE_ORIENT.load(std::sync::atomic::Ordering::Relaxed) as usize)
        .copied()
        .unwrap_or(Orient::Local)
}

pub fn set_rotate_orient(o: Orient) {
    if let Some(i) = Orient::ALL.iter().position(|x| *x == o) {
        ROTATE_ORIENT.store(i as u8, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Scale's settings, picked in its row and kept for the session (MU142,
/// Tool Rows and Scale REV01): from the opposite corner (else the
/// centre), proportions kept (else free), by a factor (else by a
/// reference length), and the step its factor goes in.
static SCALE_CORNER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
static SCALE_FREE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static SCALE_BY_REF: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// In hundredths; 0 for any factor.
static SCALE_STEP: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(5);

pub fn scale_corner() -> bool {
    SCALE_CORNER.load(std::sync::atomic::Ordering::Relaxed)
}
pub fn set_scale_corner(on: bool) {
    SCALE_CORNER.store(on, std::sync::atomic::Ordering::Relaxed);
}
pub fn scale_free() -> bool {
    SCALE_FREE.load(std::sync::atomic::Ordering::Relaxed)
}
pub fn set_scale_free(on: bool) {
    SCALE_FREE.store(on, std::sync::atomic::Ordering::Relaxed);
}
pub fn scale_by_ref() -> bool {
    SCALE_BY_REF.load(std::sync::atomic::Ordering::Relaxed)
}
pub fn set_scale_by_ref(on: bool) {
    SCALE_BY_REF.store(on, std::sync::atomic::Ordering::Relaxed);
}
/// The step Scale's factor goes in, 0 for any.
pub fn scale_step() -> f64 {
    SCALE_STEP.load(std::sync::atomic::Ordering::Relaxed) as f64 / 100.0
}
pub fn set_scale_step(step: f64) {
    SCALE_STEP.store(
        (step.max(0.0) * 100.0).round() as u32,
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// A Scale under way: what it was started on, and how far it has gone.
#[derive(Clone, Debug, Default)]
struct ScaleRun {
    /// The selection's extent in plan as picked up.
    extent: Option<(DVec2, DVec2)>,
    /// The pivot it was started with: its centre.
    centre: DVec3,
    /// The corner opposite the one the pointer started at, once known.
    corner: Option<DVec3>,
    /// By reference: the two ends of the length known, as clicked.
    refs: Vec<DVec3>,
    /// The factors last shown, along x, y and z.
    factor: DVec3,
    /// Alt held this frame: from the centre.
    alt: bool,
    /// Started from where it was clicked: the Scale tool waits for a
    /// click first, as Rotate's does (the S key starts at once).
    started: bool,
}

/// What an orientation reads besides what turns (MU142): the way the
/// view looks (View), the face picked in Edit mode (Normal), and where the
/// pivot cursor stands (Cursor).
#[derive(Clone, Copy, Debug, Default)]
pub struct OrientFrom {
    pub look: Option<DVec3>,
    pub face: Option<usize>,
    pub cursor: Option<DVec3>,
}

/// [`zero_way`] with what the other orientations read (MU142): Normal a
/// picked face's own way, View the screen's right, Cursor from the pivot
/// cursor towards what turns, Host along a door's or window's wall (or
/// the wall nearest an object). Each falls back to Global without it.
pub fn zero_way_from(
    model: &creabim_core::Model,
    els: &[Element],
    orient: Orient,
    from: OrientFrom,
) -> DVec2 {
    let middle = || -> Option<DVec2> {
        let ps: Vec<DVec2> = els
            .iter()
            .filter_map(|e| transform::pivot(model, e.id).map(|p| p.truncate()))
            .collect();
        (!ps.is_empty()).then(|| ps.iter().copied().sum::<DVec2>() / ps.len() as f64)
    };
    let found = match orient {
        Orient::View => from
            .look
            .and_then(|look| look.cross(DVec3::Z).truncate().try_normalize()),
        Orient::Cursor => match (from.cursor, middle()) {
            (Some(c), Some(m)) => (m - c.truncate()).try_normalize(),
            _ => None,
        },
        Orient::Normal => match (els, from.face) {
            ([e], Some(f)) => match &e.kind {
                ElementKind::FreeMesh(m) if f < m.mesh.faces.len() => {
                    let n = m.to_model(m.mesh.face_normal(f)) - m.to_model(DVec3::ZERO);
                    if n.z.abs() < 0.7 {
                        // a wall-like face: out of it
                        n.truncate().try_normalize()
                    } else {
                        // a flat one: along its first edge
                        let ring = &m.mesh.faces[f];
                        ring.get(1).and_then(|b| {
                            let (a, b) = (
                                m.mesh.vertices[ring[0] as usize],
                                m.mesh.vertices[*b as usize],
                            );
                            (m.to_model(b) - m.to_model(a)).truncate().try_normalize()
                        })
                    }
                }
                _ => None,
            },
            _ => None,
        },
        Orient::Host => {
            let wall_way = |w: &creabim_core::Wall| (w.end - w.start).try_normalize();
            match els {
                [e] => match &e.kind {
                    ElementKind::Opening(o) => match model.get(o.host).map(|h| &h.kind) {
                        Some(ElementKind::Wall(w)) => wall_way(w),
                        _ => None,
                    },
                    _ => middle().and_then(|m| {
                        let level = match &e.kind {
                            ElementKind::Object(o) => Some(o.level),
                            _ => None,
                        };
                        model
                            .iter()
                            .filter_map(|x| match &x.kind {
                                ElementKind::Wall(w) if level.is_none_or(|l| l == w.level) => {
                                    let d = (w.end - w.start).length_squared().max(1e-12);
                                    let t =
                                        ((m - w.start).dot(w.end - w.start) / d).clamp(0.0, 1.0);
                                    Some((m.distance(w.start + (w.end - w.start) * t), w))
                                }
                                _ => None,
                            })
                            .min_by(|a, b| a.0.total_cmp(&b.0))
                            .and_then(|(_, w)| wall_way(w))
                    }),
                },
                _ => None,
            }
        }
        Orient::Global | Orient::Local => None,
    };
    if let Some(d) = found {
        return d;
    }
    if orient == Orient::Local {
        if let [e] = els {
            if let (ElementKind::Object(o), Some(t)) =
                (&e.kind, creabim_core::objects::type_of(model, e))
            {
                let (lo, hi) = t.bounds();
                let mid = (lo.truncate() + hi.truncate()) / 2.0;
                // its back is its own +y: its front is the other way
                if let Some(d) = (o.to_plan(mid - DVec2::Y) - o.to_plan(mid)).try_normalize() {
                    return d;
                }
            }
        }
    }
    DVec2::X
}

/// What Mirror flips across (MU142), picked in its tool row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Across {
    /// Two points clicked: a line drawn where it is needed, at any angle
    /// (Revit's Draw Axis).
    TwoPoints,
    /// The selection's own centre line front to back: left for right.
    CentreSide,
    /// Its centre line side to side: front for back.
    CentreFront,
    /// A line already in the drawing, clicked: a wall's face or edge, a
    /// grid or centre line, any line drawn (Revit's Pick Axis, MU142).
    PickLine,
}

impl Across {
    pub const ALL: [Across; 4] = [
        Across::PickLine,
        Across::TwoPoints,
        Across::CentreSide,
        Across::CentreFront,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Across::PickLine => "Pick a line",
            Across::TwoPoints => "Draw a line",
            Across::CentreSide => "Its centre ↔",
            Across::CentreFront => "Its centre ↕",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Across::PickLine => "Click a line already there: a wall's face or edge, a grid or centre line, any line drawn",
            Across::TwoPoints => "Click two points for the line, at any angle, where it is needed",
            Across::CentreSide => "Flip it left for right, in place, across its own centre line",
            Across::CentreFront => "Flip it front for back, in place, across its own centre line",
        }
    }
}

/// Mirror's choices for the session: across what (0 two points, 1 its
/// centre ↔, 2 its centre ↕), and whether it makes a copy (MU142).
static MIRROR_ACROSS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static MIRROR_COPY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn mirror_across() -> Across {
    match MIRROR_ACROSS.load(std::sync::atomic::Ordering::Relaxed) {
        1 => Across::CentreSide,
        2 => Across::CentreFront,
        3 => Across::PickLine,
        _ => Across::TwoPoints,
    }
}

pub fn set_mirror_across(a: Across) {
    let v = match a {
        Across::TwoPoints => 0,
        Across::CentreSide => 1,
        Across::CentreFront => 2,
        Across::PickLine => 3,
    };
    MIRROR_ACROSS.store(v, std::sync::atomic::Ordering::Relaxed);
}

pub fn mirror_copy() -> bool {
    MIRROR_COPY.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_mirror_copy(on: bool) {
    MIRROR_COPY.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The centre line Mirror flips `els` across (MU142): through one
/// object's middle along its own axes, or through `pivot` along the plan's
/// for anything else. Returns a point on the line and its direction, or
/// None for two points.
pub fn centre_line(
    model: &creabim_core::Model,
    els: &[Element],
    pivot: DVec3,
    across: Across,
) -> Option<(DVec3, DVec3)> {
    let along_y = match across {
        Across::TwoPoints | Across::PickLine => return None,
        Across::CentreSide => true,
        Across::CentreFront => false,
    };
    if let [e] = els {
        if let (ElementKind::Object(o), Some(t)) =
            (&e.kind, creabim_core::objects::type_of(model, e))
        {
            let (lo, hi) = t.bounds();
            let mid = (lo.truncate() + hi.truncate()) / 2.0;
            let way = if along_y { DVec2::Y } else { DVec2::X };
            let c = o.to_plan(mid);
            let d = (o.to_plan(mid + way) - c).normalize_or_zero();
            return Some((c.extend(pivot.z), d.extend(0.0)));
        }
    }
    let d = if along_y { DVec3::Y } else { DVec3::X };
    Some((pivot, d))
}

/// How near a named angle (0, ±90, 180 from the start) a free turn locks
/// to it, degrees.
pub const TURN_LOCK: f64 = 4.0;

/// A turn of `d` degrees from where it started, as Rotate takes it: in
/// the picked `step` (0 for Free), or 15° while Ctrl is held when Free, else
/// in `fine` steps; and, Free, drawn to a named angle (0, ±90, 180) within
/// [`TURN_LOCK`]. Returns the turn and the named angle it locked to.
pub fn turn_degrees(d: f64, ctrl: bool, fine: f64, step: f64) -> (f64, Option<f64>) {
    let snapped = |v: f64, by: f64| if by > 0.0 { (v / by).round() * by } else { v };
    let by = if step > 0.0 {
        step
    } else if ctrl {
        15.0
    } else {
        fine
    };
    let t = snapped(d, by);
    let named = [-180.0, -90.0, 0.0, 90.0, 180.0]
        .into_iter()
        .find(|n: &f64| (d - n).abs() <= TURN_LOCK.max(by / 2.0));
    match named {
        Some(n) if (t - n).abs() < 1e-9 || step == 0.0 => (n, Some(n)),
        _ => (t, None),
    }
}

#[derive(Debug)]
pub struct Modal {
    pub mode: Mode,
    /// A mesh face being edited instead of the whole element.
    pub face: Option<usize>,
    /// A mass's picked vertices in Edit mode: what the edit moves, turns
    /// or scales, about their middle (MU39).
    pub verts: Option<Vec<u32>>,
    /// The corners of an outline picked in Edit mode: Move takes them and
    /// leaves the rest of the outline where it is (MU46).
    pub corners: Option<Vec<(crate::handles::Ring, usize)>>,
    /// Stretch (wave 3): what a box took of each element, a Move moving
    /// only that: whole, its ends or its corners.
    pub stretch: Option<Vec<(ElementId, crate::stretchtool::Part)>>,
    pub space: Space,
    /// The pane that runs it: a view, or `None` for the 3D view.
    pub pane: Option<ElementId>,
    /// The area (its interaction id) that runs it, once one has taken it:
    /// two areas can show the same view, and only one may run the move
    /// and draw its bar, or the other moved the elements with its own
    /// camera and no pointer, out of sight.
    pub owner: Option<egui::Id>,
    pub flow: Flow,
    stage: Stage,
    /// The point things turn and scale about.
    pivot: DVec3,
    /// Each element's own pivot, for individual origins.
    own: Vec<DVec3>,
    individual: bool,
    grab: Pos2,
    pointer: Pos2,
    axis: Option<Axis>,
    /// The plane a move keeps in, by the axis square to it (the handle's
    /// square, MU85).
    keep_plane: Option<Axis>,
    /// Taken by the handle: its steps are the handle's (5 degrees, and
    /// Shift for coarser ones).
    from_handle: bool,
    /// The named angle a turn has locked to (0, 90, 180, 270 from where it
    /// started), for the protractor to draw in the guide colour (MU142).
    pub locked: Option<f64>,
    /// How far the turn has swept on screen, in degrees anticlockwise
    /// from the grab, after its step and lock: the protractor's arc.
    pub swept: f64,
    /// Mirror's remembered Copy tick as this edit last saw it, so a change
    /// in its row reaches the edit while Ctrl's stays for the moment.
    copy_seen: bool,
    /// Mirror across its own centre: the line and copy last shown, so it
    /// is applied once, not every frame.
    centred: Option<(DVec3, DVec3, bool)>,
    /// Mirror's Across for this edit alone; None follows its row.
    pub across: Option<Across>,
    /// Where Rotate's protractor puts 0°, in plan (MU142 orientation).
    zero: DVec2,
    /// Where the handle was pressed, until the move has taken it as its
    /// base point.
    handle_press: Option<Pos2>,
    /// Taken by the handle: the handle's own point. A snap onto something
    /// puts this on it, so the picked corners land on the target rather
    /// than the point pressed on the arrow (owner markup 2026-11-27).
    handle_origin: Option<DVec3>,
    typed: String,
    edit: LiveEdit,
    /// Started by Shift+D: cancelling also takes the copies away.
    pub duplicated: bool,
    /// The offset now shown: how far, across what, as a copy or not. The
    /// same again asks for nothing (MU47: with the pointer still, Offset
    /// built the 3D view again every frame).
    offset_shown: Option<(f64, DVec3, bool)>,
    /// Started by a drag on the handle: letting go of the button ends it.
    release_ends: bool,
    /// What a [`Mode::Inset`] edit works on, and its depth.
    inset: Option<InsetRun>,
    scale: ScaleRun,
    /// Where the pivot cursor stood when the edit began (Cursor's
    /// orientation reads it).
    cursor: Option<DVec3>,
    shown: String,
    /// Elements left out because they can't take this edit.
    skipped: usize,
    /// Finish or Cancel pressed on the on-screen panel.
    ask: Option<bool>,
    /// The base point things move by (or a rotation's centre), and where
    /// the pointer was when it was picked.
    base: Option<DVec3>,
    base_at: Option<Pos2>,
    /// A rotation's reference point: the direction it turns from.
    reference: Option<DVec3>,
    /// The snap under the pointer this frame, for its marker.
    snap: Option<Snap>,
    /// Where the base point goes this frame.
    target: Option<DVec3>,
    /// Shift held: square to the pane's axes.
    ortho: bool,
    /// Leave copies behind instead of moving (Ctrl).
    pub copy: bool,
    copies: Option<CopyPreview>,
    /// Copies placed so far.
    placed: usize,
    /// The last move applied, for the readout and tests.
    delta: DVec3,
    ctrl_before: bool,
    /// Copy switched on the on-screen panel.
    want_copy: Option<bool>,
    /// Where the bar by the pointer sits: where the pointer was when the
    /// edit started (MU38: it stays put, it doesn't chase the pointer).
    anchor: Option<egui::Pos2>,
    /// Whole elements or one face, for Move.
    pub works_on: Target,
    /// Switched on the on-screen panel.
    want_works_on: Option<Target>,
    /// The face under the pointer while the base point is picked.
    hover: Option<FacePick>,
    push: Option<Push>,
    /// The workplane in 3D: Rotate turns about its normal, Move slides on
    /// it, Mirror and Offset work on it.
    plane: Option<Workplane>,
    /// The grip a [`Mode::Grip`] edit drives.
    grip: Option<GripRun>,
    /// The axis the pointer is running along, when none is held: the move
    /// keeps to it while the pointer stays on it (X and Y from the axes
    /// the snaps draw through the base point, Z from the pointer going
    /// straight up or down from it on the screen).
    pub inferred: Option<Axis>,
}

/// A [`Mode::Inset`] edit: the faces, the depth and what it shows.
#[derive(Clone, Debug)]
struct InsetRun {
    faces: Vec<usize>,
    /// How far the new faces sink (negative) or rise.
    depth: f64,
    /// The Depth field of the bar: typed, it wins over Ctrl and the pointer.
    depth_typed: String,
    /// Where the pointer was last frame, for Ctrl's depth.
    last: Option<Pos2>,
    /// Outset (O): a rim around the face, which keeps its size.
    out: bool,
    /// Was Ctrl held last frame?
    was_ctrl: bool,
    /// Added to what the pointer gives as a thickness: set when Ctrl is
    /// let go, so the thickness carries on from its value and does not
    /// jump to where the pointer went for the depth.
    bias: f64,
    /// The value by the pointer.
    label: String,
    /// The new edges, in model space, drawn over the view: faces made in
    /// the plane of the old one show no line of their own in 3D.
    lines: Vec<(DVec3, DVec3)>,
}

/// The grip a [`Mode::Grip`] edit drives, and the walls joined to it.
#[derive(Debug)]
struct GripRun {
    kind: HandleKind,
    constraint: Constraint,
    joined: Vec<(usize, bool)>,
}

pub enum Outcome {
    Running(String),
    Done(String),
    Cancelled(String),
}

/// Parse a typed length in the project's units: a bare number is in the
/// project's unit, and any unit can be written (`2400mm`, `2.4m`, `8'-6"`).
pub fn parse_length(s: &str) -> Option<f64> {
    crate::units::parse(s)
}

/// The normal of the plane an edit works on: the level in plans and in
/// 3D (or the workplane there), the section plane in sections.
fn drawn_on(space: Space, plane: Option<Workplane>) -> DVec3 {
    match space {
        Space::Plan => DVec3::Z,
        Space::Model => plane.map_or(DVec3::Z, |p| p.normal()),
        Space::Section { start, end } => -(end - start).normalize_or(DVec2::X).perp().extend(0.0),
    }
}

/// Two directions square to each other on that plane: along and up it.
fn plane_axes(space: Space, plane: Option<Workplane>) -> [DVec3; 2] {
    match (space, tilted(space, plane)) {
        (Space::Section { start, end }, _) => {
            [(end - start).normalize_or(DVec2::X).extend(0.0), DVec3::Z]
        }
        (_, Some(p)) => [p.x, p.y],
        _ => [DVec3::X, DVec3::Y],
    }
}

/// The 3D view's workplane when it is not level.
fn tilted(space: Space, plane: Option<Workplane>) -> Option<Workplane> {
    plane.filter(|p| space == Space::Model && p.normal().z.abs() < 0.999)
}

/// The move from `base` to `target`, kept to `axis`, square to the pane
/// (`ortho`: the larger of the two axes), level unless `axis` is Z when
/// `level` (in 3D things slide on their level), and `typed` long when a
/// length was typed (along the way the pointer goes, or `fallback`).
pub fn move_delta(
    base: DVec3,
    target: DVec3,
    axis: Option<Axis>,
    ortho: Option<[DVec3; 2]>,
    typed: Option<f64>,
    fallback: DVec3,
    level: bool,
) -> DVec3 {
    let mut d = target - base;
    if level && axis != Some(Axis::Z) {
        d.z = 0.0;
    }
    if let Some(a) = axis {
        d = a.unit() * d.dot(a.unit());
    } else if let Some([u, v]) = ortho {
        d = if d.dot(u).abs() >= d.dot(v).abs() {
            u * d.dot(u)
        } else {
            v * d.dot(v)
        };
    }
    match typed {
        Some(len) => {
            let dir = match axis {
                Some(a) if d.length() < 1e-9 => a.unit(),
                _ => d.normalize_or(fallback),
            };
            dir * len
        }
        None => d,
    }
}

impl Modal {
    /// Start editing `ids` (the active one last), or one mesh face of a
    /// single mass, from pointer `grab`.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        doc: &Document,
        mode: Mode,
        ids: &[ElementId],
        face: Option<usize>,
        space: Space,
        pane: Option<ElementId>,
        grab: Pos2,
        pivot: Pivot,
        cursor: Option<DVec3>,
        plane: Option<Workplane>,
    ) -> Result<Modal, String> {
        let model = doc.model();
        // only the 3D view works on the workplane
        let plane = plane.filter(|_| space == Space::Model);
        let mut elements: Vec<&Element> = ids.iter().filter_map(|id| model.get(*id)).collect();
        // doors and windows go with their wall when it moves too
        let hosts: Vec<ElementId> = elements.iter().map(|e| e.id).collect();
        // so do cuts with what they cut
        elements.retain(|e| match &e.kind {
            ElementKind::Opening(o) => !hosts.contains(&o.host),
            ElementKind::Cut(_) => false,
            _ => true,
        });
        let blank = |pivot: DVec3, own: Vec<DVec3>, edit: LiveEdit| Modal {
            mode,
            face: None,
            verts: None,
            corners: None,
            stretch: None,
            space,
            pane,
            owner: None,
            flow: Flow::Keys,
            stage: Stage::Follow,
            pivot,
            own,
            individual: false,
            grab,
            pointer: grab,
            axis: None,
            keep_plane: None,
            from_handle: false,
            locked: None,
            swept: 0.0,
            copy_seen: mirror_copy(),
            centred: None,
            across: None,
            zero: DVec2::X,
            handle_press: None,
            handle_origin: None,
            typed: String::new(),
            edit,
            duplicated: false,
            offset_shown: None,
            release_ends: false,
            inset: None,
            scale: ScaleRun::default(),
            cursor,
            shown: String::new(),
            skipped: 0,
            ask: None,
            base: None,
            base_at: None,
            reference: None,
            snap: None,
            target: None,
            ortho: false,
            copy: false,
            copies: None,
            placed: 0,
            delta: DVec3::ZERO,
            ctrl_before: false,
            want_copy: None,
            anchor: None,
            works_on: Target::Object,
            want_works_on: None,
            hover: None,
            push: None,
            plane,
            grip: None,
            inferred: None,
        };
        if mode == Mode::Pull {
            // it picks its own face: what is selected does not matter
            let mut m = blank(DVec3::ZERO, vec![], LiveEdit::new("Pull", vec![]));
            m.flow = Flow::Tool;
            m.stage = Stage::Base;
            return Ok(m);
        }
        let first = *elements.last().ok_or("Select something first.")?;
        let face =
            face.filter(|_| elements.len() == 1 && matches!(first.kind, ElementKind::FreeMesh(_)));
        if let (Some(f), ElementKind::FreeMesh(m)) = (face, &first.kind) {
            if f < m.mesh.faces.len() {
                let z = m
                    .level
                    .and_then(|l| model.level(l))
                    .map_or(0.0, |l| l.elevation);
                let c = m.origin + m.to_model(m.mesh.face_center(f)) + DVec3::Z * z;
                let label = format!("{} face", mode.verb());
                let mut m = blank(c, vec![c], LiveEdit::new(label, vec![first.clone()]));
                m.face = face;
                return Ok(m);
            }
        }
        // keep what can take this kind of edit
        let normal = drawn_on(space, plane);
        let [across, _] = plane_axes(space, plane);
        let tilted = tilted(space, plane);
        let probe = |c: DVec3| match mode {
            Mode::Move | Mode::Mirror | Mode::Offset | Mode::Grip | Mode::Pull | Mode::Inset => {
                Xform::Move(DVec3::X)
            }
            Mode::Rotate => match space {
                Space::Section { .. } => Xform::Tilt {
                    center: c,
                    axis: DVec3::X,
                    degrees: 1.0,
                },
                _ if tilted.is_some() => Xform::Tilt {
                    center: c,
                    axis: normal,
                    degrees: 1.0,
                },
                _ => Xform::Rotate {
                    center: c.truncate(),
                    degrees: 1.0,
                },
            },
            Mode::Scale => Xform::Scale {
                center: c,
                factor: 1.1,
            },
        };
        let takes = |e: &Element, c: DVec3| match mode {
            Mode::Mirror => transform::mirrored(model, e, c, across).is_some(),
            Mode::Offset => {
                creabim_core::offset::offset(e, 0.1, normal).is_some()
                    || creabim_core::offset::offset(e, -0.1, normal).is_some()
            }
            _ => transform::transformed(model, e, &probe(c)).is_some(),
        };
        let mut kept: Vec<(Element, DVec3)> = Vec::new();
        let mut skipped = 0;
        for e in &elements {
            let Some(c) = transform::pivot(model, e.id) else {
                skipped += 1;
                continue;
            };
            if takes(e, c) {
                kept.push(((*e).clone(), c));
            } else {
                skipped += 1;
            }
        }
        // what is written about them on the drawings goes along with a
        // move or a turn of whole elements: the rooms their walls close
        // off, dimensions with a loose end (a selection made in 3D can't
        // reach those, and they stayed where the building had been). They
        // change neither the pivot nor the count of what is moved
        let along: Vec<(Element, DVec3)> =
            if matches!(mode, Mode::Move | Mode::Rotate) && face.is_none() {
                let moving: Vec<ElementId> = kept.iter().map(|k| k.0.id).collect();
                transform::carried_along(model, &moving)
                    .into_iter()
                    .filter_map(|id| {
                        let e = model.get(id)?;
                        let c = transform::pivot(model, id)?;
                        takes(e, c).then(|| (e.clone(), c))
                    })
                    .collect()
            } else {
                vec![]
            };
        if kept.is_empty() {
            let name = &first.name;
            return Err(match (mode, elements.len(), space) {
                (Mode::Rotate, _, Space::Section { .. }) => {
                    "Only masses, sketches and guides turn in a section; walls and slabs stay upright.".into()
                }
                (Mode::Move, 1, _) => format!("{name} can't be moved this way."),
                (Mode::Rotate, 1, _) => format!("{name} can't be rotated on its own."),
                (Mode::Scale, 1, _) => format!("{name} can't be scaled."),
                (Mode::Mirror, 1, _) => format!("{name} can't be mirrored here."),
                (Mode::Offset, 1, _) => format!(
                    "{name} can't be offset here: walls, beams, grid lines, slabs and roofs offset in a plan or on a level; sketches, guides and parts drawn on a face, on that face."
                ),
                (m, n, _) => format!("None of the {n} selected can take a {}.", m.verb().to_lowercase()),
            });
        }
        let mut own: Vec<DVec3> = kept.iter().map(|k| k.1).collect();
        let n = own.len() as f64;
        let median = own.iter().copied().sum::<DVec3>() / n;
        let lo = own.iter().copied().fold(DVec3::splat(f64::MAX), DVec3::min);
        let hi = own.iter().copied().fold(DVec3::splat(f64::MIN), DVec3::max);
        let center = match pivot {
            Pivot::Median | Pivot::Individual => median,
            Pivot::Bounds => (lo + hi) / 2.0,
            Pivot::Active => *own.last().expect("kept is not empty"),
            Pivot::Cursor => cursor.unwrap_or(median),
        };
        let what = if kept.len() == 1 {
            kept[0].0.name.clone()
        } else {
            format!("{} elements", kept.len())
        };
        let individual = pivot == Pivot::Individual && kept.len() > 1;
        own.extend(along.iter().map(|k| k.1));
        let mut m = blank(
            center,
            own,
            LiveEdit::new(
                format!("{} {what}", mode.verb()),
                kept.into_iter().chain(along).map(|k| k.0).collect(),
            ),
        );
        m.individual = individual;
        m.skipped = skipped;
        // Scale: the selection's extent in plan, for its opposite corner
        // and the outline it is drawn from (MU142)
        if mode == Mode::Scale {
            m.scale.centre = center;
            let ids: std::collections::HashSet<ElementId> =
                m.edit.originals().iter().map(|e| e.id).collect();
            let mut extent: Option<(DVec2, DVec2)> = None;
            for item in creabim_geom::scene::build_scene_of(model, Some(&ids))
                .iter()
                .filter(|i| ids.contains(&i.id) && !i.ghost)
            {
                if let Some((lo, hi)) = item.mesh.bounds() {
                    let (lo, hi) = (lo.as_dvec3().truncate(), hi.as_dvec3().truncate());
                    extent = Some(extent.map_or((lo, hi), |(a, b)| (a.min(lo), b.max(hi))));
                }
            }
            m.scale.extent = extent.filter(|(lo, hi)| (*hi - *lo).min_element() > 1e-6);
        }
        // Mirror's Copy is a tick in its row, remembered (MU142)
        if mode.copies_by_default() && (mode != Mode::Mirror || mirror_copy()) {
            m.copies = CopyPreview::new(doc, m.edit.originals());
            m.copy = m.copies.is_some();
        }
        Ok(m)
    }

    /// Take clicks as the Move or Rotate tool does: a base point first.
    pub fn with_flow(mut self, flow: Flow) -> Self {
        self.flow = flow;
        if flow == Flow::Tool
            && self.uses_points()
            && !matches!(self.mode, Mode::Offset | Mode::Grip | Mode::Inset)
        {
            self.stage = Stage::Base;
        }
        self
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// The workplane it works on, in 3D.
    pub fn plane(&self) -> Option<Workplane> {
        self.plane
    }

    /// Has the edit changed the model yet? Not before the pointer has
    /// moved off the base point.
    pub fn applied(&self) -> bool {
        self.push.as_ref().map_or(&self.edit, |p| &p.edit).applied()
    }

    /// The elements being edited.
    pub fn ids(&self) -> Vec<ElementId> {
        let edit = self.push.as_ref().map_or(&self.edit, |p| &p.edit);
        edit.originals().iter().map(|e| e.id).collect()
    }

    /// Is this Move working on one face, in a pane that can pick faces?
    fn on_faces(&self, map: &PaneMap) -> bool {
        (self.mode == Mode::Pull || (self.can_take_faces() && self.works_on == Target::Face))
            && map.pick_face.is_some()
    }

    /// Pull also works by pressing on a face, dragging and letting go,
    /// as a click, a move and a click: is this press or release of the
    /// left button one of its two clicks?
    pub fn pull_drag(&self, response: &egui::Response) -> bool {
        self.mode == Mode::Pull
            && match self.stage {
                Stage::Base => response.drag_started_by(egui::PointerButton::Primary),
                _ => response.drag_stopped_by(egui::PointerButton::Primary),
            }
    }

    /// The element whose face Pull has hold of, once one is clicked.
    pub fn pulling(&self) -> Option<ElementId> {
        (self.mode == Mode::Pull)
            .then(|| self.push.as_ref().map(|p| p.pick.id))
            .flatten()
    }

    /// Is this the Pull tool with nothing taken hold of yet?
    pub fn waits_to_pull(&self) -> bool {
        self.mode == Mode::Pull && self.push.is_none()
    }

    /// Is this the Pull tool, or Move working on a face?
    pub fn takes_faces(&self) -> bool {
        self.mode == Mode::Pull || self.works_on == Target::Face
    }

    /// Can this edit switch to faces: the Move tool, on whole elements.
    fn can_take_faces(&self) -> bool {
        self.mode == Mode::Move
            && self.flow == Flow::Tool
            && self.face.is_none()
            && !self.duplicated
    }

    /// Work on whole elements or on one face, starting again from the base
    /// point with nothing moved.
    pub fn set_works_on(&mut self, doc: &mut Document, t: Target) {
        if t == self.works_on || !self.can_take_faces() {
            return;
        }
        self.set_copy(doc, false);
        self.edit.cancel(doc);
        if let Some(mut p) = self.push.take() {
            p.edit.cancel(doc);
        }
        self.works_on = t;
        self.hover = None;
        self.typed.clear();
        self.delta = DVec3::ZERO;
        self.target = None;
        self.base = None;
        self.stage = Stage::Base;
    }

    /// Started from the handle's part on `axis`: it keeps to that axis,
    /// and by a drag it ends when the button comes up (MU47).
    /// Is this edit a drag, ended by letting the button go (the handle)?
    /// The view must not turn with such a drag.
    pub fn is_a_drag(&self) -> bool {
        self.release_ends
    }

    pub fn taken_by_handle(
        mut self,
        axis: Option<Axis>,
        plane: Option<Axis>,
        by_drag: bool,
        press: Option<Pos2>,
        origin: Option<DVec3>,
    ) -> Self {
        // measured from where the part was pressed
        if let Some(p) = press {
            self.grab = p;
            self.handle_press = Some(p);
        }
        self.handle_origin = origin;
        self.axis = axis;
        self.keep_plane = plane;
        self.release_ends = by_drag;
        self.from_handle = true;
        self
    }

    /// A move kept in the plane square to an axis (the handle's square),
    /// in a 3D view: the pointer's travel since the base point, read
    /// along the plane's two axes as they lie on screen at that point.
    fn plane_delta(&self, map: &PaneMap, base: DVec3, coarse: bool) -> Option<DVec3> {
        let n = self
            .keep_plane
            .filter(|_| self.axis.is_none() && self.space == Space::Model)?;
        let (a, b) = match n {
            Axis::X => (Axis::Y, Axis::Z),
            Axis::Y => (Axis::Z, Axis::X),
            Axis::Z => (Axis::X, Axis::Y),
        };
        let c = (map.project)(base)?;
        let pa = (map.project)(base + a.unit())? - c;
        let pb = (map.project)(base + b.unit())? - c;
        let det = pa.x * pb.y - pa.y * pb.x;
        if det.abs() < 1e-3 {
            return None;
        }
        let d = self.pointer - self.base_at?;
        let step = if coarse { 0.05 } else { 0.01 };
        let round = |v: f32| (v as f64 / step).round() * step;
        let i = round((d.x * pb.y - d.y * pb.x) / det);
        let j = round((pa.x * d.y - pa.y * d.x) / det);
        Some(a.unit() * i + b.unit() * j)
    }

    /// Start the edit on whole elements or one face, as last chosen.
    pub fn with_works_on(mut self, t: Target) -> Self {
        if self.can_take_faces() {
            self.works_on = t;
        }
        self
    }

    /// The move applied this frame.
    #[cfg(test)]
    pub fn delta(&self) -> DVec3 {
        self.delta
    }

    /// Does this edit work from picked points (a base point, a target)
    /// rather than from how far the pointer went?
    fn uses_points(&self) -> bool {
        match self.mode {
            Mode::Move => true,
            Mode::Rotate => self.flow == Flow::Tool && self.face.is_none(),
            Mode::Scale => false,
            Mode::Mirror | Mode::Offset | Mode::Grip | Mode::Pull | Mode::Inset => true,
        }
    }

    /// What the pane's snaps should leave out: the moving elements once
    /// they follow the pointer (else they would snap to themselves), and
    /// nothing while a base point is picked (it may be on them). `None`
    /// keeps the pane's own choice.
    pub fn snap_exclude(&self, doc: &Document) -> Option<Vec<ElementId>> {
        if !self.uses_points() {
            return None;
        }
        Some(match self.stage {
            Stage::Base | Stage::Reference => vec![],
            // a duplicate's copy lies on its original, drawn as one with
            // it: leaving the copy out would leave the original's points
            // out too. The snaps freeze as the copy starts to follow,
            // still on the original, so nothing snaps to it moving (MU26)
            Stage::Follow if self.duplicated => vec![],
            // a pulled face stops level with a corner or an edge of its
            // own shape too, as it stood before the pull (MU46: a face
            // could not be brought level with the next edge of its shape)
            Stage::Follow if self.mode == Mode::Pull => vec![],
            Stage::Follow => {
                let mut ids = self.ids();
                let attached = attached_to(doc.model(), &ids);
                ids.extend(attached);
                ids
            }
        })
    }

    /// Snaps stay as they were while things follow the pointer.
    pub fn freezes_snaps(&self) -> bool {
        self.stage == Stage::Follow
    }

    /// The pane's own axes, for Shift.
    fn pane_axes(&self) -> [DVec3; 2] {
        plane_axes(self.space, self.plane)
    }

    /// The plane things turn, mirror and offset on, by its normal.
    fn normal(&self) -> DVec3 {
        drawn_on(self.space, self.plane)
    }

    /// The workplane, when it is not level (a wall's face, a roof's slope).
    fn tilted(&self) -> Option<Workplane> {
        tilted(self.space, self.plane)
    }

    /// Follow the pointer and keys for one frame, drawing the feedback.
    /// `click` and `right` are the pane's own clicks (not drags or pans).
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        doc: &mut Document,
        ctx: &egui::Context,
        painter: &egui::Painter,
        pointer: Option<Pos2>,
        click: bool,
        right: bool,
        map: &PaneMap,
    ) -> Outcome {
        if self.release_ends && ctx.input(|i| i.pointer.primary_released()) {
            self.ask = Some(true);
        }
        let mut before = self.ctrl_before;
        let ev = Events::read(ctx, pointer, click, right, &mut before);
        self.ctrl_before = before;
        let out = self.step(doc, &ev, map);
        self.draw(painter, map);
        out
    }

    /// One frame of the edit, without drawing. A Duplicate that ends
    /// where it began (base point and target the same, often both snapped
    /// to the original) would leave its copy unseen on the original: it
    /// is taken away instead (MU24).
    pub fn step(&mut self, doc: &mut Document, ev: &Events, map: &PaneMap) -> Outcome {
        let out = self.step_inner(doc, ev, map);
        if let Outcome::Done(_) = &out {
            if self.duplicated
                && self.mode == Mode::Move
                && self.placed == 0
                && self.delta.length() < 1e-6
            {
                self.cancel(doc);
                return Outcome::Cancelled(
                    "The copy would sit exactly on its original, so it was taken away. Click a base point, then somewhere else.".into(),
                );
            }
        }
        out
    }

    fn step_inner(&mut self, doc: &mut Document, ev: &Events, map: &PaneMap) -> Outcome {
        if let Some(p) = ev.pointer {
            self.pointer = p;
        }
        self.ortho = ev.shift;
        let follow = self.stage == Stage::Follow;
        if let Some(t) = self.want_works_on.take() {
            self.set_works_on(doc, t);
        }
        for c in ev.text.chars() {
            match c.to_ascii_lowercase() {
                // F switches between whole elements and a face (unless it
                // is part of a typed length, as in 8ft)
                'f' if self.typed.is_empty() && self.can_take_faces() => {
                    let t = match self.works_on {
                        Target::Object => Target::Face,
                        Target::Face => Target::Object,
                    };
                    self.set_works_on(doc, t);
                }
                'x' => self.toggle(Axis::X),
                'y' => self.toggle(Axis::Y),
                'z' => self.toggle(Axis::Z),
                c @ ('0'..='9' | '.' | '-' | ',') if follow => {
                    self.typed.push(if c == ',' { '.' } else { c })
                }
                c @ ('m' | 'c' | 'f' | 't' | 'i' | 'n' | '\'' | '"' | '/' | ' ')
                    if follow && !self.typed.is_empty() =>
                {
                    self.typed.push(c)
                }
                _ => {}
            }
        }
        if ev.backspace {
            self.typed.pop();
        }
        // Ctrl leaves copies behind
        if ev.ctrl_pressed && self.copies_allowed() {
            self.set_copy(doc, !self.copy);
        }
        if let Some(on) = self.want_copy.take() {
            self.set_copy(doc, on);
        }
        // the orientation picked in Rotate's row, read as picked up (MU142)
        if self.mode == Mode::Rotate {
            let from = OrientFrom {
                look: Some(map.look),
                face: self.face,
                cursor: self.cursor,
            };
            self.zero = zero_way_from(doc.model(), self.edit.originals(), rotate_orient(), from);
        }
        // Mirror's Copy ticked or cleared in its row (MU142)
        if self.mode == Mode::Mirror && self.copies_allowed() && mirror_copy() != self.copy_seen {
            self.copy_seen = mirror_copy();
            self.set_copy(doc, self.copy_seen);
        }
        if ev.escape || ev.right || self.ask == Some(false) {
            self.ask = None;
            if ev.escape && !self.typed.is_empty() {
                self.typed.clear();
            } else if let Some(out) = self.back(doc) {
                return out;
            }
        }
        if !self.uses_points() {
            return self.step_relative(doc, ev, map);
        }
        if self.on_faces(map) {
            return self.step_faces(doc, ev, map);
        }
        if self.mode == Mode::Pull {
            return Outcome::Running(
                "Pull: this view picks no faces. Point at a face in the 3D view, a plan or a section.".into(),
            );
        }
        self.hover = None;
        let mut skipped = self.skipped_note();
        if self.works_on == Target::Face && self.can_take_faces() {
            skipped.push_str(" (this view picks no faces; here Move takes whole elements)");
        }
        if self.mode == Mode::Mirror && self.stage == Stage::Base {
            let across = self.across.unwrap_or_else(mirror_across);
            // a line already in the drawing, under the pointer: shown
            // mirrored across it at once, placed with a click (MU142,
            // Revit's Pick Axis)
            if across == Across::PickLine {
                let picked = map
                    .pick_line
                    .zip(ev.pointer)
                    .and_then(|(pick, p)| pick(p))
                    .and_then(|(a, b)| Some((a, (b - a).try_normalize()?)));
                // hovering only lights the line (a preview under the
                // pointer would be picked itself); the click mirrors
                if let Some((a, d)) = picked {
                    self.base = Some(a);
                    self.target = Some(a + d);
                    self.centred = Some((a, d, self.copy));
                    if ev.click || ev.enter {
                        let n = d.cross(self.normal()).normalize_or_zero();
                        if n.length() < 0.5 {
                            return Outcome::Running(
                                "Mirror: that line runs out of the plane; pick another.".into(),
                            );
                        }
                        if let Err(e) =
                            self.apply_with(doc, &|m, e| transform::mirrored(m, e, a, n))
                        {
                            self.shown = format!("Can't mirror that: {e}");
                            return Outcome::Running(self.shown.clone());
                        }
                        let what = match (&self.copies, self.copy) {
                            (Some(k), true) => k.label.replacen("Copy", "Mirror a copy of", 1),
                            _ => self.edit.label().to_string(),
                        };
                        self.shown = format!("{what}: across the line picked");
                        return self.place(doc);
                    }
                    return Outcome::Running(format!(
                        "{}: click to mirror across this line, Ctrl: {}, Esc cancels",
                        self.edit.label(),
                        if self.copy {
                            "keep only the mirrored one"
                        } else {
                            "keep the original too"
                        }
                    ));
                }
                self.centred = None;
                self.base = None;
                self.target = None;
                return Outcome::Running(format!(
                    "{}: point at the line to mirror across (a wall's face or edge, a grid or centre line, any line drawn), then click. Esc cancels.",
                    self.edit.label()
                ));
            }
            let line = centre_line(doc.model(), self.edit.originals(), self.pivot, across);
            match line {
                Some((c, d)) => return self.step_mirror_centre(doc, ev, c, d),
                None if self.centred.take().is_some() => {
                    // back to two points: put it as it was
                    self.show_nothing(doc);
                    self.target = None;
                }
                None => {}
            }
        }
        match self.stage {
            Stage::Base => {
                self.snap = ev.pointer.and_then(|p| (map.snap)(p, None)).map(|s| s.1);
                let picked = self
                    .snap
                    .map(|s| s.p)
                    .or_else(|| ev.pointer.and_then(|p| (map.to_model)(p, None, self.pivot)));
                if ev.click {
                    if let Some(b) = picked {
                        self.base = Some(b);
                        self.base_at = ev.pointer;
                        self.stage = if self.mode == Mode::Rotate {
                            Stage::Reference
                        } else {
                            Stage::Follow
                        };
                    }
                }
                let what = self.edit.label().to_string();
                Outcome::Running(match self.mode {
                    Mode::Mirror if self.stage == Stage::Follow => format!(
                        "{what}: now click the line's second point.{skipped}"
                    ),
                    Mode::Mirror => format!(
                        "{what}: click the first point of the line to mirror across{}. It snaps to ends, middles, crossings and grid lines. Esc cancels.{skipped}",
                        if self.tilted().is_some() { ", on the workplane" } else { "" }
                    ),
                    Mode::Rotate if self.stage == Stage::Reference => format!(
                        "{what}: click the direction to turn from (a reference point).{skipped}"
                    ),
                    Mode::Rotate => format!(
                        "{what}: click the centre to turn about. It snaps to ends, middles, crossings and grid lines. Esc cancels.{skipped}"
                    ),
                    _ if self.stage == Stage::Follow => {
                        format!("{what}: now move the pointer to where the base point goes.{skipped}")
                    }
                    _ => format!(
                        "{what}: click the base point, the point you move it by. It snaps to ends, middles, crossings, grid lines{}. Shift+click adds or takes away elements. Esc cancels.{skipped}",
                        if self.space == Space::Model { " and faces" } else { "" }
                    ),
                })
            }
            Stage::Reference => {
                let base = self.base.unwrap_or(self.pivot);
                self.snap = ev
                    .pointer
                    .and_then(|p| (map.snap)(p, Some(base)))
                    .map(|s| s.1);
                self.target = self.snap.map(|s| s.p);
                if ev.click {
                    if let Some(r) = self.target.filter(|r| self.flat(*r - base).length() > 1e-6) {
                        self.reference = Some(r);
                        self.base_at = ev.pointer;
                        self.stage = Stage::Follow;
                    }
                }
                Outcome::Running(format!(
                    "{}: click the direction to turn from (a reference point). Esc goes back to the centre.{skipped}",
                    self.edit.label()
                ))
            }
            Stage::Follow => self.step_follow(doc, ev, map),
        }
    }

    /// Can Ctrl leave copies behind: a Move of whole elements.
    fn copies_allowed(&self) -> bool {
        matches!(self.mode, Mode::Move | Mode::Mirror | Mode::Offset)
            && self.face.is_none()
            && !self.duplicated
            && self.works_on == Target::Object
    }

    /// Move on faces: light up the face under the pointer, take it with the
    /// base click, then push or pull it along its normal.
    fn step_faces(&mut self, doc: &mut Document, ev: &Events, map: &PaneMap) -> Outcome {
        let Some(pick_face) = map.pick_face else {
            return Outcome::Running(String::new());
        };
        if self.push.is_none() {
            self.stage = Stage::Base;
            let model = doc.model();
            let pull = self.mode == Mode::Pull;
            self.hover = ev.pointer.and_then(pick_face).filter(|h| {
                model.get(h.id).is_some_and(|e| {
                    if pull {
                        pushpull::can_pull(&e.kind)
                    } else {
                        pushpull::can_push(&e.kind)
                    }
                })
            });
            // the base point: a snap on the face, or where the pointer meets it
            self.snap = ev
                .pointer
                .and_then(|p| (map.snap)(p, None))
                .map(|s| s.1)
                .filter(|s| {
                    self.hover.as_ref().is_some_and(|h| {
                        (s.p - h.face.point)
                            .dot(h.face.normal.normalize_or_zero())
                            .abs()
                            < 1e-3
                    })
                });
            let under = ev.pointer.and_then(pick_face);
            if ev.click {
                if let Some(h) = self.hover.clone() {
                    let base = self.snap.map_or(h.face.point, |s| s.p);
                    let originals = pushpull::affected(model, h.id);
                    let name = originals.first().map_or(String::new(), |e| e.name.clone());
                    let verb = if pull { "Pull" } else { "Push/pull" };
                    // a drawn line that is not a closed outline: say what
                    // pulling it gives, before four walls with no ends
                    // are taken for a solid
                    let note = match originals.first().map(|e| &e.kind) {
                        Some(ElementKind::Sketch(sk)) if !sk.closed => {
                            "   ·   an open line: it becomes a surface, not a solid. Close it first (Close it, on the strip) for a solid".to_string()
                        }
                        Some(ElementKind::Sketch(sk)) if sk.wire => {
                            "   ·   a closed wire: it becomes walls, open at both ends. Make face (on the strip) gives a solid instead".to_string()
                        }
                        _ => String::new(),
                    };
                    self.push = Some(Push {
                        edit: LiveEdit::new(format!("{verb} {name}"), originals),
                        pick: h,
                        distance: 0.0,
                        note,
                    });
                    self.base = Some(base);
                    self.base_at = ev.pointer;
                    self.stage = Stage::Follow;
                    self.hover = None;
                    return Outcome::Running(format!(
                        "{verb} {name}: move the pointer out or in, or type a distance + Enter."
                    ));
                }
            }
            return Outcome::Running(match (&self.hover, under, pull) {
                (Some(h), _, true) => {
                    let e = model.get(h.id);
                    let name = e.map_or("", |e| e.name.as_str());
                    match e.map(|e| &e.kind) {
                        Some(ElementKind::Sketch(sk))
                            if !sk.closed =>
                        {
                            format!("Pull: {name} is an open line. Click it and it becomes a surface along it; close it first for a solid. Esc leaves the tool.")
                        }
                        Some(ElementKind::Sketch(sk)) if sk.wire => format!(
                            "Pull: {name} is a closed wire. Click it and it becomes walls all round; Make face, on the strip, gives a solid instead. Esc leaves the tool."
                        ),
                        Some(ElementKind::Sketch(_)) => format!(
                            "Pull: click this outline of {name}, then move out or in: it becomes a solid. Esc leaves the tool."
                        ),
                        _ => format!("Pull: click this face of {name}, then move out or in. Esc leaves the tool."),
                    }
                }
                (None, Some(u), true) => {
                    let name = model.get(u.id).map_or("", |e| e.name.as_str());
                    format!("Pull: {name} has no faces to pull; walls, slabs, roofs, columns, beams, stairs, railings, doors, windows and shapes do.")
                }
                (None, None, true) => {
                    "Pull: point at a face (in a plan or section, its edge) or at a drawn sketch in 3D, and click on it. A closed sketch becomes a solid, an open one a surface; a wall, a slab or a roof changes its own size. Esc leaves the tool.".into()
                }
                (Some(h), _, false) => {
                    let name = model.get(h.id).map_or("", |e| e.name.as_str());
                    format!(
                        "Move face: click a point on this face of {name} to push or pull it. F moves whole elements instead. Esc cancels."
                    )
                }
                (None, Some(u), false) => {
                    let name = model.get(u.id).map_or("", |e| e.name.as_str());
                    format!("Move face: {name} has no faces to push or pull; walls, slabs, roofs, columns, beams, stairs, railings, doors, windows, masses, extrusions and voids do. F moves whole elements.")
                }
                (None, None, false) => {
                    "Move face: point at a face of an element (in a plan or section, its edge), and click on it. F moves whole elements instead.".into()
                }
            });
        }
        // following: how far along its normal the face goes
        let base = self.base.expect("set with the push");
        let n = self
            .push
            .as_ref()
            .map_or(DVec3::Z, |p| p.pick.face.normal.normalize_or(DVec3::Z));
        let still = self
            .base_at
            .is_some_and(|b| b.distance(self.pointer) < STILL);
        // up and down on a plane facing the viewer; across on the level
        // (on a sloped or upright workplane, which a face's normal may cross,
        // always on the plane facing the viewer)
        // a corner, a middle, an edge or a face under the pointer sets how
        // far: the face stops level with it (MU45: a pull could not be
        // ended on the corner it was meant to reach)
        let _ps = crate::app::Perf::new("pull: snap");
        let across = !(n.z.abs() > 0.5 || self.tilted().is_some());
        // looked up once a step (it was looked up twice, MU46)
        let snapped = ((self.mode == Mode::Pull && !still) || across)
            .then(|| (map.snap)(self.pointer, Some(base)))
            .flatten();
        let level_with =
            snapped.filter(|(_, s)| self.mode == Mode::Pull && !still && on_something(s.kind));
        let (target, snap) = if let Some((p, s)) = level_with {
            (Some(p), Some(s))
        } else if !across {
            ((map.to_model)(self.pointer, Some(Axis::Z), base), None)
        } else {
            match snapped {
                Some((p, s)) => (Some(p), Some(s)),
                None => ((map.to_model)(self.pointer, None, base), None),
            }
        };
        drop(_ps);
        let _pc = crate::app::Perf::new("pull: change");
        self.snap = snap;
        let mut s = match target {
            _ if still => 0.0,
            Some(t) => (t - base).dot(n),
            None => self.push.as_ref().map_or(0.0, |p| p.distance),
        };
        if let Some(len) = parse_length(&self.typed) {
            // a typed distance goes the way the pointer went (out when it
            // hasn't moved); a minus sign pulls in
            s = if len < 0.0 || s >= 0.0 { len } else { -len };
        }
        self.delta = n * s;
        self.target = Some(base + self.delta);
        let p = self.push.as_mut().expect("checked above");
        p.distance = s;
        let label = p.edit.label().to_string();
        let way = if s < 0.0 { " in" } else { " out" };
        self.shown = format!("{label}: {}{way}", crate::units::len(s.abs()));
        let moved = if self.mode == Mode::Pull {
            pushpull::pull_with(
                doc.model(),
                p.edit.originals(),
                &p.pick.face,
                &also_picked(p.edit.originals(), &p.pick.face),
                s,
            )
        } else {
            pushpull::push_face(doc.model(), p.edit.originals(), &p.pick.face, s)
        };
        let result = match moved {
            Some(changed) => p.edit.update(doc, changed).map_err(|e| e.to_string()),
            None if s.abs() < 1e-9 => {
                p.edit.cancel(doc);
                Ok(())
            }
            None => Err("the shape would turn inside out".to_string()),
        };
        if let Err(e) = result {
            self.shown = format!("Can't go there: {e}");
        }
        if ev.click || ev.enter || self.ask == Some(true) {
            self.ask = None;
            let note = self.push.as_ref().map_or("", |p| p.note.as_str());
            let done = if s.abs() < 1e-9 {
                "Nothing moved: the face is where it was.".to_string()
            } else {
                format!("{}. Ctrl+Z to undo.{note}", self.shown)
            };
            return Outcome::Done(done);
        }
        let typed = if self.typed.is_empty() {
            String::new()
        } else {
            format!("  [typed {}]", self.typed)
        };
        let note = self.push.as_ref().map_or("", |p| p.note.as_str());
        Outcome::Running(format!(
            "{}{typed}   ·   click to place, type a distance + Enter, Esc steps back{note}",
            self.shown
        ))
    }

    fn skipped_note(&self) -> String {
        match self.skipped {
            0 => String::new(),
            1 => " (1 selected element can't take this and stays put)".into(),
            n => format!(" ({n} selected elements can't take this and stay put)"),
        }
    }

    /// A vector in the plane things turn in: plan for plans and 3D, the
    /// section plane for sections, as (across, up).
    fn flat(&self, v: DVec3) -> DVec2 {
        match self.space {
            Space::Section { start, end } => {
                DVec2::new(v.truncate().dot((end - start).normalize_or(DVec2::X)), v.z)
            }
            Space::Model if self.tilted().is_some() => {
                let [x, y] = self.pane_axes();
                DVec2::new(v.dot(x), v.dot(y))
            }
            Space::Plan | Space::Model => v.truncate(),
        }
    }

    /// Following the pointer from the base point.
    fn step_follow(&mut self, doc: &mut Document, ev: &Events, map: &PaneMap) -> Outcome {
        match self.mode {
            Mode::Mirror => return self.step_mirror(doc, ev, map),
            Mode::Offset => return self.step_offset(doc, ev, map),
            Mode::Grip => return self.step_grip(doc, ev, map),
            Mode::Inset => return self.step_inset(doc, ev, map),
            _ => {}
        }
        let skipped = self.skipped_note();
        // a key move starts from the point under the pointer, snapped
        if self.base.is_none() {
            // taken by the handle: from where it was pressed, as it is,
            // not from a snap near it
            let pressed = self.handle_press.take();
            let p = pressed.or(ev.pointer).unwrap_or(self.grab);
            let b = (map.snap)(p, None)
                .filter(|_| pressed.is_none())
                .map(|s| s.0)
                .or_else(|| (map.to_model)(p, None, self.pivot))
                .unwrap_or(self.pivot);
            self.base = Some(b);
            self.base_at = Some(p);
        }
        let base = self.base.expect("just set");
        let still = self
            .base_at
            .is_some_and(|b| b.distance(self.pointer) < STILL);
        // where the base point goes: snapped, or up and down along Z
        self.inferred = None;
        let mut axis = self.axis;
        let (target, snap) = match self.axis {
            Some(Axis::Z) if self.space == Space::Model => {
                // held to the vertical, something snapped to sets how high
                match (map.snap)(self.pointer, Some(base)).filter(|(_, s)| on_something(s.kind)) {
                    Some((p, s)) => (Some(DVec3::new(base.x, base.y, p.z)), Some(s)),
                    None => ((map.to_model)(self.pointer, Some(Axis::Z), base), None),
                }
            }
            _ => {
                let s = (map.snap)(self.pointer, Some(base));
                // a corner or a middle under the pointer takes the move;
                // on empty space, straight up or down from the base point
                // is up or down (it took the Z key, which nothing on the
                // screen said)
                let point = s.as_ref().is_some_and(|(_, s)| s.kind.is_point());
                let up = self.mode == Mode::Move
                    && self.axis.is_none()
                    && self.space == Space::Model
                    && self.tilted().is_none()
                    && !self.ortho
                    && !still
                    && !point
                    && runs_vertical(map, base, self.pointer);
                if up {
                    axis = Some(Axis::Z);
                    self.inferred = Some(Axis::Z);
                    ((map.to_model)(self.pointer, Some(Axis::Z), base), None)
                } else {
                    match s {
                        Some((p, s)) => {
                            // the axes the snaps draw through the base point
                            self.inferred = match s.kind {
                                SnapKind::AxisX => Some(Axis::X),
                                SnapKind::AxisY => Some(Axis::Y),
                                _ => None,
                            };
                            (Some(p), Some(s))
                        }
                        None => ((map.to_model)(self.pointer, self.axis, base), None),
                    }
                }
            }
        };
        // taken by the handle and caught by a snap onto something: the
        // handle's own point goes there, not the point pressed on its arrow
        // (along the axis it keeps to, if it keeps to one)
        let target = match (self.handle_origin, &snap, target) {
            (Some(o), Some(s), Some(t)) if on_something(s.kind) => {
                let off = base - o;
                Some(
                    t + match axis {
                        Some(a) => a.unit() * off.dot(a.unit()),
                        None => off,
                    },
                )
            }
            _ => target,
        };
        self.snap = snap;
        let typed = parse_length(&self.typed);
        let ortho = (self.ortho && self.axis.is_none()).then(|| self.pane_axes());
        let t = match self.mode {
            Mode::Move => {
                let fallback = self.pane_axes()[0];
                let d = match target {
                    _ if still && typed.is_none() => DVec3::ZERO,
                    Some(t) => {
                        // in 3D things slide on their level, or on a
                        // sloped or upright workplane
                        let on = self.tilted().filter(|_| axis.is_none());
                        let t = match on {
                            Some(p) => t - p.normal() * (t - base).dot(p.normal()),
                            None => t,
                        };
                        // on the level, unless what is snapped to is at
                        // another height: then it goes there, up or down
                        // (owner, 2026-11-12: it landed under the corner)
                        let lifted = snap.is_some_and(|s| on_something(s.kind));
                        let level = self.space == Space::Model && on.is_none() && !lifted;
                        move_delta(base, t, axis, ortho, typed, fallback, level)
                    }
                    None => self.delta,
                };
                let in_plane = self
                    .plane_delta(map, base, ev.shift)
                    .filter(|_| typed.is_none());
                let d = in_plane.unwrap_or(d);
                self.target = Some(base + d);
                self.delta = d;
                let axis = match (self.axis, self.inferred, ortho.is_some()) {
                    (Some(a), ..) => format!(" along {}", a.label()),
                    (None, Some(Axis::Z), _) if d.z < 0.0 => {
                        " down (the pointer is on the vertical: Z holds it)".into()
                    }
                    (None, Some(Axis::Z), _) => {
                        " up (the pointer is on the vertical: Z holds it)".into()
                    }
                    (None, Some(a), _) => format!(" along {} ({} holds it)", a.label(), a.label()),
                    (None, None, true) => " square".into(),
                    _ => String::new(),
                };
                let axis = match (in_plane, self.keep_plane) {
                    (Some(_), Some(n)) => format!(
                        " in {}",
                        Axis::ALL
                            .iter()
                            .filter(|a| **a != n)
                            .map(|a| a.label())
                            .collect::<String>()
                    ),
                    _ => axis,
                };
                let label = match &self.copies {
                    Some(c) if self.copy => c.label.clone(),
                    _ => self.edit.label().to_string(),
                };
                // a snap that took it off its level says how far, so a
                // wall is not lifted a storey unnoticed
                let off = self.space == Space::Model
                    && self.axis.is_none()
                    && self.inferred.is_none()
                    && d.z.abs() > 1e-6;
                let rise = if off {
                    format!(
                        ", {} {} to what it snapped to (X or Y keeps it on the level)",
                        crate::units::len(d.z.abs()),
                        if d.z > 0.0 { "up" } else { "down" }
                    )
                } else {
                    String::new()
                };
                self.shown = format!("{label}: {}{axis}{rise}", crate::units::len(d.length()));
                Xform::Move(d)
            }
            _ => {
                // a rotation from the reference direction to the pointer's
                let r = self.reference.unwrap_or(base + DVec3::X);
                let a0 = self.flat(r - base);
                let a1 = target.map_or(a0, |t| self.flat(t - base));
                let mut deg = a0.angle_to(a1).to_degrees();
                self.locked = None;
                if !snap.is_some_and(|s| s.kind.is_point()) {
                    // the same step and lock as the R key and the turn dot
                    // (MU142: the tool had neither)
                    let (d, locked) = turn_degrees(deg, ev.ctrl, 1.0, rotate_step());
                    deg = d;
                    self.locked = locked;
                }
                if still || !deg.is_finite() {
                    deg = 0.0;
                }
                self.swept = deg;
                if let Ok(v) = self.typed.trim().parse::<f64>() {
                    deg = v;
                }
                self.target = target;
                let lock = if self.locked.is_some() {
                    " (locked)"
                } else {
                    ""
                };
                self.shown = format!("{}: {deg:.1}°{lock}", self.edit.label());
                match self.space {
                    Space::Section { start, end } => Xform::Tilt {
                        center: base,
                        axis: -(end - start).normalize_or_zero().perp().extend(0.0),
                        degrees: deg,
                    },
                    _ if self.tilted().is_some() => Xform::Tilt {
                        center: base,
                        axis: self.normal(),
                        degrees: deg,
                    },
                    _ => Xform::Rotate {
                        center: base.truncate(),
                        degrees: deg,
                    },
                }
            }
        };
        if let Err(e) = self.apply(doc, &t) {
            self.shown = format!("Can't go there: {e}");
        }
        if ev.click || ev.enter || self.ask == Some(true) {
            self.ask = None;
            return self.place(doc);
        }
        let copy = if self.copies_allowed() {
            if self.copy {
                ", Ctrl: copy is on"
            } else {
                ", Ctrl: copy"
            }
        } else {
            ""
        };
        let keys = match self.mode {
            Mode::Move if self.tilted().is_some() => {
                "type a distance + Enter, it slides on the workplane, Shift keeps square to it, X, Y or Z keeps to an axis"
            }
            Mode::Move => "type a distance + Enter, Shift keeps square, X, Y or Z keeps to an axis",
            _ => "type an angle + Enter, Ctrl snaps to 15°",
        };
        let snapped = match snap.map(|s| s.kind) {
            Some(k) if k != SnapKind::Grid => format!(" ({})", k.label().to_lowercase()),
            _ => String::new(),
        };
        let typed = if self.typed.is_empty() {
            String::new()
        } else {
            format!("  [typed {}]", self.typed)
        };
        Outcome::Running(format!(
            "{}{typed}{snapped}{skipped}   ·   click to place, {keys}{copy}, Esc steps back",
            self.shown
        ))
    }

    /// Leave things where they are now: the end of the edit, or one more
    /// copy placed.
    fn place(&mut self, doc: &mut Document) -> Outcome {
        if matches!(self.mode, Mode::Mirror | Mode::Offset) {
            let placed = match &mut self.copies {
                Some(c) if self.copy => std::mem::replace(&mut c.applied, false),
                _ => self.edit.applied(),
            };
            if !placed {
                return Outcome::Running(format!(
                    "{}: nothing to place yet. Move the pointer, or Esc to stop.",
                    self.mode.verb()
                ));
            }
            return Outcome::Done(format!(
                "{}{}. Ctrl+Z to undo.",
                self.shown,
                self.skipped_note()
            ));
        }
        if self.copy {
            if let Some(c) = &mut self.copies {
                if c.applied {
                    c.applied = false;
                    self.placed += 1;
                    // the next copy has ids of its own
                    self.copies = CopyPreview::new(doc, self.edit.originals());
                    self.typed.clear();
                    return Outcome::Running(format!(
                        "{} placed. Click to place another, Esc to finish. Ctrl+Z undoes one at a time.",
                        match self.placed {
                            1 => "1 copy".to_string(),
                            n => format!("{n} copies"),
                        }
                    ));
                }
            }
            return self.finish(doc);
        }
        let done = format!("{}{}. Ctrl+Z to undo.", self.shown, self.skipped_note());
        Outcome::Done(done)
    }

    /// Done with copies, or cancelled.
    fn finish(&mut self, doc: &mut Document) -> Outcome {
        if self.placed > 0 {
            self.edit.cancel(doc);
            if let Some(c) = &mut self.copies {
                c.cancel(doc);
            }
            Outcome::Done(format!(
                "Placed {} {}. Ctrl+Z undoes them one at a time.",
                self.placed,
                if self.placed == 1 { "copy" } else { "copies" }
            ))
        } else {
            self.cancel(doc);
            Outcome::Cancelled(format!("{} cancelled.", self.mode.verb()))
        }
    }

    /// Esc or right-click: take back the last click, or stop. `None` while
    /// the edit goes on.
    fn back(&mut self, doc: &mut Document) -> Option<Outcome> {
        if self.flow == Flow::Keys
            || !self.uses_points()
            || matches!(self.mode, Mode::Offset | Mode::Grip | Mode::Inset)
        {
            return Some(self.finish(doc));
        }
        match self.stage {
            Stage::Follow => {
                self.edit.cancel(doc);
                if let Some(c) = &mut self.copies {
                    c.cancel(doc);
                }
                if let Some(mut p) = self.push.take() {
                    p.edit.cancel(doc);
                }
                self.typed.clear();
                self.delta = DVec3::ZERO;
                self.target = None;
                if self.mode == Mode::Rotate {
                    self.reference = None;
                    self.stage = Stage::Reference;
                } else {
                    self.base = None;
                    self.stage = Stage::Base;
                }
                None
            }
            Stage::Reference => {
                self.base = None;
                self.stage = Stage::Base;
                None
            }
            Stage::Base => Some(self.finish(doc)),
        }
    }

    /// Turn copying on or off, putting back what the other way showed.
    pub fn set_copy(&mut self, doc: &mut Document, on: bool) {
        if on == self.copy {
            return;
        }
        self.offset_shown = None;
        if on {
            self.edit.cancel(doc);
            if self.copies.is_none() {
                self.copies = CopyPreview::new(doc, self.edit.originals());
            }
            self.copy = self.copies.is_some();
        } else {
            if let Some(c) = &mut self.copies {
                c.cancel(doc);
            }
            self.copy = false;
        }
    }

    /// The Scale tool before its first click: nothing has changed yet.
    pub fn waits_to_scale(&self) -> bool {
        self.mode == Mode::Scale
            && self.flow == Flow::Tool
            && !self.scale.started
            && self.face.is_none()
            && self.verts.is_none()
            && !scale_by_ref()
    }

    /// Keyboard rotate and scale: by how far the pointer went.
    fn step_relative(&mut self, doc: &mut Document, ev: &Events, map: &PaneMap) -> Outcome {
        self.scale.alt = ev.alt;
        // Scale by reference: first the two ends of a length known (MU142)
        if self.mode == Mode::Scale
            && scale_by_ref()
            && self.face.is_none()
            && self.verts.is_none()
            && self.scale.refs.len() < 2
        {
            let from = self.scale.refs.first().copied();
            if let (true, Some(p)) = (ev.click, ev.pointer) {
                let at = (map.snap)(p, from)
                    .map(|(q, _)| q)
                    .or_else(|| (map.to_model)(p, None, self.scale.centre));
                if let Some(q) = at {
                    self.scale.refs.push(q);
                    if self.scale.refs.len() == 2 {
                        self.grab = p;
                        self.typed.clear();
                    }
                }
            }
            let ask = if self.scale.refs.is_empty() {
                "click one end of a length you know"
            } else {
                "click its other end"
            };
            self.shown = format!("{} by reference: {ask}", self.edit.label);
            return Outcome::Running(format!("{}   ·   Esc to cancel", self.shown));
        }
        // the Scale tool: first a click where it starts, a corner of it
        // most often, so the pointer has room to go out as well as in;
        // until then nothing changes (MU142, owner: "it activates it
        // immediately ... doesn't give you time to click somewhere where
        // you want to start the scaling process")
        if self.waits_to_scale() {
            if let (true, Some(p)) = (ev.click, ev.pointer) {
                self.grab = p;
                self.pointer = p;
                self.scale.started = true;
                self.scale.corner = None;
                self.typed.clear();
                self.shown = format!(
                    "{}: move the pointer out to make it bigger, in to make it smaller",
                    self.edit.label
                );
            } else {
                self.shown = format!(
                    "{}: click where it starts, a corner most often (the opposite corner stays; Alt for its centre)",
                    self.edit.label
                );
            }
            return Outcome::Running(format!("{}   ·   Esc to cancel", self.shown));
        }
        // the handle's coarser steps are on Shift; the keys' on Ctrl
        let coarse = if self.from_handle { ev.shift } else { ev.ctrl };
        let xform = self.xform(map, coarse);
        let result = match xform {
            Some(t) => self.apply(doc, &t),
            None => Ok(()),
        };
        if let Err(e) = result {
            self.shown = format!("Can't go there: {e}");
        }
        let skipped = self.skipped_note();
        if ev.enter || ev.click || self.ask == Some(true) {
            let done = format!("{}{skipped}. Ctrl+Z to undo.", self.shown);
            return Outcome::Done(done);
        }
        let keys = match self.mode {
            Mode::Move => "X, Y or Z keeps to an axis",
            Mode::Rotate => "X or Y tilts about that axis",
            Mode::Scale => "X, Y or Z scales along one axis",
            Mode::Mirror | Mode::Offset | Mode::Grip | Mode::Pull | Mode::Inset => "",
        };
        Outcome::Running(format!(
            "{}{skipped}   ·   {keys}, type a value, click or Enter to finish, Esc to cancel{}",
            self.shown,
            if self.mode == Mode::Rotate {
                ", Ctrl snaps to 15°"
            } else {
                ", Ctrl snaps"
            }
        ))
    }

    /// The bar while the edit runs, for working without the keyboard
    /// (MU38): one slim row, as Archicad's and SketchUp's: what it does,
    /// object or face, the axis chips in their colours, the value, copies,
    /// then finish and cancel. Over the top of the view, or by the pointer
    /// (Settings).
    pub fn panel(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        let by_pointer = BAR_BY_POINTER.load(std::sync::atomic::Ordering::Relaxed);
        if by_pointer && self.anchor.is_none() {
            self.anchor = ctx.pointer_hover_pos().filter(|p| rect.contains(*p));
        }
        let at = match (by_pointer, self.anchor) {
            (true, Some(a)) => (a + egui::vec2(22.0, 22.0), egui::Align2::LEFT_TOP),
            _ => (
                egui::pos2(rect.center().x, rect.top() + 10.0),
                egui::Align2::CENTER_TOP,
            ),
        };
        let p = crate::theme::Palette::of(ctx);
        // Offset says how far by the pointer, as it goes (MU47)
        if let (Mode::Offset, Some((by, _, _)), Some(at)) = (
            self.mode,
            self.offset_shown,
            ctx.pointer_hover_pos().filter(|p| rect.contains(*p)),
        ) {
            let text = if self.typed.is_empty() {
                crate::units::len(by.abs())
            } else {
                format!("{} (typed)", crate::units::len(by.abs()))
            };
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("offset distance"),
            ));
            let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.0), p.text);
            let pill = egui::Rect::from_min_size(
                at + egui::vec2(16.0, -26.0),
                galley.size() + egui::vec2(12.0, 6.0),
            );
            let fill = ctx.global_style().visuals.window_fill;
            painter.rect_filled(pill, 9.0, fill);
            painter.rect_stroke(
                pill,
                9.0,
                egui::Stroke::new(1.0, p.accent),
                egui::StrokeKind::Inside,
            );
            painter.galley(pill.min + egui::vec2(6.0, 3.0), galley, p.text);
        }
        if let (Some(run), Some(at)) = (
            self.inset.as_ref().filter(|r| !r.label.is_empty()),
            ctx.pointer_hover_pos().filter(|p| rect.contains(*p)),
        ) {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("inset value"),
            ));
            let galley =
                painter.layout_no_wrap(run.label.clone(), egui::FontId::proportional(12.0), p.text);
            let pill = egui::Rect::from_min_size(
                at + egui::vec2(16.0, -26.0),
                galley.size() + egui::vec2(12.0, 6.0),
            );
            painter.rect_filled(pill, 9.0, ctx.global_style().visuals.window_fill);
            painter.rect_stroke(
                pill,
                9.0,
                egui::Stroke::new(1.0, p.accent),
                egui::StrokeKind::Inside,
            );
            painter.galley(pill.min + egui::vec2(6.0, 3.0), galley, p.text);
        }
        egui::Area::new(egui::Id::new("modal_panel"))
            .order(crate::app::overlay_order())
            .pivot(at.1)
            .fixed_pos(at.0 + bar_offset(ctx))
            // clear of the view's zoom and fit column at its right (MU39)
            .constrain_to(egui::Rect::from_min_max(
                rect.min + egui::vec2(6.0, 6.0),
                rect.max - egui::vec2(56.0, 6.0),
            ))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(ui.visuals().window_fill)
                    .stroke(egui::Stroke::new(1.0, p.border))
                    .corner_radius(if by_pointer { 10.0 } else { 18.0 })
                    .shadow(ui.visuals().popup_shadow)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.set_max_width((rect.width() - 80.0).max(160.0));
                        ui.horizontal_wrapped(|ui| self.bar_row(ui, by_pointer, &p));
                    });
            });
    }

    fn bar_row(&mut self, ui: &mut egui::Ui, compact: bool, p: &crate::theme::Palette) {
        ui.spacing_mut().item_spacing.x = 3.0;
        // a grip to carry the bar out of the way (MU39); double-click
        // puts it back
        let (gr, grip) =
            ui.allocate_exact_size(egui::vec2(10.0, 20.0), egui::Sense::click_and_drag());
        let dot = if grip.hovered() || grip.dragged() {
            p.text
        } else {
            p.muted
        };
        for (x, y) in [
            (-2.0, -5.0),
            (2.0, -5.0),
            (-2.0, 0.0),
            (2.0, 0.0),
            (-2.0, 5.0),
            (2.0, 5.0),
        ] {
            ui.painter()
                .circle_filled(gr.center() + egui::vec2(x, y), 1.2, dot);
        }
        let key = egui::Id::new("modal_bar_offset");
        if grip.dragged() {
            let d = grip.drag_delta();
            ui.ctx()
                .data_mut(|m| *m.get_persisted_mut_or_default::<egui::Vec2>(key) += d);
        }
        if grip.double_clicked() {
            ui.ctx()
                .data_mut(|m| m.insert_persisted(key, egui::Vec2::ZERO));
        }
        grip.on_hover_cursor(egui::CursorIcon::Grab)
            .on_hover_text("Drag to move the bar; double-click puts it back");
        let icon = match self.mode {
            Mode::Move => "move",
            Mode::Rotate => "rotate-cw",
            Mode::Scale => "scaling",
            Mode::Mirror => "mirror",
            Mode::Offset => "offset",
            Mode::Grip => "maximize-2",
            Mode::Inset => "shape-inset",
            Mode::Pull => "pull",
        };
        let (ir, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
        crate::svgicon::paint(ui.painter(), ir, icon, p.accent);
        if !compact {
            ui.label(egui::RichText::new(self.mode.verb()).strong());
        }
        let muted = p.muted;
        let border = p.border;
        // a chip: on, it takes the selection's fill (or its axis colour)
        let chip =
            |ui: &mut egui::Ui, on: bool, text: &str, colour: Option<egui::Color32>, tip: &str| {
                let fill = match (on, colour) {
                    (true, Some(c)) => c.gamma_multiply(0.9),
                    (true, None) => ui.visuals().selection.bg_fill,
                    _ => egui::Color32::TRANSPARENT,
                };
                let ink = match (on, colour) {
                    (true, _) => egui::Color32::WHITE,
                    (false, Some(c)) => c,
                    _ => ui.visuals().text_color(),
                };
                ui.add(
                    egui::Button::new(egui::RichText::new(text).color(ink))
                        .fill(fill)
                        .corner_radius(9.0)
                        .min_size(egui::vec2(24.0, 20.0)),
                )
                .on_hover_text(tip)
                .clicked()
            };
        let icon_chip = |ui: &mut egui::Ui, on: bool, icon: &str, tip: &str| {
            let (r, resp) = ui.allocate_exact_size(egui::vec2(24.0, 20.0), egui::Sense::click());
            if on {
                ui.painter()
                    .rect_filled(r, 9.0, ui.visuals().selection.bg_fill);
            } else if resp.hovered() {
                ui.painter()
                    .rect_filled(r, 9.0, ui.visuals().widgets.hovered.weak_bg_fill);
            }
            let ink = if on { egui::Color32::WHITE } else { muted };
            let ir = egui::Rect::from_center_size(r.center(), egui::vec2(14.0, 14.0));
            crate::svgicon::paint(ui.painter(), ir, icon, ink);
            resp.on_hover_text(tip).clicked()
        };
        let sep = |ui: &mut egui::Ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(7.0, 18.0), egui::Sense::hover());
            ui.painter().line_segment(
                [r.center_top(), r.center_bottom()],
                egui::Stroke::new(1.0, border),
            );
        };
        sep(ui);
        if self.can_take_faces() {
            if icon_chip(
                ui,
                self.works_on == Target::Object,
                "box",
                "Object: move whole elements (F switches)",
            ) {
                self.want_works_on = Some(Target::Object);
            }
            if icon_chip(ui, self.works_on == Target::Face, "square", "Face: push or pull the face under the pointer, on walls, slabs and masses (F switches)") {
                self.want_works_on = Some(Target::Face);
            }
            sep(ui);
        }
        let faces =
            self.mode == Mode::Pull || (self.works_on == Target::Face && self.can_take_faces());
        let axes: &[Axis] = match (self.mode, self.face.is_some()) {
            _ if faces => &[],
            (Mode::Mirror | Mode::Offset | Mode::Grip | Mode::Inset, _) => &[],
            (Mode::Rotate, true) => &[],
            _ => &Axis::ALL,
        };
        if !axes.is_empty() {
            if chip(
                ui,
                self.axis.is_none(),
                "Free",
                None,
                "Free: any direction, snapping as it goes",
            ) {
                self.axis = None;
            }
            for a in axes {
                let (name, tip) = match (self.mode, a) {
                    (Mode::Rotate, Axis::X) => ("X", "Tilt about X"),
                    (Mode::Rotate, Axis::Y) => ("Y", "Tilt about Y"),
                    (Mode::Rotate, Axis::Z) => ("Z", "Turn about Z (upright)"),
                    (_, Axis::X) => ("X", "Along X only (X)"),
                    (_, Axis::Y) => ("Y", "Along Y only (Y)"),
                    (_, Axis::Z) => ("Z", "Up and down only (Z)"),
                };
                let colour = axis_colour(*a);
                // lit while held, and while the pointer runs along it
                let on =
                    self.axis == Some(*a) || (self.axis.is_none() && self.inferred == Some(*a));
                if chip(ui, on, name, Some(colour), tip) {
                    self.toggle(*a);
                }
            }
            sep(ui);
        }
        let what = match self.mode {
            Mode::Move | Mode::Offset | Mode::Grip | Mode::Pull => "Distance",
            Mode::Inset => "Thickness",
            Mode::Rotate | Mode::Mirror => "Angle",
            Mode::Scale if scale_by_ref() => "True length",
            Mode::Scale => "Factor",
        };
        let letter = &what[..1];
        // a help mark, not a button: it lights as the ? marks do (MU39)
        let (br, badge) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
        let lit = badge.hovered();
        if lit {
            ui.painter()
                .circle_filled(br.center(), 8.0, p.accent.gamma_multiply(0.25));
        }
        let ink = if lit { p.accent } else { muted };
        ui.painter()
            .circle_stroke(br.center(), 8.0, egui::Stroke::new(1.0, ink));
        ui.painter().text(
            br.center(),
            egui::Align2::CENTER_CENTER,
            letter,
            egui::FontId::proportional(10.5),
            ink,
        );
        badge.on_hover_text(match self.mode {
            Mode::Move | Mode::Offset | Mode::Grip | Mode::Pull => {
                "Distance: type how far, then Enter"
            }
            Mode::Inset => "Thickness: type how far in from the edges, then Enter",
            Mode::Rotate | Mode::Mirror => "Angle: type how far it turns, then Enter",
            Mode::Scale if scale_by_ref() => {
                "True length: type what the length clicked should be, then Enter"
            }
            Mode::Scale => "Factor: type how much bigger (2) or smaller (0.5), then Enter",
        });
        // while it follows the pointer, the field shows how far it has
        // gone; typing replaces it (MU71: only the pill next to the
        // pointer said it)
        let live = (self.stage == Stage::Follow)
            .then(|| match self.mode {
                Mode::Pull => self
                    .push
                    .as_ref()
                    .map(|q| crate::units::len(q.distance.abs())),
                Mode::Move | Mode::Offset | Mode::Grip => {
                    Some(crate::units::len(self.delta.length()))
                }
                _ => None,
            })
            .flatten();
        let hint = match live {
            Some(v) => egui::RichText::new(v).color(ui.visuals().text_color()),
            None => egui::RichText::new("type"),
        };
        crate::props::text_field(
            ui,
            egui::TextEdit::singleline(&mut self.typed).hint_text(hint),
            56.0,
        )
        .on_hover_text(format!("{what}: type it and press Enter"));
        if let Some(run) = &mut self.inset {
            // a second value: how far the new face sinks (a minus sign) or
            // rises. Ctrl and the pointer set it too
            let (dr, depth) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
            let ink = if depth.hovered() { p.accent } else { muted };
            ui.painter()
                .circle_stroke(dr.center(), 8.0, egui::Stroke::new(1.0, ink));
            ui.painter().text(
                dr.center(),
                egui::Align2::CENTER_CENTER,
                "D",
                egui::FontId::proportional(10.5),
                ink,
            );
            depth.on_hover_text(
                "Depth: how far the new face sinks (-) or rises; or hold Ctrl and move",
            );
            ui.add(
                egui::TextEdit::singleline(&mut run.depth_typed)
                    .vertical_align(egui::Align::Center)
                    .desired_width(56.0)
                    .hint_text("Ctrl"),
            )
            .on_hover_text(
                "Depth: type it (a minus sign sinks the face), or hold Ctrl and move the pointer",
            );
            if chip(
                ui,
                run.out,
                "Outset",
                None,
                "Outset (O): a rim around the face, cut into its sides; the face keeps its size",
            ) {
                run.out = !run.out;
                run.bias = 0.0;
            }
        }
        if self.mode == Mode::Rotate && (!self.uses_points() || self.stage == Stage::Follow) {
            // the three turns people make most, in one click (MU142),
            // once there is a centre to turn about
            let mut turn = None;
            if icon_chip(ui, false, "rotate-ccw", "Turn 90° anticlockwise and finish") {
                turn = Some("90");
            }
            if chip(ui, false, "180°", None, "Turn 180° and finish") {
                turn = Some("180");
            }
            if icon_chip(ui, false, "rotate-cw", "Turn 90° clockwise and finish") {
                turn = Some("-90");
            }
            if let Some(by) = turn {
                self.typed = by.into();
                self.ask = Some(true);
            }
        }
        if self.copies_allowed() {
            let tip = match self.mode {
                Mode::Move => "Copy (Ctrl): leave copies behind, each click places one",
                _ => "Copy (Ctrl): make a copy and keep the original",
            };
            if icon_chip(ui, self.copy, "copy", tip) {
                if self.mode == Mode::Mirror {
                    // Mirror's tick is remembered, as its row's is
                    set_mirror_copy(!self.copy);
                }
                self.want_copy = Some(!self.copy);
            }
        }
        sep(ui);
        let ready = self.stage == Stage::Follow;
        let (r, resp) = ui.allocate_exact_size(egui::vec2(24.0, 20.0), egui::Sense::click());
        let fill = if ready {
            p.accent
        } else {
            p.accent.gamma_multiply(0.35)
        };
        ui.painter().rect_filled(r, 9.0, fill);
        let ir = egui::Rect::from_center_size(r.center(), egui::vec2(14.0, 14.0));
        crate::svgicon::paint(ui.painter(), ir, "check", egui::Color32::WHITE);
        if resp.on_hover_text("Finish (Enter)").clicked() && ready {
            self.ask = Some(true);
        }
        if icon_chip(ui, false, "x", "Cancel (Esc)") {
            self.ask = Some(false);
        }
    }

    /// Work on mass `id`'s vertices `vs` only, turning and scaling about
    /// their middle (MU39).
    pub fn set_verts(&mut self, doc: &Document, id: ElementId, vs: Vec<u32>) {
        let model = doc.model();
        if let Some(ElementKind::FreeMesh(m)) = model.get(id).map(|e| &e.kind) {
            let z = m
                .level
                .and_then(|l| model.level(l))
                .map_or(0.0, |l| l.elevation);
            let n = vs.len().max(1) as f64;
            let c = vs
                .iter()
                .map(|v| m.mesh.vertices[*v as usize])
                .sum::<DVec3>()
                / n;
            self.pivot = m.origin + m.to_model(c) + DVec3::Z * z;
            self.verts = Some(vs);
        }
    }

    /// Move only what a stretch box took (Stretch): one undo step,
    /// "Stretch".
    pub fn set_stretch(&mut self, parts: Vec<(ElementId, crate::stretchtool::Part)>) {
        if self.mode != Mode::Move || parts.is_empty() {
            return;
        }
        let originals = self.edit.originals().to_vec();
        self.edit = LiveEdit::new("Stretch", originals);
        self.stretch = Some(parts);
    }

    /// Work on corners `cs` of outline `id` only (Move).
    pub fn set_corners(
        &mut self,
        doc: &Document,
        id: ElementId,
        cs: Vec<(crate::handles::Ring, usize)>,
    ) {
        if self.mode != Mode::Move {
            return;
        }
        let model = doc.model();
        let Some(e) = model.get(id) else {
            return;
        };
        let at: Vec<DVec3> = cs
            .iter()
            .filter_map(|(ring, i)| {
                crate::editmode::ring_of(model, e, *ring)?
                    .corners
                    .get(*i)
                    .copied()
            })
            .collect();
        if at.len() != cs.len() || at.is_empty() {
            return;
        }
        self.pivot = at.iter().sum::<DVec3>() / at.len() as f64;
        self.corners = Some(cs);
    }

    fn toggle(&mut self, a: Axis) {
        if self.mode == Mode::Rotate && self.face.is_some() {
            return;
        }
        self.axis = if self.axis == Some(a) { None } else { Some(a) };
    }

    /// Undo whatever this edit has done, keeping a duplicate's copy: for
    /// the tool started again on a changed selection.
    pub fn undo_edit(&mut self, doc: &mut Document) {
        self.edit.cancel(doc);
        if let Some(c) = &mut self.copies {
            c.cancel(doc);
        }
        if let Some(p) = &mut self.push {
            p.edit.cancel(doc);
        }
    }

    /// Undo everything, a duplicate's copy too (MU23): a copy cancelled
    /// before it was placed would sit unseen on its original, and the
    /// next Duplicate stacks another one on it.
    pub fn cancel(&mut self, doc: &mut Document) {
        self.undo_edit(doc);
        if self.duplicated && doc.undo_label().is_some_and(|l| l.starts_with("Duplicate")) {
            doc.discard_last();
            self.duplicated = false;
        }
    }

    /// As [`Modal::cancel`]: an edit abandoned before it began.
    pub fn discard(&mut self, doc: &mut Document) {
        self.cancel(doc);
    }

    /// The turn axis for a rotate: about the pane's view in plans and
    /// sections, about the vertical in 3D, or about a chosen axis.
    fn turn_axis(&self) -> Option<DVec3> {
        if let (None, Some(p)) = (self.axis, self.tilted()) {
            return Some(p.normal());
        }
        match (self.axis, self.space) {
            (Some(Axis::Z), _) | (None, Space::Model | Space::Plan) => None,
            (Some(a), _) => Some(a.unit()),
            (None, Space::Section { start, end }) => {
                Some(-(end - start).normalize_or_zero().perp().extend(0.0))
            }
        }
    }

    fn xform(&mut self, map: &PaneMap, ctrl: bool) -> Option<Xform> {
        // a move is typed as a length in the project's units; turns and
        // scales as plain numbers
        let typed: Option<f64> = match self.mode {
            Mode::Move => parse_length(&self.typed),
            _ => self.typed.trim().parse().ok(),
        };
        let snap = |v: f64, step: f64| (v / step).round() * step;
        let ps = (map.project)(self.pivot).unwrap_or(self.grab);
        Some(match self.mode {
            Mode::Move => {
                let d = match typed {
                    Some(v) => {
                        let along = match (self.axis, self.space) {
                            (Some(a), _) => a.unit(),
                            (None, Space::Section { start, end }) => {
                                (end - start).normalize_or_zero().extend(0.0)
                            }
                            (None, _) => DVec3::X,
                        };
                        along * v
                    }
                    None => {
                        // measure the pointer's travel at the pivot, not where
                        // the pointer happened to be when the move began: a
                        // move started from a button begins wherever the
                        // pointer comes into the view, often near the horizon
                        let (from, to) = match (map.project)(self.pivot) {
                            Some(p) => (p, p + (self.pointer - self.grab)),
                            None => (self.grab, self.pointer),
                        };
                        let a = (map.to_model)(from, self.axis, self.pivot)?;
                        let b = (map.to_model)(to, self.axis, self.pivot)?;
                        let mut d = b - a;
                        if let Some(ax) = self.axis {
                            d = ax.unit() * d.dot(ax.unit());
                        }
                        let step = if ctrl { 0.25 } else { 0.01 };
                        DVec3::new(snap(d.x, step), snap(d.y, step), snap(d.z, step))
                    }
                };
                let axis = self
                    .axis
                    .map_or(String::new(), |a| format!(" along {}", a.label()));
                self.shown = format!(
                    "{}: {}{axis}",
                    self.edit.label,
                    crate::units::len(d.length())
                );
                Xform::Move(d)
            }
            Mode::Rotate => {
                let angle = |p: Pos2| (-(p.y - ps.y)).atan2(p.x - ps.x) as f64;
                let turn = self.turn_axis();
                let degrees = match typed {
                    Some(v) => v,
                    None => {
                        let d = (angle(self.pointer) - angle(self.grab)).to_degrees();
                        let d = (d + 180.0).rem_euclid(360.0) - 180.0;
                        // turning on screen anticlockwise turns about the
                        // axis that points at the viewer
                        let toward = match turn {
                            Some(a) if a.dot(-map.look) < 0.0 => -1.0,
                            _ => 1.0,
                        };
                        let fine = if self.from_handle { 5.0 } else { 1.0 };
                        let (d, locked) = turn_degrees(d, ctrl, fine, rotate_step());
                        self.locked = locked;
                        self.swept = d;
                        d * toward
                    }
                };
                match turn {
                    Some(axis) => {
                        let about = self
                            .axis
                            .map_or(String::new(), |a| format!(" about {}", a.label()));
                        self.shown = format!("{}: {degrees:.1}°{about}", self.edit.label);
                        Xform::Tilt {
                            center: self.pivot,
                            axis,
                            degrees,
                        }
                    }
                    None => {
                        self.shown = format!("{}: {degrees:.1}°", self.edit.label);
                        Xform::Rotate {
                            center: self.pivot.truncate(),
                            degrees,
                        }
                    }
                }
            }
            Mode::Mirror | Mode::Offset | Mode::Grip | Mode::Pull | Mode::Inset => return None,
            Mode::Scale => {
                // the step its factor goes in, from its row; Ctrl a tenth
                let step = if ctrl {
                    0.1
                } else {
                    match scale_step() {
                        s if s > 0.0 => s,
                        _ => 0.01,
                    }
                };
                let whole = self.face.is_none() && self.verts.is_none();
                // by a reference: the length known, from its first end
                if whole && scale_by_ref() {
                    let [a, b] = self.scale.refs[..] else {
                        return None;
                    };
                    let known = a.truncate().distance(b.truncate()).max(1e-9);
                    self.pivot = a;
                    let factor = match self.typed.trim() {
                        "" => {
                            let p = (map.to_model)(self.pointer, None, a)?;
                            snap(p.truncate().distance(a.truncate()) / known, step).max(0.01)
                        }
                        t => parse_length(t).filter(|v| *v > 0.0)? / known,
                    };
                    self.scale.factor = DVec3::new(factor, factor, factor);
                    self.shown = format!(
                        "{}: × {factor:.3}, {} to {}",
                        self.edit.label,
                        crate::units::len(known),
                        crate::units::len(known * factor)
                    );
                    return Some(Xform::Scale { center: a, factor });
                }
                // about the corner opposite the one it was taken from, or
                // its centre with Alt or when its row says so
                let corner =
                    whole && scale_corner() && !self.scale.alt && self.scale.extent.is_some();
                if corner && self.scale.corner.is_none() {
                    let (lo, hi) = self.scale.extent?;
                    let g = (map.to_model)(self.grab, None, self.scale.centre)?.truncate();
                    let near = DVec2::new(
                        if (g.x - lo.x).abs() < (g.x - hi.x).abs() {
                            lo.x
                        } else {
                            hi.x
                        },
                        if (g.y - lo.y).abs() < (g.y - hi.y).abs() {
                            lo.y
                        } else {
                            hi.y
                        },
                    );
                    let far = lo + hi - near;
                    self.scale.corner = Some(far.extend(self.scale.centre.z));
                }
                if whole {
                    self.pivot = match (corner, self.scale.corner) {
                        (true, Some(c)) => c,
                        _ => self.scale.centre,
                    };
                }
                let ps = (map.project)(self.pivot)?;
                // proportions free: along x and y each, as far as the pointer went
                if whole && scale_free() && self.axis.is_none() && typed.is_none() {
                    let g = (map.to_model)(self.grab, None, self.pivot)?;
                    let p = (map.to_model)(self.pointer, None, self.pivot)?;
                    let along = |k: usize| {
                        let d = g[k] - self.pivot[k];
                        if d.abs() < 1e-6 {
                            1.0
                        } else {
                            snap((p[k] - self.pivot[k]) / d, step).max(0.01)
                        }
                    };
                    let f = DVec3::new(along(0), along(1), 1.0);
                    self.scale.factor = f;
                    self.shown = format!("{}: × {:.2} by × {:.2}", self.edit.label, f.x, f.y);
                    return Some(Xform::Stretch {
                        center: self.pivot,
                        factor: f,
                    });
                }
                let factor = match typed {
                    Some(v) if v != 0.0 => v,
                    _ => {
                        let r0 = self.grab.distance(ps).max(4.0) as f64;
                        let r1 = self.pointer.distance(ps) as f64;
                        snap(r1 / r0, step).max(0.01)
                    }
                };
                self.scale.factor = DVec3::splat(factor);
                match self.axis.filter(|_| self.face.is_none()) {
                    Some(a) => {
                        self.shown =
                            format!("{}: × {factor:.2} along {}", self.edit.label, a.label());
                        let mut f = DVec3::ONE;
                        f[a.index()] = factor;
                        Xform::Stretch {
                            center: self.pivot,
                            factor: f,
                        }
                    }
                    None => {
                        self.shown = format!("{}: × {factor:.2}", self.edit.label);
                        Xform::Scale {
                            center: self.pivot,
                            factor,
                        }
                    }
                }
            }
        })
    }

    /// The same change about another centre, for individual origins.
    fn about(t: &Xform, c: DVec3) -> Xform {
        match *t {
            Xform::Move(d) => Xform::Move(d),
            Xform::Rotate { degrees, .. } => Xform::Rotate {
                center: c.truncate(),
                degrees,
            },
            Xform::Tilt { axis, degrees, .. } => Xform::Tilt {
                center: c,
                axis,
                degrees,
            },
            Xform::Scale { factor, .. } => Xform::Scale { center: c, factor },
            Xform::Stretch { factor, .. } => Xform::Stretch { center: c, factor },
        }
    }

    fn apply(&mut self, doc: &mut Document, t: &Xform) -> Result<(), String> {
        if let (true, Some(c), Xform::Move(d)) = (self.copy, &mut self.copies, t) {
            let d = *d;
            return c.update(doc, &|m, e| transform::transformed(m, e, &Xform::Move(d)));
        }
        let originals = self.edit.originals().to_vec();
        if let (Some(parts), Xform::Move(d)) = (&self.stretch, t) {
            let changed = crate::stretchtool::moved(doc.model(), &originals, parts, *d)?;
            return self.edit.update(doc, changed).map_err(|e| e.to_string());
        }
        if let (Some(vs), ElementKind::FreeMesh(m)) = (&self.verts, &originals[0].kind) {
            let mut m = m.clone();
            let n = vs.len().max(1) as f64;
            let c = vs
                .iter()
                .map(|v| m.mesh.vertices[*v as usize])
                .sum::<DVec3>()
                / n;
            match *t {
                Xform::Move(d) => m.mesh.translate_vertices(vs, m.to_local(d)),
                Xform::Scale { factor, .. } => {
                    for v in vs {
                        let p = &mut m.mesh.vertices[*v as usize];
                        *p = c + (*p - c) * factor;
                    }
                }
                Xform::Rotate { degrees, .. } => {
                    let r = DVec2::from_angle(degrees.to_radians());
                    for v in vs {
                        let p = &mut m.mesh.vertices[*v as usize];
                        *p = (c.truncate() + r.rotate(p.truncate() - c.truncate())).extend(p.z);
                    }
                }
                // about an axis that is not the vertical (the handle's
                // arcs, or R then X or Y)
                Xform::Tilt { axis, degrees, .. } => {
                    if let Some(u) = m.to_local(axis).try_normalize() {
                        let q = glam::DQuat::from_axis_angle(u, degrees.to_radians());
                        for v in vs {
                            let p = &mut m.mesh.vertices[*v as usize];
                            *p = c + q * (*p - c);
                        }
                    }
                }
                // along one axis only (the handle's cubes, or S then X, Y, Z)
                Xform::Stretch { factor, .. } => {
                    for a in Axis::ALL {
                        let f = factor[a.index()];
                        let Some(u) = m.to_local(a.unit()).try_normalize() else {
                            continue;
                        };
                        if (f - 1.0).abs() < 1e-12 {
                            continue;
                        }
                        for v in vs {
                            let p = &mut m.mesh.vertices[*v as usize];
                            *p += u * ((*p - c).dot(u) * (f - 1.0));
                        }
                    }
                }
            }
            let mut e = originals[0].clone();
            e.kind = ElementKind::FreeMesh(m);
            return self.edit.update(doc, vec![e]).map_err(|e| e.to_string());
        }
        if let (Some(cs), Xform::Move(d)) = (&self.corners, t) {
            let mut e = originals[0].clone();
            crate::editmode::move_corners(doc.model(), &mut e, cs, *d)?;
            return self.edit.update(doc, vec![e]).map_err(|e| e.to_string());
        }
        let changed = match (self.face, &originals[0].kind) {
            (Some(f), ElementKind::FreeMesh(m)) => {
                let mut m = m.clone();
                let verts = m.mesh.faces[f].clone();
                match *t {
                    Xform::Move(d) => m.mesh.translate_vertices(&verts, m.to_local(d)),
                    Xform::Scale { factor, .. } => m.mesh.scale_face(f, factor),
                    Xform::Rotate { degrees, .. } => {
                        let c = m.mesh.face_center(f);
                        let r = DVec2::from_angle(degrees.to_radians());
                        let mut moved = verts.clone();
                        moved.sort_unstable();
                        moved.dedup();
                        for v in moved {
                            let p = &mut m.mesh.vertices[v as usize];
                            *p = (c.truncate() + r.rotate(p.truncate() - c.truncate())).extend(p.z);
                        }
                    }
                    Xform::Tilt { .. } | Xform::Stretch { .. } => {}
                }
                let mut e = originals[0].clone();
                e.kind = ElementKind::FreeMesh(m);
                vec![e]
            }
            _ => {
                let mut out = Vec::with_capacity(originals.len());
                for (i, o) in originals.iter().enumerate() {
                    let t = if self.individual {
                        Self::about(t, self.own[i])
                    } else {
                        *t
                    };
                    out.push(
                        transform::transformed(doc.model(), o, &t)
                            .ok_or_else(|| format!("{} can't take that change", o.name))?,
                    );
                }
                out
            }
        };
        // a duplicate's copy is joined to nothing yet: moving it drags
        // nothing, the original least of all (it shares both its ends, MU25)
        self.edit.free |= self.duplicated;
        self.edit.update(doc, changed).map_err(|e| e.to_string())
    }

    /// Show every element (or its copy) changed by `f`.
    fn apply_with(&mut self, doc: &mut Document, f: &Change) -> Result<(), String> {
        if self.copy {
            if let Some(c) = &mut self.copies {
                return c.update(doc, f);
            }
        }
        let originals = self.edit.originals().to_vec();
        let mut changed = Vec::with_capacity(originals.len());
        for o in &originals {
            changed.push(f(doc.model(), o).ok_or_else(|| format!("{} can't take that", o.name))?);
        }
        // a duplicate's copy is joined to nothing yet: moving it drags
        // nothing, the original least of all (it shares both its ends, MU25)
        self.edit.free |= self.duplicated;
        self.edit.update(doc, changed).map_err(|e| e.to_string())
    }

    /// Take back what this frame showed, the copies or the change.
    fn show_nothing(&mut self, doc: &mut Document) {
        self.edit.cancel(doc);
        if let Some(c) = &mut self.copies {
            c.cancel(doc);
        }
    }

    /// Mirror: the line from the first point to the pointer (on the view's
    /// plane), and the selection shown mirrored across it.
    fn step_mirror(&mut self, doc: &mut Document, ev: &Events, map: &PaneMap) -> Outcome {
        let skipped = self.skipped_note();
        let base = match self.base {
            Some(b) => b,
            None => {
                self.stage = Stage::Base;
                return Outcome::Running(String::new());
            }
        };
        let still = self
            .base_at
            .is_some_and(|b| b.distance(self.pointer) < STILL);
        let w = self.normal();
        let flat = |t: DVec3| {
            let d = t - base;
            d - w * d.dot(w)
        };
        // a snap that lies straight out from the first point (off the
        // plane) gives no line: then the pointer on the plane does
        let (target, snap) = match (map.snap)(self.pointer, Some(base)) {
            Some((p, s)) if flat(p).length() > 1e-3 => (Some(p), Some(s)),
            _ => ((map.to_model)(self.pointer, None, base), None),
        };
        self.snap = snap;
        let [u, v] = self.pane_axes();
        let mut d = target.map_or(DVec3::ZERO, flat);
        let mut deg = d.dot(v).atan2(d.dot(u)).to_degrees();
        // Shift keeps the line to 15° steps from the plane's axes (Ctrl
        // is the copy's)
        if self.ortho {
            deg = (deg / 15.0).round() * 15.0;
        }
        let typed = self.typed.trim().parse::<f64>().ok();
        if let Some(a) = typed {
            deg = a;
        }
        let len = d.length().max(1.0);
        if typed.is_some() || self.ortho {
            let r = deg.to_radians();
            d = (u * r.cos() + v * r.sin()) * len;
        }
        if (still && typed.is_none()) || d.length() < 1e-6 {
            self.show_nothing(doc);
            self.target = None;
            self.shown = format!(
                "{}: move the pointer to draw the mirror line",
                self.edit.label()
            );
        } else {
            self.target = Some(base + d);
            let n = d.normalize().cross(w).normalize();
            if let Err(e) = self.apply_with(doc, &|m, e| transform::mirrored(m, e, base, n)) {
                self.shown = format!("Can't mirror that: {e}");
            } else {
                let what = match (&self.copies, self.copy) {
                    (Some(c), true) => c.label.replacen("Copy", "Mirror a copy of", 1),
                    _ => self.edit.label().to_string(),
                };
                self.shown = format!("{what}: across the line at {deg:.1}°");
            }
        }
        if ev.click || ev.enter || self.ask == Some(true) {
            self.ask = None;
            return self.place(doc);
        }
        let copy = if self.copy {
            "Ctrl: keep only the mirrored one"
        } else {
            "Ctrl: keep the original too"
        };
        let typed = if self.typed.is_empty() {
            String::new()
        } else {
            format!("  [typed {}°]", self.typed)
        };
        Outcome::Running(format!(
            "{}{typed}{skipped}   ·   click the line's second point, type its angle + Enter, Shift keeps it to 15° steps, {copy}, Esc steps back",
            self.shown
        ))
    }

    /// Mirror across the selection's own centre line (MU142): shown at
    /// once, placed with a click or Enter.
    fn step_mirror_centre(
        &mut self,
        doc: &mut Document,
        ev: &Events,
        c: DVec3,
        d: DVec3,
    ) -> Outcome {
        let skipped = self.skipped_note();
        let w = self.normal();
        let n = d.cross(w).normalize_or_zero();
        let key = (c, n, self.copy);
        if self.centred != Some(key) && n.length() > 0.5 {
            self.centred = Some(key);
            self.base = Some(c);
            self.target = Some(c + d);
            match self.apply_with(doc, &|m, e| transform::mirrored(m, e, c, n)) {
                Err(e) => self.shown = format!("Can't mirror that: {e}"),
                Ok(()) => {
                    let what = match (&self.copies, self.copy) {
                        (Some(k), true) => k.label.replacen("Copy", "Mirror a copy of", 1),
                        _ => self.edit.label().to_string(),
                    };
                    let across = match self.across.unwrap_or_else(mirror_across) {
                        Across::PickLine => "the line picked",
                        Across::CentreSide => "its centre, left for right",
                        Across::CentreFront => "its centre, front for back",
                        Across::TwoPoints => "the line",
                    };
                    self.shown = format!("{what}: across {across}");
                }
            }
        }
        if ev.click || ev.enter || self.ask == Some(true) {
            self.ask = None;
            return self.place(doc);
        }
        Outcome::Running(format!(
            "{}{skipped}   ·   click or Enter to place, Ctrl: {}, Esc cancels; Draw a line in the row to place the line yourself",
            self.shown,
            if self.copy { "keep only the mirrored one" } else { "keep the original too" }
        ))
    }

    /// Offset: the selection's lines moved (outlines grown) to the side the
    /// pointer is on, by how far it is from the last one picked.
    fn step_offset(&mut self, doc: &mut Document, ev: &Events, map: &PaneMap) -> Outcome {
        let skipped = self.skipped_note();
        let w = self.normal();
        let Some(active) = self.edit.originals().last().cloned() else {
            return Outcome::Cancelled("Offset: nothing to offset.".into());
        };
        let at = (map.snap)(self.pointer, None)
            .map(|s| s.0)
            .or_else(|| (map.to_model)(self.pointer, None, self.pivot));
        self.snap = None;
        let side = at.and_then(|p| creabim_core::offset::side_distance(doc.model(), &active, p, w));
        let typed = parse_length(&self.typed).filter(|v| *v != 0.0);
        let by = match (typed, side) {
            // a typed distance goes to the pointer's side (a minus sign
            // to the other)
            (Some(t), Some(s)) if s < 0.0 => -t,
            (Some(t), _) => t,
            (None, Some(s)) => {
                // Shift rounds to 50 mm (Ctrl is the copy's)
                let step = if ev.shift { 0.05 } else { 0.01 };
                (s / step).round() * step
            }
            (None, None) => 0.0,
        };
        self.target = at;
        let want = (by, w, self.copy);
        if by.abs() < 1e-9 {
            self.show_nothing(doc);
            self.offset_shown = None;
            self.shown = format!(
                "{}: move the pointer to the side to offset to",
                self.edit.label()
            );
        } else if self.offset_shown == Some(want) {
            // as it stands: nothing to build
        } else if let Err(e) = self.apply_with(doc, &|_, e| creabim_core::offset::offset(e, by, w))
        {
            self.offset_shown = None;
            self.shown = format!("Can't offset that far: {e}");
        } else {
            self.offset_shown = Some(want);
            let what = match (&self.copies, self.copy) {
                (Some(c), true) => c.label.replacen("Copy", "Offset a copy of", 1),
                _ => self.edit.label().to_string(),
            };
            let way = if by < 0.0 {
                " (in or to the right)"
            } else {
                ""
            };
            self.shown = format!("{what}: {}{way}", crate::units::len(by.abs()));
        }
        self.delta = DVec3::splat(by);
        if ev.click || ev.enter || self.ask == Some(true) {
            self.ask = None;
            return self.place(doc);
        }
        let copy = if self.copy {
            "Ctrl: move the original instead"
        } else {
            "Ctrl: make a copy"
        };
        let typed = if self.typed.is_empty() {
            String::new()
        } else {
            format!("  [typed {}]", self.typed)
        };
        Outcome::Running(format!(
            "{}{typed}{skipped}   ·   click to place, type a distance + Enter, Shift rounds to 50 mm, {copy}, Esc stops",
            self.shown
        ))
    }

    /// Inset `faces` of the one shape being edited, following the pointer
    /// from now (MU49, decisions 1 to 3: it follows at once; a depth in
    /// the same go; several faces each on its own).
    pub fn with_inset(mut self, doc: &Document, faces: Vec<usize>) -> Result<Self, String> {
        let e = self
            .edit
            .originals()
            .last()
            .cloned()
            .ok_or("Select a shape first.")?;
        let ElementKind::FreeMesh(m) = &e.kind else {
            return Err(format!(
                "Inset works on a face of a shape. {} is not one: Offset is the tool for an outline, and Pull makes an outline a shape.",
                e.name
            ));
        };
        let mut faces: Vec<usize> = faces
            .into_iter()
            .filter(|f| *f < m.mesh.faces.len())
            .collect();
        faces.sort_unstable();
        faces.dedup();
        let Some(first) = faces.first().copied() else {
            return Err("Inset: pick a face of the shape first (a click on it, or in Edit mode with Faces on).".into());
        };
        let z = m
            .level
            .and_then(|l| doc.model().level(l))
            .map_or(0.0, |l| l.elevation);
        self.pivot = m.origin + m.to_model(m.mesh.face_center(first)) + DVec3::Z * z;
        let what = match faces.len() {
            1 => "Inset a face of".to_string(),
            n => format!("Inset {n} faces of"),
        };
        self.mode = Mode::Inset;
        self.edit = LiveEdit::new(format!("{what} {}", e.name), vec![e]);
        self.copy = false;
        self.copies = None;
        self.base = None;
        self.stage = Stage::Follow;
        self.inset = Some(InsetRun {
            faces,
            depth: 0.0,
            depth_typed: String::new(),
            last: None,
            out: false,
            was_ctrl: false,
            bias: 0.0,
            label: String::new(),
            lines: vec![],
        });
        Ok(self)
    }

    /// [`Mode::Inset`]: the thickness is how far the pointer is inside the
    /// nearest edge of the first face (or what is typed); with Ctrl held
    /// the pointer sets the depth along the face's direction instead. It
    /// stops where a face would close up, and short of the far side.
    fn step_inset(&mut self, doc: &mut Document, ev: &Events, map: &PaneMap) -> Outcome {
        let (Some(run), Some(e)) = (self.inset.clone(), self.edit.originals().last().cloned())
        else {
            return Outcome::Cancelled("Nothing to inset.".into());
        };
        let ElementKind::FreeMesh(m) = &e.kind else {
            return Outcome::Cancelled("Nothing to inset.".into());
        };
        let z = m
            .level
            .and_then(|l| doc.model().level(l))
            .map_or(0.0, |l| l.elevation);
        let base = m.origin + DVec3::Z * z;
        // O switches between Inset and Outset (not while a value is typed)
        let mut out = run.out;
        if self.typed.is_empty() && ev.text.chars().any(|c| c.eq_ignore_ascii_case(&'o')) {
            out = !out;
        }
        if out != self.edit.label().starts_with("Outset") {
            let (from, to) = if out {
                ("Inset", "Outset")
            } else {
                ("Outset", "Inset")
            };
            let named = self.edit.label().replacen(from, to, 1);
            self.edit.relabel(named);
        }
        let first = run.faces[0];
        let ring_of = |f: usize| -> Vec<DVec3> {
            m.mesh.faces[f]
                .iter()
                .map(|v| base + m.to_model(m.mesh.vertices[*v as usize]))
                .collect()
        };
        let ring = ring_of(first);
        let normal = m.to_model(m.mesh.face_normal(first)).normalize_or_zero();
        let scale = if m.scale.abs() > 1e-9 {
            m.scale.abs()
        } else {
            1.0
        };
        // metres to pixels along `dir` at `at`, and that direction on screen
        let on_screen = |at: DVec3, dir: DVec3| -> Option<(f32, egui::Vec2)> {
            let (a, b) = ((map.project)(at)?, (map.project)(at + dir)?);
            let d = b - a;
            (d.length() > 1e-3).then(|| (d.length(), d.normalized()))
        };
        let mut depth = run.depth;
        let mut bias = run.bias;
        let mut thick = self.delta.x.max(0.0);
        let p = self.pointer;
        if ev.ctrl {
            // along the face's own direction on screen; seen end on, up
            let (px, way) = on_screen(self.pivot, normal)
                .filter(|(px, _)| *px > 2.0)
                .or_else(|| {
                    let any = ring.get(1).map(|q| (*q - ring[0]).normalize_or_zero())?;
                    on_screen(self.pivot, any).map(|(px, _)| (px, egui::vec2(0.0, -1.0)))
                })
                .unwrap_or((50.0, egui::vec2(0.0, -1.0)));
            if let Some(last) = run.last {
                depth += ((p - last).dot(way) / px) as f64;
            }
        } else {
            // inside the nearest edge, on screen, of whichever picked face
            // the pointer is nearest to
            let mut best: Option<(f32, f64)> = None;
            for f in &run.faces {
                let ring = ring_of(*f);
                let normal = m.to_model(m.mesh.face_normal(*f)).normalize_or_zero();
                let k = ring.len();
                for i in 0..k {
                    let (a, b) = (ring[i], ring[(i + 1) % k]);
                    let inward = normal.cross(b - a).normalize_or_zero();
                    // Outset runs down the sides, behind the face
                    let along = if out {
                        on_screen((a + b) / 2.0, -normal)
                            .filter(|(px, _)| *px > 2.0)
                            .or_else(|| on_screen((a + b) / 2.0, inward))
                    } else {
                        on_screen((a + b) / 2.0, inward)
                    };
                    let (Some(pa), Some(pb), Some((px, _))) =
                        ((map.project)(a), (map.project)(b), along)
                    else {
                        continue;
                    };
                    let ab = pb - pa;
                    let t = ((p - pa).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                    let d = (pa + ab * t).distance(p);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, (d / px) as f64));
                    }
                }
            }
            if let Some((_, t)) = best {
                if run.was_ctrl {
                    // Ctrl let go: carry on from the thickness it had
                    bias = thick - t;
                }
                // Shift rounds to 50 mm, else to 10
                let step = if ev.shift { 0.05 } else { 0.01 };
                thick = ((t + bias).max(0.0) / step).round() * step;
            }
        }
        if let Some(t) = parse_length(&self.typed).filter(|v| *v > 0.0) {
            thick = t;
        }
        let depth_typed = run.depth_typed.trim();
        if let Some(d) = depth_typed
            .strip_prefix('-')
            .and_then(parse_length)
            .map(|v| -v)
            .or_else(|| parse_length(depth_typed))
        {
            depth = d;
        }
        // in the shape's own measure, and no further than fits
        let want = thick / scale;
        let mut fits = if out {
            m.mesh.outset_reach(&run.faces, want)
        } else {
            m.mesh.inset_reach(&run.faces, want)
        };
        let stopped = want > 0.0 && fits < want - 1e-9;
        if stopped {
            // not the very limit, where the new face is a sliver: the
            // 10 mm step under it
            let step = 0.01 / scale;
            fits = (((fits - step * 0.5) / step).floor() * step).max(0.0);
        }
        let floor = run
            .faces
            .iter()
            .filter_map(|f| m.mesh.through(*f).map(|(_, d)| d))
            .fold(f64::MAX, f64::min);
        let mut sink = depth / scale;
        let deep = floor < f64::MAX && sink < -(floor - 0.01).max(0.0);
        if deep {
            sink = -(floor - 0.01).max(0.0);
        }
        let label = self.edit.label().to_string();
        let faces = run.faces.clone();
        let outset = out;
        let result = if fits <= 1e-9 {
            self.edit.cancel(doc);
            Err(if out {
                "no room for a rim here: the sides must run back from the face".to_string()
            } else {
                "move over the face, or type a thickness".to_string()
            })
        } else {
            self.apply_with(doc, &|_, o| {
                let mut out = o.clone();
                if let ElementKind::FreeMesh(f) = &mut out.kind {
                    let done = if outset {
                        f.mesh.outset_faces(&faces, fits, sink)
                    } else {
                        f.mesh.inset_faces(&faces, fits, sink)
                    };
                    if !done || f.mesh.validate().is_err() {
                        return None;
                    }
                }
                Some(out)
            })
        };
        let mut value = crate::units::len(fits * scale);
        if stopped {
            value = format!("as far as it goes: {value}");
        }
        if sink.abs() > 1e-9 {
            let way = if sink < 0.0 { "in" } else { "out" };
            value += &format!("  ·  {way} {}", crate::units::len(sink.abs() * scale));
            if deep {
                value += " (short of the far side)";
            }
        }
        self.delta = DVec3::splat(thick);
        self.shown = match &result {
            Ok(()) => format!("{label}: {value}"),
            Err(why) => format!("{label}: {why}"),
        };
        // the edges the edit made: every one with a new point on it
        let mut lines = vec![];
        if result.is_ok() {
            if let Some(ElementKind::FreeMesh(after)) = doc.model().get(e.id).map(|x| &x.kind) {
                let old = m.mesh.vertices.len() as u32;
                let at = |v: u32| base + after.to_model(after.mesh.vertices[v as usize]);
                for f in &after.mesh.faces {
                    for j in 0..f.len() {
                        let (a, b) = (f[j], f[(j + 1) % f.len()]);
                        if a < b && (a >= old || b >= old) {
                            lines.push((at(a), at(b)));
                        }
                    }
                }
            }
        }
        if let Some(r) = &mut self.inset {
            r.lines = lines;
            r.depth = depth;
            r.last = ev.pointer;
            r.was_ctrl = ev.ctrl;
            r.bias = if out == r.out { bias } else { 0.0 };
            r.out = out;
            r.label = if result.is_ok() { value } else { String::new() };
        }
        if ev.click || ev.enter || self.ask == Some(true) {
            self.ask = None;
            if result.is_err() {
                return Outcome::Running(self.shown.clone());
            }
            if let Some(ElementKind::FreeMesh(after)) = doc.model().get(e.id).map(|x| &x.kind) {
                let mut vs: Vec<u32> = vec![];
                for f in &run.faces {
                    for v in after.mesh.faces.get(*f).into_iter().flatten() {
                        if !vs.contains(v) {
                            vs.push(*v);
                        }
                    }
                }
                crate::handles::set_pick_next(e.id, vs);
            }
            return Outcome::Done(format!(
                "{}. The new face is still picked: Pull takes it further. Ctrl+Z to undo.",
                self.shown
            ));
        }
        let typed = if self.typed.is_empty() {
            String::new()
        } else {
            format!("  [typed {}]", self.typed)
        };
        Outcome::Running(format!(
            "{}{typed}   ·   move over the face or type a thickness, O for a rim outside it, Ctrl and move sets a depth, Shift rounds to 50 mm, click or Enter to place, Esc cancels",
            self.shown
        ))
    }

    /// Drive grip `kind` of the one element being edited from the pointer,
    /// for [`Mode::Grip`]: it follows from where the grip is now, at once.
    pub fn with_grip(mut self, doc: &Document, kind: HandleKind) -> Result<Self, String> {
        let model = doc.model();
        let id = self
            .edit
            .originals()
            .last()
            .map(|e| e.id)
            .ok_or("Select something first.")?;
        let h = crate::handles::handles_in(model, id, None, true)
            .into_iter()
            .find(|h| h.kind == kind)
            .ok_or("That grip isn't there any more.")?;
        let (originals, joined) = crate::handles::originals(model, id, kind);
        let name = originals.first().map_or(String::new(), |e| e.name.clone());
        let what = match kind {
            HandleKind::Edge { .. } => "Offset an edge of",
            _ => "Move the new corner of",
        };
        self.mode = Mode::Grip;
        self.edit = LiveEdit::new(format!("{what} {name}"), originals);
        self.copy = false;
        self.copies = None;
        self.base = Some(h.at);
        self.base_at = Some(self.grab);
        self.pivot = h.at;
        self.stage = Stage::Follow;
        self.grip = Some(GripRun {
            kind,
            constraint: h.constraint,
            joined,
        });
        Ok(self)
    }

    /// [`Mode::Grip`]: the grip goes where the pointer is on its line (an
    /// edge, square to itself) or its plane (a corner), snapped; a typed
    /// distance goes the way the pointer went.
    fn step_grip(&mut self, doc: &mut Document, ev: &Events, map: &PaneMap) -> Outcome {
        let (Some(base), Some(g)) = (self.base, self.grip.as_ref()) else {
            return Outcome::Cancelled("Nothing to edit.".into());
        };
        let still = self
            .base_at
            .is_some_and(|b| b.distance(self.pointer) < STILL);
        let snap = (map.snap)(self.pointer, Some(base));
        self.snap = snap.map(|s| s.1);
        let on_plane = |p: DVec3, n: DVec3| p - n * (p - base).dot(n);
        let pointer = match g.constraint {
            // an edge in a level outline runs across the level; one in an
            // upright outline on the plane facing the viewer
            Constraint::Line { dir, .. } => {
                let axis = (dir.z.abs() > 0.5).then_some(Axis::Z);
                snap.map(|s| s.0)
                    .or_else(|| (map.to_model)(self.pointer, axis, base))
            }
            Constraint::Plane { normal, .. } => snap
                .map(|s| s.0)
                .or_else(|| (map.to_model)(self.pointer, None, base))
                .map(|p| on_plane(p, normal)),
        };
        let mut now = match g.constraint {
            Constraint::Line { dir, .. } => pointer.map(|p| base + dir * (p - base).dot(dir)),
            Constraint::Plane { .. } => pointer,
        }
        .filter(|_| !still)
        .unwrap_or(base);
        let way = match g.constraint {
            Constraint::Line { dir, .. } => {
                if (now - base).dot(dir) < 0.0 {
                    -dir
                } else {
                    dir
                }
            }
            Constraint::Plane { normal, .. } => {
                let d = on_plane(pointer.unwrap_or(base), normal) - base;
                if d.length() > 1e-9 {
                    d.normalize()
                } else {
                    // nowhere yet: along the plane's first axis
                    normal.any_orthonormal_vector()
                }
            }
        };
        if let Some(len) = parse_length(&self.typed) {
            now = base + way * len;
        }
        self.delta = now - base;
        self.target = Some(now);
        let result = if self.delta.length() < 1e-9 {
            self.edit.cancel(doc);
            Ok(())
        } else {
            crate::handles::apply(
                doc.model(),
                self.edit.originals(),
                &g.joined,
                g.kind,
                base,
                now,
                0.0,
            )
            .and_then(|changed| self.edit.update(doc, changed).map_err(|e| e.to_string()))
        };
        let label = self.edit.label().to_string();
        self.shown = match (&result, g.kind) {
            (Err(e), _) => format!("Can't go there: {e}"),
            (Ok(()), HandleKind::Edge { .. }) => {
                let s = match g.constraint {
                    Constraint::Line { dir, .. } => self.delta.dot(dir),
                    Constraint::Plane { .. } => self.delta.length(),
                };
                let way = if s < 0.0 { " in" } else { " out" };
                format!("{label}: {}{way}", crate::units::len(s.abs()))
            }
            (Ok(()), _) => format!("{label}: {}", crate::units::len(self.delta.length())),
        };
        if ev.click || ev.enter || self.ask == Some(true) {
            self.ask = None;
            if result.is_err() {
                return Outcome::Running(self.shown.clone());
            }
            return Outcome::Done(format!("{}. Ctrl+Z to undo.", self.shown));
        }
        let typed = if self.typed.is_empty() {
            String::new()
        } else {
            format!("  [typed {}]", self.typed)
        };
        Outcome::Running(format!(
            "{}{typed}   ·   move the pointer (it snaps) and click, or type a distance + Enter; Esc stops",
            self.shown
        ))
    }

    fn draw(&self, painter: &egui::Painter, map: &PaneMap) {
        if let Some(run) = &self.inset {
            let ink = egui::Stroke::new(2.0, crate::theme::interaction());
            for (a, b) in &run.lines {
                if let (Some(pa), Some(pb)) = ((map.project)(*a), (map.project)(*b)) {
                    painter.line_segment([pa, pb], ink);
                }
            }
        }
        let accent = crate::theme::interaction();
        if self.uses_points() {
            return self.draw_points(painter, map);
        }
        if self.waits_to_scale() {
            return self.draw_scale_start(painter, map);
        }
        let Some(ps) = (map.project)(self.pivot) else {
            return;
        };
        if let Some(a) = self.axis {
            draw_axis(painter, map, self.pivot, a);
        }
        if self.individual {
            for c in &self.own {
                if let Some(p) = (map.project)(*c) {
                    painter.circle_stroke(p, 3.0, Stroke::new(1.0, accent));
                }
            }
        } else {
            painter.circle_stroke(ps, 4.0, Stroke::new(1.5, accent));
        }
        if self.mode == Mode::Rotate {
            let start = (-(self.grab.y - ps.y)).atan2(self.grab.x - ps.x);
            let r = ps.distance(self.grab).clamp(56.0, 140.0);
            let ink = if self.locked.is_some() {
                GUIDE_PINK
            } else {
                accent
            };
            // 0° where the orientation puts it, read on screen
            let zero = (map.project)(self.pivot + self.zero.extend(0.0))
                .map_or(start, |q| (-(q.y - ps.y)).atan2(q.x - ps.x));
            let (shapes, labels) = protractor(ps, r, zero, start, self.swept as f32, ink);
            for s in shapes {
                painter.add(s);
            }
            let text = crate::theme::Palette::of(painter.ctx()).muted;
            for (at, label) in labels {
                painter.text(
                    at,
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(10.0),
                    text,
                );
            }
            return;
        }
        painter.add(egui::Shape::dashed_line(
            &[ps, self.pointer],
            Stroke::new(1.0, accent),
            6.0,
            4.0,
        ));
        if self.mode == Mode::Scale {
            self.draw_scale(painter, map);
        }
    }

    /// Before the Scale tool's first click: its outline, and the corner
    /// nearest the pointer marked, where a click starts it.
    fn draw_scale_start(&self, painter: &egui::Painter, map: &PaneMap) {
        let Some((lo, hi)) = self.scale.extent else {
            return;
        };
        let z = self.scale.centre.z;
        let corners = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
        let scr: Option<Vec<Pos2>> = corners.iter().map(|q| (map.project)(q.extend(z))).collect();
        let Some(scr) = scr else {
            return;
        };
        let accent = crate::theme::interaction();
        let mut ring = scr.clone();
        ring.push(scr[0]);
        painter.add(egui::Shape::dashed_line(
            &ring,
            Stroke::new(1.0, accent),
            4.0,
            3.0,
        ));
        if let Some(near) = scr.iter().min_by(|a, b| {
            a.distance(self.pointer)
                .total_cmp(&b.distance(self.pointer))
        }) {
            painter.circle_stroke(*near, 5.0, Stroke::new(1.5, accent));
        }
    }

    /// Scale as it goes (MU142, Tool Rows and Scale REV01): the outline it
    /// was picked up with, faint; the new one with its sizes on two sides;
    /// the factor by the pointer; by reference, the length clicked.
    fn draw_scale(&self, painter: &egui::Painter, map: &PaneMap) {
        let accent = crate::theme::interaction();
        let muted = crate::theme::Palette::of(painter.ctx()).muted;
        let z = self.scale.centre.z;
        let font = egui::FontId::proportional(11.0);
        // the length known, by reference
        let refs: Vec<Pos2> = self
            .scale
            .refs
            .iter()
            .filter_map(|p| (map.project)(*p))
            .collect();
        for p in &refs {
            painter.circle_stroke(*p, 4.0, Stroke::new(1.5, GUIDE_PINK));
        }
        if let ([a, b], [pa, pb]) = (&self.scale.refs[..], &refs[..]) {
            painter.line_segment([*pa, *pb], Stroke::new(1.5, GUIDE_PINK));
            let known = a.truncate().distance(b.truncate());
            painter.text(
                *pa + (*pb - *pa) / 2.0 + egui::vec2(0.0, -12.0),
                egui::Align2::CENTER_BOTTOM,
                crate::units::len(known),
                font.clone(),
                GUIDE_PINK,
            );
        }
        let Some((lo, hi)) = self.scale.extent else {
            return;
        };
        if scale_by_ref() && self.scale.refs.len() < 2 {
            return;
        }
        let f = self.scale.factor;
        let c = self.pivot.truncate();
        let to = |q: DVec2| c + (q - c) * f.truncate();
        let corners = |a: DVec2, b: DVec2| [a, DVec2::new(b.x, a.y), b, DVec2::new(a.x, b.y)];
        let screen = |q: DVec2| (map.project)(q.extend(z));
        let ring = |pts: [DVec2; 4]| -> Option<Vec<Pos2>> {
            let mut v: Vec<Pos2> = pts.iter().map(|q| screen(*q)).collect::<Option<_>>()?;
            v.push(v[0]);
            Some(v)
        };
        if let Some(was) = ring(corners(lo, hi)) {
            painter.add(egui::Shape::dashed_line(
                &was,
                Stroke::new(1.0, muted),
                4.0,
                3.0,
            ));
        }
        let (nlo, nhi) = (to(lo).min(to(hi)), to(lo).max(to(hi)));
        if let Some(now) = ring(corners(nlo, nhi)) {
            painter.add(egui::Shape::line(now, Stroke::new(1.5, accent)));
        }
        // its sizes: the width under it, the depth beside it
        let side = |a: DVec2, b: DVec2, out: egui::Vec2, text: String, align: egui::Align2| {
            if let (Some(pa), Some(pb)) = (screen(a), screen(b)) {
                painter.text(pa + (pb - pa) / 2.0 + out, align, text, font.clone(), muted);
            }
        };
        side(
            nlo,
            DVec2::new(nhi.x, nlo.y),
            egui::vec2(0.0, 12.0),
            crate::units::len(nhi.x - nlo.x),
            egui::Align2::CENTER_TOP,
        );
        side(
            DVec2::new(nhi.x, nlo.y),
            nhi,
            egui::vec2(10.0, 0.0),
            crate::units::len(nhi.y - nlo.y),
            egui::Align2::LEFT_CENTER,
        );
        // the factor by the pointer
        let text = if (f.x - f.y).abs() > 1e-9 {
            format!("× {:.2} by × {:.2}", f.x, f.y)
        } else {
            format!("× {:.2}", f.x)
        };
        let at = self.pointer + egui::vec2(14.0, -22.0);
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.0), Color32::WHITE);
        let rect = egui::Rect::from_min_size(at, galley.size()).expand2(egui::vec2(5.0, 2.0));
        painter.rect_filled(rect, 4.0, GUIDE_PINK);
        painter.galley(at, galley, Color32::WHITE);
    }

    /// The base point, the rubber band to where it goes, and the snap
    /// marker under the pointer.
    fn draw_points(&self, painter: &egui::Painter, map: &PaneMap) {
        let accent = crate::theme::interaction();
        let stroke = Stroke::new(1.5, accent);
        let marker = |s: Snap, from: Option<Pos2>| {
            if let Some(at) = (map.project)(s.p) {
                if s.kind == SnapKind::Grid {
                    painter.circle_stroke(at, 4.0, Stroke::new(1.5, accent));
                } else {
                    crate::snaps::draw(painter, s, at, from);
                }
            }
        };
        // the face under the pointer, or the face being pushed where it is now
        let face = match (&self.push, &self.hover) {
            (Some(p), _) => Some((&p.pick.tris, self.delta)),
            (None, Some(h)) => Some((&h.tris, DVec3::ZERO)),
            _ => None,
        };
        if let Some((tris, d)) = face {
            // blue, so it reads apart from the orange of the selection
            let fill = Color32::from_rgba_unmultiplied(40, 130, 240, 110);
            for t in tris {
                let pts: Option<Vec<Pos2>> = t.iter().map(|p| (map.project)(*p + d)).collect();
                if let Some(pts) = pts {
                    painter.add(egui::Shape::convex_polygon(pts, fill, Stroke::NONE));
                }
            }
        }
        let base = self.base.and_then(|b| (map.project)(b));
        // the axis held, or the one the pointer is running along
        if let (Some(a), Some(b)) = (self.axis.or(self.inferred), self.base) {
            draw_axis(painter, map, b, a);
        }
        match self.stage {
            Stage::Base if self.mode == Mode::Mirror && self.centred.is_some() => {
                // across its own centre line: the line through it (MU142)
                if let (Some(b), Some(t)) = (self.base, self.target) {
                    let d = (t - b).normalize_or_zero();
                    let reach = |way: DVec3| {
                        [30.0, 10.0, 3.0, 1.0, 0.0]
                            .into_iter()
                            .find_map(|k| (map.project)(b + way * k))
                    };
                    if let (Some(p), Some(q)) = (reach(-d), reach(d)) {
                        painter.add(egui::Shape::dashed_line(&[p, q], stroke, 10.0, 4.0));
                    }
                }
            }
            Stage::Base => {
                if let Some(s) = self.snap {
                    marker(s, None);
                }
            }
            Stage::Reference => {
                if let (Some(b), Some(t)) = (base, self.target.and_then(|t| (map.project)(t))) {
                    painter.add(egui::Shape::dashed_line(&[b, t], stroke, 6.0, 4.0));
                }
                if let Some(s) = self.snap {
                    marker(s, base);
                }
            }
            Stage::Follow => {
                let to = self.target.and_then(|t| (map.project)(t));
                if let (Mode::Mirror, Some(b), Some(t)) = (self.mode, self.base, self.target) {
                    // the mirror line, drawn on well past both points
                    let d = (t - b).normalize_or_zero();
                    // as far past both points as still shows on screen
                    let reach = |from: DVec3, way: DVec3| {
                        [30.0, 10.0, 3.0, 1.0, 0.0]
                            .into_iter()
                            .find_map(|k| (map.project)(from + way * k))
                    };
                    if let (Some(p), Some(q)) = (reach(b, -d), reach(t, d)) {
                        painter.add(egui::Shape::dashed_line(&[p, q], stroke, 10.0, 4.0));
                    }
                    if let Some(t) = to {
                        painter.circle_filled(t, 3.0, accent);
                    }
                } else if self.mode == Mode::Offset {
                    if let Some(t) = to {
                        painter.circle_stroke(t, 4.0, stroke);
                    }
                } else if self.mode == Mode::Rotate {
                    // the protractor round the centre, from the reference
                    // (MU142: the tool showed only two lines)
                    if let (Some(b), Some(r)) =
                        (base, self.reference.and_then(|r| (map.project)(r)))
                    {
                        let ang = |p: Pos2| (-(p.y - b.y)).atan2(p.x - b.x);
                        let start = ang(r);
                        let radius = b.distance(r).clamp(56.0, 140.0);
                        let ink = if self.locked.is_some() {
                            GUIDE_PINK
                        } else {
                            accent
                        };
                        let zero = self
                            .base
                            .and_then(|c| (map.project)(c + self.zero.extend(0.0)))
                            .map_or(start, ang);
                        let (shapes, labels) =
                            protractor(b, radius, zero, start, self.swept as f32, ink);
                        for s in shapes {
                            painter.add(s);
                        }
                        let text = crate::theme::Palette::of(painter.ctx()).muted;
                        for (at, label) in labels {
                            painter.text(
                                at,
                                egui::Align2::CENTER_CENTER,
                                label,
                                egui::FontId::proportional(10.0),
                                text,
                            );
                        }
                    }
                } else if let (Some(b), Some(t)) = (base, to) {
                    // the rubber band, with its length at its middle; in
                    // its axis's colour when it keeps to one (MU39)
                    let band = match self.axis {
                        Some(a) => Stroke::new(1.5, axis_colour(a)),
                        None => stroke,
                    };
                    painter.add(egui::Shape::dashed_line(&[b, t], band, 6.0, 4.0));
                    if b.distance(t) > 30.0 {
                        let side = (t - b).normalized().rot90() * 14.0;
                        let side = if side.y > 0.0 { -side } else { side };
                        painter.text(
                            b + (t - b) / 2.0 + side,
                            egui::Align2::CENTER_CENTER,
                            crate::units::len(self.delta.length()),
                            egui::FontId::proportional(13.0),
                            accent,
                        );
                    }
                    painter.circle_filled(t, 3.0, accent);
                }
                if let Some(s) = self.snap.filter(|s| s.kind != SnapKind::Grid) {
                    marker(s, base);
                }
            }
        }
        if let Some(b) = base {
            // the base point: a small cross in a circle
            painter.circle_stroke(b, 5.0, stroke);
            painter.line_segment([b - egui::vec2(8.0, 0.0), b + egui::vec2(8.0, 0.0)], stroke);
            painter.line_segment([b - egui::vec2(0.0, 8.0), b + egui::vec2(0.0, 8.0)], stroke);
        }
    }
}

/// The line through `at` along axis `a`, in the axis' colour.
/// An axis's colour, as Blender's: X red, Y green, Z blue.
/// Is this snap on something modelled (a corner, a middle, an edge, a
/// face), which has a height of its own, and not a point worked out on the
/// level (the grid, an axis, an alignment)?
fn on_something(k: SnapKind) -> bool {
    k.is_point()
        || matches!(
            k,
            SnapKind::Perpendicular | SnapKind::OnEdge | SnapKind::OnFace
        )
}

/// How near straight up or down from the base point, on the screen, the
/// pointer must run for a move to go vertical by itself (degrees).
const VERTICAL_BAND: f32 = 12.0;

/// Is the pointer running straight up or down from `base` on the screen,
/// in a view that shows the vertical as a direction (not one looking
/// straight down, where up is a point)?
pub fn runs_vertical(map: &PaneMap, base: DVec3, pointer: Pos2) -> bool {
    let at = |p: DVec3| (map.project)(p);
    let (Some(b), Some(z), Some(x), Some(y)) = (
        at(base),
        at(base + DVec3::Z),
        at(base + DVec3::X),
        at(base + DVec3::Y),
    ) else {
        return false;
    };
    let up = z - b;
    // seen from above the vertical all but vanishes: no direction to follow
    let flat = (x - b).length().max((y - b).length());
    if up.length() < 1e-3 || up.length() < flat * 0.3 {
        return false;
    }
    let v = pointer - b;
    if v.length() < 4.0 * STILL {
        return false;
    }
    let cos = (v.normalized().dot(up.normalized())).abs();
    cos >= VERTICAL_BAND.to_radians().cos()
}

pub fn axis_colour(a: Axis) -> Color32 {
    match a {
        Axis::X => Color32::from_rgb(214, 72, 72),
        Axis::Y => Color32::from_rgb(82, 170, 72),
        Axis::Z => Color32::from_rgb(70, 125, 225),
    }
}

/// The line a move keeps to, through `at`, in the axis's colour. Drawn in
/// pieces, keeping those in front of the camera: drawn end to end, a Z
/// line's far end could fall behind the camera and the whole line went
/// missing (MU39).
/// The guide colour: a turn locked to a named angle, a centre line (MU142).
pub(crate) const GUIDE_PINK: Color32 = Color32::from_rgb(224, 69, 123);

/// The Rotate protractor (MU142): a faint ring round the pivot, a tick
/// every 15° (longer at 45°) counted from where the turn was grabbed,
/// 0/90/180/270 named from that start, and the swept arc with its ray.
/// Angles are screen anticlockwise in radians (`start`) and degrees
/// (`swept`); `ink` turns pink when the turn is locked.
pub(crate) fn protractor(
    c: Pos2,
    r: f32,
    zero: f32,
    start: f32,
    swept: f32,
    ink: Color32,
) -> (Vec<egui::Shape>, Vec<(Pos2, String)>) {
    let at = |a: f32, r: f32| c + egui::vec2(a.cos(), -a.sin()) * r;
    let faint = ink.gamma_multiply(0.45);
    let mut shapes = vec![egui::Shape::circle_stroke(c, r, Stroke::new(1.0, faint))];
    let mut labels = Vec::new();
    // the ticks and names count from the orientation's 0° (MU142); the
    // arc runs from where the turn was grabbed
    for i in 0..24 {
        let a = zero + (i as f32 * 15.0).to_radians();
        let long = i % 3 == 0;
        let len = if long { 9.0 } else { 5.0 };
        shapes.push(egui::Shape::line_segment(
            [at(a, r - len), at(a, r)],
            Stroke::new(if long { 1.5 } else { 1.0 }, faint),
        ));
        if i % 6 == 0 {
            labels.push((at(a, r + 11.0), format!("{}°", i * 15)));
        }
    }
    // the swept arc, a point every 3°, and the ray to where it ends
    let n = (swept.abs() / 3.0).ceil().max(1.0) as usize;
    let arc: Vec<Pos2> = (0..=n)
        .map(|k| at(start + (swept * k as f32 / n as f32).to_radians(), r))
        .collect();
    if swept.abs() > 0.05 {
        let mut wedge = vec![c];
        wedge.extend(arc.iter().copied());
        shapes.push(egui::Shape::Path(egui::epaint::PathShape {
            points: wedge,
            closed: true,
            fill: ink.gamma_multiply(0.12),
            stroke: Stroke::NONE.into(),
        }));
        shapes.push(egui::Shape::line(arc, Stroke::new(2.0, ink)));
    }
    shapes.push(egui::Shape::line_segment(
        [c, at(start, r)],
        Stroke::new(1.0, faint),
    ));
    let end = at(start + swept.to_radians(), r);
    shapes.push(egui::Shape::line_segment([c, end], Stroke::new(1.5, ink)));
    shapes.push(egui::Shape::circle_filled(end, 3.0, ink));
    (shapes, labels)
}

fn draw_axis(painter: &egui::Painter, map: &PaneMap, at: DVec3, a: Axis) {
    let stroke = Stroke::new(1.5, axis_colour(a));
    let steps: Vec<f64> = (-40..=40).map(|k| k as f64 * 5.0).collect();
    for w in steps.windows(2) {
        let (p, q) = (
            (map.project)(at + a.unit() * w[0]),
            (map.project)(at + a.unit() * w[1]),
        );
        if let (Some(p), Some(q)) = (p, q) {
            painter.line_segment([p, q], stroke);
        }
    }
}

/// Copies of `ids` (with the doors in copied walls), inserted as one undo
/// step. Returns the copies to keep moving.
pub fn duplicate(doc: &mut Document, ids: &[ElementId]) -> Result<Vec<ElementId>, String> {
    let model = doc.model();
    let copies = if let [one] = ids {
        // one element: a lone door's copy goes beside it on its wall
        transform::duplicate(model, *one)
    } else {
        copies_of_set(doc, ids)
    };
    if copies.is_empty() {
        return Err("That can't be duplicated.".into());
    }
    insert_copies(doc, copies)
}

/// Copies of `ids` as copy and paste makes them (with what hangs on them),
/// the picked ones named "… copy".
fn copies_of_set(doc: &Document, ids: &[ElementId]) -> Vec<Element> {
    let model = doc.model();
    let set = transform::copy_set(model, ids);
    let elements: Vec<Element> = set
        .iter()
        .filter_map(|id| model.get(*id))
        .cloned()
        .collect();
    let (mut copies, _) = transform::fresh_copies(model, &elements);
    for (c, o) in copies.iter_mut().zip(&elements) {
        if ids.contains(&o.id) {
            c.name = format!("{} copy", c.name);
        }
    }
    copies
}

/// Copies of views, made at once: a view has no place in a drawing to
/// follow the pointer to, so Duplicate on views works like copy and paste.
/// `None` when `ids` aren't all views.
pub fn duplicate_views(
    doc: &mut Document,
    ids: &[ElementId],
) -> Option<Result<Vec<ElementId>, String>> {
    let model = doc.model();
    let views = !ids.is_empty()
        && ids.iter().all(|id| {
            model
                .get(*id)
                .is_some_and(|e| matches!(e.kind, ElementKind::View(_)))
        });
    if !views {
        return None;
    }
    let copies = copies_of_set(doc, ids);
    if copies.is_empty() {
        return Some(Err("That can't be duplicated.".into()));
    }
    Some(insert_copies(doc, copies))
}

/// What the status line says after `duplicate_views`.
pub fn views_duplicated(doc: &Document, copies: &[ElementId]) -> String {
    let names: Vec<&str> = copies
        .iter()
        .filter_map(|id| doc.model().get(*id))
        .map(|e| e.name.as_str())
        .collect();
    match names.as_slice() {
        [one] => format!("Duplicated the view as {one}."),
        _ => format!("Duplicated {} views.", names.len()),
    }
}

/// Insert `copies` as one undo step; returns their ids.
fn insert_copies(doc: &mut Document, copies: Vec<Element>) -> Result<Vec<ElementId>, String> {
    let name = match copies.len() {
        1 => copies[0].name.clone(),
        n => format!("{n} elements"),
    };
    let ids: Vec<ElementId> = copies.iter().map(|c| c.id).collect();
    let mut tx = Transaction::new(format!("Duplicate {name}"));
    for c in copies {
        tx = tx.insert(c);
    }
    doc.commit(tx).map_err(|e| e.to_string())?;
    Ok(ids)
}

/// The other faces of the shape that Pull takes along with `face`: those
/// picked whole in Edit mode with it (every corner of each picked), when
/// `face` is one of them. Each goes its own way (MU47, decision 3).
fn also_picked(originals: &[Element], face: &pushpull::FaceRef) -> Vec<usize> {
    let (Some(e), Some(f)) = (originals.first(), face.mesh_face) else {
        return vec![];
    };
    let ElementKind::FreeMesh(m) = &e.kind else {
        return vec![];
    };
    let Some(picked) = crate::handles::mesh_pick(e.id) else {
        return vec![];
    };
    let whole = |i: usize| m.mesh.faces[i].iter().all(|v| picked.contains(v));
    if f >= m.mesh.faces.len() || !whole(f) {
        return vec![];
    }
    (0..m.mesh.faces.len())
        .filter(|i| *i != f && whole(*i))
        .collect()
}

#[cfg(test)]
mod turn_step_tests {
    use super::*;

    /// The protractor (MU142): 24 ticks a 15° apart counted from the grab,
    /// four named, the arc ending where the turn has swept to, and in the
    /// guide pink when the turn is locked.
    #[test]
    fn the_protractor_counts_from_the_grab_and_sweeps_to_the_turn() {
        let c = Pos2::new(100.0, 100.0);
        let ink = Color32::from_rgb(10, 20, 30);
        let start = 30f32.to_radians();
        let (shapes, labels) = protractor(c, 80.0, start, start, 90.0, ink);
        let ticks = shapes
            .iter()
            .filter(|s| {
                matches!(s, egui::Shape::LineSegment { points, .. }
                if (points[0].distance(c) - points[1].distance(c)).abs() > 1.0
                    && (points[1].distance(c) - 80.0).abs() < 0.1
                    && points[0].distance(c) > 60.0)
            })
            .count();
        assert_eq!(ticks, 24);
        let names: Vec<&str> = labels.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(names, ["0°", "90°", "180°", "270°"]);
        // 0° sits outside the ring on the grab's side (30° up and right)
        let (zero, _) = &labels[0];
        assert!(zero.x > c.x && zero.y < c.y, "{zero:?}");
        // the ray ends 90° on from the grab, at 120° on screen: up and left
        let end = shapes
            .iter()
            .rev()
            .find_map(|s| match s {
                egui::Shape::Circle(k) if k.fill == ink => Some(k.center),
                _ => None,
            })
            .expect("the end dot");
        let want = c + egui::vec2(120f32.to_radians().cos(), -120f32.to_radians().sin()) * 80.0;
        assert!(end.distance(want) < 0.01, "{end:?} vs {want:?}");
        // nothing swept: no arc, but the start ray and dot are there
        let (still, _) = protractor(c, 80.0, start, start, 0.0, ink);
        // Local or Global: the names move to the orientation's 0°, the arc
        // stays from the grab
        let (_, named) = protractor(c, 80.0, 0.0, start, 90.0, ink);
        let (east, _) = &named[0];
        assert!((east.y - c.y).abs() < 0.01 && east.x > c.x, "{east:?}");
        assert!(!still.iter().any(|s| matches!(s, egui::Shape::Path(_))));
        assert_eq!(GUIDE_PINK, Color32::from_rgb(224, 69, 123));
    }

    /// A turn locks to 0, 90, 180 and 270 as it passes them, and keeps to
    /// the step picked in Rotate's row (MU142: a 180° flip had nothing to
    /// lock to).
    #[test]
    fn a_turn_locks_to_named_angles_and_keeps_to_its_step() {
        // Free: any angle, but near a named one it locks
        assert_eq!(turn_degrees(60.0, false, 1.0, 0.0), (60.0, None));
        assert_eq!(turn_degrees(177.0, false, 1.0, 0.0), (180.0, Some(180.0)));
        assert_eq!(turn_degrees(-88.6, false, 1.0, 0.0), (-90.0, Some(-90.0)));
        // Free with Ctrl: 15° steps, as before
        assert_eq!(turn_degrees(52.0, true, 1.0, 0.0).0, 45.0);
        // a step picked: always in it, locked when it lands on a named one
        assert_eq!(turn_degrees(52.0, false, 1.0, 45.0), (45.0, None));
        assert_eq!(turn_degrees(70.0, false, 1.0, 45.0), (90.0, Some(90.0)));
        assert_eq!(turn_degrees(130.0, false, 1.0, 90.0), (90.0, Some(90.0)));
        assert_eq!(turn_degrees(23.0, false, 1.0, 15.0), (30.0, None));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use creabim_core::sample::sample_house;

    #[test]
    fn attached_to_finds_what_dependents_finds() {
        use std::collections::BTreeSet;
        let model = creabim_core::starter::starter_house();
        let all: Vec<ElementId> = model.iter().map(|e| e.id).collect();
        for ids in [&all[..], &all[..1], &all[10..60], &all[all.len() / 2..]] {
            let slow: BTreeSet<u128> = ids
                .iter()
                .flat_map(|id| model.dependents(*id))
                .map(|id| id.0.as_u128())
                .collect();
            let quick = attached_to(&model, ids);
            let set: BTreeSet<u128> = quick.iter().map(|id| id.0.as_u128()).collect();
            assert_eq!(set, slow);
            // each once
            assert_eq!(set.len(), quick.len());
        }
        assert!(attached_to(&model, &[]).is_empty());
    }

    #[test]
    fn typed_move_distances_follow_the_project_units() {
        crate::units::set(creabim_core::Units::metric());
        assert_eq!(parse_length("2400"), Some(2.4));
        assert_eq!(parse_length("2.4m"), Some(2.4));
        crate::units::set(creabim_core::Units::imperial());
        let ft = parse_length("8'-6\"").unwrap();
        assert!((ft - 2.5908).abs() < 1e-9, "{ft}");
        crate::units::set(creabim_core::Units::metric());
    }

    /// A plan-like pane: 10 points to the metre, y down, with endpoint
    /// snaps at `points` (within half a metre) and free points elsewhere.
    fn with_map<R>(points: &[DVec3], f: impl FnOnce(&PaneMap) -> R) -> R {
        let to_screen = |p: DVec3| Pos2::new((p.x * 10.0) as f32, (-p.y * 10.0) as f32);
        let to_world = |p: Pos2| DVec3::new(p.x as f64 / 10.0, -p.y as f64 / 10.0, 0.0);
        let to_model = |p: Pos2, _: Option<Axis>, pivot: DVec3| Some(to_world(p).with_z(pivot.z));
        let project = |p: DVec3| Some(to_screen(p));
        let pts = points.to_vec();
        let snap = move |p: Pos2, from: Option<DVec3>| {
            let w = to_world(p);
            let z = from.map_or(0.0, |f| f.z);
            if let Some(e) = pts
                .iter()
                .find(|e| e.truncate().distance(w.truncate()) < 0.5)
            {
                let s = Snap {
                    p: e.with_z(z),
                    kind: SnapKind::Endpoint,
                    host: None,
                };
                return Some((s.p, s));
            }
            let s = Snap {
                p: w.with_z(z),
                kind: SnapKind::Grid,
                host: None,
            };
            Some((s.p, s))
        };
        let map = PaneMap {
            to_model: &to_model,
            project: &project,
            look: -DVec3::Z,
            snap: &snap,
            pick_face: None,
            pick_line: None,
        };
        f(&map)
    }

    fn at(x: f64, y: f64) -> Pos2 {
        Pos2::new((x * 10.0) as f32, (-y * 10.0) as f32)
    }

    fn pointer(p: Pos2) -> Events {
        Events {
            pointer: Some(p),
            ..Default::default()
        }
    }

    fn click(p: Pos2) -> Events {
        Events {
            pointer: Some(p),
            click: true,
            ..Default::default()
        }
    }

    fn first_wall(doc: &Document) -> (ElementId, DVec2) {
        doc.model()
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::Wall(w) => Some((e.id, w.start)),
                _ => None,
            })
            .unwrap()
    }

    fn wall_start(doc: &Document, id: ElementId) -> DVec2 {
        match &doc.model().get(id).unwrap().kind {
            ElementKind::Wall(w) => w.start,
            _ => unreachable!(),
        }
    }

    fn tool(doc: &Document, mode: Mode, id: ElementId) -> Modal {
        Modal::start(
            doc,
            mode,
            &[id],
            None,
            Space::Plan,
            None,
            Pos2::ZERO,
            Pivot::Median,
            None,
            None,
        )
        .unwrap()
        .with_flow(Flow::Tool)
    }

    /// MU142: the four orientations that were "not yet". View counts from
    /// the screen's right, Cursor from the pivot cursor towards what turns,
    /// Host along a door's wall, Normal out of a wall-like face of a mass.
    #[test]
    fn the_orientations_count_from_the_view_the_cursor_the_host_and_a_face() {
        let model = sample_house();
        let near = |a: DVec2, b: DVec2| a.distance(b) < 1e-9;
        let opening = model
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Opening(_)))
            .cloned()
            .unwrap();
        let from = |look: Option<DVec3>, cursor: Option<DVec3>, face: Option<usize>| OrientFrom {
            look,
            cursor,
            face,
        };
        // looking north the screen's right is east; looking west it is north
        let view = |l: DVec3| {
            zero_way_from(
                &model,
                std::slice::from_ref(&opening),
                Orient::View,
                from(Some(l), None, None),
            )
        };
        assert!(near(view(DVec3::Y), DVec2::X), "{}", view(DVec3::Y));
        assert!(near(view(-DVec3::X), DVec2::Y), "{}", view(-DVec3::X));
        // from straight above there is no screen's right: the plan's east
        assert!(near(view(-DVec3::Z), DVec2::X));
        // from the cursor, 2 m west of its middle: east
        let mid = transform::pivot(&model, opening.id).unwrap();
        let cur = zero_way_from(
            &model,
            std::slice::from_ref(&opening),
            Orient::Cursor,
            from(None, Some(mid - DVec3::X * 2.0), None),
        );
        assert!(near(cur, DVec2::X), "{cur}");
        // along its wall
        let ElementKind::Opening(o) = &opening.kind else {
            unreachable!()
        };
        let Some(ElementKind::Wall(w)) = model.get(o.host).map(|h| &h.kind) else {
            unreachable!()
        };
        let host = zero_way_from(
            &model,
            std::slice::from_ref(&opening),
            Orient::Host,
            OrientFrom::default(),
        );
        assert!(near(host, (w.end - w.start).normalize()), "{host}");
        // a mass's south face: out of it, south
        let mass = Element::new(
            "Mass",
            ElementKind::FreeMesh(creabim_core::FreeMesh::new(
                None,
                DVec3::ZERO,
                creabim_core::EditMesh::cuboid(DVec3::new(2.0, 2.0, 2.0)),
            )),
        );
        let ElementKind::FreeMesh(fm) = &mass.kind else {
            unreachable!()
        };
        let south = (0..fm.mesh.faces.len())
            .find(|f| fm.mesh.face_normal(*f).dot(-DVec3::Y) > 0.99)
            .unwrap();
        let normal = zero_way_from(
            &model,
            std::slice::from_ref(&mass),
            Orient::Normal,
            from(None, None, Some(south)),
        );
        assert!(near(normal, -DVec2::Y), "{normal}");
    }

    /// A 4 m by 2 m slab from the origin, alone on a level, for Scale.
    fn slab_doc() -> (Document, ElementId) {
        use creabim_core::{Element, Level, Model, Slab};
        let lv = Element::new(
            "Ground",
            ElementKind::Level(Level {
                elevation: 0.0,
                height: 3.0,
            }),
        );
        let square = vec![
            DVec2::ZERO,
            DVec2::new(4.0, 0.0),
            DVec2::new(4.0, 2.0),
            DVec2::new(0.0, 2.0),
        ];
        let slab = Element::new("Slab", ElementKind::Slab(Slab::new(lv.id, square, 0.2)));
        let id = slab.id;
        (
            Document::from_model(Model::from_elements([lv, slab]).unwrap()),
            id,
        )
    }

    fn slab_extent(doc: &Document, id: ElementId) -> (DVec2, DVec2) {
        let ElementKind::Slab(s) = &doc.model().get(id).unwrap().kind else {
            unreachable!()
        };
        let lo = s.boundary.iter().copied().reduce(DVec2::min).unwrap();
        let hi = s.boundary.iter().copied().reduce(DVec2::max).unwrap();
        (lo, hi)
    }

    fn scale_from(doc: &Document, id: ElementId, grab: Pos2) -> Modal {
        Modal::start(
            doc,
            Mode::Scale,
            &[id],
            None,
            Space::Plan,
            None,
            grab,
            Pivot::Median,
            None,
            None,
        )
        .unwrap()
        .with_flow(Flow::Keys)
    }

    /// Scale's row (MU142, Tool Rows and Scale REV01): taken by a corner it
    /// scales about the opposite one, which stays where it is; with Alt
    /// about its centre; proportions free, wider and deeper each by how far
    /// the pointer went; by its step.
    #[test]
    fn scale_goes_from_the_opposite_corner_and_free_when_asked() {
        let _guard = SCALE_ROW.lock().unwrap_or_else(|e| e.into_inner());
        set_scale_corner(true);
        set_scale_free(false);
        set_scale_by_ref(false);
        set_scale_step(0.05);
        // taken by its top-right corner, pulled out to half as far again
        let (mut doc, id) = slab_doc();
        let mut m = scale_from(&doc, id, at(4.0, 2.0));
        with_map(&[], |map| m.step(&mut doc, &pointer(at(6.0, 3.0)), map));
        let (lo, hi) = slab_extent(&doc, id);
        assert!(
            lo.distance(DVec2::ZERO) < 1e-6,
            "the opposite corner stays: {lo}"
        );
        assert!(hi.distance(DVec2::new(6.0, 3.0)) < 1e-6, "{hi}");
        m.cancel(&mut doc);
        // with Alt, about its middle
        let mut m = scale_from(&doc, id, at(4.0, 2.0));
        let ev = Events {
            alt: true,
            ..pointer(at(6.0, 3.0))
        };
        with_map(&[], |map| m.step(&mut doc, &ev, map));
        let (lo, hi) = slab_extent(&doc, id);
        assert!(
            ((lo + hi) / 2.0).distance(DVec2::new(2.0, 1.0)) < 1e-6,
            "its middle stays: {lo} {hi}"
        );
        m.cancel(&mut doc);
        // free: twice as wide, as deep as it was
        set_scale_free(true);
        let mut m = scale_from(&doc, id, at(4.0, 2.0));
        with_map(&[], |map| m.step(&mut doc, &pointer(at(8.0, 2.0)), map));
        let (lo, hi) = slab_extent(&doc, id);
        assert!(
            lo.distance(DVec2::ZERO) < 1e-6 && hi.distance(DVec2::new(8.0, 2.0)) < 1e-6,
            "{lo} {hi}"
        );
        m.cancel(&mut doc);
        set_scale_free(false);
    }

    /// MU142 (owner: the Scale button "activates it immediately ... doesn't
    /// give you time to click somewhere where you want to start"): the Scale
    /// tool waits for a click where it starts, as Rotate's does; nothing
    /// changes before it, and from it the pointer has room to go out.
    #[test]
    fn the_scale_tool_waits_for_a_click_where_it_starts() {
        let _guard = SCALE_ROW.lock().unwrap_or_else(|e| e.into_inner());
        set_scale_corner(true);
        set_scale_free(false);
        set_scale_by_ref(false);
        set_scale_step(0.05);
        let (mut doc, id) = slab_doc();
        let was = slab_extent(&doc, id);
        // armed from the button, the pointer coming in near the slab
        let mut m = Modal::start(
            &doc,
            Mode::Scale,
            &[id],
            None,
            Space::Plan,
            None,
            at(1.0, 1.0),
            Pivot::Median,
            None,
            None,
        )
        .unwrap()
        .with_flow(Flow::Tool);
        with_map(&[], |map| {
            m.step(&mut doc, &pointer(at(1.0, 1.0)), map);
            m.step(&mut doc, &pointer(at(0.5, 0.5)), map);
        });
        assert_eq!(
            slab_extent(&doc, id),
            was,
            "nothing changes before the first click"
        );
        assert!(m.waits_to_scale());
        // clicked on its top-right corner, taken out half as far again
        with_map(&[], |map| {
            assert!(matches!(
                m.step(&mut doc, &click(at(4.0, 2.0)), map),
                Outcome::Running(_)
            ));
            m.step(&mut doc, &pointer(at(6.0, 3.0)), map);
        });
        let (lo, hi) = slab_extent(&doc, id);
        assert!(
            lo.distance(DVec2::ZERO) < 1e-6 && hi.distance(DVec2::new(6.0, 3.0)) < 1e-6,
            "{lo} {hi}"
        );
        // and a second click finishes it
        with_map(&[], |map| {
            assert!(matches!(
                m.step(&mut doc, &click(at(6.0, 3.0)), map),
                Outcome::Done(_)
            ))
        });
    }

    /// Scale by reference (MU142): click the two ends of a length known,
    /// type what it should be, and it scales by the two, about the first.
    #[test]
    fn scale_by_a_reference_length() {
        let _guard = SCALE_ROW.lock().unwrap_or_else(|e| e.into_inner());
        set_scale_by_ref(true);
        let (mut doc, id) = slab_doc();
        let mut m = scale_from(&doc, id, at(1.0, 1.0));
        // its bottom edge, 4 m, should be 6 m
        with_map(&[DVec3::ZERO, DVec3::new(4.0, 0.0, 0.0)], |map| {
            assert!(matches!(
                m.step(&mut doc, &click(at(0.0, 0.0)), map),
                Outcome::Running(_)
            ));
            assert!(matches!(
                m.step(&mut doc, &click(at(4.0, 0.0)), map),
                Outcome::Running(_)
            ));
            let typed = Events {
                text: "6m".into(),
                ..pointer(at(5.0, 0.0))
            };
            m.step(&mut doc, &typed, map);
            let enter = Events {
                enter: true,
                ..pointer(at(5.0, 0.0))
            };
            assert!(matches!(m.step(&mut doc, &enter, map), Outcome::Done(_)));
        });
        let (lo, hi) = slab_extent(&doc, id);
        set_scale_by_ref(false);
        assert!(lo.distance(DVec2::ZERO) < 1e-6, "about its first end: {lo}");
        assert!(hi.distance(DVec2::new(6.0, 3.0)) < 1e-6, "× 1.5: {hi}");
    }

    /// Scale's row settings are kept for the session: tests that set them
    /// take turns.
    static SCALE_ROW: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Pick a line (MU142, Revit's Pick Axis): pointing at a line of the
    /// drawing lights it, a click mirrors across it, at whatever angle it
    /// runs; with no line under the pointer nothing happens.
    #[test]
    fn mirror_across_a_picked_line_at_any_angle() {
        let mut doc = Document::from_model(sample_house());
        let (id, _) = first_wall(&doc);
        let ElementKind::Wall(w0) = doc.model().get(id).unwrap().kind.clone() else {
            unreachable!()
        };
        let mut m = tool(&doc, Mode::Mirror, id);
        m.across = Some(Across::PickLine);
        m.set_copy(&mut doc, false);
        let to_world = |p: Pos2| DVec3::new(p.x as f64 / 10.0, -p.y as f64 / 10.0, 0.0);
        let to_model = |p: Pos2, _: Option<Axis>, _: DVec3| Some(to_world(p));
        let project = |p: DVec3| Some(Pos2::new((p.x * 10.0) as f32, (-p.y * 10.0) as f32));
        let snap = |p: Pos2, _: Option<DVec3>| {
            let s = Snap {
                p: to_world(p),
                kind: SnapKind::Grid,
                host: None,
            };
            Some((s.p, s))
        };
        // the line y = x, picked only near it
        let pick = |p: Pos2| {
            let w = to_world(p);
            ((w.x - w.y).abs() < 0.5)
                .then_some((DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 10.0, 0.0)))
        };
        let map = PaneMap {
            to_model: &to_model,
            project: &project,
            look: -DVec3::Z,
            snap: &snap,
            pick_face: None,
            pick_line: Some(&pick),
        };
        // off any line: a click does nothing
        let off = m.step(&mut doc, &click(at(30.0, 2.0)), &map);
        assert!(matches!(off, Outcome::Running(_)));
        assert_eq!(wall_start(&doc, id), w0.start);
        // on the line: lit, then mirrored across it with the click
        m.step(&mut doc, &pointer(at(5.0, 5.0)), &map);
        assert_eq!(wall_start(&doc, id), w0.start, "hovering moves nothing");
        let out = m.step(&mut doc, &click(at(5.0, 5.0)), &map);
        assert!(matches!(out, Outcome::Done(_)), "the click placed it");
        let ElementKind::Wall(w) = &doc.model().get(id).unwrap().kind else {
            unreachable!()
        };
        let swap = |p: DVec2| DVec2::new(p.y, p.x);
        assert!(w.start.distance(swap(w0.start)) < 1e-9, "{:?}", w.start);
        assert!(w.end.distance(swap(w0.end)) < 1e-9);
    }

    #[test]
    fn mirror_makes_a_mirrored_copy_across_two_clicked_points() {
        let mut doc = Document::from_model(sample_house());
        let (id, _) = first_wall(&doc);
        let ElementKind::Wall(w0) = doc.model().get(id).unwrap().kind.clone() else {
            unreachable!()
        };
        let n = doc.model().len();
        let mut m = tool(&doc, Mode::Mirror, id);
        assert_eq!(m.stage(), Stage::Base);
        // the mirror line: x = 20, up the plan
        with_map(&[], |map| {
            m.step(&mut doc, &click(at(20.0, 0.0)), map);
            assert_eq!(m.stage(), Stage::Follow);
            m.step(&mut doc, &pointer(at(20.0, 5.0)), map);
            let out = m.step(&mut doc, &click(at(20.0, 5.0)), map);
            assert!(matches!(out, Outcome::Done(_)));
        });
        // the original stays; the copy (and its doors) is across the line
        assert_eq!(wall_start(&doc, id), w0.start);
        assert!(doc.model().len() > n);
        let copy = doc
            .model()
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::Wall(w) if e.id != id && e.name.ends_with("copy") => Some(w.clone()),
                _ => None,
            })
            .unwrap();
        let across = |p: DVec2| DVec2::new(40.0 - p.x, p.y);
        assert!(copy.start.distance(across(w0.start)) < 1e-9);
        assert!(copy.end.distance(across(w0.end)) < 1e-9);
        assert_eq!(copy.flip, !w0.flip);
        // one undo takes it all away
        doc.undo();
        assert_eq!(doc.model().len(), n);
    }

    #[test]
    fn offset_copies_a_wall_to_the_pointers_side_or_by_a_typed_distance() {
        let mut doc = Document::from_model(sample_house());
        let (id, _) = first_wall(&doc);
        let ElementKind::Wall(w0) = doc.model().get(id).unwrap().kind.clone() else {
            unreachable!()
        };
        let left = w0.direction().perp();
        let mid = (w0.start + w0.end) / 2.0;
        let mut m = tool(&doc, Mode::Offset, id);
        // no base point: it follows the pointer at once
        assert_eq!(m.stage(), Stage::Follow);
        let p = mid + left * 2.0;
        with_map(&[], |map| {
            m.step(&mut doc, &pointer(at(p.x, p.y)), map);
            let typed = Events {
                pointer: Some(at(p.x, p.y)),
                text: "1.5m".into(),
                ..Default::default()
            };
            m.step(&mut doc, &typed, map);
            let out = m.step(&mut doc, &click(at(p.x, p.y)), map);
            assert!(matches!(out, Outcome::Done(_)));
        });
        assert_eq!(wall_start(&doc, id), w0.start);
        let copy = doc
            .model()
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::Wall(w) if e.id != id && e.name.ends_with("copy") => Some(w.clone()),
                _ => None,
            })
            .unwrap();
        assert!(
            copy.start.distance(w0.start + left * 1.5) < 1e-6,
            "{:?}",
            copy.start
        );
    }

    fn first_slab(doc: &Document) -> (ElementId, creabim_core::Slab) {
        doc.model()
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::Slab(s) if s.boundary.len() >= 4 && s.holes.is_empty() => {
                    Some((e.id, s.clone()))
                }
                _ => None,
            })
            .unwrap()
    }

    fn slab(doc: &Document, id: ElementId) -> creabim_core::Slab {
        match &doc.model().get(id).unwrap().kind {
            ElementKind::Slab(s) => s.clone(),
            _ => unreachable!(),
        }
    }

    /// Edge `i` of `s`: its middle, length and the way out of the outline.
    fn edge_out(s: &creabim_core::Slab, i: usize) -> (DVec2, f64, DVec2) {
        let n = s.boundary.len();
        let (a, b) = (s.boundary[i], s.boundary[(i + 1) % n]);
        let d = (b - a).normalize();
        let ccw = creabim_core::polygon::signed_area2(&s.boundary) > 0.0;
        let out = if ccw { -d.perp() } else { d.perp() };
        ((a + b) / 2.0, a.distance(b), out)
    }

    #[test]
    fn the_edge_palette_offsets_an_edge_offsets_all_and_moves_an_inserted_point() {
        use crate::handles::{HandleKind, Ring};
        let mut doc = Document::from_model(sample_house());
        let (id, s0) = first_slab(&doc);
        let count = doc.model().iter().count();
        let (mid, len, out) = edge_out(&s0, 0);
        let typed = |p: DVec2, text: &str| Events {
            pointer: Some(at(p.x, p.y)),
            text: text.into(),
            ..Default::default()
        };
        // Offset edge: the pointer outside, then a typed 600 mm
        let mut m = tool(&doc, Mode::Grip, id)
            .with_grip(
                &doc,
                HandleKind::Edge {
                    ring: Ring::Outline,
                    i: 0,
                },
            )
            .unwrap();
        assert_eq!(m.stage(), Stage::Follow);
        let p = mid + out * 2.0;
        with_map(&[], |map| {
            m.step(&mut doc, &pointer(at(p.x, p.y)), map);
            m.step(&mut doc, &typed(p, "600mm"), map);
            assert!(matches!(
                m.step(&mut doc, &click(at(p.x, p.y)), map),
                Outcome::Done(_)
            ));
        });
        let s1 = slab(&doc, id);
        assert!(
            (s1.area() - s0.area() - len * 0.6).abs() < 1e-6,
            "{} -> {}",
            s0.area(),
            s1.area()
        );
        // Offset all, in place: no copy, every edge 300 mm out
        let mut m = tool(&doc, Mode::Offset, id);
        m.set_copy(&mut doc, false);
        let (mid, _, out) = edge_out(&s1, 1);
        let p = mid + out * 1.0;
        with_map(&[], |map| {
            m.step(&mut doc, &pointer(at(p.x, p.y)), map);
            m.step(&mut doc, &typed(p, "300mm"), map);
            assert!(matches!(
                m.step(&mut doc, &click(at(p.x, p.y)), map),
                Outcome::Done(_)
            ));
        });
        assert_eq!(doc.model().iter().count(), count, "no copy made");
        let s2 = slab(&doc, id);
        assert!(s2.area() > s1.area() + 0.3 * 4.0, "{}", s2.area());
        // Insert point: a corner in edge 0's middle, then 500 mm out
        crate::editmode::add_corner(&mut doc, id, Ring::Outline, 0, None).unwrap();
        let s3 = slab(&doc, id);
        assert_eq!(s3.boundary.len(), s2.boundary.len() + 1);
        let c = s3.boundary[1];
        let mut m = tool(&doc, Mode::Grip, id)
            .with_grip(
                &doc,
                HandleKind::Vertex {
                    ring: Ring::Outline,
                    i: 1,
                },
            )
            .unwrap();
        let (_, _, out) = edge_out(&s2, 0);
        let p = c + out * 3.0;
        with_map(&[], |map| {
            m.step(&mut doc, &pointer(at(p.x, p.y)), map);
            m.step(&mut doc, &typed(p, "500mm"), map);
            assert!(matches!(
                m.step(&mut doc, &click(at(p.x, p.y)), map),
                Outcome::Done(_)
            ));
        });
        let moved = slab(&doc, id).boundary[1];
        assert!(moved.distance(c + out * 0.5) < 1e-6, "{moved:?} from {c:?}");
    }

    #[test]
    fn on_an_upright_workplane_rotate_turns_a_part_about_its_normal() {
        use creabim_core::parts::{Form, Part};
        let model = sample_house();
        let level = model
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Level(_)))
            .unwrap()
            .id;
        let face = Workplane::facing(DVec3::new(0.0, 0.0, 1.0), -DVec3::Y);
        let sq = creabim_core::polygon::rectangle(DVec2::ZERO, DVec2::new(2.0, 0.5));
        let mut p = Part::extrusion(level, sq, 0.0, 0.1);
        if let Form::Extrusion { plane, .. } = &mut p.form {
            *plane = Some(face);
        }
        let e = Element::new("Panel", ElementKind::Part(p));
        let pid = e.id;
        let mut elements: Vec<Element> = model.iter().cloned().collect();
        elements.push(e);
        let mut doc = Document::from_model(creabim_core::Model::from_elements(elements).unwrap());
        let mut m = Modal::start(
            &doc,
            Mode::Rotate,
            &[pid],
            None,
            Space::Model,
            None,
            Pos2::ZERO,
            Pivot::Median,
            None,
            Some(face),
        )
        .unwrap()
        .with_flow(Flow::Tool);
        // a map onto the face (x across it, z up it), seen from in front
        let to_world = |p: Pos2| DVec3::new(p.x as f64 / 10.0, 0.0, -p.y as f64 / 10.0);
        let to_model = |p: Pos2, _: Option<Axis>, _: DVec3| Some(to_world(p));
        let project = |p: DVec3| Some(Pos2::new((p.x * 10.0) as f32, (-p.z * 10.0) as f32));
        let snap = |p: Pos2, _: Option<DVec3>| {
            let s = Snap {
                p: to_world(p),
                kind: SnapKind::Endpoint,
                host: None,
            };
            Some((s.p, s))
        };
        let map = PaneMap {
            to_model: &to_model,
            project: &project,
            look: DVec3::Y,
            snap: &snap,
            pick_face: None,
            pick_line: None,
        };
        let on = |x: f64, z: f64| Pos2::new((x * 10.0) as f32, (-z * 10.0) as f32);
        // centre, reference along the face, then a quarter turn up it
        m.step(&mut doc, &click(on(0.0, 1.0)), &map);
        m.step(&mut doc, &click(on(1.0, 1.0)), &map);
        m.step(&mut doc, &pointer(on(0.0, 2.0)), &map);
        let out = m.step(&mut doc, &click(on(0.0, 2.0)), &map);
        assert!(matches!(out, Outcome::Done(_)));
        let ElementKind::Part(p) = &doc.model().get(pid).unwrap().kind else {
            unreachable!()
        };
        let Form::Extrusion { plane: Some(q), .. } = &p.form else {
            unreachable!()
        };
        // still on the face, its x axis now pointing up it
        assert!((q.normal() + DVec3::Y).length() < 1e-9);
        assert!((q.x - DVec3::Z).length() < 1e-9, "{}", q.x);
    }

    #[test]
    fn a_live_edit_asked_the_same_again_leaves_the_model_as_it_is() {
        let mut doc = Document::from_model(sample_house());
        let (id, start) = first_wall(&doc);
        let original = doc.model().get(id).unwrap().clone();
        let moved = |by: DVec3| {
            vec![transform::transformed(doc.model(), &original, &Xform::Move(by)).unwrap()]
        };
        let (a, b) = (
            moved(DVec3::new(0.5, 0.0, 0.0)),
            moved(DVec3::new(0.7, 0.0, 0.0)),
        );
        let steps = doc.undo_steps().len();
        let mut edit = LiveEdit::new("Move wall", vec![original.clone()]);
        edit.update(&mut doc, a.clone()).unwrap();
        let rev = doc.model().revision();
        assert_eq!(wall_start(&doc, id), start + DVec2::new(0.5, 0.0));
        // the pointer at rest: nothing is redone
        edit.update(&mut doc, a.clone()).unwrap();
        assert_eq!(doc.model().revision(), rev);
        // it moves on: the model follows, still as one undo step
        edit.update(&mut doc, b).unwrap();
        assert_ne!(doc.model().revision(), rev);
        assert_eq!(wall_start(&doc, id), start + DVec2::new(0.7, 0.0));
        assert_eq!(doc.undo_steps().len(), steps + 1);
        // taken back and asked the same again: shown again
        edit.update(&mut doc, a.clone()).unwrap();
        edit.cancel(&mut doc);
        assert_eq!(wall_start(&doc, id), start);
        edit.update(&mut doc, a).unwrap();
        assert_eq!(wall_start(&doc, id), start + DVec2::new(0.5, 0.0));
        edit.cancel(&mut doc);
        assert_eq!(doc.undo_steps().len(), steps);
    }

    #[test]
    fn duplicating_a_view_copies_it_at_once() {
        // #35: Duplicate on a view did nothing ("That can't be duplicated.")
        let mut doc = Document::from_model(sample_house());
        let view = doc
            .model()
            .iter()
            .find(|e| e.name == "First Floor plan")
            .map(|e| e.id)
            .unwrap();
        let steps = doc.undo_steps().len();
        let copies = duplicate_views(&mut doc, &[view]).unwrap().unwrap();
        assert_eq!(copies.len(), 1);
        let (orig, copy) = (
            doc.model().get(view).unwrap(),
            doc.model().get(copies[0]).unwrap(),
        );
        assert_eq!(copy.name, "First Floor plan copy");
        assert_eq!(
            serde_json::to_value(&copy.kind).unwrap(),
            serde_json::to_value(&orig.kind).unwrap(),
            "the same settings"
        );
        assert_eq!(doc.undo_steps().len(), steps + 1, "one undo step");
        // anything but views keeps the copy that follows the pointer
        let (wall, _) = first_wall(&doc);
        assert!(duplicate_views(&mut doc, &[wall]).is_none());
        assert!(duplicate_views(&mut doc, &[view, wall]).is_none());
    }

    #[test]
    fn cancelling_a_duplicate_takes_its_copy_away_and_keeps_the_rest() {
        // MU23: a copy left on its original was invisible, and every
        // Duplicate after stacked another wall on the same spot
        let mut doc = Document::from_model(sample_house());
        let (id, start) = first_wall(&doc);
        let n = doc.model().len();
        let steps = doc.undo_steps().len();
        let copies = duplicate(&mut doc, &[id]).unwrap();
        assert!(doc.model().len() > n);
        let mut m = tool(&doc, Mode::Move, copies[0]);
        m.duplicated = true;
        m.cancel(&mut doc);
        assert_eq!(doc.model().len(), n, "the copy is gone");
        assert_eq!(doc.undo_steps().len(), steps, "and its undo step");
        assert_eq!(wall_start(&doc, id), start, "the original is untouched");
        // a second cancel takes nothing else away
        m.cancel(&mut doc);
        assert_eq!(doc.model().len(), n);
    }

    #[test]
    fn a_duplicate_moved_away_leaves_one_wall_on_the_original() {
        // MU25: after Duplicate, base point, target, a second wall still
        // sat on the original (Tab cycled two)
        let mut doc = Document::from_model(sample_house());
        let (id, start) = first_wall(&doc);
        let (end, level) = match &doc.model().get(id).unwrap().kind {
            ElementKind::Wall(w) => (w.end, w.level),
            _ => unreachable!(),
        };
        let walls = |doc: &Document| {
            doc.model()
                .iter()
                .filter(|e| {
                    matches!(&e.kind, ElementKind::Wall(w)
                        if w.level == level && w.start.distance(start) < 1e-6 && w.end.distance(end) < 1e-6)
                })
                .count()
        };
        assert_eq!(walls(&doc), 1);
        let n = doc.model().len();
        let (mut m, copies) = crate::planview::start_modal(
            &mut doc,
            (Mode::Move, true),
            &[id],
            None,
            Space::Plan,
            None,
            at(1.0, 1.0),
            Pivot::Median,
            None,
            Flow::Tool,
            Target::Object,
            None,
        )
        .unwrap();
        let n = n + copies.len();
        let out = with_map(&[], |map| {
            for p in [at(1.0, 1.0), at(1.5, 1.2), at(2.0, 2.0)] {
                m.step(&mut doc, &pointer(p), map);
            }
            m.step(&mut doc, &click(at(2.0, 2.0)), map);
            for p in [at(2.5, 2.0), at(3.0, 2.0), at(3.5, 2.0)] {
                m.step(&mut doc, &pointer(p), map);
            }
            m.step(&mut doc, &click(at(3.5, 2.0)), map)
        });
        assert!(matches!(out, Outcome::Done(_)), "placed");
        assert_eq!(doc.model().len(), n, "one copy made");
        assert_eq!(walls(&doc), 1, "only the original on its spot");
        let moved = wall_start(&doc, copies[0]);
        assert!(
            (moved - start - DVec2::new(1.5, 0.0)).length() < 1e-6,
            "{moved} {start}"
        );
        assert_eq!(wall_start(&doc, id), start);
    }

    #[test]
    fn a_move_leaving_a_copy_behind_does_not_drag_the_copy_along() {
        // MU25: Ctrl while moving leaves a copy; nothing may end up lying
        // on the original, nor the original dragged off by its copy
        let mut doc = Document::from_model(sample_house());
        let (id, start) = first_wall(&doc);
        let (end, level) = match &doc.model().get(id).unwrap().kind {
            ElementKind::Wall(w) => (w.end, w.level),
            _ => unreachable!(),
        };
        let on_spot = |doc: &Document| {
            doc.model()
                .iter()
                .filter(|e| {
                    matches!(&e.kind, ElementKind::Wall(w)
                        if w.level == level && w.start.distance(start) < 1e-6 && w.end.distance(end) < 1e-6)
                })
                .count()
        };
        let n = doc.model().len();
        let mut m = tool(&doc, Mode::Move, id);
        m.set_copy(&mut doc, true);
        with_map(&[], |map| {
            m.step(&mut doc, &click(at(2.0, 2.0)), map);
            m.step(&mut doc, &pointer(at(3.0, 2.0)), map);
            m.step(&mut doc, &click(at(3.5, 2.0)), map);
            m.step(
                &mut doc,
                &Events {
                    escape: true,
                    ..Default::default()
                },
                map,
            );
        });
        assert!(doc.model().len() > n, "a copy was placed");
        assert_eq!(on_spot(&doc), 1, "one wall on the original spot");
    }

    #[test]
    fn a_duplicate_placed_where_it_began_takes_its_copy_away() {
        // MU24: base point and target snapped to the same spot left the
        // copy on its original, unseen
        let mut doc = Document::from_model(sample_house());
        let (id, start) = first_wall(&doc);
        let n = doc.model().len();
        let copies = duplicate(&mut doc, &[id]).unwrap();
        let mut m = tool(&doc, Mode::Move, copies[0]);
        m.duplicated = true;
        let out = with_map(&[], |map| {
            m.step(&mut doc, &click(at(2.0, 2.0)), map);
            m.step(&mut doc, &click(at(2.0, 2.0)), map)
        });
        assert!(matches!(out, Outcome::Cancelled(_)));
        assert_eq!(doc.model().len(), n, "no copy left on the original");
        assert_eq!(wall_start(&doc, id), start);
    }

    #[test]
    fn nothing_moves_before_the_base_point() {
        let mut doc = Document::from_model(sample_house());
        let (id, start) = first_wall(&doc);
        let mut m = tool(&doc, Mode::Move, id);
        let steps = doc.undo_steps().len();
        with_map(&[], |map| {
            assert_eq!(m.stage(), Stage::Base);
            for p in [at(1.0, 1.0), at(5.0, -3.0), at(20.0, 9.0)] {
                assert!(matches!(
                    m.step(&mut doc, &pointer(p), map),
                    Outcome::Running(_)
                ));
                assert_eq!(wall_start(&doc, id), start);
            }
            assert_eq!(doc.undo_steps().len(), steps);
            // the base click itself moves nothing either
            m.step(&mut doc, &click(at(2.0, 2.0)), map);
            assert_eq!(m.stage(), Stage::Follow);
            assert_eq!(wall_start(&doc, id), start);
            // nor does a pointer that hasn't left the base point
            m.step(&mut doc, &pointer(at(2.0, 2.0) + egui::vec2(1.0, 1.0)), map);
            assert_eq!(wall_start(&doc, id), start);
        });
        assert_eq!(doc.undo_steps().len(), steps);
    }

    #[test]
    fn the_base_point_goes_to_the_snapped_destination() {
        let mut doc = Document::from_model(sample_house());
        let (id, start) = first_wall(&doc);
        let mut m = tool(&doc, Mode::Move, id);
        let base = DVec3::new(2.0, 2.0, 0.0);
        let target = DVec3::new(7.0, 4.0, 0.0);
        let steps = doc.undo_steps().len();
        with_map(&[base, target], |map| {
            // near the points, not on them: they snap
            m.step(&mut doc, &click(at(2.2, 1.9)), map);
            m.step(&mut doc, &pointer(at(6.8, 4.3)), map);
            assert_eq!(wall_start(&doc, id), start + DVec2::new(5.0, 2.0));
            assert!(matches!(
                m.step(&mut doc, &click(at(6.8, 4.3)), map),
                Outcome::Done(_)
            ));
        });
        assert_eq!(wall_start(&doc, id), start + DVec2::new(5.0, 2.0));
        // one move, one undo step
        assert_eq!(doc.undo_steps().len(), steps + 1);
        doc.undo();
        assert_eq!(wall_start(&doc, id), start);
    }

    #[test]
    fn a_typed_distance_goes_the_way_the_pointer_points() {
        let mut doc = Document::from_model(sample_house());
        let (id, start) = first_wall(&doc);
        let mut m = tool(&doc, Mode::Move, id);
        with_map(&[], |map| {
            m.step(&mut doc, &click(at(0.0, 0.0)), map);
            m.step(&mut doc, &pointer(at(0.0, 3.3)), map);
            let typed = Events {
                text: "2400mm".into(),
                ..pointer(at(0.0, 3.3))
            };
            m.step(&mut doc, &typed, map);
            assert!((m.delta() - DVec3::new(0.0, 2.4, 0.0)).length() < 1e-9);
            let enter = Events {
                enter: true,
                ..pointer(at(0.0, 3.3))
            };
            assert!(matches!(m.step(&mut doc, &enter, map), Outcome::Done(_)));
        });
        assert!((wall_start(&doc, id) - (start + DVec2::new(0.0, 2.4))).length() < 1e-9);
    }

    #[test]
    fn shift_keeps_square_and_an_axis_key_keeps_to_it() {
        let d = move_delta(
            DVec3::ZERO,
            DVec3::new(3.0, 1.0, 0.0),
            None,
            Some([DVec3::X, DVec3::Y]),
            None,
            DVec3::X,
            false,
        );
        assert_eq!(d, DVec3::new(3.0, 0.0, 0.0));
        let d = move_delta(
            DVec3::ZERO,
            DVec3::new(3.0, 1.0, 0.5),
            Some(Axis::Y),
            None,
            None,
            DVec3::X,
            false,
        );
        assert_eq!(d, DVec3::new(0.0, 1.0, 0.0));
        // in 3D things stay on their level unless kept to Z
        let d = move_delta(
            DVec3::ZERO,
            DVec3::new(3.0, 1.0, 2.0),
            None,
            None,
            None,
            DVec3::X,
            true,
        );
        assert_eq!(d, DVec3::new(3.0, 1.0, 0.0));
        assert_eq!(parse_length("120cm"), Some(1.2));
        assert_eq!(parse_length("-"), None);
    }

    #[test]
    fn ctrl_leaves_copies_behind_one_undo_step_each() {
        let mut doc = Document::from_model(sample_house());
        let (id, start) = first_wall(&doc);
        let walls = |doc: &Document| {
            doc.model()
                .iter()
                .filter(|e| matches!(e.kind, ElementKind::Wall(_)))
                .count()
        };
        let before = walls(&doc);
        let steps = doc.undo_steps().len();
        let mut m = tool(&doc, Mode::Move, id);
        with_map(&[], |map| {
            m.step(&mut doc, &click(at(0.0, 0.0)), map);
            let ctrl = Events {
                ctrl: true,
                ctrl_pressed: true,
                ..pointer(at(0.0, 0.0))
            };
            m.step(&mut doc, &ctrl, map);
            assert!(m.copy);
            for i in 1..=2 {
                let p = at(0.0, 3.0 * i as f64);
                m.step(&mut doc, &pointer(p), map);
                // the preview is a copy: the original stays
                assert_eq!(wall_start(&doc, id), start);
                assert_eq!(walls(&doc), before + i);
                assert!(matches!(
                    m.step(&mut doc, &click(p), map),
                    Outcome::Running(_)
                ));
            }
            // Esc leaves the follow stage, then the tool, keeping the copies
            let esc = Events {
                escape: true,
                ..pointer(at(5.0, 5.0))
            };
            assert!(matches!(m.step(&mut doc, &esc, map), Outcome::Running(_)));
            assert!(matches!(m.step(&mut doc, &esc, map), Outcome::Done(_)));
        });
        assert_eq!(walls(&doc), before + 2);
        assert_eq!(wall_start(&doc, id), start);
        assert_eq!(doc.undo_steps().len(), steps + 2);
        doc.undo();
        assert_eq!(walls(&doc), before + 1);
    }

    #[test]
    fn esc_steps_back_then_cancels_leaving_everything_as_it_was() {
        let mut doc = Document::from_model(sample_house());
        let (id, start) = first_wall(&doc);
        let steps = doc.undo_steps().len();
        let mut m = tool(&doc, Mode::Move, id);
        with_map(&[], |map| {
            m.step(&mut doc, &click(at(0.0, 0.0)), map);
            m.step(&mut doc, &pointer(at(4.0, 0.0)), map);
            assert_ne!(wall_start(&doc, id), start);
            let right = Events {
                right: true,
                ..pointer(at(4.0, 0.0))
            };
            // back to picking the base point, with the wall put back
            assert!(matches!(m.step(&mut doc, &right, map), Outcome::Running(_)));
            assert_eq!(m.stage(), Stage::Base);
            assert_eq!(wall_start(&doc, id), start);
            assert!(matches!(
                m.step(&mut doc, &right, map),
                Outcome::Cancelled(_)
            ));
        });
        assert_eq!(wall_start(&doc, id), start);
        assert_eq!(doc.undo_steps().len(), steps);
    }

    #[test]
    fn esc_cancels_a_key_move_as_a_right_click_does() {
        // G, move, then Esc: everything back, nothing left to undo
        for by_esc in [true, false] {
            let mut doc = Document::from_model(sample_house());
            let (id, start) = first_wall(&doc);
            let steps = doc.undo_steps().len();
            let grab = at(3.0, 3.0);
            let mut m = Modal::start(
                &doc,
                Mode::Move,
                &[id],
                None,
                Space::Model,
                None,
                grab,
                Pivot::Median,
                None,
                None,
            )
            .unwrap();
            with_map(&[], |map| {
                m.step(&mut doc, &pointer(grab), map);
                m.step(&mut doc, &pointer(at(8.0, 3.0)), map);
                assert_ne!(wall_start(&doc, id), start);
                let stop = Events {
                    escape: by_esc,
                    right: !by_esc,
                    ..pointer(at(8.0, 3.0))
                };
                assert!(matches!(
                    m.step(&mut doc, &stop, map),
                    Outcome::Cancelled(_)
                ));
            });
            assert_eq!(wall_start(&doc, id), start);
            assert_eq!(doc.undo_steps().len(), steps);
        }
    }

    #[test]
    fn a_key_move_starts_at_the_pointer_without_a_jump() {
        let mut doc = Document::from_model(sample_house());
        let (id, start) = first_wall(&doc);
        let grab = at(3.0, 3.0);
        let mut m = Modal::start(
            &doc,
            Mode::Move,
            &[id],
            None,
            Space::Plan,
            None,
            grab,
            Pivot::Median,
            None,
            None,
        )
        .unwrap();
        // an endpoint near the pointer: the grabbed point snaps to it
        let corner = DVec3::new(3.3, 3.0, 0.0);
        with_map(&[corner], |map| {
            m.step(&mut doc, &pointer(grab), map);
            assert_eq!(wall_start(&doc, id), start);
            m.step(&mut doc, &pointer(at(8.0, 3.0)), map);
            let moved = start + DVec2::new(8.0 - 3.3, 0.0);
            assert!((wall_start(&doc, id) - moved).length() < 1e-6);
            assert!(matches!(
                m.step(&mut doc, &click(at(8.0, 3.0)), map),
                Outcome::Done(_)
            ));
        });
    }

    #[test]
    fn a_moved_building_takes_its_ceilings_along() {
        // the house box-selected in 3D takes its ceilings with it: they
        // were counted as "can't take this", and one with an outline of
        // its own stayed where the house had been
        let mut doc = Document::from_model(creabim_core::starter::starter_house());
        // one ceiling with its own outline, the rest following their rooms
        let own = doc
            .model()
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Ceiling(_)))
            .cloned()
            .unwrap();
        let mut with_outline = own.clone();
        if let ElementKind::Ceiling(c) = &mut with_outline.kind {
            c.boundary = c.outline(doc.model()).unwrap();
            c.room = None;
        }
        doc.commit(Transaction::new("Outline").replace(own.clone(), with_outline))
            .unwrap();
        let ids: Vec<ElementId> = doc
            .model()
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    ElementKind::Wall(_) | ElementKind::Slab(_) | ElementKind::Ceiling(_)
                )
            })
            .map(|e| e.id)
            .collect();
        let middles = |doc: &Document| -> Vec<(ElementId, DVec2)> {
            doc.model()
                .iter()
                .filter_map(|e| match &e.kind {
                    ElementKind::Ceiling(c) => {
                        let o = c.outline(doc.model())?;
                        Some((e.id, o.iter().copied().sum::<DVec2>() / o.len() as f64))
                    }
                    _ => None,
                })
                .collect()
        };
        let room_at = |doc: &Document| -> DVec2 {
            doc.model()
                .iter()
                .find_map(|e| match &e.kind {
                    ElementKind::Room(r) => Some(r.at),
                    _ => None,
                })
                .unwrap()
        };
        let (before, room_was) = (middles(&doc), room_at(&doc));
        assert!(before.len() >= 9);
        let grab = at(3.0, 3.0);
        let mut m = Modal::start(
            &doc,
            Mode::Move,
            &ids,
            None,
            Space::Model,
            None,
            grab,
            Pivot::Median,
            None,
            None,
        )
        .unwrap();
        assert_eq!(m.skipped, 0, "every ceiling can take a move");
        with_map(&[], |map| {
            m.step(&mut doc, &pointer(grab), map);
            m.step(&mut doc, &pointer(at(8.0, 5.0)), map);
        });
        let by = room_at(&doc) - room_was;
        assert!(by.length() > 1.0, "the house moved: {by}");
        let after = middles(&doc);
        assert_eq!(after.len(), before.len());
        for ((id, was), (_, now)) in before.iter().zip(&after) {
            assert!(
                (*now - *was - by).length() < 1e-6,
                "{id:?}: {}",
                *now - *was
            );
        }
    }

    #[test]
    fn a_moved_building_takes_its_rooms_along_and_esc_puts_them_back() {
        // the whole house box-selected in 3D and moved: its room names
        // stayed on the drawing where the house had been
        let mut doc = Document::from_model(creabim_core::starter::starter_house());
        let ids: Vec<ElementId> = doc
            .model()
            .iter()
            .filter(|e| matches!(e.kind, ElementKind::Wall(_) | ElementKind::Slab(_)))
            .map(|e| e.id)
            .collect();
        let rooms = |doc: &Document| -> Vec<(ElementId, DVec2)> {
            doc.model()
                .iter()
                .filter_map(|e| match &e.kind {
                    ElementKind::Room(r) => Some((e.id, r.at)),
                    _ => None,
                })
                .collect()
        };
        let before = rooms(&doc);
        assert!(before.len() >= 10);
        let (wall, wall_was) = first_wall(&doc);
        let grab = at(3.0, 3.0);
        let mut m = Modal::start(
            &doc,
            Mode::Move,
            &ids,
            None,
            Space::Model,
            None,
            grab,
            Pivot::Median,
            None,
            None,
        )
        .unwrap();
        // the label still counts what was selected
        assert_eq!(m.edit.label(), format!("Move {} elements", ids.len()));
        with_map(&[], |map| {
            m.step(&mut doc, &pointer(grab), map);
            m.step(&mut doc, &pointer(at(8.0, 5.0)), map);
            // as far as the walls went
            let by = wall_start(&doc, wall) - wall_was;
            assert!(by.length() > 4.0);
            for ((id, was), (_, now)) in before.iter().zip(rooms(&doc)) {
                assert!(
                    (now - *was - by).length() < 1e-6,
                    "room {:?} went {:?}, the walls {by:?}; {} skipped, {} originals of {} + {}",
                    doc.model().get(*id).map(|e| e.name.clone()),
                    now - *was,
                    m.skipped,
                    m.edit.originals().len(),
                    ids.len(),
                    before.len()
                );
            }
            // every room is still closed off by its walls
            for (id, _) in &before {
                assert!(creabim_core::room::shape(doc.model(), *id).is_some());
            }
            m.cancel(&mut doc);
        });
        assert_eq!(rooms(&doc), before);
        // a turn takes the rooms along too
        let mut m = Modal::start(
            &doc,
            Mode::Rotate,
            &ids,
            None,
            Space::Plan,
            None,
            grab,
            Pivot::Median,
            None,
            None,
        )
        .unwrap();
        let n = m.edit.originals().len();
        assert_eq!(n, ids.len() + before.len());
        m.cancel(&mut doc);
    }

    /// R turns an object about its middle (MU142: it turned about its back
    /// edge, so a chair swung out of place as it turned).
    #[test]
    fn r_turns_an_object_about_its_middle() {
        let model = creabim_core::ridgeline::ridgeline_house();
        let chair = model
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Object(_)) && e.name.contains("chair"))
            .expect("a chair in Ridgeline");
        let id = chair.id;
        let middle = |doc: &Document| {
            let e = doc.model().get(id).unwrap();
            let (ElementKind::Object(o), Some(t)) =
                (&e.kind, creabim_core::objects::type_of(doc.model(), e))
            else {
                unreachable!()
            };
            let (lo, hi) = t.bounds();
            o.to_plan((lo.truncate() + hi.truncate()) / 2.0)
        };
        let mut doc = Document::from_model(model);
        let before = middle(&doc);
        let mut m = tool(&doc, Mode::Rotate, id).with_flow(Flow::Keys);
        let out = with_map(&[], |map| {
            m.step(&mut doc, &pointer(at(1.0, 1.0)), map);
            m.typed = "180".into();
            m.ask = Some(true);
            m.step(&mut doc, &pointer(at(1.0, 1.0)), map)
        });
        assert!(matches!(out, Outcome::Done(_)));
        assert!(
            middle(&doc).distance(before) < 1e-6,
            "{:?} from {before:?}",
            middle(&doc)
        );
    }

    /// The bar's ↺90 / 180° / ↻90 (MU142) put the angle in and finish, from
    /// the R key and from the Rotate tool once it has its centre.
    #[test]
    fn a_quick_turn_on_the_bar_turns_and_finishes() {
        let dir = |doc: &Document, id| match &doc.model().get(id).unwrap().kind {
            ElementKind::Wall(w) => w.direction(),
            _ => unreachable!(),
        };
        let quick = |m: &mut Modal, by: &str| {
            m.typed = by.into();
            m.ask = Some(true);
        };
        for (by, deg) in [("90", 90.0), ("180", 180.0), ("-90", -90.0)] {
            // the R key
            let mut doc = Document::from_model(sample_house());
            let (id, _) = first_wall(&doc);
            let before = dir(&doc, id);
            let mut m = tool(&doc, Mode::Rotate, id).with_flow(Flow::Keys);
            let out = with_map(&[], |map| {
                m.step(&mut doc, &pointer(at(1.0, 1.0)), map);
                quick(&mut m, by);
                m.step(&mut doc, &pointer(at(1.0, 1.0)), map)
            });
            assert!(matches!(out, Outcome::Done(_)), "{by}");
            let want = DVec2::from_angle(f64::to_radians(deg)).rotate(before);
            assert!(
                (dir(&doc, id) - want).length() < 1e-6,
                "{by}: {:?}",
                dir(&doc, id)
            );
            // the Rotate tool, after its centre and reference
            let mut doc = Document::from_model(sample_house());
            let (id, _) = first_wall(&doc);
            let mut m = tool(&doc, Mode::Rotate, id);
            let out = with_map(&[], |map| {
                m.step(&mut doc, &click(at(0.0, 0.0)), map);
                m.step(&mut doc, &click(at(5.0, 0.0)), map);
                m.step(&mut doc, &pointer(at(3.0, 4.0)), map);
                quick(&mut m, by);
                m.step(&mut doc, &pointer(at(3.0, 4.0)), map)
            });
            assert!(matches!(out, Outcome::Done(_)), "tool {by}");
            assert!(
                (dir(&doc, id) - want).length() < 1e-6,
                "tool {by}: {:?}",
                dir(&doc, id)
            );
        }
    }

    #[test]
    fn the_rotate_tool_turns_from_the_reference_to_the_pointer() {
        let mut doc = Document::from_model(sample_house());
        let (id, _) = first_wall(&doc);
        let dir = |doc: &Document| match &doc.model().get(id).unwrap().kind {
            ElementKind::Wall(w) => w.direction(),
            _ => unreachable!(),
        };
        let before = dir(&doc);
        let mut m = tool(&doc, Mode::Rotate, id);
        with_map(&[], |map| {
            m.step(&mut doc, &pointer(at(0.0, 0.0)), map);
            assert_eq!(dir(&doc), before);
            m.step(&mut doc, &click(at(0.0, 0.0)), map);
            assert_eq!(m.stage(), Stage::Reference);
            m.step(&mut doc, &click(at(5.0, 0.0)), map);
            assert_eq!(m.stage(), Stage::Follow);
            assert_eq!(dir(&doc), before);
            m.step(&mut doc, &pointer(at(0.0, 5.0)), map);
            assert!(matches!(
                m.step(&mut doc, &click(at(0.0, 5.0)), map),
                Outcome::Done(_)
            ));
        });
        let turned = DVec2::from_angle(90f64.to_radians()).rotate(before);
        assert!((dir(&doc) - turned).length() < 1e-6);
    }

    #[test]
    fn move_switches_to_faces_and_pushes_a_wall_end_out() {
        let mut doc = Document::from_model(sample_house());
        let (id, start) = first_wall(&doc);
        let ElementKind::Wall(w) = &doc.model().get(id).unwrap().kind else {
            panic!()
        };
        let (end, dir, length) = (w.end, w.direction(), w.length());
        let mut m = tool(&doc, Mode::Move, id);
        // the wall's end face is under the pointer, wherever it is
        let pick = move |_: Pos2| {
            Some(FacePick {
                id,
                face: FaceRef {
                    point: end.extend(1.5),
                    normal: dir.extend(0.0),
                    mesh_face: None,
                },
                tris: vec![],
            })
        };
        with_map(&[], |map| {
            let map = PaneMap {
                pick_face: Some(&pick),
                pick_line: None,
                ..*map
            };
            // F switches to faces, and back
            let f = Events {
                text: "f".into(),
                ..pointer(at(end.x, end.y))
            };
            m.step(&mut doc, &f, &map);
            assert_eq!(m.works_on, Target::Face);
            m.step(&mut doc, &f, &map);
            assert_eq!(m.works_on, Target::Object);
            m.step(&mut doc, &f, &map);
            m.step(&mut doc, &click(at(end.x, end.y)), &map);
            assert_eq!(m.stage(), Stage::Follow);
            let past = end + dir * 1.5;
            let out = m.step(&mut doc, &pointer(at(past.x, past.y)), &map);
            let Outcome::Running(text) = out else {
                panic!()
            };
            // the readout is in the project's units
            assert!(text.contains("1500"), "{text}");
            assert!(matches!(
                m.step(&mut doc, &click(at(past.x, past.y)), &map),
                Outcome::Done(_)
            ));
        });
        let length_now = |doc: &Document| match &doc.model().get(id).unwrap().kind {
            ElementKind::Wall(w) => w.length(),
            _ => 0.0,
        };
        // longer by the push, its start where it was
        assert!((length_now(&doc) - length - 1.5).abs() < 1e-6);
        assert_eq!(wall_start(&doc, id), start);
        doc.undo();
        assert!((length_now(&doc) - length).abs() < 1e-9);
    }

    #[test]
    fn a_snap_at_another_height_lifts_a_move_in_3d() {
        let mut doc = Document::from_model(sample_house());
        let (id, start) = first_wall(&doc);
        let mut m = Modal::start(
            &doc,
            Mode::Move,
            &[id],
            None,
            Space::Model,
            None,
            Pos2::ZERO,
            Pivot::Median,
            None,
            None,
        )
        .unwrap()
        .with_flow(Flow::Tool);
        // a corner 2.7 m up and 3 m along is under the pointer; then only
        // the grid, which is on the level
        let corner = start.extend(0.0) + DVec3::new(3.0, 0.0, 2.7);
        for (kind, dz) in [(SnapKind::Endpoint, 2.7), (SnapKind::Grid, 0.0)] {
            let snap = move |_: Pos2, from: Option<DVec3>| {
                from.map(|_| {
                    (
                        corner,
                        Snap {
                            p: corner,
                            kind,
                            host: None,
                        },
                    )
                })
            };
            with_map(&[], |map| {
                let map = PaneMap {
                    snap: &snap,
                    ..*map
                };
                if m.stage() == Stage::Base {
                    m.step(&mut doc, &click(at(start.x, start.y)), &map);
                }
                assert_eq!(m.stage(), Stage::Follow);
                m.step(&mut doc, &pointer(at(start.x + 3.0, start.y)), &map);
            });
            assert!((m.delta.z - dz).abs() < 1e-9, "{kind:?}: {}", m.delta.z);
            assert!((m.delta.x - 3.0).abs() < 1e-9);
        }
    }

    #[test]
    fn the_pointer_straight_up_from_the_base_point_is_the_vertical() {
        // a view from the side and above: X runs right, Y up and right a
        // little, Z straight up the screen
        let oblique = |p: DVec3| {
            Some(Pos2::new(
                (p.x * 40.0 + p.y * 20.0) as f32,
                (-p.z * 40.0 - p.y * 12.0) as f32,
            ))
        };
        let base = DVec3::new(1.0, 2.0, 0.0);
        let b = oblique(base).unwrap();
        with_map(&[], |map| {
            // a plan looks straight down: up is a point, never a direction
            assert!(!runs_vertical(map, base, at(1.0, 9.0)));
            let map = PaneMap {
                project: &oblique,
                ..*map
            };
            // straight up and straight down the screen, and a little off
            assert!(runs_vertical(&map, base, b + egui::vec2(0.0, -120.0)));
            assert!(runs_vertical(&map, base, b + egui::vec2(0.0, 90.0)));
            assert!(runs_vertical(&map, base, b + egui::vec2(12.0, -120.0)));
            // well off the vertical, or hardly moved: on the level
            assert!(!runs_vertical(&map, base, b + egui::vec2(60.0, -120.0)));
            assert!(!runs_vertical(&map, base, b + egui::vec2(120.0, 0.0)));
            assert!(!runs_vertical(&map, base, b + egui::vec2(0.0, -6.0)));
        });
    }

    #[test]
    fn pull_needs_no_selection_and_makes_a_flat_shape_solid() {
        use creabim_core::{EditMesh, FreeMesh};
        let mut flat = EditMesh::default();
        let p = [
            DVec3::ZERO,
            DVec3::new(4.0, 0.0, 0.0),
            DVec3::new(4.0, 3.0, 0.0),
            DVec3::new(0.0, 3.0, 0.0),
        ];
        for i in 0..4 {
            flat.add_line(p[i], p[(i + 1) % 4]);
        }
        flat.fill_loops(DVec3::Z);
        let shape = Element::new(
            "Outline",
            ElementKind::FreeMesh(FreeMesh::new(None, DVec3::new(20.0, 20.0, 0.0), flat)),
        );
        let id = shape.id;
        let mut doc = Document::from_model(sample_house());
        doc.commit(Transaction::new("Add").insert(shape)).unwrap();
        // nothing is selected: Pull starts all the same
        let mut m = Modal::start(
            &doc,
            Mode::Pull,
            &[],
            None,
            Space::Model,
            None,
            Pos2::ZERO,
            Pivot::Median,
            None,
            None,
        )
        .unwrap();
        assert_eq!(m.stage(), Stage::Base);
        let pick = move |_: Pos2| {
            Some(FacePick {
                id,
                face: FaceRef {
                    point: DVec3::new(22.0, 21.5, 0.0),
                    normal: DVec3::Z,
                    mesh_face: Some(0),
                },
                tris: vec![],
            })
        };
        with_map(&[], |map| {
            let map = PaneMap {
                pick_face: Some(&pick),
                pick_line: None,
                ..*map
            };
            m.step(&mut doc, &click(at(22.0, 21.5)), &map);
            assert_eq!(m.stage(), Stage::Follow);
            let typed = Events {
                text: "2500".into(),
                ..pointer(at(22.0, 21.5))
            };
            m.step(&mut doc, &typed, &map);
            let enter = Events {
                enter: true,
                ..pointer(at(22.0, 21.5))
            };
            assert!(matches!(m.step(&mut doc, &enter, &map), Outcome::Done(_)));
        });
        let ElementKind::FreeMesh(f) = &doc.model().get(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(f.mesh.kind(), creabim_core::shape::Kind::Solid);
        assert!(
            (f.mesh.volume() - 4.0 * 3.0 * 2.5).abs() < 1e-6,
            "{}",
            f.mesh.volume()
        );
        // one step back and it is the flat outline again
        doc.undo();
        let ElementKind::FreeMesh(f) = &doc.model().get(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(f.mesh.faces.len(), 1);
    }
}
