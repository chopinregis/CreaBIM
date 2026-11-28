//! The app's side of the web interface: what each pane shows, the menus,
//! and what the user's clicks there do. Menus are built as data here, so
//! the classic egui menu bar draws the same items.

use super::*;
use crate::theme::Theme;
use crate::ui_model::{self, Item};
use crate::webui::{Msg, Pane, WebUi};
use serde_json::{json, Value};

/// Height of the top bar (menus, tools and the tool options strip) and the
/// status bar, in points.
pub(super) const TOP_H: f32 = 105.0;
pub(super) const STATUS_H: f32 = 26.0;
/// Size of the 3D view's navigation buttons.
const NAV_W: f32 = 420.0;
const NAV_H: f32 = 34.0;

thread_local! {
    static LEFT_CACHE: std::cell::RefCell<Option<(String, usize, Value)>> =
        const { std::cell::RefCell::new(None) };
}

pub(super) struct Shell {
    pub web: Option<WebUi>,
    #[cfg(target_os = "macos")]
    mac_drag: Option<crate::mac_drag::MacDrag>,
    #[cfg(target_os = "macos")]
    mac_titlebar: Option<crate::mac_drag::Layout>,
    #[cfg(target_os = "macos")]
    mac_titlebar_revision: u64,
    #[cfg(target_os = "macos")]
    mac_titlebar_rendered: Option<u64>,
    /// Why the web interface couldn't start, when it couldn't.
    pub failed: Option<String>,
    /// The classic egui panels, by choice.
    pub classic: bool,
    pub theme: Theme,
    /// The theme and panel widths, kept between sessions.
    pub settings: crate::files::Settings,
    popup: Option<Popup>,
    /// A card waiting to come on: its key, the frames drawn since it was
    /// asked for (or since the window came back), whether its page has
    /// drawn it, when it was asked, and the earliest it may come.
    card_gate: Option<(String, u32, bool, std::time::Instant, std::time::Instant)>,
    /// A web row's hint to show as the app's own themed tooltip (MU26):
    /// its text and the control it is for, in window points.
    web_tip: Option<(String, egui::Rect)>,
    /// The size the tip's page reported for the tip it shows, in points.
    tip_size: Option<(String, egui::Vec2)>,
    /// Keys pressed while a page had the keyboard, for the views.
    pub keys: Vec<egui::Event>,
    /// Ctrl, Shift or Alt came with the keys fed last frame, and are to
    /// be let go of now.
    keys_held: bool,
    /// A right-click menu was asked for from the 3D view this frame.
    pub menu_3d: bool,
    /// The open right-click menu came from the 3D view (else a 2D pane).
    pub(super) context_3d: bool,
    /// Give the popup the keyboard once it is shown (the palette).
    focus_popup: bool,
    /// The ground grid spacing being typed, while its window is open.
    pub ground_custom: Option<String>,
    /// The Help panel: open or not, and its page.
    pub help: super::helpui::HelpState,
    /// The tool options strip as its page last measured it, for what goes
    /// in its More menu.
    pub bar: Option<crate::optionbar::Measure>,
    /// The tool the tools row last showed its tab for: a tool picked by
    /// its key, off the tab shown, brings its own tab.
    tab_tool: Option<Tool>,
    /// Create › Door or Window: the strip opens its picker once drawn.
    pub(super) open_kinds: bool,
}

impl Default for Shell {
    fn default() -> Self {
        Self {
            web: None,
            #[cfg(target_os = "macos")]
            mac_drag: None,
            #[cfg(target_os = "macos")]
            mac_titlebar: None,
            #[cfg(target_os = "macos")]
            mac_titlebar_revision: 0,
            #[cfg(target_os = "macos")]
            mac_titlebar_rendered: None,
            failed: None,
            classic: std::env::var_os("CREABIM_CLASSIC_UI").is_some(),
            theme: Theme::default(),
            settings: Default::default(),
            popup: None,
            card_gate: None,
            web_tip: None,
            tip_size: None,
            keys: vec![],
            keys_held: false,
            menu_3d: false,
            context_3d: false,
            focus_popup: false,
            ground_custom: None,
            help: Default::default(),
            bar: None,
            tab_tool: None,
            open_kinds: false,
        }
    }
}

impl Shell {
    /// Is the New project window (the web one) or Save as template's card open?
    pub(super) fn new_project_open(&self) -> bool {
        self.popup
            .as_ref()
            .is_some_and(|p| p.key == "newproject" || p.key == TEMPLATE_KEY)
    }

    /// Take a bar's tooltip away (MU26).
    pub fn drop_web_tip(&mut self) {
        self.web_tip = None;
    }

    /// Feed the keys pressed while a page had the keyboard to the views.
    /// egui keeps Ctrl, Shift and Alt from `ModifiersChanged` events, not
    /// from each key, and the window saw none of them go down while the
    /// page had the keyboard. So the keys' own modifiers are set for the
    /// frame they arrive in and let go of in the next: otherwise Ctrl+S
    /// over a view reads as S, which scales. True when a next frame is
    /// needed to let go of them.
    pub fn feed_keys(&mut self, raw: &mut egui::RawInput) -> bool {
        if std::mem::take(&mut self.keys_held) {
            // first, so a real change this frame still wins
            raw.events
                .insert(0, egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        }
        let held = self.keys.iter().find_map(|e| match e {
            egui::Event::Key { modifiers, .. } if !modifiers.is_none() => Some(*modifiers),
            _ => None,
        });
        if let Some(m) = held {
            raw.events.push(egui::Event::ModifiersChanged(m));
            self.keys_held = true;
        }
        raw.events.append(&mut self.keys);
        self.keys_held
    }
}

#[derive(Clone)]
struct Popup {
    key: String,
    /// Top-left corner, in points.
    at: egui::Pos2,
    /// When the popup opens upwards, where its bottom goes.
    up: Option<f32>,
    /// The top of the button it opened from: a menu with no room below it
    /// opens upwards from there instead.
    above: Option<f32>,
    size: egui::Vec2,
}

pub(super) fn view_style_icon(s: ViewStyle) -> &'static str {
    match s {
        ViewStyle::HiddenLine => "render-hidden-line",
        ViewStyle::Shaded => "render-edges",
        ViewStyle::Wireframe => "render-wireframe",
    }
}

pub(crate) fn tool_icon(t: Tool) -> &'static str {
    match t {
        Tool::Select => "mouse-pointer-2",
        Tool::Wall => "wall",
        Tool::Door => "door",
        Tool::Window => "window",
        Tool::Opening => "opening",
        Tool::Slab => "slab",
        Tool::Shaft => "shaft",
        Tool::Mass => "box",
        Tool::Section => "scissors-line-dashed",
        Tool::Elevation => "elevation",
        Tool::Dimension => "dimension",
        Tool::Tag => "tag",
        Tool::Angle => "angle",
        Tool::Radius => "radius",
        Tool::Diameter => "diameter",
        Tool::ArcLength => "arc-length",
        Tool::Note => "message-square-text",
        Tool::Symbol => "crosshair",
        Tool::SpotLevel => "spot-level",
        Tool::Sketch => "pen-tool",
        Tool::Guide => "guide",
        Tool::Grid => "grid-lines",
        Tool::Column => "column",
        Tool::Beam => "beam",
        Tool::Roof => "roof",
        Tool::Stair => "stair",
        Tool::Railing => "railing",
        Tool::Footing => "footing",
        Tool::Level => "level",
        Tool::Align => "align-start-vertical",
        Tool::Object => "object",
        Tool::Extrusion => "extrusion",
        Tool::Sweep => "sweep",
        Tool::Void => "void",
        Tool::Terrain => "terrain",
        Tool::Pad => "pad",
        Tool::Paving => "paving",
        Tool::Planting => "planting",
        Tool::Room => "room",
        Tool::RoomSeparator => "room-separator",
        Tool::Ceiling => "ceiling",
        Tool::Callout => "callout",
        Tool::Detail => "detail-component",
    }
}

pub(super) fn icon_name(i: Icon) -> &'static str {
    match i {
        Icon::Storey => "layers-3",
        Icon::Grid => "grid-lines",
        Icon::Wall => "wall",
        Icon::Slab => "slab",
        Icon::Door => "door",
        Icon::Window => "window",
        Icon::Roof => "roof",
        Icon::Column => "column",
        Icon::Beam => "beam",
        Icon::Mass => "box",
        Icon::Material => "palette",
        Icon::Plan => "map",
        Icon::CeilingPlan => "ceiling",
        Icon::Section => "scissors-line-dashed",
        Icon::Elevation => "elevation",
        Icon::View3d => "view-3d",
        Icon::Schedule => "table-2",
        Icon::Sheet => "file-text",
        Icon::Viewport => "frame",
        Icon::Dimension => "dimension",
        Icon::Tag => "tag",
        Icon::Sketch => "pen-tool",
        Icon::Guide => "guide",
        Icon::CropBox => "crop-box",
    }
}

/// A plain name for a group of the project browser.
/// The project browser's group an element is listed in: every kind of
/// type in one, the rest by IFC class.
fn class_of(e: &Element) -> String {
    match e.kind {
        ElementKind::ElementType(_) => "ElementType".to_string(),
        // a terrain with what shapes it: its earthworks, spot levels and
        // lines, listed under it (site rework, stage 5)
        ElementKind::Site(ref s)
            if matches!(
                s.form,
                creabim_core::SiteForm::Terrain { .. } | creabim_core::SiteForm::Pad { .. }
            ) || s.is_ground_data() =>
        {
            "Site".to_string()
        }
        _ => e.ifc_class().to_string(),
    }
}

/// A site element's own picture in the browser, as its tool's: a
/// terrain, a pad, a pit, a trench, a spot level, a ground line.
fn site_icon(e: &Element) -> Option<&'static str> {
    use creabim_core::{SiteForm, WorkKind};
    let ElementKind::Site(s) = &e.kind else {
        return None;
    };
    Some(match &s.form {
        SiteForm::Terrain { .. } => "terrain",
        SiteForm::Pad { .. } => match s.work_kind() {
            Some(WorkKind::Pit) => "pit",
            Some(WorkKind::Trench) => "trench",
            _ => "pad",
        },
        SiteForm::Spot { .. } => "spot-level",
        SiteForm::Line(_) => "contour",
        SiteForm::Property { .. } => "land-plot",
        SiteForm::Road(_) => "road",
        SiteForm::Walk(_) => "site-path",
        SiteForm::Drain(_) => "drain",
        SiteForm::Tank(_) => "tank",
        _ => return None,
    })
}

fn group_title(class: &str) -> String {
    match class {
        "View" => "Views".into(),
        "Site" => "Site ground".into(),
        "Sheet" => "Sheets".into(),
        "Note" => "Notes on drawings".into(),
        "Viewport" => "Viewports".into(),
        "IfcBuildingStorey" => "Levels".into(),
        "IfcBuildingElementProxy" => "Masses".into(),
        "Sketch" => "Sketches".into(),
        "IfcMaterial" => "Materials".into(),
        "IfcMaterialLayerSet" => "Assemblies".into(),
        "ElementType" => "Types".into(),
        "IfcOpeningElement" => "Openings and shafts".into(),
        "TextStyle" => "Text styles".into(),
        "CropBox" => "Crop boxes".into(),
        "IfcFurniture" => "Furniture and fixtures".into(),
        "IfcSpace" => "Rooms".into(),
        "IfcVirtualElement" => "Room separators".into(),
        "IfcCovering" => "Ceilings".into(),
        c => {
            let c = c.strip_prefix("Ifc").unwrap_or(c);
            if c.ends_with('s') {
                c.to_string()
            } else {
                format!("{c}s")
            }
        }
    }
}

/// The Wall tool's Draw by shapes (MU40): label, action, icon, hint.
const WALL_SHAPES: [(&str, &str, &str, &str); 4] = [
    (
        "Straight",
        "wall.shape:0",
        "wall",
        "Click to start, click for each corner",
    ),
    (
        "Arc",
        "wall.shape:1",
        "spline",
        "Its start, its end, then a point it passes through",
    ),
    (
        "Circle",
        "wall.shape:2",
        "circle",
        "Its centre, then a point on it",
    ),
    (
        "Rectangle",
        "wall.shape:3",
        "shape-rect",
        "Two opposite corners: four walls joined at the corners",
    ),
];

/// The icon of an outline's Draw by shape.
fn slab_mode_icon(m: crate::slabtool::SlabMode) -> &'static str {
    use crate::slabtool::SlabMode;
    match m {
        SlabMode::Polygon => "pen-tool",
        SlabMode::Rectangle => "shape-rect",
        SlabMode::FillRoom => "room",
        SlabMode::FromSketch => "pencil",
    }
}

/// The icon of a schedule of `c`.
fn schedule_icon(c: ScheduleCategory) -> &'static str {
    match c {
        ScheduleCategory::Doors => "door-open",
        ScheduleCategory::Windows => "app-window",
        ScheduleCategory::Walls => "brick-wall",
        ScheduleCategory::Slabs => "layers-2",
        ScheduleCategory::Structure => "rectangle-vertical",
        ScheduleCategory::Furniture => "sofa",
        ScheduleCategory::Fixtures => "fixtures",
        ScheduleCategory::Rooms => "room",
        ScheduleCategory::Materials => "table-2",
        ScheduleCategory::Drainage => "terrain",
        ScheduleCategory::Stairs => "stair",
        ScheduleCategory::Parking => "terrain",
    }
}

/// The New schedule submenu, one entry a category.
fn schedule_items() -> Vec<Item> {
    ScheduleCategory::ALL
        .iter()
        .enumerate()
        .map(|(i, c)| Item::new(format!("new.schedule:{i}"), c.label()).icon(schedule_icon(*c)))
        .collect()
}

/// A menu line that starts drawing a shape: `what` is line, arc, rect or
/// circle (see the "shape" action).
fn shape_item(what: &'static str) -> Item {
    let (label, icon, key, hint) = match what {
        "arc" => (
            "Arc",
            "spline",
            "A",
            "Draw an arc: its two ends, then a point on it",
        ),
        "rect" => (
            "Rectangle",
            "shape-rect",
            "R",
            "Draw a rectangle by two opposite corners",
        ),
        "circle" => (
            "Circle",
            "circle-dot",
            "C",
            "Draw a circle: its centre, then a point on it",
        ),
        _ => (
            "Line",
            "shape-line",
            "L",
            "Draw lines, click after click; back on the first point closes the outline",
        ),
    };
    Item::new(format!("shape:{what}"), label)
        .icon(icon)
        .key(key)
        .hint(hint)
}

fn command_icon(c: Command) -> &'static str {
    match c {
        Command::Undo => "undo-2",
        Command::Redo => "redo-2",
        Command::Move => "move",
        Command::Rotate => "rotate-cw",
        Command::Scale => "scaling",
        Command::Stretch => "stretch",
        Command::Mirror => "mirror",
        Command::Offset => "offset",
        Command::Pull => "pull",
        Command::Duplicate => "duplicate",
        Command::Copy => "copy",
        Command::Cut => "scissors",
        Command::Paste => "clipboard-paste",
        Command::Delete => "trash-2",
        Command::Hide => "eye-off",
        Command::Isolate => "circle-dot-dashed",
        Command::ShowAll => "eye",
        Command::SelectAll => "square-dashed-mouse-pointer",
        Command::SelectNone => "square-dashed",
        Command::CursorHere => "crosshair",
        Command::WorkplaneHere => "square-dashed",
    }
}

/// The modifier keys held this frame (1 Shift, 2 Ctrl, 4 Alt), for the
/// status bar's mouse hints.
pub(super) static MODS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

impl CreaBimApp {
    /// Is the Wall tool drawing a curtain wall type? Then it is the
    /// Curtain wall tool on the bar (MU75).
    pub(super) fn drawing_curtain(&self) -> bool {
        self.plan.tool == Tool::Wall
            && self
                .plan
                .types
                .get(&creabim_core::types::TypeCategory::Wall)
                .and_then(|id| self.doc.model().get(*id))
                .is_some_and(|e| {
                    matches!(&e.kind, ElementKind::ElementType(t)
                        if matches!(&t.params, creabim_core::types::TypeParams::Wall(w) if w.curtain.is_some()))
                })
    }

    fn cmd_item(&self, c: Command) -> Item {
        let enabled = match c {
            Command::Undo => self.doc.can_undo(),
            Command::Redo => self.doc.can_redo(),
            Command::Paste => !self.clipboard.is_empty(),
            Command::ShowAll => !self.hidden.is_empty(),
            c => self.selection.is_some() || !c.needs_selection(),
        };
        let label = match c {
            Command::Undo => self
                .doc
                .undo_label()
                .map_or("Undo".into(), |l| format!("Undo {l}")),
            Command::Redo => self
                .doc
                .redo_label()
                .map_or("Redo".into(), |l| format!("Redo {l}")),
            Command::Delete if self.selection.len() > 1 => {
                format!("Delete {} selected", self.selection.len())
            }
            c => c.label().into(),
        };
        let running = self
            .plan
            .modal
            .as_ref()
            .map(|m| m.mode)
            .or(self.plan.armed.map(|a| a.0))
            .is_some_and(|m| {
                matches!(
                    (m, c),
                    (crate::modal::Mode::Move, Command::Move)
                        | (crate::modal::Mode::Rotate, Command::Rotate)
                        | (crate::modal::Mode::Scale, Command::Scale)
                        | (crate::modal::Mode::Mirror, Command::Mirror)
                        | (crate::modal::Mode::Offset, Command::Offset)
                        | (crate::modal::Mode::Pull, Command::Pull)
                )
            });
        Item::new(format!("cmd:{c:?}"), label)
            .key(c.shortcut())
            .enabled(enabled)
            .icon(command_icon(c))
            .checked(running)
    }

    /// A tool family's drop-down panel (AMD-011, approved 2026-11-15):
    /// its tools under their headings, each with its icon and key, the
    /// one in use ticked; then which of them to keep on the bar.
    fn family_items(&self, f: crate::families::Family) -> Vec<Item> {
        let active = f.active(&self.plan, crate::sketch::last_shape());
        let mut items = vec![];
        for (head, members) in f.groups() {
            if !head.is_empty() {
                items.push(Item::Header(head.into()));
            }
            for m in members {
                items.push(
                    Item::new(m.id.clone(), m.label)
                        .icon(m.icon)
                        .key(m.key)
                        .hint(m.hint)
                        .checked(active.as_deref() == Some(m.id.as_str())),
                );
            }
        }
        items.push(Item::Separator);
        items.push(
            Item::sub(
                "Keep on the bar",
                f.members()
                    .into_iter()
                    .map(|m| {
                        Item::new(format!("fampin:{}", m.id), m.label)
                            .icon(m.icon)
                            .hint("A button of its own beside the family's, on the tools row")
                            .checked(self.shell.settings.family_pins.contains(&m.id))
                            .keep_open()
                    })
                    .collect(),
            )
            .icon("map-pin"),
        );
        items
    }

    /// Create › Views › Plan or Ceiling plan (MU55-01): a new plan view of
    /// `level`, at the scale of the level's first plan, named so it can be
    /// told apart, and opened in the focused area. One undo step.
    pub(super) fn new_plan_of(&mut self, level: ElementId, ceiling: bool) {
        let model = self.doc.model();
        let Some(level_name) = model.get(level).map(|e| e.name.clone()) else {
            return;
        };
        let mut view = creabim_core::View::plan(level);
        if let Some(scale) = creabim_core::levels::plan_views(model, level)
            .into_iter()
            .find_map(|v| match model.get(v).map(|e| &e.kind) {
                Some(ElementKind::View(x)) => Some(x.scale),
                _ => None,
            })
        {
            view.scale = scale;
        }
        super::set_looks_up(&mut view, ceiling);
        let base = if ceiling {
            format!("{level_name} ceiling plan")
        } else {
            format!("{level_name} plan")
        };
        let taken = |n: &str| model.iter().any(|e| e.name == n);
        let mut name = base.clone();
        let mut i = 2;
        while taken(&name) {
            name = format!("{base} {i}");
            i += 1;
        }
        let e = creabim_core::Element::new(name.clone(), ElementKind::View(view));
        let id = e.id;
        let label = if ceiling {
            "New ceiling plan"
        } else {
            "New plan"
        };
        // a ceiling plan comes with each ceiling's height tagged (MU142)
        let tags = if ceiling {
            creabim_core::room::ceiling_tags_in(model, id, level)
        } else {
            vec![]
        };
        let tx = tags
            .into_iter()
            .fold(creabim_core::Transaction::new(label).insert(e), |tx, t| {
                tx.insert(t)
            });
        let r = self.doc.commit(tx);
        if r.is_ok() {
            self.active_view = Some(id);
        }
        self.report(r, format!("{name} made and opened."));
    }

    /// The one Draw by panel (tool families REV04): the shape, then the
    /// edges, for every outline tool; the wall's own four shapes for Wall.
    fn draw_by_items(&self) -> Vec<Item> {
        use crate::slabtool::SlabMode;
        let tool = self.plan.tool;
        let mut items = vec![Item::Header("Shape".into())];
        if tool == Tool::Wall {
            for (i, (label, id, icon, hint)) in WALL_SHAPES.iter().enumerate() {
                items.push(
                    Item::new(*id, *label)
                        .icon(icon)
                        .hint(*hint)
                        .checked(self.plan.wall_shape as usize == i),
                );
            }
            return items;
        }
        if tool.draws_outline() {
            const KEYS: [&str; 4] = ["1", "2", "3", "4"];
            for (i, m) in SlabMode::ALL.iter().enumerate() {
                let label = m.label_for(tool);
                items.push(
                    Item::new(
                        format!("slab.mode:{i}"),
                        label.split(" (").next().unwrap_or(label),
                    )
                    .icon(slab_mode_icon(*m))
                    .key(KEYS[i.min(3)])
                    .hint(m.hint_for(tool))
                    .checked(self.plan.slab_mode == *m),
                );
            }
        } else {
            items.clear();
        }
        // a polygon's sides, line or arc, while it is drawn
        let polygon = !tool.draws_outline() || self.plan.slab_mode == SlabMode::Polygon;
        items.push(Item::Header("Edges".into()));
        items.push(
            Item::new("slab.arc:0", "Line")
                .icon("shape-line")
                .key("L")
                .hint("Straight sides")
                .enabled(polygon)
                .checked(!self.plan.slab_arc),
        );
        items.push(
            Item::new("slab.arc:1", "Arc")
                .icon("spline")
                .key("A")
                .hint("The next side is an arc: click its end, then a point on it")
                .enabled(polygon)
                .checked(self.plan.slab_arc),
        );
        items
    }

