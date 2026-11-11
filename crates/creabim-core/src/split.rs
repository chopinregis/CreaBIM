//! Split (MU39): cut the selected elements with an upright plane through a
//! line in plan, as a saw would. Walls are cut where their line crosses
//! it; slabs, plan-drawn parts and closed sketches into the outlines on
//! each side; an open sketch at every crossing; a mass by the plane
//! itself. Every piece is an element of its own, and the whole split is
//! one undo step. The line runs on past its two points: what it cuts is
//! the selection, not what the line happens to reach.

use crate::{Element, ElementId, ElementKind, Model, Transaction};
use glam::{DVec2, DVec3};

/// A cutting line in plan: through `a`, along `b - a`.
#[derive(Clone, Copy, Debug)]
pub struct Line {
    pub a: DVec2,
    pub b: DVec2,
}

impl Line {
    fn dir(&self) -> Option<DVec2> {
        (self.b - self.a).try_normalize()
    }

    /// Which side of the line `p` lies: positive to its left.
    pub fn side(&self, p: DVec2) -> f64 {
        let d = self.dir().unwrap_or(DVec2::X);
        d.perp_dot(p - self.a)
    }
}

/// What a split did: the transaction, and the elements it couldn't cut
/// (each with why).
pub struct Split {
    pub tx: Transaction,
    pub cut: usize,
    pub skipped: Vec<String>,
}

/// Split `ids` with `line`.
pub fn split(model: &Model, ids: &[ElementId], line: Line) -> Result<Split, String> {
    let d = line
        .dir()
        .ok_or("The cutting line needs two points apart.")?;
    let mut tx = Transaction::new("Split");
    let (mut cut, mut skipped) = (0, vec![]);
    for id in ids {
        let Some(e) = model.get(*id) else { continue };
        match split_one(model, e, line, d) {
            Ok(Some(ops)) => {
                tx.ops.extend(ops.ops);
                cut += 1;
            }
            Ok(None) => skipped.push(format!("{}: the line doesn't cross it", e.name)),
            Err(why) => skipped.push(format!("{}: {why}", e.name)),
        }
    }
    if cut == 0 {
        return Err(match skipped.first() {
            Some(s) => format!("Nothing split. {s}."),
            None => "Select what to split first.".into(),
        });
    }
    tx.label = if cut == 1 {
        "Split".into()
    } else {
        format!("Split {cut} elements")
    };
    Ok(Split { tx, cut, skipped })
}

/// The pieces as a transaction: the first piece replaces `e`, the rest are
/// new elements, numbered on from its name.
fn pieces(e: &Element, kinds: Vec<ElementKind>) -> Option<Transaction> {
    if kinds.len() < 2 {
        return None;
    }
    let mut tx = Transaction::new("Split");
    for (k, kind) in kinds.into_iter().enumerate() {
        let mut now = e.clone();
        now.kind = kind;
        if k == 0 {
            tx = tx.replace(e.clone(), now);
        } else {
            now.id = ElementId::new();
            now.name = format!("{} {}", e.name, k + 1);
            tx = tx.insert(now);
        }
    }
    Some(tx)
}

