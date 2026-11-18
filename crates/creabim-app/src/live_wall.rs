use super::CreaBimApp;
use crate::outline;
use crate::planview::{self, PlanState};
use creabim_core::{polygon, ElementId, ElementKind, LocationLine, TypeCategory};
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
struct CreateSlabRequest {
    level_id: ElementId,
    boundary: Vec<[f64; 2]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateSlabRequest {
    element_id: ElementId,
    thickness: Option<f64>,
    top_offset: Option<f64>,
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
        if !matches!(command, "create_wall" | "create_slab" | "update_slab") {
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
            "create_slab" => {
                self.live_create_slab(serde_json::from_value(args).map_err(|e| e.to_string())?)?
            }
            "update_slab" => {
                self.live_update_slab(serde_json::from_value(args).map_err(|e| e.to_string())?)?
            }
            _ => self.live_create_wall(serde_json::from_value(args).map_err(|e| e.to_string())?)?,
        };
        let model = self.doc.model();
        Ok(
            json!({"id": id, "element": model.get(id), "document_id": self.live_document_id,
            "revision": model.revision()}),
        )
    }

    fn live_create_wall(&mut self, wall: WallRequest) -> Result<ElementId, String> {
        let model = self.doc.model();
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
        let mut outline = outline::Outline::new_slab(self.doc.model(), request.level_id, points[0]);
        outline.points = points;
        let (message, id) = outline
            .finish(&mut self.doc, true)
            .map_err(|e| e.to_string())?;
        id.ok_or(message)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{planview::Tool, selection::Selection};
    use creabim_core::Element;

    #[test]
    fn live_slab_matches_native_creation_and_edits_in_place() {
        let mut app = CreaBimApp::blank(None);
        app.load_model(creabim_core::sample::typed_house());
        let snapshot = app
            .live_command("inspect_document", json!({}), None, None)
            .unwrap();
        let level: ElementId = serde_json::from_value(snapshot["levels"][0]["id"].clone()).unwrap();
        let document = snapshot["document_id"].as_str().unwrap();
        let boundary = [[40.0, 40.0], [40.0, 44.0], [44.0, 44.0], [44.0, 40.0]];
        let mut outline = crate::outline::Outline::new_slab(
            app.doc.model(),
            level,
            DVec2::from_array(boundary[0]),
        );
        outline.points = boundary.map(DVec2::from_array).into();
        let (_, native_id) = outline.finish(&mut app.doc, true).unwrap();
        let native = app.doc.model().get(native_id.unwrap()).unwrap().clone();
        assert!(app.doc.undo());

        let revision = app.doc.model().revision();
        assert!(app
            .live_command(
                "create_slab",
                json!({"level_id": level, "boundary": [[0, 0], [3, 3], [3, 0], [0, 2]]}),
                Some(document),
                Some(revision),
            )
            .is_err());
        assert_eq!(app.doc.model().revision(), revision);
        let created = app
            .live_command(
                "create_slab",
                json!({"level_id": level, "boundary": boundary}),
                Some(document),
                Some(revision),
            )
            .unwrap();
        let id: ElementId = serde_json::from_value(created["id"].clone()).unwrap();
        let before = app.doc.model().get(id).unwrap().clone();
        assert_eq!(before.kind, native.kind);
        assert_eq!(app.doc.undo_label(), Some("Draw slab"));

        let update = json!({"element_id": id, "thickness": 0.31, "top_offset": 0.08});
        assert!(app
            .live_command(
                "update_slab",
                update.clone(),
                Some(document),
                Some(revision)
            )
            .unwrap_err()
            .contains("revision changed"));
        let edit_revision = app.doc.model().revision();
        let invalid = json!({"element_id": id, "thickness": 0});
        assert!(app
            .live_command("update_slab", invalid, Some(document), Some(edit_revision))
            .is_err());
        assert_eq!(app.doc.model().get(id), Some(&before));
        let after = app
            .live_command("update_slab", update, Some(document), Some(edit_revision))
            .unwrap();
        assert_eq!(after["id"], created["id"]);
        let ElementKind::Slab(slab) = &app.doc.model().get(id).unwrap().kind else {
            panic!("expected a slab")
        };
        assert_eq!((slab.thickness, slab.top_offset), (0.31, 0.08));
        assert_eq!(slab.area(), 16.0);
        assert!(app.doc.undo());
        assert_eq!(app.doc.model().get(id), Some(&before));
        assert!(app.doc.undo());
        assert!(app.doc.model().get(id).is_none());
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
