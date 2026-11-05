//! Offset: an element's line moved square to itself, or its outline grown
//! or shrunk, by a distance on the plane it is drawn on. The Offset command
//! makes a copy this way (or changes the element itself); it works in
//! plans on walls, beams, grid lines, footings, railings, room separators,
//! slabs, roofs, ceilings and shafts, and on any face for sketches, guides,
//! extrusions, voids and sweeps drawn on that face.
//!
//! A positive distance goes to the line's left, looking along it with the
//! plane's normal towards you, or out of an outline; a negative one to the
//! right, or in.

use crate::{Element, ElementKind, Model, Workplane};
use glam::{DVec2, DVec3};

/// `pts` moved `by` to their left (right when negative), corners mitred.
pub fn offset_path(pts: &[DVec2], by: f64) -> Vec<DVec2> {
    let n = pts.len();
    let dir = |i: usize| (pts[i + 1] - pts[i]).normalize_or_zero();
    (0..n)
        .map(|i| {
            let before = (i > 0).then(|| dir(i - 1).perp());
            let after = (i + 1 < n).then(|| dir(i).perp());
            let m = match (before, after) {
                (Some(a), Some(b)) => {
                    let m = (a + b).normalize_or_zero();
                    let c = m.dot(a);
                    // a sharp turn is mitred no further than 4 times out
                    if c.abs() > 0.25 {
                        m / c
                    } else {
                        a
                    }
                }
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => DVec2::ZERO,
            };
            pts[i] + m * by
        })
        .collect()
}

/// An outline grown out by `by` (in when negative), or `None` when it
/// would shrink to nothing or turn inside out.
fn grow(poly: &[DVec2], by: f64) -> Option<Vec<DVec2>> {
    let before = crate::polygon::signed_area2(poly);
    let out = crate::members::offset_polygon(poly, by);
    let after = crate::polygon::signed_area2(&out);
    (after.abs() > 1e-6 && after.signum() == before.signum()).then_some(out)
}

/// The line or outline an element is offset from, in model coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Reference {
    pub points: Vec<DVec3>,
    pub closed: bool,
}

/// What `e` is offset from, or `None` when it can't be offset.
pub fn reference(model: &Model, e: &Element) -> Option<Reference> {
    let z = |l: crate::ElementId| crate::members::elevation(model, l);
    let lift = |l: Option<crate::ElementId>| l.map_or(0.0, z);
    let open = |points: Vec<DVec3>| Reference {
        points,
        closed: false,
    };
    let closed = |pts: &[DVec2], h: f64| Reference {
        points: pts.iter().map(|p| p.extend(h)).collect(),
        closed: true,
    };
    Some(match &e.kind {
        ElementKind::Wall(w) => {
            let h = z(w.level) + w.base_offset;
            open(vec![w.start.extend(h), w.end.extend(h)])
        }
        ElementKind::Beam(b) => open(vec![b.start.extend(z(b.level)), b.end.extend(z(b.level))]),
        ElementKind::Grid(g) => open(vec![g.start.extend(0.0), g.end.extend(0.0)]),
        ElementKind::RoomSeparator(s) => open(vec![s.a.extend(z(s.level)), s.b.extend(z(s.level))]),
        ElementKind::Footing(f) if f.kind == crate::FootingKind::Strip && f.host.is_none() => {
            open(vec![f.start.extend(z(f.level)), f.end.extend(z(f.level))])
        }
        ElementKind::Guide(g) => {
            let h = DVec3::Z * lift(g.level);
            open(vec![g.a + h, g.b + h])
        }
        ElementKind::Railing(r) if r.stair.is_none() => {
            let h = DVec3::Z * z(r.level);
            open(r.path.iter().map(|p| *p + h).collect())
        }
        ElementKind::Slab(s) => closed(&s.boundary, s.top(z(s.level))),
        ElementKind::Roof(r) => closed(&r.boundary, z(r.level) + r.base_offset),
        ElementKind::Ceiling(c) => closed(&c.boundary, z(c.level) + c.height),
        ElementKind::Shaft(s) => closed(&s.boundary, z(s.level) + s.base_offset),
        ElementKind::Sketch(s) => {
            let h = DVec3::Z * lift(s.level);
            Reference {
                points: s.model_points().into_iter().map(|p| p + h).collect(),
                closed: s.closed || s.circle.is_some(),
            }
        }
        ElementKind::Part(p) => {
            let h = DVec3::Z * z(p.level);
            match &p.form {
                crate::parts::Form::Extrusion {
                    outline,
                    base,
                    height,
                    plane: Some(pl),
                    ..
                } => Reference {
                    points: outline
                        .iter()
                        .map(|q| crate::parts::on_plane(pl, *q, base + height) + h)
                        .collect(),
                    closed: true,
                },
                crate::parts::Form::Extrusion { outline, base, .. } => closed(outline, h.z + base),
                crate::parts::Form::Sweep {
                    path: crate::parts::Path::Points { points, closed, .. },
                    ..
                } => Reference {
                    points: points.iter().map(|q| *q + h).collect(),
                    closed: *closed,
                },
                crate::parts::Form::Sweep { .. } => return None,
            }
        }
        _ => return None,
    })
}

