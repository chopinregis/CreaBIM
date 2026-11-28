//! Drag handles on a selected element, defined once in model space and
//! shown in every pane that can use them: the 3D view, plans and sections.
//! Each handle slides along a line or across a plane; the pane turns the
//! pointer into a ray (a perspective ray in 3D, a straight-on one in 2D) and
//! the handle's constraint picks the point on it.
//!
//! Walls stretch by their ends (joined walls follow), grow taller by their
//! top and thicker by their face. Slabs move corners and edges, and get
//! thicker by their underside (the top stays) or by their top (the
//! underside stays). Doors and windows widen from either jamb and
//! change height and sill. A selected face of a mass pushes and pulls along
//! its normal. Shafts move their corners, base and top.
//!
//! Every outline (a slab's, a roof's, a shaft's, a sketch's, a railing's
//! path) has the same grips, from [`crate::editmode::outline_handles`]:
//! its corners and edges when selected, and in Edit mode its holes and
//! add-corner grips too, with corners to pick and remove (see
//! [`GripSel`]).

use crate::modal::LiveEdit;
use crate::snaps::{Snap, SnapKind};
use creabim_core::{
    transform, walls, DetailKind, Document, Element, ElementId, ElementKind, Model, OpeningKind,
    ViewKind, WallTop, Xform,
};
use eframe::egui::{self, Color32, Painter, Pos2, Rect, Stroke, Vec2};
use glam::{DVec2, DVec3};

/// A pointer ray in model space.
#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub o: DVec3,
    pub d: DVec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Constraint {
    /// Anywhere on the plane through `point` with `normal`.
    Plane { point: DVec3, normal: DVec3 },
    /// Along the line through `point` in `dir` (a unit vector).
    Line { point: DVec3, dir: DVec3 },
}

impl Constraint {
    /// `p` brought onto the constraint: the nearest point on its line, or
    /// straight onto its plane.
    pub fn project(&self, p: DVec3) -> DVec3 {
        match *self {
            Constraint::Plane { point, normal } => p - normal * (p - point).dot(normal),
            Constraint::Line { point, dir } => point + dir * (p - point).dot(dir),
        }
    }

    /// The point on the constraint the ray points at.
    pub fn hit(&self, r: Ray) -> Option<DVec3> {
        match *self {
            Constraint::Plane { point, normal } => {
                let den = r.d.dot(normal);
                if den.abs() < 1e-9 {
                    return None;
                }
                let t = (point - r.o).dot(normal) / den;
                Some(r.o + r.d * t)
            }
            Constraint::Line { point, dir } => {
                // closest point on the line to the ray
                let w = point - r.o;
                let (a, b, c) = (dir.dot(dir), dir.dot(r.d), r.d.dot(r.d));
                let (d, e) = (dir.dot(w), r.d.dot(w));
                let den = a * c - b * b;
                if den.abs() < 1e-12 {
                    return None;
                }
                let s = (b * e - c * d) / den;
                Some(point + dir * s)
            }
        }
    }

    /// Can this be dragged in a pane looking along `look`? A line seen end
    /// on can't.
    pub fn usable(&self, look: DVec3) -> bool {
        match *self {
            Constraint::Line { dir, .. } => dir.dot(look).abs() < 0.97,
            Constraint::Plane { normal, .. } => normal.dot(look).abs() > 0.2,
        }
    }
}

/// Which outline of an element an Edit-mode grip works on: its own, or
/// hole `k` of a slab or a roof.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ring {
    Outline,
    Hole(usize),
    /// An open line of the element beside its outline: a car park's
    /// aisle drawn by hand.
    Line(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HandleKind {
    WallEnd {
        end: bool,
    },
    /// In Edit mode, a curtain wall's grid line where it stands (`at`,
    /// in micrometres along the wall for an upright, up it for a
    /// transom): slid along or up, set by hand (curtain grids, decision
    /// 2); dropped on its neighbour, it goes.
    CurtainLine {
        upright: bool,
        at: i64,
    },
    WallTop,
    WallFace,
    SlabUnderside,
    SlabTop,
    OpeningJamb {
        right: bool,
    },
    OpeningHead,
    OpeningSill,
    GridEnd {
        end: bool,
    },
    /// A grid line's bubble, dragged sideways onto an elbow (grid
    /// settings, decision 2).
    GridBubble {
        end: bool,
    },
    /// The middle of an arc grid line: dragged, how far it bows (MU66).
    GridRise,
    /// The setback line along edge `i` of a property: slid in or out, it
    /// gives that edge a setback of its own.
    Setback(usize),
    /// In Edit mode, the inner line of the strip along edge `i` of a car
    /// park's outline: slid in or out, it sets the strip's width.
    Strip(usize),
    /// In Edit mode, an end of a car park's row: the one down the `left`
    /// (or right) of stretch `seg` of aisle `aisle`, its start or its
    /// `end`, slid along the aisle to take bays off or put them back.
    RowEnd {
        aisle: usize,
        seg: usize,
        left: bool,
        end: bool,
    },
    /// Push or pull a face of a mass along its normal.
    Face(usize),
    /// Corner `i` of a mass, moved across the plan (A7-9).
    MeshVertex(usize),
    /// The edge between corners `a` and `b` of a mass, moved across the
    /// plan by its middle.
    MeshEdge(u32, u32),
    /// A whole mass, by its base point: across the plan.
    MeshMove,
    /// A whole mass, up or down.
    MeshLift,
    /// Grip `i` of a column, beam, roof, stair or railing; `free` ones
    /// move across the plan, others along a line.
    Member {
        i: usize,
        free: bool,
    },
    /// An end of a section or elevation line.
    ViewEnd {
        end: bool,
    },
    /// How far a section or elevation sees, from its line.
    ViewDepth,
    /// Face `i` of a crop box (see [`creabim_core::CropBox::face`]).
    CropFace(usize),
    /// A crop box as a whole, across the plan.
    CropMove,
    /// A reference as a whole, by its middle (Trace & Reference v2).
    RefMove,
    /// Corner `i` of a reference's outline: scales it about its middle.
    RefScale(usize),
    /// Corner `i` of an outline: a slab's, a roof's, a shaft's, a
    /// sketch's, a railing's path; in Edit mode a hole's too.
    Vertex {
        ring: Ring,
        i: usize,
    },
    /// Edge `i` of a closed outline, pushed out or pulled in square to
    /// itself.
    Edge {
        ring: Ring,
        i: usize,
    },
    /// Edge `i` of an outline taken by its body in Edit mode: it moves as
    /// it is, the edges beside it stretching (MU46). Not a grip that
    /// shows; a drag that starts on the edge.
    EdgeMove {
        ring: Ring,
        i: usize,
    },
    /// In Edit mode, the middle of edge `i`: dragging it pulls a new
    /// corner out of the edge (a click adds nothing, so a double-click
    /// can't add a corner and take it away again).
    AddVertex {
        ring: Ring,
        i: usize,
    },
    /// In Edit mode, a whole outline moved: a hole dragged from inside.
    Ring {
        ring: Ring,
    },
    /// A shaft's base and its top, up and down.
    ShaftBase,
    ShaftTop,
    /// Turn an object, a column or a stair about its own point.
    Rotate,
    /// Corner `i` of an object's outline (lo, then anticlockwise in its own
    /// frame): shown only under the pointer, as a bracket; dragged, it
    /// scales the object from the opposite corner; clicked, its palette
    /// (MU142, Grip Palette REV01).
    ScaleCorner(u8),
    /// A rectangular column's width (along its own x) or depth; a round
    /// one's diameter.
    ColumnSize {
        depth: bool,
    },
    /// Slide a whole wall square to itself; the walls joined to it stretch.
    WallSlide,
    /// The middle of a curved wall: drag it to bend the wall more or less,
    /// its ends staying put (it passes through the point dragged to).
    WallBend,
    /// Slide a door or window along its wall at the same width.
    OpeningSlide,
    /// A beam's width, both sides at once.
    BeamWidth,
    /// A detail component's width across its path, both sides at once.
    DetailWidth,
    /// A repeating detail's unit length, at the end of its first unit.
    DetailSpacing,
    /// A plant's canopy, at the edge of its crown.
    PlantCanopy,
    /// A plant's height, at its top (in 3D and sections).
    PlantHeight,
    /// A grown tree's clear trunk, at the crown's underside.
    PlantCrownBase,
    /// How far a grown tree's crown reaches on side `i` (east, then
    /// counter-clockwise in its own frame), at its widest: pulled in off
    /// a wall, its trunk stays.
    PlantReach(u8),
    /// Corner `i` of a callout's box on its parent view (lo, then
    /// counter-clockwise); the opposite corner stays.
    CalloutCorner(usize),
    /// Edge `i` of a callout's box (bottom, right, top, left), pushed out
    /// or pulled in.
    CalloutEdge(usize),
    /// In Edit mode's Layers (Tab), the start or end of layer `layer` of
    /// a layered wall, run on past its join or held back.
    LayerEnd {
        layer: usize,
        end: bool,
    },
    /// In Layers, the bottom (or `top`) of layer `layer` of a layered
    /// wall, run down past the wall's base or up past its top (A7-10).
    LayerHeight {
        layer: usize,
        top: bool,
    },
    /// In Layers, edge `edge` of layer `layer` of a layered slab or roof,
    /// pushed out past the element's edge or pulled in.
    LayerEdge {
        layer: usize,
        edge: usize,
    },
    /// In a line stair's pieces (Shift+Tab in Edit mode), the top riser
    /// of the flight along straight `straight`: more or fewer treads.
    StairFlight {
        straight: usize,
    },
    /// In a line stair's pieces, the landing at turn `turn`: pushed back
    /// to make it deeper.
    StairLanding {
        turn: usize,
    },
    /// A stair's UP label, moved off what it covers; its arrow stays
    /// (MU144).
    StairUp,
}

impl HandleKind {
    /// Round handles slide along a line; square ones move freely.
    pub fn round(self) -> bool {
        !matches!(
            self,
            HandleKind::WallEnd { .. }
                | HandleKind::GridEnd { .. }
                | HandleKind::GridBubble { .. }
                | HandleKind::GridRise
                | HandleKind::Member { free: true, .. }
                | HandleKind::ViewEnd { .. }
                | HandleKind::CropMove
                | HandleKind::RefMove
                | HandleKind::RefScale(_)
                | HandleKind::Vertex { .. }
                | HandleKind::AddVertex { .. }
                | HandleKind::EdgeMove { .. }
                | HandleKind::CalloutCorner(_)
                | HandleKind::MeshVertex(_)
                | HandleKind::MeshEdge(..)
                | HandleKind::MeshMove
                | HandleKind::StairUp
        )
    }

    pub fn verb(self) -> &'static str {
        match self {
            HandleKind::WallTop
            | HandleKind::OpeningHead
            | HandleKind::ShaftBase
            | HandleKind::ShaftTop => "Change height of",
            HandleKind::Vertex { .. }
            | HandleKind::Edge { .. }
            | HandleKind::EdgeMove { .. }
            | HandleKind::AddVertex { .. }
            | HandleKind::Ring { .. } => "Reshape",
            HandleKind::Rotate => "Rotate",
            HandleKind::ScaleCorner(_) => "Scale",
            HandleKind::ColumnSize { .. } => "Resize",
            HandleKind::WallSlide | HandleKind::OpeningSlide => "Slide",
            HandleKind::WallBend => "Bend",
            HandleKind::BeamWidth | HandleKind::DetailWidth => "Change width of",
            HandleKind::DetailSpacing => "Change the unit of",
            HandleKind::PlantCanopy => "Change the canopy of",
            HandleKind::PlantHeight => "Change height of",
            HandleKind::PlantCrownBase => "Change the clear trunk of",
            HandleKind::PlantReach(_) => "Shape the crown of",
            HandleKind::CalloutCorner(_) | HandleKind::CalloutEdge(_) => "Resize the callout of",
            HandleKind::LayerEnd { .. } => "Move a layer end of",
            HandleKind::LayerEdge { .. } => "Move a layer edge of",
            HandleKind::LayerHeight { .. } => "Move a layer's base or top of",
            HandleKind::StairFlight { .. } => "Change a flight of",
            HandleKind::StairLanding { .. } => "Change a landing of",
            HandleKind::StairUp => "Move the UP label of",
            HandleKind::WallFace | HandleKind::SlabUnderside | HandleKind::SlabTop => {
                "Change thickness of"
            }
            HandleKind::OpeningJamb { .. } => "Change width of",
            HandleKind::OpeningSill => "Change sill of",
            HandleKind::Face(_) => "Push/pull",
            HandleKind::ViewDepth => "Change depth of",
            HandleKind::CropFace(_) => "Resize",
            HandleKind::CropMove
            | HandleKind::RefMove
            | HandleKind::MeshMove
            | HandleKind::MeshLift => "Move",
            HandleKind::RefScale(_) => "Scale",
            HandleKind::MeshVertex(_) | HandleKind::MeshEdge(..) => "Reshape",
            _ => "Stretch",
        }
    }

    /// The undo label of a drag of this grip on `name`.
    pub fn label(self, name: &str) -> String {
        match self {
            HandleKind::Vertex { .. } => format!("Move a corner of {name}"),
            HandleKind::AddVertex { .. } => format!("Add a corner to {name}"),
            HandleKind::Edge { .. } => format!("Offset an edge of {name}"),
            HandleKind::EdgeMove { .. } => format!("Move an edge of {name}"),
            HandleKind::Ring {
                ring: Ring::Hole(k),
            } => format!("Move hole {} of {name}", k + 1),
            HandleKind::Ring { .. } => format!("Move the outline of {name}"),
            k => format!("{} {name}", k.verb()),
        }
    }

    /// The grip's short name, shown by it under the pointer.
    pub fn name(self) -> &'static str {
        match self {
            HandleKind::WallEnd { .. } => "Wall end",
            HandleKind::WallTop => "Top: raise",
            HandleKind::WallFace => "Face: thicken",
            HandleKind::SlabTop => "Top: raise",
            HandleKind::SlabUnderside => "Underside: thicken",
            HandleKind::OpeningJamb { .. } => "Jamb: widen",
            HandleKind::OpeningHead => "Head: raise",
            HandleKind::OpeningSill => "Sill: raise",
            HandleKind::GridEnd { .. } => "Grid end",
            HandleKind::CurtainLine { upright: true, .. } => "Mullion: slide",
            HandleKind::CurtainLine { upright: false, .. } => "Transom: slide",
            HandleKind::GridBubble { .. } => "Bubble: move aside",
            HandleKind::GridRise => "Rise: bow it",
            HandleKind::Setback(_) => "Setback: slide",
            HandleKind::Strip(_) => {
                "Edge strip: slide to set its width · Right-click the edge: more"
            }
            HandleKind::RowEnd { .. } => {
                "Row end: slide along the aisle to add or drop bays · Right-click a bay: more"
            }
            HandleKind::Face(_) => "Face: push",
            HandleKind::Member { .. } => "Point",
            HandleKind::ViewEnd { .. } => "Line end",
            HandleKind::ViewDepth => "Depth",
            HandleKind::CropFace(_) => "Crop face",
            HandleKind::CropMove => "Crop box: move",
            HandleKind::RefMove => "Reference: move",
            HandleKind::RefScale(_) => "Corner: scale",
            HandleKind::Vertex { .. } => "Corner",
            HandleKind::Edge { .. } => "Edge: push",
            HandleKind::EdgeMove { .. } => "Edge",
            HandleKind::AddVertex { .. } => "Add a corner",
            HandleKind::Ring { .. } => "Outline: move",
            HandleKind::ShaftBase => "Base",
            HandleKind::ShaftTop => "Top",
            HandleKind::Rotate => "Turn",
            HandleKind::ScaleCorner(_) => "Corner",
            HandleKind::ColumnSize { depth: false } => "Width",
            HandleKind::ColumnSize { depth: true } => "Depth",
            HandleKind::WallSlide => "Wall: slide",
            HandleKind::WallBend => "Wall: bend",
            HandleKind::OpeningSlide => "Slide along the wall",
            HandleKind::BeamWidth | HandleKind::DetailWidth => "Width",
            HandleKind::DetailSpacing => "Unit",
            HandleKind::PlantCanopy => "Canopy",
            HandleKind::PlantHeight => "Height",
            HandleKind::PlantCrownBase => "Clear trunk",
            HandleKind::PlantReach(i) => REACH_NAMES[i as usize % 8],
            HandleKind::CalloutCorner(_) => "Callout corner",
            HandleKind::CalloutEdge(_) => "Callout edge: push",
            HandleKind::LayerEnd { .. } => "Layer end",
            HandleKind::LayerEdge { .. } => "Layer edge",
            HandleKind::LayerHeight { top: false, .. } => "Layer bottom",
            HandleKind::LayerHeight { top: true, .. } => "Layer top",
            HandleKind::StairFlight { .. } => "Flight",
            HandleKind::StairLanding { .. } => "Landing",
            HandleKind::StairUp => "UP label",
            HandleKind::MeshVertex(_) => "Corner",
            HandleKind::MeshEdge(..) => "Edge",
            HandleKind::MeshMove => "Base point: move",
            HandleKind::MeshLift => "Up or down",
        }
    }

    /// What the grip does, for the tooltip by the pointer.
    pub fn tip(self, edit: bool) -> &'static str {
        match self {
            HandleKind::Vertex { .. } if edit => {
                "Drag: move corner · Click: select (Shift adds) · Del: remove · Right-click: more"
            }
            HandleKind::Vertex { .. } => {
                "Drag: move corner · Double-click: edit the outline (add and remove corners)"
            }
            HandleKind::AddVertex { .. } => "Drag: pull out a new corner · Right-click: more",
            HandleKind::Edge { .. } if edit => "Drag: offset the edge · Right-click: more",
            HandleKind::Edge { .. } => "Drag: offset the edge",
            HandleKind::WallEnd { .. } => "Drag: move the end (Ctrl: free of its joins)",
            HandleKind::WallTop | HandleKind::ShaftTop => "Drag: change the top",
            HandleKind::ShaftBase => "Drag: change the base",
            HandleKind::WallFace => "Drag: change the thickness",
            HandleKind::SlabTop => "Drag: move the top (the underside stays)",
            HandleKind::SlabUnderside => "Drag: change the thickness (the top stays)",
            HandleKind::OpeningJamb { .. } => "Drag: change the width",
            HandleKind::OpeningHead => "Drag: change the height",
            HandleKind::OpeningSill => "Drag: change the sill",
            HandleKind::Face(_) => "Drag: push or pull the face",
            HandleKind::MeshVertex(_) => "Drag: move the corner across the plan",
            HandleKind::MeshEdge(..) => "Drag: move the edge across the plan",
            HandleKind::MeshMove => "Drag: move the mass across the plan",
            HandleKind::MeshLift => "Drag: move the mass up or down",
            HandleKind::ViewDepth => "Drag: change how deep it sees",
            HandleKind::CropFace(_) => "Drag: move the crop face",
            HandleKind::RefMove => "Drag: move the reference",
            HandleKind::RefScale(_) => "Drag: scale it about its middle (Calibrate in Properties sets an exact size)",
            HandleKind::Rotate => "Drag: turn it about its middle (steps of 5°; Alt for any angle)",
            HandleKind::ScaleCorner(_) => "Drag: scale from the opposite corner · Click: more",
            HandleKind::ColumnSize { depth: false } => "Drag: change the width",
            HandleKind::ColumnSize { depth: true } => "Drag: change the depth",
            HandleKind::WallSlide => "Drag: slide the wall square to itself (joined walls follow)",
            HandleKind::WallBend => "Drag: bend the wall through this point; its ends stay (its radius is in Properties)",
            HandleKind::OpeningSlide => "Drag: slide it along the wall · Tab: type how far",
            HandleKind::BeamWidth | HandleKind::DetailWidth => "Drag: change the width",
            HandleKind::DetailSpacing => "Drag: change the unit's length (the joint stays)",
            HandleKind::PlantCanopy => "Drag: change how wide the crown is",
            HandleKind::PlantHeight => "Drag: change how tall it is",
            HandleKind::PlantCrownBase => "Drag: raise or lower where the crown starts (the clear trunk)",
            HandleKind::PlantReach(_) => "Drag: pull this side of the crown in, or push it out; the trunk stays",
            HandleKind::CalloutCorner(_) => {
                "Drag: resize the callout's box (its detail view follows)"
            }
            HandleKind::CalloutEdge(_) => "Drag: push the box's edge (its detail view follows)",
            HandleKind::LayerEnd { .. } => {
                "Drag: run the layer on or hold it back · Tab: type how far · Right-click: reset"
            }
            HandleKind::LayerEdge { .. } => {
                "Drag: push the layer's edge out or pull it in · Tab: type how far · Right-click: reset"
            }
            HandleKind::LayerHeight { .. } => {
                "Drag: run the layer down or up past the wall, or stop it short · Tab: type how far · Right-click: reset"
            }
            HandleKind::StairFlight { .. } => {
                "Drag: more or fewer treads on this flight; the others take the rest · Right-click: reset"
            }
            HandleKind::StairLanding { .. } => {
                "Drag back: a deeper landing; the flight before it stops sooner · Right-click: reset"
            }
            HandleKind::StairUp => {
                "Drag: the UP label, off what it covers; its arrow stays · Back by its place, it goes home"
            }
            _ => "Drag to change it",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Handle {
    pub kind: HandleKind,
    pub at: DVec3,
    pub constraint: Constraint,
    /// Screen pixels the grip is drawn out from `at` along its line, to
    /// keep it clear of another grip at the same point.
    pub nudge: f32,
}

/// The handles of element `id`, with `edit` those Edit mode adds (see
/// [`crate::editmode`]).
pub fn handles_in(model: &Model, id: ElementId, face: Option<usize>, edit: bool) -> Vec<Handle> {
    if edit && layers() {
        let ends = layer_handles(model, id);
        if !ends.is_empty() {
            return ends;
        }
    }
    let mut out = handles(model, id, face);
    out.extend(crate::editmode::outline_handles(model, id, edit));
    if edit {
        out.extend(mass_edit_handles(model, id, face));
        out.extend(curtain_line_handles(model, id));
        out.extend(row_end_handles(model, id));
        // vertex, edge or face picking (MU27)
        out.retain(|h| pick_allows(&h.kind));
    }
    out
}

/// In Edit mode, a grip at each end of every row of a car park whose
/// aisles are drawn by hand, sliding along its aisle.
fn row_end_handles(model: &Model, id: ElementId) -> Vec<Handle> {
    let Some(ElementKind::Site(s)) = model.get(id).map(|e| &e.kind) else {
        return vec![];
    };
    let Some(lay) = s.car_park_layout(model) else {
        return vec![];
    };
    let top = match &s.form {
        creabim_core::SiteForm::Area { top, .. } => *top,
        _ => 0.0,
    };
    let z = elevation(model, Some(s.level)) + top;
    let mut out = vec![];
    for r in &lay.rows_at {
        if !lay.by_hand {
            break;
        }
        for (end, at) in [(false, r.from), (true, r.to)] {
            let p = (r.o + r.d * at + r.back).extend(z);
            out.push(Handle {
                kind: HandleKind::RowEnd {
                    aisle: r.aisle,
                    seg: r.seg,
                    left: r.left,
                    end,
                },
                at: p,
                constraint: Constraint::Line {
                    point: p,
                    dir: r.d.extend(0.0),
                },
                nudge: 0.0,
            });
        }
    }
    // a grip in the middle of each edge strip's inner line
    for (i, ring, _) in &lay.strips {
        let inward = (ring[1] - ring[0]).normalize_or_zero().perp();
        let p = ((ring[2] + ring[3]) / 2.0).extend(z);
        out.push(Handle {
            kind: HandleKind::Strip(*i),
            at: p,
            constraint: Constraint::Line {
                point: p,
                dir: inward.extend(0.0),
            },
            nudge: 0.0,
        });
    }
    out
}

/// The bay or island of a car park under `p`: its middle.
fn bay_at(model: &Model, e: &Element, input: &PaneInput, p: Pos2) -> Option<DVec2> {
    let ElementKind::Site(s) = &e.kind else {
        return None;
    };
    let lay = s.car_park_layout(model)?;
    let top = match &s.form {
        creabim_core::SiteForm::Area { top, .. } => *top,
        _ => 0.0,
    };
    let z = elevation(model, Some(s.level)) + top;
    let under = |c: &[DVec2; 4]| {
        let pts: Option<Vec<Pos2>> = c.iter().map(|q| (input.project)(q.extend(z))).collect();
        pts.is_some_and(|pts| in_screen_polygon(&pts, p))
    };
    lay.bays
        .iter()
        .map(|b| b.corners)
        .chain(lay.islands.iter().copied())
        .find(|c| under(c))
        .map(|c| c.iter().copied().sum::<DVec2>() / 4.0)
}

static LAYERS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What Edit mode picks (MU27, as Blender): 0 everything, 1 points, 2
/// edges, 3 faces.
static PICK: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Edit mode's selection kind: vertices, edges or faces (0 any).
pub fn set_pick(k: u8) {
    PICK.store(k.min(3), std::sync::atomic::Ordering::Relaxed);
}

pub fn pick() -> u8 {
    PICK.load(std::sync::atomic::Ordering::Relaxed)
}

/// Does grip `k` belong to Edit mode's selection kind?
/// A curtain wall's grid lines as they are now, each way: `(uprights,
/// transoms)` inside its edges, in its own coordinates, and its height
/// (to the roof's highest point when attached).
pub fn curtain_lines(model: &Model, w: &creabim_core::Wall) -> Option<(Vec<f64>, Vec<f64>, f64)> {
    let g = w.curtain.as_ref()?;
    let at = creabim_geom::curtain::setting(model, w);
    let height = at.top.as_ref().map_or(w.height, |t| t.highest());
    let inner = |mut v: Vec<f64>| {
        v.pop();
        v.remove(0);
        v
    };
    Some((
        inner(g.grid_lines(true, w.length(), &at.forced.us)),
        inner(g.grid_lines(false, height, &at.forced.vs)),
        height,
    ))
}

/// In Edit mode, a grip on each of a curtain wall's grid lines: an
/// upright's at its middle height, sliding along the wall; a transom's
/// at the wall's middle, sliding up.
fn curtain_line_handles(model: &Model, id: ElementId) -> Vec<Handle> {
    let Some(ElementKind::Wall(w)) = model.get(id).map(|e| &e.kind) else {
        return vec![];
    };
    let Some((us, vs, height)) = curtain_lines(model, w) else {
        return vec![];
    };
    let z0 = elevation(model, Some(w.level)) + w.base_offset;
    let line = |kind, at: DVec3, dir: DVec3| Handle {
        kind,
        at,
        constraint: Constraint::Line {
            point: at,
            dir: dir.normalize(),
        },
        nudge: 0.0,
    };
    let mut out = vec![];
    for u in us {
        out.push(line(
            HandleKind::CurtainLine {
                upright: true,
                at: (u * 1e6).round() as i64,
            },
            w.place(u, 0.0).extend(z0 + height / 2.0),
            w.tangent_at(u).extend(0.0),
        ));
    }
    let mid = w.length() / 2.0;
    for v in vs {
        out.push(line(
            HandleKind::CurtainLine {
                upright: false,
                at: (v * 1e6).round() as i64,
            },
            w.place(mid, 0.0).extend(z0 + v),
            DVec3::Z,
        ));
    }
    out
}

pub fn pick_allows(k: &HandleKind) -> bool {
    use HandleKind as K;
    if matches!(k, K::CurtainLine { .. }) {
        return true;
    }
    match pick() {
        1 => matches!(
            k,
            K::Vertex { .. }
                | K::MeshVertex(_)
                | K::WallEnd { .. }
                | K::GridEnd { .. }
                | K::Member { .. }
                | K::MeshMove
                | K::Rotate
        ),
        2 => matches!(
            k,
            K::Edge { .. }
                | K::AddVertex { .. }
                | K::WallBend
                | K::MeshEdge(..)
                | K::OpeningJamb { .. }
                | K::OpeningHead
                | K::OpeningSill
                | K::MeshMove
                | K::Rotate
        ),
        3 => matches!(
            k,
            K::Face(_)
                | K::WallFace
                | K::WallTop
                | K::SlabTop
                | K::SlabUnderside
                | K::MeshLift
                | K::ShaftBase
                | K::ShaftTop
                | K::MeshMove
                | K::Rotate
        ),
        _ => true,
    }
}

/// Edit mode's Layers (3.7): Tab in Edit mode on a layered element shows
/// a grip at each of its layers' ends instead of its own grips.
pub fn set_layers(on: bool) {
    LAYERS.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn layers() -> bool {
    LAYERS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Does `id` have layers to edit one by one?
pub fn has_layers(model: &Model, id: ElementId) -> bool {
    !layer_handles(model, id).is_empty()
}

/// A grip at each end of each layer of wall `id`, where the joins and any
/// edit put it, sliding along the wall. Empty unless it is layered.
pub fn layer_handles(model: &Model, id: ElementId) -> Vec<Handle> {
    let Some(e) = model.get(id) else {
        return vec![];
    };
    if let ElementKind::Stair(s) = &e.kind {
        return stair_piece_handles(model, s);
    }
    let ElementKind::Wall(w) = &e.kind else {
        return edge_handles(model, e);
    };
    let Some((_, a)) = creabim_core::assembly::assembly_of(model, e) else {
        return vec![];
    };
    let bands = creabim_core::assembly::wall_bands(a, w);
    if bands.len() < 2 {
        return vec![];
    }
    let ends = creabim_core::wall_joins(model)
        .remove(&id)
        .unwrap_or_default();
    let z0 = elevation(model, Some(w.level)) + w.base_offset;
    let (d, n) = (w.direction(), w.direction().perp());
    let last = (bands.len() - 1) as f64;
    let mut out = vec![];
    for (k, b) in bands.iter().enumerate() {
        let s = (b.s0 + b.s1) / 2.0;
        // stepped up the end face, so each reads on its own in 3D
        let z = z0 + w.height * (0.3 + 0.4 * k as f64 / last);
        for end in [false, true] {
            let u = ends.place_layer(w, if end { w.length() } else { 0.0 }, s, b.core, b.ext);
            let at = (w.start + d * u + n * s).extend(z);
            out.push(Handle {
                kind: HandleKind::LayerEnd {
                    layer: b.index,
                    end,
                },
                at,
                constraint: Constraint::Line {
                    point: at,
                    dir: d.extend(0.0),
                },
                nudge: 0.0,
            });
        }
        // its bottom and top, up and down (A7-10), in the middle of the
        // wall's length
        let mid = w.start + d * (w.length() / 2.0) + n * s;
        let [down, up] = b.vext;
        for (top, z) in [(false, z0 - down), (true, z0 + w.height + up)] {
            let at = mid.extend(z);
            out.push(Handle {
                kind: HandleKind::LayerHeight {
                    layer: b.index,
                    top,
                },
                at,
                constraint: Constraint::Line {
                    point: at,
                    dir: DVec3::Z,
                },
                nudge: 0.0,
            });
        }
    }
    out
}

/// A grip on each flight's top riser and in each landing of line stair
/// `s`: its pieces, changed one at a time. Empty for a stair from before
/// lines, or one of a single flight.
fn stair_piece_handles(model: &Model, s: &creabim_core::Stair) -> Vec<Handle> {
    if s.line.len() < 2 {
        return vec![];
    }
    let rise = s.rise(model);
    let base = s.base(model);
    let lay = s.layout(rise);
    if lay.flights.len() < 2 {
        return vec![];
    }
    let mut out = vec![];
    let last = lay.flights.len() - 1;
    for (i, f) in lay.flights.iter().enumerate() {
        // the last flight takes what is left: it has no grip
        if i == last || f.risers < 2 {
            continue;
        }
        let at = f
            .end(s.tread)
            .extend(base + f.z0 + f.risers as f64 * lay.riser);
        out.push(Handle {
            kind: HandleKind::StairFlight {
                straight: f.straight,
            },
            at,
            constraint: Constraint::Line {
                point: at,
                dir: f.end_dir(s.tread).extend(0.0),
            },
            nudge: 0.0,
        });
    }
    for ((outline, z), &turn) in lay.landings.iter().zip(&lay.turns) {
        // pushed back along the flight that climbs to it
        let Some(before) = lay.flights.iter().rev().find(|f| f.straight <= turn) else {
            continue;
        };
        let mid = outline.iter().copied().sum::<DVec2>() / outline.len().max(1) as f64;
        let at = mid.extend(base + z);
        out.push(Handle {
            kind: HandleKind::StairLanding { turn },
            at,
            constraint: Constraint::Line {
                point: at,
                dir: before.end_dir(s.tread).extend(0.0),
            },
            nudge: 0.0,
        });
    }
    out
}

/// What grip `kind` of a line stair's pieces says by it: the flight's
/// treads or how much deeper the landing is.
fn stair_piece_name(model: &Model, id: ElementId, kind: HandleKind) -> Option<String> {
    let ElementKind::Stair(s) = &model.get(id)?.kind else {
        return None;
    };
    let lay = s.layout(s.rise(model));
    match kind {
        HandleKind::StairFlight { straight } => {
            let i = lay.flights.iter().position(|f| f.straight == straight)?;
            let t = lay.flights[i].risers.saturating_sub(1);
            let own = if s.pieces.treads(straight).is_some() {
                ", kept"
            } else {
                ""
            };
            Some(format!("Flight {}: {t} treads{own}", i + 1))
        }
        HandleKind::StairLanding { turn } => {
            let i = lay.turns.iter().position(|t| *t == turn)?;
            let by = s.pieces.deeper(turn);
            Some(if by > 0.0 {
                format!("Landing {}: {:+.0} mm deeper", i + 1, by * 1000.0)
            } else {
                format!("Landing {}", i + 1)
            })
        }
        _ => None,
    }
}

/// How far the layer end or edge of grip `kind` was moved by hand on
/// `e`; `None` when `kind` is no such grip of it. A stair's flight gives
/// its kept treads (0: none kept), a landing how much deeper it is.
fn layer_offset(e: &ElementKind, kind: HandleKind) -> Option<f64> {
    match (e, kind) {
        (ElementKind::Stair(s), HandleKind::StairFlight { straight }) => {
            Some(s.pieces.treads(straight).map_or(0.0, |t| t as f64))
        }
        (ElementKind::Stair(s), HandleKind::StairLanding { turn }) => Some(s.pieces.deeper(turn)),
        (ElementKind::Wall(w), HandleKind::LayerEnd { layer, end }) => {
            Some(w.layer_ext(layer)[usize::from(end)])
        }
        (ElementKind::Wall(w), HandleKind::LayerHeight { layer, top }) => {
            Some(w.layer_vext(layer)[usize::from(top)])
        }
        (ElementKind::Slab(s), HandleKind::LayerEdge { layer, edge }) => {
            Some(creabim_core::layer_edge_offsets(&s.layer_edges, layer, s.boundary.len())[edge])
        }
        (ElementKind::Roof(r), HandleKind::LayerEdge { layer, edge }) => {
            Some(creabim_core::layer_edge_offsets(&r.layer_edges, layer, r.boundary.len())[edge])
        }
        _ => None,
    }
}

/// Set how far the layer end or edge of grip `kind` is moved (a stair's
/// flight: the treads it keeps, 0 none).
fn set_layer_offset(e: &mut ElementKind, kind: HandleKind, by: f64) {
    match (e, kind) {
        (ElementKind::Stair(s), HandleKind::StairFlight { straight }) => s
            .pieces
            .set_treads(straight, (by >= 1.0).then_some(by.round() as u32)),
        (ElementKind::Stair(s), HandleKind::StairLanding { turn }) => {
            s.pieces.set_deeper(turn, by.max(0.0))
        }
        (ElementKind::Wall(w), HandleKind::LayerEnd { layer, end }) => {
            w.set_layer_ext(layer, end, by)
        }
        (ElementKind::Wall(w), HandleKind::LayerHeight { layer, top }) => {
            w.set_layer_vext(layer, top, by)
        }
        (ElementKind::Slab(s), HandleKind::LayerEdge { layer, edge }) => {
            creabim_core::set_layer_edge(&mut s.layer_edges, layer, edge, by)
        }
        (ElementKind::Roof(r), HandleKind::LayerEdge { layer, edge }) => {
            creabim_core::set_layer_edge(&mut r.layer_edges, layer, edge, by)
        }
        _ => {}
    }
}

/// The outward normal of edge `i` of outline `poly`, in plan.
fn edge_out(poly: &[DVec2], i: usize) -> DVec2 {
    let n = poly.len();
    let d = (poly[(i + 1) % n] - poly[i]).normalize_or_zero();
    if creabim_core::polygon::signed_area2(poly) > 0.0 {
        -d.perp()
    } else {
        d.perp()
    }
}

/// A layer's index, its outline, the element's own outline (whose edge
/// normals the grips slide along), and how high its middle is over a
/// point of the plan.
type LayerRow = (usize, Vec<DVec2>, Vec<DVec2>, Box<dyn Fn(DVec2) -> f64>);

/// A grip on each edge of each layer of a layered slab or roof, at the
/// middle of the edge and of the layer's depth, sliding square to the
/// edge. In plan they would sit on one another, so each is drawn a step
/// further out.
fn edge_handles(model: &Model, e: &Element) -> Vec<Handle> {
    let Some((_, a)) = creabim_core::assembly::assembly_of(model, e) else {
        return vec![];
    };
    // each layer's outline, the normal of its edges, and where its
    // middle is at a point of the plan
    let rows: Vec<LayerRow> = match &e.kind {
        ElementKind::Slab(s) => {
            let top = s.top(elevation(model, Some(s.level)));
            creabim_core::assembly::depth_bands(a, s.thickness)
                .into_iter()
                .map(|b| {
                    let mid = top - (b.s0 + b.s1) / 2.0;
                    (
                        b.index,
                        s.layer_boundary(b.index),
                        s.boundary.clone(),
                        Box::new(move |_| mid) as Box<dyn Fn(DVec2) -> f64>,
                    )
                })
                .collect()
        }
        ElementKind::Roof(r) => {
            let base = r.base(model);
            let k = r.vertical_thickness() / r.thickness.max(1e-9);
            let tv = r.vertical_thickness();
            creabim_core::assembly::depth_bands(a, r.thickness)
                .into_iter()
                .map(|b| {
                    let faces = r.layer_faces(b.index);
                    let up = tv - (b.s0 + b.s1) / 2.0 * k;
                    let z = move |p: DVec2| {
                        // the underside under p: the lowest face over it
                        let under = faces.iter().map(|f| f.z(p)).fold(f64::INFINITY, f64::min);
                        base + if under.is_finite() { under } else { 0.0 } + up
                    };
                    (
                        b.index,
                        r.layer_eaves(b.index),
                        r.boundary.clone(),
                        Box::new(z) as Box<dyn Fn(DVec2) -> f64>,
                    )
                })
                .collect()
        }
        _ => return vec![],
    };
    if rows.len() < 2 {
        return vec![];
    }
    let mut out = vec![];
    for (k, (layer, poly, own, z)) in rows.iter().enumerate() {
        let n = poly.len();
        for edge in 0..n {
            let mid = (poly[edge] + poly[(edge + 1) % n]) / 2.0;
            let at = mid.extend(z(mid));
            out.push(Handle {
                kind: HandleKind::LayerEdge {
                    layer: *layer,
                    edge,
                },
                at,
                constraint: Constraint::Line {
                    point: at,
                    dir: edge_out(own, edge).extend(0.0),
                },
                nudge: 14.0 * k as f32,
            });
        }
    }
    out
}

/// Put the layer end or edge of grip `which` of `id` back where its join
/// or the element's own edge puts it, or with `None` every layer's, as
/// one undo step.
/// Draw wall `id` by the line between its layers `k - 1` and `k`, from
/// the exterior: only its location line changes, the wall stays put.
pub fn draw_by_boundary(doc: &mut Document, id: ElementId, k: u8) -> Result<String, String> {
    let e = doc.model().get(id).ok_or("it is gone")?.clone();
    let mut after = e.clone();
    let ElementKind::Wall(w) = &mut after.kind else {
        return Err("only walls have a location line".into());
    };
    let line = creabim_core::LocationLine::Boundary(k);
    if w.location == line {
        return Err("The wall is drawn by that line already.".into());
    }
    w.location = line;
    let text = creabim_core::walls::location_label(doc.model(), w);
    doc.commit(creabim_core::Transaction::new("Wall location line").replace(e, after))
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "The wall is drawn by {text} now. It stays where it is; new widths and layers keep that line put."
    ))
}

pub fn reset_layer_ends(
    doc: &mut Document,
    id: ElementId,
    which: Option<HandleKind>,
) -> Result<String, String> {
    let e = doc.model().get(id).ok_or("it is gone")?.clone();
    let mut after = e.clone();
    let done = match (which, &mut after.kind) {
        (Some(k @ HandleKind::StairFlight { .. }), kind) => {
            set_layer_offset(kind, k, 0.0);
            (
                "Reset flight",
                "The flight takes the treads that fit again.",
            )
        }
        (Some(k @ HandleKind::StairLanding { .. }), kind) => {
            set_layer_offset(kind, k, 0.0);
            (
                "Reset landing",
                "The landing is as deep as its line leaves it.",
            )
        }
        (None, ElementKind::Stair(s)) => {
            s.pieces = Default::default();
            (
                "Reset all pieces",
                "Every flight and landing fills the line again.",
            )
        }
        (Some(k), kind) => {
            set_layer_offset(kind, k, 0.0);
            (
                "Reset layer end",
                "The layer is back where its element puts it.",
            )
        }
        (None, ElementKind::Wall(w)) => {
            w.layer_ends.clear();
            ("Reset all layers", "Every layer's end is back on its join.")
        }
        (None, ElementKind::Slab(s)) => {
            s.layer_edges.clear();
            (
                "Reset all layers",
                "Every layer's edge is back on the slab's.",
            )
        }
        (None, ElementKind::Roof(r)) => {
            r.layer_edges.clear();
            (
                "Reset all layers",
                "Every layer's edge is back on the roof's.",
            )
        }
        _ => return Err("it has no layers to reset".into()),
    };
    if after == e {
        return Err("Nothing to reset: no layer was moved.".into());
    }
    doc.commit(creabim_core::Transaction::new(done.0).replace(e, after))
        .map_err(|e| e.to_string())?;
    Ok(done.1.into())
}

/// What a layer grip says by it: its layer, which end or edge, and how
/// far it was moved.
fn layer_name(model: &Model, id: ElementId, kind: HandleKind) -> Option<String> {
    let (layer, side) = match kind {
        HandleKind::LayerEnd { layer, end } => {
            (layer, if end { "end" } else { "start" }.to_string())
        }
        HandleKind::LayerEdge { layer, edge } => (layer, format!("edge {}", edge + 1)),
        HandleKind::LayerHeight { layer, top } => {
            (layer, if top { "top" } else { "bottom" }.to_string())
        }
        _ => return None,
    };
    let e = model.get(id)?;
    let (_, a) = creabim_core::assembly::assembly_of(model, e)?;
    let l = a.layers.get(layer)?;
    let what = l
        .material
        .and_then(|m| model.get(m))
        .map_or_else(|| format!("Layer {}", layer + 1), |m| m.name.clone());
    let by = layer_offset(&e.kind, kind).unwrap_or(0.0);
    Some(if by == 0.0 {
        format!("{what}: {side}")
    } else {
        format!(
            "{what}: {side}, {}{}",
            if by > 0.0 { "+" } else { "" },
            crate::units::len(by)
        )
    })
}

/// Has the layer end or edge of grip `kind` on `e` been moved by hand?
fn edited_end(e: Option<&Element>, kind: HandleKind) -> bool {
    e.and_then(|e| layer_offset(&e.kind, kind))
        .is_some_and(|by| by != 0.0)
}

fn elevation(model: &Model, level: Option<ElementId>) -> f64 {
    level
        .and_then(|l| model.level(l))
        .map_or(0.0, |l| l.elevation)
}

/// The handles of element `id`. `face` is the selected face of a mass.
/// The section a reference is shown in, as its line's start and direction;
/// none when it is shown in plans (its coordinates are then the plan's).
fn ref_plane(model: &Model, r: &creabim_core::Reference) -> Option<(DVec2, DVec2)> {
    let v = r.shown_in.first()?;
    match &model.get(*v)?.kind {
        ElementKind::View(creabim_core::View {
            kind: creabim_core::ViewKind::Section { start, end, .. },
            ..
        }) => Some((*start, (*end - *start).normalize_or_zero())),
        _ => None,
    }
}

/// A reference's point (its views' coordinates) in the model: on a plan's
/// ground, or standing on its section's plane.
pub fn ref_world(model: &Model, r: &creabim_core::Reference, p: DVec2) -> DVec3 {
    match ref_plane(model, r) {
        Some((s, d)) => (s + d * p.x).extend(p.y),
        // on the level of the plan it is shown in, so 3D shows it there
        None => {
            let z = r.shown_in.first().and_then(|v| match &model.get(*v)?.kind {
                ElementKind::View(creabim_core::View {
                    kind: creabim_core::ViewKind::Plan { level, .. },
                    ..
                }) => model.level(*level).map(|l| l.elevation),
                _ => None,
            });
            p.extend(z.unwrap_or(0.0))
        }
    }
}

/// The other way: a model point in the reference's views' coordinates.
fn ref_local(model: &Model, r: &creabim_core::Reference, w: DVec3) -> DVec2 {
    match ref_plane(model, r) {
        Some((s, d)) => DVec2::new((w.truncate() - s).dot(d), w.z),
        None => w.truncate(),
    }
}

/// A grown tree's side grips' names, east and counter-clockwise.
const REACH_NAMES: [&str; 8] = [
    "Crown east",
    "Crown north-east",
    "Crown north",
    "Crown north-west",
    "Crown west",
    "Crown south-west",
    "Crown south",
    "Crown south-east",
];

pub fn handles(model: &Model, id: ElementId, face: Option<usize>) -> Vec<Handle> {
    let Some(e) = model.get(id) else {
        return vec![];
    };
    let line = |kind, at: DVec3, dir: DVec3| Handle {
        kind,
        at,
        constraint: Constraint::Line {
            point: at,
            dir: dir.normalize(),
        },
        nudge: 0.0,
    };
    let flat = |kind, at: DVec3| Handle {
        kind,
        at,
        constraint: Constraint::Plane {
            point: at,
            normal: DVec3::Z,
        },
        nudge: 0.0,
    };
    match &e.kind {
        ElementKind::Wall(w) => {
            let z0 = elevation(model, Some(w.level)) + w.base_offset;
            // a curved wall's middle is on its arc, square to it there
            let half = w.length() / 2.0;
            let mid = w.place(half, 0.0);
            let n = w.tangent_at(half).perp();
            // the ends are gripped on the wall's location line
            let (a, b) = walls::location_line(model, w);
            let bend = w
                .is_arc()
                .then(|| flat(HandleKind::WallBend, mid.extend(z0)));
            // on a curved wall the bend grip has the middle: the top and
            // thickness grips stand a quarter along, so seen from above
            // they don't sit on it and fight it for the pointer
            let (mid, n) = if w.is_arc() {
                let q = w.length() / 4.0;
                (w.place(q, 0.0), w.tangent_at(q).perp())
            } else {
                (mid, n)
            };
            bend.into_iter()
                .chain([
                    flat(HandleKind::WallEnd { end: false }, a.extend(z0)),
                    flat(HandleKind::WallEnd { end: true }, b.extend(z0)),
                    line(HandleKind::WallTop, mid.extend(z0 + w.height), DVec3::Z),
                    line(
                        HandleKind::WallFace,
                        (mid + n * w.thickness / 2.0).extend(z0 + w.height / 2.0),
                        n.extend(0.0),
                    ),
                    // no grip of its own on screen: a drag of the wall's body
                    // slides it (see interact); kept here for that drag
                    line(
                        HandleKind::WallSlide,
                        (mid - n * (w.thickness / 2.0 + 0.3)).extend(z0 + w.height / 2.0),
                        n.extend(0.0),
                    ),
                ])
                .collect()
        }
        ElementKind::Slab(s) => {
            // its outline's grips are every outline's (see handles_in)
            let z = s.top(elevation(model, Some(s.level)));
            let n = s.boundary.len();
            // the face a thickness handle sits on is the one that moves
            let c = s.boundary.iter().copied().sum::<DVec2>() / n.max(1) as f64;
            vec![
                line(HandleKind::SlabTop, c.extend(z), DVec3::Z),
                line(
                    HandleKind::SlabUnderside,
                    c.extend(z - s.thickness),
                    DVec3::Z,
                ),
            ]
        }
        ElementKind::Opening(o) => {
            let Some(ElementKind::Wall(w)) = model.get(o.host).map(|h| &h.kind) else {
                return vec![];
            };
            let z0 = elevation(model, Some(w.level)) + w.base_offset;
            let mid = o.offset + o.width / 2.0;
            // along the wall at the opening: on a curved wall its tangent
            // there, and the grips on the arc where the opening is
            let u = w.tangent_at(mid);
            // on the jambs, at the middle of the wall's depth, so they read
            // as part of the opening at every scale (A5-5); not on a face,
            // where at 1:10 they floated beside it
            let at = |along: f64, up: f64| w.place(along, 0.0).extend(z0 + up);
            let mut out = vec![
                line(
                    HandleKind::OpeningJamb { right: false },
                    at(o.offset, o.sill + o.height / 2.0),
                    u.extend(0.0),
                ),
                line(
                    HandleKind::OpeningJamb { right: true },
                    at(o.offset + o.width, o.sill + o.height / 2.0),
                    u.extend(0.0),
                ),
                line(
                    HandleKind::OpeningHead,
                    at(mid, o.sill + o.height),
                    DVec3::Z,
                ),
            ];
            if matches!(o.kind, OpeningKind::Window | OpeningKind::Hole) {
                out.push(line(HandleKind::OpeningSill, at(mid, o.sill), DVec3::Z));
            }
            // in the middle, to slide it along its wall
            out.push(line(
                HandleKind::OpeningSlide,
                at(mid, o.sill + o.height / 2.0),
                u.extend(0.0),
            ));
            out
        }
        ElementKind::Shaft(s) => {
            let (z0, z1) = s.span(model);
            let n = s.boundary.len().max(1) as f64;
            let c = s.boundary.iter().copied().sum::<DVec2>() / n;
            vec![
                line(HandleKind::ShaftBase, c.extend(z0), DVec3::Z),
                line(HandleKind::ShaftTop, c.extend(z1), DVec3::Z),
            ]
        }
        // a detail view's box on its parent, not the line it was cut on
        ElementKind::View(v) if v.callout.is_some() => callout_grips(model, v),
        ElementKind::View(v) => match v.kind {
            ViewKind::Section {
                start, end, depth, ..
            } => {
                let look = (end - start).normalize_or_zero().perp();
                vec![
                    flat(HandleKind::ViewEnd { end: false }, start.extend(0.0)),
                    flat(HandleKind::ViewEnd { end: true }, end.extend(0.0)),
                    line(
                        HandleKind::ViewDepth,
                        ((start + end) / 2.0 + look * depth).extend(0.0),
                        look.extend(0.0),
                    ),
                ]
            }
            _ => vec![],
        },
        ElementKind::CropBox(c) => {
            let mut out: Vec<Handle> = (0..6)
                .map(|i| {
                    let (at, n) = c.face(i);
                    line(HandleKind::CropFace(i), at, n)
                })
                .collect();
            out.push(flat(HandleKind::CropMove, c.min));
            out
        }
        ElementKind::Detail(d) => detail_grips(model, d),
        // a reference: its middle moves it, its corners scale it, the
        // turn grip above its top turns it; none when it is locked
        ElementKind::Reference(r) if !r.locked => {
            // in a section the grips stand on its plane (MU32: they lay
            // on the ground at the section's numbers)
            let w = |p: DVec2| ref_world(model, r, p);
            // dragged on its own plane: a section's upright one, so the
            // middle moves up and down in 3D too (MU33)
            let normal = ref_plane(model, r).map_or(DVec3::Z, |(_, d)| d.perp().extend(0.0));
            let on = |kind, at: DVec3| Handle {
                kind,
                at,
                constraint: Constraint::Plane { point: at, normal },
                nudge: 0.0,
            };
            let Some(c) = r.outline() else {
                return vec![on(HandleKind::RefMove, w(r.at))];
            };
            let mut out: Vec<Handle> = (0..4)
                .map(|i| on(HandleKind::RefScale(i), w(c[i])))
                .collect();
            out.push(on(HandleKind::RefMove, w(r.at)));
            // turning is in plan only; a section's is typed in Properties
            if ref_plane(model, r).is_none() {
                let top = (c[2] + c[3]) / 2.0;
                let up = (top - r.at).normalize_or_zero();
                out.push(flat(HandleKind::Rotate, (top + up * 0.6).extend(0.0)));
            }
            out
        }
        ElementKind::Site(s) => match s.form {
            creabim_core::SiteForm::Plant {
                position,
                base,
                canopy,
                height,
                ..
            } => {
                let _ = base;
                let z = creabim_geom::earth::plant_z(model, s);
                if let Some(t) = s.tree_shape() {
                    // the top, the crown's underside, and its eight sides
                    // at its widest: shaped in plan, a section or 3D
                    let wide = z + t.crown_base + t.widest() * (height - t.crown_base);
                    let mut out: Vec<Handle> = (0..8u8)
                        .map(|i| {
                            let dir = t.side(i as usize);
                            let at = position + dir * t.radius_towards(canopy, dir);
                            line(HandleKind::PlantReach(i), at.extend(wide), dir.extend(0.0))
                        })
                        .collect();
                    out.push(line(
                        HandleKind::PlantHeight,
                        position.extend(z + height),
                        DVec3::Z,
                    ));
                    out.push(line(
                        HandleKind::PlantCrownBase,
                        position.extend(z + t.crown_base),
                        DVec3::Z,
                    ));
                    return out;
                }
                vec![
                    line(
                        HandleKind::PlantCanopy,
                        (position + DVec2::X * canopy / 2.0).extend(z),
                        DVec3::X,
                    ),
                    line(
                        HandleKind::PlantHeight,
                        position.extend(z + height),
                        DVec3::Z,
                    ),
                ]
            }
            // a property: a grip on each setback line, sliding it in or out
            creabim_core::SiteForm::Property {
                ref boundary,
                ref edges,
                ref rules,
            } => {
                let z = elevation(model, Some(s.level));
                let n = boundary.len();
                let Some(b) = creabim_core::lot::buildable(boundary, edges, rules) else {
                    return vec![];
                };
                (0..n)
                    .filter_map(|i| {
                        if creabim_core::lot::edge_kind(edges, i, n) == creabim_core::LotEdge::Open
                        {
                            return None;
                        }
                        let d = (boundary[(i + 1) % n] - boundary[i]).normalize_or_zero();
                        let mid = (b[i] + b[(i + 1) % n]) / 2.0;
                        Some(line(
                            HandleKind::Setback(i),
                            mid.extend(z),
                            d.perp().extend(0.0),
                        ))
                    })
                    .collect()
            }
            // its outline's grips are every outline's (see handles_in)
            _ => vec![],
        },
        ElementKind::Grid(g) => {
            let mut out = vec![
                flat(HandleKind::GridEnd { end: false }, g.start.extend(0.0)),
                flat(HandleKind::GridEnd { end: true }, g.end.extend(0.0)),
            ];
            for (end, on) in [(false, g.bubbles.at_start()), (true, g.bubbles.at_end())] {
                if on {
                    let (at, _) =
                        creabim_core::grids::bubble(g, end, creabim_core::grids::BUBBLE_OUT);
                    out.push(flat(HandleKind::GridBubble { end }, at.extend(0.0)));
                }
            }
            if g.bulge != 0.0 {
                let mid = creabim_core::grids::curve(g).at(0.5);
                out.push(flat(HandleKind::GridRise, mid.extend(0.0)));
            }
            out
        }
        ElementKind::FreeMesh(m) => {
            // at rest only the main shape editing (MU18): its base point
            // to move it, the turn grip, and the clicked face's push/pull;
            // corners, edges and every face's arrow wait for Edit mode
            let base = m.origin + DVec3::Z * elevation(model, m.level);
            let mut out: Vec<Handle> = face
                .filter(|f| *f < m.mesh.faces.len())
                .and_then(|f| {
                    let c = base + m.to_model(m.mesh.face_center(f));
                    let n = m.to_model(m.mesh.face_normal(f)).normalize_or_zero();
                    (n != DVec3::ZERO).then(|| line(HandleKind::Face(f), c, n))
                })
                .into_iter()
                .collect();
            out.push(flat(HandleKind::MeshMove, base));
            let reach = m
                .mesh
                .vertices
                .iter()
                .map(|v| m.to_model(*v).truncate().length())
                .fold(0.0_f64, f64::max);
            let turn = (DVec2::from_angle(m.rotation.to_radians()) * (reach + 0.4)).extend(0.0);
            out.push(flat(HandleKind::Rotate, base + turn));
            out
        }
        ElementKind::Object(o) => {
            let z = creabim_core::objects::base_z(model, e).unwrap_or(0.0);
            let (lo, hi) = creabim_core::objects::type_of(model, e)
                .map(|t| t.bounds())
                .unwrap_or((DVec3::new(-0.3, -0.6, 0.0), DVec3::new(0.3, 0.0, 1.0)));
            let front = DVec2::new((lo.x + hi.x) / 2.0, lo.y - 0.25);
            let mut out = vec![flat(HandleKind::Rotate, o.to_plan(front).extend(z))];
            // its corners, shown one at a time under the pointer (MU142)
            if let Some(corners) = object_corners(model, e) {
                for (i, c) in corners.iter().enumerate() {
                    out.push(flat(HandleKind::ScaleCorner(i as u8), c.extend(z)));
                }
            }
            out
        }
        _ => creabim_core::members::grips(model, e)
            .into_iter()
            .enumerate()
            // a roof's corners and a railing's path points are outline
            // corners (see handles_in); the grips after them keep their
            // numbers
            .filter(|(i, _)| *i >= outline_grips(e))
            .map(|(i, g)| match g.how {
                creabim_core::members::GripMove::Flat => {
                    flat(HandleKind::Member { i, free: true }, g.at)
                }
                creabim_core::members::GripMove::Vertical => {
                    line(HandleKind::Member { i, free: false }, g.at, DVec3::Z)
                }
                creabim_core::members::GripMove::Along(d) => {
                    line(HandleKind::Member { i, free: false }, g.at, d.extend(0.0))
                }
            })
            .chain(member_extras(model, e))
            .collect(),
    }
}

/// Is a callout on `parent` drawn as a ring? (See the parent's annotations.)
fn callout_is_round(model: &Model, parent: ElementId) -> bool {
    matches!(
        model.get(parent).map(|e| &e.kind),
        Some(ElementKind::View(v)) if matches!(v.kind, ViewKind::Section { .. })
    ) && creabim_core::styles::active_pack(model)
        .marker_parts()
        .is_some()
}

/// The corners of a callout's box, `lo` first and counter-clockwise.
fn box_corners(lo: DVec2, hi: DVec2) -> [DVec2; 4] {
    [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)]
}