fn split_one(
    model: &Model,
    e: &Element,
    line: Line,
    d: DVec2,
) -> Result<Option<Transaction>, String> {
    // signed distance to the cut, and where along it, in plan
    let dist = |p: DVec2| d.perp_dot(p - line.a);
    let along = |p: DVec2| d.dot(p);
    match &e.kind {
        ElementKind::Wall(_) => {
            let s = crate::divide::span(model, e.id).ok_or("only a straight wall can be split")?;
            let den = s.u.perp_dot(d);
            if den.abs() < 1e-9 {
                return Ok(None);
            }
            let t = (line.a - s.o).perp_dot(d) / den;
            if t <= 1e-3 || t >= s.len - 1e-3 {
                return Ok(None);
            }
            let parts = [(0.0, t), (t, s.len - t)];
            crate::divide::divide_grid(
                model,
                e.id,
                Some(&parts),
                None,
                crate::divide::Make::Cuts,
                0.0,
                None,
            )
            .map(Some)
        }
        ElementKind::Slab(sl) => {
            if sl.bulges.iter().any(|b| b.abs() > 1e-9) {
                return Err("a slab with curved edges can't be split yet".into());
            }
            let rings = split_ring(&sl.boundary, &dist, &along);
            let kinds = rings
                .into_iter()
                .map(|r| {
                    let mut s = sl.clone();
                    s.holes = sl
                        .holes
                        .iter()
                        .filter(|h| h.first().is_some_and(|p| crate::polygon::contains(&r, *p)))
                        .cloned()
                        .collect();
                    s.boundary = r;
                    s.bulges.clear();
                    ElementKind::Slab(s)
                })
                .collect();
            Ok(pieces(e, kinds))
        }
        ElementKind::Part(p) => {
            let crate::parts::Form::Extrusion {
                outline,
                holes,
                plane: None,
                ..
            } = &p.form
            else {
                return Err("only a part pulled up from a plan outline can be split".into());
            };
            let rings = split_ring(outline, &dist, &along);
            let kinds = rings
                .into_iter()
                .map(|r| {
                    let mut q = p.clone();
                    if let crate::parts::Form::Extrusion {
                        outline: o,
                        holes: hs,
                        ..
                    } = &mut q.form
                    {
                        *hs = holes
                            .iter()
                            .filter(|h| h.first().is_some_and(|p| crate::polygon::contains(&r, *p)))
                            .cloned()
                            .collect();
                        *o = r;
                    }
                    ElementKind::Part(q)
                })
                .collect();
            Ok(pieces(e, kinds))
        }
        ElementKind::Sketch(sk) => {
            if sk.circle.is_some()
                || sk
                    .kinds
                    .iter()
                    .any(|k| !matches!(k, crate::sketch::SegmentKind::Line))
            {
                return Err("a sketch with arcs can't be split yet".into());
            }
            // in the sketch's own plane, measured in plan
            let flat = |p: DVec2| sk.plane.to_model(p).truncate();
            let dist = |p: DVec2| dist(flat(p));
            let along = |p: DVec2| along(flat(p));
            let rings = if sk.closed {
                split_ring(&sk.points, &dist, &along)
            } else {
                split_path(&sk.points, &dist)
            };
            let kinds = rings
                .into_iter()
                .map(|r| {
                    let mut s = sk.clone();
                    s.points = r;
                    s.kinds.clear();
                    // its dimensions and relations were for the whole
                    s.constraints.clear();
                    ElementKind::Sketch(s)
                })
                .collect();
            Ok(pieces(e, kinds))
        }
        ElementKind::FreeMesh(m) => {
            let z = m
                .level
                .and_then(|l| model.level(l))
                .map_or(0.0, |l| l.elevation);
            let base = m.origin + DVec3::Z * z;
            let world: Vec<DVec3> = m
                .mesh
                .vertices
                .iter()
                .map(|v| base + m.to_model(*v))
                .collect();
            let sides: Vec<f64> = world.iter().map(|w| dist(w.truncate())).collect();
            if !(sides.iter().any(|s| *s > 1e-6) && sides.iter().any(|s| *s < -1e-6)) {
                return Ok(None);
            }
            let (lo, hi) = world
                .iter()
                .fold((world[0], world[0]), |(a, b), p| (a.min(*p), b.max(*p)));
            let r = (hi - lo).length() + 1.0;
            let mid = (lo + hi) / 2.0;
            let on = line.a + d * d.dot(mid.truncate() - line.a);
            let n = d.perp();
            let polys = crate::csg::from_mesh(&m.mesh, DVec3::ZERO);
            let mut kinds = vec![];
            for sign in [1.0, -1.0] {
                // a box on one side of the plane, in the mesh's own axes
                let mut bx = crate::modifiers::Evaluated::default();
                let centre = (on + n * (sign * r / 2.0)).extend(mid.z);
                crate::modgen::push_box(
                    &mut bx,
                    centre,
                    d.extend(0.0),
                    n.extend(0.0),
                    DVec3::new(r, r / 2.0, r),
                );
                for v in &mut bx.mesh.vertices {
                    *v = m.to_local(*v - base);
                }
                if m.scale < 0.0 {
                    for f in &mut bx.mesh.faces {
                        f.reverse();
                    }
                }
                let cut = crate::csg::from_mesh(&bx.mesh, DVec3::ZERO);
                let half =
                    crate::csg::boolean(polys.clone(), cut, crate::csg::BoolOp::Intersection);
                let mesh = crate::csg::to_mesh(&half, DVec3::ZERO);
                if !mesh.faces.is_empty() {
                    let mut f = m.clone();
                    f.mesh = mesh;
                    kinds.push(ElementKind::FreeMesh(f));
                }
            }
            Ok(pieces(e, kinds))
        }
        _ => Err("this kind can't be split yet".into()),
    }
}

