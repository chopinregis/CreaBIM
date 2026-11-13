//! Commands for editing an active document through an external agent.
//! Lengths and coordinates are in metres.

use crate::{
    Beam, Column, Document, Element, ElementId, ElementKind, Op, Opening, OpeningFrame,
    OpeningKind, Profile, Slab, Transaction, Wall, Xform,
};
use glam::{DVec2, DVec3};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LevelArgs {
    name: String,
    elevation: f64,
    height: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WallArgs {
    name: String,
    level_id: ElementId,
    start: [f64; 2],
    end: [f64; 2],
    thickness: f64,
    height: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SlabArgs {
    name: String,
    level_id: ElementId,
    boundary: Vec<[f64; 2]>,
    thickness: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColumnArgs {
    name: String,
    level_id: ElementId,
    position: [f64; 2],
    width: f64,
    depth: f64,
    height: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BeamArgs {
    name: String,
    level_id: ElementId,
    start: [f64; 2],
    end: [f64; 2],
    width: f64,
    depth: f64,
    #[serde(default)]
    top_offset: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpeningArgs {
    name: String,
    wall_id: ElementId,
    offset: f64,
    width: f64,
    height: f64,
    sill: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ElementArgs {
    element_id: ElementId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveArgs {
    element_id: ElementId,
    delta: [f64; 3],
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct InspectArgs {
    offset: usize,
    limit: usize,
    kind: Option<String>,
}

impl Default for InspectArgs {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 50,
            kind: None,
        }
    }
}

fn parse<T: DeserializeOwned>(args: Value) -> Result<T, String> {
    serde_json::from_value(args).map_err(|e| format!("Invalid arguments: {e}"))
}

fn valid_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        Err("Name must not be empty".into())
    } else {
        Ok(())
    }
}

fn finite(values: &[f64]) -> Result<(), String> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err("Numbers must be finite".into())
    }
}

fn level_exists(doc: &Document, id: ElementId) -> Result<(), String> {
    if doc.model().level(id).is_some() {
        Ok(())
    } else {
        Err("level_id must refer to an existing level".into())
    }
}

fn check_frame(points: &[DVec2], radius: f64) -> Result<(), String> {
    finite(&[radius])?;
    for p in points {
        finite(&[
            p.x,
            p.y,
            p.x - radius,
            p.x + radius,
            p.y - radius,
            p.y + radius,
        ])?;
    }
    Ok(())
}

fn check_rigid_outline(before: &[DVec2], after: &[DVec2]) -> Result<(), String> {
    if before.len() != after.len() {
        return Err("Move would change the slab boundary".into());
    }
    for i in 0..before.len() {
        let j = (i + 1) % before.len();
        let edge_before = before[j] - before[i];
        let edge_after = after[j] - after[i];
        let difference = (edge_before - edge_after).length();
        if !difference.is_finite() || difference > (edge_before.length() * 1e-9).max(1e-9) {
            return Err("Move would deform the slab boundary due to precision loss".into());
        }
    }
    Ok(())
}

fn openings_overlap(a: &Opening, b: &Opening) -> bool {
    a.host == b.host
        && a.offset < b.offset + b.width - 1e-9
        && a.offset + a.width > b.offset + 1e-9
        && a.sill < b.sill + b.height - 1e-9
        && a.sill + a.height > b.sill + 1e-9
}

fn check_geometry(doc: &Document, element: &Element) -> Result<(), String> {
    element.validate()?;
    let elevation = |id| {
        doc.model()
            .level(id)
            .map(|l| l.elevation)
            .ok_or_else(|| "Element level not found".to_string())
    };
    match &element.kind {
        ElementKind::Wall(w) => {
            check_frame(&[w.start, w.end], w.thickness / 2.0)?;
            let base = elevation(w.level)? + w.base_offset;
            finite(&[base, base + w.height])?;
        }
        ElementKind::Slab(s) => {
            let top = elevation(s.level)? + s.top_offset;
            finite(&[top, top - s.thickness])?;
        }
        ElementKind::Column(c) => {
            let radius = match c.profile {
                Profile::Rectangle { width, depth } => width.hypot(depth) / 2.0,
                Profile::Round { diameter } => diameter / 2.0,
            };
            check_frame(&[c.position], radius)?;
            let base = elevation(c.level)? + c.base_offset;
            finite(&[base, base + c.height_in(doc.model())])?;
        }
        ElementKind::Beam(b) => {
            check_frame(&[b.start, b.end], b.width / 2.0)?;
            let top = b.top(doc.model());
            finite(&[top, top - b.depth])?;
        }
        _ => {}
    }
    Ok(())
}

fn commit_insert(doc: &mut Document, label: &str, element: Element) -> Result<Value, String> {
    check_geometry(doc, &element)?;
    let id = element.id;
    doc.commit(Transaction::new(label).insert(element))
        .map_err(|e| e.to_string())?;
    let e = doc.model().get(id).ok_or("Created element was not found")?;
    Ok(json!({"id": id, "element": e}))
}

/// Execute a single command against the active document. Mutations are undoable.
pub fn execute(doc: &mut Document, command: &str, args: Value) -> Result<Value, String> {
    match command {
        "inspect_document" => {
            let a: InspectArgs = parse(args)?;
            if a.limit == 0 || a.limit > 200 {
                return Err("limit must be between 1 and 200".into());
            }
            let filtered: Vec<_> = doc
                .model()
                .iter()
                .filter(|e| a.kind.as_deref().is_none_or(|k| e.kind.name() == k))
                .collect();
            let elements: Vec<_> = filtered
                .iter()
                .skip(a.offset)
                .take(a.limit)
                .map(|e| json!({"id": e.id, "name": e.name, "kind": e.kind.name()}))
                .collect();
            Ok(
                json!({"element_count": doc.model().len(), "matched_count": filtered.len(), "offset": a.offset, "limit": a.limit, "elements": elements}),
            )
        }
        "get_element" => {
            let a: ElementArgs = parse(args)?;
            let e = doc.model().get(a.element_id).ok_or("Element not found")?;
            serde_json::to_value(e).map_err(|e| e.to_string())
        }
        "create_level" => {
            let a: LevelArgs = parse(args)?;
            valid_name(&a.name)?;
            finite(&[a.elevation, a.height, a.elevation + a.height])?;
            if a.elevation + a.height == a.elevation {
                return Err("Level height cannot be represented at this elevation".into());
            }
            for (_, level) in doc.model().levels() {
                finite(&[level.elevation - a.elevation, a.elevation - level.elevation])?;
            }
            let (mut tx, id, plan) =
                crate::levels::add(doc.model(), &a.name, a.elevation, a.height, true);
            tx.label = "Create level (MCP)".into();
            doc.commit(tx).map_err(|e| e.to_string())?;
            let element = doc.model().get(id).ok_or("Created level was not found")?;
            Ok(json!({"id": id, "element": element, "plan_view_id": plan}))
        }
        "create_wall" => {
            let a: WallArgs = parse(args)?;
            valid_name(&a.name)?;
            level_exists(doc, a.level_id)?;
            finite(&[
                a.start[0],
                a.start[1],
                a.end[0],
                a.end[1],
                a.thickness,
                a.height,
            ])?;
            let wall = Wall::new(
                a.level_id,
                DVec2::from_array(a.start),
                DVec2::from_array(a.end),
                a.thickness,
                a.height,
            );
            commit_insert(
                doc,
                "Create wall (MCP)",
                Element::new(a.name, ElementKind::Wall(wall)),
            )
        }
        "create_slab" => {
            let a: SlabArgs = parse(args)?;
            valid_name(&a.name)?;
            level_exists(doc, a.level_id)?;
            finite(&[a.thickness])?;
            let mut points: Vec<_> = a.boundary.into_iter().map(DVec2::from_array).collect();
            if points.len() > 1000 {
                return Err("Slab boundary may have at most 1000 points".into());
            }
            if points.len() < 3 || points.iter().any(|p| !p.is_finite()) {
                return Err("Slab boundary needs at least three finite points".into());
            }
            if (0..points.len())
                .any(|i| points[i].distance_squared(points[(i + 1) % points.len()]) < 1e-18)
            {
                return Err("Slab boundary has a repeated adjacent point".into());
            }
            let area = crate::polygon::signed_area2(&points);
            if !area.is_finite()
                || area.abs() < 1e-9
                || crate::polygon::self_intersects(&points, true)
            {
                return Err("Slab boundary must be a simple polygon with non-zero area".into());
            }
            if area < 0.0 {
                points.reverse();
            }
            let slab = Slab::new(a.level_id, points, a.thickness);
            commit_insert(
                doc,
                "Create slab (MCP)",
                Element::new(a.name, ElementKind::Slab(slab)),
            )
        }
        "create_column" => {
            let a: ColumnArgs = parse(args)?;
            valid_name(&a.name)?;
            level_exists(doc, a.level_id)?;
            finite(&[a.position[0], a.position[1], a.width, a.depth, a.height])?;
            let mut column = Column::new(
                a.level_id,
                DVec2::from_array(a.position),
                Profile::Rectangle {
                    width: a.width,
                    depth: a.depth,
                },
            );
            column.height = Some(a.height);
            commit_insert(
                doc,
                "Create column (MCP)",
                Element::new(a.name, ElementKind::Column(column)),
            )
        }
        "create_beam" => {
            let a: BeamArgs = parse(args)?;
            valid_name(&a.name)?;
            level_exists(doc, a.level_id)?;
            finite(&[
                a.start[0],
                a.start[1],
                a.end[0],
                a.end[1],
                a.width,
                a.depth,
                a.top_offset,
            ])?;
            let mut beam = Beam::new(
                a.level_id,
                DVec2::from_array(a.start),
                DVec2::from_array(a.end),
            );
            beam.width = a.width;
            beam.depth = a.depth;
            beam.top_offset = a.top_offset;
            commit_insert(
                doc,
                "Create beam (MCP)",
                Element::new(a.name, ElementKind::Beam(beam)),
            )
        }
        "create_door" | "create_window" => {
            let a: OpeningArgs = parse(args)?;
            valid_name(&a.name)?;
            finite(&[a.offset, a.width, a.height, a.sill])?;
            let Some(ElementKind::Wall(wall)) = doc.model().get(a.wall_id).map(|e| &e.kind) else {
                return Err("wall_id must refer to an existing wall".into());
            };
            if a.width <= 0.0
                || a.height <= 0.0
                || a.offset < 0.0
                || a.sill < 0.0
                || a.offset + a.width > wall.length() + 1e-9
                || a.sill + a.height > wall.height + 1e-9
            {
                return Err("Opening must have positive dimensions and fit within its wall".into());
            }
            for e in doc.model().iter() {
                if let ElementKind::Opening(other) = &e.kind {
                    if other.host == a.wall_id
                        && a.offset < other.offset + other.width - 1e-9
                        && a.offset + a.width > other.offset + 1e-9
                        && a.sill < other.sill + other.height - 1e-9
                        && a.sill + a.height > other.sill + 1e-9
                    {
                        return Err("Opening overlaps an existing opening in this wall".into());
                    }
                }
            }
            let opening = Opening {
                host: a.wall_id,
                kind: if command == "create_door" {
                    OpeningKind::Door
                } else {
                    OpeningKind::Window
                },
                offset: a.offset,
                sill: a.sill,
                width: a.width,
                height: a.height,
                material: None,
                frame: OpeningFrame::default(),
                flip_hand: false,
                flip_facing: false,
            };
            commit_insert(
                doc,
                "Create opening (MCP)",
                Element::new(a.name, ElementKind::Opening(opening)),
            )
        }
        "move_element" => {
            let a: MoveArgs = parse(args)?;
            finite(&a.delta)?;
            if a.delta == [0.0, 0.0, 0.0] {
                return Err("Move delta must not be zero".into());
            }
            let before = doc
                .model()
                .get(a.element_id)
                .ok_or("Element not found")?
                .clone();
            if !matches!(
                before.kind,
                ElementKind::Wall(_)
                    | ElementKind::Slab(_)
                    | ElementKind::Column(_)
                    | ElementKind::Beam(_)
            ) {
                return Err("Moving this element type is not supported yet".into());
            }
            let after = crate::transform::transformed(
                doc.model(),
                &before,
                &Xform::Move(DVec3::from_array(a.delta)),
            )
            .ok_or("This element cannot be moved by that displacement")?;
            if after == before {
                return Err("Move would not change the element".into());
            }
            match &after.kind {
                ElementKind::Wall(w) => {
                    finite(&[w.start.x, w.start.y, w.end.x, w.end.y, w.base_offset])?;
                    if let crate::WallTop::UpToLevel { offset, .. } = w.top {
                        finite(&[offset])?;
                    }
                }
                ElementKind::Slab(s) => {
                    finite(&[s.top_offset])?;
                    if s.boundary
                        .iter()
                        .chain(s.holes.iter().flatten())
                        .any(|p| !p.is_finite())
                    {
                        return Err("Move would produce non-finite coordinates".into());
                    }
                }
                ElementKind::Column(c) => finite(&[c.position.x, c.position.y, c.base_offset])?,
                ElementKind::Beam(b) => {
                    finite(&[b.start.x, b.start.y, b.end.x, b.end.y, b.top_offset])?
                }
                _ => unreachable!(),
            }
            if let (ElementKind::Slab(old), ElementKind::Slab(new)) = (&before.kind, &after.kind) {
                check_rigid_outline(&old.boundary, &new.boundary)?;
                if old.holes.len() != new.holes.len() {
                    return Err("Move would change the slab holes".into());
                }
                for (old_hole, new_hole) in old.holes.iter().zip(&new.holes) {
                    check_rigid_outline(old_hole, new_hole)?;
                }
            }
            let mut changed = vec![after];
            let following = crate::joins::follow(doc.model(), &mut changed);
            let mut tx = Transaction::new("Move element (MCP)");
            if before != changed[0] {
                tx = tx.replace(before, changed.remove(0));
            }
            for (old, new) in following {
                tx = tx.replace(old, new);
            }
            if tx.ops.is_empty() {
                return Err("Move would not change the element".into());
            }
            for op in &tx.ops {
                if let Op::Replace { after, .. } = op {
                    check_geometry(doc, after)?;
                }
            }
            let replacement = |id| {
                tx.ops.iter().find_map(|op| match op {
                    Op::Replace { after, .. } if after.id == id => Some(after),
                    _ => None,
                })
            };
            let mut openings: std::collections::HashMap<ElementId, Vec<(&Opening, &Opening)>> =
                std::collections::HashMap::new();
            for original in doc.model().iter() {
                let ElementKind::Opening(previous) = &original.kind else {
                    continue;
                };
                let current = replacement(original.id).unwrap_or(original);
                let ElementKind::Opening(opening) = &current.kind else {
                    continue;
                };
                let Some(host) = replacement(opening.host) else {
                    continue;
                };
                let ElementKind::Wall(wall) = &host.kind else {
                    continue;
                };
                if opening.offset + opening.width > wall.length() + 1e-9
                    || opening.sill + opening.height > wall.height + 1e-9
                {
                    return Err("Move would place an existing opening outside its host wall".into());
                }
                openings
                    .entry(opening.host)
                    .or_default()
                    .push((previous, opening));
            }
            for group in openings.values() {
                for i in 0..group.len() {
                    for j in i + 1..group.len() {
                        if !openings_overlap(group[i].0, group[j].0)
                            && openings_overlap(group[i].1, group[j].1)
                        {
                            return Err("Move would cause existing openings to overlap".into());
                        }
                    }
                }
            }
            doc.commit(tx).map_err(|e| e.to_string())?;
            let e = doc
                .model()
                .get(a.element_id)
                .ok_or("Moved element was not found")?;
            Ok(json!({"id": e.id, "element": e}))
        }
        "delete_element" => {
            let a: ElementArgs = parse(args)?;
            let e = doc
                .model()
                .get(a.element_id)
                .ok_or("Element not found")?
                .clone();
            if !matches!(
                e.kind,
                ElementKind::Wall(_)
                    | ElementKind::Opening(_)
                    | ElementKind::Column(_)
                    | ElementKind::Beam(_)
            ) {
                return Err("Deleting this element type is not supported yet".into());
            }
            let dependents = doc.model().dependents(a.element_id);
            if !dependents.is_empty() {
                return Err(format!(
                    "Cannot delete element: {} dependent element(s) exist",
                    dependents.len()
                ));
            }
            let mut tx = Transaction::new("Delete element (MCP)");
            tx.ops.push(Op::Remove(e));
            doc.commit(tx).map_err(|e| e.to_string())?;
            Ok(json!({"deleted_id": a.element_id}))
        }
        _ => Err(format!("Unknown command: {command}")),
    }
}

#[cfg(test)]
mod tests {
    use super::execute;
    use crate::{Document, ElementKind};
    use serde_json::json;

    #[test]
    fn creates_wall_and_undoes_in_one_step() {
        let mut d = Document::new();
        let level = execute(
            &mut d,
            "create_level",
            json!({"name":"Floor","elevation":0,"height":3}),
        )
        .unwrap();
        let level_id = level["id"].as_str().unwrap();
        assert_eq!(level_id.len(), 36);
        let wall = execute(&mut d, "create_wall", json!({"name":"Wall","level_id":level_id,"start":[0,0],"end":[4,0],"thickness":0.2,"height":3})).unwrap();
        assert_eq!(wall["id"].as_str().unwrap().len(), 36);
        assert_eq!(d.model().len(), 3);
        assert!(d.undo());
        assert_eq!(d.model().len(), 2);
        assert!(d.redo());
        assert_eq!(d.model().len(), 3);
        let state = execute(&mut d, "inspect_document", json!({})).unwrap();
        assert_eq!(state["element_count"], 3);
    }

    #[test]
    fn invalid_inputs_never_change_model_or_history() {
        let mut d = Document::new();
        let level = execute(
            &mut d,
            "create_level",
            json!({"name":"Floor","elevation":0,"height":3}),
        )
        .unwrap();
        let level_id = level["id"].as_str().unwrap();
        let undo_count = d.undo_steps().len();
        for args in [
            json!({"name":"bad","level_id":level_id,"start":[0,0],"end":[0,0],"thickness":0.2,"height":3}),
            json!({"name":"bad","level_id":level_id,"start":[0,0],"end":[4,0],"thickness":-1,"height":3}),
            json!({"name":"bad","level_id":"00000000-0000-0000-0000-000000000000","start":[0,0],"end":[4,0],"thickness":0.2,"height":3}),
        ] {
            assert!(execute(&mut d, "create_wall", args).is_err());
        }
        assert!(execute(
            &mut d,
            "create_slab",
            json!({"name":"bad","level_id":level_id,"boundary":[[0,0],[1,1],[2,2]],"thickness":0.2})
        )
        .is_err());
        assert!(execute(
            &mut d,
            "create_level",
            json!({"name":"bad","elevation":null,"height":3})
        )
        .is_err());
        assert_eq!(d.model().len(), 2);
        assert_eq!(d.undo_steps().len(), undo_count);
    }

    #[test]
    fn opening_fits_host_and_delete_refuses_dependencies() {
        let mut d = Document::new();
        let level = execute(
            &mut d,
            "create_level",
            json!({"name":"Floor","elevation":0,"height":3}),
        )
        .unwrap();
        let wall = execute(&mut d, "create_wall", json!({"name":"Wall","level_id":level["id"],"start":[0,0],"end":[4,0],"thickness":0.2,"height":3})).unwrap();
        let base_count = d.model().len();
        assert!(execute(&mut d,"create_door",json!({"name":"bad","wall_id":wall["id"],"offset":3.5,"width":1.0,"height":2.0,"sill":0.0})).is_err());
        assert_eq!(d.model().len(), base_count);
        let door=execute(&mut d,"create_door",json!({"name":"Door","wall_id":wall["id"],"offset":1.0,"width":1.0,"height":2.0,"sill":0.0})).unwrap();
        assert!(execute(&mut d, "delete_element", json!({"element_id":wall["id"]})).is_err());
        execute(&mut d, "delete_element", json!({"element_id":door["id"]})).unwrap();
        execute(&mut d, "delete_element", json!({"element_id":wall["id"]})).unwrap();
        assert_eq!(d.model().len(), 2);
    }

    #[test]
    fn creates_slab_and_moves_wall_without_losing_other_elements() {
        let mut d = Document::new();
        let level = execute(
            &mut d,
            "create_level",
            json!({"name":"Floor","elevation":0,"height":3}),
        )
        .unwrap();
        let wall=execute(&mut d,"create_wall",json!({"name":"Wall","level_id":level["id"],"start":[0,0],"end":[4,0],"thickness":0.2,"height":3})).unwrap();
        execute(&mut d,"create_slab",json!({"name":"Floor slab","level_id":level["id"],"boundary":[[0,0],[4,0],[4,4],[0,4]],"thickness":0.2})).unwrap();
        execute(
            &mut d,
            "move_element",
            json!({"element_id":wall["id"],"delta":[1,2,0]}),
        )
        .unwrap();
        let e = d.model().iter().find(|e| e.name == "Wall").unwrap();
        let ElementKind::Wall(w) = &e.kind else {
            panic!("expected wall")
        };
        assert_eq!(
            (w.start.x, w.start.y, w.end.x, w.end.y),
            (1.0, 2.0, 5.0, 2.0)
        );
        assert_eq!(d.model().len(), 4);
        assert!(d.undo());
        let ElementKind::Wall(w) = &d.model().iter().find(|e| e.name == "Wall").unwrap().kind
        else {
            panic!("expected wall")
        };
        assert_eq!((w.start.x, w.start.y), (0.0, 0.0));
    }
    #[test]
    fn inspect_is_paginated_and_returns_native_properties() {
        let mut d = Document::new();
        let level = execute(
            &mut d,
            "create_level",
            json!({"name":"Floor","elevation":0.5,"height":3}),
        )
        .unwrap();
        let wall = execute(&mut d, "create_wall", json!({"name":"Wall","level_id":level["id"],"start":[0,0],"end":[4,0],"thickness":0.2,"height":3})).unwrap();
        let window = execute(&mut d, "create_window", json!({"name":"Window","wall_id":wall["id"],"offset":1,"width":1.2,"height":1.2,"sill":0.9})).unwrap();
        let list = execute(
            &mut d,
            "inspect_document",
            json!({"kind":"Opening","offset":0,"limit":1}),
        )
        .unwrap();
        assert_eq!(list["element_count"], 4);
        assert_eq!(list["matched_count"], 1);
        assert_eq!(list["elements"][0]["id"], window["id"]);
        let e = execute(&mut d, "get_element", json!({"element_id":window["id"]})).unwrap();
        assert_eq!(e["kind"]["Opening"]["width"], 1.2);
        assert!(execute(&mut d, "inspect_document", json!({"limit":201})).is_err());
        assert!(execute(&mut d, "get_element", json!({"element_id":"bad"})).is_err());
    }

    #[test]
    fn refuses_wrong_level_crossed_slab_and_invalid_opening_without_committing() {
        let mut d = Document::new();
        let level = execute(
            &mut d,
            "create_level",
            json!({"name":"Floor","elevation":0,"height":3}),
        )
        .unwrap();
        let wall=execute(&mut d,"create_wall",json!({"name":"Wall","level_id":level["id"],"start":[0,0],"end":[4,0],"thickness":0.2,"height":3})).unwrap();
        let before = d.undo_steps().len();
        assert!(execute(&mut d,"create_wall",json!({"name":"wrong level","level_id":wall["id"],"start":[0,0],"end":[4,0],"thickness":0.2,"height":3})).is_err());
        assert!(execute(&mut d,"create_slab",json!({"name":"crossed","level_id":level["id"],"boundary":[[0,0],[4,4],[0,4],[4,0]],"thickness":0.2})).is_err());
        assert!(execute(&mut d,"create_window",json!({"name":"too tall","wall_id":wall["id"],"offset":0,"width":1,"height":2.5,"sill":1})).is_err());
        assert_eq!(d.undo_steps().len(), before);
        assert_eq!(d.model().len(), 3);
    }
    #[test]
    fn creates_structural_column_and_beam_with_valid_dimensions() {
        let mut d = Document::new();
        let level = execute(
            &mut d,
            "create_level",
            json!({"name":"Floor","elevation":0,"height":3}),
        )
        .unwrap();
        let column=execute(&mut d,"create_column",json!({"name":"C1","level_id":level["id"],"position":[1,2],"width":0.3,"depth":0.4,"height":3})).unwrap();
        let beam=execute(&mut d,"create_beam",json!({"name":"B1","level_id":level["id"],"start":[1,2],"end":[4,2],"width":0.2,"depth":0.4,"top_offset":0})).unwrap();
        assert_eq!(
            column["element"]["kind"]["Column"]["profile"]["Rectangle"]["width"],
            0.3
        );
        assert_eq!(beam["element"]["kind"]["Beam"]["depth"], 0.4);
        let moved = execute(
            &mut d,
            "move_element",
            json!({"element_id":column["id"],"delta":[1,0,0]}),
        )
        .unwrap();
        assert_eq!(
            moved["element"]["kind"]["Column"]["position"],
            json!([2.0, 2.0])
        );
        assert!(d.undo());
        let before = d.undo_steps().len();
        assert!(execute(&mut d,"create_column",json!({"name":"bad","level_id":level["id"],"position":[1,2],"width":0,"depth":0.4,"height":3})).is_err());
        assert!(execute(&mut d,"create_beam",json!({"name":"bad","level_id":level["id"],"start":[1,2],"end":[1,2],"width":0.2,"depth":0.4})).is_err());
        assert_eq!(before, d.undo_steps().len());
        assert_eq!(d.model().len(), 4);
        assert!(d.undo());
        assert_eq!(d.model().len(), 3);
    }

    #[test]
    fn overflow_in_move_does_not_commit_or_corrupt_model() {
        let mut d = Document::new();
        let level = execute(
            &mut d,
            "create_level",
            json!({"name":"Floor","elevation":0,"height":3}),
        )
        .unwrap();
        let wall=execute(&mut d,"create_wall",json!({"name":"Wall","level_id":level["id"],"start":[0,0],"end":[4,0],"thickness":0.2,"height":3})).unwrap();
        execute(
            &mut d,
            "move_element",
            json!({"element_id":wall["id"],"delta":[0,0,1e308]}),
        )
        .unwrap();
        let before = d.undo_steps().len();
        assert!(execute(
            &mut d,
            "move_element",
            json!({"element_id":wall["id"],"delta":[0,0,1e308]})
        )
        .is_err());
        assert_eq!(d.undo_steps().len(), before);
        let wall = d.model().iter().find(|e| e.name == "Wall").unwrap();
        let ElementKind::Wall(wall) = &wall.kind else {
            panic!("expected wall")
        };
        assert!(wall.base_offset.is_finite());
    }
    #[test]
    fn movement_that_cannot_change_floating_point_coordinates_is_rejected() {
        let mut d = Document::new();
        let level = execute(
            &mut d,
            "create_level",
            json!({"name":"Floor","elevation":0,"height":3}),
        )
        .unwrap();
        let wall=execute(&mut d,"create_wall",json!({"name":"Wall","level_id":level["id"],"start":[100000000,0],"end":[100000004,0],"thickness":0.2,"height":3})).unwrap();
        let before = d.undo_steps().len();
        assert!(execute(
            &mut d,
            "move_element",
            json!({"element_id":wall["id"],"delta":[1e-12,0,0]})
        )
        .is_err());
        assert_eq!(d.undo_steps().len(), before);
    }
    #[test]
    fn moving_joined_wall_follows_native_join_and_undo_restores_both() {
        let mut d = Document::new();
        let level = execute(
            &mut d,
            "create_level",
            json!({"name":"Floor","elevation":0,"height":3}),
        )
        .unwrap();
        let a = execute(&mut d,"create_wall",json!({"name":"A","level_id":level["id"],"start":[0,0],"end":[4,0],"thickness":0.2,"height":3})).unwrap();
        execute(&mut d,"create_wall",json!({"name":"B","level_id":level["id"],"start":[4,0],"end":[4,4],"thickness":0.2,"height":3})).unwrap();
        let before = d.model().clone();
        let undo_before = d.undo_steps().len();
        execute(
            &mut d,
            "move_element",
            json!({"element_id":a["id"],"delta":[1,0,0]}),
        )
        .unwrap();
        let ElementKind::Wall(w) = &d.model().iter().find(|e| e.name == "B").unwrap().kind else {
            panic!("expected wall")
        };
        assert!(
            (w.start.x - 5.0).abs() < 1e-8 && w.start.y.abs() < 1e-8,
            "joined start: {:?}",
            w.start
        );
        assert_eq!(d.undo_steps().len(), undo_before + 1);
        assert!(d.undo());
        assert_eq!(d.model(), &before);
    }

    #[test]
    fn prevents_opening_overlap_but_allows_touching_or_stacked() {
        let mut d = Document::new();
        let level = execute(
            &mut d,
            "create_level",
            json!({"name":"Floor","elevation":0,"height":3}),
        )
        .unwrap();
        let wall=execute(&mut d,"create_wall",json!({"name":"Wall","level_id":level["id"],"start":[0,0],"end":[4,0],"thickness":0.2,"height":3})).unwrap();
        execute(
            &mut d,
            "create_door",
            json!({"name":"Door","wall_id":wall["id"],"offset":1,"width":1,"height":2,"sill":0}),
        )
        .unwrap();
        let before = d.undo_steps().len();
        assert!(execute(&mut d,"create_window",json!({"name":"Overlap","wall_id":wall["id"],"offset":1.5,"width":1,"height":1,"sill":1})).is_err());
        assert_eq!(d.undo_steps().len(), before);
        execute(
            &mut d,
            "create_window",
            json!({"name":"Touch","wall_id":wall["id"],"offset":2,"width":1,"height":1,"sill":1}),
        )
        .unwrap();
        execute(
            &mut d,
            "create_window",
            json!({"name":"Above","wall_id":wall["id"],"offset":1,"width":1,"height":1,"sill":2}),
        )
        .unwrap();
    }

    #[test]
    fn rejects_level_height_overflow_and_excessive_slab_outline() {
        let mut d = Document::new();
        assert!(execute(
            &mut d,
            "create_level",
            json!({"name":"overflow","elevation":1e308,"height":1e308})
        )
        .is_err());
        assert_eq!(d.model().len(), 0);
        let level = execute(
            &mut d,
            "create_level",
            json!({"name":"Floor","elevation":0,"height":3}),
        )
        .unwrap();
        let err=execute(&mut d,"create_slab",json!({"name":"too many","level_id":level["id"],"boundary":vec![[0.0,0.0];1001],"thickness":0.2})).unwrap_err();
        assert!(err.contains("1000"), "{err}");
        assert_eq!(d.model().len(), 2);
    }
    #[test]
    fn joined_move_rejects_host_shrinking_past_an_opening() {
        let mut d = Document::new();
        let level = execute(
            &mut d,
            "create_level",
            json!({"name":"Floor","elevation":0,"height":3}),
        )
        .unwrap();
        let a=execute(&mut d,"create_wall",json!({"name":"A","level_id":level["id"],"start":[0,0],"end":[4,0],"thickness":0.2,"height":3})).unwrap();
        let b=execute(&mut d,"create_wall",json!({"name":"B","level_id":level["id"],"start":[4,0],"end":[4,4],"thickness":0.2,"height":3})).unwrap();
        execute(
            &mut d,
            "create_door",
            json!({"name":"Door","wall_id":a["id"],"offset":3.3,"width":0.6,"height":2,"sill":0}),
        )
        .unwrap();
        let before = d.model().clone();
        let undo_before = d.undo_steps().len();
        assert!(execute(
            &mut d,
            "move_element",
            json!({"element_id":b["id"],"delta":[-1,0,0]})
        )
        .is_err());
        assert_eq!(d.model(), &before);
        assert_eq!(d.undo_steps().len(), undo_before);
    }

    #[test]
    fn rejects_structural_geometry_with_non_finite_derived_coordinates() {
        let mut d = Document::new();
        let level = execute(
            &mut d,
            "create_level",
            json!({"name":"High","elevation":1e308,"height":1e293}),
        )
        .unwrap();
        let before = d.undo_steps().len();
        assert!(execute(&mut d,"create_wall",json!({"name":"Overflow","level_id":level["id"],"start":[0,0],"end":[4,0],"thickness":0.2,"height":1e308})).is_err());
        assert!(execute(&mut d,"create_column",json!({"name":"Overflow","level_id":level["id"],"position":[0,0],"width":0.2,"depth":0.2,"height":1e308})).is_err());
        assert!(execute(&mut d,"create_beam",json!({"name":"Overflow","level_id":level["id"],"start":[0,0],"end":[4,0],"width":0.2,"depth":0.4,"top_offset":-1e308})).is_err());
        assert_eq!(d.undo_steps().len(), before);
        assert_eq!(d.model().len(), 2);
    }
    #[test]
    fn refuses_moving_slab_when_float_precision_deforms_boundary() {
        let mut d = Document::new();
        let level = execute(
            &mut d,
            "create_level",
            json!({"name":"Floor","elevation":0,"height":3}),
        )
        .unwrap();
        let slab=execute(&mut d,"create_slab",json!({"name":"Slab","level_id":level["id"],"boundary":[[0,0],[1,0],[1,1],[0,1]],"thickness":0.2})).unwrap();
        let before = d.model().clone();
        let undo = d.undo_steps().len();
        assert!(execute(
            &mut d,
            "move_element",
            json!({"element_id":slab["id"],"delta":[1e16,0,0]})
        )
        .is_err());
        assert_eq!(d.model(), &before);
        assert_eq!(d.undo_steps().len(), undo);
    }

    #[test]
    fn joined_move_rejects_new_overlap_between_openings() {
        let mut d = Document::new();
        let level = execute(
            &mut d,
            "create_level",
            json!({"name":"Floor","elevation":0,"height":3}),
        )
        .unwrap();
        let a=execute(&mut d,"create_wall",json!({"name":"A","level_id":level["id"],"start":[0,0],"end":[4,0],"thickness":0.2,"height":3})).unwrap();
        let b=execute(&mut d,"create_wall",json!({"name":"B","level_id":level["id"],"start":[0,0],"end":[0,4],"thickness":0.2,"height":3})).unwrap();
        execute(
            &mut d,
            "create_door",
            json!({"name":"Door 1","wall_id":a["id"],"offset":0.5,"width":1,"height":2,"sill":0}),
        )
        .unwrap();
        execute(
            &mut d,
            "create_door",
            json!({"name":"Door 2","wall_id":a["id"],"offset":2,"width":1,"height":2,"sill":0}),
        )
        .unwrap();
        let before = d.model().clone();
        let undo = d.undo_steps().len();
        assert!(execute(
            &mut d,
            "move_element",
            json!({"element_id":b["id"],"delta":[1.5,0,0]})
        )
        .is_err());
        assert_eq!(d.model(), &before);
        assert_eq!(d.undo_steps().len(), undo);
    }
}