/// A callout's grips on its parent view: a square on each corner of its
/// box and a bar on each edge.
fn callout_grips(model: &Model, v: &creabim_core::View) -> Vec<Handle> {
    let Some(c) = v.callout else { return vec![] };
    let Some(frame) = creabim_core::DrawingFrame::of(model, c.parent) else {
        return vec![];
    };
    let normal = frame.normal();
    let k = box_corners(c.lo, c.hi);
    let dir = |d: DVec2| (frame.to_model(d) - frame.to_model(DVec2::ZERO)).normalize_or_zero();
    let outs = [-DVec2::Y, DVec2::X, DVec2::Y, -DVec2::X];
    let mut out: Vec<Handle> = (0..4)
        .map(|i| {
            let at = frame.to_model(k[i]);
            Handle {
                kind: HandleKind::CalloutCorner(i),
                at,
                constraint: Constraint::Plane { point: at, normal },
                nudge: 0.0,
            }
        })
        .collect();
    for (i, o) in outs.iter().enumerate() {
        let at = frame.to_model((k[i] + k[(i + 1) % 4]) / 2.0);
        out.push(Handle {
            kind: HandleKind::CalloutEdge(i),
            at,
            constraint: Constraint::Line {
                point: at,
                dir: dir(*o),
            },
            nudge: 0.0,
        });
    }
    out
}

/// A detail component's own grips besides its points (see handles_in): a
/// width grip off the middle of its first stretch, and a repeating
/// detail's unit grip at the end of its first unit.
fn detail_grips(model: &Model, d: &creabim_core::DetailItem) -> Vec<Handle> {
    let Some(frame) = creabim_core::DrawingFrame::of(model, d.view) else {
        return vec![];
    };
    let mut out = vec![];
    let (Some(a), Some(b)) = (d.points.first(), d.points.get(1)) else {
        return out;
    };
    let across = d.across();
    let dir = |v: DVec2| (frame.to_model(v) - frame.to_model(DVec2::ZERO)).normalize_or_zero();
    let widened = !matches!(
        d.kind,
        DetailKind::Membrane | DetailKind::Region { .. } | DetailKind::BreakLine
    );
    if widened {
        let at = frame.to_model((*a + *b) / 2.0 + across * d.width / 2.0);
        out.push(Handle {
            kind: HandleKind::DetailWidth,
            at,
            constraint: Constraint::Line {
                point: at,
                dir: dir(across),
            },
            nudge: 0.0,
        });
    }
    if let DetailKind::Repeat { module, .. } = d.kind {
        let along = (*b - *a).normalize_or_zero();
        let at = frame.to_model(d.at_length(module));
        out.push(Handle {
            kind: HandleKind::DetailSpacing,
            at,
            constraint: Constraint::Line {
                point: at,
                dir: dir(along),
            },
            nudge: 0.0,
        });
    }
    out
}

/// The grips a column and a stair have besides their members' own: a
/// turning grip, and a column's width and depth.
fn member_extras(model: &Model, e: &Element) -> Vec<Handle> {
    let line = |kind, at: DVec3, dir: DVec2| Handle {
        kind,
        at,
        constraint: Constraint::Line {
            point: at,
            dir: dir.normalize().extend(0.0),
        },
        nudge: 0.0,
    };
    let flat = |kind, at: DVec3| Handle {
        kind,
        at,
        constraint: Constraint::Plane {
            point: at,
            normal: DVec3::Z,
        },
        nudge: 0.0,
    };
    match &e.kind {
        ElementKind::Column(c) => {
            let (z0, z1) = c.span(model);
            // its size bars half-way up its sides, in sections too (A4-8)
            let zm = (z0 + z1) / 2.0;
            let x = DVec2::from_angle(c.rotation.to_radians());
            let (w, d) = match c.profile {
                creabim_core::members::Profile::Rectangle { width, depth } => (width, depth),
                creabim_core::members::Profile::Round { diameter } => (diameter, diameter),
            };
            let mut out = vec![
                flat(
                    HandleKind::Rotate,
                    (c.position + x * (w / 2.0 + 0.35)).extend(z0),
                ),
                line(
                    HandleKind::ColumnSize { depth: false },
                    (c.position + x * w / 2.0).extend(zm),
                    x,
                ),
            ];
            if matches!(c.profile, creabim_core::members::Profile::Rectangle { .. }) {
                out.push(line(
                    HandleKind::ColumnSize { depth: true },
                    (c.position + x.perp() * d / 2.0).extend(zm),
                    x.perp(),
                ));
            }
            out
        }
        ElementKind::Beam(b) => {
            let n = (b.end - b.start).normalize_or_zero().perp();
            let mid = (b.start + b.end) / 2.0;
            vec![line(
                HandleKind::BeamWidth,
                // half-way down its side, in sections too (A4-8)
                (mid + n * b.width / 2.0).extend(b.top(model) - b.depth / 2.0),
                n,
            )]
        }
        ElementKind::Stair(s) => {
            let dir = DVec2::from_angle(s.direction.to_radians());
            let mut out = vec![flat(
                HandleKind::Rotate,
                (s.start - dir * 0.6).extend(s.base(model)),
            )];
            // its UP label: moved by itself, off a room's tag or a
            // fitting it covered (MU144)
            if let Some(up) = s.up_label_at(&s.layout(s.rise(model))) {
                out.push(flat(HandleKind::StairUp, up.extend(s.base(model))));
            }
            out
        }
        _ => vec![],
    }
}

/// The point a rotate grip turns its element about, in plan.
/// How far a drag of the turn dot turns (MU142), from the `raw` angle the
/// pointer has gone round: in Rotate's step from its row (5° when Free),
/// locking to 0, ±90 and 180 as it passes them, as the R key does; free
/// with Alt or Step off (`step` 0). Returns the turn and the angle it
/// locked to.
pub(crate) fn grip_turn(raw: f64, _step: f64) -> (f64, Option<f64>) {
    // free only with Alt held: the length step (Grid snap) has nothing to
    // say about angles (MU142: with Grid snap Off it never stepped or
    // locked)
    if GRIP_FREE.load(std::sync::atomic::Ordering::Relaxed) {
        (raw, None)
    } else {
        crate::modal::turn_degrees(raw, false, 1.0, crate::modal::rotate_step())
    }
}

/// How far outside the held turn dot the protractor's ring runs, px.
pub(crate) const RING_CLEAR: f32 = 22.0;

