use super::CreaBimApp;
use crate::memberstool;
use crate::planview::{self, PlanState, Tool};
use crate::selection::Selection;
use creabim_core::{ElementId, ElementKind, LocationLine, TypeCategory};
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
            for element in model.iter() {
                match &element.kind {
                    ElementKind::Level(_) => {
                        levels.push(json!({"id": element.id, "name": element.name}))
                    }
                    ElementKind::ElementType(t) if t.category() == TypeCategory::Wall => {
                        wall_types.push(json!({"id": element.id, "name": element.name}));
                    }
                    _ => {}
                }
            }
            return Ok(
                json!({"document_id": self.live_document_id, "revision": model.revision(),
                "project_path": self.path, "levels": levels, "wall_types": wall_types}),
            );
        }
        let apply: fn(&mut Self, Value) -> Result<ElementId, String> = match command {
            "create_wall" => Self::live_create_wall,
            "create_stair" => Self::live_create_stair,
            "update_stair" => Self::live_update_stair,
            _ => return Err("Unknown live MCP command".into()),
        };
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

        let id = apply(self, args)?;
        let model = self.doc.model();
        Ok(
            json!({"id": id, "element": model.get(id), "document_id": self.live_document_id,
            "revision": model.revision()}),
        )
    }

    fn live_create_wall(&mut self, args: Value) -> Result<ElementId, String> {
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
        state.pending = Some(start);
        let mut selection = Selection::default();
        memberstool::click(
            &mut self.doc,
            request.level_id,
            Tool::Stair,
            toward,
            &mut state,
            &mut selection,
        )
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use creabim_core::Element;

    #[test]
    fn live_stair_matches_native_and_updates_in_place() {
        let mut app = CreaBimApp::blank(None);
        app.load_model(creabim_core::sample::typed_house());
        let level = app.doc.model().levels().next().unwrap().0.id;
        let document = app.live_document_id.clone();
        let document_id = Some(document.as_str());
        let start = DVec2::new(40.0, 40.0);
        let toward = DVec2::new(44.0, 44.0);
        let native =
            memberstool::new_stair(app.doc.model(), level, start, toward, &Default::default());

        let before = app.doc.model().revision();
        let create = json!({
            "level_id": level, "start": start.to_array(), "toward": toward.to_array(),
        });
        let mut invalid = create.clone();
        invalid["width"] = json!(1.2);
        assert!(app
            .live_command("create_stair", invalid, document_id, Some(before))
            .is_err());
        let reply = app
            .live_command("create_stair", create, document_id, Some(before))
            .unwrap();
        let created: Element = serde_json::from_value(reply["element"].clone()).unwrap();
        assert_eq!(created.kind, ElementKind::Stair(native));
        assert_eq!(app.doc.undo_label(), Some("Draw stair"));

        let revision = app.doc.model().revision();
        assert!(app
            .live_command(
                "update_stair",
                json!({"element_id": created.id, "width": 0.0}),
                document_id,
                Some(revision)
            )
            .is_err());
        assert_eq!(app.doc.model().revision(), revision);
        let reply = app
            .live_command(
                "update_stair",
                json!({"element_id": created.id, "width": 1.4}),
                document_id,
                Some(revision),
            )
            .unwrap();
        let updated: Element = serde_json::from_value(reply["element"].clone()).unwrap();
        let mut expected = created.clone();
        let ElementKind::Stair(stair) = &mut expected.kind else {
            unreachable!()
        };
        stair.width = 1.4;
        assert_eq!(updated, expected);
        assert!(app.doc.undo());
        assert_eq!(app.doc.model().get(created.id), Some(&created));
        assert!(app.doc.undo());
        assert!(app.doc.model().get(created.id).is_none());
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
