use super::CreaBimApp;
use crate::planview::{self, PlanState};
use creabim_core::{
    walls, ElementId, ElementKind, LocationLine, Model, Opening, OpeningKind, TypeCategory,
};
use eframe::egui;
use glam::DVec2;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WallRequest {
    level_id: ElementId,
    start: [f64; 2],
    end: [f64; 2],
    wall_type_id: Option<ElementId>,
    thickness: Option<f64>,
    location: Option<LocationLine>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpeningRequest {
    wall_id: ElementId,
    center: f64,
    opening_type_id: ElementId,
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

impl CreaBimApp {
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
        let result = if request.command != "inspect_document" && interacting() {
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
        let model = self.doc.model();
        if command == "inspect_document" {
            let mut levels = Vec::new();
            let mut wall_types = Vec::new();
            let mut door_types = Vec::new();
            for element in model.iter() {
                match &element.kind {
                    ElementKind::Level(_) => {
                        levels.push(json!({"id": element.id, "name": element.name}))
                    }
                    ElementKind::ElementType(t) if t.category() == TypeCategory::Wall => {
                        wall_types.push(json!({"id": element.id, "name": element.name}));
                    }
                    ElementKind::ElementType(t) if t.category() == TypeCategory::Door => {
                        door_types.push(json!({"id": element.id, "name": element.name}));
                    }
                    _ => {}
                }
            }
            return Ok(
                json!({"document_id": self.live_document_id, "revision": model.revision(),
                "project_path": self.path, "levels": levels, "wall_types": wall_types,
                "door_types": door_types}),
            );
        }
        if !matches!(command, "create_wall" | "create_door" | "update_door") {
            return Err("Unknown live MCP command".into());
        }
        if document_id != Some(self.live_document_id.as_str()) {
            return Err("The active document changed; inspect it before editing".into());
        }
        if expected_revision != Some(model.revision()) {
            return Err("The document revision changed; inspect it before editing".into());
        }
        if self.close_ask
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

        let id = match command {
            "create_door" => self.create_live_door(args)?,
            "update_door" => self.update_live_door(args)?,
            _ => self.create_live_wall(args)?,
        };
        let model = self.doc.model();
        Ok(
            json!({"id": id, "element": model.get(id), "document_id": self.live_document_id,
            "revision": model.revision()}),
        )
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
        planview::create_straight_wall(&mut self.doc, wall.level_id, &state, start, end)
            .map_err(|e| e.to_string())
    }

    fn create_live_door(&mut self, args: Value) -> Result<ElementId, String> {
        let request: OpeningRequest = serde_json::from_value(args).map_err(|e| e.to_string())?;
        let model = self.doc.model();
        let Some(ElementKind::Wall(wall)) = model.get(request.wall_id).map(|e| &e.kind) else {
            return Err("The opening host must be an existing wall".into());
        };
        if !(0.0..=wall.length()).contains(&request.center) {
            return Err("The opening centre must be along its wall".into());
        }
        if !matches!(model.get(request.opening_type_id).map(|e| &e.kind),
            Some(ElementKind::ElementType(t)) if t.category() == TypeCategory::Door)
        {
            return Err("Choose a door type from live_inspect".into());
        }
        let mut state = PlanState::default();
        state
            .types
            .insert(TypeCategory::Door, request.opening_type_id);
        let element = planview::opening_on_wall(
            model,
            request.wall_id,
            request.center,
            planview::Tool::Door,
            &state,
        )
        .ok_or("The opening must fit within its wall")?;
        if let ElementKind::Opening(opening) = &element.kind {
            let height = walls::constrained_height(model, wall).unwrap_or(wall.height);
            if !opening.sill.is_finite() || opening.sill + opening.height > height + 1e-9 {
                return Err("The opening must fit within its wall".into());
            }
        }
        planview::insert_plan_opening(&mut self.doc, element).map_err(|e| e.to_string())
    }

    fn update_live_door(&mut self, args: Value) -> Result<ElementId, String> {
        let edit: OpeningEdit = serde_json::from_value(args).map_err(|e| e.to_string())?;
        let model = self.doc.model();
        let Some(ElementKind::Opening(opening)) = model.get(edit.element_id).map(|e| &e.kind)
        else {
            return Err("Choose a door in the open document".into());
        };
        if opening.kind != OpeningKind::Door {
            return Err("Choose a door in the open document".into());
        }
        let mut updated = opening.clone();
        edit.apply_details(model, &mut updated)?;
        self.doc
            .edit(edit.element_id, "Edit door", |e| {
                e.kind = ElementKind::Opening(updated)
            })
            .map_err(|e| e.to_string())?;
        Ok(edit.element_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{planview::Tool, selection::Selection};
    use creabim_core::Element;

    #[test]
    fn live_door_matches_native_placement_and_undoes_an_edit() {
        let mut app = CreaBimApp::blank(None);
        app.load_model(creabim_core::sample::typed_house());
        let level = app.doc.model().levels().next().unwrap().0.id;
        let door_type = app
            .doc
            .model()
            .iter()
            .find(|e| e.name == "Single door 900 × 2100")
            .unwrap()
            .id;
        let discovered = app
            .live_command("inspect_document", json!({}), None, None)
            .unwrap();
        assert!(discovered["door_types"]
            .as_array()
            .unwrap()
            .contains(&json!({"id": door_type, "name": "Single door 900 × 2100"})));
        let wall = Element::new(
            "Host",
            ElementKind::Wall(creabim_core::Wall::new(
                level,
                DVec2::new(40.0, 40.0),
                DVec2::new(44.0, 40.0),
                0.2,
                3.0,
            )),
        );
        let host = wall.id;
        app.doc
            .commit(creabim_core::Transaction::new("Host").insert(wall))
            .unwrap();
        let original = app.doc.model().clone();
        let mut selection = Selection::default();
        let mut state = PlanState::default();
        state.types.insert(TypeCategory::Door, door_type);
        planview::plan_tool(
            &mut app.doc,
            level,
            Tool::Door,
            DVec2::new(42.0, 40.0),
            &mut state,
            &mut selection,
        )
        .unwrap();
        let native = app
            .doc
            .model()
            .get(selection.active.unwrap())
            .unwrap()
            .clone();
        assert!(app.doc.undo());
        let document = app.live_document_id.clone();
        let revision = app.doc.model().revision();
        let create = |wall_id, center, type_id| json!({"wall_id": wall_id, "center": center, "opening_type_id": type_id});
        for invalid in [
            create(level, 2.0, door_type),
            create(host, -1.0, door_type),
            create(host, 2.0, level),
        ] {
            assert!(app
                .live_command("create_door", invalid, Some(&document), Some(revision))
                .is_err());
        }
        assert_eq!(&original, app.doc.model());
        let result = app
            .live_command(
                "create_door",
                create(host, 2.0, door_type),
                Some(&document),
                Some(revision),
            )
            .unwrap();
        let id: ElementId = serde_json::from_value(result["id"].clone()).unwrap();
        let before = app.doc.model().get(id).unwrap().clone();
        assert_eq!(before.kind, native.kind);
        assert_eq!(before.of_type, native.of_type);
        let revision = app.doc.model().revision();
        let ElementKind::Opening(door) = &before.kind else {
            panic!()
        };
        let native_dimensions = (door.width, door.height, door.sill, door.offset);
        let material = app
            .doc
            .model()
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Material(_)) && Some(e.id) != door.material)
            .unwrap()
            .id;
        for invalid in [
            json!({"element_id": id, "material_id": host}),
            json!({"element_id": id, "material_id": material, "clear_material": true}),
            json!({"element_id": id, "frame_width": 0.0}),
            json!({"element_id": id, "frame_depth": 1.1}),
        ] {
            assert!(app
                .live_command("update_door", invalid, Some(&document), Some(revision))
                .is_err());
            assert_eq!(app.doc.model().revision(), revision);
            assert_eq!(app.doc.model().get(id), Some(&before));
        }
        let args = json!({"element_id": id, "material_id": material,
                          "frame_width": 0.08, "frame_depth": 0.15, "flip_hand": true, "flip_facing": true});
        app.live_command("update_door", args, Some(&document), Some(revision))
            .unwrap();
        let after = app.doc.model().get(id).unwrap().clone();
        let ElementKind::Opening(door) = &after.kind else {
            panic!();
        };
        assert_eq!(door.host, host);
        assert_eq!(
            (door.width, door.height, door.sill, door.offset),
            native_dimensions
        );
        assert_eq!(door.material, Some(material));
        assert_eq!((door.frame.width, door.frame.depth), (0.08, 0.15));
        assert!(door.flip_hand && door.flip_facing);
        assert_eq!(after.of_type.as_ref().unwrap().id, door_type);
        assert!(app.doc.undo());
        assert_eq!(app.doc.model().get(id), Some(&before));
        assert!(app.doc.undo());
        assert_eq!(&original, app.doc.model());
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