/// Alt held while a grip is dragged: a turn goes free (set each frame).
pub(crate) static GRIP_FREE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// The protractor round what a turn-dot drag turns (MU142): from where
/// the dot was grabbed to where the pointer has taken it.
fn draw_grip_protractor(painter: &Painter, input: &PaneInput, model: &Model, d: &Drag, now: DVec3) {
    let Some(e) = d.edit.originals().first() else {
        return;
    };
    let Some(c) = pivot_of(model, e) else {
        return;
    };
    let z = creabim_core::objects::base_z(model, e).unwrap_or(d.grab.z);
    let (Some(ps), Some(g)) = ((input.project)(c.extend(z)), (input.project)(d.grab)) else {
        return;
    };
    let (a0, a1) = (d.grab.truncate() - c, now.truncate() - c);
    if a1.length() < 1e-6 {
        return;
    }
    let (by, locked) = grip_turn(a0.angle_to(a1).to_degrees(), crate::snaps::current_step());
    // on screen the plan's anticlockwise may read either way (a view from
    // below): the arc follows the screen
    let screen = |p: Pos2| (-(p.y - ps.y)).atan2(p.x - ps.x);
    let start = screen(g);
    let toward =
        (input.project)((c + DVec2::from_angle(a0.to_angle() + 0.3) * a0.length()).extend(z))
            .map_or(1.0, |q| {
                let d = (screen(q) - start + std::f32::consts::PI)
                    .rem_euclid(std::f32::consts::TAU)
                    - std::f32::consts::PI;
                if d < 0.0 {
                    -1.0
                } else {
                    1.0
                }
            });
    // the ring a step outside the turn dot, so the two don't sit on top
    // of each other (MU142: "too close for comfort"); taken on the ring,
    // it stays the ring it was taken on
    let r = turn_ring(model, e, input).map_or((ps.distance(g) + RING_CLEAR).max(56.0), |(_, r)| r);
    let ink = if locked.is_some() {
        crate::modal::GUIDE_PINK
    } else {
        crate::theme::interaction()
    };
    let from = crate::modal::OrientFrom {
        look: Some(input.look),
        ..Default::default()
    };
    let way = crate::modal::zero_way_from(
        model,
        d.edit.originals(),
        crate::modal::rotate_orient(),
        from,
    );
    let zero = (input.project)((c + way).extend(z)).map_or(start, screen);
    let (mut shapes, labels) =
        crate::modal::protractor(ps, r, zero, start, by as f32 * toward, ink);
    // the held dot marks where the turn is: no second dot beside it
    shapes.pop();
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

/// The protractor's ring round what `e`'s turn dot turns, on screen: its
/// middle and its radius, a step outside the dot.
fn turn_ring(model: &Model, e: &Element, input: &PaneInput) -> Option<(Pos2, f32)> {
    let c = pivot_of(model, e)?;
    let dot = handles_in(model, e.id, None, false)
        .into_iter()
        .find(|h| h.kind == HandleKind::Rotate)?;
    let z = creabim_core::objects::base_z(model, e).unwrap_or(dot.at.z);
    let (ps, g) = ((input.project)(c.extend(z))?, (input.project)(dot.at)?);
    Some((ps, (ps.distance(g) + RING_CLEAR).max(56.0)))
}

/// The protractor shown round `e` while the pointer rests on its turn dot
/// or on the ring (MU142, decision 10): a press anywhere on the ring turns
/// it as the dot does.
fn draw_turn_ring(painter: &Painter, input: &PaneInput, model: &Model, e: &Element) {
    let (Some((ps, r)), Some(c)) = (turn_ring(model, e, input), pivot_of(model, e)) else {
        return;
    };
    let z = creabim_core::objects::base_z(model, e).unwrap_or(0.0);
    let screen = |p: Pos2| (-(p.y - ps.y)).atan2(p.x - ps.x);
    let from = crate::modal::OrientFrom {
        look: Some(input.look),
        ..Default::default()
    };
    let way = crate::modal::zero_way_from(
        model,
        std::slice::from_ref(e),
        crate::modal::rotate_orient(),
        from,
    );
    let zero = (input.project)((c + way).extend(z)).map_or(0.0, screen);
    let ink = crate::theme::interaction().gamma_multiply(0.75);
    let (mut shapes, labels) = crate::modal::protractor(ps, r, zero, zero, 0.0, ink);
    // no dot of its own: the turn dot is there
    shapes.pop();
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

/// The element whose protractor is shown at rest, and the pane it shows
/// in: lit by resting on its turn dot, kept while the pointer stays within
/// the ring. Only that pane lets go of it: the other panes, the pointer
/// away from them, put it out on every frame (MU142, seen in the app: a
/// press on the ring moved the object).
static RING_LIT: std::sync::Mutex<Option<(ElementId, egui::Id)>> = std::sync::Mutex::new(None);

fn ring_lit(pane: egui::Id) -> Option<ElementId> {
    RING_LIT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .filter(|(_, p)| *p == pane)
        .map(|(id, _)| id)
}

fn set_ring_lit(lit: Option<(ElementId, egui::Id)>) {
    *RING_LIT.lock().unwrap_or_else(|e| e.into_inner()) = lit;
}

fn pivot_of(model: &Model, e: &Element) -> Option<DVec2> {
    match &e.kind {
        ElementKind::Object(o) => {
            let (lo, hi) = creabim_core::objects::type_of(model, e)?.bounds();
            Some(o.to_plan((lo.truncate() + hi.truncate()) / 2.0))
        }
        ElementKind::Column(c) => Some(c.position),
        ElementKind::Stair(s) => Some(s.start),
        ElementKind::FreeMesh(m) => Some(m.origin.truncate()),
        ElementKind::Reference(r) => Some(r.at),
        _ => None,
    }
}

/// What Edit mode adds on a mass of a few faces (A7-9, MU18): an arrow on
/// every other face, a grip on every corner and edge middle, and the
/// up/down arrow.
fn mass_edit_handles(model: &Model, id: ElementId, face: Option<usize>) -> Vec<Handle> {
    let Some(ElementKind::FreeMesh(m)) = model.get(id).map(|e| &e.kind) else {
        return vec![];
    };
    if m.mesh.faces.len() > MASS_GRIP_FACES || m.mesh.vertices.len() > MASS_GRIP_CORNERS {
        return vec![];
    }
    let base = m.origin + DVec3::Z * elevation(model, m.level);
    let line = |kind, at: DVec3, dir: DVec3| Handle {
        kind,
        at,
        constraint: Constraint::Line {
            point: at,
            dir: dir.normalize(),
        },
        nudge: 0.0,
    };
    let flat = |kind, at: DVec3| Handle {
        kind,
        at,
        constraint: Constraint::Plane {
            point: at,
            normal: DVec3::Z,
        },
        nudge: 0.0,
    };
    let mut out: Vec<Handle> = (0..m.mesh.faces.len())
        .filter(|f| Some(*f) != face)
        .filter_map(|f| {
            let c = base + m.to_model(m.mesh.face_center(f));
            let n = m.to_model(m.mesh.face_normal(f)).normalize_or_zero();
            (n != DVec3::ZERO).then(|| line(HandleKind::Face(f), c, n))
        })
        .collect();
    let at = |i: u32| base + m.to_model(m.mesh.vertices[i as usize]);
    for i in 0..m.mesh.vertices.len() {
        out.push(flat(HandleKind::MeshVertex(i), at(i as u32)));
    }
    for (a, b) in mesh_edges(&m.mesh) {
        out.push(flat(HandleKind::MeshEdge(a, b), (at(a) + at(b)) / 2.0));
    }
    let top = m
        .mesh
        .vertices
        .iter()
        .map(|v| m.to_model(*v).z)
        .fold(0.0_f64, f64::max);
    out.push(line(
        HandleKind::MeshLift,
        base + DVec3::Z * (top + 0.4),
        DVec3::Z,
    ));
    out
}

/// Above these, a mass shows only the clicked face's arrow: grips on
/// every corner of a detailed mesh would bury it.
const MASS_GRIP_FACES: usize = 2000;
const MASS_GRIP_CORNERS: usize = 4000;

/// Each edge of a mesh once, its corners in order.
pub fn mesh_edges(mesh: &creabim_core::editmesh::EditMesh) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = mesh
        .faces
        .iter()
        .flat_map(|f| (0..f.len()).map(move |i| (f[i], f[(i + 1) % f.len()])))
        .map(|(a, b)| (a.min(b), a.max(b)))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// An object turned `by` degrees about the middle of its footprint.
/// The corners of an object's outline in plan: its type's extent, lo
/// first, anticlockwise in its own frame, placed as it stands.
pub fn object_corners(model: &Model, e: &Element) -> Option<[DVec2; 4]> {
    let ElementKind::Object(o) = &e.kind else {
        return None;
    };
    let (lo, hi) = creabim_core::objects::type_of(model, e)?.bounds();
    let (lo, hi) = (lo.truncate(), hi.truncate());
    if (hi - lo).min_element() < 1e-6 {
        return None;
    }
    Some([lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)].map(|q| o.to_plan(q)))
}

pub fn turn_object(model: &Model, e: &mut Element, by: f64) {
    let pivot = pivot_of(model, e);
    if let ElementKind::Object(o) = &mut e.kind {
        let c = pivot.unwrap_or(o.position);
        let r = DVec2::from_angle(by.to_radians());
        o.position = c + r.rotate(o.position - c);
        o.rotation = (o.rotation + by).rem_euclid(360.0);
    }
}

/// How many of a member's grips are corners of its outline, given as
/// outline grips instead.
fn outline_grips(e: &Element) -> usize {
    match &e.kind {
        ElementKind::Roof(r) => r.boundary.len(),
        ElementKind::Railing(r) if r.stair.is_none() => r.path.len(),
        _ => 0,
    }
}

/// A handle that moves across the plan, seen in a section or elevation
/// (looking level along `look`): there it slides across the paper, along
/// the section line, so wall ends, slab corners, beam ends and the like
/// can be dragged there too.
pub fn in_section(h: Handle, look: DVec3) -> Handle {
    // layer edges are drawn apart only in plan, where they would sit on
    // one another; elsewhere each is at its own layer's depth
    if matches!(h.kind, HandleKind::LayerEdge { .. }) && look.z.abs() < 0.99 {
        return Handle { nudge: 0.0, ..h };
    }
    match h.constraint {
        Constraint::Plane { point, normal } if normal.z.abs() > 0.99 && look.z.abs() < 0.1 => {
            Handle {
                constraint: Constraint::Line {
                    point,
                    dir: DVec3::Z.cross(look).normalize_or(DVec3::X),
                },
                ..h
            }
        }
        _ => h,
    }
}

/// The elements a drag of `kind` on `id` changes: the element first, then
/// for a wall end the walls whose ends sit on it (so corners stay closed).
/// The second list says which end of each joined wall moves.
pub fn originals(
    model: &Model,
    id: ElementId,
    kind: HandleKind,
) -> (Vec<Element>, Vec<(usize, bool)>) {
    let Some(e) = model.get(id) else {
        return (vec![], vec![]);
    };
    let mut out = vec![e.clone()];
    let mut joined = vec![];
    if let (HandleKind::WallEnd { end }, ElementKind::Wall(w)) = (kind, &e.kind) {
        let p = if end { w.end } else { w.start };
        // an end that may not join drags alone
        let models = if w.disallow_join[usize::from(end)] {
            None
        } else {
            Some(model)
        };
        for other in models.into_iter().flat_map(|m| m.iter()) {
            if let ElementKind::Wall(o) = &other.kind {
                if other.id == id || o.level != w.level {
                    continue;
                }
                for (oe, q) in [(false, o.start), (true, o.end)] {
                    if q.distance(p) < 1e-3 && !o.disallow_join[usize::from(oe)] {
                        out.push(other.clone());
                        joined.push((out.len() - 1, oe));
                    }
                }
            }
        }
    }
    (out, joined)
}

fn round(v: f64, step: f64) -> f64 {
    if step > 0.0 {
        (v / step).round() * step
    } else {
        v
    }
}

/// The dragged elements with handle `kind` moved from `grab` to `now`
/// (model points on the handle's constraint). `snapped` is `now` already
/// snapped to something, for handles that move freely. Lengths round to
/// `step`. Returns an explanation when the drag would break the element.
pub fn apply(
    model: &Model,
    originals: &[Element],
    joined: &[(usize, bool)],
    kind: HandleKind,
    grab: DVec3,
    now: DVec3,
    step: f64,
) -> Result<Vec<Element>, String> {
    let mut out = originals.to_vec();
    let d = now - grab;
    let mut moved_end = None;
    let first = out.first_mut().ok_or("nothing to drag")?;
    match (&mut first.kind, kind) {
        (ElementKind::Wall(w), HandleKind::CurtainLine { upright, at }) => {
            let old = at as f64 / 1e6;
            let Some((us, vs, height)) = curtain_lines(model, w) else {
                return Err("not a curtain wall".into());
            };
            let (len, lines) = if upright {
                (w.length(), us)
            } else {
                (height, vs)
            };
            let along = if upright {
                d.truncate().dot(w.tangent_at(old))
            } else {
                d.z
            };
            let new = round(old + along, step).clamp(0.0, len);
            let Some(g) = w.curtain.as_mut() else {
                return Err("not a curtain wall".into());
            };
            let hand = if upright {
                &mut g.vertical_hand
            } else {
                &mut g.horizontal_hand
            };
            // where it was: a hand line moves, a rule line is taken away
            if let Some(i) = hand.added.iter().position(|x| (x - old).abs() < 1e-4) {
                hand.added.remove(i);
            } else {
                hand.removed.push(old);
            }
            // dropped on its neighbour or an edge, it goes; else it stands
            // where it was dropped
            let onto = lines
                .iter()
                .filter(|x| (**x - old).abs() > 1e-4)
                .chain([0.0, len].iter())
                .any(|x| (x - new).abs() < 0.03);
            if !onto {
                hand.added.push(new);
            }
        }
        (ElementKind::Wall(w), HandleKind::WallEnd { end }) => {
            // the grip is on the location line: move that, and the
            // centreline with it
            let p = now.truncate();
            let (mut a, mut b) = walls::location_line(model, w);
            if end {
                b = p;
            } else {
                a = p;
            }
            if a.distance(b) < 0.05 {
                return Err("a wall needs some length".into());
            }
            walls::set_location_line(model, w, a, b);
            moved_end = Some(if end { w.end } else { w.start });
        }
        (ElementKind::Wall(w), HandleKind::LayerEnd { layer, end }) => {
            // out past the end is more, back along the wall less
            let along = d.truncate().dot(w.direction());
            let by = round(if end { along } else { -along }, step / 5.0);
            let old = w.layer_ext(layer)[usize::from(end)];
            let len = w.length();
            if old + by < -len / 2.0 {
                return Err("a layer can't be held back past the wall's middle".into());
            }
            w.set_layer_ext(layer, end, old + by);
        }
        (ElementKind::Wall(w), HandleKind::LayerHeight { layer, top }) => {
            // up is more at the top, down is more at the bottom
            let by = round(if top { d.z } else { -d.z }, step / 5.0);
            let old = w.layer_vext(layer)[usize::from(top)];
            if old + by < -w.height / 2.0 {
                return Err("a layer can't stop short past the wall's middle height".into());
            }
            w.set_layer_vext(layer, top, old + by);
        }
        (ElementKind::Stair(s), HandleKind::StairFlight { straight }) => {
            let lay = s.layout(s.rise(model));
            let f = lay
                .flights
                .iter()
                .find(|f| f.straight == straight)
                .ok_or("that flight is gone")?;
            let more = (d.truncate().dot(f.end_dir(s.tread)) / s.tread.max(0.01)).round() as i64;
            let t = (f.risers as i64 - 1 + more).max(1) as u32;
            s.pieces.set_treads(straight, Some(t));
        }
        (ElementKind::Stair(s), HandleKind::StairUp) => {
            // from where it stands before the flight; dropped by it, it
            // goes home and keeps no offset
            let lay = s.layout(s.rise(model));
            let mut home = s.clone();
            home.up_offset = None;
            let at = home.up_label_at(&lay).ok_or("the stair has no flight")?;
            let now = s.up_label_at(&lay).unwrap_or(at) + d.truncate();
            let off = now - at;
            s.up_offset = (off.length() > 0.05).then_some(off);
        }
        (ElementKind::Stair(s), HandleKind::StairLanding { turn }) => {
            let lay = s.layout(s.rise(model));
            let f = lay
                .flights
                .iter()
                .rev()
                .find(|f| f.straight <= turn)
                .ok_or("that landing is gone")?;
            // back down the flight is deeper
            let by = round(-d.truncate().dot(f.end_dir(s.tread)), step / 5.0);
            s.pieces
                .set_deeper(turn, (s.pieces.deeper(turn) + by).max(0.0));
        }
        (
            ek @ (ElementKind::Slab(_) | ElementKind::Roof(_)),
            HandleKind::LayerEdge { edge, .. },
        ) => {
            let boundary = match ek {
                ElementKind::Slab(s) => s.boundary.clone(),
                ElementKind::Roof(r) => r.boundary.clone(),
                _ => unreachable!(),
            };
            let out = edge_out(&boundary, edge);
            let by = round(d.truncate().dot(out), step / 5.0);
            let old = layer_offset(ek, kind).unwrap_or(0.0);
            set_layer_offset(ek, kind, old + by);
        }
        (ElementKind::Wall(w), HandleKind::WallTop) => match &mut w.top {
            // tied to a level: the drag sets how far above it the top is
            WallTop::UpToLevel { offset, .. } => {
                let h = round(w.height + d.z, step);
                if h < 0.1 {
                    return Err("too low".into());
                }
                *offset += h - w.height;
                w.height = h;
            }
            WallTop::Unconnected => {
                w.height = round(w.height + d.z, step);
                if w.height < 0.1 {
                    return Err("too low".into());
                }
            }
        },
        (ElementKind::Wall(w), HandleKind::WallFace) => {
            let n = w.direction().perp().extend(0.0);
            w.thickness = round(w.thickness + 2.0 * d.dot(n), step / 5.0);
            if w.thickness < 0.02 {
                return Err("too thin".into());
            }
        }
        (ElementKind::Slab(s), HandleKind::SlabUnderside) => {
            s.set_thickness(round(s.thickness - d.z, step / 5.0), false);
            if s.thickness < 0.02 {
                return Err("too thin".into());
            }
        }
        (ElementKind::Slab(s), HandleKind::SlabTop) => {
            s.set_thickness(round(s.thickness + d.z, step / 5.0), true);
            if s.thickness < 0.02 {
                return Err("too thin".into());
            }
        }
        (ElementKind::Opening(o), HandleKind::OpeningJamb { right }) => {
            let Some(ElementKind::Wall(w)) = model.get(o.host).map(|h| &h.kind) else {
                return Err("its wall is gone".into());
            };
            let along = round(d.truncate().dot(w.direction()), step);
            // a jamb stops at a wall joined at the corner (MU23)
            let (lo, hi) = creabim_core::walls::clear_span(model, o.host);
            let (lo, hi) = if o.offset >= lo - 1e-9 && o.offset + o.width <= hi + 1e-9 {
                (lo, hi)
            } else {
                (0.0, w.length())
            };
            if right {
                o.width = (o.width + along).min(hi - o.offset);
            } else {
                let along = along.max(lo - o.offset);
                o.offset += along;
                o.width -= along;
            }
            if o.width < 0.2 {
                return Err("too narrow".into());
            }
        }
        (ElementKind::Opening(o), HandleKind::OpeningHead) => {
            o.height = round(o.height + d.z, step);
        }
        (ElementKind::Opening(o), HandleKind::OpeningSill) => {
            let dz = round(d.z, step).clamp(-o.sill, o.height - 0.2);
            o.sill += dz;
            o.height -= dz;
        }
        (ElementKind::Site(st), HandleKind::Strip(i)) => {
            let creabim_core::SiteForm::Area {
                boundary,
                parking: Some(cp),
                ..
            } = &mut st.form
            else {
                return Err("only a car park has edge strips".into());
            };
            let n = boundary.len();
            if n < 3 || i >= n {
                return Err("that edge is gone".into());
            }
            let (a, b) = (boundary[i], boundary[(i + 1) % n]);
            let inward = (b - a).normalize_or_zero().perp();
            let step = if step > 0.0 { step } else { 0.05 };
            let w = round((now.truncate() - a).dot(inward), step).clamp(0.1, 30.0);
            match cp.strips.iter_mut().find(|s| s.edge == i) {
                Some(s) => s.width = w,
                None => return Err("that edge has no strip".into()),
            }
        }
        (
            ElementKind::Site(st),
            HandleKind::RowEnd {
                aisle,
                seg,
                left,
                end,
            },
        ) => {
            let creabim_core::SiteForm::Area {
                parking: Some(cp), ..
            } = &mut st.form
            else {
                return Err("only a car park has rows".into());
            };
            let Some(a) = cp.aisles.get_mut(aisle) else {
                return Err("that aisle is gone".into());
            };
            let (Some(p), Some(q)) = (a.path.get(seg), a.path.get(seg + 1)) else {
                return Err("that stretch of the aisle is gone".into());
            };
            let len = p.distance(*q);
            let along = (*q - *p).normalize_or_zero();
            // where along the stretch the grip now is, to the nearest
            // 50 mm when nothing else sets the step
            let step = if step > 0.0 { step } else { 0.05 };
            let s = round((now.truncate() - *p).dot(along), step).clamp(0.0, len);
            a.trim(seg, left, end, s);
        }
        (ElementKind::Site(st), HandleKind::Setback(i)) => {
            use creabim_core::LotEdge;
            let creabim_core::SiteForm::Property {
                boundary,
                edges,
                rules,
            } = &mut st.form
            else {
                return Err("only a property has setbacks".into());
            };
            let n = boundary.len();
            if n < 3 {
                return Err("its outline is gone".into());
            }
            let (a, b) = (boundary[i % n], boundary[(i + 1) % n]);
            let inward = (b - a).normalize_or_zero().perp();
            // how far in from the edge the grip now is, to the nearest
            // 50 mm when nothing else sets the step
            let step = if step > 0.0 { step } else { 0.05 };
            let far = round((now.truncate() - a).dot(inward), step).clamp(0.0, 500.0);
            if creabim_core::lot::edge_kind(edges, i, n) == LotEdge::Open {
                return Err("this edge has no setback".into());
            }
            // the edge's own: back on its kind's when slid to that value
            let kinds = rules.setback(creabim_core::lot::edge_kind(edges, i, n));
            rules.set_own_setback(i, ((far - kinds).abs() > 1e-6).then_some(far));
            if creabim_core::lot::buildable(boundary, edges, rules).is_none() {
                return Err("the setbacks would leave nothing to build on".into());
            }
        }
        (ElementKind::Grid(g), HandleKind::GridRise) => {
            // the arc through its ends and where the grip is now; on the
            // straight line it stays an arc, just barely
            let b = creabim_core::sketch::bulge_through(g.start, g.end, now.truncate());
            if b.is_finite() {
                g.bulge = if b.abs() < 1e-3 {
                    1e-3_f64.copysign(b)
                } else {
                    b.clamp(-1.0, 1.0)
                };
            }
        }
        (ElementKind::Grid(g), HandleKind::GridBubble { end }) => {
            // how far across the line the bubble now sits: its elbow
            let base = creabim_core::grids::curve(g).at(if end { 1.0 } else { 0.0 });
            let n = creabim_core::grids::outward(g, end).perp();
            let side = (now.truncate() - base).dot(n);
            g.elbows[usize::from(end)] = if side.abs() < 0.05 {
                0.0
            } else {
                round(side, step).clamp(-20.0, 20.0)
            };
        }
        (ElementKind::Grid(g), HandleKind::GridEnd { end }) => {
            // a stretch stays on the line's own axis unless pulled well off
            // it (MU74: it drifted off square when stretched); a curved
            // grid line's end moves freely
            let mut p = now.truncate();
            let (here, other) = if end {
                (g.end, g.start)
            } else {
                (g.start, g.end)
            };
            if g.bulge.abs() < 1e-9 {
                if let Some(dir) = (here - other).try_normalize() {
                    let on = other + dir * (p - other).dot(dir);
                    let len = here.distance(other);
                    if p.distance(on) < (len * 0.1).clamp(0.2, 0.5) {
                        p = on;
                    }
                }
            }
            if end {
                g.end = p;
            } else {
                g.start = p;
            }
        }
        (ElementKind::View(v), HandleKind::ViewEnd { end: which }) => {
            if let ViewKind::Section { start, end, .. } = &mut v.kind {
                let p = now.truncate();
                if which {
                    *end = p;
                } else {
                    *start = p;
                }
                if start.distance(*end) < 0.2 {
                    return Err("a view line needs some length".into());
                }
            }
        }
        (ElementKind::View(v), HandleKind::ViewDepth) => {
            if let ViewKind::Section {
                start, end, depth, ..
            } = &mut v.kind
            {
                let look = (*end - *start).normalize_or_zero().perp();
                let mid = (*start + *end) / 2.0;
                *depth = round((now.truncate() - mid).dot(look), step).max(0.5);
            }
        }
        (ElementKind::FreeMesh(m), HandleKind::Face(f)) => {
            let n = m.to_model(m.mesh.face_normal(f)).normalize_or_zero();
            let dist = round(d.dot(n), step / 5.0);
            let verts = m.mesh.faces[f].clone();
            let local = m.to_local(n * dist);
            m.mesh.translate_vertices(&verts, local);
        }
        (ElementKind::FreeMesh(m), HandleKind::MeshVertex(i)) => {
            let by = DVec3::new(round(d.x, step), round(d.y, step), 0.0);
            let local = m.to_local(by);
            m.mesh.translate_vertices(&[i as u32], local);
        }
        (ElementKind::FreeMesh(m), HandleKind::MeshEdge(a, b)) => {
            let by = DVec3::new(round(d.x, step), round(d.y, step), 0.0);
            let local = m.to_local(by);
            m.mesh.translate_vertices(&[a, b], local);
        }
        (ElementKind::FreeMesh(m), HandleKind::MeshMove) => {
            m.origin.x = round(m.origin.x + d.x, step);
            m.origin.y = round(m.origin.y + d.y, step);
        }
        (ElementKind::FreeMesh(m), HandleKind::MeshLift) => {
            m.origin.z = round(m.origin.z + d.z, step);
        }
        (ElementKind::CropBox(c), HandleKind::CropFace(i)) => {
            let axis = i / 2;
            let (at, _) = c.face(i);
            c.set_face(i, round(at[axis] + d[axis], step));
        }
        (ElementKind::Reference(r), HandleKind::RefMove) => {
            let dl = ref_local(model, r, now) - ref_local(model, r, grab);
            r.at = DVec2::new(round(r.at.x + dl.x, step), round(r.at.y + dl.y, step));
        }
        (ElementKind::Reference(r), HandleKind::RefScale(_)) => {
            let (g, n) = (
                ref_local(model, r, grab).distance(r.at),
                ref_local(model, r, now).distance(r.at),
            );
            if g < 1e-6 || n < 1e-3 {
                return Err("too close to its middle".into());
            }
            r.scale *= n / g;
        }
        (ElementKind::CropBox(c), HandleKind::CropMove) => {
            c.fit = None;
            // the grabbed corner lands on a rounded spot
            let to = DVec3::new(
                round(c.min.x + d.x, step),
                round(c.min.y + d.y, step),
                c.min.z,
            );
            let by = to - c.min;
            c.min += by;
            c.max += by;
        }
        (ElementKind::Opening(o), HandleKind::OpeningSlide) => {
            let Some(ElementKind::Wall(w)) = model.get(o.host).map(|h| &h.kind) else {
                return Err("its wall is gone".into());
            };
            let along = round(d.truncate().dot(w.direction()), step);
            o.offset = (o.offset + along).clamp(0.0, (w.length() - o.width).max(0.0));
            creabim_core::walls::clamp_opening(model, o);
        }
        (ElementKind::Beam(b), HandleKind::BeamWidth) => {
            let n = (b.end - b.start).normalize_or_zero().perp();
            b.width = round(b.width + 2.0 * d.truncate().dot(n), step / 5.0).max(0.05);
        }
        (ElementKind::Detail(dt), HandleKind::DetailWidth | HandleKind::DetailSpacing) => {
            let frame = creabim_core::DrawingFrame::of(model, dt.view).ok_or("its view is gone")?;
            let moved = frame.to_drawing(now) - frame.to_drawing(grab);
            if kind == HandleKind::DetailWidth {
                let by = round(2.0 * moved.dot(dt.across()), step / 5.0);
                dt.width = (dt.width + by).max(0.001);
            } else if let (DetailKind::Repeat { module, .. }, [a, b, ..]) =
                (&mut dt.kind, dt.points.as_slice())
            {
                let along = (*b - *a).normalize_or_zero();
                *module = round(*module + moved.dot(along), step / 5.0).max(0.01);
            }
        }
        (ElementKind::View(v), HandleKind::CalloutCorner(_) | HandleKind::CalloutEdge(_)) => {
            let c = v.callout.as_mut().ok_or("not a callout")?;
            let frame =
                creabim_core::DrawingFrame::of(model, c.parent).ok_or("its view is gone")?;
            let to = frame.to_drawing(now);
            let moved = to - frame.to_drawing(grab);
            let r = |x: f64| round(x, step);
            let (mut lo, mut hi) = (c.lo, c.hi);
            match kind {
                HandleKind::CalloutCorner(i) => {
                    // the opposite corner stays where it is
                    let k = box_corners(lo, hi);
                    let stay = k[(i + 2) % 4];
                    let p = k[i] + DVec2::new(r(moved.x), r(moved.y));
                    lo = stay.min(p);
                    hi = stay.max(p);
                }
                HandleKind::CalloutEdge(0) => lo.y += r(moved.y),
                HandleKind::CalloutEdge(1) => hi.x += r(moved.x),
                HandleKind::CalloutEdge(2) => hi.y += r(moved.y),
                _ => lo.x += r(moved.x),
            }
            if hi.x - lo.x < 0.05 || hi.y - lo.y < 0.05 {
                return Err("a callout needs some size".into());
            }
            // a ringed callout (on a section, in the Classic family) stays
            // a circle: its box stays square
            if callout_is_round(model, c.parent) {
                let side = match kind {
                    HandleKind::CalloutEdge(0 | 2) => hi.y - lo.y,
                    HandleKind::CalloutEdge(_) => hi.x - lo.x,
                    _ => (hi - lo).max_element(),
                };
                match kind {
                    HandleKind::CalloutCorner(i) => {
                        let stay = box_corners(c.lo, c.hi)[(i + 2) % 4];
                        let away = (lo + hi) / 2.0 - stay;
                        let far = stay + DVec2::new(side * away.x.signum(), side * away.y.signum());
                        lo = stay.min(far);
                        hi = stay.max(far);
                    }
                    HandleKind::CalloutEdge(0 | 2) => {
                        let mid = (lo.x + hi.x) / 2.0;
                        lo.x = mid - side / 2.0;
                        hi.x = mid + side / 2.0;
                    }
                    _ => {
                        let mid = (lo.y + hi.y) / 2.0;
                        lo.y = mid - side / 2.0;
                        hi.y = mid + side / 2.0;
                    }
                }
            }
            c.lo = lo;
            c.hi = hi;
        }
        (
            ElementKind::Site(st),
            HandleKind::PlantCanopy
            | HandleKind::PlantHeight
            | HandleKind::PlantCrownBase
            | HandleKind::PlantReach(_),
        ) => {
            let creabim_core::SiteForm::Plant {
                canopy,
                height,
                tree,
                ..
            } = &mut st.form
            else {
                return Err("only plants have that grip".into());
            };
            match (kind, tree.as_deref_mut()) {
                (HandleKind::PlantCanopy, _) => *canopy = round(*canopy + 2.0 * d.x, step).max(0.1),
                (HandleKind::PlantHeight, t) => {
                    let low = t.map_or(0.1, |t| t.crown_base + 0.3);
                    *height = round(*height + d.z, step).max(low);
                }
                (HandleKind::PlantCrownBase, Some(t)) => {
                    t.crown_base =
                        round(t.crown_base + d.z, step).clamp(0.0, (*height - 0.3).max(0.0));
                }
                (HandleKind::PlantReach(i), Some(t)) => {
                    // the side's reach, so its edge lands where dragged to
                    let dir = t.side(i as usize);
                    let was = t.radius_towards(*canopy, dir);
                    let now = round(was + d.truncate().dot(dir), step).max(0.05);
                    let i = i as usize % 8;
                    t.reach[i] = (t.reach[i] * now / was.max(1e-6)).clamp(0.1, 2.5);
                }
                _ => return Err("only a grown tree has that grip".into()),
            }
        }
        (ElementKind::Wall(w), HandleKind::WallBend) => {
            // through the point dragged to, its ends where they are; a
            // point on the straight line between them straightens it
            let through = now.truncate();
            w.bulge = crate::planview::bulge_through(w.start, through, w.end)
                .filter(|k| k.abs() < 20.0)
                .unwrap_or(0.0);
        }
        (ElementKind::Wall(w), HandleKind::WallSlide) => {
            let n = w.direction().perp();
            let by = n * round(d.truncate().dot(n), step);
            w.start += by;
            w.end += by;
        }
        (ElementKind::Column(c), HandleKind::ColumnSize { depth }) => {
            let x = DVec2::from_angle(c.rotation.to_radians());
            let axis = if depth { x.perp() } else { x };
            // both sides move: the column stays on its point
            let by = round(2.0 * d.truncate().dot(axis), step / 5.0);
            match &mut c.profile {
                creabim_core::members::Profile::Rectangle { width, depth: dp } => {
                    let v = if depth { dp } else { width };
                    *v = (*v + by).max(0.05);
                }
                creabim_core::members::Profile::Round { diameter } => {
                    *diameter = (*diameter + by).max(0.05);
                }
            }
        }
        (_, HandleKind::Rotate) => {
            let original = originals.first().ok_or("nothing to turn")?;
            let c = pivot_of(model, original).ok_or("this doesn't turn")?;
            let (a0, a1) = (grab.truncate() - c, now.truncate() - c);
            if a1.length() < 1e-6 {
                return Err("too close to its middle".into());
            }
            let (by, _) = grip_turn(a0.angle_to(a1).to_degrees(), step);
            match &mut first.kind {
                ElementKind::Object(_) => turn_object(model, first, by),
                ElementKind::Column(col) => col.rotation = (col.rotation + by).rem_euclid(360.0),
                ElementKind::Stair(s) => s.direction = (s.direction + by).rem_euclid(360.0),
                ElementKind::FreeMesh(m) => m.rotation = (m.rotation + by).rem_euclid(360.0),
                ElementKind::Reference(r) => r.rotation = (r.rotation + by).rem_euclid(360.0),
                _ => return Err("this doesn't turn".into()),
            }
        }
        (_, HandleKind::ScaleCorner(i)) => {
            // from the opposite corner, by how far along the diagonal the
            // corner has gone, in hundredths (MU142)
            let original = originals.first().ok_or("nothing to scale")?;
            let corners = object_corners(model, original).ok_or("this doesn't scale")?;
            let (c, opp) = (corners[i as usize % 4], corners[(i as usize + 2) % 4]);
            let d = c - opp;
            let f = ((now.truncate() - opp).dot(d) / d.length_squared()).max(0.05);
            let f = (f * 100.0).round() / 100.0;
            let z = creabim_core::objects::base_z(model, original).unwrap_or(grab.z);
            *first = creabim_core::transform::transformed(
                model,
                original,
                &creabim_core::Xform::Scale {
                    center: opp.extend(z),
                    factor: f,
                },
            )
            .ok_or("this doesn't scale")?;
        }
        (_, HandleKind::Member { i, .. }) => {
            creabim_core::members::drag_grip(model, first, i, grab, now, step)?
        }
        (
            _,
            HandleKind::Vertex { .. }
            | HandleKind::Edge { .. }
            | HandleKind::EdgeMove { .. }
            | HandleKind::AddVertex { .. }
            | HandleKind::Ring { .. }
            | HandleKind::ShaftBase
            | HandleKind::ShaftTop,
        ) => crate::editmode::reshape(model, first, kind, grab, now, step)?,
        _ => return Err("this handle doesn't fit this element".into()),
    }
    let opening_ok = match &first.kind {
        ElementKind::Opening(o) => match model.get(o.host).map(|h| &h.kind) {
            Some(ElementKind::Wall(w)) => o.sill + o.height <= w.height + 1e-9,
            _ => true,
        },
        _ => true,
    };
    if !opening_ok {
        return Err("taller than its wall".into());
    }
    if let Some(p) = moved_end {
        for &(i, oe) in joined {
            if let ElementKind::Wall(o) = &mut out[i].kind {
                if oe {
                    o.end = p;
                } else {
                    o.start = p;
                }
            }
        }
    }
    for e in &out {
        e.validate()?;
    }
    Ok(out)
}

/// How far a handle has moved, for the status bar.
pub fn readout(changed: &[Element]) -> String {
    match changed.first().map(|e| &e.kind) {
        Some(ElementKind::Wall(w)) => format!(
            "length {}, height {}, thickness {}",
            crate::units::len(w.length()),
            crate::units::len(w.height),
            crate::units::len(w.thickness)
        ),
        Some(ElementKind::Opening(o)) => format!(
            "width {}, height {}, sill {}",
            crate::units::len(o.width),
            crate::units::len(o.height),
            crate::units::len(o.sill)
        ),
        Some(ElementKind::View(v)) if v.callout.is_some() => v
            .callout
            .map(|c| {
                format!(
                    "its box {} × {}",
                    crate::units::len(c.hi.x - c.lo.x),
                    crate::units::len(c.hi.y - c.lo.y)
                )
            })
            .unwrap_or_default(),
        Some(ElementKind::View(v)) => match v.kind {
            ViewKind::Section {
                start, end, depth, ..
            } => format!(
                "line {} long, sees {} deep",
                crate::units::len(start.distance(end)),
                crate::units::len(depth)
            ),
            _ => String::new(),
        },
        Some(ElementKind::Slab(s)) => format!(
            "area {}, thickness {}, top {}, underside {} from its level",
            crate::units::area(s.area()),
            crate::units::len(s.thickness),
            crate::units::elevation(s.top_offset),
            s.top_offset - s.thickness
        ),
        Some(ElementKind::Column(c)) => match c.height {
            Some(h) => format!(
                "{}, height {}",
                c.profile.label_in(&crate::units::get()),
                crate::units::len(h)
            ),
            None => c.profile.label_in(&crate::units::get()),
        },
        Some(ElementKind::Beam(b)) => format!(
            "length {}, {} × {}",
            crate::units::len(b.length()),
            crate::units::len(b.width),
            crate::units::len(b.depth)
        ),
        Some(ElementKind::Roof(r)) => format!(
            "{} roof, pitch {:.1}°, {} with overhang",
            r.effective_shape().label(),
            r.pitch,
            crate::units::area(r.plan_area())
        ),
        Some(ElementKind::Stair(s)) => {
            format!(
                "width {}, treads {}",
                crate::units::len(s.width),
                crate::units::len(s.tread)
            )
        }
        Some(ElementKind::Railing(r)) => format!("height {}", crate::units::len(r.height)),
        Some(ElementKind::Shaft(s)) => format!(
            "{}, {} corners",
            crate::units::area(creabim_core::polygon::signed_area2(&s.boundary).abs() / 2.0),
            s.boundary.len()
        ),
        Some(ElementKind::CropBox(c)) => {
            let s = c.size();
            format!(
                "crop box {} × {} × {}",
                crate::units::len(s.x),
                crate::units::len(s.y),
                crate::units::len(s.z)
            )
        }
        _ => String::new(),
    }
}

pub type SnapFn<'a> = dyn Fn(Pos2, Option<DVec3>) -> Option<(DVec3, Snap)> + 'a;

/// How a pane turns pointer positions into the model, for one frame.
pub struct PaneInput<'a> {
    /// The pointer as a ray into the model.
    pub ray: &'a dyn Fn(Pos2) -> Option<Ray>,
    /// A model point on screen.
    pub project: &'a dyn Fn(DVec3) -> Option<Pos2>,
    /// Which way the pane looks, to hide handles seen end on.
    pub look: DVec3,
    /// The best snap near the pointer as a model point, with the line's
    /// start for perpendicular and axis snaps. The pane leaves the dragged
    /// elements out.
    pub snap: &'a SnapFn<'a>,
    /// The point of `id` under the pointer, if it is there: grabbing the
    /// selected element anywhere but a handle moves it.
    pub grab_body: &'a dyn Fn(Pos2, ElementId) -> Option<DVec3>,
    /// A corner, middle or centre of `id` itself near the pointer: a drag
    /// that starts there moves the element by that point, so it lands
    /// exactly on what it snaps to.
    pub grab_snap: &'a dyn Fn(Pos2, ElementId) -> Option<DVec3>,
    /// The plane a grabbed element slides on: level in plans and 3D, the
    /// section plane in sections.
    pub body_normal: DVec3,
    /// The view the pane shows: drafting has grips only in its own.
    pub view: Option<ElementId>,
    /// The face under the pointer and its normal, leaving out what is
    /// being edited: 3D only (plans and sections give none). A grip
    /// sliding along a line stops where it meets it.
    pub face: &'a dyn Fn(Pos2) -> Option<(DVec3, DVec3)>,
}

/// A handle or a whole element being dragged.
#[derive(Debug)]
pub struct Drag {
    /// `None` moves the whole element.
    kind: Option<HandleKind>,
    constraint: Constraint,
    edit: LiveEdit,
    joined: Vec<(usize, bool)>,
    grab: DVec3,
    /// The grip's own point, on its path: what a snap puts on the target.
    /// The press is often off it (on an arrow, or beside an edge), and
    /// moving by the snap from the press left the edge short of the target
    /// by that much (owner markup 2026-11-27: ceilings, slabs).
    anchor: DVec3,
    /// Where the press was on screen: nothing moves until the pointer
    /// leaves it.
    press: Pos2,
    /// Where the pointer last put the grip, in the model.
    last: DVec3,
    /// A length being typed (Tab, or a digit, starts it): the grip goes
    /// that far from where it was grabbed, the way the pointer went.
    typed: Option<String>,
    /// The pane (its interaction id) the drag started in. Every pane runs
    /// [`interact`] with the one shared drag; only this one may go on with
    /// it or end it. Without this, a second area showing a plan saw "my
    /// pane isn't dragged" and ended the drag after its first frame, so 2D
    /// drags only moved at the press and at the release.
    owner: egui::Id,
}

/// The corners and holes picked in Edit mode, on the element being
/// edited, and what a pane is doing with them: a box over the grips, the
/// right-click menu of a corner, an edge or a hole. Shared by every pane.
#[derive(Debug, Default)]
pub struct GripSel {
    /// The element the picks are on.
    pub id: Option<ElementId>,
    pub corners: Vec<(Ring, usize)>,
    /// The two corners picked were picked as an edge, by a click on it:
    /// Delete then opens the outline there and keeps the corners (MU48).
    pub by_edge: bool,
    pub holes: Vec<usize>,
    /// A mass's picked vertices in Edit mode (an edge picks its two ends,
    /// a face its corners), kept lit until another pick (MU39).
    pub mesh: Vec<u32>,
    /// The last click on a grip: which, when and where, so a double-click
    /// still counts when the pointer moved a little between the clicks.
    last_tap: Option<(HandleKind, f64, Pos2)>,
    /// A box being dragged over the grips: the pane and where it began.
    boxsel: Option<(egui::Id, Pos2)>,
    /// The right-click menu, open in one pane.
    menu: Option<GripMenu>,
    /// A length is being typed into a drag: keys are the drag's.
    pub typing: bool,
    /// What the app should do next with Edit mode.
    pub request: Option<GripRequest>,
}

/// Something only the app can do, asked for from a grip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GripRequest {
    /// Leave Edit mode.
    Finish,
    /// Open the selected element in Edit mode.
    Enter,
    /// Push edge `i` of outline `ring` out or in, following the pointer.
    OffsetEdge(Ring, usize),
    /// Offset the whole outline, in place.
    OffsetAll,
    /// A corner on edge `i` of `ring` at this point, then moved with the
    /// pointer.
    InsertPoint(Ring, usize, DVec3),
    /// A part of the handle was taken: Move, Rotate or Scale what is
    /// picked, as the part says. By a drag (true) it ends when the button
    /// comes up; by a click, at the next click. The last is Shift: a
    /// cube then scales every way at once.
    Handle(HandlePart, bool, bool),
}

#[derive(Clone, Copy, Debug)]
struct GripMenu {
    pane: egui::Id,
    at: Pos2,
    on: MenuOn,
}

/// What a right-click in Edit mode was on.
#[derive(Clone, Copy, Debug, PartialEq)]
enum MenuOn {
    Corner(Ring, usize),
    /// An edge, and the model point clicked on it.
    Edge(Ring, usize, DVec3),
    Hole(usize),
    /// A layer's start, end or edge, in Layers.
    Layer(HandleKind),
    /// A car park's bay or island, by its middle.
    Bay(DVec2),
    Blank,
}

/// Two clicks on the same grip this close in time (seconds) and on
/// screen (pixels) are a double-click.
const DOUBLE_TAP: f64 = 0.5;
const DOUBLE_TAP_PX: f32 = 8.0;

/// A part of the handle (MU85): what the mouse takes hold of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HandlePart {
    /// The disc in the middle: move freely on the workplane.
    Middle,
    /// An arrow: move along its axis.
    Arrow(creabim_core::Axis),
    /// A square between two arrows: move in the plane square to this axis.
    Plane(creabim_core::Axis),
    /// An arc between two arrows: turn about this axis.
    Arc(creabim_core::Axis),
    /// A cube on a stem: scale along its axis.
    Cube(creabim_core::Axis),
}

impl HandlePart {
    /// What a drag of it does, for the label under the pointer.
    pub fn says(self) -> String {
        use creabim_core::Axis;
        let pair = |n: Axis| -> String {
            Axis::ALL
                .iter()
                .filter(|a| **a != n)
                .map(|a| a.label())
                .collect()
        };
        match self {
            HandlePart::Middle => "Move freely".into(),
            HandlePart::Arrow(a) => format!("Move along {}", a.label()),
            HandlePart::Plane(n) => format!("Move in {}", pair(n)),
            HandlePart::Arc(a) => format!("Turn about {}", a.label()),
            HandlePart::Cube(a) => format!("Scale along {}", a.label()),
        }
    }
}

/// How an edit taken by the handle starts: the tool, the axis it keeps
/// to, the plane it keeps in (by the axis square to it), and whether
/// letting go of the button ends it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandleStart {
    pub mode: crate::modal::Mode,
    pub axis: Option<creabim_core::Axis>,
    pub plane: Option<creabim_core::Axis>,
    pub by_drag: bool,
    /// Where the button went down on the part: the edit measures from
    /// there, not from where the pointer had got to when it began (a drag
    /// is known for one only some pixels on, and the picked points
    /// trailed the pointer by that much, MU130).
    pub press: Option<Pos2>,
    /// The handle's own point (its middle, on what is picked): a snap puts
    /// this on the target, not the point pressed on an arrow (owner markup
    /// 2026-11-27: a slab's or a ceiling's edge stopped short of the line
    /// it snapped to by the arrow's distance from it).
    pub origin: Option<DVec3>,
}

impl HandleStart {
    /// The edit `part` starts. `every_way` is Shift on a cube.
    pub fn of(part: HandlePart, by_drag: bool, every_way: bool) -> Self {
        use crate::modal::Mode;
        use creabim_core::Axis;
        let (mode, axis, plane) = match part {
            HandlePart::Middle => (Mode::Move, None, None),
            HandlePart::Arrow(a) => (Mode::Move, Some(a), None),
            // level with the ground it is the free move, snaps and all
            HandlePart::Plane(Axis::Z) => (Mode::Move, None, None),
            HandlePart::Plane(n) => (Mode::Move, None, Some(n)),
            HandlePart::Arc(a) => (Mode::Rotate, Some(a), None),
            HandlePart::Cube(_) if every_way => (Mode::Scale, None, None),
            HandlePart::Cube(a) => (Mode::Scale, Some(a), None),
        };
        HandleStart {
            mode,
            axis,
            plane,
            by_drag,
            press: HANDLE_PRESS.with(|p| p.take()),
            origin: HANDLE_ORIGIN.with(|p| p.take()),
        }
    }
}

thread_local! {
    /// Where the handle was last pressed, for [`HandleStart::press`].
    static HANDLE_PRESS: std::cell::Cell<Option<Pos2>> = const { std::cell::Cell::new(None) };
    /// The pressed handle's own point, for [`HandleStart::origin`].
    static HANDLE_ORIGIN: std::cell::Cell<Option<DVec3>> = const { std::cell::Cell::new(None) };
}

thread_local! {
    /// The edit about to start was taken by the handle, this way.
    static HANDLE_START: std::cell::Cell<Option<HandleStart>> =
        const { std::cell::Cell::new(None) };
}

thread_local! {
    /// The faces an Inset about to start works on.
    static INSET_START: std::cell::RefCell<Option<Vec<usize>>> =
        const { std::cell::RefCell::new(None) };
}

thread_local! {
    /// What Edit mode should have picked next on a shape: the points of
    /// the faces a tool just made.
    static PICK_NEXT: std::cell::RefCell<Option<(ElementId, Vec<u32>)>> =
        const { std::cell::RefCell::new(None) };
}

/// After this edit, these points of shape `id` are the ones picked.
pub fn set_pick_next(id: ElementId, vs: Vec<u32>) {
    PICK_NEXT.with(|k| *k.borrow_mut() = Some((id, vs)));
}

pub fn take_pick_next() -> Option<(ElementId, Vec<u32>)> {
    PICK_NEXT.with(|k| k.borrow_mut().take())
}

/// The Inset about to start works on these faces of the selected shape.
pub fn set_inset_start(faces: Vec<usize>) {
    INSET_START.with(|k| *k.borrow_mut() = Some(faces));
}

pub fn take_inset_start() -> Option<Vec<usize>> {
    INSET_START.with(|k| k.borrow_mut().take())
}

/// The faces of shape `id` picked whole in Edit mode: every corner of
/// each is among the picked points.
pub fn whole_faces(model: &Model, id: ElementId) -> Vec<usize> {
    let (Some(ElementKind::FreeMesh(m)), Some(picked)) =
        (model.get(id).map(|e| &e.kind), mesh_pick(id))
    else {
        return vec![];
    };
    (0..m.mesh.faces.len())
        .filter(|f| m.mesh.faces[*f].iter().all(|v| picked.contains(v)))
        .collect()
}

