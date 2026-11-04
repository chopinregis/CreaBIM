//! Plan view ranges: a plan that draws the elements of every level within
//! its range (a site level under the floor, roofs over it), and a plan
//! that looks down from above everything with nothing cut, upper roofs
//! hiding what is under them: a roof or site plan.

use crate::plan::{Pen, PlanItem, PlanView};
use creabim_core::graphics::{PlanGraphics, PlanRange};
use creabim_core::{ElementId, ElementKind, Model, View};
use glam::{DVec2, DVec3, Vec3};
use std::collections::HashSet;

/// The level an element stands on, for drawing it in a range: a door or
/// window takes its wall's.
fn element_level(model: &Model, id: ElementId) -> Option<ElementId> {
    let e = model.get(id)?;
    match &e.kind {
        ElementKind::Opening(o) => model
            .get(o.host)
            .and_then(|h| creabim_core::levels::level_of(&h.kind)),
        ElementKind::View(_) | ElementKind::Grid(_) | ElementKind::GridGroup(_) => None,
        k => creabim_core::levels::level_of(k),
    }
}

/// Where a level falls in a plan's range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Band {
    /// Between the bottom and the cut: cut, or seen below it.
    Below,
    /// Between the view depth and the bottom: lighter.
    Beyond,
    /// Above the cut, below the top: dashed.
    Overhead,
}

/// The other levels a plan of `level` cut `cut` above it draws, with
/// their elevations and bands.
fn bands(model: &Model, level: ElementId, cut: f64, r: &PlanRange) -> Vec<(ElementId, f64, Band)> {
    let own = model.level(level).map_or(0.0, |l| l.elevation);
    let cut_z = own + cut;
    let bottom = r.bottom.map_or(own, |b| b.z(model, own));
    let top = r.top.map_or_else(
        || {
            model
                .levels()
                .map(|(_, l)| l.elevation)
                .filter(|z| *z > own + 1e-6)
                .fold(f64::INFINITY, f64::min)
        },
        |t| t.z(model, own),
    );
    let beyond = r.beyond.map(|b| b.z(model, own));
    let eps = 1e-6;
    let mut out = Vec::new();
    for (e, l) in model.levels() {
        if e.id == level {
            continue;
        }
        let z = l.elevation;
        let band = if z < cut_z - eps {
            if z >= bottom - eps {
                Band::Below
            } else if beyond.is_some_and(|b| z >= b - eps) {
                Band::Beyond
            } else {
                continue;
            }
        } else if r.overhead && z < top - eps {
            Band::Overhead
        } else {
            continue;
        };
        out.push((e.id, z, band));
    }
    out
}

/// The elements a plan draws in its *beyond* style: those of the levels
/// between its view depth and its bottom.
pub fn beyond(model: &Model, v: &View) -> HashSet<ElementId> {
    let creabim_core::ViewKind::Plan { level, cut_height } = v.kind else {
        return HashSet::new();
    };
    let r = &v.graphics.range;
    if !r.spans_levels() || r.from_above || r.beyond.is_none() {
        return HashSet::new();
    }
    let far: HashSet<ElementId> = bands(model, level, cut_height, r)
        .into_iter()
        .filter(|b| b.2 == Band::Beyond)
        .map(|b| b.0)
        .collect();
    model
        .iter()
        .filter(|e| element_level(model, e.id).is_some_and(|l| far.contains(&l)))
        .map(|e| e.id)
        .collect()
}

/// The drawing of plan view `v` of `level`: its own level as always,
/// with the other levels its range reaches, or the view from above.
pub fn plan(model: &Model, v: &View, level: ElementId, cut: f64) -> PlanView {
    let r = &v.graphics.range;
    if r.from_above {
        return from_above(model, v, level, cut);
    }
    let mut d = crate::plan::plan_view_with(model, level, cut, &v.graphics);
    if !r.spans_levels() {
        return d;
    }
    let own = model.level(level).map_or(0.0, |l| l.elevation);
    // the other levels draw as plans of their own, without their range
    let g = PlanGraphics {
        range: PlanRange {
            view_depth: r.view_depth,
            ..Default::default()
        },
        ..v.graphics.clone()
    };
    let mut under = Vec::new();
    let mut over = Vec::new();
    for (l, z, band) in bands(model, level, cut, r) {
        let theirs = |it: &PlanItem| element_level(model, it.id()) == Some(l);
        match band {
            Band::Below | Band::Beyond => {
                let sub = crate::plan::plan_view_with(model, l, own + cut - z, &g);
                under.extend(sub.items.into_iter().filter(|it| theirs(it)));
            }
            Band::Overhead => {
                // just above its floor, then every line dashed
                let sub = crate::plan::plan_view_with(model, l, 0.001, &g);
                for it in sub.items.into_iter().filter(|it| theirs(it)) {
                    if let Some(it) = overhead(it) {
                        over.push(it);
                    }
                }
            }
        }
    }
    under.append(&mut d.items);
    under.append(&mut over);
    PlanView { items: under }
}

