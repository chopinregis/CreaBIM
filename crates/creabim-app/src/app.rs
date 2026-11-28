use crate::camera::OrbitCamera;
use crate::commands::Command;
use crate::editmode;
use crate::handles::{self, PaneInput, Ray};
use crate::hoverpick;
use crate::icons::{self, Icon};
use crate::modal::{Outcome, PaneMap, Space};
use crate::planview::{self, PaneStyle, PlanState, Tool};
use crate::render3d::{RenderMode, Vertex, Viewport3d};
use crate::selection::Selection;
use crate::sheetview::{self, SheetState};
use crate::sketch::{Click, SketchDraft};
use crate::snaps::{self, SnapKind, Snapper};
use creabim_core::{
    element::FREE_MESH_CLASSES, sample::sample_house, Axis, BoolOp, Cut, Document, Element,
    ElementId, ElementKind, Hatch, Material, Model, ScheduleCategory, Sheet, Transaction, Underlay,
    View, ViewKind, ViewStyle, Workplane,
};
use creabim_geom::sheet::{self as sheets, PlacedView, SheetInfo};
use creabim_geom::{pick, plan::Pen, section::Drawing, PlanItem, SceneItem};
use eframe::egui::{self, Color32, Rect, Sense, Vec2};
use eframe::egui_wgpu;
use glam::{DVec2, DVec3, Vec2 as GVec2, Vec3 as GVec3};
use std::collections::HashMap;

#[path = "actionbar.rs"]
mod actionbar;
#[path = "appmenu.rs"]
pub(crate) mod appmenu;
#[path = "areasui.rs"]
mod areasui;
#[path = "assemblies.rs"]
mod assemblies;
#[path = "cropui.rs"]
mod cropui;
#[path = "splash.rs"]
mod splash;
pub(crate) use splash::{clear_of_card, overlay_order};
#[path = "arrangeui.rs"]
mod arrangeui;
#[path = "display.rs"]
mod display;
#[path = "divideui.rs"]
mod divideui;
#[path = "docsui.rs"]
mod docsui;
#[path = "drafting.rs"]
mod drafting;
#[path = "editui.rs"]
mod editui;
#[path = "elempreview.rs"]
mod elempreview;
#[path = "gridvis.rs"]
mod gridvis;
#[path = "groundui.rs"]
pub(crate) mod groundui;
#[path = "groupsui.rs"]
mod groupsui;
#[path = "helpui.rs"]
pub(crate) mod helpui;
#[path = "importui.rs"]
mod importui;
#[path = "joinsui.rs"]
mod joinsui;
#[path = "kindpickui.rs"]
mod kindpickui;
#[path = "lamps.rs"]
mod lamps;
#[path = "levelcopy.rs"]
mod levelcopy;
#[path = "levelsui.rs"]
mod levelsui;
#[path = "librariesui.rs"]
mod librariesui;
#[path = "likeui.rs"]
mod likeui;
#[cfg(feature = "live-mcp")]
#[path = "live_wall.rs"]
mod live_wall;
#[cfg(target_os = "macos")]
#[path = "mac_menu.rs"]
mod mac_menu;
#[path = "markcard.rs"]
mod markcard;
#[path = "markplaced.rs"]
mod markplaced;
#[path = "marksui.rs"]
mod marksui;
#[path = "mastersui.rs"]
mod mastersui;
#[path = "materials.rs"]
mod materials;
#[path = "notesui.rs"]
mod notesui;
#[path = "objectsui.rs"]
mod objectsui;
#[path = "palettepage.rs"]
mod palettepage;
#[path = "patternsui.rs"]
mod patternsui;
#[path = "phasesui.rs"]
mod phasesui;
#[path = "profilesui.rs"]
mod profilesui;
#[path = "ratingsui.rs"]
mod ratingsui;
#[path = "renders.rs"]
mod renders;
#[path = "reportui.rs"]
pub(crate) mod reportui;
#[path = "selectsimilar.rs"]
mod selectsimilar;
#[path = "selecttab.rs"]
mod selecttab;
#[path = "shapestrip.rs"]
mod shapestrip;
#[path = "shell.rs"]
pub(crate) mod shell;
#[path = "solidops.rs"]
mod solidops;
#[path = "stylepages.rs"]
mod stylepages;
#[path = "stylesui.rs"]
mod stylesui;
#[path = "templates.rs"]
mod templates;
#[path = "timeui.rs"]
mod timeui;
#[path = "timeviews.rs"]
mod timeviews;
#[path = "typepreview.rs"]
mod typepreview;
#[path = "typesui.rs"]
mod typesui;
#[path = "wallsui.rs"]
mod wallsui;

/// What the 3D geometry was last built from; rebuild when any of it changes.
/// ((model revision, hidden revision), selection, selected face, drawing
/// shown in 3D, active view)
/// How a workplane is being picked in the 3D view.
/// Split while it runs (MU39): what it cuts, the first point of a line
/// being drawn, then the cutting line.
#[derive(Clone, Debug)]
struct SplitRun {
    targets: Vec<ElementId>,
    a: Option<DVec2>,
    line: Option<creabim_core::split::Line>,
}

#[derive(Clone, Debug, PartialEq)]
enum PlanePick {
    /// Click a face.
    Face,
    /// Click three points; these are the ones so far.
    Points(Vec<DVec3>),
}

type GeometryKey = (
    (u64, u64),
    Selection,
    Option<usize>,
    bool,
    Option<ElementId>,
    // the crop boxes the 3D window shows: they build nothing, so the
    // scene's revision misses a face dragged; the scene is cut with those
    // switched on, and their frames are drawn with it (MU143)
    Vec<creabim_core::CropBox>,
);

/// Where the 2D plan is shown relative to the 3D view.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PlanPlacement {
    /// Side by side with the 3D view.
    Docked,
    /// In its own OS window, e.g. on a second monitor.
    Window,
    /// Inset inside the 3D workspace.
    Overlay,
    Hidden,
}

pub struct CreaBimApp {
    doc: Document,
    #[cfg(feature = "live-mcp")]
    live_bridge: Option<crate::live_mcp::Bridge>,
    #[cfg(feature = "live-mcp")]
    live_document_id: String,
    /// Outside the docked workspace, remember which modelling pane was activated.
    #[cfg(feature = "live-mcp")]
    live_plan_active: bool,
    selection: Selection,
    /// Selected face when a free mesh is selected (mesh edit mode).
    face: Option<usize>,
    /// What the 2D pane shows: a view (plan, section, elevation, 3D) or a sheet.
    active_view: Option<ElementId>,
    sheet: SheetState,
    /// Each area's own sheet state (MU29: two areas on sheets shared one,
    /// so one cancelled the other's drags and their cameras moved together).
    sheets: HashMap<usize, SheetState>,
    /// The area being drawn now.
    pub(super) area_now: usize,
    /// Generated drawings by view, with the model revision they were made at.
    drawings: HashMap<ElementId, (u64, Drawing)>,
    /// Drawings and the 3D scene being built off the UI thread.
    builds: crate::builds::Builds,
    /// While a pointer edit runs: the elements it works on, and the 3D
    /// scene of the model as it was before it changed anything. A pull or
    /// a move of a shape that stands alone builds that shape again into
    /// this scene, and not the whole building on a worker (MU45).
    live_base: Option<(Vec<ElementId>, std::sync::Arc<Vec<creabim_geom::SceneItem>>)>,
    /// What the 3D buffers of everything but the shape that follows the
    /// pointer were built for: while it holds, they stay on the card and
    /// a step sends the moving shape's only.
    static_3d: Option<String>,
    /// The shapes a pointer edit is working on or has just left, with
    /// the 3D snaps and the model they were seen with, and whether the
    /// edit still runs.
    snaps_touch: Option<(Vec<ElementId>, u64, u64, bool)>,
    /// The view the pointer was last in: the one that shows the shape
    /// strip (it showed in every view at once, MU46).
    strip_pane: Option<egui::Id>,
    /// Each view's looks on sheets, with the model revision they are for.
    looks: HashMap<ElementId, (u64, std::sync::Arc<creabim_geom::looks::Looks>)>,
    /// The same for views seen in X-ray, which their sheets don't follow.
    xray_drawings: HashMap<ElementId, (u64, Drawing)>,
    /// Drawings of areas standing at a phase of their own, by view, X-ray
    /// and phase (MU133).
    phased_drawings: HashMap<(ElementId, bool, u32), (u64, Drawing)>,
    /// The theme the 3D view's wires were last drawn for (MU31).
    dark_3d: bool,
    /// The Divide window (MU34 B2).
    divide: divideui::DivideUi,
    /// The profile library window (MU27 block 5).
    profiles: profilesui::ProfilesUi,
    masters: mastersui::MastersUi,
    log_ui: reportui::LogUi,
    /// Project time: the book kept beside the project, and its clock.
    time: timeui::TimeUi,
    /// Manage › Libraries (the experiment of 2026-11-18).
    libs: librariesui::LibUi,
    /// Report a problem asked for: the themed window is open; whether to
    /// put the project in.
    report_ask: Option<bool>,
    /// Where the Report a problem card is, and since when.
    report_phase: reportui::ReportPhase,
    report_since: Option<std::time::Instant>,
    /// Closing with unsaved changes: Save, Don't save or Cancel is asked.
    close_ask: bool,
    /// Where the Save the changes window is: asking, saving, saved, failed.
    close_state: appmenu::CloseState,
    /// The close was settled (saved, or the changes let go): let it through.
    close_ok: bool,
    save_as_dialog: Option<appmenu::SaveAsDialog>,
    /// The pointer left the drawing area (onto a web pane, or out of the
    /// window) and hasn't come back yet.
    pointer_away: bool,
    /// When the pointer last moved over the drawing area, in seconds.
    pub(crate) pointer_seen: f64,
    /// What the area in front showed last frame: a Masters or Profile view
    /// coming forward brings its own Properties tab (MU40).
    prop_front: Option<crate::areas::Shows>,
    camera: OrbitCamera,
    cube: crate::viewcube::ViewCube,
    /// Aspect ratio of the 3D view as last drawn, for framing.
    aspect: f32,
    viewport: Option<Viewport3d>,
    geometry_key: Option<GeometryKey>,
    /// What the 3D grid lines were last built from (MU74: grids change
    /// no solid, so the scene's revision missed a stretched grid).
    grid_lines_key: u64,
    /// The rendered modes' images, rendered while the view is still.
    rendered: crate::rendered::Rendered,
    /// The project's kept renders (see `renders.rs`).
    renders: renders::Renders,
    /// How far Inset draws a face inside its edges.
    inset_by: f64,
    /// Split while it runs: what it cuts, the first point of a line being
    /// drawn, the cutting line once there is one (MU39).
    split: Option<SplitRun>,
    /// The selection when a point was first asked for: kept while points
    /// are picked, so a click in a view never changes it (MU39).
    pick_selection: Option<Selection>,
    /// Bumped at each rebuild of the model's 3D buffers.
    geometry_gen: u64,
    /// What the selection highlight was last built for.
    selection_key: Option<(u64, Selection, Option<usize>)>,
    /// Drawings and the 3D view on dark paper this frame (A4-3).
    paper_is_dark: bool,
    /// The Properties tab showing (MU19); it stays when the selection
    /// changes, as Blender's does.
    prop_tab: PropTab,
    /// The tool and whether something was selected, last frame: a change
    /// can pick the tab (MU19).
    prop_seen: (Tool, bool),
    /// Sheets as they print, on white, whatever the paper (MU21).
    print_preview: bool,
    /// The Export sheets dialog (MU22), and its choices this session.
    sheets_export_open: bool,
    sheets_export: SheetsExport,
    /// A reference to take away, asked from its Properties (MU23).
    remove_ref: Option<ElementId>,
    /// Copy to view…: the reference, and the view its copy goes to (MU38).
    copy_ref: Option<(ElementId, ElementId)>,
    /// Shift+A's Add menu, open, with what is typed in it (MU24).
    add_menu: Option<String>,
    scene: Vec<SceneItem>,
    plan: PlanState,
    placement: PlanPlacement,
    plan_in_3d: bool,
    /// The view areas and the workspace (brief 02, 2.6).
    areas: areasui::AreaState,
    /// The focused area's header controls that didn't fit, for its More menu.
    areas_over: Vec<serde_json::Value>,
    /// A handle or element being dragged in the 3D view.
    handle_drag: Option<handles::Drag>,
    /// B pressed in 3D: the next drag draws a selection box; and where it began.
    box_armed: bool,
    box_3d: Option<egui::Pos2>,
    /// The outline of the picked element in a view that hides it, as the
    /// view would draw it: (view, element, the view's drawing revision),
    /// its lines. Built once a pick, not every frame.
    #[allow(clippy::type_complexity)]
    hidden_outline: Option<(ElementId, ElementId, u64, Vec<(DVec2, DVec2)>)>,
    /// Snaps for the 3D view: built from the scene without the listed
    /// elements, at a model revision.
    snaps_3d: Option<(u64, Vec<ElementId>, Snapper)>,
    /// First click of a two-click tool in the 3D view, on a level.
    pending_3d: Option<(DVec2, ElementId)>,
    /// A beam drawn on a face (MU40): its first point, the face's plane
    /// (a point and its outward normal).
    beam_face: Option<(DVec3, DVec3, DVec3)>,
    /// Points rested on while drawing in 3D, that the next point lines up
    /// with, and the one being rested on now (MU24: as in plans).
    rested_3d: (Vec<DVec3>, Option<(DVec3, f64)>),
    /// Where the 3D right-click menu was opened, and the face there.
    menu_at_3d: Option<DVec3>,
    menu_plane_3d: Option<Workplane>,
    /// A workplane set on a face, through three points or off a level, with
    /// its name. The 3D sketch and guide tools draw on it instead of the
    /// level, and the grid lies on it.
    workplane: Option<(Workplane, String)>,
    /// Picking a workplane in the 3D view.
    plane_pick: Option<PlanePick>,
    /// The modifier card last worked in: its guides show in the views,
    /// and Pick sets its centre (MU39).
    mod_focus: Option<(ElementId, usize)>,
    /// First point of a guide being drawn on the workplane.
    guide_from: Option<DVec3>,
    extrude_distance: f64,
    path: String,
    /// The model's revision when it was last opened or saved: anything
    /// else is unsaved (A7-8).
    saved_revision: Option<u64>,
    /// The undo history when it was last opened or saved: how many steps
    /// and the last one's id, to count the changes since (Save the changes).
    saved_mark: (usize, Option<creabim_core::document::StepId>),
    /// The window title last set, so it is only sent when it changes.
    title: String,
    /// The status and tool last written to the activity log.
    logged: (String, Option<Tool>),
    status: String,
    /// Linked files (Blender...) and when they were last read.
    watch: crate::files::Watch,
    /// Reload linked files as soon as they are saved.
    live_reload: bool,
    /// Elements hidden from the 3D view and the 2D panes (not from sheets
    /// or exports), and a count of changes to them.
    hidden: std::collections::HashSet<ElementId>,
    hidden_rev: u64,
    /// The phase the 3D view stands at (see `phasesui`), for the session;
    /// none is the project's last.
    phase3d: Option<u32>,
    /// The options the 3D view shows, where not each question's pick.
    options3d: Vec<u32>,
    /// The option being worked in (see `phasesui`): what is drawn is its
    /// alone. None is what every option shares.
    work_option: Option<u32>,
    /// The browser's Groups section: its offer, picks and eyes.
    groups_ui: groupsui::GroupsUi,
    /// Options the 3D view ghosts.
    ghost3d: Vec<u32>,
    /// Whether shared parts are held while an option is worked in.
    hold_shared: bool,
    /// A change to existing work from a later phase alters it there (the
    /// old comes down, the changed is built) instead of correcting the
    /// survey.
    alter_existing: bool,
    /// What is picked in the Phases tab's Taken down list. Its own pick,
    /// not the selection: selecting would turn Properties to the Element
    /// tab and take the list away (seen in the app).
    down_pick: Vec<ElementId>,
    /// What the 3D view leaves out (categories, grid groups, elements).
    /// Its categories are kept in the settings, the rest for the session.
    vis3d: creabim_core::ViewHidden,
    /// What Ctrl+C copied, for pasting when the system clipboard has none.
    clipboard: Vec<Element>,
    recent: crate::files::Recent,
    /// The web interface round the views.
    shell: shell::Shell,
    #[cfg(target_os = "macos")]
    mac_menu: Option<mac_menu::MacMenu>,
    /// The material browser.
    mat: materials::MaterialsUi,
    /// The Patterns window.
    pat: patternsui::PatternsUi,
    /// The assembly editor.
    asm: assemblies::AssembliesUi,
    /// The Types window.
    types: typesui::TypesUi,
    /// A type opened on its own, and the type Show in model lights.
    type_preview: typepreview::TypePreview,
    /// The Catalogue window and the user's own catalogue.
    catalogue: objectsui::CatalogueUi,
    /// The Levels manager and the Project units dialog.
    levels: levelsui::LevelsUi,
    marks: marksui::MarksUi,
    gstyles: stylesui::StylesUi,
    /// The Copy to levels and Move to level windows.
    level_copy: levelcopy::LevelCopyUi,
    /// What the last IFC import brought across, and what it couldn't.
    import_report: Option<importui::ImportReport>,
    /// The Import survey window.
    ground: groundui::GroundUi,
    /// The export or import card on screen (Mark Moments REV02).
    mark: Option<markcard::MarkCard>,
    /// Enter or Esc pressed for the card, taken from this frame's input.
    mark_key: Option<&'static str>,
    /// The annotation styles and sheet templates windows.
    drafting: drafting::DraftingUi,
    /// Orbit about the selection rather than the middle of the view.
    orbit_selection: bool,
    /// Where the 3D view was drawn last frame, for its navigation buttons.
    rect_3d: Option<Rect>,
    /// Where the 2D pane was drawn last frame, and whether the pointer was
    /// last over it rather than the 3D view: what Alt+Z turns X-ray on in.
    rect_2d: Option<Rect>,
    last_in_2d: bool,
    /// What the status bar said after the last export, and the folder it
    /// went to: while the message shows, the bar offers to open the folder.
    last_export: Option<String>,
    /// Settings picked up to apply to others (Ctrl+Shift+C / V).
    like: likeui::Likeness,
    /// What a box selection took, while it is still the selection: the
    /// status bar offers Keep only… for it.
    boxed: Option<Vec<ElementId>>,
    /// What the Select tab's By its values would pick, lit in the views
    /// as it is ticked; set while the tab draws it, taken each frame.
    match_preview: Vec<ElementId>,
    /// The Reach the strip's next solid operation takes (wave 3).
    solid_reach: creabim_core::Reach,
    /// The selection last checked for why the strip can't add or cut.
    solid_seen: Vec<ElementId>,
    last_export_dir: Option<std::path::PathBuf>,
    /// The crop box whose elements clicks in the 3D view are picking.
    crop_pick: Option<ElementId>,
    /// The reading note whose object the next element selected becomes
    /// (its Object row's pipette, MU135).
    note_pick: Option<ElementId>,
    /// The element outside a crop box the status bar last spoke of.
    outside_said: Option<ElementId>,
    /// The 3D extents the crop ghosts were measured from: the model's
    /// revision, the elements asked for, and their boxes.
    #[allow(clippy::type_complexity)]
    outside_bounds: Option<(
        u64,
        Vec<ElementId>,
        std::collections::HashMap<ElementId, (DVec3, DVec3)>,
    )>,
    /// The Settings window is open.
    settings_open: bool,
    /// A note open in its own big editor window (MU26).
    note_editor: Option<ElementId>,
    /// The notes section open in the Notes area ("general", "todos").
    notes_section: String,
    /// The sheet note whose section the Notes area last opened.
    notes_synced: Option<ElementId>,
    /// The Notes area popped out into its own window.
    notes_popped: bool,
    /// What a notes toolbar button puts in, for the editor it is above
    /// (the area's header is drawn before its body).
    notes_insert: Option<(notesui::NotesAt, String, bool)>,
    /// The section being renamed, and where.
    notes_renaming: Option<(String, notesui::NotesAt)>,
    /// Where the notes show as they print rather than as Markdown.
    notes_preview: std::collections::HashSet<notesui::NotesAt>,
    /// The blocks ticked in the Placed on list.
    notes_ticked: std::collections::HashSet<ElementId>,
    /// A long note's Continue or Go to, asked in Properties.
    notes_long: Option<notesui::LongAct>,
    /// A schedule export asked from its header ("csv" or "xlsx").
    schedule_export: Option<String>,
    /// The model revision last checked for overlapping walls.
    overlap_seen: u64,
    /// Guides to copy from a sheet to every other one, next frame.
    copy_guides: Option<(ElementId, Vec<(bool, f64)>)>,
    /// Export > IFC: the window asking how walls and slabs go out (MU20).
    ifc_dialog: bool,
    /// The New project window, with the classic panels.
    new_project_classic: bool,
    /// The splash screen over the app (Mockup_Splash_REV00, A).
    splash_open: bool,
}

/// A pointer moving over the drawing area this recently (seconds) can't be
/// over a bar: a tooltip a bar asks for then is a stale one.
pub(crate) const TIP_PAUSE: f64 = 0.3;

/// How much of the accent a selected element takes in 3D (A7-2).
const SELECTION_TINT: f32 = 0.35;

/// The window kept unseen while it starts (Windows): it is shown by the
/// system as soon as its first frame is painted, small, and maximised a
/// frame later. Cloaked, it is on screen for the system but not drawn
/// there, so it can take its size and paint a few frames first.
mod start_cloak {
    /// Frames painted at full size before the window is seen.
    pub const FRAMES: u32 = 2;
    /// The count once it has been shown.
    pub const DONE: u32 = u32::MAX;

    #[cfg(windows)]
    pub fn set(frame: &impl raw_window_handle::HasWindowHandle, on: bool) {
        use raw_window_handle::RawWindowHandle;
        #[link(name = "dwmapi")]
        extern "system" {
            fn DwmSetWindowAttribute(
                hwnd: isize,
                attr: u32,
                value: *const core::ffi::c_void,
                size: u32,
            ) -> i32;
        }
        const DWMWA_CLOAK: u32 = 13;
        let Ok(handle) = frame.window_handle() else {
            return;
        };
        if let RawWindowHandle::Win32(w) = handle.as_raw() {
            let value: i32 = on as i32;
            // (a failure leaves it seen, as before)
            unsafe {
                DwmSetWindowAttribute(w.hwnd.get(), DWMWA_CLOAK, (&value as *const i32).cast(), 4);
            }
        }
    }

    #[cfg(not(windows))]
    pub fn set(_frame: &impl raw_window_handle::HasWindowHandle, _on: bool) {}
}

/// The interface scale the app starts at (Settings › Interface scale), set
/// before its first frame. It is set as the zoom itself:
/// `set_zoom_factor` treats it as a change, and to avoid a jump egui
/// rescales the screen it had last frame. Before the first frame that is
/// its 10 000-point placeholder, so the first frame was laid out on a
/// screen 12 500 points square at 80 %. The 3D view then asked the GPU for
/// a target 12 319 pixels tall, past its 8 192 limit, and the app crashed
/// at start-up at every scale but 100 %.
pub(crate) fn start_zoom(ctx: &egui::Context, scale: u16) {
    ctx.options_mut(|o| o.zoom_factor = f32::from(scale) / 100.0);
}

#[cfg(test)]
mod start_zoom_tests {
    /// Every interface scale starts on the window's own size, not on
    /// egui's placeholder screen.
    #[test]
    fn every_interface_scale_starts_on_the_windows_own_size() {
        for scale in [80u16, 90, 110, 125, 150] {
            let ctx = eframe::egui::Context::default();
            super::start_zoom(&ctx, scale);
            let z = f32::from(scale) / 100.0;
            // the owner's window, 1938 × 1038 pixels on a screen at 125 %
            let screen = eframe::egui::Rect::from_min_size(
                eframe::egui::Pos2::ZERO,
                eframe::egui::vec2(1938.0 / (1.25 * z), 1038.0 / (1.25 * z)),
            );
            let raw = eframe::egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |_| {});
            out.textures_delta.clear();
            let got = ctx.content_rect();
            assert!(
                (got.size() - screen.size()).length() < 1.0,
                "at {scale} %, the first frame is laid out on {got:?}, the window is {screen:?}"
            );
            assert!((ctx.zoom_factor() - z).abs() < 1e-6);
        }
    }
}

impl CreaBimApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings = crate::files::Settings::load();
        // The web panels and native UI share one scale preference.
        cc.egui_ctx
            .options_mut(|options| options.zoom_with_keyboard = false);
        start_zoom(&cc.egui_ctx, settings.interface_scale);
        let theme = crate::theme::Theme::from_id(&settings.theme).unwrap_or_default();
        crate::handles::set_grip_style(settings.grips);
        crate::modal::set_bar_by_pointer(settings.edit_bar == "pointer");
        crate::handles::set_edit_display(settings.edit_edge_width, settings.edit_vertex_size);
        shell::style(&cc.egui_ctx, theme);
        // maximised before its first frame is shown, so it doesn't open
        // small and jump (MU103); asked again on that frame (see `ui`)
        cc.egui_ctx
            .send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        // unseen from before it is first shown (see `ui`): cloaked on its
        // first frame only, its small frame showed for a moment as it
        // opened
        start_cloak::set(cc, true);
        crate::annotate::install_fonts(&cc.egui_ctx);
        let viewport = cc
            .wgpu_render_state
            .as_ref()
            .map(|rs| Viewport3d::new(&rs.device));
        let mut app = Self::blank(viewport);
        crate::actlog::start();
        // the graphics card, for problem reports (MU40)
        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            let i = rs.adapter.get_info();
            let gpu = format!(
                "{} ({:?}, driver {} {})",
                i.name, i.backend, i.driver, i.driver_info
            );
            crate::actlog::line(format!("graphics: {gpu}"));
            reportui::set_gpu(gpu);
        }
        crate::actlog::line(format!("system: {}", reportui::os_version()));
        app.builds.set_context(cc.egui_ctx.clone());
        app.shell.theme = theme;
        app.vis3d.categories = settings.hidden_3d.clone();
        // grids in 3D: shown only when both the old switches agreed; from
        // now on they are one (MU69)
        let grids_on = settings.grids_3d
            && !app
                .vis3d
                .hides_category(creabim_core::visibility::Category::Grids);
        app.vis3d
            .set_category(creabim_core::visibility::Category::Grids, !grids_on);
        // grid snap always starts off; its spacing and the distance marks
        // are kept
        app.plan.snap.increment = false;
        app.plan.temp_dims_off = settings.temp_dims_off;
        app.plan.snap.marks = settings.distance_marks;
        if settings.snap_spacing_um > 0 {
            app.plan.snap.step_um = settings.snap_spacing_um;
        }
        app.areas.workspace = settings.workspace.clone();
        if let Some(root) = settings.areas.clone() {
            app.areas.root = root;
            app.areas.phases = settings.area_phases.clone();
        }
        // the splash at start, as Blender's (a file to open skips it)
        app.splash_open = settings.show_splash || !settings.setup_done;
        // the 3D view's lens and straight verticals, as they were left
        if settings.camera_lens >= 8.0 {
            app.camera.set_lens_mm(settings.camera_lens);
        }
        app.camera.straight = settings.camera_straight;
        app.shell.settings = settings;
        // behind the splash, this version's starter house (owner, MU125:
        // Maple Lane House is opened only when asked for). It was started
        // on a thread of its own before the window, so it is ready by now
        app.load_model(appmenu::opening_house());
        app.status = STARTER_LOADED.into();
        // `creabim-app house.creabim` opens that file instead
        let surveys = |a: &String| groundui::is_survey_file(a);
        if let Some(path) = launch_file(std::env::args().skip(1).filter(|a| !surveys(a))) {
            app.open_path(&path);
            app.splash_open = !app.shell.settings.setup_done;
        }
        // and survey or map files after it: Import survey, or a reference
        for path in std::env::args().skip(1).filter(surveys) {
            app.open_survey_file(&path);
        }
        #[cfg(feature = "live-mcp")]
        match crate::live_mcp::Bridge::start(cc.egui_ctx.clone()) {
            Ok(bridge) => app.live_bridge = bridge,
            Err(error) => {
                app.status = format!("MCP bridge unavailable: {error}");
                eprintln!("{}", app.status);
            }
        }
        #[cfg(target_os = "macos")]
        app.refresh_mac_menu(&cc.egui_ctx);
        app
    }

    pub(super) fn uses_native_menu(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.mac_menu.is_some()
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    /// The app round an empty document, with default settings and no
    /// window: [`Self::new`] fills it in, and tests use it as it is.
    fn blank(viewport: Option<Viewport3d>) -> Self {
        Self {
            doc: Document::new(),
            #[cfg(feature = "live-mcp")]
            live_bridge: None,
            #[cfg(feature = "live-mcp")]
            live_document_id: ElementId::new().0.to_string(),
            #[cfg(feature = "live-mcp")]
            live_plan_active: false,
            selection: Selection::default(),
            face: None,
            active_view: None,
            sheet: SheetState::default(),
            sheets: HashMap::new(),
            area_now: 0,
            drawings: HashMap::new(),
            builds: Default::default(),
            live_base: None,
            static_3d: None,
            snaps_touch: None,
            strip_pane: None,
            looks: HashMap::new(),
            xray_drawings: HashMap::new(),
            phased_drawings: HashMap::new(),
            dark_3d: true,
            divide: Default::default(),
            profiles: Default::default(),
            masters: Default::default(),
            log_ui: Default::default(),
            time: Default::default(),
            libs: Default::default(),
            report_ask: None,
            report_phase: Default::default(),
            report_since: None,
            close_ask: false,
            close_state: Default::default(),
            close_ok: false,
            save_as_dialog: None,
            pointer_away: true,
            pointer_seen: 0.0,
            prop_front: None,
            camera: OrbitCamera::default(),
            cube: Default::default(),
            aspect: 0.65,
            viewport,
            geometry_key: None,
            grid_lines_key: 0,
            geometry_gen: 0,
            rendered: Default::default(),
            renders: Default::default(),
            pick_selection: None,
            inset_by: 0.1,
            split: None,
            selection_key: None,
            paper_is_dark: false,
            prop_tab: PropTab::Element,
            prop_seen: (Tool::Select, false),
            print_preview: false,
            sheets_export_open: false,
            sheets_export: SheetsExport::default(),
            remove_ref: None,
            copy_ref: None,
            add_menu: None,
            scene: vec![],
            plan: PlanState::default(),
            placement: PlanPlacement::Docked,
            plan_in_3d: false,
            areas: Default::default(),
            areas_over: vec![],
            handle_drag: None,
            box_armed: false,
            box_3d: None,
            hidden_outline: None,
            snaps_3d: None,
            pending_3d: None,
            beam_face: None,
            rested_3d: (vec![], None),
            menu_at_3d: None,
            menu_plane_3d: None,
            guide_from: None,
            workplane: None,
            plane_pick: None,
            mod_focus: None,
            extrude_distance: 1.0,
            path: "project.creabim".into(),
            saved_revision: None,
            saved_mark: (0, None),
            title: String::new(),
            logged: (String::new(), None),
            watch: Default::default(),
            live_reload: true,
            hidden: Default::default(),
            hidden_rev: 0,
            phase3d: None,
            options3d: vec![],
            work_option: None,
            groups_ui: Default::default(),
            ghost3d: vec![],
            hold_shared: true,
            down_pick: vec![],
            alter_existing: false,
            vis3d: Default::default(),
            clipboard: vec![],
            recent: crate::files::Recent::load(),
            shell: Default::default(),
            #[cfg(target_os = "macos")]
            mac_menu: None,
            mat: Default::default(),
            asm: Default::default(),
            types: Default::default(),
            type_preview: Default::default(),
            catalogue: Default::default(),
            levels: Default::default(),
            settings_open: false,
            note_editor: None,
            notes_section: "general".into(),
            notes_synced: None,
            notes_popped: false,
            notes_insert: None,
            notes_renaming: None,
            notes_preview: Default::default(),
            notes_ticked: Default::default(),
            notes_long: None,
            copy_guides: None,
            overlap_seen: 0,
            schedule_export: None,
            ifc_dialog: false,
            new_project_classic: false,
            splash_open: false,
            marks: Default::default(),
            gstyles: Default::default(),
            pat: Default::default(),
            level_copy: Default::default(),
            import_report: None,
            ground: Default::default(),
            mark: None,
            mark_key: None,
            drafting: Default::default(),
            orbit_selection: false,
            rect_3d: None,
            rect_2d: None,
            last_in_2d: false,
            last_export: None,
            like: Default::default(),
            boxed: None,
            match_preview: vec![],
            solid_reach: Default::default(),
            solid_seen: vec![],
            last_export_dir: None,
            crop_pick: None,
            note_pick: None,
            outside_said: None,
            outside_bounds: None,
            status: "Loaded the sample house. Orbit with the middle button (or Alt + left drag), pan with right drag, zoom with the wheel; a left drag selects with a box.".into(),
        }
    }

    fn load_model(&mut self, model: Model) {
        if self.save_as_dialog.take().is_some() && self.close_ask {
            self.close_ask = false;
            self.close_state = Default::default();
            self.hide_close_dialog();
        }
        self.doc = Document::from_model(with_plan_views(model));
        // a new model is unsaved until it is written or was opened from a
        // file (opening marks it saved after this): the last file's saved
        // mark, an empty history, would match the new one's
        self.saved_revision = None;
        #[cfg(feature = "live-mcp")]
        {
            self.live_document_id = ElementId::new().0.to_string();
        }
        self.selection.clear();
        self.hidden.clear();
        self.hidden_rev += 1;
        self.face = None;
        self.crop_pick = None;
        self.note_pick = None;
        self.active_view = None;
        self.sheet = SheetState::default();
        self.sheets.clear();
        self.drawings.clear();
        self.looks.clear();
        self.xray_drawings.clear();
        self.phased_drawings.clear();
        self.plan.xray.clear();
        self.plan.hover = None;
        self.geometry_key = None;
        self.drop_stale_selection();
        self.frame_all();
        // a project in feet and inches starts its doors and windows at its
        // own sizes, not metric ones
        let imperial = self.doc.model().units().is_imperial();
        self.plan.members.suit_units(imperial);
    }

    /// Tag each ceiling's height in plan `view` once it looks up, as a
    /// step of its own (MU142: turned to look up, it had no height tags).
    pub(crate) fn tag_ceilings_of(&mut self, view: ElementId) {
        let model = self.doc.model();
        let level = match model.get(view).map(|e| &e.kind) {
            Some(ElementKind::View(v)) if v.ceiling_plan => match v.kind {
                ViewKind::Plan { level, .. } => level,
                _ => return,
            },
            _ => return,
        };
        let tags = creabim_core::room::ceiling_tags_in(model, view, level);
        if tags.is_empty() {
            return;
        }
        let tx = tags
            .into_iter()
            .fold(Transaction::new("Tag ceiling heights"), |tx, t| {
                tx.insert(t)
            });
        let _ = self.doc.commit(tx);
    }

    /// Open a project file, or import and link any other file.
    fn open_path(&mut self, path: &str) {
        let lower = path.to_ascii_lowercase();
        // material cards and glTF files bring materials into this project
        if materials::is_material_file(path) {
            self.import_materials(&[path.to_string()]);
            return;
        }
        if lower.ends_with(".ifc") || lower.ends_with(".ifcx") {
            // read off the window's thread, with its card
            if self.mark_cards() {
                self.start_import(path);
                return;
            }
            // timed here too: a file named on the command line is read
            // before the cards are up, then shown in one as it ended
            let started = std::time::Instant::now();
            let r = std::fs::read_to_string(path)
                .map_err(|e| e.to_string())
                .and_then(|t| {
                    if lower.ends_with(".ifcx") {
                        creabim_ifc::import_ifcx(&t)
                    } else {
                        creabim_ifc::import_ifc(&t)
                    }
                    .map_err(|e| e.to_string())
                });
            match r {
                Ok(imported) => {
                    // apply_import writes each note to the activity log too
                    let secs = started.elapsed().as_secs_f32();
                    self.apply_import(path, imported);
                    if let Some(rep) = self.import_report.as_mut().filter(|r| r.secs.is_none()) {
                        rep.secs = Some(secs);
                        crate::actlog::line(format!("IFC import: opened in {secs:.1} s"));
                    }
                }
                Err(e) => self.status = format!("Couldn't open {path}: {e}"),
            }
            return;
        }
        if !crate::files::is_project(path) {
            self.link_file(path);
            return;
        }
        match creabim_store::load(path) {
            Ok(mut doc) => {
                // pads, pits, spot heights and contours an earlier file
                // kept inside a terrain become elements of their own: one
                // step, which undoes
                let pads = match creabim_core::site::earlier_pads(doc.model()) {
                    Some(tx) => {
                        let n = tx.ops.len();
                        doc.commit(tx).is_ok().then_some(n)
                    }
                    None => None,
                };
                // wells an earlier version cut round a spiral, a curve or
                // winders with an outline that crossed itself: cut again,
                // one step, which undoes (MU102)
                let wells = creabim_core::stairwell::mend_wells(doc.model()).and_then(|tx| {
                    let n = tx.ops.len();
                    doc.commit(tx).is_ok().then_some(n)
                });
                // ceiling heights an earlier version wrote as plain text
                // become tags in the ceiling plans: one step, which undoes
                // (MU142)
                let ceilings = creabim_core::room::tag_ceilings(doc.model()).and_then(|tx| {
                    let n = tx.ops.len();
                    doc.commit(tx).is_ok().then_some(n)
                });
                let model = doc.model().clone();
                self.load_model(model.clone());
                // keep the saved undo history when no views had to be added
                if self.doc.model() == &model {
                    self.doc = doc;
                }
                self.watch = Default::default();
                self.recent.add(path);
                self.path = path.to_string();
                self.mark_saved();
                self.status = format!("Opened {path}");
                if let Some(n) = wells {
                    self.status = format!(
                        "Opened {path}. {n} stair well{} from an earlier version had an outline that crossed itself: cut again from the stair. Ctrl+Z puts the old one back.",
                        if n == 1 { "" } else { "s" }
                    );
                }
                if pads.is_some() {
                    self.status = format!(
                        "Opened {path}. Its pads, pits, spot levels and contours are now things of their own on the ground: click one to move or change it."
                    );
                }
                if let Some(n) = ceilings {
                    self.status = format!(
                        "Opened {path}. Its ceiling heights are now tags ({n}): move them, give them a leader or a style as any tag. Ctrl+Z puts the plain text back."
                    );
                }
                // copies a Duplicate left on their originals (MU24)
                let stacked = creabim_core::checks::stacked_copies(self.doc.model()).len();
                if stacked > 0 {
                    self.status = format!(
                        "Opened {path}. {stacked} element{} sit exactly on a copy of themselves: Manage › Copies on originals removes them.",
                        if stacked == 1 { "" } else { "s" }
                    );
                }
            }
            Err(e) => self.status = format!("Couldn't open {path}: {e}"),
        }
    }

    /// Has the model changed since it was opened or saved?
    ///
    /// Every change to the model is a step in its undo history, and a step
    /// changed in place (a value dragged) takes a new id. So with the
    /// history where it was when saved, the model is as saved, whatever its
    /// revision: a tool's live edit taken back (Scale, Move or Rotate
    /// cancelled) gives the model back with a new revision, and the title
    /// kept its unsaved mark (MU143). Undone back to the save, it is saved.
    pub(crate) fn unsaved(&self) -> bool {
        let Some(saved) = self.saved_revision else {
            return true;
        };
        saved != self.doc.model().revision() && self.saved_mark != self.history_mark()
    }

    /// Where the undo history stands: how many steps, and the last one.
    fn history_mark(&self) -> (usize, Option<creabim_core::document::StepId>) {
        let steps = self.doc.undo_steps();
        (steps.len(), steps.last().map(|s| s.id))
    }

    /// The model as it is now counts as saved.
    fn mark_saved(&mut self) {
        self.saved_revision = Some(self.doc.model().revision());
        self.saved_mark = self.history_mark();
    }

    /// How many undo steps were made since it was last opened or saved
    /// (at least one while it is unsaved).
    pub(crate) fn changes_since_saved(&self) -> usize {
        let steps = self.doc.undo_steps();
        let (len, last) = self.saved_mark;
        let n = match last {
            // the step saved last is still in the history: what follows it
            Some(id) => match steps.iter().rposition(|s| s.id == id) {
                Some(at) => steps.len() - at - 1,
                None => steps.len().abs_diff(len),
            },
            None => steps.len().abs_diff(len),
        };
        if self.unsaved() {
            n.max(1)
        } else {
            n
        }
    }

    fn save(&mut self) {
        let r = self.save_file();
        self.report(r, format!("Saved {}", self.path));
    }

    /// Write the project to its file, saying why when it can't.
    pub(crate) fn save_file(&mut self) -> Result<(), String> {
        if self.path.to_ascii_lowercase().ends_with(".json") {
            std::fs::write(&self.path, self.doc.model().to_json()).map_err(|e| e.to_string())?;
            self.mark_saved();
            return Ok(());
        }
        if !self.path.to_ascii_lowercase().ends_with(".creabim") {
            self.path = with_extension(&self.path, "creabim");
        }
        creabim_store::save(&self.path, &self.doc).map_err(|e| e.to_string())?;
        let path = self.path.clone();
        self.recent.add(&path);
        self.mark_saved();
        // the project's time, beside it (never inside the file)
        self.time_frame_path_sync();
        self.time_store();
        Ok(())
    }

    fn open_dialog(&mut self) {
        if let Some(path) = crate::files::pick_open() {
            self.open_path(&path);
        }
    }

    /// Bring a file's objects in as linked elements.
    fn link_file(&mut self, path: &str) {
        let full = std::fs::canonicalize(path)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| path.to_string());
        if crate::files::linked_files(self.doc.model()).contains(&full) {
            self.reload(&full);
            return;
        }
        match crate::files::read_incoming(&full) {
            Ok(incoming) => {
                let (tx, report) = creabim_exchange::sync(self.doc.model(), &full, &incoming);
                let r = self.doc.commit(tx);
                self.watch.mark(&full);
                self.drawings.clear();
                self.frame_all();
                self.report(
                    r,
                    format!(
                        "Linked {}: {}. Pick an object and use Assign as to make it a wall, slab, door or window.",
                        file_label(&full),
                        report.summary()
                    ),
                );
            }
            Err(e) => self.status = format!("Couldn't read {path}: {e}"),
        }
    }

    /// Read a linked file again and update the elements it made.
    fn reload(&mut self, file: &str) {
        match crate::files::read_incoming(file) {
            Ok(incoming) => {
                let (tx, report) = creabim_exchange::sync(self.doc.model(), file, &incoming);
                self.watch.mark(file);
                if tx.ops.is_empty() {
                    self.status = format!("{} is up to date.", file_label(file));
                    return;
                }
                let r = self.doc.commit(tx);
                self.drop_stale_selection();
                self.report(
                    r,
                    format!("Reloaded {}: {}.", file_label(file), report.summary()),
                );
            }
            Err(e) => self.status = format!("Couldn't reload {file}: {e}"),
        }
    }

    /// The Data (IFC) tab (Properties order mockup §3c): what an IFC file
    /// says of the element. Its identity (GlobalId with a copy button,
    /// class, name), where it sits (site, building, storey), the standard
    /// property sets and quantities named with what each is for (planned:
    /// most aren't written yet), where an imported element came from, and
    /// Assign as at the foot.
    fn data_tab(&mut self, ui: &mut egui::Ui, id: ElementId) {
        let Some(e) = self.doc.model().get(id).cloned() else {
            return;
        };
        // drawings, notes and settings stay in CreaBIM: an IFC file holds
        // the building, not how it is drawn
        if !in_ifc(&e.kind) {
            let what = kind_word(&e.kind);
            crate::props::empty(
                ui,
                format!("A {what} stays in CreaBIM: IFC files hold the building, not its drawings and settings."),
            );
            return;
        }
        let model = self.doc.model();
        let class = creabim_core::objects::type_of(model, &e)
            .map_or_else(|| e.ifc_class().to_string(), |t| t.ifc.class.clone());
        let guid = e.id.ifc_guid();
        // where it sits: the names the export gives, and its storey
        let names = creabim_ifc::ExportOptions::for_model(model, "CreaBIM Project");
        let level = match &e.kind {
            ElementKind::Opening(o) => model
                .get(o.host)
                .and_then(|h| creabim_core::levels::level_of(&h.kind)),
            k => creabim_core::levels::level_of(k),
        };
        let storey = level.and_then(|l| model.get(l).map(|x| (l, x.name.clone())));
        use crate::props::{Read, ReadRow};
        let name = if e.name.trim().is_empty() {
            "(no name)".to_string()
        } else {
            e.name.clone()
        };
        // REV04, as agreed (MU56): plain rows straight on the panel, a
        // state chip on the headings that need one, no ? on the headings
        let open = self.data_heading(ui, "data", "Identity", None);
        crate::props::fold(ui, open, |ui| {
            let rows = [
                ReadRow::new(
                    "GlobalId",
                    Read::Copy(guid.clone()),
                    "Its identity in every IFC file it is exported to, made from its own id here: the same each time",
                ),
                ReadRow::new("IFC class", Read::Text(class.clone()), "What IFC calls it"),
                ReadRow::new(
                    "Name",
                    Read::Text(name),
                    "Its Name in IFC: rename it in the header above",
                ),
            ];
            if crate::props::read_rows(ui, &rows) == Some(0) {
                ui.ctx().copy_text(guid.clone());
                self.status = format!("GlobalId {guid} copied.");
            }
        });
        let open = self.data_heading(ui, "data-place", "Where it sits", None);
        crate::props::fold(ui, open, |ui| {
            let storey = match &storey {
                Some((_, n)) => ReadRow::new(
                    "Storey",
                    Read::Text(n.clone()),
                    "The IfcBuildingStorey it is on: one for each level",
                ),
                None => ReadRow::new(
                    "Storey",
                    Read::Text("None: in the building".into()),
                    "It hangs off no level, so it sits in the building itself",
                ),
            };
            crate::props::read_rows(
                ui,
                &[
                    ReadRow::new(
                        "Site",
                        Read::Text(names.site_name.clone()),
                        "The IfcSite it is in: the site's name in the Project tab",
                    ),
                    ReadRow::new(
                        "Building",
                        Read::Text(names.building_name.clone()),
                        "The IfcBuilding it is in: the building's name in the Project tab",
                    ),
                    storey,
                ],
            );
        });
        let measured = element_quantity(ui.ctx(), self.doc.model(), id);
        let (psets, qtos) = ifc_sets(self.doc.model(), &e, measured);
        for (key, title, sets) in [
            ("data-psets", "Property sets", &psets),
            ("data-qtos", "Quantities", &qtos),
        ] {
            if sets.is_empty() {
                continue;
            }
            // the heading's chip says how far the export has come; a set
            // gets its own chip only when its neighbours differ
            let written = sets.iter().filter(|s| s.written).count();
            let mixed = written > 0 && written < sets.len();
            let chip = if written == sets.len() {
                ("written", true)
            } else if mixed {
                ("partly written", false)
            } else {
                ("planned", false)
            };
            let open = self.data_heading(ui, key, title, Some(chip));
            crate::props::fold(ui, open, |ui| {
                for set in sets.iter() {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        crate::props::ifc_set_name(ui, set.name);
                        // REV04: the ? sits beside a property set's name only
                        if key == "data-psets" {
                            self.quick_help_button(ui, "ifc", set.name);
                        }
                        if mixed && set.written {
                            crate::props::chip(ui, "written", true);
                        }
                    });
                    crate::props::plain_note(ui, set.about);
                    crate::props::read_rows(ui, &set.rows);
                    ui.add_space(4.0);
                }
            });
        }
        // its documents: its own, its type's, the file's (Element
        // documents REV00)
        self.documents_section(ui, &e);
        // what the IFC file gave it that CreaBIM keeps without modelling
        // (owner 2026-11-23, "Side record"): read here, written back
        let data = self.doc.model().ifc_data();
        let kept_rows = if data.is_empty() {
            Vec::new()
        } else {
            kept_relations(data, &guid)
        };
        let rec = creabim_core::ifcdata::of(self.doc.model(), &e).cloned();
        if rec.is_some() || !kept_rows.is_empty() {
            let rec = rec.unwrap_or_default();
            use creabim_core::ifcdata::display;
            let open = self.data_heading(
                ui,
                "data-kept",
                "Kept from the IFC file",
                Some(("kept", true)),
            );
            crate::props::fold(ui, open, |ui| {
                crate::props::plain_note(
                    ui,
                    "As the file wrote them. CreaBIM doesn't use these; it carries them and writes them back on export.",
                );
                if !kept_rows.is_empty() {
                    crate::props::read_rows(ui, &kept_rows);
                    ui.add_space(4.0);
                }
                for set in &rec.psets {
                    crate::props::ifc_set_name(ui, &set.name);
                    let rows: Vec<ReadRow> = set
                        .props
                        .iter()
                        .map(|p| {
                            ReadRow::new(
                                p.name.clone(),
                                Read::Text(display(&p.value)),
                                p.value.clone(),
                            )
                        })
                        .collect();
                    crate::props::read_rows(ui, &rows);
                    ui.add_space(4.0);
                }
                if !rec.classes.is_empty() {
                    crate::props::ifc_set_name(ui, "Classification");
                    let rows: Vec<ReadRow> = rec
                        .classes
                        .iter()
                        .map(|c| {
                            let what = [c.code.as_deref(), c.name.as_deref()]
                                .into_iter()
                                .flatten()
                                .collect::<Vec<_>>()
                                .join(" ");
                            let tip = [
                                c.system.as_deref(),
                                c.edition.as_deref(),
                                c.source.as_deref(),
                                c.location.as_deref(),
                            ]
                            .into_iter()
                            .flatten()
                            .collect::<Vec<_>>()
                            .join(" · ");
                            ReadRow::new(
                                c.system.clone().unwrap_or_else(|| "Reference".into()),
                                Read::Text(what),
                                tip,
                            )
                        })
                        .collect();
                    crate::props::read_rows(ui, &rows);
                }
            });
        }
        let open = self.data_heading(
            ui,
            "data-source",
            "From another file",
            Some(("imported only", true)),
        );
        crate::props::fold(ui, open, |ui| match &e.source {
            Some(src) => {
                crate::props::read_rows(
                    ui,
                    &[
                        ReadRow::new("File", Read::Text(file_label(&src.file)), src.file.clone()),
                        ReadRow::new(
                            "Its id there",
                            Read::Text(src.object.clone()),
                            "Its name or GlobalId in that file, so a reload finds it again",
                        ),
                    ],
                );
            }
            None => crate::props::plain_note(
                ui,
                "An element read from IFC shows its file and its GlobalId there.",
            ),
        });
        ui.add_space(4.0);
        self.assign_row(ui, id);
    }

    /// A heading of the Data tab: the standard section card of every other
    /// tab (MU58: "follow our proper container card system"), its ? and
    /// grip, with REV04's state chip after the ?. Folding is remembered
    /// between sessions as Properties' are.
    fn data_heading(
        &mut self,
        ui: &mut egui::Ui,
        key: &str,
        title: &str,
        chip: Option<(&str, bool)>,
    ) -> bool {
        let open = self
            .shell
            .settings
            .props_open
            .get(key)
            .copied()
            .unwrap_or(true);
        let (resp, help_at) = crate::props::header_keyed(ui, title, open, false, key);
        let row_right = resp.rect.right();
        let mut toggle = resp
            .on_hover_text(if open {
                "Click to fold this section"
            } else {
                "Click to open this section"
            })
            .clicked();
        if help_at.is_positive() {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(help_at));
            if self.quick_help_button(&mut child, "ifc", title) {
                toggle = false;
            }
            // the chip after the ?, where there is room before the grip
            if let Some((text, accent)) = chip {
                let at = egui::pos2(help_at.right() + 6.0, help_at.center().y);
                let w = ui
                    .painter()
                    .layout_no_wrap(
                        text.to_string(),
                        egui::FontId::proportional(10.0),
                        Color32::WHITE,
                    )
                    .size()
                    .x
                    + 12.0;
                if at.x + w <= row_right - 26.0 {
                    crate::props::state_chip(ui, at, text, accent);
                }
            }
        }
        if toggle {
            let s = &mut self.shell.settings;
            if open {
                s.props_open.insert(key.to_string(), false);
            } else {
                s.props_open.remove(key);
            }
            s.store();
            return !open;
        }
        open
    }

    /// Assign as, the Data tab's last row (REV04): make a shape or an
    /// imported element a wall, slab, door or window, or give a shape a
    /// class. Shown greyed for what can't be reassigned yet.
    fn assign_row(&mut self, ui: &mut egui::Ui, id: ElementId) {
        let Some(e) = self.doc.model().get(id).cloned() else {
            return;
        };
        let can = matches!(
            e.kind,
            ElementKind::FreeMesh(_)
                | ElementKind::Wall(_)
                | ElementKind::Slab(_)
                | ElementKind::Opening(_)
        );
        let is_mesh = matches!(e.kind, ElementKind::FreeMesh(_));
        let mut pick = None;
        let r = ui.add_enabled_ui(can, |ui| {
            crate::props::Combo::new("Assign as")
                .selected_text("Assign as…")
                .show_ui(ui, |ui| {
                    for a in creabim_exchange::Assign::menu() {
                        if matches!(a, creabim_exchange::Assign::Class(_)) && !is_mesh {
                            continue;
                        }
                        if ui.selectable_label(false, a.label()).clicked() {
                            pick = Some(a);
                        }
                    }
                });
        });
        if !can {
            r.response.on_disabled_hover_text(
                "Only shapes, walls, slabs, doors and windows can be reassigned yet",
            );
        }
        if is_mesh {
            crate::props::plain_note(
                ui,
                "Make this shape a wall, slab, door or window sized to fit it, or give it a class.",
            );
        }
        if let Some(a) = pick {
            let before = e.clone();
            match creabim_exchange::assign(self.doc.model(), id, &a) {
                Ok(after) => {
                    let r = self.doc.commit(
                        Transaction::new(format!("Assign as {}", a.label())).replace(before, after),
                    );
                    self.report(
                        r,
                        format!("Now a {}. Its handles resize it.", a.label().to_lowercase()),
                    );
                }
                Err(why) => self.status = why,
            }
        }
    }

    /// Sheets in number order, as (id, "A101 Floor Plans").
    pub(super) fn sheet_list(&self) -> Vec<(ElementId, String)> {
        sheets::sheets(self.doc.model())
            .into_iter()
            .map(|(id, s)| {
                let name = self.doc.model().get(id).map_or("", |e| e.name.as_str());
                (id, format!("{} {name}", s.number))
            })
            .collect()
    }

    pub(super) fn is_sheet(&self, id: ElementId) -> bool {
        sheets::sheet_of(self.doc.model(), id).is_some()
    }

    fn sheet_info(&self) -> SheetInfo {
        SheetInfo {
            project: stem(&self.path),
            date: sheets::today(),
            ..Default::default()
        }
    }

    /// A sheet's views with their (cached) drawings.
    fn placed_views(&mut self, sheet: ElementId) -> Vec<PlacedView> {
        let ids: Vec<ElementId> = self
            .doc
            .model()
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::Viewport(vp) if vp.sheet == sheet => Some(vp.view),
                _ => None,
            })
            .collect();
        for id in ids {
            self.drawing(id);
        }
        let cache = &self.drawings;
        let model = self.doc.model();
        let looks = &mut self.looks;
        sheets::placed_views_with(
            model,
            sheet,
            &mut |v| cache.get(&v).map(|(_, d)| d.clone()),
            &mut |id, v| match looks.get(&id) {
                // (each view by its own revision: a change of its style
                // alone redraws it and no other)
                Some((r, l)) if *r == model.drawing_revision_of(id) => l.clone(),
                _ => {
                    let rev = model.drawing_revision_of(id);
                    let l = std::sync::Arc::new(creabim_geom::looks::Looks::for_view(model, id, v));
                    looks.insert(id, (rev, l.clone()));
                    l
                }
            },
        )
    }

    /// Every view in the model, plans first, in model order.
    fn views(&self) -> Vec<(ElementId, String, bool)> {
        // in the project browser's order: plans first, by level
        let model = self.doc.model();
        crate::browser::view_groups(model)
            .into_iter()
            .flat_map(|(g, ids)| {
                ids.into_iter().filter_map(move |id| {
                    let e = model.get(id)?;
                    Some((id, e.name.clone(), g == crate::browser::ViewGroup::Plans))
                })
            })
            .collect()
    }

    fn view(&self, id: ElementId) -> Option<&View> {
        match self.doc.model().get(id).map(|e| &e.kind) {
            Some(ElementKind::View(v)) => Some(v),
            _ => None,
        }
    }

    /// The drawing of a view, regenerated only when the model has changed.
    fn drawing(&mut self, id: ElementId) -> Option<Drawing> {
        let revision = self.doc.model().drawing_revision_of(id);
        let xray = self.plan.xray.contains(&id);
        // an area at a phase of its own: its drawing, kept beside the
        // view's (MU133)
        if let Some(p) = crate::phaselens::lens_of(id) {
            let key = (id, xray, p);
            if let Some((r, d)) = self.phased_drawings.get(&key) {
                if *r == revision {
                    return Some(d.clone());
                }
            }
            let d = creabim_geom::section::view_drawing_at(self.doc.model(), id, xray, Some(p))?;
            self.phased_drawings.insert(key, (revision, d.clone()));
            return Some(d);
        }
        let cache = if xray {
            &mut self.xray_drawings
        } else {
            &mut self.drawings
        };
        if let Some((r, d)) = cache.get(&id) {
            if *r == revision {
                return Some(d.clone());
            }
        }
        let t = std::time::Instant::now();
        let d = creabim_geom::section::view_drawing_with(self.doc.model(), id, xray)?;
        cache.insert(id, (revision, d.clone()));
        self.builds
            .record((id, xray), t.elapsed().as_secs_f64() * 1000.0);
        Some(d)
    }

    /// Drawings that build in less than this, in ms, are built on the spot
    /// in the view worked in while an edit follows the pointer (a plan,
    /// 15-40 ms); dearer ones go to a worker meanwhile.
    const ON_THE_SPOT_MS: f64 = 60.0;

    /// The drawing of a view to show now: as [`Self::drawing`] when it is
    /// up to date or cheap to build, else the last one finished while a
    /// worker builds the new one (see [`crate::builds`]). Exports and
    /// anything measured from a drawing use [`Self::drawing`].
    fn drawing_live(&mut self, id: ElementId) -> Option<Drawing> {
        // an area at a phase of its own is drawn on the spot (MU133)
        if crate::phaselens::lens_of(id).is_some() {
            return self.drawing(id);
        }
        let _pf = Perf::new("drawing_live: take finished");
        let finished = self.builds.finished();
        drop(_pf);
        for b in finished {
            let cache = if b.key.1 {
                &mut self.xray_drawings
            } else {
                &mut self.drawings
            };
            match b.drawing {
                // a drawing older than the one held (built on the spot meanwhile) is dropped
                Some(d) if cache.get(&b.key.0).is_none_or(|(r, _)| *r < b.revision) => {
                    cache.insert(b.key.0, (b.revision, d));
                }
                _ => {}
            }
        }
        let revision = self.doc.model().drawing_revision_of(id);
        let xray = self.plan.xray.contains(&id);
        let key = (id, xray);
        let held = if xray {
            self.xray_drawings.get(&id)
        } else {
            self.drawings.get(&id)
        };
        // a site element dragged in a plan: the drawing held from before
        // the drag with that element alone drawn fresh in it; the whole
        // plan is drawn once on release (MU37; MU98: a property's grip
        // rebuilt the plan every frame)
        if let (Some((_, d)), Some((ids, true))) = (held, self.live_edit()) {
            let preview = self
                .view(id)
                .and_then(|v| creabim_geom::ranges::site_preview(self.doc.model(), v, &ids));
            if let Some(mut fresh) = preview {
                let _pp = Perf::new("drawing_live: site preview");
                let mut d = d.clone();
                let at = d.items.iter().position(|it| ids.contains(&it.id()));
                if let Some(at) = at {
                    d.items.retain(|it| !ids.contains(&it.id()));
                    let at = at.min(d.items.len());
                    fresh.retain(|it| ids.contains(&it.id()));
                    d.items.splice(at..at, fresh);
                }
                return Some(d);
            }
        }
        match held {
            Some((r, d)) if *r == revision => Some(d.clone()),
            // the view being worked in, and every plan while a plan grip
            // is dragged, are built on the spot: their lines must follow a
            // drag frame by frame. A plan (about 15 ms) beside a 3D drag
            // goes to a worker, which kept 3D drags at 60-80 ms a frame
            Some((_, d))
                if self.builds.enabled()
                    // a drag in 3D, or a pull or a move there: every
                    // drawing, the one in front too (the plan redrawn on
                    // the spot was 20 ms of every step of a pull, MU45),
                    // and one that counts as cheap elsewhere too (a plan
                    // of 17 to 30 ms was built here every other step, MU46)
                    && ((self.handle_drag.is_some()
                        || self.plan.modal.as_ref().is_some_and(|m| m.pane.is_none()))
                        && !self.builds.under(key, 4.0)
                        || !self.builds.cheap(key)
                            && (!self.worked_in(id)
                                && !(self.plan.grip.is_some()
                                    && matches!(
                                        self.view(id).map(|v| &v.kind),
                                        Some(ViewKind::Plan { .. })
                                    )))
                        // the view worked in, while an edit follows the
                        // pointer, when it is too dear to build on every
                        // frame: a section moved in was built on the spot,
                        // 0.5-0.8 s a frame (MU142 lag). Its last drawing
                        // stays until the worker's arrives.
                        || self.live_edit().is_some()
                            && !self.builds.under(key, Self::ON_THE_SPOT_MS)) =>
            {
                let d = d.clone();
                self.builds.start(self.doc.model(), key);
                Some(d)
            }
            _ => {
                let _pb = Perf::new("drawing_live: build here");
                self.drawing(id)
            }
        }
    }

    /// Is `id` the view being worked in: the one in the area last clicked?
    ///
    /// `active_view` is not it: every area sets that to its own view as it
    /// is drawn, so with a section open beside a plan the section counted
    /// as worked in and was drawn on the spot at every step of a drag in
    /// the plan, 0.4 s a frame (owner markup MU126). Where no area shows
    /// `id` (a workspace not laid out yet), `active_view` still says.
    fn worked_in(&self, id: ElementId) -> bool {
        let shows = self.areas.root.areas();
        let here = crate::areas::Shows::View(id);
        if shows.contains(&here) {
            shows.get(self.areas.focus) == Some(&here)
        } else {
            self.active_view == Some(id)
        }
    }

    /// What follows the pointer just now, and whether it has changed
    /// anything yet: a tool's edit, or a grip dragged in 3D or a drawing.
    fn live_edit(&self) -> Option<(Vec<ElementId>, bool)> {
        if let Some(m) = &self.plan.modal {
            return Some((m.ids(), m.applied()));
        }
        self.handle_drag
            .as_ref()
            .or(self.plan.grip.as_ref())
            .map(|d| (d.ids(), d.applied()))
    }

    /// A grip is being dragged, in 3D or in a drawing.
    pub(crate) fn dragging(&self) -> bool {
        self.handle_drag.is_some() || self.plan.grip.is_some()
    }

    /// The box round the selected elements in 3D.
    fn selection_bounds(&self) -> Option<(GVec3, GVec3)> {
        self.scene
            .iter()
            .filter(|s| self.selection.contains(s.id))
            .filter_map(|s| s.mesh.bounds())
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
    }

    /// Frame the selection in the area last clicked: a plan, section or
    /// elevation zooms its drawing to it, the 3D view its camera (MU63:
    /// the header's Frame always moved the 3D view).
    pub(super) fn frame_selection_here(&mut self) {
        let focus = self.areas.focus;
        match self.areas.root.areas().get(focus) {
            Some(crate::areas::Shows::View(_)) if !self.model_in_front() => {
                self.plan.want_frame = Some(focus);
            }
            _ => self.frame_selection(),
        }
    }

    /// Zoom and turn to the selection, keeping the direction looked from.
    fn frame_selection(&mut self) {
        match self.selection_bounds() {
            Some((lo, hi)) => {
                self.camera.frame(lo, hi, self.aspect);
                self.status = "Framed the selection.".into();
            }
            None if !self.selection.is_empty() => {
                self.status = "The selection isn't shown in this 3D view: its category is hidden here (voids and openings usually are). Show it from the view's visibility to frame it.".into()
            }
            None => self.status = "Select something to frame it.".into(),
        }
    }

    /// Orbit the 3D view by a pointer drag, about the selection when asked.
    fn orbit_3d(&mut self, d: Vec2) {
        if self.cube.grabbing() {
            // the drag started on the view cube, which turns the view itself
            return;
        }
        let d = GVec2::new(d.x, d.y);
        match self.selection_bounds().filter(|_| self.orbit_selection) {
            Some((lo, hi)) => self.camera.orbit_around((lo + hi) / 2.0, d),
            None => self.camera.orbit(d),
        }
    }

    /// Frame all in one pane only: the drawing (Home over it) or the 3D
    /// view (Home over it, or its toolbar), leaving the other where it is.
    fn frame_pane(&mut self, in_2d: bool) {
        if in_2d && self.rect_2d.is_some() {
            if let Some(v) = self.active_view {
                self.plan.cameras.remove(&v);
                self.plan.area_cameras.remove(&(self.areas.focus, v));
            }
            self.status = "Framed the drawing.".into();
        } else {
            self.frame_3d();
            self.status = "Framed the 3D view.".into();
        }
    }

    /// The view cube in the 3D view's corner: a click turns the view, a
    /// drag on it orbits, and a drag on its zoom or pan button zooms or
    /// pans.
    /// The references in 3D (MU32): each where its views show it, on the
    /// plan's level or standing on the section's plane, faint as in the
    /// views. A picture shows its frame.
    /// Where Divide will cut, while its window is open (MU36): a mark at
    /// each division on the selected element's length or height.
    fn draw_divide_preview(
        &self,
        painter: &egui::Painter,
        project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
    ) {
        let accent = crate::planview::accent_selected();
        for (a, b, _) in &self.plan.divide_marks {
            if let (Some(pa), Some(pb)) = (project(*a), project(*b)) {
                if self.plan.divide_guides {
                    let ink = egui::Stroke::new(1.5, crate::planview::GUIDE_COLOR);
                    crate::snaps::dashed_clipped(painter, &[pa, pb], ink, 8.0, 5.0);
                } else {
                    painter.line_segment([pa, pb], egui::Stroke::new(2.0, accent));
                }
            }
        }
        let mut taken = vec![];
        for (at, text) in &self.plan.divide_labels {
            if let Some(q) = project(*at) {
                crate::planview::divide_label(painter, q, text, accent, &mut taken);
            }
        }
        crate::modguides::draw(painter, &self.plan.mod_guides, accent, false, project);
        // what a plan's pointer would place, here in 3D too
        if let Some((from, g)) = self
            .plan
            .ghost_shared
            .as_ref()
            .filter(|(from, _)| *from != Some(usize::MAX))
        {
            let _ = from;
            crate::ghost::draw(painter, project, g, crate::theme::interaction());
        }
    }

    /// The guides of the modifier card last worked in, while its mass is
    /// the one selected and the modifiers tab is open (MU39).
    fn modifier_guides(&mut self) -> crate::modguides::Guides {
        let Some((id, i)) = self.mod_focus else {
            return Default::default();
        };
        if self.selection.active != Some(id) || self.prop_tab != PropTab::Modifiers {
            self.mod_focus = None;
            return Default::default();
        }
        let model = self.doc.model();
        let Some(ElementKind::FreeMesh(m)) = model.get(id).map(|e| &e.kind) else {
            return Default::default();
        };
        let Some(md) = m.modifiers.get(i).filter(|md| md.enabled) else {
            return Default::default();
        };
        let z = m
            .level
            .and_then(|l| model.level(l))
            .map_or(0.0, |l| l.elevation);
        let base = m.origin + DVec3::Z * z;
        crate::modguides::of(m, &md.kind, &|v| base + m.to_model(v))
    }

    /// Start Split on the selection (MU39): next, a line to cut along.
    pub(super) fn start_split(&mut self) {
        let targets = self.selection.all();
        if targets.is_empty() {
            self.status =
                "Split: select what to cut first (walls, slabs, parts, sketches, masses).".into();
            return;
        }
        self.split = Some(SplitRun {
            targets,
            a: None,
            line: None,
        });
        self.plan.point_pick = true;
        self.plan.point_lines = true;
        self.status = "Split: click a guide, a line or an edge to cut along, or click two points. Esc leaves.".into();
    }

    /// Split's clicks as they come, and what it shows meanwhile.
    fn split_steps(&mut self, ctx: &egui::Context) {
        self.plan.point_from = None;
        let Some(mut run) = self.split.take() else {
            return;
        };
        let picked = self.plan.point_picked.take();
        let line = self.plan.point_line_picked.take();
        // Esc or a right-click in a view ends it (a click taken this frame
        // is not a leaving)
        let left = !self.plan.point_pick && picked.is_none() && line.is_none();
        if left || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.plan.point_pick = false;
            self.plan.point_lines = false;
            self.plan.mod_guides = Default::default();
            self.status = "Split left.".into();
            return;
        }
        self.plan.point_pick = true;
        use creabim_core::split::Line;
        match (run.line, run.a, line, picked) {
            (None, _, Some((a, b)), _) => {
                run.line = Some(Line { a, b });
                run.a = None;
            }
            (None, None, None, Some(p)) => {
                run.a = Some(p);
                self.status = "Split: click the line's second point.".into();
            }
            (None, Some(a), None, Some(p)) if p.distance(a) > 1e-6 => {
                run.line = Some(Line { a, b: p });
                run.a = None;
            }
            (Some(cut), _, _, Some(p)) => {
                self.do_split(&run, cut, p);
                return;
            }
            _ => {}
        }
        if run.line.is_some() {
            self.plan.point_lines = false;
            self.status = "Split: click the side to keep selected. Esc leaves.".into();
        }
        self.plan.point_from = run.a;
        self.plan.mod_guides = self.split_guides(&run);
        self.split = Some(run);
    }

    /// The cutting plane in red, or the line being drawn.
    fn split_guides(&self, run: &SplitRun) -> crate::modguides::Guides {
        let mut g = crate::modguides::Guides {
            ink: Some(crate::planview::SPLIT_RED),
            ..Default::default()
        };
        // as high and as far as what it cuts, and a little more
        let (mut lo, mut hi) = (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN));
        for item in self.scene.iter().filter(|i| run.targets.contains(&i.id)) {
            if let Some((a, b)) = item.mesh.bounds() {
                lo = lo.min(a.as_dvec3());
                hi = hi.max(b.as_dvec3());
            }
        }
        if lo.x > hi.x {
            let z = self.work_level().map_or(0.0, |(_, z)| z);
            lo = DVec3::new(-5.0, -5.0, z);
            hi = DVec3::new(5.0, 5.0, z + 3.0);
        }
        let (z0, z1) = (lo.z - 0.3, hi.z + 0.3);
        let reach = (hi - lo).truncate().length() / 2.0 + 1.0;
        let mid = ((lo + hi) / 2.0).truncate();
        match (run.line, run.a, self.plan.point_hover) {
            (Some(l), _, _) => {
                let d = (l.b - l.a).normalize_or_zero();
                let c = l.a + d * d.dot(mid - l.a);
                let (p, q) = (c - d * reach, c + d * reach);
                g.planes
                    .push([p.extend(z0), q.extend(z0), q.extend(z1), p.extend(z1)]);
                g.lines.push((p.extend(z0), q.extend(z0)));
            }
            (None, Some(a), Some(h)) => {
                g.lines
                    .push((a.extend(z0.max(lo.z)), h.extend(z0.max(lo.z))));
                g.points.push(a.extend(lo.z));
            }
            _ => {}
        }
        g
    }

    /// Cut, then keep selected what lies on the side clicked.
    fn do_split(&mut self, run: &SplitRun, cut: creabim_core::split::Line, side_at: DVec2) {
        self.plan.point_pick = false;
        self.plan.point_lines = false;
        self.plan.mod_guides = Default::default();
        match creabim_core::split::split(self.doc.model(), &run.targets, cut) {
            Ok(s) => {
                let mut pieces: Vec<ElementId> = run.targets.clone();
                for op in &s.tx.ops {
                    if let creabim_core::Op::Insert(e) = op {
                        pieces.push(e.id);
                    }
                }
                let label = s.tx.label.clone();
                if let Err(e) = self.doc.commit(s.tx) {
                    self.status = format!("Couldn't split: {e}");
                    return;
                }
                let want = cut.side(side_at) > 0.0;
                let model = self.doc.model();
                let kept: Vec<ElementId> = pieces
                    .into_iter()
                    .filter(|id| {
                        creabim_core::transform::pivot(model, *id)
                            .is_some_and(|p| (cut.side(p.truncate()) > 0.0) == want)
                    })
                    .collect();
                self.selection.clear();
                for id in &kept {
                    self.selection.toggle(*id);
                }
                self.face = None;
                self.status = format!(
                    "{label}: the {} on the side you clicked selected. Ctrl+Z to undo.{}",
                    if kept.len() == 1 {
                        "piece".to_string()
                    } else {
                        format!("{} pieces", kept.len())
                    },
                    if s.skipped.is_empty() {
                        String::new()
                    } else {
                        format!(" Not cut: {}.", s.skipped.join("; "))
                    }
                );
            }
            Err(e) => self.status = e,
        }
    }

    /// A centre picked in a plan for the radial array being set.
    fn take_picked_centre(&mut self) {
        let Some(w) = self.plan.point_picked.take() else {
            return;
        };
        let Some((id, i)) = self.mod_focus else {
            return;
        };
        let model = self.doc.model();
        let Some(ElementKind::FreeMesh(m)) = model.get(id).map(|e| &e.kind) else {
            return;
        };
        // the model point in the mesh's own axes (its turn and scale undone)
        let local = m.to_local((w - m.origin.truncate()).extend(0.0));
        let r = self.doc.edit(id, "Set radial centre", |e| {
            if let ElementKind::FreeMesh(m) = &mut e.kind {
                if let Some(creabim_core::Modifier {
                    kind: creabim_core::ModifierKind::RadialArray { center, .. },
                    ..
                }) = m.modifiers.get_mut(i)
                {
                    *center = local.truncate();
                }
            }
        });
        self.report(
            r,
            format!(
                "Centre set at {}, {}.",
                crate::units::len(local.x),
                crate::units::len(local.y)
            ),
        );
    }

    /// The references shown in 3D: not hidden, and not kept to their views.
    fn refs_in_3d(&self) -> Vec<(ElementId, creabim_core::Reference)> {
        self.doc
            .model()
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::Reference(r)
                    if r.shows_in_3d()
                        && (if self.camera.ortho {
                            r.in_orthographic
                        } else {
                            r.in_perspective
                        })
                        && !self.hidden.contains(&e.id) =>
                {
                    // its own strength in 3D, when it has one
                    let mut r = r.clone();
                    r.opacity = r.opacity_3d.unwrap_or(r.opacity);
                    Some((e.id, r))
                }
                _ => None,
            })
            .collect()
    }

    /// The reference whose line (or a picture's frame) is within 6 px of
    /// `p` in 3D.
    fn reference_near_3d(
        &mut self,
        p: egui::Pos2,
        project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
    ) -> Option<ElementId> {
        let near = |a: egui::Pos2, b: egui::Pos2| {
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
            (a + ab * t).distance(p) < 6.0
        };
        for (id, r) in self.refs_in_3d() {
            let model = self.doc.model();
            let world = |q: DVec2| crate::handles::ref_world(model, &r, q);
            let polys: Vec<Vec<DVec2>> = match self.plan.ref_cache.get(model, id, &r) {
                crate::trace::Shown::Lines(l) => l
                    .iter()
                    .map(|l| l.iter().map(|q| r.place(*q)).collect())
                    .collect(),
                _ => r
                    .outline()
                    .map(|c| vec![c.iter().chain(c.first()).copied().collect()])
                    .unwrap_or_default(),
            };
            for l in polys {
                let pts: Vec<egui::Pos2> = l.iter().filter_map(|q| project(world(*q))).collect();
                if pts.windows(2).any(|w| near(w[0], w[1])) {
                    return Some(id);
                }
            }
        }
        None
    }

    fn draw_references_3d(
        &mut self,
        painter: &egui::Painter,
        project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
    ) {
        let refs = self.refs_in_3d();
        // a picked one is outlined as the plans outline it: dashed, in the
        // selection orange, on its own plane (MU38)
        let picked: Vec<Vec<egui::Pos2>> = refs
            .iter()
            .filter(|(id, _)| self.selection.contains(*id))
            .filter_map(|(_, r)| {
                let c = r.outline()?;
                let model = self.doc.model();
                let pts: Vec<egui::Pos2> = c
                    .iter()
                    .chain(c.first())
                    .filter_map(|p| project(crate::handles::ref_world(model, r, *p)))
                    .collect();
                (pts.len() == 5).then_some(pts)
            })
            .collect();
        self.draw_reference_lines_3d(painter, project, refs);
        for pts in picked {
            crate::snaps::dashed_clipped(
                painter,
                &pts,
                egui::Stroke::new(1.5, crate::planview::accent_selected()),
                6.0,
                4.0,
            );
        }
    }

    fn draw_reference_lines_3d(
        &mut self,
        painter: &egui::Painter,
        project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
        refs: Vec<(ElementId, creabim_core::Reference)>,
    ) {
        // a huge drawing is cut short rather than slowing the 3D view
        let mut budget = 30_000usize;
        for (id, r) in refs {
            let model = self.doc.model();
            let world = |p: DVec2| crate::handles::ref_world(model, &r, p);
            let ink = crate::trace::colour(&r)
                .unwrap_or(Color32::from_gray(150))
                .gamma_multiply(r.opacity.clamp(0.15, 1.0));
            let stroke = egui::Stroke::new(1.0, ink);
            match self.plan.ref_cache.get(model, id, &r) {
                crate::trace::Shown::Lines(lines) => {
                    for l in lines.iter() {
                        if budget == 0 {
                            return;
                        }
                        let pts: Vec<egui::Pos2> = l
                            .iter()
                            .filter_map(|p| project(world(r.place(*p))))
                            .collect();
                        budget = budget.saturating_sub(pts.len());
                        if pts.len() >= 2 {
                            painter.add(egui::Shape::line(pts, stroke));
                        }
                    }
                }
                // a picture: its pixels on its plane, in a grid of small
                // pieces so its perspective holds (MU34)
                crate::trace::Shown::Picture(raster) => {
                    let Some([lo, hi]) = r.bounds else { continue };
                    let tex = crate::textures::texture(painter.ctx(), &raster);
                    let tint = crate::trace::colour(&r)
                        .unwrap_or(Color32::WHITE)
                        .gamma_multiply(r.opacity.clamp(0.0, 1.0));
                    const N: usize = 12;
                    let mut mesh = egui::Mesh::with_texture(tex);
                    let mut ok = true;
                    for j in 0..=N {
                        for i in 0..=N {
                            let (fu, fv) = (i as f64 / N as f64, j as f64 / N as f64);
                            let q =
                                DVec2::new(lo.x + (hi.x - lo.x) * fu, lo.y + (hi.y - lo.y) * fv);
                            let Some(pos) = project(world(r.place(q))) else {
                                ok = false;
                                break;
                            };
                            mesh.vertices.push(egui::epaint::Vertex {
                                pos,
                                uv: egui::pos2(fu as f32, 1.0 - fv as f32),
                                color: tint,
                            });
                        }
                    }
                    if ok {
                        for j in 0..N as u32 {
                            for i in 0..N as u32 {
                                let a = j * (N as u32 + 1) + i;
                                let (b, c, d) = (a + 1, a + N as u32 + 2, a + N as u32 + 1);
                                mesh.indices.extend([a, b, c, a, c, d]);
                            }
                        }
                        painter.add(egui::Shape::mesh(mesh));
                    }
                }
                _ => {
                    if let Some(c) = r.outline() {
                        let pts: Vec<egui::Pos2> = c
                            .iter()
                            .chain(c.first())
                            .filter_map(|p| project(world(*p)))
                            .collect();
                        painter.add(egui::Shape::line(pts, egui::Stroke::new(1.0, ink)));
                    }
                }
            }
        }
    }

    fn view_cube(&mut self, ui: &mut egui::Ui, rect: Rect) {
        use crate::viewcube::{said, target_of, Nav};
        let accent = crate::theme::Palette::of(ui.ctx()).accent;
        let (pick, nav) = self
            .cube
            .ui(ui, rect, &self.camera, accent, self.camera.ortho);
        match nav {
            Some(Nav::Orbit(d)) => self.camera.orbit(GVec2::new(d.x, d.y)),
            // the view moves with the pointer, as a right-button drag does
            Some(Nav::Pan(d)) => self.camera.pan(GVec2::new(d.x, d.y), rect.height()),
            // up zooms in
            Some(Nav::Zoom(d)) => self.camera.zoom(-d.y / 150.0),
            None => {}
        }
        let Some(pick) = pick else {
            return;
        };
        match pick {
            crate::viewcube::Pick::FitAll => {
                self.frame_3d();
                self.status = "Framed everything.".into();
                ui.ctx().request_repaint();
                return;
            }
            crate::viewcube::Pick::FitSel => {
                if self.selection.is_empty() {
                    self.status = "Select something to frame first.".into();
                } else {
                    self.frame_selection();
                    self.status = "Framed the selection.".into();
                }
                ui.ctx().request_repaint();
                return;
            }
            _ => {}
        }
        if pick == crate::viewcube::Pick::Projection {
            self.camera.ortho = !self.camera.ortho;
            self.status = if self.camera.ortho {
                "Orthographic: parallel lines stay parallel.".into()
            } else {
                "Perspective.".into()
            };
            ui.ctx().request_repaint();
            return;
        }
        let now = ui.input(|i| i.time);
        let to = match target_of(pick, &self.camera) {
            Some(to) => to,
            None => {
                let (yaw, pitch) = (self.camera.yaw, self.camera.pitch);
                self.frame_3d();
                (self.camera.yaw, self.camera.pitch) = (yaw, pitch);
                let mut iso = self.camera.clone();
                iso.look(crate::camera::Look::Iso);
                (iso.yaw, iso.pitch)
            }
        };
        self.cube.turn_to(&self.camera, to, now);
        self.status = said(pick);
        ui.ctx().request_repaint();
    }

    /// Fit the 3D view round the whole model.
    fn frame_3d(&mut self) {
        // the extents only: never the joined scene, which a big model
        // took 18 s to make (Highland Haven) before the window showed
        if let Some((lo, hi)) = creabim_geom::scene::scene_extents(self.doc.model()) {
            self.camera.frame(lo, hi, self.aspect);
        }
    }

    /// Every view framed afresh, as when a project opens.
    fn frame_all(&mut self) {
        self.frame_3d();
        self.plan.cameras.clear();
    }

    fn report(&mut self, r: Result<(), impl ToString>, ok: impl Into<String>) {
        self.status = match r {
            Ok(()) => ok.into(),
            Err(e) => format!("Couldn't do that: {}", e.to_string()),
        };
    }

    /// Run an edit picked from a key, a button or a menu. `at` is where a
    /// right-click menu was opened.
    fn run_command(&mut self, ctx: &egui::Context, c: Command, at: Option<DVec3>) {
        // a Masters view in front: Move, Rotate, Duplicate and Delete act
        // on what is picked on the master (MU40)
        if self.masters_command(c) {
            return;
        }
        if c == Command::Duplicate {
            let ids = self.selection.all();
            if let Some(r) = crate::modal::duplicate_views(&mut self.doc, &ids) {
                match r {
                    Ok(copies) => {
                        self.status = crate::modal::views_duplicated(&self.doc, &copies);
                        self.selection.extend(copies, false);
                    }
                    Err(e) => self.status = e,
                }
                return;
            }
        }
        if let Some(m) = c.modal() {
            self.arm(c, m);
            return;
        }
        match c {
            Command::Copy | Command::Cut => {
                if self.copy_selected() {
                    let n = self.clipboard.len();
                    ctx.copy_text(self.clipboard_text());
                    if c == Command::Cut {
                        self.delete_selected();
                        self.status = format!("Cut {n} elements. Ctrl+V pastes them back.");
                    } else {
                        self.status = format!("Copied {n} elements. Ctrl+V pastes them.");
                    }
                }
            }
            Command::Paste => {
                if self.clipboard.is_empty() {
                    self.status = "Nothing copied yet.".into();
                } else {
                    self.paste(self.clipboard.clone());
                }
            }
            // in Edit mode Delete takes the picked corners away, never the
            // element being edited
            Command::Delete if self.plan.edit.is_some() => self.delete_grips(),
            Command::Delete => self.delete_selected(),
            Command::Hide => self.hide_selected(),
            Command::Isolate => self.isolate_selected(),
            Command::ShowAll => self.show_all(),
            Command::SelectAll => self.select_all(),
            Command::SelectNone => {
                self.selection.clear();
                self.face = None;
            }
            Command::Undo => {
                let label = self.doc.undo_label().map(str::to_owned);
                if self.doc.undo() {
                    self.status = format!("Undid: {}", label.unwrap_or_default());
                    self.drop_stale_selection();
                }
            }
            Command::Redo => {
                let label = self.doc.redo_label().map(str::to_owned);
                if self.doc.redo() {
                    self.status = format!("Redid: {}", label.unwrap_or_default());
                    self.drop_stale_selection();
                }
            }
            Command::CursorHere => {
                if let Some(p) = at {
                    self.plan.cursor = Some(p);
                    self.plan.pivot = crate::modal::Pivot::Cursor;
                    self.status = format!(
                        "Pivot cursor at {:.2}, {:.2}, {:.2}. Rotate and scale now turn about it.",
                        p.x, p.y, p.z
                    );
                }
            }
            Command::WorkplaneHere => {
                // from a plan: a level plane through the point
                if let Some(p) = at {
                    let plane = Workplane {
                        origin: p,
                        ..Workplane::ground(p.z)
                    };
                    self.set_workplane(plane, format!("level at {}", crate::units::elevation(p.z)));
                }
            }
            Command::Stretch => self.arm_stretch(),
            Command::Move
            | Command::Rotate
            | Command::Scale
            | Command::Mirror
            | Command::Offset
            | Command::Pull
            | Command::Duplicate => {}
        }
    }

    /// Stretch (Shift+S): the next box dragged in a plan takes the
    /// corners and ends inside it, then they move.
    pub(super) fn arm_stretch(&mut self) {
        if let Some(mut m) = self.plan.modal.take() {
            m.cancel(&mut self.doc);
        }
        self.plan.cancel();
        self.plan.stretch_box = true;
        self.status = "Stretch: drag a box over the corners and ends to move. What lies wholly inside moves; what it crosses stretches. Esc stops.".into();
    }

    /// Get Move, Rotate, Scale or Duplicate ready: the view the pointer
    /// goes into starts it. With nothing selected, Move and Rotate first
    /// ask what to work on.
    fn arm(&mut self, c: Command, m: (crate::modal::Mode, bool)) {
        use crate::modal::Mode;
        let picks_first = matches!(m, (Mode::Move | Mode::Rotate | Mode::Pull, false));
        if self.selection.is_empty() && !picks_first {
            self.status = "Select something first.".into();
            return;
        }
        if let Some(old) = self.plan.modal.as_mut() {
            old.discard(&mut self.doc);
            self.plan.modal = None;
        }
        self.plan.cancel();
        self.plan.tool = Tool::Select;
        self.plan.armed = Some(m);
        self.status = match (m.0, self.selection.is_some()) {
            (Mode::Pull, _) => {
                "Pull: point at a face in a view and click on it, then move out or in, or type a distance. A flat shape becomes a solid; a wall, a slab or a roof changes its own size. Esc leaves the tool.".into()
            }
            (Mode::Move, false) => {
                "Move: click what to move (Shift+click adds more), then click the base point. Esc cancels.".into()
            }
            (Mode::Rotate, false) => {
                "Rotate: click what to rotate (Shift+click adds more), then click the centre. Esc cancels.".into()
            }
            (Mode::Move, true) => {
                "Move: click the base point in a view, then where it goes. Esc steps back.".into()
            }
            (Mode::Rotate, true) => {
                "Rotate: click the centre in a view, then a reference direction, then the new direction.".into()
            }
            (Mode::Mirror, _) => {
                "Mirror: click the first point of the line to mirror across, then the second. In 3D it lies on the workplane (a face, or the level). A mirrored copy is made; Ctrl keeps only the mirrored one.".into()
            }
            (Mode::Offset, _) => {
                "Offset: point at a view and move the pointer to the side to offset to, or type a distance; click to place a copy. Ctrl moves the original instead.".into()
            }
            _ if c == Command::Duplicate => {
                "Duplicate: point at a view and click a base point; the copy follows to where you click next. Esc takes the copy away.".into()
            }
            _ => format!(
                "{}: point at a view and move the pointer, then click to finish. Right-click cancels.",
                c.label()
            ),
        };
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        // AppKit can pass disabled menu shortcuts on to the focused view.
        #[cfg(target_os = "macos")]
        if self.uses_native_menu() && self.mac_menu_blocked() {
            return;
        }
        // a length typed into a grip's drag takes the keys
        if ctx.egui_wants_keyboard_input() || self.plan.grips.typing {
            return;
        }
        // over a Profile view its own keys win (M, R, F, D, Delete…): the
        // model's would act on the model behind it (MU37: Shift+D started a
        // dimension); undo and redo still work
        if self.profile_view_hovered(ctx)
            || self.masters_view_hovered(ctx)
            || self.pattern_view_hovered(ctx)
        {
            let (undo, redo) = ctx.input(|i| {
                let cmd = i.modifiers.command;
                (
                    cmd && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                    cmd && (i.key_pressed(egui::Key::Y)
                        || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
                )
            });
            if undo {
                self.run_command(ctx, Command::Undo, None);
            }
            if redo {
                self.run_command(ctx, Command::Redo, None);
            }
            return;
        }
        // Alt+S: grid snap on or off, as in ArchiCAD
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, egui::Key::S)) {
            let s = &mut self.plan.snap;
            s.increment = !s.increment;
            self.status = if s.increment {
                format!(
                    "Grid snap on ({}): points land on its grid. Alt+S turns it off.",
                    s.spacing_label()
                )
            } else {
                "Grid snap off: points and drags go exactly where the pointer is.".into()
            };
        }
        let editing = self.plan.edit.is_some();
        let (undo, redo, delete, extrude) = ctx.input(|i| {
            let cmd = i.modifiers.command;
            (
                cmd && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                cmd && (i.key_pressed(egui::Key::Y)
                    || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
                // in Edit mode Backspace removes picked corners too
                i.key_pressed(egui::Key::Delete)
                    || (editing && i.key_pressed(egui::Key::Backspace)),
                !cmd && i.key_pressed(egui::Key::E),
            )
        });
        // F1: the guide's page about what you are doing
        if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
            self.run_action(ctx, "help");
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F3)) {
            self.plan.snap.on = !self.plan.snap.on;
            self.status = format!(
                "Snaps {}. F3 turns them {}.",
                if self.plan.snap.on { "on" } else { "off" },
                if self.plan.snap.on { "off" } else { "on" },
            );
        }
        // Shift+G: guides on and off everywhere; Alt+Z: X-ray in 3D;
        // Alt+G: grid lines in 3D (MU42)
        let (guides_key, xray_key, grids_key) = ctx.input(|i| {
            let m = i.modifiers;
            (
                m.shift && !m.command && !m.alt && i.key_pressed(egui::Key::G),
                m.alt && !m.command && !m.shift && i.key_pressed(egui::Key::Z),
                m.alt && !m.command && !m.shift && i.key_pressed(egui::Key::G),
            )
        });
        if guides_key && self.plan.modal.is_none() {
            self.set_guides_hidden(!self.shell.settings.guides_hidden);
        }
        if grids_key && !ctx.egui_wants_keyboard_input() {
            self.toggle_grids_3d();
        }
        // which pane Alt+Z works in: the one the pointer is (or was last) over
        if let Some(p) = ctx.pointer_hover_pos() {
            if self.rect_2d.is_some_and(|r| r.contains(p)) {
                self.last_in_2d = true;
            } else if self.rect_3d.is_some_and(|r| r.contains(p)) {
                self.last_in_2d = false;
            }
        }
        if xray_key {
            match self.xray_view_target() {
                Some(view) => self.toggle_view_xray(view),
                None => self.set_xray(!self.shell.settings.xray),
            }
        }
        // Ctrl+[ folds the browser, Ctrl+] Properties (MU27: T and N, as
        // Blender's, are Tag and Note here)
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::OpenBracket)) {
            self.shell.settings.left_folded = !self.shell.settings.left_folded;
            self.shell.settings.store();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::CloseBracket)) {
            self.shell.settings.right_folded = !self.shell.settings.right_folded;
            self.shell.settings.store();
        }
        // Tab: Object and Edit mode, as Blender (MU27); 1, 2, 3 in Edit mode:
        // points, edges, faces
        {
            let free = self.plan.tool == Tool::Select
                && self.plan.modal.is_none()
                && !self.plan.busy()
                && !ctx.egui_wants_keyboard_input();
            let layered = self
                .plan
                .edit
                .is_some_and(|id| handles::has_layers(self.doc.model(), id));
            // Tab still cycles what lies under the pointer when there are
            // several things there (MU25); otherwise it switches the mode
            let cycling = self.plan.hover.as_ref().is_some_and(|h| h.under.len() >= 2);
            // in Edit mode Tab always leaves it (a layered element's layers
            // are Shift+Tab now, MU39)
            let _ = layered;
            if free
                && (self.plan.edit.is_some() || !cycling)
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab))
            {
                self.toggle_edit_mode();
            }
            if free && self.plan.edit.is_some() {
                for (k, key) in [
                    (1u8, egui::Key::Num1),
                    (2, egui::Key::Num2),
                    (3, egui::Key::Num3),
                ] {
                    if ctx.input(|i| i.modifiers.is_none() && i.key_pressed(key)) {
                        handles::set_pick(if handles::pick() == k { 0 } else { k });
                        self.status = match handles::pick() {
                            1 => "Edit mode: points.".into(),
                            2 => "Edit mode: edges.".into(),
                            3 => "Edit mode: faces.".into(),
                            _ => "Edit mode: everything.".into(),
                        };
                    }
                }
            }
        }
        // Enter opens the selected element in Edit mode; only Tab (or the
        // mode menu) closes it, as Blender's (MU39)
        let enter = ctx.input(|i| i.modifiers.is_none() && i.key_pressed(egui::Key::Enter));
        if enter
            && !ctx.egui_wants_keyboard_input()
            && self.plan.edit.is_none()
            && self.plan.tool == Tool::Select
            && self.plan.modal.is_none()
            && !self.plan.busy()
            && self.plan.outline.is_none()
        {
            self.toggle_edit_mode();
        }
        // the Annotate board's keys (MU24): D Dimension, T Tag, N Note;
        // Shift+A the Add menu, with nothing selected (with a selection it
        // keeps selecting all of its type); PgUp / PgDn the previous and
        // next sheet, or view of the same kind
        if !ctx.egui_wants_keyboard_input() && self.plan.modal.is_none() && self.add_menu.is_none()
        {
            let (d, t, n, add, prev, next) = ctx.input(|i| {
                let m = i.modifiers;
                let plain = !m.command && !m.alt && !m.shift;
                (
                    plain && i.key_pressed(egui::Key::D),
                    plain && i.key_pressed(egui::Key::T),
                    plain && i.key_pressed(egui::Key::N),
                    m.shift && !m.command && !m.alt && i.key_pressed(egui::Key::A),
                    plain && i.key_pressed(egui::Key::PageUp),
                    plain && i.key_pressed(egui::Key::PageDown),
                )
            });
            let tag_all = ctx.input(|i| {
                i.modifiers.shift
                    && !i.modifiers.command
                    && !i.modifiers.alt
                    && i.key_pressed(egui::Key::T)
            });
            if tag_all {
                self.run_action(ctx, "tag.all");
            }
            let empty = self.selection.is_empty();
            // (with a selection, D toggles its grips' live dimensions)
            if d && empty {
                self.pick_tool(ctx, Tool::Dimension);
            }
            if t {
                self.pick_tool(ctx, Tool::Tag);
            }
            // J: Align in Corner mode, from any tool (wave 3)
            let corner = crate::keymap::fired(ctx, crate::keymap::Act::Corner);
            if corner {
                self.use_corner_tool();
            }
            // Shift+S: Stretch, a box over corners and ends (wave 3)
            let stretch = crate::keymap::fired(ctx, crate::keymap::Act::Stretch);
            if stretch && self.plan.modal.is_none() {
                self.arm_stretch();
            }
            if n {
                self.pick_tool(ctx, Tool::Note);
            }
            if add && empty {
                self.add_menu = Some(String::new());
            }
            if prev || next {
                self.step_view(if next { 1 } else { -1 });
            }
        }
        self.add_menu_window(ctx);
        // Ctrl+P: this sheet as a PDF; Ctrl+Shift+P: Export sheets (MU22)
        if ctx.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::P,
            )
        }) {
            self.sheets_export_open = true;
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::P)) {
            if self.active_view.is_some_and(|id| self.is_sheet(id)) {
                self.export_sheets_now(true);
            } else {
                self.sheets_export_open = true;
            }
        }
        // Alt+F2 shows or hides the active view's references (MU18/MU19)
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, egui::Key::F2)) {
            if let Some(v) = self.active_view {
                self.toggle_references(v);
            }
        }
        // Tab in Edit mode on a layered element: its layers' ends, and back
        if let Some(id) = self.plan.edit {
            let free = self.handle_drag.is_none()
                && !self.plan.gripping()
                && !self.plan.grips.typing
                && !ctx.egui_wants_keyboard_input();
            if free && handles::has_layers(self.doc.model(), id) {
                let tab = ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab));
                if tab {
                    let on = !handles::layers();
                    handles::set_layers(on);
                    self.status = if on {
                        match self.doc.model().get(id).map(|e| &e.kind) {
                            Some(ElementKind::Wall(_)) => "Layers: drag a layer's end to run it on or hold it back. Shift+Tab: back to the element's grips.",
                            Some(ElementKind::Stair(_)) => "Flights and landings: drag a flight's top riser for more or fewer treads, a landing back to make it deeper. Each change stays when the line changes. Shift+Tab: back to the line.",
                            _ => "Layers: drag a layer's edge to push it out past the edge or pull it in. Shift+Tab: back to the element's grips.",
                        }
                        .into()
                    } else {
                        editmode::enter(self.doc.model(), id, &mut self.plan.edit)
                    };
                }
            }
        }
        // M: the Move tool
        let m_key = crate::keymap::fired(ctx, crate::keymap::Act::Move);
        if m_key && self.plan.modal.is_none() && !self.plan.busy() {
            self.run_command(ctx, Command::Move, None);
        }
        // Mirror and Offset, when the keymap gives them keys (Revit MM, OF)
        for (act, c) in [
            (crate::keymap::Act::Mirror, Command::Mirror),
            (crate::keymap::Act::Offset, Command::Offset),
        ] {
            if crate::keymap::fired(ctx, act) && self.plan.modal.is_none() && !self.plan.busy() {
                self.run_command(ctx, c, None);
            }
        }
        // L in Edit mode: the picks on a mass grow to their whole pieces
        let l_key = ctx.input(|i| i.modifiers.is_none() && i.key_pressed(egui::Key::L));
        if l_key
            && self.plan.edit.is_some()
            && self.plan.tool == Tool::Select
            && self.plan.modal.is_none()
            && !self.plan.busy()
        {
            if let Some(s) = crate::handles::pick_linked(&self.doc, &mut self.plan.grips) {
                self.status = s;
            }
        }
        // I: Inset, with a face of a shape picked (MU49, decision 4). Only
        // then, so the letter stays free for a keymap's own sequences
        let i_key = ctx.input(|i| i.modifiers.is_none() && i.key_pressed(egui::Key::I));
        if i_key
            && self.plan.tool == Tool::Select
            && self.plan.modal.is_none()
            && self.plan.armed.is_none()
            && !self.plan.busy()
        {
            let on_shape = self
                .selection
                .active
                .filter(|_| self.selection.len() == 1)
                .filter(|id| {
                    matches!(
                        self.doc.model().get(*id).map(|e| &e.kind),
                        Some(ElementKind::FreeMesh(_))
                    )
                });
            if let Some(id) = on_shape {
                if self.face.is_some()
                    || !crate::handles::whole_faces(self.doc.model(), id).is_empty()
                {
                    self.start_inset();
                }
            }
        }
        // P: Pull, the face under the pointer out or in (MU43)
        let p_key = crate::keymap::fired(ctx, crate::keymap::Act::Pull);
        if p_key && self.plan.modal.is_none() && !self.plan.busy() {
            self.run_command(ctx, Command::Pull, None);
        }
        // Space flips: the wall being drawn, else the selected walls, doors
        // and windows; Shift+Space a door's hand
        let flip = ctx.input(|i| {
            // Ctrl+Space shows an area alone instead
            let ctrl = i.events.iter().any(|e| {
                matches!(e, egui::Event::Key { key: egui::Key::Space, modifiers, .. } if modifiers.command)
            });
            (!ctrl && !i.modifiers.command && !i.modifiers.alt && i.key_pressed(egui::Key::Space))
                .then_some(i.modifiers.shift)
        });
        if let Some(hand) = flip.filter(|_| self.plan.modal.is_none()) {
            if self.plan.tool == Tool::Wall && !hand {
                self.wall_action("wall.flipnew", "");
            } else if !self.plan.busy() {
                self.flip_selected(hand);
            }
        }
        let (settings, shortcuts) = ctx.input(|i| {
            let cmd = i.modifiers.command;
            (
                cmd && i.key_pressed(egui::Key::Comma),
                cmd && i.key_pressed(egui::Key::Slash),
            )
        });
        if settings {
            self.settings_open = !self.settings_open;
        }
        if ctx.input(|i| i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::N)) {
            self.open_new_project(ctx);
        }
        if shortcuts {
            self.open_help("shortcuts");
        }
        let (palette, next_theme, home, frame_sel) = ctx.input(|i| {
            let (cmd, shift) = (i.modifiers.command, i.modifiers.shift);
            (
                cmd && i.key_pressed(egui::Key::K),
                cmd && shift && i.key_pressed(egui::Key::T),
                i.key_pressed(egui::Key::Home),
                !cmd && i.key_pressed(egui::Key::Period),
            )
        });
        if palette && self.shell.web.is_some() {
            self.open_palette(ctx);
        }
        if next_theme {
            let t = self.shell.theme.next();
            self.set_theme(ctx, t);
        }
        if self.plan.modal.is_none() {
            if home {
                self.frame_pane(self.last_in_2d);
            }
            if frame_sel {
                self.frame_selection_here();
            }
        }
        // in a text field, Ctrl+A and the letter keys are the field's own
        let typing = ctx.egui_wants_keyboard_input();
        let (all, none, hide, isolate, show, events) = ctx.input(|i| {
            let (cmd, alt, shift) = (i.modifiers.command, i.modifiers.alt, i.modifiers.shift);
            let h = !typing && i.key_pressed(egui::Key::H);
            let a = !typing && i.key_pressed(egui::Key::A);
            (
                cmd && !shift && a,
                alt && a,
                h && !cmd && !alt && !shift,
                h && shift && !cmd && !alt,
                h && alt,
                i.events.clone(),
            )
        });
        let (open, save) = ctx.input(|i| {
            let cmd = i.modifiers.command;
            (
                cmd && i.key_pressed(egui::Key::O),
                cmd && i.key_pressed(egui::Key::S),
            )
        });
        if open {
            self.open_dialog();
        }
        if save {
            self.save();
        }
        if all {
            self.select_all();
        }
        // Shift+A: all of the selection's type in the view; Ctrl+Shift+A
        // in the project
        let similar = ctx.input(|i| {
            let m = i.modifiers;
            (!typing && m.shift && !m.alt && i.key_pressed(egui::Key::A)).then_some(m.command)
        });
        if let Some(project) =
            similar.filter(|_| self.plan.tool == Tool::Select && !self.selection.is_empty())
        {
            self.shell.context_3d = !self.last_in_2d;
            self.select_similar(if project { "type.project" } else { "type.view" });
        }
        if none {
            self.selection.clear();
            self.face = None;
        }
        if hide {
            self.hide_selected();
        }
        if isolate {
            self.isolate_selected();
        }
        if show {
            self.show_all();
        }
        // Ctrl+Shift+C and Ctrl+Shift+V come as copy and paste: with Shift
        // they pick up and apply settings instead
        let shifted = ctx.input(|i| i.modifiers.shift);
        for e in &events {
            match e {
                egui::Event::Copy if shifted => self.pick_up_settings(),
                egui::Event::Paste(_) if shifted => self.apply_settings(),
                egui::Event::Copy => self.run_command(ctx, Command::Copy, None),
                egui::Event::Cut => self.run_command(ctx, Command::Cut, None),
                egui::Event::Paste(text) => self.paste_text(text),
                _ => {}
            }
        }
        if undo {
            self.run_command(ctx, Command::Undo, None);
        }
        if redo {
            self.run_command(ctx, Command::Redo, None);
        }
        if delete {
            self.run_command(ctx, Command::Delete, None);
        }
        if extrude {
            let sketch = self
                .selection
                .active
                .and_then(|id| self.doc.model().get(id))
                .is_some_and(|e| matches!(e.kind, ElementKind::Sketch(_)));
            if sketch {
                self.extrude_sketch();
            } else {
                self.extrude_face();
            }
        }
        self.drop_stale_selection();
    }

    fn drop_stale_selection(&mut self) {
        let before = self.selection.active;
        let model = self.doc.model();
        self.selection.retain(|id| model.get(id).is_some());
        if self.selection.active != before {
            self.face = None;
        }
        if self
            .active_view
            .is_none_or(|id| self.view(id).is_none() && !self.is_sheet(id))
        {
            self.active_view = self.views().first().map(|v| v.0);
        }
    }

    fn delete_selected(&mut self) {
        let ids = self.selection.all();
        let Some(&last) = ids.last() else {
            return;
        };
        // a level asks first what happens to what is on it
        if let Some(level) = ids.iter().find(|id| self.doc.model().level(**id).is_some()) {
            self.ask_delete_level(*level);
            return;
        }
        let name = match ids.len() {
            1 => self
                .doc
                .model()
                .get(last)
                .map(|e| e.name.clone())
                .unwrap_or_default(),
            n => format!("{n} elements"),
        };
        // from a later phase, what was built earlier is taken down
        if let Some(f) = self.phase_front() {
            self.work_in_phase(f);
        }
        if self.delete_in_phase(&ids, &name).is_some() {
            self.selection.retain(|_| false);
            self.face = None;
            return;
        }
        let tx =
            Transaction::new(format!("Delete {name}")).remove_all_cascade(self.doc.model(), &ids);
        // with phases, a delete in the phase a thing was built in is a
        // real delete: say how to take it down instead (MU100: deleted at
        // Existing, the owner looked for red dashes that never came)
        let hint = self.phase_delete_hint();
        let r = self.doc.commit(tx);
        self.report(
            r,
            format!("Deleted {name} and anything attached to it.{hint}"),
        );
        self.drop_stale_selection();
    }

    /// Everything that can be picked: building elements, sketches, guides,
    /// annotations, but not levels, views, sheets or materials.
    /// Select what is not selected, and drop what is (MU39).
    pub(super) fn select_invert(&mut self) {
        let was = self.selection.clone();
        let ids: Vec<ElementId> = self
            .selectable_ids()
            .into_iter()
            .filter(|id| !was.contains(*id))
            .collect();
        self.selection.extend(ids, false);
        self.face = None;
        self.status = format!("Inverted: {} selected.", self.selection.len());
    }

    /// Add what is joined to the selection: walls meeting it end to end,
    /// and the doors, windows and holes set in it (MU39).
    pub(super) fn select_linked(&mut self) {
        let model = self.doc.model();
        let mut add = vec![];
        for id in self.selection.all() {
            add.extend(planview::joined_with(&self.doc, id));
            for e in model.iter() {
                if let ElementKind::Opening(o) = &e.kind {
                    if o.host == id {
                        add.push(e.id);
                    }
                }
            }
        }
        let before = self.selection.len();
        self.selection.extend(add, true);
        self.status = format!(
            "Linked: {} added, {} selected.",
            self.selection.len() - before,
            self.selection.len()
        );
    }

    /// Keep about half of the selection, picked at random (MU39).
    pub(super) fn select_random(&mut self) {
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64)
            | 1;
        let keep: Vec<ElementId> = self
            .selection
            .all()
            .into_iter()
            .filter(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed.is_multiple_of(2)
            })
            .collect();
        self.selection.extend(keep, false);
        self.status = format!("Random: {} kept.", self.selection.len());
    }

    /// Select every element of one kind (walls, doors…), as Blender's
    /// Select All by Type (MU39).
    pub(super) fn select_kind(&mut self, kind: &str) {
        let model = self.doc.model();
        let ids: Vec<ElementId> = self
            .selectable_ids()
            .into_iter()
            .filter(|id| model.get(*id).is_some_and(|e| kind_word(&e.kind) == kind))
            .collect();
        self.selection.extend(ids, false);
        self.face = None;
        self.status = format!("{} selected.", self.selection.len());
    }

    /// Everything Select all takes.
    pub(super) fn selectable_ids(&self) -> Vec<ElementId> {
        self.doc
            .model()
            .iter()
            .filter(|e| {
                !self.hidden.contains(&e.id)
                    && !matches!(
                        e.kind,
                        ElementKind::Level(_)
                            | ElementKind::View(_)
                            | ElementKind::Sheet(_)
                            | ElementKind::Viewport(_)
                            | ElementKind::Material(_)
                            | ElementKind::Units(_)
                            | ElementKind::Project(_)
                            | ElementKind::SheetTemplate(_)
                            | ElementKind::CropBox(_)
                    )
            })
            .map(|e| e.id)
            .collect()
    }

    fn select_all(&mut self) {
        let ids = self.selectable_ids();
        self.selection.extend(ids, false);
        self.face = None;
        self.status = format!("{} selected.", self.selection.len());
    }

    fn copy_selected(&mut self) -> bool {
        let ids = self.selection.all();
        if ids.is_empty() {
            self.status = "Select something to copy first.".into();
            return false;
        }
        let model = self.doc.model();
        self.clipboard = creabim_core::transform::copy_set(model, &ids)
            .into_iter()
            .filter_map(|id| model.get(id).cloned())
            .collect();
        true
    }

    /// The clipboard as text, so a paste can reach another CreaBIM window.
    fn clipboard_text(&self) -> String {
        serde_json::json!({ "creabim_elements": self.clipboard }).to_string()
    }

    /// Paste copies of `elements` in place, as one undo step.
    fn paste(&mut self, elements: Vec<Element>) {
        if elements.is_empty() {
            self.status = "Nothing to paste: copy something first (Ctrl+C).".into();
            return;
        }
        let (copies, dropped) = creabim_core::transform::fresh_copies(self.doc.model(), &elements);
        if copies.is_empty() {
            self.status = "Nothing could be pasted here: what was copied needs elements this project doesn't have.".into();
            return;
        }
        let ids: Vec<ElementId> = copies.iter().map(|c| c.id).collect();
        let n = copies.len();
        let mut tx = Transaction::new(format!("Paste {n} elements"));
        for c in copies {
            tx = tx.insert(c);
        }
        let r = self.doc.commit(tx);
        if r.is_ok() {
            self.selection.extend(ids, false);
            self.face = None;
        }
        let left = match dropped {
            0 => String::new(),
            d => format!(" {d} left out: they hang on elements this project doesn't have."),
        };
        self.report(
            r,
            format!("Pasted {n} elements in place. Press G to move them.{left}"),
        );
    }

    /// Paste from text another window (or this one) put on the clipboard.
    fn paste_text(&mut self, text: &str) {
        let parsed = serde_json::from_str::<serde_json::Value>(text)
            .ok()
            .and_then(|v| v.get("creabim_elements").cloned())
            .and_then(|v| serde_json::from_value::<Vec<Element>>(v).ok());
        let elements = parsed.unwrap_or_else(|| self.clipboard.clone());
        self.paste(elements);
    }

    fn hide_selected(&mut self) {
        let ids = self.selection.all();
        if ids.is_empty() {
            return;
        }
        let n = ids.len();
        // a wall's doors and windows go with it
        let with = creabim_core::transform::copy_set(self.doc.model(), &ids);
        self.hidden.extend(with);
        self.hidden_rev += 1;
        self.selection.clear();
        self.face = None;
        self.status = format!("Hid {n}. Alt+H shows everything again.");
    }

    /// Hide everything but the selection.
    fn isolate_selected(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        let keep = self.selection.all();
        // keep what the selection needs to show: hosts, levels, cutters
        let model = self.doc.model();
        let mut needed = keep.clone();
        for id in &keep {
            if let Some(e) = model.get(*id) {
                needed.extend(e.references());
            }
            needed.extend(model.dependents(*id));
        }
        self.hidden = model
            .iter()
            .map(|e| e.id)
            .filter(|id| !needed.contains(id))
            .collect();
        self.hidden_rev += 1;
        self.status = format!("Showing only {}. Alt+H shows everything again.", keep.len());
    }

    fn show_all(&mut self) {
        let n = self.hidden.len();
        self.hidden.clear();
        self.hidden_rev += 1;
        self.status = match n {
            0 => "Nothing was hidden.".into(),
            n => format!("Showing {n} hidden elements again."),
        };
    }

    fn extrude_face(&mut self) {
        let (Some(id), Some(face)) = (self.selection.active, self.face) else {
            return;
        };
        let d = self.extrude_distance;
        let r = self.doc.edit(id, "Extrude face", |e| {
            if let ElementKind::FreeMesh(m) = &mut e.kind {
                if face < m.mesh.faces.len() {
                    m.mesh.extrude_face(face, d);
                }
            }
        });
        self.report(
            r,
            format!(
                "Extruded face {face} by {}. Press E again to keep going.",
                crate::units::len(d)
            ),
        );
    }

    fn extrude_sketch(&mut self) {
        let Some(id) = self.selection.active else {
            return;
        };
        let d = self.extrude_distance;
        match crate::sketch::extrude(&mut self.doc, id, d) {
            Ok((mass, cap)) => {
                self.selection.only(mass);
                self.face = cap;
                self.status = match cap {
                    Some(_) => format!(
                        "Extruded the sketch {} into a mass. Its far face is selected: press E to push it further.", crate::units::len(d)
                    ),
                    None => format!(
                        "Extruded the open sketch {} into a surface. Add a Solidify modifier to give it thickness.", crate::units::len(d)
                    ),
                };
            }
            Err(e) => self.status = e,
        }
    }

    // ---- 3D -------------------------------------------------------------

    /// What the 3D view's buffers are built from, with the scene of
    /// revision `scene_rev`: they are built again when it changes.
    fn geometry_key_at(&self, scene_rev: u64) -> GeometryKey {
        (
            (scene_rev, self.hidden_rev),
            Selection::default(),
            None,
            self.plan_in_3d,
            self.active_view,
            creabim_core::cropbox::crop_boxes(self.doc.model(), self.crop_owner())
                .into_iter()
                .map(|(_, c)| c.clone())
                .collect(),
        )
    }

    fn rebuild_geometry(&mut self, rs: &egui_wgpu::RenderState) {
        // the grid lines are built with the model's buffers: a grid moved,
        // stretched or recoloured builds them again (MU74)
        let gk = self.grid_lines_3d_key();
        if gk != self.grid_lines_key {
            self.grid_lines_key = gk;
            self.geometry_key = None;
        }
        // the scene of this revision: shared if built, else the newest a
        // worker has finished while it builds this one, so an edit or a
        // drag never waits for the 3D view (A6-1.2); the first is built here
        let model = self.doc.model();
        let shown = self.geometry_key.as_ref().map(|k| k.0 .0);
        // what is built here is the scene's own: a view's look, or a note
        // on a sheet, changes none of it (a click on a drawing style
        // rebuilt these buffers, 18 ms). With the drawing shown in 3D it
        // is part of what is built, so then any change counts
        let now = if self.plan_in_3d {
            model.revision()
        } else {
            model.scene_revision()
        };
        // a shape that stands alone, following the pointer: built again by
        // itself into the scene as it was, here and now
        let patched = || {
            let (ids, base) = self.live_base.as_ref()?;
            let (now, _) = self.live_edit()?;
            let alone = !ids.is_empty()
                && *ids == now
                && ids.iter().all(|id| creabim_geom::stands_alone(model, *id));
            alone.then(|| {
                let _pp = Perf::new("3d rebuild: one shape");
                let changed: std::collections::HashSet<ElementId> = ids.iter().copied().collect();
                creabim_geom::shared_scene_patched(model, base, &changed)
            })
        };
        let ready = creabim_geom::shared_scene_ready(model).or_else(patched);
        let (scene_rev, scene) = match ready {
            Some(s) => (now, s),
            None if !self.builds.enabled() || now == 0 => (now, creabim_geom::shared_scene(model)),
            None if shown.is_none() => {
                // nothing shown yet (a project just opened): its walls
                // and slabs at once, not yet cut into each other, while
                // the worker makes the cuts (Highland Haven: 88 ms, not
                // 18 s of a window not responding). Shown as of the
                // revision before, so the joined scene replaces it
                self.builds.start_scene(model);
                self.builds.poll();
                match self.builds.latest_scene() {
                    Some((r, s)) if r == now => (r, s),
                    _ => (now - 1, creabim_geom::scene::shared_unjoined_scene(model)),
                }
            }
            None => {
                self.builds.start_scene(model);
                self.builds.poll();
                match self.builds.latest_scene() {
                    Some((r, s)) if shown.is_none_or(|v| r > v) => (r, s),
                    _ => return,
                }
            }
        };
        // the model's buffers, built again only when what is shown changes;
        // the selection is drawn over them by its own small pass
        // (rebuild_selection), so a click builds nothing here (MU39)
        let key = self.geometry_key_at(scene_rev);
        if self.geometry_key.as_ref() == Some(&key) {
            return;
        }
        self.geometry_key = Some(key);
        self.geometry_gen += 1;
        let _perf = Perf::new("3d rebuild");
        let _pc = Perf::new("3d rebuild: copy and crop");
        // at the phase the 3D view stands at: what is not there then is
        // out of the model the scene is built from, so a wall has no hole
        // for a door yet to come
        self.scene = match creabim_core::phases::model_at(
            model,
            self.phase3d,
            &self.options3d,
            &self.ghost3d,
        ) {
            Some(m) => (*creabim_geom::scene::shared_scene(&m)).clone(),
            None => (*scene).clone(),
        };
        let left_out = self.hidden_in_3d();
        self.scene
            .retain(|i| !self.hidden.contains(&i.id) && !left_out.contains(&i.id));
        // what was there before reads paler; what comes down, red
        let reads =
            creabim_core::phases::statuses(model, self.phase3d, &self.options3d, &self.ghost3d);
        if !reads.is_empty() {
            let look = creabim_core::phases::look(model);
            for i in &mut self.scene {
                match reads.get(&i.id) {
                    Some(creabim_core::phases::Status::Existing) if look.existing_faint => {
                        i.color = i.color.map(|c| c + (0.88 - c) * 0.6);
                    }
                    Some(creabim_core::phases::Status::TakenDown) => {
                        i.color = look.down;
                        i.surface[2] = i.surface[2].min(0.6);
                    }
                    Some(creabim_core::phases::Status::New) => {
                        if let Some(c) = look.new {
                            i.color = c;
                        }
                    }
                    Some(creabim_core::phases::Status::Ghost) => {
                        i.color = creabim_geom::looks::GHOST_BLUE;
                        i.surface[2] = i.surface[2].min(0.28);
                    }
                    _ => {}
                }
            }
        }
        // the ground before its works, or as it was see-through over them
        let ground = self.shell.settings.ground_3d;
        creabim_geom::siteviews::scene_as_shown(model, &mut self.scene, ground);
        // a property's envelope and its lines, when the 3D view leaves
        // them out
        let (env_off, lines_off) = (
            self.shell.settings.envelope_3d_off,
            self.shell.settings.lot_lines_3d_off,
        );
        if env_off || lines_off {
            self.scene.retain(|i| {
                let lot = matches!(
                    model.get(i.id).map(|e| &e.kind),
                    Some(ElementKind::Site(s)) if s.is_property()
                );
                let line = i.finish == Some(creabim_geom::scene::LOT_LINES);
                !(lot && ((line && lines_off) || (!line && env_off)))
            });
        }
        if ground == creabim_core::GroundState::Both {
            let mut ghosts = creabim_geom::siteviews::ground_ghosts(model, &self.scene);
            // as solid as the 3D view's slider says
            let solid = self.shell.settings.ground_3d_was.clamp(0.05, 0.95);
            for g in &mut ghosts {
                g.surface[2] = solid;
            }
            self.scene.extend(ghosts);
        }
        let cut_edges = self.crop_scene_3d();
        drop(_pc);
        snaps::begin_edge_round();
        let _pi = Perf::new("3d rebuild: items");

        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut see_through = Vec::new();
        // the selection in the interaction colour every view and grip uses
        // (A7-2): a tint over the element's own colour and a crisp
        // outline, never a repaint
        let highlight = {
            let a = crate::planview::accent_selected();
            [
                a.r() as f32 / 255.0,
                a.g() as f32 / 255.0,
                a.b() as f32 / 255.0,
            ]
        };
        let tint = |c: [f32; 3]| {
            [0, 1, 2].map(|k| c[k] * (1.0 - SELECTION_TINT) + highlight[k] * SELECTION_TINT)
        };
        let mut ghost_lines: Vec<(DVec3, DVec3, [f32; 3])> = Vec::new();
        // what is selected of the cutters and shafts shows through everything
        let mut on_top: Vec<(DVec3, DVec3, [f32; 3])> = Vec::new();
        let xray = self.shell.settings.xray;
        let mode = self.shell.settings.render_3d;
        // wireframe on a dark theme: light wires, as Blender's (MU31: dark
        // wires on the dark ground all but vanished)
        let edge_colour = if mode == crate::render3d::RenderMode::Wireframe && self.dark_3d {
            [0.72, 0.74, 0.78]
        } else {
            [0.10, 0.10, 0.11]
        };
        // a type shown in the model: its instances lit as if selected, the
        // rest see-through when asked
        let spot = &self.plan.spotlight;
        let ghost = self.plan.spotlight_ghost && !spot.is_empty();
        // while one shape that stands alone follows the pointer, all the
        // rest is the same from step to step: built once and kept, so a
        // step builds the moving shape only (MU86: 20 ms a step went on
        // building the whole house again)
        let live_key: Option<(String, Vec<ElementId>)> =
            self.live_base.as_ref().and_then(|(ids, base)| {
                let (now_ids, _) = self.live_edit()?;
                let alone = !ids.is_empty()
                    && *ids == now_ids
                    && ids
                        .iter()
                        .all(|id| creabim_geom::stands_alone(self.doc.model(), *id));
                // not with the drawing shown in 3D, what is buried shown in
                // colour, or a type lit: those change with the moving shape
                let plain =
                    !self.plan_in_3d && !self.shell.settings.hidden_colour_3d && spot.is_empty();
                (alone && plain).then(|| {
                    (
                        format!(
                            "{:p}/{:?}/{:?}/{}/{}/{}/{}/{:?}/{:?}/{}/{:?}",
                            std::sync::Arc::as_ptr(base),
                            self.hidden_rev,
                            mode,
                            xray,
                            self.dark_3d,
                            spot.len(),
                            ghost,
                            self.active_view,
                            highlight,
                            cut_edges.len(),
                            ground,
                        ),
                        ids.clone(),
                    )
                })
            });
        let hit = self
            .static_3d
            .take()
            .is_some_and(|k| live_key.as_ref().is_some_and(|(key, _)| *key == k));
        if hit {
            snaps::keep_all_edges();
        }
        // everything but the moving shape, set aside after the first pass
        let mut rest = None;
        for pass in 0..2 {
            if pass == 0 && hit {
                continue;
            }
            for item in &self.scene {
                let is_live = live_key
                    .as_ref()
                    .is_some_and(|(_, ids)| ids.contains(&item.id));
                if is_live != (pass == 1) {
                    continue;
                }
                let selected = spot.contains(&item.id);
                // X-ray: what holds others turns see-through with its edges
                // drawn; the framing that sits in it stays solid
                let xrayed = !selected
                    && (ghost
                        || xray
                            && !self
                                .doc
                                .model()
                                .get(item.id)
                                .is_some_and(|e| is_framing(&e.kind)));
                if item.ghost {
                    // cutters show as their outline only: dashed red, as in the
                    // plans (MU71: a thin blue line read as part of the model);
                    // a stair's headroom shortfall in its own red
                    let c = if selected {
                        highlight
                    } else if item.finish == Some("Headroom") {
                        item.color
                    } else {
                        [0.84, 0.25, 0.20]
                    };
                    let out = if selected {
                        &mut on_top
                    } else {
                        &mut ghost_lines
                    };
                    for &(a, b) in snaps::feature_edges_cached(&item.mesh).iter() {
                        if selected {
                            out.push((a, b, c));
                            continue;
                        }
                        let len = a.distance(b);
                        let (dash, gap) = (0.12, 0.08);
                        let mut t = 0.0;
                        while t < len {
                            let e = (t + dash).min(len);
                            out.push((a + (b - a) * (t / len), a + (b - a) * (e / len), c));
                            t = e + gap;
                        }
                    }
                    continue;
                }
                // a selected element is outlined whatever the shading shows
                // wireframe draws every wire, X-ray or not, as Blender's (MU31:
                // X-ray left only the framing standing)
                let wire = mode == crate::render3d::RenderMode::Wireframe;
                if (mode.edges() || selected) && (!xrayed || wire) {
                    let c = if selected { highlight } else { edge_colour };
                    for &(a, b) in snaps::feature_edges_cached(&item.mesh).iter() {
                        ghost_lines.push((a, b, c));
                    }
                }
                if mode == crate::render3d::RenderMode::Wireframe {
                    continue;
                }
                let base = vertices.len() as u32;
                let face_tris = match (
                    false,
                    self.face,
                    self.doc.model().get(item.id).map(|e| &e.kind),
                ) {
                    (true, Some(f), Some(ElementKind::FreeMesh(m))) => {
                        // the face and its copies made by modifiers
                        let ev = m.evaluated();
                        let tris: Vec<usize> = (0..ev.mesh.faces.len())
                            .filter(|&i| ev.source[i] == Some(f))
                            .flat_map(|i| face_triangles(&ev.mesh, i))
                            .collect();
                        Some(tris)
                    }
                    _ => None,
                };
                let plain = if mode == crate::render3d::RenderMode::HiddenLine {
                    [0.97, 0.97, 0.96]
                } else {
                    item.color
                };
                let mut colors =
                    vec![if selected { tint(plain) } else { plain }; item.mesh.positions.len()];
                if let Some(tris) = &face_tris {
                    for &t in tris {
                        for k in 0..3 {
                            // a cut mesh no longer matches its faces one to one
                            if let Some(&v) = item.mesh.indices.get(t * 3 + k) {
                                colors[v as usize] = highlight;
                            }
                        }
                    }
                }
                // a selected element keeps its own surface (glass stays glass)
                let surface = if xrayed {
                    [
                        item.surface[0],
                        item.surface[1],
                        XRAY_OPACITY.min(item.surface[2]),
                    ]
                } else {
                    item.surface
                };
                if xrayed {
                    for &(a, b) in snaps::feature_edges_cached(&item.mesh).iter() {
                        ghost_lines.push((a, b, XRAY_EDGE));
                    }
                }
                for (i, p) in item.mesh.positions.iter().enumerate() {
                    vertices.push(Vertex {
                        pos: *p,
                        normal: item.mesh.normals[i],
                        color: colors[i],
                        surface,
                    });
                }
                let target = if surface[2] < 0.999 {
                    &mut see_through
                } else {
                    &mut indices
                };
                target.extend(item.mesh.indices.iter().map(|i| i + base));
            }
            if pass == 0 && live_key.is_some() {
                rest = Some((
                    std::mem::take(&mut vertices),
                    std::mem::take(&mut indices),
                    std::mem::take(&mut see_through),
                    std::mem::take(&mut ghost_lines),
                ));
            }
        }
        // the moving shape's own buffers, apart from the rest
        let mut moving = (vec![], vec![], vec![], vec![]);
        if hit || rest.is_some() {
            let lines: Vec<Vertex> = ghost_lines
                .drain(..)
                .flat_map(|(a, b, c)| {
                    [
                        line_vertex(a.as_vec3().to_array(), c),
                        line_vertex(b.as_vec3().to_array(), c),
                    ]
                })
                .collect();
            moving = (
                std::mem::take(&mut vertices),
                std::mem::take(&mut indices),
                std::mem::take(&mut see_through),
                lines,
            );
        }
        if let Some((v, i, s, g)) = rest {
            vertices = v;
            indices = i;
            see_through = s;
            ghost_lines = g;
        }
        self.static_3d = live_key.map(|(k, _)| k);
        if hit {
            // the rest is on the card already: send the moving shape only
            if let Some(vp) = &mut self.viewport {
                vp.set_moving(&rs.device, &moving.0, &moving.1, &moving.2, &moving.3);
            }
            return;
        }

        drop(_pi);
        let _pl = Perf::new("3d rebuild: lines and upload");
        // the working grid, 1 m spacing, just under the workplane or the level
        // the 3D tools draw on, so it stays put whatever the model's lowest
        // point is
        let level_plane = Workplane::ground(self.work_level().map_or(0.0, |(_, z)| z));
        let spacing = ground_spacing(self.shell.settings.ground_grid_spacing);
        let grid_lines = |plane: Workplane, lo: f64, hi: f64, step: f64, color: [f32; 3]| {
            let on = |u: f64, v: f64| -> [f32; 3] {
                (plane.to_model(DVec2::new(u, v)) - plane.normal() * 0.002)
                    .as_vec3()
                    .to_array()
            };
            let mut out = Vec::new();
            let (first, last) = ((lo / step).ceil() as i32, (hi / step).floor() as i32);
            for i in first..=last {
                let v = i as f64 * step;
                for (a, b) in [((v, lo), (v, hi)), ((lo, v), (hi, v))] {
                    out.push(line_vertex(on(a.0, a.1), color));
                    out.push(line_vertex(on(b.0, b.1), color));
                }
            }
            out
        };
        let mut lines = Vec::new();
        // where crop boxes cut, the outlines of the cuts
        for (a, b) in &cut_edges {
            lines.push(line_vertex(a.as_vec3().to_array(), [0.18, 0.16, 0.15]));
            lines.push(line_vertex(b.as_vec3().to_array(), [0.18, 0.16, 0.15]));
        }
        // the crop boxes' frames, with the model's depth: what stands in
        // front of a frame hides it
        for (a, b, c) in self.crop_frame_lines() {
            lines.push(line_vertex(a.as_vec3().to_array(), c));
            lines.push(line_vertex(b.as_vec3().to_array(), c));
        }
        for (a, b, c) in &ghost_lines {
            lines.push(line_vertex(a.as_vec3().to_array(), *c));
            lines.push(line_vertex(b.as_vec3().to_array(), *c));
        }
        // one plane at a time (MU18): while a face is the workplane, the
        // ground grid dims to a hint
        let face_plane = self.workplane.is_some();
        if self.shell.settings.ground_grid {
            // as far out as 50 lines each way, at least the old 50 m
            let reach = (spacing * 25.0).max(25.0);
            // quiet: a little off the paper, so edges and vertex dots
            // stand well clear of it (MU39: too strong)
            let color = match (self.paper_is_dark, face_plane) {
                (false, true) => [0.91, 0.92, 0.93],
                (false, false) => [0.865, 0.865, 0.88],
                (true, true) => [0.165, 0.17, 0.185],
                (true, false) => [0.215, 0.22, 0.245],
            };
            // the grid is left out under ground that reaches its plane: a
            // terrain lying exactly on it would show it through, checkered
            let plane_z = level_plane.to_model(DVec2::ZERO).z;
            let shown: std::collections::HashSet<ElementId> =
                self.scene.iter().map(|i| i.id).collect();
            let model = self.doc.model();
            let grounds: Vec<Vec<DVec2>> = model
                .iter()
                .filter(|e| shown.contains(&e.id))
                .filter_map(|e| match &e.kind {
                    ElementKind::Site(st)
                        if matches!(st.form, creabim_core::SiteForm::Terrain { .. }) =>
                    {
                        let (_, hi) = st.bounds(model)?;
                        (hi.z >= plane_z - 0.05)
                            .then(|| st.boundary().map(|b| b.to_vec()))
                            .flatten()
                    }
                    _ => None,
                })
                .collect();
            if grounds.is_empty() {
                lines.extend(grid_lines(
                    level_plane,
                    -reach + 5.0,
                    reach + 5.0,
                    spacing,
                    color,
                ));
            } else {
                // cell by cell, so only the stretches under the ground go
                let (lo, hi) = (-reach + 5.0, reach + 5.0);
                let (first, last) = ((lo / spacing).ceil() as i32, (hi / spacing).floor() as i32);
                let at = |u: f64, v: f64| -> [f32; 3] {
                    (level_plane.to_model(DVec2::new(u, v)) - level_plane.normal() * 0.002)
                        .as_vec3()
                        .to_array()
                };
                let under = |u: f64, v: f64| {
                    let q = level_plane.to_model(DVec2::new(u, v)).truncate();
                    grounds
                        .iter()
                        .any(|b| creabim_core::polygon::contains(b, q))
                };
                for i in first..=last {
                    let v = i as f64 * spacing;
                    for j in first..last {
                        let (a, b) = (j as f64 * spacing, (j + 1) as f64 * spacing);
                        let m = (a + b) / 2.0;
                        if !under(v, m) {
                            lines.push(line_vertex(at(v, a), color));
                            lines.push(line_vertex(at(v, b), color));
                        }
                        if !under(m, v) {
                            lines.push(line_vertex(at(a, v), color));
                            lines.push(line_vertex(at(b, v), color));
                        }
                    }
                }
            }
            // the model's X and Y axes through its origin, red and green as
            // Blender's (MU39); Z shows only while a move keeps to it
            if self.shell.settings.axis_lines && !face_plane {
                let z = level_plane.to_model(DVec2::ZERO).z as f32 + 0.001;
                let (lo, hi) = ((-reach + 5.0) as f32, (reach + 5.0) as f32);
                let (red, green) = if self.paper_is_dark {
                    ([0.78, 0.30, 0.30], [0.42, 0.66, 0.30])
                } else {
                    ([0.86, 0.36, 0.36], [0.45, 0.70, 0.35])
                };
                lines.push(line_vertex([lo, 0.0, z], red));
                lines.push(line_vertex([hi, 0.0, z], red));
                lines.push(line_vertex([0.0, lo, z], green));
                lines.push(line_vertex([0.0, hi, z], green));
            }
        }
        if let Some((plane, _)) = self.workplane {
            let bg = if self.paper_is_dark {
                BACKGROUND_3D_DARK
            } else {
                BACKGROUND_3D
            };
            lines.extend(workplane_lines(plane, spacing, WORKPLANE_FADE, bg));
        }
        lines.extend(self.grid_lines_3d());

        // 2D drawing shown in 3D space, on its level
        let mut overlay = Vec::new();
        if self.plan_in_3d {
            let placed = self.active_view.and_then(|id| {
                let kind = self.view(id)?.kind.clone();
                Some((kind, self.drawing_live(id)?))
            });
            if let Some((kind, plan)) = placed {
                // drawing coordinates → world: plans lie flat at their cut
                // height, sections stand on their section line
                let to_world: Option<Box<dyn Fn(DVec2) -> [f32; 3]>> = match kind {
                    ViewKind::Plan { level, cut_height } => {
                        let z = self
                            .doc
                            .model()
                            .level(level)
                            .map_or(0.0, |l| l.elevation + cut_height)
                            as f32;
                        Some(Box::new(move |p| [p.x as f32, p.y as f32, z]))
                    }
                    ViewKind::Section { start, end, .. } => {
                        let along = (end - start).normalize_or_zero();
                        Some(Box::new(move |p| {
                            let w = start + along * p.x;
                            [w.x as f32, w.y as f32, p.y as f32]
                        }))
                    }
                    // 3D views and schedules have no place of their own in the model
                    ViewKind::Axonometric { .. }
                    | ViewKind::Schedule { .. }
                    | ViewKind::LongSection { .. }
                    | ViewKind::BoreholeLog { .. }
                    | ViewKind::WalkSection { .. } => None,
                };
                let to_world = to_world.unwrap_or_else(|| Box::new(|_| [0.0; 3]));
                let plan = if matches!(
                    kind,
                    ViewKind::Axonometric { .. }
                        | ViewKind::Schedule { .. }
                        | ViewKind::LongSection { .. }
                        | ViewKind::BoreholeLog { .. }
                        | ViewKind::WalkSection { .. }
                ) {
                    Drawing::default()
                } else {
                    plan
                };
                let mut seg = |a: DVec2, b: DVec2, c: [f32; 3]| {
                    overlay.push(line_vertex(to_world(a), c));
                    overlay.push(line_vertex(to_world(b), c));
                };
                for it in plan.items.iter().filter(|it| !left_out.contains(&it.id())) {
                    let c = match it {
                        PlanItem::Polygon { pen: Pen::Cut, .. }
                        | PlanItem::Line { pen: Pen::Cut, .. } => [0.85, 0.15, 0.10],
                        _ => [0.15, 0.25, 0.55],
                    };
                    match it {
                        PlanItem::Polygon { points, .. } => {
                            for i in 0..points.len() {
                                seg(points[i], points[(i + 1) % points.len()], c);
                            }
                        }
                        // guides show as their own lines in 3D; underlays stay in the plan
                        PlanItem::Line {
                            pen: Pen::Guide | Pen::Underlay,
                            ..
                        }
                        | PlanItem::Arc {
                            pen: Pen::Underlay, ..
                        } => {}
                        PlanItem::Line { a, b, .. } => seg(*a, *b, c),
                        PlanItem::Arc {
                            center,
                            radius,
                            start,
                            end,
                            ..
                        } => {
                            let n = 16;
                            for i in 0..n {
                                let t0 = start + (end - start) * i as f64 / n as f64;
                                let t1 = start + (end - start) * (i + 1) as f64 / n as f64;
                                seg(
                                    *center + DVec2::from_angle(t0) * *radius,
                                    *center + DVec2::from_angle(t1) * *radius,
                                    c,
                                );
                            }
                        }
                        PlanItem::Dimension { a, b, offset, .. } => {
                            let n = (*b - *a).normalize_or_zero().perp() * *offset;
                            seg(*a + n, *b + n, [0.12, 0.24, 0.47]);
                        }
                        PlanItem::Label { .. }
                        | PlanItem::Symbol { .. }
                        | PlanItem::Fill { .. }
                        | PlanItem::Text { .. }
                        | PlanItem::Tag { .. }
                        | PlanItem::Annotation { .. }
                        | PlanItem::Tint { .. }
                        | PlanItem::Room { .. }
                        | PlanItem::Shade { .. }
                        | PlanItem::Image { .. } => {}
                    }
                }
            }
        }

        // what is inside walls, slabs and roofs, in its colours where they hide it
        let behind = if self.shell.settings.hidden_colour_3d {
            self.buried_vertices()
        } else {
            vec![]
        };
        for (a, b, c) in &on_top {
            overlay.push(line_vertex(a.as_vec3().to_array(), *c));
            overlay.push(line_vertex(b.as_vec3().to_array(), *c));
        }
        if let Some(vp) = &mut self.viewport {
            vp.set_geometry(
                &rs.device,
                &vertices,
                &indices,
                &see_through,
                &lines,
                &overlay,
            );
            vp.set_behind(&rs.device, &behind);
            vp.set_moving(&rs.device, &moving.0, &moving.1, &moving.2, &moving.3);
        }
    }

    /// The selection's highlight over the model's buffers: its triangles
    /// tinted, the face picked in the accent, its edges, a selected
    /// cutter's outline through everything. Only the selection is built.
    fn rebuild_selection(&mut self, rs: &egui_wgpu::RenderState) {
        let key = (self.geometry_gen, self.selection.clone(), self.face);
        if self.selection_key.as_ref() == Some(&key) {
            return;
        }
        self.selection_key = Some(key);
        let _perf = Perf::new("3d selection");
        let a = crate::planview::accent_selected();
        let highlight = [
            a.r() as f32 / 255.0,
            a.g() as f32 / 255.0,
            a.b() as f32 / 255.0,
        ];
        let tint = |c: [f32; 3]| {
            [0, 1, 2].map(|k| c[k] * (1.0 - SELECTION_TINT) + highlight[k] * SELECTION_TINT)
        };
        let mode = self.shell.settings.render_3d;
        let (mut tris, mut lines, mut top) = (vec![], vec![], vec![]);
        // a selected grid line, through everything (MU72)
        top.extend(self.selected_grid_lines_3d());
        for item in self.scene.iter().filter(|i| self.selection.contains(i.id)) {
            if item.ghost {
                for &(a, b) in snaps::feature_edges_cached(&item.mesh).iter() {
                    top.push(line_vertex(a.as_vec3().to_array(), highlight));
                    top.push(line_vertex(b.as_vec3().to_array(), highlight));
                }
                continue;
            }
            for &(a, b) in snaps::feature_edges_cached(&item.mesh).iter() {
                lines.push(line_vertex(a.as_vec3().to_array(), highlight));
                lines.push(line_vertex(b.as_vec3().to_array(), highlight));
            }
            if mode == crate::render3d::RenderMode::Wireframe {
                continue;
            }
            // the face picked, and its copies made by modifiers
            let face: std::collections::HashSet<usize> =
                match (self.face, self.doc.model().get(item.id).map(|e| &e.kind)) {
                    (Some(f), Some(ElementKind::FreeMesh(m))) => {
                        let ev = m.evaluated();
                        (0..ev.mesh.faces.len())
                            .filter(|&i| ev.source[i] == Some(f))
                            .flat_map(|i| face_triangles(&ev.mesh, i))
                            .collect()
                    }
                    _ => Default::default(),
                };
            let plain = if mode == crate::render3d::RenderMode::HiddenLine {
                [0.97, 0.97, 0.96]
            } else {
                item.color
            };
            for (t, c) in item.mesh.indices.as_chunks::<3>().0.iter().enumerate() {
                let color = if face.contains(&t) {
                    highlight
                } else {
                    tint(plain)
                };
                for &v in c {
                    let v = v as usize;
                    tris.push(Vertex {
                        pos: item.mesh.positions[v],
                        normal: item.mesh.normals[v],
                        color,
                        surface: item.surface,
                    });
                }
            }
        }
        if let Some(vp) = &mut self.viewport {
            vp.set_selection(&rs.device, &tris, &lines, &top);
        }
    }

    /// Triangles of the elements inside walls, slabs, roofs or masses (their
    /// middle within one's bounds), see-through, for drawing where they are
    /// hidden.
    fn buried_vertices(&self) -> Vec<Vertex> {
        let model = self.doc.model();
        let hosts: Vec<(ElementId, GVec3, GVec3)> = self
            .scene
            .iter()
            .filter(|i| !i.ghost)
            .filter(|i| {
                model
                    .get(i.id)
                    .is_some_and(|e| planview::is_xray_host(&e.kind))
            })
            .filter_map(|i| {
                let (lo, hi) = i.mesh.bounds()?;
                Some((i.id, lo, hi))
            })
            .collect();
        let mut out = Vec::new();
        for item in self.scene.iter().filter(|i| !i.ghost) {
            let Some(e) = model.get(item.id) else {
                continue;
            };
            if planview::is_xray_host(&e.kind) || matches!(e.kind, ElementKind::Opening(_)) {
                continue;
            }
            let Some((lo, hi)) = item.mesh.bounds() else {
                continue;
            };
            let mid = (lo + hi) / 2.0;
            let inside = hosts.iter().any(|(id, hlo, hhi)| {
                *id != item.id && mid.cmpge(*hlo - 0.01).all() && mid.cmple(*hhi + 0.01).all()
            });
            if !inside {
                continue;
            }
            let surface = [item.surface[0], item.surface[1], BURIED_OPACITY];
            for i in &item.mesh.indices {
                let i = *i as usize;
                out.push(Vertex {
                    pos: item.mesh.positions[i],
                    normal: item.mesh.normals[i],
                    color: item.color,
                    surface,
                });
            }
        }
        out
    }

    /// The drawing view Alt+Z turns X-ray on in, or none for the 3D view:
    /// the 2D pane's when the pointer is (or was last) over it.
    fn xray_view_target(&self) -> Option<ElementId> {
        let id = self.active_view?;
        let shown = self.rect_2d.is_some();
        let drawing = matches!(
            self.view(id).map(|v| &v.kind),
            Some(ViewKind::Plan { .. } | ViewKind::Section { .. } | ViewKind::Axonometric { .. })
        );
        (shown && drawing && self.last_in_2d).then_some(id)
    }

    /// Turn X-ray on or off in drawing view `id`, while working: what
    /// holds others faint, what is inside them drawn solid and picked first.
    pub(super) fn toggle_view_xray(&mut self, id: ElementId) {
        let on = !self.plan.xray.contains(&id);
        if on {
            self.plan.xray.insert(id);
        } else {
            self.plan.xray.remove(&id);
        }
        self.plan.hover = None;
        let name = self
            .doc
            .model()
            .get(id)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        self.status = if on {
            format!("X-ray in {name}: walls, slabs and roofs see-through, what is inside them drawn and clicked first. Alt+Z turns it off. Sheets still print it as it was.")
        } else {
            format!("X-ray off in {name}.")
        };
    }

    /// The rendered modes: the image for the view as it is, over the
    /// shaded one, with its stage.
    fn show_rendered(&mut self, ui: &mut egui::Ui, rect: Rect, size: [u32; 2], aspect: f32) {
        let settings = &self.shell.settings;
        let Some(mode) = settings.render_3d.rendered() else {
            self.rendered.clear();
            return;
        };
        let model = self.doc.model();
        let info = model.project_info().map(|(_, p)| p.clone());
        // a lawn under a model with no terrain of its own, so the house
        // stands on ground and the horizon is not the sky's underside
        let terrain = model.iter().any(|e| {
            matches!(
                &e.kind,
                creabim_core::ElementKind::Site(s)
                    if matches!(s.form, creabim_core::SiteForm::Terrain { .. })
            )
        });
        let ground = (!terrain).then_some((-0.15, [0.36, 0.49, 0.25]));
        let sun = crate::rendered::SunAt {
            latitude: info
                .as_ref()
                .and_then(|p| p.latitude)
                .unwrap_or(DEFAULT_LATITUDE) as f32,
            true_north: info.as_ref().map_or(0.0, |p| p.true_north) as f32,
            day: settings.render_day,
            hour: settings.render_hour,
            ev: settings.render_ev,
            sky: creabim_render::sky::SkyKind::of_key(&settings.render_sky),
            strength: settings.render_sky_strength,
            white: settings.render_white,
        };
        let now = ui.input(|i| i.time);
        let (lamps, lamps_key) = self.lamps_now();
        let tex = self.rendered.update(
            ui.ctx(),
            crate::rendered::View {
                mode,
                view_proj: self.camera.view_proj(aspect),
                eye: self.camera.eye(),
                target: self.camera.target,
                pixels: size,
                scene_gen: self.geometry_gen,
                items: &self.scene,
                ground,
                sun,
                lamps,
                lamps_key,
                now,
            },
        );
        let painter = ui.painter_at(rect);
        if let Some(tex) = &tex {
            painter.image(
                tex.id(),
                rect,
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        // the stage, top right: what is shown and how far it has got
        let r = &self.rendered;
        let label = if tex.is_none() {
            if r.working() {
                format!("{}: rendering…", settings.render_3d.label())
            } else {
                format!(
                    "{}: renders when the view is still",
                    settings.render_3d.label()
                )
            }
        } else if r.done {
            format!(
                "{} · {} · {:.1} s",
                settings.render_3d.label(),
                r.stage,
                r.seconds
            )
        } else {
            format!(
                "{} · {} · {:.1} s · refining…",
                settings.render_3d.label(),
                r.stage,
                r.seconds
            )
        };
        // information only: small, in the theme's own tones and partly
        // see-through, so it doesn't take the eye from the view (MU113:
        // "this big white imposing element")
        let pal = self.shell.theme.palette();
        let font = egui::FontId::proportional(11.0);
        let galley = painter.layout_no_wrap(label, font, pal.text.gamma_multiply(0.8));
        let at = rect.left_top() + Vec2::new(36.0, 9.0);
        painter.rect_filled(
            Rect::from_min_size(
                at - Vec2::new(6.0, 2.0),
                galley.size() + Vec2::new(12.0, 4.0),
            ),
            5.0,
            pal.panel.gamma_multiply(0.62),
        );
        painter.galley(at, galley, pal.text.gamma_multiply(0.8));

        // (the sun, the brightness and Save image are in Properties, the
        // Visuals tab: MU111, "is this the right place for this?")
        let _ = tex;
    }

    /// Save the rendered 3D view as a PNG, where the user picks.
    fn save_rendered(&mut self) {
        let Some(img) = self.rendered.image.clone() else {
            return;
        };
        let name = format!(
            "{} {}.png",
            self.shell.settings.render_3d.label(),
            crate::rendered::day_label(self.shell.settings.render_day)
        );
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save the rendered view")
            .add_filter("PNG image", &["png"])
            .set_file_name(name)
            .save_file()
        else {
            return;
        };
        self.status = match img.save_png(&path) {
            Ok(()) => format!(
                "Saved the rendered view ({}×{}) to {}.",
                img.w,
                img.h,
                path.display()
            ),
            Err(e) => format!("Could not save the image: {e}"),
        };
    }

    fn show_3d(&mut self, ui: &mut egui::Ui, rect: Rect, frame: &eframe::Frame) {
        #[cfg(feature = "live-mcp")]
        self.track_live_pane(ui, rect, false);
        self.work_in_phase(phasesui::PhaseOf::Window3d);
        let _perf = Perf::new("show_3d");
        self.rect_3d = Some(rect);
        // clear of the view cube and its column of buttons
        self.shape_strip(ui, rect, 208.0, egui::Id::new("3d"));
        let response = ui.interact(rect, ui.id().with("3d"), Sense::click_and_drag());
        let Some(rs) = frame.wgpu_render_state() else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "3D needs the wgpu renderer",
                egui::FontId::proportional(16.0),
                Color32::RED,
            );
            return;
        };
        // Pull waiting for a face in another view comes to the one the
        // pointer is in (its bar stayed in the view it was last used in,
        // and here nothing could be pulled, MU46)
        if response.hovered()
            && self
                .plan
                .modal
                .as_ref()
                .is_some_and(|m| m.waits_to_pull() && m.pane.is_some())
        {
            if let Some(mut m) = self.plan.modal.take() {
                m.discard(&mut self.doc);
            }
            self.plan.armed = Some((crate::modal::Mode::Pull, false));
        }
        let modal_here = self.plan.modal.as_ref().is_some_and(|m| m.pane.is_none());
        if self.cube.step(&mut self.camera, ui.input(|i| i.time)) {
            ui.ctx().request_repaint();
        }
        // the mouse (MU19): right drag pans, and so does Shift + middle;
        // middle drag orbits (Classic: middle pans too)
        let classic = self.shell.settings.navigation == "classic";
        // Revit, ArchiCAD, AutoCAD: middle pans, Shift + middle orbits
        let cad = self.shell.settings.navigation == "cad";
        let shift_now = ui.input(|i| i.modifiers.shift);
        // Ctrl + middle drag zooms, up in and down out, as Blender (MU39)
        let ctrl_now = ui.input(|i| i.modifiers.command);
        let mid = response.dragged_by(egui::PointerButton::Middle);
        if mid && ctrl_now {
            self.camera.zoom(-response.drag_delta().y / 60.0);
        } else {
            let mid_pans = classic || (cad != shift_now);
            if response.dragged_by(egui::PointerButton::Secondary) || (mid && mid_pans) {
                let d = response.drag_delta();
                self.camera.pan(GVec2::new(d.x, d.y), rect.height());
            }
            if !classic && mid && !mid_pans {
                self.orbit_3d(response.drag_delta());
            }
        }
        if response.hovered() {
            // two fingers on a touchpad pan; a wheel and a pinch zoom (MU24)
            let (pan, scroll) = crate::planview::wheel(ui);
            if pan != egui::Vec2::ZERO {
                self.camera.pan(GVec2::new(pan.x, pan.y), rect.height());
            }
            if scroll != 0.0 {
                let aspect = rect.width() / rect.height().max(1.0);
                match response.hover_pos() {
                    Some(p) => {
                        let ndc = GVec2::new(
                            (p.x - rect.left()) / rect.width().max(1.0) * 2.0 - 1.0,
                            1.0 - (p.y - rect.top()) / rect.height().max(1.0) * 2.0,
                        );
                        self.camera.zoom_at(scroll / 50.0, ndc, aspect);
                    }
                    None => self.camera.zoom(scroll / 50.0),
                }
            }
        }
        let aspect = rect.width() / rect.height().max(1.0);
        self.aspect = aspect;

        // wires in the theme's ink: a new theme draws them again
        let dark = ui.visuals().dark_mode;
        if dark != self.dark_3d {
            self.dark_3d = dark;
            self.geometry_key = None;
        }
        self.rebuild_geometry(rs);
        self.rebuild_selection(rs);
        let ppp = ui.ctx().pixels_per_point();
        // within what the graphics card can make
        let size = crate::render3d::target_size(
            rect.width(),
            rect.height(),
            ppp,
            rs.device.limits().max_texture_dimension_2d,
        );
        if let Some(vp) = &mut self.viewport {
            let _pr = Perf::new("3d render");
            let tex = vp.render(
                rs,
                size,
                self.camera.view_proj(aspect),
                self.camera.eye().to_array(),
                if self.paper_is_dark {
                    BACKGROUND_3D_DARK
                } else {
                    BACKGROUND_3D
                }
                .map(f64::from),
            );
            ui.painter().image(
                tex,
                rect,
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        self.show_rendered(ui, rect, size, aspect);
        ui.painter().text(
            rect.left_top() + Vec2::new(8.0, 8.0),
            egui::Align2::LEFT_TOP,
            "3D",
            egui::FontId::proportional(13.0),
            Color32::from_gray(80),
        );
        self.phase_tag(ui, rect, 36.0, phasesui::PhaseOf::Window3d);

        // overlays and pointer input, drawn over the rendered image
        let painter = ui.painter_at(rect);
        let cam = self.camera.clone();
        let view_proj = cam.view_proj(aspect);
        let project = move |p: DVec3| -> Option<egui::Pos2> {
            let c = view_proj * p.as_vec3().extend(1.0);
            if c.w <= 1e-4 {
                return None;
            }
            let n = c.truncate() / c.w;
            Some(egui::pos2(
                rect.left() + (n.x + 1.0) / 2.0 * rect.width(),
                rect.top() + (1.0 - n.y) / 2.0 * rect.height(),
            ))
        };
        let ray = |p: egui::Pos2| {
            let ndc = GVec2::new(
                (p.x - rect.left()) / rect.width() * 2.0 - 1.0,
                1.0 - (p.y - rect.top()) / rect.height() * 2.0,
            );
            let (o, d) = cam.ray(ndc, aspect);
            (o.as_dvec3(), d.as_dvec3())
        };
        let hover = response.hover_pos();
        let _pp = Perf::new("refs3d");
        self.draw_references_3d(&painter, &project);
        drop(_pp);
        self.draw_divide_preview(&painter, &project);
        let _pp = Perf::new("sketches");
        self.draw_sketches(&painter, &project);
        drop(_pp);
        // left off a Style or Rendered look unless asked for (Visuals
        // REV02, decision 7): they were drawn over the rendered image
        if !self.overlays_off() {
            self.draw_grid_bubbles(&painter, &project);
        }
        self.draw_crop_frames(&painter, &project);
        let outside = self.outside_crop();
        if !outside.is_empty() {
            self.draw_outside_crop(&painter, &project, &outside);
            self.outside_crop_chip(ui, rect, &outside);
        }
        let _pp = Perf::new("hover_3d");
        self.hover_3d(ui, &response, &painter, &project, &ray, modal_here);
        drop(_pp);
        self.view_cube(ui, rect);

        // the Move and Rotate tools, or a keyboard move, rotate or scale,
        // running here
        let pulling = matches!(self.plan.armed, Some((crate::modal::Mode::Pull, _)));
        let starting = response.hovered()
            && self.plan.modal.is_none()
            && !ui.ctx().egui_wants_keyboard_input()
            && (self.selection.is_some() || pulling)
            && !(planview::start_keys(ui.ctx()).is_none()
                && !pulling
                && planview::only_paper(&self.doc, &self.selection.all()))
            && (self.plan.armed.is_some() || planview::start_keys(ui.ctx()).is_some());
        if modal_here || starting {
            let _px = Perf::new("snap exclude");
            let moving = self
                .plan
                .modal
                .as_ref()
                .and_then(|m| m.snap_exclude(&self.doc));
            let exclude = moving.unwrap_or_else(|| {
                self.selection
                    .active
                    .map(|id| planview::joined_with(&self.doc, id))
                    .unwrap_or_default()
            });
            drop(_px);
            self.refresh_snaps_3d(&exclude);
        }
        if modal_here {
            // the view still orbits, pans and zooms between clicks (not
            // while the handle is being dragged: that drag is the edit)
            let dragging_handle = self.plan.modal.as_ref().is_some_and(|m| m.is_a_drag());
            // in Edit mode the shape keeps its wires while the tool runs
            if let Some(id) = self.plan.edit {
                handles::draw_edit_wires(self.doc.model(), id, &painter, &project);
            }
            if response.dragged_by(egui::PointerButton::Primary) && !dragging_handle {
                let d = response.drag_delta();
                self.orbit_3d(d);
            }
            let eye = cam.eye().as_dvec3();
            let work_z = self.work_level().map_or(0.0, |(_, z)| z);
            // a sloped or upright workplane (a face): edits slide on it
            let face_plane = self
                .workplane
                .as_ref()
                .map(|w| w.0)
                .filter(|w| w.normal().z.abs() < 0.999);
            let parallel = |at: DVec3| match face_plane {
                Some(w) => Workplane { origin: at, ..w },
                None => Workplane::ground(at.z),
            };
            let to_model = |pos: egui::Pos2, axis: Option<Axis>, pivot: DVec3| {
                let (o, d) = ray(pos);
                let plane = if axis == Some(Axis::Z) {
                    // stand a plane up through the pivot, facing the camera
                    let f = (pivot - eye).truncate().normalize_or(DVec2::X);
                    Workplane::facing(pivot, -f.extend(0.0))
                } else {
                    parallel(pivot)
                };
                plane.intersect(o, d)
            };
            let snap_cfg = self.plan.snap.live(ui.input(|i| i.modifiers.alt));
            let snapper = &self.snaps_3d.as_ref().expect("built above").2;
            let scene = &self.scene;
            let snap = |p: egui::Pos2, from: Option<DVec3>| {
                let (o, d) = ray(p);
                // from a base point: on its level; else on what is under
                // the pointer, or the working level
                let free = match from {
                    Some(f) => parallel(f).intersect(o, d),
                    None => pick::pick(scene, o.as_vec3(), d.as_vec3())
                        .map(|h| o + d * h.t as f64)
                        .or_else(|| Workplane::ground(work_z).intersect(o, d)),
                };
                let s = snapper.snap(&snap_cfg, p, &project, from, free)?;
                Some((s.p, s))
            };
            // masses, for the face of their own mesh a picked triangle is
            let faces_on = self.plan.modal.as_ref().is_some_and(|m| m.takes_faces());
            let masses: HashMap<ElementId, (creabim_core::FreeMesh, f64)> = if faces_on {
                let model = self.doc.model();
                model
                    .iter()
                    .filter_map(|e| match &e.kind {
                        ElementKind::FreeMesh(m) => {
                            let z = m
                                .level
                                .and_then(|l| model.level(l))
                                .map_or(0.0, |l| l.elevation);
                            Some((e.id, (m.clone(), z)))
                        }
                        _ => None,
                    })
                    .collect()
            } else {
                HashMap::new()
            };
            // drawn sketches, which Pull takes too: by a line of theirs
            // under the pointer, or anywhere inside a closed one
            let pull_on = self
                .plan
                .modal
                .as_ref()
                .is_some_and(|m| m.mode == crate::modal::Mode::Pull);
            let sketches: Vec<(ElementId, creabim_core::Sketch, f64)> = if pull_on {
                let model = self.doc.model();
                model
                    .iter()
                    .filter(|e| !self.hidden.contains(&e.id))
                    .filter_map(|e| match &e.kind {
                        ElementKind::Sketch(s) => {
                            let z = s
                                .level
                                .and_then(|l| model.level(l))
                                .map_or(0.0, |l| l.elevation);
                            Some((e.id, s.clone(), z))
                        }
                        _ => None,
                    })
                    .collect()
            } else {
                vec![]
            };
            let pick_sketch = |p: egui::Pos2, nearer: Option<f64>| {
                let (o, d) = ray(p);
                let mut best: Option<(f64, crate::modal::FacePick)> = None;
                for (id, s, z) in &sketches {
                    let lift = DVec3::Z * *z;
                    let plane = Workplane {
                        origin: s.plane.origin + lift,
                        ..s.plane
                    };
                    let Some(hit) = plane.intersect(o, d) else {
                        continue;
                    };
                    let t = (hit - o).dot(d);
                    let outline = s.outline();
                    let inside = s.is_face()
                        && creabim_core::polygon::contains(&outline, plane.to_plane(hit));
                    // a line of it within a few pixels
                    let on_line = s.segments().iter().any(|(a, b)| {
                        let (Some(a), Some(b)) = (project(*a + lift), project(*b + lift)) else {
                            return false;
                        };
                        let ab = b - a;
                        let k = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                        (a + ab * k).distance(p) < 6.0
                    });
                    // a line wins over what is behind or in front of it; the
                    // inside only where nothing stands in front
                    let takes = on_line || (inside && nearer.is_none_or(|n| t < n));
                    if !takes || best.as_ref().is_some_and(|b| b.0 <= t) {
                        continue;
                    }
                    // out is towards the eye
                    let n = plane.normal();
                    let normal = if n.dot(d) > 0.0 { -n } else { n };
                    let tris = if s.is_face() {
                        creabim_core::polygon::triangulate(&outline)
                            .into_iter()
                            .map(|t| t.map(|i| plane.to_model(outline[i])))
                            .collect()
                    } else {
                        // an open line: a narrow band along it, so the one
                        // that will be pulled is the one lit (MU45)
                        let (mut lo, mut hi) = (outline[0], outline[0]);
                        for q in &outline {
                            lo = lo.min(*q);
                            hi = hi.max(*q);
                        }
                        let w = ((hi - lo).length() * 0.012).clamp(0.015, 0.2);
                        // a closed wire's last side too: without it one
                        // edge looked left out of the pull (MU49)
                        let mut around = outline.clone();
                        if s.closed {
                            around.push(outline[0]);
                        }
                        around
                            .windows(2)
                            .flat_map(|seg| {
                                let side = (seg[1] - seg[0]).normalize_or_zero().perp() * w;
                                let c =
                                    [seg[0] - side, seg[1] - side, seg[1] + side, seg[0] + side]
                                        .map(|q| plane.to_model(q));
                                [[c[0], c[1], c[2]], [c[0], c[2], c[3]]]
                            })
                            .collect()
                    };
                    best = Some((
                        t,
                        crate::modal::FacePick {
                            id: *id,
                            face: creabim_core::pushpull::FaceRef {
                                point: hit,
                                normal,
                                mesh_face: None,
                            },
                            tris,
                        },
                    ));
                }
                best.map(|b| b.1)
            };
            let pick_face = |p: egui::Pos2| -> Option<crate::modal::FacePick> {
                let (o, d) = ray(p);
                let hit = pick::pick_face(scene, o.as_vec3(), d.as_vec3());
                let nearer = hit.as_ref().map(|f| (f.point - o).dot(d));
                if let Some(s) = pick_sketch(p, nearer) {
                    return Some(s);
                }
                let f = hit?;
                // a shape: the face of its own mesh under the pointer, and
                // that face alone lit. Faces in one plane (an outline drawn
                // in a face, the two parts a line cut) lit as one, and it
                // was not clear which would be pulled (MU45)
                let (mesh_face, own) = match masses.get(&f.hit.id) {
                    Some((m, z)) => {
                        let ev = m.evaluated();
                        let face = pick::face_of_triangle(&ev.mesh, f.hit.triangle)?;
                        let source = ev.source.get(face).copied().flatten();
                        let base = m.origin + DVec3::Z * *z;
                        let tris: Vec<[DVec3; 3]> = source
                            .map(|src| {
                                (0..ev.mesh.faces.len())
                                    .filter(|k| ev.source.get(*k).copied().flatten() == Some(src))
                                    .flat_map(|k| {
                                        let loop_ = &ev.mesh.faces[k];
                                        ev.mesh
                                            .face_corners(k)
                                            .into_iter()
                                            .map(|t| {
                                                t.map(|c| {
                                                    base + ev.mesh.vertices[loop_[c] as usize]
                                                })
                                            })
                                            .collect::<Vec<_>>()
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        (source, (!tris.is_empty()).then_some(tris))
                    }
                    _ => (None, None),
                };
                Some(crate::modal::FacePick {
                    id: f.hit.id,
                    face: creabim_core::pushpull::FaceRef {
                        point: f.point,
                        normal: f.normal,
                        mesh_face,
                    },
                    tris: own.unwrap_or(f.triangles),
                })
            };
            let map = PaneMap {
                to_model: &to_model,
                project: &project,
                look: ray(rect.center()).1,
                snap: &snap,
                pick_face: Some(&pick_face),
                pick_line: None,
            };
            let ctx = ui.ctx().clone();
            let shift = ui.input(|i| i.modifiers.shift);
            let base_stage = self
                .plan
                .modal
                .as_ref()
                .is_some_and(|m| m.stage() == crate::modal::Stage::Base);
            if base_stage && shift && response.clicked() {
                // Shift+click while the base point is picked adds or takes away
                if let Some(p) = response.interact_pointer_pos() {
                    let (o, d) = ray(p);
                    let hit = pick::pick(&self.scene, o.as_vec3(), d.as_vec3())
                        .map(|h| h.id)
                        .or_else(|| self.grid_bubble_near(p, &project))
                        .or_else(|| self.grid_line_near(p, &project).map(|g| g.0));
                    if let Some(id) = hit {
                        self.selection.toggle(id);
                        self.face = None;
                        let ids = self.selection.all();
                        self.status = planview::restart_modal(
                            &mut self.doc,
                            &mut self.plan.modal,
                            &ids,
                            p,
                            self.plan.pivot,
                            self.plan.cursor,
                        );
                    }
                }
            } else if let Some(m) = self.plan.modal.as_mut() {
                m.panel(&ctx, rect);
                let right = response.secondary_clicked() && !shift;
                let _pm = Perf::new("move step");
                match m.update(
                    &mut self.doc,
                    &ctx,
                    &painter,
                    hover,
                    response.clicked() || m.pull_drag(&response),
                    right,
                    &map,
                ) {
                    Outcome::Running(s) => {
                        // what is being pulled is what is selected: the
                        // selection's own highlight stayed on something
                        // else while another thing moved (MU45)
                        if let Some(id) = m.pulling() {
                            if self.selection.active != Some(id) || self.selection.len() != 1 {
                                self.selection.only(id);
                                self.face = None;
                            }
                        }
                        self.status = s;
                    }
                    Outcome::Done(s) => {
                        // Pull stays in hand for the next face
                        if m.mode == crate::modal::Mode::Pull {
                            self.plan.armed = Some((crate::modal::Mode::Pull, false));
                        }
                        self.status = s;
                        self.plan.modal = None;
                    }
                    Outcome::Cancelled(s) => {
                        self.status = s;
                        self.plan.modal = None;
                    }
                }
                if let Some(m) = &self.plan.modal {
                    if m.mode != crate::modal::Mode::Pull {
                        self.plan.move_target = m.works_on;
                    }
                }
            }
            ui.ctx().request_repaint();
            return;
        }
        if starting {
            if let Some(grab) = hover {
                let keys = planview::start_keys(ui.ctx());
                let flow = if keys.is_some() {
                    crate::modal::Flow::Keys
                } else {
                    crate::modal::Flow::Tool
                };
                let mode = keys.or(self.plan.armed);
                let extras = (
                    self.plan.armed_grip.take(),
                    std::mem::take(&mut self.plan.in_place),
                );
                self.plan.cancel();
                self.plan.armed = None;
                let ids = self.selection.all();
                if let Some(mode) = mode {
                    match planview::start_modal(
                        &mut self.doc,
                        mode,
                        &ids,
                        self.face,
                        Space::Model,
                        None,
                        grab,
                        self.plan.pivot,
                        self.plan.cursor,
                        flow,
                        self.plan.move_target,
                        self.workplane.as_ref().map(|w| w.0),
                    )
                    .and_then(|(m, t)| Ok((planview::with_extras(&mut self.doc, m, extras)?, t)))
                    {
                        Ok((m, targets)) => {
                            if targets != ids {
                                self.selection.extend(targets, false);
                                self.face = None;
                            }
                            self.status = planview::start_hint(&m);
                            self.plan.modal = Some(m);
                        }
                        Err(e) => self.status = e,
                    }
                }
                return;
            }
        }

        if self.point_pick_3d(ui, &response, &painter, &project, &ray) {
            return;
        }
        if self.pick_plane_3d(ui, &response, &painter, &project, &ray) {
            return;
        }
        if self.crop_pick_3d(ui, &response, &ray) {
            return;
        }

        // Esc or right-click steps back: drop the tool's clicks, then the
        // tool; in Select, right-click opens the menu
        if self.plan.sketch.is_none() && self.plan.outline.is_none() {
            let (esc, shift) = ui.input(|i| (i.key_pressed(egui::Key::Escape), i.modifiers.shift));
            // a grip being dragged takes Esc itself; in Edit mode a
            // right-click opens the grips' menu
            let esc = esc
                && response.hovered()
                && !ui.ctx().egui_wants_keyboard_input()
                && self.handle_drag.is_none()
                && !self.plan.grips.busy();
            let right = response.secondary_clicked()
                && !shift
                && !(self.plan.edit.is_some() && self.plan.tool == Tool::Select);
            if esc || right {
                if self.pending_3d.take().is_some() || self.guide_from.take().is_some() {
                    self.plan.cancel();
                    self.status = "Cancelled. Right-click or Esc again to leave the tool.".into();
                } else {
                    let stepped = self.plan.step_back();
                    // Esc with nothing to step back from selects nothing (MU39)
                    if !stepped
                        && esc
                        && !right
                        && self.plan.tool == Tool::Select
                        && self.plan.edit.is_none()
                        && self.selection.is_some()
                    {
                        self.selection.clear();
                        self.face = None;
                        self.status = "Nothing selected.".into();
                    } else if !stepped && right && self.plan.tool == Tool::Select {
                        if self.plan.web_menus {
                            self.plan.menu_request = response.interact_pointer_pos();
                            self.shell.menu_3d = true;
                        }
                        if let Some(p) = response.interact_pointer_pos() {
                            let (o, d) = ray(p);
                            let hit = pick::pick(&self.scene, o.as_vec3(), d.as_vec3());
                            if let Some(h) = hit.as_ref().filter(|h| !self.selection.contains(h.id))
                            {
                                self.selection.only(h.id);
                                self.face = None;
                            }
                            self.menu_at_3d = hit
                                .map(|h| o + d * h.t as f64)
                                .or_else(|| Workplane::ground(0.0).intersect(o, d));
                            self.menu_plane_3d = face_plane(&self.scene, o, d);
                        }
                    }
                }
            }
            if self.plan.tool == Tool::Select && !self.plan.web_menus {
                let n = self.selection.len();
                let at = self.menu_at_3d;
                let mut picked = None;
                response.context_menu(|ui| picked = crate::commands::context_menu(ui, n));
                match picked {
                    Some(Command::WorkplaneHere) => match self.menu_plane_3d {
                        Some(plane) => self.set_workplane(plane, "on a face".into()),
                        None => {
                            self.status = "Right-click a face to put the workplane on it.".into()
                        }
                    },
                    Some(c) => match c.modal() {
                        Some(m) => self.plan.armed = Some(m),
                        None => self.plan.commands.push((c, at)),
                    },
                    None => {}
                }
            }
        }

        // tools that draw on the face under the pointer (a wall opening
        // only once the workplane is set on a face: else a click places one)
        let on_face = match self.plan.tool {
            Tool::Sketch | Tool::Extrusion | Tool::Void | Tool::Sweep | Tool::Slab => true,
            Tool::Opening => self.workplane.is_some(),
            _ => false,
        };
        if on_face {
            if response.dragged_by(egui::PointerButton::Primary) {
                let d = response.drag_delta();
                self.orbit_3d(d);
            }
            self.sketch_3d(ui, &response, &painter, &project, &ray, &cam);
            return;
        }

        // Shift+right-click places the pivot cursor where the pointer hits
        if let Some(c) = self.plan.cursor {
            if let Some(at) = project(c) {
                planview::draw_cursor(&painter, at);
            }
        }
        if response.secondary_clicked() && ui.input(|i| i.modifiers.shift) {
            if let Some(p) = response.interact_pointer_pos() {
                let (o, d) = ray(p);
                let at = pick::pick(&self.scene, o.as_vec3(), d.as_vec3())
                    .map(|h| o + d * h.t as f64)
                    .or_else(|| Workplane::ground(0.0).intersect(o, d));
                if let Some(c) = at {
                    self.plan.cursor = Some(c);
                    self.plan.pivot = crate::modal::Pivot::Cursor;
                    self.status = format!(
                        "Pivot cursor at {:.2}, {:.2}, {:.2}. Rotate and scale now turn about it.",
                        c.x, c.y, c.z
                    );
                }
            }
            return;
        }

        // B, then drag: select with a box
        if self.plan.tool == Tool::Select {
            let (b, esc) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::B),
                    i.key_pressed(egui::Key::Escape),
                )
            });
            if response.hovered() && b && !ui.ctx().egui_wants_keyboard_input() {
                self.box_armed = true;
                self.status = "Drag a box: left to right takes what is inside, right to left what it touches. Shift adds.".into();
            }
            if esc {
                self.box_armed = false;
                self.box_3d = None;
            }
            // the mouse (MU19): a left drag on empty space draws a box,
            // as in drawings; on the cube or on something selected it
            // doesn't (that drag turns the view or moves it); Alt + left
            // drag orbits. B arms it in Classic.
            let auto_box = self.shell.settings.navigation != "classic"
                && !ui.input(|i| i.modifiers.alt)
                && response.drag_started_by(egui::PointerButton::Primary)
                && ui.input(|i| i.pointer.press_origin()).is_some_and(|p0| {
                    if self.cube.covers(p0) {
                        return false;
                    }
                    // nor on the handle: its arrows stand out past the shape
                    if self.plan.edit.is_some() && handles::handle_covers(p0) {
                        return false;
                    }
                    // nor on one of the selection's grips: that drag is the
                    // grip's (a crop box's face, MU21)
                    let on_grip = self.selection.active.is_some_and(|id| {
                        handles::handles_in(
                            self.doc.model(),
                            id,
                            self.face,
                            self.plan.edit == Some(id),
                        )
                        .iter()
                        .filter_map(|h| project(h.at))
                        .any(|s| s.distance(p0) < 14.0)
                    });
                    if on_grip {
                        return false;
                    }
                    // nor on a selected guide: that drag moves it, as in a
                    // plan (it has no surface for the pick below to find)
                    if self.guide_pressed_3d(p0, &project).is_some() {
                        return false;
                    }
                    let (o, d) = ray(p0);
                    !pick::pick(&self.scene, o.as_vec3(), d.as_vec3())
                        .is_some_and(|h| self.selection.contains(h.id))
                });
            if (self.box_armed || auto_box)
                && response.drag_started_by(egui::PointerButton::Primary)
            {
                self.box_3d = ui.input(|i| i.pointer.press_origin());
                if let Some(p0) = self.box_3d {
                    crate::boxsel::trace(p0, true);
                }
                self.box_armed = false;
            }
            if let Some(p0) = self.box_3d {
                let now = ui.input(|i| i.pointer.latest_pos()).unwrap_or(p0);
                crate::boxsel::trace(now, false);
                crate::boxsel::draw(&painter, p0, now);
                let ended = response.drag_stopped() || !response.dragged();
                {
                    let tris = self.scene.iter().flat_map(|item| {
                        let pts: Vec<Option<egui::Pos2>> = item
                            .mesh
                            .positions
                            .iter()
                            .map(|p| project(glam::Vec3::from(*p).as_dvec3()))
                            .collect();
                        item.mesh
                            .indices
                            .as_chunks::<3>()
                            .0
                            .iter()
                            .filter_map(move |t| {
                                let p = |i: u32| pts.get(i as usize).copied().flatten();
                                Some((item.id, [p(t[0])?, p(t[1])?, p(t[2])?]))
                            })
                    });
                    // guides by the line between their two points (they have
                    // no surface: a box never took them in 3D)
                    let guides = self.guide_marks_3d(&project);
                    let ids: Vec<ElementId> =
                        crate::boxsel::pick_triangles(p0, now, tris.chain(guides))
                            .into_iter()
                            .filter(|id| planview::box_selectable(&self.doc, *id))
                            .collect();
                    if !ended {
                        crate::boxsel::draw_count(&painter, now, ids.len());
                        return;
                    }
                    let (shift, ctrl) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
                    crate::boxsel::apply(
                        &mut self.selection,
                        ids,
                        crate::boxsel::combine_with(shift, ctrl),
                    );
                    self.face = None;
                    self.box_3d = None;
                    self.status = if self.plan.edit.is_some() && self.selection.is_empty() {
                        // Edit mode keeps its element: say so, not "0 selected"
                        "Nothing in the box. Still editing: Esc or Tab finishes.".into()
                    } else {
                        format!("{} selected.", self.selection.len())
                    };
                    self.plan.boxed = true;
                }
                return;
            }
        }

        // handles on the selection, and dragging the selection itself
        let exclude = self
            .selection
            .active
            .map(|id| planview::joined_with(&self.doc, id))
            .unwrap_or_default();
        // the 3D snaps are for the pointer in this view: while it works
        // in another (a plan beside it), they wait, and are made when it
        // comes back. Making them took 65 to 200 ms after every change
        // to the model, wherever the change was made
        if self.snaps_3d.is_none()
            || response.contains_pointer()
            || response.dragged()
            || self.handle_drag.is_some()
        {
            self.refresh_snaps_3d(&exclude);
        }
        let snap_cfg = self.plan.snap.live(ui.input(|i| i.modifiers.alt));
        // a grid line pressed on, to drag it square to itself
        let grid_hit = ui
            .input(|i| i.pointer.press_origin())
            .zip(self.selection.active)
            .and_then(|(p, id)| Some((id, self.grid_grab(p, id, &project)?)));
        // a selected guide pressed on: grabbed where the pointer meets its
        // level, as a body (it has none in the scene)
        let guide_held = ui
            .input(|i| i.pointer.press_origin())
            .and_then(|p| self.guide_pressed_3d(p, &project));
        let _pd = Perf::new("handles_3d");
        let dragging = {
            let snapper = &self.snaps_3d.as_ref().expect("just built").2;
            let scene = &self.scene;
            let ray_fn = |p: egui::Pos2| {
                let (o, d) = ray(p);
                Some(Ray { o, d })
            };
            let snap = |p: egui::Pos2, from: Option<DVec3>| {
                let (o, d) = ray(p);
                let free = Workplane::ground(from.map_or(0.0, |f| f.z)).intersect(o, d);
                let s = snapper.snap(&snap_cfg, p, &project, from, free)?;
                Some((s.p, s))
            };
            let grab_body = |p: egui::Pos2, id: ElementId| {
                if let Some((_, at)) = grid_hit.filter(|g| g.0 == id) {
                    return Some(at);
                }
                if let Some((_, z)) = guide_held.filter(|g| g.0 == id) {
                    let (o, d) = ray(p);
                    return Workplane::ground(z).intersect(o, d);
                }
                // the element itself, even behind or inside another (picked
                // with Tab or in X-ray)
                let (o, d) = ray(p);
                let h = pick::pick_all(scene, o.as_vec3(), d.as_vec3())
                    .into_iter()
                    .find(|h| h.id == id)?;
                Some(o + d * h.t as f64)
            };
            let grab_snap = |p: egui::Pos2, id: ElementId| {
                let only: Vec<SceneItem> = scene.iter().filter(|i| i.id == id).cloned().collect();
                let own = Snapper::from_scene(&only, &[]);
                own.snap(&snap_cfg.points_only(), p, &project, None, None)
                    .map(|s| s.p)
            };
            // the face under the pointer, what is edited left out: its point,
            // and its plane from two more rays a pixel aside
            let editing = exclude.clone();
            let face = |p: egui::Pos2| {
                let hit = |q: egui::Pos2| {
                    let (o, d) = ray(q);
                    let h = pick::pick_where(scene, o.as_vec3(), d.as_vec3(), &|id| {
                        !editing.contains(&id)
                    })?;
                    Some((h.id, o + d * h.t as f64))
                };
                let (id, a) = hit(p)?;
                let (ib, b) = hit(p + egui::vec2(1.0, 0.0))?;
                let (ic, c) = hit(p + egui::vec2(0.0, 1.0))?;
                if ib != id || ic != id {
                    return None;
                }
                let n = (b - a).cross(c - a).normalize_or_zero();
                (n != DVec3::ZERO).then_some((a, n))
            };
            let (_, look) = ray(rect.center());
            let input = PaneInput {
                ray: &ray_fn,
                project: &project,
                look,
                snap: &snap,
                grab_body: &grab_body,
                grab_snap: &grab_snap,
                body_normal: DVec3::Z,
                view: None,
                face: &face,
            };
            let mut status = None;
            let edit = self.plan.edit.is_some() && self.plan.edit == self.selection.active;
            let busy = self.plan.tool == Tool::Select
                && handles::interact(
                    &mut self.doc,
                    &painter,
                    &response,
                    self.selection.active,
                    self.face,
                    edit,
                    &mut self.handle_drag,
                    &mut self.plan.grips,
                    &input,
                    &mut status,
                );
            if let Some(s) = status {
                self.status = s;
            }
            // a corner palette's Mirror or Scale (MU142)
            if let Some(c) = handles::take_palette_command() {
                match c.modal() {
                    Some(m) => self.plan.armed = Some(m),
                    None => self.plan.commands.push((c, None)),
                }
            }
            busy
        };
        drop(_pd);
        if dragging {
            return;
        }
        // a left drag orbits in Classic, or with Alt (no middle button)
        let classic = self.shell.settings.navigation == "classic";
        if response.dragged_by(egui::PointerButton::Primary)
            && self.box_3d.is_none()
            && (classic || ui.input(|i| i.modifiers.alt))
        {
            let d = response.drag_delta();
            self.orbit_3d(d);
        }

        let slab_outline = self.plan.outline.as_ref().is_some_and(|o| !o.is_wall());
        if self.plan.tool.draws_outline() || slab_outline {
            let modes = self.plan.tool.draws_outline();
            let (doc, plan, sel) = (&mut self.doc, &mut self.plan, &mut self.selection);
            if let Some(msg) = crate::slabtool::controls(ui, rect, None, modes, doc, plan, sel) {
                self.status = msg;
            }
        }
        if self.plan.outline.is_some() {
            self.outline_3d(ui, &response, &painter, &project, &ray);
            return;
        }
        if self.plan.tool == Tool::Align {
            self.align_3d(ui, &response, &painter, &project, &ray);
            return;
        }
        if self.plan.tool != Tool::Select {
            self.tools_3d(ui, &response, &painter, &project, &ray);
            return;
        }

        let on_cube = response
            .interact_pointer_pos()
            .is_some_and(|p| self.cube.covers(p));
        // Alt+click: the card of what was clicked's material, where it was
        // clicked (Mockup_Material_Access_REV00 decision 4, Section REV00)
        if response.clicked() && !on_cube && ui.input(|i| i.modifiers.alt) {
            if let Some(p) = response.interact_pointer_pos() {
                let (o, d) = ray(p);
                let hit = pick::pick(&self.scene, o.as_vec3(), d.as_vec3());
                match hit.and_then(|h| Some((h.id, self.material_here(h.id)?))) {
                    Some((e, m)) => crate::matcard::open_at(m, Some(e), p),
                    None => self.status = "No material on what was clicked.".into(),
                }
            }
            return;
        }
        if response.clicked() && !on_cube {
            let _perf = Perf::new("click_3d");
            if let Some(p) = response.interact_pointer_pos() {
                // sketch lines first: they have no surface to hit
                let near_sketch = self.sketch_near(p, &project);
                let shift = ui.input(|i| i.modifiers.shift);
                // a double-click on the selected element opens it in Edit mode
                if response.double_clicked() && !shift {
                    if let Some(id) = self.selection.active.filter(|id| {
                        self.under_3d(p, &project, &ray).contains(id) || near_sketch == Some(*id)
                    }) {
                        self.status = editmode::enter(self.doc.model(), id, &mut self.plan.edit);
                        return;
                    }
                }
                let pick = |sel: &mut Selection, id: Option<ElementId>| match (id, shift) {
                    (Some(id), true) => sel.toggle(id),
                    (None, true) => {}
                    (id, false) => sel.set(id),
                };
                let (o, d) = ray(p);
                // what Tab moved the pre-selection to, if not the first
                let chosen = self.plan.hover.as_mut().and_then(|h| {
                    h.list = false;
                    h.at(hoverpick::Pane::Window3d, p)
                });
                let first = self.under_3d(p, &project, &ray).first().copied();
                if let Some(id) = chosen.filter(|c| Some(*c) != first) {
                    pick(&mut self.selection, Some(id));
                    let hit = pick::pick_all(&self.scene, o.as_vec3(), d.as_vec3())
                        .into_iter()
                        .find(|h| h.id == id);
                    self.face = hit.filter(|_| !shift).and_then(|h| self.face_of_hit(&h));
                    return;
                }
                // grid bubbles are drawn over everything, so they take
                // clicks first
                let bubble = self.grid_bubble_near(p, &project);
                // a reference's lines, as a sketch's (MU33: only a 2D view
                // could pick one)
                let reference = self.reference_near_3d(p, &project);
                // and so are crop box frames
                let frame = || self.crop_frame_near(p, &project);
                if let Some(id) = near_sketch.or(bubble).or(reference).or_else(frame) {
                    pick(&mut self.selection, Some(id));
                    self.face = None;
                    return;
                }
                let hit = self.first_hit_3d(o, d);
                // a grid line in front of what the click hits takes it (MU71:
                // only its bubble did, the ground under it took the click)
                if let Some(id) = self.grid_line_in_front(p, &project, o, d, hit.map(|h| h.t)) {
                    pick(&mut self.selection, Some(id));
                    self.face = None;
                    return;
                }
                // guides cross everything, so they only take clicks that miss
                if hit.is_none() {
                    let near = self.guide_near(p, &project);
                    if let Some(id) = near {
                        pick(&mut self.selection, Some(id));
                        self.face = None;
                        return;
                    }
                }
                pick(&mut self.selection, hit.map(|h| h.id));
                if shift {
                    self.face = None;
                    return;
                }
                self.face = hit.and_then(|h| self.face_of_hit(&h));
            }
        }
    }

    /// A point (or, for Split, a guide) asked for, picked in 3D: on the
    /// working level under the pointer. True while it takes the clicks.
    fn point_pick_3d(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
        ray: &dyn Fn(egui::Pos2) -> (DVec3, DVec3),
    ) -> bool {
        if !self.plan.point_pick {
            return false;
        }
        if response.secondary_clicked() {
            self.plan.point_pick = false;
            return true;
        }
        let Some(h) = response.hover_pos() else {
            return true;
        };
        let z = self.work_level().map_or(0.0, |(_, z)| z);
        let (o, d) = ray(h);
        let Some(p) = Workplane::ground(z).intersect(o, d) else {
            return true;
        };
        let step = crate::snaps::current_step();
        let w = if step > 0.0 {
            (p.truncate() / step).round() * step
        } else {
            p.truncate()
        };
        self.plan.point_hover = Some(w);
        // a guide under the pointer, for Split's line
        let guide = self
            .plan
            .point_lines
            .then(|| self.guide_near(h, project))
            .flatten()
            .and_then(|id| match self.doc.model().get(id).map(|e| &e.kind) {
                Some(ElementKind::Guide(g)) => Some((g.a.truncate(), g.b.truncate())),
                _ => None,
            });
        let red = egui::Stroke::new(1.6, crate::planview::SPLIT_RED);
        match guide {
            Some((a, b)) => {
                if let (Some(pa), Some(pb)) = (project(a.extend(z)), project(b.extend(z))) {
                    painter
                        .line_segment([pa, pb], egui::Stroke::new(3.0, crate::planview::SPLIT_RED));
                }
            }
            None => {
                if let Some(c) = project(w.extend(z)) {
                    let s = egui::Stroke::new(1.5, crate::planview::accent_selected());
                    painter.line_segment([c - egui::vec2(12.0, 0.0), c + egui::vec2(12.0, 0.0)], s);
                    painter.line_segment([c - egui::vec2(0.0, 12.0), c + egui::vec2(0.0, 12.0)], s);
                    painter.circle_stroke(c, 5.0, if self.split.is_some() { red } else { s });
                }
            }
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        if response.clicked() {
            match guide {
                Some(l) => self.plan.point_line_picked = Some(l),
                None => self.plan.point_picked = Some(w),
            }
            if !self.plan.point_lines && self.split.is_none() {
                self.plan.point_pick = false;
            }
        }
        true
    }

    /// The face of a mass a hit lands on.
    fn face_of_hit(&self, h: &pick::Hit) -> Option<usize> {
        match self.doc.model().get(h.id).map(|e| &e.kind) {
            Some(ElementKind::FreeMesh(m)) => {
                let ev = m.evaluated();
                let f = pick::face_of_triangle(&ev.mesh, h.triangle)?;
                ev.source.get(f).copied().flatten()
            }
            _ => None,
        }
    }

    /// The element a click on the 3D view takes along a ray: the nearest,
    /// or in X-ray the nearest of what is inside others.
    fn first_hit_3d(&self, o: DVec3, d: DVec3) -> Option<pick::Hit> {
        let mut hits = pick::pick_all(&self.scene, o.as_vec3(), d.as_vec3());
        if self.shell.settings.xray {
            hits.sort_by_key(|h| !self.is_framing_id(h.id));
        }
        hits.first().copied()
    }

    fn is_framing_id(&self, id: ElementId) -> bool {
        self.doc
            .model()
            .get(id)
            .is_some_and(|e| is_framing(&e.kind))
    }

    /// The sketch line within a few pixels of `p`, if any.
    fn sketch_near(
        &self,
        p: egui::Pos2,
        project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
    ) -> Option<ElementId> {
        creabim_geom::scene::sketch_lines(self.doc.model())
            .into_iter()
            .filter_map(|(id, a, b)| {
                let (a, b) = (project(a)?, project(b)?);
                let d = egui::Rect::from_two_pos(a, b)
                    .expand(6.0)
                    .contains(p)
                    .then(|| {
                        let ab = b - a;
                        let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                        (a + ab * t).distance(p)
                    })?;
                (d < 6.0).then_some((id, d))
            })
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .map(|s| s.0)
    }

    /// Everything under point `p` of the 3D view, in the order a click
    /// takes it: sketch lines and grid bubbles, then what the ray passes
    /// (in X-ray, what is inside others first), else grid lines and guides.
    fn under_3d(
        &self,
        p: egui::Pos2,
        project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
        ray: &dyn Fn(egui::Pos2) -> (DVec3, DVec3),
    ) -> Vec<ElementId> {
        let mut out: Vec<ElementId> = Vec::new();
        out.extend(self.sketch_near(p, project));
        out.extend(self.grid_bubble_near(p, project));
        // crop box frames are drawn over everything too, and take clicks
        // the same way
        out.extend(
            self.crop_frame_near(p, project)
                .filter(|f| !out.contains(f)),
        );
        // the scene is cropped, so what a box crops away is not under it
        let (o, d) = ray(p);
        let mut hits = pick::pick_all(&self.scene, o.as_vec3(), d.as_vec3());
        if self.shell.settings.xray {
            hits.sort_by_key(|h| !self.is_framing_id(h.id));
        }
        // a grid line in front of what is under the pointer comes first, so
        // it lights up and a click takes it (MU71)
        if let Some(id) = self.grid_line_in_front(p, project, o, d, hits.first().map(|h| h.t)) {
            if !out.contains(&id) {
                out.push(id);
            }
        }
        if hits.is_empty() {
            out.extend(self.guide_near(p, project).filter(|g| !out.contains(g)));
        }
        for h in hits {
            if !out.contains(&h.id) {
                out.push(h.id);
            }
        }
        out
    }

    /// The 3D view's pre-selection: read what is under the pointer, trace
    /// the element a click would take (or that another view is reading),
    /// and show the badge when several are under it.
    fn hover_3d(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
        ray: &dyn Fn(egui::Pos2) -> (DVec3, DVec3),
        modal_here: bool,
    ) {
        let pane = hoverpick::Pane::Window3d;
        if !response.dragged() {
            // (the Align tool takes faces here, so Tab reaches them too)
            let reading = matches!(self.plan.tool, Tool::Select | Tool::Align)
                && !modal_here
                && self.plan.modal.is_none()
                && self.plane_pick.is_none()
                && self.box_3d.is_none()
                && !self.box_armed
                // in Edit mode Tab is Layers', in a drag the typed length's
                && self.plan.edit.is_none()
                && self.handle_drag.is_none();
            let pointer = response.hover_pos().filter(|_| reading);
            let rev = self.doc.model().revision();
            let mut slot = self.plan.hover.take();
            let said = hoverpick::track(
                ui.ctx(),
                &mut slot,
                pane,
                pointer,
                rev,
                self.doc.model(),
                |p| self.under_3d(p, project, ray),
            );
            self.plan.hover = slot;
            if let Some(s) = said {
                self.status = s;
            }
        }
        // nothing is lit while a grip or a tool follows the pointer: the
        // last thing lit stayed lit, and its edges were worked out again
        // with every scene the drag made (MU142 lag: ~120 ms a frame
        // turning a chair over the terrain)
        let lit = hoverpick::highlighted(&self.plan.hover).filter(|id| {
            !self.selection.contains(*id)
                && !response.dragged()
                && self.handle_drag.is_none()
                && self.plan.modal.is_none()
        });
        if let Some(id) = lit {
            let stroke = egui::Stroke::new(2.0, hoverpick::HOVER.gamma_multiply(0.85));
            let seg = |a: DVec3, b: DVec3| {
                if let (Some(a), Some(b)) = (project(a), project(b)) {
                    painter.line_segment([a, b], stroke);
                }
            };
            // its edges are worked out once for the scene as it is, not at
            // every frame the pointer rests on it: a terrain has tens of
            // thousands of triangles, and this was 55 to 100 ms a frame
            thread_local! {
                #[allow(clippy::type_complexity)]
                static LIT: std::cell::RefCell<Option<((ElementId, u64), std::rc::Rc<Vec<(DVec3, DVec3)>>)>> =
                    const { std::cell::RefCell::new(None) };
            }
            let key = (id, self.geometry_gen);
            let edges = LIT.with(|c| {
                let mut c = c.borrow_mut();
                match c.as_ref() {
                    Some((k, e)) if *k == key => e.clone(),
                    _ => {
                        // from the edge memo the 3D snaps keep by mesh: the
                        // terrain, unchanged by a chair turned, was worked
                        // out again after every drag (MU142: 78 ms)
                        let e: Vec<(DVec3, DVec3)> = self
                            .scene
                            .iter()
                            .filter(|i| i.id == id)
                            .flat_map(|i| {
                                snaps::feature_edges_cached(&i.mesh)
                                    .iter()
                                    .copied()
                                    .collect::<Vec<_>>()
                            })
                            .collect();
                        let e = std::rc::Rc::new(e);
                        *c = Some((key, e.clone()));
                        e
                    }
                }
            });
            for (a, b) in edges.iter() {
                seg(*a, *b);
            }
            for (sid, a, b) in creabim_geom::scene::sketch_lines(self.doc.model()) {
                if sid == id {
                    seg(a, b);
                }
            }
            // a crop box has no mesh to light: its frame lights instead,
            // so hovering it shows it will be picked (MU136)
            if let Some(ElementKind::CropBox(c)) = self.doc.model().get(id).map(|e| &e.kind) {
                for (a, b) in c.edges() {
                    seg(a, b);
                }
            }
            // a guide lights along its whole reach, as plans light it (MU39)
            if let Some(ElementKind::Guide(g)) = self.doc.model().get(id).map(|e| &e.kind) {
                if let Some(dir) = (g.b - g.a).try_normalize() {
                    let m = (g.a + g.b) / 2.0;
                    let steps = 120;
                    let at = |i: usize| {
                        m + dir * (-GUIDE_REACH_3D + 2.0 * GUIDE_REACH_3D * i as f64 / steps as f64)
                    };
                    for i in 0..steps {
                        seg(at(i), at(i + 1));
                    }
                }
            }
        }
        if let Some(id) = hoverpick::badge(ui.ctx(), &mut self.plan.hover, pane, self.doc.model()) {
            if ui.input(|i| i.modifiers.shift) {
                self.selection.toggle(id);
            } else {
                self.selection.set(Some(id));
            }
            self.face = None;
            self.status = format!("{} selected.", hoverpick::describe(self.doc.model(), id));
        }
    }

    /// Rebuild the 3D snaps when the model changed, but not during a drag,
    /// which changes the model every frame.
    fn refresh_snaps_3d(&mut self, exclude: &[ElementId]) {
        let _perf = Perf::new("snaps_3d");
        // the 3D snaps are made from the scene, guides, grids and
        // sketches: a view's look changes none (80 ms after a click on a
        // drawing style), unless a view is traced as a reference here
        let traces_a_view = self
            .refs_in_3d()
            .iter()
            .any(|(_, r)| matches!(r.source, creabim_core::RefSource::View(_)));
        let rev = if traces_a_view {
            self.doc.model().revision()
        } else {
            self.doc.model().scene_revision()
        };
        // and from the scene as it is shown: after a delete the scene of
        // the new model may still be on its way from a worker, and snaps
        // made from the old one kept the deleted things' points until the
        // next change (MU48: "invisible snap points")
        let rev = rev.rotate_left(24) ^ self.geometry_gen;
        let frozen = self.handle_drag.is_some()
            || self
                .plan
                .modal
                .as_ref()
                .is_some_and(|m| m.pane.is_none() && m.freezes_snaps())
            // a move, a turn or a copy being placed in another view
            // changes the model at every step of its preview: the 3D
            // snaps are not its to use, and wait for it to end
            || self.plan.modal.as_ref().is_some_and(|m| m.pane.is_some());
        // built once per scene with everything; the selection is hidden
        // from it, not left out of a new one (MU39: 60 ms a click)
        // and not while a drag runs anywhere (a grip dragged in a plan
        // changes the model every frame: MU39, 60 ms a frame here)
        let frozen = frozen || crate::trace::held();
        let fresh = self
            .snaps_3d
            .as_ref()
            .is_some_and(|(r, _, _)| *r == rev || frozen);
        // after an edit to shapes that stand alone, with nothing else
        // changed since these snaps were made: theirs only are made again
        let touch = if fresh { None } else { self.snaps_touch.take() };
        let quick = touch.filter(|(_, made, model, live)| {
            !*live
                && self.snaps_3d.as_ref().is_some_and(|(r, _, _)| r == made)
                && self.doc.model().drawing_revision() == *model
        });
        let fresh = match (quick, self.snaps_3d.as_mut()) {
            (Some((ids, ..)), Some((r, _, s))) => {
                let _pq = Perf::new("snaps_3d: the edited shape only");
                s.replace_scene_items(&ids, &self.scene);
                *r = rev;
                true
            }
            _ => fresh,
        };
        if !fresh {
            let mut snapper = Snapper::from_scene(&self.scene, &[]);
            snapper.add_guides(self.doc.model(), GUIDE_REACH_3D);
            // every object's middle and centre lines, at its floor (MU142)
            snapper.add_object_centres(self.doc.model(), None, 0.6);
            snapper.add_grids(self.doc.model(), &self.hidden_in_3d());
            snapper.add_sketches(self.doc.model(), &[]);
            // the references' lines, where 3D shows them (MU33)
            let mut n = 0usize;
            for (id, r) in self.refs_in_3d() {
                let model = self.doc.model();
                if let crate::trace::Shown::Lines(lines) = self.plan.ref_cache.get(model, id, &r) {
                    for l in lines.iter() {
                        for w in l.windows(2) {
                            if n < 20_000 {
                                let a = crate::handles::ref_world(model, &r, r.place(w[0]));
                                let b = crate::handles::ref_world(model, &r, r.place(w[1]));
                                snapper.add_segment(a, b, crate::snaps::Source::Underlay, Some(id));
                                n += 1;
                            }
                        }
                    }
                }
            }
            self.snaps_3d = Some((rev, vec![], snapper));
        }
        if let Some((_, ex, s)) = &mut self.snaps_3d {
            if ex != exclude {
                *ex = exclude.to_vec();
                s.hide(exclude);
            }
        }
    }

    fn set_workplane(&mut self, plane: Workplane, name: String) {
        self.status = format!(
            "Workplane: {name}. Sketches and guides drawn in 3D go on it. Pick Level in the Workplane list to go back."
        );
        self.workplane = Some((plane, name));
        self.geometry_key = None;
    }

    /// Picking a workplane: a face, or three points. Returns true while it
    /// takes the view's input.
    fn pick_plane_3d(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
        ray: &dyn Fn(egui::Pos2) -> (DVec3, DVec3),
    ) -> bool {
        let Some(mode) = self.plane_pick.clone() else {
            return false;
        };
        let accent = crate::theme::interaction();
        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if esc || response.secondary_clicked() {
            self.plane_pick = None;
            self.status = "Workplane unchanged.".into();
            return true;
        }
        let Some(hover) = response.hover_pos() else {
            return true;
        };
        let (o, d) = ray(hover);
        match mode {
            PlanePick::Face => {
                let plane = face_plane(&self.scene, o, d);
                if let Some(plane) = plane {
                    crate::sketch::draw_plane(painter, project, &plane, accent);
                }
                self.status = "Click the face the workplane lies on. Right-click cancels.".into();
                if response.clicked() {
                    match plane {
                        Some(plane) => {
                            self.plane_pick = None;
                            self.set_workplane(plane, "on a face".into());
                        }
                        None => {
                            self.status = "That isn't a face. Click a wall, slab or mass.".into()
                        }
                    }
                }
            }
            PlanePick::Points(mut pts) => {
                self.refresh_snaps_3d(&[]);
                let free = pick::pick(&self.scene, o.as_vec3(), d.as_vec3())
                    .map(|h| o + d * h.t as f64)
                    .or_else(|| Workplane::ground(0.0).intersect(o, d));
                let snapper = &self.snaps_3d.as_ref().expect("just built").2;
                let snap_cfg = self.plan.snap.live(ui.input(|i| i.modifiers.alt));
                let snap = snapper.snap(&snap_cfg, hover, project, pts.last().copied(), free);
                let at = snap.map(|s| s.p);
                if let (Some(s), Some(c)) = (snap, at.and_then(project)) {
                    snaps::draw(painter, s, c, None);
                }
                let mut shown = pts.clone();
                shown.extend(at);
                let scr: Vec<egui::Pos2> = shown.iter().filter_map(|p| project(*p)).collect();
                for c in &scr {
                    painter.circle_filled(*c, 3.5, accent);
                }
                if scr.len() > 1 {
                    painter.add(egui::Shape::closed_line(
                        scr,
                        egui::Stroke::new(1.5, accent),
                    ));
                }
                self.status = format!(
                    "Click point {} of 3 for the workplane: the first two set its x axis. Right-click cancels.",
                    pts.len() + 1
                );
                if response.clicked() {
                    if let Some(p) = at {
                        pts.push(p);
                    }
                    if pts.len() == 3 {
                        self.plane_pick = None;
                        let x = (pts[1] - pts[0]).normalize_or_zero();
                        let n = x.cross(pts[2] - pts[0]).normalize_or_zero();
                        if x == DVec3::ZERO || n == DVec3::ZERO {
                            self.status =
                                "Those points are in a line. Pick the workplane again.".into();
                        } else {
                            let plane = Workplane {
                                origin: pts[0],
                                x,
                                y: n.cross(x),
                            };
                            self.set_workplane(plane, "through three points".into());
                        }
                    } else {
                        self.plane_pick = Some(PlanePick::Points(pts));
                    }
                }
            }
        }
        true
    }

    /// A guide drawn on the workplane in 3D: two points, snapped and laid
    /// onto the plane.
    fn guide_on_plane(
        &mut self,
        response: &egui::Response,
        painter: &egui::Painter,
        project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
        ray: &dyn Fn(egui::Pos2) -> (DVec3, DVec3),
        plane: Workplane,
    ) {
        let accent = crate::theme::interaction();
        let Some(hover) = response.hover_pos() else {
            return;
        };
        let (o, d) = ray(hover);
        let n = plane.normal();
        let onto = |p: DVec3| p - n * (p - plane.origin).dot(n);
        let from = self.guide_from;
        self.refresh_snaps_3d(&[]);
        let snapper = &self.snaps_3d.as_ref().expect("just built").2;
        let snap_cfg = self.plan.snap.live(response.ctx.input(|i| i.modifiers.alt));
        let Some(snap) = snapper.snap(&snap_cfg, hover, project, from, plane.intersect(o, d))
        else {
            return;
        };
        let at = onto(snap.p);
        if let Some(c) = project(at) {
            snaps::draw(painter, snap, c, from.and_then(project));
        }
        if let Some(a) = from {
            draw_guide_3d(painter, project, a, at, egui::Stroke::new(2.0, accent));
        }
        self.status = match from {
            None => "Guide on the workplane: click a point it runs through.".into(),
            Some(a) => format!(
                "{}. Click a second point.",
                crate::units::len(a.distance(at))
            ),
        };
        if !response.clicked() {
            return;
        }
        match from {
            None => self.guide_from = Some(at),
            Some(a) if a.distance(at) > 0.01 => {
                self.guide_from = None;
                let e = creabim_core::Element::new(
                    "Guide",
                    ElementKind::Guide(creabim_core::Guide {
                        level: None,
                        a,
                        b: at,
                    }),
                );
                let r = self
                    .doc
                    .commit(creabim_core::Transaction::new("Add guide").insert(e));
                self.report(r, "Guide added on the workplane.");
            }
            Some(_) => {}
        }
    }

    /// The level the 3D drawing tools draw on: the active plan's, else the
    /// selected element's, else the lowest.
    fn work_level(&self) -> Option<(ElementId, f64)> {
        let m = self.doc.model();
        let plan = self
            .active_view
            .and_then(|v| match self.view(v).map(|v| &v.kind) {
                Some(ViewKind::Plan { level, .. }) => Some(*level),
                _ => None,
            });
        let of_selection = self
            .selection
            .active
            .and_then(|id| match m.get(id).map(|e| &e.kind) {
                Some(ElementKind::Wall(w)) => Some(w.level),
                Some(ElementKind::Slab(s)) => Some(s.level),
                Some(ElementKind::FreeMesh(f)) => f.level,
                Some(ElementKind::Level(_)) => Some(id),
                Some(k) => creabim_core::members::level_of(k),
                _ => None,
            });
        let lowest = m
            .levels()
            .min_by(|a, b| a.1.elevation.total_cmp(&b.1.elevation))
            .map(|(e, _)| e.id);
        let id = plan.or(of_selection).or(lowest)?;
        Some((id, m.level(id).map_or(0.0, |l| l.elevation)))
    }

    /// Walls, slabs, doors, windows, masses and section lines drawn right in
    /// the 3D view, on the working level, with the same snaps as dragging.
    /// The Align tool in the 3D window: a reference, then a face to align,
    /// stretch or extend to it (see [`crate::align3d`]).
    fn align_3d(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
        ray: &dyn Fn(egui::Pos2) -> (DVec3, DVec3),
    ) {
        // on a sloped or upright workplane, things turn on it to line up
        self.plan.align.workplane = self.workplane.as_ref().map(|w| w.0);
        // the tool's hint, for 3D, once the pointer is here
        if response.hovered() && self.plan.align.reference_3d.is_none() {
            self.status = crate::align3d::hint(&self.plan.align);
        }
        let under = match response.hover_pos() {
            Some(p) => {
                let (o, d) = ray(p);
                let chosen = self
                    .plan
                    .hover
                    .as_ref()
                    .and_then(|h| h.at(hoverpick::Pane::Window3d, p));
                let face = crate::align3d::face_pick(self.doc.model(), &self.scene, o, d, chosen);
                self.refresh_snaps_3d(&[]);
                let snapper = &self.snaps_3d.as_ref().expect("just built").2;
                let cfg = self.plan.snap.live(ui.input(|i| i.modifiers.alt));
                let free = face.as_ref().map(|f| f.face.point);
                let snap = snapper
                    .snap(&cfg, p, project, None, free)
                    .filter(|s| s.kind != SnapKind::Grid);
                let edge = snap.and_then(|s| snapper.edge_at(s.p));
                let (centres, centre) = crate::align3d::centres_under(
                    self.doc.model(),
                    &self.scene,
                    face.as_ref(),
                    p,
                    project,
                );
                let centre = centre.filter(|_| crate::align3d::takes_centres(&self.plan.align));
                crate::align3d::Under {
                    face,
                    snap,
                    edge,
                    level: self.work_level(),
                    centres,
                    centre,
                }
            }
            None => Default::default(),
        };
        crate::align3d::draw(painter, project, self.doc.model(), &self.plan.align, &under);
        if response.clicked() {
            if let Some(h) = self.plan.hover.as_mut() {
                h.list = false;
            }
            self.status = crate::align3d::click(
                &mut self.doc,
                &mut self.plan.align,
                &under,
                &mut self.selection,
            );
            self.face = None;
        }
    }

    /// The Beam tool's On face mode in 3D: the first click picks a face
    /// (a roof slope, a wall) and a point on it; every next click is kept
    /// in that face's plane, and each beam lies on the face, its section
    /// square to it, sitting on its surface. Clicks chain beams; Esc or a
    /// right-click ends the chain.
    fn beam_on_face_3d(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
        ray: &dyn Fn(egui::Pos2) -> (DVec3, DVec3),
        level: ElementId,
    ) {
        let accent = crate::theme::interaction();
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || response.secondary_clicked() {
            if self.beam_face.take().is_none() {
                self.plan.tool = Tool::Select;
            }
            self.status = "Beams on face: click a face to start.".into();
            return;
        }
        let Some(hover) = response.hover_pos() else {
            return;
        };
        let (o, d) = ray(hover);
        // where the pointer is: on the face picked first, else on the face
        // under it
        let face = pick::pick_face(&self.scene, o.as_vec3(), d.as_vec3());
        let (at, plane) = match self.beam_face {
            Some((_, p0, n)) => {
                let den = d.dot(n);
                if den.abs() < 1e-9 {
                    return;
                }
                let t = (p0 - o).dot(n) / den;
                if t <= 0.0 {
                    return;
                }
                (o + d * t, Some((p0, n)))
            }
            None => match &face {
                Some(f) => (f.point, Some((f.point, f.normal))),
                None => {
                    if response.clicked() {
                        self.status =
                            "Beams on face: click a face (a roof slope, a wall, a slab).".into();
                    }
                    return;
                }
            },
        };
        // the face's corners and the middles of its edges snap
        let mut at = at;
        if let Some(f) = &face {
            let mut best = 12.0f32;
            for tri in &f.triangles {
                for k in 0..3 {
                    for q in [tri[k], (tri[k] + tri[(k + 1) % 3]) / 2.0] {
                        let on_plane = plane.is_none_or(|(p0, n)| (q - p0).dot(n).abs() < 1e-4);
                        if let (Some(s), true) = (project(q), on_plane) {
                            let dd = s.distance(hover);
                            if dd < best {
                                best = dd;
                                at = q;
                            }
                        }
                    }
                }
            }
            if best < 12.0 {
                if let Some(s) = project(at) {
                    painter.rect_stroke(
                        egui::Rect::from_center_size(s, egui::vec2(10.0, 10.0)),
                        0.0,
                        egui::Stroke::new(1.5, accent),
                        egui::StrokeKind::Middle,
                    );
                }
            }
        }
        let set = self.plan.members.clone();
        if let Some((a, _, n)) = self.beam_face {
            if let (Some(sa), Some(sb)) = (project(a), project(at)) {
                painter.line_segment([sa, sb], egui::Stroke::new(2.0, accent));
                // its depth, standing off the face
                if let (Some(sa2), Some(sb2)) = (
                    project(a + n * set.beam_depth),
                    project(at + n * set.beam_depth),
                ) {
                    painter.add(egui::Shape::closed_line(
                        vec![sa, sb, sb2, sa2],
                        egui::Stroke::new(1.0, accent),
                    ));
                }
                painter.text(
                    sb + egui::vec2(10.0, -12.0),
                    egui::Align2::LEFT_BOTTOM,
                    crate::units::len(a.distance(at)),
                    egui::FontId::proportional(13.0),
                    accent,
                );
            }
        } else if let Some(s) = project(at) {
            painter.circle_stroke(s, 6.0, egui::Stroke::new(1.5, accent));
        }
        if !response.clicked() {
            return;
        }
        let Some((p0, n)) = plane else { return };
        let Some((a, _, _)) = self.beam_face else {
            self.plan.members.last_beam = None;
            self.beam_face = Some((at, p0, n));
            self.status = "Beam on face: click its other end; it stays on this face.".into();
            return;
        };
        if a.distance(at) < 1e-3 {
            return;
        }
        // its top line stands off the face by its section's own height,
        // so it sits on it (MU40: a library section floated by the beam
        // depth instead)
        let model = self.doc.model();
        let section = set
            .beam_section
            .as_deref()
            .and_then(|nm| creabim_core::profiles::find(model, nm));
        let height = match &section {
            Some(pts) if !pts.is_empty() => {
                let lo = pts.iter().map(|p| p.y).fold(f64::MAX, f64::min);
                let hi = pts.iter().map(|p| p.y).fold(f64::MIN, f64::max);
                hi - lo
            }
            _ => set.beam_depth,
        };
        let (ta, tb) = (a + n * height, at + n * height);
        // it belongs to the level it sits on, the highest one at or below
        // its lowest point, not the plan that was open (MU40: drawn on the
        // roof, it was on the ground floor and the roof plan never showed
        // it)
        let low = ta.z.min(tb.z) - height;
        let level = model
            .levels()
            .filter(|(_, l)| l.elevation <= low + 1e-6)
            .max_by(|a, b| a.1.elevation.total_cmp(&b.1.elevation))
            .map_or(level, |(e, _)| e.id);
        let top0 = creabim_core::members::elevation(model, level)
            + creabim_core::members::storey_height(model, level);
        let mut b = creabim_core::Beam::new(level, ta.truncate(), tb.truncate());
        b.width = set.beam_width;
        b.depth = set.beam_depth;
        b.top_offset = top0 - ta.z;
        b.rise = [0.0, tb.z - ta.z];
        b.up = Some(n);
        if let Some(nm) = set.beam_section.as_deref() {
            if let Some(pts) = creabim_core::profiles::find(model, nm) {
                b.section = Some(creabim_core::members::Section {
                    name: nm.to_string(),
                    points: pts,
                });
            }
        }
        let e = self
            .plan
            .typed(model, Element::new("Beam", ElementKind::Beam(b)));
        // the one before in this chain, ending where this starts
        let prev = self.plan.members.last_beam.and_then(|id| model.get(id).cloned()).filter(|pe| {
            matches!(&pe.kind, ElementKind::Beam(pb) if pb.end.distance(ta.truncate()) < 1e-6)
        });
        let id = e.id;
        self.plan.members.last_beam = Some(id);
        let tx = match prev {
            Some(pe) => {
                let (mut pe2, mut ne) = (pe.clone(), e);
                if let (ElementKind::Beam(pb), ElementKind::Beam(nb)) =
                    (&mut pe2.kind, &mut ne.kind)
                {
                    creabim_core::members::join_beams(pb, nb, set.beam_corner);
                }
                Transaction::new("Beam on face").replace(pe, pe2).insert(ne)
            }
            None => Transaction::new("Beam on face").insert(e),
        };
        let r = self.doc.commit(tx);
        self.selection.only(id);
        self.beam_face = Some((at, p0, n));
        self.report(
            r,
            "Beam on the face. Click on to chain the next; Esc or a right-click ends.",
        );
    }

    fn tools_3d(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
        ray: &dyn Fn(egui::Pos2) -> (DVec3, DVec3),
    ) {
        let tool = self.plan.tool;
        if tool.is_annotation() {
            if response.clicked() {
                self.status = format!(
                    "The {} tool works in plans and sections for now.",
                    tool.label()
                );
            }
            return;
        }
        if let (Tool::Guide, Some(plane)) = (tool, self.workplane.as_ref().map(|w| w.0)) {
            self.guide_on_plane(response, painter, project, ray, plane);
            return;
        }
        let Some((level, z)) = self.work_level() else {
            return;
        };
        let accent = crate::theme::interaction();
        let stroke = egui::Stroke::new(2.0, accent);
        // a right-click with nothing drawn leaves the Slab tool
        if tool.draws_outline() && response.secondary_clicked() && self.pending_3d.is_none() {
            self.plan.tool = Tool::Select;
            self.status = format!("Left the {} tool.", tool.label());
            return;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || response.secondary_clicked() {
            self.pending_3d = None;
        }
        let Some(hover) = response.hover_pos() else {
            return;
        };
        let (o, d) = ray(hover);
        let hit = pick::pick(&self.scene, o.as_vec3(), d.as_vec3());
        let level_name = self
            .doc
            .model()
            .get(level)
            .map_or(String::new(), |e| e.name.clone());

        // doors and windows go into the wall under the pointer
        if matches!(tool, Tool::Door | Tool::Window | Tool::Opening) {
            let wall = hit.and_then(|h| match self.doc.model().get(h.id).map(|e| &e.kind) {
                Some(ElementKind::Wall(w)) => Some((h, w.level)),
                Some(ElementKind::Opening(op)) => {
                    match self.doc.model().get(op.host).map(|e| &e.kind) {
                        Some(ElementKind::Wall(w)) => Some((h, w.level)),
                        _ => None,
                    }
                }
                _ => None,
            });
            if let Some((h, wl)) = wall {
                let at = o + d * h.t as f64;
                // the window or door a click puts here (A7-4)
                match crate::ghost::cached(self.doc.model(), wl, tool, at.truncate(), &self.plan) {
                    Some(g) => {
                        crate::ghost::draw(painter, &project, &g, accent);
                        self.plan.ghost_now = Some((Some(usize::MAX), g));
                    }
                    None => {
                        if let Some(c) = project(at) {
                            painter.circle_stroke(c, 6.0, stroke);
                        }
                    }
                }
                if response.clicked() {
                    let mut sel = self.selection.clone();
                    let mut state = std::mem::take(&mut self.plan);
                    let r = planview::plan_tool(
                        &mut self.doc,
                        wl,
                        tool,
                        at.truncate(),
                        &mut state,
                        &mut sel,
                    );
                    self.plan = state;
                    self.selection = sel;
                    self.report(
                        r,
                        format!(
                            "{} placed. Drag its round handles to resize it.",
                            tool.label()
                        ),
                    );
                }
            } else if response.clicked() {
                self.status = format!(
                    "Click a wall to put the {} in it.",
                    tool.label().to_lowercase()
                );
            }
            return;
        }

        // a beam on a face: it lies in the face's plane, square to it,
        // wherever the second point is drawn (MU40)
        if tool == Tool::Beam && self.plan.members.beam_on_face {
            self.beam_on_face_3d(ui, response, painter, project, ray, level);
            return;
        }
        // everything else is drawn on the working level
        let from = self
            .pending_3d
            .filter(|(_, l)| *l == level)
            .map(|(p, _)| p.extend(z));
        // copying a guide parallel: the drop lines up with nothing through
        // where it was taken (MU70: at low angles those lines caught the
        // pointer from far off), see snap_from
        let copying_guide = tool == Tool::Guide && self.plan.guide_edge.is_some() && from.is_some();
        let snap_from = from.filter(|_| !copying_guide);
        let free = Workplane::ground(z).intersect(o, d);
        let exclude: Vec<ElementId> = vec![];
        self.refresh_snaps_3d(&exclude);
        let snapper = &self.snaps_3d.as_ref().expect("just built").2;
        let snap_cfg = self.plan.snap.live(ui.input(|i| i.modifiers.alt));
        // the corners of the chain of walls (or beams) drawn so far
        let chain: Vec<DVec3> = match (&self.plan.chain, from) {
            ((Some(l), pts), Some(_)) if *l == level => pts.iter().map(|p| p.extend(z)).collect(),
            _ => vec![],
        };
        // and the points rested on, as in plans (MU26: walls, beams and
        // the other level tools in 3D too)
        // drafting's temporary guides on the level too (MU37): from the
        // points rested on and through where the line starts
        let guides = guides_3d(
            &self.rested_3d.0,
            snap_from,
            z,
            self.plan.guide_step,
            &snap_cfg,
        );
        let rested: Vec<DVec3> = if guides.is_empty() {
            self.rested_3d
                .0
                .iter()
                .map(|p| p.truncate().extend(z))
                .collect()
        } else {
            vec![]
        };
        let track = snaps::track_points(&chain, &rested);
        let Some(hit) =
            snapper.snap_guided(&snap_cfg, hover, project, snap_from, &track, &guides, free)
        else {
            return;
        };
        snaps::draw_guides(painter, project, &guides, &hit.guides);
        let (snap, align) = (hit.snap, hit.align);
        rest_points(&mut self.rested_3d, ui.ctx(), Some(&snap), true);
        let p = snap.p.truncate();
        let at = p.extend(z);
        // a point snapped above or below the working plane drops onto it:
        // a dashed line shows where it lands (ArchiCAD's projections, MU18)
        if (snap.p.z - z).abs() > 1e-3 {
            if let (Some(a), Some(b)) = (project(snap.p), project(at)) {
                painter.extend(egui::Shape::dashed_line(
                    &[a, b],
                    egui::Stroke::new(1.2, Color32::from_rgb(78, 123, 196)),
                    5.0,
                    4.0,
                ));
                painter.circle_stroke(
                    a,
                    4.0,
                    egui::Stroke::new(1.2, Color32::from_rgb(78, 123, 196)),
                );
                painter.circle_filled(b, 3.0, Color32::from_rgb(78, 123, 196));
            }
        }
        if let Some(c) = project(at) {
            if snap.kind == SnapKind::Grid {
                painter.circle_stroke(c, 4.0, egui::Stroke::new(1.0, accent));
            } else if !hit.guides.is_empty() {
                snaps::draw_guided(painter, project, &hit, &guides, from.and_then(project));
            } else {
                snaps::draw_tracked(painter, project, snap, align, c, from.and_then(project));
            }
        }
        if from.is_none() && crate::ghost::shows(tool) {
            if let Some(g) = crate::ghost::cached(self.doc.model(), level, tool, p, &self.plan) {
                crate::ghost::draw(painter, &project, &g, accent);
                self.plan.ghost_now = Some((Some(usize::MAX), g));
            }
        }
        if tool == Tool::Object {
            if ui.input(|i| !i.modifiers.command && i.key_pressed(egui::Key::Space))
                && !ui.ctx().egui_wants_keyboard_input()
            {
                self.plan.objects.rotation = (self.plan.objects.rotation + 90.0).rem_euclid(360.0);
            }
            crate::objecttool::preview(
                painter,
                project,
                self.doc.model(),
                level,
                p,
                &self.plan,
                true,
            );
        }
        if let Some(a) = from {
            match tool {
                Tool::Guide => {
                    // a copy shows where it lands: the original moved from
                    // where it was grabbed (`a`) to the pointer (MU69: it
                    // was drawn from the pointer, as if held by its end)
                    let (g0, g1) = match self.plan.guide_edge {
                        Some((e0, e1)) => {
                            let by = (at - a).truncate();
                            ((e0 + by).extend(at.z), (e1 + by).extend(at.z))
                        }
                        None => (a, at),
                    };
                    draw_guide_3d(painter, project, g0, g1, stroke);
                    if let Some(sb) = project(at) {
                        let text = match self.plan.guide_edge {
                            Some((e0, e1)) => {
                                let d = (e1 - e0).normalize_or_zero();
                                crate::units::len((p - e0).perp_dot(d).abs())
                            }
                            None => crate::units::len(a.truncate().distance(p)),
                        };
                        painter.text(
                            sb + Vec2::new(10.0, -12.0),
                            egui::Align2::LEFT_BOTTOM,
                            text,
                            egui::FontId::proportional(13.0),
                            accent,
                        );
                    }
                }
                t if t.draws_outline() => {
                    let (lo, hi) = (a.truncate().min(p), a.truncate().max(p));
                    let pts = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y), lo];
                    let scr: Vec<egui::Pos2> =
                        pts.iter().filter_map(|q| project(q.extend(z))).collect();
                    painter.add(egui::Shape::line(scr, stroke));
                }
                _ => {
                    crate::memberstool::preview(
                        painter,
                        project,
                        self.doc.model(),
                        Some(level),
                        tool,
                        &self.plan,
                        a,
                        at,
                    );
                    // a shaped wall (arc, circle, rectangle) draws its own
                    // lines below: no straight one to the pointer
                    let shaped = tool == Tool::Wall
                        && crate::planview::wall_shape_preview(
                            &self.plan,
                            a.truncate(),
                            at.truncate(),
                        )
                        .is_some();
                    if let (Some(sa), Some(sb)) = (project(a), project(at)) {
                        if !shaped {
                            painter.line_segment([sa, sb], stroke);
                        }
                        painter.text(
                            sb + Vec2::new(10.0, -12.0),
                            egui::Align2::LEFT_BOTTOM,
                            crate::units::len(a.truncate().distance(p)),
                            egui::FontId::proportional(13.0),
                            accent,
                        );
                    }
                    if tool == Tool::Wall {
                        // the wall's height, so it reads as a wall; an arc,
                        // a circle or a rectangle drawn as it will be (MU40)
                        let h = self.doc.model().level(level).map_or(3.0, |l| l.height);
                        let up = DVec3::Z * h;
                        let lines = crate::planview::wall_shape_preview(
                            &self.plan,
                            a.truncate(),
                            at.truncate(),
                        )
                        .unwrap_or_else(|| vec![vec![a.truncate(), at.truncate()]]);
                        for pts in lines {
                            let base: Vec<DVec3> = pts.iter().map(|p| p.extend(z)).collect();
                            for (k, row) in [base.clone(), base.iter().map(|p| *p + up).collect()]
                                .into_iter()
                                .enumerate()
                            {
                                let scr: Vec<egui::Pos2> =
                                    row.iter().filter_map(|q| project(*q)).collect();
                                painter.add(egui::Shape::line(
                                    scr,
                                    egui::Stroke::new(if k == 0 { 2.0 } else { 1.0 }, accent),
                                ));
                            }
                            for q in [base.first(), base.last()].into_iter().flatten() {
                                if let (Some(s0), Some(s1)) = (project(*q), project(*q + up)) {
                                    painter.line_segment([s0, s1], egui::Stroke::new(1.0, accent));
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut click_p = p;
        if response.clicked() && tool == Tool::Guide && from.is_none() {
            // clicking an edge first draws the guide parallel to it
            let edge = matches!(
                snap.kind,
                SnapKind::OnEdge | SnapKind::Guide | SnapKind::Midpoint
            )
            .then(|| snapper.edge_at(snap.p))
            .flatten()
            .filter(|(a, b)| a.truncate().distance(b.truncate()) > 1e-3)
            .map(|(a, b)| (a.truncate(), b.truncate()))
            // a guide under the pointer, though the click snapped to a
            // corner or crossing near it (MU70: in 3D it then drew a new
            // guide through the two clicks)
            .or_else(|| guide_near_3d(self.doc.model(), hover, project));
            // taken where it was clicked, on its line: not at the corner
            // the click snapped to (MU70: the copy was measured from there)
            if let Some((e0, e1)) = edge {
                let d = (e1 - e0).normalize_or_zero();
                let at = free.map_or(p, |f| f.truncate());
                click_p = e0 + d * (at - e0).dot(d);
            }
            self.plan.guide_edge = edge;
        }
        if response.clicked() {
            let mut sel = self.selection.clone();
            let mut state = std::mem::take(&mut self.plan);
            state.pending = self.pending_3d.filter(|(_, l)| *l == level).map(|(p, _)| p);
            let r = planview::plan_tool(&mut self.doc, level, tool, click_p, &mut state, &mut sel);
            self.pending_3d = state.pending.take().map(|p| (p, level));
            self.plan = state;
            self.selection = sel;
            if let Err(e) = r {
                self.status = format!("Couldn't do that: {e}");
            } else {
                self.status = match (tool, self.pending_3d) {
                    (Tool::Wall, Some(_)) => format!("Drawing walls on {level_name}. Click the next corner; Esc or right-click ends."),
                    (t, _) if t.draws_outline() || t == Tool::RoomSeparator => {
                        self.plan.status.take().unwrap_or_default()
                    }
                    (Tool::Column | Tool::Beam | Tool::Stair | Tool::Railing | Tool::Footing | Tool::Object | Tool::Sweep | Tool::Planting, _) => self
                        .plan
                        .status
                        .take()
                        .unwrap_or_else(|| format!("Added on {level_name}.")),
                    (Tool::Section | Tool::Elevation, Some(_)) => "Click the second corner.".into(),
                    (Tool::Guide, Some(_)) if self.plan.guide_edge.is_some() => "Click where the parallel guide goes, or type how far across and press Enter.".into(),
                    (Tool::Guide, Some(_)) => "Click a second point the guide runs through, or type a length and press Enter.".into(),
                    _ => format!("Added on {level_name}."),
                };
            }
        } else if from.is_none() && response.hovered() && !tool.draws_outline() {
            self.status = format!(
                "{} on {level_name} (the open plan's level). {}",
                tool.label(),
                tool.hint()
            );
        }
    }

    /// While drawing in 3D, a point snapped to and rested on for a moment
    /// joins the points the next one lines up with (the last four), as in
    /// plans; a frame is asked for after the dwell, as nothing repaints
    /// while the pointer rests (MU24).
    fn rest_3d(&mut self, ctx: &egui::Context, snap: Option<&crate::snaps::Snap>, drawing: bool) {
        rest_points(&mut self.rested_3d, ctx, snap, drawing);
    }
}

/// The temporary guides of drafting, on a level's plane in 3D (MU37: "make
/// sure drafting temp guides also appear when modeling"): level, plumb and
/// angled through each point rested on, and angled through `from`.
fn guides_3d(
    rested: &[DVec3],
    from: Option<DVec3>,
    z: f64,
    step: f64,
    cfg: &crate::snaps::SnapSettings,
) -> Vec<crate::snaps::TempGuide> {
    if !cfg.allows(SnapKind::OnTempGuide) || (rested.is_empty() && from.is_none()) {
        return vec![];
    }
    let refs: Vec<snaps::Reference> = rested
        .iter()
        .map(|p| snaps::Reference::Point {
            p: *p,
            kind: SnapKind::Endpoint,
        })
        .collect();
    let mut g = snaps::temp_guides(&refs, from, step);
    snaps::lift(&mut g, z);
    g
}

/// [`CreaBimApp::rest_3d`] on the rested points alone, for callers that
/// hold another part of the app.
#[allow(clippy::type_complexity)]
fn rest_points(
    rested: &mut (Vec<DVec3>, Option<(DVec3, f64)>),
    ctx: &egui::Context,
    snap: Option<&crate::snaps::Snap>,
    drawing: bool,
) {
    {
        const DWELL: f64 = 0.4;
        const KEEP: usize = 4;
        if !drawing {
            *rested = (vec![], None);
            return;
        }
        let now = ctx.input(|i| i.time);
        let Some(p) = snap.filter(|s| s.kind.is_point()).map(|s| s.p) else {
            rested.1 = None;
            return;
        };
        match rested.1 {
            Some((q, since)) if q.distance(p) < 1e-9 => {
                let pts = &mut rested.0;
                if now - since > DWELL && !pts.iter().any(|r| r.distance(p) < 1e-9) {
                    pts.push(p);
                    if pts.len() > KEEP {
                        pts.remove(0);
                    }
                }
            }
            _ => {
                rested.1 = Some((p, now));
                ctx.request_repaint_after(std::time::Duration::from_millis(450));
            }
        }
    }
}

impl CreaBimApp {
    /// The Modifiers tab (MU27): each modifier a card, as Blender's: a
    /// header line (fold, its name, shown or not, its menu, ×) and, open,
    /// its settings; Add modifier above them.
    /// The Parking tab of hard landscape: its car park.
    fn parking_tab(&mut self, ui: &mut egui::Ui, id: ElementId) {
        let Some(before) = self.doc.model().get(id).map(|e| e.kind.clone()) else {
            return;
        };
        let mut k = before.clone();
        let label = crate::partstool::parking_tab(ui, self.doc.model(), &mut k);
        crate::props::flush_later(ui);
        if !label.is_empty() && k != before {
            let r = self.doc.edit_merged(id, label, |e| e.kind = k.clone());
            self.report(r, format!("{label}."));
        }
    }

    /// The Tree tab: a grown tree's generator. Its Worked out plate and ?
    /// box come last, after every block.
    /// A viewport's title on its sheet (MU27): its view's, so every sheet
    /// it is on agrees. A fold of its own after the viewport's settings.
    fn viewport_title(&mut self, ui: &mut egui::Ui, view_id: ElementId) {
        let view_name = self
            .doc
            .model()
            .get(view_id)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        // its title on the sheet (MU27): its view's, so every
        // sheet it is on agrees
        if let Some(ElementKind::View(view)) = self.doc.model().get(view_id).map(|e| e.kind.clone())
        {
            let mut t = view.sheet_title.clone();
            let before = t.clone();
            // its own block, apart from the viewport's settings
            // (MU55-03); on and off as check boxes under one
            // label, as Blender's Show In (DS-01 4.3)
            let open = self.prop_header(
                ui,
                "viewport-title",
                "Title on the sheet",
                true,
                Some("sheets#view-titles"),
            );
            crate::props::fold(ui, open, |ui| {
                let mut shown = !t.hidden;
                if crate::props::check(ui, "Show", &mut shown, "The title")
                    .on_hover_text("Its title under the drawing; every sheet it is on agrees")
                    .changed()
                {
                    t.hidden = !shown;
                }
                ui.add_enabled_ui(!t.hidden, |ui| {
                    crate::props::check(ui, "", &mut t.show_number, "Number in a circle");
                    crate::props::check(ui, "", &mut t.show_scale, "Scale under it");
                });
                if !t.hidden {
                    crate::props::row(ui, "Words", |ui| {
                        crate::props::text_field(
                            ui,
                            egui::TextEdit::singleline(&mut t.text)
                                .hint_text(egui::RichText::new(view_name.clone()).italics()),
                            f32::INFINITY,
                        )
                        .on_hover_text("Empty, the view's name")
                    });
                    crate::props::row(ui, "Size", |ui| {
                        crate::props::fill(
                            ui,
                            crate::units::paper_small(&mut t.size).range(1.5..=10.0),
                        )
                        .on_hover_text("Its letters' height on paper")
                    });
                    // its font, weight and colour: a text style
                    let styles: Vec<(ElementId, String)> = self
                        .doc
                        .model()
                        .iter()
                        .filter(|e| matches!(e.kind, ElementKind::TextStyle(_)))
                        .map(|e| (e.id, e.name.clone()))
                        .collect();
                    let now = t
                        .style
                        .and_then(|s| styles.iter().find(|(i, _)| *i == s))
                        .map_or("Plain".to_string(), |(_, n)| n.clone());
                    crate::props::Combo::salted("Text style", "viewport.title")
                        .selected_text(now)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(t.style.is_none(), "Plain").clicked() {
                                t.style = None;
                            }
                            for (i, n) in &styles {
                                if ui.selectable_label(t.style == Some(*i), n).clicked() {
                                    t.style = Some(*i);
                                }
                            }
                        });
                    for (name, v) in [
                        ("Moved across", &mut t.offset.x),
                        ("Moved up", &mut t.offset.y),
                    ] {
                        crate::props::row(ui, name, |ui| {
                            crate::props::fill(ui, crate::units::paper(v)).on_hover_text(
                                "From its place under the drawing's lower left, on paper",
                            )
                        });
                    }
                    if t.offset != DVec2::ZERO
                        && crate::props::action(
                            ui,
                            "",
                            "Back in place",
                            "Under the drawing's lower left again",
                            true,
                        )
                        .clicked()
                    {
                        t.offset = DVec2::ZERO;
                    }
                    crate::props::howto(ui, "Viewport title: drag it on the sheet to move it.");
                }
            });
            if t != before {
                let id = view_id;
                let r = self.doc.edit_merged(id, "View title", |e| {
                    if let ElementKind::View(v) = &mut e.kind {
                        v.sheet_title = t.clone();
                    }
                });
                if let Err(e) = r {
                    self.status = format!("Couldn't do that: {e}");
                }
            }
        }
    }

    fn site_tab(&mut self, ui: &mut egui::Ui, id: ElementId) {
        let Some(before) = self.doc.model().get(id).map(|e| e.kind.clone()) else {
            return;
        };
        let mut k = before.clone();
        let label = crate::partstool::site_tab(ui, self.doc.model(), &mut k);
        crate::props::flush_later(ui);
        if !label.is_empty() && k != before {
            // a value dragged is one undo step, not one per frame
            let r = self.doc.edit_merged(id, label, |e| e.kind = k.clone());
            self.report(r, format!("{label}."));
        }
    }

    /// The Tree tab: a grown tree's generator. Its Worked out plate and ?
    /// box come last, after every block.
    fn tree_tab(&mut self, ui: &mut egui::Ui, id: ElementId) {
        let Some(before) = self.doc.model().get(id).map(|e| e.kind.clone()) else {
            return;
        };
        let mut k = before.clone();
        let label = crate::partstool::tree_tab(ui, self.doc.model(), &mut k);
        crate::props::flush_later(ui);
        if !label.is_empty() && k != before {
            // a slider dragged is one undo step, not one per frame
            let r = self.doc.edit_merged(id, label, |e| e.kind = k.clone());
            self.report(r, format!("{label}."));
        }
    }

    fn modifiers_tab(&mut self, ui: &mut egui::Ui, id: ElementId) {
        use creabim_core::{Modifier, ModifierKind};
        let Some(ElementKind::FreeMesh(before)) = self.doc.model().get(id).map(|e| e.kind.clone())
        else {
            crate::props::empty(ui, "This element takes no modifiers yet.");
            return;
        };
        let mut m = before.clone();
        let mut label: &'static str = "";
        // what the modifiers can reach in the model: paths to follow,
        // levels to slice at, in the mesh's own axes
        let model = self.doc.model();
        let lift = |level: Option<ElementId>| {
            level
                .and_then(|l| model.level(l))
                .map_or(0.0, |l| l.elevation)
        };
        let home = m.origin + DVec3::Z * lift(m.level);
        let local = |w: DVec3| m.to_local(w - home);
        let paths: Vec<(String, Vec<DVec3>)> = model
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::Sketch(sk) => {
                    let z = DVec3::Z * lift(sk.level);
                    let mut pts: Vec<DVec3> = sk.segments().iter().map(|(a, _)| *a + z).collect();
                    if let Some((_, b)) = sk.segments().last() {
                        pts.push(*b + z);
                    }
                    Some((e.name.clone(), pts.into_iter().map(local).collect()))
                }
                ElementKind::Guide(g) => {
                    let z = DVec3::Z * lift(g.level);
                    Some((e.name.clone(), vec![local(g.a + z), local(g.b + z)]))
                }
                _ => None,
            })
            .filter(|(_, p)| p.len() >= 2)
            .collect();
        let level_heights: Vec<f64> = {
            let mut h: Vec<f64> = model
                .iter()
                .filter_map(|e| match &e.kind {
                    ElementKind::Level(l) => Some(local(DVec3::Z * l.elevation).z),
                    _ => None,
                })
                .collect();
            h.sort_by(f64::total_cmp);
            h
        };
        let (lo, hi) = creabim_core::modgen::bounds(&m.mesh);
        let middle = (lo + hi) / 2.0;
        // + Add modifier: grouped, each with its icon and what it is for
        egui::ComboBox::from_id_salt(("add_modifier", id))
            .icon(crate::props::combo_icon)
            .selected_text("+ Add modifier")
            .width(ui.available_width())
            .height(600.0)
            .show_ui(ui, |ui| {
                ui.set_min_width(180.0);
                let mut group = "";
                for (g, k, hint) in ModifierKind::menu() {
                    if g != group {
                        group = g;
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(g.to_uppercase()).small().weak());
                    }
                    // the icon in the toolbar's quiet grey, what it is
                    // for in a tooltip, so the list stays narrow (MU39)
                    let r = ui
                        .horizontal(|ui| {
                            let muted = crate::theme::Palette::of(ui.ctx()).muted;
                            let (ir, _) = ui
                                .allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                            crate::svgicon::paint(ui.painter(), ir.shrink(2.0), k.icon(), muted);
                            ui.selectable_label(false, k.label())
                        })
                        .inner
                        .on_hover_text(hint);
                    if r.clicked() {
                        let mut k = k;
                        // a sensible start for this mesh: round its middle,
                        // mirrored at its middle, cut at the project's levels
                        match &mut k {
                            ModifierKind::RadialArray { center, .. } => {
                                *center = glam::DVec2::new(hi.x + (hi.x - lo.x), middle.y);
                            }
                            ModifierKind::Mirror { plane, .. } => *plane = hi.x,
                            ModifierKind::Slice { heights, .. } if !level_heights.is_empty() => {
                                *heights = level_heights.clone();
                            }
                            _ => {}
                        }
                        m.modifiers.push(Modifier::new(k));
                        label = "Add modifier";
                    }
                }
            });
        if m.modifiers.is_empty() {
            crate::props::empty(ui, "No modifiers yet: Add modifier above.");
            crate::props::info(
                ui,
                "A modifier changes the shape without touching the mesh: repeat it, panel it, round its corners, slice it into floors. Switch it off or apply it any time.",
                "",
            );
        }
        let count = m.modifiers.len();
        let mut action: Option<(usize, i32)> = None;
        let p = crate::theme::Palette::of(ui.ctx());
        // the card worked in (its guides show in the views), and a centre
        // to pick in a plan
        let mut focus = self.mod_focus.filter(|f| f.0 == id).map(|f| f.1);
        let mut pick_centre = None;
        for (i, md) in m.modifiers.iter_mut().enumerate() {
            let key = egui::Id::new(("mod.open", id, i));
            let open = ui.ctx().data(|d| d.get_temp::<bool>(key)).unwrap_or(true);
            let mut touched = false;
            let card = egui::Frame::new()
                .fill(crate::props::box_fill(ui.visuals()))
                .stroke(egui::Stroke::new(1.0, p.border))
                .corner_radius(6.0)
                .inner_margin(egui::Margin::symmetric(6, 4))
                .show(ui, |ui| {
                    // folded or open, a card is as wide as the panel
                    ui.set_min_width(ui.available_width());
                    // one row that never wraps: the title gives way instead
                    ui.horizontal(|ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                        let chev = if open {
                            "chevron-down"
                        } else {
                            "chevron-right"
                        };
                        if crate::props::icon_button(ui, chev, if open { "Fold" } else { "Open" })
                            .clicked()
                        {
                            ui.ctx().data_mut(|d| d.insert_temp(key, !open));
                        }
                        // its icon is its switch: lit while it shows, grey
                        // when off (one button less in a narrow panel)
                        let (ir, sw) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
                        let ink = if md.enabled {
                            p.accent
                        } else {
                            ui.visuals().weak_text_color()
                        };
                        if sw.hovered() {
                            ui.painter().rect_filled(ir, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
                        }
                        crate::svgicon::paint(ui.painter(), ir.shrink(1.0), md.kind.icon(), ink);
                        if sw
                            .on_hover_text(if md.enabled {
                                "Showing: click to switch it off (the mesh without it)"
                            } else {
                                "Off: click to show it again"
                            })
                            .clicked()
                        {
                            md.enabled = !md.enabled;
                            label = "Switch modifier";
                        }
                        // its name (and, folded, its main settings) in what
                        // the three buttons leave, cut short, never under them
                        let mut job = egui::text::LayoutJob::default();
                        let font = egui::TextStyle::Body.resolve(ui.style());
                        job.append(
                            md.kind.label(),
                            0.0,
                            egui::TextFormat::simple(font.clone(), ui.visuals().strong_text_color()),
                        );
                        if !open {
                            job.append(
                                &format!("  {}", md.kind.summary()),
                                0.0,
                                egui::TextFormat::simple(
                                    egui::FontId::proportional(font.size - 1.5),
                                    ui.visuals().weak_text_color(),
                                ),
                            );
                        }
                        // the title in a box of its own, then ⋯ and ×, one
                        // after the other: nothing drawn over the title
                        let w = (ui.available_width() - 3.0 * (20.0 + ui.spacing().item_spacing.x) - 4.0).max(24.0);
                        let (tr, tresp) = ui.allocate_exact_size(egui::vec2(w, 20.0), egui::Sense::hover());
                        let galley = ui.fonts_mut(|f| {
                            job.wrap = egui::text::TextWrapping::truncate_at_width(w);
                            f.layout_job(job)
                        });
                        ui.painter().galley(
                            egui::pos2(tr.min.x, tr.center().y - galley.size().y / 2.0),
                            galley,
                            ui.visuals().text_color(),
                        );
                        tresp.on_hover_text(md.kind.summary());
                        // what it does, as the ? beside Properties' headings
                        // (MU39: not a block of small text in the card)
                        ui.add(
                            egui::Button::new(egui::RichText::new("?").size(11.0).strong())
                                .small()
                                .corner_radius(8.0)
                                .min_size(egui::vec2(16.0, 16.0)),
                        )
                        .on_hover_ui(|ui| {
                            ui.set_max_width(280.0);
                            ui.label(egui::RichText::new(md.kind.label()).strong());
                            ui.label(modifier_help(&md.kind));
                        });
                        {
                            let more = crate::props::icon_button(ui, "ellipsis", "Apply, duplicate, move");
                            egui::Popup::menu(&more).show(|ui| {
                                if i == 0
                                    && ui
                                        .button("Apply")
                                        .on_hover_text(
                                            "Make it part of the mesh, so you can edit the result",
                                        )
                                        .clicked()
                                {
                                    action = Some((i, 2));
                                    ui.close();
                                }
                                if ui.button("Duplicate").clicked() {
                                    action = Some((i, 3));
                                    ui.close();
                                }
                                if i > 0 && ui.button("Move up").clicked() {
                                    action = Some((i, -1));
                                    ui.close();
                                }
                                if i + 1 < count && ui.button("Move down").clicked() {
                                    action = Some((i, 1));
                                    ui.close();
                                }
                            });
                            if crate::props::icon_button(ui, "x", "Remove").clicked() {
                                action = Some((i, 0));
                            }
                        }
                    });
                    if !open {
                        return;
                    }
                    fn changed(ui: &mut egui::Ui, name: &str, w: egui::DragValue) -> bool {
                        crate::props::row(ui, name, |ui| crate::props::fill(ui, w).changed())
                    }
                    fn len(v: &mut f64) -> egui::DragValue<'_> {
                        crate::units::drag(v)
                    }
                    fn deg(v: &mut f64) -> egui::DragValue<'_> {
                        egui::DragValue::new(v).speed(0.5).suffix("°")
                    }
                    fn factor(v: &mut f64) -> egui::DragValue<'_> {
                        egui::DragValue::new(v).speed(0.01).range(0.0..=10.0).max_decimals(3)
                    }
                    fn two(ui: &mut egui::Ui, a: (&str, &str), b: (&str, &str)) -> Option<usize> {
                        match crate::props::actions2(ui, "", (a.0, a.1, true), (b.0, b.1, true)) {
                            (true, _) => Some(0),
                            (_, true) => Some(1),
                            _ => None,
                        }
                    }
                    let what = md.kind.label();
                    let mut edited = false;
                    match &mut md.kind {
                        ModifierKind::Array {
                            count,
                            offset,
                            relative,
                            fit_length,
                            turn,
                            grow,
                        } => {
                            let mut fit = fit_length.is_some();
                            if crate::props::choice(ui, "Fit", &mut fit, &[(false, "Count", "A number of copies"), (true, "Length", "As many as fit a length")]) {
                                *fit_length = fit.then_some(10.0);
                                edited = true;
                            }
                            match fit_length {
                                Some(l) => edited |= changed(ui, "Length", len(l)),
                                None => edited |= changed(ui, "Count", egui::DragValue::new(count).speed(0.1).range(1..=200)),
                            }
                            if crate::props::choice(ui, "Step", relative, &[(false, "Fixed", "A distance"), (true, "× size", "In multiples of its own size: 1 puts copies end to end")]) {
                                edited = true;
                            }
                            for (n, v) in [("Step X", &mut offset.x), ("Step Y", &mut offset.y), ("Step Z", &mut offset.z)] {
                                edited |= if *relative {
                                    changed(ui, n, egui::DragValue::new(v).speed(0.01).suffix(" ×").max_decimals(3))
                                } else {
                                    changed(ui, n, len(v))
                                };
                            }
                            edited |= changed(ui, "Turn each", deg(turn));
                            edited |= changed(ui, "Grow each", factor(grow));
                        }
                        ModifierKind::RadialArray {
                            count,
                            angle,
                            center,
                            turn_copies,
                        } => {
                            edited |= changed(ui, "Count", egui::DragValue::new(count).speed(0.1).range(1..=360));
                            edited |= changed(ui, "Over", egui::DragValue::new(angle).speed(1.0).range(-360.0..=360.0).suffix("°"));
                            edited |= changed(ui, "Centre X", len(&mut center.x));
                            edited |= changed(ui, "Centre Y", len(&mut center.y));
                            if let Some(w) = two(ui, ("Pick in a plan", "Click the centre in a plan view: a column's middle, a grid crossing"), ("At its middle", "Round its own middle: a ring of copies about where it stands")) {
                                if w == 0 {
                                    pick_centre = Some(i);
                                } else {
                                    *center = middle.truncate();
                                    edited = true;
                                }
                            }
                            if crate::props::switch(ui, turn_copies, "Turn each copy to face the centre").changed() {
                                edited = true;
                            }
                        }
                        ModifierKind::PathArray { path, count, align } => {
                            crate::props::row(ui, "Path", |ui| {
                                egui::ComboBox::from_id_salt(("mod.path", id, i))
                                    .icon(crate::props::combo_icon)
                                    .selected_text(if path.len() < 2 { "Pick a sketch or guide".to_string() } else { format!("{} points", path.len()) })
                                    .width(ui.available_width())
                                    .show_ui(ui, |ui| {
                                        for (name, pts) in &paths {
                                            if ui.selectable_label(false, name).clicked() {
                                                *path = pts.clone();
                                                edited = true;
                                            }
                                        }
                                    });
                            });
                            edited |= changed(ui, "Count", egui::DragValue::new(count).speed(0.1).range(1..=500));
                            if crate::props::switch(ui, align, "Turn each copy along the path").changed() {
                                edited = true;
                            }
                        }
                        ModifierKind::Mirror { axis, plane } => {
                            crate::props::row(ui, "Across", |ui| {
                                crate::props::flow(ui, |ui| {
                                    for a in Axis::ALL {
                                        if ui.selectable_value(axis, a, a.label()).changed() {
                                            edited = true;
                                        }
                                    }
                                })
                            });
                            edited |= changed(ui, "Plane at", len(plane));
                            let k = axis.index();
                            if let Some(w) = two(ui, ("At its middle", "The plane through the middle of the mesh"), ("At its edge", "The plane on its far side: a mirrored twin")) {
                                *plane = if w == 0 { middle[k] } else { hi[k] };
                                edited = true;
                            }
                        }
                        ModifierKind::Panelize {
                            width,
                            height,
                            joint,
                            depth,
                        } => {
                            edited |= changed(ui, "Panel width", len(width));
                            edited |= changed(ui, "Panel height", len(height));
                            edited |= changed(ui, "Joint", len(joint));
                            edited |= changed(ui, "Proud by", len(depth));
                        }
                        ModifierKind::Louvres {
                            sides,
                            upright,
                            spacing,
                            depth,
                            thickness,
                            angle,
                        } => {
                            edited |= crate::props::toggles(
                                ui,
                                "On sides",
                                sides,
                                &[
                                    ("N", "North: the mesh's +Y side before it is turned"),
                                    ("E", "East: its +X side"),
                                    ("S", "South: its −Y side"),
                                    ("W", "West: its −X side"),
                                ],
                            );
                            if crate::props::choice(ui, "Run", upright, &[(true, "Fins", "Upright fins"), (false, "Slats", "Level louvres")]) {
                                edited = true;
                            }
                            edited |= changed(ui, "Spacing", len(spacing));
                            edited |= changed(ui, "Depth", len(depth));
                            edited |= changed(ui, "Thickness", len(thickness));
                            edited |= changed(ui, "Angle", deg(angle));
                        }
                        ModifierKind::Bevel {
                            radius,
                            segments,
                            chamfer,
                            corners,
                        } => {
                            if crate::props::choice(ui, "Kind", chamfer, &[(false, "Round", "A fillet: rounded"), (true, "Cut", "A chamfer: cut straight")]) {
                                edited = true;
                            }
                            edited |= changed(ui, if *chamfer { "Setback" } else { "Radius" }, len(radius));
                            if !*chamfer {
                                edited |= changed(ui, "Segments", egui::DragValue::new(segments).speed(0.1).range(1..=32));
                            }
                            if crate::props::choice(ui, "Corners", corners, &[(creabim_core::modifiers::Corners::All, "All", "Every corner"), (creabim_core::modifiers::Corners::Convex, "Out", "Outside corners only"), (creabim_core::modifiers::Corners::Concave, "In", "Inside corners only")]) {
                                edited = true;
                            }
                        }
                        ModifierKind::Shell { thickness, open } => {
                            use creabim_core::modifiers::OpenFaces as O;
                            edited |= changed(ui, "Thickness", len(thickness));
                            if crate::props::choice_icons(ui, "Open", open, &[(O::None, "shell-closed", "Closed", "hollow inside, open nowhere (a shell round a void; cut a hole with a Boolean)"), (O::Top, "shell-top", "Open top", "a tub, a planter, a pool"), (O::Bottom, "shell-bottom", "Open bottom", "a shade, a cover"), (O::Both, "shell-both", "Open top and bottom", "a tube, a chimney")]) {
                                edited = true;
                            }
                        }
                        ModifierKind::Solidify { thickness } => {
                            let mut out = *thickness < 0.0;
                            let mut t = thickness.abs();
                            if changed(ui, "Thickness", len(&mut t)) {
                                edited = true;
                            }
                            if crate::props::choice(ui, "Grows", &mut out, &[(false, "In", "Behind the faces"), (true, "Out", "In front of the faces")]) {
                                edited = true;
                            }
                            *thickness = if out { -t } else { t };
                        }
                        ModifierKind::Slice { heights, gap } => {
                            edited |= changed(ui, "Gap", len(gap));
                            let mut gone = None;
                            for (k, h) in heights.iter_mut().enumerate() {
                                crate::props::row(ui, &format!("Cut {}", k + 1), |ui| {
                                    // a fixed row in whole pixels: wrapping, it flickered as the panel was resized (MU59)
                                    ui.horizontal(|ui| {
                                        let w = (ui.available_width() - 24.0).floor().max(40.0);
                                        if ui.add_sized([w, 20.0], len(h)).changed() {
                                            edited = true;
                                        }
                                        if crate::props::icon_button(ui, "x", "Take this cut away").clicked() {
                                            gone = Some(k);
                                        }
                                    })
                                });
                            }
                            if let Some(k) = gone {
                                heights.remove(k);
                                edited = true;
                            }
                            if let Some(w) = two(ui, ("+ A cut", "Another cut, 3 m above the last"), ("At the levels", "A cut at each of the project's levels")) {
                                if w == 0 {
                                    let next = heights.last().copied().unwrap_or(lo.z) + 3.0;
                                    heights.push(next);
                                } else {
                                    *heights = level_heights.clone();
                                }
                                edited = true;
                            }
                        }
                        ModifierKind::Taper { x, y } => {
                            edited |= changed(ui, "Top width ×", factor(x));
                            edited |= changed(ui, "Top depth ×", factor(y));
                        }
                        ModifierKind::Twist { angle } => {
                            edited |= changed(ui, "Turn", deg(angle));
                        }
                        ModifierKind::Subdivision { levels, simple } => {
                            edited |= changed(ui, "Levels", egui::DragValue::new(levels).speed(0.05).range(0..=4));
                            if crate::props::choice(ui, "Kind", simple, &[(false, "Smooth", "Rounded, for soft organic forms"), (true, "Split", "Finer faces in the same shape, to edit, panel or twist")]) {
                                edited = true;
                            }
                        }
                    }
                    if edited {
                        touched = true;
                        label = match what {
                            "Array" => "Array settings",
                            "Mirror" => "Mirror settings",
                            _ => "Modifier settings",
                        };
                    }
                });
            if open && (touched || card.response.contains_pointer()) {
                focus = Some(i);
            }
            ui.add_space(3.0);
        }
        self.mod_focus = focus.filter(|f| *f < m.modifiers.len()).map(|f| (id, f));
        if let Some(i) = pick_centre {
            self.mod_focus = Some((id, i));
            self.plan.point_pick = true;
            self.status =
                "Click the centre in a plan. Esc or a right-click leaves it as it is.".into();
        }
        match action {
            Some((i, 0)) => {
                m.modifiers.remove(i);
                label = "Remove modifier";
            }
            Some((i, 1)) => {
                m.modifiers.swap(i, i + 1);
                label = "Reorder modifiers";
            }
            Some((i, -1)) => {
                m.modifiers.swap(i, i - 1);
                label = "Reorder modifiers";
            }
            Some((i, 3)) => {
                let copy = m.modifiers[i].clone();
                m.modifiers.insert(i + 1, copy);
                label = "Duplicate modifier";
            }
            Some((_, _)) => {
                let first = m.modifiers.remove(0);
                m.mesh = creabim_core::modifiers::evaluate(&m.mesh, &[first]).mesh;
                self.face = None;
                label = "Apply modifier";
            }
            None => {}
        }
        if m != before && !label.is_empty() {
            let r = self
                .doc
                .edit_merged(id, label, |e| e.kind = ElementKind::FreeMesh(m.clone()));
            self.report(r, format!("{label}."));
        }
    }

    /// A mass's face tools (MU39): pick a face in a view, then extrude it,
    /// move it, inset it or taper it; or subdivide the whole mesh.
    /// A linear dimension's Text card (MU80–MU82): an override, words
    /// around the number, a note below, its place and Move text.
    fn dim_text_section(&mut self, ui: &mut egui::Ui, id: ElementId) {
        let Some(mut t) = self.doc.model().get(id).and_then(|e| match &e.kind {
            ElementKind::Dimension(d) if d.kind == creabim_core::DimensionKind::Linear => {
                Some(d.label.clone())
            }
            _ => None,
        }) else {
            return;
        };
        let open = self.prop_header(ui, "dim-text", "Text", false, Some("dimensions"));
        let changed =
            crate::props::fold(ui, open, |ui| crate::annotate::dim_text_ui(ui, &mut t)).flatten();
        if let Some(label) = changed {
            let r = self.doc.edit_merged(id, label, |e| {
                if let ElementKind::Dimension(d) = &mut e.kind {
                    d.label = t.clone();
                }
            });
            self.report(r, label);
        }
    }

    fn mesh_edit_section(&mut self, ui: &mut egui::Ui, id: ElementId) {
        let Some(ElementKind::FreeMesh(before)) = self.doc.model().get(id).map(|e| e.kind.clone())
        else {
            return;
        };
        let hint = "say:Click a face of the mass in a view to pick it, then: Extrude pushes it out with new sides (E), Move slides it, Inset draws a smaller face inside its edges (for a reveal, a recessed panel, a parapet: inset the roof face, then push it down), Taper shrinks it. The tools change the mesh under its modifiers.";
        let open = self.prop_header(ui, "meshedit", "Mesh edit", true, Some(hint));
        crate::props::fold(ui, open, |ui| {
            let mut m = before.clone();
            let mut label = "";
            let on = self.face.is_some_and(|f| f < m.mesh.faces.len());
            crate::props::row(ui, "Face", |ui| match self.face.filter(|_| on) {
                Some(f) => ui.label(format!("{} ({} corners)", f + 1, m.mesh.faces[f].len())),
                None => ui.label(egui::RichText::new("click one in a view").weak()),
            });
            crate::props::row(ui, "Distance", |ui| {
                crate::props::fill(ui, crate::units::drag(&mut self.extrude_distance))
                    .on_hover_text("How far Extrude and Move go")
            });
            let (ex, mv) = crate::props::actions2(
                ui,
                "",
                (
                    "Extrude",
                    "Push the face out by the distance, with new sides (E)",
                    on,
                ),
                ("Move", "Move the face along its normal by the distance", on),
            );
            if let (true, Some(f)) = (ex, self.face) {
                m.mesh.extrude_face(f, self.extrude_distance);
                label = "Extrude face";
            }
            if let (true, Some(f)) = (mv, self.face) {
                let n = m.mesh.face_normal(f) * self.extrude_distance;
                let verts = m.mesh.faces[f].clone();
                m.mesh.translate_vertices(&verts, n);
                label = "Move face";
            }
            crate::props::row(ui, "Inset by", |ui| {
                crate::props::fill(
                    ui,
                    crate::units::drag(&mut self.inset_by).range(0.001..=100.0),
                )
                .on_hover_text("How far inside its edges Inset draws the new face")
            });
            let (ins, tap) = crate::props::actions2(
            ui,
            "",
            (
                "Inset",
                "A smaller face inside its edges, joined by a ring of faces: then push it in for a reveal or a parapet",
                on,
            ),
            ("Taper", "Shrink the face to 80% about its centre", on),
        );
            if let (true, Some(f)) = (ins, self.face) {
                if m.mesh.inset_face(f, self.inset_by) {
                    label = "Inset face";
                } else {
                    self.status = format!(
                        "Can't inset that face by {}: it would close up. Try less.",
                        crate::units::len(self.inset_by)
                    );
                }
            }
            if let (true, Some(f)) = (tap, self.face) {
                m.mesh.scale_face(f, 0.8);
                label = "Taper face";
            }
            if crate::props::action(
                ui,
                "Mesh",
                "Subdivide",
                "Every face split in four, the shape smoothed (Catmull-Clark)",
                true,
            )
            .clicked()
            {
                m.mesh = m.mesh.subdivide();
                self.face = None;
                label = "Subdivide mesh";
            }
            if !label.is_empty() && m != before {
                let r = self
                    .doc
                    .edit(id, label, |e| e.kind = ElementKind::FreeMesh(m.clone()));
                self.report(
                r,
                if label == "Inset face" {
                    "Inset the face: it is still picked, so Extrude with a negative distance pushes it in.".to_string()
                } else {
                    format!("{label}.")
                },
            );
            }
        });
    }

    /// The project's notepad (MU28): to-dos and notes, each one row (a tick
    /// or a dot, its first line, its link, then edit and delete tucked at
    /// the right), edited in place and closed by clicking away. Copying and
    /// saving sit in the ⋯ menu; markdown is only how it is written out.
    pub(super) fn notes_pad(&mut self, ui: &mut egui::Ui, only: Option<bool>) {
        use creabim_core::project::PadItem;
        let mut info = self
            .doc
            .model()
            .project_info()
            .map(|(_, p)| p.clone())
            .unwrap_or_default();
        if info.pad.is_empty() && !info.notes.trim().is_empty() {
            info.pad = creabim_core::project::pad_from_markdown(&info.notes);
            info.notes.clear();
        }
        let before = info.clone();
        let edit_key = egui::Id::new("pad.edit");
        let mut editing: Option<usize> = ui.ctx().data(|d| d.get_temp(edit_key)).flatten();
        let mut remove = None;
        let mut follow: Option<ElementId> = None;
        let mut export: Option<&'static str> = None;
        let names: std::collections::HashMap<ElementId, String> = info
            .pad
            .iter()
            .filter_map(|i| i.link)
            .filter_map(|l| self.doc.model().get(l).map(|e| (l, e.name.clone())))
            .collect();
        let target = self.selection.active.or(self.active_view);
        // the ⋯ menu: copy and save, and tidying
        crate::props::flow(ui, |ui| {
            let open = info.pad.iter().filter(|i| i.todo && !i.done).count();
            ui.label(egui::RichText::new(format!("{} items · {open} open", info.pad.len())).weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // the house ellipsis icon: the ⋯ glyph is in none of the fonts
                let more = crate::props::icon_button(ui, "ellipsis", "Copy, save and tidy");
                egui::Popup::menu(&more).show(|ui| {
                    if ui.button("Copy all").clicked() {
                        export = Some("copy");
                        ui.close();
                    }
                    if ui.button("Save as text (.txt)\u{2026}").clicked() {
                        export = Some("txt");
                        ui.close();
                    }
                    if ui.button("Save as Markdown (.md)\u{2026}").clicked() {
                        export = Some("md");
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Remove the done to-dos").clicked() {
                        export = Some("clean");
                        ui.close();
                    }
                });
            });
        });
        for todo in [true, false] {
            if only.is_some_and(|o| o != todo) {
                continue;
            }
            let idx: Vec<usize> = (0..info.pad.len())
                .filter(|k| info.pad[*k].todo == todo)
                .collect();
            let done = idx.iter().filter(|k| info.pad[**k].done).count();
            let title = if todo {
                format!("To-dos  {done}/{}", idx.len())
            } else {
                format!("Notes  {}", idx.len())
            };
            let open = self.prop_header(
                ui,
                if todo { "pad.todos" } else { "pad.notes" },
                &title,
                true,
                None,
            );
            crate::props::fold(ui, open, |ui| {
                if idx.is_empty() {
                    crate::props::empty(ui, "None yet.");
                }
                for k in idx {
                    let item = &mut info.pad[k];
                    if editing == Some(k) {
                        // edited in place: its whole text, then two small
                        // switches; clicking away closes it
                        let r = ui.add(
                            egui::TextEdit::multiline(&mut item.text)
                                .id(egui::Id::new(("pad.text", k)))
                                .desired_rows(2)
                                .desired_width(f32::INFINITY),
                        );
                        // focused as it opens, so typing goes straight in
                        let fk = egui::Id::new("pad.focus");
                        if ui.ctx().data(|d| d.get_temp::<usize>(fk)) == Some(k) {
                            r.request_focus();
                            ui.ctx().data_mut(|d| d.remove::<usize>(fk));
                        }
                        if !r.has_focus()
                            && !r.gained_focus()
                            && ui.input(|i| i.pointer.any_pressed())
                            && !r.hovered()
                        {
                            editing = None;
                        }
                        crate::props::flow(ui, |ui| {
                            crate::props::tickbox(ui, &mut item.todo, "To-do");
                            match item.link {
                            Some(_) => {
                                if ui.small_button("Unlink").clicked() {
                                    item.link = None;
                                }
                            }
                            None => {
                                if ui
                                    .add_enabled(target.is_some(), egui::Button::new("Link to the selection").small())
                                    .on_hover_text("Ties it to the element selected, or else the view in front")
                                    .clicked()
                                {
                                    item.link = target;
                                }
                            }
                        }
                        });
                        continue;
                    }
                    let row = crate::props::flow(ui, |ui| {
                        if todo {
                            crate::props::tickbox(ui, &mut item.done, "");
                        } else {
                            ui.label(egui::RichText::new("\u{2022}").weak());
                        }
                        // tools first, from the right, so they keep their place
                        let mut title_rect = ui.available_rect_before_wrap();
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::props::icon_button(ui, "trash-2", "Delete").clicked() {
                                remove = Some(k);
                            }
                            if crate::props::icon_button(ui, "pencil", "Edit").clicked() {
                                editing = Some(k);
                                ui.ctx()
                                    .data_mut(|d| d.insert_temp(egui::Id::new("pad.focus"), k));
                            }
                            if let Some(l) = item.link {
                                let name = names
                                    .get(&l)
                                    .cloned()
                                    .unwrap_or_else(|| "something deleted".into());
                                if crate::props::icon_button(ui, "link", &format!("Go to {name}"))
                                    .clicked()
                                {
                                    follow = Some(l);
                                }
                            }
                            title_rect.max.x = ui.min_rect().left() - 4.0;
                        });
                        title_rect
                    });
                    let r = row.inner;
                    let r = egui::Rect::from_min_max(
                        egui::pos2(r.left(), row.response.rect.top()),
                        egui::pos2(r.right().max(r.left() + 20.0), row.response.rect.bottom()),
                    );
                    let resp = ui.interact(r, egui::Id::new(("pad.row", k)), egui::Sense::click());
                    let text = if item.title().is_empty() {
                        "(empty)"
                    } else {
                        item.title()
                    };
                    let color = if item.done {
                        ui.visuals().weak_text_color()
                    } else {
                        ui.visuals().text_color()
                    };
                    let galley = ui.painter().layout_no_wrap(
                        text.to_string(),
                        egui::FontId::proportional(12.0),
                        color,
                    );
                    let painter = ui.painter().with_clip_rect(r);
                    let wide = galley.size().x > r.width();
                    let y = r.center().y - galley.size().y / 2.0;
                    painter.galley(egui::pos2(r.left(), y), galley, color);
                    if item.done {
                        painter.line_segment(
                            [
                                egui::pos2(r.left(), r.center().y),
                                egui::pos2(r.right(), r.center().y),
                            ],
                            egui::Stroke::new(1.0, color.gamma_multiply(0.6)),
                        );
                    }
                    if wide {
                        let bg = ui.visuals().panel_fill;
                        for s in 0..14 {
                            let x = r.right() - 14.0 + s as f32;
                            painter.line_segment(
                                [egui::pos2(x, r.top()), egui::pos2(x, r.bottom())],
                                egui::Stroke::new(1.0, bg.gamma_multiply(s as f32 / 14.0)),
                            );
                        }
                    }
                    if resp.on_hover_text(item.text.as_str()).double_clicked() {
                        editing = Some(k);
                        ui.ctx()
                            .data_mut(|d| d.insert_temp(egui::Id::new("pad.focus"), k));
                    }
                }
            });
        }
        // a new item, as a to-do or a note
        let key = egui::Id::new("notes.new");
        let kind_key = egui::Id::new("notes.new.todo");
        let mut new: String = ui.ctx().data(|d| d.get_temp(key)).unwrap_or_default();
        let mut as_todo: bool = ui.ctx().data(|d| d.get_temp(kind_key)).unwrap_or(true);
        if let Some(o) = only {
            as_todo = o;
        }
        ui.add_space(4.0);
        crate::props::flow(ui, |ui| {
            let r = {
                let w = (ui.available_width() - 64.0).max(60.0);
                crate::props::text_field(
                    ui,
                    egui::TextEdit::singleline(&mut new).hint_text(if as_todo {
                        "New to-do, Enter"
                    } else {
                        "New note, Enter"
                    }),
                    w,
                )
            };
            if only.is_none()
                && ui
                    .selectable_label(as_todo, "To-do")
                    .on_hover_text("Add it as a to-do (else a note)")
                    .clicked()
            {
                as_todo = !as_todo;
            }
            if r.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !new.trim().is_empty()
            {
                info.pad.push(PadItem {
                    text: new.trim().to_string(),
                    todo: as_todo,
                    ..Default::default()
                });
                new.clear();
                r.request_focus();
            }
        });
        ui.ctx().data_mut(|d| {
            d.insert_temp(key, new);
            d.insert_temp(kind_key, as_todo);
        });
        if let Some(k) = remove {
            info.pad.remove(k);
            editing = None;
        }
        ui.ctx().data_mut(|d| d.insert_temp(edit_key, editing));
        if let Some(what) = export {
            let model = self.doc.model();
            let all: Vec<&PadItem> = info.pad.iter().collect();
            let md = creabim_core::project::pad_to_markdown(&all, &|id| {
                model.get(id).map(|e| e.name.clone())
            });
            match what {
                "copy" => {
                    ui.ctx().copy_text(md);
                    self.status = "Notes copied.".into();
                }
                "clean" => info.pad.retain(|i| !(i.todo && i.done)),
                ext => {
                    let stem = stem(&self.path);
                    if let Some(path) = self.export_to(format!("{stem} notes.{ext}")) {
                        self.status = match std::fs::write(&path, &md) {
                            Ok(()) => format!("Saved the notes to {path}."),
                            Err(e) => format!("Couldn't save: {e}"),
                        };
                    }
                }
            }
        }
        if info != before {
            let tx = creabim_core::set_project(self.doc.model(), info);
            if let Err(e) = self.doc.commit_merged(tx) {
                self.status = format!("Couldn't do that: {e}");
            }
        }
        if let Some(l) = follow {
            match self.doc.model().get(l).map(|e| &e.kind) {
                Some(ElementKind::View(_)) => self.active_view = Some(l),
                Some(_) => {
                    self.selection.only(l);
                    self.prop_tab = PropTab::Element;
                }
                None => self.status = "What it was linked to is gone.".into(),
            }
        }
    }

    /// After an edit, say so if a selected wall now overlaps another along
    /// its length (MU27): it looks merged in plan but stays two in 3D.
    fn warn_overlaps(&mut self) {
        let rev = self.doc.model().revision();
        if self.overlap_seen == rev {
            return;
        }
        self.overlap_seen = rev;
        let picked = self.selection.all();
        if picked.is_empty() {
            return;
        }
        let pairs = creabim_core::checks::overlapping_walls(self.doc.model());
        if let Some((a, b)) = pairs
            .iter()
            .find(|(a, b)| picked.contains(a) || picked.contains(b))
        {
            let name = |id: &ElementId| {
                self.doc
                    .model()
                    .get(*id)
                    .map_or("a wall".into(), |e| e.name.clone())
            };
            self.status = format!(
                "{} now overlaps {} along its length: one wall in plan, two in 3D. Manage \u{203a} Overlapping walls merges them.",
                name(a),
                name(b)
            );
        }
    }

    fn note_editor_window(&mut self, ctx: &egui::Context) {
        self.warn_overlaps();
        if let Some((from, g)) = self.copy_guides.take() {
            let others: Vec<ElementId> = self
                .doc
                .model()
                .iter()
                .filter(|e| e.id != from && matches!(e.kind, ElementKind::Sheet(_)))
                .map(|e| e.id)
                .collect();
            let mut tx = creabim_core::Transaction::new("Copy guides to all sheets");
            for sid in &others {
                if let Some(e) = self.doc.model().get(*sid).cloned() {
                    let mut after = e.clone();
                    if let ElementKind::Sheet(sh) = &mut after.kind {
                        sh.guides = g.clone();
                    }
                    tx = tx.replace(e, after);
                }
            }
            let r = self.doc.commit(tx);
            self.report(
                r,
                format!("Guides copied to {} other sheets.", others.len()),
            );
        }
        if let Some(id) = self.plan.edit_note.take() {
            match self.doc.model().get(id).map(|e| &e.kind) {
                Some(ElementKind::Note(n)) if n.section.is_some() => {
                    let key = n.section.clone().unwrap_or_default();
                    self.open_notes(&key);
                }
                _ => self.note_editor = Some(id),
            }
        }
        let Some(id) = self.note_editor else { return };
        let Some(ElementKind::Note(n)) = self.doc.model().get(id).map(|e| e.kind.clone()) else {
            self.note_editor = None;
            return;
        };
        let mut n = n;
        let mut open = true;
        let mut changed = false;
        crate::app::shell::window("Note text")
            .id(egui::Id::new(("note-editor", id)))
            .open(&mut open)
            .default_size([520.0, 420.0])
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                crate::props::flow(ui, |ui| {
                    changed |= crate::props::tickbox(ui, &mut n.markdown, "Markdown").changed();
                    ui.label(
                        egui::RichText::new("# heading  - bullet  1. numbered  - [ ] to do").weak(),
                    );
                });
                egui::ScrollArea::vertical().show(ui, |ui| {
                    changed |= ui
                        .add(
                            egui::TextEdit::multiline(&mut n.text)
                                .desired_width(f32::INFINITY)
                                .desired_rows(18)
                                .font(egui::TextStyle::Monospace),
                        )
                        .changed();
                });
            });
        if changed {
            let r = self
                .doc
                .edit_merged(id, "Note text", |e| e.kind = ElementKind::Note(n.clone()));
            if let Err(e) = r {
                self.status = format!("Couldn't do that: {e}");
            }
        }
        if !open {
            self.note_editor = None;
        }
    }

    /// Drawing a slab, or redrawing a slab or wall, in the 3D view, on its
    /// level, with snaps.
    fn outline_3d(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
        ray: &dyn Fn(egui::Pos2) -> (DVec3, DVec3),
    ) {
        let Some(outline) = self.plan.outline.clone() else {
            return;
        };
        let here = response.hovered();
        let typing = ui.ctx().egui_wants_keyboard_input();
        let (enter, escape, shift, backspace) = ui.input(|i| {
            (
                here && (i.key_pressed(egui::Key::Enter)
                    || !typing && !i.modifiers.any() && i.key_pressed(egui::Key::C)),
                here && i.key_pressed(egui::Key::Escape),
                i.modifiers.shift,
                here && !typing && i.key_pressed(egui::Key::Backspace),
            )
        });
        let exclude: Vec<ElementId> = outline.target.into_iter().collect();
        self.refresh_snaps_3d(&exclude);
        let snap_cfg = self.plan.snap.live(ui.input(|i| i.modifiers.alt));
        let hover = response.hover_pos().and_then(|h| {
            let (o, d) = ray(h);
            let free = Workplane::ground(outline.z).intersect(o, d);
            let from = outline.points.last().map(|p| p.extend(outline.z));
            let snapper = &self.snaps_3d.as_ref()?.2;
            let shape: Vec<DVec3> = outline.points.iter().map(|p| p.extend(outline.z)).collect();
            let guides = guides_3d(
                &self.rested_3d.0,
                from,
                outline.z,
                self.plan.guide_step,
                &snap_cfg,
            );
            let rested: Vec<DVec3> = if guides.is_empty() {
                self.rested_3d
                    .0
                    .iter()
                    .map(|p| p.truncate().extend(outline.z))
                    .collect()
            } else {
                vec![]
            };
            let track = snaps::track_points(&shape, &rested);
            let hit = snapper.snap_guided(&snap_cfg, h, project, from, &track, &guides, free)?;
            snaps::draw_guides(painter, project, &guides, &hit.guides);
            let (s, align) = (hit.snap, hit.align);
            if let Some(c) = project(s.p.truncate().extend(outline.z)) {
                if !hit.guides.is_empty() {
                    snaps::draw_guided(painter, project, &hit, &guides, from.and_then(project));
                } else if s.kind != SnapKind::Grid {
                    snaps::draw_tracked(painter, project, s, align, c, from.and_then(project));
                }
            }
            Some((h, s.p.truncate(), s))
        });
        outline.draw(painter, project, hover.map(|h| h.1));
        // resting on a point lines the next corner up with it (MU24)
        self.rest_3d(ui.ctx(), hover.as_ref().map(|h| &h.2), true);
        let hover = hover.map(|(h, p, _)| (h, p));
        let click = match (response.clicked(), hover) {
            (true, Some((h, p))) => Some((p, outline.near_first(h, project))),
            _ => None,
        };
        let right = response.secondary_clicked() && !shift;
        let arc = self.plan.slab_arc;
        let enter = enter || std::mem::take(&mut self.plan.close_outline);
        let Some(o) = self.plan.outline.as_mut() else {
            return;
        };
        match o.input_more(
            enter,
            escape,
            right,
            click,
            arc,
            response.double_clicked(),
            backspace,
        ) {
            crate::outline::Next::Drawing(hint) => self.status = hint,
            crate::outline::Next::Cancel(msg) => {
                self.plan.outline = None;
                self.status = msg.into();
            }
            crate::outline::Next::Finish(closed) => {
                if let Some(o) = self.plan.outline.take() {
                    match o.finish(&mut self.doc, closed) {
                        Ok((msg, made)) => {
                            if let Some(id) = made {
                                self.selection.only(id);
                                self.face = None;
                            }
                            self.status = msg;
                        }
                        Err(e) => self.status = format!("Couldn't draw that: {e}"),
                    }
                }
            }
        }
    }

    /// The selected guide a press at `p` in the 3D view is on, with the
    /// height of its level: a drag from there moves it.
    fn guide_pressed_3d(
        &self,
        p: egui::Pos2,
        project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
    ) -> Option<(ElementId, f64)> {
        let id = self
            .guide_near(p, project)
            .filter(|g| self.selection.contains(*g))?;
        match self.doc.model().get(id).map(|e| &e.kind) {
            Some(ElementKind::Guide(g)) if !self.hidden_in_3d().contains(&id) => Some((id, g.a.z)),
            _ => None,
        }
    }

    /// Each guide shown in 3D as a mark a box can take: the line between
    /// its two points, as a flat triangle on screen.
    fn guide_marks_3d(
        &self,
        project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
    ) -> Vec<(ElementId, [egui::Pos2; 3])> {
        let left_out = self.hidden_in_3d();
        self.doc
            .model()
            .iter()
            .filter(|e| !left_out.contains(&e.id))
            .filter_map(|e| match &e.kind {
                ElementKind::Guide(g) => {
                    let (a, b) = (project(g.a)?, project(g.b)?);
                    Some((e.id, [a, b, b]))
                }
                _ => None,
            })
            .collect()
    }

    /// The guide within a few pixels of `p` in the 3D view.
    fn guide_near(
        &self,
        p: egui::Pos2,
        project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
    ) -> Option<ElementId> {
        let mut best: Option<(ElementId, f32)> = None;
        for e in self.doc.model().iter() {
            let ElementKind::Guide(g) = &e.kind else {
                continue;
            };
            let Some(dir) = (g.b - g.a).try_normalize() else {
                continue;
            };
            let m = (g.a + g.b) / 2.0;
            let steps = 240;
            let pts: Vec<Option<egui::Pos2>> = (0..=steps)
                .map(|i| {
                    let t = -GUIDE_REACH_3D + 2.0 * GUIDE_REACH_3D * i as f64 / steps as f64;
                    project(m + dir * t)
                })
                .collect();
            for w in pts.windows(2) {
                let (Some(a), Some(b)) = (w[0], w[1]) else {
                    continue;
                };
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                let d = (a + ab * t).distance(p);
                if d < 5.0 && best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((e.id, d));
                }
            }
        }
        best.map(|(id, _)| id)
    }

    /// Every sketch as lines over the 3D view, and every guide dashed; the
    /// selected one stands out.
    fn draw_sketches(
        &self,
        painter: &egui::Painter,
        project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
    ) {
        let left_out = self.hidden_in_3d();
        for e in self.doc.model().iter() {
            let ElementKind::Guide(g) = &e.kind else {
                continue;
            };
            if left_out.contains(&e.id) {
                continue;
            }
            let stroke = if self.selection.contains(e.id) {
                egui::Stroke::new(2.0, planview::accent_selected())
            } else {
                egui::Stroke::new(1.0, planview::GUIDE_COLOR)
            };
            draw_guide_3d(painter, project, g.a, g.b, stroke);
        }
        // a closed sketch is a face: a light fill inside its outline (A7-1)
        for e in self.doc.model().iter() {
            let ElementKind::Sketch(sk) = &e.kind else {
                continue;
            };
            if !sk.is_face() || left_out.contains(&e.id) {
                continue;
            }
            let outline = sk.outline();
            if outline.len() < 3 || creabim_core::polygon::self_intersects(&outline, true) {
                continue;
            }
            let z = sk
                .level
                .and_then(|l| self.doc.model().level(l))
                .map_or(0.0, |l| l.elevation);
            let pts: Option<Vec<egui::Pos2>> = outline
                .iter()
                .map(|p| project(sk.plane.to_model(*p) + DVec3::Z * z))
                .collect();
            let Some(pts) = pts else {
                continue;
            };
            let fill = if self.selection.contains(e.id) {
                planview::accent_selected().gamma_multiply(0.24)
            } else {
                Color32::from_rgba_unmultiplied(40, 70, 140, 40)
            };
            let mut mesh = egui::Mesh::default();
            for p in &pts {
                mesh.colored_vertex(*p, fill);
            }
            for t in creabim_core::polygon::triangulate(&outline) {
                mesh.add_triangle(t[0] as u32, t[1] as u32, t[2] as u32);
            }
            painter.add(egui::Shape::mesh(mesh));
        }
        for (id, a, b) in creabim_geom::scene::sketch_lines(self.doc.model()) {
            if left_out.contains(&id) {
                continue;
            }
            let selected = self.selection.contains(id);
            let stroke = if selected {
                egui::Stroke::new(2.5, planview::accent_selected())
            } else {
                egui::Stroke::new(1.5, Color32::from_rgb(40, 70, 140))
            };
            if let (Some(a), Some(b)) = (project(a), project(b)) {
                painter.line_segment([a, b], stroke);
            }
        }
        crate::sketch::draw_marks(
            painter,
            self.doc.model(),
            self.selection.active,
            project,
            &|_| true,
        );
    }

    /// The Sketch tool in the 3D view: the first click sets the workplane on
    /// the face under the pointer (or the ground), later clicks add corners.
    fn sketch_3d(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
        ray: &dyn Fn(egui::Pos2) -> (DVec3, DVec3),
        cam: &OrbitCamera,
    ) {
        // a sketch being drawn in a 2D pane is left alone until clicking here
        let mine = self.plan.sketch.as_ref().is_some_and(|d| d.pane.is_none());
        if !mine && response.clicked() {
            self.plan.sketch = None;
        }
        // the Sketch tool's own Line / Arc / Circle; an outline tool has its
        // bar's (MU31: both set the shape each frame, so an arc's end was
        // dropped as soon as it was clicked)
        if self.plan.tool == Tool::Sketch {
            let draft = self.plan.sketch.as_mut().filter(|d| d.pane.is_none());
            if let Some(hint) = crate::sketch::shape_controls(ui, response.rect, None, draft) {
                self.status = hint;
            }
        }
        // a slab, roof or other outline tool drawn on a face: its own bar's
        // Line / Arc choice, not the Sketch tool's (MU27: Arc did nothing)
        if self.plan.tool != Tool::Sketch {
            if response.hovered() && !ui.ctx().egui_wants_keyboard_input() {
                let (a, l) = ui.input(|i| {
                    let plain = !i.modifiers.command && !i.modifiers.alt;
                    (
                        plain && i.key_pressed(egui::Key::A),
                        plain && i.key_pressed(egui::Key::L),
                    )
                });
                if a || l {
                    self.plan.slab_arc = a;
                }
            }
            let want = if self.plan.slab_arc {
                crate::sketch::Shape::Arc
            } else {
                crate::sketch::Shape::Line
            };
            if let Some(d) = self.plan.sketch.as_mut() {
                if d.shape != want {
                    d.arc_end = None;
                    d.shape = want;
                }
            }
        }
        let (enter, escape, shift, alt) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
                i.modifiers.shift,
                i.modifiers.alt,
            )
        });
        let hover = response.hover_pos();
        // corners, edges and centres of what is modelled; Shift keeps lines square instead
        self.refresh_snaps_3d(&[]);
        let snap_cfg = self.plan.snap.live(alt || shift);
        let last = self
            .plan
            .sketch
            .as_ref()
            .filter(|_| mine)
            .and_then(|d| Some(d.plane.to_model(*d.points.last()?)));
        let shape: Vec<DVec3> = self
            .plan
            .sketch
            .as_ref()
            .filter(|_| mine)
            .map(|d| d.points.iter().map(|p| d.plane.to_model(*p)).collect())
            .unwrap_or_default();
        let track = snaps::track_points(&shape, &self.rested_3d.0);
        let plane = self.plan.sketch.as_ref().filter(|_| mine).map(|d| d.plane);
        // the next point lines up along the plane drawn on: on a wall's
        // face, along the wall and straight up (they ran along the model's
        // X and Y only, so an upright face had no vertical guide)
        let _axes = plane.map(|p| snaps::TrackAxes::on(p.x, p.y));
        let tracked = hover.and_then(|h| {
            let snapper = &self.snaps_3d.as_ref()?.2;
            // on the sketch's plane, the pointer's point lines up with its corners
            let (o, d) = ray(h);
            let free = plane.and_then(|p| p.intersect(o, d));
            let (s, align) = snapper.snap_tracked(&snap_cfg, h, project, last, &track, free)?;
            // only real snaps here: a free point is placed on the plane below
            (s.kind != SnapKind::Grid).then_some((s, align))
        });
        // only snaps on the plane drawn on: a corner behind the face, seen
        // through it, would land somewhere else once laid on the plane
        let near_plane = |q: DVec3| -> bool {
            let pl = plane.or_else(|| {
                let (o, d) = ray(hover?);
                face_plane(&self.scene, o, d)
            });
            pl.is_none_or(|pl| (q - pl.origin).dot(pl.normal()).abs() < 0.02)
        };
        let tracked = tracked.filter(|(s, _)| near_plane(s.p));
        let snapped = tracked.map(|t| t.0);
        // resting on a point lines the next one up with it (MU24)
        let drawing = self.plan.sketch.is_some() || self.plan.tool != Tool::Select;
        self.rest_3d(ui.ctx(), snapped.as_ref(), drawing);
        if let Some((s, align)) = tracked {
            if let Some(c) = project(s.p) {
                snaps::draw_tracked(painter, project, s, align, c, last.and_then(project));
            }
        }
        // the plane a first click would start on; off every face, a sketch
        // starts on the ground and the other tools on the working level
        let ground = match self.plan.tool {
            Tool::Sketch => 0.0,
            _ => self.work_level().map_or(0.0, |(_, z)| z),
        };
        let (scene, custom) = (&self.scene, self.workplane.as_ref().map(|w| w.0));
        let candidate = |p: egui::Pos2| -> Option<Workplane> {
            let (o, d) = ray(p);
            if let Some(plane) = custom {
                // the first corner is where the pointer meets it
                let at = plane.intersect(o, d)?;
                return Some(Workplane {
                    origin: at,
                    ..plane
                });
            }
            face_plane(scene, o, d).or_else(|| {
                let at = Workplane::ground(ground).intersect(o, d)?;
                Some(Workplane {
                    origin: at,
                    ..Workplane::ground(ground)
                })
            })
        };
        let on_plane = match (&self.plan.sketch, hover) {
            (Some(d), Some(p)) if mine && snapped.is_some() => {
                // back on the first corner closes the outline, whatever
                // else could be snapped to near it (an alignment 100 mm
                // off took the click, and the outline stayed open)
                let (o, dir) = ray(p);
                let closing = d.plane.intersect(o, dir).and_then(|m| {
                    let dist = m.distance(cam.eye().as_dvec3());
                    let tol = dist * 2.0 * (cam.fov_y as f64 / 2.0).tan() * 10.0
                        / response.rect.height() as f64;
                    let first = *d.points.first()?;
                    let q = d.snap(d.plane.to_plane(m), tol, false, false);
                    (d.points.len() >= 3 && q == first).then_some(first)
                });
                closing
                    .or_else(|| snapped.map(|s| d.snap(d.plane.to_plane(s.p), 1e-6, false, false)))
            }
            (Some(d), Some(p)) if mine => {
                let (o, dir) = ray(p);
                d.plane.intersect(o, dir).map(|m| {
                    // 10 px at that distance, for closing on the first corner
                    let dist = m.distance(cam.eye().as_dvec3());
                    let tol = dist * 2.0 * (cam.fov_y as f64 / 2.0).tan() * 10.0
                        / response.rect.height() as f64;
                    d.snap(
                        d.plane.to_plane(m),
                        tol,
                        shift,
                        self.plan.snap.live(alt).allows(SnapKind::Grid),
                    )
                })
            }
            _ => None,
        };
        let accent = crate::theme::interaction();
        // a slab, roof or shaft's Rectangle too, not only a face's (MU31)
        let rect_preview = (crate::partstool::on_face(&self.plan).is_some()
            || self.plan.tool.draws_outline())
            && self.plan.slab_mode == crate::slabtool::SlabMode::Rectangle;
        match self.plan.sketch.as_ref().filter(|_| mine) {
            // a rectangle on a face shows whole as the corner follows (MU24)
            Some(d) if rect_preview && d.points.len() == 1 && on_plane.is_some() => {
                let (a, b) = (d.points[0], on_plane.unwrap_or_default());
                let corners = [a, DVec2::new(b.x, a.y), b, DVec2::new(a.x, b.y), a];
                let pts: Vec<egui::Pos2> = corners
                    .iter()
                    .filter_map(|c| project(d.plane.to_model(*c)))
                    .collect();
                painter.add(egui::Shape::line(pts, egui::Stroke::new(2.0, accent)));
            }
            Some(d) => d.draw(painter, project, on_plane),
            None => {
                if let Some(plane) = hover.and_then(candidate) {
                    crate::sketch::draw_plane(painter, project, &plane, accent);
                }
            }
        }
        let finish = |app: &mut Self, closed: bool| {
            let Some(d) = app.plan.sketch.take() else {
                return;
            };
            // a level slab, or a bare hole in the wall the outline is on
            if matches!(app.plan.tool, Tool::Slab | Tool::Opening) {
                if !closed && app.plan.tool == Tool::Slab {
                    app.plan.sketch = Some(d);
                    app.status = "Close the outline on its first corner to make it.".into();
                    return;
                }
                let r = if app.plan.tool == Tool::Slab {
                    d.finish_slab(&mut app.doc, &app.plan)
                } else {
                    d.finish_opening(&mut app.doc)
                };
                match r {
                    Ok(Some(id)) => {
                        app.selection.only(id);
                        app.face = None;
                        app.status = if app.plan.tool == Tool::Slab {
                            "Slab added with its top on the face. Ctrl+Z to undo."
                        } else {
                            "Opening cut in the wall, the rectangle around what you drew. Set its size and sill in Properties. Ctrl+Z to undo."
                        }
                        .into();
                    }
                    Ok(None) => app.status = "Draw at least two corners.".into(),
                    Err(m) => app.status = m,
                }
                return;
            }
            // a sweep along a path drawn on a face
            if let Some((profile, name)) = crate::partstool::face_sweep(&app.plan, app.doc.model())
            {
                let name = name.as_str();
                match d.finish_sweep(&mut app.doc, profile, name, closed) {
                    Ok(Some(id)) => {
                        app.selection.only(id);
                        app.face = None;
                        app.status = format!("{name} added along the path on the face: its profile stands out of the face. Ctrl+Z to undo.");
                    }
                    Ok(None) => app.status = "A path needs two points.".into(),
                    Err(e) => app.status = format!("Couldn't do that: {e}"),
                }
                return;
            }
            // an extrusion or a void on the face the outline was drawn on
            if let Some((base, height, void)) = crate::partstool::on_face(&app.plan) {
                if !closed {
                    app.plan.sketch = Some(d);
                    app.status = "Close the outline on its first corner to make it.".into();
                    return;
                }
                match d.finish_part(&mut app.doc, base, height, void) {
                    Ok(Some(id)) => {
                        app.selection.only(id);
                        app.face = None;
                        app.status = if void {
                            "Void added on the face: it cuts square into it. Set its depth in Properties. Ctrl+Z to undo."
                        } else {
                            "Extrusion added on the face, standing square out of it. Set its depth in Properties. Ctrl+Z to undo."
                        }
                        .into();
                    }
                    Ok(None) => app.status = "An outline needs three corners.".into(),
                    Err(e) => app.status = format!("Couldn't do that: {e}"),
                }
                return;
            }
            match d.finish(&mut app.doc, closed) {
                Ok(Some(id)) => {
                    // a line across a face of a shape cuts the face in
                    // two, and is itself not kept (MU43, the cut line)
                    // and a closed outline inside one is a face of its own
                    let cut = creabim_core::shape::imprint(app.doc.model(), id);
                    if let Some((before, after, face)) = cut {
                        let (shape, name) = (after.id, after.name.clone());
                        // one step to undo: the line is taken back, the
                        // cut put in its place
                        app.doc.undo();
                        let label = if closed {
                            "Outline on a face"
                        } else {
                            "Cut line"
                        };
                        let tx = creabim_core::Transaction::new(label).replace(before, after);
                        if app.doc.commit(tx).is_ok() {
                            app.selection.only(shape);
                            app.face = Some(face);
                            app.status = if closed {
                                format!("The outline is now a face of {name}, and picked. Pull (P) sinks it for a recess or raises it. Ctrl+Z takes it away.")
                            } else {
                                format!("The line cut a face of {name} in two; one part is picked. Pull (P) takes either part out or in. Ctrl+Z puts the face back.")
                            };
                            return;
                        }
                    }
                    app.selection.only(id);
                    app.face = None;
                    app.status = if closed {
                        "Outline drawn. Pull (P) makes it a solid; G moves it."
                    } else {
                        "Line drawn. Pull (P) makes a surface along it; close it for a solid. Drawn across a face of a shape, from edge to edge, a line cuts the face in two."
                    }
                    .into();
                }
                Ok(None) => {}
                Err(e) => app.status = format!("Couldn't do that: {e}"),
            }
        };
        if !mine && !response.clicked() && !response.secondary_clicked() {
            return;
        }
        if escape {
            self.plan.sketch = None;
            return;
        }
        // a right-click on a shape just started cancels it, as everywhere
        // (MU27: a slab on a face kept going); the Sketch tool's open
        // lines still end on it once they have two points
        if response.secondary_clicked() && self.plan.sketch.is_some() && !on_face_now(&self.plan) {
            let n = self.plan.sketch.as_ref().map_or(0, |d| d.points.len());
            if self.plan.tool == Tool::Sketch && n >= 2 {
                finish(self, false);
            } else if n > 1 {
                // as in plans: takes back the last point
                if let Some(d) = self.plan.sketch.as_mut() {
                    d.points.pop();
                    d.kinds.truncate(d.points.len().saturating_sub(1));
                    d.arc_end = None;
                }
                self.status = format!("Took back a point ({} left). Right-click again for more; with none left it cancels.", n - 1);
            } else {
                self.plan.sketch = None;
                self.status = "Cancelled. Right-click again to leave the tool.".into();
            }
            return;
        }
        // an extrusion's or a void's outline on a face (MU23): a
        // right-click with under three corners cancels it, with three or
        // more closes and makes it; again with nothing drawn, it leaves
        // the tool, as everywhere
        let on_face = crate::partstool::on_face(&self.plan).is_some();
        if response.secondary_clicked()
            && self.plan.sketch.is_none()
            && self.plan.tool != Tool::Select
        {
            self.plan.step_back();
            if let Some(s) = self.plan.status.take() {
                self.status = s;
            }
            return;
        }
        if on_face && response.secondary_clicked() {
            let corners = self.plan.sketch.as_ref().map_or(0, |d| d.points.len());
            if corners < 3 {
                self.plan.sketch = None;
                self.status = "Cancelled. Right-click again to leave the tool.".into();
            } else {
                finish(self, true);
            }
            return;
        }
        if enter || (response.secondary_clicked() && self.plan.sketch.is_some()) {
            finish(self, false);
            return;
        }
        if !response.clicked() {
            return;
        }
        let rect_on_face = (crate::partstool::on_face(&self.plan).is_some()
            || self.plan.tool.draws_outline())
            && self.plan.slab_mode == crate::slabtool::SlabMode::Rectangle;
        match &mut self.plan.sketch {
            None => {
                if let Some(mut plane) = hover.and_then(candidate) {
                    // start on the snapped point, kept on the face's plane
                    if let Some(s) = snapped {
                        plane.origin = plane.to_model(plane.to_plane(s.p));
                    }
                    let mut draft = SketchDraft::new(plane, None);
                    // an outline tool's own Line / Arc from the first click
                    // (MU31: the hint and first segment read the Sketch tool's)
                    let shape = match (self.plan.tool, self.plan.slab_arc) {
                        (Tool::Sketch, _) => crate::sketch::shape(ui.ctx()),
                        (_, true) => crate::sketch::Shape::Arc,
                        (_, false) => crate::sketch::Shape::Line,
                    };
                    draft.shape = shape;
                    // what each outline mode does on a face (MU24)
                    let on_face = crate::partstool::on_face(&self.plan).is_some();
                    let outline = on_face || self.plan.tool.draws_outline();
                    self.status = match (outline, self.plan.slab_mode) {
                        (true, crate::slabtool::SlabMode::Rectangle) => {
                            "Workplane set. Click the opposite corner of the rectangle.".into()
                        }
                        (true, crate::slabtool::SlabMode::FillRoom | crate::slabtool::SlabMode::FromSketch) => format!(
                            "Workplane set. Fill room and From sketch work on a level; on a face, click the corners. {}",
                            crate::sketch::hint(shape, Some(&draft))
                        ),
                        _ => format!("Workplane set. {}", crate::sketch::hint(shape, Some(&draft))),
                    };
                    self.plan.sketch = Some(draft);
                }
            }
            Some(d) => {
                // Rectangle (2) of an extrusion or a void: the second click
                // is the opposite corner (MU23)
                if let (Some(p), true, 1) = (on_plane, rect_on_face, d.points.len()) {
                    let a = d.points[0];
                    if (p.x - a.x).abs() > 1e-6 && (p.y - a.y).abs() > 1e-6 {
                        d.points = vec![a, DVec2::new(p.x, a.y), p, DVec2::new(a.x, p.y)];
                        d.kinds = vec![creabim_core::sketch::SegmentKind::Line; 4];
                        finish(self, true);
                    }
                    return;
                }
                if let Some(p) = on_plane {
                    match d.click(p) {
                        Some(Click::Closed) => finish(self, true),
                        Some(Click::Added) => self.status = crate::sketch::hint(d.shape, Some(d)),
                        None => {}
                    }
                }
            }
        }
    }

    fn show_plan(&mut self, ui: &mut egui::Ui, rect: Rect, background: Color32) {
        let _perf = Perf::new("show_plan");
        let Some(view) = self.active_view else { return };
        #[cfg(feature = "live-mcp")]
        self.track_live_pane(ui, rect, true);
        self.rect_2d = Some(rect);
        // what is drawn in it is built in the phase it stands at
        if self.view(view).is_some() {
            self.work_in_phase(phasesui::PhaseOf::View(view));
        }
        let (before, revision) = (self.selection.clone(), self.doc.model().revision());
        let schedule = match self.view(view).map(|v| &v.kind) {
            Some(ViewKind::Schedule { category, by_type }) => Some((*category, *by_type)),
            _ => None,
        };
        let result = if let Some((category, by_type)) = schedule {
            let csv = with_extension(
                &self.path,
                &format!("{}.csv", category.label().to_lowercase()),
            );
            // on dark paper too, as sheets are (MU29)
            let dark = self.dark_paper(ui.ctx()) && !self.print_preview;
            let layer = ui.layer_id();
            let from = ui.ctx().graphics(|g| g.get(layer).map(|l| l.next_idx()));
            let export = self.schedule_export.take();
            let msg = crate::schedulepane::show(
                ui,
                rect,
                &mut self.doc,
                view,
                (category, by_type),
                &mut self.selection,
                (&csv, &with_extension(&self.path, "xlsx")),
                export.as_deref(),
            );
            if dark {
                darken_shapes(ui.ctx(), layer, from);
            }
            if let Some(m) = msg {
                self.status = m;
            }
            None
        } else if self.is_sheet(view) {
            let views = self.placed_views(view);
            // Print preview shows the sheet as the export would make it:
            // on white when the last export was Print on white
            let info = SheetInfo {
                print_white: self.print_preview && self.sheets_export.white,
                ..self.sheet_info()
            };
            // a sheet follows dark paper while you work on it; Print
            // preview shows it as it prints, on white (MU21)
            // a master with paper of its own shows it, dark paper or not
            let own_paper = sheets::sheet_of(self.doc.model(), view).is_some_and(|s| {
                !creabim_geom::sheetpaper::sheet_paper(self.doc.model(), s, info.print_white)
                    .is_plain()
            });
            let dark = self.dark_paper(ui.ctx()) && !self.print_preview && !own_paper;
            let layer = ui.layer_id();
            let from = ui.ctx().graphics(|g| g.get(layer).map(|l| l.next_idx()));
            let r = sheetview::show(
                ui,
                rect,
                &mut self.doc,
                view,
                &views,
                &info,
                self.sheets.entry(self.area_now).or_default(),
                &mut self.plan,
                &mut self.selection,
            );
            if dark {
                darken_shapes(ui.ctx(), layer, from);
            }
            r
        } else {
            let Some(kind) = self.view(view).map(planview::pane_kind) else {
                return;
            };
            let _pp = Perf::new("drawing_live");
            let Some(mut drawing) = self.drawing_live(view) else {
                return;
            };
            drop(_pp);
            let _pr = Perf::new("show_plan: hidden and guides");
            if !self.hidden.is_empty() {
                drawing.items.retain(|it| !self.hidden.contains(&it.id()));
            }
            if self.shell.settings.guides_hidden {
                // kept in the model, left out of every view (and so not snapped to)
                drawing.items.retain(|it| {
                    !matches!(
                        it,
                        PlanItem::Line {
                            pen: Pen::Guide,
                            ..
                        }
                    )
                });
            }
            drop(_pr);
            // its palette (pens stage 7): drawn as on white paper, then
            // given the palette's colours, as dark paper turns greys over
            let palette = self
                .view(view)
                .map(|v| creabim_core::styles::view_palette(self.doc.model(), v))
                .filter(|p| !p.is_ink() && !self.print_preview)
                .map(|p| p.roles());
            let style = PaneStyle {
                background: Some(if palette.is_some() {
                    Color32::WHITE
                } else {
                    background
                }),
                decorations: true,
                camera: None,
                scale: self.view(view).map_or(100.0, |v| v.scale),
            };
            // dark paper (A4-3): the pane is drawn as on light paper, then
            // its greys turn over; colours (selection, dimensions, the
            // accent) and pictures keep theirs
            let dark = self.dark_paper(ui.ctx()) && palette.is_none();
            let layer = ui.layer_id();
            let from = ui.ctx().graphics(|g| g.get(layer).map(|l| l.next_idx()));
            let _pp = Perf::new("planview::show");
            let r = planview::show(
                ui,
                rect,
                &mut self.doc,
                view,
                kind,
                &drawing,
                &mut self.plan,
                &mut self.selection,
                style,
            );
            drop(_pp);
            if let Some(roles) = &palette {
                let _pd = Perf::new("show_plan: palette");
                crate::paletteshapes::apply(ui.ctx(), layer, from, roles);
            } else if dark {
                let _pd = Perf::new("show_plan: dark paper");
                darken_shapes(ui.ctx(), layer, from);
            }
            r
        };
        let _pt = Perf::new("show_plan: after");
        // clear of the column of view buttons
        self.shape_strip(ui, rect, 40.0, egui::Id::new(view));
        if let Some(err) = result {
            self.status = format!("Couldn't do that: {err}");
        } else if self.doc.model().revision() != revision {
            if let Some(label) = self.doc.undo_label() {
                self.status = format!("{label}. Ctrl+Z to undo.");
                if label.starts_with("Add Section") || label.starts_with("Add Elevation") {
                    self.status
                        .push_str(" Open it from the View list or the Views group.");
                }
            }
        }
        if self.selection != before {
            self.face = None;
        }
        if self.selection != before || self.doc.model().revision() != revision {
            // other views drew this frame before the edit; draw them again
            ui.ctx().request_repaint();
        }
        let name = self
            .doc
            .model()
            .get(view)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let label = match (self.view(view), sheets::sheet_of(self.doc.model(), view)) {
            (Some(_), _) if schedule.is_some() => format!("{name}  ·  schedule"),
            (Some(v), _) => format!(
                "{name}  ·  {}",
                creabim_core::standards::scale_label(self.doc.model(), v.scale)
            ),
            (_, Some(s)) => format!(
                "Sheet {} {name}  ·  {}",
                s.number,
                creabim_core::standards::size_name(
                    &creabim_core::standards::of(self.doc.model()),
                    s.width,
                    s.height
                )
                .unwrap_or_else(|| "custom size".into())
            ),
            _ => name,
        };
        // light on dark paper, so it reads in every theme (MU29)
        let ink = if self.dark_paper(ui.ctx()) && !self.print_preview {
            Color32::from_gray(190)
        } else {
            Color32::from_gray(80)
        };
        // cut with … before the zoom column, never past the area's edge
        // (owner: "Sheet A101 Ground Fl" ran under a narrow area's border)
        let mut job = egui::text::LayoutJob::single_section(
            label,
            egui::TextFormat::simple(egui::FontId::proportional(13.0), ink),
        );
        job.wrap =
            egui::text::TextWrapping::truncate_at_width((rect.width() - 8.0 - 52.0).max(20.0));
        let galley = ui.painter().layout_job(job);
        let named = galley.size().x;
        ui.painter()
            .galley(rect.left_top() + Vec2::new(8.0, 8.0), galley, ink);
        // the phase it stands at, beside its name (phases)
        if schedule.is_none() && self.view(view).is_some() {
            self.phase_tag(ui, rect, 8.0 + named + 12.0, phasesui::PhaseOf::View(view));
        }
    }

    // ---- panels ---------------------------------------------------------

    /// Where 3D sketches and guides go: the level, a face, three points,
    /// with an offset along the plane's normal.
    fn workplane_bar(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label("Workplane:").on_hover_text(
            "Where sketches and guides drawn in 3D go. Plans draw on their level and sections on the section plane.",
        );
        let text = match &self.workplane {
            Some((_, name)) => name.clone(),
            None => "Level".into(),
        };
        egui::ComboBox::from_id_salt("workplane")
            .icon(crate::props::combo_icon)
            .selected_text(text)
            .width(ui.available_width().min(150.0))
            .truncate()
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(self.workplane.is_none(), "Level (follows the plan)")
                    .clicked()
                {
                    self.workplane = None;
                    self.geometry_key = None;
                    self.status = "Workplane: the working level.".into();
                }
                if ui.button("Pick a face…").clicked() {
                    self.plane_pick = Some(PlanePick::Face);
                }
                if ui.button("Through three points…").clicked() {
                    self.plane_pick = Some(PlanePick::Points(vec![]));
                }
                let level = self.work_level();
                if let Some((_, z)) = level {
                    if ui.button("Level plane you can offset").clicked() {
                        self.set_workplane(
                            Workplane::ground(z),
                            format!("level at {}", crate::units::elevation(z)),
                        );
                    }
                }
            });
        if let Some((plane, _)) = self.workplane.as_mut() {
            let mut d = 0.0;
            let r = ui
                .add(crate::units::drag(&mut d).prefix("Offset ± "))
                .on_hover_text("Drag to slide the workplane along its normal");
            if r.changed() && d != 0.0 {
                plane.origin += plane.normal() * d;
                self.geometry_key = None;
            }
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        crate::props::flow(ui, |ui| {
            for t in Tool::ALL {
                if ui
                    .selectable_label(self.plan.tool == t, t.label())
                    .on_hover_text(t.hint())
                    .clicked()
                {
                    // a detail path being drawn is kept, not dropped
                    crate::detailtool::keep(&mut self.doc, &mut self.plan, &mut self.selection);
                    self.plan.tool = t;
                    self.plan.cancel();
                }
            }
            ui.separator();
            ui.label("Pivot:").on_hover_text(
                "What R and S turn and scale about. Shift+right-click places the pivot cursor.",
            );
            egui::ComboBox::from_id_salt("pivot")
                .icon(crate::props::combo_icon)
                .selected_text(self.plan.pivot.label())
                .width(ui.available_width().min(150.0))
                .truncate()
                .show_ui(ui, |ui| {
                    for p in crate::modal::Pivot::ALL {
                        ui.selectable_value(&mut self.plan.pivot, p, p.label());
                    }
                });
            self.workplane_bar(ui);
            snaps::snap_bar(ui, &mut self.plan.snap);
            ui.separator();
            ui.label("View:");
            let current = self
                .active_view
                .and_then(|id| self.doc.model().get(id))
                .map(|e| e.name.clone())
                .unwrap_or_default();
            let views = self.views();
            let sheet_list = self.sheet_list();
            let current = match self
                .active_view
                .and_then(|id| sheets::sheet_of(self.doc.model(), id))
            {
                Some(s) => format!("{} {current}", s.number),
                None => current,
            };
            egui::ComboBox::from_id_salt("view")
                .icon(crate::props::combo_icon)
                .selected_text(current)
                .width(ui.available_width().min(190.0))
                .truncate()
                .show_ui(ui, |ui| {
                    for (id, name, _) in views {
                        ui.selectable_value(&mut self.active_view, Some(id), name);
                    }
                    if !sheet_list.is_empty() {
                        ui.separator();
                        ui.label(egui::RichText::new("Sheets").weak());
                    }
                    for (id, name) in sheet_list {
                        ui.selectable_value(&mut self.active_view, Some(id), name);
                    }
                });
            if let Some(id) = self.active_view.filter(|id| self.is_sheet(*id)) {
                self.place_view_combo(ui, id);
            }
            if let Some(id) = self.active_view {
                if let Some(mut v) = self.view(id).cloned() {
                    let shadable = matches!(
                        v.kind,
                        ViewKind::Section { .. } | ViewKind::Axonometric { .. }
                    );
                    let (text, value, unit) = match &mut v.kind {
                        ViewKind::Plan { cut_height, .. } => ("Cut height:", cut_height, " m"),
                        ViewKind::Section { depth, .. } => ("Depth:", depth, " m"),
                        ViewKind::Axonometric { altitude, .. } => ("Altitude:", altitude, "°"),
                        ViewKind::Schedule { .. }
                        | ViewKind::LongSection { .. }
                        | ViewKind::BoreholeLog { .. }
                        | ViewKind::WalkSection { .. } => return,
                    };
                    ui.label(text);
                    let field = if unit == "°" {
                        egui::DragValue::new(value)
                            .speed(0.05)
                            .range(-89.0..=89.0)
                            .suffix(unit)
                    } else {
                        crate::units::drag(value).range(0.1..=200.0)
                    };
                    let changed = ui.add(field).changed();
                    let restyled = shadable && view_style_combo(ui, "classic_style", &mut v.style);
                    if changed || restyled {
                        let r = self.doc.edit_merged(id, "Change view", |e| {
                            e.kind = ElementKind::View(v.clone())
                        });
                        self.report(r, "View changed.");
                    }
                    if ui
                        .button("Tag doors and windows")
                        .on_hover_text("Tag every door and window in this view that has no tag yet")
                        .clicked()
                    {
                        let r = planview::tag_openings(&mut self.doc, id);
                        self.status = match r {
                            Ok(0) => "Every door and window here is tagged already.".into(),
                            Ok(n) => {
                                format!("Tagged {n} doors and windows. Drag a tag to move it.")
                            }
                            Err(e) => format!("Couldn't do that: {e}"),
                        };
                    }
                    if matches!(v.kind, ViewKind::Plan { .. })
                        && ui
                            .button("Tag fixtures")
                            .on_hover_text(
                                "Tag every socket, switch, light, drain, radiator and other fixture on this plan with its type's mark",
                            )
                            .clicked()
                    {
                        let r = planview::tag_fixtures(&mut self.doc, id);
                        self.status = match r {
                            Ok(0) => "Every fixture here is tagged already.".into(),
                            Ok(n) => format!("Tagged {n} fixtures. Drag a tag to move it."),
                            Err(e) => format!("Couldn't do that: {e}"),
                        };
                    }
                }
            }
            ui.label("Wall thickness:");
            ui.add(crate::units::drag(&mut self.plan.wall_thickness).range(0.05..=1.0));
            egui::ComboBox::from_id_salt("wall_location")
                .icon(crate::props::combo_icon)
                .selected_text(self.plan.wall_location.label())
                .show_ui(ui, |ui| {
                    for l in creabim_core::LocationLine::ALL {
                        ui.selectable_value(&mut self.plan.wall_location, l, l.label());
                    }
                });
            crate::props::tickbox(ui, &mut self.plan.wall_flip, "Exterior left");
        });
    }

    /// "Place view" picker shown while a sheet is open.
    fn place_view_combo(&mut self, ui: &mut egui::Ui, sheet: ElementId) {
        let placed: Vec<ElementId> = self
            .doc
            .model()
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::Viewport(vp) if vp.sheet == sheet => Some(vp.view),
                _ => None,
            })
            .collect();
        let mut chosen = None;
        egui::ComboBox::from_id_salt("place_view")
            .icon(crate::props::combo_icon)
            .selected_text("Place view…")
            .width(ui.available_width().min(150.0))
            .truncate()
            .show_ui(ui, |ui| {
                for (id, name, _) in self.views() {
                    let label = if placed.contains(&id) {
                        format!("{name} (placed)")
                    } else {
                        name
                    };
                    if ui.selectable_label(false, label).clicked() {
                        chosen = Some(id);
                    }
                }
            });
        if let Some(view) = chosen {
            self.place_view(sheet, view);
        }
    }

    fn new_sheet(&mut self) {
        let used: Vec<String> = sheets::sheets(self.doc.model())
            .into_iter()
            .map(|(_, s)| s.number.clone())
            .collect();
        let number = (301..)
            .map(|n| format!("A{n}"))
            .find(|n| !used.contains(n))
            .expect("unbounded");
        let std = creabim_core::standards::of(self.doc.model());
        let paper = std
            .sheet(&std.default_sheet)
            .cloned()
            .unwrap_or_else(|| creabim_core::standards::iso().sheets[3].clone());
        let e = Element::new(
            "New Sheet",
            ElementKind::Sheet(Sheet::sized(number, &paper)),
        );
        let id = e.id;
        let r = self.doc.commit(Transaction::new("New sheet").insert(e));
        self.active_view = Some(id);
        self.selection.only(id);
        self.report(
            r,
            "New A3 sheet. Use Place view… in the toolbar to put views on it.",
        );
    }

    /// Open the long section of the drain laid along `path`, making it
    /// the first time.
    pub(super) fn open_long_section(&mut self, path: &[DVec2]) {
        let model = self.doc.model();
        let Some(drain) = model.iter().find(|e| {
            matches!(&e.kind, ElementKind::Site(s)
                if matches!(&s.form, creabim_core::SiteForm::Drain(d) if d.path == path))
        }) else {
            return;
        };
        let (id, name) = (drain.id, drain.name.clone());
        let made = model.iter().find(|e| {
            matches!(&e.kind, ElementKind::View(v)
                if v.kind == ViewKind::LongSection { drain: id })
        });
        if let Some(v) = made {
            self.active_view = Some(v.id);
            return;
        }
        let e = Element::new(
            format!("{name} long section"),
            ElementKind::View(View::long_section(id)),
        );
        let v = e.id;
        let r = self
            .doc
            .commit(Transaction::new("New long section").insert(e));
        self.active_view = Some(v);
        self.report(
            r,
            "New long section. It follows the drain as it changes; place it on a sheet with Place view….",
        );
    }

    /// Open the log of the investigation point at `at`, making it the
    /// first time.
    pub(super) fn open_borehole_log(&mut self, at: DVec2) {
        let model = self.doc.model();
        let Some(probe) = model.iter().find(|e| {
            matches!(&e.kind, ElementKind::Site(s)
                if s.as_probe().is_some_and(|p| p.position == at))
        }) else {
            return;
        };
        let (id, name) = (probe.id, probe.name.clone());
        let made = model.iter().find(|e| {
            matches!(&e.kind, ElementKind::View(v)
                if v.kind == ViewKind::BoreholeLog { probe: id })
        });
        if let Some(v) = made {
            self.active_view = Some(v.id);
            return;
        }
        let e = Element::new(
            format!("{name} log"),
            ElementKind::View(View::borehole_log(id)),
        );
        let v = e.id;
        let r = self
            .doc
            .commit(Transaction::new("New borehole log").insert(e));
        self.active_view = Some(v);
        self.report(
            r,
            format!("New {name} log. It follows the borehole as its log changes; place it on a sheet with Place view…."),
        );
    }

    /// Open the walk-line section of stair `id`, making it the first time.
    pub(super) fn open_walk_section(&mut self, id: ElementId) {
        let model = self.doc.model();
        let Some(name) = model.get(id).map(|e| e.name.clone()) else {
            return;
        };
        let made = model.iter().find(|e| {
            matches!(&e.kind, ElementKind::View(v)
                if v.kind == ViewKind::WalkSection { stair: id })
        });
        if let Some(v) = made {
            self.active_view = Some(v.id);
            return;
        }
        let e = Element::new(
            format!("{name} walk-line section"),
            ElementKind::View(View::walk_section(id)),
        );
        let v = e.id;
        let r = self
            .doc
            .commit(Transaction::new("New walk-line section").insert(e));
        self.active_view = Some(v);
        self.report(
            r,
            format!("New walk-line section of {name}. It follows the stair as it changes; place it on a sheet with Place view…."),
        );
    }

    /// A stair's storeys: repeat it up through typical floors, or take a
    /// copy out of its stack.
    pub(super) fn stair_storeys_ui(&mut self, ui: &mut egui::Ui, id: ElementId) {
        use creabim_core::stairstack;
        let model = self.doc.model();
        let foot = stairstack::foot(model, id);
        let all = stairstack::stack(model, foot);
        let Some(ElementKind::Stair(fs)) = model.get(foot).map(|e| &e.kind) else {
            return;
        };
        let base = creabim_core::members::elevation(model, fs.level);
        let top = all
            .last()
            .and_then(|s| match model.get(*s).map(|e| &e.kind) {
                Some(ElementKind::Stair(s)) => creabim_core::members::level_above(model, s.level),
                _ => None,
            });
        let mut above: Vec<(f64, ElementId, String)> = model
            .levels()
            .filter(|(_, l)| l.elevation > base + 1e-6)
            .map(|(e, l)| (l.elevation, e.id, e.name.clone()))
            .collect();
        above.sort_by(|a, b| a.0.total_cmp(&b.0));
        let shown = top
            .and_then(|l| model.get(l))
            .map_or("-".to_string(), |e| e.name.clone());
        let mut pick = None;
        crate::props::row(ui, "Repeat to", |ui| {
            let w = ui.available_width();
            egui::ComboBox::from_id_salt("stair.repeat")
                .icon(crate::props::combo_icon)
                .width(w)
                .truncate()
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for (_, l, n) in &above {
                        if ui.selectable_label(Some(*l) == top, n).clicked() {
                            pick = Some(*l);
                        }
                    }
                })
                .response
                .on_hover_text(
                    "Repeat this stair on every floor up to this level, the same on each: \
                     change any one and they all change. The level just above it means \
                     this floor only.",
                );
        });
        if all.len() > 1 {
            // the stack on the plate: how many, and on a copy the stair
            // the copies follow
            let mut rows = vec![crate::props::Worked::new("Storeys", all.len().to_string())];
            if id != foot {
                let foot_name = model.get(foot).map_or(String::new(), |e| e.name.clone());
                rows.push(crate::props::Worked::new("Typical of", foot_name));
            }
            crate::props::worked_later(ui, "stair", &rows);
        }
        let copy = id != foot && all.len() > 1;
        if copy
            && crate::props::action(
                ui,
                "Stack",
                "Release this storey",
                "Take this stair out of the stack as it stands, to change it on its own.",
                true,
            )
            .clicked()
        {
            if let Some(tx) = stairstack::release(self.doc.model(), id) {
                let r = self.doc.commit(tx);
                self.report(
                    r,
                    "Released: this stair changes on its own now. Ctrl+Z undoes it.",
                );
            }
        }
        if let Some(l) = pick.filter(|l| Some(*l) != top) {
            match stairstack::repeat_to(self.doc.model(), id, l) {
                Ok(tx) => {
                    let r = self.doc.commit(tx);
                    let n = stairstack::stack(self.doc.model(), foot).len();
                    self.report(
                        r,
                        format!(
                            "The stair now runs up {n} storey{}, the same on each.",
                            if n == 1 { "" } else { "s" }
                        ),
                    );
                }
                Err(e) => self.status = format!("Couldn't repeat it: {e}."),
            }
        }
    }

    /// A new sheet documenting stair `id`, opened.
    pub(super) fn new_stair_sheet(&mut self, id: ElementId) {
        match creabim_geom::stairsheet::stair_sheet(self.doc.model(), id) {
            Ok((tx, sheet)) => {
                let r = self.doc.commit(tx);
                if r.is_ok() {
                    self.active_view = Some(sheet);
                }
                self.report(
                    r,
                    "New stair sheet: both plans, the walk-line section, a 3D view and the stair schedule. Move or crop any view as on any sheet.",
                );
            }
            Err(e) => self.status = format!("Couldn't make a stair sheet: {e}"),
        }
    }

    fn new_schedule(&mut self, category: ScheduleCategory) {
        let label = category.label();
        let e = Element::new(
            category.schedule_name(),
            ElementKind::View(View::schedule(category)),
        );
        let id = e.id;
        let r = self
            .doc
            .commit(Transaction::new(format!("New {} schedule", label.to_lowercase())).insert(e));
        self.active_view = Some(id);
        self.report(
            r,
            "New schedule. Edit cells to change the model, or place it on a sheet with Place view….",
        );
    }

    fn materials(&self) -> Vec<(ElementId, String)> {
        self.doc
            .model()
            .iter()
            .filter(|e| matches!(e.kind, ElementKind::Material(_)))
            .map(|e| (e.id, e.name.clone()))
            .collect()
    }

    /// A material picker; "New material" adds one and picks it.
    /// The material an element shows: its first layer's when it is built
    /// of an assembly, else its own (or its type's).
    pub(super) fn material_here(&self, id: ElementId) -> Option<ElementId> {
        let model = self.doc.model();
        let e = model.get(id)?;
        creabim_core::assembly::assembly_of(model, e)
            .and_then(|(_, a)| a.layers.iter().find_map(|l| l.material))
            .or_else(|| creabim_core::material_of(model, e).map(|(m, _)| m.id))
    }

    /// A copy of material `from`, used on `element` in its place: in the
    /// layer that used it, for a layered element (that changes its
    /// assembly), else as its own material.
    fn copy_material_for(&mut self, element: ElementId, from: ElementId) {
        let model = self.doc.model();
        let Some(copy) = crate::matcard::copy_of(model, from) else {
            return;
        };
        let Some(e) = model.get(element) else { return };
        let layered = creabim_core::assembly::assembly_of(model, e)
            .filter(|(_, a)| a.layers.iter().any(|l| l.material == Some(from)))
            .map(|(ae, _)| ae.clone());
        let tx = if let Some(before) = layered {
            let mut after = before.clone();
            if let ElementKind::Assembly(a) = &mut after.kind {
                for l in a.layers.iter_mut().filter(|l| l.material == Some(from)) {
                    l.material = Some(copy.id);
                }
            }
            Transaction::new("Copy material")
                .insert(copy)
                .replace(before, after)
        } else {
            let before = e.clone();
            let mut after = before.clone();
            match Self::material_slot(&mut after.kind) {
                Some(slot) => *slot = Some(copy.id),
                None => return,
            }
            Transaction::new("Copy material")
                .insert(copy)
                .replace(before, after)
        };
        let r = self.doc.commit(tx);
        self.report(r, "A copy of the material, used here only.");
    }

    /// Select every element made of material `m`, its layers included.
    pub(super) fn select_with_material(&mut self, m: ElementId) {
        let model = self.doc.model();
        let ids: Vec<ElementId> = model
            .iter()
            .filter(|e| {
                creabim_core::library::material_slot(e) == Some(Some(m))
                    || creabim_core::assembly::assembly_of(model, e)
                        .is_some_and(|(_, a)| a.layers.iter().any(|l| l.material == Some(m)))
            })
            .map(|e| e.id)
            .collect();
        let name = model.get(m).map_or_else(String::new, |e| e.name.clone());
        let n = ids.len();
        self.selection.extend(ids, false);
        self.status = match n {
            0 => format!("Nothing is made of {name}."),
            1 => format!("1 element made of {name}."),
            n => format!("{n} elements made of {name}."),
        };
    }

    /// Where an element keeps its one material, if it has one.
    fn material_slot(k: &mut ElementKind) -> Option<&mut Option<ElementId>> {
        match k {
            ElementKind::Wall(w) => Some(&mut w.material),
            ElementKind::Slab(s) => Some(&mut s.material),
            ElementKind::Opening(o) => Some(&mut o.material),
            ElementKind::FreeMesh(m) => Some(&mut m.material),
            ElementKind::Ceiling(c) => Some(&mut c.material),
            k => creabim_core::members::material_mut(k),
        }
    }

    /// The Material fold (Mockup_Material_Section_REV00): what an element
    /// is made of, in the same place for every element. A layered one
    /// lists its layers, each with its swatch, and opens its assembly; the
    /// rest carry their one material and say how it draws. Nothing shows
    /// for an element with no material (a void, a spot level). Returns the
    /// undo label of a change.
    fn material_section(&mut self, ui: &mut egui::Ui, e: &mut Element) -> Option<&'static str> {
        use creabim_core::AssemblyUse;
        // a wall, slab or roof is built of an assembly or of one material
        let usage = match &e.kind {
            ElementKind::Wall(w) if w.curtain.is_none() => Some(AssemblyUse::Wall),
            ElementKind::Slab(_) => Some(AssemblyUse::Floor),
            ElementKind::Roof(_) => Some(AssemblyUse::Roof),
            _ => None,
        };
        fn assembly_slot(k: &mut ElementKind) -> Option<&mut Option<ElementId>> {
            match k {
                ElementKind::Wall(w) if w.curtain.is_none() => Some(&mut w.assembly),
                ElementKind::Slab(s) => Some(&mut s.assembly),
                ElementKind::Roof(r) => Some(&mut r.assembly),
                _ => None,
            }
        }
        let layered = |model: &Model, e: &mut Element| {
            assembly_slot(&mut e.kind).and_then(|a| *a).filter(|a| {
                matches!(
                    model.get(*a).map(|e| &e.kind),
                    Some(ElementKind::Assembly(_))
                )
            })
        };
        let assembly = layered(self.doc.model(), e);
        // a void cuts, so it has no material to show
        let void = matches!(&e.kind, ElementKind::Part(p) if p.void);
        // a spot level and a ground line are heights, not matter
        let bare = matches!(&e.kind, ElementKind::Site(s) if matches!(
            s.form,
            creabim_core::SiteForm::Spot { .. }
                | creabim_core::SiteForm::Line(_)
                | creabim_core::SiteForm::Investigation(_)
        ));
        let one =
            assembly.is_none() && !void && !bare && Self::material_slot(&mut e.kind).is_some();
        if usage.is_none() && assembly.is_none() && !one {
            return None;
        }
        let open = self.prop_header(ui, "material", "Material", true, None);
        let mut label = None;
        crate::props::fold(ui, open, |ui| {
            if let (Some(u), Some(slot)) = (usage, assembly_slot(&mut e.kind)) {
                if self.assembly_combo(ui, u, slot) {
                    label = Some("Assembly");
                }
            }
            if let Some(aid) = layered(self.doc.model(), e) {
                let model = self.doc.model();
                let Some(ae) = model.get(aid).cloned() else {
                    return;
                };
                let ElementKind::Assembly(a) = &ae.kind else {
                    return;
                };
                let mut copy: Option<(usize, ElementId)> = None;
                for (i, l) in a.layers.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let h = ui.spacing().interact_size.y;
                        let c = crate::matcard::swatch(
                            ui,
                            model,
                            l.material,
                            ("layer", i),
                            Vec2::new(26.0, h),
                            "this layer",
                        );
                        if let Some(crate::matcard::CopyHere(from)) = c {
                            copy = Some((i, from));
                        }
                        let name = l
                            .material
                            .and_then(|m| model.get(m))
                            .map_or_else(|| "Air gap".to_string(), |m| m.name.clone());
                        // its thickness at the right, the name from the left,
                        // cut to what is left (never the thickness)
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new(crate::units::len(l.thickness)).weak());
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    let mut job = egui::text::LayoutJob::default();
                                    let font = egui::TextStyle::Body.resolve(ui.style());
                                    let strong = ui.visuals().text_color();
                                    let weak = ui.visuals().weak_text_color();
                                    job.append(
                                        &name,
                                        0.0,
                                        egui::TextFormat::simple(font.clone(), strong),
                                    );
                                    job.append(
                                        &format!(" · {}", l.function.label().to_lowercase()),
                                        0.0,
                                        egui::TextFormat::simple(font, weak),
                                    );
                                    ui.add(egui::Label::new(job).truncate());
                                },
                            );
                        });
                    });
                }
                if let Some((i, from)) = copy {
                    if let Some(m) = crate::matcard::copy_of(model, from) {
                        let mut after = ae.clone();
                        if let ElementKind::Assembly(a) = &mut after.kind {
                            if let Some(l) = a.layers.get_mut(i) {
                                l.material = Some(m.id);
                            }
                        }
                        crate::matcard::push(crate::matcard::MatAsk::Insert(m));
                        crate::matcard::push(crate::matcard::MatAsk::Edit(after));
                    }
                }
                ui.add_space(2.0);
                let w = ui.available_width();
                if ui
                    .add_sized([w, 22.0], egui::Button::new("Edit the assembly…"))
                    .on_hover_text("Its layers, thicknesses and order, in the Assemblies window")
                    .clicked()
                {
                    self.open_assemblies(Some(aid));
                }
            } else if let Some(slot) = Self::material_slot(&mut e.kind) {
                if self.material_combo(ui, slot) {
                    label = Some("Material");
                }
                // how it draws, read from its material
                let model = self.doc.model();
                if let Some(ElementKind::Material(m)) =
                    slot.and_then(|m| model.get(m)).map(|e| &e.kind)
                {
                    let surface = m
                        .surface_pattern
                        .as_deref()
                        .unwrap_or("none")
                        .to_lowercase();
                    crate::props::readout(
                        ui,
                        "Drawn",
                        format!("Cut {} · surface {surface}", m.hatch.label().to_lowercase()),
                        "How its material draws: change it on the swatch",
                    );
                }
            }
        });
        label
    }

    fn material_combo(&mut self, ui: &mut egui::Ui, current: &mut Option<ElementId>) -> bool {
        let materials = self.materials();
        let name = current
            .and_then(|id| materials.iter().find(|m| m.0 == id))
            .map_or("By type", |m| m.1.as_str())
            .to_string();
        let mut changed = false;
        let mut create = false;
        let mut copy = None;
        // its swatch opens the material's card (Mockup_Material_Access_REV00)
        crate::props::row(ui, "Material", |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let h = ui.spacing().interact_size.y;
                copy = crate::matcard::swatch(
                    ui,
                    self.doc.model(),
                    *current,
                    "element",
                    Vec2::new(26.0, h),
                    "this element",
                );
                egui::ComboBox::from_id_salt(ui.id().with("material"))
                    .icon(crate::props::combo_icon)
                    .width(ui.available_width())
                    .selected_text(name)
                    .show_ui(ui, |ui| {
                        changed |= ui.selectable_value(current, None, "By type").changed();
                        for (id, n) in &materials {
                            changed |= ui.selectable_value(current, Some(*id), n).changed();
                        }
                        ui.separator();
                        create = ui.button("New material…").clicked();
                    });
            });
        });
        if let Some(crate::matcard::CopyHere(from)) = copy {
            if let Some(e) = crate::matcard::copy_of(self.doc.model(), from) {
                *current = Some(e.id);
                let r = self.doc.commit(Transaction::new("Copy material").insert(e));
                self.report(r, "A copy of the material, used here only.");
                changed = true;
            }
        }
        if create {
            let e = Element::new(
                creabim_core::library::unique_name(self.doc.model(), "New material"),
                ElementKind::Material(Material::new(Hatch::Diagonal, [0.8, 0.8, 0.8])),
            );
            *current = Some(e.id);
            let r = self.doc.commit(Transaction::new("New material").insert(e));
            self.open_materials(*current);
            self.report(r, "New material. Set its look in the material browser.");
            changed = true;
        }
        changed
    }

    fn outliner(&mut self, ui: &mut egui::Ui) {
        let _perf = Perf::new("outliner");
        ui.heading("Model");
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut groups: Vec<(String, Vec<(ElementId, String)>)> = Vec::new();
            for e in self.doc.model().iter() {
                if matches!(e.kind, ElementKind::Units(_)) {
                    continue;
                }
                let class = e.ifc_class().to_string();
                match groups.iter_mut().find(|(c, _)| *c == class) {
                    Some((_, v)) => v.push((e.id, e.name.clone())),
                    None => groups.push((class, vec![(e.id, e.name.clone())])),
                }
            }
            for (class, items) in groups {
                let opens = class == "View" || class == "Sheet";
                let title = match class.as_str() {
                    "View" => format!("Views ({}), click to open", items.len()),
                    "Sheet" => format!("Sheets ({}), click to open", items.len()),
                    _ => format!("{class} ({})", items.len()),
                };
                let id = ui.make_persistent_id(("group", &class));
                // the same 200 ms as every fold (AMD-017)
                ui.style_mut().animation_time = 0.2;
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    class == "IfcBuildingStorey" || opens,
                )
                .show_header(ui, |ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                    Icon::of_class(&class).paint(ui.painter(), r);
                    ui.label(title);
                })
                .body(|ui| {
                    for (id, name) in items {
                        let icon = self.doc.model().get(id).map_or(Icon::Mass, Icon::of);
                        let row = icons::row(ui, icon, self.selection.contains(id), &name);
                        if row.clicked() && ui.input(|i| i.modifiers.shift || i.modifiers.command) {
                            self.selection.toggle(id);
                            self.face = None;
                        } else if row.clicked() {
                            self.selection.only(id);
                            self.face = None;
                            if opens {
                                self.active_view = Some(id);
                            }
                        }
                    }
                });
            }
        });
    }

    /// Booleans (Mockup_Booleans_REV00, A on all four): what adds to or
    /// cuts this element, a folded box each (its kind and the shape as a
    /// chip, × to let go; open for Kind and Reach). A shape also lists what
    /// it Works on. Cuts are made only from the selection strip: select a
    /// shape with what it should work on (decision 1).
    fn cuts_panel(&mut self, ui: &mut egui::Ui, id: ElementId) {
        let model = self.doc.model();
        let Some(e) = model.get(id) else { return };
        if !solidops::takes_booleans(&e.kind) {
            return;
        }
        let is_shape = matches!(e.kind, ElementKind::FreeMesh(_));
        let name_of = |x: ElementId| model.get(x).map_or(String::new(), |e| e.name.clone());
        // (the cut, what it does, the other element, its name)
        let mut on_it = vec![];
        let mut by_it = vec![];
        for c in model.iter() {
            if let ElementKind::Cut(k) = &c.kind {
                if k.target == id {
                    on_it.push((c.id, *k, k.cutter, name_of(k.cutter)));
                }
                if k.cutter == id {
                    by_it.push((c.id, *k, k.target, name_of(k.target)));
                }
            }
        }
        let mut act: Option<CutAct> = None;
        if !is_shape || !on_it.is_empty() {
            let open = self.prop_header(ui, "cuts", "Booleans", true, Some("masses"));
            crate::props::fold(ui, open, |ui| {
                if on_it.is_empty() {
                    crate::props::plain_note(
                        ui,
                        "Nothing adds to or cuts it. To cut it, draw a shape, then select the shape and this together: the strip offers Add, Subtract and Intersect.",
                    );
                }
                for c in &on_it {
                    cut_box(ui, c, "By", on_it.len() == 1, &mut act);
                }
            });
        }
        if is_shape {
            let open = self.prop_header(ui, "cuts.by", "Works on", true, Some("masses"));
            crate::props::fold(ui, open, |ui| {
                if by_it.is_empty() {
                    crate::props::plain_note(
                        ui,
                        "It works on nothing yet. Select it with walls, slabs or other elements: the strip offers Add, Subtract and Intersect.",
                    );
                }
                for c in &by_it {
                    cut_box(ui, c, "On", by_it.len() == 1, &mut act);
                }
            });
        }
        match act {
            Some(CutAct::Op(c, op)) => {
                let r = self.doc.edit_merged(c, "Boolean kind", |e| {
                    if let ElementKind::Cut(k) = &mut e.kind {
                        k.op = op;
                    }
                });
                crate::actlog::line(format!(
                    "boolean: kind set to {} (Booleans panel)",
                    op_name(op)
                ));
                self.report(r, format!("Boolean: {}.", op_name(op)));
            }
            Some(CutAct::Reach(c, reach)) => {
                let r = self.doc.edit_merged(c, "Reach", |e| {
                    if let ElementKind::Cut(k) = &mut e.kind {
                        k.reach = reach;
                    }
                });
                crate::actlog::line(format!(
                    "boolean: reach set to {} (Booleans panel)",
                    reach.label()
                ));
                self.report(r, format!("Reach: {}.", reach.label()));
            }
            Some(CutAct::Remove(c)) => {
                crate::actlog::line("boolean: removed (Booleans panel)");
                let r = self
                    .doc
                    .commit(Transaction::new("Remove boolean").remove_cascade(self.doc.model(), c));
                self.report(
                    r,
                    "Boolean removed: the shape lets go of it. Ctrl+Z brings it back.",
                );
            }
            Some(CutAct::Select(other)) => {
                self.selection.only(other);
            }
            None => {}
        }
    }

    /// While a placing or drawing tool is on: what the next element will be,
    /// the tool bar's settings as Properties rows, edited in both places
    /// (MU18). False when the tool has none.
    fn dark_paper(&self, ctx: &egui::Context) -> bool {
        match self.shell.settings.drawing_paper.as_str() {
            "dark" => true,
            "light" => false,
            _ => crate::theme::Palette::of(ctx).dark,
        }
    }

    fn next_element_panel(&mut self, ui: &mut egui::Ui) -> bool {
        // the Grid tool's tab is its grid system (REV01, decision 1)
        if self.plan.tool == Tool::Grid {
            self.grid_tool_panel(ui);
            return true;
        }
        let ctx = ui.ctx().clone();
        let (_, options) = self.tool_strip(&ctx);
        let tool_is_stair = self.plan.tool == Tool::Stair;
        let rows: Vec<&serde_json::Value> = options
            .iter()
            .filter(|o| {
                o["kind"] == "number"
                    || o["kind"] == "toggle"
                    || o["kind"] == "check"
                    // the Stair tool's four questions (REV00 A): its
                    // picture drop-downs and its Steps / Ramp segment
                    || (tool_is_stair
                        && (o["kind"] == "seg"
                            || (o["kind"] == "menu"
                                && o["key"].as_str().is_some_and(|k| k.starts_with("stair.")))))
            })
            .filter(|o| o["key"] != "view.number")
            .collect();
        if rows.is_empty() {
            return false;
        }
        let tool = self.plan.tool;
        ui.label(
            egui::RichText::new(format!("Next {}", tool.label().to_lowercase()))
                .strong()
                .size(15.0),
        );
        if let Some(t) = typesui_type_name(self, tool) {
            crate::props::readout(ui, "Type", t, "The type it will be made of");
        }
        crate::props::info(
            ui,
            "Set it before you click; the ghost shows it. The same settings are in the tool bar.",
            "",
        );
        // an object's own make: its size and how it is fixed (MU19)
        if tool == Tool::Object {
            if let Some(c) = &self.plan.objects.current {
                let (lo, hi) = c.object.bounds();
                let s = hi - lo;
                ui.label(
                    egui::RichText::new(format!(
                        "{}: {} × {} × {}, {}",
                        c.name,
                        crate::units::len(s.x),
                        crate::units::len(s.y),
                        crate::units::len(s.z),
                        match c.object.mount {
                            creabim_core::objects::Mount::Wall => "on a wall",
                            creabim_core::objects::Mount::Floor => "on the floor",
                            creabim_core::objects::Mount::Ceiling => "hung from the ceiling",
                        }
                    ))
                    .weak(),
                );
            }
        }
        ui.add_space(4.0);
        let mut set: Vec<(String, f64)> = vec![];
        let mut act: Vec<String> = vec![];
        for o in rows {
            let label = o["label"].as_str().unwrap_or("").to_string();
            let hint = o["hint"].as_str().unwrap_or("");
            if o["kind"] == "number" {
                let key = o["key"].as_str().unwrap_or("").to_string();
                let mut v = o["value"].as_f64().unwrap_or(0.0);
                let (lo, hi) = (
                    o["min"].as_f64().unwrap_or(f64::MIN),
                    o["max"].as_f64().unwrap_or(f64::MAX),
                );
                let unit = o["unit"].as_str().unwrap_or("");
                let changed = crate::props::row(ui, &label, |ui| {
                    let r = if unit == "m" {
                        crate::props::fill(ui, crate::units::drag(&mut v).range(lo..=hi))
                    } else {
                        crate::props::fill(
                            ui,
                            egui::DragValue::new(&mut v)
                                .range(lo..=hi)
                                .suffix(format!(" {unit}")),
                        )
                    };
                    r.on_hover_text(hint).changed()
                });
                if changed {
                    set.push((key, v));
                }
            } else if o["kind"] == "menu" {
                // a picture drop-down: its menu's answers as the choices
                let key = o["key"].as_str().unwrap_or("");
                let name = match key {
                    "stair.line" => "Line",
                    "stair.turns" => "Turns",
                    "stair.rails" => "Railings",
                    _ => key,
                };
                let enabled = o["enabled"].as_bool().unwrap_or(true);
                let value = o["value"].as_str().unwrap_or("").to_string();
                let items = self.menu_items(key);
                crate::props::row(ui, name, |ui| {
                    ui.add_enabled_ui(enabled, |ui| {
                        egui::ComboBox::from_id_salt(("next", key))
                            .icon(crate::props::combo_icon)
                            .width(ui.available_width())
                            .selected_text(value)
                            .show_ui(ui, |ui| {
                                for it in &items {
                                    if let crate::ui_model::Item::Action {
                                        id,
                                        label,
                                        checked,
                                        ..
                                    } = it
                                    {
                                        if ui
                                            .selectable_label(
                                                checked.unwrap_or(false),
                                                label.as_str(),
                                            )
                                            .clicked()
                                        {
                                            act.push(id.clone());
                                        }
                                    }
                                }
                            });
                    })
                    .response
                    .on_hover_text(hint);
                });
            } else if o["kind"] == "seg" {
                // Steps or Ramp: two answers, a segment
                let items = o["items"].as_array().cloned().unwrap_or_default();
                let mut now = items
                    .iter()
                    .position(|i| i["on"].as_bool().unwrap_or(false))
                    .unwrap_or(0);
                let names: Vec<(usize, String, String)> = items
                    .iter()
                    .enumerate()
                    .map(|(i, it)| {
                        let hint = it["hint"].as_str().unwrap_or("").to_string();
                        (i, it["label"].as_str().unwrap_or("").to_string(), hint)
                    })
                    .collect();
                let opts: Vec<(usize, &str, &str)> = names
                    .iter()
                    .map(|(i, w, h)| (*i, w.as_str(), h.as_str()))
                    .collect();
                if crate::props::choice(ui, "Climbs by", &mut now, &opts) {
                    if let Some(id) = items.get(now).and_then(|i| i["id"].as_str()) {
                        act.push(id.to_string());
                    }
                }
            } else if o["action"].as_bool().unwrap_or(false) {
                // a step, not a setting: a button
                if ui.button(label).on_hover_text(hint).clicked() {
                    act.push(o["id"].as_str().unwrap_or("").to_string());
                }
            } else {
                let mut on = o["on"].as_bool().unwrap_or(false);
                if crate::props::tickbox(ui, &mut on, label)
                    .on_hover_text(hint)
                    .changed()
                {
                    act.push(o["id"].as_str().unwrap_or("").to_string());
                }
            }
        }
        for (k, v) in set {
            self.set_tool_number(&k, v);
        }
        for id in act {
            self.run_action(&ctx, &id);
        }
        true
    }

    /// Trace and reference for view `v` (MU18): what shows under it, in what
    /// colour and strength, moved or turned to compare.
    /// Take up `tool`, as its button does.
    fn pick_tool(&mut self, ctx: &egui::Context, tool: Tool) {
        if let Some(i) = Tool::ALL.iter().position(|t| *t == tool) {
            self.run_action(ctx, &format!("tool:{i}"));
        }
    }

    /// The sheet, or the view of the same kind, `by` places on in the
    /// project's order (PgUp / PgDn, MU24).
    fn step_view(&mut self, by: i32) {
        let Some(now) = self.active_view else { return };
        let model = self.doc.model();
        let kind = |id: ElementId| match model.get(id).map(|e| &e.kind) {
            Some(ElementKind::Sheet(_)) => Some(0u8),
            Some(ElementKind::View(v)) => Some(match v.kind {
                ViewKind::Plan { .. } => 1,
                ViewKind::Section { .. } => 2,
                ViewKind::Axonometric { .. } => 3,
                ViewKind::Schedule { .. } => 4,
                ViewKind::LongSection { .. }
                | ViewKind::BoreholeLog { .. }
                | ViewKind::WalkSection { .. } => 2,
            }),
            _ => None,
        };
        let Some(k) = kind(now) else { return };
        let list: Vec<(ElementId, String)> = if k == 0 {
            sheets::sheets(model)
                .into_iter()
                .map(|(id, s)| {
                    (
                        id,
                        format!(
                            "{} {}",
                            s.number,
                            model.get(id).map_or("", |e| e.name.as_str())
                        ),
                    )
                })
                .collect()
        } else {
            model
                .iter()
                .filter(|e| kind(e.id) == Some(k))
                .map(|e| (e.id, e.name.clone()))
                .collect()
        };
        let Some(i) = list.iter().position(|(id, _)| *id == now) else {
            return;
        };
        let j = (i as i32 + by).rem_euclid(list.len() as i32) as usize;
        let (id, name) = list[j].clone();
        self.active_view = Some(id);
        self.status = format!(
            "{name} ({}/{}). PgUp / PgDn step through them.",
            j + 1,
            list.len()
        );
    }

    /// Shift+A (MU24, as Blender's Add menu): every tool, found by typing,
    /// the ones that can't be used in the view in front greyed.
    fn add_menu_window(&mut self, ctx: &egui::Context) {
        let Some(mut q) = self.add_menu.clone() else {
            return;
        };
        let mut pick = None;
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let at = ctx
            .input(|i| i.pointer.hover_pos())
            .unwrap_or(ctx.content_rect().center());
        let shown = egui::Area::new(egui::Id::new("add.menu"))
            .order(crate::app::overlay_order())
            .fixed_pos(at)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(240.0);
                    ui.label(egui::RichText::new("Add").strong());
                    let r = {
                        let w = ui.spacing().text_edit_width;
                        crate::props::text_field(
                            ui,
                            egui::TextEdit::singleline(&mut q).hint_text("Type to find a tool"),
                            w,
                        )
                    };
                    r.request_focus();
                    let want = q.to_lowercase();
                    let found: Vec<Tool> = Tool::ALL
                        .iter()
                        .copied()
                        .filter(|t| *t != Tool::Select && t.label().to_lowercase().contains(&want))
                        .collect();
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .show(ui, |ui| {
                            for t in &found {
                                if ui
                                    .add(egui::Button::new(t.label()).frame(false))
                                    .on_hover_text(t.hint())
                                    .clicked()
                                {
                                    pick = Some(*t);
                                }
                            }
                        });
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        pick = found.first().copied();
                    }
                    if found.is_empty() {
                        ui.label(egui::RichText::new("Nothing by that name").weak());
                    }
                });
            });
        let outside = ctx
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|p| !shown.response.rect.contains(p));
        if ctx.input(|i| i.pointer.any_click()) && outside {
            close = true;
        }
        if let Some(t) = pick {
            self.add_menu = None;
            self.pick_tool(ctx, t);
            return;
        }
        self.add_menu = if close { None } else { Some(q) };
    }

    /// A north arrow or a scale bar on `sheet`, where it usually goes:
    /// over the title block's right end, or its left (MU22).
    /// A revision cloud (the next letter after the project's last), or the
    /// sheet's revisions table, on `sheet` (Annotate board step 4).
    fn add_revision_mark(&mut self, sheet: ElementId, table: bool) {
        let model = self.doc.model();
        let Some(s) = sheets::sheet_of(model, sheet) else {
            return;
        };
        let (w, h) = (s.width, s.height);
        let mark = if table {
            let m = creabim_geom::sheet::MARGIN;
            let top =
                creabim_geom::sheet::title_block_box(model, sheet).map_or(m + 45.0, |(_, hi)| hi.y);
            creabim_core::PaperMark::revision_table(
                sheet,
                glam::DVec2::new(w - m - 112.0, top + 34.0),
            )
        } else {
            let last = model
                .iter()
                .filter_map(|e| match &e.kind {
                    ElementKind::PaperMark(pm) => match &pm.kind {
                        creabim_core::PaperMarkKind::Cloud { rev, .. } => rev.chars().next(),
                        _ => None,
                    },
                    _ => None,
                })
                .max();
            let next = last
                .map_or('A', |c| ((c as u8).saturating_add(1)) as char)
                .to_string();
            creabim_core::PaperMark::cloud(sheet, glam::DVec2::new(w / 2.0, h / 2.0), &next)
        };
        let what = if table {
            "Revisions table"
        } else {
            "Revision cloud"
        };
        let e = Element::new(what, ElementKind::PaperMark(mark));
        let id = e.id;
        if self
            .doc
            .commit(
                creabim_core::Transaction::new(format!("Add {}", what.to_lowercase())).insert(e),
            )
            .is_ok()
        {
            self.selection.only(id);
            self.prop_tab = PropTab::Element;
            self.status = if table {
                "Revisions table added: a row for each revision cloud on this sheet. Drag it where it goes.".into()
            } else {
                "Revision cloud added: drag it over the change, size it, and say what changed in Properties.".into()
            };
        }
    }

    fn add_paper_mark(&mut self, sheet: ElementId, north: bool) {
        let model = self.doc.model();
        let Some(s) = sheets::sheet_of(model, sheet) else {
            return;
        };
        let (w, m) = (s.width, creabim_geom::sheet::MARGIN);
        // clear of the title block as drawn, whatever its template
        let tb = creabim_geom::sheet::TITLE_BLOCK;
        let (tb_left, tb_top) = creabim_geom::sheet::title_block_box(model, sheet)
            .map_or((w - m - tb.x, m + tb.y), |(lo, hi)| (lo.x, hi.y));
        let mark = if north {
            creabim_core::PaperMark::north_arrow(
                sheet,
                glam::DVec2::new(w - m - 16.0, tb_top + 18.0),
            )
        } else {
            // the scale of the views on it, else 1:100
            let scale = model
                .iter()
                .find_map(|e| match &e.kind {
                    ElementKind::Viewport(vp) if vp.sheet == sheet => {
                        match model.get(vp.view).map(|v| &v.kind) {
                            Some(ElementKind::View(v))
                                // a real scale: not a schedule or a 3D view
                                if !matches!(v.kind, ViewKind::Schedule { .. } | ViewKind::Axonometric { .. }) =>
                            {
                                Some(v.scale)
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                })
                // nothing to measure: 1/8" = 1'-0" in an imperial project
                .unwrap_or(if model.units().is_imperial() { 96.0 } else { 100.0 });
            let mut bar = creabim_core::PaperMark::scale_bar(
                sheet,
                glam::DVec2::new(tb_left + 4.0, tb_top + 10.0),
                scale,
            );
            // it measures the drawing nearest it (owner, after MU91)
            bar.measures = creabim_geom::sheet::nearest_viewport(model, sheet, bar.at, |v| {
                creabim_geom::sheet::drawing_scale(model, v).is_some()
            });
            let bar = bar.in_feet(model.units().is_imperial());
            match bar
                .measures
                .and_then(|v| creabim_geom::sheet::drawing_scale(model, v))
            {
                Some(s) => bar.at_scale(s),
                None => bar,
            }
        };
        let what = if north { "North arrow" } else { "Scale bar" };
        let e = Element::new(what, ElementKind::PaperMark(mark));
        let id = e.id;
        if self
            .doc
            .commit(
                creabim_core::Transaction::new(format!("Add {}", what.to_lowercase())).insert(e),
            )
            .is_ok()
        {
            self.selection.only(id);
            self.prop_tab = PropTab::Element;
            self.status = format!(
                "{what} added. Drag it where it goes; Properties sets its size{}.",
                if north {
                    " and angle"
                } else {
                    ", scale and length"
                }
            );
        }
    }

    /// The sheets the Export sheets dialog would write, in sheet order.
    fn sheets_to_export(&self, this_only: bool) -> Vec<ElementId> {
        let all: Vec<ElementId> = sheets::sheets(self.doc.model())
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        if this_only {
            return self
                .active_view
                .filter(|id| all.contains(id))
                .into_iter()
                .collect();
        }
        match self.sheets_export.scope {
            ExportScope::All => all,
            ExportScope::Chosen => all
                .into_iter()
                .filter(|id| self.sheets_export.chosen.contains(id))
                .collect(),
            ExportScope::This => self
                .active_view
                .filter(|id| all.contains(id))
                .into_iter()
                .collect(),
        }
    }

    /// Write sheets as PDF: this one (`this_only`), else as the dialog says.
    pub(super) fn export_sheets_now(&mut self, this_only: bool) {
        // with its card, written off the window's thread
        if self.mark_cards() {
            self.open_export_card("export.pdf");
            if let Some(card) = &mut self.mark {
                card.scope = if this_only {
                    ExportScope::This
                } else {
                    self.sheets_export.scope
                };
                if this_only {
                    card.each = false;
                    self.mark_do_start();
                }
            }
            return;
        }
        let ids = self.sheets_to_export(this_only);
        if ids.is_empty() {
            self.status = if this_only {
                "Open a sheet first: Ctrl+P makes a PDF of the sheet in front.".into()
            } else {
                "No sheets to export: pick some, or choose All.".into()
            };
            return;
        }
        let info = SheetInfo {
            print_white: self.sheets_export.white,
            ..self.sheet_info()
        };
        let model = self.doc.model();
        let name_of = |id: ElementId| {
            let n = sheets::sheet_of(model, id)
                .map(|s| s.number.clone())
                .unwrap_or_default();
            let t = model.get(id).map(|e| e.name.clone()).unwrap_or_default();
            let raw = format!("{n} {t}");
            raw.trim()
                .chars()
                .map(|c| if "\\/:*?\"<>|".contains(c) { '-' } else { c })
                .collect::<String>()
        };
        if this_only || !self.sheets_export.each {
            let default = if ids.len() == 1 {
                let p = std::path::Path::new(&self.path);
                p.parent()
                    .unwrap_or(std::path::Path::new(""))
                    .join(format!("{}.pdf", name_of(ids[0])))
                    .to_string_lossy()
                    .into_owned()
            } else {
                with_extension(&self.path, "pdf")
            };
            let pdf = sheets::sheets_pdf_of(model, &info, &ids);
            let Some(out) = self.export_to(default) else {
                return;
            };
            let r = std::fs::write(&out, pdf);
            let n = ids.len();
            self.report(
                r,
                format!(
                    "Exported {n} sheet{} to {out}",
                    if n == 1 { "" } else { "s" }
                ),
            );
            return;
        }
        // a PDF each, named by number and name, in a folder
        let dir = if cfg!(test) {
            Some(std::env::temp_dir())
        } else {
            let start = std::path::absolute(&self.path)
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()));
            let mut d = rfd::FileDialog::new().set_title("Export sheets: a PDF each, into…");
            if let Some(s) = start {
                d = d.set_directory(s);
            }
            d.pick_folder()
        };
        let Some(dir) = dir else { return };
        let mut written = 0;
        for id in &ids {
            let pdf = sheets::sheets_pdf_of(model, &info, &[*id]);
            if std::fs::write(dir.join(format!("{}.pdf", name_of(*id))), pdf).is_ok() {
                written += 1;
            }
        }
        self.last_export_dir = Some(dir.clone());
        self.status = format!(
            "Exported {written} sheet PDF{} to {}",
            if written == 1 { "" } else { "s" },
            dir.display()
        );
    }

    /// The Export sheets dialog (MU22): all, chosen or this sheet, as one
    /// PDF or a PDF each.
    fn sheets_export_window(&mut self, ctx: &egui::Context) {
        if !self.sheets_export_open {
            return;
        }
        let model = self.doc.model();
        let all: Vec<(ElementId, String)> = sheets::sheets(model)
            .into_iter()
            .map(|(id, s)| {
                let t = model.get(id).map(|e| e.name.clone()).unwrap_or_default();
                (id, format!("{} {t}", s.number))
            })
            .collect();
        // chosen starts as the sheets selected in the browser
        if self.sheets_export.chosen.is_empty() {
            self.sheets_export.chosen = self
                .selection
                .all()
                .iter()
                .copied()
                .filter(|id| all.iter().any(|(s, _)| s == id))
                .collect();
        }
        let this = self
            .active_view
            .filter(|id| all.iter().any(|(s, _)| s == id));
        let mut open = true;
        let mut go = false;
        let mut cancel = false;
        let x = &mut self.sheets_export;
        crate::app::shell::window("Export sheets")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(380.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                crate::props::row(ui, "Sheets", |ui| {
                    crate::props::flow(ui, |ui| {
                        ui.selectable_value(
                            &mut x.scope,
                            ExportScope::All,
                            format!("All ({})", all.len()),
                        );
                        ui.selectable_value(
                            &mut x.scope,
                            ExportScope::Chosen,
                            format!("Chosen ({})", x.chosen.len()),
                        );
                        ui.add_enabled_ui(this.is_some(), |ui| {
                            ui.selectable_value(&mut x.scope, ExportScope::This, "This one")
                                .on_disabled_hover_text("Open a sheet first");
                        });
                    })
                });
                if x.scope == ExportScope::Chosen {
                    egui::ScrollArea::vertical()
                        .max_height(180.0)
                        .show(ui, |ui| {
                            for (id, name) in &all {
                                let mut on = x.chosen.contains(id);
                                if crate::props::check(ui, "", &mut on, name.as_str()).changed() {
                                    if on {
                                        x.chosen.insert(*id);
                                    } else {
                                        x.chosen.remove(id);
                                    }
                                }
                            }
                        });
                }
                crate::props::row(ui, "As", |ui| {
                    crate::props::flow(ui, |ui| {
                        ui.selectable_value(&mut x.each, false, "One PDF");
                        ui.selectable_value(&mut x.each, true, "A PDF each");
                    })
                });
                crate::props::row(ui, "Paper", |ui| {
                    crate::props::flow(ui, |ui| {
                        ui.selectable_value(&mut x.white, false, "As the master")
                            .on_hover_text("Each sheet on its master's paper: its colour and texture");
                        ui.selectable_value(&mut x.white, true, "Print on white")
                            .on_hover_text("For white stock: the paper's tint and texture are left off, the drawing keeps its colours");
                    })
                });
                ui.add_space(6.0);
                crate::props::flow(ui, |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        go = ui.button("Export…").clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                });
            });
        if go {
            self.sheets_export_open = false;
            self.export_sheets_now(false);
        } else if cancel || !open {
            self.sheets_export_open = false;
        }
    }

    /// Hide every reference shown in view `v`, or show them again.
    fn toggle_references(&mut self, v: ElementId) {
        let model = self.doc.model();
        let refs: Vec<Element> = creabim_core::reference::in_view(model, v)
            .iter()
            .filter_map(|(id, _)| model.get(*id).cloned())
            .collect();
        if refs.is_empty() {
            self.status = "No reference in this view: add one in Properties > Trace.".into();
            return;
        }
        let hide = refs
            .iter()
            .any(|e| matches!(&e.kind, ElementKind::Reference(r) if !r.hidden));
        let mut tx = creabim_core::Transaction::new(if hide {
            "Hide references"
        } else {
            "Show references"
        });
        for e in refs {
            let mut after = e.clone();
            if let ElementKind::Reference(r) = &mut after.kind {
                r.hidden = hide;
            }
            tx = tx.replace(e, after);
        }
        if self.doc.commit(tx).is_ok() {
            self.status = if hide {
                "References hidden (Alt+F2 shows them).".into()
            } else {
                "References shown (Alt+F2 hides them).".into()
            };
        }
    }

    /// A new reference shown in view `v`, in place, selected.
    /// Add map data to view `v`: an aerial photo as a picture where it is
    /// on the map, anything else as its lines. A project not yet on the
    /// map is placed by it first.
    pub(super) fn add_map_reference(&mut self, v: ElementId, path: &str) {
        use creabim_core::RefSource;
        let p = std::path::Path::new(path);
        let photo = creabim_exchange::gis::is_picture(p);
        // where it is on the map, and the system it names
        let read = if photo {
            std::fs::read(p)
                .map_err(|e| e.to_string())
                .and_then(|b| creabim_exchange::gis::map_picture(std::io::Cursor::new(b)))
                .map(|m| ((m.lo + m.hi) / 2.0, m.crs, Some(m.hi - m.lo)))
        } else {
            creabim_exchange::gis::read(p).and_then(|d| {
                let mut pts = d
                    .survey
                    .lines
                    .iter()
                    .flat_map(|l| l.points.iter())
                    .map(|q| q.truncate());
                let first = pts.next().ok_or("this file holds no lines to show")?;
                let (lo, hi) = pts.fold((first, first), |(lo, hi), q| (lo.min(q), hi.max(q)));
                Ok(((lo + hi) / 2.0, d.crs, None))
            })
        };
        let (mid, crs, size) = match read {
            Ok(r) => r,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        let model = self.doc.model();
        let placed = model.project_info().and_then(|(_, i)| i.map.clone());
        let mut msg = String::new();
        if placed.is_none() && mid.length() > 2000.0 {
            let mut info = model
                .project_info()
                .map(|(_, i)| i.clone())
                .unwrap_or_default();
            info.map = Some(creabim_core::MapPlace {
                crs: crs.clone().unwrap_or_default(),
                easting: mid.x.round(),
                northing: mid.y.round(),
            });
            let _ = self.doc.commit(creabim_core::set_project(model, info));
            msg = " The project is now placed on the map here (the Project tab).".into();
        } else if let (Some(ours), Some(theirs)) = (&placed, &crs) {
            if !ours.crs.trim().is_empty() && !ours.crs.trim().eq_ignore_ascii_case(theirs) {
                msg = format!(" It is in {theirs}, the project in {}: nothing is converted, so it may lie in the wrong place.", ours.crs.trim());
            }
        }
        let offset = self
            .doc
            .model()
            .project_info()
            .and_then(|(_, i)| i.map.as_ref())
            .map_or(DVec2::ZERO, |m| DVec2::new(m.easting, m.northing));
        match size {
            Some(size) => {
                self.add_reference(
                    v,
                    RefSource::Image {
                        path: path.to_string(),
                        width: size.x,
                    },
                );
                if let Some(id) = self.selection.active {
                    let _ = self.doc.edit_merged(id, "Place reference", |e| {
                        if let ElementKind::Reference(r) = &mut e.kind {
                            if matches!(&r.source, RefSource::Image { path: q, .. } if q == path) {
                                r.at = mid - offset;
                                r.opacity = 0.85;
                                // a flat photo at the level would hide the
                                // ground and the building in 3D
                                r.in_3d = false;
                            }
                        }
                    });
                }
            }
            None => {
                self.add_reference(
                    v,
                    RefSource::Map {
                        path: path.to_string(),
                    },
                );
                // map linework reads as the site's context: dark and firm
                if let Some(id) = self.selection.active {
                    let _ = self.doc.edit_merged(id, "Place reference", |e| {
                        if let ElementKind::Reference(r) = &mut e.kind {
                            if matches!(&r.source, RefSource::Map { .. }) {
                                r.colour = None;
                                r.opacity = 0.85;
                            }
                        }
                    });
                }
            }
        }
        if !msg.is_empty() {
            self.status.push_str(&msg);
        }
    }

    fn add_reference(&mut self, v: ElementId, source: creabim_core::RefSource) {
        let model = self.doc.model();
        let shown = crate::trace::read(model, &source);
        if let crate::trace::Shown::Missing(why) = &shown {
            self.status = why.clone();
            return;
        }
        let r_source_dxf = matches!(source, creabim_core::RefSource::Dxf { .. });
        let mut r = creabim_core::Reference::new(source, vec![v]);
        if let Some(b) = crate::trace::bounds_of(&r.source, &shown) {
            r.in_place(b);
        }
        // a picture keeps its own colours; lines take the first colour
        if matches!(shown, crate::trace::Shown::Picture(_)) {
            r.colour = None;
            r.opacity = 0.6;
        }
        // named for what it shows, without a prefix eating the field (MU24)
        let name = crate::trace::label(model, &r.source);
        let e = Element::new(name.clone(), ElementKind::Reference(r));
        let id = e.id;
        if self
            .doc
            .commit(creabim_core::Transaction::new(format!("Add {name}")).insert(e))
            .is_ok()
        {
            self.selection.only(id);
            self.prop_tab = PropTab::Element;
            self.status = "Reference added. Drag its middle to move it, a corner to scale it, the round grip to turn it; Calibrate sets an exact size. Alt+F2 hides it.".into();
            if r_source_dxf && crate::trace::DXF_CAPPED.load(std::sync::atomic::Ordering::Relaxed) {
                self.status = format!(
                    "Reference added, but only in part: the file makes more than {} items (blocks in arrays inside arrays), so reading stopped there.",
                    creabim_exchange::titleblock::MAX_ITEMS
                );
            }
        }
    }

    fn trace_panel(&mut self, ui: &mut egui::Ui, v: ElementId) {
        use creabim_core::RefSource;
        ui.add_space(6.0);
        // the tab's own title says what it is; this section lists them,
        // and its ? says what a reference is (MU27)
        let open = self.prop_header(
            ui,
            "trace.list",
            "References here",
            true,
            Some("trace-reference"),
        );
        crate::props::fold(ui, open, |ui| {
            let model = self.doc.model();
            let here: Vec<(ElementId, String, bool)> = creabim_core::reference::in_view(model, v)
                .into_iter()
                .map(|(id, r)| (id, crate::trace::label(model, &r.source), r.hidden))
                .collect();
            let others: Vec<(ElementId, String)> = model
                .iter()
                .filter(|e| e.id != v)
                .filter_map(|e| match &e.kind {
                    ElementKind::View(view)
                        if matches!(
                            view.kind,
                            ViewKind::Plan { .. } | ViewKind::Section { .. }
                        ) =>
                    {
                        Some((e.id, e.name.clone()))
                    }
                    _ => None,
                })
                .collect();
            if here.is_empty() {
                // empty, it says what a reference is for (MU60: the Add
                // row was all there was, squeezed)
                crate::props::empty(ui, "Nothing traced yet.");
                crate::props::plain_note(
                    ui,
                    "Lay another plan, a scanned drawing, a DXF or a map faintly under this view to draw over it.",
                );
                ui.add_space(4.0);
            }
            let mut add = None;
            let mut map = None;
            // two rows of two equal buttons (MU60: four in one row were
            // squeezed, their words wrapped and the View list overflowed)
            let gap = ui.spacing().item_spacing.x;
            let w = ((ui.available_width() - gap) / 2.0).floor().max(60.0);
            ui.horizontal(|ui| {
            let r = ui
                .add_sized([w, 24.0], egui::Button::new("+ View"))
                .on_hover_text("Another plan or section of this project, under this one");
            egui::Popup::menu(&r).show(|ui| {
                if others.is_empty() {
                    ui.label(egui::RichText::new("No other plan or section yet").weak());
                }
                for (id, name) in &others {
                    if ui.selectable_label(false, name).clicked() {
                        add = Some(RefSource::View(*id));
                    }
                }
            });
            if ui
                .add_sized([w, 24.0], egui::Button::new("+ Picture"))
                .on_hover_text("A picture to trace over: a scanned plan, a photo of a sketch (PNG or JPEG). Calibrate it to size in its properties.")
                .clicked()
            {
                if let Some(p) = rfd::FileDialog::new()
                    .set_title("Add a picture as a reference")
                    .add_filter("Pictures", &["png", "jpg", "jpeg", "tif", "tiff"])
                    .pick_file()
                {
                    add = Some(RefSource::Image {
                        path: p.display().to_string(),
                        width: 10.0,
                    });
                }
            }
            });
            ui.horizontal(|ui| {
            if ui
                .add_sized([w, 24.0], egui::Button::new("+ DXF"))
                .on_hover_text("A CAD drawing's lines (DXF). A DWG opens here once saved as DXF: any CAD program, or the free ODA File Converter, does it offline.")
                .clicked()
            {
                if let Some(p) = rfd::FileDialog::new()
                    .set_title("Add a DXF drawing as a reference")
                    .add_filter("DXF", &["dxf"])
                    .pick_file()
                {
                    add = Some(RefSource::Dxf {
                        path: p.display().to_string(),
                    });
                }
            }
            if ui
                .add_sized([w, 24.0], egui::Button::new("+ Map"))
                .on_hover_text("Map data where it is on the map: boundaries, roads and buildings (GeoJSON, a shapefile, an OpenStreetMap export), or an aerial photo (GeoTIFF). Placed by the project's map position (the Project tab); the first one places the project if nothing has yet")
                .clicked()
            {
                if let Some(p) = rfd::FileDialog::new()
                    .set_title("Add map data as a reference")
                    .add_filter("Map data and aerial photos", &[&MAP_LINE_FILES[..], &["tif", "tiff"]].concat())
                    .pick_file()
                {
                    map = Some(p.display().to_string());
                }
            }
        });
            if let Some(src) = add {
                self.add_reference(v, src);
            }
            if let Some(p) = map {
                self.add_map_reference(v, &p);
            }
            ui.add_space(4.0);
            let mut select = None;
            let mut flip = None;
            // a long list scrolls in its own box; Add stays in reach (MU28)
            egui::ScrollArea::vertical()
                .id_salt(("trace.list", v))
                .max_height(240.0)
                .show(ui, |ui| {
                    for (id, name, hidden) in &here {
                        crate::props::flow(ui, |ui| {
                            let eye = if *hidden { "eye-off" } else { "eye" };
                            let (rr, resp) = ui
                                .allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                            crate::svgicon::paint(
                                ui.painter(),
                                rr.shrink(2.0),
                                eye,
                                ui.visuals().text_color(),
                            );
                            if resp
                                .on_hover_text(if *hidden { "Show it" } else { "Hide it" })
                                .clicked()
                            {
                                flip = Some(*id);
                            }
                            // one line, cut with … and whole on hover (MU23)
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                            let on = self.selection.contains(*id);
                            if ui
                                .add_sized(
                                    [ui.available_width(), 22.0],
                                    egui::Button::selectable(on, name.as_str()).truncate(),
                                )
                                .on_hover_text(name.as_str())
                                .clicked()
                            {
                                select = Some(*id);
                            }
                        });
                    }
                });
            if let Some(id) = select {
                self.selection.only(id);
                self.prop_tab = PropTab::Element;
            }
            if let Some(id) = flip {
                if let Some(e) = self.doc.model().get(id).cloned() {
                    let mut after = e.clone();
                    if let ElementKind::Reference(r) = &mut after.kind {
                        r.hidden = !r.hidden;
                    }
                    let _ = self.doc.commit(
                        creabim_core::Transaction::new("Show or hide a reference")
                            .replace(e, after),
                    );
                }
            }
        });
    }

    /// A reference's own properties (MU19): what it shows and where, how
    /// strongly, and its size. Returns the undo label of a change.
    fn reference_fields(
        &mut self,
        ui: &mut egui::Ui,
        id: ElementId,
        r: &mut creabim_core::Reference,
    ) -> Option<&'static str> {
        use creabim_core::reference::REFERENCE_COLOURS;
        use creabim_core::RefSource;
        let mut label = None;
        let model = self.doc.model();
        let what = match &r.source {
            RefSource::View(_) => "View",
            RefSource::Image { .. } => "Picture",
            RefSource::Dxf { .. } => "DXF drawing",
            RefSource::Map { .. } => "Map",
        };
        let name = crate::trace::label(model, &r.source);
        let path = match &r.source {
            RefSource::Image { path, .. } | RefSource::Dxf { path } | RefSource::Map { path } => {
                Some(path.clone())
            }
            RefSource::View(_) => None,
        };
        // as Blender's image rows (MU23): its name, cut with … and whole
        // on hover; copy it, pick another file, or take the reference away
        let mut replace = false;
        let mut remove = false;
        crate::props::row(ui, what, |ui| {
            // a fixed row in whole pixels: wrapping, it flickered as the panel was resized (MU59)
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let icons = if path.is_some() { 3.0 } else { 2.0 };
                let w = (ui.available_width() - icons * 22.0).floor().max(30.0);
                ui.add_sized(
                    [w, 22.0],
                    egui::Label::new(egui::RichText::new(name.clone()).strong()).truncate(),
                );
                if crate::props::icon_button(ui, "copy", "Copy the name").clicked() {
                    ui.ctx().copy_text(name.clone());
                }
                if path.is_some()
                    && crate::props::icon_button(ui, "folder-open", "Another file in its place")
                        .clicked()
                {
                    replace = true;
                }
                if crate::props::icon_button(ui, "x", "Take this reference away").clicked() {
                    remove = true;
                }
            })
        });
        if let Some(p) = &path {
            crate::props::row(ui, "File", |ui| {
                // a fixed row in whole pixels: wrapping, it flickered as the panel was resized (MU59)
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let w = (ui.available_width() - 24.0).floor().max(30.0);
                    ui.add_sized(
                        [w, 22.0],
                        egui::Label::new(egui::RichText::new(p.clone()).weak()).truncate(),
                    );
                    if crate::props::icon_button(
                        ui,
                        "refresh-cw",
                        "Read the file again, after it changed",
                    )
                    .clicked()
                    {
                        self.plan.ref_cache.forget(id);
                    }
                })
            });
        }
        if replace {
            let filter: (&str, &[&str]) = match &r.source {
                RefSource::Dxf { .. } => ("DXF", &["dxf"]),
                RefSource::Map { .. } => ("Map data", &MAP_LINE_FILES),
                _ => ("Pictures", &["png", "jpg", "jpeg", "tif", "tiff"]),
            };
            if let Some(p) = rfd::FileDialog::new()
                .set_title("Another file for this reference")
                .add_filter(filter.0, filter.1)
                .pick_file()
            {
                let p = p.display().to_string();
                match &mut r.source {
                    RefSource::Image { path, .. }
                    | RefSource::Dxf { path }
                    | RefSource::Map { path } => *path = p,
                    RefSource::View(_) => {}
                }
                // it keeps its place, turn and scale; its size is read again
                let shown = crate::trace::read(self.doc.model(), &r.source);
                if let Some(b) = crate::trace::bounds_of(&r.source, &shown) {
                    let at = r.at;
                    r.bounds = Some(b);
                    r.at = at;
                }
                self.plan.ref_cache.forget(id);
                label = Some("Reference file");
            }
        }
        if remove {
            self.remove_ref = Some(id);
        }
        let model = self.doc.model();
        if let crate::trace::Shown::Missing(why) = self.plan.ref_cache.get(model, id, r) {
            ui.colored_label(ui.visuals().warn_fg_color, why);
        }
        // the views it shows in: plans and sections
        let views: Vec<(ElementId, String)> = model
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::View(view)
                    if matches!(view.kind, ViewKind::Plan { .. } | ViewKind::Section { .. }) =>
                {
                    Some((e.id, e.name.clone()))
                }
                _ => None,
            })
            .collect();
        // and the profiles, each by its trace view (MU38)
        let profs: Vec<(ElementId, String)> = creabim_core::profiles::project_profiles(model)
            .iter()
            .map(|p| (creabim_core::profiles::trace_view(&p.name), p.name.clone()))
            .collect();
        let on_profile = profs
            .iter()
            .find(|(v, _)| r.shown_in.contains(v))
            .map(|(v, n)| (*v, n.clone()));
        let mut views = views;
        views.extend(profs.iter().map(|(v, n)| (*v, format!("Profile {n}"))));
        if let Some((pv, pname)) = &on_profile {
            crate::props::row(ui, "Shown in", |ui| {
                if crate::props::tickbox(ui, &mut r.only_profile, "Only in this profile")
                    .on_hover_text(
                        "Kept to this profile: not in the plans, sections or 3D. \
                         Untick it to show it in other views too.",
                    )
                    .changed()
                {
                    if r.only_profile {
                        r.shown_in = vec![*pv];
                    }
                    label = Some(if r.only_profile {
                        "Reference only in its profile"
                    } else {
                        "Reference shared"
                    });
                }
            });
            if r.only_profile {
                crate::props::row(ui, "", |ui| {
                    ui.label(egui::RichText::new(format!("Profile: {pname}")).weak());
                });
            }
        }
        let n = r.shown_in.len();
        if !r.only_profile {
            // under the checkbox, the list needs no second label
            let title = if on_profile.is_some() { "" } else { "Shown in" };
            crate::props::row(ui, title, |ui| {
                egui::ComboBox::from_id_salt(("ref.views", id))
                    .icon(crate::props::combo_icon)
                    .selected_text(match n {
                        0 => "No view".to_string(),
                        1 => views
                            .iter()
                            .find(|(v, _)| r.shown_in.contains(v))
                            .map_or("1 view".into(), |(_, n)| n.clone()),
                        n => format!("{n} views"),
                    })
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        for (v, name) in &views {
                            let mut on = r.shown_in.contains(v);
                            if crate::props::tickbox(ui, &mut on, name).changed() {
                                if on {
                                    r.shown_in.push(*v);
                                } else {
                                    r.shown_in.retain(|x| x != v);
                                }
                                label = Some("Reference views");
                            }
                        }
                    })
            });
        }
        // narrow, the swatches wrap rather than run off the panel (MU34)
        crate::props::row(ui, "Colour", |ui| {
            crate::props::flow(ui, |ui| {
                let swatch = |ui: &mut egui::Ui, c: Option<Color32>, on: bool, tip: &str| {
                    let (rr, resp) =
                        ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                    match c {
                        Some(c) => {
                            ui.painter().circle_filled(rr.center(), 8.0, c);
                        }
                        None => {
                            // its own colours: a split swatch
                            ui.painter().circle_stroke(
                                rr.center(),
                                7.5,
                                egui::Stroke::new(1.2, ui.visuals().weak_text_color()),
                            );
                            ui.painter().line_segment(
                                [
                                    rr.center() + egui::vec2(-5.0, 5.0),
                                    rr.center() + egui::vec2(5.0, -5.0),
                                ],
                                egui::Stroke::new(1.2, ui.visuals().weak_text_color()),
                            );
                        }
                    }
                    if on {
                        ui.painter().circle_stroke(
                            rr.center(),
                            10.0,
                            egui::Stroke::new(1.5, ui.visuals().text_color()),
                        );
                    }
                    resp.on_hover_text(tip).clicked()
                };
                if swatch(ui, None, r.colour.is_none(), "Its own colours") {
                    r.colour = None;
                    label = Some("Reference colour");
                }
                for (i, (cname, [cr, cg, cb])) in REFERENCE_COLOURS.iter().enumerate() {
                    let c = Color32::from_rgb(*cr, *cg, *cb);
                    if swatch(ui, Some(c), r.colour == Some(i as u8), cname) {
                        r.colour = Some(i as u8);
                        label = Some("Reference colour");
                    }
                }
            })
        });
        crate::props::row(ui, "Strength", |ui| {
            if ui
                .add(egui::Slider::new(&mut r.opacity, 0.05..=1.0).show_value(false))
                .drag_stopped()
            {
                label = Some("Reference strength");
            }
        });
        if crate::props::choice(
            ui,
            "Drawn",
            &mut r.over,
            &[
                (false, "Under", "Under the model"),
                (true, "Over", "Over the model"),
            ],
        ) {
            label = Some(if r.over {
                "Reference over"
            } else {
                "Reference under"
            });
        }
        // one switch a row, so none runs off a narrow panel (MU34)
        if crate::props::switch(ui, &mut r.locked, "Locked").changed() {
            label = Some("Lock reference");
        }
        if crate::props::switch(ui, &mut r.hidden, "Hidden (Alt+F2)").changed() {
            label = Some("Hide reference");
        }
        // its 3D view, apart from its views', as Blender's image empties
        // (MU34)
        if !r.only_profile {
            crate::props::sub_box(ui, "ref.3d", "In 3D", None, true, |ui| {
                if crate::props::switch(ui, &mut r.in_3d, "Shown")
                    .on_hover_text(
                        "Shown in the 3D view too, where its views are; off, only in its views",
                    )
                    .changed()
                {
                    label = Some("Reference in 3D");
                }
                if r.in_3d {
                    let mut own = r.opacity_3d.is_some();
                    if crate::props::switch(ui, &mut own, "Own strength").changed() {
                        r.opacity_3d = own.then_some(r.opacity);
                        label = Some("Reference strength in 3D");
                    }
                    if let Some(o) = &mut r.opacity_3d {
                        crate::props::row(ui, "Strength", |ui| {
                            if ui
                                .add(egui::Slider::new(o, 0.05..=1.0).show_value(false))
                                .drag_stopped()
                            {
                                label = Some("Reference strength in 3D");
                            }
                        });
                    }
                    if crate::props::switch(ui, &mut r.in_perspective, "In perspective").changed() {
                        label = Some("Reference in perspective");
                    }
                    if crate::props::switch(ui, &mut r.in_orthographic, "In orthographic").changed()
                    {
                        label = Some("Reference in orthographic");
                    }
                }
            });
        }
        // its own box, folding (MU23)
        let open = self.prop_header(
            ui,
            "refplace",
            "Place and size",
            true,
            Some("trace-reference"),
        );
        crate::props::fold(ui, open, |ui| {
            let mut at = r.at;
            let mut changed = false;
            crate::props::row(ui, "X", |ui| {
                changed |= crate::props::fill(ui, crate::units::drag(&mut at.x)).changed()
            });
            crate::props::row(ui, "Y", |ui| {
                changed |= crate::props::fill(ui, crate::units::drag(&mut at.y)).changed()
            });
            if changed && !r.locked {
                r.at = at;
                label = Some("Move reference");
            }
            crate::props::row(ui, "Rotation", |ui| {
                if crate::props::fill(
                    ui,
                    egui::DragValue::new(&mut r.rotation).speed(0.5).suffix("°"),
                )
                .changed()
                {
                    label = Some("Turn reference");
                }
            });
            crate::props::row(ui, "Scale", |ui| {
                if crate::props::fill(
                    ui,
                    egui::DragValue::new(&mut r.scale)
                        .speed(0.01)
                        .range(0.0001..=10000.0)
                        .max_decimals(5),
                )
                .changed()
                {
                    label = Some("Scale reference");
                }
            });
            if let RefSource::Image { width, .. } = &mut r.source {
                let before = *width;
                crate::props::row(ui, "Picture width", |ui| {
                    crate::props::fill(ui, crate::units::drag(width).range(0.1..=10000.0))
                });
                if *width != before {
                    // the picture keeps its middle where it is
                    let k = *width / before;
                    if let Some([a, b]) = r.bounds {
                        r.bounds = Some([a * k, b * k]);
                    }
                    label = Some("Picture width");
                }
            }
            // three equal buttons across the row, as Blender's (MU24); stacked
            // when the panel is too narrow for their words (MU34)
            let mut pressed = None;
            let narrow = ui.available_width() < 270.0;
            let layout = if narrow {
                egui::Layout::top_down_justified(egui::Align::Min)
            } else {
                egui::Layout::left_to_right(egui::Align::Center)
            };
            ui.with_layout(layout, |ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let w = if narrow { ui.available_width() } else { ((ui.available_width() - 6.0) / 3.0).max(40.0) };
            for (i, (text, tip)) in [
                ("Calibrate…", "Click two points on it a known distance apart, then type that distance: it is scaled to fit (the first point stays)."),
                ("Align…", "Click two points on it (a wall line on a scan), then two on the model, a guide or an edge: it turns and moves so the first line lies on the second. Its size stays."),
                ("Back in place", "Where it was drawn: no move, turn or scale."),
            ]
            .into_iter()
            .enumerate()
            {
                if ui
                    .add_sized([w, 24.0], egui::Button::new(text).truncate())
                    .on_hover_text(tip)
                    .clicked()
                {
                    pressed = Some(i);
                }
            }
        });
            // the same drawing in another view, placed apart from this one
            // (MU38): moving one never moves the other
            let mut copy_to = None;
            let copy_btn = ui
                .add_sized(
                    [ui.available_width(), 24.0],
                    egui::Button::new("Copy to view…").truncate(),
                )
                .on_hover_text(
                    "A copy of it in another view, with its own place, size and settings",
                );
            egui::Popup::menu(&copy_btn).show(|ui| {
                for (v, name) in &views {
                    if r.shown_in.contains(v) {
                        continue;
                    }
                    if ui.button(name).clicked() {
                        copy_to = Some(*v);
                        ui.close();
                    }
                }
            });
            if let Some(v) = copy_to {
                self.copy_ref = Some((id, v));
            }
            match pressed {
                Some(0) => {
                    self.plan.calibrate = Some((id, vec![]));
                    self.plan.align_ref = None;
                    self.status = "Calibrate: click a first point on the reference (Alt: no snapping; Shift: square to it).".into();
                }
                Some(1) => {
                    self.plan.align_ref = Some((id, vec![]));
                    self.plan.calibrate = None;
                    self.status = "Align: click a first point on the reference.".into();
                }
                Some(_) => {
                    if let Some(b) = r.bounds {
                        r.in_place(b);
                        label = Some("Reference back in place");
                    }
                }
                None => {}
            }
        });
        label
    }

    /// Align's points picked: turn and move the reference (MU23); files
    /// dropped on a view become references there. While Calibrate or
    /// Align picks points, the reference stays selected, so its Properties
    /// stay in front.
    /// The note pipette (MU135): the next element selected becomes the
    /// reading note's object, and the note is selected again; Esc stops.
    fn note_pick_step(&mut self, ctx: &egui::Context) {
        let Some(note) = self.note_pick else { return };
        let gone = !matches!(
            self.doc.model().get(note).map(|e| &e.kind),
            Some(ElementKind::Note(n)) if n.reads.is_some()
        );
        if gone || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.note_pick = None;
            if !gone {
                self.selection.only(note);
                self.status = "Stopped picking.".into();
            }
            return;
        }
        match self.selection.active {
            Some(x) if x != note => {
                let Some(ElementKind::Note(mut n)) =
                    self.doc.model().get(note).map(|e| e.kind.clone())
                else {
                    return;
                };
                self.note_pick = None;
                if matches!(
                    self.doc.model().get(x).map(|e| &e.kind),
                    Some(
                        ElementKind::View(_)
                            | ElementKind::Note(_)
                            | ElementKind::Tag(_)
                            | ElementKind::Dimension(_)
                    )
                ) {
                    self.selection.only(note);
                    self.status =
                        "A note reads a model object, not an annotation or a view.".into();
                    return;
                }
                self.link_note(&mut n, x);
                let name = self
                    .doc
                    .model()
                    .get(x)
                    .map(|e| e.name.clone())
                    .unwrap_or_default();
                let _ = self
                    .doc
                    .edit(note, "Note object", |e| e.kind = ElementKind::Note(n));
                self.selection.only(note);
                self.prop_tab = PropTab::Element;
                self.status = format!("The note reads {name}. Ctrl+Z to undo.");
            }
            Some(_) => {}
            // a click on nothing keeps the note in front while it picks
            None => self.selection.only(note),
        }
    }

    fn reference_steps(&mut self) {
        let picking = self
            .plan
            .calibrate
            .as_ref()
            .map(|c| c.0)
            .or(self.plan.align_ref.as_ref().map(|a| a.0))
            .or(self.plan.calibrated.map(|c| c.0));
        if let Some(id) = picking {
            if self.selection.active != Some(id) {
                self.selection.only(id);
                self.prop_tab = PropTab::Element;
            }
        }
        if let Some((id, v)) = self.copy_ref.take() {
            if let Some(e) = self.doc.model().get(id).cloned() {
                if let ElementKind::Reference(r) = &e.kind {
                    let mut r = r.clone();
                    let to_profile = !matches!(
                        self.doc.model().get(v).map(|x| &x.kind),
                        Some(ElementKind::View(_))
                    );
                    r.shown_in = vec![v];
                    r.only_profile = to_profile;
                    r.hidden = false;
                    let name = e.name.clone();
                    let copy = Element::new(name.clone(), ElementKind::Reference(r));
                    let cid = copy.id;
                    let res = self.doc.commit(
                        creabim_core::Transaction::new(format!("Copy {name} to a view"))
                            .insert(copy),
                    );
                    if res.is_ok() {
                        self.selection.only(cid);
                    }
                    self.report(
                        res,
                        format!("{name} copied: the copy moves and scales apart from the first."),
                    );
                }
            }
        }
        if let Some(id) = self.remove_ref.take() {
            let tx = creabim_core::Transaction::new("Remove reference")
                .remove_cascade(self.doc.model(), id);
            if self.doc.commit(tx).is_ok() {
                self.selection.clear();
                self.plan.ref_cache.forget(id);
                self.status = "Reference taken away (Ctrl+Z brings it back).".into();
            }
        }
        if let Some((id, [a1, a2, b1, b2])) = self.plan.aligned.take() {
            if let Some(e) = self.doc.model().get(id).cloned() {
                let mut after = e.clone();
                if let ElementKind::Reference(r) = &mut after.kind {
                    match r.align(a1, a2, b1, b2) {
                        Ok(()) => {
                            let turn = r.rotation;
                            if self
                                .doc
                                .commit(
                                    creabim_core::Transaction::new("Align reference")
                                        .replace(e, after),
                                )
                                .is_ok()
                            {
                                self.status = format!("Aligned: turned to {turn:.2}°.");
                            }
                        }
                        Err(why) => self.status = why,
                    }
                }
            }
        }
        for (view, at, path) in std::mem::take(&mut self.plan.dropped) {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            let p = path.display().to_string();
            let source = match ext.as_str() {
                "png" | "jpg" | "jpeg" => creabim_core::RefSource::Image {
                    path: p,
                    width: 10.0,
                },
                "dxf" => creabim_core::RefSource::Dxf { path: p },
                e if MAP_LINE_FILES.contains(&e) || e == "tif" || e == "tiff" => {
                    self.add_map_reference(view, &p);
                    continue;
                }
                _ => {
                    self.status = format!(
                        "{} can't be a reference: drop a picture, a DXF drawing or map data.",
                        path.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("That file")
                    );
                    continue;
                }
            };
            let picture = matches!(source, creabim_core::RefSource::Image { .. });
            self.add_reference(view, source);
            // a picture lands where it was dropped; a drawing keeps its
            // own coordinates
            if let (true, Some(id)) = (picture, self.selection.active) {
                let _ = self.doc.edit_merged(id, "Place reference", |e| {
                    if let ElementKind::Reference(r) = &mut e.kind {
                        r.at = at;
                    }
                });
            }
        }
    }

    /// Calibrate's last step: the distance between the two points picked.
    fn calibrate_window(&mut self, ctx: &egui::Context) {
        let Some((id, a, b)) = self.plan.calibrated else {
            return;
        };
        let mut open = true;
        let mut done = false;
        let mut go = false;
        let id_d = egui::Id::new("calibrate.distance");
        let mut d: f64 = ctx.data(|m| m.get_temp(id_d)).unwrap_or(a.distance(b));
        crate::app::shell::window("Calibrate")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(format!(
                    "The two points are {} apart now. How far apart should they be?",
                    crate::units::len(a.distance(b))
                ));
                let r = ui.add(crate::units::drag(&mut d).range(0.001..=100000.0));
                r.request_focus();
                crate::props::flow(ui, |ui| {
                    go = ui.button("Scale it").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Enter));
                    done = ui.button("Cancel").clicked();
                });
            });
        ctx.data_mut(|m| m.insert_temp(id_d, d));
        if go {
            if let Some(e) = self.doc.model().get(id).cloned() {
                let mut after = e.clone();
                if let ElementKind::Reference(r) = &mut after.kind {
                    match r.calibrate(a, b, d) {
                        Ok(()) => {
                            let k = r.scale;
                            if self
                                .doc
                                .commit(
                                    creabim_core::Transaction::new("Calibrate reference")
                                        .replace(e, after),
                                )
                                .is_ok()
                            {
                                self.status = format!("Calibrated: scale {k:.4}.");
                            }
                        }
                        Err(why) => self.status = why,
                    }
                }
            }
            done = true;
        }
        if done || !open {
            self.plan.calibrated = None;
            ctx.data_mut(|m| m.remove::<f64>(id_d));
        }
    }

    /// Properties, then what its panels asked of the app this frame: Help
    /// at a page (the ? boxes, AMD-012) and how to use what is selected,
    /// for the status bar (the notes audit, 2026-11-12).
    fn properties(&mut self, ui: &mut egui::Ui) {
        crate::props::clear_later(ui.ctx());
        self.properties_inner(ui);
        crate::props::clear_later(ui.ctx());
        if let Some(page) = crate::props::take_help(ui.ctx()) {
            let page = if page.is_empty() {
                self.help_context()
            } else {
                page
            };
            self.open_help(&page);
        }
        if let Some(id) = crate::props::take_select(ui.ctx()) {
            self.selection.only(id);
        }
        // an Earthwork tab's Retaining wall tick: a wall of its own on
        // that edge of the selected work, or that wall taken away
        if let Some((edge, add)) = crate::partstool::take_wall(ui.ctx()) {
            if let Some(id) = self.selection.active {
                if let Some(tx) = crate::partstool::retaining_wall(self.doc.model(), id, edge, add)
                {
                    let r = self.doc.commit(tx);
                    self.report(
                        r,
                        if add {
                            "Retaining wall added on that edge: the ground is cut at its back. It is an ordinary wall: select it to move, stretch or thicken it."
                        } else {
                            "Retaining wall removed: the edge is cut upright with nothing to hold it."
                        }
                        .to_string(),
                    );
                }
            }
        }
        self.plan.howto = crate::props::current_howto(ui.ctx());
    }

    fn properties_inner(&mut self, ui: &mut egui::Ui) {
        let _perf = Perf::new("properties");
        if self.crop_pick.is_some() {
            self.crop_pick_banner(ui);
        }
        let tab = self.prop_tab;
        if tab == PropTab::Select {
            self.select_tab(ui);
            return;
        }
        // a master, a profile or the log: not the element picked before (#42)
        if tab.of_its_view() {
            match tab {
                PropTab::Profile => self.profile_props(ui),
                PropTab::Master => self.master_props(ui),
                _ => self.log_props(ui),
            }
            return;
        }
        let Some(id) = self.selection.active else {
            if tab == PropTab::Next {
                self.next_element_panel(ui);
                return;
            }
            if tab == PropTab::Project {
                self.project_props(ui);
                self.time_props(ui);
                return;
            }
            if tab == PropTab::Phases {
                self.phases_tab(ui);
                return;
            }
            if tab == PropTab::Notes {
                self.notes_sections(ui, notesui::NotesAt::Panel);
                return;
            }
            // Profile, Master and Log are shown above, whatever is picked
            if tab == PropTab::Pattern {
                self.pattern_props(ui);
                return;
            }
            if tab == PropTab::Sheet {
                if let Some(sheet) = self.sheet_in_front() {
                    self.sheet_tab(ui, sheet);
                }
                return;
            }
            // the 3D view's settings now live in the View tab (REV04 §4); a
            // Model3d tab kept from an older session shows the same
            if tab == PropTab::Visuals {
                self.visuals_tab(ui);
                return;
            }
            if tab == PropTab::Render {
                self.render_tab(ui);
                return;
            }
            if tab == PropTab::Model3d || (tab == PropTab::View && self.model_in_front()) {
                self.view_tab_3d(ui);
                return;
            }
            // (the Grid tool's systems are in its own tab: REV01, MU68)
            // with nothing selected, the view shown in the 2D pane
            if let Some(gridvis::VisTarget::View(v)) = self.vis_target(false) {
                if tab == PropTab::Trace {
                    self.trace_panel(ui, v);
                    return;
                }
                self.view_tab_body(ui, v);
            }
            return;
        };
        let Some(e) = self.doc.model().get(id).cloned() else {
            return;
        };
        // the Log area in front is the Log, whatever is selected (MU99: it
        // showed the ramp's fields under a Log heading)
        if tab == PropTab::Log {
            self.log_props(ui);
            return;
        }
        // so are the Profile, Master and Pattern areas' own tabs
        if tab == PropTab::Profile {
            self.profile_props(ui);
            return;
        }
        if tab == PropTab::Master {
            self.master_props(ui);
            return;
        }
        if tab == PropTab::Pattern {
            self.pattern_props(ui);
            return;
        }
        // (its name, mark and actions are in the pinned header above)
        if tab == PropTab::View {
            if self.model_in_front() {
                self.view_tab_3d(ui);
            } else if let Some(gridvis::VisTarget::View(v)) = self.vis_target(false) {
                self.view_tab_body(ui, v);
            }
            return;
        }
        if tab == PropTab::Project {
            self.project_props(ui);
            self.time_props(ui);
            return;
        }
        if tab == PropTab::Phases {
            self.phases_tab(ui);
            return;
        }
        if tab == PropTab::Notes {
            self.notes_sections(ui, notesui::NotesAt::Panel);
            return;
        }
        if tab == PropTab::Sheet {
            if let Some(sheet) = self.sheet_in_front() {
                self.sheet_tab(ui, sheet);
            }
            return;
        }
        if tab == PropTab::Modifiers {
            self.modifiers_tab(ui, id);
            return;
        }
        if tab == PropTab::Tree {
            self.tree_tab(ui, id);
            return;
        }
        if matches!(
            tab,
            PropTab::Ground
                | PropTab::Earthwork
                | PropTab::Road
                | PropTab::Drain
                | PropTab::Path
                | PropTab::Tank
                | PropTab::Property
                | PropTab::Paving
        ) {
            self.site_tab(ui, id);
            return;
        }
        if tab == PropTab::Parking {
            self.parking_tab(ui, id);
            return;
        }
        if tab == PropTab::Data {
            self.data_tab(ui, id);
            return;
        }
        if tab == PropTab::Joins {
            if creabim_core::joins::joinable(&e.kind) {
                let open = self.prop_header(ui, "joins", "Junctions", true, Some("joins"));
                crate::props::fold(ui, open, |ui| {
                    self.joins_section(ui, id);
                });
            }
            self.cuts_panel(ui, id);
            return;
        }
        if tab == PropTab::Graphics {
            if matches!(e.kind, ElementKind::View(_)) {
                self.view_tab_body(ui, id);
            } else {
                self.element_override_section(ui, id);
            }
            return;
        }
        // where it is: its level first, the same row for every element that
        // hangs off one (Properties order REV04 §1: Element = what and where)
        self.level_row(ui, &e);
        self.part_of_row(ui, &e);
        if creabim_core::types::TypeCategory::of(&e.kind).is_some() {
            self.type_picker(ui, id);
        }
        // when it is built and when it comes down (phases)
        self.element_time(ui, id);
        // the type picker may have changed it
        let Some(e) = self.doc.model().get(id).cloned() else {
            return;
        };
        // its own folding panel, as Type and Geometry are (MU32)
        let hint = format!("say:{}", editmode::panel_hint(&e.kind));
        if self.plan.edit == Some(id) {
            let open = self.prop_header(ui, "editing", "Editing", true, Some(&hint));
            crate::props::fold(ui, open, |ui| {
                // the layers' grips, without having to know Tab (MU18), as a
                // choice in the field column
                if handles::has_layers(self.doc.model(), id) {
                    let mut on = handles::layers();
                    let stair = matches!(e.kind, ElementKind::Stair(_));
                    let options: [(bool, &str, &str); 2] = if stair {
                        [
                            (false, "Line", "The stair's line (Shift+Tab)"),
                            (
                                true,
                                "Pieces",
                                "A grip on each flight and landing (Shift+Tab)",
                            ),
                        ]
                    } else {
                        [
                            (false, "Element", "The element's own grips (Shift+Tab)"),
                            (true, "Layers", "A grip at each layer's ends (Shift+Tab)"),
                        ]
                    };
                    if crate::props::choice(ui, "Grips", &mut on, &options) {
                        handles::set_layers(on);
                        self.status = if stair && on {
                            "Flights and landings: drag a flight's top riser for more or fewer treads, a landing back to make it deeper. Right-click one to reset it.".into()
                        } else if stair {
                            "Back to the stair's line.".into()
                        } else if on {
                            "Layers: a grip at each layer's ends (and, on walls, its bottom and top). Drag one to run that layer on or hold it back; right-click to reset.".into()
                        } else {
                            "Back to the element's own grips.".into()
                        };
                    }
                }
                ui.add_space(4.0);
            });
        }

        let mut edited = e.clone();
        let mut label = "";
        let mut extrude_sketch = false;
        let mut separate_shape = false;
        let mut join_shapes = false;
        let mut draw_hole = false;
        let num = |ui: &mut egui::Ui, text: &str, v: &mut f64, lo: f64, hi: f64| -> bool {
            crate::props::row(ui, text, |ui| {
                crate::props::fill(ui, crate::units::drag(v).range(lo..=hi)).changed()
            })
        };
        let mut redraw = false;
        let mut new_style = false;
        let show_visibility = matches!(e.kind, ElementKind::View(_));
        let mut object_type = false;
        // a placed view's new scale: (its view, the scale)
        let mut rescale: Option<(ElementId, f64)> = None;
        // who it is comes first (above), then its sizes and build here,
        // then how views show it; each section folds and stays folded
        let section = if matches!(e.kind, ElementKind::Object(_)) {
            // an object's own block holds where it stands and how it is
            // turned, nothing built: named for that, and short enough to
            // read whole at the panel's narrow width (MU119)
            "Placement"
        } else if creabim_core::types::TypeCategory::of(&e.kind).is_some() {
            // its sizes and how it is built. One word: "Geometry and build"
            // was cut to "Geometry an…" at the panel's usual width (MU119)
            "Geometry"
        } else {
            // named for what it is, as Blender's panels (MU23)
            match &e.kind {
                ElementKind::Reference(_) => "Reference",
                ElementKind::Note(n) if n.symbol.is_some() => "Symbol",
                ElementKind::Note(_) => "Note",
                ElementKind::PaperMark(m) => match m.kind {
                    creabim_core::PaperMarkKind::NorthArrow { .. } => "North arrow",
                    creabim_core::PaperMarkKind::ScaleBar { .. } => "Scale bar",
                    creabim_core::PaperMarkKind::Cloud { .. } => "Revision cloud",
                    creabim_core::PaperMarkKind::RevisionTable => "Revisions table",
                    creabim_core::PaperMarkKind::Picture { .. } => "Picture",
                    creabim_core::PaperMarkKind::Vector { .. } => "Vector picture",
                },
                _ => "Settings",
            }
        };
        // a plan's fold explains which way it looks (owner, issue #70:
        // "up means reflected ceiling plan")
        let help = match &e.kind {
            ElementKind::View(v) if matches!(v.kind, ViewKind::Plan { .. }) => {
                Some("views#which-way-a-plan-looks")
            }
            _ => None,
        };
        // a viewport's title on the sheet: a fold of its own after the
        // settings, so shutting them leaves it be (MU98_08)
        let title_of = match &e.kind {
            ElementKind::Viewport(vp) => Some(vp.view),
            _ => None,
        };
        // a picture of it, above Geometry (Preview REV00; owner MU105: below
        // it, a long Geometry pushed the picture out of sight)
        self.element_preview(ui, id);
        // what it is made of: a fold of its own (Mockup_Material_Section_REV00,
        // owner A on all), after the picture and before Geometry (owner
        // markup 2026-11-23)
        if let Some(l) = self.material_section(ui, &mut edited) {
            label = l;
        }
        // what it declares: fire, acoustic, U-value, load-bearing, external
        // (Common properties REV00)
        if let Some(l) = self.ratings_section(ui, &mut edited) {
            label = l;
        }
        let open = self.prop_header(ui, "element", section, true, help);
        crate::props::fold(ui, open, |ui| {
            match &mut edited.kind {
                ElementKind::Level(l) => {
                    if num(ui, "Elevation", &mut l.elevation, -100.0, 1000.0) {
                        label = "Level elevation"
                    }
                    if num(ui, "Height", &mut l.height, 0.5, 50.0) {
                        label = "Level height"
                    }
                    crate::props::info(ui, "Moving a level moves everything on it.", "");
                    if ui
                        .button("Levels…")
                        .on_hover_text("Add, duplicate, rename and delete levels")
                        .clicked()
                    {
                        self.open_levels();
                    }
                    self.level_head_controls(ui, id);
                }
                ElementKind::Wall(w) => {
                    // a retaining wall names the earthwork it holds
                    let holds = self.doc.model().iter().find_map(|e| match &e.kind {
                        ElementKind::Site(creabim_core::Site {
                            form: creabim_core::SiteForm::Pad { walls, .. },
                            ..
                        }) => walls
                            .iter()
                            .find(|w| w.1 == id)
                            .map(|w| (e.id, format!("{}, edge {}", e.name, w.0 + 1))),
                        _ => None,
                    });
                    if let Some((pad, text)) = holds {
                        if crate::props::relation(
                            ui,
                            "Holds",
                            &text,
                            "The earthwork edge this wall retains: click to select it",
                            None,
                        )
                        .select
                        {
                            crate::props::ask_select(ui, pad);
                        }
                    }
                    let mut curtain = w.curtain.is_some();
                    if crate::props::switch(ui, &mut curtain, "Curtain wall")
                    .on_hover_text("A grid of mullions holding glass or solid panels; doors in it take the place of panels")
                    .changed()
                {
                    w.curtain = curtain.then(|| creabim_core::CurtainGrid {
                        mullion_depth: w.thickness.clamp(0.05, 0.3),
                        ..Default::default()
                    });
                    if curtain {
                        w.assembly = None;
                    }
                    label = "Curtain wall"
                }
                    if let Some(c) = &mut w.curtain {
                        let hosts: Vec<String> =
                            creabim_core::curtain::hosts_of(self.doc.model(), id)
                                .iter()
                                .filter_map(|(h, _)| {
                                    self.doc.model().get(*h).map(|e| e.name.clone())
                                })
                                .collect();
                        if !hosts.is_empty() {
                            crate::props::relation(
                                ui,
                                "Set in",
                                &hosts.join(", "),
                                "It cuts its own opening there, like a door, and the opening moves with it",
                                None,
                            );
                        }
                        let l = typesui::curtain_fields(ui, c);
                        if !l.is_empty() {
                            label = l;
                        }
                        // a curtain wall is as thick as its mullions
                        w.thickness = c.mullion_depth;
                    } else {
                        let layered = w.assembly.is_some_and(|a| {
                            matches!(
                                self.doc.model().get(a).map(|e| &e.kind),
                                Some(ElementKind::Assembly(_))
                            )
                        });
                        if layered {
                            crate::props::readout(
                                ui,
                                "Thickness",
                                crate::units::len(w.thickness),
                                "Worked out from its layers: change the assembly to change it",
                            );
                        } else if num(ui, "Thickness", &mut w.thickness, 0.02, 2.0) {
                            label = "Wall thickness"
                        }
                    }
                    let l = wallsui::wall_fields(ui, self.doc.model(), id, w);
                    if !l.is_empty() {
                        label = l;
                    }
                    if crate::props::action(
                        ui,
                        "Path",
                        "Redraw",
                        "Click a new path for this wall, in a plan or the 3D view. More corners make more walls like it.",
                        true,
                    )
                    .clicked()
                    {
                        redraw = true;
                    }
                    let l = wallsui::wall_worked(ui, w);
                    if !l.is_empty() {
                        label = l;
                    }
                }
                ElementKind::Slab(s) => {
                    let layered = s.assembly.is_some_and(|a| {
                        matches!(
                            self.doc.model().get(a).map(|e| &e.kind),
                            Some(ElementKind::Assembly(_))
                        )
                    });
                    let mut thickness = s.thickness;
                    if layered {
                        crate::props::readout(
                            ui,
                            "Thickness",
                            crate::units::len(s.thickness),
                            "Worked out from its layers: change the assembly to change it",
                        );
                    } else if num(ui, "Thickness", &mut thickness, 0.02, 3.0) {
                        s.set_thickness(thickness, self.plan.slab_keep_bottom);
                        label = "Slab thickness"
                    }
                    crate::props::choice(
                        ui,
                        "Keep",
                        &mut self.plan.slab_keep_bottom,
                        &[
                            (
                                false,
                                "Top",
                                "A new thickness moves the underside; the top stays put.",
                            ),
                            (
                                true,
                                "Bottom",
                                "A new thickness moves the top; the underside stays put.",
                            ),
                        ],
                    );
                    if num(ui, "Top offset", &mut s.top_offset, -100.0, 100.0) {
                        label = "Slab top offset"
                    }
                    let level = self.doc.model().level(s.level).map_or(0.0, |l| l.elevation);
                    let arcs = s.bulges.iter().filter(|b| b.abs() > 1e-9).count();
                    // as Blender: the operator in the field column, what it
                    // works on said under it (MU31)
                    if crate::props::action(
                        ui,
                        "Outline",
                        "Redraw",
                        "Click a new outline for this slab, in a plan or the 3D view.",
                        true,
                    )
                    .clicked()
                    {
                        redraw = true;
                    }
                    let shafts = creabim_core::slab::holes_of(self.doc.model(), &e)
                        .len()
                        .saturating_sub(s.holes.len());
                    let (l, draw) = typesui::holes_fields(ui, &mut s.holes, shafts);
                    if !l.is_empty() {
                        label = l;
                    }
                    draw_hole |= draw;
                    // what is worked out, after its fields (AMD-015)
                    {
                        use crate::props::Worked;
                        let mut worked = vec![
                            Worked::new("Top", crate::units::elevation(s.top(level))),
                            Worked::new(
                                "Underside",
                                crate::units::elevation(s.top(level) - s.thickness),
                            ),
                            Worked::new("Corners", s.boundary.len().to_string()),
                        ];
                        if arcs > 0 {
                            worked.push(Worked::new("Arcs", arcs.to_string()));
                        }
                        worked.push(Worked::new("Area", crate::units::area(s.area())));
                        crate::props::worked_out(ui, "slab", &worked);
                    }
                }
                ElementKind::Opening(o) if o.roof_at.is_some() => {
                    // a roof window: across and up its slope, placed in plan
                    if num(ui, "Width", &mut o.width, 0.3, 5.0) {
                        label = "Roof window width"
                    }
                    if num(ui, "Height", &mut o.height, 0.3, 5.0) {
                        label = "Roof window height"
                    }
                    let mut at = o.roof_at.unwrap_or_default();
                    let x = num(ui, "Centre X", &mut at.x, -1e4, 1e4);
                    let y = num(ui, "Centre Y", &mut at.y, -1e4, 1e4);
                    if x || y {
                        o.roof_at = Some(at);
                        label = "Move roof window"
                    }
                    let l = wallsui::roof_window_fields(ui, self.doc.model(), o);
                    if !l.is_empty() {
                        label = l;
                    }
                    ui.separator();
                    ui.label(egui::RichText::new("Frame").strong());
                    if num(ui, "Frame width", &mut o.frame.width, 0.01, 0.5) {
                        label = "Frame width"
                    }
                    if num(ui, "Frame depth", &mut o.frame.depth, 0.02, 1.0) {
                        label = "Frame depth"
                    }
                    // how it is built round its glass (Mockup_Roof_Window_REV00,
                    // approved A on all five): every part has its default
                    let rb = &mut o.roof_build;
                    let (u60, u90) = (crate::units::len(0.06), crate::units::len(0.09));
                    if crate::props::choice(
                        ui,
                        "Upstand",
                        &mut rb.upstand,
                        &[
                            (0.0, "None", "Flush: the frame sits down on the roofing"),
                            (0.06, &u60, "The frame stands this far above the roofing"),
                            (0.09, &u90, "The frame stands this far above the roofing"),
                        ],
                    ) {
                        label = "Roof window upstand"
                    }
                    if crate::props::choice(
                        ui,
                        "Glazing",
                        &mut rb.glazing,
                        &[
                            (1, "Single", "One pane"),
                            (2, "Double", "A sealed unit of two panes"),
                            (3, "Triple", "A sealed unit of three panes"),
                        ],
                    ) {
                        label = "Roof window glazing"
                    }
                    if crate::props::check(ui, "Flashing", &mut rb.flashing, "Collar")
                        .on_hover_text("A flashing collar round it, lapped into the roofing")
                        .changed()
                    {
                        label = "Roof window flashing"
                    }
                    ui.label(egui::RichText::new("Inside").strong());
                    if crate::props::choice(
                        ui,
                        "Lining",
                        &mut rb.lining,
                        &[
                            (
                                creabim_core::RoofLining::Square,
                                "Square",
                                "Square to the glass, straight through the roof",
                            ),
                            (
                                creabim_core::RoofLining::Splayed,
                                "Splayed",
                                "Head level and sill plumb: the most light in",
                            ),
                        ],
                    ) {
                        label = "Roof window lining"
                    }
                    if crate::props::check(ui, "Insulation", &mut rb.collar, "Collar")
                        .on_hover_text("An insulation collar round its frame, in the roof")
                        .changed()
                    {
                        label = "Roof window insulation"
                    }
                }
                ElementKind::Opening(o) => {
                    // how it opens, first: the first thing chosen (MU105)
                    if !self.shell.classic {
                        wallsui::opens_row(ui, o.kind, &o.frame);
                    }
                    let old = o.width;
                    if num(ui, "Width", &mut o.width, 0.1, 20.0) {
                        // about its centre, or from the jamb by a corner
                        creabim_core::walls::widen_in_place(self.doc.model(), old, o);
                        label = "Opening width"
                    }
                    if num(ui, "Height", &mut o.height, 0.1, 20.0) {
                        label = "Opening height"
                    }
                    if num(ui, "Sill", &mut o.sill, 0.0, 20.0) {
                        label = "Opening sill"
                    }
                    if crate::props::num_tip(
                        ui,
                        "Along wall",
                        "Offset along wall: how far it is from the wall's start, measured along the wall",
                        &mut o.offset,
                        0.0,
                        1e4,
                    ) {
                        label = "Move opening"
                    }
                    // the kind is chosen in its block above (picker REV00)
                    wallsui::HIDE_OPENS.with(|h| h.set(!self.shell.classic));
                    let l = wallsui::opening_fields(ui, self.doc.model(), o);
                    wallsui::HIDE_OPENS.with(|h| h.set(false));
                    if let Some(r) = wallsui::PICK_KIND.with(|k| k.take()) {
                        let key = if o.kind == creabim_core::OpeningKind::Door {
                            "kinds:door:sel"
                        } else {
                            "kinds:window:sel"
                        };
                        self.open_kinds(key, r);
                    }
                    if !l.is_empty() {
                        label = l;
                    }
                    ui.separator();
                    ui.label(egui::RichText::new("Frame").strong());
                    if num(ui, "Width", &mut o.frame.width, 0.01, 0.5) {
                        label = "Frame width"
                    }
                    if num(ui, "Depth", &mut o.frame.depth, 0.02, 1.0) {
                        label = "Frame depth"
                    }
                    if o.kind == creabim_core::OpeningKind::Window
                        && wallsui::window_panes(ui, &mut o.frame, (o.width, o.height))
                    {
                        label = "Window panes";
                    }
                    let l =
                        wallsui::construction_fields(ui, o.kind, &mut o.frame, (o.width, o.height));
                    if !l.is_empty() {
                        label = l;
                    }
                }
                ElementKind::View(v) if matches!(v.kind, ViewKind::Schedule { .. }) => {
                    if let ViewKind::Schedule { category, by_type } = &mut v.kind {
                        crate::app::helpui::combo(ui, "Lists")
                            .selected_text(category.label())
                            .show_ui(ui, |ui| {
                                for c in ScheduleCategory::ALL {
                                    if ui.selectable_value(category, c, c.label()).changed() {
                                        label = "Schedule category"
                                    }
                                }
                            });
                        let (what, hint, undo) = if *category == ScheduleCategory::Materials {
                            (
                                "Group by level",
                                "List each level's materials under a heading with its totals.",
                                "Group take-off by level",
                            )
                        } else {
                            (
                                "Group by type",
                                "List the rows under a heading per element type, with its count.",
                                "Group schedule by type",
                            )
                        };
                        if crate::props::switch(ui, by_type, what)
                            .on_hover_text(hint)
                            .changed()
                        {
                            label = undo
                        }
                    }
                    if self.active_view != Some(id) && ui.button("Open this schedule").clicked() {
                        self.active_view = Some(id);
                    }
                }
                ElementKind::View(v) => {
                    // what it is and where (Mockup_View_Tabs_REV00): how it
                    // is drawn is all in the View tab, nothing twice
                    let kind = match &v.kind {
                        ViewKind::Plan { .. } if v.ceiling_plan => "Ceiling plan",
                        ViewKind::Plan { .. } => "Floor plan",
                        ViewKind::Section { cut: true, .. } => "Section",
                        ViewKind::Section { cut: false, .. } => "Elevation",
                        ViewKind::Axonometric { .. } => "3D view",
                        ViewKind::LongSection { .. } => "Long section",
                        ViewKind::Schedule { .. } => "Schedule",
                        ViewKind::BoreholeLog { .. } => "Borehole log",
                        ViewKind::WalkSection { .. } => "Walk-line section",
                    };
                    crate::props::row(ui, "Kind", |ui| ui.add(egui::Label::new(kind).truncate()));
                    if let ViewKind::Plan { level, .. } = &v.kind {
                        let name = self
                            .doc
                            .model()
                            .get(*level)
                            .map_or_else(|| "None".to_string(), |e| e.name.clone());
                        crate::props::row(ui, "Level", |ui| {
                            ui.add(egui::Label::new(name).truncate())
                        });
                    }
                    // a section's line is where it is
                    if let ViewKind::Section { start, end, .. } = &mut v.kind {
                        if num(ui, "Start X", &mut start.x, -1e4, 1e4)
                            | num(ui, "Start Y", &mut start.y, -1e4, 1e4)
                            | num(ui, "End X", &mut end.x, -1e4, 1e4)
                            | num(ui, "End Y", &mut end.y, -1e4, 1e4)
                        {
                            label = "Move section line"
                        }
                    }
                    // the sheets it is placed on
                    let model = self.doc.model();
                    let mut on: Vec<String> = model
                        .iter()
                        .filter_map(|e| match &e.kind {
                            ElementKind::Viewport(vp) if vp.view == id => {
                                model.get(vp.sheet).and_then(|s| match &s.kind {
                                    ElementKind::Sheet(sh) => {
                                        Some(format!("{} {}", sh.number, s.name))
                                    }
                                    _ => None,
                                })
                            }
                            _ => None,
                        })
                        .collect();
                    on.sort();
                    on.dedup();
                    let placed = if on.is_empty() {
                        "On no sheet".to_string()
                    } else {
                        on.join(", ")
                    };
                    crate::props::row(ui, "Placed on", |ui| {
                        ui.add(egui::Label::new(&placed).truncate())
                            .on_hover_text(&placed)
                    });
                    if crate::props::action_icon(
                        ui,
                        "",
                        "eye",
                        "Open its View tab",
                        "Its scale, range, graphics and template, in the View tab",
                        true,
                    )
                    .clicked()
                    {
                        self.active_view = Some(id);
                        self.prop_tab = PropTab::View;
                    }
                }
                ElementKind::Sheet(sh) => {
                    if let Some(l) = self.sheet_fields(ui, id, sh) {
                        label = l;
                    }
                    if let Some(l) = self.sheet_template_ui(ui, sh) {
                        label = l;
                    }
                    // offered only when no area shows it: "active" is the
                    // area last worked in, so a sheet already on screen
                    // beside a 3D view still offered it, and clicking only
                    // moved the focus (owner)
                    let on_screen = self
                        .areas
                        .root
                        .areas()
                        .contains(&crate::areas::Shows::View(id));
                    if !on_screen
                        && crate::props::action(
                            ui,
                            "",
                            "Open this sheet",
                            "Show it in the sheet view",
                            true,
                        )
                        .clicked()
                    {
                        self.active_view = Some(id);
                    }
                }
                ElementKind::Viewport(vp) => {
                    let view_name = self
                        .doc
                        .model()
                        .get(vp.view)
                        .map(|e| e.name.clone())
                        .unwrap_or_default();
                    // what it shows and what it is on, as relations (DS-01,
                    // MU55-03: no way to see or pick the sheet's size)
                    if crate::props::relation(
                        ui,
                        "Shows",
                        &view_name,
                        "The view it shows: click to open the view itself",
                        None,
                    )
                    .select
                    {
                        self.active_view = Some(vp.view);
                    }
                    let paper = match self.doc.model().get(vp.sheet).map(|e| &e.kind) {
                        Some(ElementKind::Sheet(sh)) => Some(format!(
                            "{} · {} {}",
                            sh.number,
                            creabim_core::standards::size_name(
                                &creabim_core::standards::of(self.doc.model()),
                                sh.width,
                                sh.height
                            )
                            .unwrap_or_else(|| "custom size".into()),
                            if sh.portrait() {
                                "portrait"
                            } else {
                                "landscape"
                            }
                        )),
                        _ => None,
                    };
                    if let Some(paper) = paper {
                        if crate::props::relation(
                            ui,
                            "On sheet",
                            &paper,
                            "The sheet it is placed on and its paper: click to set its size and orientation in the Sheet tab",
                            None,
                        )
                        .select
                        {
                            self.prop_tab = PropTab::Sheet;
                        }
                    }
                    // its scale is the view's: a new one redraws it at that
                    // size, round the viewport's centre
                    let view = vp.view;
                    let scale = self.view(view).and_then(|v| {
                        (!matches!(v.kind, ViewKind::Schedule { .. })).then_some(v.scale)
                    });
                    if let Some(scale) = scale {
                        if let Some(s) = sheetview::scale_picker(ui, id, scale) {
                            rescale = Some((view, s));
                        }
                    }
                    let mm = |ui: &mut egui::Ui, text: &str, v: &mut f64| {
                        crate::props::row(ui, text, |ui| {
                            crate::props::fill(ui, crate::units::paper(v)).changed()
                        })
                    };
                    if mm(ui, "Centre X", &mut vp.center.x) | mm(ui, "Centre Y", &mut vp.center.y) {
                        label = "Move view on sheet"
                    }
                    // following a stair: shown as it crops now, and let go of
                    // if cropped here by hand
                    if let Some(stair) = vp.fit {
                        let (c, t) = creabim_geom::sheet::viewport_frame(self.doc.model(), vp);
                        vp.crop = c;
                        vp.target = t;
                        let name = self
                            .doc
                            .model()
                            .get(stair)
                            .map_or("its stair".to_string(), |e| e.name.clone());
                        let mut follow = true;
                        if crate::props::switch(ui, &mut follow, format!("Crop follows {name}"))
                            .changed()
                        {
                            vp.fit = None;
                            label = "Stop following the stair"
                        }
                    }
                    let mut cropped = vp.crop.is_some();
                    if crate::props::switch(ui, &mut cropped, "Crop the view").changed() {
                        vp.crop = if cropped {
                            let view = vp.view;
                            self.drawing(view)
                                .and_then(|d| d.bounds())
                                .map(|(lo, hi)| [lo, hi])
                        } else {
                            None
                        };
                        label = "Crop view"
                    }
                    if let Some([lo, hi]) = &mut vp.crop {
                        if num(ui, "Crop left", &mut lo.x, -1e4, hi.x - 0.1)
                            | num(ui, "Crop right", &mut hi.x, lo.x + 0.1, 1e4)
                            | num(ui, "Crop bottom", &mut lo.y, -1e4, hi.y - 0.1)
                            | num(ui, "Crop top", &mut hi.y, lo.y + 0.1, 1e4)
                        {
                            label = "Crop view"
                        }
                    }
                    if label == "Crop view" {
                        vp.fit = None;
                    }
                    crate::props::howto(
                        ui,
                        "Viewport: drag it on the sheet to move it; double-click it to edit the model through it.",
                    );
                }
                ElementKind::Assembly(_) => {
                    self.assembly_properties(ui, id);
                }
                ElementKind::CropBox(c) => {
                    if let Some(l) = self.crop_properties(ui, id, c) {
                        label = l
                    }
                }
                ElementKind::ElementType(_) => {
                    self.type_properties(ui, id);
                }
                ElementKind::Shaft(s) => {
                    let levels: Vec<(ElementId, String)> = self
                        .doc
                        .model()
                        .levels()
                        .map(|(e, _)| (e.id, e.name.clone()))
                        .collect();
                    let name = |id: Option<ElementId>| {
                        id.and_then(|i| levels.iter().find(|l| l.0 == i))
                            .map_or("One storey up".to_string(), |l| l.1.clone())
                    };
                    crate::app::helpui::combo(ui, "Base level")
                        .selected_text(name(Some(s.level)))
                        .show_ui(ui, |ui| {
                            for (lid, n) in &levels {
                                if ui.selectable_value(&mut s.level, *lid, n).changed() {
                                    label = "Shaft base"
                                }
                            }
                        });
                    if num(ui, "Base offset", &mut s.base_offset, -100.0, 100.0) {
                        label = "Shaft base"
                    }
                    crate::app::helpui::combo(ui, "Top level")
                        .selected_text(name(s.top_level))
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_value(&mut s.top_level, None, "One storey up")
                                .changed()
                            {
                                label = "Shaft top"
                            }
                            for (lid, n) in &levels {
                                if ui
                                    .selectable_value(&mut s.top_level, Some(*lid), n)
                                    .changed()
                                {
                                    label = "Shaft top"
                                }
                            }
                        });
                    if num(ui, "Top offset", &mut s.top_offset, -100.0, 100.0) {
                        label = "Shaft top"
                    }
                    let (z0, z1) = s.span(self.doc.model());
                    let area = creabim_core::polygon::signed_area2(&s.boundary).abs() / 2.0;
                    ui.label(
                    egui::RichText::new(format!(
                        "From {} up to {}, {}. It cuts a hole in every slab and roof in between that it lies inside; move or reshape it (Edit) and they follow. Pick it in any view it passes through.",
                        crate::units::elevation(z0),
                        crate::units::elevation(z1),
                        crate::units::area(area)
                    ))
                    .weak(),
                );
                }
                ElementKind::Material(m) => {
                    materials::preview(ui, m, 72.0);
                    let pack = creabim_core::styles::active_pack(self.doc.model()).into_owned();
                    label = materials::material_fields(ui, m, &pack);
                    let users = creabim_core::library::users(self.doc.model(), id).len();
                    ui.label(egui::RichText::new(format!("Used by {users} elements.")).weak());
                    if ui.button("Open in the material browser").clicked() {
                        self.open_materials(Some(id));
                    }
                }
                ElementKind::Dimension(d) => {
                    let measured = self
                        .drawing(d.view)
                        .and_then(|dr| {
                            dr.items.into_iter().find_map(|i| match i {
                                // the real measure, whatever its Text card
                                // makes it read (MU80 3A: Worked out keeps it)
                                PlanItem::Dimension { id: i, a, b, .. } if i == id => {
                                    Some(creabim_core::write_number(
                                        self.doc.model(),
                                        creabim_core::NumberKind::Dimensions,
                                        d.format,
                                        a.distance(b),
                                    ))
                                }
                                PlanItem::Annotation { id: i, item } if i == id => Some(item.text),
                                _ => None,
                            })
                        })
                        .unwrap_or_else(|| "-".into());
                    // metric dimension text carries no unit; say which here
                    let unit = if !matches!(
                        d.kind,
                        creabim_core::DimensionKind::Linear
                            | creabim_core::DimensionKind::ArcLength
                    ) || crate::units::get().is_imperial()
                    {
                        String::new()
                    } else {
                        format!(" {}", crate::units::symbol())
                    };
                    let attached = [Some(d.a), Some(d.b), d.center]
                        .iter()
                        .flatten()
                        .filter(|p| p.host.is_some())
                        .count();
                    let worked = [
                        crate::props::Worked::new(d.kind.label(), format!("{measured}{unit}")),
                        crate::props::Worked::new(
                            "Attached",
                            match attached {
                                0 => "No: stays where it was drawn".to_string(),
                                1 => "1 end: follows what it measures".to_string(),
                                n => format!("{n} ends: follows what it measures"),
                            },
                        ),
                    ];
                    if d.kind == creabim_core::DimensionKind::Spot {
                        let mut pattern = d
                            .pattern
                            .clone()
                            .unwrap_or_else(|| creabim_core::annotation::SPOT_PATTERN.into());
                        crate::props::flow(ui, |ui| {
                            ui.label("Reads").on_hover_text(
                                "{elevation} in the project's units, {m} in metres (+0.45), {m3} to three places, {mm} in millimetres, {level} the level's name. For example: {m} FFL",
                            );
                            if ui.text_edit_singleline(&mut pattern).changed() {
                                d.pattern = Some(pattern.clone());
                                label = "Spot level text";
                            }
                        });
                    } else {
                        let what = crate::annotate::offset_label(d.kind);
                        if num(ui, what, &mut d.offset, -100.0, 100.0) {
                            label = "Move dimension line"
                        }
                    }
                    if d.kind == creabim_core::DimensionKind::Linear {
                        crate::app::helpui::combo(ui, "Measures")
                            .selected_text(d.axis.label())
                            .show_ui(ui, |ui| {
                                for a in creabim_core::DimAxis::ALL {
                                    if ui.selectable_value(&mut d.axis, a, a.label()).changed() {
                                        label = "Dimension direction"
                                    }
                                }
                            });
                    }
                    match crate::annotate::look_ui(ui, self.doc.model(), &mut d.style, &mut d.color)
                    {
                        crate::annotate::LookEdit::Changed(l) => label = l,
                        crate::annotate::LookEdit::NewStyle => new_style = true,
                        crate::annotate::LookEdit::None => {}
                    }
                    // what is worked out, after its fields (AMD-015)
                    crate::props::worked_out(ui, "dimension", &worked);
                    // its own number format over the style pack's (A5-2)
                    if d.kind != creabim_core::DimensionKind::Spot {
                        crate::props::sub_box(
                            ui,
                            "dim.format",
                            "Number format",
                            None,
                            false,
                            |ui| {
                                let mut own = d.format.is_some();
                                if crate::props::switch(ui, &mut own, "Own format")
                            .on_hover_text("This dimension writes its number its own way; off, it follows Dimensions in the style pack")
                            .changed()
                        {
                            d.format = own.then(Default::default);
                            label = "Dimension number format";
                        }
                                if let Some(f) = &mut d.format {
                                    let was = *f;
                                    let units = self.doc.model().units();
                                    levelsui::kind_format(
                                        ui,
                                        f,
                                        &units,
                                        creabim_core::NumberKind::Dimensions,
                                    );
                                    if *f != was {
                                        label = "Dimension number format";
                                    }
                                }
                                if crate::props::action(
                                    ui,
                                    "",
                                    "Number formats…",
                                    "The style pack's formats for every kind",
                                    true,
                                )
                                .clicked()
                                {
                                    self.levels.units_kind =
                                        Some(creabim_core::NumberKind::Dimensions);
                                    self.levels.units_open = true;
                                }
                            },
                        );
                    }
                }
                ElementKind::Tag(t) => {
                    let host = self
                        .doc
                        .model()
                        .get(t.host)
                        .map(|e| e.name.clone())
                        .unwrap_or_default();
                    ui.label(format!("Tags {host}"));
                    if crate::props::switch(ui, &mut t.material, "Reads the material")
                        .on_hover_text("A material tag: its material, or its layers one line each, outside to inside")
                        .changed()
                    {
                        label = "Tag reads";
                    }
                    if num(ui, "Offset X", &mut t.offset.x, -100.0, 100.0)
                        | num(ui, "Offset Y", &mut t.offset.y, -100.0, 100.0)
                    {
                        label = "Move tag"
                    }
                    // in the field column, its edges lined up with the
                    // fields above, as Select all is (MU142: it ran from
                    // the label edge to past the fields)
                    let mut pick = false;
                    crate::props::row(ui, "Tags", |ui| {
                        let w = ui.available_width().floor().max(24.0);
                        pick = ui
                            .add_sized([w, 22.0], egui::Button::new("Select it"))
                            .on_hover_text("Select the element this tag reads")
                            .clicked();
                    });
                    if pick {
                        self.selection.only(t.host);
                    }
                    if matches!(
                        self.doc.model().get(t.host).map(|e| &e.kind),
                        Some(ElementKind::Room(_))
                    ) {
                        let l = crate::roomtool::tag_fields(ui, &mut t.fields);
                        if !l.is_empty() {
                            label = l;
                        }
                        // moved out of its room, a tag points back to it;
                        // the owner may want it without (MU141)
                        let mut leader = !t.no_leader;
                        if crate::props::check(ui, "Leader", &mut leader, "Back to its room")
                            .on_hover_text("Moved out of its room, the tag draws a leader back to it. Off, it never does")
                            .changed()
                        {
                            t.no_leader = !leader;
                            label = "Room tag leader";
                        }
                    } else if let Some(ElementKind::Ceiling(c)) =
                        self.doc.model().get(t.host).map(|e| &e.kind)
                    {
                        // a ceiling's height tag: its words, then the height
                        // it reads, as a room tag's rows (MU142)
                        let height =
                            creabim_geom::rooms::ceiling_height_text(self.doc.model(), "", c);
                        let mut words = t.prefix().to_string();
                        crate::props::row(ui, "Shows", |ui| {
                            let w = (ui.available_width() * 0.4).floor().max(40.0);
                            if crate::props::text_field(
                                ui,
                                egui::TextEdit::singleline(&mut words),
                                w,
                            )
                            .on_hover_text(
                                "The words before the height; empty writes the height alone",
                            )
                            .changed()
                            {
                                t.prefix = (words != "CH").then(|| words.clone());
                                label = "Ceiling tag words";
                            }
                            ui.label(egui::RichText::new(height).weak()).on_hover_text(
                                "The ceiling's height above its floor: change it on the ceiling",
                            );
                        });
                        let mut leader = !t.no_leader;
                        if crate::props::check(ui, "Leader", &mut leader, "Back to its ceiling")
                            .on_hover_text("Moved out of its ceiling, the tag draws a leader back to it. Off, it never does")
                            .changed()
                        {
                            t.no_leader = !leader;
                            label = "Ceiling tag leader";
                        }
                    } else {
                        if let Some(l) = crate::annotate::leader_ui(
                            ui,
                            &mut t.leader_end,
                            "Style's (a plain line)",
                            &mut t.elbow,
                        ) {
                            label = l;
                        }
                        if t.end.is_some()
                            && ui
                                .button("Leader back to the centre")
                                .on_hover_text(
                                    "Its leader points at the middle of what it tags again",
                                )
                                .clicked()
                        {
                            t.end = None;
                            label = "Move leader";
                        }
                    }
                    match crate::annotate::look_ui(ui, self.doc.model(), &mut t.style, &mut t.color)
                    {
                        crate::annotate::LookEdit::Changed(l) => label = l,
                        crate::annotate::LookEdit::NewStyle => new_style = true,
                        crate::annotate::LookEdit::None => {}
                    }
                }
                ElementKind::Note(n) if n.symbol.is_some() => {
                    if let Some(l) = stylepages::placed_symbol_ui(ui, self.doc.model(), n) {
                        label = l;
                    }
                }
                ElementKind::Note(n) => {
                    // a leader's default reach: a metre at 1:100, 10 mm on a sheet
                    let unit = match self.view(n.view) {
                        Some(v) => v.scale / 100.0,
                        None => 10.0,
                    };
                    let mut pop = false;
                    let mut edit_in = None;
                    let picking = self.note_pick == Some(id);
                    let mut pick = picking;
                    if let Some(l) = self.note_shows_ui(ui, n, &mut edit_in, picking, &mut pick) {
                        label = l;
                    }
                    if pick != picking {
                        self.note_pick = pick.then_some(id);
                        self.status = if pick {
                            "Click the object the note reads, in any view. Esc stops picking."
                                .into()
                        } else {
                            "Stopped picking.".into()
                        };
                    }
                    if let Some(k) = edit_in {
                        self.open_notes(&k);
                    }
                    if let Some(l) = crate::annotate::note_ui(
                        ui,
                        n,
                        &mut self.plan.anno.focus_text,
                        unit,
                        &mut pop,
                    ) {
                        label = l;
                    }
                    if pop {
                        self.note_editor = Some(id);
                    }
                    if n.leader.is_some() {
                        if let Some(l) = crate::annotate::leader_ui(
                            ui,
                            &mut n.leader_end,
                            "Style's (an arrow)",
                            &mut n.elbow,
                        ) {
                            label = l;
                        }
                    }
                    match crate::annotate::look_ui(ui, self.doc.model(), &mut n.style, &mut n.color)
                    {
                        crate::annotate::LookEdit::Changed(l) => label = l,
                        crate::annotate::LookEdit::NewStyle => new_style = true,
                        crate::annotate::LookEdit::None => {}
                    }
                    // where it sits, typed (MU26): mm on a sheet, lengths
                    // in a view
                    let on_sheet = self.is_sheet(n.view);
                    for (name, v) in [("X", &mut n.at.x), ("Y", &mut n.at.y)] {
                        crate::props::row(ui, name, |ui| {
                            let r = if on_sheet {
                                crate::props::fill(ui, crate::units::paper(v))
                            } else {
                                crate::props::fill(ui, crate::units::drag(v))
                            };
                            if r.changed() {
                                label = "Place note";
                            }
                        });
                    }
                }
                ElementKind::Units(_) => {
                    if ui.button("Project units…").clicked() {
                        self.levels.units_open = true;
                    }
                }
                ElementKind::Project(_) => {
                    if crate::props::tab_link(
                        ui,
                        "Edited in the Project tab: name, site, building",
                        "Open the Project tab",
                    ) {
                        self.open_project_tab();
                    }
                }
                ElementKind::Reference(r) => {
                    if let Some(l) = self.reference_fields(ui, id, r) {
                        label = l;
                    }
                }
                ElementKind::PaperMark(m) => {
                    let north = self
                        .doc
                        .model()
                        .project_info()
                        .map_or(0.0, |(_, p)| p.true_north);
                    if let Some(l) = paper_mark_fields(ui, m, north, self.doc.model()) {
                        label = l;
                    }
                }
                ElementKind::MarkRules(_) => {
                    if ui.button("Mark numbering…").clicked() {
                        self.marks.open = true;
                    }
                }
                ElementKind::StylePack(p) => {
                    ui.label(egui::RichText::new(p.description.clone()).weak());
                    if ui.button("Graphic styles…").clicked() {
                        self.gstyles.open = true;
                    }
                }
                ElementKind::TextStyle(s) => {
                    if let Some(l) = crate::annotate::style_ui(ui, s) {
                        label = l;
                    }
                    let users = crate::annotate::style_users(self.doc.model(), id);
                    crate::props::worked_out(
                        ui,
                        "anno-style",
                        &[crate::props::Worked::new(
                            "Used by",
                            format!("{users} dimensions, tags and notes"),
                        )],
                    );
                    crate::props::flow(ui, |ui| {
                        if ui
                            .add_enabled(users > 0, egui::Button::new("Select all using it"))
                            .clicked()
                        {
                            self.select_style_users(id);
                        }
                        if ui.button("Annotation styles…").clicked() {
                            self.open_styles(Some(id));
                        }
                    });
                }
                ElementKind::SheetTemplate(_) => {
                    let users = sheets::sheets(self.doc.model())
                        .into_iter()
                        .filter(|(_, s)| s.template == Some(id))
                        .count();
                    ui.label(egui::RichText::new(format!("Used by {users} sheets.")).weak());
                    if ui.button("Edit the master").clicked() {
                        self.open_master(Some(id));
                    }
                }
                ElementKind::Grid(g) => {
                    // a row like every other (MU63: the label sat over it)
                    let prefix = creabim_core::grids::group_of(self.doc.model(), g)
                        .map(|(_, gg)| gg.prefix.clone())
                        .filter(|p| !p.is_empty());
                    crate::props::row(ui, "Label", |ui| {
                        if let Some(p) = &prefix {
                            ui.label(egui::RichText::new(p).weak());
                        }
                        if crate::props::text_field(
                            ui,
                            egui::TextEdit::singleline(&mut g.label),
                            f32::INFINITY,
                        )
                        .changed()
                        {
                            label = "Grid label"
                        }
                    });
                    let l = self.grid_properties(ui, id, g);
                    if !l.is_empty() {
                        label = l;
                    }
                }
                ElementKind::GridGroup(g) => {
                    label = self.grid_group_properties(ui, id, g, false);
                }
                ElementKind::Sketch(sk) => {
                    match crate::sketch::properties(ui, sk) {
                        crate::sketch::Edit::Done(l) => label = l,
                        crate::sketch::Edit::Refused(why) => self.status = why,
                        crate::sketch::Edit::None => {}
                    }
                    ui.add_space(4.0);
                    if num(ui, "Plane X", &mut sk.plane.origin.x, -1e4, 1e4)
                        | num(ui, "Plane Y", &mut sk.plane.origin.y, -1e4, 1e4)
                        | num(ui, "Plane Z", &mut sk.plane.origin.z, -1e3, 1e3)
                    {
                        label = "Move sketch"
                    }
                    let n = sk.plane.normal();
                    crate::props::row(ui, "Extrude by", |ui| {
                        crate::props::fill(ui, crate::units::drag(&mut self.extrude_distance))
                    });
                    // a closed outline that crosses itself has no inside
                    let crossed =
                        sk.closed && creabim_core::polygon::self_intersects(&sk.outline(), true);
                    let extrude = crate::props::action(
                        ui,
                        "",
                        if sk.closed {
                            "Extrude into a mass (E)"
                        } else {
                            "Extrude into a surface (E)"
                        },
                        "",
                        !crossed,
                    )
                    .clicked();
                    // what it is, read not typed (AMD-015); its plane's
                    // facing named when it is one of the axes
                    let faces = if n.z.abs() > 0.999 {
                        if n.z > 0.0 {
                            "Up".to_string()
                        } else {
                            "Down".to_string()
                        }
                    } else if n.x.abs() > 0.999 {
                        if n.x > 0.0 {
                            "East (+X)".to_string()
                        } else {
                            "West (−X)".to_string()
                        }
                    } else if n.y.abs() > 0.999 {
                        if n.y > 0.0 {
                            "North (+Y)".to_string()
                        } else {
                            "South (−Y)".to_string()
                        }
                    } else {
                        format!("{:.2}, {:.2}, {:.2}", n.x, n.y, n.z)
                    };
                    crate::props::worked_out(
                        ui,
                        "sketch",
                        &[
                            crate::props::Worked::new("Faces", faces),
                            crate::props::Worked::new(
                                "Shape",
                                if sk.closed {
                                    "Closed: extrudes to a solid"
                                } else {
                                    "Open: extrudes to a surface"
                                },
                            ),
                        ],
                    );
                    if crossed {
                        crate::props::warning(
                            ui,
                            "It crosses itself, so it isn't a face. Move a point so no lines cross.",
                            None,
                        );
                    }
                    if extrude {
                        extrude_sketch = true;
                    }
                }
                ElementKind::FreeMesh(m) => {
                    crate::app::helpui::combo(ui, "IFC class")
                        .selected_text(m.ifc_class.clone())
                        .show_ui(ui, |ui| {
                            for c in FREE_MESH_CLASSES {
                                if ui
                                    .selectable_value(&mut m.ifc_class, c.to_string(), *c)
                                    .changed()
                                {
                                    label = "Classify mesh"
                                }
                            }
                        });
                    // what it is for (MU43): part of the building, a void
                    // to cut with, or a guide to draw by
                    {
                        use creabim_core::shape::Role;
                        if crate::props::choice(
                            ui,
                            "Role",
                            &mut m.role,
                            &[
                                (Role::Solid, "Solid", "Part of the building: on the drawings, in schedules and exports"),
                                (Role::Void, "Void", "Something to cut with: shown see-through in 3D, left out of drawings and exports. It cuts only what it is joined to"),
                                (Role::Guide, "Guide", "Something to draw by: shown and snapped to in 3D, left out of drawings and exports"),
                            ],
                        ) {
                            label = "Shape role"
                        }
                        let pieces = m.mesh.pieces().len();
                        let many = self.selection.len() > 1
                            && self.selection.all().iter().all(|i| {
                                matches!(
                                    self.doc.model().get(*i).map(|e| &e.kind),
                                    Some(ElementKind::FreeMesh(_))
                                )
                            });
                        let (sep, join) = crate::props::actions2(
                            ui,
                            &match pieces {
                                1 => "1 piece".to_string(),
                                n => format!("{n} pieces"),
                            },
                            (
                                "Separate",
                                "Each piece becomes a shape of its own",
                                pieces > 1,
                            ),
                            (
                                "Join",
                                "The selected shapes become one, each a piece of it. Select two or more shapes",
                                many,
                            ),
                        );
                        separate_shape = sep;
                        join_shapes = join;
                    }
                    // Transform, as Blender's (MU24): location, rotation
                    // about each axis (XYZ Euler), scale
                    crate::props::sub_box(ui, "mesh.location", "Location", None, true, |ui| {
                        if num(ui, "X", &mut m.origin.x, -1e4, 1e4)
                            | num(ui, "Y", &mut m.origin.y, -1e4, 1e4)
                            | num(ui, "Z", &mut m.origin.z, -1e3, 1e3)
                        {
                            label = "Move mesh"
                        }
                    });
                    crate::props::sub_box(
                        ui,
                        "mesh.rotation",
                        "Rotation (XYZ Euler)",
                        None,
                        true,
                        |ui| {
                            let deg = |ui: &mut egui::Ui, name: &str, v: &mut f64| {
                                crate::props::row(ui, name, |ui| {
                                    crate::props::fill(ui, egui::DragValue::new(v).speed(1.0).suffix("°"))
                                    .on_hover_text("Turns about this axis through its origin. R in a view turns it with the pointer; X, Y or Z then keeps to an axis.")
                                    .changed()
                                })
                            };
                            let (mut x, mut y, mut z) = (m.rot_x, m.rot_y, m.rotation);
                            if deg(ui, "X", &mut x) | deg(ui, "Y", &mut y) | deg(ui, "Z", &mut z) {
                                m.rot_x = x.rem_euclid(360.0);
                                m.rot_y = y.rem_euclid(360.0);
                                m.rotation = z.rem_euclid(360.0);
                                label = "Rotate mesh"
                            }
                        },
                    );
                    crate::props::row(ui, "Scale", |ui| {
                        if crate::props::fill(
                            ui,
                            egui::DragValue::new(&mut m.scale)
                                .speed(0.01)
                                .range(0.01..=100.0),
                        )
                        .on_hover_text("M moves by a base point. With the pointer over a view: G moves, R rotates, S scales, Shift+D duplicates.")
                        .changed()
                        {
                            label = "Scale mesh"
                        }
                    });

                    crate::props::worked_out(
                        ui,
                        "mesh",
                        &[
                            crate::props::Worked::new(
                                "Vertices",
                                m.mesh.vertices.len().to_string(),
                            ),
                            crate::props::Worked::new("Faces", m.mesh.faces.len().to_string()),
                            crate::props::Worked::new(
                                "Modifiers",
                                format!("{} (the wrench tab)", m.modifiers.len()),
                            ),
                        ],
                    );
                }
                ElementKind::Cut(c) => {
                    let name = |id| {
                        self.doc
                            .model()
                            .get(id)
                            .map_or("(missing)".to_string(), |e| e.name.clone())
                    };
                    ui.label(format!(
                        "{} {} with {}",
                        c.op.label(),
                        name(c.target),
                        name(c.cutter)
                    ));
                    for op in BoolOp::ALL {
                        if crate::props::radio_tick(ui, &mut c.op, op, op.label()).changed() {
                            label = "Change boolean";
                        }
                    }
                }
                ElementKind::Room(r) => {
                    label = crate::roomtool::room_fields(ui, self.doc.model(), r);
                }
                ElementKind::Ceiling(c) => {
                    label = crate::roomtool::ceiling_fields(ui, self.doc.model(), c);
                }
                ElementKind::Join(j) => {
                    label = crate::detailtool::join_fields(ui, self.doc.model(), j);
                }
                ElementKind::Detail(d) => {
                    label = crate::detailtool::detail_fields(ui, d);
                }
                ElementKind::RoomSeparator(sep) => {
                    crate::props::worked_out(
                        ui,
                        "room-separator",
                        &[crate::props::Worked::new(
                            "Length",
                            crate::units::len(sep.a.distance(sep.b)),
                        )],
                    );
                    crate::props::info(ui, "Rooms on either side of it are measured apart.", "");
                }
                ElementKind::Object(o) => {
                    let hung = creabim_core::objects::hung(self.doc.model(), &e);
                    // its footprint's middle, in its own axes: what it turns about
                    let mid = creabim_core::objects::type_of(self.doc.model(), &e)
                        .map(|t| {
                            let (lo, hi) = t.bounds();
                            (lo.truncate() + hi.truncate()) / 2.0
                        })
                        .unwrap_or_default();
                    label = objectsui::object_fields(ui, o, hung, mid);
                    object_type = true;
                }
                k @ (ElementKind::Column(_)
                | ElementKind::Beam(_)
                | ElementKind::Roof(_)
                | ElementKind::Stair(_)
                | ElementKind::Railing(_)
                | ElementKind::Footing(_)
                | ElementKind::Part(_)
                | ElementKind::Site(_)) => {
                    let l = crate::memberstool::properties(ui, self.doc.model(), k);
                    if !l.is_empty() {
                        label = l;
                    }
                    let shafts = creabim_core::slab::holes_of(self.doc.model(), &e).len();
                    if let ElementKind::Roof(r) = k {
                        let shafts = shafts.saturating_sub(r.holes.len());
                        let (l, draw) = typesui::holes_fields(ui, &mut r.holes, shafts);
                        if !l.is_empty() {
                            label = l;
                        }
                        draw_hole |= draw;
                        // its roof windows, and a button to add one
                        let n = creabim_core::roofwindow::windows_in(self.doc.model(), id).len();
                        let add = crate::props::row(ui, "Windows", |ui| {
                            crate::props::flow(ui, |ui| {
                                ui.label(n.to_string());
                                let w = ui.available_width();
                                ui.add_sized([w, 22.0], egui::Button::new("Add"))
                                    .on_hover_text("Add a roof window in the middle of its largest slope, then move it (M) or set its centre.")
                                    .clicked()
                            })
                            .inner
                        });
                        if add {
                            if let Some(o) = creabim_core::roofwindow::new_on(self.doc.model(), id)
                            {
                                let w = Element::new("Roof window", ElementKind::Opening(o));
                                let wid = w.id;
                                let tx = Transaction::new("Add roof window").insert(w);
                                match self.doc.commit(tx) {
                                    Ok(_) => {
                                        self.selection.set(Some(wid));
                                        self.status =
                                            "Added a roof window: move it (M) or set its centre."
                                                .into();
                                    }
                                    Err(e) => self.status = format!("Couldn't do that: {e}"),
                                }
                            }
                        }
                    }
                    // the Element tab points to the plant's own tab (MU52)
                    if crate::partstool::is_plant(k)
                        && crate::props::tab_link(
                            ui,
                            "Grown in the Tree tab: form, crown, branches, leaves, plan symbol",
                            "Open the Tree tab: how this plant is made",
                        )
                    {
                        self.prop_tab = PropTab::Tree;
                    }
                    if let ElementKind::Stair(_) = k {
                        // its well in the floor above, tied to it
                        let well = self.doc.model().iter().find_map(|w| match &w.kind {
                            ElementKind::Shaft(sh) if sh.stair == Some(id) => {
                                Some((w.clone(), sh.clearance))
                            }
                            _ => None,
                        });
                        match well {
                            None => {
                                if crate::props::action(
                                    ui,
                                    "Well",
                                    "Cut a well",
                                    "An opening in the floor above, from where the floor would come lower than the headroom to the top. It follows the stair as it changes.",
                                    true,
                                )
                                .clicked()
                                {
                                    match creabim_core::stairwell::cut_well(self.doc.model(), id, 0.0) {
                                        Ok(tx) => {
                                            let r = self.doc.commit(tx);
                                            self.report(r, "Cut a well over the stair, sized from its headroom. Ctrl+Z undoes it.");
                                        }
                                        Err(e) => self.status = format!("No well cut: {e}."),
                                    }
                                }
                            }
                            Some((w, clearance)) => {
                                let mut c = clearance;
                                if crate::partstool::num(ui, "Clearance", &mut c, 0.0, 3.0) {
                                    let mut after = w.clone();
                                    if let ElementKind::Shaft(sh) = &mut after.kind {
                                        sh.clearance = c;
                                    }
                                    let tx = creabim_core::Transaction::new("Well clearance")
                                        .replace(w, after);
                                    if let Err(e) = self.doc.commit_merged(tx) {
                                        self.status = format!("Couldn't do that: {e}");
                                    }
                                }
                                crate::props::info(
                                    ui,
                                    "Its well follows it: from where the floor above would come lower than the headroom, plus the clearance, to the top.",
                                    "stairs-railings",
                                );
                            }
                        }
                        if crate::props::action(
                            ui,
                            "Section",
                            "Walk-line section",
                            "The stair unrolled along its walk line: its steps, the floor over it and its headroom line, red where the head comes too close. It follows the stair as it changes.",
                            true,
                        )
                        .clicked()
                        {
                            self.open_walk_section(id);
                        }
                        if crate::props::action(
                            ui,
                            "Sheet",
                            "Stair sheet",
                            "A new sheet for this stair: the plans of the floor it starts on and the one it reaches at 1:50, its walk-line section, a 3D view cut out round it and the stair schedule. All live views.",
                            true,
                        )
                        .clicked()
                        {
                            self.new_stair_sheet(id);
                        }
                        self.stair_storeys_ui(ui, id);
                    }
                    // a railing up each side, each on or off by itself
                    if matches!(k, ElementKind::Stair(_)) {
                        for (left, text) in [(true, "Left"), (false, "Right")] {
                            let had =
                                crate::memberstool::stair_side_railing(self.doc.model(), id, left);
                            let mut on = had.is_some();
                            if crate::props::check(ui, text, &mut on, "Railing")
                                .on_hover_text(
                                    "A wall handrail where a wall runs beside this side, else a \
                                     guard. It follows the stair; select it to change it.",
                                )
                                .changed()
                            {
                                let r = crate::memberstool::set_stair_railing(
                                    &mut self.doc,
                                    id,
                                    left,
                                    on,
                                );
                                self.report(
                                    r,
                                    if on {
                                        "Railing added along the stair."
                                    } else {
                                        "Railing taken off the stair."
                                    },
                                );
                            }
                        }
                    }
                    // a stair's sub-folds last in the block (AMD-003): its
                    // finishes, curves, flights and landings, then its rule
                    // set and checks, live as it changes
                    if let ElementKind::Stair(s) = k {
                        let l = crate::memberstool::stair_folds(ui, self.doc.model(), s);
                        if !l.is_empty() {
                            label = l;
                        }
                        let (l, own) =
                            crate::memberstool::stair_rules_ui(ui, self.doc.model(), id, s);
                        if let Some(all) = own {
                            let tx = creabim_core::stairrules::set_own_rules(
                                self.doc.model(),
                                all,
                                "Stair rule set",
                            );
                            if let Err(e) = self.doc.commit_merged(tx) {
                                self.status = format!("Couldn't do that: {e}");
                            }
                        }
                        if !l.is_empty() {
                            label = l;
                        }
                    }
                    // the member's Worked out plate and ? box, last in the
                    // block, after its holes, material and railings
                    crate::props::flush_later(ui);
                    // a site element with a tab of its own points to it, at
                    // the very foot of Element (AMD-018)
                    if let Some(own) = crate::partstool::site_own(k) {
                        if crate::props::tab_link(ui, own.link(), "Open its own tab") {
                            self.prop_tab = Self::site_prop_tab(own);
                        }
                    }
                }
                ElementKind::Guide(g) => {
                    crate::props::info(
                        ui,
                        "A construction line to snap to. It is never printed or exported.",
                        "",
                    );
                    if num(ui, "From X", &mut g.a.x, -1e4, 1e4)
                        | num(ui, "From Y", &mut g.a.y, -1e4, 1e4)
                        | num(ui, "To X", &mut g.b.x, -1e4, 1e4)
                        | num(ui, "To Y", &mut g.b.y, -1e4, 1e4)
                    {
                        label = "Move guide";
                    }
                }
            }
        });
        // a window's ribbon or corner, after its Geometry (part 9)
        if !self.shell.classic {
            self.arrangement_section(ui, id);
        }
        if let Some(v) = title_of {
            self.viewport_title(ui, v);
        }
        // a sheet note's flow across blocks: a fold of its own, not part of
        // the Note fold (MU90)
        let flows = matches!(&edited.kind, ElementKind::Note(n) if n.symbol.is_none() && self.is_sheet(n.view));
        if flows {
            let open =
                self.prop_header(ui, "note_flow", "Text flow", true, Some("notes#long-notes"));
            let mut long = None;
            let mut got = None;
            crate::props::fold(ui, open, |ui| {
                if let ElementKind::Note(n) = &mut edited.kind {
                    got = self.note_long_ui(ui, id, n, &mut long);
                }
            });
            if let Some(l) = got {
                label = l;
            }
            if long.is_some() {
                self.notes_long = long;
            }
        }
        // a sheet mark's copies: a fold of its own (Mockup_Copy_To_Sheets_REV00)
        // drawn every frame (owner: it flickered while another field
        // changed, when it was skipped); its actions never touch this mark,
        // only its copies, so they don't clash with this frame's edit
        if let ElementKind::PaperMark(m) = &edited.kind {
            let m = m.clone();
            self.mark_placed_fold(ui, id, &m);
        }
        if new_style {
            match crate::annotate::new_text_style(&mut self.doc) {
                Ok(sid) => {
                    crate::annotate::set_style(&mut edited.kind, sid);
                    label = "Text style";
                }
                Err(e) => self.status = format!("Couldn't do that: {e}"),
            }
        }
        if !label.is_empty() {
            // a layered element is always as thick as its layers
            let slot = creabim_core::assembly::assembly_slot(&edited).flatten();
            if slot.is_some() {
                creabim_core::assembly::set_assembly(self.doc.model(), &mut edited, slot);
            }
            // with several selected, the change goes to each of the same kind
            let mut others = vec![];
            for o in self.selection.all() {
                let Some(other) = self.doc.model().get(o) else {
                    continue;
                };
                if let Some(mut out) = crate::multiedit::apply_to(&e, &edited, other) {
                    if let (ElementKind::Opening(was), ElementKind::Opening(o)) =
                        (&other.kind, &mut out.kind)
                    {
                        creabim_core::walls::widen_in_place(self.doc.model(), was.width, o);
                    }
                    let slot = creabim_core::assembly::assembly_slot(&out).flatten();
                    if slot.is_some() {
                        creabim_core::assembly::set_assembly(self.doc.model(), &mut out, slot);
                    }
                    others.push((other.clone(), out));
                }
            }
            let mut msg = if others.is_empty() {
                format!("{label}.")
            } else {
                format!("{label}: {} elements.", others.len() + 1)
            };
            // a layer the new type hasn't got loses its edited ends
            let edits = |k: &ElementKind| match k {
                ElementKind::Wall(w) => w.layer_ends.len(),
                ElementKind::Slab(s) => s.layer_edges.len(),
                ElementKind::Roof(r) => r.layer_edges.len(),
                _ => 0,
            };
            let lost = edits(&e.kind).saturating_sub(edits(&edited.kind));
            if lost > 0 {
                let what = match (&edited.kind, lost) {
                    (ElementKind::Wall(_), 1) => "edited layer end",
                    (ElementKind::Wall(_), _) => "edited layer ends",
                    (_, 1) => "edited layer edge",
                    _ => "edited layer edges",
                };
                msg = format!("{msg} {lost} {what} dropped: the new type has no such layer.");
            }
            if let ElementKind::Opening(o) = &edited.kind {
                let model = self.doc.model();
                if let Some(w) = creabim_core::walls::opening_crosses_join(model, id, o) {
                    let name = model.get(w).map_or("another wall", |e| e.name.as_str());
                    msg = format!(
                        "{msg} It now runs into {name}, where the walls join: move it along or make it narrower."
                    );
                }
            }
            let r = if others.is_empty() {
                self.doc.edit_merged(id, label, |e| *e = edited.clone())
            } else {
                let mut tx = creabim_core::Transaction::new(label);
                if let Some(cur) = self.doc.model().get(id).cloned() {
                    if cur != edited {
                        tx = tx.replace(cur, edited.clone());
                    }
                }
                for (b, a) in others {
                    tx = tx.replace(b, a);
                }
                self.doc.commit_merged(tx)
            };
            self.report(r, msg);
        }
        if object_type {
            self.object_type_section(ui, id);
        }
        if let Some((view, scale)) = rescale {
            self.set_view_scale(view, scale);
        }
        if new_style {
            self.status = "New text style: set its font, size and colour below.".into();
        }
        if let Some(msg) = crate::annotate::style_inline(ui, &mut self.doc, &edited.kind, new_style)
        {
            self.status = msg;
        }
        // a dimension's text in a card of its own, folded until wanted
        // (MU82: not a box inside Settings)
        self.dim_text_section(ui, id);
        // a mass's face tools in a section of their own (MU39: the element
        // section had grown dense)
        self.mesh_edit_section(ui, id);
        // a sweep's profile from the library, under its settings (MU40:
        // settings always first)
        let own_profile = self.sweep_profile_rows(ui, id);
        // what can be made from it, in a section of its own (MU40: it hung
        // glued under Settings): a profile of its shape (a sweep's Profile
        // section has Save as new for that), even parts of its length
        let make =
            !own_profile && creabim_core::profiles::from_shape(self.doc.model(), id).is_some();
        let divide = creabim_core::divide::span(self.doc.model(), id).is_some();
        if make || divide {
            let open = self.prop_header(ui, "make", "Make from it", true, None);
            crate::props::fold(ui, open, |ui| {
                if make
                    && crate::props::action(ui, "Profile", "Make a profile", "Its shape (a sketch on any plane, open or closed; a slab; an extrusion) kept in the project's profile library, to sweep or place", true).clicked()
                {
                    self.profile_from_selection();
                }
                // even parts of its length (MU34 B2)
                if divide
                    && crate::props::action(
                        ui,
                        "Divide",
                        "Into even parts…",
                        "Cut it into parts, put guides at the divisions, or copy something into each part",
                        true,
                    )
                    .clicked()
                {
                    self.open_divide();
                }
            });
        }
        if extrude_sketch {
            self.extrude_sketch();
            return;
        }
        if separate_shape {
            self.separate_shape(id);
            return;
        }
        if join_shapes {
            self.join_shapes(id);
            return;
        }
        let _ = show_visibility;
        if redraw {
            self.plan.cancel();
            self.plan.tool = Tool::Select;
            self.plan.outline = crate::outline::Outline::start(self.doc.model(), id);
            if let Some(o) = &self.plan.outline {
                self.status = o.hint();
            }
        }
        if draw_hole {
            self.plan.cancel();
            self.plan.tool = Tool::Select;
            self.plan.outline = crate::outline::Outline::hole(self.doc.model(), id);
            if let Some(o) = &self.plan.outline {
                self.status = o.hint();
            }
        }
    }

    /// The top of Properties, pinned while its tabs scroll (MU19/MU20):
    /// what the selected element is, its name and mark, and its actions.
    fn properties_header(&mut self, ui: &mut egui::Ui) {
        // a master, a profile or the log shows its own title, not the
        // element picked before (#42)
        let picked = self
            .selection
            .active
            .filter(|_| !self.prop_tab.of_its_view());
        let Some(id) = picked else {
            // the open tab's title line, as Blender's context line
            self.tab_title_line(ui);
            ui.add_space(2.0);
            return;
        };
        let Some(e) = self.doc.model().get(id).cloned() else {
            return;
        };
        let mut multi: Option<String> = None;
        if self.selection.len() > 1 {
            // how many of each kind, then the active element below
            let mut kinds: Vec<(&str, usize)> = Vec::new();
            for e in self
                .selection
                .all()
                .iter()
                .filter_map(|i| self.doc.model().get(*i))
            {
                let k = kind_word(&e.kind);
                match kinds.iter_mut().find(|(l, _)| *l == k) {
                    Some((_, n)) => *n += 1,
                    None => kinds.push((k, 1)),
                }
            }
            let list: Vec<String> = kinds
                .iter()
                .map(|(k, n)| format!("{n} {}", plural(k, *n)))
                .collect();
            // one quiet line under the header rows, its help on hover
            // (MU36: a block above the name)
            multi = Some(format!(
                "{} selected: {}",
                self.selection.len(),
                list.join(", ")
            ));
        }
        // an object is what its type says: a basin an IfcSanitaryTerminal
        let class = creabim_core::objects::type_of(self.doc.model(), &e)
            .map_or_else(|| e.ifc_class().to_string(), |t| t.ifc.class.clone());
        // labelled rows, as every field (MU22): its name, its IFC class and
        // the guide's page on it, its mark; then its actions
        let pal = crate::theme::Palette::of(ui.ctx());
        if matches!(e.kind, ElementKind::Level(_)) {
            // a level is renamed with the plans named after it, once typed
            self.level_name_field(ui, id, &e.name);
        } else {
            let mut name = e.name.clone();
            if crate::props::head_row(ui, "Name", |ui| {
                crate::props::fill(
                    ui,
                    egui::TextEdit::singleline(&mut name).vertical_align(egui::Align::Center),
                )
                .changed()
            }) {
                let r = self
                    .doc
                    .edit_merged(id, "Rename", |e| e.name = name.clone());
                self.report(r, "Renamed.");
            }
        }
        crate::props::head_row(ui, "IFC class", |ui| {
            // the ? at the row's end, the class cut to what is left (MU36:
            // the ? fell to the next line)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let page = crate::help::element_page(&e.kind);
                let title = crate::help::text(page.split('#').next().unwrap_or(page))
                    .map(crate::help::title)
                    .unwrap_or("This element");
                self.quick_help_button(ui, page, title);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(class.clone()).color(pal.accent))
                            .truncate(),
                    )
                    .on_hover_text(class.clone());
                });
            })
        });
        if let Some(m) = multi.take() {
            ui.add(egui::Label::new(egui::RichText::new(m).weak().small()).truncate())
                .on_hover_text("G, R, S, Shift+D, Delete, Ctrl+C and H act on all of them. Below is the last one picked; Shift+click picks another.");
        }
        if marksui::markable(&e.kind) {
            crate::props::head_row(ui, "Mark", |ui| {
                let w = ui.available_width();
                self.mark_inline(ui, id, w);
            });
        }
        {
            let _p = Perf::new("properties: mark warning");
            self.mark_warning(ui, id);
        }
        ui.add_space(2.0);
        // its actions, under its name (brief 02, 2.7)
        {
            let _p = Perf::new("properties: action bar");
            self.action_bar(ui);
        }
        ui.add_space(2.0);
        let r = ui.available_rect_before_wrap();
        ui.painter()
            .hline(r.x_range(), r.top(), egui::Stroke::new(1.0, pal.border));
        ui.add_space(3.0);
        // which of its tabs is open, and what it is about (MU52)
        self.tab_title_line(ui);
        ui.add_space(2.0);
        // picked, and a view open beside it does not show it: where, why,
        // and a button that shows it (owner markup MU125: the court steps
        // picked in 3D were nowhere on the site plan, and nothing said so)
        if self.prop_tab == PropTab::Element {
            for (view, why) in self.hidden_in_open_views(id) {
                let site_part = self.doc.model().get(id).is_some_and(|e| e.is_site_part());
                let (text, button) = self.hidden_words(view, why, false, site_part);
                if crate::props::warning(ui, text, button.as_deref()) {
                    self.show_in_view(view, id, why);
                }
            }
        }
    }

    /// The views open on screen that leave `id` out, each with why: the
    /// drawings only, not sheets or schedules, each view once.
    pub(crate) fn hidden_in_open_views(
        &self,
        id: ElementId,
    ) -> Vec<(ElementId, creabim_core::WhyHidden)> {
        let mut out: Vec<(ElementId, creabim_core::WhyHidden)> = vec![];
        for s in self.areas.root.areas() {
            let crate::areas::Shows::View(v) = s else {
                continue;
            };
            if out.iter().any(|(w, _)| *w == v) || self.is_sheet(v) {
                continue;
            }
            let Some(view) = self.view(v) else { continue };
            if matches!(view.kind, ViewKind::Schedule { .. }) {
                continue;
            }
            if let Some(why) = view.hidden.why(self.doc.model(), id) {
                out.push((v, why));
            }
        }
        out
    }

    /// What to say of `why` an element is not in `view`, and the button
    /// that shows it there (none when what it hangs on is the hidden one).
    /// `here` words it for the view itself, which needs no name.
    pub(crate) fn hidden_words(
        &self,
        view: ElementId,
        why: creabim_core::WhyHidden,
        here: bool,
        site_part: bool,
    ) -> (String, Option<String>) {
        use creabim_core::WhyHidden;
        let model = self.doc.model();
        let name = |id: ElementId| model.get(id).map_or(String::new(), |e| e.name.clone());
        let place = if here {
            "here".to_string()
        } else {
            "there".to_string()
        };
        let lead = if here {
            String::new()
        } else {
            format!("Hidden in {}", name(view))
        };
        match why {
            WhyHidden::ByHand => (
                if here {
                    "hidden here by hand".to_string()
                } else {
                    format!("{lead} by hand.")
                },
                Some(if here {
                    "Show".to_string()
                } else {
                    "Show it there".to_string()
                }),
            ),
            WhyHidden::Category(creabim_core::Category::Site) if site_part => (
                if here {
                    "it is part of the site, and the site is off here".to_string()
                } else {
                    format!("{lead}: it is part of the site, and the site is off {place}.")
                },
                Some(if here {
                    "Show".to_string()
                } else {
                    "Show the site there".to_string()
                }),
            ),
            WhyHidden::Category(c) => {
                let cat = c.label();
                (
                    if here {
                        format!("{} are off here", cat.to_lowercase())
                    } else {
                        format!("{lead}: {cat} are off {place}.")
                    },
                    // the line above names the category; the button only
                    // acts, in a few words that never wrap (MU136: "Show
                    // terrain and hard landscape there" broke onto two lines)
                    Some(if here {
                        "Show".to_string()
                    } else {
                        "Show it there".to_string()
                    }),
                )
            }
            WhyHidden::Host(h) => (
                if here {
                    format!("{} is hidden here", name(h))
                } else {
                    format!("{lead} with {}, which is hidden there.", name(h))
                },
                None,
            ),
        }
    }

    /// Undo what keeps `id` out of `view`: itself shown again, or its
    /// category switched back on there.
    pub(crate) fn show_in_view(
        &mut self,
        view: ElementId,
        id: ElementId,
        why: creabim_core::WhyHidden,
    ) {
        use creabim_core::WhyHidden;
        let t = gridvis::VisTarget::View(view);
        match why {
            WhyHidden::ByHand => {
                self.set_view_hidden(t, "Show an element", |h| h.set_element(id, false))
            }
            WhyHidden::Category(c) => {
                let label = format!("Show {}", c.label().to_lowercase());
                self.set_view_hidden(t, &label, |h| h.set_category(c, false));
            }
            WhyHidden::Host(_) => {}
        }
    }

    /// In a view that leaves the picked element out: a quiet chip at its
    /// lower left saying so, with Show at its end.
    /// The lines `view` would draw `id` with if it showed it: the view
    /// drawn with nothing hidden, kept per pick and revision.
    pub(crate) fn hidden_outline_of(
        &mut self,
        view: ElementId,
        id: ElementId,
    ) -> Vec<(DVec2, DVec2)> {
        let rev = self.doc.model().drawing_revision_of(view);
        if let Some((v, e, r, lines)) = &self.hidden_outline {
            if (*v, *e, *r) == (view, id, rev) {
                return lines.clone();
            }
        }
        let lines = hidden_lines(self.doc.model(), view, id);
        self.hidden_outline = Some((view, id, rev, lines.clone()));
        lines
    }

    pub(crate) fn hidden_here_chip(&mut self, ui: &mut egui::Ui, body: Rect, view: ElementId) {
        let Some(id) = self.selection.active else {
            return;
        };
        if self.is_sheet(view) {
            return;
        }
        let Some(why) = self
            .view(view)
            .filter(|v| !matches!(v.kind, ViewKind::Schedule { .. }))
            .and_then(|v| v.hidden.why(self.doc.model(), id))
        else {
            return;
        };
        // where it is, dashed: the view drawn with it shown, its lines only
        let lines = self.hidden_outline_of(view, id);
        let cam = self
            .plan
            .area
            .and_then(|a| self.plan.area_cameras.get(&(a, view)))
            .or_else(|| self.plan.cameras.get(&view))
            .copied();
        if let Some((center, scale)) = cam {
            let xf = planview::Xf {
                rect: body,
                center,
                scale,
            };
            let accent = planview::accent_selected();
            let p = ui.painter().with_clip_rect(body);
            for (a, b) in &lines {
                p.extend(egui::Shape::dashed_line(
                    &[xf.to_screen(*a), xf.to_screen(*b)],
                    egui::Stroke::new(1.4, accent),
                    6.0,
                    4.0,
                ));
            }
        }
        let name = self
            .doc
            .model()
            .get(id)
            .map_or(String::new(), |e| e.name.clone());
        let site_part = self.doc.model().get(id).is_some_and(|e| e.is_site_part());
        let (words, button) = self.hidden_words(view, why, true, site_part);
        let text = format!("{name}: {words}");
        let pal = crate::theme::Palette::of(ui.ctx());
        let v = ui.visuals().clone();
        let (h, pad, gap) = (24.0, 9.0, 8.0);
        let font = egui::FontId::proportional(11.5);
        // the button's width first, so the words take what is left
        let show = button.as_ref().map(|b| {
            ui.painter()
                .layout_no_wrap(b.clone(), font.clone(), pal.accent)
        });
        let show_w = show.as_ref().map_or(0.0, |g| g.size().x + 18.0);
        let room =
            (body.width() - 16.0 - pad - show_w - if show.is_some() { gap + 3.0 } else { pad })
                .max(40.0);
        let mut job = egui::text::LayoutJob::simple_singleline(text, font, v.text_color());
        job.wrap = egui::text::TextWrapping::truncate_at_width(room);
        let words = ui.painter().layout_job(job);
        let w = pad
            + words.size().x
            + if show.is_some() {
                gap + show_w + 3.0
            } else {
                pad
            };
        let r = Rect::from_min_size(
            egui::pos2(body.left() + 8.0, body.bottom() - 8.0 - h),
            Vec2::new(w, h),
        );
        let p = ui.painter().with_clip_rect(body);
        p.rect_filled(r, h / 2.0, v.panel_fill);
        p.rect_stroke(
            r,
            h / 2.0,
            egui::Stroke::new(1.0, pal.border),
            egui::StrokeKind::Inside,
        );
        p.galley(
            egui::pos2(r.left() + pad, r.center().y - words.size().y / 2.0),
            words,
            v.text_color(),
        );
        if let Some(g) = show {
            let b = Rect::from_min_max(
                egui::pos2(r.right() - 3.0 - show_w, r.top() + 3.0),
                egui::pos2(r.right() - 3.0, r.bottom() - 3.0),
            );
            let resp = ui
                .interact(b, ui.id().with(("hidden-here", view)), egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let fill = pal
                .accent
                .gamma_multiply(if resp.hovered() { 0.30 } else { 0.18 });
            p.rect_filled(b, b.height() / 2.0, fill);
            p.galley(
                egui::pos2(
                    b.center().x - g.size().x / 2.0,
                    b.center().y - g.size().y / 2.0,
                ),
                g,
                pal.accent,
            );
            if resp.clicked() {
                self.show_in_view(view, id, why);
            }
        }
    }

    /// The View tab for view `v`: what it hides, then how it's drawn (the
    /// same whether the view is picked or only shown; View tabs REV00).
    fn view_tab_body(&mut self, ui: &mut egui::Ui, v: ElementId) {
        let name = self.doc.model().get(v).map_or("", |e| e.name.as_str());
        let name = name.to_string();
        self.visibility_panel(
            ui,
            gridvis::VisTarget::View(v),
            Some(&format!("View: {name}")),
        );
        self.site_view_panel(ui, v);
        self.view_time(ui, phasesui::PhaseOf::View(v));
        // how it's shown, first (View tabs REV00)
        self.view_main_section(ui, v);
        // a plan's graphics: weights, below-cut tint, view depth
        let is_plan = matches!(
            self.doc.model().get(v).map(|e| &e.kind),
            Some(ElementKind::View(x)) if matches!(x.kind, ViewKind::Plan { .. })
        );
        let plan_open = is_plan
            && self.prop_header(
                ui,
                "view.graphics",
                "Graphics",
                true,
                Some("graphic-styles"),
            );
        if let Some(ElementKind::View(view)) = self.doc.model().get(v).map(|e| &e.kind) {
            if matches!(view.kind, ViewKind::Plan { .. }) {
                let (mut g, scale) = (view.graphics.clone(), view.scale);
                let footings_hidden = view
                    .hidden
                    .categories
                    .contains(&creabim_core::visibility::Category::Footings);
                // its body folds smoothly (AMD-017)
                if let Some(label) = crate::props::fold(ui, plan_open, |ui| {
                    plan_graphics_ui(ui, &mut g, scale, false)
                })
                .flatten()
                {
                    let r = self.doc.edit_merged(v, label, |e| {
                        if let ElementKind::View(x) = &mut e.kind {
                            x.graphics = g.clone();
                        }
                    });
                    self.report(r, "View changed.");
                }
                // a view depth can't show what the view hides (A7-3)
                if footings_hidden && g.range.view_depth.is_some() && footings_note(ui) {
                    let r = self.doc.edit(v, "Show footings", |e| {
                        if let ElementKind::View(x) = &mut e.kind {
                            x.hidden
                                .categories
                                .retain(|c| *c != creabim_core::visibility::Category::Footings);
                        }
                    });
                    self.report(r, "Footings show in this view, dashed under the floor.");
                }
            }
        }
        // its range, template and overrides
        self.view_graphics_sections(ui, v);
    }

    /// The open tab's title line (the Properties rule, MU52): its name and
    /// what it is about; the View tab names the view it describes.
    fn tab_title_line(&mut self, ui: &mut egui::Ui) {
        let tab = self.prop_tab;
        let view_name = || {
            match self.vis_target(false) {
                Some(gridvis::VisTarget::View(v)) => {
                    self.doc.model().get(v).map(|e| e.name.clone())
                }
                _ => None,
            }
            .filter(|n| !n.trim().is_empty())
        };
        let about = match tab {
            PropTab::View if self.model_in_front() => "3D view".to_string(),
            PropTab::View => view_name().unwrap_or_else(|| tab.about().to_string()),
            _ => tab.about().to_string(),
        };
        crate::props::title_line_room(
            ui,
            tab.icon(),
            tab.label(),
            &about,
            self.tab_hue(ui, tab),
            0.0,
        );
    }

    /// Whether the area last clicked shows the 3D model: the View tab is
    /// then about the 3D view (REV04 §4). Focus moves on a press, never
    /// on hover (areasui), so crossing the 3D view changes nothing.
    pub(crate) fn model_in_front(&self) -> bool {
        matches!(
            self.areas.root.areas().get(self.areas.focus),
            Some(crate::areas::Shows::Model)
        )
    }

    /// The 3D view's own settings, in the View tab when the 3D area was
    /// clicked last: what it hides, then its crop boxes.
    fn view_tab_3d(&mut self, ui: &mut egui::Ui) {
        self.visibility_panel(ui, gridvis::VisTarget::Window3d, Some("3D view"));
        self.view_time(ui, phasesui::PhaseOf::Window3d);
        self.crop_list(ui, self.crop_owner());
    }

    /// The Element tab's Level row (REV04 §1): the level the element hangs
    /// off, for every kind that has one, at the top of the tab, in one
    /// place (the wall's, column's and footing's own level pickers went).
    /// Another level moves it with that level, its offsets kept; a column
    /// keeps its top where it was. A footing under a wall or column, and a
    /// view, read theirs.
    /// Part of the building or of the site (owner markup MU126, mock-up
    /// REV00): what is part of the site shows and hides with the Site
    /// category in every view. It sets everything picked that can be;
    /// with a mix picked, neither side is lit.
    fn part_of_row(&mut self, ui: &mut egui::Ui, e: &creabim_core::Element) {
        if !e.can_be_site() {
            return;
        }
        let model = self.doc.model();
        let picked: Vec<ElementId> = self
            .selection
            .all()
            .into_iter()
            .filter(|id| model.get(*id).is_some_and(|x| x.can_be_site()))
            .collect();
        let targets = if picked.contains(&e.id) {
            picked
        } else {
            vec![e.id]
        };
        let states: Vec<bool> = targets
            .iter()
            .filter_map(|id| model.get(*id).map(|x| x.on_site))
            .collect();
        let was = match states.first() {
            Some(a) if states.iter().all(|b| b == a) => Some(*a),
            _ => None,
        };
        let mut now = was;
        let changed = crate::props::row(ui, "Part of", |ui| {
            crate::props::segment(
                ui,
                &mut now,
                &[
                    (Some(false), "Building", "It shows with its own category in each view: a stair with the stairs"),
                    (Some(true), "Site", "It shows and hides with the site in every view, and an IFC export places it in the site: outdoor steps, a retaining wall, a parked car"),
                ],
            )
        });
        let Some(site) = now.filter(|_| changed) else {
            return;
        };
        let mut tx = creabim_core::Transaction::new(if site {
            "Make it part of the site"
        } else {
            "Make it part of the building"
        });
        let mut any = false;
        for id in targets {
            let Some(before) = self.doc.model().get(id).cloned() else {
                continue;
            };
            if before.on_site == site {
                continue;
            }
            let mut after = before.clone();
            after.on_site = site;
            tx = tx.replace(before, after);
            any = true;
        }
        if any {
            let r = self.doc.commit(tx);
            let msg = if site {
                "Part of the site: it shows where a view shows the site."
            } else {
                "Part of the building again."
            };
            self.report(r, msg.to_string());
        }
    }

    fn level_row(&mut self, ui: &mut egui::Ui, e: &creabim_core::Element) {
        let Some(current) = creabim_core::levels::level_of(&e.kind) else {
            return;
        };
        if matches!(e.kind, ElementKind::View(_) | ElementKind::Guide(_)) {
            return;
        }
        let levels = creabim_core::levels::sorted(self.doc.model());
        let name = levels
            .iter()
            .find(|l| l.0 == current)
            .map_or_else(|| "None".to_string(), |l| l.1.clone());
        let hosted = matches!(&e.kind, ElementKind::Footing(f) if f.host.is_some());
        if hosted {
            crate::props::row(ui, "Level", |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&name).weak()).truncate())
                    .on_hover_text("It follows its host's level: let go of the host to choose one")
            });
            return;
        }
        let mut pick = current;
        crate::app::helpui::combo(ui, "Level")
            .selected_text(name)
            .show_ui(ui, |ui| {
                for (lid, n, _) in levels.iter().rev() {
                    ui.selectable_value(&mut pick, *lid, n);
                }
            })
            .response
            .on_hover_text(
                "The level it hangs off: another level moves it with that level, its offsets kept",
            );
        if pick == current {
            return;
        }
        let model = self.doc.model().clone();
        let r = self.doc.edit(e.id, "Level", |el| match &mut el.kind {
            ElementKind::Column(c) => c.set_base_level(&model, pick),
            _ => *el = creabim_core::levels::rehost(el, pick, 0.0, 0.0),
        });
        let to = levels
            .iter()
            .find(|l| l.0 == pick)
            .map_or(String::new(), |l| l.1.clone());
        self.report(r, format!("On {to} now."));
    }

    /// A tab's quiet group colour (REV04 §1b), or none with the colour
    /// hints turned off in Settings.
    fn tab_hue(&self, ui: &egui::Ui, tab: PropTab) -> Option<Color32> {
        self.shell
            .settings
            .colour_hints
            .then(|| tab.hue().colour(crate::theme::Palette::of(ui.ctx()).dark))
    }

    /// The tabs Properties shows now, in order (MU19).
    fn prop_tabs(&self) -> Vec<PropTab> {
        // the Select tab first, at all times (wave 3, decision 6)
        let mut out = vec![PropTab::Select];
        match self
            .selection
            .active
            .and_then(|id| self.doc.model().get(id))
        {
            Some(_) => {
                out.extend([
                    PropTab::Element,
                    PropTab::Joins,
                    PropTab::Graphics,
                    PropTab::Data,
                ]);
            }
            None => {
                if self.plan.tool != Tool::Select {
                    out.push(PropTab::Next);
                }
            }
        }
        // the Profile view in front: its profile, first of the view's tabs
        if self.profiles_in_front() {
            out.push(PropTab::Profile);
        }
        if self.masters_in_front() {
            out.push(PropTab::Master);
        }
        if self.patterns_in_front() {
            out.push(PropTab::Pattern);
        }
        if matches!(
            self.areas.root.areas().get(self.areas.focus),
            Some(crate::areas::Shows::Log)
        ) {
            out.push(PropTab::Log);
        }
        // the sheet in front is the view the View tab would describe: its
        // own Sheet tab (MU55: the 3D view's crop boxes showed instead)
        let sheet = self.sheet_in_front().is_some();
        if sheet {
            out.push(PropTab::Sheet);
        }
        // the View tab is about the area last clicked, the 3D area included
        // (Properties order REV04 §4, decision 3): no separate 3D view tab
        if self.model_in_front() {
            out.push(PropTab::View);
            // how the 3D view looks, in one place (Visuals REV02)
            out.push(PropTab::Visuals);
        } else if matches!(
            self.areas.root.areas().get(self.areas.focus),
            Some(crate::areas::Shows::Renders)
        ) {
            // a render in front: what it is and what to do with it (MU115)
            out.push(PropTab::Render);
        } else if !sheet
            // a drawing in the area last clicked: the Log, the notepad, the
            // profiles or the masters have no view, and their own tab leads
            // (MU60: the Log clicked still showed the ground floor plan)
            && matches!(
                self.areas.root.areas().get(self.areas.focus),
                Some(crate::areas::Shows::View(_))
            )
            && matches!(self.vis_target(false), Some(gridvis::VisTarget::View(_)))
        {
            out.push(PropTab::View);
            if self.selection.active.is_none() {
                out.push(PropTab::Trace);
            }
        }
        if let Some(ElementKind::FreeMesh(_)) = self
            .selection
            .active
            .and_then(|id| self.doc.model().get(id))
            .map(|e| &e.kind)
        {
            if let Some(at) = out.iter().position(|t| *t == PropTab::Element) {
                out.insert(at + 1, PropTab::Modifiers);
            }
        }
        // a plant: its Tree tab beside Element, simple or grown (MU52)
        if self
            .selection
            .active
            .and_then(|id| self.doc.model().get(id))
            .is_some_and(|e| crate::partstool::is_plant(&e.kind))
        {
            if let Some(at) = out.iter().position(|t| *t == PropTab::Element) {
                out.insert(at + 1, PropTab::Tree);
            }
        }
        // a terrain, an earthwork, a road, a drain: its own tab beside
        // Element (site rework, stage 5)
        if let Some(own) = self
            .selection
            .active
            .and_then(|id| self.doc.model().get(id))
            .and_then(|e| crate::partstool::site_own(&e.kind))
        {
            if let Some(at) = out.iter().position(|t| *t == PropTab::Element) {
                out.insert(at + 1, Self::site_prop_tab(own));
                // hard landscape can be a car park: its Parking tab
                if own == crate::partstool::SiteOwn::Paving {
                    out.insert(at + 2, PropTab::Parking);
                }
            }
        }
        out.push(PropTab::Project);
        out.push(PropTab::Phases);
        out.push(PropTab::Notes);
        out
    }

    fn site_prop_tab(own: crate::partstool::SiteOwn) -> PropTab {
        match own {
            crate::partstool::SiteOwn::Ground => PropTab::Ground,
            crate::partstool::SiteOwn::Earthwork => PropTab::Earthwork,
            crate::partstool::SiteOwn::Road => PropTab::Road,
            crate::partstool::SiteOwn::Drain => PropTab::Drain,
            crate::partstool::SiteOwn::Path => PropTab::Path,
            crate::partstool::SiteOwn::Tank => PropTab::Tank,
            crate::partstool::SiteOwn::Property => PropTab::Property,
            crate::partstool::SiteOwn::Paving => PropTab::Paving,
        }
    }

    /// The sheet shown in the area in use, if it shows one.
    fn sheet_in_front(&self) -> Option<ElementId> {
        match self.areas.root.areas().get(self.areas.focus) {
            Some(crate::areas::Shows::View(id)) => match self.doc.model().get(*id).map(|e| &e.kind)
            {
                Some(ElementKind::Sheet(_)) => Some(*id),
                _ => None,
            },
            _ => None,
        }
    }

    /// The Sheet tab (MU55-03 to 05): the sheet in front, with nothing on
    /// it picked. Its name and number, its paper (size and which way up),
    /// its master and the title block's fields, revision among them.
    fn sheet_tab(&mut self, ui: &mut egui::Ui, id: ElementId) {
        let Some(before) = self.doc.model().get(id).cloned() else {
            return;
        };
        let ElementKind::Sheet(mut sh) = before.kind.clone() else {
            return;
        };
        let mut name = before.name.clone();
        let mut label: Option<&'static str> = None;
        let open = self.prop_header(ui, "sheet", "Sheet", true, Some("sheets"));
        crate::props::fold(ui, open, |ui| {
            if crate::props::row(ui, "Name", |ui| {
                crate::props::text_field(ui, egui::TextEdit::singleline(&mut name), f32::INFINITY)
            })
            .changed()
            {
                label = Some("Rename sheet");
            }
            if let Some(l) = self.sheet_fields(ui, id, &mut sh) {
                label = Some(l);
            }
        });
        let open = self.prop_header(ui, "sheet-title", "Title block", true, Some("sheets"));
        crate::props::fold(ui, open, |ui| {
            if let Some(l) = self.sheet_template_ui(ui, &mut sh) {
                label = Some(l);
            }
        });
        if let Some(l) = label {
            let r = self.doc.edit_merged(id, l, |e| {
                e.name = name.clone();
                e.kind = ElementKind::Sheet(sh.clone());
            });
            self.report(r, format!("{l}."));
        }
    }

    /// A sheet's own rows, in its Sheet tab and when it is selected: its
    /// number, paper size and which way up (MU55), and its guides.
    fn sheet_fields(
        &mut self,
        ui: &mut egui::Ui,
        id: ElementId,
        sh: &mut Sheet,
    ) -> Option<&'static str> {
        let mut label = None;
        // label column and field, as every other row (MU40: labels
        // stacked over their fields read unlike Blender)
        if crate::props::row(ui, "Number", |ui| {
            crate::props::text_field(
                ui,
                egui::TextEdit::singleline(&mut sh.number),
                f32::INFINITY,
            )
        })
        .changed()
        {
            label = Some("Sheet number");
        }
        let std = crate::sheetview::standard(ui.ctx());
        let current = creabim_core::standards::size_name(&std, sh.width, sh.height)
            .unwrap_or_else(|| "Custom".into());
        crate::props::row(ui, "Size", |ui| {
            egui::ComboBox::from_id_salt(("sheet.paper", id))
                .icon(crate::props::combo_icon)
                .width(ui.available_width())
                .selected_text(current.clone())
                .show_ui(ui, |ui| {
                    if let Some(p) = crate::sheetview::paper_menu(ui, &std, &current) {
                        sh.set_paper(&p);
                        label = Some("Sheet size");
                    }
                })
        })
        .response
        .on_hover_text(format!(
            "The paper, {} × {}. A new sheet takes its master's size",
            crate::units::paper_text(sh.width),
            crate::units::paper_text(sh.height)
        ));
        let mut portrait = sh.portrait();
        if crate::props::choice(
            ui,
            "Orientation",
            &mut portrait,
            &[
                (false, "Landscape", "wider than tall"),
                (true, "Portrait", "taller than wide"),
            ],
        ) {
            sh.set_portrait(portrait);
            label = Some("Sheet orientation");
        }
        // its schedules laid out in columns, or each where it was put
        // (MU131); one moved by hand stays where it was put either way
        let auto = creabim_geom::sheet::arranges(self.doc.model(), id);
        let mut on = sh.arrange.unwrap_or(auto);
        if crate::props::choice(
            ui,
            "Schedules",
            &mut on,
            &[
                (
                    true,
                    "Arranged",
                    "laid out in columns; one you move stays where you put it",
                ),
                (false, "Where put", "each stays where you put it"),
            ],
        ) {
            sh.arrange = Some(on);
            label = Some("Arrange schedules");
        }
        let pinned: Vec<Element> = self
            .doc
            .model()
            .iter()
            .filter(|e| matches!(&e.kind, ElementKind::Viewport(vp) if vp.sheet == id && vp.pinned))
            .cloned()
            .collect();
        if on && !pinned.is_empty() {
            let go = crate::props::action(
                ui,
                "",
                "Re-arrange",
                "Schedules you moved by hand go back into the columns",
                true,
            )
            .clicked();
            if go {
                let mut tx = creabim_core::Transaction::new("Re-arrange schedules");
                for e in pinned {
                    let mut after = e.clone();
                    if let ElementKind::Viewport(vp) = &mut after.kind {
                        vp.pinned = false;
                    }
                    tx = tx.replace(e, after);
                }
                if let Err(err) = self.doc.commit(tx) {
                    self.status = err.to_string();
                }
            }
        }
        // its guides, to reuse on other sheets (MU27)
        let n = sh.guides.len();
        if n > 0 {
            crate::props::readout(
                ui,
                "Guides",
                if n == 1 {
                    "1 guide".to_string()
                } else {
                    format!("{n} guides")
                },
                "Lines to place things by, never printed",
            );
            let (copy, clear) = crate::props::actions2(
                ui,
                "",
                (
                    "Copy to all",
                    "Every other sheet gets these guides in place of its own: one layout across the set",
                    true,
                ),
                ("Clear", "Take this sheet's guides away", true),
            );
            if copy {
                self.copy_guides = Some((id, sh.guides.clone()));
            }
            if clear {
                sh.guides.clear();
                label = Some("Clear guides");
            }
        }
        label
    }

    /// Open the Project tab, where the project's information is edited
    /// (MU50: one place, no window): Manage › Project information.
    pub(crate) fn open_project_tab(&mut self) {
        self.prop_tab = PropTab::Project;
        if self.shell.settings.right_folded {
            self.shell.settings.right_folded = false;
            self.shell.settings.store();
        }
    }

    /// The Project tab, the one place the project's information is edited
    /// (MU50, the graphic styles mockup §2): Project, Site and Building, live
    /// fields with undo and no Apply. A small sheet mark in a field says a
    /// title block shows it; the rest name the project in IFC, place it for
    /// the sun and north, or set the trees' season.
    fn project_props(&mut self, ui: &mut egui::Ui) {
        let before = self
            .doc
            .model()
            .project_info()
            .map(|(_, p)| p.clone())
            .unwrap_or_default();
        let mut info = before.clone();
        // what changed, for its undo step ("Project name")
        let mut what: Option<&'static str> = None;
        // a text field the row's width, its hint "e.g. …" in italics, and
        // the sheet mark at its right end when title blocks read it
        let text =
            |ui: &mut egui::Ui, label: &str, v: &mut String, hint: &str, on_sheets: bool| -> bool {
                crate::props::row(ui, label, |ui| {
                    let mut edit = egui::TextEdit::singleline(v);
                    if !hint.is_empty() {
                        edit = edit.hint_text(egui::RichText::new(hint).italics());
                    }
                    if on_sheets {
                        edit = edit.margin(egui::Margin {
                            left: 4,
                            right: 20,
                            top: 2,
                            bottom: 2,
                        });
                    }
                    let r = crate::props::text_field(ui, edit, f32::INFINITY);
                    if on_sheets {
                        let p = crate::theme::Palette::of(ui.ctx());
                        let at = egui::Rect::from_center_size(
                            egui::pos2(r.rect.right() - 10.0, r.rect.center().y),
                            egui::vec2(12.0, 12.0),
                        );
                        crate::svgicon::paint(
                            ui.painter(),
                            at,
                            "file-text",
                            p.muted.gamma_multiply(0.8),
                        );
                        let tip = ui.interact(at, r.id.with("sheet"), egui::Sense::hover());
                        tip.on_hover_text(
                            "Shown in title blocks, on every sheet (a sheet can still set its own)",
                        );
                    }
                    r.changed()
                })
            };
        let mut new_std: Option<String> = None;
        let mut new_ls: Option<String> = None;
        let mut new_pal: Option<String> = None;
        let open = self.prop_header(ui, "project", "Project", true, Some("project-info"));
        crate::props::fold(ui, open, |ui| {
            if text(ui, "Name", &mut info.name, "e.g. Maple Lane House", true) {
                what = Some("Project name");
            }
            if text(ui, "Number", &mut info.number, "e.g. 2026-014", true) {
                what = Some("Project number");
            }
            if text(
                ui,
                "Client",
                &mut info.client,
                "e.g. The Rivera family",
                true,
            ) {
                what = Some("Client");
            }
            if text(
                ui,
                "Status",
                &mut info.status,
                "e.g. Planning application",
                true,
            ) {
                what = Some("Project status");
            }
            if text(
                ui,
                "Issue date",
                &mut info.issue_date,
                "e.g. 14 Nov 2026",
                true,
            ) {
                what = Some("Issue date");
            }
            // the drawing standard: its sheet sizes and how scales are
            // written (standards plan, decision 1)
            let now = creabim_core::standards::of(self.doc.model()).name;
            let mut pick = now.clone();
            let names: Vec<String> = creabim_core::standards::shipped()
                .into_iter()
                .map(|s| s.name)
                .collect();
            let opts: Vec<(&str, &str, &str)> = names
                .iter()
                .map(|n| {
                    (
                        n.as_str(),
                        n.as_str(),
                        if n == "US" {
                            "ANSI and ARCH sheets, scales written 1/4\" = 1'-0\""
                        } else {
                            "A sheets, scales written 1:50"
                        },
                    )
                })
                .collect();
            let mut p = pick.as_str();
            if crate::props::choice(ui, "Standard", &mut p, &opts) {
                pick = p.to_string();
            }
            if pick != now {
                new_std = Some(pick);
            }
            // the project's line style (pens stage 4): views without their
            // own or their template's draw in it
            let pack = creabim_core::styles::active_pack(self.doc.model());
            let names = pack.line_style_names();
            let now_ls = pack
                .line_style
                .clone()
                .unwrap_or_else(|| creabim_core::linestyle::RULED.to_string());
            let mut pick_ls = now_ls.clone();
            crate::app::helpui::combo(ui, "Line style")
                .selected_text(pick_ls.clone())
                .show_ui(ui, |ui| {
                    for n in &names {
                        ui.selectable_value(&mut pick_ls, n.clone(), n);
                    }
                })
                .response
                .on_hover_text("How the project's lines are drawn, Ruled to hand-made; a view or template can pick its own");
            if pick_ls != now_ls {
                new_ls = Some(pick_ls);
            }
            // the project's palette (pens stage 7)
            let pal_names = pack.palette_names();
            let now_pal = pack.palette.clone().unwrap_or_else(|| "Ink".to_string());
            let mut pick_pal = now_pal.clone();
            crate::app::helpui::combo(ui, "Palette")
                .selected_text(pick_pal.clone())
                .show_ui(ui, |ui| {
                    for n in &pal_names {
                        ui.selectable_value(&mut pick_pal, n.clone(), n);
                    }
                })
                .response
                .on_hover_text("The colours the project's drawings take; a view or template can pick its own. Graphic styles › Palette tunes them");
            if pick_pal != now_pal {
                new_pal = Some(pick_pal);
            }
        });
        if let Some(name) = new_pal {
            let (id, mut pack) = self.edited_pack();
            pack.palette = (name != "Ink").then_some(name);
            self.commit_pack(id, pack, "Palette");
        }
        if let Some(name) = new_ls {
            let (id, mut pack) = self.edited_pack();
            pack.line_style = (name != creabim_core::linestyle::RULED).then_some(name);
            self.commit_pack(id, pack, "Line style");
        }
        if let Some(name) = new_std {
            let (id, mut pack) = self.edited_pack();
            let from = creabim_core::standards::of(self.doc.model());
            pack.standard = creabim_core::standards::shipped()
                .into_iter()
                .find(|s| s.name == name)
                .filter(|s| s.name != "ISO");
            // the views move to the new standard's scales with it, in the
            // same step (owner MU108)
            let to = pack
                .standard
                .clone()
                .unwrap_or_else(creabim_core::standards::iso);
            let moves = creabim_core::standards::rescaled_views(self.doc.model(), &from, &to);
            let n = moves.len();
            if n == 0 {
                self.commit_pack(id, pack, "Drawing standard");
            } else {
                // the pack and the views in one step, one undo
                let mut tx = match id.and_then(|id| self.doc.model().get(id).cloned()) {
                    Some(before) => {
                        let mut after = before.clone();
                        after.name = pack.name.clone();
                        after.kind = ElementKind::StylePack(Box::new(pack));
                        Transaction::new("Drawing standard").replace(before, after)
                    }
                    None => creabim_core::styles::apply_pack(self.doc.model(), &pack),
                };
                tx.label = "Drawing standard".into();
                for (b, a) in moves {
                    tx = tx.replace(b, a);
                }
                let r = self.doc.commit(tx);
                self.report(
                    r,
                    format!("Drawing standard: {n} views moved to its scales. Ctrl+Z to undo."),
                );
            }
        }
        let open = self.prop_header(ui, "project-site", "Site", true, Some("project-info"));
        crate::props::fold(ui, open, |ui| {
            if text(
                ui,
                "Site name",
                &mut info.site_name,
                "e.g. Maple Lane",
                true,
            ) {
                what = Some("Site name");
            }
            if text(
                ui,
                "Address",
                &mut info.address,
                "e.g. 12 Maple Lane, Greenfield",
                true,
            ) {
                what = Some("Site address");
            }
            if degrees_field(
                ui,
                "Latitude",
                "project.lat",
                &mut info.latitude,
                90.0,
                "Degrees, north positive. For the sun, and to turn a tree's year south of the equator. Empty is not set",
            ) {
                what = Some("Latitude");
            }
            if degrees_field(
                ui,
                "Longitude",
                "project.lon",
                &mut info.longitude,
                180.0,
                "Degrees, east positive. Empty is not set",
            ) {
                what = Some("Longitude");
            }
            if crate::props::row(ui, "Elevation", |ui| {
                crate::props::fill(
                    ui,
                    egui::DragValue::new(&mut info.elevation)
                        .speed(0.01)
                        .max_decimals(3)
                        .suffix(" m"),
                )
                .on_hover_text("The project's zero above sea level")
                .changed()
            }) {
                what = Some("Elevation");
            }
            {
                // how site levels are written, for the whole project
                let mut how = info.site_levels;
                let name = |r: creabim_core::LevelReading| match r {
                    creabim_core::LevelReading::Datum => "From datum",
                    creabim_core::LevelReading::Zero => "From zero",
                };
                crate::props::Combo::salted("Site levels", "project-site-levels")
                    .selected_text(name(how))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut how, creabim_core::LevelReading::Datum, name(creabim_core::LevelReading::Datum))
                            .on_hover_text("Site levels are written above the datum, as site drawings write them (100.45): in plans, schedules, long sections and Properties");
                        ui.selectable_value(&mut how, creabim_core::LevelReading::Zero, name(creabim_core::LevelReading::Zero))
                            .on_hover_text("Site levels are written above the project's zero, as building levels are (+0.45)");
                    });
                if how != info.site_levels {
                    info.site_levels = how;
                    what = Some("Site levels");
                }
            }
            if crate::props::row(ui, "True north", |ui| {
                crate::props::fill(
                    ui,
                    egui::DragValue::new(&mut info.true_north)
                        .range(-180.0..=180.0)
                        .speed(0.5)
                        .max_decimals(2)
                        .suffix("°"),
                )
                .on_hover_text("How far true north turns from the drawings' up, anticlockwise")
                .changed()
            }) {
                what = Some("True north");
            }
            let m = info.season();
            crate::props::Combo::new("Season in 3D")
                .selected_text(creabim_core::tree::MONTH_NAMES[(m - 1) as usize])
                .show_ui(ui, |ui| {
                    for (i, name) in creabim_core::tree::MONTH_NAMES.iter().enumerate() {
                        let n = i as u8 + 1;
                        if ui.selectable_label(m == n, *name).clicked() && m != n {
                            info.month = n;
                            what = Some("Season in 3D");
                        }
                    }
                })
                .response
                .on_hover_text("The month every tree shows in 3D: leaves, autumn colour, bare branches. A tree can keep a month of its own. Drawings always show the full crown.");
            let mut placed = info.map.is_some();
            if crate::props::check(ui, "On the map", &mut placed, "Placed")
                .on_hover_text("The project's origin has map coordinates. Importing a survey that lies in map coordinates sets this by itself; map data brought in later then lands in the right place")
                .changed()
            {
                info.map = placed.then(creabim_core::MapPlace::default);
                what = Some("Place on the map");
            }
            if let Some(m) = &mut info.map {
                if text(ui, "Coordinates", &mut m.crs, "e.g. EPSG:32617", false) {
                    what = Some("Map coordinate system");
                }
                for (label, v, tip) in [
                    ("Easting", &mut m.easting, "The map coordinate of the drawings' origin, east"),
                    ("Northing", &mut m.northing, "The map coordinate of the drawings' origin, north. The drawings' up is the map's grid north"),
                ] {
                    if crate::props::row(ui, label, |ui| {
                        crate::props::fill(
                            ui,
                            egui::DragValue::new(v)
                                .speed(0.1)
                                .max_decimals(3)
                                .suffix(" m"),
                        )
                        .on_hover_text(tip)
                        .changed()
                    }) {
                        what = Some("Map position");
                    }
                }
            }
        });
        let open = self.prop_header(
            ui,
            "project-building",
            "Building",
            true,
            Some("project-info"),
        );
        crate::props::fold(ui, open, |ui| {
            if text(ui, "Name", &mut info.building_name, "e.g. House", true) {
                what = Some("Building name");
            }
            if text(ui, "Use", &mut info.building_use, "e.g. Dwelling", false) {
                what = Some("Building use");
            }
            ui.add_space(2.0);
            crate::props::flow(ui, |ui| {
                if ui.button("Levels…").clicked() {
                    self.open_levels();
                }
                if ui.button("Materials…").clicked() {
                    self.open_materials(None);
                }
            });
        });
        let open = self.prop_header(
            ui,
            "project-openings",
            "Openings",
            false,
            Some("project-info"),
        );
        crate::props::fold(ui, open, |ui| {
            use creabim_core::{CasementSide, TriangleApex};
            // short words, and a list for the apex: at the narrowest panel
            // two buttons cut "Hinges" and "Handle" short
            let apex = [
                (TriangleApex::Hinge, "Hinges", "In elevations and sections, a door or window panel's opening triangle points to its hinges, as in North America and the UK"),
                (TriangleApex::Handle, "Handle", "The triangle points to the handle side, as in much of Europe"),
            ];
            let now = info.openings.triangle_apex;
            crate::props::Combo::new("Triangle")
                .selected_text(apex.iter().find(|a| a.0 == now).map_or("", |a| a.1))
                .show_ui(ui, |ui| {
                    for (v, name, tip) in apex {
                        if ui.selectable_label(now == v, name).on_hover_text(tip).clicked() && now != v {
                            info.openings.triangle_apex = v;
                            what = Some("Triangle apex");
                        }
                    }
                })
                .response
                .on_hover_text("Where the opening triangle of a door or window points in elevations and sections");
            if crate::props::choice(
                ui,
                "Casement",
                &mut info.openings.casements,
                &[
                    (
                        CasementSide::Outward,
                        "Out",
                        "New casement windows open outward; each can still be flipped",
                    ),
                    (
                        CasementSide::Inward,
                        "In",
                        "New casement windows open into the room; each can still be flipped",
                    ),
                ],
            ) {
                what = Some("Casement side");
            }
        });
        // how far the ground keeps from what is built (MU140): the usual
        // rules for timber, the project's to change for another country's
        let open = self.prop_header(ui, "project-ground", "Ground", false, Some("project-info"));
        crate::props::fold(ui, open, |ui| {
            // (the room under beams is kept with them, not yet checked: no
            // row until it is)
            #[allow(clippy::type_complexity)]
            let rows: [(&str, &str, fn(&mut creabim_core::GroundRules) -> &mut f64, &'static str); 2] = [
                ("Crawl space", "The room under a framed floor's joists. A framed floor on stem walls digs this far below them", |g| &mut g.crawl, "Crawl space depth"),
                ("Wood to ground", "How far the ground round the house stays below the wood: studs, sheathing, cladding", |g| &mut g.wood, "Wood to ground"),
            ];
            for (name, tip, field, label) in rows {
                let v = field(&mut info.ground);
                let mut x = *v;
                if crate::props::row(ui, name, |ui| {
                    crate::props::fill(ui, crate::units::drag(&mut x))
                        .on_hover_text(tip)
                        .changed()
                }) && x >= 0.0
                {
                    *v = x;
                    what = Some(label);
                }
            }
        });
        if let Some(label) = what.filter(|_| info != before) {
            // live, one undo step a field (typing merges into it)
            let mut tx = creabim_core::set_project(self.doc.model(), info);
            tx.label = label.to_string();
            let r = self.doc.commit_merged(tx);
            self.report(r, format!("{label} changed: title blocks and IFC follow."));
        }
    }

    /// The strip of tab icons at Properties' left (MU19).
    fn prop_strip(&mut self, ui: &mut egui::Ui, tabs: &[PropTab], height: f32) {
        // as Blender's (MU22): a column a tone darker than the panel, flush
        // at its edge; 26 px cells with 16 px icons; the open tab painted
        // in the panel's own colour so it reads as one piece with what it
        // shows, with a thin accent bar at its left
        let p = crate::theme::Palette::of(ui.ctx());
        let panel = ui.visuals().panel_fill;
        let column = {
            let (to, t) = if p.dark { (0.0, 0.25) } else { (0.0, 0.06) };
            let m = |c: u8| (c as f32 + (to - c as f32) * t).round() as u8;
            Color32::from_rgb(m(panel.r()), m(panel.g()), m(panel.b()))
        };
        const W: f32 = 30.0;
        const CELL: f32 = 26.0;
        let top = ui.cursor().min;
        // the column runs from the panel's very top, so its edge meets the
        // window's line cleanly (MU27: a stub of panel showed above it)
        let from = ui.clip_rect().top().min(top.y);
        let strip = egui::Rect::from_min_max(
            egui::pos2(top.x, from),
            egui::pos2(top.x + W, top.y + height),
        );
        ui.painter().rect_filled(strip, 0.0, column);
        ui.allocate_ui_with_layout(
            egui::vec2(W, height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.add_space(4.0);
                let mut last_group = None;
                for t in tabs {
                    let group = t.group();
                    if last_group.is_some_and(|g| g != group) {
                        // a gap and a hairline between groups, as Blender's
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(W, 9.0), egui::Sense::hover());
                        ui.painter().line_segment(
                            [
                                egui::pos2(r.left() + 7.0, r.center().y),
                                egui::pos2(r.right() - 7.0, r.center().y),
                            ],
                            egui::Stroke::new(1.0, p.border),
                        );
                    }
                    last_group = Some(group);
                    let (row, resp) =
                        ui.allocate_exact_size(egui::vec2(W, CELL), egui::Sense::click());
                    let r =
                        egui::Rect::from_min_max(egui::pos2(row.left() + 2.0, row.top()), row.max);
                    let on = *t == self.prop_tab;
                    // the group's quiet colour where the hints are on
                    // (REV04 §1b): the open tab's icon and bar in it, the
                    // others a little toward grey
                    let hue = self.tab_hue(ui, *t);
                    let mark = hue.unwrap_or(p.accent);
                    if on {
                        ui.painter().rect_filled(
                            r,
                            egui::CornerRadius {
                                nw: 4,
                                sw: 4,
                                ne: 0,
                                se: 0,
                            },
                            panel,
                        );
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(
                                r.min + egui::vec2(0.0, 4.0),
                                egui::vec2(2.0, CELL - 8.0),
                            ),
                            1.0,
                            mark,
                        );
                    } else if resp.hovered() {
                        ui.painter()
                            .rect_filled(r.shrink(1.0), 4.0, mark.gamma_multiply(0.12));
                    }
                    let ink = match (on, hue) {
                        (true, _) => mark,
                        (false, Some(h)) => {
                            // 80 % the hue, 20 % the grey, as the mockup
                            let m = |a: u8, b: u8| (a as f32 * 0.2 + b as f32 * 0.8).round() as u8;
                            Color32::from_rgb(
                                m(p.muted.r(), h.r()),
                                m(p.muted.g(), h.g()),
                                m(p.muted.b(), h.b()),
                            )
                        }
                        (false, None) => p.muted,
                    };
                    let icon = egui::Rect::from_center_size(r.center(), egui::vec2(16.0, 16.0));
                    crate::svgicon::paint(ui.painter(), icon, t.icon(), ink);
                    if resp.on_hover_text(t.label()).clicked() {
                        self.prop_tab = *t;
                        // folded, a tab opens it again on that tab (MU27)
                        if self.shell.settings.right_folded {
                            self.shell.settings.right_folded = false;
                            self.shell.settings.store();
                        }
                    }
                }
            },
        );
    }
}

/// Which sheets the Export sheets dialog writes (MU22).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ExportScope {
    #[default]
    All,
    Chosen,
    This,
}

/// The Export sheets dialog's choices, kept for the session.
#[derive(Default)]
pub(crate) struct SheetsExport {
    scope: ExportScope,
    /// A PDF a sheet, not one for all.
    each: bool,
    /// Print on white: the masters' paper tint and texture left off.
    pub white: bool,
    chosen: std::collections::BTreeSet<ElementId>,
}

/// Width and height rows for a sheet picture, its proportions kept when
/// `aspect` (height over width) is given. True when changed.
fn sheet_size_rows(ui: &mut egui::Ui, w: &mut f64, h: &mut f64, aspect: Option<f64>) -> bool {
    let (mut nw, mut nh) = (*w, *h);
    let cw = crate::props::row(ui, "Width", |ui| {
        crate::props::fill(ui, crate::units::paper(&mut nw).range(2.0..=2000.0))
    })
    .changed();
    let ch = crate::props::row(ui, "Height", |ui| {
        crate::props::fill(ui, crate::units::paper(&mut nh).range(2.0..=2000.0))
    })
    .changed();
    match (cw, ch, aspect) {
        (true, _, Some(a)) => {
            *w = nw;
            *h = nw * a;
        }
        (false, true, Some(a)) => {
            *h = nh;
            *w = nh / a.max(1e-9);
        }
        (true, _, None) | (_, true, None) => {
            *w = nw;
            *h = nh;
        }
        _ => return false,
    }
    true
}

/// A north arrow's or a scale bar's settings (MU22). The undo label of a
/// change.
fn paper_mark_fields(
    ui: &mut egui::Ui,
    m: &mut creabim_core::PaperMark,
    true_north: f64,
    model: &creabim_core::Model,
) -> Option<&'static str> {
    use creabim_core::PaperMarkKind;
    let mut label = None;
    if matches!(m.kind, PaperMarkKind::ScaleBar { .. }) {
        let drawings: Vec<(ElementId, String, f64)> =
            creabim_geom::sheet::viewports_on(model, m.sheet)
                .into_iter()
                .filter_map(|(id, _, name)| {
                    creabim_geom::sheet::drawing_scale(model, id).map(|s| (id, name, s))
                })
                .collect();
        let shown = m
            .measures
            .and_then(|v| drawings.iter().find(|d| d.0 == v))
            .map_or("Its own scale".to_string(), |d| {
                format!(
                    "{} \u{b7} {}",
                    d.1,
                    creabim_core::standards::scale_label(model, d.2)
                )
            });
        crate::props::row(ui, "Measures", |ui| {
            egui::ComboBox::from_id_salt("bar_measures")
                // the house chevron, as every Properties list (owner)
                .icon(crate::props::combo_icon)
                .width(ui.available_width())
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(m.measures.is_none(), "Its own scale")
                        .clicked()
                    {
                        m.measures = None;
                        label = Some("Scale bar measures");
                    }
                    for (id, name, s) in &drawings {
                        let text = format!(
                            "{name} \u{b7} {}",
                            creabim_core::standards::scale_label(model, *s)
                        );
                        if ui.selectable_label(m.measures == Some(*id), text).clicked() {
                            m.measures = Some(*id);
                            *m = m.at_scale(*s);
                            label = Some("Scale bar measures");
                        }
                    }
                })
                .response
                .on_hover_text(
                    "The drawing it reads its scale from; it follows that drawing's scale",
                )
        });
    }
    let measured = m
        .measures
        .and_then(|v| creabim_geom::sheet::drawing_scale(model, v));
    let color = &mut m.color;
    match &mut m.kind {
        PaperMarkKind::Cloud {
            w,
            h,
            rev,
            date,
            what,
            points,
            arc,
        } => {
            // its shape: a box sized by its grips, or points of its own
            // (MU25)
            let mut free = points.len() >= 3;
            // the house segmented control (owner, after MU92)
            let was_free = free;
            crate::props::row(ui, "Shape", |ui| {
                crate::props::segment(
                    ui,
                    &mut free,
                    &[
                        (false, "Box", "Drag its corner and edge grips to size it"),
                        (
                            true,
                            "Freeform",
                            "Its own points: drag one to move it, drag a + to add one, right-click one to remove it",
                        ),
                    ],
                );
            });
            {
                {
                    if !free && was_free {
                        let (lo, hi) = points.iter().fold(
                            (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN)),
                            |(a, b), p| (a.min(*p), b.max(*p)),
                        );
                        *w = (hi.x - lo.x).max(5.0);
                        *h = (hi.y - lo.y).max(5.0);
                        points.clear();
                        label = Some("Revision cloud shape");
                    }
                    if free && !was_free {
                        let (x, y) = (*w / 2.0, *h / 2.0);
                        *points = vec![
                            DVec2::new(-x, -y),
                            DVec2::new(x, -y),
                            DVec2::new(x, y),
                            DVec2::new(-x, y),
                        ];
                        label = Some("Revision cloud shape");
                    }
                }
            }
            crate::props::row(ui, "Scallops", |ui| {
                if crate::props::fill(ui, crate::units::paper_small(arc).range(2.0..=30.0))
                    .on_hover_text("How long each arc is along the outline")
                    .changed()
                {
                    label = Some("Revision cloud");
                }
            });
            crate::props::row(ui, "Colour", |ui| {
                crate::props::flow(ui, |ui| {
                    let mut own = color.is_some();
                    if crate::props::tickbox(ui, &mut own, "Own")
                        .on_hover_text("Black without one")
                        .changed()
                    {
                        *color = own.then_some([0.85, 0.1, 0.1]);
                        label = Some("Revision cloud colour");
                    }
                    if let Some(c) = color {
                        if crate::props::colour_button_rgb(ui, c).changed() {
                            label = Some("Revision cloud colour");
                        }
                    }
                })
            });
            let text = |ui: &mut egui::Ui, name: &str, v: &mut String| {
                crate::props::row(ui, name, |ui| {
                    crate::props::fill(
                        ui,
                        egui::TextEdit::singleline(v).vertical_align(egui::Align::Center),
                    )
                    .changed()
                })
            };
            if text(ui, "Revision", rev) | text(ui, "Date", date) | text(ui, "What changed", what) {
                label = Some("Revision cloud");
            }
            if !free {
                for (name, v) in [("Width", w), ("Height", h)] {
                    crate::props::row(ui, name, |ui| {
                        if crate::props::fill(ui, crate::units::paper(v).range(5.0..=800.0))
                            .changed()
                        {
                            label = Some("Revision cloud");
                        }
                    });
                }
            } else {
                crate::props::worked_out(
                    ui,
                    "cloud",
                    &[crate::props::Worked::new(
                        "Points",
                        points.len().to_string(),
                    )],
                );
                crate::props::howto(ui, "Cloud: drag a point to move it, drag a + to add one, right-click one to remove it.");
            }
        }
        PaperMarkKind::Picture {
            w,
            h,
            width,
            height,
            opacity,
            ..
        } => {
            let aspect = *height as f64 / (*width).max(1) as f64;
            if sheet_size_rows(ui, w, h, Some(aspect)) {
                label = Some("Picture size");
            }
            let mut pct = *opacity * 100.0;
            if crate::props::row(ui, "Opacity", |ui| {
                ui.spacing_mut().slider_width = (ui.available_width() - 64.0).max(40.0);
                ui.add(
                    egui::Slider::new(&mut pct, 0.0..=100.0)
                        .suffix(" %")
                        .max_decimals(0),
                )
            })
            .changed()
            {
                *opacity = (pct / 100.0).clamp(0.0, 1.0);
                label = Some("Picture opacity");
            }
            crate::props::row(ui, "Pixels", |ui| ui.label(format!("{width} × {height}")));
            if crate::props::action(
                ui,
                "",
                "Its own proportions",
                "Undo any stretching: its height from its width",
                ((*h / *w) - aspect).abs() > 1e-3,
            )
            .clicked()
            {
                *h = *w * aspect;
                label = Some("Picture proportions");
            }
            crate::props::howto(
                ui,
                "Picture: drag its grips to size it; the corners keep its proportions, the edges stretch it.",
            );
        }
        PaperMarkKind::Vector { w, h, shapes } => {
            let aspect = *h / w.max(1e-9);
            if sheet_size_rows(ui, w, h, Some(aspect)) {
                label = Some("Picture size");
            }
            crate::props::worked_out(
                ui,
                "vector-mark",
                &[
                    crate::props::Worked::new("Source", "SVG, crisp at any size"),
                    crate::props::Worked::new("Shapes", shapes.len().to_string()),
                ],
            );
            let mut one = match shapes.first() {
                Some(creabim_core::titleblock::TitleItem::Shape { fill: Some(c), .. })
                | Some(creabim_core::titleblock::TitleItem::Shape { colour: c, .. }) => *c,
                _ => [0.0; 3],
            };
            if crate::props::row(ui, "One colour", |ui| {
                crate::props::colour_button_rgb(ui, &mut one)
            })
            .changed()
            {
                for s in shapes.iter_mut() {
                    if let creabim_core::titleblock::TitleItem::Shape { colour, fill, .. } = s {
                        *colour = one;
                        if fill.is_some() {
                            *fill = Some(one);
                        }
                    }
                }
                label = Some("Picture colour");
            }
        }
        PaperMarkKind::RevisionTable => {
            crate::props::row(ui, "Text", |ui| {
                if crate::props::fill(ui, crate::units::paper_small(&mut m.size).range(1.5..=6.0))
                    .changed()
                {
                    label = Some("Revisions table");
                }
            });
            crate::props::info(ui, "A row for each revision cloud on this sheet.", "");
        }
        PaperMarkKind::NorthArrow { rotation, style } => {
            if stylepages::mark_style_ui(ui, model, "north", style) {
                label = Some("North arrow style");
            }
            let mut own = rotation.is_some();
            if crate::props::check(ui, "Points to", &mut own, "An angle of its own")
                .on_hover_text(format!(
                    "Off, it follows the project's true north ({true_north:.1}°, the Project tab)"
                ))
                .changed()
            {
                *rotation = own.then_some(true_north);
                label = Some("North arrow");
            }
            if let Some(r) = rotation {
                crate::props::row(ui, "Angle", |ui| {
                    if crate::props::fill(ui, egui::DragValue::new(r).speed(0.5).suffix("°"))
                        .changed()
                    {
                        label = Some("North arrow");
                    }
                });
            }
            crate::props::row(ui, "Height", |ui| {
                if crate::props::fill(ui, crate::units::paper(&mut m.size).range(6.0..=60.0))
                    .changed()
                {
                    label = Some("North arrow");
                }
            });
        }
        PaperMarkKind::ScaleBar {
            scale,
            length,
            parts,
            feet,
            style,
        } => {
            if stylepages::mark_style_ui(ui, model, "scales", style) {
                label = Some("Scale bar style");
            }
            if let Some(s) = measured {
                // read from its drawing: shown, not typed
                *scale = s;
                crate::props::row(ui, "Scale", |ui| {
                    ui.label(creabim_core::standards::scale_label(model, s))
                        .on_hover_text("From the drawing it measures")
                });
            } else {
                // one field on one line, in the standard's words (1/8" =
                // 1'-0"); a ratio or 96 can be typed (owner: "1 :" and the
                // number sat on two lines, and a US project read 1:100)
                let std = creabim_core::standards::of(model);
                let std2 = std.clone();
                crate::props::row(ui, "Scale", |ui| {
                    if crate::props::fill(
                        ui,
                        egui::DragValue::new(scale)
                            .speed(1.0)
                            .range(1.0..=10000.0)
                            .custom_formatter(move |v, _| std.scale_label(v))
                            .custom_parser(move |t| {
                                let t = t.trim();
                                std2.scales
                                    .iter()
                                    .find(|s| s.label == t)
                                    .map(|s| s.ratio)
                                    .or_else(|| {
                                        t.trim_start_matches("1:").trim().parse::<f64>().ok()
                                    })
                            }),
                    )
                    .on_hover_text("Its own scale: type a ratio (1:50, 96) or the standard's words")
                    .changed()
                    {
                        label = Some("Scale bar");
                    }
                });
            }
            // in feet for a bar counted in feet, else metres
            crate::props::row(ui, "Length", |ui| {
                let mut shown = if *feet { *length / 0.3048 } else { *length };
                let r = crate::props::fill(
                    ui,
                    egui::DragValue::new(&mut shown)
                        .speed(0.1)
                        .range(0.1..=10000.0)
                        .suffix(if *feet { " ft" } else { " m" }),
                );
                if r.changed() {
                    *length = if *feet { shown * 0.3048 } else { shown };
                    label = Some("Scale bar");
                }
            });
            // the house segmented control (owner: loose labels, not aligned)
            crate::props::row(ui, "Counts in", |ui| {
                if crate::props::segment(
                    ui,
                    feet,
                    &[
                        (false, "m", "Metres: its length and numbers in metres"),
                        (true, "ft", "Feet: its length and numbers in feet"),
                    ],
                ) {
                    *length = creabim_core::papermark::round_length_in(*length * 1.0001, *feet);
                    label = Some("Scale bar");
                }
            });
            crate::props::row(ui, "Parts", |ui| {
                if crate::props::fill(ui, egui::DragValue::new(parts).range(1..=10)).changed() {
                    label = Some("Scale bar");
                }
            });
            crate::props::row(ui, "Thickness", |ui| {
                if crate::props::fill(ui, crate::units::paper_small(&mut m.size).range(0.5..=10.0))
                    .changed()
                {
                    label = Some("Scale bar");
                }
            });
        }
    }
    // typed, to the millimetre, so marks line up sheet to sheet (MU25)
    for (name, v, tip) in [
        ("X", &mut m.at.x, "From the sheet's left edge"),
        ("Y", &mut m.at.y, "From the sheet's bottom edge"),
    ] {
        crate::props::row(ui, name, |ui| {
            if crate::props::fill(ui, crate::units::paper(v).range(-2000.0..=2000.0))
                .on_hover_text(tip)
                .changed()
            {
                label = Some("Place paper mark");
            }
        });
    }
    label
}

/// The tabs of Properties (MU19): what a tab holds is its sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PropTab {
    /// What is selected and more like it (wave 3, decision 6): shown at
    /// all times, the home of every way of selecting.
    Select,
    /// The next element a placing tool makes.
    Next,
    /// What it is: its type, sizes and build.
    Element,
    /// Its junctions and booleans.
    Joins,
    /// How views show it: overrides, visibility.
    Graphics,
    /// IFC class and data.
    Data,
    /// The view in the focused area.
    View,
    /// What is traced under the view.
    Trace,
    /// The 3D view: what it shows, crop boxes.
    Model3d,
    /// How the 3D view looks: its look, light, camera and output
    /// (Visuals REV02: one place for everything about visuals).
    Visuals,
    /// The render in a Render area in front: what it was made with, and
    /// what to do with it.
    Render,
    /// The project: information, levels, materials.
    Project,
    /// The project's notepad: notes and to-dos (MU26).
    Notes,
    /// The project's phases: Existing, Demolition, Proposed.
    Phases,
    /// The element's modifiers (MU27): array, mirror, solidify, subdivide.
    Modifiers,
    /// The profile in the Profile view in front (MU40).
    Profile,
    /// The master in the Masters view in front (MU40).
    Master,
    /// The pattern in a Patterns area.
    Pattern,
    /// The Log view in front: its filter and what is in it (MU40).
    Log,
    /// A grown tree: its crown, leaves, drawing and seed, as a subject of
    /// its own (Blender's tabs: Tree, Light, Renovation).
    Tree,
    /// A terrain: how it is shaped (site rework, stage 5).
    Ground,
    /// A pad, a pit or a trench: how it meets the ground.
    Earthwork,
    /// A road: how it is set out.
    Road,
    /// A drain: how it is laid.
    Drain,
    /// A path: how it is laid.
    Path,
    /// A tank: how it is built and buried.
    Tank,
    /// A property: the lot, its edges and the rules it is built to.
    Property,
    /// Hard landscape: how it is laid.
    Paving,
    /// Hard landscape as a car park: its bays, aisles and count.
    Parking,
    /// The sheet in front (MU55): its paper, size and way up, master,
    /// revision and title-block fields, when nothing on it is selected.
    Sheet,
}

impl PropTab {
    /// Its colour hint's group (REV04 §1b): the element and how it is
    /// made, its IFC data, how it is shown, the project.
    fn hue(self) -> crate::props::Hue {
        use crate::props::Hue;
        match self {
            PropTab::Data => Hue::Data,
            PropTab::Graphics
            | PropTab::View
            | PropTab::Trace
            | PropTab::Model3d
            | PropTab::Visuals
            | PropTab::Render
            | PropTab::Profile
            | PropTab::Master
            | PropTab::Pattern
            | PropTab::Sheet
            | PropTab::Log => Hue::View,
            PropTab::Project | PropTab::Notes | PropTab::Phases => Hue::Project,
            _ => Hue::Element,
        }
    }

    /// A tab of the view in front (a master, a profile, the log): it shows
    /// what that view edits, whatever is picked in the model (#42).
    pub(crate) fn of_its_view(self) -> bool {
        matches!(self, PropTab::Master | PropTab::Profile | PropTab::Log)
    }

    /// The strip's groups, a hairline apart: what is selected, the view,
    /// the whole project.
    fn group(self) -> u8 {
        match self {
            PropTab::Select
            | PropTab::Next
            | PropTab::Element
            | PropTab::Joins
            | PropTab::Graphics
            | PropTab::Data => 0,
            PropTab::View
            | PropTab::Trace
            | PropTab::Model3d
            | PropTab::Visuals
            | PropTab::Render
            | PropTab::Profile
            | PropTab::Master
            | PropTab::Pattern
            | PropTab::Sheet
            | PropTab::Log => 1,
            PropTab::Project | PropTab::Notes | PropTab::Phases => 2,
            PropTab::Modifiers
            | PropTab::Tree
            | PropTab::Ground
            | PropTab::Earthwork
            | PropTab::Road
            | PropTab::Drain
            | PropTab::Path
            | PropTab::Tank
            | PropTab::Property
            | PropTab::Paving
            | PropTab::Parking => 0,
        }
    }

    /// What the tab is about, after its name on its title line (the
    /// Properties rule, MU52): "Tree · how it grows".
    fn about(self) -> &'static str {
        match self {
            PropTab::Select => "what you picked, and more like it",
            PropTab::Next => "what the tool makes next",
            PropTab::Element => "what it is and where",
            PropTab::Joins => "how it meets the others",
            PropTab::Graphics => "how it is shown here",
            PropTab::Data => "what the IFC file says",
            PropTab::View => "how it draws",
            PropTab::Trace => "what is traced under the view",
            PropTab::Model3d => "what the 3D view shows",
            PropTab::Visuals => "how it looks",
            PropTab::Render => "the render in front",
            PropTab::Project => "names and place, on every sheet",
            PropTab::Notes => "the project's notes",
            PropTab::Phases => "the project through time",
            PropTab::Modifiers => "how its shape is changed",
            PropTab::Profile => "the profile in front",
            PropTab::Master => "the master in front",
            PropTab::Pattern => "the pattern in front",
            PropTab::Log => "what happened",
            PropTab::Tree => "how it grows",
            PropTab::Ground => "how it is shaped",
            PropTab::Earthwork => "how it meets the ground",
            PropTab::Road => "how it is set out",
            PropTab::Drain => "how it is laid",
            PropTab::Path => "how it is laid",
            PropTab::Tank => "how it is built",
            PropTab::Property => "the lot and its rules",
            PropTab::Paving => "how it is laid",
            PropTab::Parking => "its bays and aisles",
            PropTab::Sheet => "its paper and title block",
        }
    }

    fn label(self) -> &'static str {
        match self {
            PropTab::Select => "Select",
            PropTab::Next => "Next element",
            PropTab::Element => "Element",
            PropTab::Joins => "Junctions and booleans",
            PropTab::Graphics => "Graphics",
            PropTab::Data => "Data (IFC)",
            PropTab::View => "View",
            PropTab::Trace => "Trace and reference",
            PropTab::Model3d => "3D view",
            PropTab::Visuals => "Visuals",
            PropTab::Render => "Render",
            PropTab::Project => "Project",
            PropTab::Notes => "Notes and to-dos",
            PropTab::Phases => "Phases",
            PropTab::Modifiers => "Modifiers",
            PropTab::Profile => "Profile",
            PropTab::Master => "Master",
            PropTab::Pattern => "Pattern",
            PropTab::Log => "Log",
            PropTab::Tree => "Tree",
            PropTab::Ground => "Ground",
            PropTab::Earthwork => "Earthwork",
            PropTab::Road => "Road",
            PropTab::Drain => "Drain",
            PropTab::Path => "Path",
            PropTab::Tank => "Tank",
            PropTab::Property => "Property",
            PropTab::Paving => "Paving",
            PropTab::Parking => "Parking",
            PropTab::Sheet => "Sheet",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            PropTab::Select => "square-dashed-mouse-pointer",
            PropTab::Next => "plus",
            PropTab::Element => "box",
            PropTab::Joins => "join",
            PropTab::Graphics => "palette",
            // REV04's lines of what the file says, not a table (MU58)
            PropTab::Data => "ifc-data",
            PropTab::View => "eye",
            PropTab::Trace => "underlay",
            PropTab::Model3d => "view-3d",
            PropTab::Visuals => "visuals",
            PropTab::Render => "image",
            PropTab::Project => "house",
            PropTab::Notes => "sticky-note",
            PropTab::Phases => "layers",
            PropTab::Modifiers => "wrench",
            PropTab::Profile => "profile",
            PropTab::Master => "sheet-template",
            PropTab::Pattern => "hatch",
            PropTab::Log => "log",
            PropTab::Tree => "tree-deciduous",
            PropTab::Ground => "terrain",
            PropTab::Earthwork => "pad",
            PropTab::Road => "road",
            PropTab::Drain => "drain",
            PropTab::Path => "site-path",
            PropTab::Tank => "tank",
            PropTab::Property => "land-plot",
            PropTab::Paving => "paving",
            PropTab::Parking => "parking",
            PropTab::Sheet => "file-text",
        }
    }
}

/// What a modifier does and how to use it: the ? on its card (MU39).
fn modifier_help(k: &creabim_core::ModifierKind) -> &'static str {
    use creabim_core::ModifierKind as K;
    match k {
        K::Array { .. } => "Repeats the shape in a row: bays, posts, rows of seats. Fit a number of copies or as many as fit a length; step by a distance or by its own size (1 puts copies end to end). Each copy can turn or grow a little more than the last.",
        K::RadialArray { .. } => "Repeats the shape round a centre: columns round a rotunda, seats round a table. Set the centre with its X and Y, or Pick and click it in a view. Turn each copy to face the centre, or keep them all facing one way.",
        K::PathArray { .. } => "Copies the shape to evenly spaced points along a sketch or a guide, its own origin on each: balusters, posts, bollards along a curve. The path is copied in: change it and pick it again.",
        K::Mirror { .. } => "A mirrored copy across a plane: at its middle for a symmetric shape, at its edge for a twin.",
        K::Panelize { .. } => "Cuts its upright rectangular faces into cladding panels with joints between, as near the size asked as fits evenly. Proud by sets the panels out from the face.",
        K::Louvres { .. } => "Fins or slats on the sides you pick, for shade. N is the mesh's +Y side before it is turned. Angle tilts each blade.",
        K::Bevel { .. } => "Rounds (fillet) or cuts (chamfer) the upright corners of a shape drawn from a plan outline and extruded: a slab, a counter, a mass. Outside corners, inside corners or both.",
        K::Shell { .. } => "Hollows it to a thickness. Closed keeps a sealed shell round a void (a coconut); open the top for a tub, a planter or a pool, the bottom for a cover, both for a tube or a chimney. To cut a door into a closed shell, add a Boolean.",
        K::Solidify { .. } => "Gives a surface a thickness, behind or in front of its faces: a roof sheet, a canopy, a wall drawn as one face.",
        K::Slice { .. } => "Cuts a mass into floors at heights above its own base point, with a gap between them: a slab's depth or a shadow joint. Starts at the project's levels.",
        K::Taper { .. } => "Narrows (or flares) the shape going up: 1 keeps the top as wide as the bottom, 0.5 halves it.",
        K::Twist { .. } => "Turns the top about the middle, the bottom staying put. Give it more floors (Slice, or Smooth set to Split) for an even curve.",
        K::Subdivision { .. } => "Smooth rounds the shape into soft, organic forms; Split only cuts its faces into smaller ones, keeping its shape, for Twist and Taper to bend.",
    }
}

/// With CREABIM_PERF set, how long a frame (or a part of one) took, on
/// stderr, when it took more than 8 ms (MU37: the Profile view felt late).
///
/// Steps nest: one timed inside another is taken off the outer one's own
/// time, and a slow frame is logged as what its time went on (see
/// [`crate::actlog::step_close`]).
pub(crate) struct Perf(std::time::Instant, &'static str, usize);

impl Perf {
    pub(crate) fn new(what: &'static str) -> Self {
        let place = crate::actlog::step_open(what);
        Perf(std::time::Instant::now(), what, place)
    }
}

impl Drop for Perf {
    fn drop(&mut self) {
        let ms = self.0.elapsed().as_secs_f64() * 1000.0;
        // every slow step is kept for problem reports (MU40: lag measured,
        // not only told): a line in the log, at most one a second for each
        // kind, and its count and worst in the timing table
        crate::actlog::slow(self.1, ms);
        crate::actlog::step_close(self.2, self.1, ms);
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *ON.get_or_init(|| std::env::var_os("CREABIM_PERF").is_some()) {
            static MIN: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
            let min = *MIN.get_or_init(|| {
                std::env::var("CREABIM_PERF_MS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(8.0)
            });
            if ms > min {
                eprintln!("{}: {ms:.1} ms", self.1);
            }
        }
    }
}

impl CreaBimApp {
    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|input| input.viewport().close_requested()) {
            return;
        }
        self.time_store();
        if !self.close_ok && (self.unsaved() || self.save_as_dialog.is_some()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.save_as_dialog.is_none() {
                self.close_ask = true;
                #[cfg(target_os = "macos")]
                mac_menu::reveal_window_if_hidden(ctx);
            }
        }
    }
}

impl eframe::App for CreaBimApp {
    #[cfg(target_os = "macos")]
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Native menu commands and close requests also arrive while window painting is paused.
        self.handle_close_request(ctx);
        self.dispatch_mac_menu(ctx);
    }

    /// Keys typed while a web pane had the keyboard reach the views.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if self.shell.feed_keys(raw_input) {
            ctx.request_repaint();
        }
        // an export or import card takes Enter and Esc before anything else
        // can (an open popup's Enter walks a menu, a selection's edits it)
        self.mark_keys(raw_input);
        self.palette_keys(raw_input);
        // Tab while a line is drawn moves the tracker to its next field
        // (2.10): taken here, before egui moves the keyboard focus with it
        let hosting = crate::memberstool::hosted(self.plan.tool) && self.plan.pending.is_none();
        if (self.plan.pending.is_some() || hosting) && !ctx.egui_wants_keyboard_input() {
            let mut presses = 0;
            raw_input.events.retain(|e| match e {
                egui::Event::Key {
                    key: egui::Key::Tab,
                    modifiers,
                    pressed,
                    ..
                } if modifiers.is_none() => {
                    presses += usize::from(*pressed);
                    false
                }
                _ => true,
            });
            for _ in 0..presses {
                if hosting {
                    // a host tool: the next host under the pointer
                    self.plan.host_cycle += 1;
                    ctx.request_repaint();
                } else {
                    self.plan.tracker.next_field();
                }
            }
        }
        raw_input.safe_area_insets =
            Some(egui::SafeAreaInsets(band_insets(self.shell.web.is_some())));
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // it opens maximised, so it never runs off a small or scaled
        // screen: asked once, of the window as it stands (MU102)
        {
            use std::sync::atomic::{AtomicU32, Ordering};
            use std::sync::OnceLock;
            // frames painted, and those painted since it stood maximised
            static FRAMES: AtomicU32 = AtomicU32::new(0);
            static SETTLED: AtomicU32 = AtomicU32::new(0);
            static BEGAN: OnceLock<std::time::Instant> = OnceLock::new();
            static FULL_AT: OnceLock<std::time::Instant> = OnceLock::new();
            let n = FRAMES.fetch_add(1, Ordering::Relaxed);
            let began = *BEGAN.get_or_init(std::time::Instant::now);
            if n == 0 {
                // unseen until it stands at full size with its panels
                // drawn there: it showed small with the project in it,
                // then jumped to full size with blank patches (MU103)
                start_cloak::set(&*frame, true);
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            }
            let seen = SETTLED.load(Ordering::Relaxed);
            if seen < start_cloak::DONE {
                let full = ui.ctx().input(|i| i.viewport().maximized == Some(true));
                let settled = if full || seen > 0 { seen + 1 } else { 0 };
                // and a moment for the panels' pages to paint at that
                // size: they are windows of their own, a frame or two
                // behind
                let since_full = if settled > 0 {
                    FULL_AT
                        .get_or_init(std::time::Instant::now)
                        .elapsed()
                        .as_millis()
                } else {
                    0
                };
                // (never kept unseen for long, whatever the system says)
                let late = began.elapsed().as_millis() > 4000;
                if (settled >= start_cloak::FRAMES && since_full >= 120) || late {
                    crate::actlog::line(format!(
                        "start: shown after {} ms, {} frames at full size{}",
                        began.elapsed().as_millis(),
                        settled,
                        if late { " (not waited for longer)" } else { "" }
                    ));
                    start_cloak::set(&*frame, false);
                    SETTLED.store(start_cloak::DONE, Ordering::Relaxed);
                } else {
                    SETTLED.store(settled, Ordering::Relaxed);
                    ui.ctx().request_repaint();
                }
            }
        }
        // a render being made at Output's size: kept when it is done
        self.renders_poll(ui.ctx());
        // what a roll of the wheel does, for every view (issue #78)
        crate::planview::WheelMode::of(&self.shell.settings.wheel).set();
        // a long wait since the last frame while the pointer is held is
        // lag too, outside the frame: painting, the graphics card
        if ui.ctx().input(|i| i.pointer.any_down()) {
            if let Some(ms) = crate::actlog::since_frame() {
                crate::actlog::slow("between frames", ms);
            }
        }
        let _perf = Perf::new("frame");
        let _ps = Perf::new("frame: start");
        DRAWINGS_DARK.store(
            self.dark_paper(ui.ctx()),
            std::sync::atomic::Ordering::Relaxed,
        );
        // the project's drawing standard, for the size and scale pickers
        crate::sheetview::set_standard(ui.ctx(), creabim_core::standards::of(self.doc.model()));
        // the keymap reads this frame's keys before anything else does
        {
            let s = &self.shell.settings;
            let binds = crate::keymap::bindings(&s.background, &s.keymap);
            crate::keymap::process(ui.ctx(), &binds, s.background != "creabim");
        }
        // project time counts, and keeps its book beside the project
        self.time_frame(ui.ctx());
        // letters typed so far of a two-letter key (MV), as Revit shows them
        if let Some(t) = crate::keymap::pending_text(ui.ctx()) {
            self.status = format!("{t}… (keys typed in a row)");
        }
        // a shape selected with something it can't work on: say why the
        // strip offers no Add or Subtract (Booleans, decision 4)
        let sel = self.selection.all();
        if sel != self.solid_seen {
            if let Some(why) = self.solid_op_blocker() {
                self.status = why;
            }
            self.solid_seen = sel;
        }
        // a building being moved takes its wall and slab cuts along rather
        // than making them again on every step; once the move ends, what
        // was shown that way is cut afresh where it stands, as it always was
        // the scene before a pointer edit changes anything: what a shape
        // that stands alone is built again into (see rebuild_geometry)
        // kept for as long as the edit works on the same elements: a pull
        // brought back to where it began has changed nothing for a moment,
        // and the scene of that moment may not be built (it was dropped
        // there, and every step after it built the whole scene, MU46)
        // a grip dragged counts as a tool does: in Edit mode the wires
        // went with the pointer while the solid waited for a worker's
        // scene, and the two came apart (MU49)
        match self.live_edit() {
            Some((now, applied)) => {
                let kept = self.live_base.as_ref().is_some_and(|(ids, _)| *ids == now);
                // shapes that stand alone: after the edit their snaps only
                // are made again (see refresh_snaps_3d)
                let model = self.doc.model();
                let shapes = now.iter().all(|id| {
                    matches!(
                        model.get(*id).map(|e| &e.kind),
                        Some(ElementKind::FreeMesh(_) | ElementKind::Object(_))
                    ) && creabim_geom::stands_alone(model, *id)
                });
                self.snaps_touch = match (&self.snaps_3d, shapes) {
                    (Some((made, _, _)), true) => {
                        Some((now.clone(), *made, model.drawing_revision(), true))
                    }
                    _ => None,
                };
                if !kept {
                    self.static_3d = None;
                    self.live_base = (!applied)
                        .then(|| creabim_geom::shared_scene_ready(self.doc.model()))
                        .flatten()
                        .map(|s| (now, s));
                }
            }
            None => {
                self.live_base = None;
                self.static_3d = None;
                // the edit is over: the model as it left it
                if let Some((_, _, model, live)) = &mut self.snaps_touch {
                    if *live {
                        *model = self.doc.model().drawing_revision();
                        *live = false;
                    }
                }
            }
        }
        // Pull stays in hand face after face, so picking another tool is
        // what puts it down: it kept the views while Line or Rectangle
        // was the tool, and nothing could be drawn
        if self.plan.tool != Tool::Select {
            let pull = crate::modal::Mode::Pull;
            if self.plan.modal.as_ref().is_some_and(|m| m.mode == pull) {
                if let Some(mut m) = self.plan.modal.take() {
                    m.discard(&mut self.doc);
                }
            }
            if matches!(self.plan.armed, Some((m, _)) if m == pull) {
                self.plan.armed = None;
            }
        }
        if creabim_geom::scene::carry_cuts(self.plan.modal.is_some()) {
            self.doc.redraw();
        }
        // while a drag is live the ground waits: only the outline of what
        // is dragged moves, and the ground is worked out on the drop
        let dragging = self.plan.modal.is_some()
            || ui
                .ctx()
                .input(|i| i.pointer.primary_down() && i.pointer.is_decidedly_dragging());
        if creabim_geom::earth::hold(dragging) {
            self.doc.redraw();
        }
        // how the project reads its site levels, for the Site tools' strip
        self.plan.parts.level_shift = creabim_core::units::level_shift(self.doc.model());
        self.plan.parts.level_unit = creabim_core::units::level_unit(self.doc.model());
        #[cfg(not(target_os = "macos"))]
        self.handle_close_request(ui.ctx());
        let ctx = ui.ctx().clone();
        // on the drawing area a bar's tooltip goes: the bar never hears the
        // pointer leave it. Back from a bar (or after a pause in the moves
        // here, in case leaving went unreported) the bars forget the
        // button they last showed, so it shows again on the next hover
        let (events, now) = ctx.input(|i| (i.events.clone(), i.time));
        for e in events {
            match e {
                egui::Event::PointerGone => self.pointer_away = true,
                egui::Event::PointerMoved(_) => {
                    self.shell.drop_web_tip();
                    if self.pointer_away || now - self.pointer_seen > TIP_PAUSE {
                        if let Some(web) = &self.shell.web {
                            web.run_all("window.CB && CB.away && CB.away()");
                        }
                    }
                    self.pointer_away = false;
                    self.pointer_seen = now;
                }
                _ => {}
            }
        }
        crate::trace::hold(ctx.input(|i| i.pointer.any_down()));
        let m = ctx.input(|i| i.modifiers);
        shell::MODS.store(
            m.shift as u8 | (m.command as u8) << 1 | (m.alt as u8) << 2,
            std::sync::atomic::Ordering::Relaxed,
        );
        // the colour of what is worked on: orange, or the theme's accent;
        // the 3D view's highlight is built again when it changes (MU39)
        let ink = if self.shell.settings.interaction == "theme" {
            self.shell.theme.selection()
        } else {
            crate::theme::ORANGE
        };
        crate::planview::set_plan_look(
            self.shell.settings.plan_grid,
            self.shell.settings.axis_lines,
        );
        if ink != crate::theme::interaction() {
            crate::theme::set_interaction(ink);
            self.geometry_key = None;
            self.selection_key = None;
        }
        // the ghost the pointer's view drew last frame, for the others
        let at = |g: &Option<(Option<usize>, crate::ghost::Ghost)>| {
            g.as_ref()
                .and_then(|(_, g)| g.parts.first()?.points.first().copied())
        };
        let moved = at(&self.plan.ghost_now) != at(&self.plan.ghost_shared);
        self.plan.ghost_shared = self.plan.ghost_now.take();
        // the other views catch up once the pointer stops
        if moved {
            ctx.request_repaint();
        }
        drop(_ps);
        // Divide's marks, for every view to draw this frame (MU38)
        let _pd = Perf::new("pre-frame");
        (self.plan.divide_marks, self.plan.divide_labels) = self.divide_marks();
        self.plan.divide_guides = self.divide.s.make == creabim_core::divide::Make::Guides;
        // while a point is asked for, the selection stays as it was
        if self.plan.point_pick {
            match &self.pick_selection {
                Some(s) if *s != self.selection => self.selection = s.clone(),
                Some(_) => {}
                None => self.pick_selection = Some(self.selection.clone()),
            }
        } else {
            self.pick_selection = None;
        }
        // Split's clicks, else what the modifier being set shows, and a
        // centre picked for it
        if self.split.is_some() {
            self.split_steps(&ctx);
        } else {
            self.take_picked_centre();
            self.plan.mod_guides = self.modifier_guides();
        }
        drop(_pd);
        let _pt = Perf::new("frame: title, units and step");
        // R draws a rectangle while sketching, not a rotate (MU38)
        crate::sketch::set_sketching(&ctx, self.plan.tool == Tool::Sketch);
        // the activity log: what the status bar said (it names every edit
        // and undo) and the tool, when they change
        if self.status != self.logged.0 {
            crate::actlog::line(format!("status: {}", self.status));
            self.logged.0 = self.status.clone();
        }
        if Some(self.plan.tool) != self.logged.1 {
            crate::actlog::line(format!("tool: {:?}", self.plan.tool));
            self.logged.1 = Some(self.plan.tool);
        }
        let title = window_title(&self.path, self.unsaved());
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        crate::units::set(self.doc.model().units());
        // the step drags round to: none while Alt is held (2.10)
        let live = self.plan.snap.live(ctx.input(|i| i.modifiers.alt));
        crate::snaps::set_current_step(if live.allows(crate::snaps::SnapKind::Grid) {
            live.step_len()
        } else {
            0.0
        });
        crate::snaps::set_current_marks(live.marks_len());
        drop(_pt);
        {
            let _p = Perf::new("frame: prompts and keys");
            self.step_prompt(&ctx);
            self.workspace_prompt(&ctx);
            self.area_keys(&ctx);
            self.shortcuts(&ctx);
            self.likeness_frame(&ctx);
        }
        {
            let _p = Perf::new("frame: edit mode and spotlight");
            self.keep_edit_mode();
            self.refresh_spotlight();
        }
        let _pf = Perf::new("frame: dropped files");
        let dropped: Vec<String> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().display().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        });
        for path in dropped {
            // a survey or map file goes to Import survey
            if groundui::is_survey_file(&path) {
                self.open_survey_file(&path);
                continue;
            }
            // a dropped material file joins this project, which keeps its path
            if !materials::is_material_file(&path) {
                self.path = path.clone();
            }
            self.open_path(&path);
        }
        drop(_pf);
        if self.live_reload {
            let _p = Perf::new("frame: watch files");
            for f in self.watch.changed(self.doc.model()) {
                self.reload(&f);
            }
            if !crate::files::linked_files(self.doc.model()).is_empty() {
                ctx.request_repaint_after(std::time::Duration::from_secs(1));
            }
        }

        let _pw = Perf::new("web_panels");
        let web = self.web_panels(ui, frame);
        drop(_pw);
        #[cfg(feature = "live-mcp")]
        self.poll_live_mcp(&ctx);
        // the panes say again this frame what the one under the pointer does
        self.plan.hint = None;
        self.rect_3d = None;
        self.rect_2d = None;
        self.plan.web_menus = web;
        crate::slabtool::set_options_in_top_bar(&ctx, web);
        self.shell.menu_3d = false;
        if !web {
            egui::Panel::top("menu").show(ui, |ui| {
                self.classic_menu(ui);
                self.toolbar(ui);
                crate::props::flow(ui, |ui| {
                    let some = self.selection.is_some();
                    if let Some(c) = crate::commands::modify_bar(ui, some) {
                        self.run_command(&ctx, c, None);
                    }
                });
            });
            egui::Panel::bottom("status").show(ui, |ui| {
                crate::props::flow(ui, |ui| {
                    ui.label(&self.status);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(format!("{} elements", self.element_count()));
                    });
                });
            });
            egui::Panel::left("outliner")
                .default_size(220.0)
                .show(ui, |ui| self.outliner(ui));
        }
        // to the end of the frame from here: Properties, the areas and on
        let _pr = Perf::new("frame: properties on");
        let right_folded = self.shell.settings.right_folded;
        let right = egui::Panel::right(if right_folded {
            "properties_rail"
        } else {
            "properties"
        })
        .default_size(if right_folded {
            40.0
        } else {
            self.shell.settings.right_width
        })
        .size_range(if right_folded {
            40.0..=40.0
        } else {
            220.0..=640.0
        })
        .frame(
            // flush at the left, where the tab column is (MU22)
            egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin {
                left: 0,
                right: 4,
                top: 0,
                bottom: 4,
            }),
        )
        .show(ui, |ui| {
            // the panel keeps the width it was given: what is wider
            // wraps, folds into one column or is clipped, and never
            // pushes the panel wider as the selection changes
            let rect = ui.available_rect_before_wrap();
            let clip = rect.expand2(egui::vec2(6.0, 0.0)).intersect(ui.clip_rect());
            let folded = self.shell.settings.right_folded;
            let edge = ui.max_rect();
            if fold_handle(
                ui.ctx(),
                "props",
                egui::pos2(edge.left() - 11.0, edge.center().y - 23.0),
                folded,
                true,
                if folded {
                    "Open Properties (Ctrl+])"
                } else {
                    "Fold Properties to its tabs (Ctrl+])"
                },
            ) {
                self.shell.settings.right_folded = !folded;
                self.shell.settings.store();
            }
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            inner.set_clip_rect(clip);
            inner.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            // one grid for every row (A4-1): controls as tight as
            // Blender's (MU22), the scroll bar in a gutter of its own
            inner.spacing_mut().interact_size.y = 24.0;
            // text a size down, as Blender's panels (MU22): rows fit
            // on one line at the panel's usual width
            for (style, font) in inner.style_mut().text_styles.iter_mut() {
                font.size = match style {
                    egui::TextStyle::Small => 10.5,
                    egui::TextStyle::Heading => 14.0,
                    _ => (font.size - 1.5).clamp(11.0, 12.5),
                };
            }
            inner.spacing_mut().item_spacing = egui::vec2(4.0, 3.0);
            inner.spacing_mut().button_padding = egui::vec2(6.0, 2.0);
            // editable fields (dropdowns, numbers, text) in the field
            // colour, a tone apart from the panel, as Blender does, so
            // they read at a glance as things you can change (MU20)
            {
                let p = crate::theme::Palette::of(ui.ctx());
                let w = &mut inner.visuals_mut().widgets;
                w.inactive.weak_bg_fill = p.field;
                w.inactive.bg_stroke = egui::Stroke::new(1.0, p.edge);
            }
            let mut bar = egui::style::ScrollStyle::solid();
            bar.bar_width = 6.0;
            bar.bar_inner_margin = 4.0;
            inner.spacing_mut().scroll = bar;
            // as Blender's editor (MU22): the tab column down the left
            // edge, a tone darker, then the pinned header and the
            // tab's sections, which scroll
            inner.set_max_width(rect.width());
            let tabs = {
                let _p = Perf::new("properties: tabs");
                self.prop_tabs()
            };
            // a placing tool just picked opens its Next tab; a first
            // selection opens Element; otherwise the tab stays
            let now = (self.plan.tool, self.selection.active.is_some());
            if now != self.prop_seen {
                if now.0 != self.prop_seen.0 && now.0 != Tool::Select && !now.1 {
                    self.prop_tab = PropTab::Next;
                } else if now.1 && !self.prop_seen.1 && self.prop_tab != PropTab::Select {
                    // (a selection made from the Select tab stays there); a
                    // view picked shows how it's drawn (View tabs REV00, 3)
                    let view = self.selection.active.is_some_and(|a| {
                        matches!(
                            self.doc.model().get(a).map(|e| &e.kind),
                            Some(ElementKind::View(_))
                        )
                    });
                    self.prop_tab = if view {
                        PropTab::View
                    } else {
                        PropTab::Element
                    };
                }
                self.prop_seen = now;
            }
            // the view in front leads, as Blender's editors do (MU40: a
            // sheet's settings showed over the Masters view)
            let front = self.areas.root.areas().get(self.areas.focus).copied();
            if front != self.prop_front {
                match front {
                    Some(crate::areas::Shows::Masters) => self.prop_tab = PropTab::Master,
                    Some(crate::areas::Shows::Patterns) => self.prop_tab = PropTab::Pattern,
                    Some(crate::areas::Shows::Profiles) => self.prop_tab = PropTab::Profile,
                    Some(crate::areas::Shows::Log) => self.prop_tab = PropTab::Log,
                    Some(crate::areas::Shows::Renders) => self.prop_tab = PropTab::Render,
                    // its settings are Time and fees, in the Project tab
                    Some(crate::areas::Shows::Time) => self.prop_tab = PropTab::Project,
                    // a sheet clicked into, nothing on it picked: the sheet
                    Some(crate::areas::Shows::View(_))
                        if self.selection.active.is_none() && self.sheet_in_front().is_some() =>
                    {
                        self.prop_tab = PropTab::Sheet
                    }
                    _ => {}
                }
                self.prop_front = front;
            }
            if !tabs.contains(&self.prop_tab) {
                // the first tab after Select: it is chosen, never fallen on
                self.prop_tab = tabs
                    .iter()
                    .copied()
                    .find(|t| *t != PropTab::Select)
                    .unwrap_or(tabs[0]);
            }
            // a tool just picked opens its Next tab; a new selection
            // with no tab of its own opens Element
            inner.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let _p = Perf::new("properties: tab strip");
                self.prop_strip(ui, &tabs, rect.height());
                if self.shell.settings.right_folded {
                    return;
                }
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 3.0);
                    let w = ui.available_width();
                    // the header as wide as the boxes under it, which
                    // leave the scroll bar its gutter: wider, its
                    // fields and Delete ran off the panel (MU24)
                    ui.set_max_width(w - 16.0);
                    ui.add_space(4.0);
                    let _p = Perf::new("properties: header");
                    self.properties_header(ui);
                    drop(_p);
                    let scrolled = egui::ScrollArea::vertical()
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            ui.set_max_width(w - 16.0);
                            crate::props::boxes(ui, true);
                            self.properties(ui);
                            crate::props::close_box(ui);
                            crate::props::boxes(ui, false);
                        });
                    crate::props::note_scroll(
                        ui.ctx(),
                        scrolled.state.offset.y,
                        scrolled.content_size.y - scrolled.inner_rect.height(),
                    );
                });
            });
            ui.allocate_rect(rect, egui::Sense::hover());
        })
        .response
        .rect;
        // remember a new width once the drag is over
        let width = right.width().round();
        if (width - self.shell.settings.right_width).abs() >= 1.0
            && !ui.input(|i| i.pointer.any_down())
        {
            self.shell.settings.right_width = width;
            self.shell.settings.store();
        }

        // to the end of the frame from here: the areas, then what follows
        let _pa = Perf::new("frame: areas on");
        egui::CentralPanel::default().show(ui, |ui| {
            let full = ui.available_rect_before_wrap();
            self.mat.area = Some(full);
            let plan_bg = Color32::from_rgb(252, 252, 250);
            match self.placement {
                PlanPlacement::Docked => self.show_areas(ui, full, frame, plan_bg),
                PlanPlacement::Overlay => {
                    self.show_3d(ui, full, frame);
                    let inset = Rect::from_min_size(
                        full.right_top() + Vec2::new(-full.width() * 0.42 - 12.0, 12.0),
                        Vec2::new(full.width() * 0.42, full.height() * 0.45),
                    );
                    self.show_plan(
                        ui,
                        inset,
                        Color32::from_rgba_unmultiplied(252, 252, 250, 225),
                    );
                    ui.painter().rect_stroke(
                        inset,
                        4.0,
                        egui::Stroke::new(1.0, Color32::from_gray(160)),
                        egui::StrokeKind::Outside,
                    );
                }
                PlanPlacement::Window | PlanPlacement::Hidden => self.show_3d(ui, full, frame),
            }
            ui.allocate_rect(full, Sense::hover());
        });

        if let Some(s) = self.plan.status.take() {
            self.status = s;
        }
        for (c, at) in std::mem::take(&mut self.plan.commands) {
            self.run_command(&ctx, c, at);
        }
        if std::mem::take(&mut self.plan.want_levels) {
            self.open_levels();
        }
        // a view mark double-clicked: its view opens
        if let Some(v) = self.plan.want_view.take() {
            self.active_view = Some(v);
        }
        if std::mem::take(&mut self.plan.want_catalogue) {
            self.catalogue.open = true;
        }
        // a box selection that took more than one kind: Keep only… is offered
        if std::mem::take(&mut self.plan.boxed) {
            self.after_box_selection();
        }
        // a box dragged with the Footing tool: footings under all it took
        let hosts = std::mem::take(&mut self.plan.want_footings);
        if !hosts.is_empty() {
            self.selection.clear();
            self.selection.extend(hosts, false);
            self.footings_under_selection();
        }
        let _pw = Perf::new("frame: windows");
        self.open_view_menu();
        // what the material cards asked for this frame
        crate::matcard::floating(&ctx, self.doc.model());
        for ask in crate::matcard::take() {
            match ask {
                crate::matcard::MatAsk::Edit(after) => {
                    if let Some(before) = self.doc.model().get(after.id).cloned() {
                        let r = self
                            .doc
                            .commit(Transaction::new("Material").replace(before, after));
                        if let Err(e) = r {
                            self.status = e.to_string();
                        }
                    }
                }
                crate::matcard::MatAsk::Open(id) => self.open_materials(Some(id)),
                crate::matcard::MatAsk::Insert(e) => {
                    if let Err(err) = self.doc.commit(Transaction::new("Copy material").insert(e)) {
                        self.status = err.to_string();
                    }
                }
                crate::matcard::MatAsk::CopyFor(element, from) => {
                    self.copy_material_for(element, from)
                }
            }
        }
        self.materials_window(&ctx);
        self.patterns_window(&ctx);
        self.libraries_window(&ctx);
        self.assemblies_window(&ctx);
        self.types_window(&ctx);
        self.type_preview_window(&ctx);
        self.catalogue_window(&ctx);
        self.levels_window(&ctx);
        self.level_copy_window(&ctx);
        self.styles_window(&ctx);
        self.templates_window(&ctx);
        self.units_window(&ctx);
        self.divide_window(&ctx);
        self.profiles_window(&ctx);
        self.settings_window(&ctx);
        // the paper follows the setting and the theme; a change redraws
        // the 3D view's grid and background
        let dark = self.dark_paper(&ctx);
        if dark != self.paper_is_dark {
            self.paper_is_dark = dark;
            self.geometry_key = None;
        }
        self.note_editor_window(&ctx);
        self.notes_window(&ctx);
        // (before the old Export and Import report windows, whose flags
        // open the cards instead)
        self.mark_take_flags();
        self.ifc_export_window(&ctx);
        self.reference_steps();
        self.note_pick_step(&ctx);
        self.calibrate_window(&ctx);
        self.sheets_export_window(&ctx);
        self.new_project_window(&ctx);
        self.splash_window(&ctx);
        self.marks_window(&ctx);
        self.graphic_styles_window(&ctx);
        self.ground_spacing_window(&ctx);
        self.import_report_window(&ctx);
        self.ground_window(&ctx);
        self.help_window(&ctx);
        self.report_window(&ctx);
        // last, as Save the changes: nothing drawn after it comes over it
        if !self.close_ask {
            self.mark_card(&ctx);
        }
        self.close_window(&ctx);
        drop(_pw);
        // Save as: its picker polled a frame at a time, never inside an
        // event being handled (#87: AppKit's modal loop aborted on macOS)
        self.save_as_step(&ctx, |start| {
            Box::pin(crate::files::project_save_dialog(start, frame))
        });
        // a plan in its own window keeps egui's menus
        self.plan.web_menus = false;

        if self.placement == PlanPlacement::Window {
            let mut close = false;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("creabim_plan"),
                egui::ViewportBuilder::default()
                    .with_title("CreaBIM · Plan")
                    .with_inner_size([760.0, 560.0]),
                |ui, _class| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        let rect = ui.available_rect_before_wrap();
                        self.show_plan(ui, rect, plan_bg_color());
                        ui.allocate_rect(rect, Sense::hover());
                    });
                    if ui.input(|i| i.viewport().close_requested()) {
                        close = true;
                    }
                },
            );
            if close {
                self.placement = PlanPlacement::Docked;
            }
        }
        #[cfg(target_os = "macos")]
        self.refresh_mac_menu(&ctx);
    }
}

/// The spacings the 3D ground grid offers, in metres.
const GROUND_SPACINGS: [f64; 6] = [0.25, 0.5, 1.0, 2.0, 5.0, 10.0];

/// The finest and the widest ground grid spacing that can be typed, metres.
const GROUND_SPACING_RANGE: (f64, f64) = (0.05, 100.0);

/// A usable ground grid spacing: a setting from the file, kept sensible.
/// Turn a pane's shapes painted since `from` over to dark paper: greys
/// (black ink, hatches, fills, the paper) invert, coloured marks and
/// pictures stay as they are.
fn darken_shapes(ctx: &egui::Context, layer: egui::LayerId, from: Option<egui::layers::ShapeIdx>) {
    let start = from.map_or(0, |i| i.0);
    let keep = crate::planview::kept_colours(ctx, layer);
    ctx.graphics_mut(|g| {
        let list = g.entry(layer);
        let end = list.next_idx().0;
        for i in start..end {
            if keep.iter().any(|(a, b)| i >= *a && i <= *b) {
                continue;
            }
            list.mutate_shape(egui::layers::ShapeIdx(i), |cs| {
                if let egui::Shape::Mesh(m) = &cs.shape {
                    if m.texture_id != egui::TextureId::default() {
                        return;
                    }
                }
                egui::epaint::shape_transform::adjust_colors(&mut cs.shape, |c| {
                    *c = paper_dark(*c);
                });
            });
        }
    });
}

/// The drawings are on dark paper this frame (the Drawing paper setting,
/// else the theme), for previews painted away from a view.
static DRAWINGS_DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Are the drawings on dark paper?
pub(crate) fn drawings_dark() -> bool {
    DRAWINGS_DARK.load(std::sync::atomic::Ordering::Relaxed)
}

/// Paint a small drawing (a pattern swatch, a tag, a symbol, a line style)
/// with `f` as on white paper, then turn it over to dark paper when the
/// drawings are dark, as the plans are (DS MU21; owner 2026-11-18: the
/// library and preview tiles glared white in a dark theme).
pub(crate) fn paint_drawing<R>(painter: &egui::Painter, f: impl FnOnce() -> R) -> R {
    let layer = painter.layer_id();
    let ctx = painter.ctx().clone();
    let from = ctx.graphics_mut(|g| g.entry(layer).next_idx());
    let out = f();
    if drawings_dark() {
        ctx.graphics_mut(|g| {
            let list = g.entry(layer);
            let end = list.next_idx().0;
            for i in from.0..end {
                list.mutate_shape(egui::layers::ShapeIdx(i), |cs| {
                    if let egui::Shape::Mesh(m) = &cs.shape {
                        if m.texture_id != egui::TextureId::default() {
                            return;
                        }
                    }
                    egui::epaint::shape_transform::adjust_colors(&mut cs.shape, |c| {
                        *c = paper_dark(*c);
                    });
                });
            }
        });
    }
    out
}

/// A colour on light paper, on dark paper: a grey turns over (white paper
/// to near black, black ink to near white), a colour is kept.
fn paper_dark(c: Color32) -> Color32 {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    let (hi, lo) = (r.max(g).max(b), r.min(g).min(b));
    if hi.saturating_sub(lo) > 28 {
        // a colour keeps its hue; a dark one (a blue annotation) is lifted
        // toward white until it reads on dark paper as it did on white (MU30)
        let lum = (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0;
        const WANT: f32 = 0.5;
        if lum >= WANT {
            return c;
        }
        let t = (WANT - lum) / (1.0 - lum);
        let up = |v: u8| (v as f32 + (255.0 - v as f32) * t).round() as u8;
        return Color32::from_rgba_unmultiplied(up(r), up(g), up(b), a);
    }
    let v = (r as f32 + g as f32 + b as f32) / 3.0;
    let w = (34.0 + (255.0 - v) * 0.82).round() as u8;
    Color32::from_rgba_unmultiplied(w, w, w.saturating_add(3), a)
}

/// How far the workplane's grid reaches from its origin before it has
/// faded into the background, metres.
const WORKPLANE_FADE: f64 = 4.0;

/// The 3D view's background, which the workplane's grid fades into.
const BACKGROUND_3D: [f32; 3] = [0.93, 0.94, 0.96];

/// The 3D view's background on dark paper (A4-3).
const BACKGROUND_3D_DARK: [f32; 3] = [0.12, 0.13, 0.15];

/// A face workplane's grid (MU18): soft green-grey lines at the ground
/// grid's spacing (every fifth a little stronger) that fade into the
/// background `reach` metres from its origin, and a small axis mark there.
/// Drawn just off the face, as line pairs for the 3D view.
fn workplane_lines(
    plane: Workplane,
    spacing: f64,
    reach: f64,
    background: [f32; 3],
) -> Vec<crate::render3d::Vertex> {
    let lift = plane.normal() * 0.003;
    let at = |u: f64, v: f64| {
        (plane.to_model(DVec2::new(u, v)) + lift)
            .as_vec3()
            .to_array()
    };
    let mix = |c: [f32; 3], t: f32| {
        [
            c[0] + (background[0] - c[0]) * t,
            c[1] + (background[1] - c[1]) * t,
            c[2] + (background[2] - c[2]) * t,
        ]
    };
    let minor = [0.62, 0.72, 0.60];
    let major = [0.40, 0.55, 0.38];
    // fine enough to read, coarse enough not to fill the face
    let step = spacing.clamp(0.1, 2.0).max(reach / 40.0);
    let n = (reach / step).floor() as i32;
    let piece = step.min(reach / 16.0);
    let mut out = Vec::new();
    for i in -n..=n {
        let k = i as f64 * step;
        let base = if i % 5 == 0 { major } else { minor };
        // each line in short pieces, so it fades along its length too
        let mut t = -reach;
        while t < reach - 1e-9 {
            let t1 = (t + piece).min(reach);
            for (a, b) in [((k, t), (k, t1)), ((t, k), (t1, k))] {
                let fade = |p: (f64, f64)| {
                    let d = (p.0 * p.0 + p.1 * p.1).sqrt() / reach;
                    mix(base, (d as f32).clamp(0.0, 1.0).powf(1.5))
                };
                if (a.0 * a.0 + a.1 * a.1).sqrt() > reach && (b.0 * b.0 + b.1 * b.1).sqrt() > reach
                {
                    continue;
                }
                out.push(line_vertex(at(a.0, a.1), fade(a)));
                out.push(line_vertex(at(b.0, b.1), fade(b)));
            }
            t = t1;
        }
    }
    // its origin and axes, small
    for (d, c) in [
        ((0.6, 0.0), [0.75, 0.30, 0.25]),
        ((0.0, 0.6), [0.30, 0.55, 0.30]),
    ] {
        out.push(line_vertex(at(0.0, 0.0), c));
        out.push(line_vertex(at(d.0, d.1), c));
    }
    out
}

fn ground_spacing(s: f64) -> f64 {
    if s.is_finite() {
        s.clamp(GROUND_SPACING_RANGE.0, GROUND_SPACING_RANGE.1)
    } else {
        1.0
    }
}

/// Framing that stays solid in X-ray: columns, beams, stairs and railings,
/// and masses that stand for them (a pergola's rafters as IfcBeam).
fn is_framing(k: &ElementKind) -> bool {
    match k {
        ElementKind::Column(_)
        | ElementKind::Beam(_)
        | ElementKind::Stair(_)
        | ElementKind::Railing(_) => true,
        ElementKind::FreeMesh(m) => matches!(
            m.ifc_class.as_str(),
            "IfcBeam" | "IfcColumn" | "IfcMember" | "IfcStair" | "IfcRailing"
        ),
        _ => false,
    }
}

/// How opaque walls, slabs, roofs and masses are in X-ray.
const XRAY_OPACITY: f32 = 0.18;

/// How strongly what is inside walls and slabs shows through them, in its
/// colours.
const BURIED_OPACITY: f32 = 0.5;
/// The colour of their edges then.
const XRAY_EDGE: [f32; 3] = [0.30, 0.34, 0.42];

fn plan_bg_color() -> Color32 {
    Color32::from_rgb(252, 252, 250)
}

fn line_vertex(pos: [f32; 3], color: [f32; 3]) -> Vertex {
    Vertex {
        pos,
        normal: [0.0, 0.0, 1.0],
        color,
        surface: crate::render3d::PLAIN,
    }
}

/// Triangle indices (into the flattened mesh) that make up polygon face `face`.
fn face_triangles(mesh: &creabim_core::EditMesh, face: usize) -> std::ops::Range<usize> {
    let start: usize = mesh.faces[..face.min(mesh.faces.len())]
        .iter()
        .map(|f| f.len() - 2)
        .sum();
    let len = mesh.faces.get(face).map_or(0, |f| f.len() - 2);
    start..start + len
}

/// The project's name without its ending, to name what is exported from
/// it. Only the ending goes (and `.creabim` before `.json`): a name with
/// dots of its own, as `House-1.1.1-test.creabim`, keeps them, where
/// cutting at the first dot suggested `House-1.ifc` (discussion 17).
fn stem(path: &str) -> String {
    let name = std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = match name.len().checked_sub(".creabim".len()) {
        Some(at)
            if at > 0
                && name.is_char_boundary(at)
                && name[at..].eq_ignore_ascii_case(".creabim") =>
        {
            name[..at].to_string()
        }
        _ => name,
    };
    if name.is_empty() {
        "project".to_string()
    } else {
        name
    }
}

#[cfg(test)]
mod face_plane_tests {
    /// An element drawn as several items (a terrain is) crashed the app
    /// when a face of a later item was picked: the triangle was looked up
    /// in the element's first item (issue 22).
    #[test]
    fn a_face_of_an_elements_second_item_gives_its_plane() {
        let model = creabim_core::sample::sample_house();
        let scene = creabim_geom::build_scene(&model);
        let slab = scene
            .iter()
            .find(|i| model.get(i.id).is_some_and(|e| e.name == "FF Slab"))
            .unwrap();
        let mut small = slab.clone();
        small.mesh.indices.truncate(3);
        for p in &mut small.mesh.positions {
            p[0] += 500.0;
        }
        let scene = vec![small, slab.clone()];
        let o = glam::DVec3::new(5.0, 3.0, 50.0);
        let plane = super::face_plane(&scene, o, -glam::DVec3::Z).expect("the slab's top");
        assert!((plane.origin.z - 3.0).abs() < 1e-3, "{}", plane.origin);
    }

    /// The report itself: a terrain added to the starter house, then the
    /// pointer anywhere over it in 3D.
    #[test]
    fn the_starter_house_on_a_terrain_gives_a_plane_under_any_ray() {
        use creabim_core::{Element, ElementKind};
        let house = creabim_core::starter::starter_house();
        let level = house
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Level(_)))
            .unwrap()
            .id;
        let mut site = creabim_core::site::Site::terrain(
            level,
            creabim_core::polygon::rectangle(
                glam::DVec2::new(-20.0, -20.0),
                glam::DVec2::new(40.0, 40.0),
            ),
            0.0,
            0.5,
        );
        if let creabim_core::site::SiteForm::Terrain { points, .. } = &mut site.form {
            points.push(glam::DVec3::new(-10.0, -10.0, 2.0));
        }
        let ground = Element::new("Ground", ElementKind::Site(site));
        let model =
            creabim_core::Model::from_elements(house.iter().cloned().chain([ground])).unwrap();
        let scene = creabim_geom::build_scene(&model);
        // (this plain terrain is one item; the elements with several are
        // the ones a triangle could be looked up wrongly in)
        let mut several: Vec<String> = Vec::new();
        for e in model.iter() {
            let n = scene.iter().filter(|i| i.id == e.id).count();
            if n > 1 {
                several.push(format!("{} x{n}", e.name));
            }
        }
        assert!(
            !several.is_empty(),
            "nothing here could be looked up wrongly"
        );
        let mut planes = 0;
        for x in (-19..40).step_by(4) {
            for y in (-19..40).step_by(4) {
                let o = glam::DVec3::new(x as f64 + 0.37, y as f64 + 0.41, 80.0);
                for d in [
                    -glam::DVec3::Z,
                    glam::DVec3::new(0.3, 0.2, -1.0).normalize(),
                ] {
                    planes += super::face_plane(&scene, o, d).is_some() as usize;
                }
            }
        }
        assert!(planes > 200, "{planes}");

        // Aim at the triangles that crashed: in an item after the first of
        // its element (furniture is one item per part), at an index past
        // the end of that first item's mesh. Each ray starts just off the
        // triangle, so it is what the ray meets first.
        let mut aimed = 0;
        for (at, item) in scene.iter().enumerate() {
            let Some(first) = scene.iter().position(|s| s.id == item.id) else {
                continue;
            };
            if first == at {
                continue;
            }
            let past = scene[first].mesh.triangle_count();
            let count = item.mesh.triangle_count();
            // a few per item: each ray tests the whole scene
            let step = (count.saturating_sub(past) / 3).max(1);
            for tri in (past..count).step_by(step) {
                let idx = &item.mesh.indices[tri * 3..tri * 3 + 3];
                let v =
                    |k: usize| glam::Vec3::from(item.mesh.positions[idx[k] as usize]).as_dvec3();
                let n = (v(1) - v(0)).cross(v(2) - v(0));
                if n.length() < 1e-6 {
                    continue;
                }
                let n = n.normalize();
                let c = (v(0) + v(1) + v(2)) / 3.0;
                let plane = super::face_plane(&scene, c + n * 1e-3, -n);
                assert!(plane.is_some(), "{} item {at} triangle {tri}", item.id.0);
                aimed += 1;
            }
        }
        assert!(
            aimed > 0,
            "no triangle past the end of its element's first item"
        );
    }
}

#[cfg(test)]
mod stem_tests {
    use super::{stem, with_extension};

    #[test]
    fn an_export_keeps_the_dots_of_the_projects_name() {
        assert_eq!(stem("CreaBIM-1.1.1-test.creabim"), "CreaBIM-1.1.1-test");
        assert_eq!(stem("house.creabim"), "house");
        assert_eq!(stem("house.creabim.json"), "house");
        assert_eq!(stem("house v2.1.CREABIM.json"), "house v2.1");
        assert_eq!(stem("from revit 2.4.ifc"), "from revit 2.4");
        assert_eq!(stem("untitled"), "untitled");
        assert_eq!(stem(""), "project");
        let out = with_extension("C:/work/CreaBIM-1.1.1-test.creabim", "ifc");
        assert!(out.ends_with("CreaBIM-1.1.1-test.ifc"), "{out}");
        // a sheet's or schedule's own name still goes after the project's
        let out = with_extension("C:/work/a.b.creabim", "A-101.svg");
        assert!(out.ends_with("a.b.A-101.svg"), "{out}");
    }
}

fn with_extension(path: &str, ext: &str) -> String {
    let p = std::path::Path::new(path);
    let dir = p.parent().map(|d| d.to_path_buf()).unwrap_or_default();
    dir.join(format!("{}.{ext}", stem(path)))
        .to_string_lossy()
        .into_owned()
}

/// Give every level a plan view so older files open with something to
/// show, and every ceiling its height tag in the ceiling plans (MU142:
/// a template or a new project from before they were tags).
fn with_plan_views(model: Model) -> Model {
    let model = match creabim_core::room::tag_ceilings(&model) {
        Some(tx) => {
            let mut doc = Document::from_model(model.clone());
            // a failed insert leaves the model as it was
            match doc.commit(tx) {
                Ok(()) => doc.model().clone(),
                Err(_) => model,
            }
        }
        None => model,
    };
    let planned: Vec<ElementId> = model
        .iter()
        .filter_map(|e| match &e.kind {
            ElementKind::View(View {
                kind: ViewKind::Plan { level, .. },
                ..
            }) => Some(*level),
            _ => None,
        })
        .collect();
    let mut tx = Transaction::new("Add plan views");
    let mut any = false;
    for (e, _) in model.levels() {
        if !planned.contains(&e.id) {
            tx = tx.insert(Element::new(
                format!("{} plan", e.name),
                ElementKind::View(View::plan(e.id)),
            ));
            any = true;
        }
    }
    if !any {
        return model;
    }
    let mut doc = Document::from_model(model);
    // a failed insert leaves the model as it was, which still opens
    let _ = doc.commit(tx);
    doc.model().clone()
}

/// A file's name without its folders.
/// Turn a plan to look up (a reflected ceiling plan) or down. Up raises a
/// cut below the door heads to 2.3 m once, so the walls are cut above the
/// doors; returns what to tell the user then (issue #70).
pub(crate) fn set_looks_up(v: &mut creabim_core::View, up: bool) -> Option<String> {
    v.ceiling_plan = up;
    let ViewKind::Plan { cut_height, .. } = &mut v.kind else {
        return None;
    };
    if up && *cut_height < CEILING_PLAN_CUT - 1e-6 {
        *cut_height = CEILING_PLAN_CUT;
        return Some(format!(
            "Looks up: the cut is raised to {}, above the door heads, so it reads as a ceiling plan. Undo puts it back.",
            crate::units::len(CEILING_PLAN_CUT)
        ));
    }
    None
}

/// Where a ceiling plan is cut: above the door heads, under the ceilings.
pub(crate) const CEILING_PLAN_CUT: f64 = 2.3;

/// What to call an element in a count.
pub(crate) fn kind_word(k: &ElementKind) -> &'static str {
    match k {
        ElementKind::Level(_) => "level",
        ElementKind::Grid(_) => "grid line",
        ElementKind::GridGroup(_) => "grid system",
        ElementKind::Wall(_) => "wall",
        ElementKind::Slab(_) => "slab",
        ElementKind::Opening(o) => match o.kind {
            creabim_core::OpeningKind::Door => "door",
            creabim_core::OpeningKind::Window => "window",
            creabim_core::OpeningKind::Hole => "wall opening",
        },
        ElementKind::FreeMesh(_) => "mass",
        ElementKind::Sketch(_) => "sketch",
        ElementKind::Guide(_) => "guide",
        ElementKind::Cut(_) => "cut",
        ElementKind::Dimension(_) => "dimension",
        ElementKind::Tag(_) => "tag",
        ElementKind::Note(_) => "note",
        ElementKind::TextStyle(_) => "text style",
        ElementKind::SheetTemplate(_) => "sheet template",
        ElementKind::Units(_) => "project units",
        ElementKind::Project(_) => "project information",
        ElementKind::Reference(_) => "reference",
        ElementKind::PaperMark(_) => "paper mark",
        ElementKind::MarkRules(_) => "mark numbering",
        ElementKind::StylePack(_) => "style pack",
        ElementKind::View(_) => "view",
        ElementKind::Sheet(_) => "sheet",
        ElementKind::Viewport(_) => "placed view",
        ElementKind::Material(_) => "material",
        ElementKind::Assembly(_) => "assembly",
        ElementKind::Column(_) => "column",
        ElementKind::Beam(_) => "beam",
        ElementKind::Roof(_) => "roof",
        ElementKind::Stair(_) => "stair",
        ElementKind::Railing(_) => "railing",
        ElementKind::CropBox(_) => "crop box",
        ElementKind::Object(_) => "object",
        ElementKind::Room(_) => "room",
        ElementKind::RoomSeparator(_) => "room separator",
        ElementKind::Join(_) => "junction rule",
        ElementKind::Detail(_) => "detail component",
        ElementKind::Ceiling(_) => "ceiling",
        ElementKind::ElementType(_) => "type",
        ElementKind::Shaft(_) => "shaft",
        ElementKind::Footing(_) => "footing",
        ElementKind::Part(p) if p.void => "void",
        ElementKind::Part(_) => "part",
        ElementKind::Site(s) if s.is_plant() => "plant",
        ElementKind::Site(_) => "site element",
    }
}

/// "wall" or "walls", for counts in messages.
pub(crate) fn plural(kind: &str, n: usize) -> String {
    let k = kind.to_lowercase();
    match (n, k.as_str()) {
        (1, _) => k,
        (_, "mass") => "masses".into(),
        _ => format!("{k}s"),
    }
}

/// The window's title: the project, a • while it has unsaved changes,
/// then the app, with its version on test builds (0.x) (A7-8).
fn window_title(path: &str, unsaved: bool) -> String {
    let name = std::path::Path::new(path)
        .file_stem()
        .map_or(path.to_string(), |n| n.to_string_lossy().into_owned());
    let version = env!("CARGO_PKG_VERSION");
    // a build being worked on says so in full: which version it leads to
    // (MU39: 1.0 showed while 1.0.1 was out and the next one in hand)
    let app = match version.split('.').collect::<Vec<_>>().as_slice() {
        _ if version.contains('-') => format!("CreaBIM {version}"),
        ["0", minor, ..] => format!("CreaBIM 0.{minor} beta"),
        [major, minor, ..] => format!("CreaBIM {major}.{minor}"),
        _ => "CreaBIM".to_string(),
    };
    format!("{}{name} — {app}", if unsaved { "• " } else { "" })
}

fn file_label(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map_or(path.to_string(), |n| n.to_string_lossy().into_owned())
}

/// How far guides reach each way in the 3D view.
const GUIDE_REACH_3D: f64 = 60.0;

/// A guide through `a` and `b`, dashed across the 3D view. It is sampled
/// along its length so the parts behind the camera drop out cleanly.
/// The plane of the face a ray hits, facing back along the ray.
fn face_plane(scene: &[creabim_geom::SceneItem], o: DVec3, d: DVec3) -> Option<Workplane> {
    let h = pick::pick(scene, o.as_vec3(), d.as_vec3())?;
    // the item hit, not the first of its element: a terrain is several
    // items, and a triangle of one is past the end of another (issue 22)
    let item = scene.get(h.item)?;
    let idx = item.mesh.indices.get(h.triangle * 3..h.triangle * 3 + 3)?;
    let v = |k: usize| glam::Vec3::from(item.mesh.positions[idx[k] as usize]);
    let mut n = (v(1) - v(0))
        .cross(v(2) - v(0))
        .normalize_or_zero()
        .as_dvec3();
    if n.dot(d) > 0.0 {
        n = -n;
    }
    Some(Workplane::facing(o + d * h.t as f64, n))
}

/// The guide line under the pointer in 3D, within 8 px of where it is
/// drawn, as its two points in plan; the nearest of several.
fn guide_near_3d(
    model: &creabim_core::Model,
    at: egui::Pos2,
    project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
) -> Option<(DVec2, DVec2)> {
    let mut best: Option<(f32, (DVec2, DVec2))> = None;
    for e in model.iter() {
        let ElementKind::Guide(g) = &e.kind else {
            continue;
        };
        let Some(d) = (g.b - g.a).try_normalize() else {
            continue;
        };
        if d.truncate().length() < 1e-3 {
            continue;
        }
        let m = (g.a + g.b) / 2.0;
        let pts: Vec<Option<egui::Pos2>> = (0..=120)
            .map(|i| project(m + d * (-GUIDE_REACH_3D + 2.0 * GUIDE_REACH_3D * i as f64 / 120.0)))
            .collect();
        for w in pts.windows(2) {
            if let [Some(s0), Some(s1)] = w {
                let v = *s1 - *s0;
                let t = ((at - *s0).dot(v) / v.length_sq().max(1e-6)).clamp(0.0, 1.0);
                let dist = (*s0 + v * t).distance(at);
                if dist < 8.0 && best.as_ref().is_none_or(|b| dist < b.0) {
                    best = Some((dist, (g.a.truncate(), g.b.truncate())));
                }
            }
        }
    }
    best.map(|b| b.1)
}

fn draw_guide_3d(
    painter: &egui::Painter,
    project: &dyn Fn(DVec3) -> Option<egui::Pos2>,
    a: DVec3,
    b: DVec3,
    stroke: egui::Stroke,
) {
    let Some(d) = (b - a).try_normalize() else {
        return;
    };
    let m = (a + b) / 2.0;
    let mut run: Vec<egui::Pos2> = Vec::new();
    let steps = 240;
    for i in 0..=steps {
        let t = -GUIDE_REACH_3D + 2.0 * GUIDE_REACH_3D * i as f64 / steps as f64;
        match project(m + d * t).filter(|p| painter.clip_rect().expand(200.0).contains(*p)) {
            Some(p) => run.push(p),
            None => {
                if run.len() > 1 {
                    painter.extend(egui::Shape::dashed_line(&run, stroke, 6.0, 4.0));
                }
                run.clear();
            }
        }
    }
    if run.len() > 1 {
        painter.extend(egui::Shape::dashed_line(&run, stroke, 6.0, 4.0));
    }
}

/// Does an element of this kind go out in an IFC export? Views, sheets,
/// annotation and settings don't.
fn in_ifc(kind: &ElementKind) -> bool {
    !matches!(
        kind,
        ElementKind::View(_)
            | ElementKind::Sheet(_)
            | ElementKind::Viewport(_)
            | ElementKind::Dimension(_)
            | ElementKind::Tag(_)
            | ElementKind::Note(_)
            | ElementKind::TextStyle(_)
            | ElementKind::Detail(_)
            | ElementKind::Join(_)
            | ElementKind::SheetTemplate(_)
            | ElementKind::Guide(_)
            | ElementKind::CropBox(_)
            | ElementKind::Units(_)
            | ElementKind::Sketch(_)
            | ElementKind::Cut(_)
            | ElementKind::PaperMark(_)
            | ElementKind::Project(_)
    )
}

/// A standard IFC set an element of some kind carries: its name, what it
/// holds in a line (to learn IFC by), whether the export writes it yet,
/// and its values as CreaBIM knows them.
struct IfcSet {
    name: &'static str,
    about: &'static str,
    written: bool,
    rows: Vec<crate::props::ReadRow>,
}

/// An element's measured amounts (`takeoff`), its layers summed: length,
/// area and volume, and its thickness. Worked out once a model revision.
#[derive(Clone, Copy, Default)]
struct Measured {
    length: f64,
    area: f64,
    volume: f64,
    thickness: f64,
}

fn element_quantity(ctx: &egui::Context, model: &Model, id: ElementId) -> Option<Measured> {
    type Cache = (
        u64,
        std::sync::Arc<std::collections::HashMap<ElementId, Measured>>,
    );
    let key = egui::Id::new("data-tab.takeoff");
    let rev = model.revision();
    let cached = ctx
        .data(|d| d.get_temp::<Cache>(key))
        .filter(|(r, _)| *r == rev);
    let map = match cached {
        Some((_, m)) => m,
        None => {
            let mut m: std::collections::HashMap<ElementId, Measured> = Default::default();
            for l in creabim_core::takeoff::lines(model) {
                let q = m.entry(l.element).or_default();
                q.length = q.length.max(l.quantity.length);
                q.area = q.area.max(l.quantity.area);
                q.volume += l.quantity.volume;
                q.thickness += l.thickness;
            }
            let m = std::sync::Arc::new(m);
            ctx.data_mut(|d| d.insert_temp(key, (rev, m.clone())));
            m
        }
    };
    map.get(&id).copied()
}

/// The standard property sets and quantity sets for an element (the Data
/// tab, Properties order mockup §3c), named so they can be learnt, each
/// with a few of its properties: what CreaBIM knows, "Not set" for what it
/// doesn't track yet, quantities measured as the take-off measures them.
fn ifc_sets(
    model: &Model,
    e: &creabim_core::Element,
    q: Option<Measured>,
) -> (Vec<IfcSet>, Vec<IfcSet>) {
    use crate::props::{Read, ReadRow};
    let unset = |names: &[&str]| -> Vec<ReadRow> {
        names
            .iter()
            .map(|n| ReadRow::new(*n, Read::Unset, "Not tracked by CreaBIM yet"))
            .collect()
    };
    let q = q.unwrap_or_default();
    let len = |n: &str, v: f64| {
        ReadRow::new(
            n,
            Read::Text(crate::units::len(v)),
            "Measured from the model",
        )
    };
    let area = |n: &str, v: f64| {
        ReadRow::new(
            n,
            Read::Text(crate::units::area(v)),
            "Measured from the model",
        )
    };
    let vol = |n: &str, v: f64| {
        ReadRow::new(
            n,
            Read::Text(crate::units::volume(v)),
            "Measured from the model",
        )
    };
    let text = |n: &str, v: &str| {
        if v.trim().is_empty() {
            ReadRow::new(n, Read::Unset, "Empty in CreaBIM")
        } else {
            ReadRow::new(n, Read::Text(v.to_string()), "As CreaBIM writes it")
        }
    };
    let planned = |name, about, rows| IfcSet {
        name,
        about,
        written: false,
        rows,
    };
    let written = |name, about, rows| IfcSet {
        name,
        about,
        written: true,
        rows,
    };
    // a common set, from the element's declared ratings and what is worked
    // out (Common properties REV00): written when it says anything
    let cm = creabim_core::common::common(model, e);
    let common = |name: &'static str, about: &'static str, names: &[&str]| -> IfcSet {
        use creabim_core::common::From;
        let d = &cm.declared;
        let flag = |v: Option<(bool, From)>, what: &str| match v {
            Some((b, f)) => ReadRow::new(
                what,
                Read::Text(if b { "Yes" } else { "No" }.into()),
                match f {
                    From::Own => "Set on this element",
                    From::Type => "Its type's",
                    From::WorkedOut => "Worked out from the model",
                },
            ),
            None => ReadRow::new(what, Read::Unset, "Not known: not written"),
        };
        let txt = |what: &str, v: &str| {
            if v.trim().is_empty() {
                ReadRow::new(what, Read::Unset, "Not declared: not written")
            } else {
                ReadRow::new(what, Read::Text(v.to_string()), "As declared")
            }
        };
        let rows: Vec<ReadRow> = names
            .iter()
            .map(|n| match *n {
                "IsExternal" => flag(cm.external, n),
                "LoadBearing" => flag(cm.load_bearing, n),
                "FireRating" => txt(n, &d.fire),
                "AcousticRating" => txt(n, &d.acoustic),
                "SecurityRating" => txt(n, &d.security),
                "ThermalTransmittance" => txt(
                    n,
                    &d.u_value.map(|u| format!("{u} W/m²K")).unwrap_or_default(),
                ),
                other => ReadRow::new(other, Read::Unset, "Not tracked by CreaBIM yet"),
            })
            .collect();
        let any = rows.iter().any(|r| !matches!(r.value, Read::Unset));
        IfcSet {
            name,
            about,
            written: any,
            rows,
        }
    };
    match &e.kind {
        ElementKind::Wall(w) => (
            vec![common(
                "Pset_WallCommon",
                "The set every IFC wall carries: is it external, load-bearing, its fire rating. Revit, ArchiCAD and Bonsai all read it by this name.",
                &["IsExternal", "LoadBearing", "FireRating", "AcousticRating", "ThermalTransmittance"],
            )],
            vec![written(
                "Qto_WallBaseQuantities",
                "Its length, height, width, area and volume, for take-off in other programs.",
                vec![
                    len("Length", q.length),
                    len("Height", w.height),
                    len("Width", w.thickness),
                    area("NetSideArea", q.area),
                    vol("NetVolume", q.volume),
                ],
            )],
        ),
        ElementKind::Slab(sl) => (
            vec![common(
                "Pset_SlabCommon",
                "Every IFC slab's set: external or not, load-bearing, fire rating, its pitch.",
                &["IsExternal", "LoadBearing", "FireRating", "AcousticRating", "ThermalTransmittance"],
            )],
            vec![written(
                "Qto_SlabBaseQuantities",
                "Its depth, area and volume.",
                vec![len("Depth", sl.thickness), area("NetArea", q.area), vol("NetVolume", q.volume)],
            )],
        ),
        ElementKind::Opening(o) if o.kind == creabim_core::OpeningKind::Door => (
            vec![common(
                "Pset_DoorCommon",
                "Every IFC door's set: external or not, fire rating, security, whether it is a fire exit.",
                &["IsExternal", "FireRating", "AcousticRating", "SecurityRating", "ThermalTransmittance"],
            )],
            vec![written(
                "Qto_DoorBaseQuantities",
                "Its width, height and area.",
                vec![len("Width", o.width), len("Height", o.height), area("Area", o.width * o.height)],
            )],
        ),
        ElementKind::Opening(o) if o.kind == creabim_core::OpeningKind::Window => (
            vec![common(
                "Pset_WindowCommon",
                "Every IFC window's set: external or not, its thermal transmittance, how much of it is glass, fire rating.",
                &["IsExternal", "FireRating", "AcousticRating", "SecurityRating", "ThermalTransmittance"],
            )],
            vec![written(
                "Qto_WindowBaseQuantities",
                "Its width, height and area.",
                vec![len("Width", o.width), len("Height", o.height), area("Area", o.width * o.height)],
            )],
        ),
        ElementKind::Column(_) => (
            vec![common(
                "Pset_ColumnCommon",
                "Every IFC column's set: load-bearing, external or not, its slope and fire rating.",
                &["IsExternal", "LoadBearing", "FireRating"],
            )],
            vec![written(
                "Qto_ColumnBaseQuantities",
                "Its length and volume.",
                vec![len("Length", q.length), vol("NetVolume", q.volume)],
            )],
        ),
        ElementKind::Beam(_) => (
            vec![common(
                "Pset_BeamCommon",
                "Every IFC beam's set: its span, slope, load-bearing, fire rating.",
                &["IsExternal", "LoadBearing", "FireRating"],
            )],
            vec![written(
                "Qto_BeamBaseQuantities",
                "Its length and volume.",
                vec![len("Length", q.length), vol("NetVolume", q.volume)],
            )],
        ),
        ElementKind::Roof(_) => (
            vec![common(
                "Pset_RoofCommon",
                "Every IFC roof's set: external or not, its fire rating and thermal transmittance.",
                &["IsExternal", "FireRating", "AcousticRating", "ThermalTransmittance"],
            )],
            vec![written(
                "Qto_RoofBaseQuantities",
                "Its area along its slopes, and its volume.",
                vec![area("GrossArea", q.area), vol("NetVolume", q.volume)],
            )],
        ),
        ElementKind::Stair(_) => (
            vec![planned(
                "Pset_StairCommon",
                "Every IFC stair's set: its risers and treads, their heights and lengths, whether it is a fire exit.",
                unset(&["NumberOfRiser", "RiserHeight", "TreadLength", "FireExit"]),
            )],
            vec![planned(
                "Qto_StairBaseQuantities",
                "Its length along its going, and its volume.",
                vec![len("Length", q.length), vol("NetVolume", q.volume)],
            )],
        ),
        ElementKind::Railing(_) => (
            vec![planned(
                "Pset_RailingCommon",
                "Every IFC railing's set: its height and its handrail's diameter.",
                unset(&["Height", "Diameter", "IsExternal"]),
            )],
            vec![planned(
                "Qto_RailingBaseQuantities",
                "Its length.",
                vec![len("Length", q.length)],
            )],
        ),
        ElementKind::Footing(_) => (
            vec![
                planned(
                    "Pset_FootingCommon",
                    "Every IFC footing's set: load-bearing, its status.",
                    unset(&["LoadBearing", "Status"]),
                ),
                written(
                    "CreaBIM_Footing",
                    "CreaBIM's own: its sizes and what it carries, so it comes back editable.",
                    vec![len("Thickness", q.thickness)],
                ),
            ],
            vec![planned(
                "Qto_FootingBaseQuantities",
                "Its length, area and volume.",
                vec![len("Length", q.length), area("Area", q.area), vol("NetVolume", q.volume)],
            )],
        ),
        ElementKind::Room(r) => {
            let number = if r.number.is_empty() { e.name.clone() } else { r.number.clone() };
            let outdoor = [&r.occupancy, &r.department]
                .iter()
                .any(|s| s.eq_ignore_ascii_case("outdoor"));
            let shape = creabim_core::room::shape_of(model, r);
            let mut qto = vec![];
            if let Some(sh) = &shape {
                qto.push(area("NetFloorArea", creabim_core::room::net_area(model, r, sh)));
                qto.push(len("GrossPerimeter", sh.perimeter()));
            }
            qto.push(len("Height", creabim_core::room::height(model, r)));
            (
                vec![
                    written(
                        "Pset_SpaceCommon",
                        "Every IFC space's set: its reference and whether it is outside.",
                        vec![
                            text("Reference", &number),
                            text("IsExternal", if outdoor { "Yes" } else { "No" }),
                        ],
                    ),
                    written(
                        "Pset_SpaceCoveringRequirements",
                        "Its floor, wall and ceiling finishes.",
                        vec![
                            text("FloorCovering", &r.floor_finish),
                            text("WallCovering", &r.wall_finish),
                            text("CeilingCovering", &r.ceiling_finish),
                        ],
                    ),
                    written(
                        "Pset_SpaceOccupancyRequirements",
                        "Who uses it.",
                        vec![text("OccupancyType", &r.occupancy)],
                    ),
                ],
                vec![written(
                    "Qto_SpaceBaseQuantities",
                    "Its floor area, perimeter and height.",
                    qto,
                )],
            )
        }
        ElementKind::Ceiling(_) => (
            vec![
                planned(
                    "Pset_CoveringCommon",
                    "Every IFC covering's set: its fire rating, acoustic rating and finish.",
                    unset(&["FireRating", "AcousticRating", "Finish"]),
                ),
                written(
                    "CreaBIM_Ceiling",
                    "CreaBIM's own: its height and the room it covers.",
                    vec![len("Thickness", q.thickness)],
                ),
            ],
            vec![planned(
                "Qto_CoveringBaseQuantities",
                "Its area.",
                vec![area("NetArea", q.area)],
            )],
        ),
        _ => (vec![], vec![]),
    }
}

/// An angle that may be unset (a latitude, a longitude) as a field the
/// row's width that reads "Not set" while empty (MU50: a bare check box
/// each). What is typed is kept while the field has the keyboard, so "51."
/// isn't cut back to "51"; empty is unset, and a value past `range` or not
/// a number is left alone.
fn degrees_field(
    ui: &mut egui::Ui,
    label: &str,
    key: &str,
    v: &mut Option<f64>,
    range: f64,
    tip: &str,
) -> bool {
    crate::props::row(ui, label, |ui| {
        let id = egui::Id::new(("degrees", key));
        let shown = |v: &Option<f64>| v.map_or(String::new(), |d| format!("{d}°"));
        // kept only while the field has the keyboard (dropped below)
        let mut buf = ui
            .ctx()
            .data(|d| d.get_temp::<String>(id.with("typed")))
            .unwrap_or_else(|| shown(v));
        let edit = egui::TextEdit::singleline(&mut buf)
            .id(id)
            .horizontal_align(egui::Align::Center)
            .hint_text(egui::RichText::new("Not set").italics());
        let r = crate::props::text_field(ui, edit, f32::INFINITY).on_hover_text(tip);
        let mut changed = false;
        if r.changed() {
            let t = buf.trim().trim_end_matches('°').trim().replace(',', ".");
            let new = if t.is_empty() {
                Some(None)
            } else {
                t.parse::<f64>().ok().filter(|d| d.abs() <= range).map(Some)
            };
            if let Some(n) = new.filter(|n| n != v) {
                *v = n;
                changed = true;
            }
        }
        if r.has_focus() {
            ui.ctx().data_mut(|d| d.insert_temp(id.with("typed"), buf));
        } else {
            ui.ctx().data_mut(|d| d.remove::<String>(id.with("typed")));
        }
        changed
    })
}

/// Under a plan's view depth, when the view hides its footings: why the
/// depth shows none, and a button to show them. True when clicked.
fn footings_note(ui: &mut egui::Ui) -> bool {
    // a problem with the button that fixes it (AMD-012)
    crate::props::warning(
        ui,
        "Footings are hidden in this view, so the view depth can't show them.",
        Some("Show footings"),
    )
}

/// A plan's graphics in its Properties: the tint of what lies below the
/// cut, how far below its floor it looks, and its pens' line weights.
/// Returns the undo label of a change.
fn plan_graphics_ui(
    ui: &mut egui::Ui,
    g: &mut creabim_core::PlanGraphics,
    scale: f64,
    header: bool,
) -> Option<&'static str> {
    let mut label = None;
    // its own box, folding as the others (MU22); in Properties the panel
    // draws the header, with its ? (MU37)
    if header {
        let id = egui::Id::new("props.view.graphics.open");
        let open = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(true);
        let (resp, _) = crate::props::header(ui, "Graphics", open, false);
        if resp.clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(id, !open));
        }
        if !open {
            return None;
        }
    }
    // how what is cut is drawn (A4-11)
    crate::props::row(ui, "Drawing style", |ui| {
        egui::ComboBox::from_id_salt("view.drawing_style")
            .icon(crate::props::combo_icon)
            .selected_text(g.drawing_style.label())
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for s in creabim_core::graphics::DrawingStyle::ALL {
                    if ui
                        .selectable_value(&mut g.drawing_style, s, s.label())
                        .on_hover_text(s.hint())
                        .changed()
                    {
                        label = Some("Drawing style");
                    }
                }
            });
    });
    if crate::props::check(ui, "Below the cut", &mut g.below_fill, "Tinted")
        .on_hover_text("Furniture, fittings and imported meshes under the cut get a light fill.")
        .changed()
    {
        label = Some("Below-cut fill");
    }
    if g.below_fill {
        crate::props::row(ui, "Strength", |ui| {
            ui.spacing_mut().slider_width = (ui.available_width() - 56.0).max(40.0);
            if ui
                .add(egui::Slider::new(&mut g.below_strength, 0.05..=1.0))
                .changed()
            {
                label = Some("Below-cut fill");
            }
        });
        if crate::props::check(ui, "", &mut g.below_grey, "In grey")
            .on_hover_text("Grey, not the materials' colours")
            .changed()
        {
            label = Some("Below-cut fill");
        }
    }
    let mut to_level = g.range.view_depth.is_none();
    if crate::props::check(ui, "View depth", &mut to_level, "To the level below")
        .on_hover_text(
            "How far below its floor the plan looks, through holes in the floor such as a stair well.",
        )
        .changed()
    {
        g.range.view_depth = if to_level { None } else { Some(1.0) };
        label = Some("View depth");
    }
    if let Some(d) = &mut g.range.view_depth {
        crate::props::row_tip(
            ui,
            "Below floor",
            "Below the floor: how far under the floor the view still shows",
            |ui| {
                if crate::props::fill(ui, crate::units::drag(d).range(0.0..=100.0)).changed() {
                    label = Some("View depth");
                }
            },
        );
    }
    let mut auto = g.weights.is_none();
    if crate::props::check(ui, "Line weights", &mut auto, "From the style pack")
        .on_hover_text("The style pack's pens for the view's scale: in the Standard pack, heavier at 1:50 and larger, lighter at 1:100 and smaller.")
        .changed()
    {
        g.weights = if auto {
            None
        } else {
            Some(creabim_core::LineWeights::for_scale(scale))
        };
        label = Some("Line weights");
    }
    // pens by category are object styles now; a view's own from before
    // still draws, and can be cleared
    if g.category_pens != creabim_core::graphics::default_category_pens() {
        crate::props::worked_out(
            ui,
            "view-pens",
            &[crate::props::Worked::new("Pens", "Its own, by category")],
        );
        crate::props::flow(ui, |ui| {
            if ui
                .small_button("Use the style pack's pens")
                .on_hover_text(
                    "Object styles in File, Graphic styles set each category's pens for every view",
                )
                .clicked()
            {
                g.category_pens = creabim_core::graphics::default_category_pens();
                label = Some("Pens by category");
            }
        });
    }
    if let Some(w) = &mut g.weights {
        for (name, v) in w.fields_mut() {
            crate::props::flow(ui, |ui| {
                ui.label(name);
                if ui
                    .add(
                        egui::DragValue::new(v)
                            .speed(0.01)
                            .range(0.02..=2.0)
                            .max_decimals(2)
                            .suffix(" mm"),
                    )
                    .changed()
                {
                    label = Some("Line weights");
                }
            });
        }
    }
    label
}

/// A drop-down of the ways a section, elevation or 3D view draws.
fn view_style_combo(ui: &mut egui::Ui, salt: &str, style: &mut ViewStyle) -> bool {
    let mut changed = false;
    crate::props::row(ui, "Style", |ui| {
        egui::ComboBox::from_id_salt(salt)
            .icon(crate::props::combo_icon)
            .selected_text(style.label())
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for s in ViewStyle::ALL {
                    changed |= ui.selectable_value(style, s, s.label()).changed();
                }
            });
    });
    changed
}

/// The file named on the command line, if any: the first argument that
/// isn't an option.
pub(crate) fn launch_file(args: impl Iterator<Item = String>) -> Option<String> {
    args.into_iter()
        .find(|a| !a.starts_with('-') && !a.is_empty())
}

/// The map files whose lines a reference shows.
pub(crate) const MAP_LINE_FILES: [&str; 6] = ["geojson", "json", "shp", "osm", "xml", "landxml"];

/// What the status bar says when the starter house opens.
pub(crate) const STARTER_LOADED: &str = "Loaded the starter house. Orbit with the middle button (or Alt + left drag), pan with right drag, zoom with the wheel; a left drag selects with a box. The Sheets group has its drawings.";

/// What the web bars cover of the window, given to egui as its safe area:
/// egui's own drop-downs, windows and the splash keep out from under them,
/// drop-downs opening upwards when there is no room below.
pub(crate) fn band_insets(web: bool) -> egui::epaint::MarginF32 {
    if web {
        egui::epaint::MarginF32 {
            top: shell::TOP_H,
            bottom: shell::STATUS_H,
            ..Default::default()
        }
    } else {
        egui::epaint::MarginF32::default()
    }
}

/// The app round the sample house, for tests.
#[cfg(test)]
pub(crate) fn test_app() -> CreaBimApp {
    let mut app = CreaBimApp::blank(None);
    app.load_model(sample_house());
    app
}

impl CreaBimApp {
    /// The Snap menu's Custom step: a small window to type it in.
    fn step_prompt(&mut self, ctx: &egui::Context) {
        let Some(mut text) = self.plan.step_prompt.take() else {
            return;
        };
        let mut open = true;
        let mut done = false;
        crate::app::shell::window("Step")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("What points and drags round to: 25 mm, 1/4\", 2 cm.");
                let r = crate::props::text_field(ui, egui::TextEdit::singleline(&mut text), 140.0);
                r.request_focus();
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (enter || ui.button("Use it").clicked()) && !text.trim().is_empty() {
                    match crate::units::parse(&text) {
                        Some(m) if m > 0.0 && m < 100.0 => {
                            self.plan.snap.increment = true;
                            self.plan.snap.step_um = (m * 1e6).round() as u32;
                            self.status = format!(
                                "Step {}: points and drags round to it. Alt ignores it for a move.",
                                self.plan.snap.step_label()
                            );
                            done = true;
                        }
                        _ => {
                            self.status = format!("\"{text}\" isn't a length: try 25 mm or 1/4\".")
                        }
                    }
                }
            });
        if open && !done && !ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.plan.step_prompt = Some(text);
        }
    }
}

/// The type the tool's next element takes, by name, if one is picked.
fn typesui_type_name(app: &CreaBimApp, tool: Tool) -> Option<String> {
    let cat = typesui::category_of_tool(tool)?;
    let id = app.plan.types.get(&cat)?;
    app.doc.model().get(*id).map(|e| e.name.clone())
}

#[cfg(test)]
mod title_tests {
    use super::window_title;

    #[test]
    fn dark_paper_turns_greys_over_and_keeps_colours() {
        use super::{paper_dark, Color32};
        let paper = paper_dark(Color32::from_rgb(250, 250, 247));
        let ink = paper_dark(Color32::BLACK);
        assert!(paper.r() < 50, "paper {paper:?}");
        assert!(ink.r() > 200, "ink {ink:?}");
        let orange = crate::theme::interaction();
        assert_eq!(paper_dark(orange), orange);
    }

    #[test]
    fn the_title_names_the_project_marks_unsaved_and_the_test_version() {
        let v = env!("CARGO_PKG_VERSION");
        let saved = window_title("projects/Starter house.creabim", false);
        assert!(saved.starts_with("Starter house — CreaBIM"), "{saved}");
        if v.starts_with("0.") {
            assert!(saved.ends_with(" beta"), "{saved}");
        }
        let unsaved = window_title("Starter house.creabim", true);
        assert!(unsaved.starts_with("• Starter house — "), "{unsaved}");
    }
}

#[cfg(test)]
mod member_sizes {
    #[test]
    fn a_project_in_feet_and_inches_starts_its_doors_and_windows_at_us_sizes() {
        // the film session, 2026-11-25: in Ridgeline (feet and inches) the
        // Door tool started at 900 × 2100 mm
        let inches = |v: [f64; 3]| v.map(|x| (x / 0.0254 * 1000.0).round() / 1000.0);
        let mut app = crate::app::test_app();
        assert!(
            !app.doc.model().units().is_imperial(),
            "the sample house is metric"
        );
        assert_eq!(app.plan.members.door, [0.9, 2.1, 0.0]);
        app.load_model(creabim_core::ridgeline::ridgeline_house());
        assert!(
            app.doc.model().units().is_imperial(),
            "Ridgeline is in feet and inches"
        );
        assert_eq!(
            inches(app.plan.members.door),
            [36.0, 80.0, 0.0],
            "3'-0\" × 6'-8\""
        );
        assert_eq!(
            inches(app.plan.members.window),
            [36.0, 48.0, 32.0],
            "3'-0\" × 4'-0\", its head at 6'-8\""
        );
        // a metric project again: metric sizes again
        app.load_model(creabim_core::sample::sample_house());
        assert_eq!(app.plan.members.door, [0.9, 2.1, 0.0]);
        assert_eq!(app.plan.members.window, [1.2, 1.3, 0.9]);
    }
}

/// Issue #34: the Guide tool takes a typed length, as walls and grid lines
/// do, and a guide's copy a typed distance across ("a line 3' away").
#[cfg(test)]
mod guide_typed_length {
    use super::*;

    fn guides(model: &creabim_core::Model) -> Vec<(ElementId, DVec2, DVec2)> {
        model
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::Guide(g) => Some((e.id, g.a.truncate(), g.b.truncate())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_guide_takes_a_typed_distance_across_and_a_typed_length() {
        let mut app = crate::app::test_app();
        app.load_model(creabim_core::starter::starter_house());
        let plan = app
            .doc
            .model()
            .iter()
            .find(|e| e.name == "Ground Floor plan")
            .unwrap();
        let view = plan.id;
        let level = match &plan.kind {
            ElementKind::View(v) => match v.kind {
                creabim_core::ViewKind::Plan { level, .. } => level,
                _ => unreachable!("a plan"),
            },
            _ => unreachable!("a view"),
        };
        let z = app.doc.model().level(level).map_or(0.0, |l| l.elevation);
        // the house's walls, end to end
        let (mut lo, mut hi) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
        for e in app.doc.model().iter() {
            if let ElementKind::Wall(w) = &e.kind {
                for p in [w.start, w.end] {
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
            }
        }
        // a guide 2 m south of the house, along it
        let y0 = lo.y - 2.0;
        let original = creabim_core::Element::new(
            "Guide",
            ElementKind::Guide(creabim_core::Guide {
                level: Some(level),
                a: DVec3::new(lo.x - 1.0, y0, z),
                b: DVec3::new(hi.x + 1.0, y0, z),
            }),
        );
        let original_id = original.id;
        app.doc
            .commit(creabim_core::Transaction::new("guide").insert(original))
            .unwrap();
        app.active_view = Some(view);
        app.plan.tool = Tool::Guide;
        app.selection.clear();

        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0));
        let mut t = 0.0;
        let mut frame = |app: &mut CreaBimApp, events: Vec<egui::Event>| {
            t += 1.0 / 60.0;
            let raw = egui::RawInput {
                screen_rect: Some(rect),
                events,
                time: Some(t),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| app.show_plan(ui, rect, Color32::WHITE));
            out.textures_delta.clear();
        };
        let button = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let enter = egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        };
        for _ in 0..3 {
            frame(&mut app, vec![]);
        }
        let (center, scale) = app
            .plan
            .cameras
            .get(&view)
            .copied()
            .expect("the plan has a camera");
        let xf = planview::Xf {
            rect,
            center,
            scale,
        };
        let mid = (lo.x + hi.x) / 2.0;

        // the copy: click the guide, go to its north side, type 900, Enter
        let on = xf.to_screen(DVec2::new(mid, y0));
        frame(&mut app, vec![egui::Event::PointerMoved(on)]);
        frame(&mut app, vec![button(on, true)]);
        frame(&mut app, vec![button(on, false)]);
        assert!(
            app.plan.pending.is_some() && app.plan.guide_edge.is_some(),
            "the guide is picked to copy"
        );
        let north = xf.to_screen(DVec2::new(mid + 0.3, y0 + 0.4));
        frame(&mut app, vec![egui::Event::PointerMoved(north)]);
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(north),
                egui::Event::Text("900".into()),
            ],
        );
        frame(
            &mut app,
            vec![egui::Event::PointerMoved(north), enter.clone()],
        );
        let copies: Vec<_> = guides(app.doc.model())
            .into_iter()
            .filter(|g| g.0 != original_id)
            .collect();
        assert_eq!(copies.len(), 1, "Enter places the copy");
        let (_, a, b) = copies[0];
        // parallel to the original, 900 mm across it, on the pointer's side
        assert!(
            (a.y - (y0 + 0.9)).abs() < 1e-6 && (b.y - (y0 + 0.9)).abs() < 1e-6,
            "{a} {b}"
        );
        assert!(app.plan.pending.is_none() && !app.plan.tracker.typing());

        // two points: click, point the way, type 3000, Enter
        let start = xf.to_screen(DVec2::new(mid, y0 - 4.0));
        frame(&mut app, vec![egui::Event::PointerMoved(start)]);
        frame(&mut app, vec![button(start, true)]);
        frame(&mut app, vec![button(start, false)]);
        assert!(
            app.plan.pending.is_some() && app.plan.guide_edge.is_none(),
            "a first point"
        );
        let way = xf.to_screen(DVec2::new(mid + 1.0, y0 - 3.0));
        frame(&mut app, vec![egui::Event::PointerMoved(way)]);
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(way),
                egui::Event::Text("3000".into()),
            ],
        );
        frame(&mut app, vec![egui::Event::PointerMoved(way), enter]);
        let new: Vec<_> = guides(app.doc.model())
            .into_iter()
            .filter(|g| g.0 != original_id && g.0 != copies[0].0)
            .collect();
        assert_eq!(new.len(), 1, "Enter places the guide");
        let (_, a, b) = new[0];
        assert!(
            (a.distance(b) - 3.0).abs() < 1e-6,
            "3000 mm between its points: {}",
            a.distance(b)
        );
    }

    #[test]
    fn a_guides_copy_has_one_field_the_distance_across() {
        assert!(Tool::Guide.takes_typed_length());
        let mut t = crate::tracker::Tracker {
            across: true,
            ..Default::default()
        };
        t.fields[0] = "900".into();
        // Tab stays on the distance: the original sets the copy's angle
        t.next_field();
        assert_eq!(t.active, 0);
        // two points: Tab goes on to the angle, as for a wall
        t.across = false;
        t.next_field();
        assert_eq!(t.active, 1);
    }
}

#[cfg(test)]
mod drag_timing {
    use super::*;

    /// Not a check: where a wall drag's frames go in Ridgeline's ground
    /// floor plan, each frame's slow steps printed. Run with
    /// `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn time_a_wall_drag_in_the_plan() {
        let mut app = crate::app::test_app();
        app.load_model(creabim_core::ridgeline::ridgeline_house());
        let view = app
            .doc
            .model()
            .iter()
            .find(|e| e.name == "Ground Floor plan")
            .unwrap()
            .id;
        app.active_view = Some(view);
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0));
        let mut t = 0.0;
        let mut frame = |app: &mut CreaBimApp, events: Vec<egui::Event>, what: &str| {
            t += 1.0 / 60.0;
            let raw = egui::RawInput {
                screen_rect: Some(rect),
                events,
                time: Some(t),
                ..Default::default()
            };
            let start = std::time::Instant::now();
            {
                let _f = Perf::new("frame");
                let mut out = ctx.run_ui(raw, |ui| app.show_plan(ui, rect, Color32::WHITE));
                out.textures_delta.clear();
            }
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            let (lines, _) = crate::actlog::recent();
            let why = lines
                .iter()
                .rev()
                .find(|l| l.contains("frame of"))
                .cloned()
                .unwrap_or_default();
            println!("{what:>10}: {ms:6.1} ms  {why}");
        };
        // settle the view, then find a wall's middle on screen
        for _ in 0..3 {
            frame(&mut app, vec![], "settle");
        }
        let (center, scale) = app
            .plan
            .cameras
            .get(&view)
            .copied()
            .expect("the plan has a camera");
        let xf = planview::Xf {
            rect,
            center,
            scale,
        };
        let wall = app.doc.model().iter().find_map(|e| match &e.kind {
            ElementKind::Wall(w) => Some((e.id, (w.start + w.end) / 2.0)),
            _ => None,
        });
        let (id, mid) = wall.unwrap();
        let at = xf.to_screen(mid);
        app.selection.set(Some(id));
        frame(&mut app, vec![egui::Event::PointerMoved(at)], "hover");
        frame(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
            "press",
        );
        for k in 1..=8 {
            let p = at + egui::vec2(0.0, 6.0 * k as f32);
            frame(&mut app, vec![egui::Event::PointerMoved(p)], "drag");
        }
        let end = at + egui::vec2(0.0, 48.0);
        frame(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            "release",
        );
        for _ in 0..3 {
            frame(&mut app, vec![egui::Event::PointerMoved(end)], "after");
        }
    }
}

/// The lines `view` draws `id` with when nothing in it is hidden: its own
/// outlines, cut and seen, for the dashed outline of a hidden pick.
pub(crate) fn hidden_lines(
    model: &creabim_core::Model,
    view: ElementId,
    id: ElementId,
) -> Vec<(DVec2, DVec2)> {
    let Some(e) = model.get(view).cloned() else {
        return vec![];
    };
    let mut shown = e.clone();
    match &mut shown.kind {
        ElementKind::View(v) => v.hidden = Default::default(),
        _ => return vec![],
    }
    let mut m = model.clone();
    if creabim_core::Transaction::new("show all")
        .replace(e, shown)
        .apply(&mut m)
        .is_err()
    {
        return vec![];
    }
    let Some(d) = creabim_geom::section::view_drawing(&m, view) else {
        return vec![];
    };
    let mut out = vec![];
    for it in &d.items {
        match it {
            PlanItem::Line { id: i, a, b, .. } if *i == id => out.push((*a, *b)),
            PlanItem::Polygon { id: i, points, .. } if *i == id && points.len() > 1 => {
                for k in 0..points.len() {
                    out.push((points[k], points[(k + 1) % points.len()]));
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod box_select_tests {
    use super::*;

    #[test]
    fn an_element_hidden_in_a_plan_has_its_outline_there_to_dash() {
        let model = creabim_core::starter::starter_house();
        let plan = model
            .iter()
            .find(|e| e.name == "Ground Floor plan")
            .unwrap()
            .id;
        let wall = model
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Wall(_)))
            .unwrap()
            .id;
        // hide the wall in the plan: the plan draws nothing of it ...
        let mut m = model.clone();
        let before = m.get(plan).unwrap().clone();
        let mut after = before.clone();
        if let ElementKind::View(v) = &mut after.kind {
            v.hidden.set_element(wall, true);
        }
        creabim_core::Transaction::new("hide")
            .replace(before, after)
            .apply(&mut m)
            .unwrap();
        let drawn = creabim_geom::section::view_drawing(&m, plan).unwrap();
        assert!(!drawn.items.iter().any(|i| i.id() == wall));
        // ... but its outline is there to dash
        assert!(hidden_lines(&m, plan, wall).len() >= 4);
    }

    #[test]
    fn a_guide_in_3d_is_held_for_a_drag_once_selected_and_taken_by_a_box() {
        let mut app = crate::app::test_app();
        app.load_model(creabim_core::starter::starter_house());
        let guide = creabim_core::Element::new(
            "Guide",
            ElementKind::Guide(creabim_core::Guide {
                level: None,
                a: DVec3::new(-30.0, -20.0, 0.0),
                b: DVec3::new(-30.0, 20.0, 0.0),
            }),
        );
        let id = guide.id;
        app.doc
            .commit(creabim_core::Transaction::new("guide").insert(guide))
            .unwrap();
        // looking straight down: 10 px a metre, the origin at (600, 450)
        let project = |p: DVec3| {
            Some(egui::pos2(
                600.0 + p.x as f32 * 10.0,
                450.0 - p.y as f32 * 10.0,
            ))
        };
        let on = egui::pos2(300.0, 450.0);
        let off = egui::pos2(340.0, 450.0);
        // not selected: a drag from it is a box, as on anything else
        app.selection.clear();
        assert_eq!(app.guide_pressed_3d(on, &project), None);
        // selected: a drag from it moves it, on its own level
        app.selection.set(Some(id));
        assert_eq!(app.guide_pressed_3d(on, &project), Some((id, 0.0)));
        assert_eq!(app.guide_pressed_3d(off, &project), None);
        // a crossing box over it takes it, a box beside it does not
        app.selection.clear();
        let marks = app.guide_marks_3d(&project);
        let boxed =
            |a: egui::Pos2, b: egui::Pos2| crate::boxsel::pick_triangles(a, b, marks.clone());
        assert!(boxed(egui::pos2(320.0, 460.0), egui::pos2(280.0, 440.0)).contains(&id));
        assert!(!boxed(egui::pos2(360.0, 460.0), egui::pos2(320.0, 440.0)).contains(&id));
        // a window round both its points takes it
        assert!(boxed(egui::pos2(280.0, 200.0), egui::pos2(320.0, 700.0)).contains(&id));
        // guides switched off in 3D: neither
        app.shell.settings.guides_hidden = true;
        app.selection.set(Some(id));
        assert_eq!(app.guide_pressed_3d(on, &project), None);
        assert!(app.guide_marks_3d(&project).is_empty());
    }

    #[test]
    fn a_left_drag_in_the_apps_plan_area_selects_with_a_box() {
        let mut app = crate::app::test_app();
        app.load_model(creabim_core::starter::starter_house());
        let view = app
            .doc
            .model()
            .iter()
            .find(|e| e.name == "Ground Floor plan")
            .unwrap()
            .id;
        app.active_view = Some(view);
        app.selection.clear();
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0));
        let mut t = 0.0;
        let mut frame = |app: &mut CreaBimApp, events: Vec<egui::Event>| {
            t += 1.0 / 60.0;
            let raw = egui::RawInput {
                screen_rect: Some(rect),
                events,
                time: Some(t),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| app.show_plan(ui, rect, Color32::WHITE));
            out.textures_delta.clear();
        };
        // left to right, round the whole drawing: takes what is inside
        let (a, b) = (egui::Pos2::new(5.0, 5.0), egui::Pos2::new(1195.0, 895.0));
        frame(&mut app, vec![egui::Event::PointerMoved(a)]);
        frame(&mut app, vec![egui::Event::PointerMoved(a)]);
        frame(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: a,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
        );
        for k in 1..=10 {
            frame(
                &mut app,
                vec![egui::Event::PointerMoved(a + (b - a) * (k as f32 / 10.0))],
            );
        }
        frame(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: b,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        frame(&mut app, vec![egui::Event::PointerMoved(b)]);
        assert!(!app.selection.is_empty(), "status: {}", app.status);
    }
}

#[cfg(test)]
mod sheet_tools_tests {
    use super::*;

    fn a_sheet(app: &CreaBimApp) -> ElementId {
        sheets::sheets(app.doc.model())
            .first()
            .map(|(id, _)| *id)
            .expect("the test project has sheets")
    }

    #[test]
    fn the_header_buttons_reach_their_actions() {
        // MU22: these were handled but never routed from a click
        let mut app = crate::app::test_app();
        let ctx = egui::Context::default();
        let before = app.plan.temp_dims_off;
        app.run_action(&ctx, "view.tempdims");
        assert_ne!(
            app.plan.temp_dims_off, before,
            "temporary dimensions toggle"
        );
        app.run_action(&ctx, "sheet.preview");
        assert!(app.print_preview, "print preview toggles");
        app.run_action(&ctx, "export.sheets");
        assert!(app.sheets_export_open, "Export sheets opens");
    }

    #[test]
    fn a_north_arrow_and_a_scale_bar_go_on_the_sheet_and_into_its_pdf() {
        let mut app = crate::app::test_app();
        let ctx = egui::Context::default();
        let sheet = a_sheet(&app);
        app.active_view = Some(sheet);
        let page = |app: &CreaBimApp| {
            creabim_geom::sheet::sheets_pdf_of(app.doc.model(), &app.sheet_info(), &[sheet]).len()
        };
        let plain = page(&app);
        app.run_action(&ctx, "sheet.north");
        let north = app.selection.active.expect("the north arrow is selected");
        app.run_action(&ctx, "sheet.scalebar");
        let bar = app.selection.active.expect("the scale bar is selected");
        let marks: Vec<&creabim_core::PaperMark> = app
            .doc
            .model()
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::PaperMark(m) => Some(m),
                _ => None,
            })
            .collect();
        assert_eq!(marks.len(), 2);
        assert!(marks.iter().all(|m| m.sheet == sheet));
        // inside the sheet, clear of each other
        let s = sheets::sheet_of(app.doc.model(), sheet).unwrap().clone();
        let (_, tb_hi) = creabim_geom::sheet::title_block_box(app.doc.model(), sheet).unwrap();
        for m in &marks {
            let (lo, hi) = m.bounds();
            assert!(
                lo.x > 0.0 && lo.y > 0.0 && hi.x < s.width && hi.y < s.height,
                "{lo} {hi}"
            );
            assert!(
                lo.y > tb_hi.y,
                "clear of the title block: {lo} over {tb_hi}"
            );
        }
        assert!(page(&app) > plain, "the PDF draws them");
        // they move with Move, and only across the sheet
        let r = app.doc.commit(
            creabim_core::Transaction::new("t").replace(
                app.doc.model().get(north).unwrap().clone(),
                creabim_core::transform::transformed(
                    app.doc.model(),
                    app.doc.model().get(north).unwrap(),
                    &creabim_core::transform::Xform::Move(glam::DVec3::new(5.0, 0.0, 0.0)),
                )
                .unwrap(),
            ),
        );
        assert!(r.is_ok());
        // their Properties draw
        app.selection.only(bar);
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 900.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            app.properties_header(ui);
            app.properties(ui);
        });
        out.textures_delta.clear();
    }

    #[test]
    fn export_sheets_writes_chosen_sheets_one_pdf_each() {
        let mut app = crate::app::test_app();
        let all: Vec<ElementId> = sheets::sheets(app.doc.model())
            .iter()
            .map(|(id, _)| *id)
            .collect();
        app.sheets_export.scope = ExportScope::Chosen;
        app.sheets_export.chosen = all.iter().take(2).copied().collect();
        assert_eq!(app.sheets_to_export(false).len(), all.len().min(2));
        app.sheets_export.each = true;
        app.export_sheets_now(false);
        assert!(app.status.starts_with("Exported"), "{}", app.status);
        app.sheets_export.scope = ExportScope::This;
        app.active_view = None;
        assert!(app.sheets_to_export(false).is_empty());
    }
}

#[cfg(test)]
mod prop_tab_tests {
    use super::*;

    /// A reading note's Properties, drawn as the panel draws them: Shows
    /// says What it points at, the linked object, its Add, Follow and
    /// Capitals rows; and the Wrap at, Markdown and Leader boxes share one
    /// left edge (MU135).
    #[test]
    fn a_reading_notes_panel_shows_its_link_and_lines_up() {
        let mut app = crate::app::test_app();
        let m = app.doc.model();
        let (plan, level) = m
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::View(v) => match v.kind {
                    ViewKind::Plan { level, .. } => Some((e.id, level)),
                    _ => None,
                },
                _ => None,
            })
            .unwrap();
        let wall = m
            .iter()
            .find(|e| matches!(&e.kind, ElementKind::Wall(w) if w.level == level))
            .unwrap()
            .clone();
        let mut n = creabim_core::Note::new(plan, glam::DVec2::new(1.0, 1.0), "{Name}");
        n.leader = Some(creabim_core::element::HostPoint {
            host: Some(wall.id),
            ..creabim_core::element::HostPoint::free(glam::DVec2::ZERO)
        });
        n.reads = Some(creabim_core::NoteReads::default());
        n.width = Some(60.0);
        let note = Element::new("Reading note", ElementKind::Note(n));
        let id = note.id;
        app.doc
            .commit(Transaction::new("Note").insert(note))
            .unwrap();
        app.selection.only(id);
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 1400.0),
            )),
            ..Default::default()
        };
        // two frames: the first lays out, the second draws at the sizes
        let mut out = ctx.run_ui(raw.clone(), |ui| {
            app.properties_header(ui);
            app.properties(ui);
        });
        out.textures_delta.clear();
        let mut out = ctx.run_ui(raw, |ui| {
            app.properties_header(ui);
            app.properties(ui);
        });
        out.textures_delta.clear();
        let mut texts: Vec<(String, egui::Pos2)> = vec![];
        let mut boxes: Vec<egui::Rect> = vec![];
        fn walk(s: &egui::Shape, t: &mut Vec<(String, egui::Pos2)>, b: &mut Vec<egui::Rect>) {
            match s {
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, t, b)),
                egui::Shape::Text(x) => t.push((x.galley.text().to_string(), x.pos)),
                egui::Shape::Rect(r)
                    if (r.rect.width() - 14.0).abs() < 0.01
                        && (r.rect.height() - 14.0).abs() < 0.01 =>
                {
                    b.push(r.rect)
                }
                _ => {}
            }
        }
        for c in &out.shapes {
            walk(&c.shape, &mut texts, &mut boxes);
        }
        let has = |w: &str| texts.iter().any(|(t, _)| t.contains(w));
        for w in [
            "An object's data",
            "Object",
            "Follow the arrow",
            "Capitals",
            "Markdown",
            "Leader",
        ] {
            assert!(
                has(w),
                "{w} missing from {:?}",
                texts.iter().map(|t| &t.0).collect::<Vec<_>>()
            );
        }
        assert!(has(&wall.name), "the linked wall's name");
        // the boxes beside Wrap at, Markdown and Leader: one left edge
        let y_of = |w: &str| {
            texts
                .iter()
                .find(|(t, _)| t == w)
                .map(|(_, p)| p.y)
                .unwrap()
        };
        let left_at = |y: f32| {
            boxes
                .iter()
                .find(|r| (r.center().y - y).abs() < 14.0)
                .map(|r| r.left())
                .unwrap_or_else(|| panic!("no box near y {y}: {boxes:?}"))
        };
        let lefts: Vec<f32> = ["Wrap at", "Markdown", "Leader"]
            .iter()
            .map(|w| left_at(y_of(w) + 7.0))
            .collect();
        assert!(
            lefts.iter().all(|l| (l - lefts[0]).abs() < 0.5),
            "Wrap at, Markdown, Leader boxes at {lefts:?}"
        );
    }

    /// The note's Object: a pasted GlobalId finds its one object; the
    /// pipette makes the next element selected the note's object, keeps
    /// the note selected, and moves a free arrow onto it (MU135).
    #[test]
    fn a_note_links_its_object_by_id_or_pipette() {
        let mut app = crate::app::test_app();
        let m = app.doc.model();
        let plan = m
            .iter()
            .find(|e| matches!(&e.kind, ElementKind::View(v) if matches!(v.kind, ViewKind::Plan { .. })))
            .unwrap()
            .id;
        let wall = m
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Wall(_)))
            .unwrap()
            .clone();
        let found = super::notesui::note_objects(m, &wall.id.ifc_guid());
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].0, wall.id);
        assert!(super::notesui::note_objects(m, "IfcWall").len() > 1);
        let mut n = creabim_core::Note::new(
            plan,
            glam::DVec2::new(1.0, 1.0),
            "{Name}
{Layers}",
        );
        n.leader = Some(creabim_core::element::HostPoint::free(glam::DVec2::ZERO));
        n.reads = Some(creabim_core::NoteReads::default());
        let note = Element::new("Reading note", ElementKind::Note(n));
        let id = note.id;
        app.doc
            .commit(Transaction::new("Note").insert(note))
            .unwrap();
        app.note_pick = Some(id);
        app.selection.only(wall.id);
        app.note_pick_step(&egui::Context::default());
        assert_eq!(app.note_pick, None);
        assert_eq!(app.selection.active, Some(id));
        let Some(ElementKind::Note(n)) = app.doc.model().get(id).map(|e| e.kind.clone()) else {
            unreachable!()
        };
        assert_eq!(n.reads_from(), Some(wall.id));
        assert_eq!(
            n.leader.and_then(|l| l.host),
            Some(wall.id),
            "the free arrow moved onto it"
        );
        let (text, _) = creabim_geom::annotate::note_shown_text(app.doc.model(), &n);
        assert!(text.starts_with(&wall.name), "{text}");
        assert!(text.lines().count() > 1, "its layers, a line each: {text}");
    }

    fn draw(app: &mut CreaBimApp) {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 900.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            app.properties_header(ui);
            app.properties(ui);
        });
        out.textures_delta.clear();
    }

    #[test]
    fn the_data_tab_names_the_standard_sets_and_says_which_are_written() {
        use crate::props::Read;
        let m = creabim_core::starter::starter_house();
        let find = |f: fn(&ElementKind) -> bool| m.iter().find(|e| f(&e.kind)).unwrap().clone();
        let ctx = egui::Context::default();
        let wall = find(|k| matches!(k, ElementKind::Wall(_)));
        let q = element_quantity(&ctx, &m, wall.id);
        assert!(q.is_some_and(|q| q.length > 0.0 && q.volume > 0.0));
        let (p, qto) = ifc_sets(&m, &wall, q);
        assert_eq!(p[0].name, "Pset_WallCommon");
        // the common set goes out with what is known: the starter house
        // has rooms, so External is worked out; nothing is declared yet
        assert!(p[0].written && qto[0].written);
        assert_eq!(p[0].rows[0].label, "IsExternal");
        assert!(matches!(p[0].rows[0].value, Read::Text(_)));
        assert!(p[0]
            .rows
            .iter()
            .any(|r| r.label == "FireRating" && matches!(r.value, Read::Unset)));
        let names: Vec<&str> = qto[0].rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(
            names,
            ["Length", "Height", "Width", "NetSideArea", "NetVolume"]
        );
        assert!(qto[0].rows.iter().all(|r| matches!(r.value, Read::Text(_))));
        // rooms' space sets go out today, with their values
        let room = find(|k| matches!(k, ElementKind::Room(_)));
        let (p, qto) = ifc_sets(&m, &room, None);
        assert!(p.iter().all(|s| s.written) && qto[0].written);
        assert!(matches!(p[0].rows[0].value, Read::Text(_)));
        // a sheet is not in IFC at all
        assert!(!in_ifc(&find(|k| matches!(k, ElementKind::Sheet(_))).kind));
    }

    #[test]
    fn a_section_beside_the_plan_worked_in_is_not_drawn_on_the_spot() {
        let mut app = crate::app::test_app();
        app.load_model(creabim_core::starter::starter_house());
        let named =
            |app: &CreaBimApp, n: &str| app.doc.model().iter().find(|e| e.name == n).unwrap().id;
        let (plan, section) = (named(&app, "Ground Floor plan"), named(&app, "Section A-A"));
        // the plan in the area worked in, the section beside it
        app.areas.root = crate::areas::Node::split(
            true,
            0.5,
            crate::areas::Node::Area(crate::areas::Shows::View(plan)),
            crate::areas::Node::Area(crate::areas::Shows::View(section)),
        );
        app.areas.focus = 0;
        // the section drawn once, and known to be dear; a worker at hand
        let first = app.drawing(section).expect("the section draws");
        app.builds.record((section, false), 400.0);
        app.builds.set_context(egui::Context::default());
        // a step of an edit in the plan
        let wall = app
            .doc
            .model()
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Wall(_)))
            .unwrap()
            .clone();
        let mut moved = wall.clone();
        if let ElementKind::Wall(w) = &mut moved.kind {
            w.height += 0.01;
        }
        app.doc
            .commit(creabim_core::Transaction::new("step").replace(wall, moved))
            .unwrap();
        let now = app.doc.model().drawing_revision_of(section);
        // each area draws its own view, as a frame does
        for v in [plan, section] {
            app.active_view = Some(v);
            let _ = app.drawing_live(v);
        }
        // the section shown is still the one held: the new one is a
        // worker's to build, not this frame's
        let held = app.drawings.get(&section).map(|(r, _)| *r);
        assert_ne!(held, Some(now), "the section was built in the frame");
        let _ = first;
        // worked in itself, it is drawn on the spot as before
        app.areas.focus = 1;
        app.active_view = Some(section);
        let _ = app.drawing_live(section);
        assert_eq!(app.drawings.get(&section).map(|(r, _)| *r), Some(now));
        // but not at every step of a move made in it: a dear section was
        // built on the frame, 0.5-0.8 s a step (MU142 lag)
        let ids = [app
            .doc
            .model()
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Object(_)))
            .unwrap()
            .id];
        app.plan.modal = Some(
            crate::modal::Modal::start(
                &app.doc,
                crate::modal::Mode::Move,
                &ids,
                None,
                crate::modal::Space::Plan,
                Some(section),
                egui::Pos2::ZERO,
                crate::modal::Pivot::Median,
                None,
                None,
            )
            .expect("a move starts"),
        );
        let wall = app
            .doc
            .model()
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Wall(_)))
            .unwrap()
            .clone();
        let mut moved = wall.clone();
        if let ElementKind::Wall(w) = &mut moved.kind {
            w.height += 0.01;
        }
        app.doc
            .commit(creabim_core::Transaction::new("step").replace(wall, moved))
            .unwrap();
        let step = app.doc.model().drawing_revision_of(section);
        let _ = app.drawing_live(section);
        assert_ne!(
            app.drawings.get(&section).map(|(r, _)| *r),
            Some(step),
            "the section worked in was built on the frame during a move"
        );
    }

    #[test]
    fn a_viewport_shows_its_view_its_sheet_and_its_title_block() {
        let mut app = crate::app::test_app();
        app.load_model(creabim_core::starter::starter_house());
        let vp = app
            .doc
            .model()
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Viewport(_)))
            .unwrap()
            .id;
        app.selection.only(vp);
        app.prop_tab = PropTab::Element;
        draw(&mut app);
        // nothing it drew changed the model
        assert!(app.doc.model().get(vp).is_some());
    }

    #[test]
    fn a_sheet_in_front_with_nothing_picked_shows_its_sheet_tab() {
        let mut app = crate::app::test_app();
        let sheet = app
            .doc
            .model()
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Sheet(_)))
            .unwrap()
            .id;
        app.areas.root = crate::areas::Node::Area(crate::areas::Shows::View(sheet));
        app.areas.focus = 0;
        app.selection.clear();
        let tabs = app.prop_tabs();
        // the sheet, not the 3D view's crop boxes (MU55-05)
        assert!(tabs.contains(&PropTab::Sheet), "{tabs:?}");
        assert!(!tabs.contains(&PropTab::View), "{tabs:?}");
        app.prop_tab = PropTab::Sheet;
        draw(&mut app);
        // a 3D area in front has none
        app.areas.root = crate::areas::Node::Area(crate::areas::Shows::Model);
        assert!(!app.prop_tabs().contains(&PropTab::Sheet));
    }

    /// Every action id in a menu, submenus included.
    fn menu_ids(items: &[crate::ui_model::Item]) -> Vec<String> {
        use crate::ui_model::Item;
        let mut out = vec![];
        for i in items {
            match i {
                Item::Action { id, .. } => out.push(id.clone()),
                Item::Sub { items, .. } => out.extend(menu_ids(items)),
                _ => {}
            }
        }
        out
    }

    #[test]
    fn the_create_menu_reaches_every_tool_by_family() {
        let app = crate::app::test_app();
        let create = app.menu_items("menu:Create");
        let ids = menu_ids(&create);
        for (i, t) in Tool::ALL.iter().enumerate() {
            // Select and Align are not made; the drawing tool and the site
            // tools are reached through their families' members
            if matches!(t, Tool::Select | Tool::Align) {
                continue;
            }
            let via_family = crate::families::Family::ALL
                .iter()
                .any(|f| f.holds(*t) && f.members().iter().any(|m| ids.contains(&m.id)));
            // Door and Window open with their picker (Create › Door…)
            let via_picker = match t {
                Tool::Door => ids.iter().any(|i| i == "kinds.create:door"),
                Tool::Window => ids.iter().any(|i| i == "kinds.create:window"),
                _ => false,
            };
            assert!(
                ids.contains(&format!("tool:{i}")) || via_family || via_picker,
                "{} is not in the Create menu",
                t.label()
            );
        }
        // a plan and a ceiling plan of each level, straight from it (MU55-01)
        assert!(ids.iter().any(|i| i.starts_with("new.plan:")));
        assert!(ids.iter().any(|i| i.starts_with("new.ceiling:")));
        // the checks went to Manage
        assert!(!ids.iter().any(|i| i.starts_with("check.")));
        // shorter at its top: the singles, then one submenu a family
        assert!(create.len() < 30, "{} lines", create.len());
    }

    #[test]
    fn a_ceiling_plan_is_made_from_a_level_and_opened() {
        let mut app = crate::app::test_app();
        let level = app.doc.model().levels().next().unwrap().0.id;
        let before = app.doc.model().iter().count();
        app.new_plan_of(level, true);
        assert_eq!(app.doc.model().iter().count(), before + 1);
        let v = app.active_view.unwrap();
        match &app.doc.model().get(v).unwrap().kind {
            ElementKind::View(x) => {
                assert!(x.ceiling_plan);
                assert!(matches!(x.kind, ViewKind::Plan { level: l, .. } if l == level));
            }
            k => panic!("{k:?}"),
        }
        // one undo step takes it away
        app.doc.undo();
        assert_eq!(app.doc.model().iter().count(), before);
    }

    #[test]
    fn a_family_button_brings_back_its_last_tool_and_draw_by_has_its_panel() {
        let mut app = crate::app::test_app();
        let ctx = egui::Context::default();
        let beam = Tool::ALL.iter().position(|t| *t == Tool::Beam).unwrap();
        app.run_action(&ctx, &format!("tool:{beam}"));
        assert_eq!(app.plan.tool, Tool::Beam);
        assert_eq!(
            app.shell
                .settings
                .family_last
                .get("structure")
                .map(String::as_str),
            Some(format!("tool:{beam}").as_str())
        );
        // a Site tool: its tool, then what it draws
        app.run_action(&ctx, "site:ground.Pad");
        assert_eq!(app.plan.tool, Tool::Terrain);
        assert_eq!(app.plan.parts.ground, crate::partstool::GroundDraw::Pad);
        app.run_action(&ctx, "site:plant.Palm");
        assert_eq!(app.plan.tool, Tool::Planting);
        // the family's panel ticks the one in use, with its headings
        let items = app.menu_items("fam:site");
        assert!(items
            .iter()
            .any(|i| matches!(i, crate::ui_model::Item::Header(h) if h == "Earthworks")));
        // Draw by: shape and edges for an outline tool, four shapes for Wall
        let slab = Tool::ALL.iter().position(|t| *t == Tool::Slab).unwrap();
        app.run_action(&ctx, &format!("tool:{slab}"));
        let ids = menu_ids(&app.menu_items("drawby"));
        assert!(ids.iter().any(|i| i == "slab.mode:1") && ids.iter().any(|i| i == "slab.arc:1"));
        let wall = Tool::ALL.iter().position(|t| *t == Tool::Wall).unwrap();
        app.run_action(&ctx, &format!("tool:{wall}"));
        assert_eq!(menu_ids(&app.menu_items("drawby")).len(), 4);
        // a pin puts a member on the bar, and takes it off again
        app.run_action(&ctx, &format!("fampin:tool:{beam}"));
        assert!(app
            .shell
            .settings
            .family_pins
            .contains(&format!("tool:{beam}")));
        app.run_action(&ctx, &format!("fampin:tool:{beam}"));
        assert!(app.shell.settings.family_pins.is_empty());
    }

    #[test]
    fn colour_hints_group_the_tabs_as_the_mockup_and_start_on() {
        use crate::props::Hue;
        assert!(crate::files::Settings::default().colour_hints);
        for (t, h) in [
            (PropTab::Element, Hue::Element),
            (PropTab::Tree, Hue::Element),
            (PropTab::Joins, Hue::Element),
            (PropTab::Graphics, Hue::View),
            (PropTab::Data, Hue::Data),
            (PropTab::View, Hue::View),
            (PropTab::Sheet, Hue::View),
            (PropTab::Project, Hue::Project),
            (PropTab::Notes, Hue::Project),
        ] {
            assert_eq!(t.hue(), h, "{t:?}");
        }
        // each theme its own lighter or darker version
        assert_ne!(Hue::View.colour(true), Hue::View.colour(false));
    }

    #[test]
    fn the_view_tab_follows_the_area_clicked_3d_included() {
        let mut app = crate::app::test_app();
        let plan = app
            .doc
            .model()
            .iter()
            .find(|e| matches!(&e.kind, ElementKind::View(v) if matches!(v.kind, ViewKind::Plan { .. })))
            .unwrap()
            .id;
        let wall = app
            .doc
            .model()
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Wall(_)))
            .unwrap()
            .id;
        app.areas.root = crate::areas::Node::Split {
            across: true,
            frac: 0.5,
            kids: Box::new([
                crate::areas::Node::Area(crate::areas::Shows::Model),
                crate::areas::Node::Area(crate::areas::Shows::View(plan)),
            ]),
        };
        app.active_view = Some(plan);
        // the 3D area clicked: View is about the 3D view, with or without a
        // selection, and there is no separate 3D view tab (REV04 §4)
        app.areas.focus = 0;
        for sel in [None, Some(wall)] {
            match sel {
                Some(w) => app.selection.only(w),
                None => app.selection.clear(),
            }
            let tabs = app.prop_tabs();
            assert!(tabs.contains(&PropTab::View), "{tabs:?}");
            assert!(!tabs.contains(&PropTab::Model3d), "{tabs:?}");
            assert!(!tabs.contains(&PropTab::Trace), "{tabs:?}");
            assert!(app.model_in_front());
            app.prop_tab = PropTab::View;
            draw(&mut app);
        }
        // the plan clicked: View is about the plan again
        app.areas.focus = 1;
        app.selection.clear();
        assert!(!app.model_in_front());
        let tabs = app.prop_tabs();
        assert!(tabs.contains(&PropTab::View) && tabs.contains(&PropTab::Trace));
        // the Log clicked: no view to describe, its own tab leads (MU60)
        app.areas.root = crate::areas::Node::Split {
            across: true,
            frac: 0.5,
            kids: Box::new([
                crate::areas::Node::Area(crate::areas::Shows::Log),
                crate::areas::Node::Area(crate::areas::Shows::View(plan)),
            ]),
        };
        app.areas.focus = 0;
        let tabs = app.prop_tabs();
        assert!(!tabs.contains(&PropTab::View), "{tabs:?}");
        assert!(tabs.contains(&PropTab::Log), "{tabs:?}");
    }

    #[test]
    fn every_properties_tab_draws_for_a_wall_and_for_nothing_selected() {
        let mut app = crate::app::test_app();
        let wall = app
            .doc
            .model()
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Wall(_)))
            .unwrap()
            .id;
        app.selection.only(wall);
        let tabs = app.prop_tabs();
        assert_eq!(
            &tabs[..5],
            &[
                PropTab::Select,
                PropTab::Element,
                PropTab::Joins,
                PropTab::Graphics,
                PropTab::Data
            ]
        );
        for t in tabs {
            app.prop_tab = t;
            draw(&mut app);
        }
        app.selection.clear();
        let tabs = app.prop_tabs();
        assert!(!tabs.contains(&PropTab::Model3d) && tabs.contains(&PropTab::Project));
        // the Select tab is there with nothing selected too (decision 6)
        assert_eq!(tabs[0], PropTab::Select);
        for t in tabs {
            app.prop_tab = t;
            draw(&mut app);
        }
        app.plan.tool = Tool::Window;
        assert_eq!(app.prop_tabs()[1], PropTab::Next);
        app.prop_tab = PropTab::Next;
        draw(&mut app);
    }

    #[test]
    fn a_view_added_as_a_reference_is_placed_selected_and_hidden_with_alt_f2() {
        let mut app = crate::app::test_app();
        let plans: Vec<ElementId> = app
            .doc
            .model()
            .iter()
            .filter(|e| matches!(&e.kind, ElementKind::View(v) if matches!(v.kind, ViewKind::Plan { .. })))
            .map(|e| e.id)
            .collect();
        assert!(plans.len() >= 2, "the test project has two plans");
        let (here, below) = (plans[1], plans[0]);
        app.add_reference(here, creabim_core::RefSource::View(below));
        let id = app.selection.active.expect("the new reference is selected");
        let Some(ElementKind::Reference(r)) = app.doc.model().get(id).map(|e| &e.kind) else {
            panic!("a reference was added")
        };
        assert_eq!(r.shown_in, vec![here]);
        assert!(r.bounds.is_some(), "its bounds were read from the view");
        // in place: its own points are where they were
        let c = r.centre();
        assert!(r.place(c).distance(c) < 1e-9);
        assert_eq!(
            creabim_core::reference::in_view(app.doc.model(), here).len(),
            1
        );
        // its Properties draw, every tab
        for t in app.prop_tabs() {
            app.prop_tab = t;
            draw(&mut app);
        }
        // Alt+F2's toggle hides it, then shows it again
        app.toggle_references(here);
        assert!(
            matches!(&app.doc.model().get(id).unwrap().kind, ElementKind::Reference(r) if r.hidden)
        );
        app.toggle_references(here);
        assert!(
            matches!(&app.doc.model().get(id).unwrap().kind, ElementKind::Reference(r) if !r.hidden)
        );
        // the Trace tab lists it, with nothing selected
        app.selection.clear();
        app.prop_tab = PropTab::Trace;
        draw(&mut app);
    }
}

/// Is the tool drawing an outline on a face (an extrusion or a void)?
fn on_face_now(plan: &crate::planview::PlanState) -> bool {
    crate::partstool::on_face(plan).is_some()
}

/// A slim handle on a panel's edge that folds or opens it (MU29): a
/// control of the panel, not one of its tabs. `left_of` puts it on the
/// panel's left edge, pointing the way it will go.
pub(crate) fn fold_handle(
    ctx: &egui::Context,
    id: &str,
    at: egui::Pos2,
    folded: bool,
    left_of: bool,
    tip: &str,
) -> bool {
    // not over the splash's shade: it would stand out bright on it (MU79)
    if ctx
        .data(|d| d.get_temp::<bool>(egui::Id::new("splash.open")))
        .unwrap_or(false)
    {
        return false;
    }
    let p = crate::theme::Palette::of(ctx);
    let mut clicked = false;
    egui::Area::new(egui::Id::new(("fold-handle", id)))
        .order(crate::app::overlay_order())
        .fixed_pos(at)
        .show(ctx, |ui| {
            let (r, resp) = ui.allocate_exact_size(egui::vec2(11.0, 46.0), egui::Sense::click());
            let fill = if resp.hovered() {
                p.raise2
            } else {
                p.raise.gamma_multiply(0.8)
            };
            let round = if left_of {
                egui::CornerRadius {
                    nw: 6,
                    sw: 6,
                    ne: 0,
                    se: 0,
                }
            } else {
                egui::CornerRadius {
                    nw: 0,
                    sw: 0,
                    ne: 6,
                    se: 6,
                }
            };
            ui.painter().rect(
                r,
                round,
                fill,
                egui::Stroke::new(1.0, p.border),
                egui::StrokeKind::Inside,
            );
            // a small chevron: which way it goes
            let c = r.center();
            let s = if folded == left_of { -1.0 } else { 1.0 };
            let ink = if resp.hovered() { p.text } else { p.muted };
            let pts = [
                c + egui::vec2(-1.5 * s, -4.0),
                c + egui::vec2(1.5 * s, 0.0),
                c + egui::vec2(-1.5 * s, 4.0),
            ];
            ui.painter()
                .line_segment([pts[0], pts[1]], egui::Stroke::new(1.4, ink));
            ui.painter()
                .line_segment([pts[1], pts[2]], egui::Stroke::new(1.4, ink));
            clicked = resp.on_hover_text(tip).clicked();
        });
    clicked
}

impl CreaBimApp {
    /// How a plan view shows the site: which ground, ready-made sets of
    /// switches, and each kind of site information on or off. Shown when
    /// the project has a site.
    fn site_view_panel(&mut self, ui: &mut egui::Ui, v: ElementId) {
        use creabim_core::{GroundState, SitePreset};
        let model = self.doc.model();
        let Some(ElementKind::View(view)) = model.get(v).map(|e| &e.kind) else {
            return;
        };
        // a section or elevation shows the ground, its cut and fill and
        // its levels; a 3D view the ground alone
        let plan = matches!(view.kind, ViewKind::Plan { .. });
        let section = matches!(view.kind, ViewKind::Section { .. });
        if !plan && !section && !matches!(view.kind, ViewKind::Axonometric { .. }) {
            return;
        }
        if !model.iter().any(|e| matches!(e.kind, ElementKind::Site(_))) {
            return;
        }
        let mut sv = view.site.clone();
        let open = self.prop_header(ui, "view.site", "Site", true, Some("site"));
        crate::props::fold(ui, open, |ui| {
            let mut label = "";
            if !plan {
                self.site_view_ground_only(ui, v, sv, section);
                return;
            }
            // one drop-down, as Ground's under it: six loose labels wrapped
            // one to a line at the panel's width and read as a list (MU121)
            crate::props::row(ui, "Set", |ui| {
                let now = SitePreset::of(&sv);
                let mut shown = egui::RichText::new(site_set_name(now));
                if now.is_none() {
                    // switches changed by hand match no set
                    shown = shown.italics().color(ui.visuals().weak_text_color());
                }
                egui::ComboBox::from_id_salt("view.site.set")
                    .icon(crate::props::combo_icon)
                    .selected_text(shown)
                    .width(ui.available_width())
                    .truncate()
                    .show_ui(ui, |ui| {
                        for p in SitePreset::ALL {
                            if ui
                                .selectable_label(now == Some(p), p.label())
                                .on_hover_text(p.hint())
                                .clicked()
                                && now != Some(p)
                            {
                                sv = sv.with_preset(p);
                                label = "Site view";
                            }
                        }
                    })
                    .response
                    .on_hover_text("A ready-made set of the switches below, for the sheet each trade expects. Custom once a switch is changed by hand; picking a set puts them all back")
            });
            let mut ground = sv.ground;
            crate::props::row(ui, "Ground", |ui| {
                egui::ComboBox::from_id_salt("view.site.ground")
                .icon(crate::props::combo_icon)
                .selected_text(ground.label())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for g in GroundState::ALL {
                        ui.selectable_value(&mut ground, g, g.label());
                    }
                })
                .response
                .on_hover_text("Before shows the ground as surveyed, with no works and no roads. After shows it with the works in it; with the ground as it was, the old contours are dashed where the works changed them")
            });
            if ground != sv.ground {
                sv.ground = ground;
                label = "Ground shown";
            }
            let mut tick = |ui: &mut egui::Ui, on: &mut bool, name: &str, word: &str, tip: &str| {
                if crate::props::check(ui, name, on, word)
                    .on_hover_text(tip)
                    .changed()
                {
                    label = "Site view";
                }
            };
            tick(
                ui,
                &mut sv.contours,
                "Contours",
                "Shown",
                "Contour lines, and the old ones dashed where the ground was changed",
            );
            // how this view draws the ground: its own, or what the
            // terrain says until something here is set
            let terrain = self.doc.model().iter().find_map(|e| match &e.kind {
                ElementKind::Site(s) => match &s.form {
                    creabim_core::SiteForm::Terrain { contour, data, .. } => Some((
                        s.clone(),
                        creabim_core::GroundLook::of_terrain(*contour, data),
                    )),
                    _ => None,
                },
                _ => None,
            });
            let mut look_set = false;
            let mut reading: Vec<crate::props::Worked> = vec![];
            if let Some((_, from_terrain)) = &terrain {
                let was = sv.look.clone().unwrap_or_else(|| from_terrain.clone());
                let mut look = was.clone();
                if sv.contours {
                    crate::props::row(ui, "Interval", |ui| {
                        crate::props::fill(ui, crate::units::drag(&mut look.interval).range(0.0..=50.0))
                            .on_hover_text("A contour every so much height in this view; 0 draws none. A 1:500 site plan and a 1:100 plan can differ")
                    });
                    let mut every = look.major_every();
                    let changed = crate::props::row(ui, "Heavier", |ui| {
                        crate::props::fill(
                            ui,
                            egui::DragValue::new(&mut every)
                                .range(1..=20)
                                .prefix("every "),
                        )
                        .on_hover_text(
                            "Every so many contours one is drawn heavier, and can carry its height",
                        )
                        .changed()
                    });
                    if changed {
                        look.major = every;
                    }
                }
                if look != was {
                    sv.look = Some(look);
                    look_set = true;
                }
            }
            tick(
                ui,
                &mut sv.levels,
                "Levels",
                "And heights",
                "Contour heights, spot levels, and the levels and grades of roads and drains",
            );
            if let Some((_, from_terrain)) = &terrain {
                let was = sv.look.clone().unwrap_or_else(|| from_terrain.clone());
                let mut look = was.clone();
                if sv.levels {
                    crate::props::check(ui, "Heights", &mut look.heights, "On heavier")
                        .on_hover_text("Each heavier contour carries its height, the line broken round it. Sized on paper, so it reads at any scale");
                    crate::props::check(ui, "Spots", &mut look.spots, "Survey's")
                        .on_hover_text("Each of the survey's spot heights carries its level; ones too close together to read are left out");
                }
                let mut how = look.shade;
                crate::props::Combo::salted("Shade by", "view-ground-shade")
                    .selected_text(how.label())
                    .show_ui(ui, |ui| {
                        for k in creabim_core::GroundAnalysis::ALL {
                            ui.selectable_value(&mut how, k, k.label());
                        }
                    });
                look.shade = how;
                crate::props::check(ui, "Water", &mut look.flow, "Flow arrows")
                    .on_hover_text("Arrows show which way water runs off the ground, and circles mark the low points where it gathers and would pond");
                if look != was {
                    sv.look = Some(look);
                    look_set = true;
                }
            }
            if let Some((ts, _)) = &terrain {
                // what the shading reads, in this view: the block's plate
                if let Some((sh, lows)) =
                    creabim_geom::parts::ground_reading(self.doc.model(), ts, &sv)
                {
                    for b in sh.bands.iter().filter(|b| b.area > 1e-6) {
                        reading.push(crate::props::Worked::new(
                            b.label.clone(),
                            crate::units::area(b.area),
                        ));
                    }
                    if sv.look.as_ref().is_some_and(|l| l.flow) {
                        reading.push(crate::props::Worked::new("Low points", lows.to_string()));
                    }
                }
            }
            tick(
                ui,
                &mut sv.cut_fill,
                "Cut, fill",
                "Coloured",
                "Where the ground is dug (orange) or made up (teal), deeper for more",
            );
            tick(
                ui,
                &mut sv.banks,
                "Banks",
                "Hatched",
                "The banks of pads and roads, hatched from their top",
            );
            tick(
                ui,
                &mut sv.limit,
                "Limit",
                "Of works",
                "A dashed line round everything the works change",
            );
            tick(
                ui,
                &mut sv.legal,
                "Property",
                "Lines",
                "Property lines and easements",
            );
            // a property's setback lines and bearings, in this view
            let rules = self.doc.model().iter().find_map(|e| match &e.kind {
                ElementKind::Site(s) => match &s.form {
                    creabim_core::SiteForm::Property { rules, .. } => Some(rules.clone()),
                    _ => None,
                },
                _ => None,
            });
            let mut lot_set = false;
            if let (Some(rules), true) = (&rules, sv.legal) {
                let mut on = sv.setbacks_of(rules);
                if crate::props::check(ui, "Setbacks", &mut on, "And buildable")
                    .on_hover_text("Setback lines and the buildable area, in this view. The Setbacks check runs either way")
                    .changed()
                {
                    sv.setbacks = Some(on);
                    lot_set = true;
                }
                let mut on = sv.bearings_of(rules);
                if crate::props::check(ui, "Bearings", &mut on, "On edges")
                    .on_hover_text(
                        "Each edge's bearing and length written beside it, as a plat shows them",
                    )
                    .changed()
                {
                    sv.bearings = Some(on);
                    lot_set = true;
                }
                if crate::props::check(ui, "Labels", &mut sv.along, "Along the line")
                    .on_hover_text("Bearings, lengths and setbacks written parallel to their edge, turned to read from below or the right. Off, they are written level")
                    .changed()
                {
                    lot_set = true;
                }
            }
            tick(
                ui,
                &mut sv.roads,
                "Roads",
                "Shown",
                "Roads, drives and car parks",
            );
            tick(
                ui,
                &mut sv.services,
                "Services",
                "Drains, tanks",
                "Drain runs with their chambers, levels and gradients, and tanks",
            );
            tick(
                ui,
                &mut sv.tints,
                "Tints",
                "Coloured",
                "The site's ground, pads, paths, roads and buildable area in colour. Off, in greys of the same lightness, for a plan in black and white; the drawing palette (Project › Palette) applies after it",
            );
            tick(
                ui,
                &mut sv.investigation,
                "Soils",
                "Boreholes",
                "Where the soils report looked: boreholes, trial pits and probes with their ground levels and depths",
            );
            crate::props::worked_out(ui, "view.site.reading", &reading);
            if look_set {
                label = "Ground shown";
            } else if lot_set {
                label = "Property shown";
            }
            if sv.cut_fill {
                crate::props::note(
                    ui,
                    "Orange is dug, teal made up; the deeper, the stronger, full at 1.5 m.",
                );
            }
            if !label.is_empty() {
                let r = self.doc.edit(v, label, |e| {
                    if let ElementKind::View(x) = &mut e.kind {
                        x.site = sv.clone();
                    }
                });
                self.report(r, format!("{label} set for this view."));
            }
        });
    }

    /// The site settings of a section, an elevation or a 3D view: the
    /// ground shown, and for a section its cut and fill and level tags.
    fn site_view_ground_only(
        &mut self,
        ui: &mut egui::Ui,
        v: ElementId,
        mut sv: creabim_core::SiteView,
        section: bool,
    ) {
        use creabim_core::GroundState;
        let mut label = "";
        let mut ground = sv.ground;
        crate::props::row(ui, "Ground", |ui| {
            egui::ComboBox::from_id_salt("view.site.ground")
                .icon(crate::props::combo_icon)
                .selected_text(ground.label())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for g in GroundState::ALL {
                        ui.selectable_value(&mut ground, g, g.label());
                    }
                })
                .response
                .on_hover_text(if section {
                    "Before cuts the ground as surveyed, with no roads. After cuts it with its works, the ground as it was dashed under them; with both, that line is grey and runs on in elevations"
                } else {
                    "Before shows the ground as surveyed, with no roads"
                })
        });
        if ground != sv.ground {
            sv.ground = ground;
            label = "Ground shown";
        }
        if section {
            if crate::props::switch(ui, &mut sv.levels, "Level tags")
                .on_hover_text("Where the works change the ground: the level after them bold, and before them light under it")
                .changed()
            {
                label = "Site view";
            }
            if crate::props::switch(ui, &mut sv.cut_fill, "Cut and fill colours")
                .on_hover_text("Between the ground as it was and as built: orange where dug, teal where made up")
                .changed()
            {
                label = "Site view";
            }
        }
        if !label.is_empty() {
            let r = self.doc.edit(v, label, |e| {
                if let ElementKind::View(x) = &mut e.kind {
                    x.site = sv.clone();
                }
            });
            self.report(r, format!("{label} set for this view."));
        }
    }
}

/// What a click in a Booleans box asked for.
enum CutAct {
    Op(ElementId, BoolOp),
    Reach(ElementId, creabim_core::Reach),
    Remove(ElementId),
    Select(ElementId),
}

/// A boolean's kind as the strip names it.
fn op_name(op: BoolOp) -> &'static str {
    match op {
        BoolOp::Union => "Add",
        BoolOp::Difference => "Subtract",
        BoolOp::Intersection => "Intersect",
    }
}

/// One boolean as a folded box: its kind and the other element in the
/// title; open, the other as a chip (× lets go), then Kind and Reach.
fn cut_box(
    ui: &mut egui::Ui,
    (cid, k, other, name): &(ElementId, Cut, ElementId, String),
    side: &str,
    open: bool,
    act: &mut Option<CutAct>,
) {
    let title = format!("{} · {}", op_name(k.op), name);
    crate::props::sub_box(ui, &format!("cut.{}", cid.0), &title, None, open, |ui| {
        let r = crate::props::relation(
            ui,
            side,
            name,
            "Select it",
            Some("Remove this boolean: the shape lets go"),
        );
        if r.select {
            *act = Some(CutAct::Select(*other));
        }
        if r.let_go {
            *act = Some(CutAct::Remove(*cid));
        }
        let mut op = k.op;
        if crate::props::choice(
            ui,
            "Kind",
            &mut op,
            &[
                (BoolOp::Union, "Add", "Join the shape on"),
                (BoolOp::Difference, "Subtract", "Cut the shape out"),
                (
                    BoolOp::Intersection,
                    "Intersect",
                    "Keep only what both hold",
                ),
            ],
        ) {
            *act = Some(CutAct::Op(*cid, op));
        }
        let mut reach = k.reach;
        let opts: Vec<(creabim_core::Reach, &str, &str)> = creabim_core::Reach::ALL
            .iter()
            .map(|r| {
                let tip = match r {
                    creabim_core::Reach::AsDrawn => "As far as the shape is drawn",
                    creabim_core::Reach::Up => "Its top runs on up without end",
                    creabim_core::Reach::Down => "Its bottom runs on down without end",
                    creabim_core::Reach::Both => "Up and down without end: every floor",
                };
                (*r, r.label(), tip)
            })
            .collect();
        if crate::props::choice(ui, "Reach", &mut reach, &opts) {
            *act = Some(CutAct::Reach(*cid, reach));
        }
    });
}
/// The latitude the rendered modes' sun takes when the project has no
/// place on the map: Paris.
const DEFAULT_LATITUDE: f64 = 48.85;

/// A window's panes as one row, across × up: two fields side by side, so
/// the labels "Panes across" and "Panes up" no longer get cut at the
/// narrowest panel (owner check, 2026-11-21).
pub(crate) fn panes_pair(ui: &mut egui::Ui, across: &mut u32, up: &mut u32) -> (bool, bool) {
    ui.horizontal(|ui| {
        let gap = ui.spacing().item_spacing.x;
        // the × between the fields, about a body letter wide
        let x = ui.text_style_height(&egui::TextStyle::Body) * 0.6;
        let w = ((ui.available_width() - x - 2.0 * gap) / 2.0)
            .floor()
            .max(24.0);
        let a = ui
            .add_sized(
                [w, ui.spacing().interact_size.y],
                egui::DragValue::new(across).range(1..=12),
            )
            .on_hover_text("Panes across, split by upright glazing bars")
            .changed();
        ui.label("×");
        let b = ui
            .add_sized(
                [w, ui.spacing().interact_size.y],
                egui::DragValue::new(up).range(1..=12),
            )
            .on_hover_text("Panes up, split by level glazing bars")
            .changed();
        (a, b)
    })
    .inner
}

/// What the Site section's Set drop-down reads: the ready-made set the
/// view's switches match, or Custom when they were changed by hand.
fn site_set_name(set: Option<creabim_core::SitePreset>) -> &'static str {
    set.map_or("Custom", |p| p.label())
}

#[cfg(test)]
mod site_set_tests {
    use creabim_core::{SitePreset, SiteView};

    #[test]
    fn a_view_changed_by_hand_reads_custom_and_a_set_reads_its_name() {
        for p in SitePreset::ALL {
            let v = SiteView::default().with_preset(p);
            assert_eq!(super::site_set_name(SitePreset::of(&v)), p.label());
        }
        let mut v = SiteView::default().with_preset(SitePreset::Permit);
        v.contours = !v.contours;
        v.levels = !v.levels;
        v.roads = !v.roads;
        if SitePreset::of(&v).is_none() {
            assert_eq!(super::site_set_name(SitePreset::of(&v)), "Custom");
        } else {
            panic!("three switches flipped still match a set");
        }
    }
}

#[cfg(test)]
mod props_fit_tests {
    use super::*;

    /// What Properties paints past its right edge for the element named
    /// `name`, with the panel `width` px wide: each a short description.
    /// Laid out as the window lays it (the tab strip, the gap, the scroll
    /// bar's gutter), headless, a few frames so folds settle.
    pub(crate) fn spills(
        app: &mut CreaBimApp,
        name: &str,
        width: f32,
        open: &[&str],
    ) -> Vec<String> {
        // by name, or a note by the start of its text
        let id = app
            .doc
            .model()
            .iter()
            .find(|e| {
                e.name == name
                    || matches!(&e.kind, ElementKind::Note(n) if !name.is_empty() && n.text.starts_with(name))
            })
            .unwrap_or_else(|| panic!("{name}"))
            .id;
        app.selection.only(id);
        app.prop_tab = PropTab::Element;
        let ctx = egui::Context::default();
        crate::annotate::install_fonts(&ctx);
        for k in open {
            ctx.data_mut(|d| d.insert_persisted(egui::Id::new(*k), true));
        }
        // every text style's block open, as the owner had it
        for e in app.doc.model().iter() {
            if matches!(e.kind, ElementKind::TextStyle(_)) {
                ctx.data_mut(|d| d.insert_temp(egui::Id::new(("style.open", e.id)), true));
            }
        }
        // the content's width: the panel less its margin, the tab strip,
        // the gap after it and the scroll bar's gutter
        let content = width - 4.0 - 26.0 - 12.0 - 16.0;
        let mut found = vec![];
        for _ in 0..3 {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1600.0, 3000.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                // the panel's own look (see the right panel in `ui`)
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                ui.spacing_mut().interact_size.y = 24.0;
                for (style, font) in ui.style_mut().text_styles.iter_mut() {
                    font.size = match style {
                        egui::TextStyle::Small => 10.5,
                        egui::TextStyle::Heading => 14.0,
                        _ => (font.size - 1.5).clamp(11.0, 12.5),
                    };
                }
                ui.spacing_mut().button_padding = egui::vec2(6.0, 2.0);
                ui.set_max_width(content);
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 3.0);
                crate::props::boxes(ui, true);
                app.properties(ui);
                crate::props::close_box(ui);
                crate::props::boxes(ui, false);
            });
            out.textures_delta.clear();
            found = out
                .shapes
                .iter()
                .filter_map(|c| {
                    let r = c.shape.visual_bounding_rect();
                    // (a box's frame stands 6 px out either side, by design)
                    (r.is_finite() && r.right() > content + 1.0 && r.left() > -5.0).then(|| {
                        let what = match &c.shape {
                            egui::Shape::Text(t) => format!("text {:?}", t.galley.text()),
                            egui::Shape::Rect(_) => format!("box at {:.0}..", r.left()),
                            s => format!("{:?}", std::mem::discriminant(s)),
                        };
                        format!(
                            "{what} to {:.0} (edge {content:.0}) at y {:.0}",
                            r.right(),
                            r.top()
                        )
                    })
                })
                .collect();
        }
        found
    }

    /// Nothing in Properties runs past the panel's edge, at its narrowest
    /// and its usual width: a room tag with its text style open, a note,
    /// and a note reading an object's data (MU141: the Align buttons and
    /// the note's Object row pushed every row under them out).
    #[test]
    fn tag_and_note_properties_stay_inside_the_panel() {
        let mut app = CreaBimApp::blank(None);
        app.load_model(creabim_core::ridgeline::ridgeline_house());
        // the strip-footing note reading an object's data, as the owner had it
        let strip = app
            .doc
            .model()
            .iter()
            .find(
                |e| matches!(&e.kind, ElementKind::Note(n) if n.text.starts_with("STRIP FOOTINGS")),
            )
            .unwrap()
            .id;
        app.doc
            .edit(strip, "reads", |e| {
                if let ElementKind::Note(n) = &mut e.kind {
                    n.reads = Some(creabim_core::NoteReads::default());
                    n.text.push_str(
                        "
{Name}",
                    );
                }
            })
            .unwrap();
        for w in [220.0, 290.0] {
            for name in ["Room tag", "Note", "STRIP FOOTINGS", "Armchair"] {
                let s = spills(&mut app, name, w, &[]);
                assert!(s.is_empty(), "{name} at {w} px: {s:#?}");
            }
        }
    }
}

#[cfg(test)]
mod open_waits_for_nothing {
    use super::*;

    #[test]
    fn opening_a_project_never_waits_for_its_walls_and_slabs_to_be_joined() {
        // Highland Haven: framing the view built the whole joined scene
        // on the window's thread, 18 s of wall-slab cuts, before the
        // window was even shown. The frame needs only the extents, which
        // the cuts never grow, and the joined scene comes from the worker
        let mut app = CreaBimApp::blank(None);
        app.load_model(sample_house());
        assert!(
            creabim_geom::shared_scene_ready(app.doc.model()).is_none(),
            "opening built the joined scene on the window's thread"
        );
        // and the view is still framed on the house
        let (lo, hi) = creabim_geom::scene::scene_extents(app.doc.model()).unwrap();
        assert!((app.camera.target - (lo + hi) / 2.0).length() < 1e-3);
        assert!(app.camera.distance > (hi - lo).length() / 2.0);
    }
}

/// Data (IFC) › Kept from the IFC file: the relationships the file gave
/// element `guid` that CreaBIM doesn't model (its type when it came in as
/// a mesh, the assembly it is part of, its material set),
/// as read rows.
fn kept_relations(data: &creabim_core::ifcdata::IfcData, guid: &str) -> Vec<crate::props::ReadRow> {
    use crate::props::{Read, ReadRow};
    let name = |t: &str| data.name_of(t).unwrap_or_else(|| "(no name)".into());
    let class_of = |t: &str| data.entity(t).map(|k| k.class.clone()).unwrap_or_default();
    let mut rows = Vec::new();
    if let Some(r) = data.rels_of(guid, "IFCRELDEFINESBYTYPE").next() {
        rows.push(ReadRow::new(
            "Type",
            Read::Text(name(&r.relating)),
            format!("{} from the file", class_of(&r.relating)),
        ));
    }
    for class in ["IFCRELAGGREGATES", "IFCRELNESTS"] {
        if let Some(r) = data.rels_of(guid, class).next() {
            let whole = if r.relating.starts_with('@') {
                "an element of the model".to_string()
            } else {
                name(&r.relating)
            };
            rows.push(ReadRow::new(
                "Part of",
                Read::Text(whole),
                "The assembly it is part of in the file",
            ));
        }
    }
    if let Some(r) = data.rels_of(guid, "IFCRELASSOCIATESMATERIAL").next() {
        let kind = class_of(&r.relating).trim_start_matches("IFC").to_string();
        let kind = kind
            .replace("MATERIALLAYERSETUSAGE", "Layer set")
            .replace("MATERIALLAYERSET", "Layer set")
            .replace("MATERIALPROFILESETUSAGE", "Profile set")
            .replace("MATERIALPROFILESET", "Profile set")
            .replace("MATERIALCONSTITUENTSET", "Constituent set")
            .replace("MATERIALLIST", "Material list")
            .replace("MATERIAL", "Material");
        rows.push(ReadRow::new(
            "Material",
            Read::Text(kind),
            "As the file assigned it; written back for a mesh",
        ));
    }
    // its documents are listed by name in the Documents fold above
    rows
}

#[cfg(test)]
mod unsaved_tests {
    use super::*;

    /// A tool's live edit taken back (Scale, Move or Rotate cancelled with
    /// Esc, a grip let go where it started) leaves the project as it was
    /// saved. The title kept its unsaved mark: the model came back the
    /// same but with a new revision (MU143, seen after a cancelled Scale).
    #[test]
    fn a_cancelled_live_edit_leaves_the_project_saved() {
        let mut app = CreaBimApp::blank(None);
        app.load_model(creabim_core::sample::sample_house());
        app.mark_saved();
        assert!(!app.unsaved());
        let wall = app
            .doc
            .model()
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Wall(_)))
            .unwrap()
            .clone();
        let mut shown = wall.clone();
        shown.name = format!("{} scaled", wall.name);
        let mut live = crate::modal::LiveEdit::new("Scale", vec![wall.clone()]);
        live.update(&mut app.doc, vec![shown]).unwrap();
        assert!(app.unsaved(), "while the edit is shown");
        live.cancel(&mut app.doc);
        assert_eq!(app.doc.model().get(wall.id), Some(&wall));
        assert!(!app.unsaved(), "taken back: nothing to save");
        // a real edit is unsaved; undone back to the save, it is not
        app.doc
            .edit(wall.id, "Rename", |e| e.name = "Renamed".into())
            .unwrap();
        assert!(app.unsaved());
        assert!(app.doc.undo());
        assert!(!app.unsaved(), "undone back to what was saved");
        assert!(app.doc.redo());
        assert!(app.unsaved(), "and redone, unsaved again");
    }
}

#[cfg(test)]
mod unsaved_load_tests {
    use super::*;

    /// A model loaded without a file (a sample, the starter, an import) is
    /// unsaved, even after a project saved with no edits: both histories
    /// are empty, and the saved mark alone would match.
    #[test]
    fn a_model_loaded_without_a_file_is_unsaved() {
        let mut app = CreaBimApp::blank(None);
        app.load_model(creabim_core::sample::sample_house());
        app.mark_saved();
        assert!(!app.unsaved());
        app.load_model(creabim_core::sample::typed_house());
        assert!(app.unsaved());
    }
}