thread_local! {
    /// The handle as last drawn, in the pane it was drawn in: a press on
    /// it is the handle's, and the view must not start a selection box
    /// there (MU86: its arrows stand out past the shape).
    static HANDLE_SHOWN: std::cell::RefCell<Vec<(HandlePart, Vec<Pos2>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Is `p` on a part of the handle as it was last drawn?
pub fn handle_covers(p: Pos2) -> bool {
    HANDLE_SHOWN.with(|h| handle_hit(&h.borrow(), p).is_some())
}

/// No handle is shown any more.
pub fn forget_handle() {
    HANDLE_SHOWN.with(|h| h.borrow_mut().clear());
}

/// The wires of shape `id`, as Edit mode draws them, for a view to draw
/// while a tool runs on it: the edges between faces in one plane have no
/// line in the solid, and went missing during a move (MU86).
pub fn draw_edit_wires(
    model: &Model,
    id: ElementId,
    painter: &Painter,
    project: &dyn Fn(DVec3) -> Option<Pos2>,
) {
    let Some(ElementKind::FreeMesh(m)) = model.get(id).map(|e| &e.kind) else {
        return;
    };
    let base = m.origin + DVec3::Z * elevation(model, m.level);
    let ink = Stroke::new(edit_display().0, model_line(painter.ctx()));
    for (a, b) in mesh_edges(&m.mesh) {
        let p = |i: u32| project(base + m.to_model(m.mesh.vertices[i as usize]));
        if let (Some(pa), Some(pb)) = (p(a), p(b)) {
            painter.line_segment([pa, pb], ink);
        }
    }
}

/// The edit about to start was asked for by the handle.
pub fn set_handle_start(start: HandleStart) {
    HANDLE_START.with(|k| k.set(Some(start)));
}

pub fn take_handle_start() -> Option<HandleStart> {
    HANDLE_START.with(|k| k.take())
}

/// The handle's parts on screen, at the middle `c` of what is picked,
/// the same size at any zoom: the points of each as [`draw_handle`] and
/// [`handle_hit`] read them. `full` adds the arcs and the cubes (an
/// outline's corners only move). A part seen end on or edge on is left
/// out: there is nothing of it to take hold of.
pub fn handle_parts(
    project: &dyn Fn(DVec3) -> Option<Pos2>,
    c: DVec3,
    full: bool,
) -> Vec<(HandlePart, Vec<Pos2>)> {
    use creabim_core::Axis;
    let Some(c2) = project(c) else {
        return vec![];
    };
    // how many pixels a metre along each axis is, at the middle
    let px = |a: Axis| project(c + a.unit()).map_or(0.0, |q| q.distance(c2));
    let most = Axis::ALL.iter().map(|a| px(*a)).fold(0.0f32, f32::max);
    if most < 1e-3 {
        return vec![];
    }
    let seen = |a: Axis| px(a) >= most * 0.2;
    // a point `d` pixels out along `a`
    let out = |a: Axis, d: f32| project(c + a.unit() * (d / px(a)) as f64);
    // a point `i` pixels along `a` and `j` along `b`
    let at = |a: Axis, i: f32, b: Axis, j: f32| {
        project(c + a.unit() * (i / px(a)) as f64 + b.unit() * (j / px(b)) as f64)
    };
    let pair = |n: Axis| match n {
        Axis::X => (Axis::Y, Axis::Z),
        Axis::Y => (Axis::Z, Axis::X),
        Axis::Z => (Axis::X, Axis::Y),
    };
    let mut parts = vec![];
    for n in Axis::ALL {
        // the plane square to `n`, if it is not seen edge on
        let (a, b) = pair(n);
        if !(seen(a) && seen(b)) {
            continue;
        }
        let (Some(ta), Some(tb)) = (out(a, 40.0), out(b, 40.0)) else {
            continue;
        };
        let (ua, ub) = ((ta - c2).normalized(), (tb - c2).normalized());
        if (ua.x * ub.y - ua.y * ub.x).abs() < 0.3 {
            continue;
        }
        if full {
            let arc: Option<Vec<Pos2>> = (0..=16)
                .map(|k| {
                    let t = (12.0 + 66.0 * k as f32 / 16.0).to_radians();
                    at(a, 58.0 * t.cos(), b, 58.0 * t.sin())
                })
                .collect();
            if let Some(arc) = arc {
                parts.push((HandlePart::Arc(n), arc));
            }
        }
        let square: Option<Vec<Pos2>> = [(18.0, 18.0), (33.0, 18.0), (33.0, 33.0), (18.0, 33.0)]
            .iter()
            .map(|(i, j)| at(a, *i, b, *j))
            .collect();
        if let Some(square) = square {
            parts.push((HandlePart::Plane(n), square));
        }
    }
    for a in Axis::ALL {
        if !seen(a) {
            continue;
        }
        if let (Some(from), Some(neck), Some(tip)) = (out(a, 12.0), out(a, 66.0), out(a, 84.0)) {
            parts.push((HandlePart::Arrow(a), vec![from, neck, tip]));
        }
        if full {
            if let Some(at) = out(a, 40.0) {
                parts.push((HandlePart::Cube(a), vec![at]));
            }
        }
    }
    parts.push((HandlePart::Middle, vec![c2]));
    parts
}

/// The part of the handle under `p`, if one is. The middle is asked
/// first, then the cubes, the squares, the arrows and the arcs: each is a
/// target some 18 px across (MU85: 7 px each side of a thin line was all
/// the mouse could take).
pub fn handle_hit(parts: &[(HandlePart, Vec<Pos2>)], p: Pos2) -> Option<HandlePart> {
    let to_segment = |a: Pos2, b: Pos2| {
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
        (a + ab * t).distance(p)
    };
    let inside = |q: &[Pos2]| {
        let mut sign = 0.0f32;
        for i in 0..q.len() {
            let (a, b) = (q[i], q[(i + 1) % q.len()]);
            let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
            if cross != 0.0 {
                if sign != 0.0 && cross.signum() != sign {
                    return false;
                }
                sign = cross.signum();
            }
        }
        true
    };
    let rank = |part: &HandlePart| match part {
        HandlePart::Middle => 0,
        HandlePart::Cube(_) => 1,
        HandlePart::Plane(_) => 2,
        HandlePart::Arrow(_) => 3,
        HandlePart::Arc(_) => 4,
    };
    parts
        .iter()
        .filter(|(part, pts)| match part {
            HandlePart::Middle => pts[0].distance(p) <= 10.0,
            HandlePart::Cube(_) => pts[0].distance(p) <= 11.0,
            HandlePart::Plane(_) => inside(pts),
            HandlePart::Arrow(_) => to_segment(pts[0], pts[2]) <= 9.0,
            HandlePart::Arc(_) => pts.windows(2).any(|w| to_segment(w[0], w[1]) <= 9.0),
        })
        .min_by_key(|(part, _)| rank(part))
        .map(|(part, _)| *part)
}

/// Draw the handle: `hot` is the part under the pointer, which grows.
fn draw_handle(painter: &Painter, parts: &[(HandlePart, Vec<Pos2>)], hot: Option<HandlePart>) {
    let light = Color32::from_white_alpha(190);
    for (part, pts) in parts {
        let lit = hot == Some(*part);
        let w = if lit { 4.5 } else { 3.0 };
        match part {
            HandlePart::Arc(a) => {
                let colour = crate::modal::axis_colour(*a);
                painter.add(egui::Shape::line(pts.clone(), Stroke::new(w + 2.5, light)));
                painter.add(egui::Shape::line(pts.clone(), Stroke::new(w, colour)));
            }
            HandlePart::Plane(n) => {
                let colour = crate::modal::axis_colour(*n);
                let fill = colour.gamma_multiply(if lit { 0.85 } else { 0.4 });
                painter.add(egui::Shape::convex_polygon(
                    pts.clone(),
                    fill,
                    Stroke::new(1.4, colour),
                ));
            }
            HandlePart::Arrow(a) => {
                let colour = crate::modal::axis_colour(*a);
                let (from, neck, tip) = (pts[0], pts[1], pts[2]);
                painter.line_segment([from, neck], Stroke::new(w + 2.5, light));
                painter.line_segment([from, neck], Stroke::new(w, colour));
                let dir = (tip - neck).normalized();
                let side = dir.rot90() * if lit { 8.5 } else { 7.0 };
                painter.add(egui::Shape::convex_polygon(
                    vec![tip, neck + side, neck - side],
                    colour,
                    Stroke::new(1.2, light),
                ));
            }
            HandlePart::Cube(a) => {
                let colour = crate::modal::axis_colour(*a);
                let r = Rect::from_center_size(pts[0], Vec2::splat(if lit { 16.0 } else { 13.0 }));
                painter.rect_filled(r.expand(1.6), 2.0, light);
                painter.rect_filled(r, 1.5, colour);
            }
            HandlePart::Middle => {
                let r = if lit { 8.5 } else { 7.0 };
                painter.circle_filled(pts[0], r, Color32::WHITE);
                painter.circle_stroke(pts[0], r, Stroke::new(1.6, Color32::from_gray(40)));
            }
        }
    }
}

thread_local! {
    /// The mass vertices picked in Edit mode, for Move, Rotate and Scale.
    static MESH_PICK: std::cell::RefCell<Option<(ElementId, Vec<u32>)>> =
        const { std::cell::RefCell::new(None) };
}

/// An outline, and its corners picked in Edit mode.
type CornerPick = (ElementId, Vec<(Ring, usize)>);

thread_local! {
    /// The outline corners picked in Edit mode, for Move.
    static CORNER_PICK: std::cell::RefCell<Option<CornerPick>> =
        const { std::cell::RefCell::new(None) };
}

/// The corners picked on outline `id` in Edit mode, if any.
pub fn corner_pick(id: ElementId) -> Option<Vec<(Ring, usize)>> {
    CORNER_PICK.with(|p| {
        p.borrow()
            .as_ref()
            .filter(|(i, _)| *i == id)
            .map(|(_, c)| c.clone())
    })
}

/// The vertices picked on mass `id` in Edit mode, if any.
pub fn mesh_pick(id: ElementId) -> Option<Vec<u32>> {
    MESH_PICK.with(|p| {
        p.borrow()
            .as_ref()
            .filter(|(i, v)| *i == id && !v.is_empty())
            .map(|(_, v)| v.clone())
    })
}

/// L in Edit mode: the picks on a mass grow to the whole pieces they are
/// in. Returns what to say, or none if nothing is picked on a mass.
pub fn pick_linked(doc: &Document, grips: &mut GripSel) -> Option<String> {
    let id = grips.id?;
    let ElementKind::FreeMesh(m) = &doc.model().get(id)?.kind else {
        return None;
    };
    if grips.mesh.is_empty() {
        return Some(
            "Pick a point, an edge or a face first: L then takes the whole piece it belongs to."
                .into(),
        );
    }
    let piece = m.mesh.linked(&grips.mesh);
    let pieces = m.mesh.pieces().len();
    grips.pick_mesh(&[], false);
    grips.pick_mesh(&piece, false);
    Some(format!(
        "{} points picked, of {pieces} {} in this shape: G or M moves them, R turns, S scales.",
        piece.len(),
        if pieces == 1 { "piece" } else { "pieces" }
    ))
}

impl Drag {
    /// The elements the drag changes.
    pub fn ids(&self) -> Vec<ElementId> {
        self.edit.originals().iter().map(|e| e.id).collect()
    }

    /// Has it changed anything yet?
    pub fn applied(&self) -> bool {
        self.edit.applied()
    }
}

impl GripSel {
    pub fn is_empty(&self) -> bool {
        self.corners.is_empty() && self.holes.is_empty() && self.mesh.is_empty()
    }

    /// Pick a mass's vertices `vs` (a vertex, an edge's ends, a face's
    /// corners): only them, or with `add` (Shift) in, or out if they were
    /// all in already.
    pub fn pick_mesh(&mut self, vs: &[u32], add: bool) {
        if !add {
            self.mesh = vs.to_vec();
            self.corners.clear();
            self.holes.clear();
        } else if vs.iter().all(|v| self.mesh.contains(v)) {
            self.mesh.retain(|v| !vs.contains(v));
        } else {
            for v in vs {
                if !self.mesh.contains(v) {
                    self.mesh.push(*v);
                }
            }
        }
        MESH_PICK.with(|p| *p.borrow_mut() = self.id.map(|id| (id, self.mesh.clone())));
    }

    /// Drop the picks, and any box or menu. The last click is kept, so a
    /// double-click still counts while this runs every frame.
    pub fn clear(&mut self) {
        self.by_edge = false;
        self.corners.clear();
        self.holes.clear();
        self.mesh.clear();
        MESH_PICK.with(|p| *p.borrow_mut() = None);
        self.boxsel = None;
        self.menu = None;
    }

    /// Start afresh on element `id` (or none).
    pub fn reset(&mut self, id: Option<ElementId>) {
        if self.id != id {
            self.clear();
            self.last_tap = None;
            self.id = id;
        }
    }

    pub fn picked(&self, ring: Ring, i: usize) -> bool {
        self.corners.contains(&(ring, i))
    }

    /// Pick corner `i` of `ring`: only it, or with `add` (Shift) in or out
    /// of the picks.
    pub fn pick(&mut self, ring: Ring, i: usize, add: bool) {
        self.by_edge = false;
        if add {
            if let Some(k) = self.corners.iter().position(|c| *c == (ring, i)) {
                self.corners.remove(k);
            } else {
                self.corners.push((ring, i));
            }
        } else {
            self.corners = vec![(ring, i)];
            self.holes.clear();
        }
    }

    /// Pick edge `i` of `ring`, an outline of `n` corners: its two ends.
    /// Only them, or with `add` (Shift) in, or out if both were in.
    pub fn pick_edge(&mut self, ring: Ring, i: usize, n: usize, add: bool) {
        let ends = [(ring, i % n.max(1)), (ring, (i + 1) % n.max(1))];
        self.by_edge = !add;
        if !add {
            self.corners = ends.to_vec();
            self.holes.clear();
        } else if ends.iter().all(|c| self.corners.contains(c)) {
            self.corners.retain(|c| !ends.contains(c));
        } else {
            for c in ends {
                if !self.corners.contains(&c) {
                    self.corners.push(c);
                }
            }
        }
    }

    /// Is edge `i` of `ring` picked: both its ends?
    pub fn edge_picked(&self, ring: Ring, i: usize, n: usize) -> bool {
        n >= 2 && self.picked(ring, i % n) && self.picked(ring, (i + 1) % n)
    }

    /// The one edge picked, if what is picked is just an edge: two
    /// corners next to each other on `ring`, an outline of `n` corners.
    pub fn one_edge(&self, n: impl Fn(Ring) -> usize) -> Option<(Ring, usize)> {
        let [(ra, a), (rb, b)] = self.corners.as_slice() else {
            return None;
        };
        let n = n(*ra);
        if ra != rb || n < 2 {
            return None;
        }
        if (a + 1) % n == *b {
            Some((*ra, *a))
        } else if (b + 1) % n == *a {
            Some((*ra, *b))
        } else {
            None
        }
    }

    /// Tell Move what is picked on the element being edited (`edit`), so
    /// M moves those corners and not the whole outline (MU46).
    pub fn publish(&self, edit: Option<ElementId>) {
        let picked = edit
            .filter(|id| self.id == Some(*id) && !self.corners.is_empty())
            .map(|id| (id, self.corners.clone()));
        CORNER_PICK.with(|p| *p.borrow_mut() = picked);
    }

    /// Pick hole `k`, as [`GripSel::pick`] does a corner.
    pub fn pick_hole(&mut self, k: usize, add: bool) {
        if add {
            if let Some(j) = self.holes.iter().position(|h| *h == k) {
                self.holes.remove(j);
            } else {
                self.holes.push(k);
            }
        } else {
            self.holes = vec![k];
            self.corners.clear();
        }
    }

    /// Is a box or a menu open?
    pub fn busy(&self) -> bool {
        self.boxsel.is_some() || self.menu.is_some()
    }

    /// A click on grip `kind` at `t` (seconds) and `at` (screen): is it
    /// the second of a double-click?
    fn tap(&mut self, kind: HandleKind, t: f64, at: Pos2) -> bool {
        let double = self.last_tap.is_some_and(|(k, t0, p)| {
            k == kind && t - t0 < DOUBLE_TAP && p.distance(at) < DOUBLE_TAP_PX
        });
        self.last_tap = if double { None } else { Some((kind, t, at)) };
        double
    }
}

/// Delete or Backspace in Edit mode: remove the picked holes, else the
/// picked corners, in one undo step. Never the element itself.
pub fn delete_picked(
    doc: &mut Document,
    id: ElementId,
    sel: &mut GripSel,
) -> Result<String, String> {
    if sel.id != Some(id) || sel.is_empty() {
        return Err(
            "Select a corner to remove it: click it (Shift+click or drag a box for more), or click inside a hole to remove the hole. Esc finishes editing."
                .into(),
        );
    }
    let edge = sel
        .by_edge
        .then(|| {
            let model = doc.model();
            let e = model.get(id)?;
            sel.one_edge(|ring| {
                crate::editmode::ring_of(model, e, ring).map_or(0, |r| r.corners.len())
            })
            .filter(|_| matches!(e.kind, ElementKind::Sketch(_)))
        })
        .flatten();
    let r = if let (Some((Ring::Outline, i)), true) = (edge, sel.holes.is_empty()) {
        crate::editmode::remove_edge(doc, id, i)
    } else if sel.holes.is_empty() {
        crate::editmode::remove_corners(doc, id, &sel.corners)
    } else {
        crate::editmode::remove_holes(doc, id, &sel.holes)
    };
    if r.is_ok() {
        sel.clear();
    }
    r
}

/// The model's own line in Edit mode: dark on a light view, light on a
/// dark one, so what is not picked reads at any theme and the orange
/// still means picked (MU46, MU74).
fn model_line(ctx: &egui::Context) -> Color32 {
    if ctx.global_style().visuals.dark_mode {
        Color32::from_gray(225)
    } else {
        Color32::from_gray(20)
    }
}

fn interaction_ink() -> Color32 {
    crate::theme::interaction()
}
/// The colour of a shape that would be broken.
const INVALID: Color32 = Color32::from_rgb(210, 40, 40);

/// A short note on a light plate beside the pointer: what a grip does,
/// a live length, a typed one.
fn note(painter: &Painter, at: Pos2, text: &str, color: Color32) {
    let font = egui::FontId::proportional(12.0);
    let galley = painter.layout_no_wrap(text.to_owned(), font, color);
    let mut pos = at + Vec2::new(16.0, 14.0);
    let clip = painter.clip_rect();
    if pos.x + galley.size().x + 8.0 > clip.right() {
        pos.x = at.x - galley.size().x - 16.0;
    }
    if pos.y + galley.size().y + 6.0 > clip.bottom() {
        pos.y = at.y - galley.size().y - 18.0;
    }
    let rect = Rect::from_min_size(pos, galley.size()).expand2(Vec2::new(5.0, 3.0));
    // the theme's raised surface, as the live pill (MU75: white with the
    // accent on it, unreadable and glaring on a dark theme)
    let pill = PillLook::of(&crate::theme::Palette::of(painter.ctx()));
    let p = crate::theme::Palette::of(painter.ctx());
    let ink = crate::theme::legible(
        color,
        pill.ground,
        if p.dark {
            Color32::WHITE
        } else {
            Color32::BLACK
        },
    );
    painter.rect(
        rect,
        3.0,
        pill.ground,
        Stroke::new(1.0, pill.line),
        egui::StrokeKind::Middle,
    );
    painter.galley_with_override_text_color(pos, galley, ink);
}

/// Under this many pixels across on screen, a wall shows its thickness
/// grip only when the pointer comes near its face.
const THIN_PX: f32 = 14.0;

/// How thick wall `id` is on screen, in pixels.
fn wall_px(model: &Model, id: ElementId, input: &PaneInput) -> Option<f32> {
    let ElementKind::Wall(w) = &model.get(id)?.kind else {
        return None;
    };
    let z = elevation(model, Some(w.level)) + w.base_offset;
    let mid = (w.start + w.end) / 2.0;
    let n = w.direction().perp() * w.thickness / 2.0;
    let a = (input.project)((mid + n).extend(z))?;
    let b = (input.project)((mid - n).extend(z))?;
    Some(a.distance(b))
}

/// How long a let-go grip's value shows, seconds: about one, the last
/// third fading.
const FLASH: f64 = 1.3;

/// The values a grip can change on an element, by name, in metres.
pub fn values(e: &Element) -> Vec<(&'static str, f64)> {
    match &e.kind {
        ElementKind::Wall(w) => vec![
            ("Length", w.length()),
            ("Height", w.height),
            ("Thickness", w.thickness),
        ],
        ElementKind::Opening(o) => vec![
            ("Width", o.width),
            ("Height", o.height),
            ("Sill", o.sill),
            ("Offset", o.offset),
        ],
        ElementKind::View(v) if v.callout.is_some() => v
            .callout
            .map(|c| vec![("Width", c.hi.x - c.lo.x), ("Height", c.hi.y - c.lo.y)])
            .unwrap_or_default(),
        ElementKind::View(v) => match v.kind {
            ViewKind::Section {
                start, end, depth, ..
            } => vec![("Length", start.distance(end)), ("Depth", depth)],
            _ => vec![],
        },
        ElementKind::Slab(s) => vec![("Thickness", s.thickness), ("Top", s.top_offset)],
        ElementKind::Column(c) => c.height.map(|h| vec![("Height", h)]).unwrap_or_default(),
        ElementKind::Beam(b) => vec![("Length", b.start.distance(b.end))],
        ElementKind::Detail(d) => {
            let mut out = vec![("Length", d.length()), ("Width", d.width)];
            if let DetailKind::Repeat { module, .. } = d.kind {
                out.push(("Unit", module));
            }
            out
        }
        ElementKind::Site(s) => match s.form {
            creabim_core::SiteForm::Plant { canopy, height, .. } => match s.tree_shape() {
                Some(t) => {
                    let mut v = vec![("Height", height), ("Clear trunk", t.crown_base)];
                    for (i, name) in REACH_NAMES.iter().enumerate() {
                        v.push((*name, t.radius_towards(canopy, t.side(i))));
                    }
                    v
                }
                None => vec![("Canopy", canopy), ("Height", height)],
            },
            _ => vec![],
        },
        _ => vec![],
    }
}

/// A length's number alone, in the project's units: `7200`, `3'-6"`.
fn bare(metres: f64) -> String {
    let s = crate::units::len(metres);
    let unit = format!(" {}", crate::units::symbol());
    s.strip_suffix(&unit).map(str::to_string).unwrap_or(s)
}

/// The one value a drag is changing, as the pill shows it: its name, then
/// "old → ", then the new value and the change, which the pill draws in
/// the accent. `None` when nothing named has changed.
pub fn changing(old: &Element, new: &Element) -> Option<(String, String)> {
    let (a, b) = (values(old), values(new));
    let (name, was, now) = a
        .iter()
        .zip(&b)
        .filter(|((n, _), (m, _))| n == m)
        .map(|((n, x), (_, y))| (*n, *x, *y))
        .filter(|(_, x, y)| (y - x).abs() > 1e-6)
        .max_by(|p, q| (p.2 - p.1).abs().total_cmp(&(q.2 - q.1).abs()))?;
    let d = now - was;
    let sign = if d >= 0.0 { "+" } else { "−" };
    Some((
        format!("{name} {} → ", bare(was)),
        format!("{} {sign}{}", bare(now), bare(d.abs())),
    ))
}

/// How far an element moved between `old` and `new`, by a point of its
/// own (a wall's start, a column's base, ...), so the pill says the
/// rounded distance the model took, not the pointer's.
fn moved_by(old: Option<&Element>, new: Option<&Element>) -> Option<f64> {
    let point = |e: &Element| match &e.kind {
        ElementKind::Wall(w) => Some(w.start),
        ElementKind::Column(c) => Some(c.position),
        ElementKind::Beam(b) => Some(b.start),
        _ => None,
    };
    Some(point(old?)?.distance(point(new?)?))
}

/// Are live dimensions (the pill while dragging) on? D switches them.
pub fn live_on(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::new("grip.live")))
        .unwrap_or(true)
}

/// D, outside a text field: live dimensions off or on. True when it
/// switched them.
pub fn toggle_live(ctx: &egui::Context) -> bool {
    if ctx.egui_wants_keyboard_input() {
        return false;
    }
    // consume_key ignores Shift, which took Shift+D (Duplicate) for D
    let pressed =
        ctx.input_mut(|i| !i.modifiers.shift && i.consume_key(egui::Modifiers::NONE, egui::Key::D));
    if pressed {
        let on = !live_on(ctx);
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("grip.live"), on));
    }
    pressed
}

/// The four states of a grip, the same on every element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GripState {
    Rest,
    /// Under the pointer: it grows a little and says what it is.
    Hover,
    /// Held: a white core and a soft halo.
    Held,
    /// Another grip is held: this one fades back.
    Faded,
}

/// The colours grips and their pills take from the theme.
struct GripLook {
    accent: Color32,
    /// A picked corner's outer ring.
    ink: Color32,
    paper: Color32,
    pill: PillLook,
}

/// A value pill's colours: a soft raised surface with a line round it,
/// the theme's text, and the changing number in the theme's accent, made
/// legible (4.5:1) on that surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PillLook {
    pub ground: Color32,
    pub line: Color32,
    pub text: Color32,
    pub number: Color32,
}

impl PillLook {
    pub fn of(p: &crate::theme::Palette) -> Self {
        Self {
            ground: p.raise,
            line: p.edge,
            text: crate::theme::legible(
                p.text,
                p.raise,
                if p.dark {
                    Color32::WHITE
                } else {
                    Color32::BLACK
                },
            ),
            number: crate::theme::legible(
                p.accent,
                p.raise,
                if p.dark {
                    Color32::WHITE
                } else {
                    Color32::BLACK
                },
            ),
        }
    }
}

impl GripLook {
    fn of(ctx: &egui::Context) -> Self {
        let v = ctx.global_style().visuals.clone();
        Self {
            accent: interaction_ink(),
            ink: v.strong_text_color(),
            paper: v.panel_fill,
            pill: PillLook::of(&crate::theme::Palette::of(ctx)),
        }
    }
}

/// A grip at `c`: a square, or a bar across `bar` (a screen direction;
/// `Some(None)` is a bar seen end on, drawn as a dot).
/// How grips are drawn: a personal setting (brief 02, 2.13).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GripStyle {
    /// Squares at corners and ends, short bars on edges.
    #[default]
    Quiet,
    /// Almost nothing at rest: dots at corners; an edge's bar shows under
    /// the pointer.
    Edges,
    /// The same small circle everywhere.
    OneShape,
}

impl GripStyle {
    pub const ALL: [GripStyle; 3] = [GripStyle::Quiet, GripStyle::Edges, GripStyle::OneShape];

    pub fn label(self) -> &'static str {
        match self {
            GripStyle::Quiet => "Quiet",
            GripStyle::Edges => "Edges are the handles",
            GripStyle::OneShape => "One grip shape",
        }
    }
}

static GRIP_STYLE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Draw grips this way from now on.
static EDGE_W: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3F80_0000);
static VERT_S: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x4080_0000);

/// Edit mode's wire width and vertex dot size, in points (MU39).
pub fn set_edit_display(edge: f32, vertex: f32) {
    EDGE_W.store(edge.to_bits(), std::sync::atomic::Ordering::Relaxed);
    VERT_S.store(vertex.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

fn edit_display() -> (f32, f32) {
    (
        f32::from_bits(EDGE_W.load(std::sync::atomic::Ordering::Relaxed)),
        f32::from_bits(VERT_S.load(std::sync::atomic::Ordering::Relaxed)),
    )
}

pub fn set_grip_style(s: GripStyle) {
    GRIP_STYLE.store(s as u8, std::sync::atomic::Ordering::Relaxed);
}

pub fn grip_style() -> GripStyle {
    match GRIP_STYLE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => GripStyle::Edges,
        2 => GripStyle::OneShape,
        _ => GripStyle::Quiet,
    }
}

/// The turn grip: a round dot the size of a square grip, white in a ring of
/// the grip colour (in a halo while held), on a thin leader from `front` (an
/// object's front edge on screen). Squares stretch, dots turn (owner, MU142:
/// the ring with an arrow in it read as a ring with a square).
fn draw_turn_grip(
    painter: &Painter,
    c: Pos2,
    front: Option<Pos2>,
    state: GripState,
    look: &GripLook,
) {
    let accent = if state == GripState::Faded {
        look.accent.gamma_multiply(0.4)
    } else {
        look.accent
    };
    let r = TURN_DOT
        + match state {
            GripState::Hover => 2.0,
            GripState::Held => 1.0,
            _ => 0.0,
        };
    if state == GripState::Held {
        painter.circle_filled(c, 12.0, look.accent.gamma_multiply(0.18));
    }
    if let Some(f) = front {
        if f.distance(c) > r + 2.0 {
            let tip = c + (f - c).normalized() * r;
            painter.line_segment([f, tip], Stroke::new(1.0, accent.gamma_multiply(0.7)));
        }
    }
    // white while held too, in its halo, as a held square is (MU142: the
    // dot filled solid while a square stays white)
    painter.circle(c, r, Color32::WHITE, Stroke::new(2.0, accent));
}

/// The turn dot's radius at rest: as wide as a square grip (8 px).
pub(crate) const TURN_DOT: f32 = 4.5;

fn draw_grip(
    painter: &Painter,
    c: Pos2,
    bar: Option<Option<Vec2>>,
    state: GripState,
    filled: bool,
    picked: bool,
    look: &GripLook,
) {
    let alpha = if state == GripState::Faded { 0.4 } else { 1.0 };
    let accent = look.accent.gamma_multiply(alpha);
    let grow = match state {
        GripState::Hover => 2.0,
        GripState::Held => 1.0,
        _ => 0.0,
    };
    if state == GripState::Held {
        painter.circle_filled(c, 12.0, look.accent.gamma_multiply(0.18));
    }
    let core = if state == GripState::Held {
        Color32::WHITE
    } else if filled || picked || bar.is_some() {
        accent
    } else {
        Color32::WHITE.gamma_multiply(alpha)
    };
    match grip_style() {
        GripStyle::Quiet => {}
        GripStyle::Edges => {
            match bar {
                // an edge shows only under the pointer, long and heavy
                Some(Some(dir)) => {
                    if matches!(state, GripState::Hover | GripState::Held) {
                        let len = 14.0;
                        painter
                            .line_segment([c - dir * len, c + dir * len], Stroke::new(4.0, accent));
                    }
                }
                _ => {
                    let r = if filled || picked { 3.5 } else { 2.5 } + grow;
                    painter.circle_filled(c, r, accent);
                }
            }
            return;
        }
        GripStyle::OneShape => {
            let r = 4.0 + grow;
            let fill = if filled || picked || state == GripState::Held {
                accent
            } else {
                Color32::WHITE.gamma_multiply(alpha)
            };
            painter.circle(c, r, fill, Stroke::new(1.5, accent));
            return;
        }
    }
    match bar {
        Some(Some(dir)) => {
            // a short rounded bar across the way it moves
            let len = 6.0 + grow;
            let a = c - dir * len;
            let b = c + dir * len;
            painter.line_segment([a, b], Stroke::new(5.0 + grow, accent));
            for p in [a, b] {
                painter.circle_filled(p, (5.0 + grow) / 2.0, accent);
            }
            if state == GripState::Held {
                painter.line_segment([a, b], Stroke::new(2.0, core));
            }
        }
        Some(None) => {
            painter.circle(c, 3.5 + grow, core, Stroke::new(1.5, accent));
        }
        None => {
            let side = 8.0 + 2.0 * grow;
            painter.rect(
                Rect::from_center_size(c, Vec2::splat(side)),
                1.0,
                core,
                Stroke::new(1.5, accent),
                egui::StrokeKind::Middle,
            );
            if picked {
                painter.rect_stroke(
                    Rect::from_center_size(c, Vec2::splat(side + 5.0)),
                    2.0,
                    Stroke::new(1.0, look.ink.gamma_multiply(0.7)),
                    egui::StrokeKind::Middle,
                );
            }
        }
    }
}

/// The "+" that pulls a new corner out of an edge in Edit mode.
fn draw_plus(painter: &Painter, c: Pos2, state: GripState, look: &GripLook) {
    let r = if state == GripState::Rest { 5.5 } else { 7.0 };
    let fill = if state == GripState::Held {
        Color32::WHITE
    } else {
        look.paper
    };
    painter.circle(c, r, fill, Stroke::new(1.5, look.accent));
    let s = Stroke::new(1.5, look.accent);
    painter.line_segment([c - Vec2::X * (r - 2.5), c + Vec2::X * (r - 2.5)], s);
    painter.line_segment([c - Vec2::Y * (r - 2.5), c + Vec2::Y * (r - 2.5)], s);
}

/// A pill with its top-left corner at `at`, in a pill's look: a hint by
/// the pointer.
pub fn plain_pill(painter: &Painter, at: Pos2, text: &str, p: &PillLook) {
    let galley = painter.layout_no_wrap(text.to_owned(), egui::FontId::proportional(11.5), p.text);
    let size = galley.size() + Vec2::new(14.0, 6.0);
    let mut rect = Rect::from_min_size(at, size);
    let clip = painter.clip_rect().shrink(4.0);
    rect = rect.translate(Vec2::new(
        (clip.right() - rect.right()).min(0.0),
        (clip.bottom() - rect.bottom()).min(0.0),
    ));
    painter.rect(
        rect,
        6.0,
        p.ground,
        Stroke::new(1.0, p.line),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.min + Vec2::new(7.0, 3.0), galley, p.text);
}

/// A small pill centred under `at`: `text` in the theme's text colour,
/// then `number` in the accent.
fn pill(painter: &Painter, at: Pos2, text: &str, look: &GripLook, alpha: f32) {
    pill_value(painter, at, text, "", look, alpha);
}

/// A value pill: "Length 6600 → " then "7200 +600" in the accent, on the
/// theme's raised surface with a 1 px line, 11.5 px, 6 px corners.
fn pill_value(painter: &Painter, at: Pos2, text: &str, number: &str, look: &GripLook, alpha: f32) {
    let p = look.pill;
    let font = egui::FontId::proportional(11.5);
    // the UI font has no arrow: "old → new" draws its arrow as a stroke
    let (text, arrow) = match text.strip_suffix(" → ") {
        Some(t) => (t, true),
        None => (text, false),
    };
    let gap = if arrow { 18.0 } else { 0.0 };
    let mut job = egui::text::LayoutJob::default();
    let fmt = |c: Color32| egui::TextFormat {
        font_id: font.clone(),
        color: c.gamma_multiply(alpha),
        ..Default::default()
    };
    job.append(text, 0.0, fmt(p.text));
    let first = painter.layout_job(job.clone()).size().x;
    job.append(number, gap, fmt(p.number));
    let galley = painter.layout_job(job);
    let size = galley.size() + Vec2::new(14.0, 6.0);
    let mut rect = Rect::from_center_size(at + Vec2::new(0.0, size.y / 2.0), size);
    let clip = painter.clip_rect().shrink(4.0);
    rect = rect.translate(Vec2::new(
        (clip.left() - rect.left()).max(0.0) + (clip.right() - rect.right()).min(0.0),
        (clip.top() - rect.top()).max(0.0) + (clip.bottom() - rect.bottom()).min(0.0),
    ));
    painter.rect(
        rect,
        6.0,
        p.ground.gamma_multiply(alpha),
        Stroke::new(1.0, p.line.gamma_multiply(alpha)),
        egui::StrokeKind::Inside,
    );
    let y = rect.center().y;
    painter.galley(rect.min + Vec2::new(7.0, 3.0), galley, p.text);
    if arrow {
        let x0 = rect.min.x + 7.0 + first + 4.0;
        let (a, b) = (Pos2::new(x0, y), Pos2::new(x0 + gap - 8.0, y));
        let s = Stroke::new(1.2, p.text.gamma_multiply(alpha));
        painter.line_segment([a, b], s);
        painter.line_segment([b, b + Vec2::new(-3.0, -3.0)], s);
        painter.line_segment([b, b + Vec2::new(-3.0, 3.0)], s);
    }
}

/// The two-way pointer for a grip that moves along screen direction `d`.
fn resize_cursor(d: Vec2) -> egui::CursorIcon {
    let a = d.y.atan2(d.x).to_degrees().rem_euclid(180.0);
    if !(22.5..157.5).contains(&a) {
        egui::CursorIcon::ResizeHorizontal
    } else if (67.5..112.5).contains(&a) {
        egui::CursorIcon::ResizeVertical
    } else if a < 90.0 {
        egui::CursorIcon::ResizeNwSe
    } else {
        egui::CursorIcon::ResizeNeSw
    }
}

/// The outlines of `e` on screen, each with its ring and corners.
fn screen_rings(
    model: &Model,
    e: &Element,
    input: &PaneInput,
) -> Vec<(crate::editmode::RingInfo, Vec<Pos2>)> {
    crate::editmode::rings(model, e)
        .into_iter()
        .filter_map(|r| {
            let pts: Option<Vec<Pos2>> = r.corners.iter().map(|p| (input.project)(*p)).collect();
            Some((r, pts?))
        })
        .collect()
}

fn in_screen_polygon(pts: &[Pos2], p: Pos2) -> bool {
    let poly: Vec<DVec2> = pts
        .iter()
        .map(|q| DVec2::new(q.x as f64, q.y as f64))
        .collect();
    pts.len() >= 3 && creabim_core::polygon::contains(&poly, DVec2::new(p.x as f64, p.y as f64))
}

/// How far `p` is from segment `a`–`b` on screen, and where along it (0 to 1).
fn to_segment(a: Pos2, b: Pos2, p: Pos2) -> (f32, f32) {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    ((a + ab * t).distance(p), t)
}

/// The hole of `e` whose inside `p` (on screen) is in.
fn hole_at(model: &Model, e: &Element, input: &PaneInput, p: Pos2) -> Option<usize> {
    screen_rings(model, e, input)
        .into_iter()
        .find_map(|(r, pts)| match r.ring {
            Ring::Hole(k) if in_screen_polygon(&pts, p) => Some(k),
            _ => None,
        })
}

/// The edge of `e`'s outlines under `p` (within a few pixels), and the
/// model point there.
fn edge_at(model: &Model, e: &Element, input: &PaneInput, p: Pos2) -> Option<(Ring, usize, DVec3)> {
    let mut best: Option<(f32, Ring, usize, DVec3)> = None;
    for (r, pts) in screen_rings(model, e, input) {
        let n = pts.len();
        let edges = if r.closed { n } else { n.saturating_sub(1) };
        for i in 0..edges {
            let (d, t) = to_segment(pts[i], pts[(i + 1) % n], p);
            if d < 6.0 && best.is_none_or(|b| d < b.0) {
                let at = r.corners[i].lerp(r.corners[(i + 1) % n], t as f64);
                best = Some((d, r.ring, i, at));
            }
        }
    }
    best.map(|(_, r, i, at)| (r, i, at))
}

/// In Edit mode, the outlines being edited, traced so a hole inside a
/// slab or a shaft inside walls reads clearly; picked holes filled. A
/// `broken` shape is drawn red over them.
fn draw_rings(
    model: &Model,
    e: &Element,
    painter: &Painter,
    input: &PaneInput,
    sel: &GripSel,
    (color, ink): (Color32, Color32),
    hover: Option<(Ring, usize)>,
) {
    for (r, pts) in screen_rings(model, e, input) {
        if pts.len() < 2 {
            continue;
        }
        let n = pts.len();
        let picked = matches!(r.ring, Ring::Hole(k) if sel.holes.contains(&k));
        if picked && n >= 3 {
            // a hole may be concave: a fan from its middle is only a tint
            crate::planview::ring_edges(
                painter,
                &pts,
                Stroke::new(4.0, color.gamma_multiply(0.35)),
            );
        }
        let edges = if r.closed { n } else { n - 1 };
        for i in 0..edges {
            let on = sel.edge_picked(r.ring, i, n);
            let (width, color) = match (on, hover == Some((r.ring, i))) {
                (true, _) => (3.4, color),
                (false, true) => (3.4, ink),
                _ if picked => (2.6, color),
                _ => (1.8, ink),
            };
            // a slab's bent edge is traced along its arc
            let arc = match (&e.kind, r.ring) {
                (ElementKind::Slab(s), Ring::Outline) if s.bulge(i) != 0.0 => {
                    let c = s.edge(i);
                    let z = r.corners[i].z;
                    (0..=16)
                        .map(|k| (input.project)(c.at(k as f64 / 16.0).extend(z)))
                        .collect::<Option<Vec<Pos2>>>()
                }
                // and a stair's bent straight along its arc (MU103: a
                // selected curved stair had a straight line from foot
                // to head across it)
                (ElementKind::Stair(st), Ring::Outline)
                    if i + 1 < st.line.len() && st.bend(i) != 0.0 =>
                {
                    let (a, b) = (st.to_plan(st.line[i]), st.to_plan(st.line[i + 1]));
                    let z = r.corners[i].z;
                    let mut pts = vec![a];
                    pts.extend(creabim_core::members::arc_between(a, b, st.bend(i)));
                    pts.push(b);
                    pts.into_iter()
                        .map(|p| (input.project)(p.extend(z)))
                        .collect::<Option<Vec<Pos2>>>()
                }
                _ => None,
            };
            match arc {
                Some(line) => {
                    painter.add(egui::Shape::line(line, Stroke::new(width, color)));
                }
                None => {
                    painter.line_segment([pts[i], pts[(i + 1) % n]], Stroke::new(width, color));
                }
            }
        }
    }
}

/// The live lengths while a grip of an outline is dragged: the edges
/// either side of a corner, how far an edge or a hole moved.
fn live_lengths(model: &Model, e: &Element, kind: HandleKind, grab: DVec3, now: DVec3) -> String {
    let len = crate::units::len;
    match kind {
        HandleKind::Vertex { ring, i } | HandleKind::AddVertex { ring, i } => {
            let i = if matches!(kind, HandleKind::AddVertex { .. }) {
                i + 1
            } else {
                i
            };
            let Some(r) = crate::editmode::ring_of(model, e, ring) else {
                return String::new();
            };
            let n = r.corners.len();
            if i >= n {
                return String::new();
            }
            let c = r.corners[i];
            let mut parts = vec![];
            if i > 0 || r.closed {
                parts.push(len(c.distance(r.corners[(i + n - 1) % n])));
            }
            if i + 1 < n || r.closed {
                parts.push(len(c.distance(r.corners[(i + 1) % n])));
            }
            parts.join(" · ")
        }
        HandleKind::Edge { .. } => format!("offset {}", len((now - grab).length())),
        HandleKind::Ring { .. } | HandleKind::EdgeMove { .. } => {
            format!("moved {}", len((now - grab).length()))
        }
        _ => len((now - grab).length()),
    }
}

/// The right-click menu of Edit mode, for one frame. Runs what is picked
/// and returns the status line; `None` while nothing was picked.
fn grip_menu(
    doc: &mut Document,
    id: ElementId,
    ui_ctx: &egui::Context,
    sel: &mut GripSel,
) -> Option<String> {
    let m = sel.menu?;
    let e = doc.model().get(id)?.clone();
    let model = doc.model();
    let reshapes = |ring| crate::editmode::ring_of(model, &e, ring).is_some_and(|r| r.reshapes);
    let slab_bulge = |ring: Ring, i: usize| match (&e.kind, ring) {
        (ElementKind::Slab(s), Ring::Outline) => Some(s.bulge(i)),
        (ElementKind::Stair(s), Ring::Outline) if s.line.len() >= 2 => Some(s.bend(i)),
        _ => None,
    };
    #[derive(Clone, Copy, PartialEq)]
    enum Act {
        RemoveCorners,
        AddCorner(Ring, usize, DVec3),
        OffsetEdge(Ring, usize),
        OffsetAll,
        Arc(usize, bool),
        RemoveHole(usize),
        MoveHole(usize),
        AllCorners,
        Deselect,
        Finish,
        ResetLayer(HandleKind),
        ResetLayers,
        DrawBy(u8),
        /// A car park's bay at this place made a kind; none lets the
        /// layout decide it again.
        Bay(DVec2, Option<creabim_core::carpark::BayKind>),
        BaysBack,
        /// The strip along an edge of a car park's outline; none takes
        /// it away.
        Strip(usize, Option<creabim_core::carpark::StripUse>),
    }
    let car_park = match &e.kind {
        ElementKind::Site(s) => s.as_car_park().cloned(),
        _ => None,
    };
    let edited_layers = match &e.kind {
        ElementKind::Wall(w) => !w.layer_ends.is_empty(),
        ElementKind::Slab(s) => !s.layer_edges.is_empty(),
        ElementKind::Roof(r) => !r.layer_edges.is_empty(),
        ElementKind::Stair(s) => !s.pieces.is_empty(),
        _ => false,
    };
    let all_text = if matches!(e.kind, ElementKind::Stair(_)) {
        "Reset all pieces"
    } else {
        "Reset all layers"
    };
    let in_layers = layers() && has_layers(model, id);
    let mut act = None;
    let area = egui::Area::new(egui::Id::new(("grip-menu", m.pane)))
        .order(crate::app::overlay_order())
        .fixed_pos(m.at)
        .show(ui_ctx, |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.set_min_width(170.0);
                let mut item = |ui: &mut egui::Ui, on: bool, text: &str, a: Act| {
                    if ui
                        .add_enabled(on, egui::Button::new(text).frame(false))
                        .clicked()
                    {
                        act = Some(a);
                    }
                };
                match m.on {
                    MenuOn::Corner(ring, _) => {
                        let n = sel.corners.len();
                        let text = if n > 1 {
                            format!("Remove {n} corners")
                        } else {
                            "Remove corner".into()
                        };
                        item(ui, reshapes(ring), &text, Act::RemoveCorners);
                        item(ui, true, "Select all corners", Act::AllCorners);
                    }
                    MenuOn::Edge(ring, i, at) => {
                        item(
                            ui,
                            reshapes(ring),
                            "Add corner here",
                            Act::AddCorner(ring, i, at),
                        );
                        item(ui, reshapes(ring), "Offset edge", Act::OffsetEdge(ring, i));
                        item(ui, reshapes(ring), "Offset all", Act::OffsetAll);
                        match slab_bulge(ring, i) {
                            Some(b) if b != 0.0 => item(ui, true, "Straighten", Act::Arc(i, false)),
                            Some(_) => item(ui, true, "Make arc", Act::Arc(i, true)),
                            None => {
                                ui.add_enabled(
                                    false,
                                    egui::Button::new("Make arc (slabs, stairs)").frame(false),
                                );
                            }
                        }
                        if let Ring::Hole(k) = ring {
                            ui.separator();
                            item(ui, true, "Remove hole", Act::RemoveHole(k));
                        }
                        // a car park's outline: a strip along this edge
                        if let (Ring::Outline, Some(c)) = (ring, &car_park) {
                            ui.separator();
                            let now = c.strips.iter().find(|s| s.edge == i).map(|s| s.what);
                            for u in creabim_core::carpark::StripUse::ALL {
                                item(
                                    ui,
                                    now != Some(u),
                                    &format!("Edge strip: {}", u.label()),
                                    Act::Strip(i, Some(u)),
                                );
                            }
                            item(ui, now.is_some(), "No edge strip", Act::Strip(i, None));
                        }
                    }
                    MenuOn::Hole(k) => {
                        item(ui, true, "Remove hole", Act::RemoveHole(k));
                        item(ui, true, "Move hole", Act::MoveHole(k));
                    }
                    MenuOn::Bay(at) => {
                        use creabim_core::carpark::BayKind as B;
                        for k in [B::Standard, B::Ev, B::Parent, B::Motorbike, B::Delivery] {
                            item(
                                ui,
                                true,
                                &format!("Make it: {}", k.label()),
                                Act::Bay(at, Some(k)),
                            );
                        }
                        ui.separator();
                        item(ui, true, "Leave this bay out", Act::Bay(at, Some(B::Out)));
                        item(ui, true, "Island here", Act::Bay(at, Some(B::Island)));
                        ui.separator();
                        let near = car_park.as_ref().is_some_and(|c| {
                            c.changes
                                .iter()
                                .any(|x| x.at.distance(at) <= c.bay_width.max(c.bay_length) / 2.0)
                        });
                        item(ui, near, "Let the layout decide it", Act::Bay(at, None));
                        ui.separator();
                        item(ui, true, "Finish editing   Esc", Act::Finish);
                    }
                    MenuOn::Layer(k) => {
                        let moved = edited_end(Some(&e), k);
                        let text = match k {
                            HandleKind::StairFlight { .. } => "Reset flight",
                            HandleKind::StairLanding { .. } => "Reset landing",
                            HandleKind::LayerEdge { .. } => "Reset layer edge",
                            HandleKind::LayerHeight { top: false, .. } => "Reset layer bottom",
                            HandleKind::LayerHeight { top: true, .. } => "Reset layer top",
                            _ => "Reset layer end",
                        };
                        item(ui, moved, text, Act::ResetLayer(k));
                        item(ui, edited_layers, all_text, Act::ResetLayers);
                        if let (HandleKind::LayerEnd { layer, .. }, ElementKind::Wall(w)) =
                            (k, &e.kind)
                        {
                            ui.separator();
                            let at = |b: usize| {
                                let l = creabim_core::LocationLine::Boundary(b.min(255) as u8);
                                (w.location != l, b.min(255) as u8)
                            };
                            let (on, b) = at(layer);
                            item(ui, on, "Draw by its exterior face", Act::DrawBy(b));
                            let (on, b) = at(layer + 1);
                            item(ui, on, "Draw by its interior face", Act::DrawBy(b));
                        }
                        ui.separator();
                        item(ui, true, "Finish editing   Esc", Act::Finish);
                    }
                    MenuOn::Blank if in_layers => {
                        item(ui, edited_layers, all_text, Act::ResetLayers);
                        ui.separator();
                        item(ui, true, "Finish editing   Esc", Act::Finish);
                    }
                    MenuOn::Blank => {
                        if let Some(c) = &car_park {
                            let out = c
                                .changes
                                .iter()
                                .filter(|x| x.kind == creabim_core::carpark::BayKind::Out)
                                .count();
                            item(ui, out > 0, "Put back the bays left out", Act::BaysBack);
                            ui.separator();
                        }
                        item(ui, true, "Select all corners", Act::AllCorners);
                        item(ui, !sel.is_empty(), "Deselect corners", Act::Deselect);
                        ui.separator();
                        item(ui, true, "Finish editing   Esc", Act::Finish);
                    }
                }
            });
        });
    let pressed_out = ui_ctx.input(|i| {
        (i.pointer.any_pressed() || i.key_pressed(egui::Key::Escape))
            && i.pointer
                .interact_pos()
                .is_none_or(|p| !area.response.rect.contains(p))
    });
    let Some(a) = act else {
        if pressed_out {
            sel.menu = None;
        }
        return None;
    };
    sel.menu = None;
    let r = match a {
        Act::RemoveCorners => delete_picked(doc, id, sel),
        Act::RemoveHole(k) => {
            let r = crate::editmode::remove_holes(doc, id, &[k]);
            if r.is_ok() {
                sel.clear();
            }
            r
        }
        Act::AddCorner(ring, i, at) => crate::editmode::add_corner(doc, id, ring, i, Some(at)),
        Act::OffsetEdge(ring, i) => {
            sel.request = Some(GripRequest::OffsetEdge(ring, i));
            return None;
        }
        Act::OffsetAll => {
            sel.request = Some(GripRequest::OffsetAll);
            return None;
        }
        Act::Arc(i, on) => crate::editmode::set_arc(doc, id, i, on),
        Act::Bay(at, kind) => doc
            .edit(id, "Change bay", |e| {
                if let ElementKind::Site(s) = &mut e.kind {
                    if let creabim_core::SiteForm::Area {
                        parking: Some(cp), ..
                    } = &mut s.form
                    {
                        cp.change(at, kind);
                    }
                }
            })
            .map(|_| match kind {
                Some(k) => format!("Bay: {}.", k.label()),
                None => "The layout decides that bay again.".into(),
            })
            .map_err(|e| e.to_string()),
        Act::Strip(i, what) => doc
            .edit(id, "Edge strip", |e| {
                if let ElementKind::Site(s) = &mut e.kind {
                    if let creabim_core::SiteForm::Area {
                        parking: Some(cp), ..
                    } = &mut s.form
                    {
                        use creabim_core::carpark::StripUse as U;
                        // its width kept when only its use changes
                        let was = cp.strip_width(i);
                        let width = |u: U| {
                            if was > 0.0 {
                                was
                            } else {
                                match u {
                                    U::Walkway => 2.0,
                                    U::Planting => 1.5,
                                    U::Clear => 0.5,
                                }
                            }
                        };
                        cp.set_strip(i, what.map(|u| (u, width(u))));
                    }
                }
            })
            .map(|_| match what {
                Some(u) => format!(
                    "Edge strip: {}. Drag its inner line to set its width.",
                    u.label()
                ),
                None => "Edge strip taken away.".into(),
            })
            .map_err(|e| e.to_string()),
        Act::BaysBack => doc
            .edit(id, "Put back bays", |e| {
                if let ElementKind::Site(s) = &mut e.kind {
                    if let creabim_core::SiteForm::Area {
                        parking: Some(cp), ..
                    } = &mut s.form
                    {
                        cp.changes
                            .retain(|x| x.kind != creabim_core::carpark::BayKind::Out);
                    }
                }
            })
            .map(|_| "The bays left out are back.".to_string())
            .map_err(|e| e.to_string()),
        Act::MoveHole(k) => {
            sel.pick_hole(k, false);
            Ok("Drag inside the hole to move it. Esc cancels the drag.".into())
        }
        Act::AllCorners => {
            sel.holes.clear();
            // a shape has points, not corners (MU47: "0 corners selected")
            if let ElementKind::FreeMesh(m) = &e.kind {
                let all: Vec<u32> = (0..m.mesh.vertices.len() as u32).collect();
                sel.pick_mesh(&all, false);
                Ok(format!(
                    "{} points picked: G or M moves them, R turns, S scales.",
                    sel.mesh.len()
                ))
            } else {
                sel.corners = crate::editmode::rings(doc.model(), &e)
                    .iter()
                    .flat_map(|r| (0..r.corners.len()).map(move |i| (r.ring, i)))
                    .collect();
                Ok(format!(
                    "{} corners selected: Delete removes them.",
                    sel.corners.len()
                ))
            }
        }
        Act::Deselect => {
            sel.clear();
            Ok("Nothing picked.".into())
        }
        Act::Finish => {
            sel.request = Some(GripRequest::Finish);
            Ok("Finished editing.".into())
        }
        Act::ResetLayer(k) => reset_layer_ends(doc, id, Some(k)),
        Act::ResetLayers => reset_layer_ends(doc, id, None),
        Act::DrawBy(k) => draw_by_boundary(doc, id, k),
    };
    Some(r.unwrap_or_else(|e| e))
}

