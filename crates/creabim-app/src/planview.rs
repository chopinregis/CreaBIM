//! The 2D plan view: draws the generated plan of a level and hosts the
//! drawing tools. Edits made here are ordinary model edits, so the 3D view
//! and any other open view update at once.

use crate::boxsel;
use crate::commands::Command;
use crate::handles::{self, PaneInput};
use crate::hoverpick::{self, Pane};
use crate::modal::{self, Flow, Modal, Mode, Outcome, PaneMap, Space, Stage};
use crate::outline::Next;
use crate::selection::Selection;
use crate::sketch::{level_below, Click, SketchDraft};
use crate::snaps::{self, SnapKind, Snapper};
use creabim_core::Workplane;
use creabim_core::{
    Dimension, Document, EditMesh, Element, ElementId, ElementKind, FreeMesh, HostPoint, Opening,
    OpeningKind, Slab, Tag, Transaction, View, ViewKind, Wall,
};
use creabim_geom::annotate::{self, dimension_geometry, tag_geometry};
use creabim_geom::annotation::TextLook;
use creabim_geom::SceneItem;
use creabim_geom::{paper, plan::Pen, section::Drawing, triangulate::triangulate, PlanItem};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use glam::{DVec2, DVec3};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Wall,
    Door,
    Window,
    /// A bare opening in a wall: a hole with no door or window.
    Opening,
    Slab,
    /// A vertical hole through the slabs and roofs between two levels.
    Shaft,
    Mass,
    Section,
    Elevation,
    Dimension,
    Tag,
    Angle,
    Radius,
    Diameter,
    ArcLength,
    Note,
    /// A CreaBIM symbol placed where clicked (Mockup_Symbols_REV00).
    Symbol,
    /// A spot level or a level marker: the height of a point.
    SpotLevel,
    Sketch,
    Guide,
    Grid,
    Column,
    Beam,
    Roof,
    Stair,
    Railing,
    /// A strip footing under a wall or a pad footing under a column.
    Footing,
    Level,
    Align,
    /// A piece of furniture or a fixture from the catalogue.
    Object,
    /// A plan outline pulled up into a solid.
    Extrusion,
    /// A profile carried along a wall or a path: copings, plinths, steps.
    Sweep,
    /// A solid that cuts what it meets.
    Void,
    /// The ground: a surface over an outline.
    Terrain,
    /// A building pad (MU41).
    Pad,
    /// Paving, gravel, a driveway: hard landscape with a pattern.
    Paving,
    /// Trees and shrubs.
    Planting,
    /// A room: click inside walls, or draw its outline.
    Room,
    /// A line that splits a room in two where there is no wall.
    RoomSeparator,
    /// A ceiling under a room, or drawn.
    Ceiling,
    /// A box on a plan or section that makes a larger-scale detail view.
    Callout,
    /// A detail component: flashing, membrane, insulation, blocking...
    Detail,
}

impl Tool {
    /// Drawing a line in a plan, it takes a typed length (and angle) in the
    /// tracker by the pointer. Guides were left out, though their help said
    /// a distance could be typed (#34).
    pub fn takes_typed_length(self) -> bool {
        matches!(
            self,
            Tool::Wall
                | Tool::Section
                | Tool::Elevation
                | Tool::Grid
                | Tool::Beam
                | Tool::Railing
                | Tool::Footing
                | Tool::RoomSeparator
                | Tool::Stair
                | Tool::Guide
        )
    }

    pub const ALL: [Tool; 43] = [
        Tool::Select,
        Tool::Wall,
        Tool::Door,
        Tool::Window,
        Tool::Opening,
        Tool::Slab,
        Tool::Shaft,
        Tool::Mass,
        Tool::Column,
        Tool::Beam,
        Tool::Roof,
        Tool::Stair,
        Tool::Railing,
        Tool::Footing,
        Tool::Object,
        Tool::Extrusion,
        Tool::Sweep,
        Tool::Void,
        Tool::Terrain,
        Tool::Pad,
        Tool::Paving,
        Tool::Planting,
        Tool::Room,
        Tool::RoomSeparator,
        Tool::Ceiling,
        Tool::Section,
        Tool::Elevation,
        Tool::Callout,
        Tool::Dimension,
        Tool::Tag,
        Tool::Angle,
        Tool::Radius,
        Tool::Diameter,
        Tool::ArcLength,
        Tool::Note,
        Tool::Symbol,
        Tool::Detail,
        Tool::SpotLevel,
        Tool::Sketch,
        Tool::Guide,
        Tool::Grid,
        Tool::Level,
        Tool::Align,
    ];

    /// Dimensions, tags and notes: tools that annotate a view.
    pub fn is_annotation(self) -> bool {
        matches!(self, Tool::Dimension | Tool::Tag) || crate::annotate::is_tool(self)
    }

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Wall => "Wall",
            Tool::Door => "Door",
            Tool::Window => "Window",
            Tool::Opening => "Wall opening",
            Tool::Slab => "Slab",
            Tool::Shaft => "Shaft",
            Tool::Mass => "Mass",
            Tool::Section => "Section",
            Tool::Elevation => "Elevation",
            Tool::Dimension => "Dimension",
            Tool::Tag => "Tag",
            Tool::Angle => "Angle",
            Tool::Radius => "Radius",
            Tool::Diameter => "Diameter",
            Tool::ArcLength => "Arc length",
            Tool::Note => "Note",
            Tool::Symbol => "Symbol",
            Tool::SpotLevel => "Spot level",
            Tool::Sketch => "Sketch",
            Tool::Guide => "Guide",
            Tool::Grid => "Grid",
            Tool::Column => "Column",
            Tool::Beam => "Beam",
            Tool::Roof => "Roof",
            Tool::Stair => "Stair",
            Tool::Railing => "Railing",
            Tool::Footing => "Footing",
            Tool::Level => "Level",
            Tool::Align => "Align",
            Tool::Object => "Object",
            Tool::Extrusion => "Extrusion",
            Tool::Sweep => "Sweep",
            Tool::Void => "Void",
            Tool::Terrain => "Terrain",
            Tool::Pad => "Building pad",
            Tool::Paving => "Hard landscape",
            Tool::Planting => "Planting",
            Tool::Room => "Room",
            Tool::RoomSeparator => "Room separator",
            Tool::Ceiling => "Ceiling",
            Tool::Callout => "Callout",
            Tool::Detail => "Detail component",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Tool::Select => "Click an element to select it. Drag a selected element to move it. M (Move) moves it by a base point; over a view, G moves it with the pointer, R rotates, S scales, Shift+D duplicates.",
            Tool::Wall => "Click to start, click again to place each wall. Esc or right-click ends the chain. Shift keeps it orthogonal.",
            Tool::Pad => "Pads are drawn with the Terrain tool now: pick Pad in its options.",
            Tool::Door => "Click on a wall to place a door.",
            Tool::Window => "Click on a wall to place a window.",
            Tool::Opening => "Click on a wall to cut a bare rectangular opening in it: no door or window, just the hole. Set its size and sill in Properties.",
            Tool::Shaft => "Draw the shaft's outline like a slab (1–4: Polygon, Rectangle, Fill room, From sketch). It runs from this level to the one above, cutting every slab and roof in between.",
            Tool::Slab => "Click the slab's corners, then the first again or Enter. A draws arcs, L lines; Esc or right-click takes back a point, right-click again leaves. 1–4: Polygon, Rectangle, Fill room, From sketch.",
            Tool::Mass => "Click to place a freeform mass you can then edit like a mesh.",
            Tool::Section => "Click two points for the section line. It looks to the left of the direction you draw.",
            Tool::Elevation => "Click two points for the elevation line, outside the building. It looks to the left of the direction you draw.",
            Tool::Dimension => "Click two points to measure, then click where the dimension line goes. Points snap to wall corners, door and window edges and levels, and stay attached to them.",
            Tool::Tag => "Click a door, window, wall, slab, roof, column, beam, stair, railing, room or ceiling, then click where its tag goes.",
            Tool::Angle => "Click two lines (wall faces, edges) for the angle between them, or click the corner and a point on each arm. Then click where the arc goes.",
            Tool::Radius => "Click an arc or circle, or three points on one. Then click where the dimension line goes.",
            Tool::Diameter => "Click an arc or circle, or three points on one. Then click where the dimension line goes.",
            Tool::ArcLength => "Click an arc, or its start, a point on it and its end. Then click where the dimension line goes.",
            Tool::SpotLevel => "Click a point on a floor, slab or roof in a section or elevation, or on a slab in a plan (elsewhere in a plan it marks the plan's level), then click where its text goes. Its text pattern is in Properties.",
            Tool::Symbol => "Click where a symbol goes: a point marker, slope arrow, centre or break line, material key or fixture. Pick its kind and style, turn it and type its text in Properties.",
            Tool::Note => "Click what the note points at, then where its text goes (turn Leader off to just click). Type its text in Properties. Works on sheets too.",
            Tool::Sketch => "Click the corners of a sketch on this level. Click the first corner to close it, Enter or right-click to leave it open, Esc to cancel. L, A and C draw lines, arcs and circles. Then E extrudes it.",
            Tool::Guide => "Click two points for a guide through them, or click one and type a length. Or click an edge, then click where its parallel guide goes, or type how far across and press Enter. Everything snaps to guides.",
            Tool::Grid => "Click two points for a grid line. It goes in the active grid system (top bar) and takes its next label. Shift keeps it straight.",
            Tool::Column => "Click to place a column. It runs up to the next level; set its size in the top bar or Properties.",
            Tool::Beam => "Click the beam's start, then its end; keep clicking to chain beams. Its top sits at the top of the storey. Esc or right-click ends.",
            Tool::Roof => "Draw the roof's outline like a slab (1–4: Polygon, Rectangle, Fill room, From sketch). Pick Flat, Mono-pitch, Gable or Hip in the bar first.",
            Tool::Stair => "Click the stair's line: its walk line, or its left or right side (top bar). Click again at each turn for a landing there (winders with Winders ticked). It finishes itself when the line reaches the floor above; Enter or clicking the last point again finishes sooner (the last flight runs on), Esc cancels. With Ramp ticked it is one smooth 1:12 slope, landings at its turns. With Split ticked, draw one arm: up the middle, a landing, back down one side; its mirror image comes with it. With Spiral ticked, click its centre, then where it starts. Handrails and guards come with it.",
            Tool::Railing => "Click inside a stair to rail both its sides, or click the points of a path; click the last point again to finish.",
            Tool::Footing => "Showing footings while the tool is on. Strip: click a wall to put a strip footing under it, or click two points for one on its own. Pad: click a column to put a pad under it, or click anywhere for one on its own. Keep clicking for more, or drag a box over walls or columns for all of them at once. Pick Strip or Pad, and the size, in the top bar.",
            Tool::Level => "In a section or elevation, click where the level line starts (its height), then where it ends; its floor plan is made with it. In other views the Levels window opens to add, rename and move levels.",
            Tool::Align => "Click a reference line (a wall face, slab edge, guide or grid line), then the edge of what to align: it turns parallel and moves flush. In Extend mode a wall, beam or grid line end stretches to the reference instead; in Stretch mode the face moves and the rest stays. Centre lines (dashed purple) can be picked too, both ways. In 3D, click a face, edge, corner, centre line or grid line, then a face.",
            Tool::Object => "Click to place the object picked in the Catalogue window. Sinks, WCs, kitchen units, beds and sofas back onto the nearest wall; Space turns the next one a quarter.",
            Tool::Extrusion => "Draw the outline like a slab (1–4: Polygon, Rectangle, Fill room, From sketch). It is pulled up from its base by its height, set in the bar: a chimney, a hearth, a pier. In the 3D view, draw it on any face: it stands square out of that face.",
            Tool::Sweep => "Click a wall to run the profile along its top (coping) or base (plinth); it follows the wall. Or click the points of a path, and the last one again to finish. Pick the profile and its sizes in the bar.",
            Tool::Void => "Draw the void's outline like a slab. It cuts every wall, slab, roof, site surface and extrusion it meets, between its base and height: a firebox, a hole through a floor. In the 3D view, draw it on any face, a sloped roof or a wall: it cuts square into that face, as deep as Depth on a face.",
            Tool::Terrain => "Draw the ground's outline like a slab, then shape it: Import survey brings in a table of points or a DXF, Contour line and Spot level (in the Site tools) draw heights onto it. Paving drawn inside flat terrain sits flush.",
            Tool::Paving => "Draw paving, gravel, a driveway or decking like a slab. Pick what in the bar; each has its own pattern and material.",
            Tool::Planting => "Click to plant a tree, conifer, palm or shrub. Pick the form, canopy and height in the bar.",
            Tool::Room => "Click inside walls for a room that follows them (3, Fill room), or draw its outline (1 Polygon, 2 Rectangle, 4 From sketch) where no walls close it off. Its name, number and finishes are in Properties.",
            Tool::RoomSeparator => "Click two points for a line that splits a room in two where there is no wall, such as between a kitchen and a dining room.",
            Tool::Callout => "On a plan or section, click two corners of a box round what to detail: it becomes a detail view at the scale in the top bar, numbered in a bubble on this view.",
            Tool::Detail => "Pick what to draw in the top bar, then click its points; Enter, a double-click or a right-click finishes (the first point again closes a region). Esc keeps what is drawn. Detail components show in this view only.",
            Tool::Ceiling => "Click inside a room for a ceiling over it that follows its walls (3, Fill room), or draw its outline (1 Polygon, 2 Rectangle, 4 From sketch). Set its height and thickness in the top bar or Properties.",
        }
    }

    /// Tools that draw an outline the way the Slab tool does.
    pub fn draws_outline(self) -> bool {
        matches!(
            self,
            Tool::Slab
                | Tool::Roof
                | Tool::Shaft
                | Tool::Extrusion
                | Tool::Void
                | Tool::Terrain
                | Tool::Pad
                | Tool::Paving
                | Tool::Room
                | Tool::Ceiling
        )
    }

    /// The hint for this tool in a section or elevation.
    fn section_hint(self) -> &'static str {
        match self {
            Tool::Select => "Click to select, Shift+click to add, drag on empty paper for a box. M moves by a base point, G with the pointer, R tilts, S scales, Shift+D duplicates.",
            Tool::Wall => "Click two points: the wall runs along the section line, as high as you draw it.",
            Tool::Slab => "Click two corners of the slab's edge. Its top snaps to the nearest level; it runs back as far as the walls.",
            Tool::Section => "Click where the new section cuts, then click on the side it should look towards.",
            Tool::Elevation => "Click where the new elevation stands, then click on the side it should look towards.",
            Tool::Dimension => "Click two points to measure, then click where the dimension line goes. Points snap to levels.",
            Tool::Guide => "Click two points for a guide on the section plane. It shows here, in 3D and in any section on the same plane.",
            Tool::Level => "Click where the new level line starts (it sets the height; it snaps to model points), then where it ends. Its floor plan is made with it.",
            Tool::Sketch => "Click the corners of a sketch on the section plane. Click the first corner to close it, Enter or right-click to leave it open. L, A and C draw lines, arcs and circles. Then E extrudes it.",
            t => t.hint(),
        }
    }
}

/// What kind of drawing the 2D pane is showing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PaneKind {
    /// A plan of a level: every drawing tool works here.
    Plan { level: ElementId },
    /// A section or elevation along `start` → `end`. Walls, doors, windows
    /// and masses can be drawn here too; drawing x runs along the line and
    /// y is height.
    Section { start: DVec2, end: DVec2 },
    /// A parallel 3D view: drag to orbit, click to select.
    Axonometric,
    /// A schedule drawn as a table: click a row to select its element.
    Schedule,
}

impl PaneKind {
    /// World plan position of a section drawing point on the section plane.
    fn section_point(start: DVec2, end: DVec2, p: DVec2) -> DVec2 {
        start + (end - start).normalize_or_zero() * p.x
    }
}

/// How a pane is shown: on its own, or inside a viewport on a sheet.
#[derive(Clone, Copy, Debug)]
pub struct PaneStyle {
    /// Background fill; `None` leaves what is behind (the sheet) visible.
    pub background: Option<Color32>,
    /// Metric grid and tool hint.
    pub decorations: bool,
    /// Fixed pan and zoom (centre, pixels per metre) instead of the view's own.
    pub camera: Option<(DVec2, f32)>,
    /// Drawing scale denominator, for hatch spacing and annotation sizes.
    pub scale: f64,
}

impl PaneStyle {
    /// Drawing units per paper millimetre.
    pub fn mm(&self) -> f64 {
        self.scale / 1000.0
    }
}

/// Which part of a selected section line is being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Handle {
    Start,
    End,
    Line,
}

pub struct PlanState {
    /// Pan and zoom per view: centre in drawing units, pixels per metre.
    pub cameras: HashMap<ElementId, (DVec2, f32)>,
    /// While Divide is open: where it will divide (model lines; true: plans
    /// show it), and each part's size at its middle (MU38).
    pub divide_marks: Vec<(DVec3, DVec3, bool)>,
    pub divide_labels: Vec<(DVec3, String)>,
    /// The element a click would place, from the view the pointer is in
    /// (its view, or None for 3D): set as it is drawn there, and shown a
    /// frame later in every other view, so placing a mass in 3D shows it
    /// in the plans too (MU39).
    /// Keyed by the pane (two panes may show the same view; the 3D view
    /// is `usize::MAX`).
    pub ghost_now: Option<(Option<usize>, crate::ghost::Ghost)>,
    pub ghost_shared: Option<(Option<usize>, crate::ghost::Ghost)>,
    /// The pane being drawn now, while the areas draw theirs.
    pub pane_now: Option<usize>,
    /// Divide is making guides: its marks show as guides do (MU39).
    pub divide_guides: bool,
    /// While a point is asked for (a radial array's centre): the next
    /// click in a plan gives it, in model x and y (MU39).
    pub point_pick: bool,
    pub point_picked: Option<DVec2>,
    /// While a point is asked for: where the pointer is, and (when
    /// `point_lines`) the drawing's straight line under it, which a click
    /// takes instead of a point (Split's cutting line, MU39).
    pub point_hover: Option<DVec2>,
    /// The first point of a line being picked (Split's), for tracking.
    pub point_from: Option<DVec2>,
    pub point_lines: bool,
    pub point_line_picked: Option<(DVec2, DVec2)>,
    /// Lines and see-through planes a modifier being set shows in the
    /// views (its centre, its mirror plane, its cuts), in model space.
    pub mod_guides: crate::modguides::Guides,
    /// A view to show at its drawing scale on the next frame: 1 paper mm
    /// on 1 screen mm (A5-7, the view's More menu).
    pub zoom_to_scale: Option<ElementId>,
    /// Sheets show their rulers and guides, and snap to them (MU27).
    pub sheet_guides: bool,
    /// A note double-clicked on a sheet, to open in its editor (MU27).
    pub edit_note: Option<ElementId>,
    /// A second area showing the same view pans and zooms on its own
    /// (A4-4): the area being drawn when it does, and those cameras.
    pub area: Option<usize>,
    pub area_cameras: HashMap<(usize, ElementId), (DVec2, f32)>,
    /// Areas kept in step with the first area showing their view.
    pub linked_areas: std::collections::HashSet<usize>,
    /// What the pane under the pointer does, for the status bar.
    pub hint: Option<&'static str>,
    /// How to use what is selected, from its Properties: said in the
    /// status bar instead of `hint` while the pointer is over a view.
    pub howto: Option<String>,
    pub tool: Tool,
    /// First click of a two-click tool (wall start, slab corner).
    pub pending: Option<DVec2>,
    pub wall_thickness: f64,
    /// The line new walls are drawn along, and which side their exterior
    /// is on (the right of the direction drawn, or flipped its left).
    pub wall_location: creabim_core::LocationLine,
    pub wall_flip: bool,
    /// What the Wall tool draws (MU40): 0 straight, 1 an arc (start, end,
    /// then a point it passes through), 2 a circle (centre, then radius).
    pub wall_shape: u8,
    /// An arc wall's end, once clicked: the next click bends it.
    pub wall_arc_end: Option<DVec2>,
    /// Section line handle being dragged, and the line when the drag began.
    drag: Option<(Handle, DVec2, DVec2, DVec2)>,
    /// Dimension being drawn: its points so far, attached and as drawn.
    dim: Vec<(HostPoint, DVec2)>,
    /// Element picked for a tag, waiting for the tag's position.
    tag_host: Option<ElementId>,
    /// Tag or dimension being dragged: where the drag began and its offset then.
    anno_drag: Option<(ElementId, DVec2, DVec2)>,
    /// `anno_drag` drags a dimension's text, not its line (MU80).
    anno_text: bool,
    /// The area whose drag holds `anno_drag`, `level_drag` or `drag`:
    /// another area, not dragged, must not end it (MU32: a dimension moved
    /// a step per drag).
    drag_owner: Option<egui::Id>,
    /// Angles, radii, arc lengths and notes being placed or dragged.
    pub anno: crate::annotate::Draft,
    /// A keyboard move, rotate or scale, in whichever pane started it.
    pub modal: Option<Modal>,
    /// A sketch being drawn, in whichever pane started it.
    pub sketch: Option<SketchDraft>,
    /// A handle or selected element being dragged in a plan or section.
    pub(crate) grip: Option<handles::Drag>,
    /// The element open in Edit mode (see [`crate::editmode`]): all its
    /// grips show, in every view.
    pub edit: Option<ElementId>,
    /// The corners and holes picked in Edit mode, in every view.
    pub grips: handles::GripSel,
    /// Snaps for each 2D view: built from its drawing without the listed
    /// elements, at a model revision.
    snaps: HashMap<ElementId, (u64, Vec<ElementId>, Snapper)>,
    /// A message for the status bar, taken by the app each frame.
    pub status: Option<String>,
    /// The edge a guide is drawn parallel to, picked by its first click.
    pub guide_edge: Option<(DVec2, DVec2)>,
    /// A grid line's end being dragged in a section: the grid, the top
    /// end (else the bottom), and its foot and top as they were (MU70).
    pub grid_drag: Option<(ElementId, bool, f64, f64)>,
    /// Where that drag has the line now, saved when the button comes up
    /// (MU72: saving every frame redrew the elevation each time).
    pub grid_drag_now: Option<(f64, f64)>,
    /// A slab outline being drawn, or a slab outline or wall path being
    /// redrawn.
    pub outline: Option<crate::outline::Outline>,
    /// How the Slab tool draws.
    pub slab_mode: crate::slabtool::SlabMode,
    /// The next side of a slab outline is an arc.
    pub slab_arc: bool,
    /// A new slab thickness keeps the underside where it is (the top
    /// moves) instead of the top.
    pub slab_keep_bottom: bool,
    /// Where a box selection began, on screen.
    /// A box being dragged: where it started, and the area dragging it
    /// (two areas may show one view; only that one ends it, MU21).
    pub boxsel: Option<(Pos2, egui::Id)>,
    /// What rotate and scale turn about.
    pub pivot: modal::Pivot,
    /// What the Move tool last worked on: whole elements or a face.
    pub move_target: modal::Target,
    /// The pivot cursor, placed with Shift+right-click.
    pub cursor: Option<DVec3>,
    /// A move, rotate or scale picked from a button or menu, started by the
    /// next view the pointer is over: (mode, copy first).
    pub armed: Option<(Mode, bool)>,
    /// With [`Mode::Grip`] armed: the grip it drives (from the edge palette).
    pub armed_grip: Option<crate::handles::HandleKind>,
    /// With [`Mode::Offset`] armed: change the original, no copy (the edge
    /// palette's *Offset all*).
    pub in_place: bool,
    /// Commands picked from a view's right-click menu, run by the app, with
    /// the model point the menu was opened at.
    pub commands: Vec<(Command, Option<DVec3>)>,
    /// Where the right-click menu was opened, in the model.
    pub menu_at: Option<DVec3>,
    /// Menus are drawn by the web interface: a right-click asks for one at
    /// this point on screen instead of opening egui's.
    pub web_menus: bool,
    pub menu_request: Option<egui::Pos2>,
    /// Which snaps are on, in every pane and the 3D view.
    pub snap: snaps::SnapSettings,
    /// The grid system the Grid tool puts new lines in.
    pub grid_group: Option<ElementId>,
    /// The Grid tool draws arcs: two ends, then a point on it (MU66).
    pub grid_arc: bool,
    /// An arc grid line's two ends, clicked; the next click bends it.
    pub grid_arc_ends: Option<(DVec2, DVec2)>,
    /// A level being dragged in a section or elevation, and how far above
    /// the pointer its line was when the drag began.
    level_drag: Option<(ElementId, f64)>,
    /// Sizes for new columns, beams, roofs, stairs and railings, and a
    /// railing path being clicked.
    pub members: crate::memberstool::MemberSettings,
    /// Sizes for new extrusions, sweeps, voids and site elements, and a
    /// sweep path being clicked.
    pub parts: crate::partstool::PartSettings,
    /// The Level tool was used where level lines can't be drawn: the app
    /// opens the Levels window instead.
    pub want_levels: bool,
    /// What the Object tool places.
    pub objects: crate::objecttool::ObjectSettings,
    /// The Object tool was used with nothing picked: the app opens the
    /// Catalogue window.
    pub want_catalogue: bool,
    /// Walls or columns a box dragged with the Footing tool took: the app
    /// puts footings under them all, in one step.
    pub want_footings: Vec<ElementId>,
    /// Stretch armed (Shift+S): the next box dragged in a plan takes the
    /// corners and ends inside it.
    pub stretch_box: bool,
    /// The area whose drawing frames the selection next frame (the
    /// header's Frame or ".", MU63: it framed the 3D view instead).
    pub want_frame: Option<usize>,
    /// What that box took, for the Move it starts.
    pub stretch_parts: Option<Vec<(ElementId, crate::stretchtool::Part)>>,
    /// A box selection just made: the app offers Keep only… while the
    /// selection is still what the box took.
    pub boxed: bool,
    /// Elements shown as if selected without being so (a type's instances,
    /// from its Show in model), and whether the rest is drawn faint.
    pub spotlight: Vec<ElementId>,
    pub spotlight_ghost: bool,
    /// The points clicked so far for a chain of walls (or beams, grid
    /// lines...), on a level, for alignment tracking.
    pub chain: (Option<ElementId>, Vec<DVec2>),
    /// Points the pointer rested on while drawing in a view, tracked like
    /// the shape's own points, and the point it is resting on (since when).
    rested: (Option<ElementId>, Vec<DVec2>),
    rest: Option<(DVec2, f64)>,
    /// Snap references in each view: points and edges the pointer rested
    /// on, that temporary guide lines run from. They stay while you work,
    /// until rested on again, cleared (Esc with nothing to cancel), or kept.
    pub refs: HashMap<ElementId, Vec<snaps::Reference>>,
    /// When each view's references were last picked or followed: they go
    /// after [`REF_FADE`] seconds unused, so none linger until Esc.
    refs_used: HashMap<ElementId, f64>,
    /// What the pointer is resting on for a reference: since when, and
    /// whether that rest has already picked (or dropped) it.
    ref_rest: Option<(snaps::Reference, f64, bool)>,
    /// Angled guides every this many degrees through references and the
    /// start point; 0 for none.
    pub guide_step: f64,
    /// The temporary guide under the pointer in the pane it is over, in
    /// that pane's drawing: what K keeps as a guide.
    pub hover_guide: Option<(ElementId, snaps::TempGuide)>,
    /// The Dimension tool's axis; none picks it from where the line goes.
    pub dim_axis: Option<creabim_core::DimAxis>,
    /// Continuous dimensions (MU23 board): each click after the first
    /// dimension adds the next on the same line.
    pub dim_chain: bool,
    /// The line a chain runs on: its axis and a point on it.
    pub chain_line: Option<(creabim_core::DimAxis, DVec2)>,
    /// Rows of dimensions land on steps of this many paper mm from what
    /// they measure (0: free).
    pub dim_step_mm: f64,
    /// The spacing typed for that, kept while it is off.
    pub dim_step_size: f64,
    /// The axis a dimension is held to while Shift is down.
    dim_lock: Option<creabim_core::DimAxis>,
    /// The Align tool's picks so far.
    pub align: crate::aligntool::AlignState,
    /// What the pointer is over before a click, and what else is under it.
    pub hover: Option<crate::hoverpick::HoverPick>,
    /// The drawing views seen in X-ray while working: what holds others
    /// see-through, what is inside them drawn and picked first.
    pub xray: HashSet<ElementId>,
    /// Each view's model as its view rays pick from, with the revision it
    /// was built at.
    pick_scenes: HashMap<ElementId, (u64, Arc<Vec<SceneItem>>)>,
    /// A pick scene being built off the frame, for a view at a revision:
    /// the whole house's 3D scene took 150 to 300 ms of the first hover
    /// after every change (owner's trace, slow frame on release).
    pick_jobs: HashMap<ElementId, (u64, std::sync::mpsc::Receiver<Vec<SceneItem>>)>,
    /// Each 3D view's frame and snaps for the Align tool, with the
    /// revision they were made at.
    axo_snaps: HashMap<ElementId, (u64, Arc<(creabim_geom::section::AxoFrame, Snapper)>)>,
    /// Each plan's and section's centre lines for the Align tool, with the
    /// revision they were made at.
    centre_lines: HashMap<ElementId, (u64, Arc<Vec<crate::aligntool::CentreLine>>)>,
    /// The type each tool draws new elements as.
    pub types: HashMap<creabim_core::TypeCategory, ElementId>,
    /// Create similar (Shift+C): the element new ones are made like, while
    /// the tool it started is the one in hand.
    pub like: Option<(Element, Tool)>,
    /// What the Detail and Callout tools draw, and the Detail tool's
    /// points so far.
    pub details: crate::detailtool::DetailSettings,
    /// A view made in a pane that the app should open: a new detail view.
    pub want_view: Option<ElementId>,
    /// The Snap menu's Custom step being typed.
    pub step_prompt: Option<String>,
    /// The tracker's typed length and angle while a line is drawn (2.10).
    pub tracker: crate::tracker::Tracker,
    /// The temporary dimension being typed into (2.10 part 3).
    pub temp: crate::tempdims::TempState,
    /// The host a host tool's click goes on, as the pointer shows it, and
    /// how many times Tab has moved past the first (A3-2).
    /// What each view shows traced under it (MU18): the viewer's, never
    /// saved in the project.
    /// What the project's references show, read (Trace & Reference v2).
    pub ref_cache: crate::trace::RefCache,
    /// Calibrate: the reference being scaled and the points picked on
    /// it so far; with two, `calibrated` asks for the distance.
    pub calibrate: Option<(ElementId, Vec<DVec2>)>,
    /// The tool options' Close button was pressed (A5-3).
    pub close_outline: bool,
    /// Align (MU23): the reference, and the points picked so far (two on
    /// it, then two on what it lines up with).
    pub align_ref: Option<(ElementId, Vec<DVec2>)>,
    /// Align's four points, picked, for the app to apply.
    pub aligned: Option<(ElementId, [DVec2; 4])>,
    /// Picture or DXF files dropped on a view, and where (MU23).
    pub dropped: Vec<(ElementId, DVec2, std::path::PathBuf)>,
    pub calibrated: Option<(ElementId, DVec2, DVec2)>,
    /// Temporary dimensions muted (MU21): none shown, none clicked.
    pub temp_dims_off: bool,
    /// Sections show the floor plans' ranges: each plan's cut and view
    /// depth as a band, dragged by their grips (MU34 A1).
    pub plan_ranges: bool,
    /// A plan's range line being dragged in a section: the plan, its cut
    /// (else its view depth), its level's height.
    range_drag: Option<(ElementId, RangeGrip, f64)>,
    /// A level tag being dragged: what its moves are kept by, how far it
    /// was moved, and where the drag began.
    tag_drag: Option<(TagKey, DVec2, DVec2)>,
    /// A level tag's leader being bent: what its moves are kept by, and
    /// the point on the ground it measures.
    tag_bend: Option<(TagKey, DVec2)>,
    /// Each section's plan-range band ends, in its drawing (MU36).
    range_ends: HashMap<ElementId, (f64, f64)>,
    /// A plan range line clicked, its height being typed: the plan, its
    /// cut (else its depth), its level, where.
    range_edit: Option<(ElementId, bool, f64, Pos2)>,
    /// The plan whose range a section reads out last (MU35).
    range_focus: Option<ElementId>,
    /// A band's height while its grip is dragged, in drawing units: the
    /// band follows it at once and the model changes only on release
    /// (owner: dragging rebuilt the section and 3D every frame).
    range_preview: Option<f64>,
    /// The band picked with a click: only its grips show and drag, as a
    /// grid's or section mark's do once selected (owner, after MU94).
    range_pick: Option<ElementId>,
    pub host_pick: Option<ElementId>,
    pub host_cycle: usize,
    /// A footing was just placed on a host: the status bar offers the
    /// Foundation plan.
    pub footing_placed: bool,
    /// What a Fill room click would fill, under the pointer.
    pub fill_look: Option<crate::fillpreview::Cached>,
}

impl PlanState {
    /// `e`, made an instance of the type its tool draws (if there is one
    /// and it fits).
    pub fn typed(&self, model: &creabim_core::Model, mut e: Element) -> Element {
        let t = creabim_core::TypeCategory::of(&e.kind).and_then(|c| self.types.get(&c));
        if let Some(t) = t {
            creabim_core::types::stamp(model, &mut e, *t);
        }
        // Create similar: every setting of the element it was started on,
        // its type too, but not its phase (new work is in the phase
        // being worked in)
        if let Some((like, tool)) = &self.like {
            if *tool == self.tool {
                use creabim_core::likeness::Group;
                let on: Vec<Group> = Group::ALL
                    .into_iter()
                    .filter(|g| *g != Group::Phase)
                    .collect();
                creabim_core::likeness::apply(like, &mut e, &on);
            }
        }
        e
    }
}

impl Default for PlanState {
    fn default() -> Self {
        Self {
            cameras: HashMap::new(),
            divide_marks: vec![],
            divide_labels: vec![],
            divide_guides: false,
            ghost_now: None,
            ghost_shared: None,
            pane_now: None,
            point_pick: false,
            point_picked: None,
            point_hover: None,
            point_from: None,
            point_lines: false,
            point_line_picked: None,
            mod_guides: Default::default(),
            zoom_to_scale: None,
            edit_note: None,
            sheet_guides: false,
            area: None,
            area_cameras: HashMap::new(),
            linked_areas: Default::default(),
            hint: None,
            howto: None,
            tool: Tool::Select,
            pending: None,
            wall_thickness: 0.2,
            wall_location: Default::default(),
            wall_flip: false,
            wall_shape: 0,
            wall_arc_end: None,
            drag: None,
            dim: vec![],
            tag_host: None,
            anno_drag: None,
            anno_text: false,
            drag_owner: None,
            anno: Default::default(),
            modal: None,
            sketch: None,
            grip: None,
            edit: None,
            grips: Default::default(),
            snaps: HashMap::new(),
            status: None,
            guide_edge: None,
            grid_drag: None,
            grid_drag_now: None,
            outline: None,
            like: None,
            slab_mode: Default::default(),
            slab_arc: false,
            slab_keep_bottom: false,
            boxsel: None,
            pivot: modal::Pivot::default(),
            move_target: modal::Target::default(),
            cursor: None,
            armed: None,
            armed_grip: None,
            in_place: false,
            commands: vec![],
            menu_at: None,
            web_menus: false,
            menu_request: None,
            snap: snaps::SnapSettings::default(),
            grid_group: None,
            level_drag: None,
            members: Default::default(),
            parts: Default::default(),
            want_levels: false,
            objects: Default::default(),
            want_catalogue: false,
            want_footings: vec![],
            stretch_box: false,
            want_frame: None,
            grid_arc: false,
            grid_arc_ends: None,
            stretch_parts: None,
            refs_used: HashMap::new(),
            boxed: false,
            spotlight: vec![],
            spotlight_ghost: false,
            chain: (None, vec![]),
            rested: (None, vec![]),
            rest: None,
            refs: HashMap::new(),
            ref_rest: None,
            guide_step: 45.0,
            hover_guide: None,
            dim_axis: None,
            dim_chain: false,
            chain_line: None,
            dim_step_mm: 0.0,
            dim_step_size: 7.0,
            dim_lock: None,
            align: Default::default(),
            hover: None,
            xray: HashSet::new(),
            pick_scenes: HashMap::new(),
            pick_jobs: HashMap::new(),
            axo_snaps: HashMap::new(),
            centre_lines: HashMap::new(),
            types: HashMap::new(),
            details: Default::default(),
            want_view: None,
            step_prompt: None,
            temp: Default::default(),
            tracker: Default::default(),
            ref_cache: Default::default(),
            calibrate: None,
            close_outline: false,
            align_ref: None,
            aligned: None,
            dropped: vec![],
            calibrated: None,
            temp_dims_off: false,
            plan_ranges: false,
            range_drag: None,
            tag_drag: None,
            tag_bend: None,
            range_ends: HashMap::new(),
            range_edit: None,
            range_focus: None,
            range_pick: None,
            range_preview: None,
            host_pick: None,
            host_cycle: 0,
            footing_placed: false,
            fill_look: None,
        }
    }
}

impl PlanState {
    /// Rebuild every view's snaps on their next draw (a snap setting
    /// that changes what is snapped to).
    pub fn forget_snaps(&mut self) {
        self.snaps.clear();
    }

    /// Drop any half-finished tool input.
    pub fn cancel(&mut self) {
        self.pending = None;
        self.stretch_box = false;
        self.stretch_parts = None;
        self.grid_arc_ends = None;
        self.dim.clear();
        self.chain_line = None;
        self.anno.clear();
        self.tag_host = None;
        self.sketch = None;
        self.guide_edge = None;
        self.outline = None;
        self.members.path.clear();
        self.members.stair_line.clear();
        self.parts.path.clear();
        self.chain.1.clear();
        self.rested.1.clear();
        self.align.clear();
        self.details.path.clear();
        self.temp = Default::default();
    }

    /// Is a tool part way through, with clicks to undo?
    pub fn has_input(&self) -> bool {
        self.pending.is_some()
            || !self.dim.is_empty()
            || self.anno.has_input()
            || self.tag_host.is_some()
            || self.sketch.is_some()
            || self.guide_edge.is_some()
            || self.outline.is_some()
            || self.armed.is_some()
            || !self.members.path.is_empty()
            || !self.members.stair_line.is_empty()
            || self.align.has_input()
            || !self.details.path.is_empty()
    }

    /// Drop every snap reference, and the guides they make.
    pub fn clear_references(&mut self) -> usize {
        let n = self.refs.values().map(Vec::len).sum();
        self.refs.clear();
        self.ref_rest = None;
        n
    }

    /// Esc or right-click: drop what the tool has so far, or else leave the
    /// tool for Select. False when there was nothing to step back from.
    pub fn step_back(&mut self) -> bool {
        if self.tool == Tool::Terrain && !self.parts.path.is_empty() {
            self.parts.path.pop();
            self.pending = self.parts.path.last().copied();
            self.status = Some(if self.parts.path.is_empty() {
                "Nothing drawn. Click to start again; right-click or Esc leaves the tool.".into()
            } else {
                "Took back the last point. Right-click or Esc takes back another; Enter finishes."
                    .into()
            });
            return true;
        }
        if self.has_input() {
            self.cancel();
            self.armed = None;
            self.status = Some("Cancelled. Right-click or Esc again to leave the tool.".into());
            true
        } else if self.clear_references() > 0 {
            self.status = Some("Guide lines cleared. Esc again leaves the tool.".into());
            true
        } else if self.edit.is_some() && !self.grips.is_empty() {
            self.grips.clear();
            self.status = Some("Nothing picked. Tab finishes editing.".into());
            true
        } else if self.edit.is_some() {
            // Esc never leaves Edit mode, as Blender's: Tab does (MU39)
            self.status = Some("Still editing: Tab finishes.".into());
            true
        } else if self.tool != Tool::Select {
            self.tool = Tool::Select;
            self.status = Some("Back to Select.".into());
            true
        } else {
            false
        }
    }

    /// Is a tool waiting for more clicks?
    /// Is a grip being dragged in a plan or section?
    pub fn gripping(&self) -> bool {
        self.grip.is_some()
    }

    pub fn busy(&self) -> bool {
        self.pending.is_some()
            || !self.dim.is_empty()
            || self.anno.has_input()
            || self.tag_host.is_some()
            || self.sketch.is_some()
            || self.modal.is_some()
            || !self.details.path.is_empty()
            || self.temp.editing.is_some()
    }
}

pub struct Xf {
    pub rect: Rect,
    pub center: DVec2,
    pub scale: f32,
}

impl Xf {
    pub fn to_screen(&self, p: DVec2) -> Pos2 {
        let d = (p - self.center).as_vec2() * self.scale;
        self.rect.center() + Vec2::new(d.x, -d.y)
    }

    pub fn to_world(&self, p: Pos2) -> DVec2 {
        let d = (p - self.rect.center()) / self.scale;
        self.center + DVec2::new(d.x as f64, -d.y as f64)
    }
}

/// Show a view's drawing in `rect`. Returns a status message when an edit fails.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    rect: Rect,
    doc: &mut Document,
    view: ElementId,
    kind: PaneKind,
    plan: &Drawing,
    state: &mut PlanState,
    selection: &mut Selection,
    style: PaneStyle,
) -> Option<String> {
    // while a point is asked for, a click gives it and selects nothing
    let picking = state.point_pick && matches!(kind, PaneKind::Plan { .. });
    let sense = if picking {
        Sense::drag()
    } else {
        Sense::click_and_drag()
    };
    let response = ui.interact(rect, ui.id().with(("view", view)), sense);
    let painter = ui.painter_at(rect);
    if let Some(bg) = style.background {
        painter.rect_filled(rect, 0.0, bg);
    }

    let (mut center, mut scale) = match style.camera {
        Some(c) => c,
        None => {
            let shared = *state.cameras.entry(view).or_insert_with(|| fit(plan, rect));
            match state.area {
                // its own, starting where the first area is
                Some(a) => *state.area_cameras.entry((a, view)).or_insert(shared),
                None => shared,
            }
        }
    };
    if style.camera.is_none() && state.zoom_to_scale == Some(view) {
        state.zoom_to_scale = None;
        if let Some(ElementKind::View(v)) = doc.model().get(view).map(|e| &e.kind) {
            // a metre is 1000/scale paper mm; a screen mm is 96/25.4 points
            scale = (1000.0 / v.scale.max(1.0) * 96.0 / 25.4) as f32 / ui.ctx().zoom_factor();
        }
    }
    let tool = match (kind, state.tool) {
        (PaneKind::Axonometric, Tool::Tag) => Tool::Tag,
        (PaneKind::Axonometric, Tool::Note) => Tool::Note,
        (PaneKind::Axonometric, Tool::Align) => Tool::Align,
        (PaneKind::Axonometric | PaneKind::Schedule, _) => Tool::Select,
        (_, t) => t,
    };
    let mm = style.mm();

    // orbit a 3D view by dragging it
    if kind == PaneKind::Axonometric && response.dragged_by(egui::PointerButton::Primary) {
        let d = response.drag_delta();
        let r = doc.edit_merged(view, "Orbit 3D view", |e| {
            if let ElementKind::View(View {
                kind: ViewKind::Axonometric { azimuth, altitude },
                ..
            }) = &mut e.kind
            {
                *azimuth = (*azimuth - d.x as f64 * 0.4).rem_euclid(360.0);
                *altitude = (*altitude + d.y as f64 * 0.4).clamp(-89.0, 89.0);
            }
        });
        if let Err(e) = r {
            return Some(e.to_string());
        }
    }

    // pan and zoom; Ctrl + middle drag zooms about where it started (MU39)
    let ctrl_mid =
        response.dragged_by(egui::PointerButton::Middle) && ui.input(|i| i.modifiers.command);
    if ctrl_mid {
        let k = (1.0 - response.drag_delta().y * 0.01).clamp(0.5, 2.0);
        if let Some(at) = ui.input(|i| i.pointer.press_origin()) {
            let xf = Xf {
                rect,
                center,
                scale,
            };
            let before = xf.to_world(at);
            scale = (scale * k).clamp(2.0, 2000.0);
            let xf = Xf {
                rect,
                center,
                scale,
            };
            center += before - xf.to_world(at);
        }
    } else if response.dragged_by(egui::PointerButton::Secondary)
        || response.dragged_by(egui::PointerButton::Middle)
    {
        let d = response.drag_delta() / scale;
        center -= DVec2::new(d.x as f64, -d.y as f64);
    }
    if response.hovered() {
        // a touchpad's two fingers pan; a wheel and a pinch zoom (MU24)
        let (pan, scroll) = wheel(ui);
        center -= DVec2::new(pan.x as f64, -pan.y as f64) / scale as f64;
        if scroll != 0.0 {
            if let Some(hover) = response.hover_pos() {
                let xf = Xf {
                    rect,
                    center,
                    scale,
                };
                let before = xf.to_world(hover);
                scale = (scale * (1.0 + scroll * 0.002)).clamp(2.0, 2000.0);
                let xf = Xf {
                    rect,
                    center,
                    scale,
                };
                center += before - xf.to_world(hover);
            }
        }
    }
    // zoom and pan buttons, for a mouse without a wheel button or a
    // trackpad (MU18): zoom in, zoom out, fit, and a hand dragged to pan
    // the zoom column reads its clicks here and is drawn once the drawing
    // is, over it (owner: the controls showed behind the site plan)
    let mut nav_look = None;
    if style.decorations && style.camera.is_none() {
        // the selection, framed as the drawing is (MU24)
        let picked = creabim_geom::PlanView {
            items: plan
                .items
                .iter()
                .filter(|i| selection.contains(i.id()))
                .cloned()
                .collect(),
        };
        let fit_sel = (!picked.items.is_empty()).then(|| fit(&picked, rect));
        let (asked, look) = nav_input(ui, rect, view, || fit(plan, rect), fit_sel, center, scale);
        nav_look = look;
        if let Some((c, k)) = asked {
            center = c;
            scale = k;
        }
        // Frame from the header or ".": this area was the one clicked
        if state.want_frame.is_some() && state.want_frame == state.area {
            state.want_frame = None;
            match fit_sel {
                Some((c, k)) => {
                    (center, scale) = (c, k);
                    state.status = Some("Framed the selection.".into());
                }
                None => {
                    state.status = Some(
                        "The selection isn't drawn in this view: nothing to frame here.".into(),
                    )
                }
            }
            ui.ctx().request_repaint();
        }
    }
    if style.camera.is_none() {
        match state.area {
            Some(a) => {
                state.area_cameras.insert((a, view), (center, scale));
            }
            None => {
                state.cameras.insert(view, (center, scale));
            }
        }
    }
    let xf = Xf {
        rect,
        center,
        scale,
    };

    if style.decorations && kind != PaneKind::Schedule {
        let (grid, axes) = plan_look();
        if grid {
            draw_metric_grid(&painter, &xf);
        }
        // the origin in a plan, faint: where the building is anchored, red
        // along X and green along Y as in 3D (MU40)
        if axes && matches!(kind, PaneKind::Plan { .. }) {
            draw_origin_axes(&painter, &xf);
        }
    }
    // a point asked for: a cross follows the pointer, a click gives it
    if picking {
        if let Some(h) = response.hover_pos() {
            let w = xf.to_world(h);
            let step = crate::snaps::current_step();
            // the drawing tools' snaps (MU39): corners, middles, where
            // guides and lines cross, along a guide; else the grid step
            let cfg = state.snap.live(ui.input(|i| i.modifiers.alt));
            let from = state.point_from.map(|f| f.extend(0.0));
            let snapped = state.snaps.get(&view).and_then(|(_, _, sn)| {
                sn.snap(
                    &cfg,
                    h,
                    &|p: DVec3| Some(xf.to_screen(p.truncate())),
                    from,
                    Some(w.extend(0.0)),
                )
            });
            if let Some(sn) = snapped {
                let top = ui
                    .ctx()
                    .layer_painter(egui::LayerId::new(
                        crate::app::overlay_order(),
                        ui.id().with("picksnap"),
                    ))
                    .with_clip_rect(rect);
                crate::snaps::draw(
                    &top,
                    sn,
                    xf.to_screen(sn.p.truncate()),
                    from.map(|f| xf.to_screen(f.truncate())),
                );
            }
            let corner = snapped
                .filter(|s| s.kind.is_point())
                .map(|s| s.p.truncate());
            let along = snapped
                .filter(|s| !s.kind.is_point())
                .map(|s| s.p.truncate());
            let w = match corner.or(if state.point_lines { None } else { along }) {
                Some(c) => c,
                None if step > 0.0 => (w / step).round() * step,
                None => w,
            };
            state.point_hover = Some(w);
            // a straight line of the drawing under the pointer: a guide, a
            // wall's face, an edge
            let line = if state.point_lines && corner.is_none() {
                let reach = 6.0 / scale as f64;
                let to_seg = |a: DVec2, b: DVec2| {
                    let ab = b - a;
                    let t = ((w - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
                    (a + ab * t).distance(w)
                };
                plan.items
                    .iter()
                    .flat_map(|it| match it {
                        PlanItem::Line { a, b, .. } => vec![(*a, *b)],
                        PlanItem::Polygon { points, .. } => (0..points.len())
                            .map(|i| (points[i], points[(i + 1) % points.len()]))
                            .collect(),
                        _ => vec![],
                    })
                    .filter(|(a, b)| a.distance(*b) > 1e-6 && to_seg(*a, *b) < reach)
                    .min_by(|p, q| to_seg(p.0, p.1).total_cmp(&to_seg(q.0, q.1)))
            } else {
                None
            };
            if let Some((a, b)) = line {
                let top = ui
                    .ctx()
                    .layer_painter(egui::LayerId::new(
                        crate::app::overlay_order(),
                        ui.id().with("pickline"),
                    ))
                    .with_clip_rect(rect);
                top.line_segment(
                    [xf.to_screen(a), xf.to_screen(b)],
                    Stroke::new(3.0, SPLIT_RED),
                );
            }
            let c = xf.to_screen(w);
            let top = ui
                .ctx()
                .layer_painter(egui::LayerId::new(
                    crate::app::overlay_order(),
                    ui.id().with("pick"),
                ))
                .with_clip_rect(rect);
            let s = Stroke::new(1.5, accent_selected());
            top.line_segment([c - egui::vec2(12.0, 0.0), c + egui::vec2(12.0, 0.0)], s);
            top.line_segment([c - egui::vec2(0.0, 12.0), c + egui::vec2(0.0, 12.0)], s);
            top.circle_stroke(c, 5.0, s);
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            if ui.input(|i| i.pointer.primary_clicked()) {
                match line {
                    Some(l) => state.point_line_picked = Some(l),
                    None => state.point_picked = Some(w),
                }
                if !state.point_lines {
                    state.point_pick = false;
                }
            }
        }
        if ui.input(|i| i.pointer.secondary_clicked() || i.key_pressed(egui::Key::Escape)) {
            state.point_pick = false;
        }
    }
    // the references shown in this view, under the drawing (MU19)
    if style.decorations {
        crate::trace::draw_view(
            &painter,
            doc.model(),
            &mut state.ref_cache,
            view,
            false,
            &|p| xf.to_screen(p),
        );
    }
    // the slabs of a plan's level: a light fill under the rest, stronger
    // where the pointer is, so they can be seen and clicked
    let slabs = match kind {
        PaneKind::Plan { level } => level_slabs(doc.model(), level, plan),
        _ => vec![],
    };
    // pre-selection: what a click here would take, and what else is
    // under the pointer (Tab cycles them)
    let pane = Pane::View(view);
    if !response.dragged() {
        // (the Align tool takes faces in a 3D view, so Tab reaches them)
        let reading = (matches!(tool, Tool::Select | Tool::Tag)
            || (tool == Tool::Align && kind == PaneKind::Axonometric))
            && state.modal.is_none()
            && state.boxsel.is_none()
            // in Edit mode Tab is Layers', in a drag the typed length's
            && state.edit.is_none()
            && state.grip.is_none();
        let pointer = response.hover_pos().filter(|_| reading);
        let tol = 6.0 / xf.scale as f64;
        let rev = doc.model().revision();
        let mut hover = state.hover.take();
        let _ph = crate::app::Perf::new("hover track");
        let said = hoverpick::track(ui.ctx(), &mut hover, pane, pointer, rev, doc.model(), |p| {
            under_pointer(state, doc, view, kind, plan, xf.to_world(p), tol, mm)
        });
        state.hover = hover;
        drop(_ph);
        if said.is_some() {
            state.status = said;
        }
    }
    let hovered = hoverpick::highlighted(&state.hover)
        .filter(|id| !selection.contains(*id) && !response.dragged());
    let hovered_slab = hovered.filter(|id| slabs.iter().any(|(s, _)| s == id));
    for (id, points) in &slabs {
        let fill = if selection.contains(*id) {
            SLAB_SELECTED
        } else if hovered_slab == Some(*id) {
            SLAB_HOVER
        } else {
            SLAB_FILL
        };
        fill_polygon(&painter, &xf, points, fill);
    }
    let xray = state.xray.contains(&view);
    // a type's instances shown in the model: drawn as selected, the rest
    // faint when asked
    let ghost = state.spotlight_ghost && !state.spotlight.is_empty();
    let mut faint = painter.clone();
    faint.set_opacity(0.22);
    // the view's graphic overrides, as on its sheets (MU27)
    let mut overrides = view_overrides(doc.model(), view);
    // grid lines in their system's colour, under the view's own overrides
    for e in doc.model().iter() {
        if let ElementKind::Grid(g) = &e.kind {
            if let Some(c) = grid_colour(doc.model(), g) {
                let o = overrides.entry(e.id).or_default();
                o.colour.get_or_insert(c);
            }
        }
    }
    // the project's own patterns (brick coursing, tiles, drawn tiles), as
    // its sheets draw them
    let looks = view_looks(doc.model(), view);
    // the view's line style, drawn on screen as on its sheets (MU84: a
    // style set on a view showed nowhere but on paper)
    let hand = match doc.model().get(view).map(|e| &e.kind) {
        Some(ElementKind::View(v)) => creabim_core::styles::view_line_style(doc.model(), v),
        _ => None,
    };
    // its leaders and ticks as the style draws them
    let styled = hand
        .as_ref()
        .map(|s| creabim_geom::handline::restyle(&plan.items, s));
    let items = styled.as_ref().unwrap_or(&plan.items);
    let _hand = ScreenHand::set(hand);
    // a paving's pattern is opened up where the zoom would close it into
    // a patch: its drawing's lines give way to fewer, wider apart (MU125)
    let _pp = crate::app::Perf::new("plan paint");
    let opened = screen_patterns(doc.model(), view, items, xf.scale as f64);
    // the scale table's row for the view, and its layers' frames: cut
    // pieces patterned on screen as on paper
    let table = looks.as_deref().filter(|l| l.rule.is_some());
    let frames = table
        .map(|l| creabim_geom::looks::layer_frames(items, l))
        .unwrap_or_default();
    let mut seen: HashSet<ElementId> = HashSet::new();
    let mut shown: Vec<&PlanItem> = Vec::with_capacity(items.len());
    for item in items {
        match item {
            PlanItem::Line {
                id,
                pen: Pen::Pattern,
                ..
            } if opened.contains_key(id) => {
                if seen.insert(*id) {
                    shown.extend(opened[id].iter());
                }
            }
            _ => shown.push(item),
        }
    }
    for item in shown {
        let lit = state.spotlight.contains(&item.id());
        let selected = selection.contains(item.id()) || lit;
        if ghost && !selected {
            draw_item(&faint, &xf, item, false, mm);
        } else if xray {
            draw_item_xray(&painter, &xf, item, selected, mm, doc.model());
        } else {
            let mut o = overrides.get(&item.id()).copied().unwrap_or_default();
            let view_off = o.hatch_off;
            // a cut piece, by the scale table: its lines as the sheet draws
            // them (true size, opened up, none for thin layers and at Fill)
            if let (
                Some(l),
                PlanItem::Fill {
                    id,
                    points,
                    hatch,
                    rgb,
                    material,
                },
            ) = (table, item)
            {
                o.hatch_off = true;
                let mut p = painter.clone();
                if o.fade > 0.0 && !selected {
                    p.set_opacity(1.0 - o.fade);
                }
                draw_item_with(&p, &xf, item, selected, mm, &o);
                let spacing = l.rule.map_or(paper::HATCH_SPACING, |r| r.hatch_spacing);
                let readable = spacing * mm * xf.scale as f64 > 3.0;
                if let (false, true, (_, Some(pat))) =
                    (view_off, readable, l.cut_pattern_of(*id, *hatch, *rgb))
                {
                    let k = 1.0 / mm.max(1e-9);
                    let pat = l.at_scale(pat, k);
                    let frame = creabim_geom::looks::frame_of(&frames, *id, *material, points);
                    let (lines, _) = l.cut_hatch(*material, &pat, points, k, frame);
                    let stroke = Stroke::new(0.6, o.colour.unwrap_or(Color32::from_gray(70)));
                    for (a, b) in lines {
                        p.line_segment([xf.to_screen(a), xf.to_screen(b)], stroke);
                    }
                }
                continue;
            }
            let own = looks.as_deref().and_then(|l| own_pattern(l, item, mm));
            if own.is_some() && matches!(item, PlanItem::Fill { .. }) {
                o.hatch_off = true;
            }
            let mut p = painter.clone();
            if o.fade > 0.0 && !selected {
                p.set_opacity(1.0 - o.fade);
            }
            draw_item_with(&p, &xf, item, selected, mm, &o);
            if let (Some(pat), Some(points)) = (own, item_points(item)) {
                // as the built-in hatches: not once too dense to read
                let readable = paper::HATCH_SPACING * mm * xf.scale as f64 > 3.0;
                if !view_off && readable {
                    draw_pattern(&p, &xf, &pat, points, mm, o.colour);
                }
            }
        }
    }
    drop(_pp);
    // the references drawn over the model, then the outline of each one
    // selected or hovered, which is where it is picked (MU19)
    if style.decorations {
        crate::trace::draw_view(
            &painter,
            doc.model(),
            &mut state.ref_cache,
            view,
            true,
            &|p| xf.to_screen(p),
        );
        for (id, r) in creabim_core::reference::in_view(doc.model(), view) {
            let lit = selection.contains(id);
            if !(lit || hovered == Some(id)) {
                continue;
            }
            if let Some(c) = r.outline() {
                let mut pts: Vec<Pos2> = c.iter().map(|p| xf.to_screen(*p)).collect();
                pts.push(pts[0]);
                let ink = if lit {
                    accent_selected()
                } else {
                    accent_selected().gamma_multiply(0.6)
                };
                painter.extend(egui::Shape::dashed_line(
                    &pts,
                    Stroke::new(1.5, ink),
                    6.0,
                    4.0,
                ));
            }
        }
    }
    // a dimension that holds its spacing shows a small closed padlock by
    // its value, on screen only: paper prints it as any dimension. Left
    // alone it is a faint ghost; selected, or with an end selected, it
    // shows strongly
    for item in &plan.items {
        if let PlanItem::Dimension {
            id,
            a,
            b,
            offset,
            witness,
            text,
            look,
            ..
        } = item
        {
            let current = match doc.model().get(*id).map(|e| &e.kind) {
                Some(ElementKind::Dimension(d)) if d.held.is_some() => Some(
                    selection.contains(*id)
                        || [d.a.host, d.b.host]
                            .into_iter()
                            .flatten()
                            .any(|h| selection.contains(h)),
                ),
                _ => None,
            };
            if let Some(current) = current {
                let g = dimension_geometry(*a, *b, *offset, *witness, look, mm);
                let px = crate::annotate::text_px(look, mm, &xf);
                let half = paper::text_width(text, look.size) * mm * xf.scale as f64 / 2.0;
                let along = DVec2::from_angle(g.angle);
                let at = xf.to_screen(g.text_at);
                let dir = Vec2::new(along.x as f32, -along.y as f32);
                let spot = at + dir * (half as f32 + px * 0.9);
                crate::tempdims::held_mark(&painter, spot, px.clamp(9.0, 16.0), current);
            }
        }
    }
    // footings: an ordinary plan doesn't show them, but while the Footing
    // tool is on it shows its level's, and a selected one always shows
    // (A3-1)
    if let PaneKind::Plan { level } = kind {
        draw_footings(
            &painter,
            &xf,
            doc.model(),
            level,
            tool == Tool::Footing,
            selection,
        );
    }
    // 3D crop boxes in plans and sections (A4-5): the selected one, and
    // every one whose frame is shown, dashed in the crop colour; the one
    // under the pointer lit as any hovered element (MU138: only the 3D
    // window lit it, so picking it here was a guess)
    if style.decorations {
        for (id, lo, hi) in crop_rects(doc.model(), view) {
            let shown = matches!(
                doc.model().get(id).map(|e| &e.kind),
                Some(ElementKind::CropBox(c)) if !c.frame_hidden
            );
            let Some((w, c)) = crop_frame_look(selection.contains(id), hovered == Some(id), shown)
            else {
                continue;
            };
            let corners = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y), lo];
            let pts: Vec<Pos2> = corners.iter().map(|p| xf.to_screen(*p)).collect();
            for seg in pts.windows(2) {
                painter.extend(egui::Shape::dashed_line(seg, Stroke::new(w, c), 6.0, 4.0));
            }
        }
    }
    // a selected mark is traced in the accent, as a hover traces it
    for item in plan.items.iter().filter(|it| is_mark(it)) {
        if selection.contains(item.id()) && !ghost {
            trace(&painter, &xf, item, accent_selected(), mm);
        }
    }
    if let Some(id) = hovered {
        draw_hover(&painter, &xf, &plan.items, id, mm);
    }
    // a note picked or under the pointer shows its block as a dashed box,
    // as trace images do, so its extent is plain (MU88)
    if !ghost {
        let notes: Vec<(ElementId, bool)> = selection
            .all()
            .into_iter()
            .map(|id| (id, true))
            .chain(hovered.filter(|h| !selection.contains(*h)).map(|h| (h, false)))
            .filter(|(id, _)| {
                matches!(doc.model().get(*id).map(|e| &e.kind), Some(ElementKind::Note(n)) if n.symbol.is_none())
            })
            .collect();
        for (id, picked) in notes {
            let pts: Vec<DVec2> = plan
                .items
                .iter()
                .filter(|it| it.id() == id)
                .flat_map(|it| it.extent())
                .collect();
            let Some(first) = pts.first().copied() else {
                continue;
            };
            let (mut lo, mut hi) = pts
                .iter()
                .fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
            let pad = DVec2::splat(1.5 * mm);
            lo -= pad;
            hi += pad;
            let corners = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y), lo];
            let pts: Vec<Pos2> = corners.iter().map(|p| xf.to_screen(*p)).collect();
            let c = if picked {
                accent_selected()
            } else {
                accent_selected().gamma_multiply(0.5)
            };
            for seg in pts.windows(2) {
                painter.extend(egui::Shape::dashed_line(seg, Stroke::new(1.2, c), 6.0, 4.0));
            }
        }
    }
    if let Some(id) = hoverpick::badge(ui.ctx(), &mut state.hover, pane, doc.model()) {
        if ui.input(|i| i.modifiers.shift) {
            selection.toggle(id);
        } else {
            selection.set(Some(id));
        }
        state.status = Some(format!(
            "{} selected.",
            hoverpick::describe(doc.model(), id)
        ));
    }
    if let Some((_, points)) = slabs.iter().find(|(id, _)| Some(*id) == hovered_slab) {
        // its edge over the walls that sit on it
        let pts: Vec<Pos2> = points.iter().map(|p| xf.to_screen(*p)).collect();
        ring_edges(&painter, &pts, Stroke::new(2.0, SLAB_EDGE));
    }
    crate::annotate::draw_grips(&painter, &xf, doc.model(), &plan.items, selection, mm);
    crate::sketch::draw_marks_in_pane(&painter, doc.model(), selection.active, kind, &xf);
    if let Some(look) = &nav_look {
        nav_draw(ui, look);
    }
    // the centre lines the Align tool can take here
    // (and Mirror, which can take any of them as its line, MU142)
    let mirroring = state
        .modal
        .as_ref()
        .is_some_and(|m| m.mode == crate::modal::Mode::Mirror);
    let centre_lines = match (tool, kind) {
        (Tool::Align, PaneKind::Plan { .. } | PaneKind::Section { .. }) => {
            pane_centre_lines(state, doc, view, kind, plan)
        }
        (_, PaneKind::Plan { .. } | PaneKind::Section { .. }) if mirroring => {
            pane_centre_lines(state, doc, view, kind, plan)
        }
        _ => Arc::new(vec![]),
    };
    if tool == Tool::Align && kind != PaneKind::Axonometric {
        let model = doc.model();
        let hover = response.hover_pos().and_then(|p| {
            crate::aligntool::hover_edge(
                model,
                &state.align,
                &plan.items,
                &centre_lines,
                xf.to_world(p),
                6.0 / xf.scale as f64,
            )
        });
        crate::aligntool::draw_picks(&painter, &xf, &state.align, view, hover, &centre_lines);
        // Corner: the result as a ghost before the second click (MU69)
        if let (crate::aligntool::AlignMode::Corner, Some((v, first, keep)), Some(p)) =
            (state.align.mode, state.align.corner, response.hover_pos())
        {
            let at = xf.to_world(p);
            let second = crate::aligntool::corner_pick(
                model,
                &plan.items,
                &centre_lines,
                at,
                6.0 / xf.scale as f64,
            );
            if let Some(lines) = second
                .filter(|_| v == view)
                .and_then(|s| crate::aligntool::corner_preview(model, (first, keep), (s, at)))
            {
                crate::aligntool::paint_corner_ghost(&painter, &|q| Some(xf.to_screen(q)), lines);
            }
        }
        if let (Some(h), Some(p), true) = (
            hover,
            response.hover_pos(),
            matches!(kind, PaneKind::Plan { .. }),
        ) {
            crate::aligntool::draw_outline_preview(
                &painter,
                &xf,
                model,
                &state.align,
                view,
                h,
                xf.to_world(p),
            );
        }
    }

    let (shift, alt) = ui.input(|i| (i.modifiers.shift, i.modifiers.alt));
    // holding Alt picks free of snaps
    let snap_cfg = state.snap.live(alt);
    // the Align tool in a 3D view works on the model's faces
    let align_under = match (kind, tool) {
        (PaneKind::Axonometric, Tool::Align) => {
            let fs = axo_frame_snaps(state, doc, view);
            let under = match (&fs, response.hover_pos()) {
                (Some(fs), Some(p)) => axo_align_under(state, doc, view, fs, &xf, p, &snap_cfg),
                _ => Default::default(),
            };
            if let Some(fs) = &fs {
                let project = |m: DVec3| Some(xf.to_screen(fs.0.project(m).truncate()));
                crate::align3d::draw(&painter, &project, doc.model(), &state.align, &under);
            }
            Some(under)
        }
        _ => None,
    };
    // Pull waiting for a face in another view comes to this one (MU46)
    if response.hovered()
        && state
            .modal
            .as_ref()
            .is_some_and(|m| m.waits_to_pull() && m.pane != Some(view))
    {
        if let Some(mut m) = state.modal.take() {
            m.discard(doc);
        }
        state.armed = Some((crate::modal::Mode::Pull, false));
    }
    // snaps: the selection and what joins it are left out while selecting,
    // since dragging them must not snap to themselves
    let moving = state
        .modal
        .as_ref()
        .filter(|m| m.pane == Some(view))
        .and_then(|m| m.snap_exclude(doc));
    let exclude = match (moving, tool) {
        (Some(ids), _) => ids,
        (None, Tool::Select) => selection
            .active
            .map(|id| joined_with(doc, id))
            .unwrap_or_default(),
        _ => vec![],
    };
    if matches!(kind, PaneKind::Plan { .. } | PaneKind::Section { .. }) {
        refresh_snaps(state, doc.model(), view, plan, &exclude);
    }
    let pane_project = |p: DVec3| Some(xf.to_screen(p.truncate()));
    // a line starts from the tool's first click, or the sketch's last corner
    let sketch_last = state
        .sketch
        .as_ref()
        .filter(|d| d.pane == Some(view))
        .and_then(|d| Some(d.plane.to_model(*d.points.last()?)));
    let from_pane = |m: DVec3| match kind {
        PaneKind::Section { start, end } => model_to_pane(Space::Section { start, end }, m),
        _ => m.truncate(),
    };
    let last = state
        .outline
        .as_ref()
        .and_then(|o| o.points.last().copied());
    let drawing_from = state
        .pending
        .or(sketch_last.map(from_pane))
        .or(last)
        .map(|a| a.extend(0.0));
    // temporary guide lines: from the points and edges rested on in the
    // drawing tools, and through where the line being drawn starts
    let guided = matches!(kind, PaneKind::Plan { .. } | PaneKind::Section { .. })
        && tool != Tool::Select
        && snap_cfg.allows(SnapKind::OnTempGuide);
    // a guide copied parallel to an edge or guide: only its distance
    // across counts, so nothing lines up with the spot it was taken by
    // (MU70: the drop snapped to the axes through it and jumped aside)
    let copying_guide = tool == Tool::Guide && state.guide_edge.is_some();
    let drawing_from = drawing_from.filter(|_| !copying_guide);
    let guide_from = drawing_from.or(state.dim.first().map(|d| d.1.extend(0.0)));
    snaps::set_free(ui.input(|i| i.modifiers.alt));
    let mut guides = if guided {
        let refs = state.refs.get(&view).map_or(&[][..], |r| r.as_slice());
        let _p = crate::app::Perf::new("plan snaps: guides");
        snaps::temp_guides(refs, guide_from, state.guide_step)
    } else {
        vec![]
    };
    // drawing a stair: the heads, feet, sides and landings of the other
    // stairs on this level line up with it on their own, near the
    // pointer; Alt goes free of them (Stair Guides REV02 §1, §7)
    if let (true, Tool::Stair, PaneKind::Plan { level }, Some(p)) =
        (guided, tool, kind, response.hover_pos())
    {
        if !ui.input(|i| i.modifiers.alt) {
            let lines = snaps::stair_lines(doc.model(), level, None);
            guides.extend(snaps::stair_guides(
                &lines,
                xf.to_world(p),
                STAIR_REACH / xf.scale as f64,
            ));
        }
    }
    // the points of the shape being drawn here, for the next to line up with
    let track = {
        let shape = shape_points(state, view, kind, &from_pane);
        // (with guide lines on, points rested on make guides instead)
        let rested: Vec<DVec3> = match (&state.rested, drawing_from) {
            ((Some(v), pts), Some(_)) if *v == view && !guided => {
                pts.iter().map(|p| p.extend(0.0)).collect()
            }
            _ => vec![],
        };
        let shape: Vec<DVec3> = shape.iter().map(|p| p.extend(0.0)).collect();
        let mut track = snaps::track_points(&shape, &rested);
        // drawing grid lines: the other lines' ends to line up with, so a
        // row of them starts and ends square (MU68: only a guide did it)
        if let (Tool::Grid, PaneKind::Plan { level }) = (tool, kind) {
            for e in doc.model().iter() {
                if let ElementKind::Grid(g) = &e.kind {
                    if !creabim_core::grids::on_level(g, level) {
                        continue;
                    }
                    for p in [g.start, g.end] {
                        let p = p.extend(0.0);
                        if !track.iter().any(|(q, _)| q.distance(p) < 1e-6) {
                            track.push((p, snaps::Through::GridEnd));
                        }
                    }
                }
            }
        }
        track
    };
    // a host tool's host under the pointer wins over snaps and guides
    // before the first click; Tab goes through overlapping hosts (A3-2)
    let host_hits: Vec<ElementId> = match (kind, response.hover_pos()) {
        (PaneKind::Plan { level }, Some(p)) if crate::memberstool::hosted(tool) => {
            let _p = crate::app::Perf::new("plan snaps: hosts");
            crate::memberstool::hosts_at(doc.model(), level, tool, state, xf.to_world(p))
        }
        _ => vec![],
    };
    let host = (!host_hits.is_empty()).then(|| host_hits[state.host_cycle % host_hits.len()]);
    if response.hovered() {
        state.host_pick = host;
        if host_hits.len() < 2 {
            state.host_cycle = 0;
        }
    }
    let guides = if host.is_some() { vec![] } else { guides };
    let tracked = response
        .hover_pos()
        .filter(|_| host.is_none())
        .and_then(|p| {
            let _p = crate::app::Perf::new("plan snaps: guided");
            let snapper = &state.snaps.get(&view)?.2;
            snapper.snap_guided(
                &snap_cfg,
                p,
                &pane_project,
                drawing_from,
                &track,
                &guides,
                Some(xf.to_world(p).extend(0.0)),
            )
        });
    let tool_snap = tracked.as_ref().map(|t| t.snap);
    let tool_align = tracked.as_ref().and_then(|t| t.align);
    let now = ui.input(|i| i.time);
    if guided {
        // rest on a point or edge (or press Q over it) to pick it as a
        // reference, and again to drop it
        let under = response.hover_pos().and_then(|p| {
            let snapper = &state.snaps.get(&view)?.2;
            let s = snapper.snap(&snap_cfg, p, &pane_project, None, None)?;
            snapper.reference_at(&s)
        });
        let q = response.hovered()
            && !ui.ctx().egui_wants_keyboard_input()
            && ui.input(|i| i.key_pressed(egui::Key::Q) && !i.modifiers.command);
        click_is_not_a_rest(state, tool, response.clicked(), under, now);
        let under = under.filter(|_| host.is_none());
        if let Some(msg) = rest_reference(state, view, under, now, q) {
            // a tool part way through keeps its own hint: how to finish
            // matters more than the guide lines' note
            if state.pending.is_none() && state.parts.path.is_empty() && state.outline.is_none() {
                state.status = Some(msg);
            }
        }
        // the app draws no frames while nothing moves (A6), so a pointer
        // resting still would never be seen to rest: ask for one frame
        // just after the dwell (MU21, guide lines stopped appearing)
        if matches!(state.ref_rest, Some((_, _, false))) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(REST_DWELL + 0.05));
        }
        // guide lines last while they are used: a snap following one, or
        // a shape being drawn from them; otherwise they go after a while
        let following = tracked.as_ref().is_some_and(|t| !t.guides.is_empty());
        fade_references(state, view, now, following || drawing_from.is_some());
        if state.refs.get(&view).is_some_and(|r| !r.is_empty()) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(REF_FADE / 4.0));
        }
    } else {
        // a point snapped to and rested on while drawing is tracked too
        rest_on(state, view, tool_snap, drawing_from.is_some(), now);
        // a frame after the dwell, as nothing repaints while the pointer
        // rests (A6, MU21)
        if state.rest.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(450));
        }
    }
    if guided {
        // the references, and their guides faintly; those a snap follows
        // are drawn again with it
        if let Some(refs) = state.refs.get(&view).filter(|_| host.is_none()) {
            snaps::draw_references(&painter, &pane_project, refs);
        }
        snaps::draw_guides(&painter, &pane_project, &guides, &[]);
        if response.hovered() {
            let on = tracked.as_ref().and_then(|t| t.guides.first().copied());
            state.hover_guide = on.and_then(|i| guides.get(i)).map(|g| (view, *g));
        }
        // K keeps the guide line under the pointer
        if response.hovered()
            && !ui.ctx().egui_wants_keyboard_input()
            && ui.input(|i| i.key_pressed(egui::Key::K) && !i.modifiers.command)
        {
            let g = state.hover_guide.filter(|h| h.0 == view).map(|h| h.1);
            state.status = Some(match keep_guide(doc, kind, g) {
                Ok(msg) => msg,
                Err(e) => format!("Couldn't do that: {e}"),
            });
        }
    }
    let pointer_world = response.hover_pos().map(|p| match kind {
        PaneKind::Plan { .. } | PaneKind::Section { .. } if !shift => {
            tool_snap.map_or(xf.to_world(p), |s| s.p.truncate())
        }
        PaneKind::Plan { level } => snap(
            doc,
            level,
            &xf,
            xf.to_world(p),
            state.pending,
            shift,
            &[],
            &snap_cfg,
        ),
        PaneKind::Axonometric | PaneKind::Schedule => xf.to_world(p),
        PaneKind::Section { .. } => {
            let mut p = xf.to_world(p);
            if let (true, Some(a)) = (shift, state.pending) {
                let d = p - a;
                p = if d.x.abs() >= d.y.abs() {
                    DVec2::new(p.x, a.y)
                } else {
                    DVec2::new(a.x, p.y)
                };
            }
            if snap_cfg.allows(SnapKind::Grid) && snap_cfg.step_len() > 0.0 {
                let s = snap_cfg.step_len();
                (p / s).round() * s
            } else {
                p
            }
        }
    });

    // the tracker: a typed length and angle take over from the pointer
    // while a line is being drawn, and Enter places it (2.10)
    let line_tool = tool.takes_typed_length() && matches!(kind, PaneKind::Plan { .. });
    // a stair's tracker has a Gap field: between the runs of a half-turn;
    // a gap typed is kept for the next stairs (Stair Guides REV02 §5, 9 A)
    state.tracker.gap = tool == Tool::Stair && !state.members.stair_spiral;
    // a guide's copy has one field: how far across it goes (#34)
    state.tracker.across = tool == Tool::Guide && state.guide_edge.is_some();
    let mut tracker_enter = false;
    let hover_world = match (state.pending, pointer_world, line_tool) {
        (Some(a), Some(b), true) if response.hovered() => {
            if state.tracker.keys(ui.ctx()) == crate::tracker::Keys::Enter {
                tracker_enter = true;
            }
            if let Some(g) = state.tracker.gap_typed() {
                state.members.stair_gap = g;
            }
            match (tool, state.guide_edge, state.tracker.length()) {
                // a typed distance is the copy's distance across, to the
                // pointer's side (MU70: it went along the pointer instead)
                (Tool::Guide, Some((e0, e1)), Some(len)) => {
                    let d = (e1 - e0).normalize_or_zero();
                    let n = d.perp();
                    let side = if (b - a).dot(n) < 0.0 { -1.0 } else { 1.0 };
                    Some(a + n * side * len)
                }
                (Tool::Guide, Some(_), None) => Some(b),
                _ => Some(state.tracker.end(a, b)),
            }
        }
        _ => {
            if state.pending.is_none() && !state.tracker.fields.iter().all(String::is_empty) {
                state.tracker.clear();
            }
            pointer_world
        }
    };

    // the Slab tool's modes, and Line / Arc while clicking a slab outline
    if let PaneKind::Plan { level } = kind {
        let slab_outline = state
            .outline
            .as_ref()
            .is_some_and(|o| o.level == level && !o.is_wall());
        if tool.draws_outline() || slab_outline {
            let modes = tool.draws_outline();
            if let Some(msg) =
                crate::slabtool::controls(ui, rect, Some(view), modes, doc, state, selection)
            {
                state.status = Some(msg);
            }
        }
    }

    // drawing a slab, or redrawing a slab or wall, on this plan's level
    if let (Some(o), PaneKind::Plan { level }) = (&state.outline, kind) {
        if o.level == level {
            let o = o.clone();
            let project = |p: DVec3| Some(xf.to_screen(p.truncate()));
            if let (Some(t), false) = (&tracked, shift) {
                let from = o.points.last().map(|a| xf.to_screen(*a));
                snaps::draw_guides(&painter, &project, &guides, &t.guides);
                snaps::draw_guided(&painter, &project, t, &guides, from);
            }
            o.draw(&painter, &project, hover_world);
            // keys go to the pane under the pointer, as another pane may
            // show the same outline
            let here = response.hovered();
            let typing = ui.ctx().egui_wants_keyboard_input();
            let (enter, escape, backspace) = ui.input(|i| {
                (
                    here && (i.key_pressed(egui::Key::Enter)
                        || !typing && !i.modifiers.any() && i.key_pressed(egui::Key::C)),
                    here && i.key_pressed(egui::Key::Escape),
                    here && !typing && i.key_pressed(egui::Key::Backspace),
                )
            });
            let click = match (response.clicked(), hover_world, response.hover_pos()) {
                (true, Some(p), Some(h)) => Some((p, o.near_first(h, &project))),
                _ => None,
            };
            let right = response.secondary_clicked() && !shift;
            let double = response.double_clicked();
            let enter = enter || std::mem::take(&mut state.close_outline);
            let arc = state.slab_arc;
            let next = state
                .outline
                .as_mut()?
                .input_more(enter, escape, right, click, arc, double, backspace);
            match next {
                Next::Drawing(hint) => state.status = Some(hint),
                Next::Cancel(msg) => {
                    state.outline = None;
                    state.status = Some(msg.into());
                }
                Next::Finish(closed) => {
                    let o = state.outline.take()?;
                    return match o.finish(doc, closed) {
                        Ok((msg, made)) => {
                            if let Some(id) = made {
                                selection.only(id);
                            }
                            state.status = Some(msg);
                            None
                        }
                        Err(e) => Some(e.to_string()),
                    };
                }
            }
            return None;
        }
    }

    // pointer edits: keyboard move, rotate and scale; sketches; grips
    let space = match kind {
        PaneKind::Plan { .. } => Some(Space::Plan),
        PaneKind::Section { start, end } => Some(Space::Section { start, end }),
        PaneKind::Axonometric | PaneKind::Schedule => None,
    };
    // temporary dimensions of the one element selected: click a value
    // and type, or click its padlock (2.10 part 3)
    // a level's in a section or elevation (A5-2)
    let level_picked = selection.active.is_some_and(|id| {
        doc.model()
            .get(id)
            .is_some_and(|e| matches!(e.kind, ElementKind::Level(_)))
            || crate::tempdims::dim_levels(doc.model(), id).is_some()
    });
    if response.clicked() {
        state.temp.click = response.interact_pointer_pos().map(|p| xf.to_world(p));
    }
    let temp_level = match kind {
        PaneKind::Plan { level } if !level_picked => Some(level),
        PaneKind::Section { .. } if level_picked => Some(view),
        _ => None,
    };
    if let (Some(level), Tool::Select, false) = (temp_level, tool, state.temp_dims_off) {
        let quiet = state.modal.is_some()
            || state.grip.is_some()
            || state.edit.is_some()
            || state.anno_drag.is_some()
            || state.boxsel.is_some()
            || response.dragged();
        if let Some(r) = crate::tempdims::interact(
            ui,
            doc,
            view,
            level,
            &xf,
            &painter,
            &response,
            &mut state.temp,
            selection,
            &mut state.status,
            &mut state.hint,
            quiet,
        ) {
            return r;
        }
    }
    if let Some(space) = space {
        let to_model = |pos: Pos2, _axis: Option<creabim_core::Axis>, pivot: DVec3| {
            Some(pane_to_model(space, xf.to_world(pos), pivot.z))
        };
        let project = |p: DVec3| Some(xf.to_screen(model_to_pane(space, p)));
        // the element another view's pointer would place, here too
        if let Some((from, g)) = &state.ghost_shared {
            if *from != state.pane_now {
                crate::ghost::draw(&painter, &project, g, accent_selected());
            }
        }
        // what the modifier being set shows: its centre, plane or cuts
        crate::modguides::draw(
            &painter,
            &state.mod_guides,
            state.mod_guides.ink.unwrap_or(accent_selected()),
            matches!(space, Space::Plan),
            &project,
        );
        // Divide's marks: ticks across in plans, lines standing in sections
        for (a, b, plan) in &state.divide_marks {
            if *plan != matches!(space, Space::Plan) {
                continue;
            }
            let (pa, pb) = (
                xf.to_screen(model_to_pane(space, *a)),
                xf.to_screen(model_to_pane(space, *b)),
            );
            if pa.distance(pb) > 1.0 {
                if state.divide_guides {
                    crate::snaps::dashed_clipped(
                        &painter,
                        &[pa, pb],
                        Stroke::new(1.5, GUIDE_COLOR),
                        8.0,
                        5.0,
                    );
                } else {
                    painter.line_segment([pa, pb], Stroke::new(2.0, accent_selected()));
                }
            }
        }
        let mut taken = vec![];
        for (at, text) in &state.divide_labels {
            let plan_label =
                (at.z - state.divide_marks.first().map_or(at.z, |m| m.0.z)).abs() < 1e-6;
            if matches!(space, Space::Plan) && !plan_label {
                continue;
            }
            divide_label(
                &painter,
                xf.to_screen(model_to_pane(space, *at)),
                text,
                accent_selected(),
                &mut taken,
            );
        }
        // the pane's snaps, lent out while a move or rotate runs here
        let lent = std::mem::take(&mut state.snaps);
        let plane_z = match kind {
            PaneKind::Plan { level } => doc.model().level(level).map_or(0.0, |l| l.elevation),
            _ => 0.0,
        };
        let snap_fn = |pos: Pos2, from: Option<DVec3>| {
            let snapper = &lent.get(&view)?.2;
            let to_pane = |p: DVec3| model_to_pane(space, p).extend(0.0);
            let pane_project = |p: DVec3| Some(xf.to_screen(p.truncate()));
            let free = Some(xf.to_world(pos).extend(0.0));
            let s = snapper.snap(&snap_cfg, pos, &pane_project, from.map(to_pane), free)?;
            let p = pane_to_model(space, s.p.truncate(), from.map_or(plane_z, |f| f.z));
            Some((p, snaps::Snap { p, ..s }))
        };
        // Move on faces: the edge under the pointer stands for the face
        // through it, square to the paper
        let pushable: std::collections::HashSet<ElementId> = if state.modal.is_some() {
            let model = doc.model();
            plan.items
                .iter()
                .map(|i| i.id())
                .filter(|id| {
                    model
                        .get(*id)
                        .is_some_and(|e| creabim_core::pushpull::can_push(&e.kind))
                })
                .collect()
        } else {
            Default::default()
        };
        // the selection's own faces win over those lying on them
        let chosen = selection.all();
        let pick_face = |pos: Pos2| {
            let takes = |id: ElementId| {
                pushable
                    .contains(&id)
                    .then_some(u8::from(chosen.contains(&id)))
            };
            let px = 1.0 / xf.scale as f64;
            crate::paneface::face_at(
                &plan.items,
                space,
                xf.to_world(pos),
                6.0 * px,
                plane_z,
                5.0 * px,
                &takes,
            )
            .map(|f| f.pick)
        };
        // a line of the drawing under the pointer, for Mirror's picked
        // line: a wall's face, an edge, a grid line, a centre line (MU142)
        let pick_line = |pos: Pos2| {
            crate::aligntool::pick_line(
                &plan.items,
                &centre_lines,
                xf.to_world(pos),
                6.0 / xf.scale as f64,
            )
            .map(|(_, a, b, _)| {
                (
                    pane_to_model(space, a, plane_z),
                    pane_to_model(space, b, plane_z),
                )
            })
        };
        let map = PaneMap {
            to_model: &to_model,
            project: &project,
            look: match space {
                Space::Section { start, end } => {
                    (end - start).normalize_or_zero().perp().extend(0.0)
                }
                _ => -DVec3::Z,
            },
            snap: &snap_fn,
            pick_face: Some(&pick_face),
            pick_line: Some(&pick_line),
        };
        let hover_now = state.hover.clone();
        let works_now = Works::of(doc.model());
        let pick = |p: Pos2| {
            hover_now.as_ref().and_then(|h| h.at(pane, p)).or_else(|| {
                let w = xf.to_world(p);
                let mut under = hit_all(&plan.items, w, 6.0 / xf.scale as f64, mm);
                if matches!(kind, PaneKind::Plan { .. }) {
                    works_now.first(&mut under, w);
                }
                under.first().copied()
            })
        };
        let edits = keyboard_edits(
            ui, doc, view, space, &painter, &response, state, selection, &map, &pick,
        );
        state.snaps = lent;
        if let Some(r) = edits {
            return r;
        }
        if tool == Tool::Sketch {
            let level_z = match kind {
                PaneKind::Plan { level } => doc.model().level(level).map_or(0.0, |l| l.elevation),
                _ => 0.0,
            };
            let hover = hover_world.map(|w| pane_to_model(space, w, level_z));
            // a point snapped to something, or with increments off, is kept as is
            let exact = !snap_cfg.allows(SnapKind::Grid)
                || (!shift && tool_snap.is_some_and(|s| s.kind != SnapKind::Grid));
            if let Some(r) = sketch_tool(
                ui, doc, view, space, &xf, &painter, &response, state, selection, hover, shift,
                exact,
            ) {
                return r;
            }
        }
        // a grid line's ends in a section, dragged up and down for this
        // view only (MU70), before the grips: a plan's grips of a grid lie
        // on the ground here, and a drag on it moved the whole line (MU71)
        let grid_in_section = matches!(kind, PaneKind::Section { .. })
            && selection
                .active
                .and_then(|id| doc.model().get(id))
                .is_some_and(|e| matches!(e.kind, ElementKind::Grid(_)));
        if let (PaneKind::Section { .. }, Tool::Select) = (kind, tool) {
            if let Some(r) =
                drag_grid_ends_section(doc, view, plan, &painter, &xf, &response, state, selection)
            {
                return r;
            }
        }
        if tool == Tool::Select && !grid_in_section {
            let level_z = match kind {
                PaneKind::Plan { level } => doc.model().level(level).map_or(0.0, |l| l.elevation),
                _ => 0.0,
            };
            let look = match space {
                Space::Section { start, end } => {
                    (end - start).normalize_or_zero().perp().extend(0.0)
                }
                _ => -DVec3::Z,
            };
            let snapper = &state.snaps[&view].2;
            let ray = |pos: Pos2| {
                let p = pane_to_model(space, xf.to_world(pos), level_z);
                Some(handles::Ray {
                    o: p - look * 1000.0,
                    d: look,
                })
            };
            // a stair's grip lines up with the other stairs on its level
            // on its own (Stair Guides REV02 §1); Alt goes free of them
            let stair_lines = match (kind, selection.active) {
                (PaneKind::Plan { level }, Some(id))
                    if snap_cfg.allows(SnapKind::OnTempGuide)
                        && !ui.input(|i| i.modifiers.alt)
                        && matches!(
                            doc.model().get(id).map(|e| &e.kind),
                            Some(ElementKind::Stair(_))
                        ) =>
                {
                    snaps::stair_lines(doc.model(), level, Some(id))
                }
                _ => vec![],
            };
            let snap = |pos: Pos2, from: Option<DVec3>| {
                let to_pane = |p: DVec3| model_to_pane(space, p).extend(0.0);
                let pane_project = |p: DVec3| Some(xf.to_screen(p.truncate()));
                let free = Some(xf.to_world(pos).extend(0.0));
                if !stair_lines.is_empty() {
                    let g = snaps::stair_guides(
                        &stair_lines,
                        xf.to_world(pos),
                        STAIR_REACH / xf.scale as f64,
                    );
                    let hit = snapper.snap_guided(
                        &snap_cfg,
                        pos,
                        &pane_project,
                        from.map(to_pane),
                        &[],
                        &g,
                        free,
                    );
                    if let Some(h) = hit.filter(|h| !h.guides.is_empty()) {
                        snaps::draw_guides(&painter, &pane_project, &g, &h.guides);
                        // the line it lines up with, in words, under the
                        // snap's own label
                        if let Some(gk) = h.guides.first().and_then(|i| g.get(*i)) {
                            painter.text(
                                xf.to_screen(h.snap.p.truncate()) + egui::vec2(12.0, 26.0),
                                egui::Align2::LEFT_TOP,
                                gk.kind.label(),
                                egui::FontId::proportional(12.0),
                                snaps::GUIDE,
                            );
                        }
                        let z = from.map_or(level_z, |f| f.z);
                        let p = pane_to_model(space, h.snap.p.truncate(), z);
                        return Some((p, snaps::Snap { p, ..h.snap }));
                    }
                }
                let s = snapper.snap(&snap_cfg, pos, &pane_project, from.map(to_pane), free)?;
                let z = from.map_or(level_z, |f| f.z);
                let p = pane_to_model(space, s.p.truncate(), z);
                Some((p, snaps::Snap { p, ..s }))
            };
            // what is under others (picked with Tab, or in X-ray) can be
            // dragged by its body too
            let hover_now = state.hover.clone();
            let grab_body = |pos: Pos2, id: ElementId| {
                let w = xf.to_world(pos);
                let on = hit_all(&plan.items, w, 6.0 / xf.scale as f64, mm).contains(&id)
                    || hover_now.as_ref().and_then(|h| h.at(pane, pos)) == Some(id);
                on.then(|| pane_to_model(space, w, level_z))
            };
            let grab_snap = |pos: Pos2, id: ElementId| {
                let only: Vec<PlanItem> = plan
                    .items
                    .iter()
                    .filter(|i| i.id() == id)
                    .cloned()
                    .collect();
                let own = Snapper::from_drawing(&only, &[]);
                let pane_project = |p: DVec3| Some(xf.to_screen(p.truncate()));
                let s = own.snap(&snap_cfg.points_only(), pos, &pane_project, None, None)?;
                Some(pane_to_model(space, s.p.truncate(), level_z))
            };
            let input = PaneInput {
                ray: &ray,
                project: &project,
                look,
                snap: &snap,
                grab_body: &grab_body,
                grab_snap: &grab_snap,
                body_normal: if matches!(space, Space::Section { .. }) {
                    look
                } else {
                    DVec3::Z
                },
                view: Some(view),
                face: &|_| None,
            };
            // a selected stair whose line is short: the treads it still
            // needs past the line's end, as a red ghost with their count
            // (Stair Guides REV00 §2, A)
            if let (PaneKind::Plan { level }, Some(id)) = (kind, selection.active) {
                // its landing sizes and its fixed end's padlock show in
                // Edit mode only: on a stair merely selected they sat over
                // its treads and numbers, and the padlock explained
                // nothing (MU103)
                let editing = state.edit.is_some() && state.edit == selection.active;
                draw_short_treads(&painter, &xf, doc.model(), level, id, editing);
                // its fixed end's padlock: a click swaps the end (REV00 §3)
                let lock = if editing {
                    stair_lock(&painter, &xf, doc.model(), level, id, ui.ctx())
                } else {
                    None
                };
                if let Some((rect, foot)) = lock {
                    // what it is, on hover
                    if ui
                        .ctx()
                        .pointer_hover_pos()
                        .is_some_and(|p| rect.expand(4.0).contains(p))
                    {
                        let text = if foot {
                            "Its foot stays put when its treads change. Click: its head stays instead."
                        } else {
                            "Its head stays put when its treads change. Click: its foot stays instead."
                        };
                        let ink = ui.visuals().text_color();
                        let galley = painter.layout_no_wrap(
                            text.into(),
                            egui::FontId::proportional(12.0),
                            ink,
                        );
                        let at = rect.right_top() + egui::vec2(8.0, 0.0);
                        let bg = egui::Rect::from_min_size(at, galley.size()).expand(4.0);
                        painter.rect_filled(bg, 4.0, ui.visuals().extreme_bg_color);
                        painter.rect_stroke(
                            bg,
                            4.0,
                            ui.visuals().widgets.noninteractive.bg_stroke,
                            egui::StrokeKind::Outside,
                        );
                        painter.galley(at, galley, ink);
                    }
                    let hit = response.clicked()
                        && response
                            .interact_pointer_pos()
                            .is_some_and(|p| rect.expand(4.0).contains(p));
                    if hit {
                        if let Some(mut e) = doc.model().get(id).cloned() {
                            if let ElementKind::Stair(s) = &mut e.kind {
                                s.fixed_foot = !foot;
                            }
                            let before = doc.model().get(id).cloned().unwrap();
                            let _ = doc.commit(
                                creabim_core::Transaction::new("Stair fixed end")
                                    .replace(before, e),
                            );
                            state.status = Some(if foot {
                                "Its head stays put now: more treads grow it from the foot.".into()
                            } else {
                                "Its foot stays put now: more treads grow it from the head.".into()
                            });
                            return None;
                        }
                    }
                }
            }
            let mut status = None;
            let edit = state.edit.is_some() && state.edit == selection.active;
            let busy = handles::interact(
                doc,
                &painter,
                &response,
                selection.active,
                None,
                edit,
                &mut state.grip,
                &mut state.grips,
                &input,
                &mut status,
            );
            if status.is_some() {
                state.status = status;
            }
            // a corner palette's Mirror or Scale (MU142)
            if let Some(c) = handles::take_palette_command() {
                match c.modal() {
                    Some(m) => state.armed = Some(m),
                    None => state.commands.push((c, None)),
                }
            }
            if busy {
                return None;
            }
        }
    }

    // a level line or head in a section drags the level up and down; after
    // the selection's grips, so a wall's top grip lying on a level line
    // moves the wall's top, not the level
    // the floor plans' ranges, drawn and dragged by their grips (MU34 A1)
    if let (PaneKind::Section { .. }, true) = (kind, state.plan_ranges) {
        if let Some(r) = plan_ranges(
            doc,
            &painter,
            &xf,
            &response,
            state,
            selection,
            tool == Tool::Select,
            view,
            plan,
        ) {
            return r;
        }
    }
    // a section's level tags, and a plan's site levels (MU142)
    if let (PaneKind::Section { .. } | PaneKind::Plan { .. }, Tool::Select) = (kind, tool) {
        if let Some(r) = drag_tag(doc, &painter, plan, &xf, &response, state, view, mm) {
            return r;
        }
    }
    if let (PaneKind::Section { .. }, Tool::Select) = (kind, tool) {
        let snapped = tool_snap.filter(|s| !shift && s.kind != SnapKind::Grid);
        if let Some(r) = drag_level(doc, plan, &xf, &response, state, selection, mm, snapped) {
            return r;
        }
    }
    // a selected section line in a plan can be dragged by its ends or middle
    if let (PaneKind::Plan { .. }, Tool::Select) = (kind, tool) {
        if let Some(r) = drag_section_line(doc, &painter, &xf, &response, state, selection.active) {
            return r;
        }
    }
    // a tag, dimension or note can be dragged into place, once selected:
    // a drag that starts on one that isn't draws a box instead (MU20)
    let press_on_selected = response.drag_started_by(egui::PointerButton::Primary)
        && ui.input(|i| i.pointer.press_origin()).is_some_and(|p0| {
            hit_all(&plan.items, xf.to_world(p0), 6.0 / scale as f64, mm)
                .into_iter()
                .any(|id| selection.contains(id))
                // or on a grip of one, such as a lot note's (MU98_06)
                || crate::annotate::on_grip(doc.model(), &plan.items, selection, &xf, p0, mm)
        });
    let annotation_drag = state.anno.grab.is_some()
        || !response.drag_started_by(egui::PointerButton::Primary)
        || press_on_selected;
    if tool == Tool::Select && annotation_drag {
        if let Some(r) = drag_annotation(doc, view, plan, &xf, &response, state, selection, mm) {
            return r;
        }
        let view_def = match doc.model().get(view).map(|e| &e.kind) {
            Some(ElementKind::View(v)) => Some(v.clone()),
            _ => None,
        };
        let leader_drag = matches!(state.anno.grab, Some(crate::annotate::Grab::Leader(_)));
        let snapped = match (&view_def, response.interact_pointer_pos(), leader_drag) {
            (Some(v), Some(p), true) => {
                let snapper = state.snaps.get(&view).map(|s| &s.2);
                let (hp, at, _) = dim_snap(doc, v, kind, snapper, &snap_cfg, &xf, p);
                Some((hp, at))
            }
            _ => None,
        };
        if let Some(r) = crate::annotate::drag(
            doc,
            Some(view),
            view_def.as_ref(),
            &plan.items,
            &xf,
            &response,
            &mut state.anno,
            selection,
            mm,
            snapped,
        ) {
            return r;
        }
    }

    // drag on empty paper to select with a box; with the Footing tool, a
    // box anywhere on a plan puts footings under the walls (Strip) or the
    // columns (Pad) it takes, as Revit's multiple selection does
    let footing_box =
        tool == Tool::Footing && matches!(kind, PaneKind::Plan { .. }) && state.pending.is_none();
    // Stretch armed: a box anywhere on a plan takes the corners inside it
    let stretch_box =
        state.stretch_box && matches!(kind, PaneKind::Plan { .. }) && state.modal.is_none();
    if (tool == Tool::Select && matches!(kind, PaneKind::Plan { .. } | PaneKind::Section { .. }))
        || footing_box
        || stretch_box
    {
        if response.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui.input(|i| i.pointer.press_origin());
            if let Some(p0) = origin {
                // as in Revit: a drag starts a box unless it starts on
                // something selected (that drag moves it); grid lines,
                // dimensions and the rest under the press don't stop it
                // (MU19)
                let on_selected = hit_all(&plan.items, xf.to_world(p0), 6.0 / scale as f64, mm)
                    .into_iter()
                    .any(|id| selection.contains(id));
                if !on_selected || footing_box || stretch_box {
                    state.boxsel = Some((p0, response.id));
                    boxsel::trace(p0, true);
                }
            }
        }
        if let Some((p0, _)) = state.boxsel.filter(|b| b.1 == response.id) {
            let now = ui.input(|i| i.pointer.latest_pos()).unwrap_or(p0);
            boxsel::trace(now, false);
            boxsel::draw(&painter, p0, now);
            let ended = response.drag_stopped() || !response.dragged();
            if stretch_box {
                stretch_box_frame(doc, plan, &xf, &painter, p0, now, ended, state, selection);
                return None;
            }
            {
                let parts = plan.items.iter().filter_map(|it| {
                    if creabim_geom::plan::is_underlay(it) {
                        return None;
                    }
                    let (lo, hi) = it.bounds()?;
                    Some((
                        it.id(),
                        Rect::from_two_pos(xf.to_screen(lo), xf.to_screen(hi)),
                    ))
                });
                let ids = boxsel::pick(p0, now, parts)
                    .into_iter()
                    .filter(|id| box_selectable(doc, *id));
                if !ended {
                    // what it would take, counted as it is drawn (MU39)
                    if !footing_box {
                        boxsel::draw_count(&painter, now, ids.count());
                    }
                    return None;
                }
                if footing_box {
                    let pad = crate::memberstool::footing_kind(doc.model(), state)
                        == creabim_core::FootingKind::Pad;
                    let hosts: Vec<ElementId> = ids
                        .filter(|id| match doc.model().get(*id).map(|e| &e.kind) {
                            Some(ElementKind::Wall(w)) => !pad && w.curtain.is_none(),
                            Some(ElementKind::Column(_)) => pad,
                            _ => false,
                        })
                        .collect();
                    if hosts.is_empty() {
                        state.status = Some(if pad {
                            "The box took no columns: drag it over the columns to put pads under."
                        } else {
                            "The box took no walls: drag it over the walls to put strips under."
                        }
                        .into());
                    }
                    state.want_footings = hosts;
                } else {
                    let ctrl = ui.input(|i| i.modifiers.command);
                    boxsel::apply(selection, ids.collect(), boxsel::combine_with(shift, ctrl));
                    state.boxed = true;
                }
                state.boxsel = None;
            }
            return None;
        }
    }

    // dimension and tag tools in progress
    let view_def = match doc.model().get(view).map(|e| &e.kind) {
        Some(ElementKind::View(v)) => Some(v.clone()),
        _ => None,
    };
    let dim_snapper = state.snaps.get(&view).map(|s| &s.2);
    let dim_guided = match (tool, &view_def, response.hover_pos()) {
        (t, Some(v), Some(p)) if t == Tool::Dimension || crate::annotate::is_tool(t) => {
            Some(dim_snap_guided(
                doc,
                v,
                kind,
                dim_snapper,
                &snap_cfg,
                &xf,
                p,
                &guides,
                state.dim.last().map(|d| d.1.extend(0.0)),
                &snaps::track_points(
                    &state
                        .dim
                        .iter()
                        .map(|d| d.1.extend(0.0))
                        .collect::<Vec<_>>(),
                    &[],
                ),
            ))
        }
        _ => None,
    };
    let dim_hit = dim_guided.as_ref().map(|(h, at, k, _)| (*h, *at, *k));
    let accent = crate::theme::interaction();
    if let Some((_, at, k, used)) = &dim_guided {
        let c = xf.to_screen(*at);
        match k {
            Some(k) if *k != SnapKind::Grid => {
                let from = state.dim.first().map(|d| xf.to_screen(d.1));
                let hit = snaps::Guided {
                    snap: snaps::Snap {
                        p: at.extend(0.0),
                        kind: *k,
                        host: None,
                    },
                    align: None,
                    guides: used.clone(),
                };
                snaps::draw_guides(&painter, &pane_project, &guides, used);
                snaps::draw_guided(&painter, &pane_project, &hit, &guides, from);
                if response.hovered() && !used.is_empty() {
                    state.hover_guide = guides.get(used[0]).map(|g| (view, *g));
                }
            }
            _ => {
                painter.circle_stroke(c, 5.0, Stroke::new(1.5, accent));
            }
        }
    }
    let dim_hover = dim_hit.map(|(h, at, _)| (h, at));
    // the way a linear dimension measures: as set, held with Shift, or
    // picked from where its line is going
    let dim_axis = match (state.dim.as_slice(), response.hover_pos()) {
        ([(_, a), (_, b)], Some(h)) => {
            let picked = state
                .dim_axis
                .unwrap_or_else(|| creabim_core::DimAxis::pick(*a, *b, xf.to_world(h)));
            if shift {
                *state.dim_lock.get_or_insert(picked)
            } else {
                state.dim_lock = None;
                picked
            }
        }
        _ => {
            state.dim_lock = None;
            state.dim_axis.unwrap_or_default()
        }
    };
    let dim_look = annotate::look_of(doc.model(), state.anno.dim_style, None);
    if crate::annotate::is_tool(tool) {
        let raw = response.hover_pos().map(|p| xf.to_world(p));
        crate::annotate::preview(
            &painter,
            &xf,
            tool,
            &state.anno,
            dim_hover.map(|h| h.1),
            raw,
            &plan.items,
            6.0 / scale as f64,
            mm,
        );
    }
    match (state.dim.as_slice(), dim_hover, response.hover_pos()) {
        ([(_, a)], Some((_, b)), _) if a.distance(b) > 1e-6 => {
            preview_dimension(&painter, &xf, *a, b, dim_axis, 0.0, &dim_look, mm, accent);
        }
        ([(_, a), (_, b)], _, Some(h)) => {
            let off = dim_offset(dim_axis, *a, *b, xf.to_world(h));
            preview_dimension(&painter, &xf, *a, *b, dim_axis, off, &dim_look, mm, accent);
            let what = if state.dim_lock.is_some() {
                format!("{} (held by Shift)", dim_axis.label())
            } else if state.dim_axis.is_some() {
                dim_axis.label().to_string()
            } else {
                format!("{} (Shift holds it)", dim_axis.label())
            };
            painter.text(
                h + Vec2::new(14.0, 16.0),
                egui::Align2::LEFT_TOP,
                what,
                egui::FontId::proportional(12.0),
                accent,
            );
        }
        _ => {}
    }
    if let (Tool::Tag, Some(host), Some(h)) = (tool, state.tag_host, response.hover_pos()) {
        if let Some(v) = &view_def {
            if let Some(proj) = annotate::Projector::of(doc.model(), v) {
                if let Some(c) = annotate::centre(doc.model(), host) {
                    let (text, round) = annotate::tag_text(doc.model(), host);
                    let item = PlanItem::Tag {
                        id: host,
                        anchor: proj.project(c),
                        at: xf.to_world(h),
                        elbow: None,
                        text,
                        round,
                        look: annotate::look_of(doc.model(), None, None),
                    };
                    draw_item(&painter, &xf, &item, true, mm);
                }
            }
        }
    }

    // the element a click would place, as a ghost (A7-4)
    if let (PaneKind::Plan { level }, Some(h), true) = (kind, hover_world, response.hovered()) {
        if crate::ghost::shows(tool) {
            // the interaction colour, as the 3D view's ghost (MU39)
            let accent = accent_selected();
            match crate::ghost::cached(doc.model(), level, tool, h, state) {
                Some(g) => {
                    crate::ghost::draw(&painter, &|p| Some(xf.to_screen(p.truncate())), &g, accent);
                    state.ghost_now = Some((state.pane_now, g));
                }
                None if matches!(tool, Tool::Door | Tool::Window | Tool::Opening) => {
                    if let Some(at) = response.hover_pos() {
                        let look =
                            crate::handles::PillLook::of(&crate::theme::Palette::of(ui.ctx()));
                        crate::handles::plain_pill(
                            &painter,
                            at + Vec2::new(16.0, 18.0),
                            "Move onto a wall",
                            &look,
                        );
                    }
                }
                None => {}
            }
        }
    }
    // the object about to be placed, under the pointer
    if let (Tool::Object, PaneKind::Plan { level }, Some(h)) = (tool, kind, hover_world) {
        if ui.input(|i| !i.modifiers.command && i.key_pressed(egui::Key::Space))
            && !ui.ctx().egui_wants_keyboard_input()
        {
            state.objects.rotation = (state.objects.rotation + 90.0).rem_euclid(360.0);
        }
        crate::objecttool::preview(
            &painter,
            &|p| Some(xf.to_screen(p.truncate())),
            doc.model(),
            level,
            h,
            state,
            false,
        );
    }
    // a callout's box or a detail component being drawn, and Enter to
    // finish the component
    if matches!(tool, Tool::Callout | Tool::Detail) {
        crate::detailtool::preview(
            doc.model(),
            &painter,
            &xf,
            tool,
            state,
            view,
            hover_world,
            mm,
        );
        let enter = response.hovered()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && !ui.ctx().egui_wants_keyboard_input();
        // a double-click finishes too (its first click put the last point)
        let space = response.hovered()
            && ui.input(|i| !i.modifiers.command && i.key_pressed(egui::Key::Space))
            && !ui.ctx().egui_wants_keyboard_input();
        if tool == Tool::Detail
            && space
            && state.details.kind == creabim_core::DetailKind::Membrane
            && state.details.view == Some(view)
            && !state.details.path.is_empty()
        {
            state.details.swap = !state.details.swap;
            state.status = Some("The membrane goes on the other side.".into());
        }
        let double = response.double_clicked() && crate::detailtool::finishable(state);
        if tool == Tool::Detail && (enter || double) && state.details.view == Some(view) {
            if let Err(e) = crate::detailtool::finish(doc, state, selection) {
                return Some(e.to_string());
            }
            // the double-click's second click is not a new path's first
            if double {
                return None;
            }
        }
    }
    // Enter finishes a stair line that has not reached the floor above:
    // its last flight runs on past the line
    if tool == Tool::Stair
        && !state.members.stair_line.is_empty()
        && response.hovered()
        && ui.input(|i| i.key_pressed(egui::Key::Enter))
        && !ui.ctx().egui_wants_keyboard_input()
    {
        if let PaneKind::Plan { level } = kind {
            if let Err(e) = crate::memberstool::finish_stair(doc, level, state, selection) {
                return Some(e.to_string());
            }
        }
    }
    // a sweep traced: its band (the profile's width either side of the
    // path) as a ghost, and its section at the pointer, so a wrong profile
    // shows at once (MU37)
    // a contour line being drawn: its points so far and on to the pointer
    if let (Tool::Terrain, PaneKind::Plan { .. }, Some(b)) = (tool, kind, hover_world) {
        if matches!(
            state.parts.ground,
            crate::partstool::GroundDraw::Contour
                | crate::partstool::GroundDraw::Pad
                | crate::partstool::GroundDraw::Pit
                | crate::partstool::GroundDraw::Trench
                | crate::partstool::GroundDraw::Road
                | crate::partstool::GroundDraw::Drain
                | crate::partstool::GroundDraw::Path
        ) && !state.parts.path.is_empty()
        {
            let ghost = crate::theme::interaction();
            let mut pts: Vec<Pos2> = state.parts.path.iter().map(|p| xf.to_screen(*p)).collect();
            pts.push(xf.to_screen(b));
            painter.add(egui::Shape::line(pts.clone(), Stroke::new(1.5, ghost)));
            for p in &pts[..pts.len() - 1] {
                painter.circle_filled(*p, 2.5, ghost);
            }
            // an outline (a pad, a pit) also shows the edge that would
            // close it, dashed and faint: what Enter makes of it now
            let closes = matches!(
                state.parts.ground,
                crate::partstool::GroundDraw::Pad | crate::partstool::GroundDraw::Pit
            );
            if closes && pts.len() >= 3 {
                painter.extend(egui::Shape::dashed_line(
                    &[pts[pts.len() - 1], pts[0]],
                    Stroke::new(1.0, ghost.gamma_multiply(0.55)),
                    5.0,
                    4.0,
                ));
            }
        }
    }
    // Enter finishes a site line or outline being drawn (a contour, a pad,
    // a pit, a trench, a road, a drain, a path), as it does a slab's
    // outline: the same as a click on its last point again
    if let (Tool::Terrain, PaneKind::Plan { level }) = (tool, kind) {
        let enter = response.hovered()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && !ui.ctx().egui_wants_keyboard_input();
        let lines = !matches!(
            state.parts.ground,
            crate::partstool::GroundDraw::Outline
                | crate::partstool::GroundDraw::Property
                | crate::partstool::GroundDraw::Spot
                | crate::partstool::GroundDraw::Tank
        );
        if let (true, true, Some(last)) = (enter, lines, state.parts.path.last().copied()) {
            if state.parts.path.len() < state.parts.ground.fewest() {
                state.status = Some(format!(
                    "{} needs {} points: click on, or Esc to cancel.",
                    state.parts.ground.label(),
                    state.parts.ground.fewest()
                ));
            } else if let Err(e) = crate::partstool::click(doc, level, tool, last, state, selection)
            {
                return Some(e.to_string());
            }
        }
    }
    if let (Tool::Sweep, PaneKind::Plan { .. }, Some(b)) = (tool, kind, hover_world) {
        let (prof, name) = state.parts.sweep_profile(doc.model(), 0.2);
        if !prof.is_empty() {
            let lo = prof.iter().map(|p| p.x).fold(f64::MAX, f64::min);
            let hi = prof.iter().map(|p| p.x).fold(f64::MIN, f64::max);
            let mut path = state.parts.path.clone();
            path.push(b);
            let ghost = crate::theme::interaction();
            for w in path.windows(2) {
                let d = (w[1] - w[0]).normalize_or_zero();
                if d == DVec2::ZERO {
                    continue;
                }
                let n = d.perp();
                let band = [w[0] + n * lo, w[1] + n * lo, w[1] + n * hi, w[0] + n * hi];
                let pts: Vec<Pos2> = band.iter().map(|p| xf.to_screen(*p)).collect();
                painter.add(egui::Shape::convex_polygon(
                    pts,
                    ghost.gamma_multiply(0.15),
                    Stroke::new(1.0, ghost.gamma_multiply(0.7)),
                ));
            }
            // the section itself, small, by the pointer
            let span = prof
                .iter()
                .fold((f64::MAX, f64::MIN, f64::MAX, f64::MIN), |a, p| {
                    (a.0.min(p.x), a.1.max(p.x), a.2.min(p.y), a.3.max(p.y))
                });
            let size = (span.1 - span.0).max(span.3 - span.2).max(1e-6);
            let k = 42.0 / size;
            let at = xf.to_screen(b) + Vec2::new(26.0, 26.0);
            let box_r = Rect::from_min_size(at, Vec2::splat(50.0));
            let pal = crate::theme::Palette::of(ui.ctx());
            painter.rect(
                box_r,
                6.0,
                pal.panel,
                Stroke::new(1.0, pal.border),
                egui::StrokeKind::Inside,
            );
            let mid = DVec2::new((span.0 + span.1) / 2.0, (span.2 + span.3) / 2.0);
            let sec: Vec<Pos2> = prof
                .iter()
                .map(|p| {
                    box_r.center()
                        + Vec2::new(((p.x - mid.x) * k) as f32, (-(p.y - mid.y) * k) as f32)
                })
                .collect();
            ring_edges(&painter, &sec, Stroke::new(1.2, pal.text));
            painter.text(
                box_r.center_bottom() + Vec2::new(0.0, 3.0),
                egui::Align2::CENTER_TOP,
                name,
                egui::FontId::proportional(10.5),
                pal.muted,
            );
        }
    }
    // an arc grid line, its ends clicked: the arc through the pointer
    if let (Tool::Grid, Some((a, b)), Some(p)) = (tool, state.grid_arc_ends, pointer_world) {
        let bulge = creabim_core::sketch::bulge_through(a, b, p);
        let pts: Vec<Pos2> = creabim_core::sketch::Curve::arc(a, b, bulge)
            .points()
            .into_iter()
            .map(|q| xf.to_screen(q))
            .collect();
        painter.extend(egui::Shape::dashed_line(
            &pts,
            Stroke::new(1.5, crate::theme::interaction()),
            12.0,
            4.0,
        ));
        ui.ctx().request_repaint();
    }
    // preview of the tool in progress
    if let (Some(a), Some(b)) = (state.pending, hover_world) {
        let accent = Stroke::new(1.5, crate::theme::interaction());
        match tool {
            Tool::Level => {
                let end = DVec2::new(b.x, a.y);
                painter.extend(egui::Shape::dashed_line(
                    &[xf.to_screen(a), xf.to_screen(end)],
                    accent,
                    10.0,
                    4.0,
                ));
                painter.text(
                    xf.to_screen(end) + Vec2::new(8.0, -6.0),
                    egui::Align2::LEFT_BOTTOM,
                    crate::units::elevation(a.y),
                    egui::FontId::proportional(13.0),
                    accent.color,
                );
            }
            Tool::Wall
            | Tool::Section
            | Tool::Elevation
            | Tool::Grid
            | Tool::Beam
            | Tool::Stair
            | Tool::Railing
            | Tool::Footing
            | Tool::RoomSeparator => {
                let level = match kind {
                    PaneKind::Plan { level } => Some(level),
                    _ => None,
                };
                crate::memberstool::preview(
                    &painter,
                    &|p| Some(xf.to_screen(p.truncate())),
                    doc.model(),
                    level,
                    tool,
                    state,
                    a.extend(0.0),
                    b.extend(0.0),
                );
                // an arc, circle or rectangle of walls being drawn: its
                // centrelines and both faces (MU40); no straight line to
                // the pointer, which read as a straight wall
                let shape = (tool == Tool::Wall)
                    .then(|| wall_shape_preview(state, a, b))
                    .flatten();
                if shape.is_none() {
                    painter.line_segment([xf.to_screen(a), xf.to_screen(b)], accent);
                } else if state.wall_shape == 2 {
                    // a circle's radius, faint
                    painter.extend(egui::Shape::dashed_line(
                        &[xf.to_screen(a), xf.to_screen(b)],
                        Stroke::new(1.0, accent.color.gamma_multiply(0.6)),
                        6.0,
                        4.0,
                    ));
                }
                if let Some(lines) = shape {
                    let t = state.wall_thickness / 2.0;
                    for pts in lines {
                        painter.add(egui::Shape::line(
                            pts.iter().map(|p| xf.to_screen(*p)).collect(),
                            Stroke::new(2.0, accent.color),
                        ));
                        for side in [-t, t] {
                            painter.add(egui::Shape::line(
                                offset_line(&pts, side)
                                    .iter()
                                    .map(|p| xf.to_screen(*p))
                                    .collect(),
                                Stroke::new(1.0, accent.color),
                            ));
                        }
                    }
                } else if tool == Tool::Wall && matches!(kind, PaneKind::Plan { .. }) {
                    // the wall's faces about the line it is drawn along
                    if let Some(faces) = wall_preview(doc.model(), state, a, b) {
                        let thin = Stroke::new(1.0, accent.color);
                        for (p, q) in faces {
                            painter.line_segment([xf.to_screen(p), xf.to_screen(q)], thin);
                        }
                    }
                }
                let len = a.distance(b);
                let text = match (tool, state.wall_shape) {
                    (Tool::Wall, 2) => format!("R {}", crate::units::len(len)),
                    (Tool::Wall, 3) => format!(
                        "{} × {}",
                        crate::units::len((b.x - a.x).abs()),
                        crate::units::len((b.y - a.y).abs())
                    ),
                    (Tool::Wall, 1) => match state.wall_arc_end.and_then(|e| {
                        bulge_through(a, b, e).map(|k| creabim_core::sketch::Curve::arc(a, e, k))
                    }) {
                        Some(c) => format!(
                            "Arc {}",
                            crate::units::len(
                                c.points()
                                    .windows(2)
                                    .map(|w| w[0].distance(w[1]))
                                    .sum::<f64>()
                            )
                        ),
                        None => crate::units::len(len),
                    },
                    _ => crate::units::len(len),
                };
                painter.text(
                    xf.to_screen((a + b) / 2.0) + Vec2::new(0.0, -12.0),
                    egui::Align2::CENTER_BOTTOM,
                    text,
                    egui::FontId::proportional(13.0),
                    accent.color,
                );
            }
            t if t.draws_outline() && !(t == Tool::Terrain && !state.parts.path.is_empty()) => {
                painter.rect_stroke(
                    Rect::from_two_pos(xf.to_screen(a), xf.to_screen(b)),
                    0.0,
                    accent,
                    egui::StrokeKind::Middle,
                );
            }
            Tool::Guide => {
                let (through, dir, text) = match state.guide_edge {
                    Some((e0, e1)) => {
                        let d = (e1 - e0).normalize_or_zero();
                        let off = (b - e0).perp_dot(d).abs();
                        (b, d, crate::units::len(off))
                    }
                    None => {
                        let d = (b - a).normalize_or_zero();
                        let deg = d.y.atan2(d.x).to_degrees().rem_euclid(180.0);
                        (a, d, format!("{deg:.1}°"))
                    }
                };
                let reach = rect.width().max(rect.height()) as f64 / xf.scale as f64;
                painter.extend(egui::Shape::dashed_line(
                    &[
                        xf.to_screen(through - dir * reach),
                        xf.to_screen(through + dir * reach),
                    ],
                    accent,
                    10.0,
                    5.0,
                ));
                // in a plan the tracker's pill says it, and takes it typed
                if !line_tool {
                    painter.text(
                        xf.to_screen(b) + Vec2::new(12.0, -12.0),
                        egui::Align2::LEFT_BOTTOM,
                        text,
                        egui::FontId::proportional(13.0),
                        accent.color,
                    );
                }
            }
            _ => {}
        }
    }
    if let (Some(s), PaneKind::Plan { .. } | PaneKind::Section { .. }, false) =
        (tool_snap, kind, shift)
    {
        if tool != Tool::Select && !tool.is_annotation() {
            let from = state
                .pending
                .or(sketch_last.map(from_pane))
                .map(|a| xf.to_screen(a));
            let at = xf.to_screen(s.p.truncate());
            match &tracked {
                Some(t) if !t.guides.is_empty() => {
                    snaps::draw_guides(&painter, &pane_project, &guides, &t.guides);
                    snaps::draw_guided(&painter, &pane_project, t, &guides, from);
                }
                _ => snaps::draw_tracked(&painter, &pane_project, s, tool_align, at, from),
            }
        }
    }
    if let Some(p) = hover_world {
        if tool != Tool::Select && !tool.is_annotation() {
            painter.circle_stroke(
                xf.to_screen(p),
                4.0,
                Stroke::new(1.0, crate::theme::interaction()),
            );
        }
    }

    // Shift+right-click places the pivot cursor
    if let (Some(space), true, true, Some(p)) = (
        space,
        response.secondary_clicked(),
        shift,
        response.interact_pointer_pos(),
    ) {
        let level_z = match kind {
            PaneKind::Plan { level } => doc.model().level(level).map_or(0.0, |l| l.elevation),
            _ => 0.0,
        };
        let w = hover_world.unwrap_or_else(|| xf.to_world(p));
        let c = pane_to_model(space, w, level_z);
        state.cursor = Some(c);
        state.pivot = modal::Pivot::Cursor;
        state.status = Some(format!(
            "Pivot cursor at {:.2}, {:.2}, {:.2}. Rotate and scale now turn about it.",
            c.x, c.y, c.z
        ));
        return None;
    }
    if let (Some(space), Some(c)) = (space, state.cursor) {
        draw_cursor(&painter, xf.to_screen(model_to_pane(space, c)));
    }
    let right = response.secondary_clicked() && !shift;
    let esc =
        ui.input(|i| i.key_pressed(egui::Key::Escape)) && !ui.ctx().egui_wants_keyboard_input();
    // a detail path that can be made is finished, never dropped
    if tool == Tool::Detail
        && (esc || right)
        && state.details.view == Some(view)
        && crate::detailtool::keep(doc, state, selection)
    {
        return None;
    }
    if esc {
        let stepped = state.step_back();
        // with nothing to step back from, Esc selects nothing, as in 3D
        // (the select bar says so)
        if !stepped
            && tool == Tool::Select
            && state.edit.is_none()
            && response.hovered()
            && selection.is_some()
        {
            selection.clear();
            state.status = Some("Nothing selected.".into());
        }
    }
    let menu = right && !state.step_back() && tool == Tool::Select;
    if menu {
        // right-click on something not yet selected selects it first
        if let Some(p) = response.interact_pointer_pos() {
            let raw = xf.to_world(p);
            let hit = state
                .hover
                .as_ref()
                .and_then(|h| h.at(pane, p))
                .or_else(|| hit_test(&plan.items, raw, 6.0 / scale as f64, mm))
                .or_else(|| ray_pick(doc, view, kind, raw));
            if let Some(id) = hit.filter(|id| !selection.contains(*id)) {
                selection.only(id);
            }
            let level_z = match kind {
                PaneKind::Plan { level } => doc.model().level(level).map_or(0.0, |l| l.elevation),
                _ => 0.0,
            };
            let w = hover_world.unwrap_or(raw);
            state.menu_at = space.map(|space| pane_to_model(space, w, level_z));
        }
    }
    if tool == Tool::Select && state.modal.is_none() && state.web_menus {
        if menu {
            state.menu_request = response.interact_pointer_pos();
        }
    } else if tool == Tool::Select && state.modal.is_none() {
        let n = selection.len();
        let at = state.menu_at;
        response.context_menu(|ui| {
            if let Some(c) = crate::commands::context_menu(ui, n) {
                match c.modal() {
                    Some(m) => state.armed = Some(m),
                    None => state.commands.push((c, at)),
                }
            }
        });
    }
    if let (PaneKind::Section { .. }, Some(a), Some(b)) = (kind, state.pending, hover_world) {
        if matches!(tool, Tool::Section | Tool::Elevation) {
            preview_cross_section(&painter, &xf, a, b, accent);
        }
    }

    // Fill room: what a click would fill, or where the walls are open
    if let (PaneKind::Plan { level }, Some(p), Some(at), true) = (
        kind,
        hover_world,
        response.hover_pos(),
        crate::fillpreview::on(state),
    ) {
        let look = crate::fillpreview::cached(&mut state.fill_look, doc, level, tool, p).clone();
        let units = doc.model().units();
        crate::fillpreview::draw(&painter, &xf, &look, at, &units, tool);
    }
    // the host a click would go on, and what it would make there
    if let (Some(h), Some(at)) = (host, response.hover_pos()) {
        if hovered != Some(h) {
            draw_hover(&painter, &xf, &plan.items, h, mm);
        }
        let mut text = crate::memberstool::host_preview(doc.model(), state, tool, h);
        if host_hits.len() > 1 {
            let i = state.host_cycle % host_hits.len();
            text += &format!("   {}/{} · Tab", i + 1, host_hits.len());
        }
        let look = crate::handles::PillLook::of(&crate::theme::Palette::of(ui.ctx()));
        crate::handles::plain_pill(&painter, at + Vec2::new(16.0, 18.0), &text, &look);
    }
    // the tracker's pill by the pointer
    if let (true, Some(a), Some(b), Some(at)) =
        (line_tool, state.pending, hover_world, response.hover_pos())
    {
        let look = crate::handles::PillLook::of(&crate::theme::Palette::of(ui.ctx()));
        // a guide's copy: the distance straight across from its original
        let len = match (tool, state.guide_edge) {
            (Tool::Guide, Some((e0, e1))) => (b - e0).perp_dot((e1 - e0).normalize_or_zero()).abs(),
            _ => a.distance(b),
        };
        crate::tracker::draw(
            &painter,
            at,
            &state.tracker,
            len,
            crate::tracker::angle_of(a, b),
            &snap_cfg.step_label(),
            state.tracker.gap.then_some(state.members.stair_gap),
            &look,
        );
    }

    let mut error = None;
    if let (true, Some(p), PaneKind::Plan { level }) = (tracker_enter, hover_world, kind) {
        state.tracker.clear();
        if let Err(e) = plan_tool(doc, level, tool, p, state, selection) {
            error = Some(e.to_string());
        }
    }
    // Calibrate: two clicks on the reference, then its distance (MU19);
    // right-click or Esc stops it (MU23)
    if state.calibrate.is_some()
        && (response.secondary_clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)))
    {
        state.calibrate = None;
        state.status = Some("Calibrate cancelled.".into());
        return None;
    }
    // files dropped on the view: pictures and DXF drawings become
    // references here, at the pointer (MU23)
    let drops: Vec<std::path::PathBuf> = ui.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .map(|f| f.path().to_path_buf())
            .filter(|p| !p.as_os_str().is_empty())
            .collect()
    });
    if !drops.is_empty() && response.contains_pointer() {
        let at = hover_world.unwrap_or(xf.center);
        for p in drops {
            state.dropped.push((view, at, p));
        }
    }
    // Align: two clicks on the reference, two on the model (MU23)
    if state.align_ref.is_some()
        && (response.secondary_clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)))
    {
        state.align_ref = None;
        state.status = Some("Align cancelled.".into());
        return None;
    }
    if let Some((rid, pts)) = state.align_ref.as_mut() {
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        let ink = |i: usize| {
            if i < 2 {
                accent_selected()
            } else {
                GUIDE_COLOR
            }
        };
        for (i, q) in pts.iter().enumerate() {
            painter.circle_stroke(xf.to_screen(*q), 5.0, Stroke::new(1.5, ink(i)));
        }
        if pts.len() == 2 {
            painter.line_segment(
                [xf.to_screen(pts[0]), xf.to_screen(pts[1])],
                Stroke::new(1.5, accent_selected()),
            );
        }
        if let Some(h) = hover_world {
            painter.circle_stroke(xf.to_screen(h), 4.0, Stroke::new(1.0, ink(pts.len())));
            if pts.len() % 2 == 1 {
                let a = pts[pts.len() - 1];
                painter.line_segment(
                    [xf.to_screen(a), xf.to_screen(h)],
                    Stroke::new(1.0, ink(pts.len())),
                );
            }
        }
        if response.clicked() {
            if let Some(q) = hover_world {
                pts.push(q);
            }
            state.status = Some(
                match pts.len() {
                    1 => "Align: a second point on the reference, along the line to lay down.",
                    2 => {
                        "Align: now the first point of the line it lies along (the model, a guide)."
                    }
                    3 => "Align: the second point of that line.",
                    _ => "",
                }
                .into(),
            );
            if pts.len() >= 4 {
                let (id, p) = (*rid, pts.clone());
                state.align_ref = None;
                state.aligned = Some((id, [p[0], p[1], p[2], p[3]]));
            }
        }
        return None;
    }
    if let Some((rid, pts)) = state.calibrate.as_mut() {
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        // where the next click goes, before the first too (MU23)
        if let (Some(h), true) = (hover_world, pts.is_empty()) {
            painter.circle_stroke(xf.to_screen(h), 4.0, Stroke::new(1.0, accent_selected()));
        }
        for q in pts.iter() {
            painter.circle_stroke(xf.to_screen(*q), 5.0, Stroke::new(1.5, accent_selected()));
        }
        if let (Some(a), Some(h)) = (pts.first(), hover_world) {
            painter.line_segment(
                [xf.to_screen(*a), xf.to_screen(h)],
                Stroke::new(1.0, accent_selected()),
            );
        }
        if response.clicked() {
            if let Some(q) = hover_world {
                pts.push(q);
            }
            if pts.len() >= 2 {
                state.calibrated = Some((*rid, pts[0], pts[1]));
                state.calibrate = None;
            } else {
                state.status =
                    Some("Calibrate: click a second point a known distance from the first.".into());
            }
        }
        return None;
    }
    if response.clicked() {
        state.tracker.clear();
        let p = hover_world?;
        let at = response.interact_pointer_pos().unwrap_or_default();
        let raw = xf.to_world(at);
        // what is pre-selected there (Tab may have moved past the first);
        // else linework first (edges, symbols), then what the view ray hits
        let hover_pick = state.hover.as_mut().and_then(|h| {
            h.list = false;
            h.at(pane, at)
        });
        let picked = || {
            hover_pick
                .or_else(|| hit_test(&plan.items, raw, 6.0 / scale as f64, mm))
                .or_else(|| ray_pick(doc, view, kind, raw))
        };
        let result = match (kind, tool) {
            (_, Tool::Select) => {
                let under = picked();
                // a double-click on the selected element opens it in Edit mode
                if response.double_clicked()
                    && !shift
                    && under.is_some()
                    && under == selection.active
                {
                    if let Some(id) = under {
                        // a callout, section or elevation mark opens its view
                        let is_view = matches!(
                            doc.model().get(id).map(|e| &e.kind),
                            Some(ElementKind::View(_))
                        );
                        if is_view && id != view {
                            state.want_view = Some(id);
                        } else {
                            state.status =
                                Some(crate::editmode::enter(doc.model(), id, &mut state.edit));
                        }
                    }
                    return None;
                }
                match (under, shift) {
                    (Some(id), true) => selection.toggle(id),
                    (None, true) => {}
                    (id, false) => selection.set(id),
                }
                Ok(())
            }
            (_, Tool::Dimension) => match (dim_hover, &view_def) {
                (Some(hp), Some(_)) => {
                    let lines: Vec<(DVec2, DVec2, f64)> = plan
                        .items
                        .iter()
                        .filter_map(|it| match it {
                            PlanItem::Dimension { a, b, offset, .. } => Some((*a, *b, *offset)),
                            _ => None,
                        })
                        .collect();
                    let tol = 8.0 / xf.scale as f64;
                    dimension_tool(doc, view, hp, raw, dim_axis, state, selection, &lines, tol)
                }
                _ => Ok(()),
            },
            (PaneKind::Axonometric, Tool::Align) => {
                let under = align_under.unwrap_or_default();
                state.status = Some(crate::align3d::click(
                    doc,
                    &mut state.align,
                    &under,
                    selection,
                ));
                Ok(())
            }
            (_, Tool::Align) => {
                state.align.shift = shift;
                crate::aligntool::click(
                    doc,
                    view,
                    kind,
                    &plan.items,
                    &centre_lines,
                    raw,
                    tool_snap.filter(|_| !shift),
                    6.0 / scale as f64,
                    mm,
                    state,
                    selection,
                )
            }
            (_, Tool::Tag) => tag_tool(doc, view, &view_def, picked(), raw, state, selection),
            (PaneKind::Plan { .. } | PaneKind::Section { .. }, Tool::Callout) => {
                // the new callout is selected where it was drawn; its view
                // opens from Properties or with a double-click
                crate::detailtool::callout_click(doc, view, p, state, selection).map(|_| ())
            }
            (PaneKind::Plan { .. } | PaneKind::Section { .. }, Tool::Detail) => {
                crate::detailtool::detail_click(doc, view, p, raw, state, selection)
            }
            (_, t) if crate::annotate::is_tool(t) => match &view_def {
                Some(v) => crate::annotate::click(
                    doc,
                    view,
                    v,
                    t,
                    dim_hit,
                    raw,
                    &plan.items,
                    6.0 / scale as f64,
                    &mut state.anno,
                    selection,
                )
                .map(|msg| {
                    if !msg.is_empty() {
                        state.status = Some(msg);
                    }
                }),
                None => Ok(()),
            },
            (PaneKind::Section { start, end }, _) => {
                // a guide or edge clicked first: the copy goes parallel to
                // it, as in plans (MU71: sections only drew through two
                // points), taken on its line where it was clicked
                let mut p = p;
                if tool == Tool::Guide && state.pending.is_none() {
                    state.guide_edge = tool_snap
                        .filter(|s| {
                            !shift
                                && matches!(
                                    s.kind,
                                    snaps::SnapKind::OnEdge
                                        | snaps::SnapKind::Guide
                                        | snaps::SnapKind::Midpoint
                                )
                        })
                        .and_then(|s| state.snaps.get(&view)?.2.edge_at(s.p))
                        .map(|(a, b)| (a.truncate(), b.truncate()));
                    if let Some((e0, e1)) = state.guide_edge {
                        let d = (e1 - e0).normalize_or_zero();
                        p = e0 + d * (raw - e0).dot(d);
                    }
                }
                section_tool(doc, start, end, tool, p, picked(), state, selection)
            }
            (PaneKind::Plan { level }, tool) => {
                if tool == Tool::Guide && state.pending.is_none() {
                    state.guide_edge = tool_snap
                        .filter(|s| {
                            !shift
                                && matches!(
                                    s.kind,
                                    snaps::SnapKind::OnEdge
                                        | snaps::SnapKind::Guide
                                        | snaps::SnapKind::Midpoint
                                )
                        })
                        .and_then(|s| state.snaps.get(&view)?.2.edge_at(s.p))
                        .map(|(a, b)| (a.truncate(), b.truncate()))
                        // a guide under the pointer, though the click
                        // snapped to a crossing on it (MU70)
                        .or_else(|| {
                            (!shift)
                                .then(|| guide_near(doc.model(), raw, 6.0 / scale as f64))
                                .flatten()
                        });
                }
                // a guide or edge picked to copy: held where it was clicked,
                // not at its end node, so the copy is measured from there
                // (MU68: it jumped to the node and was placed off it)
                let p = match (tool, state.pending, state.guide_edge) {
                    (Tool::Guide, None, Some((e0, e1))) => {
                        let d = (e1 - e0).normalize_or_zero();
                        e0 + d * (raw - e0).dot(d)
                    }
                    _ => p,
                };
                plan_tool(doc, level, tool, p, state, selection)
            }
            (PaneKind::Axonometric | PaneKind::Schedule, _) => Ok(()),
        };
        if let Err(e) = result {
            error = Some(e.to_string());
        }
    }

    // what the pane under the pointer does is said in the status bar,
    // never over the drawing
    if style.decorations && ui.rect_contains_pointer(rect) {
        state.hint = Some(
            match (kind, tool) {
                (PaneKind::Plan { .. }, Tool::Select) => {
                    "Click to select, Shift+click to add, drag on empty paper for a box. Drag grips to stretch; Enter or a double-click opens Edit mode to add and remove corners. M move, G grab, R rotate, S scale, Shift+D duplicate."
                }
                (PaneKind::Plan { .. }, t) => t.hint(),
                (PaneKind::Section { .. }, t) => t.section_hint(),
                (PaneKind::Axonometric, Tool::Tag) => Tool::Tag.hint(),
                (PaneKind::Axonometric, Tool::Note) => Tool::Note.hint(),
                (PaneKind::Axonometric, Tool::Align) => {
                    "Align in 3D: click a face, edge, corner, centre line or grid line for the reference (empty space takes the plane of the level nearest the ground), then a face to align, stretch or extend to it. Tab picks what is behind. Esc lets go."
                }
                (PaneKind::Axonometric, _) => {
                    "Drag to orbit, click to select. The Tag tool works here too. Right-drag pans, the wheel zooms."
                }
                (PaneKind::Schedule, _) => {
                    "Click a row to select its element."
                }
            },
        );
    }
    error
}

/// Underlays (another level traced under a plan) are drawn in this colour.
pub const UNDERLAY_COLOR: Color32 = Color32::from_rgb(178, 184, 196);

/// Drag a level's line or head in a section or elevation: the level (and
/// everything on it) follows the pointer up and down, onto model points it
/// snaps to, else to the nearest centimetre (half inch). Returns `None`
/// when the pointer isn't dragging a level.
#[allow(clippy::too_many_arguments)]
/// A level tag on a section's ground, as drawn: a tag's note whose leader
/// ends on the ground (MU142: level tags are tags).
#[derive(Clone, Copy, Debug)]
struct LevelTag {
    /// What its moves are kept by in its view.
    key: TagKey,
    /// Its words' middle and half size.
    centre: DVec2,
    half: DVec2,
    /// Where its leader ends, on the ground, and where it bends.
    foot: DVec2,
    elbow: Option<DVec2>,
    /// Its leader as drawn, from its words to the ground (`len` points).
    path: [DVec2; 4],
    len: usize,
}

/// What a level tag's moves are kept by in its view: in a section, its
/// terrain and where along the section it stands; in a plan, its site
/// element and the point it reads (MU142: site plan levels are tags too).
#[derive(Clone, Copy, Debug, PartialEq)]
struct TagKey {
    site: ElementId,
    x: f64,
    at: Option<DVec2>,
}

impl TagKey {
    /// How far it is moved in view `v`.
    fn by(&self, v: &creabim_core::View) -> DVec2 {
        match self.at {
            Some(a) => v.site.spot_tag(self.site, a).map(|m| m.by),
            None => v.site.tag_move(self.site, self.x),
        }
        .unwrap_or(DVec2::ZERO)
    }

    /// Is it moved or bent in view `v`?
    fn kept(&self, v: &creabim_core::View) -> bool {
        match self.at {
            Some(a) => v.site.spot_tag(self.site, a).is_some(),
            None => v.site.tag(self.site, self.x).is_some(),
        }
    }

    fn set_by(&self, v: &mut creabim_core::View, by: Option<DVec2>) {
        match self.at {
            Some(a) => v.site.set_spot_move(self.site, a, by),
            None => v.site.set_tag_move(self.site, self.x, by),
        }
    }

    fn set_bend(&self, v: &mut creabim_core::View, elbow: Option<DVec2>) {
        match self.at {
            Some(a) => v.site.set_spot_bend(self.site, a, elbow),
            None => v.site.set_tag_bend(self.site, self.x, elbow),
        }
    }
}

impl LevelTag {
    /// Its leader's grip: on its bend, else half way along its last run,
    /// on the line as drawn.
    fn bend_grip(&self) -> DVec2 {
        let before = self.path[self.len.saturating_sub(2)];
        self.elbow.unwrap_or_else(|| (before + self.foot) / 2.0)
    }

    fn on_words(&self, w: DVec2) -> bool {
        (w.x - self.centre.x).abs() <= self.half.x && (w.y - self.centre.y).abs() <= self.half.y
    }

    /// Is `w` on its leader (within `reach`), off its words?
    fn on_leader(&self, w: DVec2, reach: f64) -> bool {
        if self.on_words(w) {
            return false;
        }
        let pts = &self.path[..self.len];
        pts.windows(2).any(|s| {
            let (a, b) = (s[0], s[1]);
            let d = b - a;
            let t = ((w - a).dot(d) / d.length_squared().max(1e-12)).clamp(0.0, 1.0);
            (a + d * t).distance(w) <= reach
        })
    }
}

/// The level tags of a section's ground, or of a plan's site.
fn level_tags(model: &creabim_core::Model, plan: &Drawing, mm: f64) -> Vec<LevelTag> {
    let mut out = vec![];
    for it in &plan.items {
        // a plan's: a tag over the point it reads (spot levels, a survey's
        // heights, a road's levels)
        if let PlanItem::Tag {
            id,
            anchor,
            at,
            elbow,
            text,
            round,
            look,
        } = it
        {
            if !matches!(model.get(*id).map(|e| &e.kind), Some(ElementKind::Site(_))) {
                continue;
            }
            let Some((b, _)) = creabim_geom::declutter::text_box(it, mm) else {
                continue;
            };
            let g =
                creabim_geom::annotate::tag_geometry(*anchor, *at, *elbow, text, *round, look, mm);
            // its leader as drawn; at home it has none, and the short run
            // from under its words to its point stands for it
            let drawn = if g.leader.len() >= 2 {
                g.leader.clone()
            } else {
                vec![DVec2::new((b.lo.x + b.hi.x) / 2.0, b.lo.y), *anchor]
            };
            let mut path = [*anchor; 4];
            let len = drawn.len().min(4);
            path[..len].copy_from_slice(&drawn[..len]);
            out.push(LevelTag {
                key: TagKey {
                    site: *id,
                    x: anchor.x,
                    at: Some(*anchor),
                },
                centre: (b.lo + b.hi) / 2.0,
                half: (b.hi - b.lo) / 2.0 + DVec2::splat(0.5 * mm),
                foot: *anchor,
                elbow: *elbow,
                path,
                len,
            });
            continue;
        }
        let PlanItem::Annotation { id, item } = it else {
            continue;
        };
        let creabim_geom::annotation::Shape::Note {
            leader: Some(foot),
            elbow,
            ..
        } = item.shape
        else {
            continue;
        };
        let terrain = matches!(
            model.get(*id).map(|e| &e.kind),
            Some(ElementKind::Site(s)) if matches!(s.form, creabim_core::SiteForm::Terrain { .. })
        );
        let Some((b, _)) = creabim_geom::declutter::text_box(it, mm).filter(|_| terrain) else {
            continue;
        };
        // its leader as it is drawn: the last of its lines
        let marks = item.marks(item.mm);
        let drawn = marks.lines.last().cloned().unwrap_or_else(|| vec![foot]);
        let mut path = [foot; 4];
        let len = drawn.len().min(4);
        path[..len].copy_from_slice(&drawn[..len]);
        // its was-line, a note of its own just over it, is part of it
        let (mut lo, mut hi) = (b.lo, b.hi);
        for other in &plan.items {
            let PlanItem::Annotation { id: oid, item: o } = other else {
                continue;
            };
            if oid != id
                || !matches!(
                    o.shape,
                    creabim_geom::annotation::Shape::Note { leader: None, .. }
                )
            {
                continue;
            }
            let Some((w, _)) = creabim_geom::declutter::text_box(other, mm) else {
                continue;
            };
            let over = w.lo.y >= b.hi.y - (b.hi.y - b.lo.y) && w.lo.y <= b.hi.y + (b.hi.y - b.lo.y);
            let across = w.lo.x < b.hi.x && w.hi.x > b.lo.x;
            if over && across {
                lo = lo.min(w.lo);
                hi = hi.max(w.hi);
            }
        }
        out.push(LevelTag {
            key: TagKey {
                site: *id,
                x: foot.x,
                at: None,
            },
            centre: (lo + hi) / 2.0,
            half: (hi - lo) / 2.0 + DVec2::splat(0.5 * mm),
            foot,
            elbow,
            path,
            len,
        });
    }
    out
}

/// A level tag in a section or a plan dragged aside by its words, the
/// point it measures kept, or its leader bent by its grip; a double-click
/// puts a moved one back, or straightens a bent leader (owner, cut test;
/// MU142: level tags are tags, with their grips). Over a tag, its grips
/// show.
#[allow(clippy::too_many_arguments)]
fn drag_tag(
    doc: &mut Document,
    painter: &egui::Painter,
    plan: &Drawing,
    xf: &Xf,
    response: &egui::Response,
    state: &mut PlanState,
    view: ElementId,
    mm: f64,
) -> Option<Option<String>> {
    let reach = 6.0 / xf.scale as f64;
    let tags = level_tags(doc.model(), plan, mm);
    let on_tag = |p: Pos2| {
        let w = xf.to_world(p);
        tags.iter().copied().find(|t| t.on_words(w))
    };
    let on_leader = |p: Pos2| {
        let w = xf.to_world(p);
        tags.iter().copied().find(|t| t.on_leader(w, reach))
    };
    // the grips of the tag under the pointer, as a selected tag's
    let held = state.tag_drag.map(|d| d.0).or(state.tag_bend.map(|b| b.0));
    let shown = response
        .hover_pos()
        .and_then(|p| on_tag(p).or_else(|| on_leader(p)))
        .or_else(|| held.and_then(|k| tags.iter().copied().find(|g| g.key == k)));
    if let Some(t) = shown {
        let ink = Stroke::new(1.5, crate::theme::interaction());
        let top = xf.to_screen(t.centre + DVec2::new(0.0, t.half.y)) - Vec2::new(0.0, 5.0);
        painter.rect(
            egui::Rect::from_center_size(top, Vec2::splat(8.0)),
            1.0,
            Color32::WHITE,
            ink,
            egui::StrokeKind::Middle,
        );
        painter.circle(xf.to_screen(t.bend_grip()), 3.5, Color32::WHITE, ink);
    }
    let this_view = |doc: &Document| match doc.model().get(view).map(|e| &e.kind) {
        Some(ElementKind::View(v)) => Some(v.clone()),
        _ => None,
    };
    if response.double_clicked() {
        let p = response.interact_pointer_pos()?;
        // only one moved or bent: on the others a double-click is the
        // element's own (its Edit mode)
        let v = this_view(doc)?;
        if let Some(t) = on_tag(p).filter(|t| t.key.kept(&v)) {
            let r = doc.edit(view, "Put back level tag", |e| {
                if let ElementKind::View(v) = &mut e.kind {
                    t.key.set_by(v, None);
                }
            });
            state.status = Some("Level tag put back.".into());
            return Some(r.err().map(|e| e.to_string()));
        }
        let t = on_leader(p).filter(|t| t.elbow.is_some())?;
        let r = doc.edit(view, "Straighten level tag leader", |e| {
            if let ElementKind::View(v) = &mut e.kind {
                t.key.set_bend(v, None);
            }
        });
        state.status = Some("Leader straightened.".into());
        return Some(r.err().map(|e| e.to_string()));
    }
    if response.drag_started_by(egui::PointerButton::Primary) {
        let press = response.ctx.input(|i| i.pointer.press_origin())?;
        if let Some(t) = on_tag(press) {
            let was = this_view(doc).map_or(DVec2::ZERO, |v| t.key.by(&v));
            state.tag_drag = Some((t.key, was, xf.to_world(press)));
            state.drag_owner = Some(response.id);
        } else if let Some(t) = on_leader(press) {
            state.tag_bend = Some((t.key, t.foot));
            state.drag_owner = Some(response.id);
        }
    }
    if let Some((key, foot)) = state.tag_bend {
        if state.drag_owner != Some(response.id) {
            return None;
        }
        if response.drag_stopped() || !response.dragged() {
            state.tag_bend = None;
            state.status = Some("Leader bent. Double-click it to straighten it.".into());
            return Some(None);
        }
        let w = xf.to_world(response.interact_pointer_pos()?);
        let r = doc.edit_merged(view, "Bend level tag leader", |e| {
            if let ElementKind::View(v) = &mut e.kind {
                key.set_bend(v, Some(w - foot));
            }
        });
        return Some(r.err().map(|e| e.to_string()));
    }
    let (key, was, from) = state.tag_drag?;
    if state.drag_owner != Some(response.id) {
        return None;
    }
    if response.drag_stopped() || !response.dragged() {
        state.tag_drag = None;
        state.status = Some("Level tag moved. Double-click it to put it back.".into());
        return Some(None);
    }
    let w = xf.to_world(response.interact_pointer_pos()?);
    let by = was + (w - from);
    let r = doc.edit_merged(view, "Move level tag", |e| {
        if let ElementKind::View(v) = &mut e.kind {
            key.set_by(v, Some(by));
        }
    });
    Some(r.err().map(|e| e.to_string()))
}

#[allow(clippy::too_many_arguments)]
fn drag_level(
    doc: &mut Document,
    plan: &Drawing,
    xf: &Xf,
    response: &egui::Response,
    state: &mut PlanState,
    selection: &mut Selection,
    mm: f64,
    snapped: Option<snaps::Snap>,
) -> Option<Option<String>> {
    if response.drag_started_by(egui::PointerButton::Primary) {
        let press = response.ctx.input(|i| i.pointer.press_origin())?;
        // the grip on a selected level's head moves the head instead
        let on_head = plan.items.iter().any(|it| match it {
            PlanItem::Annotation { id, item } => {
                selection.contains(*id)
                    && matches!(item.shape, creabim_geom::annotation::Shape::Level { at, .. } if xf.to_screen(at).distance(press) < 9.0)
            }
            _ => false,
        });
        if on_head {
            return None;
        }
        let w = xf.to_world(press);
        let id = hit_test(&plan.items, w, 6.0 / xf.scale as f64, mm)?;
        let l = doc.model().level(id)?;
        state.level_drag = Some((id, l.elevation - w.y));
        state.drag_owner = Some(response.id);
        selection.only(id);
    }
    if state.level_drag.is_some() && state.drag_owner != Some(response.id) {
        return None;
    }
    let (id, off) = state.level_drag?;
    if response.drag_stopped() || !response.dragged() {
        state.level_drag = None;
        return Some(None);
    }
    let p = response.interact_pointer_pos()?;
    let step = if crate::units::get().is_imperial() {
        0.0127
    } else {
        0.01
    };
    let z = match snapped {
        Some(s) => s.p.y,
        None => ((xf.to_world(p).y + off) / step).round() * step,
    };
    let name = doc
        .model()
        .get(id)
        .map(|e| e.name.clone())
        .unwrap_or_default();
    let r = doc.edit_merged(id, "Move level", |e| {
        if let ElementKind::Level(l) = &mut e.kind {
            l.elevation = z;
        }
    });
    state.status = Some(format!(
        "{name} at {}. Everything on it moves with it.",
        crate::units::elevation(z)
    ));
    Some(r.err().map(|e| e.to_string()))
}

/// Guides are drawn in this colour, in every pane.
/// Split's cutting line and plane (MU39).
pub const SPLIT_RED: Color32 = Color32::from_rgb(222, 60, 60);

pub const GUIDE_COLOR: Color32 = Color32::from_rgb(190, 60, 170);

/// The part of the screen segment ab inside `r`.
pub fn clip_to_rect(a: Pos2, b: Pos2, r: Rect) -> Option<(Pos2, Pos2)> {
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    let d = b - a;
    for (p, q) in [
        (-d.x, a.x - r.min.x),
        (d.x, r.max.x - a.x),
        (-d.y, a.y - r.min.y),
        (d.y, r.max.y - a.y),
    ] {
        if p.abs() < 1e-9 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    (t0 < t1).then(|| (a + d * t0, a + d * t1))
}

/// A click with a drawing tool in a plan (or on a level in the 3D view).
/// The points of a chain of clicks (walls, beams) are kept for alignment
/// tracking while it goes on.
pub fn plan_tool(
    doc: &mut Document,
    level: ElementId,
    tool: Tool,
    p: DVec2,
    state: &mut PlanState,
    selection: &mut Selection,
) -> Result<(), creabim_core::ModelError> {
    let started = state.pending.is_none();
    let r = plan_click(doc, level, tool, p, state, selection);
    match state.pending {
        Some(q) => {
            if started || state.chain.0 != Some(level) {
                state.chain = (Some(level), vec![]);
            }
            if state.chain.1.last() != Some(&q) {
                state.chain.1.push(q);
            }
        }
        None => state.chain.1.clear(),
    }
    r
}

/// A wall as the Wall tool draws it between two clicks: along its location
/// line, the exterior to the right of the direction drawn unless flipped.
pub fn new_wall(
    model: &creabim_core::Model,
    level: ElementId,
    state: &PlanState,
    a: DVec2,
    b: DVec2,
) -> Wall {
    let height = model.level(level).map_or(3.0, |l| l.height);
    let mut w = Wall::new(level, a, b, state.wall_thickness, height);
    w.location = state.wall_location;
    // the wall type in use sets its thickness, location line and grid
    let typed = state.typed(model, Element::new("Wall", ElementKind::Wall(w.clone())));
    if let ElementKind::Wall(tw) = typed.kind {
        w = tw;
    }
    w.flip = state.wall_flip;
    creabim_core::walls::set_location_line(model, &mut w, a, b);
    w
}

/// Place a straight wall after the caller resolves its two location-line points.
pub(crate) fn create_straight_wall(
    doc: &mut Document,
    level: ElementId,
    state: &PlanState,
    start: DVec2,
    end: DVec2,
) -> Result<ElementId, creabim_core::ModelError> {
    let element = straight_wall_element(doc.model(), level, state, start, end);
    let id = element.id;
    doc.commit(Transaction::new("Draw wall").insert(element))?;
    Ok(id)
}

pub(crate) fn straight_wall_element(
    model: &creabim_core::Model,
    level: ElementId,
    state: &PlanState,
    start: DVec2,
    end: DVec2,
) -> Element {
    let wall = new_wall(model, level, state, start, end);
    state.typed(model, Element::new("Wall", ElementKind::Wall(wall)))
}

/// The centrelines the Wall tool's shape would make from its clicks so
/// far (`a`, the arc's end if clicked) to the pointer `b`, for its ghost in
/// plans and in 3D (MU40): an arc, a circle, a rectangle's four sides.
/// None for a straight wall, which draws its own.
pub fn wall_shape_preview(state: &PlanState, a: DVec2, b: DVec2) -> Option<Vec<Vec<DVec2>>> {
    match (state.wall_shape, state.wall_arc_end) {
        // the arc's end not yet clicked: already an arc (a quarter turn),
        // so it never reads as a straight wall; the next click bends it
        (1, None) if a.distance(b) > 1e-6 => Some(vec![creabim_core::sketch::Curve::arc(
            a,
            b,
            -(std::f64::consts::PI / 8.0).tan(),
        )
        .points()]),
        (1, Some(e)) => Some(vec![match bulge_through(a, b, e) {
            Some(k) => creabim_core::sketch::Curve::arc(a, e, k).points(),
            None => vec![a, e],
        }]),
        (2, _) if a.distance(b) > 1e-6 => Some(vec![(0..=72)
            .map(|i| a + DVec2::from_angle(i as f64 / 72.0 * std::f64::consts::TAU) * a.distance(b))
            .collect()]),
        (3, _) => Some(vec![vec![
            a,
            DVec2::new(b.x, a.y),
            b,
            DVec2::new(a.x, b.y),
            a,
        ]]),
        _ => None,
    }
}

/// The bulge of the arc from `a` through `t` to `b` (as [`Wall::bulge`]);
/// none when the three are in a line.
/// `pts` moved `d` to its left, corner by corner (a ghost's faces).
pub fn offset_line(pts: &[DVec2], d: f64) -> Vec<DVec2> {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let before = (i > 0).then(|| (pts[i] - pts[i - 1]).normalize_or_zero());
            let after = (i + 1 < n).then(|| (pts[i + 1] - pts[i]).normalize_or_zero());
            let dir = match (before, after) {
                (Some(a), Some(b)) => (a + b).normalize_or(b),
                (Some(a), None) => a,
                (None, Some(b)) => b,
                _ => DVec2::X,
            };
            pts[i] + dir.perp() * d
        })
        .collect()
}

pub fn bulge_through(a: DVec2, t: DVec2, b: DVec2) -> Option<f64> {
    let d = 2.0 * (a.x * (t.y - b.y) + t.x * (b.y - a.y) + b.x * (a.y - t.y));
    if d.abs() < 1e-9 {
        return None;
    }
    let sq = |p: DVec2| p.length_squared();
    let c = DVec2::new(
        (sq(a) * (t.y - b.y) + sq(t) * (b.y - a.y) + sq(b) * (a.y - t.y)) / d,
        (sq(a) * (b.x - t.x) + sq(t) * (a.x - b.x) + sq(b) * (t.x - a.x)) / d,
    );
    let tau = std::f64::consts::TAU;
    let ang = |p: DVec2| (p - c).y.atan2((p - c).x);
    let norm = |x: f64| x.rem_euclid(tau);
    let (a0, at, ae) = (ang(a), ang(t), ang(b));
    let sweep = if norm(at - a0) < norm(ae - a0) {
        norm(ae - a0)
    } else {
        -norm(a0 - ae)
    };
    // Curve::arc's bulge: tan(sweep / 4), anticlockwise positive
    Some((sweep / 4.0).tan())
}

/// The faces of the wall being drawn from `a` to `b`, and where its
/// exterior is, for the preview.
pub fn wall_preview(
    model: &creabim_core::Model,
    state: &PlanState,
    a: DVec2,
    b: DVec2,
) -> Option<[(DVec2, DVec2); 3]> {
    if a.distance(b) < 1e-6 {
        return None;
    }
    let w = new_wall(model, ElementId::new(), state, a, b);
    let n = w.direction().perp() * (w.thickness / 2.0);
    let out = if w.flip { n } else { -n };
    let mid = (w.start + w.end) / 2.0;
    Some([
        (w.start + n, w.end + n),
        (w.start - n, w.end - n),
        // a tick from the middle out of the exterior face
        (mid + out, mid + out * 2.5),
    ])
}

fn plan_click(
    doc: &mut Document,
    level: ElementId,
    tool: Tool,
    p: DVec2,
    state: &mut PlanState,
    selection: &mut Selection,
) -> Result<(), creabim_core::ModelError> {
    match tool {
        Tool::Select | Tool::Align => Ok(()),
        Tool::Level => {
            state.status = Some(
                "Level lines are drawn in a section or elevation. Here, add, rename and move levels in the Levels window.".into(),
            );
            state.want_levels = true;
            Ok(())
        }
        Tool::Section | Tool::Elevation => match state.pending.take() {
            None => {
                state.pending = Some(p);
                Ok(())
            }
            Some(a) if a.distance(p) > 0.2 => {
                add_section_view(doc, a, p, tool == Tool::Section, selection)
            }
            Some(_) => Ok(()),
        },
        // a circle: its centre, then a point on it; two half walls (MU40)
        Tool::Wall if state.wall_shape == 2 => match state.pending.take() {
            None => {
                state.pending = Some(p);
                state.status = Some("Circle wall: click a point on it (its radius).".into());
                Ok(())
            }
            Some(c) if c.distance(p) > 0.1 => {
                let a = p;
                let b = c - (p - c);
                let mut w1 = new_wall(doc.model(), level, state, a, b);
                let mut w2 = new_wall(doc.model(), level, state, b, a);
                for (w, s, e) in [(&mut w1, a, b), (&mut w2, b, a)] {
                    // on its centreline: a curve's location line is its middle
                    w.start = s;
                    w.end = e;
                    w.bulge = 1.0;
                }
                let e1 = state.typed(doc.model(), Element::new("Wall", ElementKind::Wall(w1)));
                let e2 = state.typed(doc.model(), Element::new("Wall", ElementKind::Wall(w2)));
                selection.only(e1.id);
                state.status = Some(format!(
                    "Circle wall, radius {}: two half walls. Click a centre for another.",
                    crate::units::len(c.distance(p))
                ));
                doc.commit(Transaction::new("Draw circle wall").insert(e1).insert(e2))
            }
            Some(_) => Ok(()),
        },
        // a rectangle of walls: two opposite corners (MU40)
        Tool::Wall if state.wall_shape == 3 => match state.pending.take() {
            None => {
                state.pending = Some(p);
                state.status = Some("Rectangle of walls: click the opposite corner.".into());
                Ok(())
            }
            Some(a) if (p - a).abs().min_element() > 0.1 => {
                let c = [a, DVec2::new(p.x, a.y), p, DVec2::new(a.x, p.y)];
                let mut tx = Transaction::new("Draw rectangle of walls");
                let mut first = None;
                for i in 0..4 {
                    let w = new_wall(doc.model(), level, state, c[i], c[(i + 1) % 4]);
                    let e = state.typed(doc.model(), Element::new("Wall", ElementKind::Wall(w)));
                    first.get_or_insert(e.id);
                    tx = tx.insert(e);
                }
                if let Some(id) = first {
                    selection.only(id);
                }
                state.status =
                    Some("Four walls, joined at the corners. Click a corner for another.".into());
                doc.commit(tx)
            }
            Some(_) => Ok(()),
        },
        // an arc: its start, its end, then a point it passes through
        Tool::Wall if state.wall_shape == 1 => match (state.pending, state.wall_arc_end) {
            (None, _) => {
                state.pending = Some(p);
                state.wall_arc_end = None;
                state.status = Some("Arc wall: click its end.".into());
                Ok(())
            }
            (Some(a), None) if a.distance(p) > 0.05 => {
                state.wall_arc_end = Some(p);
                state.status = Some("Arc wall: click a point it passes through.".into());
                Ok(())
            }
            (Some(a), Some(b)) => {
                let Some(bulge) = bulge_through(a, p, b) else {
                    state.status =
                        Some("That point is on the straight line: pick one off it.".into());
                    return Ok(());
                };
                let mut w = new_wall(doc.model(), level, state, a, b);
                w.start = a;
                w.end = b;
                w.bulge = bulge;
                let e = state.typed(doc.model(), Element::new("Wall", ElementKind::Wall(w)));
                selection.only(e.id);
                // the next arc starts where this one ends
                state.pending = Some(b);
                state.wall_arc_end = None;
                state.status = Some("Arc wall placed. Click the next one's end, or Esc.".into());
                doc.commit(Transaction::new("Draw arc wall").insert(e))
            }
            _ => Ok(()),
        },
        Tool::Wall => match state.pending {
            None => {
                state.pending = Some(p);
                Ok(())
            }
            Some(a) if a.distance(p) > 0.05 => {
                selection.only(create_straight_wall(doc, level, state, a, p)?);
                state.pending = Some(p);
                Ok(())
            }
            Some(_) => Ok(()),
        },
        Tool::Door | Tool::Window | Tool::Opening => {
            place_opening(doc, level, p, tool, state, selection)
        }
        Tool::Dimension
        | Tool::Tag
        | Tool::Angle
        | Tool::Radius
        | Tool::Diameter
        | Tool::ArcLength
        | Tool::Note
        | Tool::Symbol
        | Tool::SpotLevel
        | Tool::Sketch
        | Tool::Callout
        | Tool::Detail => Ok(()),
        // a library profile placed at the click, pulled up (MU36)
        Tool::Extrusion if state.parts.extrusion_library.is_some() => {
            let name = state.parts.extrusion_library.clone().unwrap_or_default();
            let Some(outline) = creabim_core::profiles::find(doc.model(), &name) else {
                state.status = Some(format!("There is no profile {name} any more."));
                return Ok(());
            };
            let s = &state.parts;
            let mut part = creabim_core::parts::Part::extrusion(
                level,
                outline.iter().map(|q| *q + p).collect(),
                s.extrusion_base,
                s.extrusion_height,
            );
            part.profile = Some(name.clone());
            let e = Element::new(name.clone(), ElementKind::Part(part));
            let id = e.id;
            doc.commit(creabim_core::Transaction::new("Place profile").insert(e))?;
            selection.only(id);
            state.status = Some(format!(
                "{name} placed, pulled up {}. Click again for another.",
                crate::units::len(s.extrusion_height)
            ));
            Ok(())
        }
        // the Terrain tool also draws contour lines and spot heights
        Tool::Terrain
            if !matches!(
                state.parts.ground,
                crate::partstool::GroundDraw::Outline | crate::partstool::GroundDraw::Property
            ) || state.parts.pick_parcel =>
        {
            crate::partstool::click(doc, level, tool, p, state, selection)
        }
        Tool::Slab
        | Tool::Roof
        | Tool::Shaft
        | Tool::Extrusion
        | Tool::Void
        | Tool::Terrain
        | Tool::Pad
        | Tool::Paving
        | Tool::Room
        | Tool::Ceiling => crate::slabtool::click(doc, level, p, state, selection),
        Tool::Sweep | Tool::Planting => {
            crate::partstool::click(doc, level, tool, p, state, selection)
        }
        Tool::Column | Tool::Beam | Tool::Stair | Tool::Railing | Tool::Footing => {
            crate::memberstool::click(doc, level, tool, p, state, selection)
        }
        Tool::Object => crate::objecttool::click(doc, level, p, state, selection),
        Tool::RoomSeparator => crate::roomtool::separator_click(doc, level, p, state, selection),
        Tool::Guide => match state.pending.take() {
            None => {
                state.pending = Some(p);
                Ok(())
            }
            Some(a) => {
                let (ga, gb) = match state.guide_edge.take() {
                    // the copy is the original moved from where it was
                    // grabbed to where it is dropped (MU69)
                    Some((e0, e1)) => (e0 + (p - a), e1 + (p - a)),
                    None if a.distance(p) > 0.01 => (a, p),
                    None => return Ok(()),
                };
                let z = doc.model().level(level).map_or(0.0, |l| l.elevation);
                let e = Element::new(
                    "Guide",
                    ElementKind::Guide(creabim_core::Guide {
                        level: Some(level),
                        a: ga.extend(z),
                        b: gb.extend(z),
                    }),
                );
                doc.commit(Transaction::new("Add guide").insert(e))
            }
        },
        // an arc: its two ends, then a point it passes through (MU66)
        Tool::Grid if state.grid_arc => {
            if let Some((a, b)) = state.grid_arc_ends.take() {
                let bulge = creabim_core::sketch::bulge_through(a, b, p);
                if bulge.abs() < 1e-4 || !bulge.is_finite() {
                    state.grid_arc_ends = Some((a, b));
                    state.status = Some(
                        "Click off the line between its ends: the arc passes through there.".into(),
                    );
                    return Ok(());
                }
                let e = creabim_core::grids::new_line(
                    doc.model(),
                    creabim_core::Grid {
                        start: a,
                        end: b,
                        bulge,
                        group: state.grid_group,
                        ..Default::default()
                    },
                );
                state.status = Some(format!(
                    "Added {}, an arc. Click again for the next line.",
                    e.name
                ));
                selection.only(e.id);
                return doc.commit(Transaction::new("Add grid line").insert(e));
            }
            match state.pending.take() {
                None => {
                    state.pending = Some(p);
                }
                Some(a) if a.distance(p) > 0.1 => {
                    state.grid_arc_ends = Some((a, p));
                    state.status = Some("Now click a point the arc passes through.".into());
                }
                Some(a) => state.pending = Some(a),
            }
            Ok(())
        }
        Tool::Grid => match state.pending.take() {
            None => {
                state.pending = Some(p);
                Ok(())
            }
            Some(a) if a.distance(p) > 0.1 => {
                let e = creabim_core::grids::new_grid(doc.model(), state.grid_group, a, p);
                state.status = Some(format!("Added {}. Click again for the next line.", e.name));
                selection.only(e.id);
                doc.commit(Transaction::new("Add grid line").insert(e))
            }
            Some(a) => {
                state.pending = Some(a);
                Ok(())
            }
        },
        Tool::Mass => {
            let e = Element::new(
                "Mass",
                ElementKind::FreeMesh(FreeMesh::new(
                    Some(level),
                    DVec3::new(p.x, p.y, 0.0),
                    EditMesh::cuboid(DVec3::new(2.0, 2.0, 3.0)),
                )),
            );
            selection.only(e.id);
            doc.commit(Transaction::new("Add mass").insert(e))
        }
    }
}

/// The centre lines of what plan or section `view` draws, seen edge on
/// (see `crate::centres`), kept until the model changes.
fn pane_centre_lines(
    state: &mut PlanState,
    doc: &Document,
    view: ElementId,
    kind: PaneKind,
    plan: &Drawing,
) -> Arc<Vec<crate::aligntool::CentreLine>> {
    let rev = doc.model().revision();
    if let Some((r, lines)) = state.centre_lines.get(&view) {
        if *r == rev {
            return lines.clone();
        }
    }
    let (space, look) = match kind {
        PaneKind::Plan { .. } => (Space::Plan, -DVec3::Z),
        PaneKind::Section { start, end } => (
            Space::Section { start, end },
            (end - start).normalize_or_zero().perp().extend(0.0),
        ),
        _ => return Arc::new(vec![]),
    };
    let drawn: HashSet<ElementId> = plan.items.iter().map(|i| i.id()).collect();
    let scene = cached_pick_scene(state, doc, view);
    let to_pane = |p: DVec3| model_to_pane(space, p);
    let lines: Vec<crate::aligntool::CentreLine> =
        crate::centres::all(doc.model(), &scene, &|id| drawn.contains(&id))
            .iter()
            .filter_map(|c| {
                let (a, b) = crate::centres::edge_on(c, look, &to_pane)?;
                Some((c.id, a, b))
            })
            .collect();
    let lines = Arc::new(lines);
    state.centre_lines.insert(view, (rev, lines.clone()));
    lines
}

/// A 3D view's frame and the snaps of what it shows, kept until the model
/// changes.
fn axo_frame_snaps(
    state: &mut PlanState,
    doc: &Document,
    view: ElementId,
) -> Option<Arc<(creabim_geom::section::AxoFrame, Snapper)>> {
    let rev = doc.model().revision();
    if let Some((r, fs)) = state.axo_snaps.get(&view) {
        if *r == rev {
            return Some(fs.clone());
        }
    }
    let Some(ElementKind::View(View {
        kind: ViewKind::Axonometric { azimuth, altitude },
        hidden,
        ..
    })) = doc.model().get(view).map(|e| &e.kind)
    else {
        return None;
    };
    let frame = creabim_geom::section::AxoFrame::new(doc.model(), *azimuth, *altitude);
    let hidden = hidden.hidden_set(doc.model());
    let scene = cached_pick_scene(state, doc, view);
    let mut snapper = Snapper::from_scene(&scene, &[]);
    snapper.add_grids(doc.model(), &hidden);
    let fs = Arc::new((frame, snapper));
    state.axo_snaps.insert(view, (rev, fs.clone()));
    Some(fs)
}

/// What the Align tool takes under pointer `p` of 3D view `view`: the
/// face of what the pre-selection reads there (Tab may have moved it), an
/// object snap, and the lowest level for empty space.
fn axo_align_under(
    state: &mut PlanState,
    doc: &Document,
    view: ElementId,
    fs: &(creabim_geom::section::AxoFrame, Snapper),
    xf: &Xf,
    p: Pos2,
    cfg: &snaps::SnapSettings,
) -> crate::align3d::Under {
    let model = doc.model();
    // the level nearest the ground
    let level = model
        .levels()
        .map(|(e, l)| (e.id, l.elevation))
        .min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()));
    let scene = cached_pick_scene(state, doc, view);
    let (frame, snapper) = (&fs.0, &fs.1);
    let (o, d) = frame.ray(xf.to_world(p));
    let chosen = state.hover.as_ref().and_then(|h| h.at(Pane::View(view), p));
    let face = crate::align3d::face_pick(model, &scene, o, d, chosen);
    let project = |m: DVec3| Some(xf.to_screen(frame.project(m).truncate()));
    let free = face.as_ref().map(|f| f.face.point);
    let snap = snapper
        .snap(cfg, p, &project, None, free)
        .filter(|s| s.kind != SnapKind::Grid);
    let edge = snap.and_then(|s| snapper.edge_at(s.p));
    let (centres, centre) =
        crate::align3d::centres_under(model, &scene, face.as_ref(), p, &project);
    let centre = centre.filter(|_| crate::align3d::takes_centres(&state.align));
    crate::align3d::Under {
        face,
        snap,
        edge,
        level,
        centres,
        centre,
    }
}

/// The element a section or 3D view shows at drawing point `p`, found by
/// casting the viewing ray through the model.
fn ray_pick(doc: &Document, view: ElementId, kind: PaneKind, p: DVec2) -> Option<ElementId> {
    let scene = pick_scene(doc.model(), view);
    ray_pick_all(doc.model(), &scene, view, kind, p, None)
        .first()
        .copied()
}

/// The model view `view` picks from: what it leaves out is not in it.
fn pick_scene(model: &creabim_core::Model, view: ElementId) -> Vec<SceneItem> {
    // what the view leaves out can't be clicked in it, and isn't in the
    // model its drawing was made from
    let (hidden, axo, phase, options) = match model.get(view).map(|e| &e.kind) {
        Some(ElementKind::View(v)) => (
            v.hidden.hidden_set(model),
            matches!(v.kind, creabim_core::ViewKind::Axonometric { .. }),
            crate::phaselens::lensed(view, v.phase),
            v.options.clone(),
        ),
        _ => Default::default(),
    };
    // nor can what is not there at the phase the view stands at (a
    // ghosted option is seen, not clicked)
    let at = creabim_core::phases::model_at(model, phase, &options, &[]);
    let seen = creabim_core::visibility::without(at.as_deref().unwrap_or(model), &hidden);
    let mut scene = creabim_geom::build_scene(&seen);
    scene.retain(|s| !hidden.contains(&s.id));
    // a 3D view's crop boxes cut its drawing, and what they cut away
    // can't be clicked either
    if axo {
        let boxes = creabim_geom::cropbox::active(model, Some(view));
        creabim_geom::cropbox::crop_scene(model, &mut scene, &boxes);
    }
    scene
}

/// [`pick_scene`], kept until the model changes.
fn cached_pick_scene(
    state: &mut PlanState,
    doc: &Document,
    view: ElementId,
) -> Arc<Vec<SceneItem>> {
    let rev = doc.model().revision();
    if let Some((r, s)) = state.pick_scenes.get(&view) {
        if *r == rev {
            return s.clone();
        }
    }
    // tests pick at once, as before
    if cfg!(test) {
        let s = Arc::new(pick_scene(doc.model(), view));
        state.pick_scenes.insert(view, (rev, s.clone()));
        return s;
    }
    // one built off the frame and done: taken
    if let Some((r, rx)) = state.pick_jobs.get(&view) {
        if let Ok(s) = rx.try_recv() {
            let r = *r;
            state.pick_jobs.remove(&view);
            let s = Arc::new(s);
            state.pick_scenes.insert(view, (r, s.clone()));
            if r == rev {
                return s;
            }
        }
    }
    // else one started for this revision
    if state.pick_jobs.get(&view).is_none_or(|(r, _)| *r != rev) {
        let (tx, rx) = std::sync::mpsc::channel();
        let model = doc.model().clone();
        std::thread::spawn(move || {
            let _ = tx.send(pick_scene(&model, view));
        });
        state.pick_jobs.insert(view, (rev, rx));
    }
    // meanwhile the last one, a step behind (only rays through the floor
    // use it; what the plan draws is picked from its drawing)
    state
        .pick_scenes
        .get(&view)
        .map(|(_, s)| s.clone())
        .unwrap_or_default()
}

/// Every element the viewing ray through drawing point `p` passes, nearest
/// first. In a plan the ray looks down through the level, and only what
/// the plan draws (`drawn`) counts.
fn ray_pick_all(
    model: &creabim_core::Model,
    scene: &[SceneItem],
    view: ElementId,
    kind: PaneKind,
    p: DVec2,
    drawn: Option<&HashSet<ElementId>>,
) -> Vec<ElementId> {
    let (o, d) = match kind {
        PaneKind::Plan { level } => {
            // a click inside a floor picks its slab
            let slabs: Vec<(ElementId, Vec<DVec2>)> = model
                .iter()
                .filter_map(|e| match &e.kind {
                    ElementKind::Slab(s) if s.level == level => Some((e.id, s.outline())),
                    _ => None,
                })
                .collect();
            let slab = slab_at(&slabs, p);
            let Some(drawn) = drawn else {
                return slab.into_iter().collect();
            };
            let z = model.level(level).map_or(0.0, |l| l.elevation);
            let o = DVec3::new(p.x, p.y, z + 100.0);
            let mut out: Vec<ElementId> =
                creabim_geom::pick::pick_all(scene, o.as_vec3(), -glam::Vec3::Z)
                    .into_iter()
                    .filter(|h| drawn.contains(&h.id) && o.z - h.t as f64 > z - 1.0)
                    .map(|h| h.id)
                    .collect();
            if let Some(s) = slab.filter(|s| !out.contains(s)) {
                out.push(s);
            }
            return out;
        }
        PaneKind::Schedule => return vec![],
        PaneKind::Section { start, end } => {
            let look = (end - start).normalize_or_zero().perp();
            let xy = PaneKind::section_point(start, end, p);
            (DVec3::new(xy.x, xy.y, p.y), look.extend(0.0))
        }
        PaneKind::Axonometric => {
            let Some(ElementKind::View(View {
                kind: ViewKind::Axonometric { azimuth, altitude },
                ..
            })) = model.get(view).map(|e| &e.kind)
            else {
                return vec![];
            };
            creabim_geom::section::AxoFrame::new(model, *azimuth, *altitude).ray(p)
        }
    };
    creabim_geom::pick::pick_all(scene, o.as_vec3(), d.as_vec3())
        .into_iter()
        .map(|h| h.id)
        .collect()
}

/// What holds others and turns see-through in X-ray: walls, slabs,
/// roofs and masses.
pub fn is_xray_host(kind: &ElementKind) -> bool {
    match kind {
        ElementKind::Wall(_) | ElementKind::Slab(_) | ElementKind::Roof(_) => true,
        ElementKind::FreeMesh(m) => !matches!(
            m.ifc_class.as_str(),
            "IfcBeam" | "IfcColumn" | "IfcMember" | "IfcStair" | "IfcRailing"
        ),
        _ => false,
    }
}

/// With X-ray on, what is inside comes before what holds it: the beam in
/// the wall is what a click takes first.
pub fn xray_order(model: &creabim_core::Model, under: &mut [ElementId]) {
    under.sort_by_key(|id| model.get(*id).is_some_and(|e| is_xray_host(&e.kind)));
}

/// Everything under drawing point `p` of a view, in the order a click
/// takes it: its linework first, then what the view ray passes.
#[allow(clippy::too_many_arguments)]
fn under_pointer(
    state: &mut PlanState,
    doc: &Document,
    view: ElementId,
    kind: PaneKind,
    plan: &Drawing,
    p: DVec2,
    tol: f64,
    mm: f64,
) -> Vec<ElementId> {
    let mut under = hit_all(&plan.items, p, tol, mm);
    if matches!(kind, PaneKind::Plan { .. }) {
        Works::of(doc.model()).first(&mut under, p);
    }
    // a crop box's dashed outline picks it (A4-5)
    for (id, lo, hi) in crop_rects(doc.model(), view) {
        let c = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
        if (0..4).any(|i| segment_distance(p, c[i], c[(i + 1) % 4]) < tol) && !under.contains(&id) {
            under.push(id);
        }
    }
    // a reference's outline picks it (MU19), after what is drawn
    for (id, r) in creabim_core::reference::in_view(doc.model(), view) {
        if r.hidden || under.contains(&id) {
            continue;
        }
        if let Some(c) = r.outline() {
            if (0..4).any(|i| segment_distance(p, c[i], c[(i + 1) % 4]) < tol) {
                under.push(id);
            }
        }
    }
    let scene = cached_pick_scene(state, doc, view);
    let drawn: HashSet<ElementId> = match kind {
        PaneKind::Plan { .. } => plan.items.iter().map(|i| i.id()).collect(),
        _ => HashSet::new(),
    };
    for id in ray_pick_all(doc.model(), &scene, view, kind, p, Some(&drawn)) {
        if !under.contains(&id) {
            under.push(id);
        }
    }
    // underlays and guides drawn from elsewhere are not picked here
    under.retain(|id| doc.model().get(*id).is_some());
    // the ground is under everything: what stands on it comes first, so a
    // click takes the pad or the paving, not the terrain round them (MU41)
    // and so is the lot: its area takes in the whole site, so pointing at
    // a wall or a paving inside it took the property (MU130). It comes
    // after everything that stands on it, and before the ground
    let ground = |id: &ElementId| match doc.model().get(*id).map(|e| &e.kind) {
        Some(ElementKind::Site(s)) if matches!(s.form, creabim_core::SiteForm::Terrain { .. }) => 2,
        Some(ElementKind::Site(s)) if matches!(s.form, creabim_core::SiteForm::Property { .. }) => {
            1
        }
        _ => 0,
    };
    under.sort_by_key(|id| ground(id));
    if state.xray.contains(&view) {
        xray_order(doc.model(), &mut under);
    }
    // in a section, grid and level lines run through everything: what the
    // building is made of comes first, Tab reaches them (MU69, grids
    // drawn in sections)
    if !matches!(kind, PaneKind::Plan { .. }) {
        let datum = |id: &ElementId| {
            matches!(
                doc.model().get(*id).map(|e| &e.kind),
                Some(ElementKind::Grid(_) | ElementKind::Level(_))
            )
        };
        under.sort_by_key(|id| datum(id));
    }
    under
}

/// A click with a drawing tool in a section or elevation. `p` is the
/// drawing point (along the line, height); `picked` is what is under it.
#[allow(clippy::too_many_arguments)]
fn section_tool(
    doc: &mut Document,
    start: DVec2,
    end: DVec2,
    tool: Tool,
    p: DVec2,
    picked: Option<ElementId>,
    state: &mut PlanState,
    selection: &mut Selection,
) -> Result<(), creabim_core::ModelError> {
    let at = |p: DVec2| PaneKind::section_point(start, end, p);
    match tool {
        Tool::Level => match state.pending.take() {
            None => {
                state.pending = Some(p);
                state.status = Some(format!(
                    "New level at {}. Click where its line ends.",
                    crate::units::elevation(p.y)
                ));
                Ok(())
            }
            Some(a) => {
                let model = doc.model();
                let z = a.y;
                if model.levels().any(|(_, l)| (l.elevation - z).abs() < 1e-4) {
                    state.status = Some("There is a level at that height already.".into());
                    return Ok(());
                }
                // up to the next level, or as tall as the one below
                let list = creabim_core::levels::sorted(model);
                let height = list
                    .iter()
                    .find(|(_, _, l)| l.elevation > z + 1e-6)
                    .map(|(_, _, l)| l.elevation - z)
                    .or_else(|| {
                        list.iter()
                            .rev()
                            .find(|(_, _, l)| l.elevation < z)
                            .map(|(_, _, l)| l.height)
                    })
                    .unwrap_or(3.0)
                    .max(0.5);
                let name = creabim_core::levels::next_name(model);
                let (mut tx, id, _) = creabim_core::levels::add(model, &name, z, height, true);
                tx.label = "Draw level".into();
                selection.only(id);
                state.status = Some(format!(
                    "Added {name} at {} with its floor plan. Drag its line to move it.",
                    crate::units::elevation(z)
                ));
                doc.commit(tx)
            }
        },
        Tool::Wall => match state.pending.take() {
            None => {
                state.pending = Some(p);
                Ok(())
            }
            Some(a) => {
                let (x0, x1) = (a.x.min(p.x), a.x.max(p.x));
                let (z0, z1) = (a.y.min(p.y), a.y.max(p.y));
                if x1 - x0 < 0.05 {
                    return Ok(());
                }
                let Some((level, elevation)) = level_below(doc, z0) else {
                    return Ok(());
                };
                let height = if z1 - z0 < 0.1 {
                    doc.model().level(level).map_or(3.0, |l| l.height)
                } else {
                    z1 - z0
                };
                // set just behind the section plane, so its face is what you drew
                let behind =
                    (end - start).normalize_or_zero().perp() * (state.wall_thickness / 2.0 + 0.01);
                let e = Element::new(
                    "Wall",
                    ElementKind::Wall(Wall {
                        level,
                        start: at(DVec2::new(x0, 0.0)) + behind,
                        end: at(DVec2::new(x1, 0.0)) + behind,
                        thickness: state.wall_thickness,
                        height,
                        base_offset: z0 - elevation,
                        material: None,
                        assembly: None,
                        flip: false,
                        location: Default::default(),
                        top: Default::default(),
                        disallow_join: [false; 2],
                        attach_top: vec![],
                        curtain: None,
                        layer_ends: vec![],
                        bulge: 0.0,
                    }),
                );
                // its type's thickness, but its drawn height
                let mut e = state.typed(doc.model(), e);
                if let ElementKind::Wall(w) = &mut e.kind {
                    w.height = height;
                }
                selection.only(e.id);
                doc.commit(Transaction::new("Draw wall in section").insert(e))
            }
        },
        Tool::Door | Tool::Window | Tool::Opening => {
            // the wall under the cursor hosts the opening (or the wall of
            // the door or window under it)
            let Some(mut host) = picked else {
                return Ok(());
            };
            if let Some(ElementKind::Opening(o)) = doc.model().get(host).map(|e| &e.kind) {
                host = o.host;
            }
            let Some(ElementKind::Wall(w)) = doc.model().get(host).map(|e| e.kind.clone()) else {
                return Ok(());
            };
            let look = (end - start).normalize_or_zero().perp();
            let d = w.direction();
            if d.perp_dot(look).abs() < 1e-6 {
                return Ok(()); // seen edge-on
            }
            // where the viewing ray through the click meets the wall's centreline
            let o = at(p);
            let t = (w.start - o).perp_dot(d) / look.perp_dot(d);
            let u = (o + look * t - w.start).dot(d);
            let base = doc.model().level(w.level).map_or(0.0, |l| l.elevation) + w.base_offset;
            let (kind, name) = match tool {
                Tool::Door => (OpeningKind::Door, "Door"),
                Tool::Opening => (OpeningKind::Hole, "Wall opening"),
                _ => (OpeningKind::Window, "Window"),
            };
            let mut e = state.typed(
                doc.model(),
                Element::new(
                    name,
                    ElementKind::Opening(Opening {
                        host,
                        kind,
                        offset: 0.0,
                        sill: 0.0,
                        width: if kind == OpeningKind::Door { 0.9 } else { 1.2 },
                        height: match kind {
                            OpeningKind::Door => 2.1,
                            OpeningKind::Window => 1.3,
                            OpeningKind::Hole => 1.0,
                        },
                        material: None,
                        frame: next_frame(doc.model(), kind, state),
                        flip_hand: false,
                        flip_facing: false,
                        roof_at: None,
                        roof_build: Default::default(),
                        arrangement: None,
                    }),
                ),
            );
            let ElementKind::Opening(op) = &mut e.kind else {
                return Ok(());
            };
            if op.width > w.length() {
                return Ok(());
            }
            // where it was clicked: along the wall, and up it but for doors
            op.offset = (u - op.width / 2.0).clamp(0.0, w.length() - op.width);
            if kind != OpeningKind::Door {
                op.sill =
                    (p.y - base - op.height / 2.0).clamp(0.0, (w.height - op.height).max(0.0));
            }
            selection.only(e.id);
            doc.commit(
                Transaction::new(format!("Place {} in section", name.to_lowercase())).insert(e),
            )
        }
        Tool::Mass => {
            let Some((level, elevation)) = level_below(doc, p.y) else {
                return Ok(());
            };
            let xy = at(p);
            let e = Element::new(
                "Mass",
                ElementKind::FreeMesh(FreeMesh::new(
                    Some(level),
                    DVec3::new(xy.x, xy.y, p.y - elevation),
                    EditMesh::cuboid(DVec3::new(2.0, 2.0, 3.0)),
                )),
            );
            selection.only(e.id);
            doc.commit(Transaction::new("Add mass").insert(e))
        }
        Tool::Slab => match state.pending.take() {
            None => {
                state.pending = Some(p);
                Ok(())
            }
            Some(a) => {
                let (x0, x1) = (a.x.min(p.x), a.x.max(p.x));
                if x1 - x0 < 0.1 {
                    return Ok(());
                }
                // the top snaps to the nearest level; the drawn height is the thickness
                let top = a.y.max(p.y);
                let Some((level, _)) = doc
                    .model()
                    .levels()
                    .map(|(e, l)| (e.id, l.elevation))
                    .min_by(|x, y| (x.1 - top).abs().total_cmp(&(y.1 - top).abs()))
                else {
                    return Ok(());
                };
                let dy = (a.y - p.y).abs();
                let thickness = if dy > 0.05 { dy } else { 0.25 };
                let (d0, d1) = depth_of_walls(doc, level, start, end, x0, x1);
                let look = (end - start).normalize_or_zero().perp();
                let corner = |x: f64, d: f64| at(DVec2::new(x, 0.0)) + look * d;
                let mut boundary = vec![
                    corner(x0, d0),
                    corner(x1, d0),
                    corner(x1, d1),
                    corner(x0, d1),
                ];
                if signed_area(&boundary) < 0.0 {
                    boundary.reverse();
                }
                let e = Element::new(
                    "Slab",
                    ElementKind::Slab(Slab {
                        layer_edges: vec![],
                        level,
                        boundary,
                        thickness,
                        material: None,
                        top_offset: 0.0,
                        bulges: vec![],
                        assembly: None,
                        holes: vec![],
                    }),
                );
                // the slab type in use, unless its thickness was drawn
                let e = if dy > 0.05 {
                    e
                } else {
                    state.typed(doc.model(), e)
                };
                selection.only(e.id);
                doc.commit(Transaction::new("Draw slab in section").insert(e))
            }
        },
        Tool::Guide => match state.pending.take() {
            None => {
                state.pending = Some(p);
                Ok(())
            }
            Some(a) if a.distance(p) > 0.01 || state.guide_edge.is_some() => {
                // a copy: the edge moved from where it was taken to here
                let (a, p) = match state.guide_edge.take() {
                    Some((e0, e1)) => (e0 + (p - a), e1 + (p - a)),
                    None => (a, p),
                };
                let e = Element::new(
                    "Guide",
                    ElementKind::Guide(creabim_core::Guide {
                        level: None,
                        a: at(a).extend(a.y),
                        b: at(p).extend(p.y),
                    }),
                );
                doc.commit(Transaction::new("Add guide").insert(e))
            }
            Some(_) => Ok(()),
        },
        Tool::Section | Tool::Elevation => match state.pending.take() {
            None => {
                state.pending = Some(p);
                Ok(())
            }
            Some(a) if (p.x - a.x).abs() > 0.05 => {
                // a new view square to this one, through the first click,
                // looking towards the side of the second
                let look = (end - start).normalize_or_zero().perp();
                let base = at(DVec2::new(a.x, 0.0));
                let (near, far) = model_depth(doc, start, end);
                let (s, e) = if p.x > a.x {
                    (base + look * far, base + look * near)
                } else {
                    (base + look * near, base + look * far)
                };
                add_section_view(doc, s, e, tool == Tool::Section, selection)
            }
            Some(_) => Ok(()),
        },
        _ => Ok(()),
    }
}

fn signed_area(p: &[DVec2]) -> f64 {
    (0..p.len())
        .map(|i| p[i].perp_dot(p[(i + 1) % p.len()]))
        .sum::<f64>()
        / 2.0
}

/// How far back from a section line the walls of `level` reach, within the
/// drawing range `x0..x1`: (nearest, furthest). A slab drawn in a section
/// runs back this far.
fn depth_of_walls(
    doc: &Document,
    level: ElementId,
    start: DVec2,
    end: DVec2,
    x0: f64,
    x1: f64,
) -> (f64, f64) {
    let along = (end - start).normalize_or_zero();
    let look = along.perp();
    let mut range: Option<(f64, f64)> = None;
    for e in doc.model().iter() {
        let ElementKind::Wall(w) = &e.kind else {
            continue;
        };
        if w.level != level {
            continue;
        }
        let pts = [w.start, w.end].map(|q| ((q - start).dot(along), (q - start).dot(look)));
        let (u0, u1) = (pts[0].0.min(pts[1].0), pts[0].0.max(pts[1].0));
        if u1 < x0 - 0.5 || u0 > x1 + 0.5 {
            continue;
        }
        for (_, d) in pts {
            range = Some(range.map_or((d, d), |(lo, hi)| (lo.min(d), hi.max(d))));
        }
    }
    match range {
        Some((lo, hi)) if hi - lo > 0.1 => (lo, hi),
        _ => (0.0, 5.0),
    }
}

/// The range of the model in front of a section line, with room to spare:
/// how long a section drawn across it should be.
fn model_depth(doc: &Document, start: DVec2, end: DVec2) -> (f64, f64) {
    let look = (end - start).normalize_or_zero().perp();
    let scene = (*creabim_geom::shared_scene(doc.model())).clone();
    let (lo, hi) = scene
        .iter()
        .flat_map(|s| s.mesh.positions.iter())
        .map(|p| (DVec2::new(p[0] as f64, p[1] as f64) - start).dot(look))
        .fold((f64::MAX, f64::MIN), |(a, b), d| (a.min(d), b.max(d)));
    if lo > hi {
        (-2.0, 10.0)
    } else {
        (lo - 2.0, hi + 2.0)
    }
}

/// Add a section or elevation view along `a` → `b`, named in sequence.
fn add_section_view(
    doc: &mut Document,
    a: DVec2,
    b: DVec2,
    cut: bool,
    selection: &mut Selection,
) -> Result<(), creabim_core::ModelError> {
    let n = doc
        .model()
        .iter()
        .filter(|e| matches!(&e.kind, ElementKind::View(v) if matches!(v.kind, ViewKind::Section { cut: c, .. } if c == cut)))
        .count();
    let name = if cut {
        format!("Section {}", section_letter(n))
    } else {
        format!("Elevation {}", n + 1)
    };
    let e = Element::new(
        name.clone(),
        ElementKind::View(View::section(a, b, 30.0, cut)),
    );
    selection.only(e.id);
    doc.commit(Transaction::new(format!("Add {name}")).insert(e))
}

/// Preview of a section or elevation being placed in a section: a vertical
/// line at the first click with an arrow to the side it will look.
fn preview_cross_section(painter: &egui::Painter, xf: &Xf, a: DVec2, b: DVec2, accent: Color32) {
    let lo = xf.to_world(xf.rect.left_bottom());
    let hi = xf.to_world(xf.rect.right_top());
    let stroke = Stroke::new(1.5, accent);
    painter.line_segment(
        [
            xf.to_screen(DVec2::new(a.x, lo.y)),
            xf.to_screen(DVec2::new(a.x, hi.y)),
        ],
        stroke,
    );
    let side = if b.x > a.x { 1.0 } else { -1.0 };
    let tip = xf.to_screen(DVec2::new(a.x, b.y)) + Vec2::new(side * 24.0, 0.0);
    let root = xf.to_screen(DVec2::new(a.x, b.y));
    painter.arrow(root, tip - root, stroke);
}

/// From a pane's drawing point to the model: plans are level at `z`;
/// sections stand on their line.
fn pane_to_model(space: Space, w: DVec2, z: f64) -> DVec3 {
    match space {
        Space::Section { start, end } => {
            (start + (end - start).normalize_or_zero() * w.x).extend(w.y)
        }
        Space::Plan | Space::Model => w.extend(z),
    }
}

/// A part's size while Divide is open: small, on a paper chip.
/// None is drawn over one already in `taken`: zoomed out, sizes thin out
/// rather than pile up.
pub fn divide_label(
    painter: &egui::Painter,
    at: Pos2,
    text: &str,
    ink: Color32,
    taken: &mut Vec<egui::Rect>,
) {
    let font = egui::FontId::proportional(11.0);
    let g = painter.layout_no_wrap(text.to_string(), font, ink);
    let r = egui::Rect::from_center_size(at, g.size() + egui::vec2(8.0, 4.0));
    if taken.iter().any(|t| t.expand(2.0).intersects(r)) {
        return;
    }
    taken.push(r);
    painter.rect(
        r,
        4.0,
        Color32::from_black_alpha(170),
        Stroke::new(1.0, ink.gamma_multiply(0.6)),
        egui::StrokeKind::Middle,
    );
    painter.galley(r.min + egui::vec2(4.0, 2.0), g, ink);
}

fn model_to_pane(space: Space, p: DVec3) -> DVec2 {
    match space {
        Space::Section { start, end } => DVec2::new(
            (p.truncate() - start).dot((end - start).normalize_or_zero()),
            p.z,
        ),
        Space::Plan | Space::Model => p.truncate(),
    }
}

/// The Move and Rotate tools, and G, R and S (move, rotate and scale) and
/// Shift+D (copy) while the pointer is over this pane. Returns `Some` while
/// one runs here. `pick` finds the element under the pointer, for
/// Shift+click while the base point is picked.
#[allow(clippy::too_many_arguments)]
fn keyboard_edits(
    ui: &egui::Ui,
    doc: &mut Document,
    view: ElementId,
    space: Space,
    painter: &egui::Painter,
    response: &egui::Response,
    state: &mut PlanState,
    selection: &mut Selection,
    map: &PaneMap,
    pick: &dyn Fn(Pos2) -> Option<ElementId>,
) -> Option<Option<String>> {
    let shift = ui.input(|i| i.modifiers.shift);
    // the area that runs this view's move: the one that took it, else the
    // one under the pointer takes it now
    let mine = |m: &Modal| {
        m.pane == Some(view)
            && match m.owner {
                Some(o) => o == response.id,
                None => response.hovered(),
            }
    };
    // a tool's move still waiting for its base point follows the pointer
    // into another area (MU29: crossing a view on the way left it stuck
    // there): it is put back and armed again for this one
    let follows = state.modal.as_ref().is_some_and(|m| {
        m.flow == Flow::Tool && m.stage() == Stage::Base && response.hovered() && !mine(m)
    });
    if follows {
        if let Some(mut old) = state.modal.take() {
            let again = (old.mode, old.duplicated);
            old.discard(doc);
            state.armed = Some(again);
        }
    }
    if let Some(m) = state.modal.as_mut().filter(|m| mine(m)) {
        m.owner = Some(response.id);
        // Shift+click while the base point is picked adds or takes away
        if m.stage() == Stage::Base && shift && response.clicked() {
            if let Some(id) = response.interact_pointer_pos().and_then(pick) {
                selection.toggle(id);
                let grab = response.hover_pos().unwrap_or_default();
                state.status = Some(restart_modal(
                    doc,
                    &mut state.modal,
                    &selection.all(),
                    grab,
                    state.pivot,
                    state.cursor,
                ));
            }
            return Some(None);
        }
        m.panel(ui.ctx(), response.rect);
        let click = response.clicked() || m.pull_drag(response);
        let right = response.secondary_clicked() && !shift;
        match m.update(
            doc,
            ui.ctx(),
            painter,
            response.hover_pos(),
            click,
            right,
            map,
        ) {
            Outcome::Running(s) => state.status = Some(s),
            Outcome::Done(s) => {
                // Pull stays in hand for the next face
                if m.mode == Mode::Pull {
                    state.armed = Some((Mode::Pull, false));
                }
                state.status = Some(s);
                state.modal = None;
            }
            Outcome::Cancelled(s) => {
                state.status = Some(s);
                state.modal = None;
            }
        }
        if let Some(m) = &state.modal {
            if m.mode != Mode::Pull {
                state.move_target = m.works_on;
            }
        }
        return Some(None);
    }
    if state.modal.is_some() || !response.hovered() || ui.ctx().egui_wants_keyboard_input() {
        return None;
    }
    let pulling = matches!(state.armed, Some((Mode::Pull, _)));
    if !pulling {
        selection.active?;
    }
    let keys = start_keys(ui.ctx());
    let flow = if keys.is_some() {
        Flow::Keys
    } else {
        Flow::Tool
    };
    let mode = keys.or(state.armed)?;
    // Shift+D on views copies them at once, as the Duplicate button does
    if mode == (Mode::Move, true) {
        if let Some(r) = modal::duplicate_views(doc, &selection.all()) {
            state.armed = None;
            state.status = Some(match r {
                Ok(copies) => {
                    let said = modal::views_duplicated(doc, &copies);
                    selection.extend(copies, false);
                    said
                }
                Err(e) => e,
            });
            return Some(None);
        }
    }
    if keys.is_none() && !pulling && only_paper(doc, &selection.all()) {
        return None;
    }
    let grab = response.hover_pos()?;
    state.armed = None;
    let extras = (state.armed_grip.take(), std::mem::take(&mut state.in_place));
    let stretch = state.stretch_parts.take();
    state.cancel();
    let ids = selection.all();
    match start_modal(
        doc,
        mode,
        &ids,
        None,
        space,
        Some(view),
        grab,
        state.pivot,
        state.cursor,
        flow,
        state.move_target,
        None,
    )
    .and_then(|(m, copies)| Ok((with_extras(doc, m, extras)?, copies)))
    {
        Ok((mut m, copies)) => {
            if let Some(parts) = stretch {
                m.set_stretch(parts);
            }
            if copies != ids {
                selection.extend(copies, false);
            }
            state.status = Some(start_hint(&m));
            state.modal = Some(m);
        }
        Err(e) => state.status = Some(e),
    }
    Some(None)
}

impl PlanState {
    /// Edit mode's handle was taken: arm the move, the turn or the scale
    /// its part stands for, to start in the view under the pointer.
    /// Returns what to say.
    pub fn take_handle(&mut self) -> Option<String> {
        let Some(handles::GripRequest::Handle(part, by_drag, shift)) = self.grips.request else {
            return None;
        };
        self.grips.request = None;
        let start = handles::HandleStart::of(part, by_drag, shift);
        handles::set_handle_start(start);
        self.armed = Some((start.mode, false));
        Some(format!(
            "{}: move the pointer, or type a value.",
            part.says()
        ))
    }
}

/// Is everything selected written on a sheet (notes, marks)? Then only a
/// sheet runs its Move: a plan or the 3D view the pointer crosses on the
/// way must not take it (MU30).
pub fn only_paper(doc: &Document, ids: &[ElementId]) -> bool {
    let model = doc.model();
    let sheet = |v: ElementId| matches!(model.get(v).map(|e| &e.kind), Some(ElementKind::Sheet(_)));
    !ids.is_empty()
        && ids.iter().all(|id| match model.get(*id).map(|e| &e.kind) {
            Some(ElementKind::PaperMark(_)) => true,
            Some(ElementKind::Note(n)) => sheet(n.view),
            _ => false,
        })
}

/// What the edge palette asked of an armed edit: the grip [`Mode::Grip`]
/// drives, and whether an offset changes the original.
pub fn with_extras(
    doc: &mut Document,
    mut m: Modal,
    (grip, in_place): (Option<crate::handles::HandleKind>, bool),
) -> Result<Modal, String> {
    if let Some(k) = grip {
        m = m.with_grip(doc, k)?;
    }
    if in_place {
        m.set_copy(doc, false);
    }
    Ok(m)
}

/// What to do first, as a running edit begins.
pub fn start_hint(m: &Modal) -> String {
    if m.duplicated && m.mode == Mode::Move {
        return match m.stage() {
            Stage::Base => "Copy made: click a base point, then where it goes. Typed distances work; Esc or a right-click takes the copy away.".into(),
            _ => "Copy made: move the pointer and click where it goes. Esc or a right-click takes the copy away.".into(),
        };
    }
    match (m.stage(), m.mode) {
        (Stage::Base, Mode::Rotate) => "Rotate: click the centre to turn about.".into(),
        (_, Mode::Pull) => {
            "Pull: point at a face and click on it, then move out or in. Esc leaves the tool.".into()
        }
        (Stage::Base, _) => {
            "Move: click the base point, the point you move by. It snaps; Shift+click adds or takes away elements.".into()
        }
        (_, mode) => format!(
            "{mode:?}: move the pointer, or type a value. X, Y or Z keeps to an axis."
        ),
    }
}

/// Start the running Move or Rotate tool again on a new selection, as it
/// was changed with Shift+click before the base point. Returns a message.
pub fn restart_modal(
    doc: &mut Document,
    modal: &mut Option<Modal>,
    ids: &[ElementId],
    grab: Pos2,
    pivot: modal::Pivot,
    cursor: Option<DVec3>,
) -> String {
    let Some(old) = modal.take() else {
        return String::new();
    };
    let mut old = old;
    old.undo_edit(doc);
    if ids.is_empty() {
        return "Nothing selected: click what to move.".into();
    }
    match Modal::start(
        doc,
        old.mode,
        ids,
        None,
        old.space,
        old.pane,
        grab,
        pivot,
        cursor,
        old.plane(),
    ) {
        Ok(m) => {
            let n = ids.len();
            *modal = Some(m.with_flow(Flow::Tool).with_works_on(old.works_on));
            format!(
                "{n} selected. Shift+click adds or takes away more; click the base point to go on."
            )
        }
        Err(e) => e,
    }
}

/// A frame of the Stretch box from `p0` to `now`: the corners and ends it
/// takes lit and counted; let go, a Move of just those is armed.
#[allow(clippy::too_many_arguments)]
fn stretch_box_frame(
    doc: &Document,
    plan: &creabim_geom::PlanView,
    xf: &Xf,
    painter: &egui::Painter,
    p0: Pos2,
    now: Pos2,
    ended: bool,
    state: &mut PlanState,
    selection: &mut Selection,
) {
    let mut bounds: Vec<(ElementId, (DVec2, DVec2))> = vec![];
    for it in &plan.items {
        if creabim_geom::plan::is_underlay(it) || !box_selectable(doc, it.id()) {
            continue;
        }
        let Some((lo, hi)) = it.bounds() else {
            continue;
        };
        match bounds.iter_mut().find(|(id, _)| *id == it.id()) {
            Some((_, b)) => *b = (b.0.min(lo), b.1.max(hi)),
            None => bounds.push((it.id(), (lo, hi))),
        }
    }
    let (a, b) = (xf.to_world(p0), xf.to_world(now));
    let (parts, n) = crate::stretchtool::parts_in(doc.model(), &bounds, a, b);
    if !ended {
        let lit = crate::stretchtool::lit_points(doc.model(), &parts);
        let orange = crate::theme::ORANGE;
        for p in lit {
            let s = xf.to_screen(p);
            painter.circle(s, 4.0, orange, egui::Stroke::new(1.5, egui::Color32::WHITE));
        }
        boxsel::draw_count(painter, now, n);
        return;
    }
    state.boxsel = None;
    if parts.is_empty() {
        state.status = Some(
            "The box took no corners: drag it over the corners and ends to stretch. Esc stops."
                .into(),
        );
        return;
    }
    selection.extend(parts.iter().map(|(id, _)| *id), false);
    state.stretch_box = false;
    state.stretch_parts = Some(parts);
    state.armed = Some((Mode::Move, false));
    state.status = Some(format!(
        "Stretch: {n} corners and ends taken. Click the base point, then where it goes, or type a distance."
    ));
}

/// Which keyboard edit the keys pressed this frame start, if any:
/// (mode, duplicate first).
pub fn start_keys(ctx: &egui::Context) -> Option<(Mode, bool)> {
    let sketching = crate::sketch::sketching(ctx);
    // the keymap's keys, by the background (first-run setup)
    if crate::keymap::active(ctx) {
        use crate::keymap::{fired, Act};
        return if fired(ctx, Act::Copy) {
            Some((Mode::Move, true))
        } else if fired(ctx, Act::Grab) {
            Some((Mode::Move, false))
        } else if fired(ctx, Act::Rotate) && !sketching {
            Some((Mode::Rotate, false))
        } else if fired(ctx, Act::Scale) {
            Some((Mode::Scale, false))
        } else {
            None
        };
    }
    ctx.input(|i| {
        if i.modifiers.command || i.modifiers.alt {
            return None;
        }
        let shift = i.modifiers.shift;
        if shift && i.key_pressed(egui::Key::D) {
            Some((Mode::Move, true))
        } else if shift {
            None
        } else if i.key_pressed(egui::Key::G) {
            Some((Mode::Move, false))
        } else if i.key_pressed(egui::Key::R) && !sketching {
            Some((Mode::Rotate, false))
        } else if i.key_pressed(egui::Key::S) {
            Some((Mode::Scale, false))
        } else {
            None
        }
    })
}

/// Start a keyboard edit on the selection, copying it first for Shift+D.
/// Returns the edit and the elements it works on (the copies, if made).
#[allow(clippy::too_many_arguments)]
pub fn start_modal(
    doc: &mut Document,
    (mode, duplicate): (Mode, bool),
    ids: &[ElementId],
    face: Option<usize>,
    space: Space,
    pane: Option<ElementId>,
    grab: Pos2,
    pivot: modal::Pivot,
    cursor: Option<DVec3>,
    flow: Flow,
    works_on: modal::Target,
    plane: Option<creabim_core::Workplane>,
) -> Result<(Modal, Vec<ElementId>), String> {
    let targets = if duplicate {
        modal::duplicate(doc, ids)?
    } else {
        ids.to_vec()
    };
    // a selected mass face is what G moves; the Move tool picks its face
    // itself when it works on faces
    let face = face.filter(|_| !duplicate && flow == Flow::Keys);
    match Modal::start(
        doc, mode, &targets, face, space, pane, grab, pivot, cursor, plane,
    ) {
        Ok(mut m) => {
            m.duplicated = duplicate;
            // a mass's picked vertices in Edit mode are what moves (MU39)
            let mut on_picked = false;
            if let ([one], false) = (targets.as_slice(), duplicate) {
                if let Some(vs) = crate::handles::mesh_pick(*one) {
                    m.set_verts(doc, *one, vs);
                    on_picked = true;
                } else if let Some(cs) = crate::handles::corner_pick(*one) {
                    m.set_corners(doc, *one, cs);
                    on_picked = true;
                }
            }
            if mode == Mode::Inset {
                let faces = crate::handles::take_inset_start().unwrap_or_default();
                m = m.with_inset(doc, faces)?;
                return Ok((m.with_flow(flow), targets));
            }
            // taken by a part of the handle, the edit keeps to its axis or
            // its plane
            let from_handle = crate::handles::take_handle_start();
            let mut flow = flow;
            if on_picked {
                if let Some(start) = from_handle.filter(|h| h.mode == mode) {
                    m = m.taken_by_handle(start.axis, start.plane, start.by_drag, start.press);
                    // it follows the pointer from the handle at once: no
                    // base point to click first
                    flow = Flow::Keys;
                }
            }
            Ok((m.with_flow(flow).with_works_on(works_on), targets))
        }
        Err(e) => {
            if duplicate {
                doc.discard_last();
            }
            Err(e)
        }
    }
}

/// Drawing a sketch in a plan or section. Returns `Some` when it used the
/// frame's input.
#[allow(clippy::too_many_arguments)]
fn sketch_tool(
    ui: &egui::Ui,
    doc: &mut Document,
    view: ElementId,
    space: Space,
    xf: &Xf,
    painter: &egui::Painter,
    response: &egui::Response,
    state: &mut PlanState,
    selection: &mut Selection,
    hover: Option<DVec3>,
    shift: bool,
    exact: bool,
) -> Option<Option<String>> {
    let project = |p: DVec3| Some(xf.to_screen(model_to_pane(space, p)));
    // a sketch being drawn in another pane is left alone
    let mine = state.sketch.as_ref().is_some_and(|d| d.pane == Some(view));
    let draft = state.sketch.as_mut().filter(|d| d.pane == Some(view));
    if let Some(hint) = crate::sketch::shape_controls(ui, response.rect, Some(view), draft) {
        state.status = Some(hint);
    }
    let tol = 10.0 / xf.scale as f64;
    let on_plane = match (&state.sketch, hover) {
        (Some(d), Some(m)) if mine => Some(d.snap(d.plane.to_plane(m), tol, shift, !exact)),
        _ => None,
    };
    if let (Some(d), true) = (&state.sketch, mine) {
        d.draw(painter, &project, on_plane);
    }
    if !mine && !response.clicked() {
        return None;
    }
    let (enter, escape) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    let finish = |doc: &mut Document, state: &mut PlanState, selection: &mut Selection, closed| {
        let d = state.sketch.take()?;
        match d.finish(doc, closed) {
            Ok(Some(id)) => {
                selection.only(id);
                state.status = Some(
                    "Sketch added. Press E to extrude it into a mass, or G to move it.".into(),
                );
                Some(None)
            }
            Ok(None) => Some(None),
            Err(e) => Some(Some(e.to_string())),
        }
    };
    if escape && state.sketch.is_some() {
        state.sketch = None;
        return Some(None);
    }
    if enter || (response.secondary_clicked() && state.sketch.is_some()) {
        return finish(doc, state, selection, false);
    }
    if !response.clicked() {
        return None;
    }
    let m = hover?;
    if !mine {
        state.sketch = None;
    }
    match &mut state.sketch {
        None => {
            let plane = match space {
                Space::Section { start, end } => Workplane {
                    origin: m,
                    x: (end - start).normalize_or_zero().extend(0.0),
                    y: DVec3::Z,
                },
                Space::Plan | Space::Model => Workplane {
                    origin: m,
                    ..Workplane::ground(m.z)
                },
            };
            let draft = SketchDraft::new(plane, Some(view));
            let shape = crate::sketch::shape(ui.ctx());
            state.status = Some(crate::sketch::hint(shape, Some(&draft)));
            state.sketch = Some(draft);
            Some(None)
        }
        Some(d) => match d.click(on_plane?) {
            Some(Click::Closed) => finish(doc, state, selection, true),
            Some(Click::Added) => {
                state.status = Some(crate::sketch::hint(d.shape, Some(d)));
                Some(None)
            }
            None => Some(None),
        },
    }
}

/// The element and the walls whose ends meet it: what a drag on it may
/// change, so it must not snap to itself.
pub fn joined_with(doc: &Document, id: ElementId) -> Vec<ElementId> {
    let mut out = vec![id];
    if let Some(ElementKind::Wall(w)) = doc.model().get(id).map(|e| &e.kind) {
        for e in doc.model().iter() {
            if let ElementKind::Wall(o) = &e.kind {
                let touch = [o.start, o.end]
                    .iter()
                    .any(|p| p.distance(w.start) < 1e-3 || p.distance(w.end) < 1e-3);
                if touch && e.id != id {
                    out.push(e.id);
                }
            }
        }
    }
    out
}

/// Rebuild a view's snaps when the model changed, but not during a drag,
/// which changes the model every frame.
/// How far an object's centre lines reach past its footprint, metres.
const OBJECT_CENTRE_REACH: f64 = 0.6;

fn refresh_snaps(
    state: &mut PlanState,
    model: &creabim_core::Model,
    view: ElementId,
    plan: &Drawing,
    exclude: &[ElementId],
) {
    let revision = model.revision();
    // or follows the pointer in another view: these snaps are not asked
    // for until it ends (10 ms of every step of a pull in 3D, MU46)
    let dragging = state.grip.is_some() || state.modal.as_ref().is_some_and(|m| m.freezes_snaps());
    // built once per model change with everything in it; what is left out
    // (the selection) is only hidden, so selecting builds nothing (MU39)
    let fresh = state
        .snaps
        .get(&view)
        .is_some_and(|(r, _, _)| *r == revision || dragging);
    if !fresh {
        let _p = crate::app::Perf::new("plan snaps: build");
        let mut s = Snapper::from_drawing(&plan.items, &[]);
        s.add_underlay_centres(model, view);
        // an object's middle and centre lines, in a plan of its level
        if let Some(ElementKind::View(v)) = model.get(view).map(|e| &e.kind) {
            if let creabim_core::ViewKind::Plan { level, .. } = v.kind {
                s.add_object_centres(model, Some(level), OBJECT_CENTRE_REACH);
            }
        }
        add_reference_snaps(&mut s, state, model, view);
        state.snaps.insert(view, (revision, vec![], s));
    }
    if let Some((_, ex, s)) = state.snaps.get_mut(&view) {
        if ex != exclude {
            *ex = exclude.to_vec();
            s.hide(exclude);
        }
    }
}

/// The lines of the references shown in `view` (another view's linework,
/// a DXF drawing) snap as an underlay's do: ends, middles and along them
/// (MU32). A picture has no lines to snap to.
fn add_reference_snaps(
    s: &mut Snapper,
    state: &mut PlanState,
    model: &creabim_core::Model,
    view: ElementId,
) {
    // a huge drawing is cut short rather than slowing every snap
    const MOST: usize = 20_000;
    let mut n = 0;
    for e in model.iter() {
        let ElementKind::Reference(r) = &e.kind else {
            continue;
        };
        if r.hidden || !r.shown_in.contains(&view) {
            continue;
        }
        let crate::trace::Shown::Lines(lines) = state.ref_cache.get(model, e.id, r) else {
            continue;
        };
        for l in lines.iter() {
            for w in l.windows(2) {
                if n >= MOST {
                    return;
                }
                s.add_segment(
                    r.place(w[0]).extend(0.0),
                    r.place(w[1]).extend(0.0),
                    crate::snaps::Source::Underlay,
                    Some(e.id),
                );
                n += 1;
            }
        }
    }
}

/// Where a value box beside the point `at` goes, `size` big, so it stays
/// inside `bounds` (its view area): to the point's right and a little up,
/// or to its left when the right edge is near, then pushed in from any
/// edge (MU55-02).
fn pill_pos(at: egui::Pos2, size: Vec2, bounds: Rect) -> egui::Pos2 {
    let gap = 12.0;
    let mut x = at.x + gap;
    if x + size.x > bounds.right() - 4.0 {
        x = at.x - gap - size.x;
    }
    let x = x.clamp(
        bounds.left() + 4.0,
        (bounds.right() - 4.0 - size.x).max(bounds.left() + 4.0),
    );
    let y = (at.y - 14.0).clamp(
        bounds.top() + 4.0,
        (bounds.bottom() - 4.0 - size.y).max(bounds.top() + 4.0),
    );
    egui::pos2(x, y)
}

/// The floor plans' ranges in a section (MU34 A1, MU36): a band per plan
/// from its view depth up to its cut, as wide as the section's range
/// (its own ends, dragged out or in, kept per section); the cut a solid
/// line, the depth dashed. One plan is read at a time: the one dragged,
/// selected or under the pointer shows its band, name and grips; the
/// others a faint cut line. A line is dragged up and down by its grip,
/// or clicked to type its height. Returns `Some` while dragging.
#[allow(clippy::too_many_arguments)]
fn plan_ranges(
    doc: &mut Document,
    painter: &egui::Painter,
    xf: &Xf,
    response: &egui::Response,
    state: &mut PlanState,
    selection: &Selection,
    grips: bool,
    section: ElementId,
    drawing: &Drawing,
) -> Option<Option<String>> {
    let model = doc.model();
    // a floor plan and a ceiling plan of each level: the floor plan's
    // band runs down from its cut, the ceiling plan's up to the level above
    // (issue #70: the bands only ever looked down)
    let mut seen = std::collections::HashSet::new();
    let mut ranges: Vec<(ElementId, f64, f64, f64, String)> = vec![];
    let mut looks_up: std::collections::HashSet<ElementId> = Default::default();
    for e in model.iter() {
        let ElementKind::View(v) = &e.kind else {
            continue;
        };
        let ViewKind::Plan { level, cut_height } = v.kind else {
            continue;
        };
        if v.callout.is_some() || !seen.insert((level, v.ceiling_plan)) {
            continue;
        }
        let Some(l) = model.level(level) else {
            continue;
        };
        let far = if v.ceiling_plan {
            looks_up.insert(e.id);
            v.graphics
                .range
                .top
                .map(|t| t.z(model, l.elevation))
                .unwrap_or_else(|| {
                    model
                        .levels()
                        .map(|(_, x)| x.elevation)
                        .filter(|z| *z > l.elevation + 1e-6)
                        .fold(None, |m: Option<f64>, z| Some(m.map_or(z, |m| m.min(z))))
                        // the top floor: up to 3 m over it
                        .unwrap_or(l.elevation + 3.0)
                })
        } else {
            let below = model
                .levels()
                .map(|(_, x)| x.elevation)
                .filter(|z| *z < l.elevation - 1e-6)
                .fold(None, |m: Option<f64>, z| Some(m.map_or(z, |m| m.max(z))));
            l.elevation - v.graphics.depth(l.elevation, below)
        };
        let (mut cut_z, mut far) = (l.elevation + cut_height, far);
        if let (Some((d, g, _)), Some(y)) = (state.range_drag, state.range_preview) {
            if d == e.id {
                match g {
                    RangeGrip::Cut => cut_z = y.max(l.elevation + 0.1),
                    RangeGrip::Depth => far = y.min(l.elevation),
                    _ => {}
                }
            }
        }
        ranges.push((e.id, l.elevation, cut_z, far, e.name.clone()));
    }
    let down_ink = crate::theme::interaction();
    let up_ink = down_ink;
    // a ceiling plan's band in orange, so the two read apart at a glance
    // the band's ends, in the drawing: its own, else the drawing's width
    let (ex0, ex1) = *state.range_ends.entry(section).or_insert_with(|| {
        drawing
            .bounds()
            .map_or((-5.0, 5.0), |(lo, hi)| (lo.x - 1.0, hi.x + 1.0))
    });
    let sx = |x: f64| xf.to_screen(DVec2::new(x, 0.0)).x;
    let y_of = |z: f64| xf.to_screen(DVec2::new(0.0, z)).y;
    let (x0, x1) = (sx(ex0), sx(ex1));
    let pointer = response.hover_pos();
    let dragged = state.range_drag.map(|d| d.0);
    let near_line = pointer
        .filter(|p| p.x >= x0 - 8.0 && p.x <= x1 + 8.0)
        .and_then(|p| {
            ranges
                .iter()
                .flat_map(|r| {
                    [
                        (r.0, (y_of(r.2) - p.y).abs()),
                        (r.0, (y_of(r.3) - p.y).abs() + 2.0),
                    ]
                })
                .filter(|(_, d)| *d < 8.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(id, _)| id)
        });
    // a band's label (its name at the left end, above its cut line) picks
    // it too: a far bigger target than the thin line at any zoom (owner,
    // after MU94)
    let near_line = near_line.or_else(|| {
        let p = pointer?;
        ranges.iter().find_map(|r| {
            let yc = y_of(r.2);
            let up = looks_up.contains(&r.0);
            let words = format!(
                "{} \u{b7} looks {} \u{b7} cut {}",
                r.4,
                if up { "up" } else { "down" },
                crate::units::len(r.2 - r.1)
            );
            let w = painter
                .layout_no_wrap(words, egui::FontId::proportional(11.0), Color32::WHITE)
                .size()
                .x;
            let rect = Rect::from_min_max(
                egui::pos2(x0 + 2.0, yc - 17.0),
                egui::pos2(x0 + 10.0 + w, yc + 2.0),
            );
            rect.contains(p).then_some(r.0)
        })
    });
    // the last one read stays only while the pointer is inside its band
    // or by its end handles; else none is lit (MU36: quiet until asked)
    let kept = state.range_focus.filter(|f| {
        state.range_edit.is_some()
            || pointer.is_some_and(|p| {
                ranges.iter().find(|r| r.0 == *f).is_some_and(|r| {
                    let (a, b) = (y_of(r.2), y_of(r.3));
                    p.y >= a.min(b) - 10.0
                        && p.y <= a.max(b) + 10.0
                        && p.x >= x0 - 14.0
                        && p.x <= x1 + 14.0
                })
            })
    });
    // a band picked by a click (or its plan selected) owns the grips;
    // hovering only lights a band up
    if state
        .range_pick
        .is_some_and(|p| !ranges.iter().any(|r| r.0 == p))
    {
        state.range_pick = None;
    }
    let picked = dragged
        .or(state.range_pick)
        .or_else(|| ranges.iter().find(|r| selection.contains(r.0)).map(|r| r.0));
    let focus = picked.or(near_line).or(kept);
    state.range_focus = focus;
    let mut over: Option<(ElementId, RangeGrip, f64)> = None;
    let mut order: Vec<&(ElementId, f64, f64, f64, String)> = ranges.iter().collect();
    order.sort_by_key(|r| Some(r.0) == focus);
    let font = egui::FontId::proportional(11.0);
    // what a grip is tested against: where the press began on the frame a
    // drag starts (the pointer has moved several pixels by then, and off a
    // small grip when frames are slow), else the pointer (owner: grabs
    // missed)
    let probe = if response.drag_started() {
        painter
            .ctx()
            .input(|i| i.pointer.press_origin())
            .or(pointer)
    } else {
        pointer
    };
    for (id, z, cut, deep, name) in order {
        let on = Some(*id) == focus;
        let (yc, yd) = (y_of(*cut), y_of(*deep));
        let up = looks_up.contains(id);
        let accent = if up { up_ink } else { down_ink };
        let ink = if on {
            accent
        } else {
            accent.gamma_multiply(0.45)
        };
        // one colour, the theme's; a ceiling plan's band is hatched and a
        // floor plan's plain, so they read apart in any theme and in grey
        // (MU93: the fixed orange didn't follow the theme)
        let band = Rect::from_min_max(egui::pos2(x0, yc.min(yd)), egui::pos2(x1, yc.max(yd)));
        if on {
            painter.rect_filled(band, 0.0, accent.gamma_multiply(0.10));
            painter.add(egui::Shape::dashed_line(
                &[egui::pos2(x0, yd), egui::pos2(x1, yd)],
                Stroke::new(1.2, accent),
                5.0,
                3.0,
            ));
        }
        if up {
            let hatch = painter.with_clip_rect(band.intersect(painter.clip_rect()));
            let step = 9.0;
            let h = band.height();
            let mut x = band.left() - h;
            while x < band.right() {
                hatch.line_segment(
                    [egui::pos2(x, band.bottom()), egui::pos2(x + h, band.top())],
                    Stroke::new(0.8, accent.gamma_multiply(if on { 0.45 } else { 0.22 })),
                );
                x += step;
            }
        }
        // its name always at the band's left end, so a line is never
        // unexplained (MU93); its far end's words when picked
        let words = if up {
            format!(
                "{name} \u{b7} looks up \u{b7} cut {}",
                crate::units::len(cut - z)
            )
        } else {
            format!(
                "{name} \u{b7} looks down \u{b7} cut {}",
                crate::units::len(cut - z)
            )
        };
        // the same place whether the plan looks up or down
        painter.text(
            egui::pos2(x0 + 6.0, yc - 3.0),
            egui::Align2::LEFT_BOTTOM,
            words,
            font.clone(),
            ink,
        );
        if on {
            let far = if up {
                format!("looks up to {}", crate::units::len(deep - z))
            } else {
                format!("view depth {}", crate::units::len(z - deep))
            };
            let (y, al) = if up {
                (yd - 3.0, egui::Align2::LEFT_BOTTOM)
            } else {
                (yd + 3.0, egui::Align2::LEFT_TOP)
            };
            painter.text(egui::pos2(x0 + 6.0, y), al, far, font.clone(), accent);
        }
        // (no arrows past the band's end: they sat 14 px from the grips and
        // read as the handle, so presses on them missed; owner found them
        // ugly too. The label and the hatch say which way it looks.)
        painter.line_segment(
            [egui::pos2(x0, yc), egui::pos2(x1, yc)],
            Stroke::new(if on { 1.8 } else { 1.0 }, ink),
        );
        if !grips || Some(*id) != picked {
            continue;
        }
        let mut gs = vec![(yc, RangeGrip::Cut)];
        if !up {
            gs.push((yd, RangeGrip::Depth));
        }
        let pointer = probe;
        for (y, g) in gs {
            let at = egui::pos2(x1, y);
            // a generous target; and once the band is picked its whole
            // line drags too, like a selected grid line
            let hot = pointer.is_some_and(|p| {
                p.distance(at) < 11.0 || (p.x >= x0 && p.x <= x1 && (p.y - y).abs() < 5.0)
            });
            {
                painter.circle(
                    at,
                    if hot { 5.5 } else { 4.5 },
                    Color32::WHITE,
                    Stroke::new(1.5, ink),
                );
            }
            if hot {
                over = Some((*id, g, *z));
            }
        }
    }
    // the band's own ends, dragged out or in like a section's
    if grips && picked.is_some() {
        let pointer = probe;
        let (ya, yb) = ranges
            .iter()
            .find(|r| Some(r.0) == picked)
            .map_or((0.0, 0.0), |r| (y_of(r.2), y_of(r.3)));
        let ym = (ya + yb) / 2.0;
        let accent = if picked.is_some_and(|f| looks_up.contains(&f)) {
            up_ink
        } else {
            down_ink
        };
        for (x, g) in [(x0, RangeGrip::Left), (x1, RangeGrip::Right)] {
            let at = egui::pos2(x, ym);
            let r = Rect::from_center_size(at, Vec2::new(7.0, 14.0));
            let hot = pointer.is_some_and(|p| r.expand(3.0).contains(p));
            painter.rect(
                r,
                2.0,
                if hot { accent } else { Color32::WHITE },
                Stroke::new(1.4, accent),
                egui::StrokeKind::Middle,
            );
            if hot {
                over = Some((picked.unwrap_or(section), g, 0.0));
            }
        }
    }
    if over.is_some() {
        painter.ctx().set_cursor_icon(match over.map(|o| o.1) {
            Some(RangeGrip::Left | RangeGrip::Right) => egui::CursorIcon::ResizeHorizontal,
            _ => egui::CursorIcon::ResizeVertical,
        });
    }
    // a click on a line: its height typed (MU36: precise values)
    // a cut line, or either grip; a depth line alone lies on the level
    // below's line, which a click there selects
    // a click on a band's line picks it (its grips appear); a double-click
    // on its line or a grip types the value; a click on empty space or Esc
    // lets it go (owner, after MU94: grabbing by hover alone fought with
    // walls and levels, and depended on the zoom)
    if response.double_clicked() && state.range_drag.is_none() {
        let hit = match over {
            Some((id, RangeGrip::Cut, _)) => Some((id, true)),
            Some((id, RangeGrip::Depth, _)) => Some((id, false)),
            Some(_) => None,
            None => near_line.map(|id| (id, true)),
        };
        if let (Some((f, is_cut)), Some(p)) = (hit, pointer) {
            let z = ranges.iter().find(|r| r.0 == f).map_or(0.0, |r| r.1);
            state.range_pick = Some(f);
            state.range_edit = Some((f, is_cut, z, p));
            return Some(None);
        }
    } else if response.clicked() && state.range_drag.is_none() && state.range_edit.is_none() {
        if over.is_some() {
            return Some(None);
        }
        if let Some(id) = near_line {
            state.range_pick = Some(id);
            let name = ranges
                .iter()
                .find(|r| r.0 == id)
                .map_or("", |r| r.4.as_str());
            state.status = Some(format!(
                "{name}: drag a grip to change it, double-click to type a value, Esc to let go."
            ));
            return Some(None);
        }
        state.range_pick = None;
    }
    if state.range_pick.is_some()
        && state.range_edit.is_none()
        && response.hovered()
        && painter.ctx().input(|i| i.key_pressed(egui::Key::Escape))
    {
        state.range_pick = None;
        return Some(None);
    }
    // a press on a grip closes the typed box first: open, it wrote its own
    // value back every frame and undid the drag (MU94: grips wouldn't
    // drag after a click on a line)
    if over.is_some() && painter.ctx().input(|i| i.pointer.primary_pressed()) {
        state.range_edit = None;
    }
    if let Some((id, is_cut, z, at)) = state.range_edit.filter(|_| state.range_drag.is_none()) {
        let cur = ranges
            .iter()
            .find(|r| r.0 == id)
            .map(|r| if is_cut { r.2 - r.1 } else { r.1 - r.3 });
        let mut v = cur.unwrap_or(0.0);
        let mut close = false;
        // inside its area (MU55-02: by the right edge it bled out of the
        // section's window): beside the point, flipped to its left near
        // the edge, its size as it was drawn last
        let box_id = egui::Id::new(("range.edit", section));
        let size = painter
            .ctx()
            .memory(|m| m.area_rect(box_id))
            .map_or(Vec2::new(200.0, 34.0), |r| r.size());
        let bounds = painter.clip_rect();
        egui::Area::new(box_id)
            .order(crate::app::overlay_order())
            .fixed_pos(pill_pos(at, size, bounds))
            .constrain_to(bounds)
            .show(painter.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(if is_cut { "Cut height" } else { "View depth" });
                        let r = ui.add(crate::units::drag(&mut v).range(0.0..=100.0));
                        r.request_focus();
                        if ui.input(|i| {
                            i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape)
                        }) {
                            close = true;
                        }
                    });
                });
            });
        if cur.is_some_and(|c| (c - v).abs() > 1e-9) {
            let r = doc.edit_merged(
                id,
                if is_cut {
                    "Plan cut height"
                } else {
                    "Plan view depth"
                },
                |e| {
                    if let ElementKind::View(view) = &mut e.kind {
                        if is_cut {
                            if let ViewKind::Plan { cut_height, .. } = &mut view.kind {
                                *cut_height = v.max(0.1);
                            }
                        } else {
                            view.graphics.range.view_depth = Some(v.max(0.0));
                        }
                    }
                },
            );
            let _ = z;
            if let Err(e) = r {
                return Some(Some(e.to_string()));
            }
        }
        // a click elsewhere, Enter or Esc closes it
        if close || (response.clicked() && near_line.is_none()) {
            state.range_edit = None;
        }
    }
    if response.drag_started_by(egui::PointerButton::Primary) && over.is_some() {
        state.range_drag = over;
        state.range_preview = None;
        state.range_edit = None;
        state.drag_owner = Some(response.id);
    }
    let (id, g, z) = state.range_drag?;
    if state.drag_owner != Some(response.id) {
        return None;
    }
    if response.drag_stopped() || !response.dragged() {
        state.range_drag = None;
        // the one change to the model, on release
        let Some(y) = state.range_preview.take() else {
            return Some(None);
        };
        if !matches!(g, RangeGrip::Cut | RangeGrip::Depth) {
            return Some(None);
        }
        let is_cut = g == RangeGrip::Cut;
        let r = doc.edit(
            id,
            if is_cut {
                "Plan cut height"
            } else {
                "Plan view depth"
            },
            |e| {
                if let ElementKind::View(v) = &mut e.kind {
                    if is_cut {
                        if let ViewKind::Plan { cut_height, .. } = &mut v.kind {
                            *cut_height = (y - z).max(0.1);
                        }
                    } else {
                        v.graphics.range.view_depth = Some((z - y).max(0.0));
                    }
                }
            },
        );
        return Some(r.err().map(|e| e.to_string()));
    }
    let w = xf.to_world(response.interact_pointer_pos()?);
    match g {
        RangeGrip::Left | RangeGrip::Right => {
            let ends = state.range_ends.entry(section).or_insert((ex0, ex1));
            if g == RangeGrip::Left {
                ends.0 = w.x.min(ends.1 - 0.5);
            } else {
                ends.1 = w.x.max(ends.0 + 0.5);
            }
            Some(None)
        }
        RangeGrip::Cut | RangeGrip::Depth => {
            let y = (w.y * 100.0).round() / 100.0;
            let is_cut = g == RangeGrip::Cut;
            // only the band moves while dragging; the model on release
            state.range_preview = Some(y);
            painter.ctx().request_repaint();
            state.status = Some(if is_cut {
                format!("Cut height {}", crate::units::len((y - z).max(0.1)))
            } else {
                format!("View depth {}", crate::units::len((z - y).max(0.0)))
            });
            Some(None)
        }
    }
}

/// What a grip of a plan's range in a section moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RangeGrip {
    Cut,
    Depth,
    Left,
    Right,
}

/// Handles on a selected section or elevation line. Returns `Some` while a
/// handle is being dragged (the pane skips its other input then).
fn drag_section_line(
    doc: &mut Document,
    painter: &egui::Painter,
    xf: &Xf,
    response: &egui::Response,
    state: &mut PlanState,
    selection: Option<ElementId>,
) -> Option<Option<String>> {
    let id = selection?;
    // a detail view's box has grips of its own, not its parent's line
    let Some(ElementKind::View(View {
        kind: ViewKind::Section { start, end, .. },
        callout: None,
        ..
    })) = doc.model().get(id).map(|e| &e.kind)
    else {
        state.drag = None;
        return None;
    };
    let (start, end) = (*start, *end);
    let handles = [
        (Handle::Start, start),
        (Handle::End, end),
        (Handle::Line, (start + end) / 2.0),
    ];
    let accent = crate::theme::interaction();
    for (h, p) in handles {
        let c = xf.to_screen(p);
        if h == Handle::Line {
            painter.circle(c, 5.0, Color32::WHITE, Stroke::new(1.5, accent));
        } else {
            painter.rect(
                Rect::from_center_size(c, Vec2::splat(9.0)),
                0.0,
                Color32::WHITE,
                Stroke::new(1.5, accent),
                egui::StrokeKind::Middle,
            );
        }
    }
    if response.drag_started_by(egui::PointerButton::Primary) {
        // where the button went down, not where the drag was recognised
        let p = painter
            .ctx()
            .input(|i| i.pointer.press_origin())
            .or(response.interact_pointer_pos())?;
        state.drag = handles
            .iter()
            .find(|(_, q)| xf.to_screen(*q).distance(p) < 10.0)
            .map(|(h, _)| (*h, xf.to_world(p), start, end));
        state.drag_owner = Some(response.id);
    }
    if state.drag.is_some() && state.drag_owner != Some(response.id) {
        return None;
    }
    let (h, grab, s0, e0) = state.drag?;
    if response.drag_stopped() || !response.dragged() {
        state.drag = None;
        return Some(None);
    }
    let p = xf.to_world(response.interact_pointer_pos()?);
    let snap = |v: DVec2| (v / 0.05).round() * 0.05;
    let (s1, e1) = match h {
        Handle::Start => (snap(p), e0),
        Handle::End => (s0, snap(p)),
        Handle::Line => {
            let d = snap(p - grab);
            (s0 + d, e0 + d)
        }
    };
    if s1.distance(e1) < 0.2 {
        return Some(None);
    }
    let r = doc.edit_merged(id, "Move section line", |e| {
        if let ElementKind::View(View {
            kind: ViewKind::Section { start, end, .. },
            ..
        }) = &mut e.kind
        {
            (*start, *end) = (s1, e1);
        }
    });
    Some(r.err().map(|e| e.to_string()))
}

/// This frame's wheel input, split as touchpads expect (MU24, MU25): a
/// two-finger scroll pans, returned as screen points the view follows; a
/// mouse wheel and a pinch zoom, returned as a zoom amount in the units of
/// `smooth_scroll_delta.y`.
///
/// Windows reports a precision touchpad's scroll in lines, as a wheel's,
/// so the unit alone can't tell them apart: a wheel notch is a whole line
/// straight up or down, a touchpad sends fractions of lines and sideways
/// steps. Once a gesture shows itself as a touchpad's, the rest of it
/// (within 0.4 s) pans too.
///
/// A fraction of a line isn't proof of a touchpad: wheels that spin free
/// or step finely send them too, and such a wheel's zoom turned to
/// panning as it rolled (issue #78). So a computer with no touchpad never
/// pans by its wheel, and Settings > Mouse can say what a roll does.
pub(crate) fn wheel(ui: &egui::Ui) -> (egui::Vec2, f32) {
    let mode = WheelMode::now();
    let pad = crate::touchpad::present();
    let id = egui::Id::new("touchpad-gesture");
    let now = ui.input(|i| i.time);
    let last: f64 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(-10.0);
    let mut seen = None;
    let out = ui.input(|i| {
        let mut pan = egui::Vec2::ZERO;
        let mut zoom = 0.0;
        for e in &i.raw.events {
            if let egui::Event::MouseWheel {
                unit,
                delta,
                modifiers,
                ..
            } = e
            {
                if modifiers.ctrl || modifiers.command {
                    // a pinch and Ctrl+wheel are in zoom_delta already
                    continue;
                }
                let touchpad = wheel_pans(mode, pad, *unit, *delta, now - last < 0.4);
                if touchpad {
                    seen = Some(now);
                    pan += match unit {
                        egui::MouseWheelUnit::Point => *delta,
                        _ => *delta * 40.0,
                    };
                } else {
                    zoom += match unit {
                        egui::MouseWheelUnit::Page => delta.y * 400.0,
                        _ => delta.y * 40.0,
                    };
                }
            }
        }
        let pinch = i.zoom_delta();
        if (pinch - 1.0).abs() > 1e-4 {
            zoom += (pinch - 1.0) * 500.0;
        }
        (pan, zoom)
    });
    if let Some(t) = seen {
        if t - last > 0.4 {
            crate::actlog::line("wheel: a touchpad scroll, panning");
        }
        ui.ctx().data_mut(|d| d.insert_temp(id, t));
    }
    out
}

/// What rolling a wheel does in a view (Settings > Mouse).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WheelMode {
    /// A mouse wheel zooms, a touchpad's two fingers pan.
    Auto,
    Zoom,
    Pan,
}

static WHEEL_MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

impl WheelMode {
    pub(crate) fn of(setting: &str) -> Self {
        match setting {
            "zoom" => Self::Zoom,
            "pan" => Self::Pan,
            _ => Self::Auto,
        }
    }

    /// Tell the views what the setting is (once a frame).
    pub(crate) fn set(self) {
        WHEEL_MODE.store(self as u8, std::sync::atomic::Ordering::Relaxed);
    }

    fn now() -> Self {
        match WHEEL_MODE.load(std::sync::atomic::Ordering::Relaxed) {
            1 => Self::Zoom,
            2 => Self::Pan,
            _ => Self::Auto,
        }
    }
}

/// Whether one wheel event pans the view (else it zooms). `pad`: the
/// computer has a touchpad; `in_gesture`: a touchpad scroll was seen a
/// moment ago.
pub(crate) fn wheel_pans(
    mode: WheelMode,
    pad: bool,
    unit: egui::MouseWheelUnit,
    delta: egui::Vec2,
    in_gesture: bool,
) -> bool {
    match mode {
        WheelMode::Zoom => return false,
        WheelMode::Pan => return true,
        WheelMode::Auto => {}
    }
    // no touchpad here: whatever it sends, it is a wheel
    if !pad {
        return false;
    }
    let whole = |v: f32| (v - v.round()).abs() < 1e-3;
    match unit {
        egui::MouseWheelUnit::Point => true,
        egui::MouseWheelUnit::Line => delta.x.abs() > 1e-3 || !whole(delta.y) || in_gesture,
        egui::MouseWheelUnit::Page => false,
    }
}

#[cfg(test)]
mod wheel_tests {
    use super::*;
    use egui::{vec2, MouseWheelUnit::*};

    #[test]
    fn a_fine_wheel_on_a_computer_with_no_touchpad_zooms() {
        // issue #78: fractions of a notch, and the 0.4 s after one
        for y in [1.0, 0.25, 0.125, -0.4] {
            assert!(
                !wheel_pans(WheelMode::Auto, false, Line, vec2(0.0, y), false),
                "{y}"
            );
            assert!(
                !wheel_pans(WheelMode::Auto, false, Line, vec2(0.0, y), true),
                "{y}"
            );
        }
        // even sideways: a tilt wheel isn't two fingers
        assert!(!wheel_pans(
            WheelMode::Auto,
            false,
            Line,
            vec2(0.5, 0.0),
            false
        ));
    }

    #[test]
    fn with_a_touchpad_two_fingers_pan_and_a_whole_notch_zooms() {
        assert!(wheel_pans(
            WheelMode::Auto,
            true,
            Line,
            vec2(0.0, 0.07),
            false
        ));
        assert!(wheel_pans(
            WheelMode::Auto,
            true,
            Line,
            vec2(0.3, 0.0),
            false
        ));
        assert!(wheel_pans(
            WheelMode::Auto,
            true,
            Point,
            vec2(0.0, 12.0),
            false
        ));
        assert!(!wheel_pans(
            WheelMode::Auto,
            true,
            Line,
            vec2(0.0, 1.0),
            false
        ));
        // the rest of a touchpad's gesture pans too
        assert!(wheel_pans(
            WheelMode::Auto,
            true,
            Line,
            vec2(0.0, 1.0),
            true
        ));
    }

    #[test]
    fn the_setting_decides_when_it_is_set() {
        for pad in [false, true] {
            assert!(!wheel_pans(
                WheelMode::Zoom,
                pad,
                Line,
                vec2(0.3, 0.07),
                true
            ));
            assert!(!wheel_pans(
                WheelMode::Zoom,
                pad,
                Point,
                vec2(0.0, 9.0),
                true
            ));
            assert!(wheel_pans(WheelMode::Pan, pad, Line, vec2(0.0, 1.0), false));
        }
        assert_eq!(WheelMode::of("zoom"), WheelMode::Zoom);
        assert_eq!(WheelMode::of("pan"), WheelMode::Pan);
        assert_eq!(WheelMode::of("anything else"), WheelMode::Auto);
    }
}

/// How the zoom column looks this frame, to draw it once what lies
/// under it is drawn (a sheet's paper, MU25).
pub(crate) struct NavLook {
    rect: Rect,
    column: Rect,
    fade: f32,
    buttons: Vec<(Rect, &'static str, bool)>,
}

/// The zoom column's clicks and drags: the camera asked for, and how to
/// draw it with [`nav_draw`].
pub(crate) fn nav_input(
    ui: &egui::Ui,
    rect: Rect,
    view: ElementId,
    fit_to: impl Fn() -> (DVec2, f32),
    fit_sel: Option<(DVec2, f32)>,
    center: DVec2,
    scale: f32,
) -> (Option<(DVec2, f32)>, Option<NavLook>) {
    let size = 28.0;
    let x = rect.right() - size - 10.0;
    let y0 = rect.top() + 10.0;
    // a short area cuts the column off at its edge rather than taking it
    // away (MU40, as Blender's)
    if rect.width() < 200.0 || rect.height() < size + 20.0 {
        return (None, None);
    }
    let column = Rect::from_min_size(
        egui::pos2(x - 3.0, y0 - 3.0),
        egui::vec2(size + 6.0, 5.0 * size + 6.0),
    )
    .intersect(rect);
    let near = ui
        .input(|i| i.pointer.hover_pos())
        .is_some_and(|h| column.expand(40.0).contains(h));
    let fade = if near { 1.0 } else { 0.45 };
    let mut out = None;
    let mut look = NavLook {
        rect,
        column,
        fade,
        buttons: vec![],
    };
    let buttons = [
        ("zoom-in", "Zoom in: click a step, or drag up and down"),
        ("zoom-out", "Zoom out: click a step, or drag up and down"),
        ("frame-all", "Fit the drawing"),
        ("frame-selection", "Frame the selection"),
        ("hand", "Drag to pan"),
    ];
    for (i, (icon, tip)) in buttons.iter().enumerate() {
        let r = Rect::from_min_size(egui::pos2(x, y0 + i as f32 * size), egui::vec2(size, size));
        // one cut off by a short area is not there to click
        if r.bottom() > rect.bottom() {
            continue;
        }
        let resp = ui
            .interact(
                r,
                ui.id().with(("nav2d", view, i)),
                egui::Sense::click_and_drag(),
            )
            .on_hover_text(*tip);
        look.buttons.push((r, icon, resp.hovered()));
        let zoom = |by: f32| {
            // plans count points per metre, sheets per millimetre (MU25)
            let k = (scale * by).clamp(0.05, 5000.0);
            Some((center, k))
        };
        match i {
            // a click steps; a drag up or down zooms smoothly, as the 3D
            // view's zoom button does (MU20)
            0 | 1 if resp.dragged() => {
                let d = resp.drag_delta().y;
                out = zoom((1.0 - d * 0.01).clamp(0.5, 2.0));
            }
            0 if resp.clicked() => out = zoom(1.25),
            1 if resp.clicked() => out = zoom(1.0 / 1.25),
            2 if resp.clicked() => out = Some(fit_to()),
            3 if resp.clicked() => out = fit_sel,
            4 if resp.dragged() => {
                let d = resp.drag_delta() / scale;
                out = Some((center - DVec2::new(d.x as f64, -d.y as f64), scale));
            }
            _ => {}
        }
    }
    (out, Some(look))
}

/// Draw the zoom column [`nav_input`] read.
pub(crate) fn nav_draw(ui: &egui::Ui, look: &NavLook) {
    let p = crate::theme::Palette::of(ui.ctx());
    let fade = look.fade;
    // on the pane's own layer, so windows and pop-ups cover it (MU23);
    // dark paper is told to leave it alone, as it turns the drawing over
    // but never the controls (MU20)
    let painter = ui.painter().clone().with_clip_rect(look.rect);
    let first = painter.add(egui::Shape::Noop);
    painter.rect(
        look.column,
        8.0,
        // all but solid, faded or not: the drawing's lines no longer run
        // over its buttons (MU39); only its border and icons fade
        p.panel.gamma_multiply(0.94),
        egui::Stroke::new(1.0, p.border.gamma_multiply(fade)),
        egui::StrokeKind::Inside,
    );
    for (r, icon, hovered) in &look.buttons {
        if *hovered {
            painter.rect_filled(r.shrink(2.0), 6.0, p.accent.gamma_multiply(0.15));
        }
        crate::svgicon::paint(&painter, r.shrink(6.0), icon, p.text.gamma_multiply(fade));
    }
    let last = painter.add(egui::Shape::Noop);
    keep_colours(ui.ctx(), painter.layer_id(), first, last);
}

/// Shapes `first..=last` on `layer` keep their colours when dark paper
/// turns the drawing over (the zoom column's), this frame.
pub(crate) fn keep_colours(
    ctx: &egui::Context,
    layer: egui::LayerId,
    first: egui::layers::ShapeIdx,
    last: egui::layers::ShapeIdx,
) {
    let id = egui::Id::new("paper.keep");
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let list = d.get_temp_mut_or_default::<(u64, Vec<(egui::LayerId, usize, usize)>)>(id);
        if list.0 != pass {
            *list = (pass, vec![]);
        }
        list.1.push((layer, first.0, last.0));
    });
}

/// The shapes on `layer` dark paper leaves alone this frame.
pub(crate) fn kept_colours(ctx: &egui::Context, layer: egui::LayerId) -> Vec<(usize, usize)> {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| {
        d.get_temp::<(u64, Vec<(egui::LayerId, usize, usize)>)>(egui::Id::new("paper.keep"))
    })
    .filter(|l| l.0 == pass)
    .map(|l| {
        l.1.into_iter()
            .filter(|r| r.0 == layer)
            .map(|r| (r.1, r.2))
            .collect()
    })
    .unwrap_or_default()
}

/// How a crop box's frame is drawn on a plan or section: its width and
/// colour, or nothing when it is neither picked, hovered nor shown.
fn crop_frame_look(selected: bool, hovered: bool, shown: bool) -> Option<(f32, Color32)> {
    if selected {
        Some((1.8, Color32::from_rgb(217, 115, 64)))
    } else if hovered {
        Some((2.5, hoverpick::HOVER))
    } else if shown {
        Some((1.0, Color32::from_rgb(217, 115, 64)))
    } else {
        None
    }
}

#[cfg(test)]
mod overhead_outline_tests {
    use super::*;

    /// How many shapes the plan view paints for `item`.
    fn painted(item: &PlanItem, selected: bool) -> usize {
        let ctx = egui::Context::default();
        let out = ctx.run_ui(Default::default(), |ui| {
            let painter = ui.ctx().layer_painter(egui::LayerId::background());
            let xf = Xf {
                rect: Rect::from_min_size(Pos2::ZERO, Vec2::splat(400.0)),
                center: DVec2::ZERO,
                scale: 100.0,
            };
            draw_item(&painter, &xf, item, selected, 1.0);
        });
        let mut out = out;
        out.textures_delta.clear();
        out.shapes.len()
    }

    /// An outline overhead (a roof window, a ceiling light's symbol) is
    /// dashed in the plan view, selected or not, as sheets draw it (MU141:
    /// it drew solid here).
    #[test]
    fn an_overhead_outline_is_dashed_in_the_plan_view() {
        let square = vec![
            DVec2::new(-1.0, -1.0),
            DVec2::new(1.0, -1.0),
            DVec2::new(1.0, 1.0),
            DVec2::new(-1.0, 1.0),
        ];
        let item = |pen| PlanItem::Polygon {
            id: ElementId::NIL,
            pen,
            points: square.clone(),
        };
        // four edges seen: four strokes
        assert_eq!(painted(&item(Pen::Projection), false), 4);
        // overhead: each 200 px edge in 6 px dashes
        for selected in [false, true] {
            let n = painted(&item(Pen::Hidden), selected);
            assert!(
                n > 40,
                "{n} strokes for a dashed outline, selected {selected}"
            );
        }
    }
}

#[cfg(test)]
mod crop_frame_tests {
    use super::*;

    #[test]
    fn a_crop_box_under_the_pointer_is_lit_in_plans_and_sections() {
        // MU138: hovering one lit it only in the 3D window
        let (w, c) = crop_frame_look(false, true, true).unwrap();
        assert_eq!(c, hoverpick::HOVER);
        assert!(w > 1.0);
        // even one whose frame is hidden: what a click would take shows
        assert_eq!(
            crop_frame_look(false, true, false).map(|l| l.1),
            Some(hoverpick::HOVER)
        );
        assert!(crop_frame_look(false, false, false).is_none());
    }
}

/// Each 3D crop box as a rectangle on `view`'s drawing: its footprint
/// in a plan, its extent across a section.
fn crop_rects(model: &creabim_core::Model, view: ElementId) -> Vec<(ElementId, DVec2, DVec2)> {
    let Some(frame) = creabim_core::DrawingFrame::of(model, view) else {
        return vec![];
    };
    model
        .iter()
        .filter_map(|e| match &e.kind {
            ElementKind::CropBox(c) => {
                let mut lo = DVec2::splat(f64::MAX);
                let mut hi = DVec2::splat(f64::MIN);
                for i in 0..8 {
                    let p = DVec3::new(
                        if i & 1 == 0 { c.min.x } else { c.max.x },
                        if i & 2 == 0 { c.min.y } else { c.max.y },
                        if i & 4 == 0 { c.min.z } else { c.max.z },
                    );
                    let d = frame.to_drawing(p);
                    lo = lo.min(d);
                    hi = hi.max(d);
                }
                Some((e.id, lo, hi))
            }
            _ => None,
        })
        .collect()
}

fn fit(d: &Drawing, rect: Rect) -> (DVec2, f32) {
    match d.bounds() {
        Some((lo, hi)) => {
            let size = (hi - lo).max(DVec2::splat(1.0));
            let scale = (rect.width() as f64 / size.x).min(rect.height() as f64 / size.y) * 0.85;
            ((lo + hi) / 2.0, scale.clamp(2.0, 2000.0) as f32)
        }
        None => (DVec2::ZERO, 45.0),
    }
}

/// A, B, … Z, AA, AB, …
fn section_letter(n: usize) -> String {
    let mut n = n;
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        if n < 26 {
            break s;
        }
        n = n / 26 - 1;
    }
}

static PLAN_GRID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
static PLAN_AXES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// The settings' grid behind drawings and X/Y lines in plans, for this
/// frame.
pub fn set_plan_look(grid: bool, axes: bool) {
    use std::sync::atomic::Ordering::Relaxed;
    PLAN_GRID.store(grid, Relaxed);
    PLAN_AXES.store(axes, Relaxed);
}

fn plan_look() -> (bool, bool) {
    use std::sync::atomic::Ordering::Relaxed;
    (PLAN_GRID.load(Relaxed), PLAN_AXES.load(Relaxed))
}

/// The model's X axis (red) and Y axis (green) through the origin, toned
/// well down so they place the building without competing with it.
fn draw_origin_axes(painter: &egui::Painter, xf: &Xf) {
    let r = xf.rect;
    let o = xf.to_screen(DVec2::ZERO);
    if (r.top()..=r.bottom()).contains(&o.y) {
        painter.line_segment(
            [egui::pos2(r.left(), o.y), egui::pos2(r.right(), o.y)],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(214, 72, 72, 70)),
        );
    }
    if (r.left()..=r.right()).contains(&o.x) {
        painter.line_segment(
            [egui::pos2(o.x, r.top()), egui::pos2(o.x, r.bottom())],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(70, 160, 80, 70)),
        );
    }
}

fn draw_metric_grid(painter: &egui::Painter, xf: &Xf) {
    let step = if xf.scale > 30.0 {
        1.0
    } else if xf.scale > 6.0 {
        5.0
    } else {
        25.0
    };
    let lo = xf.to_world(xf.rect.left_bottom());
    let hi = xf.to_world(xf.rect.right_top());
    let stroke = Stroke::new(1.0, Color32::from_gray(236));
    let mut x = (lo.x / step).floor() * step;
    while x <= hi.x {
        painter.line_segment(
            [
                xf.to_screen(DVec2::new(x, lo.y)),
                xf.to_screen(DVec2::new(x, hi.y)),
            ],
            stroke,
        );
        x += step;
    }
    let mut y = (lo.y / step).floor() * step;
    while y <= hi.y {
        painter.line_segment(
            [
                xf.to_screen(DVec2::new(lo.x, y)),
                xf.to_screen(DVec2::new(hi.x, y)),
            ],
            stroke,
        );
        y += step;
    }
}

/// Draw one drawing item. `mm` is drawing units per paper millimetre, which
/// sizes hatching and annotations.
/// A view's graphic overrides for one element, as the working view draws
/// them (MU27: they only showed on sheets and in PDF).
#[derive(Clone, Copy, Debug)]
pub struct Ovr {
    /// 0 drawn fully, 1 gone: halftone and transparency.
    pub fade: f32,
    pub colour: Option<Color32>,
    pub hatch_off: bool,
    /// Projection and cut pens, as factors of the view's own.
    pub proj: f32,
    pub cut: f32,
    pub dashed: bool,
}

impl Default for Ovr {
    fn default() -> Self {
        Self {
            fade: 0.0,
            colour: None,
            hatch_off: false,
            proj: 1.0,
            cut: 1.0,
            dashed: false,
        }
    }
}

/// The overrides view `view` sets, element by element: its categories'
/// first, then its own elements'. Empty when it sets none.
/// The grid colour a plan draws in on its own (a grid system's Auto).
/// A grid bubble's size on screen from its true size: annotation text's
/// own rule (annotate::text_px), so the two read the same while zooming:
/// never under 9 px, but never over twice its true size (MU69).
pub fn bubble_px(true_px: f32) -> f32 {
    true_px.max(9.0_f32.min(true_px * 2.0))
}

pub const GRID_INK: Color32 = Color32::from_rgb(120, 140, 170);

/// A grid line's colour as its system sets it (MU67); None for Auto.
pub fn grid_colour(model: &creabim_core::Model, g: &creabim_core::Grid) -> Option<Color32> {
    use creabim_core::grids::GridColour;
    let (_, s) = creabim_core::grids::group_of(model, g)?;
    match s.colour_mode {
        GridColour::Auto => None,
        GridColour::Annotation => Some(
            creabim_geom::annotate::look_of(model, None, None)
                .rgb
                .map_or(crate::annotate::INK, crate::annotate::color32),
        ),
        GridColour::Custom => Some(Color32::from_rgb(
            (s.color[0].clamp(0.0, 1.0) * 255.0) as u8,
            (s.color[1].clamp(0.0, 1.0) * 255.0) as u8,
            (s.color[2].clamp(0.0, 1.0) * 255.0) as u8,
        )),
    }
}

pub fn view_overrides(model: &creabim_core::Model, view: ElementId) -> HashMap<ElementId, Ovr> {
    use creabim_core::styles::Target;
    let mut out = HashMap::new();
    let Some(ElementKind::View(v)) = model.get(view).map(|e| &e.kind) else {
        return out;
    };
    // how each element reads from the phase the view stands at: what was
    // there before fainter and unhatched, what comes down in red dashes,
    // as on its sheets (phases)
    use creabim_core::phases::Status;
    let mut phase = creabim_core::phases::statuses(
        model,
        crate::phaselens::lensed(view, v.phase),
        &v.options,
        &v.ghost,
    );
    let phase_look = creabim_core::phases::look(model);
    phase.retain(|_, s| match s {
        Status::Existing => phase_look.existing_faint,
        Status::TakenDown => true,
        Status::New => phase_look.new.is_some(),
        Status::Ghost => true,
        Status::Absent => false,
    });
    let rgb = |c: [f32; 3]| {
        Color32::from_rgb(
            (c[0] * 255.0) as u8,
            (c[1] * 255.0) as u8,
            (c[2] * 255.0) as u8,
        )
    };
    if v.look.overrides.is_empty() && phase.is_empty() {
        return out;
    }
    let pens = creabim_core::styles::view_pens(model, v);
    let w = pens.weights;
    let cutters = creabim_core::visibility::cutters(model);
    for e in model.iter() {
        let Some(cat) = creabim_core::visibility::Category::of(e, &cutters) else {
            continue;
        };
        let mut list: Vec<&creabim_core::styles::Override> = v
            .look
            .overrides
            .iter()
            .filter(|o| match o.target {
                Target::Category(c) => c == cat,
                Target::Element(id) => id == e.id,
            })
            .collect();
        let reads = phase.get(&e.id).copied();
        if list.is_empty() && reads.is_none() {
            continue;
        }
        list.sort_by_key(|o| matches!(o.target, Target::Element(_)));
        let mut r = Ovr::default();
        for o in list {
            if let Some(x) = o.weight {
                r.proj = (pens.mm(x) / w.projection.max(1e-6)) as f32;
            }
            if let Some(x) = o.cut_weight {
                r.cut = (pens.mm(x) / w.cut.max(1e-6)) as f32;
            }
            if let Some(c) = o.colour {
                let [a, b, d] = c.0;
                r.colour = Some(Color32::from_rgb(
                    (a * 255.0) as u8,
                    (b * 255.0) as u8,
                    (d * 255.0) as u8,
                ));
            }
            if let Some(pat) = &o.pattern {
                r.dashed = !matches!(pat, creabim_core::styles::LinePattern::Solid);
            }
            if o.halftone {
                r.fade = 1.0 - (1.0 - r.fade) * 0.5;
            }
            if let Some(t) = o.transparency {
                r.fade = 1.0 - (1.0 - r.fade) * (1.0 - t);
            }
            if let Some(h) = o.hatch {
                r.hatch_off = !h;
            }
        }
        match reads {
            Some(Status::Existing) => {
                r.fade = 1.0 - (1.0 - r.fade) * 0.5;
                r.hatch_off = true;
            }
            Some(Status::TakenDown) => {
                r.colour = Some(rgb(phase_look.down));
                r.dashed = phase_look.down_dashed;
                r.hatch_off = true;
            }
            Some(Status::New) => r.colour = phase_look.new.map(rgb).or(r.colour),
            Some(Status::Ghost) => {
                r.colour = Some(rgb(creabim_geom::looks::GHOST_BLUE));
                r.dashed = true;
                r.fade = 1.0 - (1.0 - r.fade) * 0.65;
                r.hatch_off = true;
            }
            _ => {}
        }
        r.proj = r.proj.clamp(0.2, 6.0);
        r.cut = r.cut.clamp(0.2, 6.0);
        out.insert(e.id, r);
    }
    out
}

type SharedLooks = Arc<creabim_geom::looks::Looks>;

/// The view's looks (styles, patterns), kept while the model doesn't
/// change; none when the project has no patterns of its own, so plain
/// projects draw as before.
fn view_looks(
    model: &creabim_core::Model,
    view: ElementId,
) -> Option<Arc<creabim_geom::looks::Looks>> {
    thread_local! {
        static CACHE: std::cell::RefCell<HashMap<ElementId, (u64, Option<SharedLooks>)>> =
            std::cell::RefCell::new(HashMap::new());
    }
    let rev = model.revision();
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((r, l)) = c.get(&view) {
            if *r == rev {
                return l.clone();
            }
        }
        let l = match model.get(view).map(|e| &e.kind) {
            Some(ElementKind::View(v)) => {
                let _p = crate::app::Perf::new("view looks: build");
                let l = creabim_geom::looks::Looks::for_view(model, view, v);
                // the scale table patterns every cut piece, own patterns or not
                (l.has_own_patterns() || l.rule.is_some()).then(|| Arc::new(l))
            }
            _ => None,
        };
        c.insert(view, (rev, l.clone()));
        l
    })
}

/// The patterned surfaces among `items` whose pattern the zoom (`px`
/// pixels a metre) would close up, each with the lines to paint in place
/// of its drawing's own.
fn screen_patterns(
    model: &creabim_core::Model,
    view: ElementId,
    items: &[PlanItem],
    px: f64,
) -> HashMap<ElementId, Vec<PlanItem>> {
    let scale = match model.get(view).map(|e| &e.kind) {
        Some(ElementKind::View(v)) => v.scale,
        _ => 100.0,
    };
    let mut out = HashMap::new();
    for item in items {
        let PlanItem::Line {
            id,
            pen: Pen::Pattern,
            ..
        } = item
        else {
            continue;
        };
        if out.contains_key(id) {
            continue;
        }
        let Some(ElementKind::Site(creabim_core::Site {
            form:
                creabim_core::SiteForm::Area {
                    boundary,
                    holes,
                    pattern,
                    ..
                },
            ..
        })) = model.get(*id).map(|e| &e.kind)
        else {
            continue;
        };
        let mut loops = vec![boundary.clone()];
        loops.extend(holes.iter().cloned());
        let drawn = creabim_geom::parts::pattern_at_scale(pattern, scale);
        if let Some(lines) = creabim_geom::parts::pattern_on_screen(&drawn, &loops, px) {
            let lines = lines
                .into_iter()
                .map(|(a, b)| PlanItem::Line {
                    id: *id,
                    pen: Pen::Pattern,
                    a,
                    b,
                })
                .collect();
            out.insert(*id, lines);
        }
    }
    out
}

/// A pattern of the project's own that `item` draws with instead of the
/// built-in hatch, at the view's scale (`mm` model units a paper mm).
fn own_pattern(
    looks: &creabim_geom::looks::Looks,
    item: &PlanItem,
    mm: f64,
) -> Option<creabim_core::styles::HatchPattern> {
    let k = 1.0 / mm.max(1e-9);
    let p = match item {
        PlanItem::Fill { id, hatch, rgb, .. } => {
            let (_, p) = looks.cut_pattern_of(*id, *hatch, *rgb);
            p.filter(|p| *p != creabim_core::styles::builtin_pattern(*hatch))?
        }
        PlanItem::Shade { id, .. } => looks.surface_pattern(*id)?,
        _ => return None,
    };
    Some(looks.at_scale(p, k))
}

fn item_points(item: &PlanItem) -> Option<&[DVec2]> {
    match item {
        PlanItem::Fill { points, .. } | PlanItem::Shade { points, .. } => Some(points),
        _ => None,
    }
}

/// Pattern `p`'s lines (and holes) over `points`, as the sheets draw it.
fn draw_pattern(
    painter: &egui::Painter,
    xf: &Xf,
    p: &creabim_core::styles::HatchPattern,
    points: &[DVec2],
    mm: f64,
    colour: Option<Color32>,
) {
    let k = 1.0 / mm.max(1e-9);
    let ink = colour.unwrap_or(Color32::from_gray(70));
    if p.recipe.is_some() {
        if let Some(l) = creabim_geom::looks::recipe_layout(p, points, k) {
            for (pts, job) in &l.shapes {
                if *job == creabim_core::pattern::Job::Hole && pts.len() >= 3 {
                    fill_polygon(painter, xf, pts, ink.gamma_multiply(0.6));
                }
            }
        }
    }
    let stroke = Stroke::new(0.6, ink);
    for (a, b) in creabim_geom::looks::pattern_lines(p, points, k) {
        painter.line_segment([xf.to_screen(a), xf.to_screen(b)], stroke);
    }
}

thread_local! {
    /// The line style of the view being painted, while its items paint.
    static SCREEN_HAND: std::cell::RefCell<Option<Arc<creabim_core::linestyle::LineStyle>>> =
        const { std::cell::RefCell::new(None) };
}

/// Holds a view's line style for the items painted while it lives.
struct ScreenHand;

impl ScreenHand {
    fn set(s: Option<creabim_core::linestyle::LineStyle>) -> ScreenHand {
        SCREEN_HAND.with(|h| *h.borrow_mut() = s.map(Arc::new));
        ScreenHand
    }
    fn get() -> Option<Arc<creabim_core::linestyle::LineStyle>> {
        SCREEN_HAND.with(|h| h.borrow().clone())
    }
}

impl Drop for ScreenHand {
    fn drop(&mut self) {
        SCREEN_HAND.with(|h| *h.borrow_mut() = None);
    }
}

/// A polyline (model units) drawn by hand on screen: the engine works in
/// paper mm (`mm` model units each) with the pen's paper width `paper_w`;
/// its strokes come back as short segments `px` wide for that width.
#[allow(clippy::too_many_arguments)]
fn draw_hand(
    painter: &egui::Painter,
    xf: &Xf,
    pts: &[DVec2],
    paper_w: f64,
    px: f32,
    colour: Color32,
    mm: f64,
    style: &creabim_core::linestyle::LineStyle,
) {
    use creabim_geom::handline::Mark;
    let mm = mm.max(1e-9);
    let paper: Vec<DVec2> = pts.iter().map(|p| *p / mm).collect();
    let k = px as f64 / paper_w;
    let to = |p: DVec2| xf.to_screen(p * mm);
    for m in creabim_geom::handline::hand_marks(&paper, paper_w, style) {
        match m {
            Mark::Ribbon { pts, widths } => {
                for i in 0..pts.len().saturating_sub(1) {
                    let w = ((widths[i] + widths[i + 1]) / 2.0 * k) as f32;
                    painter.line_segment(
                        [to(pts[i]), to(pts[i + 1])],
                        Stroke::new(w.max(0.5), colour),
                    );
                }
            }
            Mark::Dot { at, r } => {
                painter.circle_filled(to(at), ((r * k) as f32).max(0.4), colour);
            }
            Mark::Hook(t) => {
                painter.add(egui::Shape::convex_polygon(
                    t.iter().map(|p| to(*p)).collect(),
                    colour,
                    Stroke::NONE,
                ));
            }
        }
    }
}

/// A drawing's text as the plan pane paints it: the one way, for the
/// drawing and for the highlight over it, so the two never part.
#[allow(clippy::too_many_arguments)]
fn plan_text(
    painter: &egui::Painter,
    xf: &Xf,
    at: DVec2,
    text: &str,
    size: f64,
    align: paper::Anchor,
    bold: bool,
    angle: f64,
    colour: Color32,
    mm: f64,
) {
    let px = crate::annotate::snap_px((size * mm * xf.scale as f64) as f32);
    if px < 4.0 {
        return;
    }
    if angle != 0.0 {
        // turned about its anchor: the plan's y runs up, the screen's down
        let at = xf.to_screen(at);
        let font = egui::FontId::proportional(px);
        let g = painter.layout_no_wrap(text.to_string(), font, colour);
        let (w, h) = (g.size().x, g.size().y);
        let dx = match align {
            paper::Anchor::Start => 0.0,
            paper::Anchor::Middle => -w / 2.0,
            paper::Anchor::End => -w,
        };
        let a = -angle as f32;
        let (sa, ca) = a.sin_cos();
        let off = Vec2::new(dx * ca - (-h / 2.0) * sa, dx * sa + (-h / 2.0) * ca);
        let passes = if bold { 2 } else { 1 };
        for k in 0..passes {
            let nudge = Vec2::new(ca, sa) * (k as f32 * px / 28.0);
            painter.add(
                egui::epaint::TextShape::new(at + off + nudge, g.clone(), colour).with_angle(a),
            );
        }
        return;
    }
    let align = match align {
        paper::Anchor::Start => egui::Align2::LEFT_CENTER,
        paper::Anchor::Middle => egui::Align2::CENTER_CENTER,
        paper::Anchor::End => egui::Align2::RIGHT_CENTER,
    };
    let at = xf.to_screen(at);
    let font = egui::FontId::proportional(px);
    painter.text(at, align, text, font.clone(), colour);
    if bold {
        // a second pass a hair to the right reads as semibold
        painter.text(at + Vec2::new(px / 28.0, 0.0), align, text, font, colour);
    }
}

pub fn draw_item(painter: &egui::Painter, xf: &Xf, item: &PlanItem, selected: bool, mm: f64) {
    draw_item_with(painter, xf, item, selected, mm, &Ovr::default());
}

/// [`draw_item`] with a view's overrides for the element.
pub fn draw_item_with(
    painter: &egui::Painter,
    xf: &Xf,
    item: &PlanItem,
    selected: bool,
    mm: f64,
    o: &Ovr,
) {
    let highlight = crate::theme::interaction();
    let ink = if selected {
        highlight
    } else {
        o.colour.unwrap_or(Color32::BLACK)
    };
    let grey = |g: u8| o.colour.unwrap_or(Color32::from_gray(g));
    let hatch = paper::HATCH_SPACING * mm;
    // annotation text is a fixed size on paper, so it scales with zoom
    let text_px = |size: f64| crate::annotate::snap_px((size * mm * xf.scale as f64) as f32);
    match item {
        PlanItem::Fill {
            points,
            hatch: pattern,
            rgb,
            ..
        } => {
            let gray = (paper::hatch_fill(*pattern) * 255.0) as u8;
            let own = match rgb {
                // Colour and hatch: the material's colour under it (MU32)
                Some([r, g, b]) => {
                    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
                }
                None => Color32::from_gray(gray),
            };
            // selected: its own fill washed toward the selection's colour,
            // so its hatch, grips and dimensions still read (MU75: a solid
            // light orange hid them)
            fill_polygon(painter, xf, points, own);
            if selected {
                fill_polygon(painter, xf, points, selected_wash());
            }
            // skip hatching once it would be too dense to read, or when
            // the view turns it off
            if hatch * xf.scale as f64 > 3.0 && !o.hatch_off {
                let stroke = Stroke::new(0.6, grey(70));
                let hand = ScreenHand::get();
                for (a, b) in creabim_geom::plan::hatch_lines(*pattern, points, hatch) {
                    match &hand {
                        // laid by hand: turned, shifted, now and then broken
                        Some(s) => {
                            let m = mm.max(1e-9);
                            for [p, q] in creabim_geom::handline::hatch_by_hand(a / m, b / m, s) {
                                painter.line_segment(
                                    [xf.to_screen(p * m), xf.to_screen(q * m)],
                                    stroke,
                                );
                            }
                        }
                        None => {
                            painter.line_segment([xf.to_screen(a), xf.to_screen(b)], stroke);
                        }
                    }
                }
            }
        }
        PlanItem::Polygon { pen, points, .. } => {
            let pts: Vec<Pos2> = points.iter().map(|p| xf.to_screen(*p)).collect();
            match pen {
                Pen::Cut => {
                    let fill = Color32::from_gray(150);
                    // any outline, a curved wall's too (MU40): filled as a
                    // mesh, then its edge
                    fill_polygon(painter, xf, points, fill);
                    if selected {
                        fill_polygon(painter, xf, points, selected_wash());
                    }
                    match ScreenHand::get() {
                        Some(s) => {
                            let mut ring = points.clone();
                            ring.extend(points.first().copied());
                            draw_hand(painter, xf, &ring, 0.5, 1.6 * o.cut, ink, mm, &s);
                        }
                        None => {
                            ring_edges(painter, &pts, Stroke::new(1.6 * o.cut, ink));
                        }
                    }
                }
                // overhead, dashed as an overhead line is: a roof window, a
                // ceiling light's symbol (MU141: they drew solid here,
                // though sheets dashed them)
                Pen::Hidden => {
                    let (w, c) = if selected {
                        (2.0, highlight)
                    } else {
                        (1.0, Color32::from_gray(110))
                    };
                    painter.extend(dashed_ring(&pts, Stroke::new(w, c)));
                }
                _ => {
                    let (w, c) = if selected {
                        (2.0, highlight)
                    } else {
                        (1.0 * o.proj, grey(120))
                    };
                    match ScreenHand::get() {
                        Some(s) => {
                            let mut ring = points.clone();
                            ring.extend(points.first().copied());
                            draw_hand(painter, xf, &ring, 0.25, w, c, mm, &s);
                        }
                        None => {
                            ring_edges(painter, &pts, Stroke::new(w, c));
                        }
                    }
                }
            }
        }
        PlanItem::Line { pen, a, b, .. } => {
            let (pa, pb) = (a, b);
            let (a, b) = (xf.to_screen(*a), xf.to_screen(*b));
            match pen {
                Pen::Guide => {
                    if let Some((a, b)) = clip_to_rect(a, b, painter.clip_rect()) {
                        let (w, c) = if selected {
                            (2.0, highlight)
                        } else {
                            (1.0, GUIDE_COLOR)
                        };
                        painter.extend(egui::Shape::dashed_line(
                            &[a, b],
                            Stroke::new(w, c),
                            6.0,
                            4.0,
                        ));
                    }
                }
                Pen::Grid => {
                    let (w, c) = if selected {
                        (2.0, highlight)
                    } else {
                        (1.0, o.colour.unwrap_or(GRID_INK))
                    };
                    painter.extend(egui::Shape::dashed_line(
                        &[a, b],
                        Stroke::new(w, c),
                        12.0,
                        4.0,
                    ));
                }
                Pen::Underlay => {
                    painter.line_segment([a, b], Stroke::new(1.0, UNDERLAY_COLOR));
                }
                Pen::Projection => {
                    let (w, c) = if selected {
                        (2.0, highlight)
                    } else {
                        (1.0 * o.proj, grey(120))
                    };
                    if o.dashed {
                        painter.extend(egui::Shape::dashed_line(
                            &[a, b],
                            Stroke::new(w, c),
                            6.0,
                            4.0,
                        ));
                    } else if let Some(s) = ScreenHand::get() {
                        draw_hand(painter, xf, &[*pa, *pb], 0.25, w, c, mm, &s);
                    } else {
                        painter.line_segment([a, b], Stroke::new(w, c));
                    }
                }
                Pen::Cut => {
                    if o.dashed {
                        painter.extend(egui::Shape::dashed_line(
                            &[a, b],
                            Stroke::new(1.8 * o.cut, ink),
                            6.0,
                            4.0,
                        ));
                    } else if let Some(s) = ScreenHand::get() {
                        draw_hand(painter, xf, &[*pa, *pb], 0.5, 1.8 * o.cut, ink, mm, &s);
                    } else {
                        painter.line_segment([a, b], Stroke::new(1.8 * o.cut, ink));
                    }
                }
                Pen::Hidden => {
                    let (w, c) = if selected {
                        (2.0, highlight)
                    } else {
                        (1.0, Color32::from_gray(110))
                    };
                    painter.extend(egui::Shape::dashed_line(
                        &[a, b],
                        Stroke::new(w, c),
                        6.0,
                        4.0,
                    ));
                }
                // a property line: heavy, a long dash and two short ones
                Pen::Boundary => {
                    let (w, c) = if selected {
                        (2.6, highlight)
                    } else {
                        (2.0 * o.proj, grey(25))
                    };
                    painter.extend(egui::Shape::dashed_line_with_offset(
                        &[a, b],
                        Stroke::new(w, c),
                        &[22.0, 4.0, 4.0],
                        &[4.0, 4.0, 4.0],
                        0.0,
                    ));
                }
                // a setback line: light, a dash and a dot
                Pen::Setback => {
                    let (w, c) = if selected {
                        (1.6, highlight)
                    } else {
                        (1.1 * o.proj, grey(70))
                    };
                    painter.extend(egui::Shape::dashed_line_with_offset(
                        &[a, b],
                        Stroke::new(w, c),
                        &[12.0, 3.0],
                        &[3.0, 3.0],
                        0.0,
                    ));
                }
                // a shape that adds to or cuts others: dashed red (MU71)
                Pen::Cutter => {
                    let (w, c) = if selected {
                        (2.0, highlight)
                    } else {
                        (1.3, Color32::from_rgb(214, 64, 52))
                    };
                    painter.extend(egui::Shape::dashed_line(
                        &[a, b],
                        Stroke::new(w, c),
                        6.0,
                        4.0,
                    ));
                }
                // the ground as it was: light dashes
                Pen::Existing => {
                    let (w, c) = if selected {
                        (1.2, highlight)
                    } else {
                        (0.9 * o.proj, grey(140))
                    };
                    painter.extend(egui::Shape::dashed_line(
                        &[a, b],
                        Stroke::new(w, c),
                        5.0,
                        4.0,
                    ));
                }
                // contours: light, every so many one heavier
                Pen::Contour | Pen::ContourHeavy => {
                    let heavy = *pen == Pen::ContourHeavy;
                    let (w, c) = match (selected, heavy) {
                        (true, true) => (2.2, highlight),
                        (true, false) => (1.2, highlight),
                        (false, true) => (1.7 * o.proj, grey(60)),
                        (false, false) => (0.8 * o.proj, grey(135)),
                    };
                    painter.line_segment([a, b], Stroke::new(w, c));
                }
                _ => {
                    painter.line_segment([a, b], Stroke::new(1.0, ink));
                }
            }
        }
        PlanItem::Arc {
            center,
            radius,
            start,
            end,
            pen,
            ..
        } => {
            let ink = if *pen == Pen::Underlay {
                UNDERLAY_COLOR
            } else {
                ink
            };
            let n = 48;
            let pts: Vec<Pos2> = (0..=n)
                .map(|i| {
                    let t = start + (end - start) * i as f64 / n as f64;
                    xf.to_screen(*center + DVec2::from_angle(t) * *radius)
                })
                .collect();
            if *pen == Pen::Grid {
                // an arc grid line reads as the straight ones (MU66: it was
                // a plain line in another colour); sheets already dash it
                let (w, c) = if selected {
                    (2.0, highlight)
                } else {
                    (1.0, o.colour.unwrap_or(GRID_INK))
                };
                painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(w, c), 12.0, 4.0));
            } else if *pen == Pen::Hidden {
                // dashed as a hidden line is: a window's swing (MU105: its
                // leaf line was dashed, its arc solid; sheets were right)
                let (w, c) = if selected {
                    (2.0, highlight)
                } else {
                    (1.0, Color32::from_gray(110))
                };
                painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(w, c), 6.0, 4.0));
            } else {
                painter.add(egui::Shape::line(pts, Stroke::new(1.0, ink)));
            }
        }
        // a mark drawn with its kind's CreaBIM symbol (Mockup_Symbols_REV00)
        PlanItem::Symbol {
            symbol,
            at,
            text,
            unit,
            angle,
            origin,
            size,
            ..
        } => {
            // a grid's in its system's colour, as its line; the rest in ink
            let grid = symbol.id.starts_with("grids.");
            let c = if selected {
                highlight
            } else if grid {
                o.colour.unwrap_or(GRID_INK)
            } else {
                ink
            };
            let sh = creabim_geom::symbol::shapes_at(
                symbol,
                *at,
                glam::DVec2::splat(*unit),
                *angle,
                *origin,
            );
            for f in &sh.fills {
                fill_polygon(painter, xf, f, c);
            }
            for (l, w) in &sh.lines {
                let pts: Vec<Pos2> = l.iter().map(|p| xf.to_screen(*p)).collect();
                painter.add(egui::Shape::line(pts, Stroke::new((w / 0.18) as f32, c)));
            }
            let px = if grid {
                bubble_px(text_px(*size))
            } else {
                text_px(*size)
            };
            // text only where the symbol has a slot for it (MU84)
            if px >= 4.0 && !text.is_empty() && symbol.text.is_some() {
                painter.text(
                    xf.to_screen(sh.text_at),
                    egui::Align2::CENTER_CENTER,
                    text,
                    egui::FontId::proportional(px),
                    c,
                );
            }
        }
        PlanItem::Label { at, text, .. } => {
            let p = xf.to_screen(*at);
            // a grid's bubble in its system's colour, as its line (MU67)
            let blue = o.colour.unwrap_or(GRID_INK);
            // short tags (grids, section marks) sit in a bubble; longer
            // labels such as level datums are plain text
            // at their paper size, as on sheets: they grow and shrink with
            // the drawing, as text does (MU68: a fixed size on screen
            // ballooned when zoomed out)
            // never below readable: a dot can't be read (first look)
            let px = bubble_px(text_px(2.5));
            if creabim_geom::plan::is_bubble(text) {
                let r = px * creabim_geom::plan::bubble_scale(text) as f32;
                painter.circle(p, r, Color32::WHITE, Stroke::new(1.0, blue));
            }
            if px >= 4.0 {
                painter.text(
                    p,
                    egui::Align2::CENTER_CENTER,
                    text,
                    egui::FontId::proportional(px),
                    blue,
                );
            }
        }
        PlanItem::Text {
            at,
            text,
            size,
            align,
            rgb,
            bold,
            angle,
            ..
        } => {
            let colour = match rgb {
                Some([r, g, b]) if !selected => {
                    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
                }
                _ => ink,
            };
            plan_text(
                painter, xf, *at, text, *size, *align, *bold, *angle, colour, mm,
            );
        }
        PlanItem::Dimension {
            a,
            b,
            offset,
            text,
            look,
            witness,
            breaks,
            label,
            ..
        } => {
            let ink = crate::annotate::ink(look, selected, highlight);
            let d = (*a, *b, *offset, *witness);
            draw_dimension_weighted(
                painter, xf, d, breaks, text, look, mm, ink, 1.0, label, selected,
            );
        }
        PlanItem::Annotation { item, .. } => {
            let ink = crate::annotate::ink(&item.look, selected, highlight);
            crate::annotate::paint(painter, xf, item, ink, mm);
        }
        PlanItem::Tag {
            anchor,
            at,
            elbow,
            text,
            round,
            look,
            ..
        } => {
            let g = tag_geometry(*anchor, *at, *elbow, text, *round, look, mm);
            let pts: Vec<Pos2> = g.outline.iter().map(|p| xf.to_screen(*p)).collect();
            let ink = crate::annotate::ink(look, selected, highlight);
            if !g.leader.is_empty() {
                let line: Vec<Pos2> = g.leader.iter().map(|p| xf.to_screen(*p)).collect();
                painter.add(egui::Shape::line(line, Stroke::new(1.0, ink)));
                crate::annotate::paint_ends(painter, xf, &g.ends, ink);
            }
            if g.closed {
                painter.add(egui::Shape::convex_polygon(
                    pts,
                    Color32::WHITE,
                    Stroke::new(1.2, ink),
                ));
            } else if pts.len() > 1 {
                painter.add(egui::Shape::line(pts, Stroke::new(1.2, ink)));
            }
            crate::annotate::paint_ends(painter, xf, &g.symbol, ink);
            let px = crate::annotate::text_px(look, mm, xf);
            crate::annotate::text(
                painter,
                xf.to_screen(*at),
                text,
                px,
                ink,
                0.0,
                paper::Anchor::Middle,
                look,
            );
        }
        PlanItem::Tint { points, rgb, .. } => {
            let a = (paper::TINT_ALPHA * 255.0) as u8;
            fill_polygon(
                painter,
                xf,
                points,
                crate::annotate::color32(*rgb).gamma_multiply_u8(a),
            );
            if selected {
                fill_polygon(painter, xf, points, selected_wash());
            }
        }
        // a room shows only in its view's colours, and when it is selected
        PlanItem::Room { points, rgb, .. } => {
            if let Some(rgb) = rgb {
                let a = (paper::TINT_ALPHA * 255.0) as u8;
                let fill = crate::annotate::color32(*rgb).gamma_multiply_u8(a);
                fill_polygon(painter, xf, points, fill);
            }
            if selected {
                // the theme's selection colour, faint, and its edge (MU75:
                // a fixed orange)
                let wash = accent_selected().gamma_multiply(0.18);
                fill_polygon(painter, xf, points, wash);
                let pts: Vec<Pos2> = points.iter().map(|p| xf.to_screen(*p)).collect();
                ring_edges(painter, &pts, Stroke::new(1.5, accent_selected()));
            }
        }
        PlanItem::Shade { points, rgb, .. } => {
            fill_polygon(painter, xf, points, crate::annotate::color32(*rgb));
            if selected {
                fill_polygon(painter, xf, points, selected_wash());
            }
        }
        PlanItem::Image {
            raster,
            lo,
            hi,
            clip,
            ..
        } => {
            let rect = Rect::from_two_pos(xf.to_screen(*lo), xf.to_screen(*hi));
            let clip = clip.map(|(a, b)| Rect::from_two_pos(xf.to_screen(a), xf.to_screen(b)));
            crate::textures::paint(painter, rect, clip, raster);
        }
    }
}

/// An item drawn in X-ray: what holds others (walls, slabs, roofs,
/// masses) faint and see-through, and what is hidden inside them drawn
/// solid instead of dashed, as if nothing were in front.
fn draw_item_xray(
    painter: &egui::Painter,
    xf: &Xf,
    item: &PlanItem,
    selected: bool,
    mm: f64,
    model: &creabim_core::Model,
) {
    let host = || model.get(item.id()).is_some_and(|e| is_xray_host(&e.kind));
    match item {
        PlanItem::Fill { points, .. } if !selected && host() => {
            fill_polygon(
                painter,
                xf,
                points,
                Color32::from_gray(150).gamma_multiply(0.18),
            );
        }
        PlanItem::Polygon {
            pen: Pen::Cut,
            points,
            ..
        } if !selected && host() => {
            // filled as it is: a curved wall's band is not convex
            fill_polygon(
                painter,
                xf,
                points,
                Color32::from_gray(150).gamma_multiply(0.18),
            );
            let pts: Vec<Pos2> = points.iter().map(|p| xf.to_screen(*p)).collect();
            ring_edges(painter, &pts, Stroke::new(1.2, Color32::from_gray(90)));
        }
        PlanItem::Line {
            pen: Pen::Cut,
            a,
            b,
            ..
        } if !selected && host() => {
            painter.line_segment(
                [xf.to_screen(*a), xf.to_screen(*b)],
                Stroke::new(1.2, Color32::from_gray(90)),
            );
        }
        PlanItem::Line {
            pen: Pen::Hidden,
            a,
            b,
            ..
        } => {
            let c = if selected {
                crate::theme::interaction()
            } else {
                Color32::from_gray(25)
            };
            painter.line_segment([xf.to_screen(*a), xf.to_screen(*b)], Stroke::new(1.5, c));
        }
        _ => draw_item(painter, xf, item, selected, mm),
    }
}

/// Footings over a plan of `level`, dashed with a light tint in the
/// accent: all of the level's while the Footing tool is on (`working`),
/// and any selected one at all times, even below the view's range.
pub fn draw_footings(
    painter: &egui::Painter,
    xf: &Xf,
    model: &creabim_core::Model,
    level: ElementId,
    working: bool,
    selection: &Selection,
) {
    let accent = accent_selected();
    let strips = creabim_core::footing::strip_outlines(model);
    for e in model.iter() {
        let ElementKind::Footing(f) = &e.kind else {
            continue;
        };
        let selected = selection.contains(e.id);
        if !(selected || working && f.plan_level(model) == level) {
            continue;
        }
        let outlines = match f.kind {
            creabim_core::FootingKind::Strip => strips.get(&e.id).cloned().unwrap_or_default(),
            creabim_core::FootingKind::Pad => vec![f.pad_outline(model)],
        };
        let tint = accent.gamma_multiply(if selected { 0.22 } else { 0.12 });
        let stroke = Stroke::new(if selected { 1.8 } else { 1.2 }, accent);
        for o in outlines.iter().filter(|o| o.len() > 2) {
            fill_polygon(painter, xf, o, tint);
            let mut pts: Vec<Pos2> = o.iter().map(|p| xf.to_screen(*p)).collect();
            pts.push(pts[0]);
            painter.extend(egui::Shape::dashed_line(&pts, stroke, 6.0, 4.0));
        }
    }
}

/// The selection's colour in a pane: what selected linework is drawn in.
pub fn accent_selected() -> Color32 {
    crate::theme::interaction()
}

/// How much heavier a traced mark is than the mark itself (A3-5).
pub const TRACE: f32 = 1.5;

/// The pre-selection highlight of element `id`: its linework traced and
/// its fills washed in the hover colour, over the drawing. Marks, text,
/// tags and dimensions are traced themselves, never boxed.
pub fn draw_hover(painter: &egui::Painter, xf: &Xf, items: &[PlanItem], id: ElementId, mm: f64) {
    let line = Stroke::new(2.5, hoverpick::HOVER.gamma_multiply(0.75));
    let wash = hoverpick::HOVER.gamma_multiply(0.16);
    for it in items.iter().filter(|it| it.id() == id) {
        if is_mark(it) {
            trace(painter, xf, it, hoverpick::HOVER, mm);
            continue;
        }
        match it {
            PlanItem::Fill { points, .. }
            | PlanItem::Tint { points, .. }
            | PlanItem::Shade { points, .. } => {
                fill_polygon(painter, xf, points, wash);
            }
            PlanItem::Room { points, .. } => {
                fill_polygon(painter, xf, points, wash);
                let pts: Vec<Pos2> = points.iter().map(|p| xf.to_screen(*p)).collect();
                ring_edges(painter, &pts, line);
            }
            PlanItem::Polygon { points, .. } => {
                let pts: Vec<Pos2> = points.iter().map(|p| xf.to_screen(*p)).collect();
                ring_edges(painter, &pts, line);
            }
            PlanItem::Line { pen, a, b, .. } => {
                let (a, b) = (xf.to_screen(*a), xf.to_screen(*b));
                // guides and grid lines run far: just the part in view
                if let Some((a, b)) = match pen {
                    Pen::Guide | Pen::Grid => clip_to_rect(a, b, painter.clip_rect()),
                    _ => Some((a, b)),
                } {
                    painter.line_segment([a, b], line);
                }
            }
            PlanItem::Arc {
                center,
                radius,
                start,
                end,
                ..
            } => {
                painter.add(egui::Shape::line(
                    arc_points(xf, *center, *radius, *start, *end),
                    line,
                ));
            }
            _ => {}
        }
    }
}

/// Is `item` part of a mark drawn on paper (a view mark's bubble, wedge
/// and line, a text, tag, label or dimension), which a hover or a
/// selection traces rather than washes?
pub fn is_mark(item: &PlanItem) -> bool {
    match item {
        PlanItem::Label { .. }
        | PlanItem::Text { .. }
        | PlanItem::Dimension { .. }
        | PlanItem::Tag { .. }
        | PlanItem::Annotation { .. } => true,
        PlanItem::Line { pen, .. } | PlanItem::Arc { pen, .. } => *pen == Pen::Symbol,
        // a view mark's solid wedge
        PlanItem::Shade { rgb, .. } => rgb.iter().sum::<f32>() < 1.0,
        _ => false,
    }
}

fn arc_points(xf: &Xf, center: DVec2, radius: f64, start: f64, end: f64) -> Vec<Pos2> {
    (0..=24)
        .map(|i| {
            let t = start + (end - start) * i as f64 / 24.0;
            xf.to_screen(center + DVec2::from_angle(t) * radius)
        })
        .collect()
}

/// One mark drawn over itself in `color`, its strokes [`TRACE`] times
/// their weight: what hovering it or selecting it shows.
pub fn trace(painter: &egui::Painter, xf: &Xf, item: &PlanItem, color: Color32, mm: f64) {
    let stroke = Stroke::new(TRACE, color);
    match item {
        PlanItem::Line { a, b, .. } => {
            painter.line_segment([xf.to_screen(*a), xf.to_screen(*b)], stroke);
        }
        PlanItem::Arc {
            center,
            radius,
            start,
            end,
            ..
        } => {
            painter.add(egui::Shape::line(
                arc_points(xf, *center, *radius, *start, *end),
                stroke,
            ));
        }
        PlanItem::Shade { points, .. } => fill_polygon(painter, xf, points, color),
        PlanItem::Label { at, text, .. } => {
            let p = xf.to_screen(*at);
            // the bubble's own paper size (see draw_item_with)
            let px = bubble_px((2.5 * mm * xf.scale as f64) as f32);
            if creabim_geom::plan::is_bubble(text) {
                let r = px * creabim_geom::plan::bubble_scale(text) as f32;
                painter.circle_stroke(p, r, stroke);
            }
            painter.text(
                p,
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::proportional(px.max(4.0)),
                color,
            );
        }
        PlanItem::Text {
            at,
            text,
            size,
            align,
            bold,
            angle,
            ..
        } => {
            // laid out as the drawing's own, so it covers it exactly
            // (owner, MU98_02: a hovered or selected label read twice)
            plan_text(
                painter, xf, *at, text, *size, *align, *bold, *angle, color, mm,
            );
        }
        PlanItem::Dimension {
            a,
            b,
            offset,
            text,
            look,
            witness,
            breaks,
            label,
            ..
        } => {
            let d = (*a, *b, *offset, *witness);
            draw_dimension_weighted(
                painter, xf, d, breaks, text, look, mm, color, TRACE, label, false,
            );
        }
        PlanItem::Annotation { item, .. } => {
            crate::annotate::paint_weighted(painter, xf, item, color, mm, TRACE);
        }
        PlanItem::Tag {
            anchor,
            at,
            elbow,
            text,
            round,
            look,
            ..
        } => {
            let g = tag_geometry(*anchor, *at, *elbow, text, *round, look, mm);
            if !g.leader.is_empty() {
                let line: Vec<Pos2> = g.leader.iter().map(|p| xf.to_screen(*p)).collect();
                painter.add(egui::Shape::line(line, stroke));
                crate::annotate::paint_ends_weighted(painter, xf, &g.ends, color, TRACE);
            }
            let pts: Vec<Pos2> = g.outline.iter().map(|p| xf.to_screen(*p)).collect();
            let heavy = Stroke::new(1.2 * TRACE, color);
            if g.closed {
                ring_edges(painter, &pts, heavy);
            } else if pts.len() > 1 {
                painter.add(egui::Shape::line(pts, heavy));
            }
            let px = crate::annotate::text_px(look, mm, xf);
            crate::annotate::text(
                painter,
                xf.to_screen(*at),
                text,
                px,
                color,
                0.0,
                paper::Anchor::Middle,
                look,
            );
        }
        _ => {}
    }
}

/// A dimension: witness lines, the line with its ticks, and its value.
#[allow(clippy::too_many_arguments)]
fn draw_dimension(
    painter: &egui::Painter,
    xf: &Xf,
    (a, b, offset, witness): (DVec2, DVec2, f64, Option<[DVec2; 2]>),
    breaks: &[[f64; 2]],
    text: &str,
    look: &TextLook,
    mm: f64,
    ink: Color32,
) {
    draw_dimension_weighted(
        painter,
        xf,
        (a, b, offset, witness),
        breaks,
        text,
        look,
        mm,
        ink,
        1.0,
        &Default::default(),
        false,
    );
}

/// A dimension with its strokes `w` times their weight.
#[allow(clippy::too_many_arguments)]
fn draw_dimension_weighted(
    painter: &egui::Painter,
    xf: &Xf,
    (a, b, offset, witness): (DVec2, DVec2, f64, Option<[DVec2; 2]>),
    breaks: &[[f64; 2]],
    text: &str,
    look: &TextLook,
    mm: f64,
    ink: Color32,
    w: f32,
    label: &creabim_geom::annotate::DimLabel,
    selected: bool,
) {
    let mut g =
        creabim_geom::annotate::dimension_layout(a, b, offset, witness, look, mm, text, label);
    g.break_line(breaks);
    for (p, q) in g.lines {
        painter.line_segment([xf.to_screen(p), xf.to_screen(q)], Stroke::new(w, ink));
    }
    crate::annotate::paint_ends_weighted(painter, xf, &g.ends, ink, w);
    let px = crate::annotate::text_px(look, mm, xf);
    crate::annotate::text(
        painter,
        xf.to_screen(g.text_at),
        text,
        px,
        ink,
        g.angle,
        paper::Anchor::Middle,
        look,
    );
    if !label.below.is_empty() {
        let small = TextLook {
            size: look.size * creabim_geom::annotate::BELOW_SIZE,
            ..*look
        };
        crate::annotate::text(
            painter,
            xf.to_screen(g.below_at),
            &label.below,
            crate::annotate::text_px(&small, mm, xf),
            ink,
            g.angle,
            paper::Anchor::Middle,
            &small,
        );
    }
    if selected {
        let r = DVec2::from_angle(g.angle);
        let half = look.width(text) * mm / 2.0;
        // an overridden number: a dot after it, on screen only (MU80 3A)
        if label.overridden {
            let at = g.text_at + r * (half + 1.2 * mm);
            painter.circle_filled(xf.to_screen(at), 2.5, crate::theme::interaction());
        }
        // Move text: the text's grip, the same square as a grid's or a
        // note's grip, just before the text so it reads as "drag this"
        // (MU81; MU83 asked what the small diamond meant)
        if label.movable {
            let at = xf.to_screen(g.text_at - r * (half + 1.2 * mm));
            painter.rect(
                egui::Rect::from_center_size(at, Vec2::splat(9.0)),
                1.5,
                Color32::WHITE,
                Stroke::new(1.5, crate::theme::interaction()),
                egui::StrokeKind::Middle,
            );
        }
    }
}

/// A dimension being drawn, measuring along `axis`.
#[allow(clippy::too_many_arguments)]
fn preview_dimension(
    painter: &egui::Painter,
    xf: &Xf,
    a: DVec2,
    b: DVec2,
    axis: creabim_core::DimAxis,
    offset: f64,
    look: &TextLook,
    mm: f64,
    accent: Color32,
) {
    let Some((b2, witness)) = annotate::squared(axis, a, b) else {
        return;
    };
    let text = annotate::dimension_text(&crate::units::get(), a, b2);
    draw_dimension(
        painter,
        xf,
        (a, b2, offset, witness),
        &[],
        &text,
        look,
        mm,
        accent,
    );
}

/// The point a dimension click lands on, what it attaches to, and the snap
/// that found it. Wall corners and door or window edges come first, since
/// those keep the most useful attachment; then any snap in the pane's
/// drawing: corners, edges and centres of masses, slabs, columns, beams,
/// sketches, and grid and level lines. A point on an element follows it.
fn dim_snap(
    doc: &Document,
    view: &View,
    kind: PaneKind,
    snapper: Option<&Snapper>,
    cfg: &snaps::SnapSettings,
    xf: &Xf,
    cursor: Pos2,
) -> (HostPoint, DVec2, Option<SnapKind>) {
    let (hp, at, k, _) = dim_snap_guided(doc, view, kind, snapper, cfg, xf, cursor, &[], None, &[]);
    (hp, at, k)
}

/// The line offset of a dimension from `a` to `b` measuring along `axis`
/// whose line goes through `at`.
fn dim_offset(axis: creabim_core::DimAxis, a: DVec2, b: DVec2, at: DVec2) -> f64 {
    let u = axis.direction(a, b).unwrap_or((b - a).normalize_or_zero());
    (at - a).dot(u.perp())
}

/// [`dim_snap`] with temporary guides too: where they cross each other
/// and the drawing's edges. Also returns the guides the point is on.
#[allow(clippy::too_many_arguments)]
fn dim_snap_guided(
    doc: &Document,
    view: &View,
    kind: PaneKind,
    snapper: Option<&Snapper>,
    cfg: &snaps::SnapSettings,
    xf: &Xf,
    cursor: Pos2,
    guides: &[snaps::TempGuide],
    from: Option<DVec3>,
    track: &[(DVec3, snaps::Through)],
) -> (HostPoint, DVec2, Option<SnapKind>, Vec<usize>) {
    let model = doc.model();
    let p = xf.to_world(cursor);
    let tol = 10.0 / xf.scale as f64;
    let mut best: Option<(f64, ElementId, DVec2)> = None;
    let mut consider = |host: ElementId, q: DVec2| {
        let d = q.distance(p);
        if d < tol && best.is_none_or(|(bd, ..)| d < bd) {
            best = Some((d, host, q));
        }
    };
    if let (PaneKind::Plan { level }, true) = (kind, cfg.allows(SnapKind::Endpoint)) {
        let joins = creabim_core::wall_joins(model);
        for e in model.iter() {
            let ElementKind::Wall(w) = &e.kind else {
                continue;
            };
            if w.level != level {
                continue;
            }
            let ends = joins.get(&e.id).cloned().unwrap_or_default();
            for q in ends.footprint(w) {
                consider(e.id, q);
            }
            for q in [w.start, w.end] {
                consider(e.id, q);
            }
            let (d, n, t) = (w.direction(), w.direction().perp(), w.thickness / 2.0);
            for oid in model.dependents(e.id) {
                let Some(ElementKind::Opening(o)) = model.get(oid).map(|e| &e.kind) else {
                    continue;
                };
                for u in [o.offset, o.offset + o.width] {
                    for s in [-t, t] {
                        consider(
                            oid,
                            if w.is_arc() {
                                w.place(u, s)
                            } else {
                                w.start + d * u + n * s
                            },
                        );
                    }
                }
            }
        }
    }
    let project = |q: DVec3| Some(xf.to_screen(q.truncate()));
    let hit = snapper.and_then(|s| {
        // from the point clicked last, as the drawing tools snap from
        // theirs: on its axes, square to it, lined up with the points
        // picked (A7-11)
        s.snap_guided(
            cfg,
            cursor,
            &project,
            from,
            track,
            guides,
            Some(p.extend(0.0)),
        )
    });
    let (host, at, kind, used) = match (best, hit) {
        // a nearer corner of something else beats the wall's
        (Some((d, host, q)), h)
            if h.as_ref().is_none_or(|h| {
                !h.snap.kind.is_point() || h.snap.p.truncate().distance(p) >= d
            }) =>
        {
            (Some(host), q, Some(SnapKind::Endpoint), vec![])
        }
        (_, Some(h)) => (
            h.snap.host,
            h.snap.p.truncate(),
            Some(h.snap.kind),
            h.guides,
        ),
        (_, None) => (None, p, None, vec![]),
    };
    (annotate::attach(model, view, host, at), at, kind, used)
}

/// Keep a temporary guide line as a guide element: in a plan on its
/// level, in a section on the section's plane.
fn keep_guide(
    doc: &mut Document,
    kind: PaneKind,
    guide: Option<snaps::TempGuide>,
) -> Result<String, creabim_core::ModelError> {
    let Some(g) = guide else {
        return Ok("Point at a guide line first, then press K to keep it.".into());
    };
    let snaps::GuideShape::Line { p, d } = g.shape else {
        return Ok("Only straight guide lines can be kept; circles stay temporary.".into());
    };
    let (level, a, b) = match kind {
        PaneKind::Plan { level } => {
            let z = doc.model().level(level).map_or(0.0, |l| l.elevation);
            (Some(level), p.extend(z), (p + d).extend(z))
        }
        PaneKind::Section { start, end } => {
            let space = Space::Section { start, end };
            (
                None,
                pane_to_model(space, p, 0.0),
                pane_to_model(space, p + d, 0.0),
            )
        }
        _ => return Ok(String::new()),
    };
    let e = Element::new(
        format!("Guide ({})", g.kind.label().to_lowercase()),
        ElementKind::Guide(creabim_core::Guide { level, a, b }),
    );
    doc.commit(Transaction::new("Keep guide line").insert(e))?;
    Ok(format!(
        "Kept the {} guide line. It stays until you delete it.",
        g.kind.label().to_lowercase()
    ))
}

/// A click with the Dimension tool: first point, second point, then where
/// the line goes. Continuous: then each click adds the next on that line.
/// The line snaps onto another dimension's line within `tol`, and to the
/// spacing steps (MU23 board).
#[allow(clippy::too_many_arguments)]
fn dimension_tool(
    doc: &mut Document,
    view: ElementId,
    hit: (HostPoint, DVec2),
    raw: DVec2,
    axis: creabim_core::DimAxis,
    state: &mut PlanState,
    selection: &mut Selection,
    others: &[(DVec2, DVec2, f64)],
    tol: f64,
) -> Result<(), creabim_core::ModelError> {
    // a chain: the next point, at once, on the chain's line
    if let (Some((axis, on_line)), 1) = (state.chain_line, state.dim.len()) {
        let (a, pa) = state.dim[0];
        if hit.1.distance(pa) < 1e-3 {
            return Ok(());
        }
        let (b, pb) = hit;
        // the line stays where it is, whichever way the chain runs
        let offset = dim_offset(axis, pa, pb, on_line);
        let mut d = Dimension::linear(view, a, b, offset);
        d.axis = axis;
        let e = state
            .anno
            .styled(Element::new("Dimension", ElementKind::Dimension(d)));
        selection.only(e.id);
        state.dim = vec![hit];
        return doc.commit(Transaction::new("Add dimension").insert(e));
    }
    match state.dim.len() {
        0 => {
            state.dim.push(hit);
            Ok(())
        }
        1 => {
            if hit.1.distance(state.dim[0].1) > 1e-3 {
                state.dim.push(hit);
            }
            Ok(())
        }
        _ => {
            let (a, pa) = state.dim[0];
            let (b, pb) = state.dim[1];
            state.dim.clear();
            // a horizontal or vertical one needs its points apart that way
            let axis = if axis.direction(pa, pb).is_some() {
                axis
            } else {
                creabim_core::DimAxis::Aligned
            };
            let mut offset = dim_offset(axis, pa, pb, raw);
            let u = axis
                .direction(pa, pb)
                .unwrap_or((pb - pa).normalize_or_zero());
            let n = u.perp();
            // onto another dimension's line, parallel and near (MU23)
            let mine = pa.dot(n) + offset;
            let mut aligned = false;
            for (oa, ob, oo) in others {
                let ou = (*ob - *oa).normalize_or_zero();
                if ou.perp_dot(u).abs() > 1e-3 {
                    continue;
                }
                let theirs = oa.dot(n) + *oo * ou.perp().dot(n);
                if (theirs - mine).abs() < tol {
                    offset = theirs - pa.dot(n);
                    aligned = true;
                    break;
                }
            }
            // else on spacing steps from what it measures
            if !aligned && state.dim_step_mm > 0.0 {
                let scale = match doc.model().get(view).map(|e| &e.kind) {
                    Some(ElementKind::View(v)) => v.scale,
                    _ => 50.0,
                };
                let step = state.dim_step_mm / 1000.0 * scale;
                offset = (offset / step).round() * step;
                if offset.abs() < 1e-9 {
                    offset = step.copysign(dim_offset(axis, pa, pb, raw));
                }
            }
            let mut d = Dimension::linear(view, a, b, offset);
            d.axis = axis;
            let e = state
                .anno
                .styled(Element::new("Dimension", ElementKind::Dimension(d)));
            selection.only(e.id);
            if state.dim_chain {
                // the chain goes on from the second point, on this line
                state.chain_line = Some((axis, pa + n * offset));
                state.dim = vec![(b, pb)];
                state.status = Some(
                    "Continuous: click the next point; Esc or right-click ends the chain.".into(),
                );
            }
            doc.commit(Transaction::new("Add dimension").insert(e))
        }
    }
}

/// A click with the Tag tool: pick what to tag, then where the tag goes.
fn tag_tool(
    doc: &mut Document,
    view: ElementId,
    v: &Option<View>,
    picked: Option<ElementId>,
    raw: DVec2,
    state: &mut PlanState,
    selection: &mut Selection,
) -> Result<(), creabim_core::ModelError> {
    let Some(v) = v else { return Ok(()) };
    match state.tag_host.take() {
        None => {
            let taggable = picked.filter(|id| is_taggable(doc.model(), *id));
            if picked.is_some() && taggable.is_none() {
                state.status = Some(
                    "That can't take a tag: pick a door, window, wall, slab, roof, column, beam, stair, railing, furniture or a fixture."
                        .into(),
                );
            }
            state.tag_host = taggable;
            Ok(())
        }
        Some(host) => {
            let (Some(proj), Some(c)) = (
                annotate::Projector::of(doc.model(), v),
                annotate::centre(doc.model(), host),
            ) else {
                return Ok(());
            };
            let mut t = Tag::new(view, host, raw - proj.project(c));
            t.material = state.anno.tag_material;
            let e = state.anno.styled(Element::new(
                if t.material { "Material tag" } else { "Tag" },
                ElementKind::Tag(t),
            ));
            selection.only(e.id);
            doc.commit(Transaction::new("Add tag").insert(e))
        }
    }
}

/// Can the Tag tool tag element `id`? Building elements can; views,
/// annotation and the like can't.
pub fn is_taggable(model: &creabim_core::Model, id: ElementId) -> bool {
    matches!(
        model.get(id).map(|e| &e.kind),
        Some(
            ElementKind::Opening(_)
                | ElementKind::Wall(_)
                | ElementKind::Slab(_)
                | ElementKind::FreeMesh(_)
                | ElementKind::Roof(_)
                | ElementKind::Column(_)
                | ElementKind::Beam(_)
                | ElementKind::Stair(_)
                | ElementKind::Railing(_)
                | ElementKind::Object(_)
                | ElementKind::Room(_)
                | ElementKind::Ceiling(_)
        )
    ) && annotate::centre(model, id).is_some()
}

/// Tag every services fixture (electrical, lighting, plumbing, heating and
/// ventilation) on a plan's level that has no tag there yet, with its
/// type's mark: in front of what stands against a wall, beside what stands
/// free or hangs from the ceiling. Furniture and sanitary ware are left
/// for the Tag tool, so a furnished plan doesn't drown in tags.
/// Returns how many were added.
pub fn tag_fixtures(
    doc: &mut Document,
    view: ElementId,
) -> Result<usize, creabim_core::ModelError> {
    let model = doc.model();
    let Some(ElementKind::View(v)) = model.get(view).map(|e| &e.kind) else {
        return Ok(0);
    };
    let ViewKind::Plan { level, .. } = v.kind else {
        return Ok(0);
    };
    let tagged: Vec<ElementId> = model
        .iter()
        .filter_map(|e| match &e.kind {
            ElementKind::Tag(t) if t.view == view => Some(t.host),
            _ => None,
        })
        .collect();
    let scale = v.scale / 100.0;
    let mut tx = Transaction::new("Tag fixtures");
    let mut n = 0;
    for e in model.iter() {
        let ElementKind::Object(o) = &e.kind else {
            continue;
        };
        if o.level != level || tagged.contains(&e.id) {
            continue;
        }
        let Some(t) = creabim_core::objects::type_of(model, e) else {
            continue;
        };
        if !t.category.is_services() {
            continue;
        }
        let offset = match t.mount {
            creabim_core::objects::Mount::Wall => o.facing() * (t.size().y / 2.0 + 0.45 * scale),
            _ => {
                DVec2::new(1.0, 1.0).normalize()
                    * (t.size().truncate().length() / 2.0 + 0.3 * scale)
            }
        };
        tx = tx.insert(Element::new(
            "Tag",
            ElementKind::Tag(Tag::new(view, e.id, offset)),
        ));
        n += 1;
    }
    if n > 0 {
        doc.commit(tx)?;
    }
    Ok(n)
}

/// Tag every door and window in a view that has no tag there yet, just
/// outside its wall. Returns how many were added.
pub fn tag_openings(
    doc: &mut Document,
    view: ElementId,
) -> Result<usize, creabim_core::ModelError> {
    let model = doc.model();
    let Some(ElementKind::View(v)) = model.get(view).map(|e| &e.kind) else {
        return Ok(0);
    };
    let level = match v.kind {
        ViewKind::Plan { level, .. } => Some(level),
        _ => None,
    };
    let Some(proj) = annotate::Projector::of(model, v) else {
        return Ok(0);
    };
    let tagged: Vec<ElementId> = model
        .iter()
        .filter_map(|e| match &e.kind {
            ElementKind::Tag(t) if t.view == view => Some(t.host),
            _ => None,
        })
        .collect();
    // the drawing's middle, to tell outside from inside
    let middle = creabim_geom::section::view_drawing(model, view)
        .and_then(|d| d.bounds())
        .map_or(DVec2::ZERO, |(lo, hi)| (lo + hi) / 2.0);
    let scale = v.scale / 100.0;
    let mut tx = Transaction::new("Tag doors and windows");
    let mut n = 0;
    for e in model.iter() {
        let ElementKind::Opening(o) = &e.kind else {
            continue;
        };
        if tagged.contains(&e.id) {
            continue;
        }
        let Some(ElementKind::Wall(w)) = model.get(o.host).map(|h| &h.kind) else {
            continue;
        };
        if level.is_some_and(|l| l != w.level) {
            continue;
        }
        let Some(c) = annotate::centre(model, e.id) else {
            continue;
        };
        let anchor = proj.project(c);
        let offset = match level {
            Some(_) => {
                let n = w.direction().perp();
                let out = if (anchor - middle).dot(n) > 0.0 {
                    n
                } else {
                    -n
                };
                out * 0.9 * scale
            }
            // in sections and 3D views, above the opening
            None => DVec2::new(0.0, (o.height / 2.0 + 0.5) * scale.max(0.5)),
        };
        tx = tx.insert(Element::new(
            "Tag",
            ElementKind::Tag(Tag::new(view, e.id, offset)),
        ));
        n += 1;
    }
    doc.commit(tx)?;
    Ok(n)
}

/// Drag a tag (moves it) or a dimension (moves its line) in the Select
/// tool. Returns `Some` while a drag is in progress.
#[allow(clippy::too_many_arguments)]
fn drag_annotation(
    doc: &mut Document,
    view: ElementId,
    drawing: &Drawing,
    xf: &Xf,
    response: &egui::Response,
    state: &mut PlanState,
    selection: &mut Selection,
    mm: f64,
) -> Option<Option<String>> {
    if response.drag_started_by(egui::PointerButton::Primary) {
        let p = response
            .ctx
            .input(|i| i.pointer.press_origin())
            .or(response.interact_pointer_pos())?;
        // a press on a grip of the selection is the grip's: a room tag's
        // box takes in its leader, so its end grip moved the whole tag
        // (MU142, seen in the app)
        if crate::annotate::on_grip(doc.model(), &drawing.items, selection, xf, p, mm) {
            return None;
        }
        let w = xf.to_world(p);
        // a dimension's text with Move text on: its own grip (MU80, MU81)
        let text_hit = drawing.items.iter().rev().find_map(|it| match it {
            PlanItem::Dimension {
                id,
                a,
                b,
                offset,
                witness,
                look,
                text,
                label,
                ..
            } if label.movable => {
                let g = creabim_geom::annotate::dimension_layout(
                    *a, *b, *offset, *witness, look, mm, text, label,
                );
                let r = DVec2::from_angle(g.angle);
                let d = w - g.text_at;
                let half = look.width(text) * mm / 2.0;
                let near = d.dot(r).abs() <= half + (2.4 * mm).max(5.0 / xf.scale as f64)
                    && d.dot(r.perp()).abs() <= (look.size * 0.7 + 1.0) * mm;
                near.then(|| {
                    let centred = creabim_geom::annotate::dimension_geometry(
                        *a, *b, *offset, *witness, look, mm,
                    )
                    .text_at;
                    (*id, g.text_at - centred)
                })
            }
            _ => None,
        });
        state.anno_text = text_hit.is_some();
        let hit = drawing.items.iter().rev().find_map(|it| match it {
            PlanItem::Tag { id, at, .. }
                if at.distance(w) < 3.0 * mm.max(6.0 / xf.scale as f64 / 3.0) =>
            {
                Some(*id)
            }
            PlanItem::Dimension {
                id, a, b, offset, ..
            } => {
                let n = (*b - *a).normalize_or_zero().perp() * *offset;
                (segment_distance(w, *a + n, *b + n) < 6.0 / xf.scale as f64).then_some(*id)
            }
            // a room's tag, drawn as its lines of text
            PlanItem::Annotation { id, item }
                if matches!(
                    doc.model().get(*id).map(|e| &e.kind),
                    Some(ElementKind::Tag(_))
                ) =>
            {
                let pts = item.extent();
                let first = *pts.first()?;
                let (lo, hi) = pts
                    .iter()
                    .fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
                let pad = DVec2::splat(mm);
                (w.cmpge(lo - pad).all() && w.cmple(hi + pad).all()).then_some(*id)
            }
            _ => None,
        });
        state.anno_drag = if let Some((id, moved)) = text_hit {
            Some((id, w, moved))
        } else {
            hit.and_then(|id| {
                let start = match &doc.model().get(id)?.kind {
                    ElementKind::Tag(t) if t.view == view => t.offset,
                    ElementKind::Dimension(d) if d.view == view => DVec2::new(d.offset, 0.0),
                    _ => return None,
                };
                Some((id, w, start))
            })
        };
        if let Some((id, ..)) = state.anno_drag {
            selection.only(id);
            state.drag_owner = Some(response.id);
        }
    }
    if state.anno_drag.is_some() && state.drag_owner != Some(response.id) {
        return None;
    }
    let (id, grab, start) = state.anno_drag?;
    if response.drag_stopped() || !response.dragged() {
        state.anno_drag = None;
        return Some(None);
    }
    let p = xf.to_world(response.interact_pointer_pos()?);
    if state.anno_text {
        let moved = start + p - grab;
        let r = doc.edit_merged(id, "Move dimension text", |e| {
            if let ElementKind::Dimension(d) = &mut e.kind {
                d.label.place = creabim_core::DimPlace::Moved;
                d.label.moved = Some(moved.to_array());
            }
        });
        return Some(r.err().map(|e| e.to_string()));
    }
    let r = match doc.model().get(id).map(|e| e.kind.clone()) {
        Some(ElementKind::Tag(_)) => doc.edit_merged(id, "Move tag", |e| {
            if let ElementKind::Tag(t) = &mut e.kind {
                t.offset = start + p - grab;
            }
        }),
        Some(ElementKind::Dimension(_)) => {
            // move the line square to what it measures
            let item = drawing.items.iter().find_map(|it| match it {
                PlanItem::Dimension { id: i, a, b, .. } if *i == id => Some((*a, *b)),
                _ => None,
            });
            let Some((a, b)) = item else {
                return Some(None);
            };
            let n = (b - a).normalize_or_zero().perp();
            let offset = start.x + (p - grab).dot(n);
            doc.edit_merged(id, "Move dimension line", |e| {
                if let ElementKind::Dimension(d) = &mut e.kind {
                    d.offset = offset;
                }
            })
        }
        _ => Ok(()),
    };
    Some(r.err().map(|e| e.to_string()))
}

/// Snap to wall endpoints within 10 px, else to a 5 cm grid, as far as
/// `cfg` allows. With `ortho`, keep the segment from `from` horizontal or
/// vertical.
#[allow(clippy::too_many_arguments)]
fn snap(
    doc: &Document,
    level: ElementId,
    xf: &Xf,
    p: DVec2,
    from: Option<DVec2>,
    ortho: bool,
    exclude: &[ElementId],
    cfg: &snaps::SnapSettings,
) -> DVec2 {
    let mut p = p;
    if let (true, Some(a)) = (ortho, from) {
        let d = p - a;
        p = if d.x.abs() >= d.y.abs() {
            DVec2::new(p.x, a.y)
        } else {
            DVec2::new(a.x, p.y)
        };
    }
    let tol = 10.0 / xf.scale as f64;
    let mut best: Option<(f64, DVec2)> = None;
    for e in doc.model().iter() {
        if let ElementKind::Wall(w) = &e.kind {
            if w.level != level || exclude.contains(&e.id) || !cfg.allows(SnapKind::Endpoint) {
                continue;
            }
            for q in [w.start, w.end] {
                let d = q.distance(p);
                if d < tol && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, q));
                }
            }
        }
    }
    best.map(|(_, q)| q).unwrap_or_else(|| {
        if cfg.allows(SnapKind::Grid) {
            (p / 0.05).round() * 0.05
        } else {
            p
        }
    })
}

/// Fill a possibly concave polygon.
/// Laid over a selected element's own fill: the theme's selection colour,
/// see-through, so its hatch, grips and dimensions still read (MU75: a
/// solid light orange, or a peach, hid them). Over the fill, not mixed
/// into it, so on dark paper the fill turns dark as its neighbours do and
/// only the tint shows.
fn selected_wash() -> Color32 {
    accent_selected().gamma_multiply(0.26)
}

/// A slab's fill in its level's plan, under the pointer, and selected.
const SLAB_FILL: Color32 = Color32::from_rgba_unmultiplied_const(70, 110, 170, 30);
const SLAB_HOVER: Color32 = Color32::from_rgba_unmultiplied_const(60, 110, 200, 70);
const SLAB_SELECTED: Color32 = Color32::from_rgba_unmultiplied_const(240, 140, 50, 60);
/// The edge of the slab under the pointer.
const SLAB_EDGE: Color32 = Color32::from_rgb(60, 110, 200);

/// The slabs of `level` the plan's drawing shows in projection (not cut
/// through), with their outlines.
fn level_slabs(
    model: &creabim_core::Model,
    level: ElementId,
    plan: &Drawing,
) -> Vec<(ElementId, Vec<DVec2>)> {
    plan.items
        .iter()
        .filter_map(|it| match it {
            PlanItem::Polygon {
                id,
                pen: Pen::Projection,
                points,
            } if matches!(
                model.get(*id).map(|e| &e.kind),
                Some(ElementKind::Slab(s)) if s.level == level
            ) =>
            {
                Some((*id, points.clone()))
            }
            _ => None,
        })
        .collect()
}

/// The slab whose outline holds `p`; the smallest when they overlap.
fn slab_at(slabs: &[(ElementId, Vec<DVec2>)], p: DVec2) -> Option<ElementId> {
    slabs
        .iter()
        .filter(|(_, pts)| point_in_polygon(p, pts))
        .min_by(|a, b| signed_area(&a.1).abs().total_cmp(&signed_area(&b.1).abs()))
        .map(|(id, _)| *id)
}

/// A closed outline as its edges, each on its own: egui mitres the
/// corners of a closed line, and where an outline turns back on itself
/// (a stair well round a spiral) the mitre shot half a million pixels
/// across the plan (MU100).
pub(crate) fn ring_edges(painter: &egui::Painter, pts: &[Pos2], stroke: Stroke) {
    let n = pts.len();
    if n < 2 {
        return;
    }
    let mut shapes = Vec::with_capacity(n);
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        if a != b {
            shapes.push(egui::Shape::line_segment([a, b], stroke));
        }
    }
    painter.extend(shapes);
}

/// A closed outline dashed as an overhead line is (6 on, 4 off), edge by
/// edge as [`ring_edges`] draws it.
pub(crate) fn dashed_ring(pts: &[Pos2], stroke: Stroke) -> Vec<egui::Shape> {
    let n = pts.len();
    let mut out = vec![];
    if n < 2 {
        return out;
    }
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        if a != b {
            out.extend(egui::Shape::dashed_line(&[a, b], stroke, 6.0, 4.0));
        }
    }
    out
}

pub fn fill_polygon(painter: &egui::Painter, xf: &Xf, points: &[DVec2], fill: Color32) {
    let mut mesh = egui::Mesh::default();
    for p in points {
        mesh.colored_vertex(xf.to_screen(*p), fill);
    }
    for [a, b, c] in triangulate(points) {
        mesh.add_triangle(a as u32, b as u32, c as u32);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// The pivot cursor: a red and white ring with a cross, as in Blender.
pub fn draw_cursor(painter: &egui::Painter, at: Pos2) {
    let red = Color32::from_rgb(220, 50, 50);
    painter.circle_stroke(at, 7.0, Stroke::new(2.0, Color32::WHITE));
    painter.add(egui::Shape::dashed_line(
        &[
            at + Vec2::new(7.0, 0.0),
            at + Vec2::new(0.0, 7.0),
            at + Vec2::new(-7.0, 0.0),
            at + Vec2::new(0.0, -7.0),
            at + Vec2::new(7.0, 0.0),
        ],
        Stroke::new(2.0, red),
        4.0,
        4.0,
    ));
    for d in [Vec2::X, Vec2::Y, -Vec2::X, -Vec2::Y] {
        painter.line_segment(
            [at + d * 9.0, at + d * 14.0],
            Stroke::new(1.0, Color32::BLACK),
        );
    }
}

/// Can a box take this element? Views and levels are left to clicks, so a
/// box round the building doesn't pick up its section marks.
pub fn box_selectable(doc: &Document, id: ElementId) -> bool {
    doc.model().get(id).is_some_and(|e| {
        !matches!(
            e.kind,
            ElementKind::View(_)
                | ElementKind::Level(_)
                | ElementKind::Sheet(_)
                | ElementKind::Viewport(_)
        )
    })
}

/// The earthworks of a model and its terrains, gathered once so a pick
/// can ask which a point is inside (see [`Works::first`]).
pub struct Works {
    terrains: Vec<ElementId>,
    /// Each work's area, id and outline, the smallest first.
    works: Vec<(f64, ElementId, Vec<DVec2>)>,
}

impl Works {
    pub fn of(model: &creabim_core::Model) -> Self {
        use creabim_core::SiteForm;
        let mut w = Works {
            terrains: vec![],
            works: vec![],
        };
        for e in model.iter() {
            let ElementKind::Site(s) = &e.kind else {
                continue;
            };
            if matches!(s.form, SiteForm::Terrain { .. }) {
                w.terrains.push(e.id);
            } else if let (Some(_), Some(b)) = (s.work_kind(), s.boundary()) {
                w.works.push((
                    creabim_core::polygon::signed_area2(b).abs(),
                    e.id,
                    b.to_vec(),
                ));
            }
        }
        w.works.sort_by(|a, b| a.0.total_cmp(&b.0));
        w
    }

    /// A click inside a pad, a pit or a trench takes it, not the ground
    /// it is worked into: where a terrain is among what is under `p` (a
    /// plan point), the earthworks `p` is inside go in before it, the
    /// smallest first. What is drawn over the ground (a wall, a slab)
    /// still wins.
    pub fn first(&self, under: &mut Vec<ElementId>, p: DVec2) {
        let Some(at) = under.iter().position(|id| self.terrains.contains(id)) else {
            return;
        };
        let inside: Vec<ElementId> = self
            .works
            .iter()
            .filter(|w| !under[..at].contains(&w.1) && creabim_core::polygon::contains(&w.2, p))
            .map(|w| w.1)
            .collect();
        under.retain(|id| !inside.contains(id));
        let at = under
            .iter()
            .position(|id| self.terrains.contains(id))
            .unwrap_or(under.len());
        for (k, id) in inside.into_iter().enumerate() {
            under.insert(at + k, id);
        }
    }
}

pub fn hit_test(items: &[PlanItem], p: DVec2, tol: f64, mm: f64) -> Option<ElementId> {
    hit_all(items, p, tol, mm).first().copied()
}

/// Every element whose linework is at `p`, in the order a click takes
/// them: annotations, then symbols (doors and windows over the walls they
/// sit in), then fills (a tinted element over the wall it is inside),
/// then lines; each once.
pub fn hit_all(items: &[PlanItem], p: DVec2, tol: f64, mm: f64) -> Vec<ElementId> {
    let mut out: Vec<ElementId> = Vec::new();
    let mut found = |id: ElementId| {
        if !out.contains(&id) {
            out.push(id);
        }
    };
    // symbols (doors, windows) win over the walls they sit in
    let near_line = |a: DVec2, b: DVec2| segment_distance(p, a, b) < tol;
    // annotations sit on top of everything
    for it in items.iter().rev() {
        match it {
            PlanItem::Tag { id, at, .. } if p.distance(*at) < (3.0 * mm).max(tol) => found(*id),
            PlanItem::Annotation { id, item } if item.hit(p, tol, mm) => found(*id),
            PlanItem::Symbol {
                id,
                symbol,
                at,
                unit,
                angle,
                origin,
                ..
            } => {
                let sh = creabim_geom::symbol::shapes_at(
                    symbol,
                    *at,
                    DVec2::splat(*unit),
                    *angle,
                    *origin,
                );
                if let Some((lo, hi)) = sh.bounds() {
                    if p.x >= lo.x - tol
                        && p.x <= hi.x + tol
                        && p.y >= lo.y - tol
                        && p.y <= hi.y + tol
                    {
                        found(*id);
                    }
                }
            }
            PlanItem::Dimension {
                id, a, b, offset, ..
            } => {
                let n = (*b - *a).normalize_or_zero().perp() * *offset;
                let text = (*a + *b) / 2.0 + n;
                if near_line(*a + n, *b + n) || p.distance(text) < (3.0 * mm).max(tol) {
                    found(*id);
                }
            }
            PlanItem::Text {
                id,
                at,
                text,
                size,
                align,
                ..
            } if *id != ElementId::NIL => {
                // schedule rows: anywhere along the row's height
                let w = paper::text_width(text, *size) * mm;
                let x0 = match align {
                    paper::Anchor::Start => at.x,
                    paper::Anchor::Middle => at.x - w / 2.0,
                    paper::Anchor::End => at.x - w,
                };
                if (p.y - at.y).abs() < size * mm && p.x >= x0 - tol && p.x <= x0 + w + tol {
                    found(*id);
                }
            }
            _ => {}
        }
    }
    for it in items.iter().rev() {
        match it {
            PlanItem::Line {
                id,
                pen: Pen::Symbol,
                a,
                b,
            } if near_line(*a, *b) => found(*id),
            PlanItem::Arc {
                id,
                center,
                radius,
                pen,
                ..
            } if !matches!(pen, Pen::Underlay | Pen::Hidden)
                && (p.distance(*center) - radius).abs() < tol =>
            {
                found(*id)
            }
            // view marks and grid ends: their arrow or their bubble
            PlanItem::Polygon {
                id,
                pen: Pen::Symbol,
                points,
            } if point_in_polygon(p, points) => found(*id),
            PlanItem::Label { id, at, text }
                if text.chars().count() <= 3 && p.distance(*at) < tol * 1.2 =>
            {
                found(*id)
            }
            _ => {}
        }
    }
    // the filled shapes the pointer is in, the smallest first: a path on a
    // pad on the ground takes the click before the pad, and the pad before
    // the ground, whatever order they are painted in (MU130: the order of
    // painting put the pad before the path lying on it)
    let mut filled: Vec<(f64, ElementId)> = items
        .iter()
        .rev()
        .filter_map(|it| match it {
            PlanItem::Fill { id, points, .. }
            | PlanItem::Tint { id, points, .. }
            | PlanItem::Shade { id, points, .. }
                if point_in_polygon(p, points) =>
            {
                Some((creabim_core::polygon::signed_area2(points).abs(), *id))
            }
            _ => None,
        })
        .collect();
    // an element drawn as many small pieces (a stair's treads) counts as
    // the whole of them, so it is not always first
    let mut whole: std::collections::HashMap<ElementId, f64> = Default::default();
    for it in items {
        if let PlanItem::Fill { id, points, .. }
        | PlanItem::Tint { id, points, .. }
        | PlanItem::Shade { id, points, .. } = it
        {
            if filled.iter().any(|f| f.1 == *id) {
                *whole.entry(*id).or_default() += creabim_core::polygon::signed_area2(points).abs();
            }
        }
    }
    for f in &mut filled {
        f.0 = whole.get(&f.1).copied().unwrap_or(f.0);
    }
    filled.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, id) in filled {
        found(id);
    }
    for it in items.iter().rev() {
        match it {
            PlanItem::Line { id, a, b, pen }
                if !matches!(pen, Pen::Underlay | Pen::Hidden) && near_line(*a, *b) =>
            {
                found(*id)
            }
            PlanItem::Polygon { id, points, pen } if *pen != Pen::Hidden => {
                let n = points.len();
                if (0..n).any(|i| near_line(points[i], points[(i + 1) % n])) {
                    found(*id);
                }
            }
            _ => {}
        }
    }
    // what is in the cut wins over what is drawn dashed below or above it:
    // a click in a door's opening takes the door, not the footing under it
    let mut boxes: Vec<(ElementId, DVec2, DVec2)> = vec![];
    for it in items {
        let (id, pts): (ElementId, Vec<DVec2>) = match it {
            PlanItem::Line {
                id,
                pen: Pen::Symbol,
                a,
                b,
            } => (*id, vec![*a, *b]),
            PlanItem::Arc {
                id,
                pen: Pen::Symbol,
                center,
                radius,
                start,
                end,
            } => (
                *id,
                (0..=8)
                    .map(|i| {
                        let t = start + (end - start) * i as f64 / 8.0;
                        *center + DVec2::from_angle(t) * *radius
                    })
                    .collect(),
            ),
            _ => continue,
        };
        let lo = pts.iter().fold(DVec2::splat(f64::MAX), |m, q| m.min(*q));
        let hi = pts.iter().fold(DVec2::splat(f64::MIN), |m, q| m.max(*q));
        match boxes.iter_mut().find(|b| b.0 == id) {
            Some(b) => {
                b.1 = b.1.min(lo);
                b.2 = b.2.max(hi);
            }
            None => boxes.push((id, lo, hi)),
        }
    }
    let below = |id: &ElementId| {
        items.iter().all(|it| match it {
            PlanItem::Line { id: i, pen, .. }
            | PlanItem::Arc { id: i, pen, .. }
            | PlanItem::Polygon { id: i, pen, .. } => i != id || *pen == Pen::Hidden,
            _ => true,
        })
    };
    let mut hidden: Vec<ElementId> = vec![];
    for it in items.iter().rev() {
        let hit = match it {
            PlanItem::Line {
                id,
                pen: Pen::Hidden,
                a,
                b,
            } if near_line(*a, *b) => Some(*id),
            PlanItem::Arc {
                id,
                pen: Pen::Hidden,
                center,
                radius,
                ..
            } if (p.distance(*center) - radius).abs() < tol => Some(*id),
            PlanItem::Polygon {
                id,
                pen: Pen::Hidden,
                points,
            } if {
                let n = points.len();
                (0..n).any(|i| near_line(points[i], points[(i + 1) % n]))
            } =>
            {
                Some(*id)
            }
            _ => None,
        };
        if let Some(id) = hit {
            if !hidden.contains(&id) {
                hidden.push(id);
            }
        }
    }
    if hidden.iter().any(below) {
        for (id, lo, hi) in &boxes {
            let (lo, hi) = (*lo - DVec2::splat(tol), *hi + DVec2::splat(tol));
            if p.cmpge(lo).all() && p.cmple(hi).all() {
                found(*id);
            }
        }
    }
    for id in hidden {
        found(id);
    }
    // rooms last: a click inside one that finds nothing else picks it
    for it in items.iter().rev() {
        if let PlanItem::Room { id, points, .. } = it {
            if point_in_polygon(p, points) {
                found(*id);
            }
        }
    }
    out
}

fn segment_distance(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

fn point_in_polygon(p: DVec2, poly: &[DVec2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}

/// A new door's or window's frame: the kind picked for the tool, else the
/// usual (a type, when picked, sets its own after).
fn next_frame(
    model: &creabim_core::Model,
    kind: OpeningKind,
    state: &PlanState,
) -> creabim_core::OpeningFrame {
    let pick = match kind {
        OpeningKind::Door => state.members.door_kind,
        OpeningKind::Window => state.members.window_kind,
        OpeningKind::Hole => None,
    };
    pick.filter(|p| p.kind() == kind).map_or_else(
        || creabim_core::OpeningFrame::for_kind(kind),
        |p| p.frame(model),
    )
}

fn place_opening(
    doc: &mut Document,
    level: ElementId,
    p: DVec2,
    tool: Tool,
    state: &PlanState,
    selection: &mut Selection,
) -> Result<(), creabim_core::ModelError> {
    let mut best: Option<(f64, ElementId, f64)> = None;
    // the wall the pointer showed wins (Tab may have moved past the first)
    let shown = state
        .host_pick
        .filter(|h| crate::memberstool::walls_at(doc.model(), level, p).contains(h));
    for e in doc.model().iter() {
        let ElementKind::Wall(w) = &e.kind else {
            continue;
        };
        if w.level != level || shown.is_some_and(|h| h != e.id) {
            continue;
        }
        // round the arc on a curved wall
        let (u, d) = w.along(p);
        if d <= w.thickness.max(0.3) && best.is_none_or(|b| d < b.0) {
            best = Some((d, e.id, u));
        }
    }
    let Some((_, host, center)) = best else {
        return Ok(());
    };
    let Some(e) = opening_on_wall(doc.model(), host, center, tool, state) else {
        return Ok(());
    };
    selection.only(e.id);
    doc.commit(Transaction::new(format!("Place {}", tool.label().to_lowercase())).insert(e))
}

/// The same opening the plan tool places, with an explicit host and centre.
pub(crate) fn opening_on_wall(
    model: &creabim_core::Model,
    host: ElementId,
    center: f64,
    tool: Tool,
    state: &PlanState,
) -> Option<Element> {
    let ElementKind::Wall(wall) = &model.get(host)?.kind else {
        return None;
    };
    let len = wall.length();
    // the sizes in the bar (a type, when picked, sets its own)
    let m = &state.members;
    let (kind, [width, height, sill], name) = match tool {
        Tool::Door => (OpeningKind::Door, m.door, "Door"),
        Tool::Opening => (OpeningKind::Hole, m.hole, "Wall opening"),
        _ => (OpeningKind::Window, m.window, "Window"),
    };
    let mut e = state.typed(
        model,
        Element::new(
            name,
            ElementKind::Opening(Opening {
                host,
                kind,
                offset: 0.0,
                sill,
                width,
                height,
                material: None,
                frame: next_frame(model, kind, state),
                flip_hand: false,
                flip_facing: false,
                roof_at: None,
                roof_build: Default::default(),
                arrangement: None,
            }),
        ),
    );
    // centred on the click, as wide as its type makes it
    if let ElementKind::Opening(o) = &mut e.kind {
        if o.width > len {
            return None;
        }
        o.offset = (center - o.width / 2.0).clamp(0.0, len - o.width);
        // clear of a corner (MU23)
        creabim_core::walls::clamp_opening(model, o);
    }
    Some(e)
}

/// The pane behaviour for a view.
pub fn pane_kind(v: &View) -> PaneKind {
    match v.kind {
        ViewKind::Plan { level, .. } => PaneKind::Plan { level },
        ViewKind::Section { start, end, .. } => PaneKind::Section { start, end },
        ViewKind::Axonometric { .. } => PaneKind::Axonometric,
        ViewKind::Schedule { .. } => PaneKind::Schedule,
        // drawn from its drain, nothing in it is drawn on
        ViewKind::LongSection { .. }
        | ViewKind::BoreholeLog { .. }
        | ViewKind::WalkSection { .. } => PaneKind::Schedule,
    }
}

/// The points of the shape being drawn in a view, in its drawing
/// coordinates: a slab outline, a sketch, a railing path or a chain of
/// walls. Alignment tracking lines the next point up with them.
fn shape_points(
    state: &PlanState,
    view: ElementId,
    kind: PaneKind,
    from_pane: &dyn Fn(DVec3) -> DVec2,
) -> Vec<DVec2> {
    let level = match kind {
        PaneKind::Plan { level } => Some(level),
        _ => None,
    };
    if let Some(o) = state.outline.as_ref().filter(|o| Some(o.level) == level) {
        return o.points.clone();
    }
    if let Some(d) = state.sketch.as_ref().filter(|d| d.pane == Some(view)) {
        return d
            .points
            .iter()
            .map(|p| from_pane(d.plane.to_model(*p)))
            .collect();
    }
    if level.is_some() && !state.members.path.is_empty() {
        return state.members.path.clone();
    }
    if level.is_some() && !state.members.stair_line.is_empty() {
        return state.members.stair_line.clone();
    }
    match (&state.chain, state.pending) {
        ((Some(l), pts), Some(_)) if Some(*l) == level => pts.clone(),
        _ => vec![],
    }
}

/// Rest on a point or edge for a moment (or press Q over it) and it becomes
/// a snap reference in this view, its guide lines showing; rest on one
/// again and it goes. Returns a status message when one is picked or
/// dropped.
/// An Align click is not a rest: the pointer left still on the edge it
/// clicked doesn't make guides there, so the message for the click stays.
fn click_is_not_a_rest(
    state: &mut PlanState,
    tool: Tool,
    clicked: bool,
    under: Option<snaps::Reference>,
    now: f64,
) {
    if let (Tool::Align, true, Some(r)) = (tool, clicked, under) {
        state.ref_rest = Some((r, now, true));
    }
}

fn rest_reference(
    state: &mut PlanState,
    view: ElementId,
    under: Option<snaps::Reference>,
    now: f64,
    key: bool,
) -> Option<String> {
    const DWELL: f64 = REST_DWELL;
    let Some(r) = under else {
        state.ref_rest = None;
        return None;
    };
    let toggle = match state.ref_rest {
        Some((q, since, done)) if q.same(&r) => {
            let due = !done && (key || now - since > DWELL);
            state.ref_rest = Some((q, since, done || due));
            due
        }
        _ => {
            state.ref_rest = Some((r, now, key));
            key
        }
    };
    if !toggle {
        return None;
    }
    let refs = state.refs.entry(view).or_default();
    let what = match r {
        snaps::Reference::Point { .. } => "point",
        snaps::Reference::Edge { arc: Some(_), .. } => "arc",
        snaps::Reference::Edge { .. } => "edge",
    };
    if let Some(i) = refs.iter().position(|q| q.same(&r)) {
        refs.remove(i);
        return Some(format!("Dropped the {what}'s guide lines."));
    }
    refs.push(r);
    if refs.len() > snaps::MAX_REFERENCES {
        refs.remove(0);
    }
    state.refs_used.insert(view, now);
    Some(format!(
        "Guide lines from this {what}: where they cross snaps. They go when not used for a few seconds; K keeps one."
    ))
}

/// A selected line stair's padlock, at the end that stays put when its
/// treads change: where it was drawn, and whether that is its foot.
fn stair_lock(
    painter: &egui::Painter,
    xf: &Xf,
    model: &creabim_core::Model,
    level: ElementId,
    id: ElementId,
    ctx: &egui::Context,
) -> Option<(egui::Rect, bool)> {
    let Some(ElementKind::Stair(s)) = model.get(id).map(|e| &e.kind) else {
        return None;
    };
    if s.level != level || s.line.len() < 2 || s.curved() {
        return None;
    }
    let rise = s.rise(model);
    let lay = s.layout(rise);
    let c = if s.fixed_foot {
        // beside its first riser, clear of the UP mark at its foot
        let f = lay.flights.first()?;
        xf.to_screen(f.start - f.dir.perp() * (s.width / 2.0)) + {
            let n = -f.dir.perp();
            egui::vec2(n.x as f32, -n.y as f32) * 16.0
        }
    } else {
        // just past its head, off the stair
        let (at, dir) = s.head(rise)?;
        xf.to_screen(at) + egui::vec2(dir.x as f32, -dir.y as f32) * 16.0
    };
    let rect = egui::Rect::from_center_size(c, egui::vec2(16.0, 18.0));
    let col = crate::theme::Palette::of(ctx).accent;
    let body = egui::Rect::from_min_max(
        rect.min + egui::vec2(1.0, 8.0),
        rect.max - egui::vec2(1.0, 0.0),
    );
    painter.rect_filled(body, 2.5, col);
    let shackle = vec![
        egui::pos2(rect.min.x + 4.0, body.min.y),
        egui::pos2(rect.min.x + 4.0, rect.min.y + 4.5),
        egui::pos2(rect.center().x, rect.min.y + 1.0),
        egui::pos2(rect.max.x - 4.0, rect.min.y + 4.5),
        egui::pos2(rect.max.x - 4.0, body.min.y),
    ];
    painter.add(egui::Shape::line(shackle, egui::Stroke::new(2.0, col)));
    Some((rect, s.fixed_foot))
}

/// A selected stair's missing treads past the end of its line: dashed red
/// outlines and "N treads short" by the last one.
fn draw_short_treads(
    painter: &egui::Painter,
    xf: &Xf,
    model: &creabim_core::Model,
    level: ElementId,
    id: ElementId,
    sizes: bool,
) {
    let Some(ElementKind::Stair(s)) = model.get(id).map(|e| &e.kind) else {
        return;
    };
    if s.level != level || s.line.len() < 2 {
        return;
    }
    // its landings' depths and a half-turn's gap (REV02 §5)
    if sizes {
        crate::memberstool::draw_landing_sizes(
            painter,
            &|p| Some(xf.to_screen(p)),
            &s.landing_sizes(s.rise(model)),
        );
    }
    let (short, ghosts) = s.short_treads(s.rise(model));
    if short == 0 {
        return;
    }
    let red = egui::Color32::from_rgb(200, 60, 50);
    let stroke = egui::Stroke::new(1.3, red);
    for g in &ghosts {
        let mut pts: Vec<Pos2> = g.iter().map(|p| xf.to_screen(*p)).collect();
        if let Some(f) = pts.first().copied() {
            pts.push(f);
        }
        painter.extend(egui::Shape::dashed_line(&pts, stroke, 4.0, 3.0));
    }
    if let Some(last) = ghosts.last() {
        let c = last.iter().copied().sum::<DVec2>() / last.len().max(1) as f64;
        let word = if short == 1 { "tread" } else { "treads" };
        let at = xf.to_screen(c) + egui::vec2(10.0, -18.0);
        let galley = painter.layout_no_wrap(
            format!("{short} {word} short · drag the line's end on"),
            egui::FontId::proportional(12.0),
            red,
        );
        let bg = egui::Rect::from_min_size(at, galley.size()).expand(3.0);
        painter.rect_filled(
            bg,
            4.0,
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 220),
        );
        painter.galley(at, galley, red);
    }
}

/// How near the pointer (screen px) a stair's line must pass to be offered
/// as a guide: just outside the snap's own reach, so a line shows as the
/// pointer comes onto it and not before (MU102; REV00's 40 px showed a
/// crowd of them).
const STAIR_REACH: f64 = 16.0;

/// How long the pointer rests on a point or edge before it becomes a
/// snap reference, as ArchiCAD's hover delay.
pub const REST_DWELL: f64 = 0.6;

/// Seconds a view's guide lines last with nothing using them.
pub const REF_FADE: f64 = 6.0;

/// Drop a view's references once nothing has used them for [`REF_FADE`].
fn fade_references(state: &mut PlanState, view: ElementId, now: f64, used: bool) {
    if state.refs.get(&view).is_none_or(|r| r.is_empty()) {
        state.refs_used.remove(&view);
        return;
    }
    let last = state.refs_used.entry(view).or_insert(now);
    if used {
        *last = now;
    } else if now - *last > REF_FADE {
        state.refs.remove(&view);
        state.refs_used.remove(&view);
    }
}

/// While drawing, a point snapped to and rested on for a moment joins the
/// points tracked (the last few of them), as in SketchUp.
fn rest_on(
    state: &mut PlanState,
    view: ElementId,
    snap: Option<snaps::Snap>,
    drawing: bool,
    now: f64,
) {
    const DWELL: f64 = 0.4;
    const KEEP: usize = 4;
    if !drawing {
        state.rested.1.clear();
        state.rest = None;
        return;
    }
    if state.rested.0 != Some(view) {
        state.rested = (Some(view), vec![]);
    }
    let Some(p) = snap.filter(|s| s.kind.is_point()).map(|s| s.p.truncate()) else {
        state.rest = None;
        return;
    };
    match state.rest {
        Some((q, since)) if q.distance(p) < 1e-9 => {
            let pts = &mut state.rested.1;
            if now - since > DWELL && !pts.iter().any(|r| r.distance(p) < 1e-9) {
                pts.push(p);
                if pts.len() > KEEP {
                    pts.remove(0);
                }
            }
        }
        _ => state.rest = Some((p, now)),
    }
}

#[cfg(test)]
mod dimension_chain_tests {
    use super::*;

    #[test]
    fn a_continuous_chain_stays_on_one_line_and_rows_snap_together() {
        // MU23 board: continuous dimensions, and lines onto other lines
        let mut doc = creabim_core::Document::from_model(creabim_core::starter::starter_house());
        let view = doc
            .model()
            .iter()
            .find(|e| matches!(&e.kind, ElementKind::View(v) if matches!(v.kind, creabim_core::ViewKind::Plan { .. })))
            .unwrap()
            .id;
        let mut state = PlanState {
            dim_chain: true,
            ..Default::default()
        };
        let mut sel = Selection::default();
        let h = |x: f64| (HostPoint::free(DVec2::new(x, 0.0)), DVec2::new(x, 0.0));
        let ax = creabim_core::DimAxis::Horizontal;
        let mut click =
            |doc: &mut creabim_core::Document, state: &mut PlanState, x: f64, raw: DVec2| {
                dimension_tool(doc, view, h(x), raw, ax, state, &mut sel, &[], 0.1).unwrap();
            };
        click(&mut doc, &mut state, 0.0, DVec2::ZERO);
        click(&mut doc, &mut state, 2.0, DVec2::ZERO);
        click(&mut doc, &mut state, 1.0, DVec2::new(1.0, 1.5)); // the line, 1.5 up
        click(&mut doc, &mut state, 5.0, DVec2::ZERO); // the chain goes on
        click(&mut doc, &mut state, 6.5, DVec2::ZERO);
        let dims: Vec<(f64, f64)> = doc
            .model()
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::Dimension(d) if d.view == view => Some((d.a.local.x, d.offset)),
                _ => None,
            })
            .collect();
        // the three just made are the last ones
        let made: Vec<&(f64, f64)> = dims.iter().rev().take(3).collect();
        assert_eq!(
            made.iter().map(|d| d.0).collect::<Vec<_>>(),
            vec![5.0, 2.0, 0.0],
            "{dims:?}"
        );
        for (_, off) in made {
            assert!((off - 1.5).abs() < 1e-9, "on one line: {dims:?}");
        }
        // a new row placed near another dimension's line lands on it
        state.cancel();
        state.dim_chain = false;
        let others = [(DVec2::new(0.0, 0.0), DVec2::new(2.0, 0.0), 3.0)];
        dimension_tool(
            &mut doc,
            view,
            h(10.0),
            DVec2::ZERO,
            ax,
            &mut state,
            &mut sel,
            &others,
            0.2,
        )
        .unwrap();
        dimension_tool(
            &mut doc,
            view,
            h(12.0),
            DVec2::ZERO,
            ax,
            &mut state,
            &mut sel,
            &others,
            0.2,
        )
        .unwrap();
        dimension_tool(
            &mut doc,
            view,
            h(11.0),
            DVec2::new(11.0, 3.1),
            ax,
            &mut state,
            &mut sel,
            &others,
            0.2,
        )
        .unwrap();
        let last = doc
            .model()
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::Dimension(d)
                    if d.view == view && (d.a.local.x - 10.0).abs() < 1e-9 =>
                {
                    Some(d.offset)
                }
                _ => None,
            })
            .last()
            .unwrap();
        assert!((last - 3.0).abs() < 1e-9, "{last}");
    }
}

#[cfg(test)]
mod text_twice {
    use super::*;

    /// The text shapes `paint` puts down: where each sits and how big.
    fn texts(paint: impl Fn(&egui::Painter)) -> Vec<(Pos2, egui::Vec2, String)> {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            paint(&ui.ctx().layer_painter(egui::LayerId::background()));
        });
        // no renderer here to take the font atlas
        out.textures_delta.clear();
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some((t.pos, t.galley.size(), t.galley.text().to_string())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_highlighted_label_lies_exactly_on_the_drawings() {
        let xf = Xf {
            rect: Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0)),
            center: DVec2::ZERO,
            scale: 400.0,
        };
        for (align, bold) in [
            (paper::Anchor::Middle, false),
            (paper::Anchor::Start, false),
            (paper::Anchor::End, true),
        ] {
            let item = PlanItem::Text {
                id: creabim_core::ElementId::new(),
                at: DVec2::new(0.4, -0.2),
                text: "N 0°00'00\" E  22.88 m".into(),
                size: 2.5,
                align,
                mm: 0.1,
                rgb: Some([0.2, 0.2, 0.2]),
                bold,
                angle: 0.0,
            };
            let drawn = texts(|p| draw_item(p, &xf, &item, false, 0.1));
            let over = texts(|p| trace(p, &xf, &item, Color32::RED, 0.1));
            assert!(!drawn.is_empty());
            assert_eq!(
                drawn, over,
                "{align:?}: the highlight's text parts from the drawing's"
            );
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn the_cut_height_box_stays_inside_its_area() {
        use super::{egui, pill_pos, Rect, Vec2};
        let area = Rect::from_min_max(egui::pos2(850.0, 540.0), egui::pos2(1480.0, 975.0));
        let size = Vec2::new(200.0, 34.0);
        // mid-area: to the point's right
        let p = pill_pos(egui::pos2(1000.0, 700.0), size, area);
        assert_eq!(p.x, 1012.0);
        // by the right edge (MU55-02, a grip at x 1356): flipped to its left
        for x in [1356.0, 1470.0, 1479.0] {
            let p = pill_pos(egui::pos2(x, 750.0), size, area);
            assert!(p.x + size.x <= area.right() - 4.0 + 1e-3, "{x}: {p:?}");
            assert!(p.x >= area.left() + 4.0 - 1e-3);
        }
        // at the top edge: pushed down inside
        let p = pill_pos(egui::pos2(1000.0, 541.0), size, area);
        assert!(p.y >= area.top() + 4.0 - 1e-3);
    }

    #[test]
    fn an_arc_wall_passes_through_the_point_clicked() {
        let (a, b) = (DVec2::new(3.0, 0.0), DVec2::new(-3.0, 0.0));
        for t in [
            DVec2::new(0.0, 3.0),
            DVec2::new(0.0, -3.0),
            DVec2::new(2.0, 1.0),
        ] {
            let k = bulge_through(a, t, b).unwrap();
            let c = creabim_core::sketch::Curve::arc(a, b, k);
            let pts = c.points();
            let near = pts.iter().map(|p| p.distance(t)).fold(f64::MAX, f64::min);
            // within a step of the arc's own drawing
            assert!(near < 0.25, "{t}: {near}");
        }
        assert!(bulge_through(a, DVec2::ZERO, b).is_none());
    }

    /// The handle in a plan (MU130: the owner could not move with it in
    /// Edit mode): one corner of an outline picked, its X arrow dragged
    /// 40 px. The corner goes 0.4 m along X and nowhere else, with the
    /// button down all the way; letting go ends it; no box is drawn. Then
    /// the same by a click, a move and a click.
    #[test]
    fn the_handle_moves_a_picked_corner_in_a_plan() {
        use eframe::egui::{Pos2, Rect};
        crate::units::set(creabim_core::Units::metric());
        let mut doc = creabim_core::Document::from_model(creabim_core::starter::starter_house());
        let (view, level) = doc
            .model()
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::View(v) if e.name == "Ground Floor plan" => match v.kind {
                    ViewKind::Plan { level, .. } => Some((e.id, level)),
                    _ => None,
                },
                _ => None,
            })
            .unwrap();
        let outline = creabim_core::Element::new(
            "Probe",
            ElementKind::Sketch(creabim_core::Sketch::polyline(
                Some(level),
                creabim_core::Workplane::ground(0.0),
                vec![
                    DVec2::new(30.0, 0.0),
                    DVec2::new(34.0, 0.0),
                    DVec2::new(34.0, 2.0),
                    DVec2::new(30.0, 2.0),
                ],
                true,
            )),
        );
        let id = outline.id;
        doc.commit(creabim_core::Transaction::new("probe").insert(outline))
            .unwrap();
        let ctx = egui::Context::default();
        let mut state = PlanState::default();
        state.cameras.insert(view, (DVec2::new(32.0, 1.0), 100.0));
        let mut sel = crate::selection::Selection::default();
        sel.only(id);
        state.edit = Some(id);
        state.grips.reset(Some(id));
        state.grips.pick(handles::Ring::Outline, 1, false);
        state.grips.publish(state.edit);
        let area = Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 800.0));
        let xf = Xf {
            rect: area,
            center: DVec2::new(32.0, 1.0),
            scale: 100.0,
        };
        let corner = xf.to_screen(DVec2::new(34.0, 0.0));
        let mut t = 0.0;
        let mut frame = |events: Vec<egui::Event>,
                         doc: &mut creabim_core::Document,
                         state: &mut PlanState,
                         sel: &mut crate::selection::Selection| {
            t += 1.0 / 60.0;
            let raw = egui::RawInput {
                screen_rect: Some(area),
                events,
                time: Some(t),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                let d = creabim_geom::section::view_drawing_with(doc.model(), view, false).unwrap();
                let style = PaneStyle {
                    background: None,
                    decorations: false,
                    camera: None,
                    scale: 50.0,
                };
                show(
                    ui,
                    area,
                    doc,
                    view,
                    PaneKind::Plan { level },
                    &d,
                    state,
                    sel,
                    style,
                );
            });
            out.textures_delta.clear();
            // what the app does between frames
            state.take_handle();
            state.grips.publish(state.edit);
        };
        let at = |doc: &creabim_core::Document| match &doc.model().get(id).unwrap().kind {
            ElementKind::Sketch(s) => s.points[1],
            _ => DVec2::ZERO,
        };
        let button = |pos: Pos2, pressed: bool| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        // on the X arrow, 50 px out from the corner
        let on_arrow = corner + egui::vec2(50.0, 0.0);
        for _ in 0..2 {
            frame(
                vec![egui::Event::PointerMoved(on_arrow)],
                &mut doc,
                &mut state,
                &mut sel,
            );
        }
        frame(vec![button(on_arrow, true)], &mut doc, &mut state, &mut sel);
        let mut seen = std::collections::BTreeSet::new();
        for dx in 1..=40 {
            // a little off the line too: the arrow keeps it to X
            let p = on_arrow + egui::vec2(dx as f32, dx as f32 * 0.2);
            frame(
                vec![egui::Event::PointerMoved(p)],
                &mut doc,
                &mut state,
                &mut sel,
            );
            seen.insert((at(&doc).x * 1e4).round() as i64);
            assert!(state.boxsel.is_none(), "a selection box began at {dx} px");
        }
        assert!(
            seen.len() > 20,
            "the corner took only {} places over 40 px",
            seen.len()
        );
        let end = on_arrow + egui::vec2(40.0, 8.0);
        frame(vec![button(end, false)], &mut doc, &mut state, &mut sel);
        frame(
            vec![egui::Event::PointerMoved(end)],
            &mut doc,
            &mut state,
            &mut sel,
        );
        assert!(
            state.modal.is_none(),
            "letting go ends it: {:?}",
            state.status
        );
        let p = at(&doc);
        assert!(
            (p.x - 34.4).abs() < 0.02 && p.y.abs() < 1e-6,
            "dragged to {p}: {:?}",
            state.status
        );
        assert_eq!(state.edit, Some(id), "still in Edit mode");
        assert!(
            state.grips.picked(handles::Ring::Outline, 1),
            "still picked"
        );

        // by a click: picked up, it follows with the button up; a click places
        let corner = xf.to_screen(DVec2::new(p.x, p.y));
        let on_arrow = corner + egui::vec2(50.0, 0.0);
        for _ in 0..2 {
            frame(
                vec![egui::Event::PointerMoved(on_arrow)],
                &mut doc,
                &mut state,
                &mut sel,
            );
        }
        frame(vec![button(on_arrow, true)], &mut doc, &mut state, &mut sel);
        frame(
            vec![button(on_arrow, false)],
            &mut doc,
            &mut state,
            &mut sel,
        );
        for dx in 1..=30 {
            let q = on_arrow + egui::vec2(dx as f32, 0.0);
            frame(
                vec![egui::Event::PointerMoved(q)],
                &mut doc,
                &mut state,
                &mut sel,
            );
        }
        assert!(
            state.modal.is_some(),
            "picked up by the click, it still follows: {:?}",
            state.status
        );
        let there = on_arrow + egui::vec2(30.0, 0.0);
        frame(vec![button(there, true)], &mut doc, &mut state, &mut sel);
        frame(vec![button(there, false)], &mut doc, &mut state, &mut sel);
        frame(
            vec![egui::Event::PointerMoved(there)],
            &mut doc,
            &mut state,
            &mut sel,
        );
        assert!(state.modal.is_none(), "the second click places it");
        let q = at(&doc);
        assert!(
            (q.x - (p.x + 0.3)).abs() < 0.02 && q.y.abs() < 1e-6,
            "clicked to {q}"
        );
    }

    /// One drag of the handle in a plan or a section, for the tests below:
    /// with `id` in Edit mode and `pick` done, press 70 px out from
    /// `grip` along `dir` (an arrow of the handle), drag 40 px on along
    /// it and a little across, let go. Returns what `read` gave before
    /// and after. No box may begin, and letting go must end the edit.
    #[allow(clippy::too_many_arguments)]
    fn drag_the_handle(
        doc: &mut creabim_core::Document,
        view: ElementId,
        kind: PaneKind,
        centre: DVec2,
        id: ElementId,
        pick: &dyn Fn(&mut PlanState),
        grip: DVec2,
        dir: egui::Vec2,
        read: &dyn Fn(&creabim_core::Document) -> f64,
    ) -> (f64, f64) {
        use eframe::egui::{Pos2, Rect};
        let ctx = egui::Context::default();
        let mut state = PlanState::default();
        state.cameras.insert(view, (centre, 100.0));
        let mut sel = crate::selection::Selection::default();
        sel.only(id);
        state.edit = Some(id);
        state.grips.reset(Some(id));
        pick(&mut state);
        state.grips.publish(state.edit);
        let area = Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 800.0));
        let xf = Xf {
            rect: area,
            center: centre,
            scale: 100.0,
        };
        let mut t = 0.0;
        let mut frame = |events: Vec<egui::Event>,
                         doc: &mut creabim_core::Document,
                         state: &mut PlanState,
                         sel: &mut crate::selection::Selection| {
            t += 1.0 / 60.0;
            let raw = egui::RawInput {
                screen_rect: Some(area),
                events,
                time: Some(t),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                let d = creabim_geom::section::view_drawing_with(doc.model(), view, false).unwrap();
                let style = PaneStyle {
                    background: None,
                    decorations: false,
                    camera: None,
                    scale: 50.0,
                };
                show(ui, area, doc, view, kind, &d, state, sel, style);
            });
            out.textures_delta.clear();
            state.take_handle();
            state.grips.publish(state.edit);
        };
        let button = |pos: Pos2, pressed: bool| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let before = read(doc);
        let across = egui::vec2(-dir.y, dir.x);
        let press = xf.to_screen(grip) + dir * 70.0;
        for _ in 0..2 {
            frame(
                vec![egui::Event::PointerMoved(press)],
                doc,
                &mut state,
                &mut sel,
            );
        }
        frame(vec![button(press, true)], doc, &mut state, &mut sel);
        for k in 1..=40 {
            let p = press + dir * k as f32 + across * (k as f32 * 0.2);
            frame(
                vec![egui::Event::PointerMoved(p)],
                doc,
                &mut state,
                &mut sel,
            );
            assert!(state.boxsel.is_none(), "a selection box began at {k} px");
        }
        let end = press + dir * 40.0 + across * 8.0;
        frame(vec![button(end, false)], doc, &mut state, &mut sel);
        frame(
            vec![egui::Event::PointerMoved(end)],
            doc,
            &mut state,
            &mut sel,
        );
        assert!(
            state.modal.is_none(),
            "letting go ends it: {:?}",
            state.status
        );
        assert_eq!(state.edit, Some(id), "still in Edit mode");
        eprintln!(
            "after the drag: {:?}; picked {:?} {:?}",
            state.status, state.grips.corners, state.grips.mesh
        );
        (before, read(doc))
    }

    /// MU142: a section's ground levels are tags. Each is found by its
    /// words and by its leader apart (the words move it, the leader bends
    /// it), its leader ends on the ground it measures, and a bend kept in
    /// the view is drawn where it was put.
    #[test]
    fn a_level_tag_is_taken_by_its_words_or_its_leader() {
        use creabim_core::{Element, Level, Pad, Site, SiteForm, SiteView, View};
        use glam::DVec3;
        // a terrain rising 1 in 10 to the north, a pad levelled at 2 m in
        // its middle, and a section across it from south to north
        let lv = Element::new(
            "Ground",
            ElementKind::Level(Level {
                elevation: 0.0,
                height: 3.0,
            }),
        );
        let mut t = Site::terrain(
            lv.id,
            creabim_core::polygon::rectangle(DVec2::ZERO, DVec2::new(40.0, 40.0)),
            0.0,
            1.0,
        );
        if let SiteForm::Terrain { points, data, .. } = &mut t.form {
            for i in 0..=8 {
                for j in 0..=8 {
                    let (x, y) = (i as f64 * 5.0, j as f64 * 5.0);
                    points.push(DVec3::new(x, y, y * 0.1));
                }
            }
            data.survey = true;
            data.faceted = true;
            data.pads.push(Pad::new(
                creabim_core::polygon::rectangle(DVec2::new(15.0, 15.0), DVec2::new(25.0, 25.0)),
                2.0,
            ));
        }
        let te = Element::new("Terrain", ElementKind::Site(t));
        let mut v = View::section(DVec2::new(20.0, -2.0), DVec2::new(20.0, 42.0), 30.0, true);
        v.scale = 100.0;
        v.site = SiteView::default();
        let ve = Element::new("Section", ElementKind::View(v));
        let (tid, vid) = (te.id, ve.id);
        let mut m = creabim_core::Model::from_elements([lv, te, ve]).unwrap();
        let mm = 0.1;
        let d = creabim_geom::section::view_drawing(&m, vid).unwrap();
        let tags = level_tags(&m, &d, mm);
        assert!(tags.len() >= 3, "{tags:?}");
        let t0 = tags[0];
        assert_eq!(t0.key.site, tid);
        // its leader ends on the ground, under its words
        assert!(t0.foot.y < t0.centre.y - t0.half.y, "{t0:?}");
        // the words take it, not the leader; the leader's middle takes the
        // leader, not the words
        assert!(t0.on_words(t0.centre) && !t0.on_leader(t0.centre, 0.06));
        let mid = t0.bend_grip();
        assert!(t0.on_leader(mid, 0.06) && !t0.on_words(mid));
        // a was-line over a tag is part of its words: it takes the tag too
        let was_at = d.items.iter().find_map(|i| match i {
            PlanItem::Annotation { item, .. } if item.text.starts_with("was ") => {
                creabim_geom::declutter::text_box(i, mm).map(|(b, _)| (b.lo + b.hi) / 2.0)
            }
            _ => None,
        });
        let was_at = was_at.expect("a level the works moved has its was-line");
        assert!(
            tags.iter().any(|t| t.on_words(was_at)),
            "{was_at} on no tag: {tags:?}"
        );
        // bent in the view: drawn through the bend
        let bend = DVec2::new(0.8, 0.5);
        let ve = m.get(vid).unwrap().clone();
        let mut bent = ve.clone();
        if let ElementKind::View(v) = &mut bent.kind {
            v.site.set_tag_bend(tid, t0.key.x, Some(bend));
        }
        creabim_core::Transaction::new("bend")
            .replace(ve, bent)
            .apply(&mut m)
            .unwrap();
        let d = creabim_geom::section::view_drawing(&m, vid).unwrap();
        let again = level_tags(&m, &d, mm);
        let same = again
            .iter()
            .find(|t| (t.key.x - t0.key.x).abs() < 1e-9)
            .expect("still there");
        assert!(
            same.elbow
                .is_some_and(|e| e.distance(t0.foot + bend) < 1e-9),
            "{same:?}"
        );
    }

    /// MU142: a site plan's levels are tags, taken as a section's are: by
    /// their words or the run to their point, kept by the point they read;
    /// moved through that key, drawn moved.
    #[test]
    fn a_site_plan_level_is_a_level_tag_kept_by_its_point() {
        let mut m = creabim_core::ridgeline::ridgeline_house();
        let view = m.iter().find(|e| e.name == "Site plan").unwrap().id;
        let mm = match &m.get(view).unwrap().kind {
            ElementKind::View(v) => v.scale / 1000.0,
            _ => unreachable!(),
        };
        let d = creabim_geom::section::view_drawing(&m, view).unwrap();
        let tags = level_tags(&m, &d, mm);
        assert!(!tags.is_empty(), "the road's levels");
        // one at home: its words over its point
        let t0 = *tags
            .iter()
            .find(|t| t.elbow.is_none() && t.centre.y > t.foot.y)
            .expect("one at home");
        assert_eq!(t0.key.at, Some(t0.foot));
        assert!(t0.on_words(t0.centre));
        let mid = t0.bend_grip();
        assert!(t0.on_leader(mid, 0.05) && !t0.on_words(mid), "{t0:?}");
        // moved through its key: drawn moved, found by the same key
        let ve = m.get(view).unwrap().clone();
        let mut moved = ve.clone();
        if let ElementKind::View(v) = &mut moved.kind {
            t0.key.set_by(v, Some(DVec2::new(2.0, 1.5)));
            assert!(t0.key.kept(v));
            assert_eq!(t0.key.by(v), DVec2::new(2.0, 1.5));
        }
        creabim_core::Transaction::new("move")
            .replace(ve, moved)
            .apply(&mut m)
            .unwrap();
        let d = creabim_geom::section::view_drawing(&m, view).unwrap();
        let again = level_tags(&m, &d, mm);
        let same = again
            .iter()
            .find(|t| t.key == t0.key)
            .expect("found by its point");
        assert!(
            same.centre.distance(t0.centre + DVec2::new(2.0, 1.5)) < 1.0,
            "{same:?} from {t0:?}"
        );
        // away from its point, its leader is drawn and takes it
        assert!(same.on_leader(same.bend_grip(), 0.05));
    }

    /// A room tag moved out of its room: dragging its leader's end moves
    /// the end, not the tag (MU142, seen in the app: the tag's box takes
    /// in its leader, and the end grip moved the whole tag).
    #[test]
    fn a_room_tags_leader_end_drags_the_end_not_the_tag() {
        use eframe::egui::{Pos2, Rect};
        let mut m = creabim_core::ridgeline::ridgeline_house();
        let named = |m: &creabim_core::Model, n: &str| m.iter().find(|e| e.name == n).unwrap().id;
        let (view, kitchen) = (named(&m, "Ground Floor plan"), named(&m, "Kitchen"));
        let tag = m
            .iter()
            .find(|e| matches!(&e.kind, ElementKind::Tag(t) if t.host == kitchen && t.view == view))
            .unwrap()
            .clone();
        let mut out = tag.clone();
        if let ElementKind::Tag(t) = &mut out.kind {
            t.offset = DVec2::new(0.0, -4.0);
        }
        creabim_core::Transaction::new("out")
            .replace(tag.clone(), out)
            .apply(&mut m)
            .unwrap();
        let level = match &m.get(view).unwrap().kind {
            ElementKind::View(v) => match v.kind {
                creabim_core::ViewKind::Plan { level, .. } => level,
                _ => unreachable!(),
            },
            _ => unreachable!(),
        };
        let mut doc = creabim_core::Document::from_model(m);
        let d = creabim_geom::section::view_drawing_with(doc.model(), view, false).unwrap();
        let tip = d
            .items
            .iter()
            .find_map(|i| match i {
                PlanItem::Annotation { id, item } if *id == tag.id => match item.shape {
                    creabim_geom::annotation::Shape::Note { leader, .. } => leader,
                    _ => None,
                },
                _ => None,
            })
            .expect("moved out, it has a leader");
        let read = |doc: &creabim_core::Document| match &doc.model().get(tag.id).unwrap().kind {
            ElementKind::Tag(t) => (t.offset, t.end),
            _ => unreachable!(),
        };
        let (offset0, end0) = read(&doc);
        let ctx = egui::Context::default();
        let mut state = PlanState::default();
        state.cameras.insert(view, (tip, 100.0));
        let mut sel = crate::selection::Selection::default();
        sel.only(tag.id);
        let area = Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 800.0));
        let at = area.center();
        let mut t = 0.0;
        let button = |pos: Pos2, pressed: bool| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let mut steps = vec![
            vec![egui::Event::PointerMoved(at)],
            vec![egui::Event::PointerMoved(at)],
            vec![button(at, true)],
        ];
        for k in 1..=30 {
            steps.push(vec![egui::Event::PointerMoved(
                at + egui::vec2(-(k as f32), 0.0),
            )]);
        }
        steps.push(vec![button(at + egui::vec2(-30.0, 0.0), false)]);
        for events in steps {
            t += 1.0 / 60.0;
            let raw = egui::RawInput {
                screen_rect: Some(area),
                events,
                time: Some(t),
                ..Default::default()
            };
            let mut o = ctx.run_ui(raw, |ui| {
                let d = creabim_geom::section::view_drawing_with(doc.model(), view, false).unwrap();
                let style = PaneStyle {
                    background: None,
                    decorations: false,
                    camera: None,
                    scale: 50.0,
                };
                show(
                    ui,
                    area,
                    &mut doc,
                    view,
                    PaneKind::Plan { level },
                    &d,
                    &mut state,
                    &mut sel,
                    style,
                );
            });
            o.textures_delta.clear();
        }
        let (offset1, end1) = read(&doc);
        assert_eq!(offset1, offset0, "the tag itself moved: {:?}", state.status);
        assert_ne!(
            end1, end0,
            "its leader's end did not move: {:?}",
            state.status
        );
    }

    /// The handle moves what is picked on every kind of thing Edit mode
    /// opens, in a plan: a site area (the owner's entry path, MU130), a
    /// slab, a shape's point and a shape's edge. 40 px at 100 px a metre
    /// is 0.4 m along the arrow.
    #[test]
    fn the_handle_moves_what_is_picked_on_every_kind_in_a_plan() {
        crate::units::set(creabim_core::Units::metric());
        let mut doc = creabim_core::Document::from_model(creabim_core::starter::starter_house());
        let (view, level) = doc
            .model()
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::View(v) if e.name == "Ground Floor plan" => match v.kind {
                    ViewKind::Plan { level, .. } => Some((e.id, level)),
                    _ => None,
                },
                _ => None,
            })
            .unwrap();
        let square = || {
            vec![
                DVec2::new(30.0, 0.0),
                DVec2::new(34.0, 0.0),
                DVec2::new(34.0, 2.0),
                DVec2::new(30.0, 2.0),
            ]
        };
        let path = creabim_core::Element::new(
            "Path",
            ElementKind::Site(creabim_core::Site::area(
                level,
                square(),
                0.1,
                creabim_core::site::Pattern::None,
            )),
        );
        let slab = creabim_core::Element::new(
            "Slab",
            ElementKind::Slab(creabim_core::Slab::new(level, square(), 0.2)),
        );
        let shape = creabim_core::Element::new(
            "Shape",
            ElementKind::FreeMesh(FreeMesh::new(
                Some(level),
                DVec3::new(32.0, 1.0, 0.0),
                creabim_core::editmesh::EditMesh::cuboid(DVec3::new(4.0, 2.0, 1.0)),
            )),
        );
        let (path_id, slab_id, shape_id) = (path.id, slab.id, shape.id);
        let kind = PaneKind::Plan { level };
        let centre = DVec2::new(32.0, 1.0);
        let right = egui::vec2(1.0, 0.0);
        let up = egui::vec2(0.0, -1.0);

        // outlines: the corner nearest (34, 0), by the X arrow, then the Y
        for (name, e, id) in [("a site area", path, path_id), ("a slab", slab, slab_id)] {
            let mut d = creabim_core::Document::from_model(doc.model().clone());
            d.commit(creabim_core::Transaction::new("probe").insert(e))
                .unwrap();
            let corners = |d: &creabim_core::Document| {
                crate::editmode::ring_of(
                    d.model(),
                    d.model().get(id).unwrap(),
                    handles::Ring::Outline,
                )
                .unwrap()
                .corners
            };
            let i = corners(&d)
                .iter()
                .position(|c| c.truncate().distance(DVec2::new(34.0, 0.0)) < 1e-6)
                .unwrap();
            let pick = move |s: &mut PlanState| s.grips.pick(handles::Ring::Outline, i, false);
            let (x0, x1) = drag_the_handle(
                &mut d,
                view,
                kind,
                centre,
                id,
                &pick,
                DVec2::new(34.0, 0.0),
                right,
                &|d| corners(d)[i].x,
            );
            assert!((x1 - x0 - 0.4).abs() < 0.02, "{name}: X went {x0} to {x1}");
            let (y0, y1) = drag_the_handle(
                &mut d,
                view,
                kind,
                centre,
                id,
                &pick,
                DVec2::new(x1, 0.0),
                up,
                &|d| corners(d)[i].y,
            );
            assert!((y1 - y0 - 0.4).abs() < 0.02, "{name}: Y went {y0} to {y1}");
            assert!(
                (corners(&d)[i].x - x1).abs() < 1e-6,
                "{name}: Y moved X too"
            );
        }

        // a shape: one point, then an edge (two points)
        doc.commit(creabim_core::Transaction::new("probe").insert(shape))
            .unwrap();
        let mesh = |d: &creabim_core::Document| match &d.model().get(shape_id).unwrap().kind {
            ElementKind::FreeMesh(m) => m.clone(),
            _ => unreachable!(),
        };
        let world = |d: &creabim_core::Document, v: u32| {
            let m = mesh(d);
            m.origin + m.to_model(m.mesh.vertices[v as usize])
        };
        let m = mesh(&doc);
        // a top point, and the top edge it shares with a neighbour
        let top: Vec<u32> = (0..m.mesh.vertices.len() as u32)
            .filter(|v| world(&doc, *v).z > 0.5)
            .collect();
        let a = top[0];
        let b = *top
            .iter()
            .find(|v| **v != a && (world(&doc, **v) - world(&doc, a)).truncate().length() < 2.5)
            .unwrap();
        let wa = world(&doc, a);
        let (x0, x1) = drag_the_handle(
            &mut doc,
            view,
            kind,
            centre,
            shape_id,
            &move |s: &mut PlanState| s.grips.pick_mesh(&[a], false),
            wa.truncate(),
            right,
            &|d| world(d, a).x,
        );
        assert!(
            (x1 - x0 - 0.4).abs() < 0.02,
            "a shape's point: X went {x0} to {x1}"
        );
        assert!(
            (world(&doc, b).x - wa.x).abs() < 1e-6 || b != a,
            "only the point moved"
        );
        let mid = (world(&doc, a) + world(&doc, b)) / 2.0;
        let (y0, y1) = drag_the_handle(
            &mut doc,
            view,
            kind,
            centre,
            shape_id,
            &move |s: &mut PlanState| s.grips.pick_mesh(&[a, b], false),
            mid.truncate(),
            up,
            &|d| world(d, a).y + world(d, b).y,
        );
        assert!(
            (y1 - y0 - 0.8).abs() < 0.04,
            "a shape's edge: Y sum went {y0} to {y1}"
        );
    }

    /// The handle in a section: a shape's top edge picked, its Z arrow
    /// dragged up 40 px, goes up 0.4 m and nowhere else.
    #[test]
    fn the_handle_moves_a_picked_edge_in_a_section() {
        crate::units::set(creabim_core::Units::metric());
        let mut doc = creabim_core::Document::from_model(creabim_core::starter::starter_house());
        let (view, start, end) = doc
            .model()
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::View(v) => match v.kind {
                    ViewKind::Section { start, end, .. } => Some((e.id, start, end)),
                    _ => None,
                },
                _ => None,
            })
            .unwrap();
        let along = (end - start).normalize();
        // a shape standing on the section line, 2 m from its start
        let foot = start + along * 2.0;
        let shape = creabim_core::Element::new(
            "Shape",
            ElementKind::FreeMesh(FreeMesh::new(
                None,
                DVec3::new(foot.x, foot.y, 20.0),
                creabim_core::editmesh::EditMesh::cuboid(DVec3::new(1.0, 1.0, 1.0)),
            )),
        );
        let id = shape.id;
        doc.commit(creabim_core::Transaction::new("probe").insert(shape))
            .unwrap();
        let world = |d: &creabim_core::Document, v: u32| match &d.model().get(id).unwrap().kind {
            ElementKind::FreeMesh(m) => m.origin + m.to_model(m.mesh.vertices[v as usize]),
            _ => unreachable!(),
        };
        let top: Vec<u32> = (0..8u32).filter(|v| world(&doc, *v).z > 20.0).collect();
        let (a, b) = (top[0], top[1]);
        let mid = (world(&doc, a) + world(&doc, b)) / 2.0;
        let on_paper = DVec2::new((mid.truncate() - start).dot(along), mid.z);
        let (z0, z1) = drag_the_handle(
            &mut doc,
            view,
            PaneKind::Section { start, end },
            DVec2::new(2.0, 20.0),
            id,
            &move |s: &mut PlanState| s.grips.pick_mesh(&[a, b], false),
            on_paper,
            egui::vec2(0.0, -1.0),
            &|d| world(d, a).z + world(d, b).z,
        );
        assert!(
            (z1 - z0 - 0.8).abs() < 0.04,
            "a shape's edge in a section: Z sum went {z0} to {z1}"
        );
        let now = (world(&doc, a) + world(&doc, b)) / 2.0;
        assert!(
            (now.truncate() - mid.truncate()).length() < 1e-6,
            "it went sideways too"
        );
    }

    /// Two areas showing the same plan, side by side, share the drag; a
    /// drag started in one goes on every frame until the button is let go
    /// (the owner's MU16 report: in plan the drag only moved at the press
    /// and the release, because the other area ended it after one frame).
    #[test]
    fn a_drag_in_one_of_two_plan_areas_follows_the_pointer() {
        use eframe::egui::{Pos2, Rect};
        crate::units::set(creabim_core::Units::metric());
        let mut doc = creabim_core::Document::from_model(creabim_core::starter::starter_house());
        let (view, level) = doc
            .model()
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::View(v) if e.name == "Ground Floor plan" => match v.kind {
                    ViewKind::Plan { level, .. } => Some((e.id, level)),
                    _ => None,
                },
                _ => None,
            })
            .unwrap();
        let wall = creabim_core::Element::new(
            "Probe",
            ElementKind::Wall(creabim_core::Wall::new(
                level,
                DVec2::new(30.0, 0.0),
                DVec2::new(34.0, 0.0),
                0.2,
                3.0,
            )),
        );
        let id = wall.id;
        doc.commit(creabim_core::Transaction::new("probe").insert(wall))
            .unwrap();
        let ctx = egui::Context::default();
        let mut state = PlanState::default();
        state.cameras.insert(view, (DVec2::new(32.0, 1.5), 100.0));
        let mut sel = crate::selection::Selection::default();
        sel.only(id);
        let left = Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 800.0));
        let right = Rect::from_min_size(Pos2::new(1000.0, 0.0), egui::vec2(1000.0, 800.0));
        let xf = Xf {
            rect: left,
            center: DVec2::new(32.0, 1.5),
            scale: 100.0,
        };
        let end = xf.to_screen(DVec2::new(34.0, 0.0));
        let mut t = 0.0;
        let mut frame = |events: Vec<egui::Event>,
                         doc: &mut creabim_core::Document,
                         state: &mut PlanState,
                         sel: &mut crate::selection::Selection| {
            t += 1.0 / 60.0;
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(2000.0, 800.0))),
                events,
                time: Some(t),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                for (i, rect) in [left, right].into_iter().enumerate() {
                    ui.push_id(("area", i), |ui| {
                        let d = creabim_geom::section::view_drawing_with(doc.model(), view, false)
                            .unwrap();
                        let style = PaneStyle {
                            background: None,
                            decorations: false,
                            camera: None,
                            scale: 50.0,
                        };
                        show(
                            ui,
                            rect,
                            doc,
                            view,
                            PaneKind::Plan { level },
                            &d,
                            state,
                            sel,
                            style,
                        );
                    });
                }
            });
            out.textures_delta.clear();
        };
        let length = |doc: &creabim_core::Document| match &doc.model().get(id).unwrap().kind {
            ElementKind::Wall(w) => w.length(),
            _ => 0.0,
        };
        frame(
            vec![egui::Event::PointerMoved(end)],
            &mut doc,
            &mut state,
            &mut sel,
        );
        frame(
            vec![egui::Event::PointerMoved(end)],
            &mut doc,
            &mut state,
            &mut sel,
        );
        frame(
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
            &mut doc,
            &mut state,
            &mut sel,
        );
        let mut seen = std::collections::BTreeSet::new();
        for dx in 1..=40 {
            let p = end + egui::vec2(dx as f32, 0.0);
            frame(
                vec![egui::Event::PointerMoved(p)],
                &mut doc,
                &mut state,
                &mut sel,
            );
            seen.insert((length(&doc) * 1e4).round() as i64);
        }
        // free (grid snap off by default): a new length nearly every pixel
        assert!(
            seen.len() > 30,
            "the wall took only {} lengths over 40 px",
            seen.len()
        );
        assert!(
            (length(&doc) - 4.4).abs() < 0.02,
            "ended at {}",
            length(&doc)
        );
    }

    /// A grip sliding along its line stops where an edge crosses its path:
    /// a wall's end dragged up to another wall's face stops on the face
    /// (the owner's MU16 report: a wall's base wouldn't stop on a slab's
    /// top in a section; only corners and middles caught a sliding grip).
    #[test]
    fn a_sliding_grip_stops_on_an_edge_across_its_path() {
        use eframe::egui::{Pos2, Rect};
        crate::units::set(creabim_core::Units::metric());
        let mut doc = creabim_core::Document::from_model(creabim_core::starter::starter_house());
        let (view, level) = doc
            .model()
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::View(v) if e.name == "Ground Floor plan" => match v.kind {
                    ViewKind::Plan { level, .. } => Some((e.id, level)),
                    _ => None,
                },
                _ => None,
            })
            .unwrap();
        let wall = |a: DVec2, b: DVec2| {
            creabim_core::Element::new(
                "Probe",
                ElementKind::Wall(creabim_core::Wall::new(level, a, b, 0.2, 3.0)),
            )
        };
        let probe = wall(DVec2::new(30.0, 0.0), DVec2::new(34.0, 0.0));
        let id = probe.id;
        // a long parallel wall, its near face at y = 0.9, its corners far
        let across = wall(DVec2::new(20.0, 1.0), DVec2::new(45.0, 1.0));
        doc.commit(
            creabim_core::Transaction::new("probe")
                .insert(probe)
                .insert(across),
        )
        .unwrap();
        let ctx = egui::Context::default();
        let mut state = PlanState::default();
        state.cameras.insert(view, (DVec2::new(33.0, 0.5), 100.0));
        let mut sel = crate::selection::Selection::default();
        sel.only(id);
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 800.0));
        let xf = Xf {
            rect,
            center: DVec2::new(33.0, 0.5),
            scale: 100.0,
        };
        let mut t = 0.0;
        let mut frame = |events: Vec<egui::Event>,
                         doc: &mut creabim_core::Document,
                         state: &mut PlanState,
                         sel: &mut crate::selection::Selection| {
            t += 1.0 / 60.0;
            let raw = egui::RawInput {
                screen_rect: Some(rect),
                events,
                time: Some(t),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                let d = creabim_geom::section::view_drawing_with(doc.model(), view, false).unwrap();
                let style = PaneStyle {
                    background: None,
                    decorations: false,
                    camera: None,
                    scale: 50.0,
                };
                show(
                    ui,
                    rect,
                    doc,
                    view,
                    PaneKind::Plan { level },
                    &d,
                    state,
                    sel,
                    style,
                );
            });
            out.textures_delta.clear();
        };
        // the probe's face (y = 0.1) slides square to the wall
        let face = xf.to_screen(DVec2::new(32.0, 0.1));
        frame(
            vec![egui::Event::PointerMoved(face)],
            &mut doc,
            &mut state,
            &mut sel,
        );
        frame(
            vec![egui::Event::PointerMoved(face)],
            &mut doc,
            &mut state,
            &mut sel,
        );
        frame(
            vec![egui::Event::PointerButton {
                pos: face,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
            &mut doc,
            &mut state,
            &mut sel,
        );
        // to 2.7 px past the other wall's face at y = 0.9
        let target = xf.to_screen(DVec2::new(32.0, 0.927));
        for k in 1..=10 {
            let p = face + (target - face) * (k as f32 / 10.0);
            frame(
                vec![egui::Event::PointerMoved(p)],
                &mut doc,
                &mut state,
                &mut sel,
            );
        }
        let (start, end, t) = match &doc.model().get(id).unwrap().kind {
            ElementKind::Wall(w) => (w.start, w.end, w.thickness),
            _ => unreachable!(),
        };
        // the dragged face, whichever way the wall grew, is on y = 0.9
        let mid = (start + end) / 2.0;
        let face_y = mid.y + t / 2.0;
        assert!(
            (face_y - 0.9).abs() < 1e-6,
            "the face is at y = {face_y} (thickness {t}), not on the other wall's face at 0.9"
        );
    }

    #[test]
    fn guide_lines_go_when_nothing_uses_them() {
        let view = ElementId::new();
        let edge = snaps::Reference::Edge {
            a: DVec3::ZERO,
            b: DVec3::X,
            arc: None,
        };
        let mut state = PlanState::default();
        rest_reference(&mut state, view, Some(edge), 0.0, false);
        assert!(rest_reference(&mut state, view, Some(edge), 1.0, false).is_some());
        fade_references(&mut state, view, 2.0, false);
        assert_eq!(state.refs[&view].len(), 1);
        // a snap following them keeps them
        fade_references(&mut state, view, 6.5, true);
        fade_references(&mut state, view, 10.0, false);
        assert_eq!(state.refs[&view].len(), 1);
        fade_references(&mut state, view, 6.5 + REF_FADE + 0.1, false);
        assert!(!state.refs.contains_key(&view));
    }

    #[test]
    fn resting_after_an_align_click_keeps_its_message() {
        let view = ElementId::new();
        let edge = snaps::Reference::Edge {
            a: DVec3::ZERO,
            b: DVec3::X,
            arc: None,
        };
        // resting on an edge makes guides from it, with a message
        let mut state = PlanState::default();
        assert!(rest_reference(&mut state, view, Some(edge), 0.0, false).is_none());
        assert!(rest_reference(&mut state, view, Some(edge), 1.0, false).is_some());
        // but not right after the Align tool clicked it
        let mut state = PlanState::default();
        click_is_not_a_rest(&mut state, Tool::Align, true, Some(edge), 0.0);
        assert!(rest_reference(&mut state, view, Some(edge), 0.0, false).is_none());
        assert!(rest_reference(&mut state, view, Some(edge), 5.0, false).is_none());
        assert!(state.refs.get(&view).is_none_or(|r| r.is_empty()));
        // other tools still rest where they clicked
        let mut state = PlanState::default();
        click_is_not_a_rest(&mut state, Tool::Wall, true, Some(edge), 0.0);
        assert!(rest_reference(&mut state, view, Some(edge), 0.0, false).is_none());
        assert!(rest_reference(&mut state, view, Some(edge), 1.0, false).is_some());
    }

    use super::*;

    /// Drive a plan pane with pointer events, frame by frame.
    fn drive_box(kind_section: bool, decorations: bool, two: bool) -> usize {
        let m = creabim_core::starter::starter_house();
        let (view, kind) = m
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::View(v) if !kind_section && e.name == "Ground Floor plan" => {
                    Some((e.id, pane_kind(v)))
                }
                ElementKind::View(v) if kind_section && e.name == "Section A-A" => {
                    Some((e.id, pane_kind(v)))
                }
                _ => None,
            })
            .unwrap();
        let mut doc = creabim_core::Document::from_model(m);
        let ctx = egui::Context::default();
        let mut state = PlanState::default();
        let mut sel = crate::selection::Selection::default();
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(1200.0, 900.0));
        let d = creabim_geom::section::view_drawing_with(doc.model(), view, false).unwrap();
        let mut t = 0.0;
        let mut frame = |events: Vec<egui::Event>,
                         doc: &mut creabim_core::Document,
                         state: &mut PlanState,
                         sel: &mut crate::selection::Selection| {
            t += 1.0 / 60.0;
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(2400.0, 900.0))),
                events,
                time: Some(t),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                let style = PaneStyle {
                    background: None,
                    decorations,
                    camera: None,
                    scale: 50.0,
                };
                ui.push_id(("area", 0), |ui| {
                    show(ui, rect, doc, view, kind, &d, state, sel, style)
                });
                // a second area showing the same view, beside it (MU21)
                if two {
                    let other = rect.translate(egui::vec2(rect.width(), 0.0));
                    ui.push_id(("area", 1), |ui| {
                        show(ui, other, doc, view, kind, &d, state, sel, style)
                    });
                }
            });
            out.textures_delta.clear();
        };
        // over the middle of the drawing, then a drag of 300 x 200 px
        let a = Pos2::new(700.0, 550.0);
        let b = Pos2::new(400.0, 350.0);
        for e in [
            vec![egui::Event::PointerMoved(a)],
            vec![egui::Event::PointerMoved(a)],
            vec![egui::Event::PointerButton {
                pos: a,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
        ] {
            frame(e, &mut doc, &mut state, &mut sel);
        }
        for k in 1..=10 {
            let p = a + (b - a) * (k as f32 / 10.0);
            frame(
                vec![egui::Event::PointerMoved(p)],
                &mut doc,
                &mut state,
                &mut sel,
            );
        }
        frame(
            vec![egui::Event::PointerButton {
                pos: b,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            &mut doc,
            &mut state,
            &mut sel,
        );
        frame(
            vec![egui::Event::PointerMoved(b)],
            &mut doc,
            &mut state,
            &mut sel,
        );
        sel.len()
    }

    #[test]
    fn a_left_drag_on_a_plan_with_nothing_selected_selects_with_a_box() {
        assert!(
            drive_box(false, false, false) > 0,
            "the box took nothing in the plan"
        );
        assert!(drive_box(false, true, false) > 0, "with decorations");
        assert!(drive_box(true, true, false) > 0, "in a section");
        // two areas showing the same plan: the other one used to end the
        // box the moment it started (MU21)
        assert!(
            drive_box(false, true, true) > 0,
            "with a second area on the same plan"
        );
    }

    #[test]
    fn a_crop_box_shows_as_its_footprint_in_a_plan_and_its_extent_in_a_section() {
        let m = creabim_core::sample::sample_house();
        let views: Vec<(ElementId, bool)> = m
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::View(v) => match v.kind {
                    creabim_core::ViewKind::Plan { .. } => Some((e.id, true)),
                    creabim_core::ViewKind::Section { .. } => Some((e.id, false)),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        let crop = creabim_core::CropBox {
            view: None,
            min: DVec3::new(1.0, 2.0, 0.5),
            max: DVec3::new(4.0, 6.0, 3.0),
            scope: Default::default(),
            enabled: true,
            frame_hidden: false,
            except: vec![],
            outside: Default::default(),
            fit: None,
        };
        let e = creabim_core::Element::new("Crop", ElementKind::CropBox(crop));
        let id = e.id;
        let mut doc = Document::from_model(m);
        doc.commit(Transaction::new("crop").insert(e)).unwrap();
        let plan = views.iter().find(|v| v.1).unwrap().0;
        let r = crop_rects(doc.model(), plan);
        let (_, lo, hi) = r.iter().find(|r| r.0 == id).unwrap();
        assert_eq!((*lo, *hi), (DVec2::new(1.0, 2.0), DVec2::new(4.0, 6.0)));
        let section = views.iter().find(|v| !v.1).unwrap().0;
        let (_, lo, hi) = crop_rects(doc.model(), section)
            .into_iter()
            .find(|r| r.0 == id)
            .unwrap();
        assert!(
            (lo.y - 0.5).abs() < 1e-9 && (hi.y - 3.0).abs() < 1e-9,
            "{lo} {hi}"
        );
    }
    use creabim_core::{EditMesh, FreeMesh};

    #[test]
    fn a_view_marks_parts_are_traced_not_boxed() {
        // every part of a bubble with its wedge is traced by a hover
        let id = ElementId::new();
        let items =
            creabim_geom::markers::bubble(id, DVec2::ZERO, "A", "A-201", Some(DVec2::X), 0.05);
        assert!(items.len() >= 4);
        for it in &items {
            assert!(is_mark(it), "{it:?} would be washed, not traced");
        }
        // a wall's cut linework and fill are not marks
        let wall = PlanItem::Line {
            id,
            pen: Pen::Cut,
            a: DVec2::ZERO,
            b: DVec2::X,
        };
        assert!(!is_mark(&wall));
    }

    #[test]
    fn a_click_in_a_doors_opening_takes_the_door_not_the_footing_below() {
        let (door, footing) = (ElementId::new(), ElementId::new());
        let items = vec![
            PlanItem::Line {
                id: footing,
                pen: Pen::Hidden,
                a: DVec2::new(-3.0, 0.0),
                b: DVec2::new(3.0, 0.0),
            },
            PlanItem::Line {
                id: door,
                pen: Pen::Symbol,
                a: DVec2::new(0.0, 0.0),
                b: DVec2::new(0.0, 0.9),
            },
            PlanItem::Arc {
                id: door,
                pen: Pen::Symbol,
                center: DVec2::ZERO,
                radius: 0.9,
                start: 0.0,
                end: std::f64::consts::FRAC_PI_2,
            },
        ];
        let hits = hit_all(&items, DVec2::new(0.45, 0.0), 0.05, 0.05);
        assert_eq!(hits, [door, footing]);
        // away from the door the footing is still picked
        assert_eq!(
            hit_test(&items, DVec2::new(2.0, 0.0), 0.05, 0.05),
            Some(footing)
        );
    }

    #[test]
    fn tools_draw_new_elements_as_their_type() {
        let mut doc = Document::from_model(creabim_core::sample::sample_house());
        let (tx, ids) = creabim_core::types::add_starters(doc.model());
        doc.commit(tx).unwrap();
        let level = doc.model().levels().next().unwrap().0.id;
        let storefront = ids[5];
        let window = ids[9];
        let mut state = PlanState::default();
        state
            .types
            .insert(creabim_core::TypeCategory::Wall, storefront);
        state
            .types
            .insert(creabim_core::TypeCategory::Window, window);
        let mut sel = Selection::default();
        let (a, b) = (DVec2::new(0.0, -3.0), DVec2::new(6.0, -3.0));
        plan_click(&mut doc, level, Tool::Wall, a, &mut state, &mut sel).unwrap();
        plan_click(&mut doc, level, Tool::Wall, b, &mut state, &mut sel).unwrap();
        let wall = sel.active.unwrap();
        let e = doc.model().get(wall).unwrap();
        assert_eq!(e.of_type.as_ref().map(|r| r.id), Some(storefront));
        let ElementKind::Wall(w) = &e.kind else {
            panic!()
        };
        assert!(w.curtain.is_some(), "drawn as a curtain wall");
        // a window clicked into a plain wall takes its type's size
        let mut sel = Selection::default();
        plan_click(
            &mut doc,
            level,
            Tool::Window,
            DVec2::new(5.0, 6.0),
            &mut state,
            &mut sel,
        )
        .unwrap();
        let e = doc.model().get(sel.active.unwrap()).unwrap();
        assert_eq!(e.of_type.as_ref().map(|r| r.id), Some(window));
        // and the opening tool cuts a bare hole, with no type
        let mut sel = Selection::default();
        plan_click(
            &mut doc,
            level,
            Tool::Opening,
            DVec2::new(10.0, 4.5),
            &mut state,
            &mut sel,
        )
        .unwrap();
        let e = doc.model().get(sel.active.unwrap()).unwrap();
        assert!(matches!(&e.kind, ElementKind::Opening(o) if o.kind == OpeningKind::Hole));
        assert!(e.of_type.is_none());
    }

    /// A dimension's second point locks onto the first point's
    /// horizontal and vertical, as a wall's end does (A7-11).
    #[test]
    fn a_dimension_snaps_on_the_axes_of_its_first_point() {
        let doc = Document::from_model(creabim_core::sample::sample_house());
        let v = doc
            .model()
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::View(v) if matches!(v.kind, ViewKind::Plan { .. }) => Some(v.clone()),
                _ => None,
            })
            .unwrap();
        // far from the house, so nothing else is under the pointer
        let first = DVec2::new(40.0, 40.0);
        let xf = Xf {
            rect: Rect::from_min_size(Pos2::ZERO, Vec2::splat(1000.0)),
            center: first,
            scale: 100.0,
        };
        let snapper = Snapper::from_drawing(&[], &[]);
        let cfg = snaps::SnapSettings::default();
        for (aim, want) in [
            (DVec2::new(43.0, 40.05), DVec2::new(43.0, 40.0)),
            (DVec2::new(39.96, 37.0), DVec2::new(40.0, 37.0)),
        ] {
            let (_, at, kind, _) = dim_snap_guided(
                &doc,
                &v,
                pane_kind(&v),
                Some(&snapper),
                &cfg,
                &xf,
                xf.to_screen(aim),
                &[],
                Some(first.extend(0.0)),
                &[],
            );
            assert!(
                at.distance(want) < 1e-6,
                "aimed at {aim}, got {at} ({kind:?}), wanted {want}"
            );
        }
    }

    #[test]
    fn a_dimension_attaches_to_a_mass_corner_and_follows_it() {
        let mut doc = Document::from_model(creabim_core::sample::sample_house());
        let (view, v) = doc
            .model()
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::View(v) if matches!(v.kind, ViewKind::Plan { .. }) => {
                    Some((e.id, v.clone()))
                }
                _ => None,
            })
            .unwrap();
        let ViewKind::Plan { level, .. } = v.kind else {
            unreachable!()
        };
        let mass = Element::new(
            "Mass",
            ElementKind::FreeMesh(FreeMesh::new(
                Some(level),
                DVec3::new(20.0, 5.0, 0.0),
                EditMesh::cuboid(DVec3::new(2.0, 4.0, 3.0)),
            )),
        );
        let id = mass.id;
        doc.commit(Transaction::new("Add mass").insert(mass))
            .unwrap();
        let drawing = creabim_geom::section::view_drawing(doc.model(), view).unwrap();
        let snapper = Snapper::from_drawing(&drawing.items, &[]);
        let xf = Xf {
            rect: Rect::from_min_size(Pos2::ZERO, Vec2::splat(1000.0)),
            center: DVec2::new(20.0, 5.0),
            scale: 100.0,
        };
        let cursor = xf.to_screen(DVec2::new(21.03, 6.98));
        let cfg = snaps::SnapSettings::default();
        let (hp, at, kind) = dim_snap(&doc, &v, pane_kind(&v), Some(&snapper), &cfg, &xf, cursor);
        assert_eq!(kind, Some(SnapKind::Endpoint));
        assert!(at.distance(DVec2::new(21.0, 7.0)) < 1e-9);
        assert_eq!(hp.host, Some(id));
        // move the mass: the point goes with it
        doc.edit(id, "Move mass", |e| {
            if let ElementKind::FreeMesh(m) = &mut e.kind {
                m.origin += DVec3::new(1.5, -1.0, 0.0);
            }
        })
        .unwrap();
        let now = annotate::resolve(doc.model(), &v, &hp).unwrap();
        assert!(now.distance(DVec2::new(22.5, 6.0)) < 1e-9, "{now}");
        // with snapping off it is a free point
        let off = cfg.live(true);
        let (hp, ..) = dim_snap(&doc, &v, pane_kind(&v), Some(&snapper), &off, &xf, cursor);
        assert_eq!(hp.host, None);
    }

    #[test]
    fn the_beam_inside_a_wall_is_under_the_pointer_in_a_section_and_first_in_x_ray() {
        let (model, beam) = creabim_core::sample::sample_with_buried_beam();
        let doc = Document::from_model(model);
        let sec = doc
            .model()
            .iter()
            .find(|e| e.name == "Section A")
            .unwrap()
            .id;
        let wall = doc
            .model()
            .iter()
            .find(|e| e.name == "GF Wall 1")
            .unwrap()
            .id;
        let Some(ElementKind::View(v)) = doc.model().get(sec).map(|e| &e.kind) else {
            panic!("Section A is a view")
        };
        let kind = pane_kind(v);
        let drawing = creabim_geom::section::view_drawing(doc.model(), sec).unwrap();
        let mut state = PlanState::default();
        // Section A runs north from y = -5: the wall is 5 m along it; the
        // beam sits 2.3 to 2.6 m up inside it
        let p = DVec2::new(5.0, 2.45);
        let under = under_pointer(&mut state, &doc, sec, kind, &drawing, p, 0.02, 0.1);
        assert_eq!(under.first(), Some(&wall), "a plain click takes the wall");
        assert!(
            under.contains(&beam),
            "the beam is found under it: {under:?}"
        );
        // what a click takes without Tab is what it always took
        assert_eq!(
            hit_test(&drawing.items, p, 0.02, 0.1).or_else(|| ray_pick(&doc, sec, kind, p)),
            Some(wall)
        );
        state.xray.insert(sec);
        let xray = creabim_geom::section::view_drawing_with(doc.model(), sec, true).unwrap();
        let under = under_pointer(&mut state, &doc, sec, kind, &xray, p, 0.02, 0.1);
        assert_eq!(under.first(), Some(&beam), "in X-ray the beam comes first");
        assert!(under.contains(&wall));
    }

    #[test]
    fn what_a_3d_views_crop_box_cuts_away_cannot_be_picked_in_it() {
        let mut doc = Document::from_model(creabim_core::sample::sample_house());
        let wall = doc
            .model()
            .iter()
            .find(|e| e.name == "GF Wall 1")
            .unwrap()
            .id;
        let axo = Element::new(
            "Axo",
            ElementKind::View(creabim_core::View::axonometric(45.0, 30.0)),
        );
        let view = axo.id;
        doc.commit(Transaction::new("view").insert(axo)).unwrap();
        assert!(pick_scene(doc.model(), view).iter().any(|s| s.id == wall));
        // a box over the north half of the house: the south wall (y = 0)
        // is cut away
        let b = creabim_core::CropBox::new(
            Some(view),
            DVec3::new(-1.0, 3.0, -1.0),
            DVec3::new(11.0, 7.0, 7.0),
        );
        doc.commit(Transaction::new("crop").insert(Element::new("Box", ElementKind::CropBox(b))))
            .unwrap();
        let scene = pick_scene(doc.model(), view);
        assert!(
            !scene.iter().any(|s| s.id == wall),
            "cut away, not pickable"
        );
        assert!(!scene.is_empty(), "what is inside the box still is");
    }

    #[test]
    fn in_an_elevation_the_slab_behind_a_wall_is_under_the_pointer_too() {
        let doc = Document::from_model(creabim_core::sample::sample_house());
        let named = |n: &str| doc.model().iter().find(|e| e.name == n).unwrap().id;
        let (elev, wall, slab) = (
            named("South Elevation S"),
            named("GF Wall 1"),
            named("FF Slab"),
        );
        let Some(ElementKind::View(v)) = doc.model().get(elev).map(|e| &e.kind) else {
            panic!("the elevation is a view")
        };
        let kind = pane_kind(v);
        let drawing = creabim_geom::section::view_drawing(doc.model(), elev).unwrap();
        let mut state = PlanState::default();
        // the elevation runs east from x = -2: 7 m along is the middle of
        // the south wall; the first floor slab is 2.75 to 3 m up behind it
        let p = DVec2::new(7.0, 2.9);
        let under = under_pointer(&mut state, &doc, elev, kind, &drawing, p, 0.02, 0.1);
        assert_eq!(under.first(), Some(&wall), "a plain click takes the wall");
        assert!(under.contains(&slab), "Tab reaches the slab: {under:?}");
        // and in X-ray the slab, though it holds others too, is still there
        state.xray.insert(elev);
        let xray = creabim_geom::section::view_drawing_with(doc.model(), elev, true).unwrap();
        let under = under_pointer(&mut state, &doc, elev, kind, &xray, p, 0.02, 0.1);
        assert!(under.contains(&slab) && under.contains(&wall));
    }

    #[test]
    fn tag_fixtures_tags_services_fixtures_once_and_leaves_furniture() {
        let mut doc = Document::from_model(creabim_core::sample::furnished_house());
        let plan = doc
            .model()
            .iter()
            .find(|e| e.name == "Ground Floor plan")
            .unwrap()
            .id;
        let n = tag_fixtures(&mut doc, plan).unwrap();
        assert!(
            n > 10,
            "the ground floor has its sockets, switches and lights"
        );
        let model = doc.model();
        for e in model.iter() {
            let ElementKind::Tag(t) = &e.kind else {
                continue;
            };
            if t.view != plan {
                continue;
            }
            let host = model.get(t.host).unwrap();
            if let Some(ty) = creabim_core::objects::type_of(model, host) {
                assert!(ty.category.is_services(), "{} is not a fixture", host.name);
            }
        }
        // a second run finds nothing left to tag
        assert_eq!(tag_fixtures(&mut doc, plan).unwrap(), 0);
    }
}

/// A selected grid line's ends in a section: square grips at its foot and
/// top, dragged up and down; the heights are this view's own (MU70).
#[allow(clippy::too_many_arguments)]
fn drag_grid_ends_section(
    doc: &mut Document,
    view: ElementId,
    plan: &creabim_geom::plan::PlanView,
    painter: &egui::Painter,
    xf: &Xf,
    response: &egui::Response,
    state: &mut PlanState,
    selection: &Selection,
) -> Option<Option<String>> {
    // every grid line this section draws, where it stands
    let all: Vec<(ElementId, f64, f64, f64)> = plan
        .items
        .iter()
        .filter_map(|it| match it {
            PlanItem::Line {
                id,
                pen: creabim_geom::plan::Pen::Grid,
                a,
                b,
            } => Some((*id, a.x, a.y.min(b.y), a.y.max(b.y))),
            _ => None,
        })
        .fold(Vec::new(), |mut acc: Vec<(ElementId, f64, f64, f64)>, l| {
            // the line itself, not the short leaders into its bubbles
            // (MU74): the longest of each grid's
            match acc.iter_mut().find(|k| k.0 == l.0) {
                Some(k) if l.3 - l.2 > k.3 - k.2 => *k = l,
                Some(_) => {}
                None => acc.push(l),
            }
            acc
        });
    // the selected ones carry grips
    let lines: Vec<(ElementId, f64, f64, f64)> = all
        .iter()
        .copied()
        .filter(|(id, ..)| selection.contains(*id))
        .collect();
    if lines.is_empty() && state.grid_drag.is_none() {
        return None;
    }
    let accent = crate::theme::interaction();
    for (_, x, lo, hi) in &lines {
        for y in [*lo, *hi] {
            painter.rect(
                Rect::from_center_size(xf.to_screen(DVec2::new(*x, y)), Vec2::splat(9.0)),
                0.0,
                Color32::WHITE,
                Stroke::new(1.5, accent),
                egui::StrokeKind::Middle,
            );
        }
    }
    if response.drag_started_by(egui::PointerButton::Primary) {
        let p = painter
            .ctx()
            .input(|i| i.pointer.press_origin())
            .or(response.interact_pointer_pos())?;
        state.grid_drag = lines.iter().find_map(|(id, x, lo, hi)| {
            let near = |y: f64| xf.to_screen(DVec2::new(*x, y)).distance(p) < 10.0;
            if near(*hi) {
                Some((*id, true, *lo, *hi))
            } else if near(*lo) {
                Some((*id, false, *lo, *hi))
            } else {
                None
            }
        });
        if state.grid_drag.is_some() {
            state.drag_owner = Some(response.id);
        }
    }
    let (id, top, lo, hi) = state.grid_drag?;
    if state.drag_owner != Some(response.id) {
        return None;
    }
    if response.drag_stopped() || !response.dragged() {
        // saved once, as the button comes up
        state.grid_drag = None;
        let Some((lo, hi)) = state.grid_drag_now.take() else {
            return Some(None);
        };
        let r = doc.edit_merged(view, "Grid line height in this view", |e| {
            if let ElementKind::View(v) = &mut e.kind {
                v.grid_ends.insert(id, [lo, hi]);
            }
        });
        return Some(r.err().map(|e| e.to_string()));
    }
    let Some(x) = all.iter().find(|l| l.0 == id).map(|l| l.1) else {
        return Some(None);
    };
    let ptr = response.interact_pointer_pos()?;
    let mut y = xf.to_world(ptr).y;
    // the other grid lines' ends close by on screen: it lands level with
    // them, a dashed guide across shows which (MU72)
    let mut along: Option<f64> = None;
    let mut best = 8.0f32;
    for (oid, ox, olo, ohi) in &all {
        if *oid == id {
            continue;
        }
        for oy in [*olo, *ohi] {
            let d = (xf.to_screen(DVec2::new(*ox, oy)).y - ptr.y).abs();
            if d < best {
                best = d;
                y = oy;
                along = Some(*ox);
            }
        }
    }
    if along.is_none() {
        y = (y / 0.05).round() * 0.05;
    }
    let (lo, hi) = if top {
        (lo, y.max(lo + 0.5))
    } else {
        (y.min(hi - 0.5), hi)
    };
    state.grid_drag_now = Some((lo, hi));
    // the line as it will be, drawn live: nothing is saved yet
    let accent_c = accent;
    painter.extend(egui::Shape::dashed_line(
        &[
            xf.to_screen(DVec2::new(x, lo)),
            xf.to_screen(DVec2::new(x, hi)),
        ],
        Stroke::new(2.0, accent_c),
        10.0,
        4.0,
    ));
    let end = xf.to_screen(DVec2::new(x, if top { hi } else { lo }));
    painter.rect(
        Rect::from_center_size(end, Vec2::splat(9.0)),
        0.0,
        accent_c,
        Stroke::new(1.5, Color32::WHITE),
        egui::StrokeKind::Middle,
    );
    if let Some(ox) = along {
        let o = xf.to_screen(DVec2::new(ox, y));
        painter.extend(egui::Shape::dashed_line(
            &[o, end],
            Stroke::new(1.0, Color32::from_rgb(78, 123, 196)),
            5.0,
            4.0,
        ));
        painter.circle_stroke(o, 4.0, Stroke::new(1.2, Color32::from_rgb(78, 123, 196)));
    }
    painter.ctx().request_repaint();
    state.status = Some(format!(
        "Grid line {}: {}. Let go to set it; it lines up with the other grid ends.",
        if top { "top" } else { "foot" },
        crate::units::len(if top { hi } else { lo })
    ));
    Some(None)
}

/// The guide whose line passes within `tol` of `p`, as its two points in
/// plan; the nearest of several.
fn guide_near(model: &creabim_core::Model, p: DVec2, tol: f64) -> Option<(DVec2, DVec2)> {
    model
        .iter()
        .filter_map(|e| match &e.kind {
            ElementKind::Guide(g) => {
                let (a, b) = (g.a.truncate(), g.b.truncate());
                let d = (b - a).try_normalize()?;
                let off = (p - a).perp_dot(d).abs();
                (off < tol).then_some((off, (a, b)))
            }
            _ => None,
        })
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .map(|x| x.1)
}

#[cfg(test)]
mod outline_tests {
    use super::*;

    #[test]
    fn an_outline_that_turns_back_stays_where_it_is() {
        // a well's outline round a spiral: it runs out and comes straight
        // back along itself, which a mitred closed line throws far away
        let pts = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(2.0, 0.0),
            DVec2::new(2.0, 2.0),
            DVec2::new(1.0, 2.0),
            DVec2::new(1.0, 1.0),
            DVec2::new(1.000_001, 2.0),
            DVec2::new(0.0, 2.0),
        ];
        let item = PlanItem::Polygon {
            id: ElementId::new(),
            pen: Pen::Symbol,
            points: pts,
        };
        let xf = Xf {
            rect: Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0)),
            center: DVec2::new(1.0, 1.0),
            scale: 100.0,
        };
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(xf.rect),
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            let p = ui.ctx().layer_painter(egui::LayerId::background());
            draw_item(&p, &xf, &item, false, 0.05);
        });
        out.textures_delta.clear();
        let room = Rect::from_two_pos(
            xf.to_screen(DVec2::ZERO),
            xf.to_screen(DVec2::new(2.0, 2.0)),
        )
        .expand(10.0);
        for cp in ctx.tessellate(out.shapes, 1.0) {
            if let egui::epaint::Primitive::Mesh(mesh) = cp.primitive {
                for v in &mesh.vertices {
                    assert!(room.contains(v.pos), "{:?} is outside {room:?}", v.pos);
                }
            }
        }
    }
}

#[cfg(test)]
mod pick_order_tests {
    use super::*;

    #[test]
    fn the_smallest_filled_shape_under_the_pointer_is_taken_first() {
        let sq = |x0: f64, x1: f64| {
            vec![
                DVec2::new(x0, x0),
                DVec2::new(x1, x0),
                DVec2::new(x1, x1),
                DVec2::new(x0, x1),
            ]
        };
        let (ground, pad, path) = (ElementId::new(), ElementId::new(), ElementId::new());
        let shade = |id: ElementId, points: Vec<DVec2>| PlanItem::Shade {
            id,
            points,
            rgb: [0.8; 3],
        };
        // painted ground, path, pad: the pad last, so over the path
        let items = vec![
            shade(ground, sq(-50.0, 50.0)),
            shade(path, sq(-1.0, 1.0)),
            shade(pad, sq(-10.0, 10.0)),
        ];
        let p = DVec2::new(0.2, 0.3);
        assert_eq!(hit_all(&items, p, 0.05, 0.1), [path, pad, ground]);
        // and the same whatever order they are painted in
        let items: Vec<PlanItem> = items.into_iter().rev().collect();
        assert_eq!(hit_all(&items, p, 0.05, 0.1), [path, pad, ground]);
        // outside the path, the pad
        assert_eq!(
            hit_all(&items, DVec2::new(5.0, 5.0), 0.05, 0.1),
            [pad, ground]
        );
    }
}