/// An item of a level above the cut, as an overhead plan draws it:
/// its lines dashed, its fills and labels left out.
fn overhead(it: PlanItem) -> Option<PlanItem> {
    Some(match it {
        PlanItem::Line { id, a, b, .. } => PlanItem::Line {
            id,
            pen: Pen::Hidden,
            a,
            b,
        },
        PlanItem::Polygon { id, points, .. } => PlanItem::Polygon {
            id,
            pen: Pen::Hidden,
            points,
        },
        PlanItem::Arc {
            id,
            center,
            radius,
            start,
            end,
            ..
        } => PlanItem::Arc {
            id,
            pen: Pen::Hidden,
            center,
            radius,
            start,
            end,
        },
        _ => return None,
    })
}

/// The heights a view from above spans: from the top of its range (else
/// above everything) down to its bottom (else under everything).
pub fn from_above_span(model: &Model, v: &View, level: ElementId, _cut: f64) -> (f64, f64) {
    let r = &v.graphics.range;
    let own = model.level(level).map_or(0.0, |l| l.elevation);
    let (lo, hi) = crate::scene::drawn_scene(model)
        .iter()
        .filter_map(|s| s.mesh.bounds())
        .fold((f64::MAX, f64::MIN), |(lo, hi), (a, b)| {
            (lo.min(a.z as f64), hi.max(b.z as f64))
        });
    let (lo, hi) = if lo > hi { (own, own + 3.0) } else { (lo, hi) };
    let top = r.top.map_or(hi + 1.0, |t| t.z(model, own));
    let bottom = r.bottom.map_or(lo - 1.0, |b| b.z(model, own));
    (top, bottom.min(top - 0.01))
}

/// A plan looking straight down from the top of its range: nothing cut,
/// each surface hiding what is under it; the roofs' slope arrows, the
/// grids and section marks over it, and its cues' sun shadows under it.
fn from_above(model: &Model, v: &View, level: ElementId, cut: f64) -> PlanView {
    let (top, bottom) = from_above_span(model, v, level, cut);
    let scene = crate::scene::drawn_scene(model);
    let project = |p: DVec3| DVec3::new(p.x, p.y, top - p.z);
    let (lines, image) =
        crate::section::hidden_lines(&scene, &project, top - bottom, None, v.style, None);
    let mut items = Vec::new();
    // shadows on the ground, from every face, each point thrown by its height
    let tris = |s: &crate::SceneItem| -> Vec<[DVec3; 3]> {
        s.mesh
            .indices
            .chunks(3)
            .map(|t| {
                let p = |k: usize| Vec3::from(s.mesh.positions[t[k] as usize]).as_dvec3();
                [p(0), p(1), p(2)]
            })
            .filter(|t| t.iter().all(|p| p.z <= top - 1e-4 && p.z >= bottom - 1e-6))
            .collect()
    };
    if let Some(sh) = v.look.cues.shadows {
        let dir = DVec2::from_angle(sh.direction.to_radians()) * sh.length;
        let grey = 1.0 - sh.opacity.clamp(0.0, 1.0);
        for s in &scene {
            for t in tris(s) {
                let pts: Vec<DVec2> = t
                    .iter()
                    .map(|p| p.truncate() + dir * (p.z - bottom).max(0.0))
                    .collect();
                if (pts[1] - pts[0]).perp_dot(pts[2] - pts[0]).abs() > 1e-8 {
                    items.push(PlanItem::Shade {
                        id: ElementId::NIL,
                        points: pts,
                        rgb: [grey; 3],
                    });
                }
            }
        }
    }
    // the surfaces seen from above, lowest first, so higher ones paint
    // over them: the fill a style gives them, white without one
    let mut faces: Vec<(f64, ElementId, Vec<DVec2>)> = Vec::new();
    for s in &scene {
        // a roof without holes as its faces, not their triangles, so its
        // pattern runs on without seams
        if let Some(ElementKind::Roof(r)) = model.get(s.id).map(|e| &e.kind) {
            let base = r.base(model) + r.vertical_thickness();
            let top_z = |f: &creabim_core::members::RoofFace| {
                f.outline
                    .iter()
                    .map(|p| base + f.z(*p))
                    .fold(f64::MIN, f64::max)
            };
            let fs = r.faces();
            if r.holes.is_empty() && !fs.is_empty() && fs.iter().all(|f| top_z(f) < top) {
                if s.layer.is_none_or(|l| l.index == 0) {
                    for f in fs {
                        faces.push((top_z(&f), s.id, f.outline));
                    }
                }
                continue;
            }
        }
        for t in tris(s) {
            let n = (t[1] - t[0]).cross(t[2] - t[0]);
            if n.z.abs() < 0.2 * n.length() || n.length() < 1e-9 {
                continue;
            }
            let z = t.iter().map(|p| p.z).fold(f64::MIN, f64::max);
            faces.push((z, s.id, t.iter().map(|p| p.truncate()).collect()));
        }
    }
    faces.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, id, points) in faces {
        items.push(PlanItem::Shade {
            id,
            points,
            rgb: [1.0; 3],
        });
    }
    items.extend(image);
    items.extend(lines);
    // which way each roof falls
    for e in model.iter() {
        if !matches!(e.kind, ElementKind::Roof(_)) {
            continue;
        }
        for it in crate::members::member_plan(model, e, 1e4) {
            match it {
                PlanItem::Line {
                    pen: Pen::Symbol, ..
                }
                | PlanItem::Label { .. } => items.push(it),
                _ => {}
            }
        }
    }
    // grids and section marks, from the plan of its level
    for it in crate::plan::plan_view_with(model, level, cut, &v.graphics).items {
        let keep = model
            .get(it.id())
            .is_some_and(|e| matches!(e.kind, ElementKind::Grid(_) | ElementKind::View(_)));
        if keep {
            items.push(it);
        }
    }
    PlanView { items }
}