    /// The items of a menu or dropdown, by its key.
    pub(super) fn menu_items(&self, key: &str) -> Vec<Item> {
        if let Some(items) = self.area_menu(key) {
            return items;
        }
        // a group of the folded browser's rail: its items, to pick (MU29)
        if let Some(group) = key.strip_prefix("rail:") {
            let state = self.left_state();
            let Some(g) = state["groups"]
                .as_array()
                .and_then(|gs| gs.iter().find(|g| g["key"].as_str() == Some(group)))
            else {
                return vec![];
            };
            let mut items = vec![];
            for r in g["rows"].as_array().into_iter().flatten().take(60) {
                items.push(
                    Item::new(
                        format!("railpick:{}", r["id"].as_str().unwrap_or("")),
                        r["name"].as_str().unwrap_or(""),
                    )
                    .icon(crate::svgicon::named(r["icon"].as_str().unwrap_or("")))
                    .checked(r["sel"].as_bool().unwrap_or(false)),
                );
            }
            return items;
        }
        if let Some(f) = key
            .strip_prefix("fam:")
            .and_then(crate::families::Family::from_key)
        {
            return self.family_items(f);
        }
        match key {
            "drawby" => self.draw_by_items(),
            "menu:CreaBIM" => self.app_menu(),
            "units.menu" => self.unit_items(),
            "select.menu" => self.select_menu_items(),
            "sweep.library" => self.sweep_library_items(),
            "beam.corner" => creabim_core::members::BeamCorner::ALL
                .iter()
                .map(|c| {
                    Item::new(format!("beam.corner:{}", c.label()), c.label())
                        .icon(c.icon())
                        .hint(c.hint())
                        .checked(*c == self.plan.members.beam_corner)
                })
                .collect(),
            "railing.library" => self.library_items(
                "railing.lib",
                self.plan.members.railing_section.clone(),
                Some("Its own: a square bar"),
            ),
            "column.library" => self.library_items(
                "column.lib",
                self.plan.members.column_section.clone(),
                Some("Its own: square or round"),
            ),
            "beam.library" => self.library_items(
                "beam.lib",
                self.plan.members.beam_section.clone(),
                Some("Its own: width × depth"),
            ),
            "extrusion.library" => self.extrusion_library_items(),
            "menu:File" => self.file_menu(),
            "menu:Edit" => self.edit_menu(),
            "menu:View" => self.view_menu(),
            "menu:Materials" => self.materials_menu(),
            "menu:Help" => self.help_menu(),
            "menu:Create" => self.create_menu(),
            "tab:schedules" => schedule_items(),
            "tab:line" => vec![shape_item("line"), shape_item("arc")],
            "tab:rect" => vec![shape_item("rect"), shape_item("circle")],
            "tab:ifc" => vec![
                Item::new("open", "Open an IFC file…").icon("folder-open"),
                Item::Separator,
                Item::new("export.ifc4", "Export IFC4 (.ifc)…").icon("upload"),
                Item::new("export.ifcx", "Export IFC5 (.ifcx, alpha)…").icon("upload"),
            ],
            "underlay" => self.underlay_items(),
            "keeponly" => self.keep_only_items(),
            "render3d" => self.render_items(),
            "display" => self.display_items(),
            "viewstyle" => self.view_style_items(),
            "wall.location" => self.wall_location_items(),
            "type.tool" => self.tool_type_items(),
            "palette" => self.palette_items(),
            "look" => crate::camera::Look::ALL
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    // a cube with the face looked at filled (MU29)
                    let icon = match l.label() {
                        "Top" => "look-top",
                        "Bottom" => "look-bottom",
                        "Front" => "look-front",
                        "Back" => "look-back",
                        "Right" => "look-right",
                        "Left" => "look-left",
                        _ => "look-iso",
                    };
                    Item::new(format!("nav.look:{i}"), l.label()).icon(icon)
                })
                .collect(),
            "views" => {
                let mut items = vec![];
                let model = self.doc.model();
                for (g, ids) in crate::browser::view_groups(model) {
                    items.push(Item::Header(g.title().into()));
                    for id in ids {
                        let Some(e) = model.get(id) else { continue };
                        items.push(
                            Item::new(format!("view:{}", id.0), e.name.clone())
                                .checked(self.active_view == Some(id))
                                .icon(icon_name(Icon::of(e))),
                        );
                    }
                }
                let sheets = self.sheet_list();
                if !sheets.is_empty() {
                    items.push(Item::Header("Sheets".into()));
                }
                for (id, name) in sheets {
                    items.push(
                        Item::new(format!("view:{}", id.0), name)
                            .checked(self.active_view == Some(id))
                            .icon("file-text"),
                    );
                }
                items
            }
            "placeview" => {
                let Some(sheet) = self.active_view.filter(|id| self.is_sheet(*id)) else {
                    return vec![];
                };
                let placed: Vec<ElementId> = self
                    .doc
                    .model()
                    .iter()
                    .filter_map(|e| match &e.kind {
                        ElementKind::Viewport(vp) if vp.sheet == sheet => Some(vp.view),
                        _ => None,
                    })
                    .collect();
                self.views()
                    .into_iter()
                    .map(|(id, name, _)| {
                        let label = if placed.contains(&id) {
                            format!("{name} (placed)")
                        } else {
                            name
                        };
                        Item::new(format!("place:{}", id.0), label)
                    })
                    .collect()
            }
            "context" => {
                let n = self.selection.len();
                let ctx = |c: Command| match self.cmd_item(c) {
                    Item::Action {
                        label,
                        shortcut,
                        enabled,
                        icon,
                        ..
                    } => Item::new(format!("ctx:{c:?}"), label)
                        .key(shortcut)
                        .enabled(enabled)
                        .icon(icon),
                    other => other,
                };
                let mut items = vec![];
                if n > 0 {
                    items.push(Item::Header(match n {
                        1 => "1 selected".into(),
                        n => format!("{n} selected"),
                    }));
                    // a view placed on a sheet: its scale first
                    let placed = self.selection.active.and_then(|id| {
                        match &self.doc.model().get(id)?.kind {
                            ElementKind::Viewport(vp) => Some(vp.view),
                            _ => None,
                        }
                    });
                    if let Some(view) = placed.filter(|_| n == 1) {
                        let scale = self.view(view).map_or(0.0, |v| v.paper_scale());
                        if self
                            .view(view)
                            .is_some_and(|v| !matches!(v.kind, ViewKind::Schedule { .. }))
                        {
                            items.push(
                                Item::sub(
                                    format!("Scale 1:{scale:.0}"),
                                    self.viewport_scale_items(view),
                                )
                                .icon("ruler"),
                            );
                        }
                        items.push(
                            Item::new(format!("view:{}", view.0), "Open the view itself")
                                .icon("map"),
                        );
                        items.push(Item::Separator);
                    }
                    // Create similar, Pick up and Apply settings
                    items.extend(self.like_menu_items());
                    // cutting it up first, where they are found (MU39): Split
                    // for anything, Divide for what has a length
                    items.push(Item::new("split", "Split…").icon("split").hint(
                        "Cut it along a guide, a line or two points you click; keep the side you click",
                    ));
                    if self.selection.active.is_some_and(|id| {
                        creabim_core::divide::span(self.doc.model(), id).is_some()
                    }) {
                        items.push(
                            Item::new("divide", "Divide…")
                                .icon("mod-array")
                                .hint("Into parts along it, up it or both: cuts, guides or copies"),
                        );
                    }
                    items.push(Item::Separator);
                    // the strip's tools for this selection (MU43)
                    items.extend(self.strip_menu_items());
                    items.extend(self.wall_menu_items());
                    items.extend(self.join_menu_items());
                    items.extend(self.level_menu_items());
                    for group in [
                        &[
                            Command::Move,
                            Command::Rotate,
                            Command::Scale,
                            Command::Mirror,
                            Command::Offset,
                            Command::Pull,
                            Command::Duplicate,
                        ][..],
                        &[Command::Copy, Command::Cut, Command::Delete],
                        &[Command::Hide, Command::Isolate],
                    ] {
                        items.extend(group.iter().map(|c| ctx(*c)));
                        items.push(Item::Separator);
                    }
                    if !self.shell.context_3d {
                        items.push(align_item());
                        // two that can meet: run them to their corner
                        if self.corner_pair().is_some() {
                            items.push(
                                Item::new("trim.corner", "Trim to corner")
                                    .icon("align-tool")
                                    .key("J")
                                    .hint("Run the two to where they meet, each keeping its longer part (J, then click the parts to keep, to choose)"),
                            );
                        }
                        items.push(Item::Separator);
                    }
                    items.extend(self.hide_in_view_items(self.shell.context_3d));
                    items.extend(self.similar_items());
                    // its material, at hand (Mockup_Material_Section_REV00)
                    let mat = self
                        .selection
                        .active
                        .filter(|_| n == 1)
                        .and_then(|id| self.material_here(id))
                        .and_then(|m| self.doc.model().get(m).map(|e| e.name.clone()));
                    if let Some(name) = mat {
                        items.push(Item::Separator);
                        items.push(
                            Item::new("matcard", format!("Material: {name}…"))
                                .key("Alt+click")
                                .hint("Its colour, cut hatch and surface, here"),
                        );
                        items.push(
                            Item::new("matselect", "Select all with this material")
                                .hint("Every element made of it, its layers included"),
                        );
                    }
                    items.push(Item::Separator);
                }
                items.extend([
                    ctx(Command::Paste),
                    ctx(Command::ShowAll),
                    Item::Separator,
                    ctx(Command::SelectAll),
                ]);
                if n > 0 {
                    items.push(ctx(Command::SelectNone));
                }
                items.push(Item::Separator);
                items.push(ctx(Command::CursorHere));
                items.push(ctx(Command::WorkplaneHere));
                items
            }
            "orient" => crate::modal::Orient::ALL
                .iter()
                .enumerate()
                .map(|(i, o)| {
                    let label = if o.ready() {
                        o.label().to_string()
                    } else {
                        format!("{} (not yet)", o.label())
                    };
                    Item::new(format!("orient:{i}"), label)
                        .checked(crate::modal::rotate_orient() == *o)
                        .enabled(o.ready())
                        .hint(o.hint())
                })
                .collect(),
            "pivot" => {
                let mut items: Vec<Item> = crate::modal::Pivot::ALL
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        Item::new(format!("pivot:{i}"), p.label()).checked(self.plan.pivot == *p)
                    })
                    .collect();
                items.push(Item::Separator);
                items.push(
                    Item::new("cursor.clear", "Clear the pivot cursor")
                        .enabled(self.plan.cursor.is_some()),
                );
                items
            }
            "workplane" => {
                let mut items = vec![
                    Item::new("wp.level", "Level (follows the plan)")
                        .checked(self.workplane.is_none()),
                    Item::new("wp.face", "Pick a face…").icon("square-dashed-mouse-pointer"),
                    Item::new("wp.points", "Through three points…").icon("spline"),
                ];
                if let Some((_, z)) = self.work_level() {
                    items.push(
                        Item::new("wp.offset", "Level plane you can offset")
                            .icon("level-offset")
                            .hint(format!(
                                "A plane at {} to slide up and down",
                                crate::units::elevation(z)
                            )),
                    );
                }
                items
            }
            "step" => self.step_items(),
            "held" => self.held_items(),
            "snaps" => {
                let mut s = self.plan.snap;
                let mut items = vec![
                    Item::new("snap.on", "Snapping")
                        .key("F3")
                        .checked(s.on)
                        .keep_open()
                        .hint("Hold Alt while picking to suspend snaps for a moment"),
                    Item::Separator,
                    Item::new("snap.all", "Turn all on").keep_open(),
                    Item::new("snap.none", "Turn all off").keep_open(),
                    Item::Separator,
                ];
                const ICONS: [&str; 12] = [
                    "snap-endpoint",
                    "snap-midpoint",
                    "snap-intersection",
                    "snap-perpendicular",
                    "snap-nearest",
                    "snap-centre",
                    "snap-quadrant",
                    "hash",
                    "slash",
                    "snap-axis",
                    "snap-track",
                    "ruler",
                ];
                for (i, (label, v)) in s.switches().into_iter().enumerate() {
                    items.push(
                        Item::new(format!("snap:{i}"), label)
                            .checked(*v)
                            .icon(ICONS.get(i).copied().unwrap_or(""))
                            .keep_open()
                            .enabled(self.plan.snap.on),
                    );
                }
                items.push(Item::Separator);
                items.push(Item::sub(
                    format!("Grid snap: {}", self.plan.snap.step_label()),
                    self.step_items(),
                ));
                // every edge at its even parts (MU34 B1)
                // Off, a field to type any number of parts, and the last two
                // used, to pick again (MU102: a fixed list missed numbers)
                let now = crate::snaps::divisions();
                let mut div = vec![
                    Item::new("snap.div:0", "Off").checked(now == 0),
                    Item::new("input:snap.div", "Parts, 2 to 20").hint(if now == 0 {
                        String::new()
                    } else {
                        now.to_string()
                    }),
                ];
                let recent = &self.shell.settings.recent_divisions;
                if !recent.is_empty() {
                    div.push(Item::Separator);
                    for n in recent.iter().take(2) {
                        div.push(
                            Item::new(format!("snap.div:{n}"), format!("{n} parts"))
                                .checked(now == *n),
                        );
                    }
                }
                items.push(Item::sub(
                    if now == 0 {
                        "Divisions: off".to_string()
                    } else {
                        format!("Divisions: {now}")
                    },
                    div,
                ));
                items
            }
            key => crate::memberstool::stair_menu(&self.plan, key)
                .or_else(|| self.drafting_menu(key))
                .or_else(|| self.grid_vis_menu(key))
                .unwrap_or_default(),
        }
    }

    fn file_menu(&self) -> Vec<Item> {
        let mut recent: Vec<Item> = self
            .recent
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| Item::new(format!("recent:{i}"), file_label(f)).hint(f.clone()))
            .collect();
        if !recent.is_empty() {
            recent.push(Item::Separator);
            recent.push(Item::new("recent.clear", "Clear the list"));
        }
        let mut items = vec![
            Item::new("new.project", "New project…")
                .key("Ctrl+N")
                .icon("file-plus")
                .hint("Start empty or from the starter house, or open a sample made for learning"),
            Item::new("new.sheet", "New sheet").icon("file-text"),
            Item::sub("New schedule", schedule_items()).icon("table-2"),
            Item::Separator,
            Item::new("open", "Open…")
                .key("Ctrl+O")
                .icon("folder-open")
                .hint("Open a .creabim, .ifc or .ifcx file. Other files (.blend, .FCStd) are linked into this project."),
            Item::sub("Open recent", recent).icon("folder"),
        ];
        if self.shell.web.is_none() {
            items.push(
                Item::new("open.path", "Open the path in the File box")
                    .icon("folder-open")
                    .hint("For typing or pasting a path instead of browsing."),
            );
        }
        items.extend([
            Item::new("save", "Save")
                .key("Ctrl+S")
                .icon("save")
                .hint("Save as a .creabim project file, with its undo history."),
            Item::new("saveas", "Save as…").icon("save"),
            Item::new("template.save", "Save as template…")
                .icon("layout-dashboard")
                .hint("Keep this project's look, or its look and its building, as a start for new projects."),
            Item::new("template.apply", "Apply a template's look…")
                .icon("palette")
                .hint("Bring a template's graphic styles, annotation styles and masters into this project, in one step that can be undone. Sheets, views, types and the building are not touched."),
            Item::Separator,
            Item::Header("Project".into()),
            Item::new("units", "Project units…")
                .icon("units")
                .hint("Metric or imperial: how lengths are shown and typed, in dimensions, schedules and on sheets"),
            Item::new("levels", "Levels…")
                .icon("level")
                .hint("Add, duplicate, rename, move and delete levels"),
            Item::new("marks", "Mark numbering…")
                .icon("tag")
                .hint("How doors, windows and other elements are numbered: prefixes, digits, level-based marks, order. Renumber, and check for marks used twice."),
            Item::new("graphicstyles", "Graphic styles…")
                .icon("align-left")
                .hint("Style packs: pens by scale, object styles, hatch patterns and view templates. Load an office's pack to restyle every drawing."),
            Item::new("styles", "Annotation styles…")
                .icon("anno-styles")
                .hint("Named styles for dimensions, notes and tags: text, ends, tag shape, colour"),
            Item::new("templates", "Masters")
                .icon("sheet-template")
                .hint("Title blocks for sheets: change the built-in one, or import one from DXF, SVG or another project"),
            Item::Separator,
            Item::Header("Linked files".into()),
            Item::new("link", "Import and link…")
                .icon("link")
                .hint("Bring in the objects of a .blend or .FCStd file. Saving it in Blender or FreeCAD updates them here."),
            Item::new("live", "Reload linked files when saved").checked(self.live_reload),
        ]);
        if self.import_report.is_some() {
            items.push(
                Item::new("import.report", "Import report")
                    .icon("file-text")
                    .hint("What the last IFC import brought in as native elements and as meshes, and why, element by element. It can be copied."),
            );
        }
        let linked = crate::files::linked_files(self.doc.model());
        if !linked.is_empty() {
            items.push(
                Item::sub(
                    "Reload a linked file",
                    linked
                        .iter()
                        .enumerate()
                        .map(|(i, f)| Item::new(format!("reload:{i}"), file_label(f)))
                        .collect(),
                )
                .icon("refresh-cw"),
            );
        }
        items.extend([
            Item::Separator,
            Item::Header("Export".into()),
            Item::new("export.ifc4", "IFC4 (.ifc)").icon("upload"),
            Item::new("export.ifcx", "IFC5 (.ifcx, alpha)")
                .icon("upload")
                .hint("The new JSON-based IFC. Its schema is still an alpha, so other tools may not read it yet."),
            Item::new("export.glb", "glTF (.glb) for Blender")
                .icon("box")
                .hint("Every element as a mesh with its CreaBIM id, so edits saved in Blender come back to the same elements."),
            Item::new("export.fcstd", "FreeCAD (.FCStd)")
                .icon("freecad")
                .hint("Every element as a solid with its BIM type and CreaBIM id; open it in FreeCAD, edit, save, and link it back."),
            Item::new("export.pdf", "Sheets as PDF").icon("file-text"),
            Item::new("export.svg", "This view or sheet as SVG").icon("file"),
            Item::new("export.xlsx", "Schedules to Excel (.xlsx)")
                .icon("table-2")
                .hint("A summary, then a sheet for each schedule, with numbers as numbers so the spreadsheet can sum them."),
        ]);
        items
    }

    fn edit_menu(&self) -> Vec<Item> {
        let mut items = vec![];
        for group in [
            &[Command::Undo, Command::Redo][..],
            &[
                Command::Move,
                Command::Rotate,
                Command::Scale,
                Command::Mirror,
                Command::Offset,
                Command::Pull,
                Command::Duplicate,
            ],
            &[Command::Copy, Command::Cut, Command::Paste, Command::Delete],
            &[Command::SelectAll, Command::SelectNone],
            &[Command::Hide, Command::Isolate, Command::ShowAll],
        ] {
            items.extend(group.iter().map(|c| self.cmd_item(*c)));
            if group.contains(&Command::SelectNone) {
                items.extend(self.similar_items());
            }
            if group.contains(&Command::Paste) {
                items.push(
                    Item::sub("Paste aligned to level", self.paste_level_items())
                        .icon("copy-levels"),
                );
            }
            // cutting up, after the edits (MU39): found in the menu and by
            // typing, not only on a right-click
            if group.contains(&Command::Duplicate) {
                let some = !self.selection.is_empty();
                let span = self
                    .selection
                    .active
                    .is_some_and(|id| creabim_core::divide::span(self.doc.model(), id).is_some());
                items.push(
                    Item::new("split", "Split…")
                        .icon("split")
                        .enabled(some)
                        .hint("Cut the selection along a guide, a line or two points; keep the side you click"),
                );
                items.push(
                    Item::new("divide", "Divide…")
                        .icon("mod-array")
                        .enabled(span)
                        .hint("Into parts along it, up it or both: cuts, guides or copies"),
                );
            }
            items.push(Item::Separator);
        }
        let levels = self.level_menu_items();
        if !levels.is_empty() {
            items.extend(levels);
            items.push(Item::Separator);
        }
        items.push(align_item());
        items.push(Item::Separator);
        let mut pivot = self.menu_items("pivot");
        pivot.insert(0, Item::Header("Rotate and scale about".into()));
        items.push(Item::sub("Pivot", pivot).icon("crosshair"));
        items
    }

    /// Everything that can be placed or made, by kind, as in the tool
    /// palette, with the views and sheets the File menu can also make.
    fn create_menu(&self) -> Vec<Item> {
        // the tools the shape tools replace (MU43): kept for one release,
        // each saying what does its job now
        const EARLIER: [(Tool, &str); 4] = [
            (Tool::Sketch, "Now Line and Rectangle"),
            (Tool::Mass, "Now Rectangle, then Pull"),
            (Tool::Extrusion, "Now Pull"),
            (Tool::Pad, "Now Site › Pad"),
        ];
        // tool families REV04, decision 5 (MU55-01, MU57): the single tools
        // first, then one submenu per family in the tools row's order, so
        // the menu and the bar teach each other; the checks are on Manage
        let tool = |t: Tool| {
            let i = Tool::ALL.iter().position(|x| *x == t).unwrap_or(0);
            Item::new(format!("tool:{i}"), t.label())
                .icon(tool_icon(t))
                .hint(t.hint())
        };
        let family = |f: crate::families::Family| -> Vec<Item> {
            let mut out = vec![];
            for (head, members) in f.groups() {
                if !head.is_empty() {
                    out.push(Item::Header(head.into()));
                }
                for m in members {
                    out.push(
                        Item::new(m.id, m.label)
                            .icon(m.icon)
                            .key(m.key)
                            .hint(m.hint),
                    );
                }
            }
            out
        };
        use crate::families::Family as F;
        let mut items = vec![Item::Header("Building elements".into())];
        items.extend([Tool::Wall].map(tool));
        items.push(
            Item::new("tool.curtain", "Curtain wall")
                .icon("curtain-wall")
                .hint("The Wall tool drawing a curtain wall type: mullions and glass on a grid"),
        );
        items.push(
            Item::new("tool.screen", "Screen wall")
                .icon("screen-wall")
                .hint(
                    "The Wall tool drawing a screen wall type: breeze blocks, louvres or a lattice",
                ),
        );
        items.push(
            Item::new("kinds.create:door", "Door…")
                .icon(tool_icon(Tool::Door))
                .hint(
                    "The Door tool, with the picker open: choose how it opens, then click a wall",
                ),
        );
        items.push(
            Item::new("kinds.create:window", "Window…")
                .icon(tool_icon(Tool::Window))
                .hint(
                    "The Window tool, with the picker open: choose how it opens, then click a wall",
                ),
        );
        items.extend(
            [
                Tool::Opening,
                Tool::Slab,
                Tool::Shaft,
                Tool::Roof,
                Tool::Ceiling,
            ]
            .map(tool),
        );
        items.push(Item::Separator);
        let mut rooms = family(F::Rooms);
        rooms.push(
            Item::new("rooms.fromnotes", "Rooms from room notes")
                .icon("room")
                .hint("Turn room labels written as notes (a name over an area) into rooms with tags, wherever walls close them off"),
        );
        let mut drafting = family(F::Drafting);
        drafting.push(
            Item::new("cmd:Pull", "Pull")
                .icon("pull")
                .key("P")
                .hint("Pull the face under the pointer out or in; a drawn outline becomes a solid"),
        );
        drafting.extend([Tool::Sweep, Tool::Void, Tool::Guide].map(tool));
        items.push(Item::sub("Drafting and shapes", drafting).icon("shape-line"));
        items.push(Item::sub("Structure", family(F::Structure)).icon("column"));
        items.push(Item::sub("Stairs", family(F::Stairs)).icon("stair"));
        items.push(Item::sub("Furnishing", family(F::Furnishing)).icon("sofa"));
        items.push(Item::sub("Rooms", rooms).icon("room"));
        let mut site = family(F::Site);
        site.push(Item::Separator);
        site.push(
            Item::new("catalogue.entourage", "Cars and people…")
                .icon("car")
                .hint("Entourage from the catalogue: a car, people, for scale"),
        );
        items.push(Item::sub("Site", site).icon("terrain"));
        // views: a plan or a ceiling plan of any level, straight away
        // (MU55-01: plans come from levels; no hidden duplicate step)
        let levels = creabim_core::levels::sorted(self.doc.model());
        let of_levels = |what: &str, icon: &'static str, hint: &str| -> Vec<Item> {
            levels
                .iter()
                .rev()
                .map(|(id, name, _)| {
                    Item::new(format!("{what}:{}", id.0), name.clone())
                        .icon(icon)
                        .hint(hint.to_string())
                })
                .collect()
        };
        let mut views = vec![
            Item::sub(
                "Plan",
                of_levels("new.plan", "layers-3", "A new floor plan of this level, opened"),
            )
            .icon("layers-3"),
            Item::sub(
                "Ceiling plan",
                of_levels(
                    "new.ceiling",
                    "ceiling",
                    "A reflected ceiling plan of this level, opened: the ceilings and what hangs from them",
                ),
            )
            .icon("ceiling"),
        ];
        views.extend([Tool::Section, Tool::Elevation, Tool::Callout].map(tool));
        views.push(Item::Separator);
        views.push(Item::new("new.sheet", "Sheet").icon("file-text"));
        views.push(Item::sub("Schedule", schedule_items()).icon("table-2"));
        views.push(Item::Separator);
        views.push(tool(Tool::Level));
        views.push(tool(Tool::Grid));
        views.push(
            Item::new("level.add", "Level above the top one")
                .icon("level")
                .hint("A new level one storey up, with its floor plan"),
        );
        items.push(Item::sub("Views and levels", views).icon("view-3d"));
        items.push(
            Item::sub(
                "Annotation",
                [
                    Tool::Dimension,
                    Tool::Angle,
                    Tool::Radius,
                    Tool::Diameter,
                    Tool::ArcLength,
                    Tool::SpotLevel,
                    Tool::Tag,
                    Tool::Note,
                    Tool::Symbol,
                    Tool::Detail,
                ]
                .map(tool)
                .into(),
            )
            .icon("ruler"),
        );
        items.push(Item::Separator);
        items.push(
            Item::sub(
                "Earlier tools",
                EARLIER
                    .iter()
                    .map(|(t, now)| {
                        let i = Tool::ALL.iter().position(|x| x == t).unwrap_or(0);
                        Item::new(format!("tool:{i}"), t.label())
                            .icon(tool_icon(*t))
                            .hint(format!("{now}. {}", t.hint()))
                    })
                    .collect(),
            )
            .icon("folder"),
        );
        items
    }

    /// Every action of the menus in one list, with its menu path, for the
    /// command palette.
    fn palette_items(&self) -> Vec<Item> {
        fn flatten(items: Vec<Item>, path: &str, out: &mut Vec<Item>) {
            for item in items {
                match item {
                    Item::Action {
                        id,
                        label,
                        shortcut,
                        hint,
                        enabled,
                        icon,
                        ..
                    } if enabled
                        && !out
                            .iter()
                            .any(|o| matches!(o, Item::Action { id: i, .. } if *i == id)) =>
                    {
                        out.push(
                            Item::new(id, format!("{path}{label}"))
                                .key(shortcut)
                                .hint(hint)
                                .icon(icon),
                        )
                    }
                    Item::Sub { label, items, .. } => {
                        flatten(items, &format!("{path}{label} › "), out)
                    }
                    _ => {}
                }
            }
        }
        let mut out = vec![];
        flatten(self.create_menu(), "Create › ", &mut out);
        flatten(self.edit_menu(), "", &mut out);
        flatten(self.view_menu(), "View › ", &mut out);
        flatten(self.file_menu(), "File › ", &mut out);
        flatten(self.materials_menu(), "Materials › ", &mut out);
        flatten(self.help_menu(), "Help › ", &mut out);
        // what the tool tabs hold that the menus don't (MU39: Profiles, on
        // Manage, could not be found by typing)
        for (tab, name) in crate::tooltabs::TABS.iter().enumerate() {
            for e in crate::tooltabs::entries(tab) {
                use crate::tooltabs::Entry;
                match e {
                    Entry::Act {
                        id,
                        label,
                        icon,
                        hint,
                    } => flatten(
                        vec![Item::new(id, label).icon(icon).hint(hint)],
                        &format!("{name} › "),
                        &mut out,
                    ),
                    Entry::Menu { key, label, .. } => flatten(
                        self.menu_items(key),
                        &format!("{name} › {label} › "),
                        &mut out,
                    ),
                    // a family's tools are in the Create menu, under it
                    Entry::Tool(_) | Entry::Family(_) | Entry::Sep => {}
                }
            }
        }
        // every page of the guide, found by its title
        for (section, id, md) in crate::help::PAGES {
            let item = Item::new(format!("help:{id}"), crate::help::title(md))
                .icon("book-open")
                .hint(*section);
            flatten(vec![item], "Help › ", &mut out);
        }
        flatten(self.menu_items("views"), "Open › ", &mut out);
        // and the model's elements, by name: picking one selects and frames it
        for e in self.doc.model().iter() {
            if matches!(
                e.kind,
                ElementKind::Units(_) | ElementKind::View(_) | ElementKind::Sheet(_)
            ) {
                continue;
            }
            out.push(
                Item::new(
                    format!("find:{}", e.id.0),
                    format!("Find › {} › {}", group_title(&class_of(e)), e.name),
                )
                .icon(icon_name(Icon::of(e)))
                .hint("Select it and frame it in the 3D view"),
            );
        }
        out
    }

    /// How many elements the project has, leaving out its units, which
    /// every project has once.
    pub(super) fn element_count(&self) -> usize {
        self.doc
            .model()
            .iter()
            .filter(|e| !matches!(e.kind, ElementKind::Units(_)))
            .count()
    }

    /// The 3D view's navigation buttons.
    pub(super) fn nav_state(&self) -> Value {
        let b = |id: &str, icon: &str, hint: &str, on: bool| json!({"id": id, "icon": icon, "hint": hint, "on": on});
        let sel = self.selection.is_some();
        let mut fitsel = b("nav.fitsel", "scan", "Frame the selection (.)", false);
        fitsel["off"] = json!(!sel);
        json!({
            "theme": self.theme(),
            "popup": self.popup_key(),
            "buttons": [
                b("frame", "frame-all", "Frame all (Home)", false),
                fitsel,
                b("nav.orbitsel", "rotate-3d", "Orbit around the selection", self.orbit_selection),
                b("nav.zoomin", "zoom-in", "Zoom in", false),
                b("nav.zoomout", "zoom-out", "Zoom out", false),
                b("nav.ortho", if self.camera.ortho { "box" } else { "cuboid" },
                  if self.camera.ortho { "Orthographic: click for perspective" } else { "Perspective: click for orthographic" },
                  self.camera.ortho),
                b("ground.grid", "grid-3x3",
                  if self.shell.settings.ground_grid { "Ground grid shown: click to hide it" } else { "Ground grid hidden: click to show it" },
                  self.shell.settings.ground_grid),
                b("xray", "xray",
                  if self.shell.settings.xray { "X-ray on: click for solid surfaces (Alt+Z)" } else { "X-ray: see through walls, slabs and roofs to what is inside them (Alt+Z)" },
                  self.shell.settings.xray),
            ],
            "menus": [
                {"key": "vis3d", "icon": if self.vis3d.is_empty() { "eye" } else { "eye-off" },
                 "label": "Show", "on": !self.vis3d.is_empty(),
                 "hint": "What the 3D view shows: grids, annotation, guides, sketches and every category"},
                {"key": "render3d", "icon": self.shell.settings.render_3d.icon(), "label": "",
                 "hint": format!("How the 3D view draws: {}. Shaded, with edges, hidden line or wireframe", self.shell.settings.render_3d.label())},
            ],
            "look": {"key": "look", "hint": "Look from the top, a side or the corner"},
        })
    }

    /// The View menu: where the 2D view goes and how the 3D view looks,
    /// with the longer lists in submenus so it stays short.
    pub(super) fn view_menu(&self) -> Vec<Item> {
        let place = |p: PlanPlacement, i: u8, label: &str| {
            Item::new(format!("placement:{i}"), label).checked(self.placement == p)
        };
        let layout = self.area_menu("workspaces").unwrap_or_default();
        let now = ground_spacing(self.shell.settings.ground_grid_spacing);
        let custom = !GROUND_SPACINGS.iter().any(|s| (now - s).abs() < 1e-9);
        let mut ground = vec![Item::new("ground.grid", "Show the ground grid")
            .checked(self.shell.settings.ground_grid)
            .hint(
                "The grid on the working level (or the workplane), also on the 3D view's buttons",
            )];
        ground.push(Item::Separator);
        ground.push(Item::Header("Spacing".into()));
        ground.extend(GROUND_SPACINGS.iter().enumerate().map(|(i, s)| {
            Item::new(format!("ground.spacing:{i}"), format!("{s} m"))
                .checked((now - s).abs() < 1e-9)
                .keep_open()
        }));
        ground.push(
            Item::new(
                "ground.custom",
                if custom {
                    format!("Custom: {}…", crate::units::len(now))
                } else {
                    "Custom…".to_string()
                },
            )
            .checked(custom)
            .hint("Type any spacing, in the project's units"),
        );
        let mut camera = vec![
            Item::new("nav.orbitsel", "Orbit around the selection")
                .checked(self.orbit_selection)
                .hint(
                    "Orbiting swings round the selected elements instead of the middle of the view",
                ),
            Item::new("nav.ortho", "Orthographic")
                .checked(self.camera.ortho)
                .hint("Parallel projection, without perspective"),
            Item::Separator,
            Item::Header("Look from".into()),
        ];
        camera.extend(self.menu_items("look"));
        let mut themes: Vec<Item> = Theme::ALL
            .into_iter()
            .map(|t| {
                Item::new(format!("theme:{}", t.id()), t.label())
                    .checked(self.shell.theme == t)
                    .keep_open()
            })
            .collect();
        themes.push(Item::Separator);
        themes.push(
            Item::new("theme.next", "Next theme")
                .key("Ctrl+Shift+T")
                .icon("palette"),
        );
        vec![
            Item::Header("2D view".into()),
            place(PlanPlacement::Docked, 0, "Docked beside 3D"),
            place(PlanPlacement::Window, 1, "In its own window"),
            place(PlanPlacement::Overlay, 2, "Overlaid in the 3D workspace"),
            place(PlanPlacement::Hidden, 3, "Hidden"),
            Item::sub("Workspace", layout).icon("workspace"),
            Item::new("plan3d", "Show the 2D view in 3D space").checked(self.plan_in_3d),
            Item::sub("Underlay", self.underlay_items())
                .icon("layers")
                .enabled(self.active_plan().is_some()),
            Item::new("levels", "Levels…").icon("level").hint(
                "Add, rename, move and delete levels, and make their floor plans, from any view",
            ),
            Item::Separator,
            Item::Header("3D view".into()),
            Item::new("frame", "Frame all")
                .key("Home")
                .icon("frame-all"),
            Item::new("nav.fitsel", "Frame the selection")
                .key(".")
                .icon("scan")
                .enabled(self.selection.is_some()),
            Item::sub(
                "3D view visibility",
                self.visibility_items(super::gridvis::VisTarget::Window3d),
            )
            .icon("eye"),
            Item::new("xray", "X-ray")
                .key("Alt+Z")
                .checked(self.shell.settings.xray)
                .icon("xray-ball")
                .hint("Walls, slabs, roofs and masses see-through, so beams, columns and what is inside them show"),
            Item::sub("Render", self.render_items()).icon(self.shell.settings.render_3d.icon()),
            Item::sub("Crop boxes", self.crop_menu()).icon("crop-box"),
            Item::sub("Camera", camera).icon("axis-3d"),
            Item::sub("Ground grid", ground).icon("grid-3x3"),
            Item::Separator,
            Item::Header("Every view".into()),
            Item::new("guides.toggle", "Guides")
                .key("Shift+G")
                .checked(!self.shell.settings.guides_hidden)
                .icon("slash")
                .hint("Hide the guides in every view without deleting them, and bring them back"),
            Item::sub(
                "2D view visibility",
                self.vis_target(false)
                    .map(|t| self.visibility_items(t))
                    .unwrap_or_default(),
            )
            .icon("eye"),
            Item::Separator,
            Item::Header("Interface".into()),
            Item::sub("Theme", themes).icon("palette"),
            Item::new("palette", "Command palette")
                .key("Ctrl+K")
                .icon("search"),
            Item::new("ui.classic", "Classic panels")
                .checked(self.shell.classic)
                .hint("The plain built-in panels instead of the new interface"),
        ]
    }

    /// The Step choices: Off, three in the project's units, and Custom.
    pub(super) fn step_items(&self) -> Vec<Item> {
        let s = self.plan.snap;
        let steps: &[u32] = if crate::units::get().is_imperial() {
            &crate::snaps::STEPS_IMPERIAL
        } else {
            &crate::snaps::STEPS_METRIC
        };
        let mut items = vec![
            Item::Header("Grid snap (Alt+S)".into()),
            Item::new("step.off", "Off: free")
                .checked(!s.increment)
                .hint("Points and drags go exactly where the pointer is; other snaps still catch them when near"),
        ];
        for um in steps {
            items.push(
                Item::new(format!("step:{um}"), crate::units::len(*um as f64 / 1e6))
                    .checked(s.increment && s.step_um == *um),
            );
        }
        let custom = s.increment && !steps.contains(&s.step_um);
        items.push(
            Item::new("step.custom", "Custom…")
                .checked(custom)
                .hint("Type any spacing: 25 mm, 1/4\", 2 cm"),
        );
        items.push(Item::Separator);
        items.push(
            Item::new(
                "marks.toggle",
                format!("Distance marks every {}", s.spacing_label()),
            )
            .checked(s.marks)
            .hint("While dragging, faint marks from where you grabbed; a mark holds the drag only when the pointer is near it"),
        );
        items
    }

    /// Open the Foundation plan, or make one first: a level 600 mm below the
    /// lowest, its plan showing the level above as underlay (A3-1).
    pub(super) fn show_foundation_plan(&mut self) {
        self.plan.footing_placed = false;
        if let Some(v) = crate::memberstool::foundation_plan(self.doc.model()) {
            self.active_view = Some(v);
            self.status = "Foundation plan: the footings, under the floor above.".into();
            return;
        }
        let lowest = creabim_core::levels::sorted(self.doc.model())
            .first()
            .map_or(0.0, |(_, _, l)| l.elevation);
        let level = Element::new(
            "Foundation",
            ElementKind::Level(creabim_core::Level {
                elevation: lowest - 0.6,
                height: 0.6,
            }),
        );
        let mut view = creabim_core::View::plan(level.id);
        view.underlay = creabim_core::Underlay::Above;
        let view = Element::new("Foundation plan", ElementKind::View(view));
        let plan = view.id;
        let tx = creabim_core::Transaction::new("Make a Foundation plan")
            .insert(level)
            .insert(view);
        match self.doc.commit(tx) {
            Ok(()) => {
                self.active_view = Some(plan);
                self.status = format!(
                    "Made a Foundation plan at {}, with the floor above as underlay.",
                    crate::units::elevation(lowest - 0.6)
                );
            }
            Err(e) => self.status = format!("Couldn't make a Foundation plan: {e}"),
        }
    }

    /// Run an action picked in a menu, a toolbar or the project browser.
    pub(super) fn run_action(&mut self, ctx: &egui::Context, id: &str) {
        let (head, arg) = id.split_once(':').unwrap_or((id, ""));
        let index = arg.parse::<usize>().ok();
        let by_uuid = |s: &Self| {
            s.doc
                .model()
                .iter()
                .find(|e| e.id.0.to_string() == arg)
                .map(|e| e.id)
        };
        if self.area_action(head, arg) {
            return;
        }
        // the right-click menu's material entries
        if matches!(id, "matcard" | "matselect") {
            let Some(e) = self.selection.active else {
                return;
            };
            let Some(m) = self.material_here(e) else {
                return;
            };
            if id == "matcard" {
                let at = ctx.pointer_latest_pos().unwrap_or(egui::pos2(240.0, 240.0));
                crate::matcard::open_at(m, Some(e), at);
            } else {
                self.select_with_material(m);
            }
            return;
        }
        if self.held_action(ctx, id) {
            return;
        }
        // a template's own file path has a drive's colon in it: all that
        // follows the first colon is the path
        if self.templates_action(ctx, head, arg) {
            return;
        }
        // a family's tool picked: its button shows it from now on (REV04)
        if let Some(f) = crate::families::Family::of_action(id) {
            let s = &mut self.shell.settings;
            if s.family_last.get(f.key()).map(String::as_str) != Some(id) {
                s.family_last.insert(f.key().to_string(), id.to_string());
                s.store();
            }
        }
        // the 3D view's display buttons and their caret (Visuals REV02)
        if self.display_action(head, arg) {
            return;
        }
        match head {
            "site" => {
                // a Site family tool: its tool, then what it draws
                let tool = if arg.starts_with("area.") {
                    Tool::Paving
                } else if arg.starts_with("plant.") {
                    Tool::Planting
                } else {
                    Tool::Terrain
                };
                let i = Tool::ALL.iter().position(|t| *t == tool).unwrap_or(0);
                if self.plan.tool != tool {
                    self.run_action(ctx, &format!("tool:{i}"));
                }
                if self.plan.tool == tool {
                    let part = match arg.split_once('.') {
                        Some(("area", n)) => format!("area.{n}"),
                        _ => arg.to_string(),
                    };
                    crate::partstool::toggle_option(&mut self.plan, &part);
                    if let Some(m) = crate::families::member_of(id) {
                        self.status = m.hint;
                    }
                }
            }
            "new.plan" | "new.ceiling" => {
                if let Some(level) = by_uuid(self) {
                    self.new_plan_of(level, head == "new.ceiling");
                }
            }
            "sheet.symboltool" => {
                self.set_sheet_symbol_last("sheet.symboltool");
                if let Some(i) = Tool::ALL.iter().position(|t| *t == Tool::Symbol) {
                    self.run_action(ctx, &format!("tool:{i}"));
                }
            }
            "fampin" => {
                let s = &mut self.shell.settings;
                if let Some(at) = s.family_pins.iter().position(|p| p == arg) {
                    s.family_pins.remove(at);
                } else {
                    s.family_pins.push(arg.to_string());
                }
                s.store();
            }
            "cmd" => {
                if let Some(c) = Command::ALL.into_iter().find(|c| format!("{c:?}") == arg) {
                    self.run_command(ctx, c, None);
                }
            }
            "ctx" => {
                let Some(c) = Command::ALL.into_iter().find(|c| format!("{c:?}") == arg) else {
                    return;
                };
                let at = if self.shell.context_3d {
                    self.menu_at_3d
                } else {
                    self.plan.menu_at
                };
                match (c, c.modal()) {
                    (_, Some(m)) => self.arm(c, m),
                    (Command::WorkplaneHere, None) if self.shell.context_3d => match self
                        .menu_plane_3d
                    {
                        Some(plane) => self.set_workplane(plane, "on a face".into()),
                        None => {
                            self.status = "Right-click a face to put the workplane on it.".into()
                        }
                    },
                    _ => self.run_command(ctx, c, at),
                }
            }
            "tab" => {
                if let Some(i) = index.filter(|i| *i < crate::tooltabs::TABS.len()) {
                    self.shell.settings.tools_tab = i;
                    self.shell.settings.store();
                }
            }
            "strip" => self.run_strip_action(ctx, id),
            "shape" => {
                // Line, Arc, Rectangle, Circle (MU43): the drawing tool,
                // starting with that shape
                use crate::sketch::Shape;
                let shape = match arg {
                    "arc" => Shape::Arc,
                    "rect" => Shape::Rectangle,
                    "circle" => Shape::Circle,
                    _ => Shape::Line,
                };
                if let Some(why) = self.tool_unusable(Tool::Sketch) {
                    self.status = format!("Drawing: not here, {why}.");
                    return;
                }
                crate::detailtool::keep(&mut self.doc, &mut self.plan, &mut self.selection);
                self.plan.tool = Tool::Sketch;
                self.plan.cancel();
                self.pending_3d = None;
                self.beam_face = None;
                crate::sketch::pick_shape(ctx, shape);
                self.status = crate::sketch::hint(shape, None);
            }
            "tool" => {
                if let Some(why) = index
                    .and_then(|i| Tool::ALL.get(i))
                    .and_then(|t| self.tool_unusable(*t).map(|w| (t.label(), w)))
                {
                    self.status = format!("{}: not here, {}.", why.0, why.1);
                    return;
                }
                if let Some(t) = index.and_then(|i| Tool::ALL.get(i)) {
                    // a detail path being drawn is kept, not dropped
                    crate::detailtool::keep(&mut self.doc, &mut self.plan, &mut self.selection);
                    self.plan.tool = *t;
                    self.plan.cancel();
                    // a chain begun in 3D ends with its tool (MU40)
                    self.pending_3d = None;
                    self.beam_face = None;
                    self.plan.members.last_beam = None;
                    self.status = t.hint().into();
                    if *t == Tool::Guide && self.shell.settings.guides_hidden {
                        // drawing guides you can't see helps no one
                        self.set_guides_hidden(false);
                        self.status = format!("Guides shown again. {}", t.hint());
                    }
                    if *t == Tool::Level && !self.shows_section() {
                        // level lines need a section or elevation; anywhere
                        // else the levels are edited in their window
                        self.open_levels();
                        self.status = "Levels window: add, rename and move levels here. To draw a level line, open a section or elevation.".into();
                    }
                }
            }
            "view" => {
                if let Some(v) = by_uuid(self) {
                    self.active_view = Some(v);
                }
            }
            "place" => {
                if let (Some(view), Some(sheet)) = (
                    by_uuid(self),
                    self.active_view.filter(|id| self.is_sheet(*id)),
                ) {
                    self.place_view(sheet, view);
                }
            }
            "find" => {
                if let Some(id) = by_uuid(self) {
                    self.selection.only(id);
                    self.frame_selection();
                    self.status = format!(
                        "{} selected and framed.",
                        crate::hoverpick::describe(self.doc.model(), id)
                    );
                }
            }
            // a link to a website, from the guide: the system's browser
            "url" if arg.starts_with("https://") || arg.starts_with("http://") => {
                ctx.open_url(egui::OpenUrl::new_tab(arg));
                self.status = format!("Opened {arg} in your browser.");
            }
            "vpscale" => {
                // the scale of the view placed in the selected viewport
                let view =
                    self.selection
                        .active
                        .and_then(|id| match &self.doc.model().get(id)?.kind {
                            ElementKind::Viewport(vp) => Some(vp.view),
                            _ => None,
                        });
                if let (Some(view), Ok(scale)) = (view, arg.parse::<f64>()) {
                    self.set_view_scale(view, scale);
                }
            }
            "pivot" => {
                if let Some(p) = index.and_then(|i| crate::modal::Pivot::ALL.get(i)) {
                    self.plan.pivot = *p;
                }
            }
            "orient" => {
                if let Some(o) = index.and_then(|i| crate::modal::Orient::ALL.get(i)) {
                    crate::modal::set_rotate_orient(*o);
                }
            }
            "cursor.clear" => self.plan.cursor = None,
            "wp.level" => {
                self.workplane = None;
                self.geometry_key = None;
                self.status = "Workplane: the working level.".into();
            }
            "wp.face" => {
                self.plane_pick = Some(PlanePick::Face);
                self.status = "Click a face in the 3D view to put the workplane on it.".into();
            }
            "wp.points" => {
                self.plane_pick = Some(PlanePick::Points(vec![]));
                self.status = "Click three points in the 3D view for the workplane.".into();
            }
            "wp.offset" => {
                if let Some((_, z)) = self.work_level() {
                    self.set_workplane(
                        Workplane::ground(z),
                        format!("level at {}", crate::units::elevation(z)),
                    );
                }
            }
            "snap.on" => self.plan.snap.on = !self.plan.snap.on,
            "step.off" => {
                self.plan.snap.increment = false;
                self.status =
                    "Grid snap off: points and drags go exactly where the pointer is.".into();
            }
            "marks.toggle" => {
                let s = &mut self.plan.snap;
                s.marks = !s.marks;
                self.status = if s.marks {
                    format!(
                        "Distance marks every {} while dragging: near one, the drag holds on it.",
                        s.spacing_label()
                    )
                } else {
                    "Distance marks off.".into()
                };
                self.shell.settings.distance_marks = s.marks;
                self.shell.settings.store();
            }
            "step.custom" => {
                self.plan.step_prompt =
                    Some(crate::units::len(self.plan.snap.step_um as f64 / 1e6));
            }
            "step" => {
                if let Some(um) = index.and_then(|i| u32::try_from(i).ok()) {
                    self.plan.snap.increment = true;
                    self.plan.snap.step_um = um;
                    self.status = format!(
                        "Grid snap {}: points land on its grid. Alt+S turns it off; Alt frees one move.",
                        self.plan.snap.step_label()
                    );
                    self.shell.settings.snap_spacing_um = um;
                    self.shell.settings.store();
                }
            }
            "snap.div" => {
                let n = index.unwrap_or(0).min(20) as u8;
                let n = if n == 1 { 2 } else { n };
                crate::snaps::set_divisions(n);
                if n > 0 {
                    let r = &mut self.shell.settings.recent_divisions;
                    r.retain(|x| *x != n);
                    r.insert(0, n);
                    r.truncate(2);
                    self.shell.settings.store();
                }
                self.plan.forget_snaps();
                self.snaps_3d = None;
                self.status = if n == 0 {
                    "Divisions off.".into()
                } else {
                    format!("Divisions: every edge snaps at its {n} even parts while you place or draw anything.")
                };
            }
            "snap.all" | "snap.none" => {
                let on = head == "snap.all";
                self.plan
                    .snap
                    .switches()
                    .into_iter()
                    .for_each(|(_, v)| *v = on);
            }
            "snap" => {
                if let Some((_, v)) =
                    index.and_then(|i| self.plan.snap.switches().into_iter().nth(i))
                {
                    *v = !*v;
                }
            }
            "new.layered" => {
                self.path = new_path("Layered walls");
                self.load_model(creabim_core::sample::layered_house());
                self.status = "Loaded the layered sample house.".into();
            }
            "new.typed" => {
                self.path = new_path("Wall types");
                self.load_model(creabim_core::sample::typed_house());
                self.status =
                    "Loaded the typed sample house: see Materials › Types… for its types.".into();
            }
            "new.furnished" => {
                self.path = new_path("Furnished house");
                self.load_model(creabim_core::sample::furnished_house());
                self.status =
                    "Loaded the furnished sample house: see Create › Catalogue… to place more."
                        .into();
            }
            "new.garden" => {
                self.path = new_path("House in its garden");
                self.load_model(creabim_core::garden::garden_house());
                self.status =
                    "Loaded the garden sample: see Create › Site and Create › Building elements."
                        .into();
            }
            // this version's starter house
            "new.sample" => {
                self.path = new_path("Ridgeline House");
                self.load_model(crate::app::appmenu::ridgeline_house());
                self.status = STARTER_LOADED.into();
            }
            // the starter house of 1.1
            "new.maple" => {
                self.path = new_path("Maple Lane House");
                self.load_model(creabim_core::starter::starter_house());
                self.status = STARTER_LOADED.into();
            }
            "new.simple" => {
                self.path = new_path("Simple house");
                self.load_model(sample_house());
                self.status = "Loaded the simple sample house.".into();
            }
            "new.buried" => {
                self.path = new_path("Beam in a wall");
                self.load_model(creabim_core::sample::sample_with_buried_beam().0);
                self.status = "Loaded the sample house with a beam inside its south wall. Hover it in Section A and press Tab to reach the beam.".into();
            }
            "new.empty" => {
                // two levels, in the units of the standard picked at
                // setup, else the units in use
                // (or of the one picked in New project, for this project)
                let mut units = self.doc.model().units();
                let standard = if arg.is_empty() { self.shell.settings.standard.as_str() } else { arg };
                if let Some(l) = splash::standard_length(standard) {
                    units.length = l;
                }
                let mut m = Document::new();
                let mut tx = Transaction::new("New project");
                for (name, z) in [("Level 0", 0.0), ("Level 1", 3.0)] {
                    tx = tx.insert(Element::new(
                        name,
                        ElementKind::Level(creabim_core::Level {
                            elevation: z,
                            height: 3.0,
                        }),
                    ));
                }
                // the styles every project starts with (MU85)
                for (name, s) in creabim_core::annotation::starter_text_styles() {
                    tx = tx.insert(Element::new(name, ElementKind::TextStyle(s)));
                }
                let _ = m.commit(tx);
                let tx = creabim_core::units::set_units(m.model(), units);
                let _ = m.commit(tx);
                // the drawing standard picked at setup: its sheets and
                // scales (ISO is a project without one)
                if standard == "us" {
                    let mut pack = creabim_core::styles::StylePack::standard();
                    pack.standard = Some(creabim_core::standards::us());
                    let tx = creabim_core::styles::apply_pack(m.model(), &pack);
                    let _ = m.commit(tx);
                }
                self.load_model(m.model().clone());
                self.path = new_path("Untitled");
                self.status =
                    "New project with two levels. Pick the Wall tool to start drawing.".into();
            }
            "new.project" => self.open_new_project(ctx),
            "app.splash" => self.splash_open = true,
            "new.sheet" => self.new_sheet(),
            "levels" => self.open_levels(),
            "project.info" => self.open_project_tab(),
            "level.add" => self.add_level_above(),
            "browser.sort" => {
                if let Some(g) = crate::browser::ViewGroup::from_key(arg) {
                    let tx = crate::browser::sort_auto(self.doc.model(), g);
                    let r = self.doc.commit(tx);
                    self.report(
                        r,
                        match g {
                            crate::browser::ViewGroup::Plans => {
                                "Floor plans sorted by level again; new plans take their place by level.".to_string()
                            }
                            g => format!("{} back in their automatic order.", g.title()),
                        },
                    );
                }
            }
            "units" => self.levels.units_open = true,
            "marks" => self.marks.open = true,
            "graphicstyles" => self.gstyles.open = true,
            "libraries" => self.open_libraries(None),
            "underlay" => {
                if let Some((view, level)) = self.active_plan() {
                    let u = match arg {
                        "below" => Underlay::Below,
                        "above" => Underlay::Above,
                        "none" => Underlay::None,
                        _ => match by_uuid(self) {
                            Some(l) if l != level => Underlay::Level(l),
                            _ => return,
                        },
                    };
                    self.set_underlay(view, u);
                }
            }
            "new.schedule" => {
                if let Some(c) = index.and_then(|i| ScheduleCategory::ALL.get(i)) {
                    self.new_schedule(*c);
                }
            }
            "open" => self.open_dialog(),
            "open.path" => {
                let path = self.path.clone();
                self.open_path(&path);
            }
            "recent" => {
                if let Some(f) = index.and_then(|i| self.recent.files.get(i)).cloned() {
                    self.open_path(&f);
                }
            }
            "recent.clear" => self.recent.clear(),
            "save" => self.save(),
            "saveas" => self.save_as(),
            "link" => {
                if let Some(path) = crate::files::pick_open() {
                    self.link_file(&path);
                }
            }
            "live" => self.live_reload = !self.live_reload,
            "import.report" => {
                // the last import's card again, where the app has its pages
                if self.mark_cards() {
                    self.reopen_import_card();
                    ctx.request_repaint();
                } else if let Some(r) = &mut self.import_report {
                    r.open = true;
                }
            }
            "reload" => {
                let linked = crate::files::linked_files(self.doc.model());
                if let Some(f) = index.and_then(|i| linked.get(i)) {
                    self.reload(f);
                }
            }
            "export.glb" | "export.fcstd" | "export.ifcx" | "export.ifc4" | "export.pdf"
            | "export.svg" | "export.xlsx" => self.export(head),
            "export.folder" => self.open_export_folder(),
            "footing.foundation" => self.show_foundation_plan(),
            "placement" => {
                self.placement = match index {
                    Some(1) => PlanPlacement::Window,
                    Some(2) => PlanPlacement::Overlay,
                    Some(3) => PlanPlacement::Hidden,
                    _ => PlanPlacement::Docked,
                }
            }
            "grids3d" => self.toggle_grids_3d(),
            "ground.grid" => {
                let s = &mut self.shell.settings;
                s.ground_grid = !s.ground_grid;
                s.store();
                self.geometry_key = None;
                self.status = if self.shell.settings.ground_grid {
                    "Ground grid shown. View > 3D view sets its spacing.".into()
                } else {
                    "Ground grid hidden.".into()
                };
            }
            "xray" => self.set_xray(!self.shell.settings.xray),
            "ground3d" => {
                if let Some(g) = index.and_then(|i| creabim_core::GroundState::ALL.get(i)) {
                    self.set_ground_3d(*g);
                }
            }
            "render3d" => {
                if let Some(m) = index.and_then(|i| RenderMode::ALL.get(i)) {
                    self.set_look(*m);
                }
            }
            "viewstyle" => {
                if let Some(st) = index.and_then(|i| ViewStyle::ALL.get(i)) {
                    self.set_view_style(*st);
                }
            }
            "guides.toggle" => self.set_guides_hidden(!self.shell.settings.guides_hidden),
            "ground.custom" => {
                let now = ground_spacing(self.shell.settings.ground_grid_spacing);
                self.shell.ground_custom = Some(crate::units::len(now));
            }
            "ground.spacing" => {
                if let Some(v) = index.and_then(|i| GROUND_SPACINGS.get(i)) {
                    let s = &mut self.shell.settings;
                    s.ground_grid_spacing = *v;
                    s.ground_grid = true;
                    s.store();
                    self.geometry_key = None;
                    self.status = format!("Ground grid every {v} m.");
                }
            }
            "plan3d" => self.plan_in_3d = !self.plan_in_3d,
            "divide" => self.open_divide(),
            "split" => self.start_split(),
            "frame" => self.frame_pane(false),
            "nav.fitsel" => self.frame_selection_here(),
            "sched.csv" => self.schedule_export = Some("csv".into()),
            "sched.xlsx" => self.schedule_export = Some("xlsx".into()),
            "railpick" => {
                if let Some(e) = self.doc.model().iter().find(|e| e.id.0.to_string() == arg) {
                    let (id, opens) = (e.id, matches!(e.kind, ElementKind::View(_) | ElementKind::Sheet(_)));
                    self.selection.only(id);
                    if opens {
                        self.active_view = Some(id);
                    }
                    self.face = None;
                }
            }
            "browser.fold" => {
                self.shell.settings.left_folded = !self.shell.settings.left_folded;
                self.shell.settings.store();
            }
            "props.fold" => {
                self.shell.settings.right_folded = !self.shell.settings.right_folded;
                self.shell.settings.store();
            }
            "mode.object" => {
                if self.plan.edit.is_some() {
                    self.toggle_edit_mode();
                }
            }
            "mode.edit" => {
                if self.plan.edit.is_none() {
                    self.toggle_edit_mode();
                }
            }
            "view.zoomscale" => {
                if let Some(v) = self.active_view {
                    self.plan.zoom_to_scale = Some(v);
                    self.status = "At the drawing's scale: 1 mm on paper is 1 mm on screen.".into();
                }
            }
            "nav.orbitsel" => self.orbit_selection = !self.orbit_selection,
            "nav.ortho" => self.camera.ortho = !self.camera.ortho,
            "nav.zoomin" => self.camera.zoom(2.0),
            "nav.zoomout" => self.camera.zoom(-2.0),
            "nav.look" => {
                if let Some(l) = index.and_then(|i| crate::camera::Look::ALL.get(i)) {
                    self.camera.look(*l);
                }
            }
            "palette" => self.open_palette(ctx),
            "theme" | "theme.next" => {
                let t = match Theme::from_id(arg) {
                    Some(t) => t,
                    None => self.shell.theme.next(),
                };
                self.set_theme(ctx, t);
            }
            "ui.classic" => {
                self.shell.classic = !self.shell.classic;
                if let Some(web) = &mut self.shell.web {
                    web.hide_all(self.shell.classic);
                }
            }
            "view.shaded" | "view.tag" | "view.tagfixtures" | "view.reveal" | "view.colour"
            | "view.xray" | "view.rooms" | "view.ceiling" | "view.tagrooms" | "view.roomtags"
            // (these four were handled below but never routed here: MU22)
            | "view.tempdims" | "view.ranges" | "view.reference" | "sheet.preview" | "sheet.text"
            | "sheet.north" | "sheet.scalebar" | "sheet.pdf" | "export.sheets" | "sheet.cloud"
            | "sheet.revtable" | "tag.all" | "sheet.keyplan" | "sheet.guides" => {
                self.view_action(head)
            }
            "cleanup.stacked" => {
                // copies left on their originals (MU24): one undo step
                let ids = creabim_core::checks::stacked_copies(self.doc.model());
                if ids.is_empty() {
                    self.status = "No element sits on a copy of itself.".into();
                } else {
                    let n = ids.len();
                    let tx = creabim_core::Transaction::new("Remove copies on their originals")
                        .remove_all_cascade(self.doc.model(), &ids);
                    let r = self.doc.commit(tx);
                    self.selection.clear();
                    self.report(r, format!("Removed {n} copies that sat exactly on their originals. Ctrl+Z brings them back."));
                }
            }
            "cleanup.overlap" => {
                // walls along each other (MU27): the first time it selects
                // them; asked again with them selected, it merges each pair
                let pairs = creabim_core::checks::overlapping_walls(self.doc.model());
                if pairs.is_empty() {
                    self.status = "No walls overlap along their length.".into();
                    return;
                }
                let all: Vec<ElementId> = pairs.iter().flat_map(|(a, b)| [*a, *b]).collect();
                let picked = all.iter().all(|id| self.selection.contains(*id));
                if !picked {
                    self.selection.extend(all.iter().copied(), false);
                    self.status = format!(
                        "{} pair(s) of walls overlap along their length (selected). Run Overlapping walls again to merge each pair into one wall covering both.",
                        pairs.len()
                    );
                    return;
                }
                let model = self.doc.model();
                let mut tx = creabim_core::Transaction::new("Merge overlapping walls");
                let mut gone: Vec<ElementId> = vec![];
                for (a, b) in &pairs {
                    if gone.contains(a) || gone.contains(b) {
                        continue;
                    }
                    let (Some(ea), Some(eb)) = (model.get(*a), model.get(*b)) else { continue };
                    let (ElementKind::Wall(wa), ElementKind::Wall(wb)) = (&ea.kind, &eb.kind) else { continue };
                    // the thicker one stays, stretched over both
                    let (keep, k, other, o) = if wa.thickness >= wb.thickness { (ea, wa, eb, wb) } else { (eb, wb, ea, wa) };
                    let d = k.direction();
                    let along = |p: glam::DVec2| (p - k.start).dot(d);
                    let lo = [0.0, along(o.start), along(o.end)].into_iter().fold(f64::MAX, f64::min);
                    let hi = [k.length(), along(o.start), along(o.end)].into_iter().fold(f64::MIN, f64::max);
                    let mut after = keep.clone();
                    if let ElementKind::Wall(w) = &mut after.kind {
                        w.start = k.start + d * lo;
                        w.end = k.start + d * hi;
                    }
                    tx = tx.replace(keep.clone(), after);
                    gone.push(other.id);
                }
                let tx = tx.remove_all_cascade(model, &gone);
                let r = self.doc.commit(tx);
                self.selection.clear();
                self.report(r, format!("Merged {} pair(s) of overlapping walls. Ctrl+Z undoes it.", gone.len()));
            }
            "rooms.fromnotes" => {
                self.status = crate::roomtool::rooms_from_notes(&mut self.doc);
            }
            "check.doorways" => {
                let (ids, msg) = crate::roomtool::check_doorways(self.doc.model());
                if !ids.is_empty() {
                    self.selection.extend(ids, false);
                }
                self.status = msg;
            }
            "check.stairs" => {
                let (ids, msg) = crate::memberstool::check_stairs(self.doc.model());
                if !ids.is_empty() {
                    self.selection.extend(ids, false);
                }
                self.status = msg;
            }
            "check.layers" => {
                let (ids, msg) = crate::roomtool::check_layers(self.doc.model());
                if !ids.is_empty() {
                    self.selection.extend(ids, false);
                }
                self.status = msg;
            }
            "hidden.colour3d" => self.set_hidden_colour_3d(!self.shell.settings.hidden_colour_3d),
            "object" => {
                if crate::objecttool::toggle_option(&mut self.plan, arg) {
                    self.catalogue.open = true;
                }
            }
            "part" => crate::partstool::toggle_option(&mut self.plan, arg),
            "member" => {
                if arg == "square" || arg == "round" {
                    self.plan.types.remove(&creabim_core::TypeCategory::Column);
                }
                crate::memberstool::toggle_option(&mut self.plan, arg)
            }
            "slab.mode" => {
                if let Some(m) = index.and_then(|i| crate::slabtool::SlabMode::ALL.get(i)) {
                    crate::slabtool::pick(ctx, Some(*m), None);
                }
            }
            "slab.arc" => crate::slabtool::pick(ctx, None, Some(arg == "1")),
            "sel.shape" => {
                use crate::boxsel::Shape;
                crate::boxsel::set_shape(match arg {
                    "1" => Shape::Circle,
                    "2" => Shape::Lasso,
                    _ => Shape::Box,
                });
            }
            "sel.combine" => {
                use crate::boxsel::Combine;
                crate::boxsel::set_combine(match arg {
                    "1" => Combine::Add,
                    "2" => Combine::Subtract,
                    "3" => Combine::Invert,
                    "4" => Combine::Intersect,
                    _ => Combine::New,
                });
            }
            "sel.pick" => {
                let k: u8 = arg.parse().unwrap_or(0);
                crate::handles::set_pick(if crate::handles::pick() == k { 0 } else { k });
            }
            "sel.invert" => self.select_invert(),
            "trim.corner" => self.trim_corner(),
            // the Select tab, where every way of selecting lives (wave 3)
            "seltab" => {
                self.prop_tab = PropTab::Select;
                self.shell.settings.props_open.insert("selsimilar".into(), true);
            }
            "sel.linked" => self.select_linked(),
            "sel.random" => self.select_random(),
            "sel.kind" => self.select_kind(arg),
            "sketch.shape" => {
                if let Some(s) = index.and_then(|i| crate::sketch::Shape::ALL.get(i)) {
                    crate::sketch::pick_shape(ctx, *s);
                }
            }
            "note.leader" => self.plan.anno.leader = !self.plan.anno.leader,
            "detail.kind" => {
                if let Some(k) = index.and_then(|i| creabim_core::DetailKind::ALL.get(i)) {
                    self.plan.details.pick(*k);
                    self.plan.details.path.clear();
                }
            }
            "rotate.step" => {
                // Rotate's step, picked in its row (MU142)
                if let Some(d) = index.and_then(|i| [0.0, 15.0, 45.0, 90.0].get(i)) {
                    crate::modal::set_rotate_step(*d);
                }
            }
            "mirror.across" => {
                if let Some(a) = index.and_then(|i| crate::modal::Across::ALL.get(i)) {
                    crate::modal::set_mirror_across(*a);
                }
            }
            "mirror.copy" => crate::modal::set_mirror_copy(!crate::modal::mirror_copy()),
            // Scale's row (MU142)
            "scale.pivot" => crate::modal::set_scale_corner(index == Some(0)),
            "scale.keep" => crate::modal::set_scale_free(index == Some(1)),
            "scale.by" => crate::modal::set_scale_by_ref(index == Some(1)),
            "scale.step" => {
                if let Some(d) = index.and_then(|i| [0.0, 0.05, 0.1, 0.25].get(i)) {
                    crate::modal::set_scale_step(*d);
                }
            }
            "callout.scale" => {
                if let Some(s) = index.and_then(|i| crate::detailtool::CALLOUT_SCALES.get(i)) {
                    self.plan.details.callout_scale = *s;
                }
            }
            _ if self.join_action(head, arg) => {}
            _ if self.crop_action(head, arg) => {}
            _ if self.renders_action(ctx, head, arg) => {}
            _ if self.profiles_action(head, arg) => {}
            _ if self.notes_action(ctx, head, arg) => {}
            _ if self.masters_action(head, arg) => {}
            _ if self.patterns_action(head, arg) => {}
            _ if self.log_action(ctx, head) => {}
            _ if self.time_action(ctx, head, arg) => {}
            _ if self.kinds_action(head, arg) => {}
            _ if self.groups_action(head, arg) => {}
            "kinds.create" => {
                // Create › Door or Window: the tool, its picker open
                self.plan.tool = if arg == "window" { Tool::Window } else { Tool::Door };
                self.plan.cancel();
                self.shell.open_kinds = true;
            }
            "kinds.opened" => self.shell.open_kinds = false,
            _ if self.similar_action(head, arg) => {}
            _ if self.app_action(ctx, head, arg) => {}
            _ if self.help_action(head, arg) => {}
            _ if self.wall_action(head, arg) => {}
            _ if self.level_action(head, arg) => {}
            _ => {
                if !self.materials_action(head, arg) && !self.drafting_action(head, arg) {
                    self.grid_vis_action(head, arg);
                }
            }
        }
    }

    /// Why `t` can't be used in the view in front, if it can't: the
    /// annotation tools on a sheet, in 3D or a schedule (MU24).
    pub(super) fn tool_unusable(&self, t: Tool) -> Option<&'static str> {
        let front = self
            .active_view
            .and_then(|v| match self.doc.model().get(v).map(|e| &e.kind) {
                Some(ElementKind::Sheet(_)) => Some("sheet"),
                Some(ElementKind::View(view)) => Some(match view.kind {
                    ViewKind::Plan { .. }
                    | ViewKind::Section { .. }
                    | ViewKind::LongSection { .. }
                    | ViewKind::BoreholeLog { .. }
                    | ViewKind::WalkSection { .. } => "drawing",
                    ViewKind::Axonometric { .. } => "3d",
                    ViewKind::Schedule { .. } => "schedule",
                }),
                _ => None,
            });
        let measures = matches!(
            t,
            Tool::Dimension
                | Tool::Angle
                | Tool::Radius
                | Tool::Diameter
                | Tool::ArcLength
                | Tool::SpotLevel
                | Tool::Tag
                | Tool::Detail
        );
        match front? {
            "sheet" if measures => {
                Some("on a sheet, double-click a view to dimension or tag inside it")
            }
            "3d" if measures || t == Tool::Note || t == Tool::Symbol => {
                Some("annotation goes in plans, sections and sheets, not the 3D view")
            }
            "schedule" if measures || t == Tool::Note || t == Tool::Symbol => {
                Some("a schedule takes no annotation")
            }
            _ => None,
        }
    }

    /// A key plan on `sheet` (Annotate step 5): the lowest floor plan
    /// again as a view of its own at 1:500, its annotation hidden, placed
    /// over the title block, all one undo step.
    pub(super) fn add_key_plan(&mut self, sheet: ElementId) {
        use creabim_core::visibility::Category;
        let model = self.doc.model();
        let plan = model
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::View(v)
                    if matches!(v.kind, creabim_core::ViewKind::Plan { .. })
                        && v.callout.is_none() =>
                {
                    let lvl = match v.kind {
                        creabim_core::ViewKind::Plan { level, .. } => {
                            creabim_core::members::elevation(model, level)
                        }
                        _ => 0.0,
                    };
                    Some((lvl, e.clone(), v.clone()))
                }
                _ => None,
            })
            .filter(|(z, ..)| *z >= -0.5)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let Some((_, from, mut v)) = plan else {
            self.status = "A key plan needs a floor plan to draw from.".into();
            return;
        };
        v.scale = 500.0;
        v.browser_order = None;
        for c in [
            Category::Dimensions,
            Category::Tags,
            Category::Notes,
            Category::Grids,
            Category::SectionMarks,
            Category::Objects,
            Category::Guides,
            Category::Sketches,
        ] {
            if !v.hidden.categories.contains(&c) {
                v.hidden.categories.push(c);
            }
        }
        let view = Element::new(format!("Key plan ({})", from.name), ElementKind::View(v));
        let view_id = view.id;
        let Some(s) = creabim_geom::sheet::sheet_of(model, sheet) else {
            return;
        };
        let m = creabim_geom::sheet::MARGIN;
        let top =
            creabim_geom::sheet::title_block_box(model, sheet).map_or(m + 45.0, |(_, hi)| hi.y);
        let at = glam::DVec2::new(s.width - m - 40.0, top + 30.0);
        // the view first, then its viewport, made against the model with
        // the view in it; one undo step
        if let Err(e) = self
            .doc
            .commit(Transaction::new("Add key plan").insert(view))
        {
            self.status = format!("Couldn't do that: {e}");
            return;
        }
        let vp = creabim_geom::sheet::new_viewport(self.doc.model(), sheet, view_id, at);
        let e = Element::new("Key plan", ElementKind::Viewport(vp));
        let id = e.id;
        let r = self
            .doc
            .commit_merged(Transaction::new("Add key plan").insert(e));
        self.selection.only(id);
        self.report(r, "Key plan placed over the title block: the whole floor at 1:500. Drag it where it goes; it is a view of its own in the browser.");
    }

    pub(super) fn place_view(&mut self, sheet: ElementId, view: ElementId) {
        let at = sheets::free_spot(self.doc.model(), sheet, view);
        let vp = sheets::new_viewport(self.doc.model(), sheet, view, at);
        let e = Element::new("Viewport", ElementKind::Viewport(vp));
        let id = e.id;
        let r = self
            .doc
            .commit(Transaction::new("Place view on sheet").insert(e));
        self.selection.only(id);
        self.report(
            r,
            "Placed the view. Drag it to where it belongs on the sheet.",
        );
    }

    pub(super) fn export(&mut self, what: &str) {
        self.export_inner(what);
        // what was exported can be found from the status bar
        if self.status.starts_with("Exported ") {
            self.last_export = Some(self.status.clone());
        }
    }

    /// Where to write an export: asked with the system's save window, which
    /// starts next to the project with `default`'s name.
    pub(super) fn export_to(&mut self, default: String) -> Option<String> {
        let out = if cfg!(test) {
            Some(default)
        } else {
            Self::ask_export(default)
        }?;
        self.last_export_dir = std::path::absolute(&out)
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));
        Some(out)
    }

    fn ask_export(default: String) -> Option<String> {
        let ext = std::path::Path::new(&default)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();
        let full = std::path::absolute(&default)
            .map(|p| p.display().to_string())
            .unwrap_or(default);
        let asked = std::time::Instant::now();
        let picked = crate::files::pick_save(&full, "Export", &ext);
        // a save window that closes at once never opened (no file picker
        // on this system): write next to the project as before
        if picked.is_none() && asked.elapsed() < std::time::Duration::from_millis(300) {
            return Some(full);
        }
        picked
    }

    /// Open the folder the last export went to, in the system's file browser.
    pub(super) fn open_export_folder(&mut self) {
        let Some(dir) = self.last_export_dir.clone() else {
            return;
        };
        let r = crate::files::open_folder(&dir);
        self.report(r, format!("Opened {}.", dir.display()));
    }

    fn export_inner(&mut self, what: &str) {
        if self.mark_cards() && matches!(what, "export.glb" | "export.fcstd" | "export.ifcx") {
            self.open_export_card(what);
            return;
        }
        match what {
            "export.glb" => {
                let Some(out) = self.export_to(with_extension(&self.path, "glb")) else {
                    return;
                };
                let r = std::fs::write(&out, creabim_exchange::gltf::export_glb(self.doc.model()));
                self.report(r, format!("Exported {out}. In Blender: File > Import > glTF, or use the CreaBIM add-on."));
            }
            "export.fcstd" => {
                let Some(out) = self.export_to(with_extension(&self.path, "FCStd")) else {
                    return;
                };
                let r =
                    creabim_exchange::freecad::export_fcstd(self.doc.model(), &stem(&self.path))
                        .map_err(|e| e.to_string())
                        .and_then(|b| std::fs::write(&out, b).map_err(|e| e.to_string()));
                self.report(r, format!("Exported {out}"));
            }
            "export.ifcx" => {
                let Some(out) = self.export_to(with_extension(&self.path, "ifcx")) else {
                    return;
                };
                let r = std::fs::write(&out, creabim_ifc::export_ifcx(self.doc.model()));
                self.report(r, format!("Exported {out}"));
            }
            // first how walls and slabs go out (MU20), then where
            "export.ifc4" => self.ifc_dialog = true,
            "export.xlsx" => {
                let Some(out) = self.export_to(with_extension(&self.path, "xlsx")) else {
                    return;
                };
                let model = self.doc.model();
                let n = creabim_geom::xlsx::schedules(model).len();
                let r = creabim_geom::xlsx::schedules_xlsx(model, &stem(&self.path))
                    .and_then(|b| std::fs::write(&out, b).map_err(|e| e.to_string()));
                self.report(r, format!("Exported {n} schedules and a summary to {out}"));
            }
            "export.pdf" if !cfg!(test) => {
                self.sheets_export_open = true;
            }
            "export.pdf" => {
                let Some(out) = self.export_to(with_extension(&self.path, "pdf")) else {
                    return;
                };
                if sheets::sheets(self.doc.model()).is_empty() {
                    self.status = "There are no sheets to export yet.".into();
                } else {
                    let pdf = sheets::sheets_pdf(self.doc.model(), &self.sheet_info());
                    let r = std::fs::write(&out, pdf);
                    let n = sheets::sheets(self.doc.model()).len();
                    self.report(r, format!("Exported {n} sheets to {out}"));
                }
            }
            "export.svg" => {
                if let Some(sheet) = self.active_view.filter(|id| self.is_sheet(*id)) {
                    let number = sheets::sheet_of(self.doc.model(), sheet)
                        .map(|s| s.number.clone())
                        .unwrap_or_default();
                    let Some(out) =
                        self.export_to(with_extension(&self.path, &format!("{number}.svg")))
                    else {
                        return;
                    };
                    if let Some(svg) =
                        sheets::sheet_svg(self.doc.model(), sheet, &self.sheet_info())
                    {
                        let r = std::fs::write(&out, svg);
                        self.report(r, format!("Exported {out}"));
                    }
                } else if let Some(view) = self.active_view {
                    let name = self
                        .doc
                        .model()
                        .get(view)
                        .map(|e| e.name.replace(' ', "-"))
                        .unwrap_or_default();
                    let scale = self.view(view).map_or(100.0, |v| v.scale);
                    let Some(out) =
                        self.export_to(with_extension(&self.path, &format!("{name}.svg")))
                    else {
                        return;
                    };
                    if let Some(d) = self.drawing(view) {
                        let r = std::fs::write(&out, d.to_svg(scale));
                        self.report(r, format!("Exported {out} at 1:{scale:.0}"));
                    }
                }
            }
            _ => {}
        }
    }

    /// The value the active view's number field shows: cut height, depth or
    /// altitude.
    fn view_number(&self) -> Option<(&'static str, f64, &'static str, ElementId)> {
        let id = self.active_view?;
        let v = self.view(id)?;
        match &v.kind {
            ViewKind::Plan { cut_height, .. } => Some(("Cut height", *cut_height, "m", id)),
            ViewKind::Section { depth, .. } => Some(("Depth", *depth, "m", id)),
            ViewKind::Axonometric { altitude, .. } => Some(("Altitude", *altitude, "°", id)),
            ViewKind::Schedule { .. }
            | ViewKind::LongSection { .. }
            | ViewKind::BoreholeLog { .. }
            | ViewKind::WalkSection { .. } => None,
        }
    }

    pub(super) fn set_view_number(&mut self, value: f64) {
        let Some(id) = self.active_view else { return };
        let Some(mut v) = self.view(id).cloned() else {
            return;
        };
        match &mut v.kind {
            ViewKind::Plan { cut_height, .. } => *cut_height = value.clamp(0.1, 200.0),
            ViewKind::Section { depth, .. } => *depth = value.clamp(0.1, 200.0),
            ViewKind::Axonometric { altitude, .. } => *altitude = value.clamp(-89.0, 89.0),
            ViewKind::Schedule { .. }
            | ViewKind::LongSection { .. }
            | ViewKind::BoreholeLog { .. }
            | ViewKind::WalkSection { .. } => return,
        }
        let r = self
            .doc
            .edit_merged(id, "Change view", |e| e.kind = ElementKind::View(v.clone()));
        self.report(r, "View changed.");
    }

    /// Draw `view` at 1:`scale`. Each sheet it is placed on keeps the
    /// middle of it where it was, so the viewport grows or shrinks round
    /// its centre. One undo step.
    pub(super) fn set_view_scale(&mut self, view: ElementId, scale: f64) {
        let Some(before) = self.doc.model().get(view).cloned() else {
            return;
        };
        let ElementKind::View(v) = &before.kind else {
            return;
        };
        let old = v.scale;
        if (old - scale).abs() < 1e-9 || scale < 1.0 {
            return;
        }
        let mid = self
            .drawing(view)
            .and_then(|d| d.bounds())
            .map(|(lo, hi)| (lo + hi) / 2.0);
        let mut after = before.clone();
        if let ElementKind::View(v) = &mut after.kind {
            v.scale = scale;
        }
        let mut tx = Transaction::new("View scale").replace(before, after);
        for e in self.doc.model().iter() {
            let ElementKind::Viewport(vp) = &e.kind else {
                continue;
            };
            if vp.view != view {
                continue;
            }
            // as it shows now (a viewport following a stair crops live)
            let (crop, target) = creabim_geom::sheet::viewport_frame(self.doc.model(), vp);
            let Some(m) = crop.map(|[lo, hi]| (lo + hi) / 2.0).or(mid) else {
                continue;
            };
            let live = creabim_core::Viewport {
                target,
                crop,
                ..vp.clone()
            };
            let mut moved = e.clone();
            if let ElementKind::Viewport(p) = &mut moved.kind {
                p.center = sheetview::rescaled_center(&live, m, old, scale).round();
            }
            tx = tx.replace(e.clone(), moved);
        }
        let r = self.doc.commit(tx);
        self.report(
            r,
            format!("Drawn at 1:{scale:.0}, on its sheets too: each viewport grows or shrinks round its centre."),
        );
    }

    /// The scales a placed view can be drawn at, for its right-click menu.
    fn viewport_scale_items(&self, view: ElementId) -> Vec<Item> {
        let now = self.view(view).map_or(0.0, |v| v.scale);
        let std = creabim_core::standards::of(self.doc.model());
        let mut items: Vec<Item> = std
            .scales
            .iter()
            .map(|s| {
                Item::new(format!("vpscale:{:.0}", s.ratio), s.label.clone())
                    .checked((s.ratio - now).abs() < 1e-6)
            })
            .collect();
        if !std.offers(now) {
            items.push(Item::Separator);
            items.push(
                Item::new(
                    "vpscale.custom",
                    format!("1:{now:.0} (custom, set in Properties)"),
                )
                .checked(true)
                .enabled(false),
            );
        }
        items
    }

    /// How the 3D view can draw, the one it uses ticked.
    fn render_items(&self) -> Vec<Item> {
        RenderMode::ALL
            .iter()
            .enumerate()
            .map(|(i, m)| {
                Item::new(format!("render3d:{i}"), m.label())
                    .checked(self.shell.settings.render_3d == *m)
                    .icon(m.icon())
                    .hint(m.hint())
            })
            .collect()
    }

    /// How the active section, elevation or 3D view can draw.
    fn view_style_items(&self) -> Vec<Item> {
        let now = self
            .active_view
            .and_then(|id| self.view(id))
            .map(|v| v.style);
        ViewStyle::ALL
            .iter()
            .enumerate()
            .map(|(i, s)| {
                Item::new(format!("viewstyle:{i}"), s.label())
                    .checked(now == Some(*s))
                    .icon(view_style_icon(*s))
            })
            .collect()
    }

    /// Draw the active view in `style`.
    pub(super) fn set_view_style(&mut self, style: ViewStyle) {
        let Some(id) = self.active_view else { return };
        let Some(mut v) = self.view(id).cloned() else {
            return;
        };
        v.style = style;
        let r = self
            .doc
            .edit_merged(id, "View style", |e| e.kind = ElementKind::View(v.clone()));
        self.report(r, format!("Drawn {}.", style.label().to_lowercase()));
    }

    fn view_action(&mut self, what: &str) {
        let Some(id) = self.active_view else { return };
        match what {
            "view.shaded" => {
                let Some(mut v) = self.view(id).cloned() else {
                    return;
                };
                v.style = if v.style == ViewStyle::Shaded {
                    ViewStyle::HiddenLine
                } else {
                    ViewStyle::Shaded
                };
                let r = self
                    .doc
                    .edit_merged(id, "Change view", |e| e.kind = ElementKind::View(v.clone()));
                self.report(r, "View changed.");
            }
            "view.rooms" | "view.ceiling" => {
                let Some(mut v) = self.view(id).cloned() else {
                    return;
                };
                if what == "view.rooms" {
                    v.room_colours = !v.room_colours;
                } else if let Some(said) = {
                    let up = !v.ceiling_plan;
                    super::set_looks_up(&mut v, up)
                } {
                    self.status = said;
                }
                let r = self
                    .doc
                    .edit_merged(id, "Change view", |e| e.kind = ElementKind::View(v.clone()));
                self.report(r, "View changed.");
                // turned to look up: each ceiling's height tagged (MU142)
                self.tag_ceilings_of(id);
            }
            "view.roomtags" => {
                let Some(mut v) = self.view(id).cloned() else {
                    return;
                };
                let on = !v.shows_room_tags();
                v.room_tags = Some(on);
                let r = self
                    .doc
                    .edit_merged(id, "Change view", |e| e.kind = ElementKind::View(v.clone()));
                self.report(
                    r,
                    if on {
                        "Room tags shown in this view."
                    } else {
                        "Room tags hidden in this view."
                    },
                );
            }
            "view.tagrooms" => {
                self.status = match crate::roomtool::tag_rooms(&mut self.doc, id) {
                    Ok(0) => "Every room on this plan is tagged already.".into(),
                    Ok(n) => format!("Tagged {n} rooms. Drag a tag to move it; pick what it shows in Properties."),
                    Err(e) => format!("Couldn't do that: {e}"),
                };
            }
            "view.reveal" => self.toggle_hidden_elements(id),
            "view.colour" => self.toggle_hidden_colour(id),
            "view.xray" => self.toggle_view_xray(id),
            "view.reference" => {
                // the Trace tab of the view in the focused area (MU19)
                if matches!(self.vis_target(false), Some(gridvis::VisTarget::View(_))) {
                    self.selection.clear();
                    self.prop_tab = PropTab::Trace;
                    self.status = "Add a reference in Properties > Trace: another view, a picture or a DXF drawing.".into();
                } else {
                    self.status = "Open a plan or a section first: references show in them.".into();
                }
            }
            "sheet.text" => {
                self.plan.tool = Tool::Note;
                self.plan.cancel();
                self.status = "Text on the sheet: click where it starts, then type.".into();
            }
            "sheet.picture" => {
                if let Some(sheet) = self.active_view.filter(|id| self.is_sheet(*id)) {
                    self.add_sheet_picture(sheet);
                }
            }
            "sheet.north" | "sheet.scalebar" => {
                self.set_sheet_symbol_last(what);
                if let Some(sheet) = self.active_view.filter(|id| self.is_sheet(*id)) {
                    self.add_paper_mark(sheet, what == "sheet.north");
                }
            }
            "sheet.pdf" => self.export_sheets_now(true),
            "sheet.guides" => {
                self.plan.sheet_guides = !self.plan.sheet_guides;
                self.status = if self.plan.sheet_guides {
                    "Guides on: drag one out of a ruler at the sheet's top or left edge.".into()
                } else {
                    "Guides hidden; they are kept with the sheet.".into()
                };
            }
            "sheet.keyplan" => {
                if let Some(sheet) = self.active_view.filter(|id| self.is_sheet(*id)) {
                    self.add_key_plan(sheet);
                }
            }
            "sheet.cloud" | "sheet.revtable" => {
                if let Some(sheet) = self.active_view.filter(|id| self.is_sheet(*id)) {
                    self.add_revision_mark(sheet, what == "sheet.revtable");
                }
            }
            "tag.all" => {
                // doors and windows, fixtures and rooms, at once (Annotate board)
                let Some(view) = self.active_view else { return };
                let a = planview::tag_openings(&mut self.doc, view).unwrap_or(0);
                let b = planview::tag_fixtures(&mut self.doc, view).unwrap_or(0);
                let c = crate::roomtool::tag_rooms(&mut self.doc, view).unwrap_or(0);
                self.status = match a + b + c {
                    0 => "Everything here is tagged already (tags go on plans and sections).".into(),
                    _ => format!("Tagged {a} doors and windows, {b} fixtures, {c} rooms. Drag a tag to move it."),
                };
            }
            "export.sheets" => self.sheets_export_open = true,
            "sheet.preview" => {
                self.print_preview = !self.print_preview;
                self.status = if self.print_preview {
                    "Print preview: the sheet as it prints, on white.".into()
                } else {
                    "Back to the working paper.".into()
                };
            }
            "view.ranges" => {
                self.plan.plan_ranges = !self.plan.plan_ranges;
                self.status = if self.plan.plan_ranges {
                    "Plan ranges: each plan's cut and how far it looks. Click a band to pick it, then drag its grips; double-click to type a value.".into()
                } else {
                    "Plan ranges hidden.".into()
                };
            }
            "view.grid" => {
                let s = &mut self.shell.settings;
                s.plan_grid = !s.plan_grid;
                s.store();
                self.status = if self.shell.settings.plan_grid {
                    "Grid on behind drawings.".into()
                } else {
                    "Grid off: plain paper behind drawings.".into()
                };
            }
            "view.tempdims" => {
                self.plan.temp_dims_off = !self.plan.temp_dims_off;
                self.shell.settings.temp_dims_off = self.plan.temp_dims_off;
                self.shell.settings.store();
                self.status = if self.plan.temp_dims_off {
                    "Temporary dimensions off: none show by what you select. The button turns them back on.".into()
                } else {
                    "Temporary dimensions on.".into()
                };
            }
            "view.tag" => {
                let r = planview::tag_openings(&mut self.doc, id);
                self.status = match r {
                    Ok(0) => "Every door and window here is tagged already.".into(),
                    Ok(n) => format!("Tagged {n} doors and windows. Drag a tag to move it."),
                    Err(e) => format!("Couldn't do that: {e}"),
                };
            }
            "view.tagfixtures" => {
                let r = planview::tag_fixtures(&mut self.doc, id);
                self.status = match r {
                    Ok(0) => "Every fixture here is tagged already (tags go on plans).".into(),
                    Ok(n) => {
                        format!("Tagged {n} fixtures with their type marks. Drag a tag to move it.")
                    }
                    Err(e) => format!("Couldn't do that: {e}"),
                };
            }
            _ => {}
        }
    }

    /// The tool options strip: what the tool in use (or the running Move
    /// or Rotate) can be set to. It has its own row under the tools, so it
    /// never covers a view or its buttons.
    /// A number set in the tool bar, or in Properties' "Next …" (MU18).
    pub(super) fn set_tool_number(&mut self, key: &str, v: f64) {
        match key {
            "wall.thickness" => {
                // a size of its own: new walls have no type
                self.plan.types.remove(&creabim_core::TypeCategory::Wall);
                self.plan.wall_thickness = v.clamp(0.05, 1.0)
            }
            k if k.starts_with("object.") => crate::objecttool::set_option(&mut self.plan, k, v),
            k if k.starts_with("part.") => crate::partstool::set_option(&mut self.plan, k, v),
            k if k.starts_with("member.") => {
                if k.contains("column") {
                    self.plan.types.remove(&creabim_core::TypeCategory::Column);
                }
                if k.contains("beam") {
                    self.plan.types.remove(&creabim_core::TypeCategory::Beam);
                }
                if k.contains("door_") {
                    self.plan.types.remove(&creabim_core::TypeCategory::Door);
                }
                if k.contains("window_") {
                    self.plan.types.remove(&creabim_core::TypeCategory::Window);
                }
                crate::memberstool::set_option(&mut self.plan, k, v)
            }
            "view.number" => self.set_view_number(v),
            "detail.width" => self.plan.details.width = v.clamp(0.0, 5.0),
            "dim.spacing" => {
                self.plan.dim_step_size = v.clamp(0.5, 100.0);
                self.plan.dim_step_mm = self.plan.dim_step_size;
            }
            _ => {}
        }
    }

    pub(super) fn tool_strip(&self, ctx: &egui::Context) -> (Value, Vec<Value>) {
        let seg = |items: Vec<Value>| json!({"kind": "seg", "items": items});
        let opt = |id: String, label: &str, on: bool, hint: &str| json!({"id": id, "label": label, "on": on, "hint": hint});
        let mut options = vec![];
        // Rotate's row: its step, as the mock-up's tool row (MU142)
        let running = self
            .plan
            .modal
            .as_ref()
            .map(|m| m.mode)
            .or(self.plan.armed.map(|(m, _)| m));
        if running == Some(crate::modal::Mode::Rotate) {
            // Pivot and Orientation, then Step (MU142 decision 9)
            options.push(json!({"kind": "menu", "key": "pivot", "label": "Pivot", "value": self.plan.pivot.label(), "icon": "crosshair",
                "hint": "What it turns about: the next turn takes it. Shift+right-click places the pivot cursor."}));
            options.push(json!({"kind": "menu", "key": "orient", "label": "Orientation", "value": crate::modal::rotate_orient().label(), "icon": "axis-3d",
                "hint": "What the protractor counts from: Global, the plan's east; Local, the object's front as picked up"}));
            let now = crate::modal::rotate_step();
            options.push(json!({"kind": "label", "label": "Step"}));
            options.push(seg([
                (
                    0.0,
                    "Free",
                    "Any angle; Ctrl for 15° steps; 0, 90, 180 and 270 lock as you pass them",
                ),
                (15.0, "15°", "In 15° steps"),
                (45.0, "45°", "In 45° steps"),
                (90.0, "90°", "In quarter turns"),
            ]
            .iter()
            .enumerate()
            .map(|(i, (d, l, h))| opt(format!("rotate.step:{i}"), l, (now - d).abs() < 1e-9, h))
            .collect()));
        }
        // Mirror's row: across what, and whether it copies (MU142)
        if running == Some(crate::modal::Mode::Mirror) {
            use crate::modal::{mirror_across, mirror_copy, Across};
            let now = mirror_across();
            options.push(json!({"kind": "label", "label": "Across"}));
            options.push(seg(Across::ALL
                .iter()
                .enumerate()
                .map(|(i, a)| opt(format!("mirror.across:{i}"), a.label(), *a == now, a.hint()))
                .collect()));
            options.push(seg(vec![opt(
                "mirror.copy".into(),
                "Copy",
                mirror_copy(),
                "Make a mirrored copy and keep the original (Ctrl for the moment)",
            )]));
        }
        // Scale's row: what it scales about, its proportions, by what, and
        // its step (MU142, Tool Rows and Scale REV01)
        if running == Some(crate::modal::Mode::Scale) {
            use crate::modal::{scale_by_ref, scale_corner, scale_free, scale_step};
            options.push(json!({"kind": "label", "label": "Pivot"}));
            options.push(seg(vec![
                opt(
                    "scale.pivot:0".into(),
                    "Opposite corner",
                    scale_corner(),
                    "About the corner opposite the one you take it by; Alt for its centre",
                ),
                opt(
                    "scale.pivot:1".into(),
                    "Centre",
                    !scale_corner(),
                    "About its centre, or the pivot picked in the status bar",
                ),
            ]));
            options.push(json!({"kind": "label", "label": "Keep"}));
            options.push(seg(vec![
                opt(
                    "scale.keep:0".into(),
                    "Proportions",
                    !scale_free(),
                    "The same factor every way; X or Y keeps it to one direction",
                ),
                opt(
                    "scale.keep:1".into(),
                    "Free",
                    scale_free(),
                    "Wider and deeper each by how far the pointer goes",
                ),
            ]));
            options.push(json!({"kind": "label", "label": "By"}));
            options.push(seg(vec![
                opt("scale.by:0".into(), "Factor", !scale_by_ref(),
                    "By how far the pointer goes, or a factor typed (2 doubles it)"),
                opt("scale.by:1".into(), "Reference", scale_by_ref(),
                    "Click the two ends of a length you know, then type its true length (an imported plan to its true size)"),
            ]));
            let now = scale_step();
            options.push(json!({"kind": "label", "label": "Step"}));
            options.push(seg([
                (0.0, "Free", "Any factor; Ctrl for steps of 0.1"),
                (0.05, "0.05", "In steps of 0.05"),
                (0.1, "0.1", "In steps of 0.1"),
                (0.25, "0.25", "In quarters"),
            ]
            .iter()
            .enumerate()
            .map(|(i, (d, l, h))| opt(format!("scale.step:{i}"), l, (now - d).abs() < 1e-9, h))
            .collect()));
        }
        if let Some(m) = &self.plan.modal {
            let verb = m.mode.verb();
            let icon = match m.mode {
                crate::modal::Mode::Move => "move",
                crate::modal::Mode::Rotate => "rotate-cw",
                crate::modal::Mode::Scale => "scaling",
                crate::modal::Mode::Mirror => "mirror",
                crate::modal::Mode::Offset => "offset",
                crate::modal::Mode::Grip => "spline",
                crate::modal::Mode::Inset => "shape-inset",
                crate::modal::Mode::Pull => "pull",
            };
            return (
                json!({"label": verb, "icon": icon, "help": crate::help::mode_page(m.mode)}),
                options,
            );
        }
        if let Some((m, _)) = self.plan.armed {
            let verb = format!("{m:?}");
            let icon = match m {
                crate::modal::Mode::Move => "move",
                crate::modal::Mode::Rotate => "rotate-cw",
                crate::modal::Mode::Scale => "scaling",
                crate::modal::Mode::Mirror => "mirror",
                crate::modal::Mode::Offset => "offset",
                crate::modal::Mode::Grip => "spline",
                crate::modal::Mode::Inset => "shape-inset",
                crate::modal::Mode::Pull => "pull",
            };
            return (
                json!({"label": verb, "icon": icon, "hint": self.status, "help": crate::help::mode_page(m)}),
                options,
            );
        }
        let tool = self.plan.tool;
        // Create similar: what the tool is set like, first in its bar
        if let Some(c) = self.like_chip() {
            options.push(c);
        }
        if let Some(k) = self.kind_control() {
            options.push(k);
        }
        if let Some(name) = self.tool_type_label() {
            options.push(json!({
                "kind": "menu", "key": "type.tool", "label": "Type", "value": name,
                "hint": "The type new elements are drawn as: they share its values, and follow it when it changes",
            }));
        }
        if tool == Tool::Wall {
            // straight, an arc, a circle or a rectangle (MU40), in the one
            // Draw by (tool families REV04)
            let (shape, _, icon, _) = WALL_SHAPES[(self.plan.wall_shape as usize).min(3)];
            options.push(json!({
                "kind": "menu", "key": "drawby", "label": "Draw by", "value": shape,
                "icon": icon,
                "hint": "How the next walls are drawn: straight, an arc, a circle or a rectangle",
            }));
            options.push(json!({
                "kind": "number", "key": "wall.thickness", "label": "Thickness",
                "value": self.plan.wall_thickness, "unit": "m", "step": 0.01, "min": 0.05, "max": 1.0,
                "hint": "Thickness of new walls. Drag the label or type a value.",
            }));
            options.push(json!({
                "kind": "menu", "key": "wall.location", "label": "Location line",
                "value": self.plan.wall_location.label(),
                "hint": "The line you draw new walls on: it stays put when they get thicker or are flipped",
            }));
            options.push(json!({
                "kind": "toggle", "id": "wall.flipnew", "label": "Exterior left",
                "on": self.plan.wall_flip, "icon": "brick-wall",
                "hint": "Which side of the line you draw the exterior face goes on (Space flips it while drawing)",
            }));
        }
        let redrawing = self.plan.outline.as_ref().is_some_and(|o| !o.is_wall());
        // of the Site tools only the ground's outline and a property line
        // are outlines with a shape to pick: the rest are lines or points
        let shaped = tool != Tool::Terrain
            || matches!(
                self.plan.parts.ground,
                crate::partstool::GroundDraw::Outline | crate::partstool::GroundDraw::Property
            );
        if (tool.draws_outline() && shaped) || redrawing {
            // one Draw by for every outline tool (tool families REV04): its
            // panel holds the shape and the edges; 1–4 and L/A still work
            use crate::slabtool::SlabMode;
            let edges = if self.plan.slab_arc { "Arc" } else { "Line" };
            let (value, icon) = if tool.draws_outline() {
                let m = self.plan.slab_mode;
                let shape = m.label_for(tool);
                let shape = shape.split(" (").next().unwrap_or(shape);
                if m == SlabMode::Polygon {
                    (format!("{shape} · {edges}"), slab_mode_icon(m))
                } else {
                    (shape.to_string(), slab_mode_icon(m))
                }
            } else {
                (format!("Edges · {edges}"), "spline")
            };
            options.push(json!({
                "kind": "menu", "key": "drawby", "label": "Draw by", "value": value,
                "icon": icon,
                "hint": "How the outline is drawn: its shape (1–4) and its edges, line or arc (L, A)",
            }));
        }
        if tool == Tool::Sketch {
            let now = crate::sketch::shape(ctx);
            options.push(seg(crate::sketch::Shape::ALL
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    opt(
                        format!("sketch.shape:{i}"),
                        s.label(),
                        now == *s,
                        "What the next clicks draw",
                    )
                })
                .collect()));
        }
        options.extend(crate::memberstool::tool_options(&self.plan));
        options.extend(crate::partstool::tool_options(&self.plan));
        if tool == Tool::Footing {
            // like Revit's multiple selection: footings under all the walls
            // and columns picked, in one step
            let n = self.footing_hosts().len();
            options.push(json!({
                "kind": "button", "id": "footing.under", "wide": true,
                "label": match n {
                    0 => "Under selected".to_string(),
                    n => format!("Under {n} selected"),
                },
                "icon": "footing",
                "hint": "Put footings under the selected walls and columns that have none: a strip under each wall, a pad under each column, in one undo step. Partitions thinner than 150 mm are left out unless only they are picked. Select them first, or drag a box over them with this tool.",
            }));
        }
        options.extend(
            crate::objecttool::tool_options(&self.plan)
                .into_iter()
                .map(|mut o| {
                    // the object placed is named, not drawn as an icon
                    if o["id"] == "object:catalogue" {
                        o["wide"] = json!(true);
                    }
                    o
                }),
        );
        if tool == Tool::Detail {
            options.push(seg(creabim_core::DetailKind::ALL
                .iter()
                .enumerate()
                .map(|(i, k)| {
                    opt(
                        format!("detail.kind:{i}"),
                        k.label(),
                        self.plan.details.kind.same(*k),
                        "What the next clicks draw",
                    )
                })
                .collect()));
            if !matches!(
                self.plan.details.kind,
                creabim_core::DetailKind::Membrane
                    | creabim_core::DetailKind::Region { .. }
                    | creabim_core::DetailKind::BreakLine
            ) {
                options.push(json!({
                    "kind": "number", "key": "detail.width", "label": "Width",
                    "value": self.plan.details.width, "unit": "m", "step": 0.005, "min": 0.0, "max": 5.0,
                    "hint": "How wide the component is across its path: a batt's thickness, a course's depth",
                }));
            }
        }
        if tool == Tool::Callout {
            options.push(seg(crate::detailtool::CALLOUT_SCALES
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    opt(
                        format!("callout.scale:{i}"),
                        &format!("1:{s:.0}"),
                        self.plan.details.callout_scale == *s,
                        "The scale of the detail view the box makes",
                    )
                })
                .collect()));
        }
        if tool == Tool::Note {
            options.push(json!({
                "kind": "toggle", "id": "note.leader", "label": "Leader",
                "on": self.plan.anno.leader, "icon": "arrow-up-right",
                "hint": "Draw an arrow to what the note is about: click its point first, then the text",
            }));
        }
        options.extend(self.drafting_options());
        // the Grid tool's grid system
        options.extend(
            self.grid_vis_options()
                .into_iter()
                .filter(|o| o["key"] == "gridgroups" || o["key"] == "gridshape"),
        );
        while options.last().is_some_and(|o| o["kind"] == "sep") {
            options.pop();
        }
        // Select's own tools (MU39, option B): the shape a drag takes, how
        // it joins what is selected, in Edit mode what a click picks, and
        // the Select menu; the hint names what is selected
        let mut hint = tool.hint().to_string();
        if tool == Tool::Select {
            let iseg = |items: Vec<(String, &str, bool, &str)>| {
                json!({"kind": "seg", "items": items
                    .into_iter()
                    .map(|(id, icon, on, h)| json!({"id": id, "label": "", "icon": icon, "on": on, "hint": h}))
                    .collect::<Vec<_>>()})
            };
            use crate::boxsel::{Combine as C, Shape as S};
            let shape = crate::boxsel::shape();
            let mut sel_opts = vec![iseg(vec![
                ("sel.shape:0".into(), "sel-box", shape == S::Box, "Box (B): drag a box; left to right takes what is inside, right to left what it touches"),
                ("sel.shape:1".into(), "sel-circle", shape == S::Circle, "Circle: drag out a circle; it takes what it touches"),
                ("sel.shape:2".into(), "sel-lasso", shape == S::Lasso, "Lasso: draw round what to take"),
            ])];
            let how = crate::boxsel::combine();
            sel_opts.push(iseg(vec![
                (
                    "sel.combine:0".into(),
                    "sel-new",
                    how == C::New,
                    "New: what you take replaces the selection",
                ),
                (
                    "sel.combine:1".into(),
                    "sel-add",
                    how == C::Add,
                    "Add (Shift): what you take joins the selection",
                ),
                (
                    "sel.combine:2".into(),
                    "sel-sub",
                    how == C::Subtract,
                    "Subtract (Ctrl): what you take leaves the selection",
                ),
                (
                    "sel.combine:3".into(),
                    "sel-invert",
                    how == C::Invert,
                    "Invert: what you take flips, in or out",
                ),
                (
                    "sel.combine:4".into(),
                    "sel-intersect",
                    how == C::Intersect,
                    "Intersect: keep only what is both selected and taken",
                ),
            ]));
            if self.plan.edit.is_some() {
                let k = crate::handles::pick();
                sel_opts.push(iseg(vec![
                    (
                        "sel.pick:1".into(),
                        "pick-vertex",
                        k == 1,
                        "Points (1): corners, ends, vertices",
                    ),
                    (
                        "sel.pick:2".into(),
                        "pick-edge",
                        k == 2,
                        "Edges (2): sides, edges, jambs",
                    ),
                    (
                        "sel.pick:3".into(),
                        "pick-face",
                        k == 3,
                        "Faces (3): faces, tops, undersides",
                    ),
                ]));
            }
            sel_opts.push(json!({
                "kind": "menu", "key": "select.menu", "label": "", "value": "Select",
                "hint": "All, none, invert, similar, linked, random",
            }));
            sel_opts.push(json!({"kind": "sep"}));
            sel_opts.extend(options);
            options = sel_opts;
            hint = self.selection_hint();
        }
        // a Site family tool names what it draws ("Pad", "Road", "Palm"):
        // the what is picked in the Site panel, no longer on this strip
        let (label, icon) = match crate::families::Family::Site
            .active(&self.plan, crate::sketch::shape(ctx))
            .and_then(|id| crate::families::member_of(&id))
        {
            Some(m) => (m.label, m.icon),
            // the Wall tool drawing a curtain wall type is the Curtain
            // wall tool (MU75: the button lit Wall and the strip said Wall)
            None if self.drawing_curtain() => ("Curtain wall".to_string(), "curtain-wall"),
            None => (tool.label().to_string(), tool_icon(tool)),
        };
        let head = json!({
            "label": label, "icon": icon, "hint": hint,
            "help": crate::help::tool_page(tool), "trail": tool == Tool::Select,
        });
        (head, options)
    }

    /// What the left, middle and right buttons do over the view the
    /// pointer was last in, for the status bar (MU39).
    fn mouse_hints(&self) -> Value {
        // as Blender's status bar: they change while Shift, Ctrl or Alt
        // is held, to what the buttons then do (MU39)
        let m = MODS.load(std::sync::atomic::Ordering::Relaxed);
        let (shift, ctrl, alt) = (m & 1 != 0, m & 2 != 0, m & 4 != 0);
        let in_2d = self.last_in_2d;
        let classic = self.shell.settings.navigation == "classic";
        let select = self.plan.tool == Tool::Select;
        let editing = self.plan.edit.is_some();
        let (left, middle, right): (String, &str, &str) = if self.plan.modal.is_some() {
            (
                if ctrl { "Place, snapping" } else { "Place" }.into(),
                if in_2d { "Pan" } else { "Rotate view" },
                "Cancel",
            )
        } else if ctrl {
            (
                if select {
                    "Take away"
                } else {
                    self.plan.tool.label()
                }
                .into(),
                "Zoom",
                "Menu",
            )
        } else if shift {
            (
                if editing {
                    "Pick more"
                } else if select {
                    "Add to selection"
                } else {
                    self.plan.tool.label()
                }
                .into(),
                "Pan",
                if in_2d { "Menu" } else { "Place pivot" },
            )
        } else if alt && !in_2d {
            ("Rotate view".into(), "Rotate view", "Menu")
        } else {
            (
                if editing {
                    "Pick a point".into()
                } else if select {
                    "Select".into()
                } else {
                    self.plan.tool.label().to_string()
                },
                match (in_2d, classic) {
                    (true, _) | (false, true) => "Pan",
                    (false, false) => "Rotate view",
                },
                if select { "Menu" } else { "Back" },
            )
        };
        json!([
            {"b": "l", "label": left},
            {"b": "m", "label": middle},
            {"b": "r", "label": right},
        ])
    }

    /// The tool bar's hint while Select is the tool: what is selected and
    /// what keys do to it, or how to select (MU39).
    fn selection_hint(&self) -> String {
        let model = self.doc.model();
        let ids = self.selection.all();
        match ids.len() {
            0 => "Click or drag to select. Shift adds, Ctrl takes away. Ctrl+A all, Alt+A or Esc none."
                .into(),
            1 => {
                let id = ids[0];
                format!(
                    "{} · M move · R rotate · Shift+A all of its type",
                    crate::hoverpick::describe(model, id)
                )
            }
            n => {
                let mut kinds: Vec<(String, usize)> = vec![];
                for e in ids.iter().filter_map(|i| model.get(*i)) {
                    let k = crate::app::kind_word(&e.kind).to_string();
                    match kinds.iter_mut().find(|(w, _)| *w == k) {
                        Some((_, c)) => *c += 1,
                        None => kinds.push((k, 1)),
                    }
                }
                let list: Vec<String> = kinds
                    .iter()
                    .map(|(k, c)| format!("{c} {}", crate::app::plural(k, *c)))
                    .collect();
                format!("{n} selected: {} · M move · R rotate", list.join(", "))
            }
        }
    }

    /// The Select menu of the tool bar (MU39).
    fn select_menu_items(&self) -> Vec<Item> {
        let some = !self.selection.is_empty();
        let mut items = vec![
            self.cmd_item(Command::SelectAll),
            self.cmd_item(Command::SelectNone),
            Item::new("sel.invert", "Invert")
                .hint("Select what is not selected, and drop what is"),
            Item::Separator,
            Item::new("sel.shape:0", "Box select").key("B").icon("sel-box"),
            Item::new("sel.shape:1", "Circle select").icon("sel-circle"),
            Item::new("sel.shape:2", "Lasso select").icon("sel-lasso"),
            Item::Separator,
            Item::new("sel.linked", "Select linked")
                .enabled(some)
                .hint("Add what is joined to the selection: walls meeting it, the doors and windows in it"),
            Item::new("sel.random", "Select random")
                .enabled(some)
                .hint("Keep about half of the selection, picked at random: to thin out a row"),
        ];
        // every kind the project holds, without a selection first (MU39)
        let mut kinds: Vec<(&'static str, usize)> = vec![];
        for id in self.selectable_ids() {
            if let Some(e) = self.doc.model().get(id) {
                let k = crate::app::kind_word(&e.kind);
                match kinds.iter_mut().find(|(w, _)| *w == k) {
                    Some((_, n)) => *n += 1,
                    None => kinds.push((k, 1)),
                }
            }
        }
        kinds.sort_by_key(|k| k.0);
        let by_kind: Vec<Item> = kinds
            .into_iter()
            .map(|(k, n)| {
                let word = crate::app::plural(k, 2);
                let mut c = word.chars();
                let label = c
                    .next()
                    .map_or(String::new(), |f| f.to_uppercase().chain(c).collect());
                Item::new(format!("sel.kind:{k}"), format!("{label} ({n})"))
            })
            .collect();
        if !by_kind.is_empty() {
            items.push(Item::sub("All by type", by_kind).icon("layers"));
        }
        let similar = self.similar_items();
        if !similar.is_empty() {
            items.push(Item::sub("Select similar", similar).icon("copy"));
        }
        items
    }

    /// The quick help the ? by the tool's name opens: what the tool is and
    /// how to use it, in a few lines, and its page in the guide.
    fn tool_quick_help(&self) -> Value {
        let tool = self.plan.tool;
        let page = crate::help::tool_page(tool);
        let mut lines = crate::help::quick(page);
        // a page that opens with what it is: then how to use the tool
        if lines.len() < 2 {
            lines.push(tool.hint().to_string());
        }
        json!({
            "title": format!("{} tool", tool.label()), "icon": tool_icon(tool),
            "keys": "F1", "lines": lines, "page": page,
        })
    }

    fn top_state(&self, ctx: &egui::Context) -> Value {
        let quick: Vec<Value> = [
            Some(Command::Undo),
            Some(Command::Redo),
            None,
            Some(Command::Move),
            Some(Command::Rotate),
            Some(Command::Scale),
            Some(Command::Duplicate),
            // beside the other moves (MU142)
            Some(Command::Mirror),
            None,
            Some(Command::Copy),
            Some(Command::Cut),
            Some(Command::Paste),
            Some(Command::Delete),
            None,
            Some(Command::Hide),
            Some(Command::Isolate),
            Some(Command::ShowAll),
        ]
        .into_iter()
        .map(|c| match c {
            Some(c) => match self.cmd_item(c) {
                Item::Action {
                    id,
                    label,
                    shortcut,
                    enabled,
                    icon,
                    ..
                } => json!({"id": id, "label": label, "key": shortcut, "enabled": enabled, "icon": icon}),
                _ => Value::Null,
            },
            None => Value::Null,
        })
        .collect();
        // Split beside the other edits (MU39), its own picture
        let mut quick = quick;
        if let Some(i) = quick
            .iter()
            .position(|q| q["id"] == "ctx:Duplicate" || q["label"] == "Duplicate")
        {
            quick.insert(
                i + 1,
                json!({"id": "split", "label": "Split", "key": "", "enabled": !self.selection.is_empty(), "icon": "split"}),
            );
        }
        let tab = self
            .shell
            .settings
            .tools_tab
            .min(crate::tooltabs::TABS.len() - 1);
        let unusable = |t: Tool| self.tool_unusable(t);
        let curtain = self.drawing_curtain();
        let tool = |t: Tool| {
            let i = Tool::ALL.iter().position(|x| *x == t).unwrap_or(0);
            let off = unusable(t);
            let hint = match off {
                Some(why) => format!("{} (not here: {why})", t.hint()),
                None => t.hint().to_string(),
            };
            json!({
                "kind": "tool", "id": format!("tool:{i}"), "label": t.label(), "hint": hint,
                "off": off.is_some(),
                // while Move, Rotate and the rest wait or run, it is
                // their button that is lit, not Select's (MU19)
                "icon": tool_icon(t),
                "active": self.plan.tool == t
                    && !(t == Tool::Select && (self.plan.armed.is_some() || self.plan.modal.is_some()))
                    // drawing a curtain wall lights its own button (MU75)
                    && !(t == Tool::Wall && curtain),
            })
        };
        let mut tools: Vec<Value> = crate::tooltabs::ALWAYS.iter().map(|t| tool(*t)).collect();
        tools.push(json!({"kind": "sep"}));
        for e in crate::tooltabs::entries(tab) {
            use crate::tooltabs::Entry;
            tools.push(match e {
                Entry::Tool(t) => tool(t),
                Entry::Act {
                    id,
                    label,
                    icon,
                    hint,
                } => json!({
                    "kind": "act", "id": id, "label": label, "hint": hint, "icon": icon,
                    "active": id == "tool.curtain" && curtain,
                }),
                Entry::Menu {
                    key,
                    label,
                    icon,
                    hint,
                } => json!({
                    "kind": "menu", "key": key, "label": label, "hint": hint, "icon": icon,
                    "open": self.popup_key().as_deref() == Some(key),
                }),
                Entry::Family(f) => {
                    // its button: the tool in use, else the last one used;
                    // lit while one of its tools is on (REV04, AMD-001)
                    let active = f.active(&self.plan, crate::sketch::shape(ctx));
                    let shown = active
                        .clone()
                        .or_else(|| self.shell.settings.family_last.get(f.key()).cloned())
                        .and_then(|id| crate::families::member_of(&id))
                        .unwrap_or_else(|| f.first());
                    let key = format!("fam:{}", f.key());
                    json!({
                        "kind": "family", "key": key, "act": shown.id, "icon": shown.icon,
                        "label": shown.label, "family": f.label(),
                        "hint": shown.hint, "key_hint": shown.key,
                        "on": active.is_some(),
                        "open": self.popup_key().as_deref() == Some(key.as_str()),
                    })
                }
                Entry::Sep => json!({"kind": "sep"}),
            });
            // its pinned tools beside it, single buttons
            if let crate::tooltabs::Entry::Family(f) = e {
                let active = f.active(&self.plan, crate::sketch::shape(ctx));
                for m in f.members() {
                    if self.shell.settings.family_pins.contains(&m.id) {
                        tools.push(json!({
                            "kind": "act", "id": m.id, "label": m.label, "hint": m.hint,
                            "icon": m.icon, "active": active.as_deref() == Some(m.id.as_str()),
                        }));
                    }
                }
            }
        }
        let tabs: Vec<Value> = crate::tooltabs::TABS
            .iter()
            .enumerate()
            .map(|(i, t)| json!({"id": format!("tab:{i}"), "label": t, "on": i == tab}))
            .collect();
        let (tool_head, tool_options, options, more) = self.option_strip(ctx);
        #[cfg(target_os = "macos")]
        let mac_titlebar = self.shell.mac_titlebar.map(|layout| {
            json!({
                "right": layout.right,
                "inset": layout.inset(),
                "revision": self.shell.mac_titlebar_revision,
            })
        });
        #[cfg(not(target_os = "macos"))]
        let mac_titlebar = Option::<Value>::None;
        json!({
            "theme": self.theme(),
            "popup": self.popup_key(),
            "native_menu": self.uses_native_menu(),
            "mac_titlebar": mac_titlebar,
            "menus": (["File", "Edit", "View", "Create", "Materials", "Help"].map(|m| json!({"name": m, "key": format!("menu:{m}")}))),
            "quick": quick,
            "open_kinds": self.shell.open_kinds,
            // the selection count is in the status bar, where it belongs
            "doc": {
                "name": file_label(&self.path),
                "path": self.path,
                "unsaved": self.unsaved(),
                "note": "",
            },
            "app": {"key": "menu:CreaBIM", "label": "CreaBIM"},
            "right": [{
                "kind": "menu", "key": "workspaces", "label": "", "value": self.areas.workspace,
                "icon": "workspace", "big": true,
                "hint": "Workspace: named layouts of view areas (Ctrl+PgUp and Ctrl+PgDn step through them)",
            }],
            "tabs": tabs,
            "tools": tools,
            "tool": tool_head,
            "tool_options": tool_options,
            "options": options,
            "more": more,
        })
    }

    /// The tool options strip: the tool's head and options, the view's
    /// options, each keyed and marked compact or for the More menu as the
    /// page last measured them, and how many are in More.
    pub(super) fn option_strip(
        &self,
        ctx: &egui::Context,
    ) -> (Value, Vec<Value>, Vec<Value>, usize) {
        let (tool_head, mut tool_options) = self.tool_strip(ctx);
        // docked, each area's header has its view's controls
        let mut options = if self.placement == PlanPlacement::Docked {
            vec![]
        } else {
            self.view_options()
        };
        // lengths show and take the project's units; what is typed is read here
        for o in tool_options.iter_mut().chain(options.iter_mut()) {
            if o["kind"] == "number" && o["unit"] == "m" {
                let v = o["value"].as_f64().unwrap_or(0.0);
                o["text"] = json!(crate::units::len(v));
                o["unit"] = json!("");
            }
        }
        crate::optionbar::prepare(&mut tool_options, false);
        crate::optionbar::prepare(&mut options, true);
        let more =
            crate::optionbar::place(&mut tool_options, &mut options, self.shell.bar.as_ref());
        (tool_head, tool_options, options, more)
    }

    /// The active view's settings, at the right of the tool options strip.
    pub(super) fn view_options(&self) -> Vec<Value> {
        let mut options = vec![];
        if let Some((label, value, unit, _)) = self.view_number() {
            let (step, min, max) = if unit == "°" {
                (1.0, -89.0, 89.0)
            } else {
                (0.05, 0.1, 200.0)
            };
            options.push(json!({
                "kind": "number", "key": "view.number", "label": label, "value": value,
                "unit": unit, "step": step, "min": min, "max": max,
                "hint": "For the view shown in the 2D pane",
            }));
            if let Some(v) = self.active_view.and_then(|id| self.view(id)) {
                if matches!(
                    v.kind,
                    ViewKind::Section { .. } | ViewKind::Axonometric { .. }
                ) {
                    if matches!(v.kind, ViewKind::Section { .. }) {
                        options.push(json!({
                            "kind": "toggle", "id": "view.ranges", "label": "Plan ranges",
                            "icon": "view-range", "on": self.plan.plan_ranges,
                            "hint": "Each floor plan's cut and view depth as a band: drag their grips to change them (MU34)",
                        }));
                    }
                    options.push(json!({
                        "kind": "menu", "key": "viewstyle", "label": "Style",
                        "value": v.style.label(), "icon": view_style_icon(v.style),
                        "hint": "How this view draws, on its sheets too: hidden line, shaded with edges, or wireframe (every edge, the hidden ones too)",
                    }));
                }
                options.push(json!({
                    "kind": "toggle", "id": "view.reveal", "label": "Hidden",
                    "on": v.show_hidden, "icon": "hidden-elements",
                    "hint": "Show elements hidden inside others (a beam in a wall) with dashed lines",
                }));
                options.push(json!({
                    "kind": "toggle", "id": "view.colour", "label": "In colour",
                    "on": v.show_hidden && v.hidden_in_colour, "icon": "hidden-colour",
                    "hint": "For detailing: hidden elements in their material colours, see-through over what hides them",
                }));
                if let Some(id) = self.active_view {
                    options.push(json!({
                        "kind": "ball", "id": "view.xray", "label": "X-ray",
                        "on": self.plan.xray.contains(&id), "icon": "xray", "shortcut": "Alt+Z",
                        "hint": "X-ray this view while working (Alt+Z over it): walls, slabs and roofs see-through, what is inside them solid and clicked first",
                    }));
                }
            }
            if self.active_plan().is_some() {
                options.push(json!({
                    "kind": "toggle", "id": "view.tempdims", "label": "Temporary dimensions",
                    "icon": "dimension", "on": !self.plan.temp_dims_off,
                    "hint": "The blue dimensions by what you select: on, they show and can be typed into; off, none show (MU21)",
                }));
            }
            // the grid behind the drawing, on or off for every drawing
            // (MU40: some prefer plain paper)
            options.push(json!({
                "kind": "toggle", "id": "view.grid", "label": "Grid",
                "icon": "grid-3x3", "on": self.shell.settings.plan_grid,
                "hint": "The light grid behind drawings: off for plain paper. For every plan, section and detail",
            }));
            options.push(json!({
                "kind": "button", "id": "view.tag", "label": "Tag openings", "icon": "tag",
                "hint": "Tag every door and window in this view that has no tag yet",
            }));
            if self.active_plan().is_some() {
                options.push(json!({
                    "kind": "button", "id": "view.tagfixtures", "label": "Tag fixtures", "icon": "fixtures",
                    "hint": "Tag every socket, switch, light, drain, radiator and other fixture on this plan with its type's mark",
                }));
                let shown = self.active_view.and_then(|id| self.view(id)).cloned();
                let (ceiling, colours) = shown
                    .as_ref()
                    .map_or((false, false), |v| (v.ceiling_plan, v.room_colours));
                options.push(json!({
                    "kind": "button", "id": "view.tagrooms", "label": "Tag rooms", "icon": "room",
                    "hint": "Tag every room on this plan that has no tag here yet",
                }));
                options.push(json!({
                    "kind": "toggle", "id": "view.roomtags", "label": "Show room tags",
                    "on": shown.as_ref().is_some_and(|v| v.shows_room_tags()), "icon": "tag",
                    "hint": "Draw this view's room tags. Off by default in detail and callout views, where they come out tiny",
                }));
                options.push(json!({
                    "kind": "toggle", "id": "view.ceiling", "label": "Ceiling plan",
                    "on": ceiling, "icon": "ceiling",
                    "hint": "A reflected ceiling plan: the ceilings, their grids and what hangs from them, not the floor and furniture",
                }));
                options.push(json!({
                    "kind": "toggle", "id": "view.rooms", "label": "Room colours",
                    "on": colours, "icon": "colour-scheme",
                    "hint": "Fill each room with a colour of its own, the same in every view",
                }));
            }
        }
        if let Some((view, level)) = self.active_plan() {
            let u = self.view(view).map(|v| v.underlay).unwrap_or_default();
            let value = self
                .underlay_choices(level)
                .into_iter()
                .find(|c| c.0 == u)
                .map_or("None".to_string(), |c| c.1);
            options.push(json!({
                "kind": "menu", "key": "underlay", "label": "Underlay", "value": value,
                "icon": "layers", "hint": "Show another level faintly under this plan, to trace and snap to",
            }));
        }
        options.extend(
            self.grid_vis_options().into_iter().filter(|o| {
                o["key"] != "gridgroups" && o["key"] != "gridshape" && o["kind"] != "sep"
            }),
        );
        if self
            .active_view
            .and_then(|id| self.view(id))
            .is_some_and(|v| matches!(v.kind, ViewKind::Schedule { .. }))
        {
            options.push(json!({
                "kind": "button", "id": "sched.csv", "label": "Export CSV", "icon": "table-2",
                "hint": "Save this table as a CSV file beside the project",
            }));
            options.push(json!({
                "kind": "button", "id": "sched.xlsx", "label": "Export Excel", "icon": "sheet",
                "hint": "Save every schedule as one Excel workbook beside the project: a summary, then a sheet for each",
            }));
        }
        if self.active_view.is_some_and(|id| self.is_sheet(id)) {
            // compose
            options.push(json!({
                "kind": "menu", "key": "placeview", "label": "", "value": "Place view…",
                "icon": "place-view", "hint": "Put a view on this sheet", "wide": true,
            }));
            // paper marks: what belongs to the sheet, not to a view (MU22)
            options.push(json!({"kind": "sep"}));
            options.push(json!({
                "kind": "button", "id": "sheet.text", "label": "Text", "icon": "text",
                "hint": "Text on the sheet: click where it goes (N)",
            }));
            options.push(json!({
                "kind": "button", "id": "sheet.picture", "label": "Picture", "icon": "image",
                "hint": "A picture on the sheet: a collage, site context, a mood board (PNG, JPEG; an SVG stays crisp). Drag its grips to size it",
            }));
            // north arrow, scale bar and symbols as one split button: the
            // one used last, its arrow the three (Mockup_Sheet_Symbols_REV00)
            let last = self.sheet_symbol_last();
            let (last, icon, label) = match last.as_str() {
                "sheet.scalebar" => ("sheet.scalebar".to_string(), "scale-bar", "Scale bar"),
                _ => ("sheet.north".to_string(), "north-arrow", "North arrow"),
            };
            options.push(json!({
                "kind": "split", "id": last, "key": "sheet.symbols", "label": label, "icon": icon,
                "hint": "Sheet symbols: the icon places this one; the arrow beside it picks North arrow or Scale bar",
            }));
            options.push(json!({
                "kind": "button", "id": "sheet.cloud", "label": "Revision cloud", "icon": "revision-cloud",
                "hint": "A revision cloud round a change, with the next revision letter",
            }));
            options.push(json!({
                "kind": "button", "id": "sheet.keyplan", "label": "Key plan", "icon": "map",
                "hint": "A small plan of the whole building by the title block, at 1:500, without dimensions, tags or notes",
            }));
            options.push(json!({
                "kind": "button", "id": "sheet.revtable", "label": "Revisions table", "icon": "table-2",
                "hint": "The sheet's revisions table: a row for each revision cloud on it",
            }));
            options.push(json!({
                "kind": "toggle", "id": "sheet.guides", "label": "Guides", "icon": "guides",
                "on": self.plan.sheet_guides,
                "hint": "Rulers along the sheet's edges to drag guides out of; marks, notes and views snap to the guides. Off, they hide and nothing snaps to them",
            }));
            // output
            options.push(json!({"kind": "sep"}));
            options.push(json!({
                "kind": "button", "id": "sheet.pdf", "label": "Sheet as PDF", "icon": "file-text",
                "hint": "This sheet as a PDF (Ctrl+P); all or chosen sheets: File › Export › Sheets as PDF (Ctrl+Shift+P)",
            }));
            // the sheet as it prints, on white, while the paper is dark (MU21)
            options.push(json!({
                "kind": "toggle", "id": "sheet.preview", "label": "Print preview",
                "icon": "print-preview", "on": self.print_preview,
                "hint": "Show the sheet as it prints, on white paper; off, it follows the drawing paper (dark or light)",
            }));
        }
        options
    }

    /// The browser's state, built again only when what it shows changed:
    /// building its rows (every element, marks and hints) took ~5 ms a
    /// frame, every frame (MU39: the app felt a tad slower).
    fn left_state(&self) -> Value {
        let key = format!(
            "{}|{:?}|{:?}|{}|{}|{:?}",
            self.doc.model().revision(),
            self.selection,
            self.active_view,
            self.shell.settings.left_folded,
            self.theme(),
            self.profiles.current,
        );
        // a move, rotate or scale following the pointer changes the model
        // each frame as a drag does; with as many elements as before (no
        // copies shown yet), the tree's rows are the same
        let count = self.doc.model().len();
        let following = self
            .plan
            .modal
            .as_ref()
            .is_some_and(|m| m.stage() == crate::modal::Stage::Follow);
        let mut v = LEFT_CACHE.with(|c| {
            if let Some((k, n, v)) = c.borrow().as_ref() {
                // the tree holds still through a drag: the model changes
                // each frame, its names and rows don't (5 ms a frame)
                if *k == key || self.dragging() || (following && *n == count) {
                    return v.clone();
                }
            }
            let v = self.left_state_now();
            *c.borrow_mut() = Some((key, count, v.clone()));
            v
        });
        // project time pinned at the browser's foot (MU101, B): it ticks,
        // so it is added each frame, outside the tree's cache
        v["dock"] = self.time_dock_json().unwrap_or(Value::Null);
        v
    }

    fn left_state_now(&self) -> Value {
        let model = self.doc.model();
        // the search finds elements by their marks (D01, W05) too
        let marks = creabim_core::marks::marks(model);
        let mark = |e: &Element| match &e.kind {
            ElementKind::Object(_) => creabim_core::marks::mark_of(model, e.id),
            _ => marks.get(&e.id).cloned(),
        };
        let row = |e: &Element, opens: bool| {
            json!({
                "id": e.id.0.to_string(), "name": e.name, "icon": site_icon(e).unwrap_or(icon_name(Icon::of(e))),
                "mark": mark(e),
                "sel": self.selection.contains(e.id),
                "active": opens && self.active_view == Some(e.id),
                "hint": if opens {
                    format!("{}: click to open", e.name)
                } else if matches!(e.kind, ElementKind::ElementType(_)) {
                    format!("{}: double-click to see it on its own, in 3D, plan and section", e.name)
                } else {
                    format!("{}: double-click to frame it in the 3D view", e.name)
                },
            })
        };
        // views by kind, plans in their levels' order; drag a row to move it
        let mut out: Vec<Value> = crate::browser::view_groups(model)
            .into_iter()
            .map(|(g, ids)| {
                let rows: Vec<Value> = ids
                    .iter()
                    .filter_map(|id| model.get(*id))
                    .map(|e| row(e, true))
                    .collect();
                let manual = crate::browser::is_manual(model, g);
                let icon = match g {
                    crate::browser::ViewGroup::Plans => "map",
                    crate::browser::ViewGroup::CeilingPlans => "ceiling",
                    crate::browser::ViewGroup::Views3d => "view-3d",
                    crate::browser::ViewGroup::Sections => "scissors-line-dashed",
                    crate::browser::ViewGroup::Elevations => "elevation",
                    crate::browser::ViewGroup::Details => "callout",
                    crate::browser::ViewGroup::Schedules => "table-2",
                };
                let mut acts = vec![];
                if manual {
                    acts.push(json!({
                        "id": format!("browser.sort:{}", g.key()),
                        "icon": "sort-by-level",
                        "hint": match g {
                            crate::browser::ViewGroup::Plans => "Sort by level: put the plans back in the order of their levels, lowest first".to_string(),
                            _ => format!("{}: undo the order you dragged them into", g.auto_label()),
                        },
                    }));
                }
                json!({
                    "key": g.key(), "title": g.title(), "icon": icon, "open": true,
                    "rows": rows, "acts": acts, "sortable": true,
                    "note": if manual { "Your order" } else if g == crate::browser::ViewGroup::Plans { "By level" } else { "" },
                })
            })
            .collect();
        let mut groups: Vec<(String, Vec<&Element>)> = vec![];
        for e in model.iter() {
            if matches!(e.kind, ElementKind::Units(_) | ElementKind::View(_)) {
                continue;
            }
            let class = class_of(e);
            match groups.iter_mut().find(|(c, _)| *c == class) {
                Some((_, v)) => v.push(e),
                None => groups.push((class, vec![e])),
            }
        }
        let rank = |c: &str| match c {
            "View" => 0,
            "Sheet" => 1,
            "IfcBuildingStorey" => 2,
            _ => 3,
        };
        groups.sort_by_key(|(c, _)| rank(c));
        let groups: Vec<Value> = groups
            .into_iter()
            .map(|(class, items)| {
                let opens = class == "Sheet";
                let mut items = items;
                if class == "IfcBuildingStorey" {
                    // levels from the top down, as in a section
                    let z = |e: &Element| model.level(e.id).map_or(0.0, |l| l.elevation);
                    items.sort_by(|a, b| z(b).total_cmp(&z(a)));
                }
                // each terrain, then what lies on it: its earthworks,
                // its lines, its spot levels; what lies on none comes last
                let mut kids: Vec<ElementId> = vec![];
                if class == "Site" {
                    let mut ordered: Vec<&Element> = vec![];
                    for t in items.iter().copied() {
                        let ElementKind::Site(s) = &t.kind else {
                            continue;
                        };
                        if !matches!(s.form, creabim_core::SiteForm::Terrain { .. }) {
                            continue;
                        }
                        ordered.push(t);
                        let (spots, lines) = creabim_core::site::ground_objects(model, s);
                        let on = creabim_geom::earth::work_pads(model, s)
                            .into_iter()
                            .map(|w| w.0)
                            .chain(lines.into_iter().map(|l| l.0))
                            .chain(spots.into_iter().map(|p| p.0));
                        for id in on {
                            if let Some(e) = items.iter().copied().find(|e| e.id == id) {
                                if !ordered.iter().any(|o| o.id == id) {
                                    ordered.push(e);
                                    kids.push(id);
                                }
                            }
                        }
                    }
                    for e in items.iter().copied() {
                        if !ordered.iter().any(|o| o.id == e.id) {
                            ordered.push(e);
                        }
                    }
                    items = ordered;
                }
                let mut rows: Vec<Value> = items.iter().map(|e| row(e, opens)).collect();
                for (r, e) in rows.iter_mut().zip(&items) {
                    if kids.contains(&e.id) {
                        r["kid"] = json!(true);
                    }
                }
                // a 3D view's crop boxes say whose they are
                for (r, e) in rows.iter_mut().zip(&items) {
                    if let ElementKind::CropBox(c) = &e.kind {
                        let owner = c.view.and_then(|v| model.get(v));
                        let off = if c.enabled { "" } else { " (off)" };
                        r["name"] = json!(match owner {
                            Some(v) => format!("{} · {}{off}", e.name, v.name),
                            None => format!("{}{off}", e.name),
                        });
                    }
                }
                // levels and assemblies are managed from their groups
                let act = match class.as_str() {
                    "IfcBuildingStorey" => Some(json!([
                        {"id": "level.add", "icon": "plus", "hint": "Add a level above the top one, with its floor plan"},
                        {"id": "levels", "icon": "ellipsis", "hint": "Levels window: add, duplicate, rename, move and delete levels, and make their plans"},
                    ])),
                    "CropBox" => Some(json!([
                        {"id": "crop.add", "icon": "plus", "hint": "Add a crop box to the 3D view: it crops everything outside it"},
                    ])),
                    "IfcMaterialLayerSet" => Some(json!([
                        {"id": "asm.browser", "icon": "ellipsis", "hint": "Assemblies: edit layers, add, duplicate, delete, assign"},
                    ])),
                    "IfcFurniture" => Some(json!([
                        {"id": "catalogue", "icon": "plus", "hint": "Catalogue: furniture and fixtures to place"},
                    ])),
                    "ElementType" => Some(json!([
                        {"id": "types.browser", "icon": "ellipsis", "hint": "Types: edit, add, duplicate, delete, assign, select instances"},
                    ])),
                    _ => None,
                };
                // each group its own picture (MU29: many shared the cube)
                let title = group_title(&class);
                let icon = match class.as_str() {
                    "ElementType" => "element-types",
                    "Site" => "terrain",
                    "IfcFurniture" => "sofa",
                    _ => match title.to_lowercase().as_str() {
                        t if t.contains("curtain") => "curtain-wall",
                        t if t.contains("opening") || t.contains("shaft") => "shaft",
                        t if t.contains("railing") => "railing",
                        t if t.contains("footing") => "footing",
                        t if t.contains("room") => "room",
                        t if t.contains("ceiling") => "ceiling",
                        t if t.contains("join") => "join",
                        t if t.contains("stair") => "stair",
                        t if t.contains("detail") => "callout",
                        t if t.contains("note") => "sticky-note",
                        t if t.contains("text style") => "anno-styles",
                        t if t.contains("tag") => "tag",
                        t if t.contains("template") => "sheet-template",
                        t if t.contains("planting") || t.contains("tree") => "tree",
                        t if t.contains("void") => "void",
                        _ => icon_name(Icon::of_class(&class)),
                    },
                };
                json!({
                    "key": class, "title": group_title(&class), "icon": icon,
                    "open": rank(&class) < 3, "rows": rows, "acts": act,
                })
            })
            .collect();
        out.extend(groups);
        // the project's notes, after the sheets
        let after_sheets = out
            .iter()
            .position(|g| g["key"] == "Sheet")
            .map_or(out.len(), |i| i + 1);
        out.insert(after_sheets, self.notes_group());
        // the renders kept from the 3D view, as views (Visuals REV02)
        out.insert(after_sheets, self.renders_group());
        if let Some(g) = self.groups_group() {
            out.push(g);
        }
        out.push(self.kinds_group());
        out.push(self.profiles_group());
        out.push(self.masters_group());
        let groups = out;
        json!({
            "theme": self.theme(),
            "count": format!("{} elements", self.element_count()),
            "folded": self.shell.settings.left_folded,
            "groups": groups,
        })
    }

    pub(super) fn status_state(&self) -> Value {
        let mut s = self.plan.snap;
        let n = s.switches().iter().filter(|(_, v)| **v).count();
        let total = s.switches().len();
        let mut snaps = match n {
            n if n == total => "All".to_string(),
            0 => "None".to_string(),
            n => format!("{n} of {total}"),
        };
        // divisions on: its number in sight (MU102)
        let d = crate::snaps::divisions();
        if d > 0 {
            snaps = format!("{snaps} · ÷{d}");
        }
        let workplane = match &self.workplane {
            Some((_, name)) => name.clone(),
            None => "Level".into(),
        };
        let mut state = json!({
            "theme": self.theme(),
            "popup": self.popup_key(),
            "message": self.status,
            // what the drawing under the pointer does, after the message
            // (what is selected says how to use it instead, from its
            // Properties, while the pointer is over a view)
            "hint": self.plan.hint.map(|h| match &self.plan.howto {
                Some(t) => t.clone(),
                None => h.to_string(),
            }).filter(|h| *h != self.status),
            // what each mouse button does here, as Blender's status bar (MU39)
            "mouse": self.mouse_hints(),
            "controls": [
                // MU101 (A): Snap and its filter as one split button, Snap
                // still on and off in one click; grid, workplane and pivot
                // an icon while at their usual setting, with the word only
                // when changed; units stay a menu, without the word
                {"kind": "toggle", "id": "snap.on", "label": "Snap", "on": self.plan.snap.on, "icon": "magnet", "join": "l",
                 "hint": "Object snaps on or off (F3). Hold Alt while picking to suspend them."},
                {"kind": "menu", "key": "snaps", "label": "", "value": snaps, "join": "r", "hint": "Which snaps are on"},
                {"kind": "menu", "key": "step", "label": "Grid snap", "value": self.plan.snap.step_label(), "icon": "grid-3x3",
                 "quiet": self.plan.snap.step_label() == "Off",
                 "hint": "Off: points and drags go exactly where the pointer is. On (Alt+S): they land on a grid in space. Also: distance marks while dragging."},
                {"kind": "menu", "key": "workplane", "label": "Workplane", "value": workplane, "icon": "layers-2",
                 "quiet": workplane == "Level",
                 "hint": "Where sketches and guides drawn in 3D go"},
                {"kind": "menu", "key": "pivot", "label": "Pivot", "value": self.plan.pivot.label(), "icon": "crosshair",
                 "quiet": self.plan.pivot == crate::modal::Pivot::default(),
                 "hint": "What rotate and scale turn about. Shift+right-click places the pivot cursor."},
                {"kind": "sep"},
                {"kind": "menu", "key": "units.menu", "label": "", "value": crate::app::appmenu::unit_short(self.doc.model().units().length),
                 "hint": "Units: how lengths are shown and typed. Any unit can still be typed: 3 m, 10'6\", 450 mm."},
                {"kind": "meta", "label": self.element_count().to_string(), "hint": format!("{} elements in the project", self.element_count())},
            ],
        });
        // the settings held (Ctrl+Shift+C), as a chip
        if let Some(chip) = self.held_chip() {
            if let Some(c) = state["controls"].as_array_mut() {
                for (k, v) in chip.into_iter().enumerate() {
                    c.insert(k, v);
                }
            }
        }
        // project time's pill, while it is kept for the project
        if let Some(t) = self.time_pill_json() {
            state["timer"] = t;
        }
        // right after a box took several kinds, a way to keep one of them
        if self.offers_keep_only() {
            if let Some(c) = state["controls"].as_array_mut() {
                c.insert(0, json!({"kind": "menu", "key": "keeponly", "label": "Keep only", "value": "…", "icon": "square-dashed-mouse-pointer",
                    "hint": "Keep only one kind of what the box took: the walls, the doors…"}));
            }
        }
        // right after a footing went on a host, the plan that shows it
        if self.plan.footing_placed && self.status.contains(" footing placed under ") {
            let label = if crate::memberstool::foundation_plan(self.doc.model()).is_some() {
                "Show in Foundation plan"
            } else {
                "Make a Foundation plan"
            };
            if let Some(c) = state["controls"].as_array_mut() {
                c.insert(0, json!({"kind": "button", "id": "footing.foundation", "label": label, "icon": "footing",
                    "hint": "Open the plan where footings are drawn, with the floor above as its underlay"}));
            }
        }
        // right after an export, the way to the file it wrote
        if self.last_export.as_deref() == Some(self.status.as_str()) {
            if let Some(c) = state["controls"].as_array_mut() {
                c.insert(0, json!({"kind": "button", "id": "export.folder", "label": "Open the folder", "icon": "folder-open",
                    "hint": "Show the exported file in the system's file browser"}));
            }
        }
        state
    }

    fn theme(&self) -> &'static str {
        self.shell.theme.id()
    }

    /// Switch theme everywhere and remember it for next time.
    pub(super) fn set_theme(&mut self, ctx: &egui::Context, t: Theme) {
        self.shell.theme = t;
        style(ctx, t);
        self.shell.settings.theme = t.id().into();
        self.shell.settings.store();
        self.status = format!("Theme: {}.", t.label());
    }

    fn popup_key(&self) -> Option<String> {
        self.shell.popup.as_ref().map(|p| p.key.clone())
    }

    /// While the palette is open, the keys typed in the views go to it: its
    /// page doesn't always get the keyboard when opened from a shortcut.
    pub(super) fn palette_keys(&mut self, raw: &mut egui::RawInput) {
        let Some(key) = self.popup_key() else { return };
        let Some(web) = &self.shell.web else { return };
        if key != "palette" {
            // only a menu that shows takes them: one left open but parked
            // off the window took Enter and the arrows from every text box
            // (MU87: Enter made no new line in the notes)
            if !web.popup_showing() {
                let enter = raw.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Key {
                            key: egui::Key::Enter,
                            pressed: true,
                            ..
                        }
                    )
                });
                if enter {
                    crate::actlog::line(format!(
                        "popup {key:?} open but not shown: Enter left to the views"
                    ));
                }
                return;
            }
            // arrows and Enter walk an open menu
            let mut keys = vec![];
            raw.events.retain(|e| match e {
                egui::Event::Key {
                    key, pressed: true, ..
                } if matches!(
                    key,
                    egui::Key::ArrowDown
                        | egui::Key::ArrowUp
                        | egui::Key::ArrowLeft
                        | egui::Key::ArrowRight
                        | egui::Key::Enter
                ) =>
                {
                    keys.push(match key {
                        egui::Key::ArrowDown => "ArrowDown",
                        egui::Key::ArrowUp => "ArrowUp",
                        egui::Key::ArrowLeft => "ArrowLeft",
                        egui::Key::ArrowRight => "ArrowRight",
                        _ => "Enter",
                    });
                    false
                }
                _ => true,
            });
            for k in keys {
                web.run(Pane::Popup, &format!("window.CB && CB.nav({})", json!(k)));
            }
            return;
        }
        let mut text = String::new();
        let mut keys = vec![];
        raw.events.retain(|e| match e {
            egui::Event::Text(t) => {
                text.push_str(t);
                false
            }
            egui::Event::Key { key, pressed, .. } => {
                if *pressed {
                    // the page's names, as a browser gives them
                    keys.push(match key {
                        egui::Key::ArrowDown => "ArrowDown",
                        egui::Key::ArrowUp => "ArrowUp",
                        k => k.name(),
                    });
                }
                false
            }
            _ => true,
        });
        if !text.is_empty() || !keys.is_empty() {
            web.run(
                Pane::Popup,
                &format!(
                    "window.CB && CB.feed({})",
                    json!({"text": text, "keys": keys})
                ),
            );
        }
    }

    /// The command palette: every action, found by typing.
    pub(super) fn open_palette(&mut self, ctx: &egui::Context) {
        let screen = ctx.viewport_rect();
        let w = 560.0f32.min(screen.width() - 32.0);
        self.shell.popup = Some(Popup {
            key: "palette".into(),
            at: egui::pos2(screen.center().x - w / 2.0, screen.top() + 90.0),
            up: None,
            above: None,
            size: egui::vec2(w, 380.0),
        });
        self.shell.focus_popup = true;
    }

    /// Save as template's card, in the middle of the window. False where
    /// the app has no web panels to draw it.
    pub(crate) fn open_template_card(&mut self, ctx: &egui::Context) -> bool {
        if self.shell.web.is_none() {
            return false;
        }
        let screen = ctx.viewport_rect();
        // the page says how tall it is once drawn
        let size = egui::vec2(540.0f32.min(screen.width() - 32.0), 300.0);
        self.shell.popup = Some(Popup {
            key: TEMPLATE_KEY.into(),
            at: screen.center() - size / 2.0,
            up: None,
            above: None,
            size,
        });
        self.shell.focus_popup = true;
        true
    }

    /// What Save as template's card shows: the name to start from, and
    /// each part with what this project has of it.
    fn template_card_json(&self) -> Value {
        use crate::app::templates as t;
        let n = t::part_counts(self.doc.model());
        let name = std::path::Path::new(&self.path)
            .file_stem()
            .map_or("Template".to_string(), |s| s.to_string_lossy().to_string());
        let look = t::look_only();
        json!({
            "name": name,
            "building": t::BUILDING,
            "details": t::DETAILS,
            "parts": t::PARTS.iter().enumerate().map(|(i, (key, title, line))| json!({
                "key": key, "title": title, "line": line, "on": look[i],
                "count": if i == t::DETAILS { Value::Null } else { json!(n[i]) },
            })).collect::<Vec<_>>(),
        })
    }

    /// The New project window, in the middle of the window.
    pub(super) fn open_new_project(&mut self, ctx: &egui::Context) {
        if self.shell.web.is_none() {
            // the classic panels keep the File menu's list
            self.new_project_classic = true;
            return;
        }
        let screen = ctx.viewport_rect();
        // taller once earlier versions' starter houses have a row of
        // their own under the two large starts, so nothing scrolls
        let tall = if crate::app::appmenu::STARTER_HOUSES.len() > 1 {
            600.0f32
        } else {
            540.0
        };
        let size = egui::vec2(
            900.0f32.min(screen.width() - 32.0),
            tall.min(screen.height() - 32.0),
        );
        self.shell.popup = Some(Popup {
            key: "newproject".into(),
            at: screen.center() - size / 2.0,
            up: None,
            above: None,
            size,
        });
        self.shell.focus_popup = true;
    }

    /// The New project window's choices, for its page (Templates in the
    /// app, REV05): sources on the left, each with its tiles.
    fn new_project_state(&self) -> Value {
        use crate::app::templates;
        let card = |s: &crate::app::appmenu::Start, holds: &str| json!({"id": s.id, "title": s.title, "line": s.line, "icon": s.icon, "holds": holds});
        // an empty project, this version's starter house, and under them
        // the starter houses of earlier versions, each with its picture
        let mut starts: Vec<Value> = vec![card(
            &crate::app::appmenu::STARTS[0],
            "Two levels · the standard look",
        )];
        for (i, h) in crate::app::appmenu::STARTER_HOUSES.iter().enumerate() {
            let pic = format!("data:image/jpeg;base64,{}", crate::help::base64(h.picture));
            // the version it came with, as a tag on its tile (owner, MU109)
            let tag = format!("v{}", h.version);
            starts.push(if i == 0 {
                // under its own name, with "Starter house" on its picture
                json!({"id": h.id, "title": h.name, "line": h.line, "badge": "Starter house",
                       "icon": "house", "holds": h.holds, "pic": pic, "tag": tag})
            } else {
                json!({"id": h.id, "title": h.name, "line": h.line,
                       "icon": "house", "holds": h.holds, "pic": pic, "tag": tag, "earlier": true})
            });
        }
        let samples: Vec<Value> = crate::app::appmenu::SAMPLES
            .iter()
            .map(|s| card(s, "Opens as a new, unsaved project"))
            .collect();
        let mine: Vec<Value> = templates::list(&templates::templates_dir())
            .iter()
            .map(|t| {
                json!({
                    "id": format!("new.template:{}", t.path.display()),
                    "pic": creabim_store::picture(&t.path),
                    "title": t.name,
                    "line": t.when.map_or("Your template".to_string(), |w| format!("Saved {}", templates::ago_label(std::time::SystemTime::now(), w))),
                    "icon": "layout-dashboard",
                    "holds": "Its sheets, styles and building",
                })
            })
            .collect();
        let standard = match self.shell.settings.standard.as_str() {
            s @ ("us" | "metric" | "uk") => s,
            _ => "metric",
        };
        // CreaBIM's own sets: drawn by this version, each with a picture
        // of its sheet
        let sets: Vec<Value> = templates::sets()
            .iter()
            .map(|s| {
                json!({
                    "id": format!("new.set:{}", s.key),
                    "set": true,
                    "pic": s.picture,
                    "pics": s.pictures.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>(),
                    "title": s.name,
                    "line": s.line,
                    "icon": "layout-dashboard",
                    "holds": "Its title blocks for every paper, colours, font and pens",
                })
            })
            .collect();
        json!({
            "title": "New project",
            // the drawing standard an empty project takes its units from
            "standard": standard,
            "standards": [["metric", "Metric"], ["us", "US"], ["uk", "UK"]],
            "sources": [
                {"key": "start", "label": "Start", "icon": "file-plus", "sub": "The everyday starts", "items": starts,
                 "earlier": "Earlier starter houses"},
                {"key": "sets", "label": "Templates", "icon": "layout-dashboard", "count": sets.len(), "sub": "CreaBIM's sets · a look for your sheets", "items": sets},
                {"key": "mine", "label": "My templates", "icon": "folder", "count": mine.len(), "sub": "Your own", "items": mine, "folder": true,
                 "hint": "Make more with File › Save as template…",
                 "empty": {"title": "No templates of your own yet", "line": "Open a project that looks the way you like, then choose File › Save as template… It will appear here. A template you downloaded goes in the same folder."}},
                {"key": "samples", "label": "Samples", "icon": "house", "count": samples.len(), "sub": "Samples to learn from · each shows one thing", "items": samples},
            ],
            "note": "A template or a sample opens as a new, unsaved project, so the original is never changed.",
        })
    }

    /// Open the door and window picker under `r` (Properties' Change…).
    pub(super) fn open_kinds(&mut self, key: &str, r: egui::Rect) {
        self.shell.popup = Some(Popup {
            key: key.to_string(),
            at: egui::pos2(r.left().min(r.right() - 640.0).max(8.0), r.bottom() + 6.0),
            up: None,
            above: Some(r.top()),
            size: egui::vec2(640.0, 420.0),
        });
    }

    pub(super) fn close_popup(&mut self) {
        // the Save the changes window goes only by its own buttons and keys
        if self.close_ask
            && self
                .shell
                .popup
                .as_ref()
                .is_some_and(|p| p.key == CLOSE_KEY)
        {
            return;
        }
        // and so do the export and import cards, and Report a problem
        if self.mark.is_some() && self.shell.popup.as_ref().is_some_and(|p| p.key == MARK_KEY) {
            return;
        }
        if self.report_ask.is_some()
            && self
                .shell
                .popup
                .as_ref()
                .is_some_and(|p| p.key == REPORT_KEY)
        {
            return;
        }
        self.shell.popup = None;
        if let Some(web) = &mut self.shell.web {
            web.hide_popup();
        }
    }

    /// The Save the changes window, in the middle of the window, open
    /// while the close is asked (it takes the place of any menu).
    pub(super) fn show_close_dialog(&mut self, ctx: &egui::Context) {
        if self
            .shell
            .popup
            .as_ref()
            .is_some_and(|p| p.key == CLOSE_KEY)
        {
            return;
        }
        let screen = ctx.viewport_rect();
        // the page says how tall it is once drawn
        let size = egui::vec2(480.0, 160.0);
        self.shell.popup = Some(Popup {
            key: CLOSE_KEY.into(),
            at: screen.center() - size / 2.0,
            up: None,
            above: None,
            size,
        });
        self.shell.focus_popup = true;
    }

    /// The export or import card, in the middle of the window.
    pub(super) fn show_mark_card(&mut self, ctx: &egui::Context) {
        if self.shell.popup.as_ref().is_some_and(|p| p.key == MARK_KEY) {
            return;
        }
        let screen = ctx.viewport_rect();
        // the page says how tall it is once drawn
        let size = egui::vec2(440.0, 160.0);
        self.shell.popup = Some(Popup {
            key: MARK_KEY.into(),
            at: screen.center() - size / 2.0,
            up: None,
            above: None,
            size,
        });
        self.shell.focus_popup = true;
    }

    /// The Report a problem card, in the middle of the window.
    pub(super) fn show_report_card(&mut self, ctx: &egui::Context) {
        if self
            .shell
            .popup
            .as_ref()
            .is_some_and(|p| p.key == REPORT_KEY)
        {
            return;
        }
        let screen = ctx.viewport_rect();
        // the page says how tall it is once drawn
        let size = egui::vec2(470.0, 180.0);
        self.shell.popup = Some(Popup {
            key: REPORT_KEY.into(),
            at: screen.center() - size / 2.0,
            up: None,
            above: None,
            size,
        });
        self.shell.focus_popup = true;
    }

    pub(super) fn hide_report_card(&mut self) {
        if let Some(web) = &mut self.shell.web {
            web.shade(false);
        }
        if self
            .shell
            .popup
            .as_ref()
            .is_some_and(|p| p.key == REPORT_KEY)
        {
            self.shell.popup = None;
            if let Some(web) = &mut self.shell.web {
                web.hide_popup();
            }
        }
    }

    pub(super) fn hide_mark_card(&mut self) {
        if let Some(web) = &mut self.shell.web {
            web.shade(false);
        }
        if self.shell.popup.as_ref().is_some_and(|p| p.key == MARK_KEY) {
            self.shell.popup = None;
            if let Some(web) = &mut self.shell.web {
                web.hide_popup();
            }
        }
    }

    pub(super) fn hide_close_dialog(&mut self) {
        if let Some(web) = &mut self.shell.web {
            web.shade(false);
        }
        if self
            .shell
            .popup
            .as_ref()
            .is_some_and(|p| p.key == CLOSE_KEY)
        {
            self.shell.popup = None;
            if let Some(web) = &mut self.shell.web {
                web.hide_popup();
            }
        }
    }

    fn handle_web(&mut self, ctx: &egui::Context, msg: Msg) {
        let b = &msg.body;
        let t = b["t"].as_str().unwrap_or("");
        match t {
            "ready" => {
                if msg.pane == Pane::Help {
                    self.shell.help.resend();
                }
                if let Some(web) = &mut self.shell.web {
                    web.resend(msg.pane);
                }
            }
            "down" if msg.pane != Pane::Popup => self.close_popup(),
            #[cfg(target_os = "macos")]
            "mac-titlebar" if msg.pane == Pane::Top => {
                self.shell.mac_titlebar_rendered = b["revision"].as_u64();
            }
            "focus" => {
                if let Some(web) = &mut self.shell.web {
                    web.focus_views();
                }
            }
            "close" => self.close_popup(),
            // Save the changes: save, discard or cancel
            "closeask" => {
                let what = b["do"].as_str().unwrap_or("").to_string();
                self.close_do(ctx, &what);
            }
            // Report a problem's buttons and its tick
            "report" => {
                let what = b["do"].as_str().unwrap_or("").to_string();
                self.report_do(ctx, &what);
            }
            // Save as template's card: everything it chose comes with Save
            "tpl" => {
                if b["do"].as_str() == Some("save") {
                    let name = b["name"].as_str().unwrap_or("").to_string();
                    let mut keep = crate::app::templates::look_only();
                    if let Some(a) = b["parts"].as_array() {
                        for (i, v) in a.iter().take(keep.len()).enumerate() {
                            keep[i] = v.as_bool().unwrap_or(false);
                        }
                    }
                    let to_file = b["to"].as_str() == Some("file");
                    self.close_popup();
                    self.save_template_card(&name, keep, to_file);
                } else {
                    self.close_popup();
                }
            }
            // the export and import cards' buttons and choices
            "mark" => {
                let what = b["do"].as_str().unwrap_or("").to_string();
                let value = b["value"].as_str().unwrap_or("").to_string();
                self.mark_do(ctx, &what, &value);
            }
            "bar" => self.shell.bar = Some(crate::optionbar::Measure::from_json(b)),
            // a double-click in the project browser: a type opens on its
            // own, anything else in the model is framed in 3D
            "open" => {
                let want = b["id"].as_str().unwrap_or("");
                let Some(e) = self.doc.model().iter().find(|e| e.id.0.to_string() == want) else {
                    return;
                };
                match e.kind {
                    ElementKind::ElementType(_) => self.open_type_preview(e.id),
                    ElementKind::View(_) | ElementKind::Sheet(_) => {}
                    _ => {
                        self.selection.only(e.id);
                        self.frame_selection();
                    }
                }
            }
            "reorder" => {
                let id = |k: &str| {
                    let s = b[k].as_str()?;
                    self.doc
                        .model()
                        .iter()
                        .find(|e| e.id.0.to_string() == s)
                        .map(|e| e.id)
                };
                if let Some(moved) = id("id") {
                    let tx = crate::browser::reorder(self.doc.model(), moved, id("before"));
                    let r = self.doc.commit(tx);
                    let name = self
                        .doc
                        .model()
                        .get(moved)
                        .map(|e| e.name.clone())
                        .unwrap_or_default();
                    self.report(
                        r,
                        format!("Moved {name}. The group keeps your order; its sort button puts it back."),
                    );
                }
            }
            "act" => {
                let id = b["id"].as_str().unwrap_or("").to_string();
                let keep = b["keep"].as_bool().unwrap_or(false);
                if msg.pane == Pane::Popup && !keep {
                    self.close_popup();
                }
                self.run_action(ctx, &id);
            }
            "tip" => {
                // a web row's hint, drawn by the app in its theme (MU26)
                let text = b["text"].as_str().unwrap_or("").trim().to_string();
                // asked for while the pointer moves over the drawing area: a
                // bar redrawn under where it last saw the pointer
                let now = ctx.input(|i| i.time);
                let stale = now - self.pointer_seen < crate::app::TIP_PAUSE;
                self.shell.web_tip = match (&self.shell.web, text.is_empty() || stale) {
                    (Some(web), false) => {
                        let o = web.rects[msg.pane as usize].min;
                        let f = |k: &str| b[k].as_f64().unwrap_or(0.0) as f32;
                        let r = egui::Rect::from_min_size(
                            o + egui::vec2(f("x"), f("y")),
                            egui::vec2(f("w"), f("h")),
                        );
                        Some((text, r))
                    }
                    _ => None,
                };
            }
            "popup" => {
                let Some(web) = &self.shell.web else { return };
                let origin = web.rects[msg.pane as usize].min;
                let x = b["x"].as_f64().unwrap_or(0.0) as f32;
                let y = b["y"].as_f64().unwrap_or(0.0) as f32;
                let w = b["w"].as_f64().unwrap_or(0.0) as f32;
                let up = b["up"].as_f64().map(|u| origin.y + u as f32);
                let above = b["top"].as_f64().map(|u| origin.y + u as f32);
                let key = b["key"].as_str().unwrap_or("").to_string();
                // no menus over the Save the changes window or a card
                if self.close_ask || self.mark.is_some() {
                    return;
                }
                let n = self.menu_items(&key).len() as f32;
                self.shell.popup = Some(Popup {
                    key,
                    at: origin + egui::vec2(x, y),
                    up,
                    above,
                    size: egui::vec2(w.max(220.0), 12.0 + 29.0 * n),
                });
            }
            // the tip's page has laid its tip out: the size to give it
            "size" if msg.pane == Pane::Tip => {
                let text = b["text"].as_str().unwrap_or("").to_string();
                let v = egui::vec2(
                    b["w"].as_f64().unwrap_or(0.0) as f32,
                    b["h"].as_f64().unwrap_or(0.0) as f32,
                );
                self.shell.tip_size = Some((text, v));
                ctx.request_repaint();
            }
            "size" => {
                // the New project window keeps the size it opened with;
                // its report says only that it has drawn itself
                if self
                    .shell
                    .popup
                    .as_ref()
                    .is_some_and(|p| p.key == "newproject")
                {
                    if let Some(g) = self
                        .shell
                        .card_gate
                        .as_mut()
                        .filter(|g| g.0 == "newproject")
                    {
                        g.2 = true;
                        ctx.request_repaint();
                    }
                }
                if let Some(p) = self.shell.popup.as_mut().filter(|p| p.key != "newproject") {
                    let h = b["h"].as_f64().unwrap_or(100.0) as f32;
                    // the palette and Save the changes keep the width they
                    // were opened with
                    let w = match p.key.as_str() {
                        "palette" | CLOSE_KEY | MARK_KEY | REPORT_KEY | TEMPLATE_KEY => p.size.x,
                        _ => b["w"].as_f64().unwrap_or(200.0) as f32,
                    };
                    let size = egui::vec2(w, h);
                    // placed again with it: the frame that brought it has
                    // placed the popup already
                    if p.size != size {
                        p.size = size;
                        ctx.request_repaint();
                    }
                    // a card's first real height (measured at its own width,
                    // not while parked) fixes where its top stays
                    let own_width = (b["w"].as_f64().unwrap_or(0.0) as f32 - p.size.x).abs() < 1.5;
                    // a card measured at its own width has drawn itself
                    if own_width {
                        if let Some(g) = self.shell.card_gate.as_mut().filter(|g| g.0 == p.key) {
                            g.2 = true;
                            ctx.request_repaint();
                        }
                    }
                    if p.key == MARK_KEY && own_width {
                        if let Some(card) = &self.mark {
                            if card.top.get().is_none() {
                                let screen = ctx.viewport_rect();
                                card.top.set(Some(screen.center().y - h / 2.0));
                            }
                        }
                    }
                }
            }
            "set" if b["key"].as_str() == Some("time.note") => {
                let text = b["text"].as_str().unwrap_or("").to_string();
                self.time_set_note(&text);
            }
            "set" => {
                // a length field sends what was typed, in any unit
                let v = match (b["value"].as_f64(), b["text"].as_str()) {
                    (Some(v), _) => v,
                    (None, Some(t)) => match crate::units::parse(t) {
                        Some(v) => v,
                        None => {
                            self.status = format!(
                                "Couldn't read \"{t}\" as a length. Try 3'-6 1/2\", 1067 or 1.2 m."
                            );
                            return;
                        }
                    },
                    _ => return,
                };
                self.set_tool_number(b["key"].as_str().unwrap_or(""), v);
            }
            "pick" => {
                let want = b["id"].as_str().unwrap_or("");
                // a profile of the browser's Profiles group (MU27 block 5)
                // a section of the browser's Notes group
                if let Some(key) = want.strip_prefix("notes:") {
                    let key = key.to_string();
                    self.open_notes(&key);
                    return;
                }
                if let Some(name) = want.strip_prefix("render:") {
                    let name = name.to_string();
                    self.open_render(&name);
                    return;
                }
                if let Some(g) = want
                    .strip_prefix("group:")
                    .and_then(|g| g.parse::<u32>().ok())
                {
                    self.pick_group(g, b["add"].as_bool().unwrap_or(false));
                    return;
                }
                if let Some(t) = want.strip_prefix("kindtype:") {
                    let id = self
                        .doc
                        .model()
                        .iter()
                        .find(|e| e.id.0.to_string() == t)
                        .map(|e| e.id);
                    self.open_types(id);
                    return;
                }
                if let Some(name) = want.strip_prefix("profile:") {
                    let name = name.to_string();
                    self.profiles_action("profiles.open", &name);
                    return;
                }
                // a master of the browser's Masters group (MU40)
                if let Some(id) = want.strip_prefix("master:") {
                    let id = id.to_string();
                    self.masters_action("master.open", &id);
                    return;
                }
                let Some(e) = self.doc.model().iter().find(|e| e.id.0.to_string() == want) else {
                    return;
                };
                let (id, opens) = (
                    e.id,
                    matches!(e.kind, ElementKind::View(_) | ElementKind::Sheet(_)),
                );
                if b["add"].as_bool().unwrap_or(false) {
                    self.selection.toggle(id);
                } else {
                    self.selection.only(id);
                    if opens {
                        self.active_view = Some(id);
                        // a view shows how it's drawn, what you're about to
                        // change (Mockup_View_Tabs_REV00, decision 3)
                        if self.view(id).is_some() {
                            self.prop_tab = super::PropTab::View;
                        }
                    }
                }
                self.face = None;
            }
            "key" => {
                let code = b["code"].as_str().unwrap_or("");
                let name = b["key"].as_str().unwrap_or("");
                let Some(key) = egui::Key::from_name(code).or_else(|| egui::Key::from_name(name))
                else {
                    return;
                };
                let ctrl = b["ctrl"].as_bool().unwrap_or(false);
                let modifiers = egui::Modifiers {
                    alt: b["alt"].as_bool().unwrap_or(false),
                    ctrl,
                    shift: b["shift"].as_bool().unwrap_or(false),
                    mac_cmd: false,
                    command: ctrl,
                };
                for pressed in [true, false] {
                    self.shell.keys.push(egui::Event::Key {
                        key,
                        physical_key: Some(key),
                        pressed,
                        repeat: b["repeat"].as_bool().unwrap_or(false),
                        modifiers,
                    });
                }
                if !ctrl && name.chars().count() == 1 {
                    self.shell.keys.push(egui::Event::Text(name.into()));
                }
                ctx.request_repaint();
            }
            _ => {}
        }
    }

    #[cfg(target_os = "macos")]
    fn place_mac_drag(
        &mut self,
        frame: &eframe::Frame,
        ctx: &egui::Context,
        web_top: Option<egui::Rect>,
    ) {
        if !self.uses_native_menu() {
            return;
        }
        let titlebar = web_top.unwrap_or_else(|| {
            let viewport = ctx.viewport_rect();
            egui::Rect::from_min_size(
                viewport.min,
                egui::vec2(viewport.width(), crate::mac_drag::TITLEBAR_HEIGHT),
            )
        });
        let allowed = crate::app::clear_of_card(ctx, titlebar) == Some(titlebar)
            && !self.mac_menu_blocked()
            && self.shell.popup.is_none();
        if self.shell.mac_drag.is_none() {
            self.shell.mac_drag = crate::mac_drag::MacDrag::new(frame);
        }
        let Some(drag) = &self.shell.mac_drag else {
            return;
        };
        let layout = drag.layout(ctx.zoom_factor());
        if layout != self.shell.mac_titlebar {
            self.shell.mac_titlebar = layout;
            self.shell.mac_titlebar_revision += 1;
        }
        if let Some(layout) = layout {
            // WebKit applies interface-scale changes asynchronously.
            let ready = web_top.is_none()
                || self.shell.mac_titlebar_rendered == Some(self.shell.mac_titlebar_revision);
            drag.place(layout, allowed && ready);
        }
    }

    /// Lay out the web panes round the views. Returns false when there is
    /// no web interface, and the classic panels are drawn instead.
    pub(super) fn web_panels(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) -> bool {
        if self.shell.classic || self.shell.failed.is_some() {
            #[cfg(target_os = "macos")]
            self.place_mac_drag(frame, ui.ctx(), None);
            WebUi::pump();
            if let Some(web) = &self.shell.web {
                for m in web.take() {
                    let _ = m;
                }
            }
            return false;
        }
        if self.shell.web.is_none() {
            #[cfg(target_os = "macos")]
            {
                // Place the hit region after the newly attached WKWebView.
                self.shell.mac_drag = None;
                self.shell.mac_titlebar_rendered = None;
            }
            match WebUi::new(frame, ui.ctx()) {
                Ok(w) => self.shell.web = Some(w),
                Err(e) => {
                    self.status = format!("Using the classic panels: {e}");
                    self.shell.failed = Some(e);
                    #[cfg(target_os = "macos")]
                    self.place_mac_drag(frame, ui.ctx(), None);
                    return false;
                }
            }
        }
        let ctx = ui.ctx().clone();
        WebUi::pump();
        let msgs = self
            .shell
            .web
            .as_ref()
            .map(|w| w.take())
            .unwrap_or_default();
        for m in msgs {
            self.handle_web(&ctx, m);
        }
        // what the app draws itself: below the bars, right of the browser,
        // above the status bar
        if let Some(web) = &self.shell.web {
            use crate::webui::Pane;
            let mut r = ctx.content_rect();
            let top = web.rects[Pane::Top as usize];
            let left = web.rects[Pane::Left as usize];
            let status = web.rects[Pane::Status as usize];
            if top.height() > 1.0 {
                r.min.y = r.min.y.max(top.bottom());
            }
            if left.width() > 1.0 {
                r.min.x = r.min.x.max(left.right());
            }
            if status.height() > 1.0 {
                r.max.y = r.max.y.min(status.top());
            }
            NATIVE.with(|n| n.set(Some(r)));
        }
        // a web row's hint, as the app's own tooltips look (MU26)
        let tip = self
            .shell
            .web_tip
            .clone()
            .filter(|_| self.shell.popup.is_none());
        let screen_all = ctx.content_rect();
        let theme = self.theme();
        if let Some(web) = self.shell.web.as_mut() {
            // its own panel over everything (MU102): right under its
            // control, or right above one in the status bar; kept inside
            // the window
            let ppp = ctx.pixels_per_point();
            match &tip {
                Some((text, r)) => {
                    web.send(Pane::Tip, &json!({"theme": theme, "tip": text}));
                    match self.shell.tip_size.as_ref().filter(|(t, _)| t == text) {
                        Some((_, size)) if size.x > 1.0 => {
                            let up = r.bottom() > screen_all.bottom() - 60.0;
                            let y = if up {
                                r.top() - 4.0 - size.y
                            } else {
                                r.bottom() + 4.0
                            };
                            let x = r
                                .left()
                                .min(screen_all.right() - size.x - 4.0)
                                .max(screen_all.left() + 4.0);
                            web.place(
                                Pane::Tip,
                                egui::Rect::from_min_size(egui::pos2(x, y), *size),
                                ppp,
                            );
                        }
                        _ => web.park(Pane::Tip),
                    }
                }
                None => web.park(Pane::Tip),
            }
        }
        if let Some((text, r)) = tip.filter(|_| self.shell.web.is_none()) {
            let screen = ctx.content_rect();
            // a status bar control's tip goes above it: below, it was
            // pushed back up over the browser panel and cut (MU101)
            let up = r.bottom() > screen.bottom() - 60.0;
            let (y, pivot) = if up {
                (r.top() - 6.0, egui::Align2::LEFT_BOTTOM)
            } else {
                (r.bottom() + 6.0, egui::Align2::LEFT_TOP)
            };
            let (span0, span1) = if up { (y - 90.0, y) } else { (y, y + 40.0) };
            // web panels paint over the app's own drawing, so the tip
            // keeps to the right of the browser panel (MU27: it was cut)
            let left_edge = self
                .shell
                .web
                .as_ref()
                .map(|w| w.rects[crate::webui::Pane::Left as usize])
                .filter(|l| l.width() > 1.0 && l.top() < span1 && l.bottom() > span0)
                .map_or(screen.left(), |l| l.right());
            let x = r.left().min(screen.right() - 380.0).max(left_edge + 4.0);
            egui::Area::new(egui::Id::new("web-tip"))
                .order(egui::Order::Tooltip)
                .pivot(pivot)
                .fixed_pos(egui::pos2(x, y))
                .interactable(false)
                .show(&ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_max_width(360.0);
                        ui.label(text);
                    });
                });
        }
        // a tool picked off the tab shown (by its key, a menu or the
        // palette) brings its own tab
        if self.shell.tab_tool != Some(self.plan.tool) {
            self.shell.tab_tool = Some(self.plan.tool);
            if !crate::tooltabs::holds(self.shell.settings.tools_tab, self.plan.tool) {
                self.shell.settings.tools_tab = crate::tooltabs::home_of(self.plan.tool);
            }
        }
        if self.shell.classic {
            // just switched to the classic panels
            return false;
        }
        // A click in the views or Escape closes an open menu.
        if self.shell.popup.is_some()
            && ctx.input(|i| i.pointer.any_pressed() || i.key_pressed(egui::Key::Escape))
        {
            self.close_popup();
        }

        let ppp = ctx.pixels_per_point();
        let none = egui::Frame::NONE;
        let top = egui::Panel::top("web_top")
            .exact_size(TOP_H)
            .resizable(false)
            .show_separator_line(false)
            .frame(none)
            .show(ui, |_| ())
            .response
            .rect;
        let status = egui::Panel::bottom("web_status")
            .exact_size(STATUS_H)
            .resizable(false)
            .show_separator_line(false)
            .frame(none)
            .show(ui, |_| ())
            .response
            .rect;
        // drag the browser's right edge to widen it
        let folded = self.shell.settings.left_folded;
        let left = egui::Panel::left(if folded { "web_left_rail" } else { "web_left" })
            .default_size(if folded {
                44.0
            } else {
                self.shell.settings.left_width
            })
            .size_range(if folded { 44.0..=44.0 } else { 180.0..=520.0 })
            .resizable(!folded)
            .show_separator_line(true)
            .frame(egui::Frame::NONE.fill(ui.visuals().panel_fill))
            .show(ui, |ui| ui.allocate_space(ui.available_size()))
            .response
            .rect;
        // the page sits just inside the edge so the edge stays grabbable
        let left = left.with_max_x(left.max.x - 3.0);
        // the browser's fold handle on its right edge (MU29)
        if crate::app::fold_handle(
            &ctx,
            "browser",
            egui::pos2(left.right() + 3.0, left.center().y - 23.0),
            folded,
            false,
            if folded {
                "Open the project browser (Ctrl+[)"
            } else {
                "Fold the browser to a rail (Ctrl+[)"
            },
        ) {
            self.shell.settings.left_folded = !folded;
            self.shell.settings.store();
        }
        let width = (left.width() + 3.0).round();
        if !folded
            && (width - self.shell.settings.left_width).abs() >= 1.0
            && !ctx.input(|i| i.pointer.any_down())
        {
            self.shell.settings.left_width = width;
            self.shell.settings.store();
        }
        // the guide, at the right of everything while it is open; drag
        // its left edge to widen it
        // it slides in and out over a quarter second, easing at both
        // ends, so the views beside it make room gently (MU64: it jumped
        // open and shut and the eye lost its place); the guide itself
        // keeps its full width and slides, its text never reflowing
        let slide = ctx.animate_bool_with_time_and_easing(
            egui::Id::new("help.slide"),
            self.shell.help.open,
            0.25,
            egui::emath::easing::cubic_in_out,
        );
        let help = (slide > 0.0).then(|| {
            let full = self.shell.settings.help_width;
            let sliding = slide < 1.0;
            let slid_id = egui::Id::new("help.slid");
            // the frame after a slide pins the width once more, so the
            // eased width egui remembers never becomes the saved one
            let was_sliding = ctx.data(|d| d.get_temp::<bool>(slid_id)).unwrap_or(false);
            ctx.data_mut(|d| d.insert_temp(slid_id, sliding));
            let panel = egui::Panel::right("web_help")
                .show_separator_line(true)
                .frame(egui::Frame::NONE.fill(ui.visuals().panel_fill));
            let panel = if sliding {
                panel.exact_size((full * slide).max(1.0)).resizable(false)
            } else if was_sliding {
                panel.exact_size(full).resizable(true)
            } else {
                panel
                    .default_size(full)
                    .size_range(300.0..=760.0)
                    .resizable(true)
            };
            let r = panel
                .show(ui, |ui| ui.allocate_space(ui.available_size()))
                .response
                .rect;
            if sliding {
                ctx.request_repaint();
            } else {
                let width = r.width().round();
                if (width - full).abs() >= 1.0 && !ctx.input(|i| i.pointer.any_down()) {
                    self.shell.settings.help_width = width;
                    self.shell.settings.store();
                }
            }
            let w = if sliding { full } else { r.width() };
            egui::Rect::from_min_max(
                egui::pos2(r.min.x + 3.0, r.min.y),
                egui::pos2(r.min.x + w, r.max.y),
            )
        });
        let help_state = help.and_then(|_| self.help_state());

        // the 3D view's buttons, over its top-right corner (where it was
        // drawn last frame)
        let nav = self
            .rect_3d
            // docked, the 3D area's header has these
            .filter(|_| self.placement != PlanPlacement::Docked)
            .filter(|r| r.width() > 320.0 && r.height() > 120.0)
            .map(|r| {
                // as wide as it needs, or as the view when that is narrower
                let w = NAV_W.min(r.width() - 16.0);
                // the overlaid 2D view takes the top-right corner
                let x = if self.placement == PlanPlacement::Overlay {
                    r.left() + 36.0
                } else {
                    r.right() - w - 8.0
                };
                egui::Rect::from_min_size(egui::pos2(x, r.top() + 8.0), egui::vec2(w, NAV_H))
            })
            // a web pane draws over everything egui draws, so the buttons
            // step aside while a window (Types, Assemblies, materials, a
            // type's preview) lies over them
            .filter(|n| {
                !ctx.memory(|m| {
                    m.areas().visible_layer_ids().into_iter().any(|l| {
                        l.order == egui::Order::Middle
                            && m.area_rect(l.id).is_some_and(|a| a.intersects(*n))
                    })
                })
            });
        #[cfg(target_os = "macos")]
        self.place_mac_drag(frame, &ctx, Some(top));
        let states = [
            (Pane::Top, top, {
                let _p = crate::app::Perf::new("top_state");
                self.top_state(&ctx)
            }),
            (Pane::Left, left, {
                let _p = crate::app::Perf::new("left_state");
                self.left_state()
            }),
            (Pane::Status, status, self.status_state()),
        ];
        let nav_state = self.nav_state();
        // web menus float over the whole window, the web bands included
        let screen = ctx.viewport_rect();
        let popup = self.shell.popup.as_ref().map(|p| {
            let up = p.up.or_else(|| opens_up(p, screen));
            let size = popup_size(&Popup { up, ..p.clone() }, screen);
            let mut at = p.at;
            if let Some(bottom) = up {
                at.y = bottom - size.y;
            }
            at.x =
                at.x.min(screen.right() - size.x - 4.0)
                    .max(screen.left() + 4.0);
            at.y =
                at.y.min(screen.bottom() - size.y - 4.0)
                    .max(screen.top() + 4.0);
            let rect = egui::Rect::from_min_size(at, size);
            match p.key.as_str() {
                // the strip's controls that don't fit it
                "more" => {
                    let (head, tool, view, _) = self.option_strip(&ctx);
                    let rows = crate::optionbar::more_rows(
                        &tool,
                        &view,
                        head["label"].as_str().unwrap_or(""),
                    );
                    let empty = rows.is_empty();
                    let state = json!({"theme": self.theme(), "key": p.key, "items": [], "controls": rows});
                    return (rect, state, empty);
                }
                // the New project window keeps its own size, but never
                // more than the window has now: one made smaller after it
                // opened still shows the buttons (issue 24)
                "newproject" => {
                    let room = (screen.size() - egui::vec2(32.0, 32.0)).max(egui::vec2(240.0, 200.0));
                    let rect = egui::Rect::from_center_size(screen.center(), p.size.min(room));
                    let state = json!({"theme": self.theme(), "key": p.key, "items": [], "dialog": self.new_project_state()});
                    return (rect, state, false);
                }
                // Save the changes: in the middle, as tall as its page asks,
                // never more than the window (it scrolls then)
                CLOSE_KEY => {
                    let room = (screen.size() - egui::vec2(32.0, 32.0)).max(egui::vec2(240.0, 120.0));
                    let rect = egui::Rect::from_center_size(screen.center(), p.size.min(room));
                    let state = json!({"theme": self.theme(), "key": p.key, "items": [], "close": self.close_state_json()});
                    return (rect, state, false);
                }
                // Report a problem: as the Save the changes window
                REPORT_KEY => {
                    let room = (screen.size() - egui::vec2(32.0, 32.0)).max(egui::vec2(240.0, 120.0));
                    let rect = egui::Rect::from_center_size(screen.center(), p.size.min(room));
                    let state = json!({"theme": self.theme(), "key": p.key, "items": [], "report": self.report_state_json()});
                    return (rect, state, false);
                }
                // Save as template's card
                TEMPLATE_KEY => {
                    let room = (screen.size() - egui::vec2(32.0, 32.0)).max(egui::vec2(240.0, 120.0));
                    let rect = egui::Rect::from_center_size(screen.center(), p.size.min(room));
                    let state = json!({"theme": self.theme(), "key": p.key, "items": [], "tpl": self.template_card_json()});
                    return (rect, state, false);
                }
                // an export or import card: as the Save the changes window
                MARK_KEY => {
                    let room = (screen.size() - egui::vec2(32.0, 32.0)).max(egui::vec2(240.0, 120.0));
                    let size = p.size.min(room);
                    // in the middle when it opens, then its top stays: it
                    // grows down as its job ends or its report opens
                    let rect = match self.mark.as_ref().and_then(|c| c.top.get()) {
                        Some(top) => {
                            let top = top
                                .min(screen.bottom() - 16.0 - size.y)
                                .max(screen.top() + 16.0);
                            egui::Rect::from_min_size(
                                egui::pos2(screen.center().x - size.x / 2.0, top),
                                size,
                            )
                        }
                        None => egui::Rect::from_center_size(screen.center(), size),
                    };
                    let state = json!({"theme": self.theme(), "key": p.key, "items": [], "mark": self.mark_state_json()});
                    return (rect, state, false);
                }
                // the door and window picker (picker REV00)
                k if k.starts_with("kinds:") => {
                    let state = json!({"theme": self.theme(), "key": p.key, "items": [], "kinds": self.kinds_popup_json(k)});
                    return (rect, state, false);
                }
                // project time's panel, over the pill
                "time" => {
                    let state = json!({"theme": self.theme(), "key": p.key, "items": [], "time": self.time_popup_json()});
                    return (rect, state, false);
                }
                // a few lines on the tool in use
                "toolhelp" => {
                    // never over a view's own toolbar or over Properties:
                    // under the 3D view's buttons when it would cover them
                    let mut rect = rect;
                    if let Some(n) = nav.filter(|n| n.intersects(rect)) {
                        rect = rect.translate(egui::vec2(0.0, n.bottom() + 6.0 - rect.top()));
                    }
                    let right = [self.rect_2d, self.rect_3d]
                        .into_iter()
                        .flatten()
                        .map(|r| r.right())
                        .fold(f32::NEG_INFINITY, f32::max);
                    if right.is_finite() && rect.right() > right - 4.0 {
                        rect = rect.translate(egui::vec2(right - 4.0 - rect.right(), 0.0));
                    }
                    let help = self.tool_quick_help();
                    let state = json!({"theme": self.theme(), "key": p.key, "items": [], "help": help});
                    return (rect, state, false);
                }
                _ => {}
            }
            let items = self.menu_items(&p.key);
            // the Site family's panel has six headed groups: side by side,
            // so it never scrolls (AMD-011). The Create menu, a family a
            // submenu since wave 2, is short enough for one column
            let columns = match p.key.as_str() {
                "fam:site" => 3,
                _ => 1,
            };
            (
                rect,
                json!({"theme": self.theme(), "key": p.key, "items": ui_model::list(&items), "columns": columns}),
                items.is_empty(),
            )
        });
        let icons_extra = self.profile_thumbs();
        let dir = if self.shell.settings.window_buttons == "left" {
            "left"
        } else {
            "right"
        };
        let card = crate::theme::Palette::of(&ctx).panel2;
        let Some(web) = &mut self.shell.web else {
            return false;
        };
        web.dir = dir;
        web.set_card_ground(card);
        if web.tick() {
            ctx.request_repaint();
        }
        // the pages at the interface scale, as egui is (#96)
        web.set_zoom(ctx.zoom_factor());
        // the splash dims the whole window: the web bands dim themselves,
        // as egui's shade cannot reach over them (MU79)
        let dim = self.splash_open;
        for (pane, rect, mut state) in states {
            if let Some(o) = state.as_object_mut() {
                o.insert("dim".into(), json!(dim));
            }
            match crate::app::clear_of_card(&ctx, rect) {
                Some(rect) => web.place(pane, rect, ppp),
                None => web.park(pane),
            }
            web.send(pane, &state);
        }
        match nav.map(|r| crate::app::clear_of_card(&ctx, r)) {
            Some(None) => web.park(Pane::Nav),
            Some(Some(rect)) => {
                web.place(Pane::Nav, rect, ppp);
                let mut nav_state = nav_state;
                if let Some(o) = nav_state.as_object_mut() {
                    o.insert("dim".into(), json!(dim));
                }
                web.send(Pane::Nav, &nav_state);
            }
            None => web.park(Pane::Nav),
        }
        match help.map(|r| (r, crate::app::clear_of_card(&ctx, r))) {
            Some((_, None)) => web.park(Pane::Help),
            Some((_, Some(rect))) => {
                web.place(Pane::Help, rect, ppp);
                if let Some(state) = help_state {
                    web.send(Pane::Help, &state);
                }
            }
            None => web.park(Pane::Help),
        }
        match popup {
            Some((_, _, true)) => self.close_popup(),
            Some((rect, state, false)) => {
                let mut state = state;
                state["icons"] = serde_json::Value::Object(icons_extra);
                web.send(Pane::Popup, &state);
                // a card over the shaded app comes on whole, and once the
                // shade is on screen: its window keeps what lay behind it
                // when it came, so one that came early showed as a pale
                // block, and its round corners as lighter squares (MU103)
                let key = state["key"].as_str().unwrap_or("").to_string();
                let ready = if matches!(
                    key.as_str(),
                    CLOSE_KEY | MARK_KEY | REPORT_KEY | TEMPLATE_KEY | "newproject"
                ) {
                    let now = std::time::Instant::now();
                    let g = match &mut self.shell.card_gate {
                        Some(g) if g.0 == key => g,
                        slot => slot.insert((key, 0, false, now, now)),
                    };
                    // the window minimised, or come back to: the card
                    // leaves and comes on again once the app is drawn
                    // behind it. Left on, it kept the black of a window
                    // not yet drawn: black square corners, and a dark
                    // card while Windows brought the window up (MU103)
                    let (minimised, back, left) = ctx.input(|i| {
                        (
                            i.viewport().minimized == Some(true),
                            i.events
                                .iter()
                                .any(|e| matches!(e, egui::Event::WindowFocused(true))),
                            i.events
                                .iter()
                                .any(|e| matches!(e, egui::Event::WindowFocused(false))),
                        )
                    });
                    // come back to means away for a while (another
                    // window in front). A click inside the card also
                    // passes the focus away and back, at once: that took
                    // the card off for a moment at every click (MU120)
                    let away = egui::Id::new("card-gate.away");
                    if left && !back {
                        ctx.data_mut(|d| d.insert_temp(away, Some(now)));
                    }
                    let _back = back && {
                        let since = ctx.data_mut(|d| {
                            let t = d.get_temp::<Option<std::time::Instant>>(away).flatten();
                            d.insert_temp(away, None::<std::time::Instant>);
                            t
                        });
                        since.is_some_and(|t| now.duration_since(t).as_millis() > 400)
                    };
                    // (held while minimised; a little after it is back)
                    let held = g.4 > now + std::time::Duration::from_secs(1);
                    if minimised {
                        g.4 = now + std::time::Duration::from_secs(3600);
                    } else if held {
                        // (only back from being minimised: coming back from
                        // another window in front, the card's window kept
                        // its picture, and taking it off made it blink,
                        // MU102 with the snipping tool)
                        g.1 = 0;
                        g.3 = now;
                        g.4 = now + std::time::Duration::from_millis(180);
                    }
                    g.1 += 1;
                    // (a page that never reports is not kept back for good)
                    !minimised
                        && now >= g.4
                        && ((g.1 > 3 && g.2) || now.duration_since(g.3).as_millis() > 600)
                } else {
                    self.shell.card_gate = None;
                    true
                };
                if !ready {
                    web.stage_popup(rect, ppp);
                    ctx.request_repaint();
                } else {
                    if web.show_popup(rect, ppp) {
                        ctx.request_repaint();
                    }
                    if std::mem::take(&mut self.shell.focus_popup) {
                        web.focus(Pane::Popup);
                    }
                }
            }
            None => {
                self.shell.card_gate = None;
                web.hide_popup()
            }
        }
        // Pages wake the app when they send a message (see WebUi::new). On
        // Linux they live on GTK, which only runs while frames pump it.
        #[cfg(target_os = "linux")]
        ctx.request_repaint_after(std::time::Duration::from_millis(30));
        true
    }

    /// Open the menu `key` under `r` (a button drawn by egui), or close it
    /// when it is open already.
    pub(super) fn toggle_popup(&mut self, key: &str, r: egui::Rect) {
        if self.shell.web.is_none() {
            return;
        }
        if self.shell.popup.as_ref().is_some_and(|p| p.key == key) {
            self.shell.popup = None;
            return;
        }
        let n = self.menu_items(key).len() as f32;
        // under its button, its left edge on the button's; only a menu that
        // would leave the window ends under the button's right edge (MU91:
        // it opened a long way to the left)
        let (right, bottom) = self
            .areas
            .rects
            .iter()
            .fold((r.right(), r.bottom()), |(x, y), a| {
                (x.max(a.right()), y.max(a.bottom()))
            });
        let x = if r.left() + 240.0 > right + 280.0 {
            r.right() - 240.0
        } else {
            r.left()
        };
        // as tall as its items, short of the status bar; longer lists scroll
        let want = 12.0 + 29.0 * n;
        let below = (bottom - r.bottom() - 16.0).max(120.0);
        self.shell.popup = Some(Popup {
            key: key.into(),
            at: egui::pos2(x, r.bottom() + 4.0),
            up: None,
            above: Some(r.top() - 4.0),
            size: egui::vec2(240.0, want.min(below)),
        });
    }

    /// Open the right-click menu a view asked for this frame.
    pub(super) fn open_view_menu(&mut self) {
        let Some(pos) = self.plan.menu_request.take() else {
            return;
        };
        if self.shell.web.is_none() {
            return;
        }
        let n = self.menu_items("context").len() as f32;
        self.shell.context_3d = self.shell.menu_3d;
        self.shell.popup = Some(Popup {
            key: "context".into(),
            at: pos + egui::vec2(2.0, 2.0),
            up: None,
            above: Some(pos.y - 2.0),
            size: egui::vec2(230.0, 12.0 + 28.0 * n),
        });
    }

    /// The classic egui menu bar, drawn from the same items as the web one.
    pub(super) fn classic_menu(&mut self, ui: &mut egui::Ui) {
        #[cfg(target_os = "macos")]
        if self.uses_native_menu() {
            let (_, mut rect) = ui.allocate_space(egui::vec2(
                ui.available_width(),
                crate::mac_drag::TITLEBAR_HEIGHT,
            ));
            if let Some(layout) = self.shell.mac_titlebar {
                let inset = (layout.inset() as f32).min(rect.width());
                if layout.right {
                    rect.max.x -= inset;
                } else {
                    rect.min.x += inset;
                }
            }
            let mut row = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            row.shrink_clip_rect(rect);
            row.label("File:");
            crate::props::text_field(&mut row, egui::TextEdit::singleline(&mut self.path), 220.0);
            return;
        }
        let mut picked = None;
        egui::MenuBar::new().ui(ui, |ui| {
            for m in ["File", "Edit", "View", "Materials", "Help"] {
                let items = self.menu_items(&format!("menu:{m}"));
                ui.menu_button(m, |ui| {
                    if let Some(id) = ui_model::show(ui, &items) {
                        picked = Some(id);
                    }
                });
            }
            ui.separator();
            ui.label("File:");
            crate::props::text_field(ui, egui::TextEdit::singleline(&mut self.path), 220.0);
        });
        if let Some(id) = picked {
            let ctx = ui.ctx().clone();
            self.run_action(&ctx, &id);
        }
    }
}

