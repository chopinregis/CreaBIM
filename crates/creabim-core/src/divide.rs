//! Divide (MU34 B2): a length split into even parts, and something made in
//! them. The length is a straight element's: a wall, a beam, a guide, or a
//! part pulled up from a plan outline (its longest side). What is made:
//! the element cut into the parts (cabinet bays, shelves), guides at the
//! divisions to snap to, or copies of another element, one in each part.

use crate::{Element, ElementId, ElementKind, Model, Transaction};
use glam::{DVec2, DVec3};

/// What a division makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Make {
    /// The element itself, cut into the parts (a part only).
    Cuts,
    /// A guide at each division.
    Guides,
    /// The other element, copied into each part: along, at each part's
    /// middle; up, at each division (a shelf at each).
    Copies,
}

/// Which way a length is divided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// Along its length, in plan.
    Along,
    /// Up its height.
    Up,
}

/// A straight element's length in plan: from `o` along the unit `u` for
/// `len`, `width` across it to its left (`v`), from `z0` up to `z1` above
/// its level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub level: Option<ElementId>,
    pub o: DVec2,
    pub u: DVec2,
    pub v: DVec2,
    pub len: f64,
    pub width: f64,
    pub z0: f64,
    pub z1: f64,
}

/// The span of `id`, if it has one.
pub fn span(model: &Model, id: ElementId) -> Option<Span> {
    let e = model.get(id)?;
    let line = |level: Option<ElementId>, a: DVec2, b: DVec2, width: f64, z0: f64, z1: f64| {
        let len = a.distance(b);
        (len > 1e-6).then(|| {
            let u = (b - a) / len;
            Span { level, o: a, u, v: u.perp(), len, width, z0, z1 }
        })
    };
    match &e.kind {
        ElementKind::Wall(w) => line(Some(w.level), w.start, w.end, w.thickness, w.base_offset, w.base_offset + w.height),
        ElementKind::Beam(b) => line(Some(b.level), b.start, b.end, 0.0, 0.0, 0.0),
        ElementKind::Guide(g) => line(g.level, g.a.truncate(), g.b.truncate(), 0.0, g.a.z, g.a.z),
        ElementKind::Part(p) => match &p.form {
            crate::parts::Form::Extrusion { outline, holes, base, height, plane: None } if holes.is_empty() && outline.len() >= 3 => {
                // along its longest side, across to the far side of it
                let n = outline.len();
                let i = (0..n)
                    .max_by(|a, b| {
                        let la = outline[*a].distance(outline[(*a + 1) % n]);
                        let lb = outline[*b].distance(outline[(*b + 1) % n]);
                        la.total_cmp(&lb)
                    })?;
                let (a, b) = (outline[i], outline[(i + 1) % n]);
                let len = a.distance(b);
                let u = (b - a) / len;
                let c = outline.iter().copied().sum::<DVec2>() / n as f64;
                let v = if (c - a).dot(u.perp()) >= 0.0 { u.perp() } else { -u.perp() };
                let along: Vec<f64> = outline.iter().map(|p| (*p - a).dot(u)).collect();
                let across: Vec<f64> = outline.iter().map(|p| (*p - a).dot(v)).collect();
                let lo = along.iter().copied().fold(f64::MAX, f64::min);
                let hi = along.iter().copied().fold(f64::MIN, f64::max);
                let w = across.iter().copied().fold(f64::MIN, f64::max);
                Some(Span { level: Some(p.level), o: a + u * lo, u, v, len: hi - lo, width: w, z0: *base, z1: base + height })
            }
            _ => None,
        },
        _ => None,
    }
}

/// The parts' (start, size) along a length `len` split into `n` with
/// `gap` between them.
pub fn parts(len: f64, n: usize, gap: f64) -> Option<Vec<(f64, f64)>> {
    let n = n.max(1);
    let size = (len - gap * (n as f64 - 1.0)) / n as f64;
    (size > 1e-4).then(|| (0..n).map(|k| (k as f64 * (size + gap), size)).collect())
}

