use super::{phasesui::PhaseOf, CreaBimApp, PlanPlacement};
use crate::areas::Shows;
use crate::planview::{self, PlanState, Tool};
use crate::selection::Selection;
use crate::{memberstool, outline};
use creabim_core::{
    polygon, walls, Element, ElementId, ElementKind, LocationLine, Model, Opening, OpeningFrame,
    OpeningKind, Slab, Transaction, TypeCategory,
};
use eframe::egui;
use glam::DVec2;
use serde::Deserialize;
use serde_json::{json, Value};

#[path = "live_elements.rs"]
mod live_elements;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WallRequest {
    level_id: ElementId,
    start: [f64; 2],
    end: [f64; 2],
    wall_type_id: Option<ElementId>,
    thickness: Option<f64>,
    location: Option<LocationLine>,
    name: Option<String>,
    height: Option<f64>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OpeningRequest {
    Typed(TypedOpeningRequest),
    Sized(SizedOpeningRequest),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TypedOpeningRequest {
    wall_id: ElementId,
    center: f64,
    opening_type_id: ElementId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SizedOpeningRequest {
    wall_id: ElementId,
    name: String,
    offset: f64,
    width: f64,
    height: f64,
    sill: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpeningEdit {
    element_id: ElementId,
    material_id: Option<ElementId>,
    #[serde(default)]
    clear_material: bool,
    frame_width: Option<f64>,
    frame_depth: Option<f64>,
    flip_hand: Option<bool>,
    flip_facing: Option<bool>,
    panes_across: Option<u32>,
    panes_up: Option<u32>,
}

impl OpeningEdit {
    fn apply_details(&self, model: &Model, opening: &mut Opening) -> Result<(), String> {
        if let Some(id) = self.material_id {
            if self.clear_material {
                return Err("Choose either material_id or clear_material".into());
            }
            if !matches!(
                model.get(id).map(|e| &e.kind),
                Some(ElementKind::Material(_))
            ) {
                return Err("Choose an existing material".into());
            }
            opening.material = Some(id);
        } else if self.clear_material {
            opening.material = None;
        }
        if let Some(width) = self.frame_width {
            if !(0.01..=0.5).contains(&width) {
                return Err("Frame width must be between 0.01 and 0.5 m".into());
            }
            opening.frame.width = width;
        }
        if let Some(depth) = self.frame_depth {
            if !(0.02..=1.0).contains(&depth) {
                return Err("Frame depth must be between 0.02 and 1 m".into());
            }
            opening.frame.depth = depth;
        }
        if self.flip_hand.is_some() && opening.kind != OpeningKind::Door {
            return Err("Only doors have a hinge side".into());
        }
        for (axis, count) in [self.panes_across, self.panes_up].into_iter().enumerate() {
            if let Some(count) = count {
                if opening.kind != OpeningKind::Window || !(1..=12).contains(&count) {
                    return Err("Choose 1 to 12 panes per direction on a window".into());
                }
                for cell in &mut opening.frame.grid.cells {
                    cell.panes[axis] = count;
                }
            }
        }
        if self.flip_hand.is_some_and(|hand| hand != opening.flip_hand) {
            walls::flip_hand(opening);
        }
        if self
            .flip_facing
            .is_some_and(|facing| facing != opening.flip_facing)
        {
            walls::flip_facing(opening);
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateSlabRequest {
    level_id: ElementId,
    boundary: Vec<[f64; 2]>,
    name: Option<String>,
    thickness: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateSlabRequest {
    element_id: ElementId,
    thickness: Option<f64>,
    top_offset: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StairRequest {
    level_id: ElementId,
    start: [f64; 2],
    toward: [f64; 2],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StairEdit {
    element_id: ElementId,
    width: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RailingRequest {
    level_id: ElementId,
    path: Vec<[f64; 2]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RailingEdit {
    element_id: ElementId,
    height: f64,
}

impl CreaBimApp {
    pub(super) fn track_live_pane(&mut self, ui: &egui::Ui, rect: egui::Rect, plan: bool) {
        let activated = match self.placement {
            PlanPlacement::Docked if plan => self.plan.pane_now == Some(self.areas.focus),
            PlanPlacement::Docked => self.model_in_front(),
            PlanPlacement::Overlay => {
                ui.rect_contains_pointer(rect) && ui.input(|i| i.pointer.any_pressed())
            }
            PlanPlacement::Window => ui.input(|i| i.viewport().focused == Some(true)),
            PlanPlacement::Hidden => true,
        };
        if activated {
            self.live_plan_active = plan;
        }
    }

    fn use_active_modeling_context(&mut self) -> Result<(), String> {
        if creabim_core::phases::list(self.doc.model()).is_empty() {
            return Ok(());
        }
        let focus = self.areas.focus;
        let pane = match self.placement {
            PlanPlacement::Docked => self.areas.root.areas().get(focus).copied(),
            PlanPlacement::Overlay | PlanPlacement::Window if self.live_plan_active => {
                Some(Shows::Current)
            }
            _ => Some(Shows::Model),
        };
        let view = match pane {
            Some(Shows::Model) => {
                self.work_in_phase(PhaseOf::Window3d);
                return Ok(());
            }
            Some(Shows::View(id)) => Some(id),
            Some(Shows::Current) => self.active_view,
            _ => None,
        };
        let view = view
            .filter(|id| self.view(*id).is_some())
            .ok_or("Activate a model view in CreaBIM to choose the editing phase")?;
        // A docked area can show the same drawing at a phase of its own.
        if self.placement == PlanPlacement::Docked {
            crate::phaselens::set_lens(self.area_phase(focus).map(|p| (view, p)));
        }
        self.work_in_phase(PhaseOf::View(view));
        crate::phaselens::set_lens(None);
        Ok(())
    }

    pub(super) fn poll_live_mcp(&mut self, ctx: &egui::Context) {
        let Some(request) = self
            .live_bridge
            .as_ref()
            .and_then(|bridge| bridge.try_recv())
        else {
            return;
        };
        let interacting = || {
            ctx.input(|i| {
                i.pointer.any_down() || i.pointer.any_released() || i.viewport().close_requested()
            }) || ctx.egui_wants_keyboard_input()
                || !self.shell.keys.is_empty()
                || self.shell.web.as_ref().is_some_and(|web| web.has_pending())
        };
        let read_only = matches!(request.command.as_str(), "inspect_document" | "get_element");
        let result = if !read_only && interacting() {
            Err("Finish the current interaction in CreaBIM before editing".into())
        } else {
            self.live_command(
                &request.command,
                request.args,
                request.document_id.as_deref(),
                request.expected_revision,
            )
        };
        let response = match result {
            Ok(result) => {
                ctx.request_repaint();
                json!({"ok": true, "result": result})
            }
            Err(error) => json!({"ok": false, "error": error}),
        };
        let _ = request.respond.send(response);
    }

    fn live_command(
        &mut self,
        command: &str,
        args: Value,
        document_id: Option<&str>,
        expected_revision: Option<u64>,
    ) -> Result<Value, String> {
        let read_only = matches!(command, "inspect_document" | "get_element");
        if command != "inspect_document" && document_id != Some(self.live_document_id.as_str()) {
            return Err("The active document changed; inspect it before continuing".into());
        }
        if !read_only {
            if expected_revision != Some(self.doc.model().revision()) {
                return Err("The document revision changed; inspect it before editing".into());
            }
            if self.close_ask
                || self.save_as_dialog.is_some()
                || self.plan.has_input()
                || self.plan.busy()
                || self.plan.gripping()
                || self.plan.grips.busy()
                || self.plan.edit.is_some()
                || self.plan.point_pick
                || self.plan.calibrate.is_some()
                || self.plan.align_ref.is_some()
                || !self.plan.parts.path.is_empty()
                || self.handle_drag.is_some()
                || self.pending_3d.is_some()
                || self.beam_face.is_some()
                || self.plane_pick.is_some()
                || self.guide_from.is_some()
                || self.box_armed
                || self.split.is_some()
                || self.crop_pick.is_some()
            {
                return Err("Finish the current operation in CreaBIM before editing".into());
            }
            if command != "undo" {
                self.use_active_modeling_context()?;
            }
        }

        let mut args = args;
        let edited_id = match command {
            "create_wall" => Some(self.create_live_wall(args.take())?),
            "create_slab" => Some(self.live_create_slab(
                serde_json::from_value(args.take()).map_err(|e| e.to_string())?,
            )?),
            "update_slab" => Some(self.live_update_slab(
                serde_json::from_value(args.take()).map_err(|e| e.to_string())?,
            )?),
            "create_door" => Some(self.create_live_opening(
                args.take(),
                Tool::Door,
                OpeningKind::Door,
                TypeCategory::Door,
            )?),
            "create_window" => Some(self.create_live_opening(
                args.take(),
                Tool::Window,
                OpeningKind::Window,
                TypeCategory::Window,
            )?),
            "update_door" => {
                Some(self.update_live_opening(args.take(), OpeningKind::Door, "Edit door")?)
            }
            "update_window" => {
                Some(self.update_live_opening(args.take(), OpeningKind::Window, "Edit window")?)
            }
            "create_stair" => Some(self.live_create_stair(args.take())?),
            "update_stair" => Some(self.live_update_stair(args.take())?),
            "create_railing" => Some(self.live_railing_create(args.take())?),
            "update_railing" => Some(self.live_railing_update(args.take())?),
            _ => None,
        };
        let mut result = if let Some(id) = edited_id {
            json!({"id": id, "element": self.doc.model().get(id)})
        } else if command == "undo" {
            let label = self.doc.undo_label().ok_or("Nothing to undo")?.to_owned();
            if !self.doc.undo() {
                return Err("The last edit could not be undone".into());
            }
            json!({"undone": label})
        } else {
            live_elements::execute(&mut self.doc, command, args)?
        };
        if !read_only {
            self.drop_stale_selection();
            self.status = format!("MCP: {command}");
        }
        let object = result.as_object_mut().ok_or("Invalid command result")?;
        let model = self.doc.model();
        object.insert("document_id".into(), json!(self.live_document_id));
        object.insert("revision".into(), json!(model.revision()));
        object.insert("project_path".into(), json!(self.path));
        object.insert("unsaved".into(), json!(self.unsaved()));
        if command == "inspect_document" {
            let mut levels = Vec::new();
            let mut wall_types = Vec::new();
            let mut door_types = Vec::new();
            let mut window_types = Vec::new();
            for element in model.iter() {
                let list = match &element.kind {
                    ElementKind::Level(_) => &mut levels,
                    ElementKind::ElementType(t) if t.category() == TypeCategory::Wall => {
                        &mut wall_types
                    }
                    ElementKind::ElementType(t) if t.category() == TypeCategory::Door => {
                        &mut door_types
                    }
                    ElementKind::ElementType(t) if t.category() == TypeCategory::Window => {
                        &mut window_types
                    }
                    _ => continue,
                };
                list.push(json!({"id": element.id, "name": element.name}));
            }
            object.insert("levels".into(), json!(levels));
            object.insert("wall_types".into(), json!(wall_types));
            object.insert("door_types".into(), json!(door_types));
            object.insert("window_types".into(), json!(window_types));
        }
        Ok(result)
    }

    fn create_live_wall(&mut self, args: Value) -> Result<ElementId, String> {
        let model = self.doc.model();
        let wall: WallRequest = serde_json::from_value(args).map_err(|e| e.to_string())?;
        if model.level(wall.level_id).is_none() {
            return Err("Choose a level from live_inspect".into());
        }
        let start = DVec2::from_array(wall.start);
        let end = DVec2::from_array(wall.end);
        if !wall
            .start
            .into_iter()
            .chain(wall.end)
            .all(|x| x.abs() <= 1e9)
            || start.distance(end) <= 0.05
        {
            return Err("Wall coordinates must be finite and within ±1e9 m, with a length greater than 0.05 m".into());
        }
        let mut state = PlanState::default();
        if let Some(id) = wall.wall_type_id {
            if wall.thickness.is_some() || wall.location.is_some() {
                return Err("A typed wall uses its type's thickness and location line".into());
            }
            if !matches!(model.get(id).map(|e| &e.kind), Some(ElementKind::ElementType(t)) if t.category() == TypeCategory::Wall)
            {
                return Err("Choose a wall type from live_inspect".into());
            }
            state.types.insert(TypeCategory::Wall, id);
        } else {
            if let Some(thickness) = wall.thickness {
                if !(0.0 < thickness && thickness <= 1e9) {
                    return Err("Wall thickness must be positive and within 1e9 m".into());
                }
                state.wall_thickness = thickness;
            }
            state.wall_location = wall.location.unwrap_or_default();
            if !LocationLine::ALL.contains(&state.wall_location) {
                return Err("An untyped wall has no layer boundaries".into());
            }
        }
        if let Some(name) = &wall.name {
            live_elements::valid_name(name)?;
        }
        if wall
            .height
            .is_some_and(|height| !(0.0 < height && height <= 1e9))
        {
            return Err("Wall height must be positive and within 1e9 m".into());
        }
        let mut element =
            planview::straight_wall_element(self.doc.model(), wall.level_id, &state, start, end);
        if let Some(name) = wall.name {
            element.name = name;
        }
        if let (Some(height), ElementKind::Wall(wall)) = (wall.height, &mut element.kind) {
            wall.height = height;
        }
        let id = element.id;
        self.doc
            .commit(Transaction::new("Draw wall").insert(element))
            .map_err(|e| e.to_string())?;
        Ok(id)
    }

    fn create_live_opening(
        &mut self,
        args: Value,
        tool: Tool,
        kind: OpeningKind,
        category: TypeCategory,
    ) -> Result<ElementId, String> {
        let request: OpeningRequest = serde_json::from_value(args).map_err(|e| e.to_string())?;
        let model = self.doc.model();
        let host = match &request {
            OpeningRequest::Typed(r) => r.wall_id,
            OpeningRequest::Sized(r) => r.wall_id,
        };
        let Some(ElementKind::Wall(wall)) = model.get(host).map(|e| &e.kind) else {
            return Err("The opening host must be an existing wall".into());
        };
        let wall_height = walls::constrained_height(model, wall).unwrap_or(wall.height);
        let element = match request {
            OpeningRequest::Typed(r) => {
                if !(0.0..=wall.length()).contains(&r.center) {
                    return Err("The opening centre must be along its wall".into());
                }
                if !matches!(model.get(r.opening_type_id).map(|e| &e.kind),
                    Some(ElementKind::ElementType(t)) if t.category() == category)
                {
                    return Err("Choose a type for the opening from live_inspect".into());
                }
                let mut state = PlanState::default();
                state.types.insert(category, r.opening_type_id);
                planview::opening_on_wall(model, r.wall_id, r.center, tool, &state)
                    .ok_or("The opening must fit within its wall")?
            }
            OpeningRequest::Sized(r) => {
                live_elements::valid_name(&r.name)?;
                if ![r.offset, r.sill, r.width, r.height]
                    .into_iter()
                    .all(f64::is_finite)
                    || r.offset < 0.0
                    || r.sill < 0.0
                    || r.width <= 0.0
                    || r.height <= 0.0
                    || r.offset + r.width > wall.length() + 1e-9
                    || r.sill + r.height > wall_height + 1e-9
                {
                    return Err(
                        "Opening must have positive dimensions and fit within its wall".into(),
                    );
                }
                let opening = Opening {
                    host,
                    kind,
                    offset: r.offset,
                    sill: r.sill,
                    width: r.width,
                    height: r.height,
                    material: None,
                    frame: OpeningFrame::for_kind(kind),
                    flip_hand: false,
                    flip_facing: false,
                    roof_at: None,
                    roof_build: Default::default(),
                    arrangement: None,
                };
                if model.iter().any(|e| {
                    matches!(&e.kind, ElementKind::Opening(other)
                        if other.host == host && r.offset < other.offset + other.width - 1e-9
                            && r.offset + r.width > other.offset + 1e-9
                            && r.sill < other.sill + other.height - 1e-9
                            && r.sill + r.height > other.sill + 1e-9
                    )
                }) {
                    return Err("Opening overlaps an existing opening in this wall".into());
                }
                Element::new(r.name, ElementKind::Opening(opening))
            }
        };
        if let ElementKind::Opening(opening) = &element.kind {
            if !opening.sill.is_finite() || opening.sill + opening.height > wall_height + 1e-9 {
                return Err("The opening must fit within its wall".into());
            }
        }
        let id = element.id;
        self.doc
            .commit(
                Transaction::new(format!("Place {}", tool.label().to_lowercase())).insert(element),
            )
            .map_err(|e| e.to_string())?;
        Ok(id)
    }

    fn update_live_opening(
        &mut self,
        args: Value,
        kind: OpeningKind,
        label: &str,
    ) -> Result<ElementId, String> {
        let edit: OpeningEdit = serde_json::from_value(args).map_err(|e| e.to_string())?;
        let model = self.doc.model();
        let Some(ElementKind::Opening(opening)) = model.get(edit.element_id).map(|e| &e.kind)
        else {
            return Err("Choose an opening of the requested kind".into());
        };
        if opening.kind != kind {
            return Err("Choose an opening of the requested kind".into());
        }
        let mut updated = opening.clone();
        edit.apply_details(model, &mut updated)?;
        self.doc
            .edit(edit.element_id, label, |e| {
                e.kind = ElementKind::Opening(updated)
            })
            .map_err(|e| e.to_string())?;
        Ok(edit.element_id)
    }

    fn live_create_slab(&mut self, request: CreateSlabRequest) -> Result<ElementId, String> {
        if self.doc.model().level(request.level_id).is_none() {
            return Err("Choose a level from live_inspect".into());
        }
        if !(3..=1000).contains(&request.boundary.len()) {
            return Err("A slab outline needs 3 through 1000 corners".into());
        }
        let points: Vec<_> = request
            .boundary
            .into_iter()
            .map(DVec2::from_array)
            .collect();
        if points.iter().enumerate().any(|(i, p)| {
            !p.is_finite()
                || p.abs().max_element() > 1e9
                || points[..i].iter().any(|q| p.distance_squared(*q) < 1e-12)
        }) || polygon::signed_area2(&points).abs() < 1e-8
            || polygon::self_intersects(&points, true)
        {
            return Err(
                "Give a simple slab polygon with distinct finite corners within ±1e9 m".into(),
            );
        }
        let name = request.name.as_deref().unwrap_or("Slab");
        live_elements::valid_name(name)?;
        let thickness = request.thickness.unwrap_or(outline::NEW_SLAB_THICKNESS);
        if !thickness.is_finite() || thickness <= 0.0 {
            return Err("Slab thickness must be positive and finite".into());
        }
        let mut slab = Slab::new(request.level_id, points, thickness);
        slab.make_ccw();
        let e = Element::new(name, ElementKind::Slab(slab));
        let id = e.id;
        self.doc
            .commit(Transaction::new("Draw slab").insert(e))
            .map_err(|e| e.to_string())?;
        Ok(id)
    }

    fn live_update_slab(&mut self, request: UpdateSlabRequest) -> Result<ElementId, String> {
        let id = request.element_id;
        let Some(ElementKind::Slab(slab)) = self.doc.model().get(id).map(|e| &e.kind) else {
            return Err("Choose an existing slab".into());
        };
        if request.thickness.is_none() && request.top_offset.is_none() {
            return Err("Specify a slab thickness or top offset".into());
        }
        let mut edited = slab.clone();
        if let Some(thickness) = request.thickness {
            if slab.assembly.is_some() {
                return Err("A layered slab's thickness is set by its assembly".into());
            }
            if !(0.02..=3.0).contains(&thickness) {
                return Err("Slab thickness must be between 0.02 and 3 m".into());
            }
            edited.set_thickness(thickness, false);
        }
        if let Some(offset) = request.top_offset {
            if !(-100.0..=100.0).contains(&offset) {
                return Err("Slab top offset must be within ±100 m".into());
            }
            edited.top_offset = offset;
        }
        self.doc
            .edit(id, "Edit slab", |element| {
                element.kind = ElementKind::Slab(edited)
            })
            .map_err(|e| e.to_string())?;
        Ok(id)
    }

    fn live_create_stair(&mut self, args: Value) -> Result<ElementId, String> {
        let request: StairRequest = serde_json::from_value(args).map_err(|e| e.to_string())?;
        let model = self.doc.model();
        if model.level(request.level_id).is_none() {
            return Err("Choose a level from live_inspect".into());
        }
        let start = DVec2::from_array(request.start);
        let toward = DVec2::from_array(request.toward);
        if !request
            .start
            .into_iter()
            .chain(request.toward)
            .all(|v| v.abs() <= 1e9)
            || start.distance(toward) <= 0.05
        {
            return Err(
                "Stair points must be finite and within ±1e9 m, and more than 0.05 m apart".into(),
            );
        }
        let mut state = PlanState::default();
        state.members.stair_line = vec![start, toward];
        let mut selection = Selection::default();
        memberstool::finish_stair(&mut self.doc, request.level_id, &mut state, &mut selection)
            .map_err(|e| e.to_string())?;
        selection
            .active
            .ok_or_else(|| "The Stair tool did not place a stair".into())
    }

    fn live_update_stair(&mut self, args: Value) -> Result<ElementId, String> {
        let request: StairEdit = serde_json::from_value(args).map_err(|e| e.to_string())?;
        if !matches!(
            self.doc.model().get(request.element_id).map(|e| &e.kind),
            Some(ElementKind::Stair(stair)) if stair.landing.is_none()
                && stair.line.len() <= 2 && stair.bends.iter().all(|bend| bend.abs() < 1e-9)
                && !stair.is_ramp() && !stair.split
        ) {
            return Err("Choose a straight stair without landings".into());
        }
        if !(0.5..=5.0).contains(&request.width) {
            return Err("Stair width must be between 0.5 and 5 m".into());
        }
        self.doc
            .edit(request.element_id, "Stair width", |element| {
                if let ElementKind::Stair(stair) = &mut element.kind {
                    stair.width = request.width;
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(request.element_id)
    }

    fn live_railing_create(&mut self, args: Value) -> Result<ElementId, String> {
        let railing: RailingRequest = serde_json::from_value(args).map_err(|e| e.to_string())?;
        if self.doc.model().level(railing.level_id).is_none() {
            return Err("Choose a level from live_inspect".into());
        }
        let path: Vec<_> = railing.path.into_iter().map(DVec2::from_array).collect();
        if path.len() < 2 || path.iter().any(|p| !(p.abs().max_element() <= 1e9)) {
            return Err("Provide at least two railing points within ±1e9 m".into());
        }
        let mut length = 0.0;
        for pair in path.windows(2) {
            let span = pair[0].distance(pair[1]);
            if span < 0.05 {
                return Err("Successive railing points must be at least 0.05 m apart".into());
            }
            length += span;
        }
        // Posts are allocated for the whole path when the scene rebuilds.
        if length / 1.2 + path.len() as f64 > 10_000.0 {
            return Err("The railing path would create too many posts".into());
        }
        let mut state = PlanState::default();
        let last = *path.last().unwrap();
        state.members.path = path;
        let mut selection = Selection::default();
        memberstool::click(
            &mut self.doc,
            railing.level_id,
            Tool::Railing,
            last,
            &mut state,
            &mut selection,
        )
        .map_err(|e| e.to_string())?;
        selection
            .active
            .ok_or_else(|| "Railing was not created".into())
    }

    fn live_railing_update(&mut self, args: Value) -> Result<ElementId, String> {
        let edit: RailingEdit = serde_json::from_value(args).map_err(|e| e.to_string())?;
        if !matches!(
            self.doc.model().get(edit.element_id).map(|e| &e.kind),
            Some(ElementKind::Railing(_))
        ) {
            return Err("Choose a railing element to update".into());
        }
        if !(0.2..=3.0).contains(&edit.height) {
            return Err("Railing height must be between 0.2 and 3.0 m".into());
        }
        self.doc
            .edit(edit.element_id, "Edit railing", |element| {
                if let ElementKind::Railing(railing) = &mut element.kind {
                    railing.height = edit.height;
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(edit.element_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_app;

    #[test]
    fn live_paging_and_reading_keep_the_document_guard() {
        let mut app = test_app();
        let page = app
            .live_command(
                "inspect_document",
                json!({"kind": "level", "limit": 1}),
                None,
                None,
            )
            .unwrap();
        assert!(page["levels"]
            .as_array()
            .is_some_and(|levels| !levels.is_empty()));
        assert_eq!(page["elements"].as_array().unwrap().len(), 1);
        let id = &page["elements"][0]["id"];
        let args = json!({"element_id": id});
        assert!(app
            .live_command("get_element", args.clone(), Some("changed"), None)
            .is_err());
        let element = app
            .live_command("get_element", args, page["document_id"].as_str(), None)
            .unwrap();
        assert_eq!(element["id"], *id);
        assert_eq!(element["revision"], page["revision"]);
    }

    #[test]
    fn explicit_wall_dimensions_still_use_the_active_views_phase() {
        let mut app = test_app();
        let tx = creabim_core::phases::start(app.doc.model(), true).unwrap();
        app.doc.commit(tx).unwrap();
        app.placement = PlanPlacement::Hidden;
        app.set_phase(PhaseOf::Window3d, 3);
        let info = app
            .live_command("inspect_document", json!({}), None, None)
            .unwrap();
        let undo = app.doc.undo_steps().len();
        let result = app
            .live_command(
                "create_wall",
                json!({"level_id": info["levels"][0]["id"], "start": [80, 0], "end": [84, 0],
                "name": "Prototype wall", "height": 2.6, "thickness": 0.28}),
                info["document_id"].as_str(),
                info["revision"].as_u64(),
            )
            .unwrap();
        let e: Element = serde_json::from_value(result["element"].clone()).unwrap();
        assert_eq!(e.name, "Prototype wall");
        assert_eq!(e.phase.built, Some(3));
        let ElementKind::Wall(wall) = e.kind else {
            panic!("expected wall");
        };
        assert_eq!(wall.height, 2.6);
        assert_eq!(wall.thickness, 0.28);
        assert_eq!(app.doc.undo_steps().len(), undo + 1);
    }

    #[test]
    fn live_column_uses_the_active_panes_phase_and_option() {
        use creabim_core::phases;

        let mut app = test_app();
        app.doc
            .commit(phases::start(app.doc.model(), true).unwrap())
            .unwrap();
        let (tx, [_, option]) = phases::add_question(app.doc.model(), "Extension").unwrap();
        app.doc.commit(tx).unwrap();
        let plan = PhaseOf::View(app.active_view.unwrap());
        app.placement = PlanPlacement::Overlay;
        app.set_phase(PhaseOf::Window3d, 3);
        app.set_option(PhaseOf::Window3d, option, true);
        app.set_phase(plan, 2);
        // The 3D pane is active, but the inset plan was drawn last.
        app.work_in_phase(plan);
        assert_eq!(app.doc.working_phase(), Some(2));
        assert_eq!(app.doc.working_option(), None);

        let info = app
            .live_command("inspect_document", json!({}), None, None)
            .unwrap();
        let result = app
            .live_command(
                "create_column",
                json!({"level_id": info["levels"][0]["id"], "name": "MCP column",
                    "position": [80, 0], "width": 0.3, "depth": 0.3, "height": 2.6}),
                info["document_id"].as_str(),
                info["revision"].as_u64(),
            )
            .unwrap();
        let element: Element = serde_json::from_value(result["element"].clone()).unwrap();
        assert_eq!(
            (element.phase.built, element.phase.option),
            (Some(3), Some(option))
        );
    }

    fn create_live_wall(app: &mut CreaBimApp) -> Result<Element, String> {
        let inspect = app.live_command("inspect_document", json!({}), None, None)?;
        let args = json!({"level_id": inspect["levels"][0]["id"], "start": [0, 0], "end": [4, 0]});
        let reply = app.live_command(
            "create_wall",
            args,
            inspect["document_id"].as_str(),
            inspect["revision"].as_u64(),
        )?;
        Ok(serde_json::from_value(reply["element"].clone()).unwrap())
    }

    #[test]
    fn live_wall_uses_the_active_views_phase() {
        use crate::areas::Node;

        let mut app = test_app();
        let tx = creabim_core::phases::start(app.doc.model(), true).unwrap();
        app.doc.commit(tx).unwrap();
        let plan = app.active_view.unwrap();
        app.set_phase(PhaseOf::Window3d, 3); // Proposed
        app.set_phase(PhaseOf::View(plan), 2); // Demolition
        app.placement = PlanPlacement::Overlay;
        // The 3D view is active, but the inset plan paints last.
        app.work_in_phase(PhaseOf::Window3d);
        app.work_in_phase(PhaseOf::View(plan));
        assert_eq!(create_live_wall(&mut app).unwrap().phase.built, Some(3));
        assert!(app.doc.undo());
        let ctx = egui::Context::default();
        let click = egui::Event::PointerButton {
            pos: egui::pos2(20.0, 20.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        };
        let mut frame = ctx.run_ui(
            egui::RawInput {
                events: vec![click],
                ..Default::default()
            },
            |ui| {
                app.track_live_pane(ui, ui.max_rect(), false);
                app.track_live_pane(
                    ui,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(50.0, 50.0)),
                    true,
                );
            },
        );
        frame.textures_delta.clear();
        assert_eq!(create_live_wall(&mut app).unwrap().phase.built, Some(2));
        assert!(app.doc.undo());

        // Two docked areas show the same plan. Only the first overrides its phase.
        app.placement = PlanPlacement::Docked;
        app.areas.root = Node::split(
            true,
            0.5,
            Node::Area(Shows::View(plan)),
            Node::Area(Shows::View(plan)),
        );
        app.set_area_phase(0, Some(3));
        for (focus, expected) in [(0, 3), (1, 2)] {
            app.areas.focus = focus;
            assert_eq!(
                create_live_wall(&mut app).unwrap().phase.built,
                Some(expected)
            );
            assert!(app.doc.undo());
        }
        app.areas.root = Node::Area(Shows::Notes);
        app.areas.focus = 0;
        let revision = app.doc.model().revision();
        assert!(create_live_wall(&mut app).is_err());
        assert_eq!(app.doc.model().revision(), revision);
    }

    #[test]
    fn live_wall_matches_native_wall_and_undo() {
        for typed in [true, false] {
            let mut app = CreaBimApp::blank(None);
            app.load_model(creabim_core::sample::typed_house());
            let snapshot = app
                .live_command("inspect_document", json!({}), None, None)
                .unwrap();
            let level: ElementId =
                serde_json::from_value(snapshot["levels"][0]["id"].clone()).unwrap();
            let wall_type: ElementId =
                serde_json::from_value(snapshot["wall_types"][0]["id"].clone()).unwrap();
            let document_id = snapshot["document_id"].as_str().unwrap();
            let old_revision = snapshot["revision"].as_u64().unwrap();
            let start = DVec2::new(40.0, 40.0);
            let end = DVec2::new(44.0, 40.0);
            let original = app.doc.model().clone();
            let mut state = PlanState::default();
            let mut args =
                json!({"level_id": level, "start": start.to_array(), "end": end.to_array()});
            if typed {
                state.types.insert(TypeCategory::Wall, wall_type);
                args["wall_type_id"] = json!(wall_type);
            } else {
                state.wall_thickness = 0.25;
                state.wall_location = LocationLine::ExteriorFace;
                args["thickness"] = json!(0.25);
                args["location"] = json!(LocationLine::ExteriorFace);
            }
            let mut selection = Selection::default();
            for point in [start, end] {
                planview::plan_tool(
                    &mut app.doc,
                    level,
                    Tool::Wall,
                    point,
                    &mut state,
                    &mut selection,
                )
                .unwrap();
            }
            let native_id = app
                .doc
                .model()
                .iter()
                .find(|e| original.get(e.id).is_none())
                .unwrap()
                .id;
            let native = app.doc.model().get(native_id).unwrap().clone();
            let edited = app.doc.model().clone();
            let before_stale = app.doc.undo_steps().len();
            let error = app
                .live_command(
                    "create_wall",
                    args.clone(),
                    Some(document_id),
                    Some(old_revision),
                )
                .unwrap_err();
            assert!(error.contains("revision changed"));
            assert_eq!(&edited, app.doc.model());
            assert_eq!(before_stale, app.doc.undo_steps().len());
            assert_eq!(app.doc.undo_label(), Some("Draw wall"));
            assert!(app.doc.undo());
            assert_eq!(&original, app.doc.model());

            let revision = app.doc.model().revision();
            app.plan.guide_edge = Some((start, end));
            let error = app
                .live_command(
                    "create_wall",
                    args.clone(),
                    Some(document_id),
                    Some(revision),
                )
                .unwrap_err();
            assert!(error.contains("current operation"));
            assert_eq!(&original, app.doc.model());
            assert_eq!(revision, app.doc.model().revision());
            app.plan.guide_edge = None;
            let reply = app
                .live_command("create_wall", args, Some(document_id), Some(revision))
                .unwrap();
            let mut live: Element = serde_json::from_value(reply["element"].clone()).unwrap();
            assert_ne!(native.id, live.id);
            live.id = native.id;
            assert_eq!(live, native);
            assert_eq!(app.doc.undo_label(), Some("Draw wall"));
            assert!(app.doc.undo());
            assert_eq!(&original, app.doc.model());
        }
    }
}