/// How big a popup is shown: as its page asks, but never past the window.
/// A menu opening down keeps to the room below where it opens (or the
/// whole window height when that is too little); the page scrolls the rest.
/// Where the bottom of a menu goes that has no room below its button but
/// more above it: it opens upwards from the button's top.
fn opens_up(p: &Popup, screen: egui::Rect) -> Option<f32> {
    let top = p.above?;
    let below = screen.bottom() - STATUS_H - p.at.y - 12.0;
    let above = top - screen.top() - 6.0;
    (below < p.size.y && above > below).then_some(top)
}

/// The popup key of the Save the changes window.
/// The shade behind a card that waits for an answer, over everything egui
/// draws. It comes at once, as the web panels' shade does: egui's own
/// fades in over a fifth of a second, so the bars went dark first and the
/// views after them, and a card that came meanwhile kept the half-shaded
/// picture behind its corners (MU103).
pub(crate) fn card_shade(ctx: &egui::Context, id: &str) {
    let id = egui::Id::new(id);
    egui::Modal::new(id)
        .area(egui::Modal::default_area(id).fade_in(false))
        .frame(egui::Frame::NONE)
        .show(ctx, |_| {});
}

const CLOSE_KEY: &str = "closeask";
const REPORT_KEY: &str = "report";
const MARK_KEY: &str = "markcard";
/// Save as template's card.
const TEMPLATE_KEY: &str = "templatecard";