/// Divide `id` into `n` parts `dir`, making `make` (`item`: what Copies
/// copies). One undo step.
pub fn divide(
    model: &Model,
    id: ElementId,
    n: usize,
    dir: Dir,
    make: Make,
    gap: f64,
    item: Option<ElementId>,
) -> Result<Transaction, String> {
    if n < 2 {
        return Err("Divide into two parts or more.".into());
    }
    let e = model.get(id).ok_or("It is gone.")?;
    let s = span(model, id).ok_or_else(|| {
        format!("{} has no straight length to divide: pick a wall, a beam, a guide or a part drawn from a plan outline.", e.name)
    })?;
    let total = match dir {
        Dir::Along => s.len,
        Dir::Up => s.z1 - s.z0,
    };
    if total < 1e-6 {
        return Err(format!("{} has no height to divide.", e.name));
    }
    let gap = if make == Make::Cuts { gap.max(0.0) } else { 0.0 };
    let ps = parts(total, n, gap).ok_or("The gaps leave no room for the parts.")?;
    let mut tx = Transaction::new(format!("Divide {} in {n}", e.name));
    match make {
        Make::Cuts => {
            let ElementKind::Part(p) = &e.kind else {
                return Err("Only a part can be cut into parts; Guides or Copies divide the rest.".into());
            };
            let crate::parts::Form::Extrusion { base, .. } = &p.form else {
                return Err("Only a part pulled up from a plan outline can be cut.".into());
            };
            for (k, (t, size)) in ps.iter().enumerate() {
                let mut now = if k == 0 { e.clone() } else { Element::new(format!("{} {}", e.name, k + 1), e.kind.clone()) };
                if let ElementKind::Part(q) = &mut now.kind {
                    if let crate::parts::Form::Extrusion { outline, base: b, height, .. } = &mut q.form {
                        match dir {
                            Dir::Along => {
                                let a = s.o + s.u * *t;
                                let b2 = a + s.u * *size;
                                *outline = vec![a, b2, b2 + s.v * s.width, a + s.v * s.width];
                                // anticlockwise, as outlines are
                                if crate::polygon::signed_area2(outline) < 0.0 {
                                    outline.reverse();
                                }
                            }
                            Dir::Up => {
                                *b = base + t;
                                *height = *size;
                            }
                        }
                    }
                }
                tx = if k == 0 { tx.replace(e.clone(), now) } else { tx.insert(now) };
            }
        }
        Make::Guides => {
            for part in &ps[1..] {
                let t = part.0 - gap / 2.0;
                let g = match dir {
                    Dir::Along => {
                        let p = s.o + s.u * t;
                        let reach = s.width.max(1.0);
                        crate::Guide {
                            level: s.level,
                            a: (p - s.v * 0.5).extend(s.z0),
                            b: (p + s.v * (reach + 0.5)).extend(s.z0),
                        }
                    }
                    Dir::Up => {
                        let z = s.z0 + t;
                        crate::Guide { level: s.level, a: s.o.extend(z), b: (s.o + s.u * s.len).extend(z) }
                    }
                };
                tx = tx.insert(Element::new("Guide", ElementKind::Guide(g)));
            }
        }
        Make::Copies => {
            let item = item.ok_or("Select the element to copy too: the one to divide first, then what goes in each part.")?;
            let it = model.get(item).ok_or("It is gone.")?;
            let at = crate::transform::pivot(model, item).ok_or_else(|| format!("{} can't be placed.", it.name))?;
            let base = crate::members::elevation(model, s.level.unwrap_or(item));
            let base = if s.level.is_some() { base } else { 0.0 };
            // along: each part's middle; up: each division
            let spots: Vec<f64> = match dir {
                Dir::Along => ps.iter().map(|(t, size)| t + size / 2.0).collect(),
                Dir::Up => ps[1..].iter().map(|p| p.0).collect(),
            };
            for (k, t) in spots.iter().enumerate() {
                let d = match dir {
                    Dir::Along => (s.u * (t - (at.truncate() - s.o).dot(s.u))).extend(0.0),
                    Dir::Up => DVec3::Z * (base + s.z0 + t - at.z),
                };
                if k == 0 {
                    let moved = crate::transform::transformed(model, it, &crate::transform::Xform::Move(d))
                        .ok_or_else(|| format!("{} doesn't move that way.", it.name))?;
                    tx = tx.replace(it.clone(), moved);
                } else {
                    for c in crate::transform::duplicate(model, item) {
                        let moved = crate::transform::transformed(model, &c, &crate::transform::Xform::Move(d)).unwrap_or(c);
                        tx = tx.insert(moved);
                    }
                }
            }
        }
    }
    Ok(tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, Level};

    fn setup(outline: Vec<DVec2>) -> (Document, ElementId) {
        let l = Element::new("L0", ElementKind::Level(Level { elevation: 0.0, height: 3.0 }));
        let lid = l.id;
        let p = Element::new(
            "Cabinet",
            ElementKind::Part(crate::parts::Part {
                level: lid,
                form: crate::parts::Form::Extrusion { outline, holes: vec![], base: 0.0, height: 2.0, plane: None },
                void: false,
                material: None,
                ifc_class: "IfcBuildingElementProxy".into(),
            }),
        );
        let id = p.id;
        let mut doc = Document::new();
        doc.commit(Transaction::new("setup").insert(l).insert(p)).unwrap();
        (doc, id)
    }

    fn rect(w: f64, d: f64) -> Vec<DVec2> {
        vec![DVec2::ZERO, DVec2::new(w, 0.0), DVec2::new(w, d), DVec2::new(0.0, d)]
    }

    #[test]
    fn a_cabinet_cuts_into_even_bays_and_shelves() {
        let (mut doc, id) = setup(rect(3.68, 0.6));
        let tx = divide(doc.model(), id, 4, Dir::Along, Make::Cuts, 0.0, None).unwrap();
        doc.commit(tx).unwrap();
        let parts: Vec<f64> = doc
            .model()
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::Part(p) => match &p.form {
                    crate::parts::Form::Extrusion { outline, .. } => Some(crate::polygon::signed_area2(outline) / 2.0),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(parts.len(), 4);
        for a in parts {
            assert!((a - 0.92 * 0.6).abs() < 1e-9, "{a}");
        }
        let (mut doc, id) = setup(rect(1.0, 0.6));
        doc.commit(divide(doc.model(), id, 5, Dir::Up, Make::Cuts, 0.018, None).unwrap()).unwrap();
        let tops: Vec<f64> = doc
            .model()
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::Part(p) => match p.form {
                    crate::parts::Form::Extrusion { base, height, .. } => Some(base + height),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(tops.len(), 5);
        assert!(tops.iter().any(|t| (t - 2.0).abs() < 1e-9));
    }

    #[test]
    fn guides_mark_the_divisions_and_one_part_is_refused() {
        let (doc, id) = setup(rect(4.0, 0.6));
        let tx = divide(doc.model(), id, 4, Dir::Along, Make::Guides, 0.0, None).unwrap();
        assert_eq!(tx.ops.len(), 3);
        assert!(divide(doc.model(), id, 1, Dir::Along, Make::Guides, 0.0, None).is_err());
    }
}