/// What a selected section or elevation sees: its line, and the band
/// as deep as it looks, so it can be found and its depth dragged.
/// Whether `snapped` shows close to `free` on screen: in 3D a point that
/// lies under the pointer can be far away along the line being slid on.
/// Where handle `h` shows on screen: at its point, or nudged out along
/// its line.
/// A mass's edit grips that would sit on top of each other on screen
/// (a mass seen small, far off) are left out, so a few clear grips show
/// instead of a cluster (MU39): its corners first, then its faces, then
/// its edge middles; the move, lift and turn grips always stay.
fn declutter(shown: Vec<Handle>, input: &PaneInput) -> Vec<Handle> {
    let rank = |k: &HandleKind| match k {
        HandleKind::MeshVertex(_) => 1,
        HandleKind::Face(_) => 2,
        HandleKind::MeshEdge(..) => 3,
        _ => 0,
    };
    if !shown.iter().any(|h| rank(&h.kind) > 0) {
        return shown;
    }
    // the grips that always stay, then the rest in their order
    let mut order: Vec<usize> = (0..shown.len()).collect();
    order.sort_by_key(|i| rank(&shown[*i].kind));
    let mut placed: Vec<Pos2> = vec![];
    let mut keep = vec![false; shown.len()];
    for i in order {
        let h = &shown[i];
        let Some(c) = screen_at(h, input) else {
            keep[i] = true;
            continue;
        };
        // corners need room of their own; faces and edges a little more
        let room = match rank(&h.kind) {
            0 => 0.0,
            1 => 14.0,
            _ => 22.0,
        };
        if room > 0.0 && placed.iter().any(|p| p.distance(c) < room) {
            continue;
        }
        placed.push(c);
        keep[i] = true;
    }
    shown
        .into_iter()
        .zip(keep)
        .filter_map(|(h, k)| k.then_some(h))
        .collect()
}

fn screen_at(h: &Handle, input: &PaneInput) -> Option<Pos2> {
    let c = (input.project)(h.at)?;
    let Constraint::Line { dir, .. } = h.constraint else {
        return Some(c);
    };
    if h.nudge == 0.0 {
        return Some(c);
    }
    let tip = (input.project)(h.at + dir * 0.3)?;
    let v = (tip - c).normalized();
    Some(if v.is_finite() { c + v * h.nudge } else { c })
}

/// Where a grip sliding along the line through `point` (direction `dir`)
/// meets the face under the pointer, if that is within reach of where the
/// pointer puts it (`free`) and the face isn't edge-on to the line.
fn on_face(input: &PaneInput, pos: Pos2, point: DVec3, dir: DVec3, free: DVec3) -> Option<DVec3> {
    let (hit, n) = (input.face)(pos)?;
    let across = dir.dot(n);
    if across.abs() < 0.2 {
        return None;
    }
    let on = point + dir * ((hit - point).dot(n) / across);
    near_on_screen(input, on, free).then_some(on)
}

/// Where a grip sliding along the line through `point` (direction `dir`)
/// meets what the snap at `p` found. A point snap: the foot of `p` on the
/// line. An edge, a level line or a guide: the snap gives the point on it
/// nearest the pointer, so ask again from the line, and again: for a line
/// crossing the path at any angle this closes on the crossing in a few
/// rounds (exact at once for one square to it, as a slab's top under a
/// wall's base).
fn crossing(
    input: &PaneInput,
    fixed: Option<DVec3>,
    point: DVec3,
    dir: DVec3,
    p: DVec3,
    kind: SnapKind,
) -> DVec3 {
    let foot = |q: DVec3| point + dir * (q - point).dot(dir);
    let mut on = foot(p);
    if !matches!(
        kind,
        SnapKind::OnEdge
            | SnapKind::OnGridLine
            | SnapKind::Guide
            | SnapKind::OnTempGuide
            | SnapKind::UnderlayFace
    ) {
        return on;
    }
    for _ in 0..6 {
        let Some(at) = (input.project)(on) else {
            break;
        };
        let Some((q, again)) = (input.snap)(at, fixed) else {
            break;
        };
        if again.kind != kind {
            break;
        }
        let next = foot(q);
        let moved = (next - on).length();
        on = next;
        if moved < 1e-6 {
            break;
        }
    }
    on
}

fn near_on_screen(input: &PaneInput, snapped: DVec3, free: DVec3) -> bool {
    match ((input.project)(snapped), (input.project)(free)) {
        (Some(a), Some(b)) => a.distance(b) < 16.0,
        _ => false,
    }
}

fn draw_view_range(painter: &Painter, input: &PaneInput, a: DVec2, b: DVec2, depth: f64) {
    let look = (b - a).normalize_or_zero().perp();
    let corners = [a, b, b + look * depth, a + look * depth];
    let pts: Option<Vec<Pos2>> = corners
        .iter()
        .map(|p| (input.project)(p.extend(0.0)))
        .collect();
    let Some(pts) = pts else {
        return;
    };
    painter.add(egui::Shape::convex_polygon(
        pts.clone(),
        interaction_ink().gamma_multiply(0.08),
        Stroke::NONE,
    ));
    painter.line_segment([pts[0], pts[1]], Stroke::new(2.5, interaction_ink()));
    for (p, q) in [(pts[1], pts[2]), (pts[2], pts[3]), (pts[3], pts[0])] {
        painter.add(egui::Shape::dashed_line(
            &[p, q],
            Stroke::new(1.2, interaction_ink().gamma_multiply(0.8)),
            6.0,
            4.0,
        ));
    }
}