fn popup_size(p: &Popup, screen: egui::Rect) -> egui::Vec2 {
    let margin = 6.0;
    let room = match p.up {
        Some(bottom) => bottom - screen.top() - margin,
        // short of the status bar, with a gap (owner after MU91: Place
        // view's list still touched it; the page's own size overrode the cap)
        None => screen.bottom() - STATUS_H - p.at.y - 2.0 * margin,
    };
    let room = if room < 240.0 {
        screen.height() - 2.0 * margin
    } else {
        room
    };
    egui::vec2(
        p.size.x.min(screen.width() - 2.0 * margin),
        p.size.y.min(room).max(40.0),
    )
}

/// Where a new project or a sample would be saved: beside the projects
/// opened before, never over one of them.
pub(super) fn new_path(name: &str) -> String {
    format!("{name}.creabim")
}

/// egui's spacing and type, roomier than its defaults.
pub(super) fn style(ctx: &egui::Context, theme: Theme) {
    let p = theme.palette();
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("theme.palette"), p));
    let mode = if p.dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_theme(mode);
    ctx.set_visuals_of(mode, visuals(theme));
    ctx.all_styles_mut(|s| {
        use egui::{FontId, TextStyle};
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(8.0, 3.0);
        s.spacing.interact_size.y = 24.0;
        // a slider's track thick enough to read as one
        s.spacing.slider_rail_height = 6.0;
        // soft corners on every menu, list and popup, as the web menus
        // have (MU35: the app's own were square)
        s.visuals.menu_corner_radius = egui::CornerRadius::same(10);
        s.visuals.window_corner_radius = egui::CornerRadius::same(10);
        s.spacing.menu_margin = egui::Margin::same(6);
        // thin floating scroll bars, like the web panels' (MU18)
        let mut scroll = egui::style::ScrollStyle::thin();
        scroll.foreground_color = false;
        s.spacing.scroll = scroll;
        s.text_styles
            .insert(TextStyle::Body, FontId::proportional(13.0));
        s.text_styles
            .insert(TextStyle::Button, FontId::proportional(13.0));
        s.text_styles
            .insert(TextStyle::Heading, FontId::proportional(16.0));
    });
}