#[cfg(test)]
mod tests {
    use super::*;
    use creabim_core::graphics::RangeLimit;
    use creabim_core::{Element, Level, Slab, Transaction, ViewKind, Wall};

    /// A site level at -0.45, a floor at 0, roofs at +4.6 and +6.35, each
    /// with an element of its own; a plan of the floor.
    fn levels() -> (Model, [ElementId; 4], [ElementId; 4]) {
        let mut m = Model::new();
        let mut t = Transaction::new("t");
        let mut lv = [ElementId::NIL; 4];
        for (i, (name, z)) in [
            ("Site", -0.45),
            ("Floor", 0.0),
            ("Roof 1", 4.6),
            ("Roof 2", 6.35),
        ]
        .iter()
        .enumerate()
        {
            let l = Element::new(
                *name,
                ElementKind::Level(Level {
                    elevation: *z,
                    height: 3.0,
                }),
            );
            lv[i] = l.id;
            t = t.insert(l);
        }
        // a boundary wall on the site, a house wall on the floor, a roof
        // slab over the house and a higher one over part of it
        let site = Element::new(
            "Boundary wall",
            ElementKind::Wall(Wall::new(
                lv[0],
                DVec2::new(-5.0, -5.0),
                DVec2::new(15.0, -5.0),
                0.2,
                1.8,
            )),
        );
        let wall = Element::new(
            "House wall",
            ElementKind::Wall(Wall::new(
                lv[1],
                DVec2::ZERO,
                DVec2::new(10.0, 0.0),
                0.3,
                4.6,
            )),
        );
        let slab = |l: ElementId, lo: DVec2, hi: DVec2| {
            let mut s = Slab::new(
                l,
                vec![lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)],
                0.3,
            );
            s.top_offset = 0.3;
            s
        };
        let r1 = Element::new(
            "Roof 1",
            ElementKind::Slab(slab(lv[2], DVec2::new(-1.0, -1.0), DVec2::new(11.0, 8.0))),
        );
        let r2 = Element::new(
            "Roof 2",
            ElementKind::Slab(slab(lv[3], DVec2::new(2.0, 2.0), DVec2::new(6.0, 6.0))),
        );
        let el = [site.id, wall.id, r1.id, r2.id];
        t = t.insert(site).insert(wall).insert(r1).insert(r2);
        t.apply(&mut m).unwrap();
        (m, lv, el)
    }

    fn ids(d: &PlanView) -> HashSet<ElementId> {
        d.items.iter().map(|it| it.id()).collect()
    }

    #[test]
    fn a_plan_without_a_range_draws_its_own_level_only() {
        let (m, lv, el) = levels();
        let v = View::plan(lv[1]);
        let d = plan(&m, &v, lv[1], 1.2);
        let got = ids(&d);
        assert!(got.contains(&el[1]));
        assert!(!got.contains(&el[0]) && !got.contains(&el[2]) && !got.contains(&el[3]));
    }

    #[test]
    fn a_range_draws_each_level_its_way() {
        let (m, lv, el) = levels();
        let mut v = View::plan(lv[1]);
        v.graphics.range.bottom = Some(RangeLimit {
            level: lv[0],
            offset: 0.0,
        });
        v.graphics.range.top = Some(RangeLimit {
            level: lv[3],
            offset: 1.0,
        });
        v.graphics.range.overhead = true;
        let d = plan(&m, &v, lv[1], 1.2);
        let pens = |id: ElementId| -> Vec<Pen> {
            d.items
                .iter()
                .filter(|it| it.id() == id)
                .filter_map(|it| match it {
                    PlanItem::Line { pen, .. } | PlanItem::Polygon { pen, .. } => Some(*pen),
                    _ => None,
                })
                .collect()
        };
        // the site wall, 1.8 m from -0.45, is cut by the plane at +1.2
        assert!(pens(el[0]).contains(&Pen::Cut), "{:?}", pens(el[0]));
        assert!(pens(el[1]).contains(&Pen::Cut));
        // both roofs above the cut, dashed
        for r in [el[2], el[3]] {
            let p = pens(r);
            assert!(
                !p.is_empty() && p.iter().all(|p| *p == Pen::Hidden),
                "{p:?}"
            );
        }
        // a lower top leaves the upper roof out
        v.graphics.range.top = Some(RangeLimit {
            level: lv[3],
            offset: -0.1,
        });
        let got = ids(&plan(&m, &v, lv[1], 1.2));
        assert!(got.contains(&el[2]) && !got.contains(&el[3]));
        // the site level as the view depth, under the bottom: beyond
        v.graphics.range.bottom = None;
        v.graphics.range.beyond = Some(RangeLimit {
            level: lv[0],
            offset: 0.0,
        });
        let far = beyond(&m, &v);
        assert!(far.contains(&el[0]) && !far.contains(&el[1]));
        assert!(ids(&plan(&m, &v, lv[1], 1.2)).contains(&el[0]));
    }

    #[test]
    fn from_above_upper_roofs_hide_what_is_under_them() {
        let (m, lv, el) = levels();
        let mut v = View::plan(lv[1]);
        v.graphics.range.from_above = true;
        let d = plan(&m, &v, lv[1], 1.2);
        let lines: Vec<(ElementId, DVec2, DVec2)> = d
            .items
            .iter()
            .filter_map(|it| match it {
                PlanItem::Line { id, a, b, .. } => Some((*id, *a, *b)),
                _ => None,
            })
            .collect();
        // every level shows, the site wall included; the house wall is
        // wholly under roof 1
        for e in [el[0], el[2], el[3]] {
            assert!(lines.iter().any(|l| l.0 == e), "{e:?}");
        }
        // the house wall is under roof 1 everywhere: no line of it shows
        // inside the roof, away from the roof's edge
        let inside = |p: DVec2| p.x > -0.9 && p.x < 10.9 && p.y > -0.9 && p.y < 7.9;
        assert!(!lines
            .iter()
            .any(|l| l.0 == el[1] && inside((l.1 + l.2) / 2.0)));
        // roof 1's lines stop where roof 2 lies over it
        let under_r2 = |p: DVec2| p.x > 2.1 && p.x < 5.9 && p.y > 2.1 && p.y < 5.9;
        assert!(!lines
            .iter()
            .any(|l| l.0 == el[2] && under_r2((l.1 + l.2) / 2.0)));
        // a top under roof 2 leaves it out and shows roof 1 whole again
        v.graphics.range.top = Some(RangeLimit {
            level: lv[3],
            offset: 0.0,
        });
        let d = plan(&m, &v, lv[1], 1.2);
        assert!(!ids(&d).contains(&el[3]));
        let _ = ViewKind::Plan {
            level: lv[1],
            cut_height: 1.2,
        };
    }

    #[test]
    fn shadows_fall_on_the_ground_away_from_the_light() {
        let (m, lv, _) = levels();
        let mut v = View::plan(lv[1]);
        v.graphics.range.from_above = true;
        v.look.cues.shadows = Some(creabim_core::styles::Shadows {
            direction: 0.0,
            length: 1.0,
            opacity: 0.5,
        });
        let d = plan(&m, &v, lv[1], 1.2);
        let shadow_x = d
            .items
            .iter()
            .filter_map(|it| match it {
                PlanItem::Shade { id, points, rgb } if *id == ElementId::NIL && rgb[0] < 1.0 => {
                    Some(points)
                }
                _ => None,
            })
            .flatten()
            .map(|p| p.x)
            .fold(f64::MIN, f64::max);
        // roof 1 is 11 m east; its shadow reaches past it toward +x
        assert!(shadow_x > 13.0, "{shadow_x}");
    }
}
