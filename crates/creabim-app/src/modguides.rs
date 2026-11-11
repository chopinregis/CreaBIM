//! What a modifier being set shows in the views (MU39): only where it
//! helps to see where the modifier works, never for the rest. A radial
//! array's centre and the circle its copies sit on; a mirror's plane; a
//! slice's cuts as see-through floors. Every view draws them over its
//! drawing, in model space, as the views draw Divide's marks.

use eframe::egui::{self, Color32, Painter, Pos2, Stroke};
use glam::DVec3;

/// The guides of the modifier being set, in model space.
#[derive(Clone, Debug, Default)]
pub struct Guides {
    /// See-through planes: four corners each.
    pub planes: Vec<[DVec3; 4]>,
    /// Lines (a circle comes as its segments).
    pub lines: Vec<(DVec3, DVec3)>,
    /// Points marked with a cross: a centre.
    pub points: Vec<DVec3>,
    /// Their colour, when not the selection's (Split's red).
    pub ink: Option<Color32>,
}

/// The guides of `md` on mass `m`, whose own axes reach the model through
/// `to_world`.
pub fn of(
    m: &creabim_core::FreeMesh,
    md: &creabim_core::ModifierKind,
    to_world: &dyn Fn(DVec3) -> DVec3,
) -> Guides {
    use creabim_core::ModifierKind as K;
    let mut g = Guides::default();
    let Some((lo, hi)) = bounds(&m.mesh.vertices) else {
        return g;
    };
    let pad = 0.3;
    match md {
        K::RadialArray { center, .. } => {
            let c = DVec3::new(center.x, center.y, lo.z);
            g.points.push(to_world(c));
            // the circle the shape's middle runs round
            let mid = (lo + hi) / 2.0;
            let r = (mid.truncate() - *center).length();
            if r > 1e-3 {
                let n = 64;
                let at = |k: usize| {
                    let t = k as f64 / n as f64 * std::f64::consts::TAU;
                    to_world(c + DVec3::new(t.cos() * r, t.sin() * r, 0.0))
                };
                for k in 0..n {
                    g.lines.push((at(k), at(k + 1)));
                }
                g.lines
                    .push((to_world(c), to_world(DVec3::new(mid.x, mid.y, lo.z))));
            }
        }
        K::Mirror { axis, plane } => {
            let k = axis.index();
            // the plane across the axis, as big as the shape and a little more
            let (u, v) = ((k + 1) % 3, (k + 2) % 3);
            let corner = |a: f64, b: f64| {
                let mut p = DVec3::ZERO;
                p[k] = *plane;
                p[u] = a;
                p[v] = b;
                to_world(p)
            };
            let (u0, u1) = (lo[u] - pad, hi[u] + pad);
            let (v0, v1) = (lo[v] - pad, hi[v] + pad);
            g.planes.push([
                corner(u0, v0),
                corner(u1, v0),
                corner(u1, v1),
                corner(u0, v1),
            ]);
        }
        K::Slice { heights, .. } => {
            for h in heights {
                if *h <= lo.z + 1e-6 || *h >= hi.z - 1e-6 {
                    continue;
                }
                let at = |x: f64, y: f64| to_world(DVec3::new(x, y, *h));
                let (x0, x1, y0, y1) = (lo.x - pad, hi.x + pad, lo.y - pad, hi.y + pad);
                g.planes
                    .push([at(x0, y0), at(x1, y0), at(x1, y1), at(x0, y1)]);
            }
        }
        K::PathArray { path, .. } => {
            for w in path.windows(2) {
                g.lines.push((to_world(w[0]), to_world(w[1])));
            }
        }
        _ => {}
    }
    g
}

fn bounds(v: &[DVec3]) -> Option<(DVec3, DVec3)> {
    let first = *v.first()?;
    Some(
        v.iter()
            .fold((first, first), |(a, b), p| (a.min(*p), b.max(*p))),
    )
}

/// Draw `g` in a view that shows model point `p` at `project(p)`; a plan
/// (`plan`) leaves out the level planes it would only cover itself with.
pub fn draw(
    painter: &Painter,
    g: &Guides,
    ink: Color32,
    plan: bool,
    project: &dyn Fn(DVec3) -> Option<Pos2>,
) {
    let fill = ink.gamma_multiply(0.16);
    let edge = Stroke::new(1.0, ink.gamma_multiply(0.75));
    for plane in &g.planes {
        if plan && plane.iter().all(|p| (p.z - plane[0].z).abs() < 1e-6) {
            continue;
        }
        let pts: Option<Vec<Pos2>> = plane.iter().map(|p| project(*p)).collect();
        let Some(pts) = pts else { continue };
        // seen edge on (a slice in a plan, a mirror in a section): its line
        let area = (0..4)
            .map(|i| {
                let (a, b) = (pts[i], pts[(i + 1) % 4]);
                a.x * b.y - b.x * a.y
            })
            .sum::<f32>()
            .abs()
            / 2.0;
        if area > 40.0 {
            painter.add(egui::Shape::convex_polygon(pts.clone(), fill, Stroke::NONE));
        }
        let mut ring = pts.clone();
        ring.push(pts[0]);
        crate::snaps::dashed_clipped(painter, &ring, edge, 6.0, 4.0);
    }
    let line = Stroke::new(1.2, ink.gamma_multiply(0.85));
    for (a, b) in &g.lines {
        if let (Some(pa), Some(pb)) = (project(*a), project(*b)) {
            crate::snaps::dashed_clipped(painter, &[pa, pb], line, 5.0, 4.0);
        }
    }
    for p in &g.points {
        if let Some(c) = project(*p) {
            let s = Stroke::new(1.6, ink);
            painter.line_segment([c - egui::vec2(9.0, 0.0), c + egui::vec2(9.0, 0.0)], s);
            painter.line_segment([c - egui::vec2(0.0, 9.0), c + egui::vec2(0.0, 9.0)], s);
            painter.circle_stroke(c, 4.0, s);
        }
    }
}
