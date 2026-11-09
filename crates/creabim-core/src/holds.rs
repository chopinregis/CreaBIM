//! Dimensions that hold a spacing (brief 02, 2.10 part 3: the padlock).
//!
//! A held dimension measures between two elements, each a line (a wall's
//! centreline or a grid line) or a point (a column's centre), and keeps the
//! distance it was locked at. When an edit moves one of the two and not the
//! other, the other moves by as much, square to the line, so the spacing
//! holds. What it moves can carry others in turn: a grid takes the walls and
//! columns standing on it, a wall the walls joined to it. Each round of
//! moves can set off held dimensions further on, up to a few rounds.

use crate::{Element, ElementId, ElementKind, Model, Transaction};
use glam::DVec2;
use std::collections::{HashMap, HashSet};

/// How close (metres) a wall or column must be to a grid line to stand on
/// it and go with it.
pub const ON_LINE: f64 = 0.001;

/// What a held dimension measures from or to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Place {
    /// A line through `a` and `b`, endless for measuring.
    Line(DVec2, DVec2),
    Point(DVec2),
}

impl Place {
    fn origin(self) -> DVec2 {
        match self {
            Place::Line(a, _) | Place::Point(a) => a,
        }
    }
}

/// Where `id` is for measuring: walls and grid lines are lines, columns
/// points. Others can't hold a spacing.
pub fn place_of(model: &Model, id: ElementId) -> Option<Place> {
    match &model.get(id)?.kind {
        ElementKind::Wall(w) if w.length() > 1e-9 => Some(Place::Line(w.start, w.end)),
        ElementKind::Grid(g) if g.start.distance(g.end) > 1e-9 => Some(Place::Line(g.start, g.end)),
        ElementKind::Column(c) => Some(Place::Point(c.position)),
        // a level, in a section's (along, height) space: a level line at
        // its elevation (A5-2), held only against other levels
        ElementKind::Level(l) => Some(Place::Line(
            DVec2::new(0.0, l.elevation),
            DVec2::new(1.0, l.elevation),
        )),
        _ => None,
    }
}

/// The unit normal the spacing between `a` and `b` is measured along: a
/// line's, or the other's when `a` is a point. None when two lines aren't
/// parallel (within about a tenth of a degree) or both are points.
pub fn normal(a: Place, b: Place) -> Option<DVec2> {
    let dir = |p: DVec2, q: DVec2| (q - p).normalize_or_zero();
    match (a, b) {
        (Place::Line(p, q), Place::Line(r, s)) => {
            let (u, v) = (dir(p, q), dir(r, s));
            (u.perp_dot(v).abs() < 2e-3).then(|| u.perp())
        }
        (Place::Line(p, q), Place::Point(_)) | (Place::Point(_), Place::Line(p, q)) => {
            Some(dir(p, q).perp())
        }
        (Place::Point(_), Place::Point(_)) => None,
    }
}

/// The signed spacing from `a` to `b` along `n`.
pub fn signed(a: Place, b: Place, n: DVec2) -> f64 {
    (b.origin() - a.origin()).dot(n)
}

/// The spacing between two elements now, signed from `a` to `b`, with the
/// normal it is measured along.
pub fn spacing(model: &Model, a: ElementId, b: ElementId) -> Option<(f64, DVec2)> {
    let (pa, pb) = (place_of(model, a)?, place_of(model, b)?);
    let n = normal(pa, pb)?;
    Some((signed(pa, pb, n), n))
}