/// Draw the selected element's handles and run a drag on them (or on the
/// element itself). Returns `true` while a drag runs, so the pane leaves
/// the pointer alone, and puts a message for the status bar in `status`.
/// In Edit mode (`edit`) it also picks corners and holes into `grips`
/// (a click, Shift+click, a box), opens their right-click menu, and moves
/// a hole dragged from inside.
#[allow(clippy::too_many_arguments)]
pub fn interact(
    doc: &mut Document,
    painter: &Painter,
    response: &egui::Response,
    selection: Option<ElementId>,
    face: Option<usize>,
    edit: bool,
    drag: &mut Option<Drag>,
    grips: &mut GripSel,
    input: &PaneInput,
    status: &mut Option<String>,
) -> bool {
    let Some(id) = selection else {
        *drag = None;
        grips.reset(None);
        return false;
    };
    grips.reset(Some(id));
    *TAP_PANE.lock().unwrap_or_else(|e| e.into_inner()) = Some(response.id);
    if toggle_live(painter.ctx()) {
        *status = Some(if live_on(painter.ctx()) {
            "Live dimensions on: a drag shows the value it changes. D hides them.".into()
        } else {
            "Live dimensions off. D shows them again.".into()
        });
    }
    if !edit {
        grips.clear();
    }
    let ctx = painter.ctx().clone();
    let shift = ctx.input(|i| i.modifiers.shift);
    // drafting is on its own view's paper only
    let elsewhere = match doc.model().get(id).map(|e| &e.kind) {
        Some(ElementKind::Detail(d)) => Some(d.view) != input.view,
        Some(ElementKind::View(v)) => v.callout.is_some_and(|c| Some(c.parent) != input.view),
        // a reference has grips only where it is shown: in its views, or
        // in 3D when it shows there (MU38: a profile's trace had grips in
        // the plans)
        Some(ElementKind::Reference(r)) => match input.view {
            Some(v) => !r.shown_in.contains(&v),
            None => !r.shows_in_3d(),
        },
        _ => false,
    };
    let shown: Vec<Handle> = handles_in(doc.model(), id, face, edit)
        .into_iter()
        .filter(|_| !elsewhere)
        .map(|h| in_section(h, input.look))
        .filter(|h| h.constraint.usable(input.look))
        // grips sit on the element: a wall slides by its body
        .filter(|h| h.kind != HandleKind::WallSlide)
        // an object turns about the upright only, which a section or an
        // elevation sees end on: its turn grip turned it there by a dial
        // drawn upright on the view's plane (MU142, owner: "so in 3d it
        // shows this way?"), where Rotate itself refuses
        .filter(|h| {
            !(h.kind == HandleKind::Rotate
                && turns_upright_only(doc.model(), id)
                && seen_side_on(input))
        })
        .collect();
    // every face's grip, before the view is cleared of crowded ones: a
    // face is taken anywhere on it, however close its middle lies to
    // another's on screen (MU130: seen from above or from far off, the top's
    // and the bottom's middles meet, one was left out, and a click on that
    // face picked nothing)
    let face_grips: Vec<Handle> = shown
        .iter()
        .filter(|h| matches!(h.kind, HandleKind::Face(_)))
        .copied()
        .collect();
    let shown = declutter(shown, input);
    let screen = |h: &Handle| screen_at(h, input);
    // a mass's edge is taken anywhere along it, not only at its middle,
    // as Blender's (MU39: edges were hard to pick)
    let mesh_ends: Option<(DVec3, creabim_core::FreeMesh)> =
        match doc.model().get(id).map(|e| &e.kind) {
            Some(ElementKind::FreeMesh(m)) if edit => Some((
                m.origin + DVec3::Z * elevation(doc.model(), m.level),
                m.clone(),
            )),
            _ => None,
        };
    let along = |h: &Handle, p: Pos2| -> Option<f32> {
        let HandleKind::MeshEdge(a, b) = h.kind else {
            return None;
        };
        let (base, m) = mesh_ends.as_ref()?;
        let s = |i: u32| (input.project)(*base + m.to_model(m.mesh.vertices[i as usize]));
        let (pa, pb) = (s(a)?, s(b)?);
        let ab = pb - pa;
        let t = ((p - pa).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.08, 0.92);
        Some((pa + ab * t).distance(p))
    };
    // a face of a shape in Edit mode is taken anywhere on it, the one in
    // front: its dot at rest is gone (MU47, decision 2)
    let face_of_mesh = |p: Pos2| -> Option<Handle> {
        let (base, m) = mesh_ends.as_ref()?;
        let ray = (input.ray)(p)?;
        let mut best: Option<(f64, usize)> = None;
        for (f, ring) in m.mesh.faces.iter().enumerate() {
            let pts: Vec<DVec3> = ring
                .iter()
                .map(|v| *base + m.to_model(m.mesh.vertices[*v as usize]))
                .collect();
            if pts.len() < 3 {
                continue;
            }
            let mut n = DVec3::ZERO;
            for i in 0..pts.len() {
                n += pts[i].cross(pts[(i + 1) % pts.len()]);
            }
            let den = ray.d.dot(n);
            if n.length() < 1e-12 || den.abs() < 1e-12 {
                continue;
            }
            let t = (pts[0] - ray.o).dot(n) / den;
            let hit = ray.o + ray.d * t;
            // flattened along the axis the face looks most along
            let drop = |q: DVec3| {
                let a = n.abs();
                if a.x >= a.y && a.x >= a.z {
                    DVec2::new(q.y, q.z)
                } else if a.y >= a.z {
                    DVec2::new(q.z, q.x)
                } else {
                    DVec2::new(q.x, q.y)
                }
            };
            let flat: Vec<DVec2> = pts.iter().map(|q| drop(*q)).collect();
            if creabim_core::polygon::contains(&flat, drop(hit))
                && best.is_none_or(|(bt, _)| t < bt)
            {
                best = Some((t, f));
            }
        }
        let f = best?.1;
        face_grips
            .iter()
            .find(|h| h.kind == HandleKind::Face(f))
            .copied()
    };
    let nearest = |p: Pos2| {
        shown
            .iter()
            .filter_map(|h| Some((h, along(h, p).or_else(|| Some(screen(h)?.distance(p)))?)))
            // 8 px to see, 16 px to hit
            .filter(|(_, d)| *d < 8.0)
            // a corner wins over an add-corner grip or an edge's at the
            // same spot
            .min_by(|a, b| {
                let rank = |h: &Handle| match h.kind {
                    HandleKind::Vertex { .. } => 0.0,
                    _ => 1.5,
                };
                (a.1 + rank(a.0)).total_cmp(&(b.1 + rank(b.0)))
            })
            .map(|(h, _)| *h)
            .or_else(|| face_of_mesh(p))
    };
    let element = doc.model().get(id).cloned();
    // an edge of an outline being edited is taken anywhere along it, to
    // pick or to drag, when corners are not all that is picked (MU46)
    let edge_under = |p: Pos2| -> Option<(Ring, usize, DVec3)> {
        let e = element
            .as_ref()
            .filter(|_| edit && matches!(pick(), 0 | 2))?;
        if nearest(p).is_some() {
            return None;
        }
        let (ring, i, at) = edge_at(doc.model(), e, input, p)?;
        crate::editmode::ring_of(doc.model(), e, ring)
            .is_some_and(|r| r.reshapes)
            .then_some((ring, i, at))
    };
    // with Faces on, the inside of a closed outline is its face
    let face_under = |p: Pos2| -> Option<DVec3> {
        let e = element.as_ref().filter(|_| edit && pick() == 3)?;
        if hole_at(doc.model(), e, input, p).is_some() {
            return None;
        }
        let r = crate::editmode::ring_of(doc.model(), e, Ring::Outline)?;
        let pts: Option<Vec<Pos2>> = r.corners.iter().map(|c| (input.project)(*c)).collect();
        if !(r.closed && r.reshapes && in_screen_polygon(&pts?, p)) {
            return None;
        }
        (input.ray)(p).and_then(|ray| {
            Constraint::Plane {
                point: r.corners[0],
                normal: r.normal,
            }
            .hit(ray)
        })
    };
    let edge_hot = response
        .hover_pos()
        .filter(|_| drag.is_none() && !grips.busy())
        .and_then(edge_under);
    // selected, not in Edit mode: its outline traced bold in the selection's
    // colour, as the plans and the section marks show it (MU74: a
    // one-pixel line in 3D)
    // kept while a grip is dragged: the model follows the drag (MU75:
    // thin until the release)
    if let (false, Some(e)) = (edit, &element) {
        let a = crate::planview::accent_selected();
        draw_rings(
            doc.model(),
            e,
            painter,
            input,
            &GripSel::default(),
            (a, a),
            None,
        );
    }
    if let (true, Some(e)) = (edit, &element) {
        draw_rings(
            doc.model(),
            e,
            painter,
            input,
            grips,
            // what is not picked is drawn in the model's own dark line,
            // so the orange means picked and nothing else (MU46: the
            // edge under the pointer and the others looked alike)
            // the model's line in the view's own contrast (MU74: a fixed
            // near-black, from MU46, vanished on dark themes)
            (crate::theme::interaction(), model_line(&ctx)),
            edge_hot.map(|(r, i, _)| (r, i)),
        );
    }
    if let Some(ElementKind::View(v)) = element.as_ref().map(|e| &e.kind) {
        if let ViewKind::Section {
            start, end, depth, ..
        } = v.kind
        {
            draw_view_range(painter, input, start, end, depth);
        }
    }
    let pointer = response.hover_pos();
    // the handle, at the middle of what is picked (MU47, MU85): every
    // part at once; an outline's corners only move
    let handle_at: Option<(DVec3, bool)> = (edit && drag.is_none() && !grips.busy())
        .then(|| match (mesh_ends.as_ref(), element.as_ref()) {
            (Some((base, m)), _) if !grips.mesh.is_empty() => {
                let sum: DVec3 = grips
                    .mesh
                    .iter()
                    .filter_map(|v| m.mesh.vertices.get(*v as usize))
                    .map(|v| *base + m.to_model(*v))
                    .sum();
                // one point has nothing to turn or to scale: it moves only
                Some((sum / grips.mesh.len() as f64, grips.mesh.len() > 1))
            }
            (None, Some(e)) if !grips.corners.is_empty() => {
                let pts: Vec<DVec3> = grips
                    .corners
                    .iter()
                    .filter_map(|(ring, i)| {
                        crate::editmode::ring_of(doc.model(), e, *ring)?
                            .corners
                            .get(*i)
                            .copied()
                    })
                    .collect();
                (!pts.is_empty()).then(|| (pts.iter().sum::<DVec3>() / pts.len() as f64, false))
            }
            _ => None,
        })
        .flatten();
    let handle = handle_at.map(|(c, full)| handle_parts(input.project, c, full));
    if input.view.is_none() {
        HANDLE_SHOWN.with(|h| *h.borrow_mut() = handle.clone().unwrap_or_default());
    }
    let handle_hot = match (&handle, pointer) {
        (Some(parts), Some(p)) => handle_hit(parts, p),
        _ => None,
    };
    // the protractor's ring, once resting on the turn dot has lit it: on
    // the ring the pointer has the turn grip as on the dot (MU142,
    // decision 10)
    let turn_dot = shown.iter().find(|h| h.kind == HandleKind::Rotate).copied();
    let ring = element
        .as_ref()
        .filter(|e| ring_lit(response.id) == Some(e.id) && turn_dot.is_some())
        .and_then(|e| turn_ring(doc.model(), e, input));
    let on_ring = |p: Pos2| ring.is_some_and(|(c, r)| (p.distance(c) - r).abs() < 9.0);
    let hot = pointer
        .filter(|_| drag.is_none() && !grips.busy() && handle_hot.is_none())
        .and_then(|p| nearest(p).or_else(|| turn_dot.filter(|_| on_ring(p))))
        .map(|h| h.kind);
    // lit by the dot, kept while the pointer is within the ring
    if hot == Some(HandleKind::Rotate) {
        set_ring_lit(Some((id, response.id)));
    } else if drag.is_none() && ring_lit(response.id) == Some(id) {
        let inside = match (ring, pointer) {
            (Some((c, r)), Some(p)) => p.distance(c) < r + 12.0,
            _ => false,
        };
        if !inside {
            set_ring_lit(None);
        }
    }
    if let (true, Some(e)) = (
        drag.is_none() && ring_lit(response.id) == Some(id),
        &element,
    ) {
        draw_turn_ring(painter, input, doc.model(), e);
    }
    let held = drag.as_ref().and_then(|d| d.kind);
    // in Edit mode the "+" shows only on the edge under the pointer
    let hovered_edge = match (edit, pointer, &element) {
        (true, Some(p), Some(e)) if drag.is_none() => {
            edge_at(doc.model(), e, input, p).map(|(r, i, _)| (r, i))
        }
        _ => None,
    };
    let look = GripLook::of(&ctx);
    // a mass in Edit mode, as Blender's: its wires thin and dark, its
    // corners small dots; edges and faces light up only under the pointer
    // (MU39)
    let mesh_edit = edit
        && matches!(
            element.as_ref().map(|e| &e.kind),
            Some(ElementKind::FreeMesh(_))
        );
    let (wire_w, dot) = edit_display();
    if mesh_edit {
        if let Some(ElementKind::FreeMesh(m)) = element.as_ref().map(|e| &e.kind) {
            let base = m.origin + DVec3::Z * elevation(doc.model(), m.level);
            let ink = model_line(&ctx);
            for (a, b) in mesh_edges(&m.mesh) {
                let p = |i: u32| (input.project)(base + m.to_model(m.mesh.vertices[i as usize]));
                if let (Some(pa), Some(pb)) = (p(a), p(b)) {
                    painter.line_segment([pa, pb], Stroke::new(wire_w, ink));
                }
            }
        }
    }
    // what is picked on the mass: its faces tinted, its edges and points
    // in the accent, lit until another pick (MU39)
    if mesh_edit && !grips.mesh.is_empty() {
        if let Some(ElementKind::FreeMesh(m)) = element.as_ref().map(|e| &e.kind) {
            let base = m.origin + DVec3::Z * elevation(doc.model(), m.level);
            let accent = crate::theme::interaction();
            let p = |i: u32| (input.project)(base + m.to_model(m.mesh.vertices[i as usize]));
            let on = |v: &u32| grips.mesh.contains(v);
            for f in &m.mesh.faces {
                if f.iter().all(on) {
                    let pts: Option<Vec<Pos2>> = f.iter().map(|v| p(*v)).collect();
                    if let Some(pts) = pts {
                        painter.add(egui::Shape::Path(egui::epaint::PathShape {
                            points: pts,
                            closed: true,
                            fill: accent.gamma_multiply(0.28),
                            stroke: Stroke::NONE.into(),
                        }));
                    }
                }
            }
            for (a, b) in mesh_edges(&m.mesh) {
                if on(&a) && on(&b) {
                    if let (Some(pa), Some(pb)) = (p(a), p(b)) {
                        painter.line_segment([pa, pb], Stroke::new(edit_display().0 + 1.5, accent));
                    }
                }
            }
            for v in &grips.mesh {
                if let Some(c) = p(*v) {
                    painter.circle_filled(c, edit_display().1 / 2.0 + 1.0, accent);
                }
            }
        }
    }
    for h in &shown {
        let Some(c) = screen(h) else {
            continue;
        };
        if mesh_edit {
            let state = if hot == Some(h.kind) || held == Some(h.kind) {
                GripState::Hover
            } else {
                GripState::Rest
            };
            let accent = crate::theme::interaction();
            match h.kind {
                HandleKind::MeshVertex(_) => {
                    // at rest a point is the corner itself; with Points on
                    // a small dark square marks it. Under the pointer it
                    // lights (MU47: the dots were "a visual eye sore")
                    let lit = state == GripState::Hover;
                    if lit {
                        let r = Rect::from_center_size(c, Vec2::splat(dot + 3.0));
                        painter.rect_filled(r, 1.5, accent);
                        painter.rect_stroke(
                            r,
                            1.5,
                            Stroke::new(1.0, Color32::WHITE),
                            egui::StrokeKind::Outside,
                        );
                        ctx.set_cursor_icon(egui::CursorIcon::Move);
                    } else if pick() == 1 {
                        let r = Rect::from_center_size(c, Vec2::splat((dot - 1.0).max(3.0)));
                        painter.rect_filled(r, 0.5, Color32::from_gray(25));
                    }
                    continue;
                }
                HandleKind::MeshEdge(a, b) if state == GripState::Hover => {
                    if let Some(ElementKind::FreeMesh(m)) = element.as_ref().map(|e| &e.kind) {
                        let base = m.origin + DVec3::Z * elevation(doc.model(), m.level);
                        let p = |i: u32| {
                            (input.project)(base + m.to_model(m.mesh.vertices[i as usize]))
                        };
                        if let (Some(pa), Some(pb)) = (p(a), p(b)) {
                            painter.line_segment([pa, pb], Stroke::new(wire_w + 1.5, accent));
                        }
                    }
                    ctx.set_cursor_icon(egui::CursorIcon::Move);
                    continue;
                }
                // Edges picked (2): every edge shows, to grab (MU39)
                HandleKind::MeshEdge(a, b) if pick() == 2 => {
                    if let Some(ElementKind::FreeMesh(m)) = element.as_ref().map(|e| &e.kind) {
                        let base = m.origin + DVec3::Z * elevation(doc.model(), m.level);
                        let p = |i: u32| {
                            (input.project)(base + m.to_model(m.mesh.vertices[i as usize]))
                        };
                        if let (Some(pa), Some(pb)) = (p(a), p(b)) {
                            painter.line_segment(
                                [pa, pb],
                                Stroke::new(wire_w + 1.0, accent.gamma_multiply(0.55)),
                            );
                        }
                    }
                    continue;
                }
                HandleKind::MeshEdge(..) => continue,
                // a face has no dot: under the pointer the face itself is
                // tinted and outlined, and a click or a drag takes it
                HandleKind::Face(f) if state == GripState::Hover && held.is_none() => {
                    if let Some(ElementKind::FreeMesh(m)) = element.as_ref().map(|e| &e.kind) {
                        let base = m.origin + DVec3::Z * elevation(doc.model(), m.level);
                        let pts: Option<Vec<Pos2>> = m.mesh.faces.get(f).and_then(|ring| {
                            ring.iter()
                                .map(|v| {
                                    (input.project)(base + m.to_model(m.mesh.vertices[*v as usize]))
                                })
                                .collect()
                        });
                        if let Some(pts) = pts {
                            painter.add(egui::Shape::Path(egui::epaint::PathShape {
                                points: pts,
                                closed: true,
                                fill: accent.gamma_multiply(0.16),
                                stroke: Stroke::new(wire_w + 1.0, accent).into(),
                            }));
                        }
                    }
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                    continue;
                }
                HandleKind::Face(_) if state == GripState::Rest => continue,
                _ => {}
            }
        }
        let state = if held == Some(h.kind) {
            GripState::Held
        } else if held.is_some() {
            GripState::Faded
        } else if hot == Some(h.kind) {
            GripState::Hover
        } else {
            GripState::Rest
        };
        if let HandleKind::AddVertex { ring, i } = h.kind {
            if state == GripState::Rest && hovered_edge != Some((ring, i)) {
                continue;
            }
            draw_plus(painter, c, state, &look);
            continue;
        }
        let picked = matches!(h.kind, HandleKind::Vertex { ring, i } if grips.picked(ring, i));
        // a square is a point you move, a bar an edge or a face you push
        let bar = h.kind.round().then(|| match h.constraint {
            Constraint::Line { dir, .. } => (input.project)(h.at + dir * 0.3)
                .map(|tip| (tip - c).normalized())
                .filter(|v| v.is_finite())
                .map(|v| v.rot90()),
            Constraint::Plane { .. } => None,
        });
        // a wall too thin on screen for its face grip shows it only when
        // the pointer comes near the face
        if h.kind == HandleKind::WallFace
            && state == GripState::Rest
            && wall_px(doc.model(), id, input).is_some_and(|px| px < THIN_PX)
            && pointer.is_none_or(|p| p.distance(c) > 14.0)
        {
            continue;
        }
        // a corner in Edit mode, or a layer end moved by hand: a dot
        let filled = edit
            && (matches!(h.kind, HandleKind::Vertex { .. })
                || edited_end(element.as_ref(), h.kind));
        if let HandleKind::ScaleCorner(i) = h.kind {
            // shown only under the pointer, or while held: at rest the
            // selection looks as it does (MU142, Grip Palette REV01)
            if matches!(state, GripState::Rest | GripState::Faded) {
                continue;
            }
            let z = element
                .as_ref()
                .and_then(|e| creabim_core::objects::base_z(doc.model(), e))
                .unwrap_or(h.at.z);
            let scr: Option<Vec<Pos2>> = element
                .as_ref()
                .and_then(|e| object_corners(doc.model(), e))
                .and_then(|cs| cs.iter().map(|q| (input.project)(q.extend(z))).collect());
            if let Some(scr) = scr {
                let k = i as usize % 4;
                draw_bracket(
                    painter,
                    scr[k],
                    scr[(k + 3) % 4],
                    scr[(k + 1) % 4],
                    look.accent,
                );
                if state == GripState::Hover {
                    ctx.set_cursor_icon(resize_cursor(scr[k] - scr[(k + 2) % 4]));
                }
            }
            continue;
        }
        if h.kind == HandleKind::Rotate {
            // an object's turn grip: a curved arrow on a short leader from
            // its front edge, so it reads as part of the object (A7-7)
            let front = element.as_ref().and_then(|e| {
                let ElementKind::Object(o) = &e.kind else {
                    return None;
                };
                let (lo, hi) = creabim_core::objects::type_of(doc.model(), e)?.bounds();
                let z = creabim_core::objects::base_z(doc.model(), e).unwrap_or(0.0);
                (input.project)(o.to_plan(DVec2::new((lo.x + hi.x) / 2.0, lo.y)).extend(z))
            });
            draw_turn_grip(painter, c, front, state, &look);
            if state == GripState::Hover {
                ctx.set_cursor_icon(egui::CursorIcon::Grab);
            }
            continue;
        }
        draw_grip(painter, c, bar, state, filled, picked, &look);
        if state == GripState::Hover {
            if let Some(dir) = bar.flatten() {
                ctx.set_cursor_icon(resize_cursor(dir.rot90()));
            } else {
                ctx.set_cursor_icon(egui::CursorIcon::Move);
            }
        }
    }
    // the held grip's old place stays as a faint tick
    if let Some(d) = drag.as_ref().filter(|d| d.kind.is_some()) {
        if let Some(c) = (input.project)(d.grab) {
            let s = Stroke::new(1.0, look.accent.gamma_multiply(0.45));
            painter.line_segment([c - Vec2::splat(4.0), c + Vec2::splat(4.0)], s);
            painter.line_segment([c + Vec2::new(-4.0, 4.0), c + Vec2::new(4.0, -4.0)], s);
        }
    }
    // a let-go grip's new value, for a second
    // only in the pane it was dragged in
    let flash_id = egui::Id::new("grip.flash").with(response.id);
    if let Some((text, number, at, t0)) =
        ctx.data(|d| d.get_temp::<(String, String, Pos2, f64)>(flash_id))
    {
        let age = ctx.input(|i| i.time) - t0;
        if age < FLASH {
            // steady for a second, then gone in the last third
            let fade = ((FLASH - age) / (FLASH * 0.3)).clamp(0.0, 1.0) as f32;
            pill_value(
                painter,
                at + Vec2::new(0.0, -34.0),
                &text,
                &number,
                &look,
                fade,
            );
            ctx.request_repaint();
        } else {
            ctx.data_mut(|d| d.remove::<(String, String, Pos2, f64)>(flash_id));
        }
    }
    if let (Some(k), Some(p)) = (hot, pointer) {
        let fixed = matches!(k, HandleKind::Vertex { ring, .. } | HandleKind::AddVertex { ring, .. }
            if element.as_ref().and_then(|e| crate::editmode::ring_of(doc.model(), e, ring)).is_some_and(|r| !r.reshapes));
        let at = shown
            .iter()
            .find(|h| h.kind == k)
            .and_then(screen)
            .unwrap_or(p);
        // the grip says what it is; in Edit mode, what more it does
        let name = layer_name(doc.model(), id, k).or_else(|| stair_piece_name(doc.model(), id, k));
        pill(
            painter,
            at + Vec2::new(0.0, -34.0),
            name.as_deref().unwrap_or(k.name()),
            &look,
            1.0,
        );
        if edit {
            let tip = if fixed {
                "Drag: move corner · This sketch's arcs or constraints keep its corners: no adding or removing"
            } else {
                k.tip(edit)
            };
            note(
                painter,
                p + Vec2::new(0.0, 22.0),
                tip,
                Color32::from_gray(40),
            );
        }
    }

    // the edge under the pointer names itself with its length: what a
    // click picks and a drag moves; not among a stair's pieces, whose
    // line stays as it is
    let pieces = layers()
        && element
            .as_ref()
            .is_some_and(|e| matches!(e.kind, ElementKind::Stair(_)));
    if let (Some((ring, i, _)), Some(p), Some(e), false) = (edge_hot, pointer, &element, pieces) {
        if let Some(r) = crate::editmode::ring_of(doc.model(), e, ring) {
            let n = r.corners.len();
            let len = r.corners[i % n].distance(r.corners[(i + 1) % n]);
            pill(
                painter,
                p + Vec2::new(0.0, -34.0),
                &format!("Edge · {}", crate::units::len(len)),
                &look,
                1.0,
            );
            note(
                painter,
                p + Vec2::new(0.0, 22.0),
                "Drag: move the edge · Click: select (Shift adds) · Right-click: more",
                Color32::from_gray(40),
            );
            ctx.set_cursor_icon(egui::CursorIcon::Move);
        }
    }

    // a corner's palette, opened by a click on its bracket (MU142)
    // while it is open the grips wait, as for the right-click menu
    if let Some(p) = palette().filter(|p| p.id == id && p.pane == response.id) {
        if let Some(s) = corner_palette(doc, &ctx, p, response.rect) {
            *status = Some(s);
        }
        return true;
    }
    // the right-click menu, open in this pane
    if grips.menu.is_some_and(|m| m.pane == response.id) {
        if let Some(s) = grip_menu(doc, id, &ctx, grips) {
            *status = Some(s);
        }
        return true;
    }
    if edit && drag.is_none() && response.secondary_clicked() {
        if let (Some(p), Some(e)) = (response.interact_pointer_pos(), &element) {
            let on = match nearest(p).map(|h| h.kind) {
                Some(
                    k @ (HandleKind::LayerEnd { .. }
                    | HandleKind::LayerEdge { .. }
                    | HandleKind::LayerHeight { .. }
                    | HandleKind::StairFlight { .. }
                    | HandleKind::StairLanding { .. }),
                ) => MenuOn::Layer(k),
                Some(HandleKind::Vertex { ring, i }) => {
                    if !grips.picked(ring, i) {
                        grips.pick(ring, i, false);
                    }
                    MenuOn::Corner(ring, i)
                }
                Some(HandleKind::AddVertex { ring, i } | HandleKind::Edge { ring, i }) => {
                    let at = edge_at(doc.model(), e, input, p)
                        .filter(|(r, j, _)| *r == ring && *j == i)
                        .map(|(_, _, at)| at)
                        .unwrap_or_else(|| {
                            shown
                                .iter()
                                .find(|h| h.kind == HandleKind::AddVertex { ring, i })
                                .map_or(DVec3::ZERO, |h| h.at)
                        });
                    MenuOn::Edge(ring, i, at)
                }
                _ => match edge_at(doc.model(), e, input, p) {
                    Some((ring, i, at)) => MenuOn::Edge(ring, i, at),
                    None if bay_at(doc.model(), e, input, p).is_some() => {
                        MenuOn::Bay(bay_at(doc.model(), e, input, p).unwrap_or_default())
                    }
                    None => match hole_at(doc.model(), e, input, p) {
                        Some(k) => {
                            grips.pick_hole(k, false);
                            MenuOn::Hole(k)
                        }
                        None => MenuOn::Blank,
                    },
                },
            };
            grips.menu = Some(GripMenu {
                pane: response.id,
                at: p,
                on,
            });
            return true;
        }
    }

    // a box over the grips, in Edit mode
    if let Some((pane, from)) = grips.boxsel {
        if pane == response.id {
            let to = response.interact_pointer_pos().or(pointer).unwrap_or(from);
            let rect = Rect::from_two_pos(from, to);
            painter.rect(
                rect,
                0.0,
                interaction_ink().gamma_multiply(0.08),
                Stroke::new(1.0, interaction_ink()),
                egui::StrokeKind::Middle,
            );
            if response.drag_stopped() || !response.dragged() {
                if !shift {
                    grips.corners.clear();
                    grips.holes.clear();
                }
                let mut mesh = vec![];
                for h in &shown {
                    if let HandleKind::Vertex { ring, i } = h.kind {
                        if screen(h).is_some_and(|c| rect.contains(c)) && !grips.picked(ring, i) {
                            grips.corners.push((ring, i));
                        }
                    }
                }
                // a mass's vertices in the box, whichever grips show
                if let Some(ElementKind::FreeMesh(m)) = element.as_ref().map(|e| &e.kind) {
                    let base = m.origin + DVec3::Z * elevation(doc.model(), m.level);
                    for (i, v) in m.mesh.vertices.iter().enumerate() {
                        if (input.project)(base + m.to_model(*v)).is_some_and(|c| rect.contains(c))
                        {
                            mesh.push(i as u32);
                        }
                    }
                }
                if !mesh.is_empty() || !grips.mesh.is_empty() {
                    grips.pick_mesh(&mesh, shift);
                    grips.boxsel = None;
                    *status = Some(format!(
                        "{} points picked: G or M moves them, R turns, S scales.",
                        grips.mesh.len()
                    ));
                    return true;
                }
                grips.boxsel = None;
                let shape = matches!(
                    element.as_ref().map(|e| &e.kind),
                    Some(ElementKind::FreeMesh(_))
                );
                *status = Some(match grips.corners.len() {
                    0 if shape => "No points in the box.".into(),
                    0 => "No corners in the box.".into(),
                    n => {
                        format!("{n} corners selected: Delete removes them, right-click for more.")
                    }
                });
            }
            return true;
        }
    }

    // a selected wall's location line, and a cross at an end that may not join
    if let Some(ElementKind::Wall(w)) = element.as_ref().map(|e| &e.kind) {
        let z = walls::base_z(doc.model(), w);
        let (a, b) = walls::location_line(doc.model(), w);
        if let (Some(pa), Some(pb)) = ((input.project)(a.extend(z)), (input.project)(b.extend(z))) {
            painter.add(egui::Shape::dashed_line(
                &[pa, pb],
                Stroke::new(1.2, interaction_ink()),
                8.0,
                3.0,
            ));
            for (i, p) in [pa, pb].into_iter().enumerate() {
                if w.disallow_join[i] {
                    let o = Vec2::new(10.0, -10.0);
                    let s = Stroke::new(2.0, Color32::from_rgb(200, 40, 40));
                    let c = p + o;
                    painter.line_segment([c - Vec2::splat(4.0), c + Vec2::splat(4.0)], s);
                    painter.line_segment([c + Vec2::new(-4.0, 4.0), c + Vec2::new(4.0, -4.0)], s);
                }
            }
        }
    }

    // flips are Space (facing) and Shift+Space (hand), and the action
    // bar: no circles on the plan (see flip_arrows)

    if let Some(parts) = &handle {
        draw_handle(painter, parts, handle_hot);
        if let (Some(part), Some(p)) = (handle_hot, pointer) {
            // it names itself by the pointer, and the status bar says the rest
            ctx.set_cursor_icon(egui::CursorIcon::Grab);
            let says = part.says();
            let text = ctx.global_style().visuals.text_color();
            let galley =
                painter.layout_no_wrap(says.clone(), egui::FontId::proportional(12.0), text);
            let pill = Rect::from_min_size(
                p + Vec2::new(16.0, -28.0),
                galley.size() + Vec2::new(12.0, 6.0),
            );
            painter.rect_filled(pill, 9.0, ctx.global_style().visuals.window_fill);
            painter.rect_stroke(
                pill,
                9.0,
                Stroke::new(1.0, interaction_ink()),
                egui::StrokeKind::Inside,
            );
            painter.galley(pill.min + Vec2::new(6.0, 3.0), galley, text);
            *status = Some(format!(
                "{says}: drag to do it now, or click to pick it up and type a value."
            ));
        }
        // a part of the handle taken: by a drag it ends on letting go, by
        // a click at the next click. The handle is asked before anything
        // under it
        let by_drag = response.drag_started_by(egui::PointerButton::Primary);
        if by_drag || response.clicked() {
            let at = if by_drag {
                ctx.input(|i| i.pointer.press_origin())
            } else {
                response.interact_pointer_pos()
            };
            if let Some(part) = at.and_then(|p| handle_hit(parts, p)) {
                HANDLE_PRESS.with(|p| p.set(at));
                HANDLE_ORIGIN.with(|p| p.set(handle_at.map(|(c, _)| c)));
                let shift = ctx.input(|i| i.modifiers.shift);
                grips.request = Some(GripRequest::Handle(part, by_drag, shift));
                return true;
            }
        }
    }
    // a click on a grip keeps the selection rather than picking what lies
    // under it; in Edit mode it picks the corner (Shift adds), and a
    // second click on the same corner takes it away
    if drag.is_none() && response.clicked() {
        if let Some(p) = response.interact_pointer_pos() {
            let hit = nearest(p).map(|h| h.kind);
            let now = ctx.input(|i| i.time);
            if let Some(k) = hit {
                // when the frames are slow both clicks can come in one
                // frame: egui still counts them as a double-click
                if response.double_clicked() {
                    grips.last_tap = Some((k, now, p));
                }
                if let Some(s) = tap_grip(doc, id, k, edit, shift, now, p, grips) {
                    *status = Some(s);
                }
                return true;
            }
            if edit {
                if let Some(k) = element
                    .as_ref()
                    .and_then(|e| hole_at(doc.model(), e, input, p))
                {
                    grips.pick_hole(k, shift);
                    *status = Some(format!(
                        "Hole {} selected: Delete removes it, a drag inside moves it.",
                        k + 1
                    ));
                    return true;
                }
                if let (Some((ring, i, _)), Some(e)) = (edge_under(p), &element) {
                    let n = crate::editmode::ring_of(doc.model(), e, ring)
                        .map_or(0, |r| r.corners.len());
                    grips.pick_edge(ring, i, n, shift);
                    *status = Some(picked_note(grips));
                    return true;
                }
                if let (Some(_), Some(e)) = (face_under(p), &element) {
                    let n = crate::editmode::ring_of(doc.model(), e, Ring::Outline)
                        .map_or(0, |r| r.corners.len());
                    grips.holes.clear();
                    grips.corners = (0..n).map(|i| (Ring::Outline, i)).collect();
                    *status = Some(
                        "The face picked: a drag moves the whole outline, M moves it by a base point."
                            .into(),
                    );
                    return true;
                }
                // a click on nothing lets go of what is picked and stays
                // in Edit mode (it used to leave it, with "0 selected")
                if !shift {
                    grips.clear();
                }
                *status = Some("Nothing picked. Still editing: Esc or Tab finishes.".into());
                return true;
            }
        }
    }

    if drag.is_none() && response.drag_started_by(egui::PointerButton::Primary) {
        let Some(press) = ctx
            .input(|i| i.pointer.press_origin())
            .or(response.interact_pointer_pos())
        else {
            return false;
        };
        let near = nearest(press).or_else(|| turn_dot.filter(|_| on_ring(press)));
        // in Edit mode a drag from inside a hole moves the hole
        let hole = (near.is_none() && edit)
            .then(|| {
                element
                    .as_ref()
                    .and_then(|e| hole_at(doc.model(), e, input, press))
            })
            .flatten()
            .and_then(|k| {
                let e = element.as_ref()?;
                let r = crate::editmode::ring_of(doc.model(), e, Ring::Hole(k))?;
                let at = (input.ray)(press)
                    .and_then(|ray| {
                        Constraint::Plane {
                            point: r.corners[0],
                            normal: r.normal,
                        }
                        .hit(ray)
                    })
                    .unwrap_or(r.corners[0]);
                let h = in_section(
                    Handle {
                        kind: HandleKind::Ring {
                            ring: Ring::Hole(k),
                        },
                        at,
                        constraint: Constraint::Plane {
                            point: at,
                            normal: r.normal,
                        },
                        nudge: 0.0,
                    },
                    input.look,
                );
                h.constraint.usable(input.look).then_some(h)
            });
        let taken = |kind: HandleKind, at: DVec3, normal: DVec3| {
            let h = in_section(
                Handle {
                    kind,
                    at,
                    constraint: Constraint::Plane { point: at, normal },
                    nudge: 0.0,
                },
                input.look,
            );
            h.constraint.usable(input.look).then_some(h)
        };
        let normal_of = |ring: Ring| {
            element
                .as_ref()
                .and_then(|e| crate::editmode::ring_of(doc.model(), e, ring))
                .map(|r| r.normal)
        };
        let edge = edge_under(press).and_then(|(ring, i, at)| {
            taken(HandleKind::EdgeMove { ring, i }, at, normal_of(ring)?)
        });
        let whole = face_under(press).and_then(|at| {
            taken(
                HandleKind::Ring {
                    ring: Ring::Outline,
                },
                at,
                normal_of(Ring::Outline)?,
            )
        });
        let picked = near.or(edge).or(hole).or(whole);
        let grip_at = picked.map(|h| h.at);
        let start = match picked {
            Some(h) => {
                let grab = (input.ray)(press)
                    .and_then(|r| h.constraint.hit(r))
                    .unwrap_or(h.at);
                Some((Some(h.kind), h.constraint, grab))
            }
            // in Edit mode a drag off the grips draws a box round some, in
            // the 3D view too (there it selected whole elements, MU45)
            None if edit => {
                grips.boxsel = Some((response.id, press));
                grips.menu = None;
                return true;
            }
            None => {
                let movable = !matches!(
                    element.as_ref().map(|e| &e.kind),
                    Some(
                        ElementKind::View(_)
                            | ElementKind::Dimension(_)
                            | ElementKind::Tag(_)
                            | ElementKind::Note(_)
                            | ElementKind::TextStyle(_)
                            | ElementKind::SheetTemplate(_)
                            | ElementKind::Level(_)
                            | ElementKind::Sheet(_)
                            | ElementKind::Viewport(_)
                    ) | None
                );
                (input.grab_body)(press, id).filter(|_| movable).map(|p| {
                    let p = (input.grab_snap)(press, id).unwrap_or(p);
                    // a wall's body slides it square to itself, its joined
                    // walls stretching; Shift frees the direction
                    if let (Some(ElementKind::Wall(w)), false) =
                        (element.as_ref().map(|e| &e.kind), shift)
                    {
                        let c = Constraint::Line {
                            point: p,
                            dir: w.direction().perp().extend(0.0),
                        };
                        return (Some(HandleKind::WallSlide), c, p);
                    }
                    let c = match element.as_ref().map(|e| &e.kind) {
                        // a grid line slides square to itself
                        Some(ElementKind::Grid(g)) => Constraint::Line {
                            point: p,
                            dir: (g.end - g.start).normalize_or_zero().perp().extend(0.0),
                        },
                        _ => Constraint::Plane {
                            point: p,
                            normal: input.body_normal,
                        },
                    };
                    (None, c, p)
                })
            }
        };
        if let Some((kind, constraint, grab)) = start {
            let (mut originals, mut joined) = match kind {
                Some(k) => originals(doc.model(), id, k),
                None => (element.clone().into_iter().collect(), vec![]),
            };
            // Ctrl or Alt drags a wall end free of the walls joined to it
            let free = matches!(kind, Some(HandleKind::WallEnd { .. }))
                && ctx.input(|i| i.modifiers.command || i.modifiers.alt);
            if free {
                originals.truncate(1);
                joined.clear();
            }
            let name = originals.first().map_or(String::new(), |e| e.name.clone());
            let label = match kind {
                Some(k) if free => format!("{} {name} free", k.verb()),
                Some(k) => k.label(&name),
                None => format!("Move {name}"),
            };
            crate::actlog::line(format!(
                "drag start: {label} | pane {} | grab {} | step {} mm | grip {:?}",
                pane_name(input),
                crate::actlog::pt(grab),
                crate::actlog::mm(crate::snaps::current_step()),
                kind,
            ));
            // a whole element taken by its body moves from where it was
            // taken; a grip from its own point
            let anchor = match (kind, grip_at) {
                (Some(_), Some(at)) => constraint.project(at),
                _ => grab,
            };
            *drag = Some(Drag {
                kind,
                constraint,
                edit: LiveEdit::new(label, originals).free(free),
                joined,
                grab,
                anchor,
                press,
                last: grab,
                typed: None,
                owner: response.id,
            });
        }
    }

    let Some(d) = drag.as_mut() else {
        grips.typing = false;
        return false;
    };
    // another pane's drag: leave it to that pane
    if d.owner != response.id {
        return false;
    }
    // Tab, or a digit, starts typing a length
    let typed_now = ctx.input_mut(|i| {
        let tab = i.consume_key(egui::Modifiers::NONE, egui::Key::Tab);
        let digit = i.events.iter().find_map(|e| match e {
            egui::Event::Text(t) if t.chars().all(|c| c.is_ascii_digit() || c == '.') => {
                Some(t.clone())
            }
            _ => None,
        });
        (tab, digit)
    });
    if d.kind.is_some() && d.typed.is_none() {
        match typed_now {
            (true, _) => d.typed = Some(String::new()),
            (_, Some(t)) => d.typed = Some(t),
            _ => {}
        }
        if d.typed.is_some() {
            return true;
        }
    }
    if d.typed.is_some() {
        grips.typing = true;
        if typed_length(doc, painter, d, input, status) {
            *drag = None;
            grips.typing = false;
        }
        return true;
    }
    grips.typing = false;
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        d.edit.cancel(doc);
        *drag = None;
        *status = Some("Drag cancelled. Tab finishes editing.".into());
        return true;
    }
    if response.drag_stopped() || !response.dragged() {
        let kind = d.kind;
        let applied = d.edit.applied();
        if applied {
            // the full summary in the status bar; by the grip, for a
            // second, only the value that changed
            let now = doc.model().get(id).cloned();
            let summary = now
                .as_ref()
                .map(|e| readout(std::slice::from_ref(e)))
                .filter(|v| !v.is_empty());
            *status = doc.undo_label().map(|l| match &summary {
                Some(s) => format!("{l}: {s}. Ctrl+Z to undo."),
                None => format!("{l}. Ctrl+Z to undo."),
            });
            let change = match (d.kind, d.edit.originals().first(), &now) {
                (None | Some(HandleKind::WallSlide), old, new) => Some((
                    "Moved ".to_string(),
                    bare(moved_by(old, new.as_ref()).unwrap_or((d.last - d.grab).length())),
                )),
                (Some(_), Some(old), Some(new)) => changing(old, new),
                _ => None,
            };
            if let (Some((text, number)), Some(at), true) =
                (change, (input.project)(d.last), live_on(&ctx))
            {
                let t = ctx.input(|i| i.time);
                ctx.data_mut(|m| {
                    m.insert_temp(
                        egui::Id::new("grip.flash").with(response.id),
                        (text, number, at, t),
                    )
                });
            }
        }
        let press = d.press;
        crate::actlog::line(format!(
            "drag end: {} | applied {}",
            d.edit.label(),
            d.edit.applied()
        ));
        *drag = None;
        match kind {
            // a press on a grip that never moved is a click on it
            Some(k) if !applied => {
                let now = ctx.input(|i| i.time);
                if let Some(s) = tap_grip(doc, id, k, edit, shift, now, press, grips) {
                    *status = Some(s);
                }
            }
            // the corner dragged stays picked
            Some(HandleKind::Vertex { ring, i }) if edit => grips.pick(ring, i, false),
            Some(HandleKind::AddVertex { ring, i }) if edit => grips.pick(ring, i + 1, false),
            // the edge dragged stays picked
            Some(HandleKind::EdgeMove { ring, i }) if edit => {
                let n = doc
                    .model()
                    .get(id)
                    .and_then(|e| crate::editmode::ring_of(doc.model(), e, ring))
                    .map_or(0, |r| r.corners.len());
                grips.pick_edge(ring, i, n, false);
            }
            Some(HandleKind::Ring {
                ring: Ring::Hole(k),
            }) if edit => grips.pick_hole(k, false),
            _ => {}
        }
        return true;
    }
    let Some(pos) = response.interact_pointer_pos() else {
        return true;
    };
    // a click held a moment, or a shaky one, moves nothing
    if pos.distance(d.press) < crate::modal::STILL && !d.edit.applied() {
        return true;
    }
    let Some(free) = (input.ray)(pos).and_then(|r| d.constraint.hit(r)) else {
        return true;
    };
    let fixed = match (d.kind, d.edit.originals().first().map(|e| &e.kind)) {
        (Some(HandleKind::WallEnd { end }), Some(ElementKind::Wall(w))) => {
            let (a, b) = walls::location_line(doc.model(), w);
            Some(if end { a } else { b }.extend(free.z))
        }
        (None, _) => Some(d.grab),
        _ => None,
    };
    let snapped = (input.snap)(pos, fixed);
    // whether a snap (or a face) caught it, rather than the pointer alone
    let mut caught = false;
    let now = match (d.constraint, snapped) {
        (Constraint::Plane { point, normal }, Some((p, snap))) => {
            if snap.kind != SnapKind::Grid {
                caught = true;
                if let Some(at) = (input.project)(p) {
                    crate::snaps::draw(painter, snap, at, fixed.and_then(|f| (input.project)(f)));
                }
            }
            // onto the plane, along its normal
            p - normal * (p - point).dot(normal)
        }
        // a grip sliding along a line stops on points near its path, and
        // where an edge, a level or grid line, or a guide crosses it (the
        // snap's point is the nearest on that line, so near the crossing):
        // a wall's base dragged in a section stops on a slab's top
        (Constraint::Line { point, dir }, Some((p, snap)))
            if matches!(
                snap.kind,
                SnapKind::Endpoint
                    | SnapKind::Midpoint
                    | SnapKind::Center
                    | SnapKind::Intersection
                    | SnapKind::Quadrant
                    | SnapKind::OnEdge
                    | SnapKind::OnGridLine
                    | SnapKind::Guide
                    | SnapKind::OnTempGuide
                    | SnapKind::GuideCross
                    | SnapKind::UnderlayFace
            ) && near_on_screen(input, point + dir * (p - point).dot(dir), free) =>
        {
            let on = crossing(input, fixed, point, dir, p, snap.kind);
            caught = true;
            if let Some(at) = (input.project)(on) {
                crate::snaps::draw(painter, snap, at, None);
            }
            on
        }
        // nothing caught it: a face it slides into (3D), when near
        (Constraint::Line { point, dir }, _) => match on_face(input, pos, point, dir, free) {
            Some(on) => {
                caught = true;
                if let Some(at) = (input.project)(on) {
                    let snap = crate::snaps::Snap {
                        p: on,
                        kind: SnapKind::OnFace,
                        host: None,
                    };
                    crate::snaps::draw(painter, snap, at, None);
                }
                on
            }
            None => free,
        },
        _ => free,
    };
    // a snap (or a face) caught it: the grip itself goes there, so the move
    // is measured from the grip's own point, not from where it was pressed
    let now = if caught {
        now + (d.grab - d.anchor)
    } else {
        now
    };
    // distance marks from where it was grabbed, when no real snap caught it
    let now = if snapped
        .as_ref()
        .is_none_or(|(_, sn)| sn.kind == SnapKind::Grid)
    {
        distance_marks(painter, input, d.grab, now, crate::snaps::current_marks())
    } else {
        now
    };
    // a line's end holds the line's own axis, with a guide along it,
    // unless a real snap caught it or it is pulled well off (MU76: a
    // section or elevation stretched skewed; MU74 for grids)
    let now = match (&snapped, line_axis(doc.model(), d)) {
        (Some((_, sn)), _) if sn.kind != SnapKind::Grid => now,
        (_, Some((far, dir))) => hold_on_axis(painter, input, far, dir, now),
        _ => now,
    };
    crate::actlog::line(format!(
        "drag frame: pane {} | pointer ({:.1}, {:.1}) px | free {} | snap {} | used {} | step {} mm | frame {:.1} ms",
        pane_name(input),
        pos.x,
        pos.y,
        crate::actlog::pt(free),
        match &snapped {
            Some((p, sn)) => format!("{:?} at {}", sn.kind, crate::actlog::pt(*p)),
            None => "none".into(),
        },
        crate::actlog::pt(now),
        crate::actlog::mm(crate::snaps::current_step()),
        painter.ctx().input(|i| i.unstable_dt) * 1000.0,
    ));
    d.last = now;
    let grid = snapped.is_none_or(|(_, s)| s.kind == SnapKind::Grid);
    follow(doc, painter, d, input, now, grid, Some(pos), status);
    painter.ctx().request_repaint();
    true
}

/// The line whose end is being dragged: its other end (fixed) and its
/// direction, for a wall's location line, a straight grid line and a
/// section or elevation line, as they were when the drag began.
fn line_axis(model: &creabim_core::Model, d: &Drag) -> Option<(DVec3, DVec2)> {
    let e = d.edit.originals().first()?;
    let (a, b, end) = match (d.kind?, &e.kind) {
        (HandleKind::WallEnd { end }, ElementKind::Wall(w)) => {
            let (a, b) = walls::location_line(model, w);
            (a, b, end)
        }
        (HandleKind::GridEnd { end }, ElementKind::Grid(g)) if g.bulge.abs() < 1e-9 => {
            (g.start, g.end, end)
        }
        (HandleKind::ViewEnd { end }, ElementKind::View(v)) => match &v.kind {
            ViewKind::Section { start, end: e2, .. } => (*start, *e2, end),
            _ => return None,
        },
        _ => return None,
    };
    let (here, far) = if end { (b, a) } else { (a, b) };
    let dir = (here - far).try_normalize()?;
    Some((far.extend(d.grab.z), dir))
}

/// Within 14 px of the line's own axis on screen, the point is put on it
/// and a dashed guide is drawn along the line through its fixed end;
/// further off it moves freely.
fn hold_on_axis(painter: &Painter, input: &PaneInput, far: DVec3, dir: DVec2, now: DVec3) -> DVec3 {
    let f2 = far.truncate();
    let t = (now.truncate() - f2).dot(dir);
    let on = (f2 + dir * t).extend(now.z);
    let (Some(at), Some(here)) = ((input.project)(on), (input.project)(now)) else {
        return now;
    };
    if at.distance(here) > 14.0 || t <= 0.0 {
        return now;
    }
    // the guide runs from behind the fixed end to well past the grip
    let reach = t.abs().max(1.0);
    let a = (f2 - dir * reach * 0.25).extend(now.z);
    let b = (f2 + dir * (t + reach * 0.6)).extend(now.z);
    if let (Some(pa), Some(pb)) = ((input.project)(a), (input.project)(b)) {
        painter.extend(egui::Shape::dashed_line(
            &[pa, pb],
            Stroke::new(1.2, crate::snaps::GUIDE),
            6.0,
            4.0,
        ));
    }
    painter.circle_filled(at, 3.0, crate::snaps::GUIDE);
    on
}

/// Distance marks (the Grid snap menu): faint marks every `every` metres
/// from `grab`, along the way the drag is going, drawn while it lasts.
/// Within 10 px of one the drag holds on it; elsewhere it stays where the
/// pointer put it. Marks closer than 6 px apart on screen are neither drawn
/// nor caught.
fn distance_marks(
    painter: &Painter,
    input: &PaneInput,
    grab: DVec3,
    now: DVec3,
    every: f64,
) -> DVec3 {
    if every <= 0.0 {
        return now;
    }
    let v = now - grab;
    let len = v.length();
    if len < 1e-9 {
        return now;
    }
    let dir = v / len;
    let (Some(a), Some(b)) = ((input.project)(grab), (input.project)(grab + dir * every)) else {
        return now;
    };
    let px = a.distance(b);
    if px < 6.0 {
        return now;
    }
    let held = mark_held(len, every, px as f64);
    let k = held.unwrap_or(0.0) / every;
    let held = held.is_some();
    let side = (b - a).normalized().rot90();
    let quiet = Stroke::new(1.0, Color32::from_rgb(79, 107, 76).gamma_multiply(0.75));
    let hot = Stroke::new(2.0, crate::planview::accent_selected());
    let n = ((len / every).ceil() as i64 + 1).min(400);
    for i in 1..=n {
        let Some(c) = (input.project)(grab + dir * (every * i as f64)) else {
            continue;
        };
        let caught = held && i as f64 == k;
        let (h, stroke) = if caught { (7.0, hot) } else { (4.0, quiet) };
        painter.line_segment([c - side * h, c + side * h], stroke);
    }
    if held {
        grab + dir * (k * every)
    } else {
        now
    }
}

/// The distance mark a drag `len` metres long holds on, if the pointer is
/// within 10 px of one (marks `every` metres apart, `px` pixels apart on
/// screen); never the grab point itself.
fn mark_held(len: f64, every: f64, px: f64) -> Option<f64> {
    let k = (len / every).round();
    (k >= 1.0 && ((len - k * every) / every).abs() * px <= 10.0).then_some(k * every)
}

/// Which kind of pane a drag is in, for the activity log.
/// A section or elevation pane: one that looks sideways at the model.
fn seen_side_on(input: &PaneInput) -> bool {
    input.view.is_some() && input.look.z.abs() < 0.5
}

/// Does `id` turn only about the upright (an object)?
fn turns_upright_only(model: &creabim_core::Model, id: ElementId) -> bool {
    matches!(model.get(id).map(|e| &e.kind), Some(ElementKind::Object(_)))
}

fn pane_name(input: &PaneInput) -> &'static str {
    match input.view {
        None => "3D",
        Some(_) if input.look.z.abs() > 0.99 => "plan",
        Some(_) => "section or elevation",
    }
}

/// A click on grip `kind`: in Edit mode it picks a corner (Shift adds or
/// drops it), and a second click on the same corner takes it away; an
/// add-corner grip only says to drag it. Outside Edit mode a double-click
/// on a corner opens Edit mode. What the status bar says.
#[allow(clippy::too_many_arguments)]
fn tap_grip(
    doc: &mut Document,
    id: ElementId,
    kind: HandleKind,
    edit: bool,
    shift: bool,
    time: f64,
    at: Pos2,
    grips: &mut GripSel,
) -> Option<String> {
    // a click on a corner's bracket, not a drag: its palette (MU142)
    if let HandleKind::ScaleCorner(corner) = kind {
        set_palette(Some(Palette {
            id,
            corner,
            at,
            pane: tap_pane(),
            opened: time,
        }));
        return Some("Corner: turn it, mirror it or scale it by a factor. Esc closes.".into());
    }
    let double = grips.tap(kind, time, at);
    // a mass in Edit mode: a click picks the vertex, the edge or the face,
    // and it stays picked to act on, as in Blender (MU39)
    if edit {
        if let Some(ElementKind::FreeMesh(m)) = doc.model().get(id).map(|e| &e.kind) {
            let vs: Option<Vec<u32>> = match kind {
                HandleKind::MeshVertex(i) => Some(vec![i as u32]),
                HandleKind::MeshEdge(a, b) => Some(vec![a, b]),
                HandleKind::Face(f) => m.mesh.faces.get(f).cloned(),
                _ => None,
            };
            if let Some(vs) = vs {
                // a double-click takes the whole piece it belongs to
                if double {
                    let piece = m.mesh.linked(&vs);
                    let all = piece.len() == m.mesh.vertices.len();
                    grips.pick_mesh(&[], false);
                    grips.pick_mesh(&piece, false);
                    return Some(format!(
                        "{} picked ({} points): G or M moves it, R turns, S scales. L does the same from what is picked.",
                        if all { "The whole shape" } else { "A piece" },
                        piece.len()
                    ));
                }
                grips.pick_mesh(&vs, shift);
                let what = match (crate::handles::pick(), kind) {
                    (_, HandleKind::MeshEdge(..)) | (2, _) => "edge",
                    (_, HandleKind::Face(_)) | (3, _) => "face",
                    _ => "point",
                };
                return Some(match grips.mesh.len() {
                    0 => "Nothing picked.".into(),
                    n => format!(
                        "{n} points picked ({what}): G or M moves them, R turns, S scales; Shift+click picks more."
                    ),
                });
            }
        }
    }
    match kind {
        HandleKind::Vertex { ring, i } if edit => {
            if double && !shift {
                let r = crate::editmode::remove_corners(doc, id, &[(ring, i)]);
                if r.is_ok() {
                    grips.clear();
                }
                return Some(r.unwrap_or_else(|e| e));
            }
            grips.pick(ring, i, shift);
            Some(match grips.corners.len() {
                0 => "Nothing picked.".into(),
                1 => "Corner selected: Delete removes it, a drag moves it, right-click for more."
                    .into(),
                n => format!("{n} corners selected: Delete removes them."),
            })
        }
        HandleKind::Vertex { .. } if double => {
            grips.request = Some(GripRequest::Enter);
            None
        }
        HandleKind::AddVertex { .. } => Some(
            "Drag the ◇ to pull out a new corner, or right-click the edge › Add corner here."
                .into(),
        ),
        // a press on an edge that never moved is a click on it
        HandleKind::EdgeMove { ring, i } if edit => {
            let n = doc
                .model()
                .get(id)
                .and_then(|e| crate::editmode::ring_of(doc.model(), e, ring))
                .map_or(0, |r| r.corners.len());
            grips.pick_edge(ring, i, n, shift);
            Some(picked_note(grips))
        }
        _ => None,
    }
}

/// What is picked on an outline in Edit mode, for the status bar.
fn picked_note(grips: &GripSel) -> String {
    match grips.corners.len() {
        0 => "Nothing picked.".into(),
        2 => "Edge picked: a drag moves it, M moves it by a base point, Delete opens the outline there, right-click or the strip for Offset edge and Insert point. Shift+click picks more.".into(),
        n => format!("{n} corners picked: M moves them by a base point, Delete removes them."),
    }
}

/// Remake the drag with the grip at `now`: the elements it changes shown
/// in place, or (an outline that would be broken) red, with the reason.
#[allow(clippy::too_many_arguments)]
fn follow(
    doc: &mut Document,
    painter: &Painter,
    d: &mut Drag,
    input: &PaneInput,
    now: DVec3,
    grid: bool,
    pointer: Option<Pos2>,
    status: &mut Option<String>,
) {
    let model = doc.model().clone();
    GRIP_FREE.store(
        painter.ctx().input(|i| i.modifiers.alt),
        std::sync::atomic::Ordering::Relaxed,
    );
    let result = match d.kind {
        Some(k) => apply(
            &model,
            d.edit.originals(),
            &d.joined,
            k,
            d.grab,
            now,
            crate::snaps::current_step(),
        ),
        None => {
            let delta = now - d.grab;
            let delta = if grid {
                let s = crate::snaps::current_step();
                if s > 0.0 {
                    (delta / s).round() * s
                } else {
                    delta
                }
            } else {
                delta
            };
            d.edit.originals()[..1]
                .iter()
                .map(|e| {
                    transform::transformed(&model, e, &Xform::Move(delta))
                        .ok_or_else(|| "this can't move that way".to_string())
                })
                .collect()
        }
    };
    if d.kind == Some(HandleKind::Rotate) {
        draw_grip_protractor(painter, input, &model, d, now);
    }
    let outline = matches!(
        d.kind,
        Some(
            HandleKind::Vertex { .. }
                | HandleKind::AddVertex { .. }
                | HandleKind::Edge { .. }
                | HandleKind::EdgeMove { .. }
                | HandleKind::Ring { .. }
        )
    );
    match result {
        Ok(changed) => {
            let mut said = match d.kind {
                // a whole element moved: how far
                None => format!(
                    "{}: {}",
                    d.edit.label(),
                    crate::units::len((now - d.grab).length())
                ),
                Some(_) => format!("{}  {}", d.edit.label(), readout(&changed)),
            };
            crate::actlog::line(format!(
                "  result: {said} | step {} mm",
                crate::actlog::mm(crate::snaps::current_step())
            ));
            if let Some(ElementKind::Shaft(s)) = changed.first().map(|e| &e.kind) {
                let missed: Vec<String> = creabim_core::slab::shaft_misses(&model, s)
                    .into_iter()
                    .filter_map(|m| model.get(m).map(|e| e.name.clone()))
                    .collect();
                if !missed.is_empty() {
                    said += &format!(
                        "  Warning: it doesn't cut {}, as it crosses the edge",
                        missed.join(", ")
                    );
                }
            }
            if let (true, Some(k), Some(p), Some(e)) = (outline, d.kind, pointer, changed.first()) {
                note(
                    painter,
                    p,
                    &live_lengths(&model, e, k, d.grab, now),
                    interaction_ink(),
                );
            }
            // the live pill: only the value that's changing, by the grip
            let ctx = painter.ctx();
            if live_on(ctx) && !outline {
                let change = match (d.kind, d.edit.originals().first(), changed.first()) {
                    (None | Some(HandleKind::WallSlide), old, new) => Some((
                        "Moved ".to_string(),
                        bare(moved_by(old, new).unwrap_or((now - d.grab).length())),
                    )),
                    (Some(_), Some(old), Some(new)) => changing(old, new),
                    _ => None,
                };
                if let (Some((text, number)), Some(at)) = (change, (input.project)(now)) {
                    let look = GripLook::of(ctx);
                    pill_value(
                        painter,
                        at + Vec2::new(0.0, -34.0),
                        &text,
                        &number,
                        &look,
                        1.0,
                    );
                }
            }
            *status = Some(said);
            if let Err(e) = d.edit.update(doc, changed) {
                *status = Some(format!("Can't go there: {e}"));
            }
        }
        Err(e) => {
            // the broken shape in red, never kept: the last good one stays
            if let (true, Some(k), Some(orig)) = (outline, d.kind, d.edit.originals().first()) {
                let mut broken = orig.clone();
                if crate::editmode::reshape_unchecked(
                    &model,
                    &mut broken,
                    k,
                    d.grab,
                    now,
                    crate::snaps::current_step(),
                )
                .is_ok()
                {
                    draw_rings(
                        &model,
                        &broken,
                        painter,
                        input,
                        &GripSel::default(),
                        (INVALID, INVALID),
                        None,
                    );
                }
                if let Some(p) = pointer {
                    note(painter, p, &format!("Not allowed: {e}"), INVALID);
                }
            }
            *status = Some(format!(
                "Can't go there: {e}. Release keeps the last good shape; Esc cancels."
            ));
        }
    }
}