/// `a` moved `t` of the way to `b`.
fn mix(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    egui::Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// egui's look, matched to the web interface so the properties panel and
/// the views' own controls sit with it. Every control has a fill and an
/// outline that stand out from the window it is in, so a button reads as
/// a button, a dropdown as a field with its arrow, a text box as a box and
/// a slider as a track; hovering strengthens both.
pub(super) fn visuals(theme: Theme) -> egui::Visuals {
    use egui::{CornerRadius, Stroke};
    let p = theme.palette();
    let mut v = if p.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = p.panel;
    v.window_fill = p.panel2;
    v.faint_bg_color = p.panel2;
    v.extreme_bg_color = p.field;
    v.text_edit_bg_color = Some(p.field);
    v.slider_trailing_fill = true;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(8);
    // also the filled part of a slider's track
    v.selection.bg_fill = p.accent.gamma_multiply(if p.dark { 0.45 } else { 0.4 });
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.hyperlink_color = p.accent;
    // a steady caret in the accent colour (MU27: the app repaints only on
    // input, so a blinking one was often caught off and seemed missing)
    v.text_cursor.stroke = Stroke::new(2.0, p.accent);
    v.text_cursor.blink = false;
    v.weak_text_color = Some(p.muted);
    v.override_text_color = None;
    let w = &mut v.widgets;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke.color = p.text;
    w.noninteractive.bg_fill = p.panel;
    w.inactive.fg_stroke.color = p.text;
    // buttons, dropdowns and slider tracks are filled a step off the
    // window; text fields stay light (dark in the dark themes), so the two
    // read apart
    let control = if p.dark {
        p.raise
    } else {
        mix(p.panel, p.raise2, 0.7)
    };
    let hover = if p.dark {
        p.raise2
    } else {
        mix(p.raise2, p.edge, 0.6)
    };
    for (state, fill, edge) in [
        (&mut w.inactive, control, p.edge),
        (&mut w.hovered, hover, p.muted),
        (&mut w.active, hover, p.accent),
        (&mut w.open, control, p.accent),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.corner_radius = CornerRadius::same(6);
        state.bg_stroke = Stroke::new(1.0, edge);
    }
    w.noninteractive.corner_radius = CornerRadius::same(6);
    v
}

/// The Align tool, in the Edit and right-click menus.
fn align_item() -> Item {
    Item::new("align", "Align to a reference…")
        .icon("align-tool")
        .hint("Pick a reference line (a wall face, an edge, a guide or a grid line), then an edge to bring flush with it")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A key as a page forwards it: pressed and let go in one frame, with
    /// its modifiers on the key only.
    fn forwarded(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
        [true, false]
            .map(|pressed| egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers,
            })
            .to_vec()
    }

    #[test]
    fn ctrl_s_typed_in_a_page_saves_and_does_not_scale() {
        let ctx = egui::Context::default();
        let mut shell = Shell {
            keys: forwarded(egui::Key::S, egui::Modifiers::COMMAND),
            ..Default::default()
        };
        let mut raw = egui::RawInput::default();
        assert!(shell.feed_keys(&mut raw), "a next frame lets go of Ctrl");
        let mut seen = (false, None);
        ctx.run_ui(raw, |ui| {
            seen = (
                ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)),
                crate::planview::start_keys(ui.ctx()),
            );
        })
        .textures_delta
        .clear();
        assert!(seen.0, "Ctrl+S reads as Save");
        assert_eq!(seen.1, None, "and starts no Scale");

        // the next frame Ctrl is up again, so S alone scales
        let mut raw = egui::RawInput::default();
        assert!(!shell.feed_keys(&mut raw));
        ctx.run_ui(raw, |ui| {
            assert!(!ui.input(|i| i.modifiers.command));
        })
        .textures_delta
        .clear();
        shell.keys = forwarded(egui::Key::S, egui::Modifiers::NONE);
        let mut raw = egui::RawInput::default();
        shell.feed_keys(&mut raw);
        let mut scale = None;
        ctx.run_ui(raw, |ui| scale = crate::planview::start_keys(ui.ctx()))
            .textures_delta
            .clear();
        assert_eq!(scale, Some((crate::modal::Mode::Scale, false)));
    }

    #[test]
    fn browser_rows_carry_marks_for_the_search() {
        let app = crate::app::test_app();
        let model = app.doc.model();
        let door = model
            .iter()
            .find(|e| matches!(&e.kind, ElementKind::Opening(o) if o.kind == creabim_core::OpeningKind::Door))
            .unwrap();
        let mark = creabim_core::marks::mark_of(model, door.id).expect("doors are marked");
        let left = app.left_state();
        let row = left["groups"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|g| g["rows"].as_array().unwrap())
            .find(|r| r["id"] == door.id.0.to_string())
            .unwrap();
        assert_eq!(row["mark"], mark.as_str());
    }
}

