//! Commands for editing an active document through an external agent.
//! Lengths and coordinates are in metres.

use creabim_core::{
    Beam, Column, Document, Element, ElementId, ElementKind, Op, Opening, Profile, Transaction,
    Xform,
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

pub(super) fn valid_name(name: &str) -> Result<(), String> {
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
pub(super) fn execute(doc: &mut Document, command: &str, args: Value) -> Result<Value, String> {
    match command {
        "inspect_document" => {
            let a: InspectArgs = parse(args)?;
            if a.limit == 0 || a.limit > 200 {
                return Err("limit must be between 1 and 200".into());
            }
            let filtered: Vec<_> = doc
                .model()
                .iter()
                .filter(|e| {
                    a.kind
                        .as_deref()
                        .is_none_or(|k| e.kind.name().eq_ignore_ascii_case(k))
                })
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
                creabim_core::levels::add(doc.model(), &a.name, a.elevation, a.height, true);
            tx.label = "Create level (MCP)".into();
            doc.commit(tx).map_err(|e| e.to_string())?;
            let element = doc.model().get(id).ok_or("Created level was not found")?;
            Ok(json!({"id": id, "element": element, "plan_view_id": plan}))
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
            let after = creabim_core::transform::transformed(
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
                    if let creabim_core::WallTop::UpToLevel { offset, .. } = w.top {
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
            let following = creabim_core::joins::follow(doc.model(), &mut changed);
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
                    || opening.sill + opening.height
                        > creabim_core::walls::constrained_height(doc.model(), wall)
                            .unwrap_or(wall.height)
                            + 1e-9
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