/// A length typed into a drag: shown by the grip; Enter moves the grip
/// that far from where it was grabbed, the way the pointer went, and
/// finishes; Esc goes back to following the pointer. True when the drag
/// is done.
fn typed_length(
    doc: &mut Document,
    painter: &Painter,
    d: &mut Drag,
    input: &PaneInput,
    status: &mut Option<String>,
) -> bool {
    let ctx = painter.ctx().clone();
    let (text, back, enter, esc) = ctx.input_mut(|i| {
        let text: String = i
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Text(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        (
            text,
            i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
        )
    });
    let Some(t) = d.typed.as_mut() else {
        return false;
    };
    for c in text.chars() {
        match c {
            '0'..='9' | '.' | '-' | '\'' | '"' | '/' | ' ' => t.push(c),
            ',' => t.push('.'),
            'm' | 'c' | 'f' | 't' | 'i' | 'n' if !t.is_empty() => t.push(c),
            _ => {}
        }
    }
    if back {
        t.pop();
    }
    if esc {
        d.typed = None;
        *status = Some("Typing cancelled: the grip follows the pointer again.".into());
        return false;
    }
    let dir = match d.constraint {
        Constraint::Line { dir, .. } => {
            let s = (d.last - d.grab).dot(dir);
            if s < 0.0 {
                -dir
            } else {
                dir
            }
        }
        Constraint::Plane { .. } => (d.last - d.grab).normalize_or_zero(),
    };
    let shown = t.clone();
    let at = (input.project)(d.grab + dir * crate::units::parse(&shown).unwrap_or(0.0));
    if let Some(p) = at.or((input.project)(d.grab)) {
        note(
            painter,
            p,
            &format!(
                "{shown}| {}  Enter: apply · Esc: back to the pointer",
                crate::units::symbol()
            ),
            interaction_ink(),
        );
    }
    *status = Some(format!(
        "Type a length ({}) and press Enter: the grip goes that far the way you dragged it.",
        crate::units::symbol()
    ));
    if !enter {
        return false;
    }
    let Some(v) = crate::units::parse(&shown) else {
        *status = Some(format!("“{shown}” isn't a length."));
        return false;
    };
    if dir == DVec3::ZERO {
        *status = Some("Drag the grip a little first, to show which way it goes.".into());
        return false;
    }
    let now = d.grab + dir * v;
    follow(doc, painter, d, input, now, false, None, status);
    if d.edit.applied() {
        *status = doc.undo_label().map(|l| format!("{l}. Ctrl+Z to undo."));
    }
    true
}

#[cfg(test)]
mod reference_grip_tests {
    use super::*;
    use creabim_core::{RefSource, Reference};

    #[test]
    fn a_reference_moves_by_its_middle_scales_by_a_corner_and_turns() {
        let mut r = Reference::new(
            RefSource::Dxf {
                path: "survey.dxf".into(),
            },
            vec![],
        );
        r.in_place([DVec2::new(0.0, 0.0), DVec2::new(10.0, 6.0)]);
        let e = Element::new("Reference", ElementKind::Reference(r));
        let id = e.id;
        let model = Model::from_elements([e]).unwrap();
        let hs = handles(&model, id, None);
        let kinds: Vec<HandleKind> = hs.iter().map(|h| h.kind).collect();
        assert!(kinds.contains(&HandleKind::RefMove) && kinds.contains(&HandleKind::Rotate));
        assert_eq!(
            kinds
                .iter()
                .filter(|k| matches!(k, HandleKind::RefScale(_)))
                .count(),
            4
        );
        let drag = |kind: HandleKind, grab: DVec2, now: DVec2| {
            let (orig, joined) = originals(&model, id, kind);
            let out = apply(
                &model,
                &orig,
                &joined,
                kind,
                grab.extend(0.0),
                now.extend(0.0),
                0.0,
            )
            .unwrap();
            let ElementKind::Reference(r) = &out[0].kind else {
                panic!()
            };
            r.clone()
        };
        // its middle, dragged 2 m right
        let moved = drag(
            HandleKind::RefMove,
            DVec2::new(5.0, 3.0),
            DVec2::new(7.0, 3.0),
        );
        assert!(moved.at.distance(DVec2::new(7.0, 3.0)) < 1e-9);
        // a corner dragged twice as far from the middle doubles it
        let big = drag(
            HandleKind::RefScale(2),
            DVec2::new(10.0, 6.0),
            DVec2::new(15.0, 9.0),
        );
        assert!((big.scale - 2.0).abs() < 1e-9, "{}", big.scale);
        // the turn grip, a quarter turn about the middle
        let turned = drag(
            HandleKind::Rotate,
            DVec2::new(5.0, 9.0),
            DVec2::new(-1.0, 3.0),
        );
        assert!((turned.rotation - 90.0).abs() < 1e-9, "{}", turned.rotation);
        // locked, it has no grips
        let mut e2 = model.get(id).unwrap().clone();
        if let ElementKind::Reference(r) = &mut e2.kind {
            r.locked = true;
        }
        let locked = Model::from_elements([e2]).unwrap();
        assert!(handles(&locked, id, None).is_empty());
    }
}

/// An L wrapping corner `c` from outside, along its sides towards `prev`
/// and `next`: the corner's scale mark (Tool Rows and Scale REV01, A).
fn draw_bracket(painter: &Painter, c: Pos2, prev: Pos2, next: Pos2, colour: Color32) {
    let (a, b) = ((prev - c).normalized(), (next - c).normalized());
    let out = -(a + b).normalized();
    if !(a.is_finite() && b.is_finite() && out.is_finite()) {
        return;
    }
    let k = c + out * 4.0;
    painter.add(egui::Shape::line(
        vec![k + a * 10.0, k, k + b * 10.0],
        Stroke::new(2.6, colour),
    ));
}

/// A corner's palette, open (MU142, Grip Palette REV01): which object and
/// corner, where it was clicked, in which pane, and when.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub id: ElementId,
    #[allow(dead_code)]
    pub corner: u8,
    pub at: Pos2,
    pub pane: egui::Id,
    pub opened: f64,
}

static PALETTE: std::sync::Mutex<Option<Palette>> = std::sync::Mutex::new(None);
static PALETTE_COMMAND: std::sync::Mutex<Option<crate::commands::Command>> =
    std::sync::Mutex::new(None);
/// The pane whose grips are being handled this frame.
static TAP_PANE: std::sync::Mutex<Option<egui::Id>> = std::sync::Mutex::new(None);

fn tap_pane() -> egui::Id {
    TAP_PANE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .unwrap_or(egui::Id::NULL)
}

pub fn palette() -> Option<Palette> {
    *PALETTE.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn set_palette(p: Option<Palette>) {
    *PALETTE.lock().unwrap_or_else(|e| e.into_inner()) = p;
}

/// What the palette asked the view to start (Mirror, Scale), once.
pub fn take_palette_command() -> Option<crate::commands::Command> {
    PALETTE_COMMAND
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
}

/// What a click in a corner's palette does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pick {
    Turn(f64),
    Mirror,
    Scale,
}

/// Do what was picked in a corner's palette: a turn is made at once,
/// Mirror and Scale are handed to the view to start.
fn palette_pick(doc: &mut Document, id: ElementId, what: Pick) -> Option<String> {
    match what {
        Pick::Turn(by) => {
            let before = doc.model().get(id)?.clone();
            let mut after = before.clone();
            turn_object(doc.model(), &mut after, by);
            let label = format!("Turn {}", before.name);
            doc.commit(creabim_core::Transaction::new(label).replace(before, after))
                .ok()?;
            Some(format!("Turned {by}\u{b0}. Ctrl+Z to undo."))
        }
        Pick::Mirror | Pick::Scale => {
            let c = if what == Pick::Mirror {
                crate::commands::Command::Mirror
            } else {
                crate::commands::Command::Scale
            };
            *PALETTE_COMMAND.lock().unwrap_or_else(|e| e.into_inner()) = Some(c);
            None
        }
    }
}

/// Where a corner's palette opens: just below and right of the corner,
/// or turned to its other sides where it would run past the pane's edge
/// (MU142, seen in the app: by a pane's right edge it ran over the
/// Properties panel). `size`: the palette's, once it has been shown.
fn palette_at(at: Pos2, pane: egui::Rect, size: Option<egui::Vec2>) -> Pos2 {
    let size = size.unwrap_or(egui::vec2(320.0, 36.0));
    let gap = 12.0;
    let mut x = at.x + gap;
    if x + size.x > pane.right() - 4.0 {
        x = at.x - gap - size.x;
    }
    let mut y = at.y + gap;
    if y + size.y > pane.bottom() - 4.0 {
        y = at.y - gap - size.y;
    }
    Pos2::new(x.max(pane.left() + 4.0), y.max(pane.top() + 4.0))
}

/// The actions a corner offers, in a row by it: one click does one, and
/// Esc or a click elsewhere closes it.
fn corner_palette(
    doc: &mut Document,
    ctx: &egui::Context,
    p: Palette,
    pane: egui::Rect,
) -> Option<String> {
    let mut pick = None;
    let shown = egui::Area::new(egui::Id::new("corner.palette"))
        .order(egui::Order::Foreground)
        .fixed_pos(palette_at(
            p.at,
            pane,
            ctx.memory(|m| {
                m.area_rect(egui::Id::new("corner.palette"))
                    .map(|r| r.size())
            }),
        ))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (label, tip, what) in [
                        (
                            "\u{21ba} 90\u{b0}",
                            "Turn 90\u{b0} anticlockwise about its middle",
                            Pick::Turn(90.0),
                        ),
                        (
                            "\u{21bb} 90\u{b0}",
                            "Turn 90\u{b0} clockwise about its middle",
                            Pick::Turn(-90.0),
                        ),
                        ("180\u{b0}", "Turn it round", Pick::Turn(180.0)),
                        (
                            "Mirror",
                            "Mirror it: across its own centre line, or a line picked",
                            Pick::Mirror,
                        ),
                        (
                            "Scale",
                            "Scale it by a factor typed, or by a reference",
                            Pick::Scale,
                        ),
                    ] {
                        if ui.button(label).on_hover_text(tip).clicked() {
                            pick = Some(what);
                        }
                    }
                });
            });
        });
    let now = ctx.input(|i| i.time);
    let away = ctx.input(|i| i.key_pressed(egui::Key::Escape))
        || (now > p.opened + 0.05
            && ctx.input(|i| i.pointer.any_click())
            && !shown.response.hovered()
            && pick.is_none());
    if away {
        set_palette(None);
        return None;
    }
    let what = pick?;
    set_palette(None);
    palette_pick(doc, p.id, what)
}

#[cfg(test)]
mod turn_dot_tests {
    use super::*;

