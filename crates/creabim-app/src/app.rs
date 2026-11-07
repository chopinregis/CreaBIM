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
    ElementId, ElementKind, Hatch, Material, Model, Modifier, ModifierKind, ScheduleCategory,
    Sheet, Transaction, Underlay, View, ViewKind, ViewStyle, Workplane,
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
#[path = "drafting.rs"]
mod drafting;
#[path = "editui.rs"]
mod editui;
#[path = "gridvis.rs"]
mod gridvis;
#[path = "helpui.rs"]
pub(crate) mod helpui;
#[path = "importui.rs"]
mod importui;
#[path = "joinsui.rs"]
mod joinsui;
#[path = "levelcopy.rs"]
mod levelcopy;
#[path = "levelsui.rs"]
mod levelsui;
#[path = "marksui.rs"]
mod marksui;
#[path = "materials.rs"]
mod materials;
#[path = "objectsui.rs"]
mod objectsui;
#[path = "selectsimilar.rs"]
mod selectsimilar;
#[path = "shell.rs"]
mod shell;
#[path = "stylesui.rs"]
mod stylesui;
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
    selection: Selection,
    /// Selected face when a free mesh is selected (mesh edit mode).
    face: Option<usize>,
    /// What the 2D pane shows: a view (plan, section, elevation, 3D) or a sheet.
    active_view: Option<ElementId>,
    sheet: SheetState,
    /// Generated drawings by view, with the model revision they were made at.
    drawings: HashMap<ElementId, (u64, Drawing)>,
    /// Drawings and the 3D scene being built off the UI thread.
    builds: crate::builds::Builds,
    /// Each view's looks on sheets, with the model revision they are for.
    looks: HashMap<ElementId, (u64, std::sync::Arc<creabim_geom::looks::Looks>)>,
    /// The same for views seen in X-ray, which their sheets don't follow.
    xray_drawings: HashMap<ElementId, (u64, Drawing)>,
    camera: OrbitCamera,
    cube: crate::viewcube::ViewCube,
    /// Aspect ratio of the 3D view as last drawn, for framing.
    aspect: f32,
    viewport: Option<Viewport3d>,
    geometry_key: Option<GeometryKey>,
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
    /// Snaps for the 3D view: built from the scene without the listed
    /// elements, at a model revision.
    snaps_3d: Option<(u64, Vec<ElementId>, Snapper)>,
    /// First click of a two-click tool in the 3D view, on a level.
    pending_3d: Option<(DVec2, ElementId)>,
    /// Where the 3D right-click menu was opened, and the face there.
    menu_at_3d: Option<DVec3>,
    menu_plane_3d: Option<Workplane>,
    /// A workplane set on a face, through three points or off a level, with
    /// its name. The 3D sketch and guide tools draw on it instead of the
    /// level, and the grid lies on it.
    workplane: Option<(Workplane, String)>,
    /// Picking a workplane in the 3D view.
    plane_pick: Option<PlanePick>,
    /// First point of a guide being drawn on the workplane.
    guide_from: Option<DVec3>,
    extrude_distance: f64,
    path: String,
    /// The model's revision when it was last opened or saved: anything
    /// else is unsaved (A7-8).
    saved_revision: Option<u64>,
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
    /// What the 3D view leaves out (categories, grid groups, elements).
    /// Its categories are kept in the settings, the rest for the session.
    vis3d: creabim_core::ViewHidden,
    /// What Ctrl+C copied, for pasting when the system clipboard has none.
    clipboard: Vec<Element>,
    recent: crate::files::Recent,
    /// The web interface round the views.
    shell: shell::Shell,
    /// The material browser.
    mat: materials::MaterialsUi,
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
    /// What a box selection took, while it is still the selection: the
    /// status bar offers Keep only… for it.
    boxed: Option<Vec<ElementId>>,
    last_export_dir: Option<std::path::PathBuf>,
    /// The crop box whose elements clicks in the 3D view are picking.
    crop_pick: Option<ElementId>,
    /// The Settings window is open.
    settings_open: bool,
    /// The Project information window (A7-5).
    project_open: bool,
    /// The project information being edited, until Apply.
    project_draft: Option<creabim_core::ProjectInfo>,
    /// Export > IFC: the window asking how walls and slabs go out (MU20).
    ifc_dialog: bool,
    /// The New project window, with the classic panels.
    new_project_classic: bool,
}

/// How much of the accent a selected element takes in 3D (A7-2).
const SELECTION_TINT: f32 = 0.35;

