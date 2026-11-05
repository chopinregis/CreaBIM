//! A Properties change made with several elements selected goes to every
//! selected element of the same kind: set one wall's height to 3 m with
//! four walls picked and all four are 3 m tall, in one undo step.
//!
//! What was changed is read by comparing the element before and after the
//! edit, field by field. Where an element is placed (its points, its host,
//! its place along a host) stays its own, so a change never stacks them up.

use creabim_core::{Element, ElementKind};
use serde_json::Value;

/// Fields that say where an element is, never spread to the others.
const PLACE: &[&str] = &[
    "start", "end", "a", "b", "at", "points", "outline", "boundary", "path", "profile", "origin",
    "centre", "center", "position", "pos", "host", "offset", "along", "holes", "vertices", "faces",
    "mesh", "rotation", "angle",
];

/// A list of numbers or of lists of numbers: a point or a set of points.
fn is_geometry(v: &Value) -> bool {
    match v {
        Value::Array(xs) => {
            !xs.is_empty()
                && xs
                    .iter()
                    .all(|x| x.is_number() || matches!(x, Value::Array(_)) && is_geometry(x))
        }
        _ => false,
    }
}

/// Copy into `other` what changed from `old` to `new`, at the same paths.
/// Returns whether anything was copied.
fn spread(old: &Value, new: &Value, other: &mut Value, key: Option<&str>) -> bool {
    if old == new {
        return false;
    }
    if key.is_some_and(|k| PLACE.contains(&k)) {
        return false;
    }
    match (old, new, &mut *other) {
        (Value::Object(o), Value::Object(n), Value::Object(t)) => {
            let mut any = false;
            for (k, nv) in n {
                let ov = o.get(k).unwrap_or(&Value::Null);
                match t.get_mut(k) {
                    Some(tv) => any |= spread(ov, nv, tv, Some(k)),
                    // a field the other leaves at its default, changed here
                    None if ov != nv && !PLACE.contains(&k.as_str()) && !is_geometry(nv) => {
                        t.insert(k.clone(), nv.clone());
                        any = true;
                    }
                    None => {}
                }
            }
            // a field cleared on the edited one is cleared on the others
            for k in o.keys().filter(|k| !n.contains_key(*k)) {
                if !PLACE.contains(&k.as_str()) && t.remove(k).is_some() {
                    any = true;
                }
            }
            any
        }
        _ if is_geometry(new) || is_geometry(old) => false,
        _ => {
            if *other == *new {
                return false;
            }
            *other = new.clone();
            true
        }
    }
}

/// `other` with the change from `before` to `after` made to it too, or
/// `None` when it is another kind of element or nothing would change.
pub fn apply_to(before: &Element, after: &Element, other: &Element) -> Option<Element> {
    if other.id == before.id
        || std::mem::discriminant(&before.kind) != std::mem::discriminant(&other.kind)
    {
        return None;
    }
    let (Ok(o), Ok(n), Ok(mut t)) = (
        serde_json::to_value(&before.kind),
        serde_json::to_value(&after.kind),
        serde_json::to_value(&other.kind),
    ) else {
        return None;
    };
    if !spread(&o, &n, &mut t, None) {
        return None;
    }
    let kind: ElementKind = serde_json::from_value(t).ok()?;
    let mut out = other.clone();
    out.kind = kind;
    // the type an element is an instance of also goes with it
    if before.of_type != after.of_type {
        out.of_type = after.of_type.clone();
    }
    (out != *other).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use creabim_core::{Document, ElementId};
    use glam::DVec2;

    fn walls(n: usize) -> (Document, Vec<ElementId>) {
        let mut doc = Document::new();
        let l = Element::new(
            "L0",
            ElementKind::Level(creabim_core::Level {
                elevation: 0.0,
                height: 3.0,
            }),
        );
        let level = l.id;
        let mut tx = creabim_core::Transaction::new("setup").insert(l);
        let mut ids = vec![];
        for i in 0..n {
            let y = i as f64 * 3.0;
            let w =
                creabim_core::Wall::new(level, DVec2::new(0.0, y), DVec2::new(5.0, y), 0.2, 2.7);
            let e = Element::new("W", ElementKind::Wall(w));
            ids.push(e.id);
            tx = tx.insert(e);
        }
        doc.commit(tx).unwrap();
        (doc, ids)
    }

    fn wall(doc: &Document, id: ElementId) -> creabim_core::Wall {
        match &doc.model().get(id).unwrap().kind {
            ElementKind::Wall(w) => w.clone(),
            _ => unreachable!(),
        }
    }

    #[test]
    fn a_height_change_goes_to_every_wall_but_each_keeps_its_place() {
        let (doc, ids) = walls(3);
        let before = doc.model().get(ids[0]).unwrap().clone();
        let mut after = before.clone();
        if let ElementKind::Wall(w) = &mut after.kind {
            w.height = 3.0;
            w.start.x = 1.0; // a move of the edited one is its own
        }
        let other = doc.model().get(ids[1]).unwrap();
        let out = apply_to(&before, &after, other).expect("the other wall changes");
        let ElementKind::Wall(w) = &out.kind else {
            unreachable!()
        };
        assert_eq!(w.height, 3.0);
        assert_eq!(w.start, wall(&doc, ids[1]).start);
        assert_eq!(w.thickness, 0.2);
    }

    #[test]
    fn nothing_spreads_to_another_kind_or_when_nothing_changed() {
        let (doc, ids) = walls(2);
        let before = doc.model().get(ids[0]).unwrap().clone();
        assert!(apply_to(&before, &before, doc.model().get(ids[1]).unwrap()).is_none());
        let level = doc
            .model()
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Level(_)))
            .unwrap()
            .clone();
        let mut after = before.clone();
        if let ElementKind::Wall(w) = &mut after.kind {
            w.height = 3.0;
        }
        assert!(apply_to(&before, &after, &level).is_none());
    }
}

#[cfg(test)]
mod house_tests {
    use super::*;
    use creabim_core::Document;

    #[test]
    fn a_base_offset_on_two_hall_walls_leaves_their_joins() {
        let m = creabim_core::starter::starter_house();
        let id = |n: &str| m.iter().find(|e| e.name == n).unwrap().id;
        let (w, e) = (id("GF Hall West"), id("GF Hall East"));
        let mut doc = Document::from_model(m.clone());
        let before = doc.model().get(w).unwrap().clone();
        let mut after = before.clone();
        if let ElementKind::Wall(x) = &mut after.kind {
            x.base_offset = 0.15;
        }
        let other = doc.model().get(e).unwrap().clone();
        let out = apply_to(&before, &after, &other).unwrap();
        let joins = |el: &Element| match &el.kind {
            ElementKind::Wall(x) => x.disallow_join,
            _ => unreachable!(),
        };
        assert_eq!(joins(&out), joins(&other));
        assert!(matches!(&out.kind, ElementKind::Wall(x) if x.base_offset == 0.15));
        let tx = creabim_core::Transaction::new("Base offset")
            .replace(before, after)
            .replace(other.clone(), out);
        doc.commit_merged(tx).unwrap();
        assert_eq!(joins(doc.model().get(e).unwrap()), joins(&other));
    }
}