    /// The turn grip is the round dot: one ring the size of a square grip,
    /// white inside, nothing drawn in it (owner, MU142: the ring with an
    /// arrow in it read as a ring with a square; squares stretch, dots
    /// turn).
    #[test]
    fn the_turn_grip_is_a_round_dot_the_size_of_a_square_grip() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            let painter = ui.ctx().layer_painter(egui::LayerId::background());
            let look = GripLook::of(ui.ctx());
            draw_turn_grip(
                &painter,
                Pos2::new(50.0, 50.0),
                None,
                GripState::Rest,
                &look,
            );
        });
        out.textures_delta.clear();
        assert_eq!(out.shapes.len(), 1, "one shape, no arrow in it");
        match &out.shapes[0].shape {
            egui::Shape::Circle(c) => {
                assert!(c.radius <= 5.0, "a dot, not a ring: {}", c.radius);
                assert_eq!(c.fill, Color32::WHITE);
            }
            s => panic!("not a dot: {s:?}"),
        }
    }

    /// A turn-dot drag steps and locks to 0/90/180 with Grid snap Off (the
    /// length step 0); only Alt frees it (MU142: the log read "step 0.0
    /// mm" and the drag never locked).
    #[test]
    fn a_dot_drag_locks_with_grid_snap_off_and_alt_frees_it() {
        GRIP_FREE.store(false, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(grip_turn(88.2, 0.0), (90.0, Some(90.0)));
        assert_eq!(grip_turn(37.4, 0.0).0, 37.0);
        GRIP_FREE.store(true, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(grip_turn(88.2, 0.0), (88.2, None));
        GRIP_FREE.store(false, std::sync::atomic::Ordering::Relaxed);
        // and Mirror has an icon of its own (MU142)
        assert!(crate::svgicon::has("mirror"));
    }

    /// Held, the dot stays white in its halo, as a held square does (MU142:
    /// the dot filled solid while a square stayed white).
    #[test]
    fn a_held_turn_dot_is_white_in_a_halo_as_a_held_square_is() {
        let ctx = egui::Context::default();
        let cores = |dot: bool| {
            let mut out = ctx.run_ui(Default::default(), |ui| {
                let painter = ui.ctx().layer_painter(egui::LayerId::background());
                let look = GripLook::of(ui.ctx());
                let c = Pos2::new(50.0, 50.0);
                if dot {
                    draw_turn_grip(&painter, c, None, GripState::Held, &look);
                } else {
                    draw_grip(&painter, c, None, GripState::Held, false, false, &look);
                }
            });
            out.textures_delta.clear();
            out.shapes.into_iter().map(|s| s.shape).collect::<Vec<_>>()
        };
        let dot = cores(true);
        // a halo first, then the dot, white
        assert!(dot.len() >= 2, "{dot:?}");
        match dot.last() {
            Some(egui::Shape::Circle(c)) => assert_eq!(c.fill, Color32::WHITE, "held dot"),
            s => panic!("not a dot: {s:?}"),
        }
        let square = cores(false);
        assert!(
            matches!(&square[0], egui::Shape::Circle(c) if c.radius > 10.0),
            "the square's halo"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use creabim_core::sample::sample_house;

    /// A layered wall of the typed house, and the index of its core layer.
    fn layered_wall(m: &Model) -> (ElementId, usize) {
        m.iter()
            .find_map(|e| {
                let ElementKind::Wall(w) = &e.kind else {
                    return None;
                };
                let (_, a) = creabim_core::assembly::assembly_of(m, e)?;
                let bands = creabim_core::assembly::wall_bands(a, w);
                (bands.len() > 1)
                    .then(|| bands.iter().find(|b| b.core).map(|b| (e.id, b.index)))
                    .flatten()
            })
            .expect("a layered wall")
    }

    #[test]
    fn drawing_by_a_layer_boundary_keeps_the_wall_where_it_is() {
        let m = creabim_core::sample::typed_house();
        let (id, core) = layered_wall(&m);
        let mut doc = Document::from_model(m);
        let w = |doc: &Document| match &doc.model().get(id).unwrap().kind {
            ElementKind::Wall(w) => w.clone(),
            _ => unreachable!(),
        };
        let before = w(&doc);
        let k = (core + 1) as u8;
        let text = draw_by_boundary(&mut doc, id, k).unwrap();
        let after = w(&doc);
        assert_eq!(after.location, creabim_core::LocationLine::Boundary(k));
        assert_eq!((after.start, after.end), (before.start, before.end));
        assert!(text.contains("stays where it is"), "{text}");
        // the line drawn by is that boundary, on the core's interior face
        let off = creabim_core::walls::offset_of(doc.model(), &after, after.location);
        let want = creabim_core::walls::boundary_offset(doc.model(), &after, k as usize).unwrap();
        assert!((off - want).abs() < 1e-9);
        assert!(off.abs() < after.thickness / 2.0);
        let core_in = creabim_core::LocationLine::CoreInterior;
        let core_in = creabim_core::walls::offset_of(doc.model(), &after, core_in);
        assert!((off - core_in).abs() < 1e-9, "{off} vs {core_in}");
        // asking again says so
        assert!(draw_by_boundary(&mut doc, id, k).is_err());
    }

    #[test]
    fn a_layer_end_runs_on_by_its_grip_keeps_through_a_move_and_resets() {
        let m = creabim_core::sample::typed_house();
        let (id, core) = layered_wall(&m);
        assert!(has_layers(&m, id));
        let mut doc = Document::from_model(m);
        let kind = HandleKind::LayerEnd {
            layer: core,
            end: true,
        };
        let h = layer_handles(doc.model(), id)
            .into_iter()
            .find(|h| h.kind == kind)
            .unwrap();
        let w = |doc: &Document| match &doc.model().get(id).unwrap().kind {
            ElementKind::Wall(w) => w.clone(),
            _ => unreachable!(),
        };
        let dir = w(&doc).direction().extend(0.0);
        // 20 mm on past the end
        let (orig, joined) = originals(doc.model(), id, kind);
        let model = doc.model().clone();
        let out = apply(&model, &orig, &joined, kind, h.at, h.at + dir * 0.02, 0.0).unwrap();
        let new = out[0].clone();
        doc.edit(id, "drag", |e| *e = new.clone()).unwrap();
        assert!((w(&doc).layer_ext(core)[1] - 0.02).abs() < 1e-9);
        // its grip moved with it, and shows filled
        let moved = layer_handles(doc.model(), id)
            .into_iter()
            .find(|h| h.kind == kind)
            .unwrap();
        assert!((moved.at - h.at).dot(dir) > 0.019);
        assert!(edited_end(doc.model().get(id), kind));
        // the other layers and the start are as they were
        assert_eq!(w(&doc).layer_ext(core)[0], 0.0);
        // moved, the wall keeps its edit
        doc.edit(id, "move", |e| {
            if let ElementKind::Wall(w) = &mut e.kind {
                w.start += DVec2::new(1.0, 0.5);
                w.end += DVec2::new(1.0, 0.5);
            }
        })
        .unwrap();
        assert!((w(&doc).layer_ext(core)[1] - 0.02).abs() < 1e-9);
        // and resetting puts it back
        reset_layer_ends(&mut doc, id, Some(kind)).unwrap();
        assert!(w(&doc).layer_ends.is_empty());
        assert!(reset_layer_ends(&mut doc, id, None).is_err());
    }

    #[test]
    fn a_wall_layer_runs_down_past_the_base_by_its_bottom_grip() {
        let m = creabim_core::sample::typed_house();
        let (id, core) = layered_wall(&m);
        // an outer layer: the one that runs down past the slab's edge
        let outer = if core == 0 { 1 } else { 0 };
        let mut doc = Document::from_model(m);
        let kind = HandleKind::LayerHeight {
            layer: outer,
            top: false,
        };
        let h = layer_handles(doc.model(), id)
            .into_iter()
            .find(|h| h.kind == kind)
            .expect("a bottom grip on every layer");
        let Constraint::Line { dir, .. } = h.constraint else {
            panic!()
        };
        assert_eq!(dir, DVec3::Z, "it slides up and down");
        let (orig, joined) = originals(doc.model(), id, kind);
        let model = doc.model().clone();
        let out = apply(
            &model,
            &orig,
            &joined,
            kind,
            h.at,
            h.at - DVec3::Z * 0.5,
            0.0,
        )
        .unwrap();
        let new = out[0].clone();
        doc.edit(id, "drag", |e| *e = new.clone()).unwrap();
        let ElementKind::Wall(w) = &doc.model().get(id).unwrap().kind else {
            panic!()
        };
        assert!((w.layer_vext(outer)[0] - 0.5).abs() < 1e-9);
        assert_eq!(w.layer_vext(core), [0.0, 0.0], "the core stays on the slab");
        // its solid goes 500 mm lower; the core's does not
        let low = |layer: usize| {
            creabim_geom::scene::build_scene_of(doc.model(), Some(&[id].into_iter().collect()))
                .iter()
                .filter(|i| i.layer.is_some_and(|l| l.index == layer))
                .flat_map(|i| i.mesh.positions.iter().map(|p| p[2] as f64))
                .fold(f64::MAX, f64::min)
        };
        assert!(
            (low(core) - low(outer) - 0.5).abs() < 1e-4,
            "{} {}",
            low(core),
            low(outer)
        );
        // the grip follows, and Reset puts it back
        let moved = layer_handles(doc.model(), id)
            .into_iter()
            .find(|h| h.kind == kind)
            .unwrap();
        assert!((h.at.z - moved.at.z - 0.5).abs() < 1e-9);
        reset_layer_ends(&mut doc, id, Some(kind)).unwrap();
        assert!(!edited_end(doc.model().get(id), kind));
    }

    #[test]
    fn slab_and_roof_layer_edges_push_out_by_their_grips() {
        let m = creabim_core::sample::typed_house();
        let layered = |f: fn(&ElementKind) -> bool| {
            m.iter()
                .find(|e| f(&e.kind) && has_layers(&m, e.id))
                .map(|e| e.id)
        };
        let slab = layered(|k| matches!(k, ElementKind::Slab(_))).expect("a layered slab");
        let roof = layered(|k| matches!(k, ElementKind::Roof(_)));
        for id in std::iter::once(slab).chain(roof) {
            let mut doc = Document::from_model(m.clone());
            let hs = layer_handles(doc.model(), id);
            let h = hs[0];
            assert!(matches!(
                h.kind,
                HandleKind::LayerEdge { layer: 0, edge: 0 }
            ));
            // the next layer's grip on the same edge is drawn further out
            assert!(hs.iter().any(|g| g.nudge > 0.0));
            let Constraint::Line { dir, .. } = h.constraint else {
                panic!()
            };
            let (orig, joined) = originals(doc.model(), id, h.kind);
            let model = doc.model().clone();
            let out = apply(&model, &orig, &joined, h.kind, h.at, h.at + dir * 0.05, 0.0).unwrap();
            let new = out[0].clone();
            doc.edit(id, "drag", |e| *e = new.clone()).unwrap();
            let e = doc.model().get(id).unwrap();
            assert!((layer_offset(&e.kind, h.kind).unwrap() - 0.05).abs() < 1e-9);
            assert!(edited_end(Some(e), h.kind));
            let moved = layer_handles(doc.model(), id)[0];
            assert!((moved.at - h.at).truncate().dot(dir.truncate()) > 0.049);
            reset_layer_ends(&mut doc, id, None).unwrap();
            assert!(!edited_end(doc.model().get(id), h.kind));
        }
    }

    #[test]
    fn a_layer_end_held_back_goes_the_other_way_at_the_start() {
        let m = creabim_core::sample::typed_house();
        let (id, core) = layered_wall(&m);
        let kind = HandleKind::LayerEnd {
            layer: core,
            end: false,
        };
        let h = layer_handles(&m, id)
            .into_iter()
            .find(|h| h.kind == kind)
            .unwrap();
        let ElementKind::Wall(w) = &m.get(id).unwrap().kind else {
            unreachable!()
        };
        // dragged back along the wall from its start: held back 50 mm
        let into = w.direction().extend(0.0) * 0.05;
        let (orig, joined) = originals(&m, id, kind);
        let out = apply(&m, &orig, &joined, kind, h.at, h.at + into, 0.0).unwrap();
        let ElementKind::Wall(w) = &out[0].kind else {
            unreachable!()
        };
        assert!((w.layer_ext(core)[0] + 0.05).abs() < 1e-9);
    }

    /// How far a wall's location line sits off its centreline.
    fn walls_offset(m: &Model, w: &creabim_core::Wall) -> f64 {
        let (a, _) = walls::location_line(m, w);
        (a - w.start).dot(w.direction().perp()).abs()
    }

    fn wall_with_door(m: &Model) -> (ElementId, ElementId) {
        m.iter()
            .find_map(|e| match &e.kind {
                ElementKind::Opening(o) if o.kind == OpeningKind::Door => Some((o.host, e.id)),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn detail_components_have_points_a_width_and_a_unit_to_drag() {
        use creabim_core::{DetailItem, View};
        let mut doc = Document::new();
        let section = Element::new(
            "Section",
            ElementKind::View(View::section(DVec2::ZERO, DVec2::new(5.0, 0.0), 3.0, true)),
        );
        let sid = section.id;
        let p = DVec2::new;
        let mut batt = DetailItem::new(
            sid,
            DetailKind::Repeat {
                module: 0.215,
                joint: 0.01,
            },
            vec![p(0.0, 0.0), p(2.0, 0.0)],
        );
        batt.width = 0.1;
        let d = Element::new("Brick", ElementKind::Detail(batt));
        let did = d.id;
        doc.commit(
            creabim_core::Transaction::new("setup")
                .insert(section)
                .insert(d),
        )
        .unwrap();
        let m = doc.model().clone();
        let hs = handles_in(&m, did, None, false);
        let kinds: Vec<HandleKind> = hs.iter().map(|h| h.kind).collect();
        assert!(kinds.contains(&HandleKind::Vertex {
            ring: Ring::Outline,
            i: 1
        }));
        assert!(kinds.contains(&HandleKind::DetailWidth));
        assert!(kinds.contains(&HandleKind::DetailSpacing));
        let get = |k| *hs.iter().find(|h| h.kind == k).unwrap();
        let drag = |h: Handle, by: DVec3| {
            let (orig, joined) = originals(&m, did, h.kind);
            let out = apply(&m, &orig, &joined, h.kind, h.at, h.at + by, 0.0).unwrap();
            match &out[0].kind {
                ElementKind::Detail(d) => d.clone(),
                _ => panic!(),
            }
        };
        // the section looks along +Y: up on its paper is +Z in the model
        let w = drag(get(HandleKind::DetailWidth), DVec3::Z * 0.05);
        assert!((w.width - 0.2).abs() < 1e-9);
        let u = drag(get(HandleKind::DetailSpacing), DVec3::X * 0.1);
        assert!(
            matches!(u.kind, DetailKind::Repeat { module, .. } if (module - 0.315).abs() < 1e-9)
        );
        let end = get(HandleKind::Vertex {
            ring: Ring::Outline,
            i: 1,
        });
        let e = drag(end, DVec3::new(0.5, 0.0, 1.0));
        assert!(e.points[1].distance(p(2.5, 1.0)) < 1e-9);
        // a plant: its crown and its height
        let mut doc = Document::from_model(creabim_core::sample::sample_house());
        let lid = doc
            .model()
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Level(_)))
            .unwrap()
            .id;
        let tree = Element::new(
            "Tree",
            ElementKind::Site(creabim_core::Site::plant(
                lid,
                p(20.0, 20.0),
                creabim_core::PlantForm::Tree,
                4.0,
                6.0,
            )),
        );
        let tid = tree.id;
        doc.commit(creabim_core::Transaction::new("tree").insert(tree))
            .unwrap();
        let m = doc.model().clone();
        let hs = handles(&m, tid, None);
        let canopy = *hs
            .iter()
            .find(|h| h.kind == HandleKind::PlantCanopy)
            .unwrap();
        let (orig, _) = originals(&m, tid, canopy.kind);
        let out = apply(
            &m,
            &orig,
            &[],
            canopy.kind,
            canopy.at,
            canopy.at + DVec3::X * 0.5,
            0.0,
        )
        .unwrap();
        assert!(matches!(
            out[0].kind,
            ElementKind::Site(creabim_core::Site {
                form: creabim_core::SiteForm::Plant { canopy, .. },
                ..
            }) if (canopy - 5.0).abs() < 1e-9
        ));
        assert!(hs.iter().any(|h| h.kind == HandleKind::PlantHeight));
    }

    #[test]
    fn a_grown_trees_side_pulls_in_off_a_wall_and_its_trunk_stays() {
        let mut doc = Document::from_model(creabim_core::sample::sample_house());
        let lid = doc
            .model()
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Level(_)))
            .unwrap()
            .id;
        let shape = creabim_core::TreeShape {
            depth: 6.0,
            ..creabim_core::TreeShape::default()
        };
        let site = creabim_core::Site::tree(lid, DVec2::new(20.0, 20.0), shape, 6.0, 8.0);
        let tree = Element::new("Tree", ElementKind::Site(site));
        let tid = tree.id;
        doc.commit(creabim_core::Transaction::new("tree").insert(tree))
            .unwrap();
        let m = doc.model().clone();
        let hs = handles(&m, tid, None);
        assert_eq!(hs.len(), 10, "eight sides, the top and the clear trunk");
        let east = *hs
            .iter()
            .find(|h| h.kind == HandleKind::PlantReach(0))
            .unwrap();
        assert!((east.at.x - 23.0).abs() < 1e-9);
        // pulled 1.8 m in: the crown's east edge is 1.2 m from the trunk
        let (orig, _) = originals(&m, tid, east.kind);
        let out = apply(
            &m,
            &orig,
            &[],
            east.kind,
            east.at,
            east.at - DVec3::X * 1.8,
            0.0,
        )
        .unwrap();
        let ElementKind::Site(s) = &out[0].kind else {
            panic!()
        };
        let t = s.tree_shape().unwrap();
        assert!((t.reach[0] - 0.4).abs() < 1e-9);
        assert!((t.reach[4] - 1.0).abs() < 1e-9, "the west side stays");
        assert!(
            matches!(s.form, creabim_core::SiteForm::Plant { position, .. } if position == DVec2::new(20.0, 20.0))
        );
        // and the clear trunk can't pass the top
        let base = *hs
            .iter()
            .find(|h| h.kind == HandleKind::PlantCrownBase)
            .unwrap();
        let out = apply(
            &m,
            &orig,
            &[],
            base.kind,
            base.at,
            base.at + DVec3::Z * 20.0,
            0.0,
        )
        .unwrap();
        let ElementKind::Site(s) = &out[0].kind else {
            panic!()
        };
        assert!(s.validate().is_ok());
    }

    #[test]
    fn a_callouts_box_resizes_with_its_grips_and_moves_with_its_body() {
        use creabim_core::View;
        let mut doc = Document::new();
        let p = DVec2::new;
        let section = View::section(DVec2::ZERO, DVec2::new(5.0, 0.0), 3.0, true);
        let s = Element::new("Section", ElementKind::View(section.clone()));
        let sid = s.id;
        let d = Element::new(
            "Detail 1",
            ElementKind::View(View::callout(sid, &section, p(1.0, 0.0), p(2.0, 1.0), 10.0)),
        );
        let did = d.id;
        doc.commit(creabim_core::Transaction::new("setup").insert(s).insert(d))
            .unwrap();
        let m = doc.model().clone();
        let hs = handles(&m, did, None);
        // four corners and four edges, none of the section line's
        assert_eq!(hs.len(), 8);
        assert!(hs
            .iter()
            .all(|h| !matches!(h.kind, HandleKind::ViewEnd { .. })));
        let get = |k| *hs.iter().find(|h| h.kind == k).unwrap();
        let drag = |h: Handle, by: DVec3| {
            let (orig, joined) = originals(&m, did, h.kind);
            let out = apply(&m, &orig, &joined, h.kind, h.at, h.at + by, 0.0).unwrap();
            match &out[0].kind {
                ElementKind::View(v) => v.callout.unwrap(),
                _ => panic!(),
            }
        };
        // on a section it is a ring: pushing the top up widens it about its middle
        let c = drag(get(HandleKind::CalloutEdge(2)), DVec3::Z * 0.5);
        assert!(c.lo.distance(p(0.75, 0.0)) < 1e-9 && c.hi.distance(p(2.25, 1.5)) < 1e-9);
        // a corner: the opposite one stays
        let c = drag(get(HandleKind::CalloutCorner(2)), DVec3::new(0.5, 0.0, 0.5));
        assert!(c.lo.distance(p(1.0, 0.0)) < 1e-9 && c.hi.distance(p(2.5, 1.5)) < 1e-9);
        // its body moves the box, not the line its view was cut on
        let moved = transform::transformed(
            &m,
            m.get(did).unwrap(),
            &Xform::Move(DVec3::new(0.25, 0.0, 0.5)),
        )
        .unwrap();
        let ElementKind::View(v) = moved.kind else {
            panic!()
        };
        assert!(v.callout.unwrap().lo.distance(p(1.25, 0.5)) < 1e-9);
        assert_eq!(v.kind, section.kind);
    }

    #[test]
    fn a_line_constraint_takes_the_point_nearest_the_ray() {
        let c = Constraint::Line {
            point: DVec3::ZERO,
            dir: DVec3::Z,
        };
        let r = Ray {
            o: DVec3::new(5.0, 0.0, 2.0),
            d: -DVec3::X,
        };
        assert!(c.hit(r).unwrap().distance(DVec3::new(0.0, 0.0, 2.0)) < 1e-9);
        assert!(!c.usable(DVec3::Z));
    }

    #[test]
    fn dragging_a_door_jamb_widens_it_and_the_head_raises_it() {
        let m = sample_house();
        let (_, door) = wall_with_door(&m);
        let hs = handles(&m, door, None);
        let right = hs
            .iter()
            .find(|h| h.kind == HandleKind::OpeningJamb { right: true })
            .unwrap();
        let ElementKind::Opening(o) = &m.get(door).unwrap().kind else {
            panic!()
        };
        let Constraint::Line { dir, .. } = right.constraint else {
            panic!()
        };
        let (orig, joined) = originals(&m, door, right.kind);
        let out = apply(
            &m,
            &orig,
            &joined,
            right.kind,
            right.at,
            right.at + dir * 0.2,
            0.05,
        )
        .unwrap();
        let ElementKind::Opening(n) = &out[0].kind else {
            panic!()
        };
        assert!((n.width - (o.width + 0.2)).abs() < 1e-9);
        assert!((n.offset - o.offset).abs() < 1e-9);
        let head = hs
            .iter()
            .find(|h| h.kind == HandleKind::OpeningHead)
            .unwrap();
        let out = apply(
            &m,
            &orig,
            &[],
            head.kind,
            head.at,
            head.at + DVec3::Z * 0.1,
            0.05,
        )
        .unwrap();
        let ElementKind::Opening(n) = &out[0].kind else {
            panic!()
        };
        assert!((n.height - (o.height + 0.1)).abs() < 1e-9);
        // far too tall for the wall
        assert!(apply(
            &m,
            &orig,
            &[],
            head.kind,
            head.at,
            head.at + DVec3::Z * 5.0,
            0.05
        )
        .is_err());
    }

    #[test]
    fn stretching_a_wall_end_takes_the_joined_wall_along() {
        let m = sample_house();
        let (id, w) = m
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::Wall(w) => Some((e.id, w.clone())),
                _ => None,
            })
            .unwrap();
        let kind = HandleKind::WallEnd { end: true };
        let (orig, joined) = originals(&m, id, kind);
        assert!(!joined.is_empty(), "sample walls meet at corners");
        let to = (w.end + w.direction() * 0.5).extend(0.0);
        let out = apply(&m, &orig, &joined, kind, w.end.extend(0.0), to, 0.05).unwrap();
        for &(i, oe) in &joined {
            let ElementKind::Wall(o) = &out[i].kind else {
                panic!()
            };
            let p = if oe { o.end } else { o.start };
            assert!(p.distance(to.truncate()) < 1e-9);
        }
    }

    #[test]
    fn a_wall_end_is_gripped_on_its_location_line_and_may_be_left_unjoined() {
        let m = sample_house();
        let (id, mut w) = m
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::Wall(w) => Some((e.id, w.clone())),
                _ => None,
            })
            .unwrap();
        w.location = creabim_core::LocationLine::ExteriorFace;
        w.disallow_join[1] = true;
        let mut doc = Document::from_model(m);
        doc.edit(id, "edit", |e| e.kind = ElementKind::Wall(w.clone()))
            .unwrap();
        let m = doc.model().clone();
        let kind = HandleKind::WallEnd { end: true };
        let grip = handles(&m, id, None)
            .into_iter()
            .find(|h| h.kind == kind)
            .unwrap();
        let (_, b) = walls::location_line(&m, &w);
        assert!(
            grip.at.truncate().distance(b) < 1e-9,
            "on the exterior face"
        );
        assert!(b.distance(w.end) > 0.05);
        // the end may not join: nothing comes along
        let (orig, joined) = originals(&m, id, kind);
        assert!(joined.is_empty() && orig.len() == 1);
        let to = (b + w.direction() * 0.5).extend(0.0);
        let out = apply(&m, &orig, &joined, kind, grip.at, to, 0.05).unwrap();
        let ElementKind::Wall(n) = &out[0].kind else {
            panic!()
        };
        let (_, nb) = walls::location_line(&m, n);
        assert!(nb.distance(to.truncate()) < 1e-9);
    }

    #[test]
    fn pulling_a_slab_underside_makes_it_thicker() {
        let m = sample_house();
        let (id, t) = m
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::Slab(s) => Some((e.id, s.thickness)),
                _ => None,
            })
            .unwrap();
        let h = handles(&m, id, None)
            .into_iter()
            .find(|h| h.kind == HandleKind::SlabUnderside)
            .unwrap();
        let (orig, _) = originals(&m, id, h.kind);
        let out = apply(&m, &orig, &[], h.kind, h.at, h.at - DVec3::Z * 0.05, 0.05).unwrap();
        let ElementKind::Slab(s) = &out[0].kind else {
            panic!()
        };
        assert!((s.thickness - (t + 0.05)).abs() < 1e-9);
        // the top stayed at the level
        assert!(s.top_offset.abs() < 1e-12);
    }

    #[test]
    fn pulling_a_slab_top_moves_the_top_and_keeps_the_underside() {
        let m = sample_house();
        let (id, t) = m
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::Slab(s) => Some((e.id, s.thickness)),
                _ => None,
            })
            .unwrap();
        let hs = handles(&m, id, None);
        let top = hs.iter().find(|h| h.kind == HandleKind::SlabTop).unwrap();
        let under = hs
            .iter()
            .find(|h| h.kind == HandleKind::SlabUnderside)
            .unwrap();
        assert!((top.at.z - under.at.z - t).abs() < 1e-9);
        let (orig, _) = originals(&m, id, top.kind);
        let out = apply(
            &m,
            &orig,
            &[],
            top.kind,
            top.at,
            top.at + DVec3::Z * 0.1,
            0.05,
        )
        .unwrap();
        let ElementKind::Slab(s) = &out[0].kind else {
            panic!()
        };
        assert!((s.thickness - (t + 0.1)).abs() < 1e-9);
        assert!((s.top_offset - 0.1).abs() < 1e-9);
        assert!((s.top_offset - s.thickness + t).abs() < 1e-9);
    }

    #[test]
    fn slab_corners_can_be_added_on_an_edge_and_taken_away() {
        let m = creabim_core::sample::sample_house();
        let (id, n) = m
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::Slab(s) => Some((e.id, s.boundary.len())),
                _ => None,
            })
            .unwrap();
        let mut doc = Document::from_model(m);
        use crate::editmode::{add_corner, remove_corners};
        add_corner(&mut doc, id, Ring::Outline, 0, None).unwrap();
        let corners = |doc: &Document| match &doc.model().get(id).unwrap().kind {
            ElementKind::Slab(s) => s.boundary.clone(),
            _ => panic!(),
        };
        let b = corners(&doc);
        assert_eq!(b.len(), n + 1);
        assert!(b[1].distance((b[0] + b[2]) / 2.0) < 1e-9);
        remove_corners(&mut doc, id, &[(Ring::Outline, 1)]).unwrap();
        assert_eq!(corners(&doc).len(), n);
        while corners(&doc).len() > 3 {
            remove_corners(&mut doc, id, &[(Ring::Outline, 0)]).unwrap();
        }
        assert!(remove_corners(&mut doc, id, &[(Ring::Outline, 0)]).is_err());
    }

    #[test]
    fn plan_handles_slide_along_a_section() {
        let m = sample_house();
        let (id, _) = wall_with_door(&m);
        // a section looking north: wall ends slide east and west
        let look = DVec3::Y;
        let end = handles(&m, id, None)
            .into_iter()
            .find(|h| matches!(h.kind, HandleKind::WallEnd { .. }))
            .unwrap();
        assert!(!end.constraint.usable(look));
        let h = in_section(end, look);
        assert!(h.constraint.usable(look));
        let Constraint::Line { dir, .. } = h.constraint else {
            panic!()
        };
        assert!(dir.dot(look).abs() < 1e-9 && dir.z.abs() < 1e-9);
        // a ray straight into the paper lands on the line, at the pointer
        let ray = Ray {
            o: DVec3::new(3.0, -50.0, end.at.z),
            d: look,
        };
        let p = h.constraint.hit(ray).unwrap();
        assert!((p.x - 3.0).abs() < 1e-9);
        // plans keep the handle as it was
        assert_eq!(in_section(end, -DVec3::Z), end);
    }

    fn a_slab(doc: &Document) -> (ElementId, Vec<DVec2>) {
        doc.model()
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::Slab(s) => Some((e.id, s.boundary.clone())),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn a_double_click_on_an_add_corner_grip_changes_nothing() {
        let mut doc = Document::from_model(sample_house());
        let (id, before) = a_slab(&doc);
        let mut sel = GripSel::default();
        sel.reset(Some(id));
        let k = HandleKind::AddVertex {
            ring: Ring::Outline,
            i: 0,
        };
        let p = Pos2::new(100.0, 100.0);
        for t in [0.0, 0.2] {
            tap_grip(&mut doc, id, k, true, false, t, p, &mut sel);
        }
        assert_eq!(a_slab(&doc).1, before);
        assert!(!doc.can_undo(), "no undo step at all");
    }

    #[test]
    fn a_double_click_on_a_corner_takes_it_away_even_after_a_small_move() {
        let mut doc = Document::from_model(sample_house());
        let (id, _) = a_slab(&doc);
        doc.edit(id, "five", |e| {
            if let ElementKind::Slab(s) = &mut e.kind {
                s.split_edge(0);
            }
        })
        .unwrap();
        let n = a_slab(&doc).1.len();
        let mut sel = GripSel::default();
        sel.reset(Some(id));
        let k = HandleKind::Vertex {
            ring: Ring::Outline,
            i: 1,
        };
        // the first click picks it
        tap_grip(
            &mut doc,
            id,
            k,
            true,
            false,
            1.0,
            Pos2::new(50.0, 50.0),
            &mut sel,
        );
        assert!(sel.picked(Ring::Outline, 1));
        assert_eq!(a_slab(&doc).1.len(), n);
        // the second, three pixels off, takes it away
        tap_grip(
            &mut doc,
            id,
            k,
            true,
            false,
            1.3,
            Pos2::new(53.0, 50.0),
            &mut sel,
        );
        assert_eq!(a_slab(&doc).1.len(), n - 1);
        // clicks far apart in time only pick
        tap_grip(
            &mut doc,
            id,
            k,
            true,
            false,
            5.0,
            Pos2::new(50.0, 50.0),
            &mut sel,
        );
        tap_grip(
            &mut doc,
            id,
            k,
            true,
            false,
            6.0,
            Pos2::new(50.0, 50.0),
            &mut sel,
        );
        assert_eq!(a_slab(&doc).1.len(), n - 1);
        // Shift+click adds to the picks, and again takes away
        let other = HandleKind::Vertex {
            ring: Ring::Outline,
            i: 0,
        };
        tap_grip(
            &mut doc,
            id,
            other,
            true,
            true,
            8.0,
            Pos2::new(0.0, 0.0),
            &mut sel,
        );
        assert_eq!(sel.corners.len(), 2);
        tap_grip(
            &mut doc,
            id,
            other,
            true,
            true,
            9.0,
            Pos2::new(0.0, 0.0),
            &mut sel,
        );
        assert_eq!(sel.corners.len(), 1);
        // outside Edit mode a double-click on a corner asks for Edit mode
        let mut out = GripSel::default();
        tap_grip(&mut doc, id, k, false, false, 10.0, Pos2::ZERO, &mut out);
        // the picks are cleared every frame outside Edit mode; the click
        // must survive that
        out.clear();
        tap_grip(&mut doc, id, k, false, false, 10.2, Pos2::ZERO, &mut out);
        assert_eq!(out.request, Some(GripRequest::Enter));
        assert_eq!(a_slab(&doc).1.len(), n - 1);
    }

    #[test]
    fn delete_picked_removes_holes_or_corners_and_never_the_slab() {
        let mut doc = Document::from_model(sample_house());
        let (id, b) = a_slab(&doc);
        let c = b.iter().copied().sum::<DVec2>() / b.len() as f64;
        doc.edit(id, "hole", |e| {
            if let ElementKind::Slab(s) = &mut e.kind {
                s.holes = vec![creabim_core::polygon::rectangle(c, c + DVec2::ONE)];
            }
        })
        .unwrap();
        let mut sel = GripSel::default();
        sel.reset(Some(id));
        assert!(delete_picked(&mut doc, id, &mut sel).is_err());
        sel.pick_hole(0, false);
        delete_picked(&mut doc, id, &mut sel).unwrap();
        let ElementKind::Slab(s) = &doc.model().get(id).unwrap().kind else {
            panic!()
        };
        assert!(s.holes.is_empty());
        // a triangle keeps its corners
        sel.corners = (0..b.len()).map(|i| (Ring::Outline, i)).collect();
        assert!(delete_picked(&mut doc, id, &mut sel).is_err());
        assert!(doc.model().get(id).is_some());
    }

    fn drag_to(
        m: &Model,
        id: ElementId,
        kind: HandleKind,
        to: impl Fn(&Handle) -> DVec3,
    ) -> Element {
        let h = handles_in(m, id, None, false)
            .into_iter()
            .chain(handles_in(m, id, None, true))
            .find(|h| h.kind == kind)
            .unwrap_or_else(|| panic!("no {kind:?}"));
        let (orig, joined) = originals(m, id, kind);
        apply(m, &orig, &joined, kind, h.at, to(&h), 0.05).unwrap()[0].clone()
    }

    /// MU144 (owner, the Ridgeline's ground floor: "so we don't have the
    /// ability to move the stair tag into a better place?", then "just the
    /// stair tag bubble, we don't want to touch the stair arrow"): a stair's
    /// UP label has a grip of its own. Dragged, the label moves and the
    /// arrow stays; dragged back by its place, it goes home.
    #[test]
    fn a_stairs_up_label_moves_by_its_grip_and_its_arrow_stays() {
        use creabim_geom::plan::{Pen, PlanItem, PlanView};
        let m = creabim_core::ridgeline::ridgeline_house();
        let view = m.iter().find(|e| e.name == "Ground Floor plan").unwrap().id;
        let plan = |m: &Model| creabim_geom::section::view_drawing(m, view).unwrap();
        let first = plan(&m);
        let stair = first
            .items
            .iter()
            .find_map(|i| match i {
                PlanItem::Label { id, text, .. } if text == "UP" => Some(*id),
                _ => None,
            })
            .expect("a stair going up from the ground floor");
        let up = |d: &PlanView| {
            d.items.iter().find_map(|i| match i {
                PlanItem::Label { id, at, text } if *id == stair && text == "UP" => Some(*at),
                _ => None,
            })
        };
        let arrow = |d: &PlanView| {
            let mut v: Vec<[i64; 4]> = d
                .items
                .iter()
                .filter_map(|i| match i {
                    PlanItem::Line {
                        id,
                        pen: Pen::Symbol,
                        a,
                        b,
                    } if *id == stair => {
                        Some([a.x, a.y, b.x, b.y].map(|c| (c * 1e6).round() as i64))
                    }
                    _ => None,
                })
                .collect();
            v.sort();
            v
        };
        let home = up(&first).unwrap();
        let grip = handles_in(&m, stair, None, false)
            .into_iter()
            .find(|h| h.kind == HandleKind::StairUp)
            .expect("a grip on the UP label");
        assert!(
            grip.at.truncate().distance(home) < 1e-9,
            "the grip is on the label"
        );
        // off what it covered
        let by = DVec2::new(0.6, -0.4);
        let moved = drag_to(&m, stair, HandleKind::StairUp, |h| h.at + by.extend(0.0));
        let mut doc = creabim_core::Document::from_model(m.clone());
        doc.edit(stair, "drag", |e| *e = moved.clone()).unwrap();
        let after = plan(doc.model());
        assert!(
            up(&after).unwrap().distance(home + by) < 1e-6,
            "the label moved"
        );
        assert!(!arrow(&first).is_empty());
        assert_eq!(arrow(&after), arrow(&first), "the arrow stays");
        // back by its place: home, no offset kept
        let back = drag_to(doc.model(), stair, HandleKind::StairUp, |_| {
            (home + DVec2::new(0.02, 0.01)).extend(grip.at.z)
        });
        let ElementKind::Stair(s) = &back.kind else {
            unreachable!()
        };
        assert_eq!(s.up_offset, None);
        assert_eq!(HandleKind::StairUp.verb(), "Move the UP label of");
    }

    /// MU142 (Grip Palette REV01; Tool Rows and Scale REV01, A and 6): an
    /// object has a corner grip at each corner of its outline, shown only
    /// under the pointer; dragged, it scales the object from the opposite
    /// corner, which stays put, into a size of its own, its type as it was.
    #[test]
    fn an_objects_corner_scales_it_from_the_opposite_corner() {
        let m = creabim_core::starter::starter_house();
        let e = m
            .iter()
            .find(|e| object_corners(&m, e).is_some())
            .cloned()
            .expect("the starter house has furniture");
        let corners = object_corners(&m, &e).unwrap();
        let hs = handles_in(&m, e.id, None, false);
        for (i, c) in corners.iter().enumerate() {
            let h = hs
                .iter()
                .find(|h| h.kind == HandleKind::ScaleCorner(i as u8))
                .expect("a grip at each corner");
            assert!(h.at.truncate().distance(*c) < 1e-9);
        }
        // corner 2 taken half as far again along the diagonal
        let (c, opp) = (corners[2], corners[0]);
        let to = opp + (c - opp) * 1.5;
        let out = drag_to(&m, e.id, HandleKind::ScaleCorner(2), |h| to.extend(h.at.z));
        let now = object_corners(&m, &out).unwrap();
        assert!(
            now[0].distance(opp) < 1e-6,
            "the opposite corner stays: {} {}",
            now[0],
            opp
        );
        assert!(now[2].distance(to) < 1e-6, "{} {}", now[2], to);
        let ElementKind::Object(o) = &out.kind else {
            unreachable!()
        };
        assert!(
            (o.scale - 1.5).abs() < 1e-9,
            "a size of its own: {}",
            o.scale
        );
        assert_eq!(
            creabim_core::objects::type_of(&m, &out).map(|t| t.bounds()),
            creabim_core::objects::type_of(&m, &e).map(|t| t.bounds()),
            "its type as it was"
        );
    }

    /// MU142 (decision 10): resting on the turn dot shows the protractor
    /// round the object's middle, a step outside the dot, and a turn taken
    /// anywhere on that ring turns it by the angle swept, as from the dot.
    #[test]
    fn the_turn_ring_is_round_its_middle_and_turns_from_where_it_is_taken() {
        let m = creabim_core::starter::starter_house();
        let e = m
            .iter()
            .find(|e| object_corners(&m, e).is_some())
            .cloned()
            .unwrap();
        let k = 50.0;
        let project = move |p: DVec3| {
            Some(Pos2::new(
                (500.0 + k * p.x) as f32,
                (400.0 - k * p.y) as f32,
            ))
        };
        let ray = move |s: Pos2| {
            let o = DVec3::new((s.x as f64 - 500.0) / k, (400.0 - s.y as f64) / k, 100.0);
            Some(Ray { o, d: -DVec3::Z })
        };
        let snap = |_: Pos2, _: Option<DVec3>| None;
        let none = |_: Pos2, _: ElementId| None;
        let no_face = |_: Pos2| None;
        let input = PaneInput {
            ray: &ray,
            project: &project,
            look: -DVec3::Z,
            snap: &snap,
            grab_body: &none,
            grab_snap: &none,
            body_normal: DVec3::Z,
            view: None,
            face: &no_face,
        };
        let (c, r) = turn_ring(&m, &e, &input).unwrap();
        let mid = pivot_of(&m, &e).unwrap();
        assert!(
            c.distance(project(mid.extend(0.0)).unwrap()) < 0.01,
            "round its middle"
        );
        let dot = handles_in(&m, e.id, None, false)
            .into_iter()
            .find(|h| h.kind == HandleKind::Rotate)
            .unwrap();
        let d = c.distance(project(dot.at).unwrap());
        assert!(
            r >= d + RING_CLEAR - 0.01 && r >= 56.0,
            "a step outside the dot: {r} {d}"
        );
        // taken on the ring across from the dot, carried a quarter turn round
        let ElementKind::Object(o0) = &e.kind else {
            unreachable!()
        };
        let rr = r as f64 / k;
        let a = (dot.at.truncate() - mid).to_angle() + std::f64::consts::PI;
        let grab = (mid + DVec2::from_angle(a) * rr).extend(dot.at.z);
        let now = (mid + DVec2::from_angle(a + std::f64::consts::FRAC_PI_2) * rr).extend(dot.at.z);
        let (orig, joined) = originals(&m, e.id, HandleKind::Rotate);
        let out =
            apply(&m, &orig, &joined, HandleKind::Rotate, grab, now, 0.05).unwrap()[0].clone();
        let ElementKind::Object(o1) = &out.kind else {
            unreachable!()
        };
        let by = (o1.rotation - o0.rotation).rem_euclid(360.0);
        assert!((by - 90.0).abs() < 1e-6, "turned {by}");
    }

    /// MU142, seen in the app: a press on the protractor's ring in a plan
    /// moved the object, as the 3D pane, the pointer away from it, put the
    /// ring out on every frame. The ring belongs to the pane that lit it.
    #[test]
    fn a_lit_turn_ring_belongs_to_its_pane() {
        let (id, plan, other) = (ElementId::new(), egui::Id::new("plan"), egui::Id::new("3d"));
        set_ring_lit(Some((id, plan)));
        assert_eq!(ring_lit(plan), Some(id));
        assert_eq!(
            ring_lit(other),
            None,
            "another pane neither sees it nor puts it out"
        );
        set_ring_lit(None);
    }

    /// MU142, seen in the app: a corner's palette by a pane's right edge ran
    /// over the Properties panel. It opens on whichever side keeps it in.
    #[test]
    fn a_corners_palette_stays_inside_its_pane() {
        let pane = egui::Rect::from_min_max(Pos2::new(100.0, 100.0), Pos2::new(600.0, 500.0));
        let size = egui::vec2(300.0, 36.0);
        let inside = |at: Pos2| {
            let r = egui::Rect::from_min_size(palette_at(at, pane, Some(size)), size);
            pane.contains_rect(r)
        };
        for at in [
            Pos2::new(150.0, 150.0),
            Pos2::new(580.0, 150.0),
            Pos2::new(580.0, 490.0),
            Pos2::new(150.0, 490.0),
        ] {
            assert!(inside(at), "{at:?}");
        }
        // by the corner when there is room
        assert_eq!(
            palette_at(Pos2::new(150.0, 150.0), pane, Some(size)),
            Pos2::new(162.0, 162.0)
        );
        // and the edit it starts says what it does
        assert_eq!(HandleKind::ScaleCorner(0).verb(), "Scale");
    }

    /// MU142 (Grip Palette REV01): a corner's palette turns the object about
    /// its middle at once, and hands Mirror and Scale to the view to start.
    #[test]
    fn a_corners_palette_turns_it_and_hands_over_mirror_and_scale() {
        let mut doc = Document::from_model(creabim_core::starter::starter_house());
        let e = doc
            .model()
            .iter()
            .find(|e| object_corners(doc.model(), e).is_some())
            .cloned()
            .unwrap();
        let ElementKind::Object(o0) = &e.kind else {
            unreachable!()
        };
        let (r0, mid) = (o0.rotation, pivot_of(doc.model(), &e).unwrap());
        assert!(palette_pick(&mut doc, e.id, Pick::Turn(90.0)).is_some());
        let after = doc.model().get(e.id).unwrap().clone();
        let ElementKind::Object(o1) = &after.kind else {
            unreachable!()
        };
        assert!(
            (o1.rotation - (r0 + 90.0).rem_euclid(360.0)).abs() < 1e-9,
            "{} {}",
            o1.rotation,
            r0
        );
        assert!(
            pivot_of(doc.model(), &after).unwrap().distance(mid) < 1e-9,
            "about its middle"
        );
        let _ = take_palette_command();
        assert!(palette_pick(&mut doc, e.id, Pick::Scale).is_none());
        assert!(matches!(
            take_palette_command(),
            Some(crate::commands::Command::Scale)
        ));
        palette_pick(&mut doc, e.id, Pick::Mirror);
        assert!(matches!(
            take_palette_command(),
            Some(crate::commands::Command::Mirror)
        ));
    }

    #[test]
    fn side_bars_sit_at_the_middle_of_their_side_not_on_a_corner() {
        // A4-8: a bar that pushes a side across sits between the element's
        // bottom and top, so in a section it is half-way up the side
        let m = creabim_core::starter::starter_house();
        let scene = creabim_geom::scene::build_scene_of(&m, None);
        let mut checked = 0;
        for e in m.iter() {
            let (lo, hi) = scene
                .iter()
                .filter(|i| i.id == e.id)
                .flat_map(|i| i.mesh.positions.iter().map(|p| p[2] as f64))
                .fold((f64::MAX, f64::MIN), |(a, b), z| (a.min(z), b.max(z)));
            if lo >= hi {
                continue;
            }
            for h in handles_in(&m, e.id, None, false) {
                let Constraint::Line { dir, .. } = h.constraint else {
                    continue;
                };
                // a member's other grips are its real ends (a stair's run);
                // a slab's or roof's edge bars too (MU25)
                let side = matches!(
                    h.kind,
                    HandleKind::WallFace | HandleKind::BeamWidth | HandleKind::ColumnSize { .. }
                ) || (matches!(h.kind, HandleKind::Edge { .. })
                    && matches!(e.kind, ElementKind::Slab(_) | ElementKind::Roof(_)))
                    || (matches!(h.kind, HandleKind::Member { .. })
                        && matches!(e.kind, ElementKind::Footing(_)));
                if !side || dir.z.abs() > 0.1 {
                    continue;
                }
                checked += 1;
                let z = h.at.z;
                assert!(
                    z > lo + 1e-3 && z < hi - 1e-3,
                    "{} {:?} at z {z} on its edge ({lo}..{hi})",
                    e.name,
                    h.kind
                );
            }
        }
        assert!(checked > 0);
    }

    /// MU130: a click anywhere on a face of a shape in Edit mode picks it,
    /// even where another face's middle falls near it on screen. Seen
    /// slanting from far off, the top's and the bottom's middles of a
    /// shape lie 14 px apart; one of their grips was left out to keep the
    /// view clear, and a click on that face picked nothing ("Nothing
    /// picked"). Which one was left out hung on the order of the grips,
    /// so it came and went (it seemed to follow a Pull).
    #[test]
    fn a_click_on_any_face_picks_it_however_crowded_the_view() {
        let mut doc = creabim_core::Document::new();
        let mass = creabim_core::FreeMesh::new(
            None,
            DVec3::ZERO,
            creabim_core::editmesh::EditMesh::cuboid(DVec3::new(4.0, 2.0, 1.0)),
        );
        let e = Element::new("Mass", ElementKind::FreeMesh(mass));
        let id = e.id;
        doc.commit(creabim_core::Transaction::new("Mass").insert(e))
            .unwrap();
        // a parallel view looking north and down at 45 degrees, 20 px a metre
        let k = 20.0f64;
        let (cx, cy) = (500.0f64, 400.0f64);
        let look = DVec3::new(0.0, 1.0, -1.0).normalize();
        let right = DVec3::X;
        let up = right.cross(look).normalize() * -1.0;
        let up = if up.z < 0.0 { -up } else { up };
        let project = move |p: DVec3| {
            Some(Pos2::new(
                (cx + k * p.dot(right)) as f32,
                (cy - k * p.dot(up)) as f32,
            ))
        };
        let ray = move |s: Pos2| {
            let o = right * ((s.x as f64 - cx) / k) + up * ((cy - s.y as f64) / k) - look * 100.0;
            Some(Ray { o, d: look })
        };
        let snap = |_: Pos2, _: Option<DVec3>| None;
        let none = |_: Pos2, _: ElementId| None;
        let no_face = |_: Pos2| None;
        let input = PaneInput {
            ray: &ray,
            project: &project,
            look,
            snap: &snap,
            grab_body: &none,
            grab_snap: &none,
            body_normal: DVec3::Z,
            view: None,
            face: &no_face,
        };
        let m = match &doc.model().get(id).unwrap().kind {
            ElementKind::FreeMesh(m) => m.clone(),
            _ => unreachable!(),
        };
        let ctx = egui::Context::default();
        let area = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 800.0));
        let mut t = 0.0;
        let mut grips = GripSel::default();
        let mut drag = None;
        // the two faces this view sees: the top and the south one (east
        // and west are seen edge on)
        for want in [DVec3::Z, -DVec3::Y] {
            let f = (0..m.mesh.faces.len())
                .find(|f| m.mesh.face_normal(*f).dot(want) > 0.9)
                .unwrap();
            let mut verts = m.mesh.faces[f].clone();
            verts.sort_unstable();
            // halfway from the face's middle to a corner: clear of the
            // shape's own up-and-down grip over the top's middle, and of the
            // corners' and edges' grips
            let mid = m.mesh.face_center(f);
            let corner = m.mesh.vertices[m.mesh.faces[f][0] as usize];
            let c = m.origin + m.to_model(mid + (corner - mid) * 0.5);
            let at = project(c).unwrap();
            grips.clear();
            let mut status = None;
            for events in [
                vec![egui::Event::PointerMoved(at)],
                vec![egui::Event::PointerMoved(at)],
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                }],
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }],
            ] {
                t += 1.0 / 60.0;
                let raw = egui::RawInput {
                    screen_rect: Some(area),
                    events,
                    time: Some(t),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(raw, |ui| {
                    let response = ui.allocate_rect(area, egui::Sense::click_and_drag());
                    let painter = ui.painter_at(area);
                    interact(
                        &mut doc,
                        &painter,
                        &response,
                        Some(id),
                        None,
                        true,
                        &mut drag,
                        &mut grips,
                        &input,
                        &mut status,
                    );
                });
                out.textures_delta.clear();
            }
            let mut got = grips.mesh.clone();
            got.sort_unstable();
            assert_eq!(
                got, verts,
                "a click on the face looking {want} picked {got:?}: {status:?}"
            );
        }
    }

    /// An object's turn grip turns it in a plan, but a section or an
    /// elevation has none: the object turns about the upright only, which
    /// they see end on (MU142: the dial stood upright on the section's
    /// plane, and in 3D).
    #[test]
    fn an_objects_turn_grip_is_in_plans_not_in_sections() {
        let m = creabim_core::ridgeline::ridgeline_house();
        let chair = m
            .iter()
            .find(|e| e.name == "Dining chair" && matches!(e.kind, ElementKind::Object(_)))
            .unwrap()
            .id;
        let section = m
            .iter()
            .find(|e| matches!(&e.kind, ElementKind::View(v) if matches!(v.kind, creabim_core::ViewKind::Section { .. })))
            .unwrap()
            .id;
        let plan = m.iter().find(|e| e.name == "Ground Floor plan").unwrap().id;
        let turn = |sideways: bool| -> f64 {
            let mut doc = Document::from_model(m.clone());
            let before = match &doc.model().get(chair).unwrap().kind {
                ElementKind::Object(o) => o.rotation,
                _ => unreachable!(),
            };
            let k = 40.0f64;
            let rot = doc.model().get(chair).map(|e| match &e.kind {
                ElementKind::Object(o) => o.position,
                _ => unreachable!(),
            });
            let o = rot.unwrap();
            // the pane, centred on the chair: from the south, or from above
            let (look, right, up) = if sideways {
                (DVec3::Y, DVec3::X, DVec3::Z)
            } else {
                (-DVec3::Z, DVec3::X, DVec3::Y)
            };
            let (cx, cy) = (500.0f64, 400.0f64);
            let origin = o.extend(0.0);
            let project = move |p: DVec3| {
                let d = p - origin;
                Some(Pos2::new(
                    (cx + k * d.dot(right)) as f32,
                    (cy - k * d.dot(up)) as f32,
                ))
            };
            let ray = move |s: Pos2| {
                let p = origin + right * ((s.x as f64 - cx) / k) + up * ((cy - s.y as f64) / k)
                    - look * 100.0;
                Some(Ray { o: p, d: look })
            };
            let snap = |_: Pos2, _: Option<DVec3>| None;
            let none = |_: Pos2, _: ElementId| None;
            let no_face = |_: Pos2| None;
            let input = PaneInput {
                ray: &ray,
                project: &project,
                look,
                snap: &snap,
                grab_body: &none,
                grab_snap: &none,
                body_normal: if sideways { DVec3::Y } else { DVec3::Z },
                view: Some(if sideways { section } else { plan }),
                face: &no_face,
            };
            let grip = handles_in(doc.model(), chair, None, false)
                .into_iter()
                .find(|h| h.kind == HandleKind::Rotate)
                .expect("an object has a turn grip");
            let at = project(grip.at).unwrap();
            let to = at + Vec2::new(60.0, 60.0);
            let ctx = egui::Context::default();
            let area = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 800.0));
            let (mut grips, mut drag, mut status, mut t) = (GripSel::default(), None, None, 0.0);
            let press = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            for events in [
                vec![egui::Event::PointerMoved(at)],
                vec![press(at, true)],
                vec![egui::Event::PointerMoved(at + Vec2::new(20.0, 20.0))],
                vec![egui::Event::PointerMoved(to)],
                vec![egui::Event::PointerMoved(to)],
                vec![press(to, false)],
            ] {
                t += 1.0 / 60.0;
                let raw = egui::RawInput {
                    screen_rect: Some(area),
                    events,
                    time: Some(t),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(raw, |ui| {
                    let response = ui.allocate_rect(area, egui::Sense::click_and_drag());
                    let painter = ui.painter_at(area);
                    interact(
                        &mut doc,
                        &painter,
                        &response,
                        Some(chair),
                        None,
                        false,
                        &mut drag,
                        &mut grips,
                        &input,
                        &mut status,
                    );
                });
                out.textures_delta.clear();
            }
            let after = match &doc.model().get(chair).unwrap().kind {
                ElementKind::Object(o) => o.rotation,
                _ => unreachable!(),
            };
            after - before
        };
        assert!(turn(false).abs() > 1.0, "in plan the grip turns the chair");
        assert!(turn(true).abs() < 1e-9, "in a section it has no turn grip");
    }

    #[test]
    fn a_mass_has_grips_on_every_face_corner_and_edge_and_moves_turns_by_them() {
        let mut doc = creabim_core::Document::new();
        let mass = creabim_core::FreeMesh::new(
            None,
            DVec3::new(1.0, 2.0, 0.0),
            creabim_core::editmesh::EditMesh::cuboid(DVec3::new(2.0, 1.0, 3.0)),
        );
        let e = Element::new("Mass", ElementKind::FreeMesh(mass));
        let id = e.id;
        doc.commit(creabim_core::Transaction::new("Mass").insert(e))
            .unwrap();
        let m = doc.model().clone();
        // at rest: the base point and the turn grip only
        let rest = handles_in(&m, id, None, false);
        assert_eq!(rest.len(), 2, "{rest:?}");
        // a clicked face adds its arrow
        assert_eq!(handles_in(&m, id, Some(0), false).len(), 3);
        let hs = handles_in(&m, id, None, true);
        let count = |f: fn(&HandleKind) -> bool| hs.iter().filter(|h| f(&h.kind)).count();
        assert_eq!(
            count(|k| matches!(k, HandleKind::Face(_))),
            6,
            "an arrow on every face"
        );
        assert_eq!(count(|k| matches!(k, HandleKind::MeshVertex(_))), 8);
        assert_eq!(count(|k| matches!(k, HandleKind::MeshEdge(..))), 12);
        for k in [
            HandleKind::MeshMove,
            HandleKind::MeshLift,
            HandleKind::Rotate,
        ] {
            assert_eq!(hs.iter().filter(|h| h.kind == k).count(), 1, "{k:?}");
        }
        // the base point moves the whole mass, to the step
        let out = drag_to(&m, id, HandleKind::MeshMove, |h| {
            h.at + DVec3::new(0.52, -0.31, 0.0)
        });
        let ElementKind::FreeMesh(t) = &out.kind else {
            panic!()
        };
        assert!(
            t.origin.distance(DVec3::new(1.5, 1.7, 0.0)) < 1e-9,
            "{:?}",
            t.origin
        );
        // a corner moves alone
        let out = drag_to(&m, id, HandleKind::MeshVertex(0), |h| h.at + DVec3::X * 0.5);
        let ElementKind::FreeMesh(t) = &out.kind else {
            panic!()
        };
        let ElementKind::FreeMesh(was) = &m.get(id).unwrap().kind else {
            panic!()
        };
        let moved = (0..8)
            .filter(|i| t.mesh.vertices[*i].distance(was.mesh.vertices[*i]) > 1e-9)
            .count();
        assert_eq!(moved, 1);
        // the turn grip turns it about its base point
        let out = drag_to(&m, id, HandleKind::Rotate, |h| {
            let o = DVec2::new(1.0, 2.0);
            (o + (h.at.truncate() - o).perp()).extend(h.at.z)
        });
        let ElementKind::FreeMesh(t) = &out.kind else {
            panic!()
        };
        assert!((t.rotation - 90.0).abs() < 1e-6, "{}", t.rotation);
        assert_eq!(t.origin, DVec3::new(1.0, 2.0, 0.0));
    }

    #[test]
    fn a_drags_pill_shows_only_the_value_that_changes() {
        let m = sample_house();
        let (id, _) = wall_with_door(&m);
        let old = m.get(id).unwrap().clone();
        let mut new = old.clone();
        let ElementKind::Wall(w) = &mut new.kind else {
            panic!()
        };
        let len = w.length();
        let dir = w.direction();
        w.end += dir * 0.6;
        let (text, number) = changing(&old, &new).unwrap();
        assert_eq!(text, format!("Length {} → ", bare(len)));
        assert_eq!(number, format!("{} +600", bare(len + 0.6)));
        // nothing named changed: no pill
        assert!(changing(&old, &old).is_none());
    }

    #[test]
    fn value_pills_are_legible_in_every_theme() {
        for t in crate::theme::Theme::ALL {
            let p = PillLook::of(&t.palette());
            for (what, c) in [("text", p.text), ("number", p.number)] {
                let r = crate::theme::contrast_ratio(c, p.ground);
                assert!(r >= 4.5, "{}: the pill's {what} is {r:.2}:1", t.id());
            }
            assert_ne!(p.ground, p.text);
        }
    }

    #[test]
    fn a_wall_shows_no_floating_slide_grip() {
        // the slide grip is kept for the body drag, but the grips drawn
        // all sit on the wall: none off its faces
        let m = sample_house();
        let (id, _) = wall_with_door(&m);
        let ElementKind::Wall(w) = &m.get(id).unwrap().kind else {
            panic!()
        };
        let n = w.direction().perp();
        for h in handles(&m, id, None) {
            if h.kind == HandleKind::WallSlide {
                continue;
            }
            let off = (h.at.truncate() - w.start).dot(n).abs();
            assert!(
                off <= w.thickness / 2.0 + 1e-6 + walls_offset(&m, w),
                "{:?} sits {off} off the wall",
                h.kind
            );
        }
    }

    #[test]
    fn a_wall_slides_square_to_itself() {
        let m = sample_house();
        let (id, w) = m
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::Wall(w) => Some((e.id, w.clone())),
                _ => None,
            })
            .unwrap();
        let n = w.direction().perp();
        let out = drag_to(&m, id, HandleKind::WallSlide, |h| {
            h.at + n.extend(0.0) * 0.5
        });
        let ElementKind::Wall(s) = &out.kind else {
            panic!()
        };
        assert!(s.start.distance(w.start + n * 0.5) < 1e-9);
        assert!(s.end.distance(w.end + n * 0.5) < 1e-9);
        assert!((s.thickness - w.thickness).abs() < 1e-12);
    }

    #[test]
    fn a_column_turns_and_changes_width_and_depth() {
        let m = creabim_core::sample::layered_house();
        let Some(id) = m
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Column(_)))
            .map(|e| e.id)
        else {
            return;
        };
        let ElementKind::Column(c) = &m.get(id).unwrap().kind else {
            panic!()
        };
        // a quarter turn about its point
        let out = drag_to(&m, id, HandleKind::Rotate, |h| {
            let r = h.at.truncate() - c.position;
            (c.position + r.perp()).extend(h.at.z)
        });
        let ElementKind::Column(t) = &out.kind else {
            panic!()
        };
        assert!(((t.rotation - c.rotation).rem_euclid(360.0) - 90.0).abs() < 1e-6);
        let creabim_core::members::Profile::Rectangle { width, .. } = c.profile else {
            return;
        };
        let x = DVec2::from_angle(c.rotation.to_radians());
        let out = drag_to(&m, id, HandleKind::ColumnSize { depth: false }, |h| {
            h.at + x.extend(0.0) * 0.05
        });
        let ElementKind::Column(t) = &out.kind else {
            panic!()
        };
        let creabim_core::members::Profile::Rectangle { width: w2, .. } = t.profile else {
            panic!()
        };
        assert!((w2 - (width + 0.1)).abs() < 1e-9);
        assert!(t.position.distance(c.position) < 1e-12);
    }

    #[test]
    fn an_object_turns_about_its_middle_and_turns_round() {
        let m = creabim_core::starter::starter_house();
        let Some(e) = m
            .iter()
            .find(|e| {
                matches!(e.kind, ElementKind::Object(_))
                    && creabim_core::objects::type_of(&m, e).is_some()
            })
            .cloned()
        else {
            return;
        };
        let mid = pivot_of(&m, &e).unwrap();
        let mut turned = e.clone();
        turn_object(&m, &mut turned, 180.0);
        // the footprint stays where it was, facing the other way
        assert!(pivot_of(&m, &turned).unwrap().distance(mid) < 1e-9);
        let (ElementKind::Object(a), ElementKind::Object(b)) = (&e.kind, &turned.kind) else {
            panic!()
        };
        assert!((a.facing() + b.facing()).length() < 1e-9);
        assert!(handles(&m, e.id, None)
            .iter()
            .any(|h| h.kind == HandleKind::Rotate));
    }

    #[test]
    fn a_door_slides_along_its_wall_at_the_same_width() {
        let m = sample_house();
        let (_, door) = wall_with_door(&m);
        let ElementKind::Opening(o) = &m.get(door).unwrap().kind else {
            panic!()
        };
        let h = handles(&m, door, None)
            .into_iter()
            .find(|h| h.kind == HandleKind::OpeningSlide)
            .unwrap();
        let Constraint::Line { dir, .. } = h.constraint else {
            panic!()
        };
        let out = drag_to(&m, door, HandleKind::OpeningSlide, |h| h.at - dir * 0.1);
        let ElementKind::Opening(n) = &out.kind else {
            panic!()
        };
        assert!((n.offset - (o.offset - 0.1)).abs() < 1e-9);
        assert!((n.width - o.width).abs() < 1e-12);
    }

    /// Snapping is free by default (the owner's review): no grid snap, no
    /// step, no distance marks; drags go exactly where the pointer is.
    #[test]
    fn drags_are_free_by_default() {
        let s = crate::snaps::SnapSettings::default();
        assert!(!s.increment && !s.marks);
        assert_eq!(s.step_len(), 0.0);
        assert_eq!(s.marks_len(), 0.0);
        assert_eq!(s.step_label(), "Off");
    }

    /// A distance mark holds a drag only within 10 px of it, never at the
    /// grab point, and lets go beyond.
    #[test]
    fn distance_marks_hold_only_when_near() {
        // marks 50 mm apart, 40 px apart on screen: 10 px is 12.5 mm
        assert_eq!(super::mark_held(0.112, 0.05, 40.0), Some(0.1));
        assert_eq!(super::mark_held(0.088, 0.05, 40.0), Some(0.1));
        assert_eq!(super::mark_held(0.075, 0.05, 40.0), None);
        assert_eq!(super::mark_held(0.004, 0.05, 40.0), None);
        // the same distances with marks only 10 px apart: always near one
        assert_eq!(super::mark_held(0.077, 0.05, 10.0), Some(0.1));
    }

    /// A grip sliding along a line (a wall's top going up) stops on a face
    /// across its path when the pointer puts it near, and not otherwise.
    #[test]
    fn a_sliding_grip_stops_on_a_face_in_3d() {
        // a side view: x across, z up, 100 px a metre
        let project = |p: DVec3| Some(Pos2::new(p.x as f32 * 100.0, -p.z as f32 * 100.0));
        let ray = |_: Pos2| None;
        let snap = |_: Pos2, _: Option<DVec3>| None;
        let none = |_: Pos2, _: ElementId| None;
        // a slab's soffit at 2.5 m, and an upright face the path runs along
        let soffit = |_: Pos2| Some((DVec3::new(0.3, 0.0, 2.5), DVec3::Z));
        let upright = |_: Pos2| Some((DVec3::new(1.0, 0.0, 2.5), DVec3::X));
        let input = |face: &dyn Fn(Pos2) -> Option<(DVec3, DVec3)>| {
            let (point, dir) = (DVec3::new(0.0, 0.0, 2.0), DVec3::Z);
            let pane = PaneInput {
                ray: &ray,
                project: &project,
                look: DVec3::Y,
                snap: &snap,
                grab_body: &none,
                grab_snap: &none,
                body_normal: DVec3::Z,
                view: None,
                face,
            };
            [2.47, 2.0]
                .map(|z| super::on_face(&pane, Pos2::ZERO, point, dir, DVec3::new(0.0, 0.0, z)))
        };
        // 3 px short of it: on it; 50 px short: free
        let [near, far] = input(&soffit);
        assert!(near.is_some_and(|p| (p.z - 2.5).abs() < 1e-9));
        assert!(far.is_none());
        // a face the path runs along: never
        assert_eq!(input(&upright), [None, None]);
    }
}