impl CreaBimApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings = crate::files::Settings::load();
        let theme = crate::theme::Theme::from_id(&settings.theme).unwrap_or_default();
        crate::handles::set_grip_style(settings.grips);
        shell::style(&cc.egui_ctx, theme);
        let viewport = cc
            .wgpu_render_state
            .as_ref()
            .map(|rs| Viewport3d::new(&rs.device));
        let mut app = Self::blank(viewport);
        crate::actlog::start();
        app.builds.set_context(cc.egui_ctx.clone());
        app.shell.theme = theme;
        app.vis3d.categories = settings.hidden_3d.clone();
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
        }
        app.shell.settings = settings;
        app.load_model(creabim_core::starter::starter_house());
        app.status = STARTER_LOADED.into();
        // `creabim-app house.creabim` opens that file instead
        if let Some(path) = launch_file(std::env::args().skip(1)) {
            app.open_path(&path);
        }
        app
    }

    /// The app round an empty document, with default settings and no
    /// window: [`Self::new`] fills it in, and tests use it as it is.
    fn blank(viewport: Option<Viewport3d>) -> Self {
        Self {
            doc: Document::new(),
            selection: Selection::default(),
            face: None,
            active_view: None,
            sheet: SheetState::default(),
            drawings: HashMap::new(),
            builds: Default::default(),
            looks: HashMap::new(),
            xray_drawings: HashMap::new(),
            camera: OrbitCamera::default(),
            cube: Default::default(),
            aspect: 0.65,
            viewport,
            geometry_key: None,
            paper_is_dark: false,
            prop_tab: PropTab::Element,
            prop_seen: (Tool::Select, false),
            print_preview: false,
            scene: vec![],
            plan: PlanState::default(),
            placement: PlanPlacement::Docked,
            plan_in_3d: false,
            areas: Default::default(),
            areas_over: vec![],
            handle_drag: None,
            box_armed: false,
            box_3d: None,
            snaps_3d: None,
            pending_3d: None,
            menu_at_3d: None,
            menu_plane_3d: None,
            guide_from: None,
            workplane: None,
            plane_pick: None,
            extrude_distance: 1.0,
            path: "project.creabim".into(),
            saved_revision: None,
            title: String::new(),
            logged: (String::new(), None),
            watch: Default::default(),
            live_reload: true,
            hidden: Default::default(),
            hidden_rev: 0,
            vis3d: Default::default(),
            clipboard: vec![],
            recent: crate::files::Recent::load(),
            shell: Default::default(),
            mat: Default::default(),
            asm: Default::default(),
            types: Default::default(),
            type_preview: Default::default(),
            catalogue: Default::default(),
            levels: Default::default(),
            settings_open: false,
            project_open: false,
            project_draft: None,
            ifc_dialog: false,
            new_project_classic: false,
            marks: Default::default(),
            gstyles: Default::default(),
            level_copy: Default::default(),
            import_report: None,
            drafting: Default::default(),
            orbit_selection: false,
            rect_3d: None,
            rect_2d: None,
            last_in_2d: false,
            last_export: None,
            boxed: None,
            last_export_dir: None,
            crop_pick: None,
            status: "Loaded the sample house. Orbit with the middle button (or Alt + left drag), pan with right drag, zoom with the wheel; a left drag selects with a box.".into(),
        }
    }

    fn load_model(&mut self, model: Model) {
        self.doc = Document::from_model(with_plan_views(model));
        self.selection.clear();
        self.hidden.clear();
        self.hidden_rev += 1;
        self.face = None;
        self.crop_pick = None;
        self.active_view = None;
        self.sheet = SheetState::default();
        self.drawings.clear();
        self.looks.clear();
        self.xray_drawings.clear();
        self.plan.xray.clear();
        self.plan.hover = None;
        self.geometry_key = None;
        self.drop_stale_selection();
        self.frame_all();
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
                    let n = imported.model.len();
                    let warnings = imported.warnings.len();
                    let report = importui::ImportReport::new(&file_label(path), &imported);
                    for w in &imported.warnings {
                        eprintln!("IFC import: {w}");
                    }
                    for d in &imported.details {
                        eprintln!("IFC import:   {d}");
                    }
                    self.import_report = Some(report);
                    self.load_model(imported.model);
                    self.watch = Default::default();
                    self.recent.add(path);
                    self.path = with_extension(path, "creabim");
                    self.status = format!(
                        "Opened {} ({}): {n} elements{}. Save keeps it as {}.",
                        file_label(path),
                        imported.schema,
                        if warnings > 0 {
                            format!(", {warnings} notes (File › Import report)")
                        } else {
                            String::new()
                        },
                        file_label(&self.path)
                    );
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
            Ok(doc) => {
                let model = doc.model().clone();
                self.load_model(model.clone());
                // keep the saved undo history when no views had to be added
                if self.doc.model() == &model {
                    self.doc = doc;
                }
                self.watch = Default::default();
                self.recent.add(path);
                self.path = path.to_string();
                self.saved_revision = Some(self.doc.model().revision());
                self.status = format!("Opened {path}");
            }
            Err(e) => self.status = format!("Couldn't open {path}: {e}"),
        }
    }

    /// Has the model changed since it was opened or saved?
    pub(crate) fn unsaved(&self) -> bool {
        self.saved_revision != Some(self.doc.model().revision())
    }

    fn save(&mut self) {
        if self.path.to_ascii_lowercase().ends_with(".json") {
            let r = std::fs::write(&self.path, self.doc.model().to_json());
            if r.is_ok() {
                self.saved_revision = Some(self.doc.model().revision());
            }
            self.report(r, format!("Saved {}", self.path));
            return;
        }
        if !self.path.to_ascii_lowercase().ends_with(".creabim") {
            self.path = with_extension(&self.path, "creabim");
        }
        let r = creabim_store::save(&self.path, &self.doc);
        if r.is_ok() {
            let path = self.path.clone();
            self.recent.add(&path);
            self.saved_revision = Some(self.doc.model().revision());
        }
        self.report(r, format!("Saved {}", self.path));
    }

    fn open_dialog(&mut self) {
        if let Some(path) = crate::files::pick_open() {
            self.open_path(&path);
        }
    }

    fn save_as(&mut self) {
        let start = with_extension(&self.path, "creabim");
        if let Some(path) = crate::files::pick_save(&start, "Save the project as", "creabim") {
            self.path = path;
            self.save();
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

    /// "Assign as": give a mesh a building meaning, or change its class.
    fn assign_panel(&mut self, ui: &mut egui::Ui, id: ElementId) {
        let Some(e) = self.doc.model().get(id) else {
            return;
        };
        if !matches!(
            e.kind,
            ElementKind::FreeMesh(_)
                | ElementKind::Wall(_)
                | ElementKind::Slab(_)
                | ElementKind::Opening(_)
        ) {
            return;
        }
        if let Some(s) = &e.source {
            ui.label(
                egui::RichText::new(format!("From {}: {}", file_label(&s.file), s.object)).weak(),
            );
        }
        let is_mesh = matches!(e.kind, ElementKind::FreeMesh(_));
        let mut pick = None;
        egui::ComboBox::from_id_salt("assign_as")
            .icon(crate::props::combo_icon)
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
        if is_mesh {
            ui.label(
                egui::RichText::new(
                    "Make this shape a wall, slab, door or window sized to fit it, or give it a class.",
                )
                .weak(),
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
        let rev = model.revision();
        let looks = &mut self.looks;
        sheets::placed_views_with(
            model,
            sheet,
            &mut |v| cache.get(&v).map(|(_, d)| d.clone()),
            &mut |id, v| match looks.get(&id) {
                Some((r, l)) if *r == rev => l.clone(),
                _ => {
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
        let revision = self.doc.model().revision();
        let xray = self.plan.xray.contains(&id);
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

    /// The drawing of a view to show now: as [`Self::drawing`] when it is
    /// up to date or cheap to build, else the last one finished while a
    /// worker builds the new one (see [`crate::builds`]). Exports and
    /// anything measured from a drawing use [`Self::drawing`].
    fn drawing_live(&mut self, id: ElementId) -> Option<Drawing> {
        for b in self.builds.finished() {
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
        let revision = self.doc.model().revision();
        let xray = self.plan.xray.contains(&id);
        let key = (id, xray);
        let held = if xray {
            self.xray_drawings.get(&id)
        } else {
            self.drawings.get(&id)
        };
        match held {
            Some((r, d)) if *r == revision => Some(d.clone()),
            // the view being worked in, and every plan, are built on the
            // spot: their lines must follow a drag frame by frame, and a
            // plan (about 15 ms) sat on the edge of "cheap", so it could go
            // to a worker and move in steps
            Some((_, d))
                if self.builds.enabled()
                    && !self.builds.cheap(key)
                    && Some(id) != self.active_view
                    && !matches!(self.view(id).map(|v| &v.kind), Some(ViewKind::Plan { .. })) =>
            {
                let d = d.clone();
                self.builds.start(self.doc.model(), key);
                Some(d)
            }
            _ => self.drawing(id),
        }
    }

    /// The box round the selected elements in 3D.
    fn selection_bounds(&self) -> Option<(GVec3, GVec3)> {
        self.scene
            .iter()
            .filter(|s| self.selection.contains(s.id))
            .filter_map(|s| s.mesh.bounds())
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
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
        let scene = (*creabim_geom::shared_scene(self.doc.model())).clone();
        let bounds = scene
            .iter()
            .filter_map(|s| s.mesh.bounds())
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)));
        if let Some((lo, hi)) = bounds {
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
            Command::Move
            | Command::Rotate
            | Command::Scale
            | Command::Mirror
            | Command::Offset
            | Command::Duplicate => {}
        }
    }

    /// Get Move, Rotate, Scale or Duplicate ready: the view the pointer
    /// goes into starts it. With nothing selected, Move and Rotate first
    /// ask what to work on.
    fn arm(&mut self, c: Command, m: (crate::modal::Mode, bool)) {
        use crate::modal::Mode;
        let picks_first = matches!(m, (Mode::Move | Mode::Rotate, false));
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
                "Duplicate: point at a view and click a base point; the copy follows to where you click next. Esc leaves it in place.".into()
            }
            _ => format!(
                "{}: point at a view and move the pointer, then click to finish. Right-click cancels.",
                c.label()
            ),
        };
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        // a length typed into a grip's drag takes the keys
        if ctx.egui_wants_keyboard_input() || self.plan.grips.typing {
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
        // Shift+G: guides on and off everywhere; Alt+Z: X-ray in 3D
        let (guides_key, xray_key) = ctx.input(|i| {
            let m = i.modifiers;
            (
                m.shift && !m.command && !m.alt && i.key_pressed(egui::Key::G),
                m.alt && !m.command && !m.shift && i.key_pressed(egui::Key::Z),
            )
        });
        if guides_key && self.plan.modal.is_none() {
            self.set_guides_hidden(!self.shell.settings.guides_hidden);
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
        // Enter opens the selected element in Edit mode, and closes it again
        let enter = ctx.input(|i| i.modifiers.is_none() && i.key_pressed(egui::Key::Enter));
        if enter
            && self.plan.tool == Tool::Select
            && self.plan.modal.is_none()
            && !self.plan.busy()
            && self.plan.outline.is_none()
        {
            self.toggle_edit_mode();
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
                let tab = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
                if tab {
                    let on = !handles::layers();
                    handles::set_layers(on);
                    self.status = if on {
                        match self.doc.model().get(id).map(|e| &e.kind) {
                            Some(ElementKind::Wall(_)) => "Layers: drag a layer's end to run it on or hold it back. Tab: back to the element's grips.",
                            _ => "Layers: drag a layer's edge to push it out past the edge or pull it in. Tab: back to the element's grips.",
                        }
                        .into()
                    } else {
                        editmode::enter(self.doc.model(), id, &mut self.plan.edit)
                    };
                }
            }
        }
        // M: the Move tool
        let m_key = ctx.input(|i| {
            !i.modifiers.command
                && !i.modifiers.alt
                && !i.modifiers.shift
                && i.key_pressed(egui::Key::M)
        });
        if m_key && self.plan.modal.is_none() && !self.plan.busy() {
            self.run_command(ctx, Command::Move, None);
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
                self.frame_selection();
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
        if let Some(project) = similar.filter(|_| self.plan.tool == Tool::Select) {
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
        for e in &events {
            match e {
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
        let tx =
            Transaction::new(format!("Delete {name}")).remove_all_cascade(self.doc.model(), &ids);
        let r = self.doc.commit(tx);
        self.report(r, format!("Deleted {name} and anything attached to it."));
        self.drop_stale_selection();
    }

    /// Everything that can be picked: building elements, sketches, guides,
    /// annotations, but not levels, views, sheets or materials.
    fn select_all(&mut self) {
        let ids: Vec<ElementId> = self
            .doc
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
            .collect();
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

    fn rebuild_geometry(&mut self, rs: &egui_wgpu::RenderState) {
        // the scene of this revision: shared if built, else the newest a
        // worker has finished while it builds this one, so an edit or a
        // drag never waits for the 3D view (A6-1.2); the first is built here
        let model = self.doc.model();
        let shown = self.geometry_key.as_ref().map(|k| k.0 .0);
        let (scene_rev, scene) = match creabim_geom::shared_scene_ready(model) {
            Some(s) => (model.revision(), s),
            None if shown.is_none() || !self.builds.enabled() => {
                (model.revision(), creabim_geom::shared_scene(model))
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
        let key = (
            (scene_rev, self.hidden_rev),
            self.selection.clone(),
            self.face,
            self.plan_in_3d,
            self.active_view,
        );
        if self.geometry_key.as_ref() == Some(&key) {
            return;
        }
        self.geometry_key = Some(key);
        self.scene = (*scene).clone();
        let left_out = self.hidden_in_3d();
        self.scene
            .retain(|i| !self.hidden.contains(&i.id) && !left_out.contains(&i.id));
        let cut_edges = self.crop_scene_3d();
        snaps::begin_edge_round();

        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut see_through = Vec::new();
        // the selection in the interaction colour every view and grip uses
        // (A7-2): a tint over the element's own colour and a crisp
        // outline, never a repaint
        let highlight = {
            let a = crate::planview::ACCENT_SELECTED;
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
        let edge_colour = [0.10, 0.10, 0.11];
        // a type shown in the model: its instances lit as if selected, the
        // rest see-through when asked
        let spot = &self.plan.spotlight;
        let ghost = self.plan.spotlight_ghost && !spot.is_empty();
        for item in &self.scene {
            let selected = self.selection.contains(item.id) || spot.contains(&item.id);
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
                // cutters show as their outline only
                let c = if selected {
                    highlight
                } else {
                    [0.35, 0.45, 0.75]
                };
                let out = if selected {
                    &mut on_top
                } else {
                    &mut ghost_lines
                };
                for &(a, b) in snaps::feature_edges_cached(&item.mesh).iter() {
                    out.push((a, b, c));
                }
                continue;
            }
            // a selected element is outlined whatever the shading shows
            if (mode.edges() || selected) && !xrayed {
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
                selected,
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
            let color = match (self.paper_is_dark, face_plane) {
                (false, true) => [0.88, 0.89, 0.91],
                (false, false) => [0.80, 0.80, 0.82],
                (true, true) => [0.18, 0.19, 0.21],
                (true, false) => [0.26, 0.27, 0.30],
            };
            lines.extend(grid_lines(
                level_plane,
                -reach + 5.0,
                reach + 5.0,
                spacing,
                color,
            ));
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
                    ViewKind::Axonometric { .. } | ViewKind::Schedule { .. } => None,
                };
                let to_world = to_world.unwrap_or_else(|| Box::new(|_| [0.0; 3]));
                let plan = if matches!(
                    kind,
                    ViewKind::Axonometric { .. } | ViewKind::Schedule { .. }
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

    fn show_3d(&mut self, ui: &mut egui::Ui, rect: Rect, frame: &eframe::Frame) {
        self.rect_3d = Some(rect);
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
        let modal_here = self.plan.modal.as_ref().is_some_and(|m| m.pane.is_none());
        if self.cube.step(&mut self.camera, ui.input(|i| i.time)) {
            ui.ctx().request_repaint();
        }
        // the mouse (MU19): right drag pans, and so does Shift + middle;
        // middle drag orbits (Classic: middle pans too)
        let classic = self.shell.settings.navigation == "classic";
        let shift_now = ui.input(|i| i.modifiers.shift);
        if response.dragged_by(egui::PointerButton::Secondary)
            || (response.dragged_by(egui::PointerButton::Middle) && (classic || shift_now))
        {
            let d = response.drag_delta();
            self.camera.pan(GVec2::new(d.x, d.y), rect.height());
        }
        if !classic && response.dragged_by(egui::PointerButton::Middle) && !shift_now {
            self.orbit_3d(response.drag_delta());
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
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

        self.rebuild_geometry(rs);
        let ppp = ui.ctx().pixels_per_point();
        let size = [(rect.width() * ppp) as u32, (rect.height() * ppp) as u32];
        if let Some(vp) = &mut self.viewport {
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
        ui.painter().text(
            rect.left_top() + Vec2::new(8.0, 8.0),
            egui::Align2::LEFT_TOP,
            "3D",
            egui::FontId::proportional(13.0),
            Color32::from_gray(80),
        );

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
        self.draw_sketches(&painter, &project);
        self.draw_grid_bubbles(&painter, &project);
        self.draw_crop_frames(&painter, &project);
        self.hover_3d(ui, &response, &painter, &project, &ray, modal_here);
        self.view_cube(ui, rect);

        // the Move and Rotate tools, or a keyboard move, rotate or scale,
        // running here
        let starting = response.hovered()
            && self.plan.modal.is_none()
            && !ui.ctx().egui_wants_keyboard_input()
            && self.selection.is_some()
            && (self.plan.armed.is_some() || planview::start_keys(ui.ctx()).is_some());
        if modal_here || starting {
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
            self.refresh_snaps_3d(&exclude);
        }
        if modal_here {
            // the view still orbits, pans and zooms between clicks
            if response.dragged_by(egui::PointerButton::Primary) {
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
            let faces_on = self
                .plan
                .modal
                .as_ref()
                .is_some_and(|m| m.works_on == crate::modal::Target::Face);
            let masses: HashMap<ElementId, creabim_core::FreeMesh> = if faces_on {
                self.doc
                    .model()
                    .iter()
                    .filter_map(|e| match &e.kind {
                        ElementKind::FreeMesh(m) => Some((e.id, m.clone())),
                        _ => None,
                    })
                    .collect()
            } else {
                HashMap::new()
            };
            let pick_face = |p: egui::Pos2| -> Option<crate::modal::FacePick> {
                let (o, d) = ray(p);
                let f = pick::pick_face(scene, o.as_vec3(), d.as_vec3())?;
                let mesh_face = match masses.get(&f.hit.id) {
                    Some(m) => {
                        let ev = m.evaluated();
                        let face = pick::face_of_triangle(&ev.mesh, f.hit.triangle)?;
                        ev.source.get(face).copied().flatten()
                    }
                    _ => None,
                };
                Some(crate::modal::FacePick {
                    id: f.hit.id,
                    face: creabim_core::pushpull::FaceRef {
                        point: f.point,
                        normal: f.normal,
                        mesh_face,
                    },
                    tris: f.triangles,
                })
            };
            let map = PaneMap {
                to_model: &to_model,
                project: &project,
                look: ray(rect.center()).1,
                snap: &snap,
                pick_face: Some(&pick_face),
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
                match m.update(
                    &mut self.doc,
                    &ctx,
                    &painter,
                    hover,
                    response.clicked(),
                    right,
                    &map,
                ) {
                    Outcome::Running(s) => self.status = s,
                    Outcome::Done(s) | Outcome::Cancelled(s) => {
                        self.status = s;
                        self.plan.modal = None;
                    }
                }
                if let Some(m) = &self.plan.modal {
                    self.plan.move_target = m.works_on;
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
                } else if !self.plan.step_back() && right && self.plan.tool == Tool::Select {
                    if self.plan.web_menus {
                        self.plan.menu_request = response.interact_pointer_pos();
                        self.shell.menu_3d = true;
                    }
                    if let Some(p) = response.interact_pointer_pos() {
                        let (o, d) = ray(p);
                        let hit = pick::pick(&self.scene, o.as_vec3(), d.as_vec3());
                        if let Some(h) = hit.as_ref().filter(|h| !self.selection.contains(h.id)) {
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
                    let (o, d) = ray(p0);
                    !pick::pick(&self.scene, o.as_vec3(), d.as_vec3())
                        .is_some_and(|h| self.selection.contains(h.id))
                });
            if (self.box_armed || auto_box)
                && response.drag_started_by(egui::PointerButton::Primary)
            {
                self.box_3d = ui.input(|i| i.pointer.press_origin());
                self.box_armed = false;
            }
            if let Some(p0) = self.box_3d {
                let now = ui.input(|i| i.pointer.latest_pos()).unwrap_or(p0);
                crate::boxsel::draw(&painter, p0, now);
                if response.drag_stopped() || !response.dragged() {
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
                    let ids: Vec<ElementId> = crate::boxsel::pick_triangles(p0, now, tris)
                        .into_iter()
                        .filter(|id| planview::box_selectable(&self.doc, *id))
                        .collect();
                    let shift = ui.input(|i| i.modifiers.shift);
                    self.selection.extend(ids, shift);
                    self.face = None;
                    self.box_3d = None;
                    self.status = format!("{} selected.", self.selection.len());
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
        self.refresh_snaps_3d(&exclude);
        let snap_cfg = self.plan.snap.live(ui.input(|i| i.modifiers.alt));
        // a grid line pressed on, to drag it square to itself
        let grid_hit = ui
            .input(|i| i.pointer.press_origin())
            .zip(self.selection.active)
            .and_then(|(p, id)| Some((id, self.grid_grab(p, id, &project)?)));
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
            busy
        };
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
        if response.clicked() && !on_cube {
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
                // and so are crop box frames
                let frame = || self.crop_frame_near(p, &project);
                if let Some(id) = near_sketch.or(bubble).or_else(frame) {
                    pick(&mut self.selection, Some(id));
                    self.face = None;
                    return;
                }
                let hit = self.first_hit_3d(o, d);
                // guides and grid lines cross everything, so they only take
                // clicks that miss
                if hit.is_none() {
                    let near = self
                        .grid_line_near(p, &project)
                        .map(|g| g.0)
                        .or_else(|| self.guide_near(p, &project));
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
        if hits.is_empty() {
            let near = self
                .grid_line_near(p, project)
                .map(|g| g.0)
                .or_else(|| self.guide_near(p, project));
            out.extend(near);
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
        let lit = hoverpick::highlighted(&self.plan.hover)
            .filter(|id| !self.selection.contains(*id) && !response.dragged());
        if let Some(id) = lit {
            let stroke = egui::Stroke::new(2.0, hoverpick::HOVER.gamma_multiply(0.85));
            let seg = |a: DVec3, b: DVec3| {
                if let (Some(a), Some(b)) = (project(a), project(b)) {
                    painter.line_segment([a, b], stroke);
                }
            };
            for item in self.scene.iter().filter(|i| i.id == id) {
                for (a, b) in snaps::feature_edges(&item.mesh) {
                    seg(a, b);
                }
            }
            for (sid, a, b) in creabim_geom::scene::sketch_lines(self.doc.model()) {
                if sid == id {
                    seg(a, b);
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
        let rev = self.doc.model().revision();
        let frozen = self.handle_drag.is_some()
            || self
                .plan
                .modal
                .as_ref()
                .is_some_and(|m| m.pane.is_none() && m.freezes_snaps());
        let fresh = self
            .snaps_3d
            .as_ref()
            .is_some_and(|(r, ex, _)| ex == exclude && (*r == rev || frozen));
        if !fresh {
            let mut snapper = Snapper::from_scene(&self.scene, exclude);
            snapper.add_guides(self.doc.model(), GUIDE_REACH_3D);
            snapper.add_grids(self.doc.model(), &self.hidden_in_3d());
            snapper.add_sketches(self.doc.model(), exclude);
            self.snaps_3d = Some((rev, exclude.to_vec(), snapper));
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
        let accent = Color32::from_rgb(230, 120, 30);
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
        let accent = Color32::from_rgb(230, 120, 30);
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
        let accent = Color32::from_rgb(230, 120, 30);
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
                    Some(g) => crate::ghost::draw(painter, &project, &g, accent),
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

        // everything else is drawn on the working level
        let from = self
            .pending_3d
            .filter(|(_, l)| *l == level)
            .map(|(p, _)| p.extend(z));
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
        let track = snaps::track_points(&chain, &[]);
        let Some((snap, align)) =
            snapper.snap_tracked(&snap_cfg, hover, project, from, &track, free)
        else {
            return;
        };
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
            } else {
                snaps::draw_tracked(painter, project, snap, align, c, from.and_then(project));
            }
        }
        if from.is_none() && crate::ghost::shows(tool) {
            if let Some(g) = crate::ghost::cached(self.doc.model(), level, tool, p, &self.plan) {
                crate::ghost::draw(painter, &project, &g, accent);
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
                    let (through, dir) = match self.plan.guide_edge {
                        Some((e0, e1)) => (at, (e1 - e0).extend(0.0)),
                        None => (a, at - a),
                    };
                    draw_guide_3d(painter, project, through, through + dir, stroke);
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
                    if let (Some(sa), Some(sb)) = (project(a), project(at)) {
                        painter.line_segment([sa, sb], stroke);
                        painter.text(
                            sb + Vec2::new(10.0, -12.0),
                            egui::Align2::LEFT_BOTTOM,
                            crate::units::len(a.truncate().distance(p)),
                            egui::FontId::proportional(13.0),
                            accent,
                        );
                    }
                    if tool == Tool::Wall {
                        // the wall's height, so it reads as a wall
                        let h = self.doc.model().level(level).map_or(3.0, |l| l.height);
                        let up = DVec3::Z * h;
                        let quad = [a, at, at + up, a + up, a];
                        let scr: Vec<egui::Pos2> =
                            quad.iter().filter_map(|q| project(*q)).collect();
                        painter.add(egui::Shape::line(scr, egui::Stroke::new(1.0, accent)));
                    }
                }
            }
        }
        if response.clicked() && tool == Tool::Guide && from.is_none() {
            // clicking an edge first draws the guide parallel to it
            self.plan.guide_edge = matches!(
                snap.kind,
                SnapKind::OnEdge | SnapKind::Guide | SnapKind::Midpoint
            )
            .then(|| snapper.edge_at(snap.p))
            .flatten()
            .filter(|(a, b)| a.truncate().distance(b.truncate()) > 1e-3)
            .map(|(a, b)| (a.truncate(), b.truncate()));
        }
        if response.clicked() {
            let mut sel = self.selection.clone();
            let mut state = std::mem::take(&mut self.plan);
            state.pending = self.pending_3d.filter(|(_, l)| *l == level).map(|(p, _)| p);
            let r = planview::plan_tool(&mut self.doc, level, tool, p, &mut state, &mut sel);
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
                    (Tool::Guide, Some(_)) if self.plan.guide_edge.is_some() => "Click where the parallel guide goes.".into(),
                    (Tool::Guide, Some(_)) => "Click a second point the guide runs through.".into(),
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
        let (enter, escape, shift) = ui.input(|i| {
            (
                here && i.key_pressed(egui::Key::Enter),
                here && i.key_pressed(egui::Key::Escape),
                i.modifiers.shift,
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
            let track = snaps::track_points(&shape, &[]);
            let (s, align) = snapper.snap_tracked(&snap_cfg, h, project, from, &track, free)?;
            if let Some(c) = project(s.p.truncate().extend(outline.z)) {
                if s.kind != SnapKind::Grid {
                    snaps::draw_tracked(painter, project, s, align, c, from.and_then(project));
                }
            }
            Some((h, s.p.truncate()))
        });
        outline.draw(painter, project, hover.map(|h| h.1));
        let click = match (response.clicked(), hover) {
            (true, Some((h, p))) => Some((p, outline.near_first(h, project))),
            _ => None,
        };
        let right = response.secondary_clicked() && !shift;
        let arc = self.plan.slab_arc;
        let Some(o) = self.plan.outline.as_mut() else {
            return;
        };
        match o.input(enter, escape, right, click, arc) {
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
                egui::Stroke::new(2.0, Color32::from_rgb(242, 140, 51))
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
            if !sk.closed || left_out.contains(&e.id) {
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
                Color32::from_rgba_unmultiplied(242, 140, 51, 60)
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
                egui::Stroke::new(2.5, Color32::from_rgb(242, 140, 51))
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
        let draft = self.plan.sketch.as_mut().filter(|d| d.pane.is_none());
        if let Some(hint) = crate::sketch::shape_controls(ui, response.rect, None, draft) {
            self.status = hint;
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
        let track = snaps::track_points(&shape, &[]);
        let plane = self.plan.sketch.as_ref().filter(|_| mine).map(|d| d.plane);
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
            (Some(d), Some(_)) if mine && snapped.is_some() => {
                snapped.map(|s| d.snap(d.plane.to_plane(s.p), 1e-6, false, false))
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
        let accent = Color32::from_rgb(230, 120, 30);
        match self.plan.sketch.as_ref().filter(|_| mine) {
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
            if let Some((profile, name)) = crate::partstool::face_sweep(&app.plan) {
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
                    app.selection.only(id);
                    app.face = None;
                    app.status =
                        "Sketch added. Press E to extrude it into a mass, or G to move it.".into();
                }
                Ok(None) => {}
                Err(e) => app.status = format!("Couldn't do that: {e}"),
            }
        };
        if !mine && !response.clicked() {
            return;
        }
        if escape {
            self.plan.sketch = None;
            return;
        }
        if enter || (response.secondary_clicked() && self.plan.sketch.is_some()) {
            finish(self, false);
            return;
        }
        if !response.clicked() {
            return;
        }
        match &mut self.plan.sketch {
            None => {
                if let Some(mut plane) = hover.and_then(candidate) {
                    // start on the snapped point, kept on the face's plane
                    if let Some(s) = snapped {
                        plane.origin = plane.to_model(plane.to_plane(s.p));
                    }
                    let draft = SketchDraft::new(plane, None);
                    let shape = crate::sketch::shape(ui.ctx());
                    self.status = format!(
                        "Workplane set. {}",
                        crate::sketch::hint(shape, Some(&draft))
                    );
                    self.plan.sketch = Some(draft);
                }
            }
            Some(d) => {
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
        let Some(view) = self.active_view else { return };
        self.rect_2d = Some(rect);
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
            let msg = crate::schedulepane::show(
                ui,
                rect,
                &mut self.doc,
                view,
                (category, by_type),
                &mut self.selection,
                (&csv, &with_extension(&self.path, "xlsx")),
            );
            if let Some(m) = msg {
                self.status = m;
            }
            None
        } else if self.is_sheet(view) {
            let views = self.placed_views(view);
            let info = self.sheet_info();
            // a sheet follows dark paper while you work on it; Print
            // preview shows it as it prints, on white (MU21)
            let dark = self.dark_paper(ui.ctx()) && !self.print_preview;
            let layer = ui.layer_id();
            let from = ui.ctx().graphics(|g| g.get(layer).map(|l| l.next_idx()));
            let r = sheetview::show(
                ui,
                rect,
                &mut self.doc,
                view,
                &views,
                &info,
                &mut self.sheet,
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
            let Some(mut drawing) = self.drawing_live(view) else {
                return;
            };
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
            let style = PaneStyle {
                background: Some(background),
                decorations: true,
                camera: None,
                scale: self.view(view).map_or(100.0, |v| v.scale),
            };
            // dark paper (A4-3): the pane is drawn as on light paper, then
            // its greys turn over; colours (selection, dimensions, the
            // accent) and pictures keep theirs
            let dark = self.dark_paper(ui.ctx());
            let layer = ui.layer_id();
            let from = ui.ctx().graphics(|g| g.get(layer).map(|l| l.next_idx()));
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
            if dark {
                darken_shapes(ui.ctx(), layer, from);
            }
            r
        };
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
            (Some(v), _) => format!("{name}  ·  1:{:.0}", v.scale),
            (_, Some(s)) => format!(
                "Sheet {} {name}  ·  {}",
                s.number,
                s.size_name().unwrap_or("custom size")
            ),
            _ => name,
        };
        ui.painter().text(
            rect.left_top() + Vec2::new(8.0, 8.0),
            egui::Align2::LEFT_TOP,
            label,
            egui::FontId::proportional(13.0),
            Color32::from_gray(80),
        );
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
            .width(150.0)
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
        ui.horizontal(|ui| {
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
            egui::ComboBox::from_id_salt("pivot").icon(crate::props::combo_icon)
                .selected_text(self.plan.pivot.label())
                .width(150.0)
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
            egui::ComboBox::from_id_salt("view").icon(crate::props::combo_icon)
                .selected_text(current)
                .width(190.0)
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
                        ViewKind::Schedule { .. } => return,
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
            egui::ComboBox::from_id_salt("wall_location").icon(crate::props::combo_icon)
                .selected_text(self.plan.wall_location.label())
                .show_ui(ui, |ui| {
                    for l in creabim_core::LocationLine::ALL {
                        ui.selectable_value(&mut self.plan.wall_location, l, l.label());
                    }
                });
            ui.checkbox(&mut self.plan.wall_flip, "Exterior left");
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
            .width(150.0)
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
        let e = Element::new("New Sheet", ElementKind::Sheet(Sheet::a(number, "A3")));
        let id = e.id;
        let r = self.doc.commit(Transaction::new("New sheet").insert(e));
        self.active_view = Some(id);
        self.selection.only(id);
        self.report(
            r,
            "New A3 sheet. Use Place view… in the toolbar to put views on it.",
        );
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
    fn material_combo(&mut self, ui: &mut egui::Ui, current: &mut Option<ElementId>) -> bool {
        let materials = self.materials();
        let name = current
            .and_then(|id| materials.iter().find(|m| m.0 == id))
            .map_or("By type", |m| m.1.as_str())
            .to_string();
        let mut changed = false;
        let mut create = false;
        crate::app::helpui::combo(ui, "Material")
            .selected_text(name)
            .show_ui(ui, |ui| {
                changed |= ui.selectable_value(current, None, "By type").changed();
                for (id, n) in &materials {
                    changed |= ui.selectable_value(current, Some(*id), n).changed();
                }
                ui.separator();
                create = ui.button("New material…").clicked();
            });
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

    /// Booleans on a wall, slab or mass: the cuts it has, and a menu to cut
    /// it with another element (which then turns into a see-through ghost).
    fn cuts_panel(&mut self, ui: &mut egui::Ui, id: ElementId) {
        let cuttable = |k: &ElementKind| {
            matches!(
                k,
                ElementKind::Wall(_) | ElementKind::Slab(_) | ElementKind::FreeMesh(_)
            )
        };
        let model = self.doc.model();
        if !model.get(id).is_some_and(|e| cuttable(&e.kind)) {
            return;
        }
        let existing: Vec<(ElementId, Cut, String)> = model
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::Cut(c) if c.target == id => Some((
                    e.id,
                    *c,
                    model
                        .get(c.cutter)
                        .map_or(String::new(), |x| x.name.clone()),
                )),
                _ => None,
            })
            .collect();
        // masses first: they are what you usually cut with
        let mut candidates: Vec<(bool, ElementId, String)> = model
            .iter()
            .filter(|e| e.id != id && cuttable(&e.kind))
            .filter(|e| !existing.iter().any(|(_, c, _)| c.cutter == e.id))
            .map(|e| {
                (
                    !matches!(e.kind, ElementKind::FreeMesh(_)),
                    e.id,
                    e.name.clone(),
                )
            })
            .collect();
        candidates.sort_by_key(|c| c.0);
        ui.separator();
        ui.label("Booleans");
        let mut remove = None;
        let mut select = None;
        for (cid, c, name) in &existing {
            ui.horizontal(|ui| {
                ui.label(format!("{} {name}", c.op.label()));
                if ui.small_button("Select").clicked() {
                    select = Some(*cid);
                }
                if ui.small_button("Remove").clicked() {
                    remove = Some(*cid);
                }
            });
        }
        let mut add = None;
        egui::ComboBox::from_id_salt("cut_with")
            .icon(crate::props::combo_icon)
            .selected_text("Subtract…")
            .show_ui(ui, |ui| {
                for (_, cid, name) in &candidates {
                    if ui.selectable_label(false, name).clicked() {
                        add = Some(*cid);
                    }
                }
            });
        ui.label(
            egui::RichText::new(
                "Pick an element to cut away. It turns into a see-through ghost you can still select and move.",
            )
            .weak(),
        );
        if let Some(c) = remove {
            let r = self
                .doc
                .commit(Transaction::new("Remove boolean").remove_cascade(self.doc.model(), c));
            self.report(r, "Boolean removed.");
        }
        if let Some(cutter) = add {
            let e = Element::new(
                "Cut",
                ElementKind::Cut(Cut {
                    target: id,
                    cutter,
                    op: BoolOp::Difference,
                }),
            );
            let r = self.doc.commit(Transaction::new("Subtract").insert(e));
            self.report(r, "Subtracted. Move the ghost to change the cut.");
        }
        if let Some(c) = select {
            self.selection.only(c);
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
        let ctx = ui.ctx().clone();
        let (_, options) = self.tool_strip(&ctx);
        let rows: Vec<&serde_json::Value> = options
            .iter()
            .filter(|o| o["kind"] == "number" || o["kind"] == "toggle")
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
            ui.label(egui::RichText::new(format!("Type: {t}")).weak());
        }
        ui.label(
            egui::RichText::new(
                "Set it before you click; the ghost shows it. The same settings are in the tool bar.",
            )
            .weak(),
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
            } else if o["action"].as_bool().unwrap_or(false) {
                // a step, not a setting: a button
                if ui.button(label).on_hover_text(hint).clicked() {
                    act.push(o["id"].as_str().unwrap_or("").to_string());
                }
            } else {
                let mut on = o["on"].as_bool().unwrap_or(false);
                if ui.checkbox(&mut on, label).on_hover_text(hint).changed() {
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
    fn add_reference(&mut self, v: ElementId, source: creabim_core::RefSource) {
        let model = self.doc.model();
        let shown = crate::trace::read(model, &source);
        if let crate::trace::Shown::Missing(why) = &shown {
            self.status = why.clone();
            return;
        }
        let mut r = creabim_core::Reference::new(source, vec![v]);
        if let Some(b) = crate::trace::bounds_of(&r.source, &shown) {
            r.in_place(b);
        }
        // a picture keeps its own colours; lines take the first colour
        if matches!(shown, crate::trace::Shown::Picture(_)) {
            r.colour = None;
            r.opacity = 0.6;
        }
        let name = format!("Reference: {}", crate::trace::label(model, &r.source));
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
        }
    }

    fn trace_panel(&mut self, ui: &mut egui::Ui, v: ElementId) {
        use creabim_core::RefSource;
        ui.add_space(6.0);
        crate::props::section(ui, "Trace and reference");
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
                    if matches!(view.kind, ViewKind::Plan { .. } | ViewKind::Section { .. }) =>
                {
                    Some((e.id, e.name.clone()))
                }
                _ => None,
            })
            .collect();
        if here.is_empty() {
            ui.label(
                egui::RichText::new("Another view, a picture or a DXF drawing, shown faintly under this view to draw over or compare. It is saved with the project and can show in several views.")
                    .weak(),
            );
        }
        let mut select = None;
        let mut flip = None;
        for (id, name, hidden) in &here {
            ui.horizontal(|ui| {
                let eye = if *hidden { "eye-off" } else { "eye" };
                let (rr, resp) =
                    ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                crate::svgicon::paint(ui.painter(), rr.shrink(2.0), eye, ui.visuals().text_color());
                if resp
                    .on_hover_text(if *hidden { "Show it" } else { "Hide it" })
                    .clicked()
                {
                    flip = Some(*id);
                }
                let on = self.selection.contains(*id);
                if ui.selectable_label(on, name).clicked() {
                    select = Some(*id);
                }
            });
        }
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
                    creabim_core::Transaction::new("Show or hide a reference").replace(e, after),
                );
            }
        }
        ui.add_space(6.0);
        let mut add = None;
        crate::props::row(ui, "Add", |ui| {
            egui::ComboBox::from_id_salt(("trace.add", v))
                .icon(crate::props::combo_icon)
                .selected_text("Another view…")
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for (id, name) in &others {
                        if ui.selectable_label(false, name).clicked() {
                            add = Some(RefSource::View(*id));
                        }
                    }
                })
        });
        ui.horizontal(|ui| {
            if ui
                .button("Picture…")
                .on_hover_text("A picture to trace over: a scanned plan, a photo of a sketch (PNG or JPEG). Calibrate it to size in its properties.")
                .clicked()
            {
                if let Some(p) = rfd::FileDialog::new()
                    .set_title("Add a picture as a reference")
                    .add_filter("Pictures", &["png", "jpg", "jpeg"])
                    .pick_file()
                {
                    add = Some(RefSource::Image {
                        path: p.display().to_string(),
                        width: 10.0,
                    });
                }
            }
            if ui
                .button("DXF drawing…")
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
        });
        if let Some(src) = add {
            self.add_reference(v, src);
        }
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
        };
        let name = crate::trace::label(model, &r.source);
        crate::props::row(ui, what, |ui| ui.label(name));
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
        let n = r.shown_in.len();
        crate::props::row(ui, "Shown in", |ui| {
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
                        if ui.checkbox(&mut on, name).changed() {
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
        crate::props::row(ui, "Colour", |ui| {
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
        });
        crate::props::row(ui, "Strength", |ui| {
            if ui
                .add(egui::Slider::new(&mut r.opacity, 0.05..=1.0).show_value(false))
                .drag_stopped()
            {
                label = Some("Reference strength");
            }
        });
        crate::props::row(ui, "Drawn", |ui| {
            ui.horizontal(|ui| {
                if ui.selectable_label(!r.over, "Under").clicked() && r.over {
                    r.over = false;
                    label = Some("Reference under");
                }
                if ui.selectable_label(r.over, "Over").clicked() && !r.over {
                    r.over = true;
                    label = Some("Reference over");
                }
            })
        });
        ui.horizontal(|ui| {
            if ui.checkbox(&mut r.locked, "Locked").changed() {
                label = Some("Lock reference");
            }
            if ui.checkbox(&mut r.hidden, "Hidden (Alt+F2)").changed() {
                label = Some("Hide reference");
            }
        });
        ui.add_space(4.0);
        crate::props::section(ui, "Place and size");
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
        ui.horizontal(|ui| {
            if ui
                .button("Calibrate…")
                .on_hover_text("Click two points on it a known distance apart, then type that distance: it is scaled to fit (the first point stays).")
                .clicked()
            {
                self.plan.calibrate = Some((id, vec![]));
                self.status = "Calibrate: click a first point on the reference.".into();
            }
            if ui
                .button("Back in place")
                .on_hover_text("Where it was drawn: no move, turn or scale.")
                .clicked()
            {
                if let Some(b) = r.bounds {
                    r.in_place(b);
                    label = Some("Reference back in place");
                }
            }
            if !matches!(r.source, RefSource::View(_))
                && ui
                    .button("Reload")
                    .on_hover_text("Read the file again, after it changed.")
                    .clicked()
            {
                self.plan.ref_cache.forget(id);
            }
        });
        label
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
        egui::Window::new("Calibrate")
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
                ui.horizontal(|ui| {
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

    fn properties(&mut self, ui: &mut egui::Ui) {
        if self.crop_pick.is_some() {
            self.crop_pick_banner(ui);
        }
        let tab = self.prop_tab;
        let Some(id) = self.selection.active else {
            if tab == PropTab::Next {
                self.next_element_panel(ui);
                return;
            }
            if tab == PropTab::Project {
                self.project_props(ui);
                return;
            }
            if tab == PropTab::Model3d {
                self.visibility_panel(ui, gridvis::VisTarget::Window3d, Some("3D view"));
                self.crop_list(ui, self.crop_owner());
                return;
            }
            if self.plan.tool == Tool::Grid {
                self.grid_systems_panel(ui);
            }
            // with nothing selected, the view shown in the 2D pane
            if let Some(gridvis::VisTarget::View(v)) = self.vis_target(false) {
                if tab == PropTab::Trace {
                    self.trace_panel(ui, v);
                    return;
                }
                let name = self.doc.model().get(v).map_or("", |e| e.name.as_str());
                let name = name.to_string();
                self.visibility_panel(
                    ui,
                    gridvis::VisTarget::View(v),
                    Some(&format!("View: {name}")),
                );
                // a section's or elevation's drawing style, as a plan's (MU21)
                if let Some(ElementKind::View(view)) = self.doc.model().get(v).map(|e| &e.kind) {
                    if matches!(view.kind, ViewKind::Section { .. }) {
                        let mut style = view.graphics.drawing_style;
                        crate::props::section(ui, "Graphics");
                        crate::props::row(ui, "Drawing style", |ui| {
                            egui::ComboBox::from_id_salt("section.panel.drawing_style")
                                .icon(crate::props::combo_icon)
                                .selected_text(style.label())
                                .width(ui.available_width())
                                .show_ui(ui, |ui| {
                                    for s in creabim_core::graphics::DrawingStyle::ALL {
                                        ui.selectable_value(&mut style, s, s.label())
                                            .on_hover_text(s.hint());
                                    }
                                });
                        });
                        if style != view.graphics.drawing_style {
                            let r = self.doc.edit(v, "Drawing style", |e| {
                                if let ElementKind::View(x) = &mut e.kind {
                                    x.graphics.drawing_style = style;
                                }
                            });
                            self.report(r, format!("Drawing style: {}.", style.label()));
                        }
                    }
                }
                // a plan's graphics: weights, below-cut tint, view depth
                if let Some(ElementKind::View(view)) = self.doc.model().get(v).map(|e| &e.kind) {
                    if matches!(view.kind, ViewKind::Plan { .. }) {
                        let (mut g, scale) = (view.graphics.clone(), view.scale);
                        let footings_hidden = view
                            .hidden
                            .categories
                            .contains(&creabim_core::visibility::Category::Footings);
                        if let Some(label) = plan_graphics_ui(ui, &mut g, scale) {
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
                                    x.hidden.categories.retain(|c| {
                                        *c != creabim_core::visibility::Category::Footings
                                    });
                                }
                            });
                            self.report(r, "Footings show in this view, dashed under the floor.");
                        }
                    }
                }
                // its range, template and overrides
                self.view_graphics_sections(ui, v);
            }
            return;
        };
        let Some(e) = self.doc.model().get(id).cloned() else {
            return;
        };
        // (its name, mark and actions are in the pinned header above)
        if tab == PropTab::View {
            if let Some(gridvis::VisTarget::View(v)) = self.vis_target(false) {
                self.visibility_panel(ui, gridvis::VisTarget::View(v), None);
                self.view_graphics_sections(ui, v);
            }
            return;
        }
        if tab == PropTab::Project {
            self.project_props(ui);
            return;
        }
        if tab == PropTab::Data {
            self.assign_panel(ui, id);
            return;
        }
        if tab == PropTab::Joins {
            if creabim_core::joins::joinable(&e.kind)
                && self.prop_header(ui, "joins", "Junctions", true, Some("joins"))
            {
                self.joins_section(ui, id);
            }
            self.cuts_panel(ui, id);
            return;
        }
        if tab == PropTab::Graphics {
            if matches!(e.kind, ElementKind::View(_)) {
                self.visibility_panel(ui, gridvis::VisTarget::View(id), None);
                self.view_graphics_sections(ui, id);
            } else {
                self.element_override_section(ui, id);
            }
            return;
        }
        if creabim_core::types::TypeCategory::of(&e.kind).is_some() {
            self.type_picker(ui, id);
        }
        // the type picker may have changed it
        let Some(e) = self.doc.model().get(id).cloned() else {
            return;
        };
        if self.plan.edit == Some(id) {
            // Edit outline in the action bar opened it
            ui.label(
                egui::RichText::new("Editing")
                    .strong()
                    .color(ui.visuals().selection.bg_fill),
            );
            ui.label(egui::RichText::new(editmode::panel_hint(&e.kind)).weak());
            // the layers' grips, without having to know Tab (MU18)
            if handles::has_layers(self.doc.model(), id) {
                let mut on = handles::layers();
                let text = if on {
                    "Back to the element's grips (Tab)"
                } else {
                    "Edit its layers in the views (Tab)"
                };
                if ui.toggle_value(&mut on, text).clicked() {
                    handles::set_layers(on);
                    self.status = if on {
                        "Layers: a grip at each layer's ends (and, on walls, its bottom and top). Drag one to run that layer on or hold it back; right-click to reset.".into()
                    } else {
                        "Back to the element's own grips.".into()
                    };
                }
            }
            ui.add_space(4.0);
        }

        let mut edited = e.clone();
        let mut label = "";
        let mut extrude_sketch = false;
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
        let section = if creabim_core::types::TypeCategory::of(&e.kind).is_some() {
            "Geometry and build"
        } else {
            "Settings"
        };
        if self.prop_header(ui, "element", section, true, None) {
            match &mut edited.kind {
                ElementKind::Level(l) => {
                    if num(ui, "Elevation", &mut l.elevation, -100.0, 1000.0) {
                        label = "Level elevation"
                    }
                    if num(ui, "Height", &mut l.height, 0.5, 50.0) {
                        label = "Level height"
                    }
                    ui.label(egui::RichText::new("Moving a level moves everything on it.").weak());
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
                            ui.label(
                            egui::RichText::new(format!(
                                "Set in {}: it cuts its own opening there, like a door, and the opening moves with it.",
                                hosts.join(", ")
                            ))
                            .weak(),
                        );
                        }
                        let l = typesui::curtain_fields(ui, c);
                        if !l.is_empty() {
                            label = l;
                        }
                        // a curtain wall is as thick as its mullions
                        w.thickness = c.mullion_depth;
                        if self.material_combo(ui, &mut w.material) {
                            label = "Wall material"
                        }
                    } else {
                        if self.assembly_combo(ui, creabim_core::AssemblyUse::Wall, &mut w.assembly)
                        {
                            label = "Wall assembly"
                        }
                        let layered = w.assembly.is_some_and(|a| {
                            matches!(
                                self.doc.model().get(a).map(|e| &e.kind),
                                Some(ElementKind::Assembly(_))
                            )
                        });
                        if layered {
                            ui.label(format!(
                                "Thickness {}, from its layers",
                                crate::units::len(w.thickness)
                            ));
                        } else if num(ui, "Thickness", &mut w.thickness, 0.02, 2.0) {
                            label = "Wall thickness"
                        }
                        if !layered && self.material_combo(ui, &mut w.material) {
                            label = "Wall material"
                        }
                    }
                    let l = wallsui::wall_fields(ui, self.doc.model(), id, w);
                    if !l.is_empty() {
                        label = l;
                    }
                    if ui
                    .button("Redraw path")
                    .on_hover_text("Click a new path for this wall, in a plan or the 3D view. More corners make more walls like it.")
                    .clicked()
                {
                    redraw = true;
                }
                }
                ElementKind::Slab(s) => {
                    if self.assembly_combo(ui, creabim_core::AssemblyUse::Floor, &mut s.assembly) {
                        label = "Slab assembly"
                    }
                    let layered = s.assembly.is_some_and(|a| {
                        matches!(
                            self.doc.model().get(a).map(|e| &e.kind),
                            Some(ElementKind::Assembly(_))
                        )
                    });
                    let mut thickness = s.thickness;
                    if layered {
                        ui.label(format!(
                            "Thickness {}, from its layers",
                            crate::units::len(s.thickness)
                        ));
                    } else if num(ui, "Thickness", &mut thickness, 0.02, 3.0) {
                        s.set_thickness(thickness, self.plan.slab_keep_bottom);
                        label = "Slab thickness"
                    }
                    let keep = &mut self.plan.slab_keep_bottom;
                    ui.horizontal(|ui| {
                        ui.label("Keep");
                        ui.selectable_value(keep, false, "Top").on_hover_text(
                            "A new thickness moves the underside; the top stays put.",
                        );
                        ui.selectable_value(keep, true, "Bottom").on_hover_text(
                            "A new thickness moves the top; the underside stays put.",
                        );
                    });
                    if num(ui, "Top offset", &mut s.top_offset, -100.0, 100.0) {
                        label = "Slab top offset"
                    }
                    let level = self.doc.model().level(s.level).map_or(0.0, |l| l.elevation);
                    ui.label(
                    egui::RichText::new(format!(
                        "Top at {}, underside at {}. The top handle moves the top, the underside handle the underside.",
                        crate::units::elevation(s.top(level)),
                        crate::units::elevation(s.top(level) - s.thickness)
                    ))
                    .weak(),
                );
                    if !layered && self.material_combo(ui, &mut s.material) {
                        label = "Slab material"
                    }
                    let arcs = s.bulges.iter().filter(|b| b.abs() > 1e-9).count();
                    ui.label(match arcs {
                        0 => format!(
                            "{} corners, {}",
                            s.boundary.len(),
                            crate::units::area(s.area())
                        ),
                        n => format!(
                            "{} corners, {n} arc{}, {}",
                            s.boundary.len(),
                            if n == 1 { "" } else { "s" },
                            crate::units::area(s.area())
                        ),
                    });
                    if ui
                        .button("Redraw outline")
                        .on_hover_text(
                            "Click a new outline for this slab, in a plan or the 3D view.",
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
                }
                ElementKind::Opening(o) => {
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
                    if num(ui, "Offset along wall", &mut o.offset, 0.0, 1e4) {
                        label = "Move opening"
                    }
                    if self.material_combo(ui, &mut o.material) {
                        label = "Material";
                    }
                    let l = wallsui::opening_fields(ui, self.doc.model(), o);
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
                    if o.kind == creabim_core::OpeningKind::Window {
                        let panes = |ui: &mut egui::Ui, text: &str, v: &mut u32| {
                            crate::props::row(ui, text, |ui| {
                                crate::props::fill(ui, egui::DragValue::new(v).range(1..=12))
                                    .changed()
                            })
                        };
                        if panes(ui, "Panes across", &mut o.frame.panes_across) {
                            label = "Window panes"
                        }
                        if panes(ui, "Panes up", &mut o.frame.panes_up) {
                            label = "Window panes"
                        }
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
                    // on its sheets too, each viewport round its centre
                    if let Some(s) = sheetview::scale_picker(ui, id, v.scale) {
                        rescale = Some((id, s));
                    }
                    match &mut v.kind {
                        ViewKind::Plan { cut_height, level } => {
                            if num(ui, "Cut height", cut_height, 0.1, 50.0) {
                                label = "View cut height"
                            }
                            let choices = self.underlay_choices(*level);
                            let current = choices
                                .iter()
                                .find(|(u, _)| *u == v.underlay)
                                .map_or("None".to_string(), |(_, l)| l.clone());
                            crate::app::helpui::combo(ui, "Underlay")
                                .selected_text(current)
                                .show_ui(ui, |ui| {
                                    for (u, l) in &choices {
                                        if ui.selectable_value(&mut v.underlay, *u, l).changed() {
                                            label = "View underlay"
                                        }
                                    }
                                });
                            ui.label(
                            egui::RichText::new(
                                "Another level drawn faintly under this one, to trace and snap to. Not printed.",
                            )
                            .weak(),
                        );
                            if let Some(l) = plan_graphics_ui(ui, &mut v.graphics, v.scale) {
                                label = l;
                            }
                        }
                        ViewKind::Section {
                            start,
                            end,
                            depth,
                            cut,
                        } => {
                            if crate::props::switch(ui, cut, "Cut through the model (section)")
                                .changed()
                            {
                                label = "Section or elevation"
                            }
                            if num(ui, "Depth", depth, 0.1, 500.0) {
                                label = "View depth"
                            }
                            if num(ui, "Start X", &mut start.x, -1e4, 1e4)
                                | num(ui, "Start Y", &mut start.y, -1e4, 1e4)
                                | num(ui, "End X", &mut end.x, -1e4, 1e4)
                                | num(ui, "End Y", &mut end.y, -1e4, 1e4)
                            {
                                label = "Move section line"
                            }
                        }
                        ViewKind::Axonometric { azimuth, altitude } => {
                            let deg =
                                |ui: &mut egui::Ui, text: &str, v: &mut f64, lo: f64, hi: f64| {
                                    crate::props::row(ui, text, |ui| {
                                        crate::props::fill(
                                            ui,
                                            egui::DragValue::new(v)
                                                .speed(0.5)
                                                .range(lo..=hi)
                                                .suffix("°"),
                                        )
                                        .changed()
                                    })
                                };
                            if deg(ui, "Azimuth", azimuth, 0.0, 360.0)
                                | deg(ui, "Altitude", altitude, -89.0, 89.0)
                            {
                                label = "Orbit 3D view"
                            }
                            ui.label(
                                egui::RichText::new("Or drag in the view to orbit it.").weak(),
                            );
                        }
                        ViewKind::Schedule { .. } => {}
                    }
                    if matches!(
                        v.kind,
                        ViewKind::Plan { .. } | ViewKind::Section { cut: true, .. }
                    ) && crate::props::switch(ui, &mut v.room_colours, "Colour rooms")
                        .on_hover_text(
                            "Fill each room with a colour of its own, the same in every view",
                        )
                        .changed()
                    {
                        label = "Colour rooms"
                    }
                    if matches!(v.kind, ViewKind::Plan { .. })
                        && crate::props::switch(ui, &mut v.ceiling_plan, "Reflected ceiling plan")
                            .on_hover_text("Draw the ceilings and what hangs from them, not the floor and furniture")
                            .changed()
                    {
                        label = "Ceiling plan"
                    }
                    if matches!(
                        v.kind,
                        ViewKind::Section { .. } | ViewKind::Axonometric { .. }
                    ) && view_style_combo(ui, "view_style", &mut v.style)
                    {
                        label = "View style"
                    }
                    // how what is cut is drawn, as in plans (A4-11, MU20)
                    if matches!(v.kind, ViewKind::Section { .. }) {
                        crate::props::row(ui, "Drawing style", |ui| {
                            egui::ComboBox::from_id_salt("section.drawing_style")
                                .icon(crate::props::combo_icon)
                                .selected_text(v.graphics.drawing_style.label())
                                .width(ui.available_width())
                                .show_ui(ui, |ui| {
                                    for s in creabim_core::graphics::DrawingStyle::ALL {
                                        if ui
                                            .selectable_value(
                                                &mut v.graphics.drawing_style,
                                                s,
                                                s.label(),
                                            )
                                            .on_hover_text(s.hint())
                                            .changed()
                                        {
                                            label = "Drawing style";
                                        }
                                    }
                                });
                        });
                    }
                    if matches!(v.kind, ViewKind::Axonometric { .. }) {
                        ui.add_space(6.0);
                        self.crop_list(ui, Some(id));
                    }
                }
                ElementKind::Sheet(sh) => {
                    ui.horizontal(|ui| {
                        ui.label("Number");
                        if ui.text_edit_singleline(&mut sh.number).changed() {
                            label = "Sheet number"
                        }
                    });
                    let current = sh.size_name().unwrap_or("Custom");
                    crate::app::helpui::combo(ui, "Paper")
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for (name, w, h) in Sheet::SIZES {
                                if ui.selectable_label(current == *name, *name).clicked() {
                                    (sh.width, sh.height) = (*w, *h);
                                    label = "Sheet size"
                                }
                            }
                        });
                    if let Some(l) = self.sheet_template_ui(ui, sh) {
                        label = l;
                    }
                    if self.active_view != Some(id) && ui.button("Open this sheet").clicked() {
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
                    ui.label(format!("Shows {view_name}"));
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
                            crate::props::fill(ui, egui::DragValue::new(v).speed(1.0).suffix(" mm"))
                                .changed()
                        })
                    };
                    if mm(ui, "Centre X", &mut vp.center.x) | mm(ui, "Centre Y", &mut vp.center.y) {
                        label = "Move view on sheet"
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
                    ui.label(
                    egui::RichText::new(
                        "Drag the view on the sheet to move it; double-click it to edit the model through it.",
                    )
                    .weak(),
                );
                    if ui.button("Open the view itself").clicked() {
                        self.active_view = Some(vp.view);
                    }
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
                    label = materials::material_fields(ui, m);
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
                                PlanItem::Dimension { id: i, text, .. } if i == id => Some(text),
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
                    ui.label(format!("{}: {measured}{unit}", d.kind.label()));
                    let attached = [Some(d.a), Some(d.b), d.center]
                        .iter()
                        .flatten()
                        .filter(|p| p.host.is_some())
                        .count();
                    ui.label(
                        egui::RichText::new(match attached {
                            0 => "Not attached: it stays where it was drawn.".to_string(),
                            n => format!("Attached at {n} end(s): it follows what it measures."),
                        })
                        .weak(),
                    );
                    if d.kind == creabim_core::DimensionKind::Spot {
                        let mut pattern = d
                            .pattern
                            .clone()
                            .unwrap_or_else(|| creabim_core::annotation::SPOT_PATTERN.into());
                        ui.horizontal(|ui| {
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
                }
                ElementKind::Tag(t) => {
                    let host = self
                        .doc
                        .model()
                        .get(t.host)
                        .map(|e| e.name.clone())
                        .unwrap_or_default();
                    ui.label(format!("Tags {host}"));
                    if num(ui, "Offset X", &mut t.offset.x, -100.0, 100.0)
                        | num(ui, "Offset Y", &mut t.offset.y, -100.0, 100.0)
                    {
                        label = "Move tag"
                    }
                    if ui.button("Select the tagged element").clicked() {
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
                ElementKind::Note(n) => {
                    // a leader's default reach: a metre at 1:100, 10 mm on a sheet
                    let unit = match self.view(n.view) {
                        Some(v) => v.scale / 100.0,
                        None => 10.0,
                    };
                    if let Some(l) =
                        crate::annotate::note_ui(ui, n, &mut self.plan.anno.focus_text, unit)
                    {
                        label = l;
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
                }
                ElementKind::Units(_) => {
                    if ui.button("Project units…").clicked() {
                        self.levels.units_open = true;
                    }
                }
                ElementKind::Project(_) => {
                    if ui.button("Project information…").clicked() {
                        self.project_open = true;
                    }
                }
                ElementKind::Reference(r) => {
                    if let Some(l) = self.reference_fields(ui, id, r) {
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
                    ui.label(
                        egui::RichText::new(format!(
                            "Used by {users} dimensions, tags and notes. Rename it above."
                        ))
                        .weak(),
                    );
                    ui.horizontal(|ui| {
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
                    if ui.button("Sheet templates…").clicked() {
                        self.drafting.templates_open = true;
                    }
                }
                ElementKind::Grid(g) => {
                    ui.horizontal(|ui| {
                        ui.label("Label");
                        if let Some((_, gg)) = creabim_core::grids::group_of(self.doc.model(), g) {
                            ui.label(egui::RichText::new(&gg.prefix).weak());
                        }
                        if ui.text_edit_singleline(&mut g.label).changed() {
                            label = "Grid label"
                        }
                    });
                    let l = self.grid_properties(ui, g);
                    if !l.is_empty() {
                        label = l;
                    }
                }
                ElementKind::GridGroup(g) => {
                    label = self.grid_group_properties(ui, id, g);
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
                    ui.label(
                        egui::RichText::new(format!(
                            "Its plane faces ({:.2}, {:.2}, {:.2}).",
                            n.x, n.y, n.z
                        ))
                        .weak(),
                    );
                    ui.horizontal(|ui| {
                        ui.label("Extrude by");
                        ui.add(crate::units::drag(&mut self.extrude_distance));
                    });
                    // a closed outline that crosses itself has no inside
                    let crossed =
                        sk.closed && creabim_core::polygon::self_intersects(&sk.outline(), true);
                    ui.label(
                        egui::RichText::new(if crossed {
                            "Closed, but it crosses itself: not a face. Move a point so no lines cross."
                        } else if sk.closed {
                            "Closed: a face. Extruding it makes a solid."
                        } else {
                            "Open: a wire. Extruding it makes a surface."
                        })
                        .weak(),
                    );
                    if ui
                        .add_enabled(
                            !crossed,
                            egui::Button::new(if sk.closed {
                                "Extrude into a mass (E)"
                            } else {
                                "Extrude into a surface (E)"
                            }),
                        )
                        .clicked()
                    {
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
                    if self.material_combo(ui, &mut m.material) {
                        label = "Mesh material"
                    }
                    if num(ui, "X", &mut m.origin.x, -1e4, 1e4)
                        | num(ui, "Y", &mut m.origin.y, -1e4, 1e4)
                        | num(ui, "Z", &mut m.origin.z, -1e3, 1e3)
                    {
                        label = "Move mesh"
                    }
                    ui.horizontal(|ui| {
                        ui.label("Rotation");
                        if ui
                            .add(egui::DragValue::new(&mut m.rotation).speed(1.0).suffix("°"))
                            .changed()
                        {
                            label = "Rotate mesh"
                        }
                        ui.label("Scale");
                        if ui
                            .add(
                                egui::DragValue::new(&mut m.scale)
                                    .speed(0.01)
                                    .range(0.01..=100.0),
                            )
                            .changed()
                        {
                            label = "Scale mesh"
                        }
                    });
                    ui.label(
                    egui::RichText::new(
                        "M moves by a base point. With the pointer over a view: G moves, R rotates, S scales, Shift+D duplicates.",
                    )
                    .weak(),
                );

                    ui.separator();
                    ui.label(egui::RichText::new("Modifiers").strong());
                    if m.modifiers.is_empty() {
                        ui.label(
                            egui::RichText::new(
                                "None yet. They change the shape without touching the mesh.",
                            )
                            .weak(),
                        );
                    }
                    let count = m.modifiers.len();
                    let mut action: Option<(usize, i32)> = None; // (index, -1 up, 1 down, 0 remove, 2 apply)
                    for (i, md) in m.modifiers.iter_mut().enumerate() {
                        ui.push_id(("modifier", i), |ui| {
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if ui
                                        .checkbox(&mut md.enabled, md.kind.label())
                                        .on_hover_text("Switch it off to see the mesh without it")
                                        .changed()
                                    {
                                        label = "Switch modifier"
                                    }
                                    if ui.small_button("Remove").clicked() {
                                        action = Some((i, 0));
                                    }
                                    if i + 1 < count && ui.small_button("Down").clicked() {
                                        action = Some((i, 1));
                                    }
                                    if i > 0 && ui.small_button("Up").clicked() {
                                        action = Some((i, -1));
                                    }
                                    if i == 0
                                    && ui
                                        .small_button("Apply")
                                        .on_hover_text(
                                            "Make it part of the mesh, so you can edit the result",
                                        )
                                        .clicked()
                                {
                                    action = Some((i, 2));
                                }
                                });
                                match &mut md.kind {
                                    ModifierKind::Array { count, offset } => {
                                        ui.horizontal(|ui| {
                                            ui.label("Count");
                                            if ui
                                                .add(
                                                    egui::DragValue::new(count)
                                                        .speed(0.1)
                                                        .range(1..=200),
                                                )
                                                .changed()
                                            {
                                                label = "Array count"
                                            }
                                        });
                                        if num(ui, "Offset X", &mut offset.x, -1e3, 1e3)
                                            | num(ui, "Offset Y", &mut offset.y, -1e3, 1e3)
                                            | num(ui, "Offset Z", &mut offset.z, -1e3, 1e3)
                                        {
                                            label = "Array offset"
                                        }
                                    }
                                    ModifierKind::Mirror { axis, plane } => {
                                        ui.horizontal(|ui| {
                                            ui.label("Across");
                                            for a in Axis::ALL {
                                                if ui.radio_value(axis, a, a.label()).changed() {
                                                    label = "Mirror axis"
                                                }
                                            }
                                        });
                                        if num(ui, "Mirror plane at", plane, -1e3, 1e3) {
                                            label = "Mirror plane"
                                        }
                                    }
                                    ModifierKind::Solidify { thickness } => {
                                        if num(ui, "Thickness", thickness, -10.0, 10.0) {
                                            label = "Solidify thickness"
                                        }
                                    }
                                    ModifierKind::Subdivision { levels } => {
                                        ui.horizontal(|ui| {
                                            ui.label("Levels");
                                            if ui
                                                .add(
                                                    egui::DragValue::new(levels)
                                                        .speed(0.05)
                                                        .range(0..=4),
                                                )
                                                .changed()
                                            {
                                                label = "Subdivision levels"
                                            }
                                        });
                                    }
                                }
                            });
                        });
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
                        Some((_, _)) => {
                            let first = m.modifiers.remove(0);
                            m.mesh = creabim_core::modifiers::evaluate(&m.mesh, &[first]).mesh;
                            self.face = None;
                            label = "Apply modifier";
                        }
                        None => {}
                    }
                    egui::ComboBox::from_id_salt("add_modifier")
                        .icon(crate::props::combo_icon)
                        .selected_text("Add modifier…")
                        .show_ui(ui, |ui| {
                            for k in ModifierKind::presets() {
                                if ui.selectable_label(false, k.label()).clicked() {
                                    m.modifiers.push(Modifier::new(k));
                                    label = "Add modifier";
                                }
                            }
                        });
                    ui.label(format!(
                        "{} vertices, {} faces",
                        m.mesh.vertices.len(),
                        m.mesh.faces.len()
                    ));
                    ui.separator();
                    ui.label(egui::RichText::new("Mesh edit").strong());
                    if !m.modifiers.is_empty() {
                        ui.label(
                            egui::RichText::new("Edits change the mesh under the modifiers.")
                                .weak(),
                        );
                    }
                    match self.face {
                        Some(f) => ui.label(format!("Face {f} selected (click a face in 3D)")),
                        None => ui.label("Click a face in the 3D view to edit it."),
                    };
                    ui.horizontal(|ui| {
                        ui.label("Distance");
                        ui.add(crate::units::drag(&mut self.extrude_distance));
                    });
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(self.face.is_some(), egui::Button::new("Extrude (E)"))
                            .clicked()
                        {
                            if let Some(f) = self.face {
                                m.mesh.extrude_face(f, self.extrude_distance);
                                label = "Extrude face";
                            }
                        }
                        if ui
                            .add_enabled(self.face.is_some(), egui::Button::new("Taper"))
                            .on_hover_text("Scale the face to 80%")
                            .clicked()
                        {
                            if let Some(f) = self.face {
                                m.mesh.scale_face(f, 0.8);
                                label = "Taper face";
                            }
                        }
                        if ui
                            .add_enabled(self.face.is_some(), egui::Button::new("Move"))
                            .on_hover_text("Move the face along its normal by the distance")
                            .clicked()
                        {
                            if let Some(f) = self.face {
                                let n = m.mesh.face_normal(f) * self.extrude_distance;
                                let verts = m.mesh.faces[f].clone();
                                m.mesh.translate_vertices(&verts, n);
                                label = "Move face";
                            }
                        }
                    });
                    if ui.button("Subdivide (Catmull-Clark)").clicked() {
                        m.mesh = m.mesh.subdivide();
                        self.face = None;
                        label = "Subdivide mesh";
                    }
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
                        if ui.radio_value(&mut c.op, op, op.label()).changed() {
                            label = "Change boolean";
                        }
                    }
                }
                ElementKind::Room(r) => {
                    label = crate::roomtool::room_fields(ui, self.doc.model(), r);
                }
                ElementKind::Ceiling(c) => {
                    label = crate::roomtool::ceiling_fields(ui, self.doc.model(), c);
                    if self.material_combo(ui, &mut c.material) {
                        label = "Material";
                    }
                }
                ElementKind::Join(j) => {
                    label = crate::detailtool::join_fields(ui, self.doc.model(), j);
                }
                ElementKind::Detail(d) => {
                    label = crate::detailtool::detail_fields(ui, d);
                }
                ElementKind::RoomSeparator(sep) => {
                    ui.label(format!(
                        "{} long. Rooms on either side of it are measured apart.",
                        crate::units::len(sep.a.distance(sep.b))
                    ));
                }
                ElementKind::Object(o) => {
                    let hung = creabim_core::objects::hung(self.doc.model(), &e);
                    label = objectsui::object_fields(ui, o, hung);
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
                    if let ElementKind::Roof(r) = k {
                        if self.assembly_combo(ui, creabim_core::AssemblyUse::Roof, &mut r.assembly)
                        {
                            label = "Roof assembly";
                        }
                    }
                    let layered = matches!(k, ElementKind::Roof(r) if r.assembly.is_some_and(|a| {
                        matches!(self.doc.model().get(a).map(|e| &e.kind), Some(ElementKind::Assembly(_)))
                    }));
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
                    }
                    // a void cuts, so it has no material to show
                    let void = matches!(k, ElementKind::Part(p) if p.void);
                    if let Some(m) =
                        creabim_core::members::material_mut(k).filter(|_| !layered && !void)
                    {
                        if self.material_combo(ui, m) {
                            label = "Material";
                        }
                    }
                    if matches!(k, ElementKind::Stair(_))
                        && ui
                            .button("Add railings")
                            .on_hover_text("A railing along each side; they follow the stair.")
                            .clicked()
                    {
                        let mut sel = self.selection.clone();
                        let h = self.plan.members.railing_height;
                        let r =
                            crate::memberstool::add_stair_railings(&mut self.doc, id, h, &mut sel);
                        self.report(r, "Railings added along both sides.");
                    }
                }
                ElementKind::Guide(g) => {
                    ui.label(
                        egui::RichText::new(
                            "A construction line to snap to. It is never printed or exported.",
                        )
                        .weak(),
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
        if extrude_sketch {
            self.extrude_sketch();
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
        let Some(id) = self.selection.active else {
            // the open tab's name, as Blender's context line
            ui.horizontal(|ui| {
                let p = crate::theme::Palette::of(ui.ctx());
                let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 22.0), egui::Sense::hover());
                crate::svgicon::paint(
                    ui.painter(),
                    egui::Rect::from_center_size(r.center(), egui::vec2(15.0, 15.0)),
                    self.prop_tab.icon(),
                    p.muted,
                );
                ui.add(
                    egui::Label::new(egui::RichText::new(self.prop_tab.label()).strong())
                        .truncate(),
                );
            });
            ui.add_space(2.0);
            return;
        };
        let Some(e) = self.doc.model().get(id).cloned() else {
            return;
        };
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
            ui.label(format!(
                "{} selected: {}.",
                self.selection.len(),
                list.join(", ")
            ));
            ui.label(
                egui::RichText::new(
                    "G, R, S, Shift+D, Delete, Ctrl+C and H act on all of them. Below is the last one picked; Shift+click picks another.",
                )
                .weak(),
            );
            ui.separator();
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
            if crate::props::row(ui, "Name", |ui| {
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
        crate::props::row(ui, "IFC class", |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.add(egui::Label::new(egui::RichText::new(class.clone()).color(pal.accent)).truncate());
                // what this kind of element is, and its page in the guide
                let page = crate::help::element_page(&e.kind);
                let title = crate::help::text(page.split('#').next().unwrap_or(page))
                    .map(crate::help::title)
                    .unwrap_or("This element");
                self.quick_help_button(ui, page, title);
            })
        });
        if marksui::markable(&e.kind) {
            crate::props::row(ui, "Mark", |ui| {
                let w = ui.available_width();
                self.mark_inline(ui, id, w);
            });
        }
        self.mark_warning(ui, id);
        ui.add_space(2.0);
        // its actions, under its name (brief 02, 2.7)
        self.action_bar(ui);
        ui.add_space(2.0);
        let r = ui.available_rect_before_wrap();
        ui.painter()
            .hline(r.x_range(), r.top(), egui::Stroke::new(1.0, pal.border));
        ui.add_space(3.0);
    }

    /// The tabs Properties shows now, in order (MU19).
    fn prop_tabs(&self) -> Vec<PropTab> {
        let mut out = vec![];
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
        if matches!(self.vis_target(false), Some(gridvis::VisTarget::View(_))) {
            out.push(PropTab::View);
            if self.selection.active.is_none() {
                out.push(PropTab::Trace);
            }
        }
        if self.selection.active.is_none() {
            out.push(PropTab::Model3d);
        }
        out.push(PropTab::Project);
        out
    }

    /// The Project tab: what the whole project holds (MU19).
    fn project_props(&mut self, ui: &mut egui::Ui) {
        crate::props::section(ui, "Project");
        let info = self
            .doc
            .model()
            .project_info()
            .map(|(_, p)| p.clone())
            .unwrap_or_default();
        let name = if info.name.trim().is_empty() {
            "No name yet".to_string()
        } else {
            info.name.clone()
        };
        crate::props::row(ui, "Name", |ui| ui.label(name));
        if !info.client.trim().is_empty() {
            crate::props::row(ui, "Client", |ui| ui.label(info.client.clone()));
        }
        if ui.button("Project information…").clicked() {
            self.project_open = true;
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Levels…").clicked() {
                self.open_levels();
            }
            if ui.button("Materials…").clicked() {
                self.open_materials(None);
            }
        });
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
        let strip = egui::Rect::from_min_size(top, egui::vec2(W, height));
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
                            p.accent,
                        );
                    } else if resp.hovered() {
                        ui.painter()
                            .rect_filled(r.shrink(1.0), 4.0, p.accent.gamma_multiply(0.12));
                    }
                    let ink = if on { p.accent } else { p.muted };
                    let icon = egui::Rect::from_center_size(r.center(), egui::vec2(16.0, 16.0));
                    crate::svgicon::paint(ui.painter(), icon, t.icon(), ink);
                    if resp.on_hover_text(t.label()).clicked() {
                        self.prop_tab = *t;
                    }
                }
            },
        );
    }
}

/// The tabs of Properties (MU19): what a tab holds is its sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PropTab {
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
    /// The project: information, levels, materials.
    Project,
}

impl PropTab {
    /// The strip's groups, a hairline apart: what is selected, the view,
    /// the whole project.
    fn group(self) -> u8 {
        match self {
            PropTab::Next
            | PropTab::Element
            | PropTab::Joins
            | PropTab::Graphics
            | PropTab::Data => 0,
            PropTab::View | PropTab::Trace | PropTab::Model3d => 1,
            PropTab::Project => 2,
        }
    }

    fn label(self) -> &'static str {
        match self {
            PropTab::Next => "Next element",
            PropTab::Element => "Element",
            PropTab::Joins => "Junctions and booleans",
            PropTab::Graphics => "Graphics",
            PropTab::Data => "Data (IFC)",
            PropTab::View => "View",
            PropTab::Trace => "Trace and reference",
            PropTab::Model3d => "3D view",
            PropTab::Project => "Project",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            PropTab::Next => "plus",
            PropTab::Element => "box",
            PropTab::Joins => "join",
            PropTab::Graphics => "palette",
            PropTab::Data => "table-2",
            PropTab::View => "eye",
            PropTab::Trace => "underlay",
            PropTab::Model3d => "cuboid",
            PropTab::Project => "house",
        }
    }
}

impl eframe::App for CreaBimApp {
    /// Keys typed while a web pane had the keyboard reach the views.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if self.shell.feed_keys(raw_input) {
            ctx.request_repaint();
        }
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
        // the web bars cover the top and bottom of the window: egui's own
        // drop-downs and windows keep out from under them, opening upwards
        // when there is no room below
        let bands = if self.shell.web.is_some() {
            egui::epaint::MarginF32 {
                top: shell::TOP_H,
                bottom: shell::STATUS_H,
                ..Default::default()
            }
        } else {
            egui::epaint::MarginF32::default()
        };
        raw_input.safe_area_insets = Some(egui::SafeAreaInsets(bands));
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
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
        self.step_prompt(&ctx);
        self.workspace_prompt(&ctx);
        self.area_keys(&ctx);
        self.shortcuts(&ctx);
        self.keep_edit_mode();
        self.refresh_spotlight();
        let dropped: Vec<String> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().display().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        });
        for path in dropped {
            // a dropped material file joins this project, which keeps its path
            if !materials::is_material_file(&path) {
                self.path = path.clone();
            }
            self.open_path(&path);
        }
        if self.live_reload {
            for f in self.watch.changed(self.doc.model()) {
                self.reload(&f);
            }
            if !crate::files::linked_files(self.doc.model()).is_empty() {
                ctx.request_repaint_after(std::time::Duration::from_secs(1));
            }
        }

        let web = self.web_panels(ui, frame);
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
                ui.horizontal(|ui| {
                    let some = self.selection.is_some();
                    if let Some(c) = crate::commands::modify_bar(ui, some) {
                        self.run_command(&ctx, c, None);
                    }
                });
            });
            egui::Panel::bottom("status").show(ui, |ui| {
                ui.horizontal(|ui| {
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
        let right = egui::Panel::right("properties")
            .default_size(self.shell.settings.right_width)
            .size_range(220.0..=640.0)
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
                let tabs = self.prop_tabs();
                // a placing tool just picked opens its Next tab; a first
                // selection opens Element; otherwise the tab stays
                let now = (self.plan.tool, self.selection.active.is_some());
                if now != self.prop_seen {
                    if now.0 != self.prop_seen.0 && now.0 != Tool::Select && !now.1 {
                        self.prop_tab = PropTab::Next;
                    } else if now.1 && !self.prop_seen.1 {
                        self.prop_tab = PropTab::Element;
                    }
                    self.prop_seen = now;
                }
                if !tabs.contains(&self.prop_tab) {
                    self.prop_tab = tabs[0];
                }
                // a tool just picked opens its Next tab; a new selection
                // with no tab of its own opens Element
                inner.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    self.prop_strip(ui, &tabs, rect.height());
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(4.0, 3.0);
                        let w = ui.available_width();
                        ui.set_max_width(w);
                        ui.add_space(4.0);
                        self.properties_header(ui);
                        egui::ScrollArea::vertical()
                            .auto_shrink(false)
                            .show(ui, |ui| {
                                ui.set_max_width(w - 16.0);
                                crate::props::boxes(ui, true);
                                self.properties(ui);
                                crate::props::close_box(ui);
                                crate::props::boxes(ui, false);
                            });
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
        self.open_view_menu();
        self.materials_window(&ctx);
        self.assemblies_window(&ctx);
        self.types_window(&ctx);
        self.type_preview_window(&ctx);
        self.catalogue_window(&ctx);
        self.levels_window(&ctx);
        self.level_copy_window(&ctx);
        self.styles_window(&ctx);
        self.templates_window(&ctx);
        self.units_window(&ctx);
        self.settings_window(&ctx);
        // the paper follows the setting and the theme; a change redraws
        // the 3D view's grid and background
        let dark = self.dark_paper(&ctx);
        if dark != self.paper_is_dark {
            self.paper_is_dark = dark;
            self.geometry_key = None;
        }
        self.project_window(&ctx);
        self.ifc_export_window(&ctx);
        self.calibrate_window(&ctx);
        self.new_project_window(&ctx);
        self.marks_window(&ctx);
        self.graphic_styles_window(&ctx);
        self.ground_spacing_window(&ctx);
        self.import_report_window(&ctx);
        self.help_window(&ctx);
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
    ctx.graphics_mut(|g| {
        let list = g.entry(layer);
        let end = list.next_idx().0;
        for i in start..end {
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

/// A colour on light paper, on dark paper: a grey turns over (white paper
/// to near black, black ink to near white), a colour is kept.
fn paper_dark(c: Color32) -> Color32 {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    let (hi, lo) = (r.max(g).max(b), r.min(g).min(b));
    if hi.saturating_sub(lo) > 28 {
        return c;
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

fn stem(path: &str) -> String {
    let name = std::path::Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.split('.').next().unwrap_or("project").to_string()
}

fn with_extension(path: &str, ext: &str) -> String {
    let p = std::path::Path::new(path);
    let dir = p.parent().map(|d| d.to_path_buf()).unwrap_or_default();
    dir.join(format!("{}.{ext}", stem(path)))
        .to_string_lossy()
        .into_owned()
}

/// Give every level a plan view so older files open with something to show.
fn with_plan_views(model: Model) -> Model {
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
/// What to call an element in a count.
fn kind_word(k: &ElementKind) -> &'static str {
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
fn plural(kind: &str, n: usize) -> String {
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
    let app = match version.split('.').collect::<Vec<_>>().as_slice() {
        ["0", minor, ..] => format!("CreaBIM 0.{minor} beta"),
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
    let item = scene.iter().find(|s| s.id == h.id)?;
    let idx = &item.mesh.indices[h.triangle * 3..h.triangle * 3 + 3];
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

/// Under a plan's view depth, when the view hides its footings: why the
/// depth shows none, and a button to show them. True when clicked.
fn footings_note(ui: &mut egui::Ui) -> bool {
    let mut show = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new(
                "Footings are hidden in this view, so the view depth can't show them.",
            )
            .weak(),
        );
        show = ui
            .small_button("Show footings")
            .on_hover_text("Footings under the floor draw dashed, as the slab hides them.")
            .clicked();
    });
    show
}

/// A plan's graphics in its Properties: the tint of what lies below the
/// cut, how far below its floor it looks, and its pens' line weights.
/// Returns the undo label of a change.
fn plan_graphics_ui(
    ui: &mut egui::Ui,
    g: &mut creabim_core::PlanGraphics,
    scale: f64,
) -> Option<&'static str> {
    let mut label = None;
    // its own box, folding as the others (MU22)
    let id = egui::Id::new("props.view.graphics.open");
    let open = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(true);
    let (resp, _) = crate::props::header(ui, "Graphics", open, false);
    if resp.clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(id, !open));
    }
    if !open {
        return None;
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
        crate::props::row(ui, "Below the floor", |ui| {
            if crate::props::fill(ui, crate::units::drag(d).range(0.0..=100.0)).changed() {
                label = Some("View depth");
            }
        });
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
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("This view has pens by category of its own.").weak());
            if ui
                .small_button("Use the style pack's")
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
            ui.horizontal(|ui| {
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

/// What the status bar says when the starter house opens.
pub(crate) const STARTER_LOADED: &str = "Loaded the starter house. Orbit with the middle button (or Alt + left drag), pan with right drag, zoom with the wheel; a left drag selects with a box. The Sheets group has its drawings.";

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
        egui::Window::new("Step")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("What points and drags round to: 25 mm, 1/4\", 2 cm.");
                let r = ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .vertical_align(egui::Align::Center)
                        .desired_width(140.0),
                );
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
        let orange = Color32::from_rgb(230, 120, 30);
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
mod box_select_tests {
    use super::*;

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
mod prop_tab_tests {
    use super::*;

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
            &tabs[..4],
            &[
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
        assert!(tabs.contains(&PropTab::Model3d) && tabs.contains(&PropTab::Project));
        for t in tabs {
            app.prop_tab = t;
            draw(&mut app);
        }
        app.plan.tool = Tool::Window;
        assert_eq!(app.prop_tabs()[0], PropTab::Next);
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