/// `id` moved bodily by `by` in plan, with what goes with it: the walls and
/// columns standing on a grid line, and the walls joined to a moved wall
/// (see [`crate::joins::follow`]). Nothing in `keep` is carried. Returns
/// (before, after) for each element that changes, the moved one first.
pub fn shift(
    model: &Model,
    id: ElementId,
    by: DVec2,
    keep: &HashSet<ElementId>,
) -> Vec<(Element, Element)> {
    let Some(e) = model.get(id) else {
        return vec![];
    };
    let mut moved = vec![];
    let mut now = e.clone();
    let carried: Vec<ElementId> = match &mut now.kind {
        ElementKind::Wall(w) => {
            w.start += by;
            w.end += by;
            vec![]
        }
        ElementKind::Column(c) => {
            c.position += by;
            vec![]
        }
        // up or down: what stands on it follows through its level
        ElementKind::Level(l) => {
            l.elevation += by.y;
            vec![]
        }
        ElementKind::Grid(g) => {
            let on = standing_on(model, g.start, g.end);
            g.start += by;
            g.end += by;
            on.into_iter().filter(|o| !keep.contains(o)).collect()
        }
        _ => return vec![],
    };
    moved.push(now);
    for c in carried {
        let Some(o) = model.get(c) else { continue };
        let mut n = o.clone();
        match &mut n.kind {
            ElementKind::Wall(w) => {
                w.start += by;
                w.end += by;
            }
            ElementKind::Column(col) => col.position += by,
            _ => continue,
        }
        moved.push(n);
    }
    let follow = crate::joins::follow(model, &mut moved);
    let mut out: Vec<(Element, Element)> = moved
        .into_iter()
        .filter_map(|n| Some((model.get(n.id)?.clone(), n)))
        .collect();
    out.extend(follow.into_iter().filter(|(o, _)| !keep.contains(&o.id)));
    out.retain(|(o, n)| o != n);
    out
}

/// The walls and columns standing on the grid line through `a` and `b`:
/// a wall whose centreline lies along it, a column centred on it.
pub fn standing_on(model: &Model, a: DVec2, b: DVec2) -> Vec<ElementId> {
    let d = (b - a).normalize_or_zero();
    if d == DVec2::ZERO {
        return vec![];
    }
    let off = |p: DVec2| (p - a).perp_dot(d).abs();
    model
        .iter()
        .filter(|e| match &e.kind {
            ElementKind::Wall(w) => off(w.start) < ON_LINE && off(w.end) < ON_LINE,
            ElementKind::Column(c) => off(c.position) < ON_LINE,
            _ => false,
        })
        .map(|e| e.id)
        .collect()
}

/// The held dimensions in `model`: (dimension, a's host, b's host, the
/// spacing held).
pub fn held(model: &Model) -> Vec<(ElementId, ElementId, ElementId, f64)> {
    model
        .iter()
        .filter_map(|e| match &e.kind {
            ElementKind::Dimension(d) => Some((
                e.id,
                d.a.host?,
                d.b.host?,
                d.held.filter(|v| v.is_finite())?,
            )),
            _ => None,
        })
        .collect()
}

/// The held dimension between `a` and `b`, either way round, if there is
/// one.
pub fn holding(model: &Model, a: ElementId, b: ElementId) -> Option<ElementId> {
    held(model)
        .into_iter()
        .find(|(_, x, y, _)| (*x == a && *y == b) || (*x == b && *y == a))
        .map(|h| h.0)
}

/// What `tx` (already applied to `model`) changed.
fn touched(tx: &Transaction) -> HashSet<ElementId> {
    tx.ops
        .iter()
        .map(|o| match o {
            crate::Op::Insert(e) | crate::Op::Remove(e) => e.id,
            crate::Op::Replace { after, .. } => after.id,
        })
        .collect()
}