/// A closed outline cut by a line into the outlines on each side: where
/// the outline crosses the line, sorted along it, pair up as the bridges
/// each piece runs across; a U cut across both arms gives three pieces.
/// Unchanged (one ring) when the line misses it.
pub fn split_ring(
    pts: &[DVec2],
    dist: &dyn Fn(DVec2) -> f64,
    along: &dyn Fn(DVec2) -> f64,
) -> Vec<Vec<DVec2>> {
    let n = pts.len();
    if n < 3 {
        return vec![pts.to_vec()];
    }
    // a corner on the line counts as on its left, so crossings pair up
    let side: Vec<f64> = pts
        .iter()
        .map(|p| {
            let s = dist(*p);
            if s.abs() < 1e-9 {
                1e-9
            } else {
                s
            }
        })
        .collect();
    let mut ring: Vec<(DVec2, bool)> = vec![];
    let mut crossings: Vec<(f64, usize)> = vec![];
    for i in 0..n {
        let j = (i + 1) % n;
        ring.push((pts[i], false));
        if (side[i] > 0.0) != (side[j] > 0.0) {
            let t = side[i] / (side[i] - side[j]);
            let p = pts[i] + (pts[j] - pts[i]) * t;
            crossings.push((along(p), ring.len()));
            ring.push((p, true));
        }
    }
    if crossings.len() < 2 || crossings.len() % 2 == 1 {
        return vec![pts.to_vec()];
    }
    crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
    let m = ring.len();
    let mut partner = vec![usize::MAX; m];
    for pair in crossings.chunks(2) {
        partner[pair[0].1] = pair[1].1;
        partner[pair[1].1] = pair[0].1;
    }
    let mut seen = vec![false; m];
    let mut out = vec![];
    for start in 0..m {
        if seen[start] || ring[start].1 {
            continue;
        }
        let mut piece = vec![];
        let mut cur = start;
        for _ in 0..2 * m + 2 {
            seen[cur] = true;
            piece.push(ring[cur].0);
            let next = if ring[cur].1 {
                let j = partner[cur];
                seen[j] = true;
                piece.push(ring[j].0);
                (j + 1) % m
            } else {
                (cur + 1) % m
            };
            if next == start {
                break;
            }
            cur = next;
        }
        piece.dedup_by(|a, b| a.distance(*b) < 1e-9);
        if piece.len() >= 3 && crate::polygon::signed_area2(&piece).abs() > 1e-9 {
            out.push(piece);
        }
    }
    if out.is_empty() {
        vec![pts.to_vec()]
    } else {
        out
    }
}