/// How far `p` is from `e`'s line or outline on the plane facing `normal`,
/// signed as [`offset`] takes it: to the line's left, or out of the
/// outline, is positive. `None` when `e` can't be offset.
pub fn side_distance(model: &Model, e: &Element, p: DVec3, normal: DVec3) -> Option<f64> {
    let r = reference(model, e)?;
    let first = *r.points.first()?;
    let plane = Workplane::facing(first, normal);
    let pts: Vec<DVec2> = r.points.iter().map(|q| plane.to_plane(*q)).collect();
    let q = plane.to_plane(p);
    let n = pts.len();
    let segs = if r.closed { n } else { n.saturating_sub(1) };
    let mut best: Option<(f64, f64)> = None;
    for i in 0..segs {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let ab = b - a;
        if ab.length_squared() < 1e-18 {
            continue;
        }
        let t = ((q - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
        let d = q.distance(a + ab * t);
        let side = ab.perp_dot(q - a).signum();
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, side));
        }
    }
    let (d, side) = best?;
    Some(if r.closed {
        if crate::polygon::contains(&pts, q) {
            -d
        } else {
            d
        }
    } else {
        d * side
    })
}

/// `e` offset by `by` on the plane facing `normal` (see the module notes
/// for which way), or `None` when it can't be offset there: plan elements
/// only in plan (a level plane), sketches and parts drawn on a face only
/// on that face, a slab with arcs not at all.
pub fn offset(e: &Element, by: f64, normal: DVec3) -> Option<Element> {
    let n = normal.try_normalize()?;
    // in plan, the left of a line seen from above (from below it swaps)
    let plan = (n.z.abs() > 0.999).then_some(n.z.signum());
    let mut out = e.clone();
    let line = |a: &mut DVec2, b: &mut DVec2| -> Option<()> {
        let s = plan?;
        let left = (*b - *a).try_normalize()?.perp() * s * by;
        *a += left;
        *b += left;
        Some(())
    };
    // on a plane of its own: the same plane, and which way its left is
    let facing = |own: DVec3| -> Option<f64> {
        let d = own.normalize_or_zero().dot(n);
        (d.abs() > 0.999).then_some(d.signum())
    };
    match &mut out.kind {
        ElementKind::Wall(w) => line(&mut w.start, &mut w.end)?,
        ElementKind::Beam(b) => line(&mut b.start, &mut b.end)?,
        ElementKind::Grid(g) => line(&mut g.start, &mut g.end)?,
        ElementKind::RoomSeparator(s) => line(&mut s.a, &mut s.b)?,
        ElementKind::Footing(f) if f.kind == crate::FootingKind::Strip && f.host.is_none() => {
            line(&mut f.start, &mut f.end)?
        }
        ElementKind::Guide(g) => {
            let d = (g.b - g.a).try_normalize()?;
            if d.dot(n).abs() > 1e-3 {
                return None;
            }
            let left = n.cross(d).normalize() * by;
            g.a += left;
            g.b += left;
        }
        ElementKind::Railing(r) if r.stair.is_none() => {
            let s = plan?;
            let flat: Vec<DVec2> = r.path.iter().map(|p| p.truncate()).collect();
            let moved = offset_path(&flat, by * s);
            for (p, m) in r.path.iter_mut().zip(moved) {
                *p = m.extend(p.z);
            }
        }
        ElementKind::Slab(s) => {
            plan?;
            if s.bulges.iter().any(|b| b.abs() > 1e-12) {
                return None;
            }
            s.boundary = grow(&s.boundary, by)?;
        }
        ElementKind::Roof(r) => {
            plan?;
            r.boundary = grow(&r.boundary, by)?;
        }
        ElementKind::Ceiling(c) => {
            plan?;
            c.boundary = grow(&c.boundary, by)?;
        }
        ElementKind::Shaft(s) => {
            plan?;
            s.boundary = grow(&s.boundary, by)?;
        }
        ElementKind::Sketch(s) => {
            let side = facing(s.plane.normal())?;
            if let Some(c) = &mut s.circle {
                c.radius += by;
                if c.radius <= 1e-6 {
                    return None;
                }
            } else {
                if s.kinds.iter().any(|k| *k != crate::SegmentKind::Line) {
                    return None;
                }
                s.points = if s.closed {
                    grow(&s.points, by)?
                } else {
                    offset_path(&s.points, by * side)
                };
            }
            // its dimensions and relations were for the line it came from
            s.constraints.clear();
        }
        ElementKind::Part(p) => match &mut p.form {
            crate::parts::Form::Extrusion {
                outline,
                plane: Some(pl),
                ..
            } => {
                facing(pl.normal())?;
                *outline = grow(outline, by)?;
            }
            crate::parts::Form::Extrusion { outline, .. } => {
                plan?;
                *outline = grow(outline, by)?;
            }
            crate::parts::Form::Sweep {
                path:
                    crate::parts::Path::Points {
                        points,
                        closed,
                        normal,
                    },
                ..
            } => {
                let own = normal.unwrap_or(DVec3::Z);
                let side = facing(own)?;
                let first = *points.first()?;
                let pl = Workplane::facing(first, own);
                let flat: Vec<DVec2> = points.iter().map(|q| pl.to_plane(*q)).collect();
                let moved = if *closed {
                    grow(&flat, by)?
                } else {
                    offset_path(&flat, by * side)
                };
                for (q, m) in points.iter_mut().zip(moved) {
                    // keep how far each point stood off the plane
                    let h = (*q - first).dot(pl.normal());
                    *q = pl.to_model(m) + pl.normal() * h;
                }
            }
            crate::parts::Form::Sweep { .. } => return None,
        },
        _ => return None,
    }
    (out != *e).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Level, Slab, Wall};

    fn level() -> Element {
        Element::new(
            "L0",
            ElementKind::Level(Level {
                elevation: 3.0,
                height: 3.0,
            }),
        )
    }

    #[test]
    fn a_wall_goes_to_the_side_the_pointer_is_on() {
        let lv = level();
        let w = Element::new(
            "W",
            ElementKind::Wall(Wall::new(
                lv.id,
                DVec2::ZERO,
                DVec2::new(4.0, 0.0),
                0.2,
                2.0,
            )),
        );
        let m = Model::from_elements([lv, w.clone()]).unwrap();
        // the pointer 1.5 m to the wall's left (+y), seen from above
        let p = DVec3::new(2.0, 1.5, 0.0);
        let d = side_distance(&m, &w, p, DVec3::Z).unwrap();
        assert!((d - 1.5).abs() < 1e-9);
        let ElementKind::Wall(o) = offset(&w, d, DVec3::Z).unwrap().kind else {
            panic!()
        };
        assert!((o.start - DVec2::new(0.0, 1.5)).length() < 1e-9);
        assert!((o.end - DVec2::new(4.0, 1.5)).length() < 1e-9);
        // a wall offsets only in plan
        assert!(offset(&w, 1.0, DVec3::X).is_none());
    }

    #[test]
    fn a_slab_grows_out_and_shrinks_in_but_not_to_nothing() {
        let lv = level();
        let sq = crate::polygon::rectangle(DVec2::ZERO, DVec2::new(4.0, 2.0));
        let s = Element::new("S", ElementKind::Slab(Slab::new(lv.id, sq, 0.2)));
        let m = Model::from_elements([lv, s.clone()]).unwrap();
        // inside, half a metre in from the long edge
        let d = side_distance(&m, &s, DVec3::new(2.0, 0.5, 3.0), DVec3::Z).unwrap();
        assert!((d + 0.5).abs() < 1e-9);
        let ElementKind::Slab(o) = offset(&s, 0.5, DVec3::Z).unwrap().kind else {
            panic!()
        };
        let area = crate::polygon::signed_area2(&o.boundary) / 2.0;
        assert!((area - 5.0 * 3.0).abs() < 1e-9);
        assert!(offset(&s, -1.5, DVec3::Z).is_none());
    }

    #[test]
    fn a_part_on_a_wall_face_offsets_on_that_face() {
        let lv = level();
        let l = lv.id;
        let sq = crate::polygon::rectangle(DVec2::ZERO, DVec2::ONE);
        let mut p = crate::parts::Part::extrusion(l, sq, 0.0, 0.1);
        let face = Workplane::facing(DVec3::new(0.0, -0.1, 0.5), -DVec3::Y);
        if let crate::parts::Form::Extrusion { plane, .. } = &mut p.form {
            *plane = Some(face);
        }
        let e = Element::new("Panel", ElementKind::Part(p));
        let m = Model::from_elements([lv, e.clone()]).unwrap();
        // in plan it is not on the plane it is drawn on
        assert!(offset(&e, 0.1, DVec3::Z).is_none());
        let o = offset(&e, 0.1, -DVec3::Y).unwrap();
        let r = reference(&m, &o).unwrap();
        let ys: Vec<f64> = r.points.iter().map(|q| q.y).collect();
        // grown on the face, not off it: 1.2 m square
        assert!(ys.iter().all(|y| (y - (-0.2)).abs() < 1e-9), "{ys:?}");
        let xs = r.points.iter().map(|q| q.x);
        let (lo, hi) = xs.fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(x), b.max(x)));
        assert!((hi - lo - 1.2).abs() < 1e-9);
    }
}