/// After `tx` has been applied to `model`: the moves that make every held
/// spacing true again, as rounds of transactions to apply in order. The
/// element of a pair that `tx` (or an earlier round) moved stays; the other
/// one goes. A pair both or neither of which moved is left alone.
pub fn settle(model: &Model, tx: &Transaction) -> Vec<Transaction> {
    let holds = held(model);
    if holds.is_empty() {
        return vec![];
    }
    let mut moved = touched(tx);
    // a held dimension that tx itself edited (a new value typed) counts as
    // its own: both its ends may then need to settle against it
    let edited: HashSet<ElementId> = moved.clone();
    let mut model = model.clone();
    let mut rounds = vec![];
    for _ in 0..8 {
        let mut round = Transaction::new(tx.label.clone());
        let mut this_round: HashMap<ElementId, ()> = HashMap::new();
        for &(dim, a, b, value) in &held(&model) {
            let (ma, mb) = (moved.contains(&a), moved.contains(&b));
            let (fixed, go) = match (ma, mb) {
                (true, false) => (a, b),
                (false, true) => (b, a),
                // a dimension whose value was just set, neither end moved:
                // b goes, as the one typed against
                (false, false) if edited.contains(&dim) => (a, b),
                _ => continue,
            };
            if this_round.contains_key(&go) {
                continue;
            }
            let Some((s, n)) = spacing(&model, fixed, go) else {
                continue;
            };
            let side = if s < 0.0 { -1.0 } else { 1.0 };
            let by = n * (side * value - s);
            if by.length() < 1e-7 {
                continue;
            }
            let mut keep = moved.clone();
            keep.remove(&go);
            for (o, n) in shift(&model, go, by, &keep) {
                this_round.insert(n.id, ());
                round = round.replace(o, n);
            }
        }
        if round.ops.is_empty() {
            break;
        }
        if round.apply(&mut model).is_err() {
            break;
        }
        moved.extend(this_round.keys().copied());
        rounds.push(round);
    }
    rounds
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Dimension, Document, HostPoint};

    fn grid(label: &str, x: f64) -> Element {
        Element::new(
            label,
            ElementKind::Grid(crate::Grid {
                label: label.into(),
                start: DVec2::new(x, -5.0),
                end: DVec2::new(x, 15.0),
                group: None,
            }),
        )
    }

    fn x_of(doc: &Document, id: ElementId) -> f64 {
        match &doc.model().get(id).unwrap().kind {
            ElementKind::Grid(g) => g.start.x,
            ElementKind::Wall(w) => w.start.x,
            _ => unreachable!(),
        }
    }

    #[test]
    fn a_held_spacing_holds_when_one_end_moves() {
        let mut doc = Document::new();
        let (a, b, c) = (grid("A", 0.0), grid("B", 3.4), grid("C", 7.0));
        let (ia, ib, ic) = (a.id, b.id, c.id);
        let level = Element::new(
            "Level 0",
            ElementKind::Level(crate::Level {
                elevation: 0.0,
                height: 2.7,
            }),
        );
        let plan = Element::new("Plan", ElementKind::View(crate::View::plan(level.id)));
        let view = plan.id;
        let mut dim = Dimension::linear(
            view,
            HostPoint {
                host: Some(ia),
                ..HostPoint::free(DVec2::ZERO)
            },
            HostPoint {
                host: Some(ib),
                ..HostPoint::free(DVec2::new(3.4, 0.0))
            },
            1.0,
        );
        dim.held = Some(3.4);
        let d = Element::new("Dimension", ElementKind::Dimension(dim));
        doc.commit(
            Transaction::new("set up")
                .insert(level)
                .insert(plan)
                .insert(a)
                .insert(b)
                .insert(c)
                .insert(d),
        )
        .unwrap();
        // grid A moves 0.5 to the right: B follows, C stays
        let before = doc.model().get(ia).unwrap().clone();
        let mut after = before.clone();
        if let ElementKind::Grid(g) = &mut after.kind {
            g.start.x += 0.5;
            g.end.x += 0.5;
        }
        doc.commit(Transaction::new("Move A").replace(before, after))
            .unwrap();
        assert!((x_of(&doc, ib) - 3.9).abs() < 1e-9);
        assert!((x_of(&doc, ic) - 7.0).abs() < 1e-9);
        // one undo puts both back
        doc.undo();
        assert!((x_of(&doc, ia)).abs() < 1e-9);
        assert!((x_of(&doc, ib) - 3.4).abs() < 1e-9);
    }

    #[test]
    fn a_grid_carries_the_walls_on_it() {
        let model_level = Element::new(
            "Level 0",
            ElementKind::Level(crate::Level {
                elevation: 0.0,
                height: 2.7,
            }),
        );
        let lid = model_level.id;
        let g = grid("B", 3.4);
        let gid = g.id;
        let on = Element::new(
            "Wall",
            ElementKind::Wall(crate::Wall::new(
                lid,
                DVec2::new(3.4, 0.0),
                DVec2::new(3.4, 6.0),
                0.2,
                2.7,
            )),
        );
        let off = Element::new(
            "Wall",
            ElementKind::Wall(crate::Wall::new(
                lid,
                DVec2::new(5.0, 0.0),
                DVec2::new(5.0, 6.0),
                0.2,
                2.7,
            )),
        );
        let (on_id, off_id) = (on.id, off.id);
        let mut doc = Document::new();
        doc.commit(
            Transaction::new("set up")
                .insert(model_level)
                .insert(g)
                .insert(on)
                .insert(off),
        )
        .unwrap();
        let moves = shift(doc.model(), gid, DVec2::new(0.2, 0.0), &HashSet::new());
        let ids: Vec<ElementId> = moves.iter().map(|(o, _)| o.id).collect();
        assert!(ids.contains(&gid) && ids.contains(&on_id) && !ids.contains(&off_id));
    }
}