/// An open line cut at every place it crosses.
pub fn split_path(pts: &[DVec2], dist: &dyn Fn(DVec2) -> f64) -> Vec<Vec<DVec2>> {
    let mut out = vec![];
    let Some(first) = pts.first() else {
        return out;
    };
    let mut cur = vec![*first];
    for w in pts.windows(2) {
        let (s0, s1) = (dist(w[0]), dist(w[1]));
        if (s0 > 1e-9 && s1 < -1e-9) || (s0 < -1e-9 && s1 > 1e-9) {
            let p = w[0] + (w[1] - w[0]) * (s0 / (s0 - s1));
            cur.push(p);
            out.push(std::mem::replace(&mut cur, vec![p]));
        }
        cur.push(w[1]);
    }
    out.push(cur);
    out.retain(|p| p.len() >= 2);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d_x(x: f64) -> impl Fn(DVec2) -> f64 {
        move |p: DVec2| p.x - x
    }

    #[test]
    fn a_square_cut_down_the_middle_is_two_halves() {
        let sq = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(4.0, 0.0),
            DVec2::new(4.0, 2.0),
            DVec2::new(0.0, 2.0),
        ];
        let rings = split_ring(&sq, &d_x(1.0), &|p| p.y);
        assert_eq!(rings.len(), 2);
        let mut areas: Vec<f64> = rings
            .iter()
            .map(|r| (crate::polygon::signed_area2(r) / 2.0).abs())
            .collect();
        areas.sort_by(f64::total_cmp);
        assert!((areas[0] - 2.0).abs() < 1e-9 && (areas[1] - 6.0).abs() < 1e-9);
    }

    #[test]
    fn a_u_cut_across_both_arms_is_three_pieces() {
        // a U open at the top: arms 0..1 and 3..4, base below y = 1
        let u = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(4.0, 0.0),
            DVec2::new(4.0, 3.0),
            DVec2::new(3.0, 3.0),
            DVec2::new(3.0, 1.0),
            DVec2::new(1.0, 1.0),
            DVec2::new(1.0, 3.0),
            DVec2::new(0.0, 3.0),
        ];
        // a level cut through both arms at y = 2
        let rings = split_ring(&u, &|p| p.y - 2.0, &|p| p.x);
        assert_eq!(rings.len(), 3);
        let total: f64 = rings
            .iter()
            .map(|r| (crate::polygon::signed_area2(r) / 2.0).abs())
            .sum();
        assert!((total - 8.0).abs() < 1e-9);
    }

    #[test]
    fn an_open_line_is_cut_at_each_crossing() {
        let zig = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(2.0, 1.0),
            DVec2::new(0.0, 2.0),
            DVec2::new(2.0, 3.0),
        ];
        assert_eq!(split_path(&zig, &d_x(1.0)).len(), 4);
        assert_eq!(split_path(&zig, &d_x(5.0)).len(), 1);
    }

    #[test]
    fn a_wall_is_cut_where_the_line_crosses_it() {
        let l = Element::new(
            "L0",
            ElementKind::Level(crate::Level {
                elevation: 0.0,
                height: 3.0,
            }),
        );
        let w = crate::Wall::new(l.id, DVec2::ZERO, DVec2::new(6.0, 0.0), 0.2, 3.0);
        let e = Element::new("Wall", ElementKind::Wall(w));
        let id = e.id;
        let mut doc = crate::Document::new();
        doc.commit(Transaction::new("w").insert(l).insert(e))
            .unwrap();
        let line = Line {
            a: DVec2::new(2.0, -1.0),
            b: DVec2::new(2.0, 1.0),
        };
        let s = split(doc.model(), &[id], line).unwrap();
        doc.commit(s.tx).unwrap();
        let mut lens: Vec<f64> = doc
            .model()
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::Wall(w) => Some((w.length() * 1000.0).round() / 1000.0),
                _ => None,
            })
            .collect();
        lens.sort_by(f64::total_cmp);
        assert_eq!(lens, vec![2.0, 4.0]);
    }

    #[test]
    fn a_mass_is_cut_by_the_plane_into_two_masses() {
        let m = crate::FreeMesh::new(
            None,
            DVec3::ZERO,
            crate::EditMesh::cuboid(DVec3::new(4.0, 2.0, 3.0)),
        );
        let e = Element::new("Mass", ElementKind::FreeMesh(m));
        let id = e.id;
        let mut doc = crate::Document::new();
        doc.commit(Transaction::new("m").insert(e)).unwrap();
        let line = Line {
            a: DVec2::new(1.0, -5.0),
            b: DVec2::new(1.0, 5.0),
        };
        let s = split(doc.model(), &[id], line).unwrap();
        doc.commit(s.tx).unwrap();
        let mut vols: Vec<f64> = doc
            .model()
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::FreeMesh(m) => Some((m.mesh.volume().abs() * 1000.0).round() / 1000.0),
                _ => None,
            })
            .collect();
        vols.sort_by(f64::total_cmp);
        assert_eq!(vols, vec![6.0, 18.0]);
    }
}