thread_local! {
    /// The part of the window the app draws itself, clear of the web panels
    /// (the browser, the bars), set each frame.
    static NATIVE: std::cell::Cell<Option<egui::Rect>> = const { std::cell::Cell::new(None) };
}

/// A window kept inside the part of the screen the app draws itself: web
/// panels paint over anything under them (MU28: the note editor slid
/// behind the browser).
pub(crate) fn window(title: impl Into<egui::WidgetText>) -> egui::Window<'static> {
    let w = egui::Window::new(title);
    match NATIVE.with(|n| n.get()) {
        Some(r) if r.width() > 200.0 && r.height() > 150.0 => w.constrain_to(r),
        _ => w,
    }
}

#[cfg(test)]
mod curtain_tool_tests {
    use super::*;

    /// Every object in `v` with "id" `id`, wherever it sits.
    fn find<'a>(v: &'a Value, id: &str, out: &mut Vec<&'a Value>) {
        match v {
            Value::Object(m) => {
                if m.get("id").and_then(|x| x.as_str()) == Some(id) {
                    out.push(v);
                }
                for x in m.values() {
                    find(x, id, out);
                }
            }
            Value::Array(a) => a.iter().for_each(|x| find(x, id, out)),
            _ => {}
        }
    }

    #[test]
    fn the_curtain_wall_button_is_its_own_tool() {
        let ctx = egui::Context::default();
        let mut app = crate::app::test_app();
        app.run_action(&ctx, "tool.curtain");
        assert!(app.drawing_curtain());
        let top = app.top_state(&ctx);
        let mut curtain = vec![];
        find(&top, "tool.curtain", &mut curtain);
        assert!(
            curtain.iter().any(|v| v["active"] == true),
            "Curtain wall not lit"
        );
        let wall = format!(
            "tool:{}",
            Tool::ALL.iter().position(|t| *t == Tool::Wall).unwrap()
        );
        let mut walls = vec![];
        find(&top, &wall, &mut walls);
        assert!(walls.iter().all(|v| v["active"] != true), "Wall still lit");
        let (head, _) = app.tool_strip(&ctx);
        assert_eq!(head["label"], "Curtain wall");
        // the plain Wall tool is Wall again
        app.set_tool_type(creabim_core::types::TypeCategory::Wall, None);
        assert!(!app.drawing_curtain());
        assert_eq!(app.tool_strip(&ctx).0["label"], "Wall");
    }
}
