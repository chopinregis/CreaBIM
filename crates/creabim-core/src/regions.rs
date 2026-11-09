//! The faces closed outlines make where they cross and nest (MU37: two
//! overlapping shapes in a profile filled wrongly; the owner asked to pick
//! a void, "a bit like AutoCAD's trim", and to fill it back).
//!
//! Every edge is split where it meets another; the pieces are walked into
//! faces, each with its holes (a loop drawn inside it without touching it).
//! A face is filled as a hatch's islands are: when an odd number of closed
//! outlines cover it, or any solid one does (a wire thickened), and then
//! turned over when an odd number of the picked `flips` points lie in it.
//! What is filled comes back as outer loops (anticlockwise) and holes
//! (clockwise).

use crate::polygon::{contains, signed_area2};
use glam::DVec2;

const EPS: f64 = 1e-9;

/// An outline to split into faces; `solid` fills whatever it covers.
#[derive(Clone, Debug)]
pub struct Loop {
    pub points: Vec<DVec2>,
    pub solid: bool,
}

/// One face: its outline (anticlockwise), its holes, a point inside it,
/// and whether it is filled.
#[derive(Clone, Debug)]
pub struct Face {
    pub outer: Vec<DVec2>,
    pub holes: Vec<Vec<DVec2>>,
    pub sample: DVec2,
    pub filled: bool,
}

impl Face {
    /// Whether `p` is in it (in its outline, in none of its holes).
    pub fn holds(&self, p: DVec2) -> bool {
        contains(&self.outer, p) && !self.holes.iter().any(|h| contains(h, p))
    }
}

struct Graph {
    verts: Vec<DVec2>,
    /// Half-edges: (from, to); the twin of `i` is `i ^ 1`.
    half: Vec<(usize, usize)>,
    /// Outgoing half-edges of each vertex, by angle.
    out: Vec<Vec<usize>>,
}

fn vid(verts: &mut Vec<DVec2>, p: DVec2, tol: f64) -> usize {
    if let Some(i) = verts.iter().position(|q| q.distance(p) < tol) {
        return i;
    }
    verts.push(p);
    verts.len() - 1
}

fn build(loops: &[Loop]) -> Graph {
    let mut segs: Vec<(DVec2, DVec2)> = vec![];
    for l in loops {
        let n = l.points.len();
        if n < 3 {
            continue;
        }
        for i in 0..n {
            let (a, b) = (l.points[i], l.points[(i + 1) % n]);
            if a.distance(b) > EPS {
                segs.push((a, b));
            }
        }
    }
    let span = segs
        .iter()
        .flat_map(|(a, b)| [*a, *b])
        .fold(0.0f64, |m, p| m.max(p.x.abs()).max(p.y.abs()))
        .max(1e-3);
    let tol = span * 1e-9;
    let mut verts: Vec<DVec2> = vec![];
    let mut edges: Vec<(usize, usize)> = vec![];
    for (i, &(a, b)) in segs.iter().enumerate() {
        let d = b - a;
        let len2 = d.length_squared();
        let mut ts: Vec<f64> = vec![0.0, 1.0];
        for (j, &(c, e)) in segs.iter().enumerate() {
            if i == j {
                continue;
            }
            let f = e - c;
            let den = d.perp_dot(f);
            if den.abs() > EPS * d.length() * f.length() {
                let t = (c - a).perp_dot(f) / den;
                let u = (c - a).perp_dot(d) / den;
                if (-1e-12..=1.0 + 1e-12).contains(&t) && (-1e-12..=1.0 + 1e-12).contains(&u) {
                    ts.push(t.clamp(0.0, 1.0));
                }
            } else if (c - a).perp_dot(d).abs() <= tol * d.length().max(1.0) {
                // collinear: the other's ends that lie on this one
                for q in [c, e] {
                    let t = (q - a).dot(d) / len2;
                    if (0.0..=1.0).contains(&t) {
                        ts.push(t);
                    }
                }
            }
        }
        ts.sort_by(|x, y| x.total_cmp(y));
        let mut prev = vid(&mut verts, a, tol);
        for t in ts.into_iter().skip(1) {
            let v = vid(&mut verts, a + d * t, tol);
            if v != prev {
                let key = (prev.min(v), prev.max(v));
                if !edges.contains(&key) {
                    edges.push(key);
                }
                prev = v;
            }
        }
    }
    let mut half = vec![];
    for (u, v) in edges {
        half.push((u, v));
        half.push((v, u));
    }
    let mut out: Vec<Vec<usize>> = vec![vec![]; verts.len()];
    for (h, &(u, _)) in half.iter().enumerate() {
        out[u].push(h);
    }
    for (u, list) in out.iter_mut().enumerate() {
        let p = verts[u];
        list.sort_by(|x, y| {
            let ax = (verts[half[*x].1] - p).to_angle();
            let ay = (verts[half[*y].1] - p).to_angle();
            ax.total_cmp(&ay)
        });
    }
    Graph { verts, half, out }
}

impl Graph {
    /// The half-edge after `h` round the face on its left: at its end, the
    /// next one clockwise from the way back, among those `keep` allows.
    fn next(&self, h: usize, keep: &dyn Fn(usize) -> bool) -> Option<usize> {
        let v = self.half[h].1;
        let list = &self.out[v];
        let back = list.iter().position(|&x| x == (h ^ 1))?;
        let n = list.len();
        (1..=n).map(|k| list[(back + n - k) % n]).find(|&x| keep(x))
    }

    /// Every cycle of the half-edges `keep` allows, as vertex loops with
    /// the half-edges they are made of.
    fn cycles(&self, keep: &dyn Fn(usize) -> bool) -> Vec<(Vec<DVec2>, Vec<usize>)> {
        let mut seen = vec![false; self.half.len()];
        let mut out = vec![];
        for start in 0..self.half.len() {
            if seen[start] || !keep(start) {
                continue;
            }
            let (mut pts, mut hs) = (vec![], vec![]);
            let mut h = start;
            loop {
                if seen[h] {
                    break;
                }
                seen[h] = true;
                pts.push(self.verts[self.half[h].0]);
                hs.push(h);
                match self.next(h, keep) {
                    Some(x) => h = x,
                    None => break,
                }
                if h == start {
                    break;
                }
            }
            if pts.len() >= 3 {
                out.push((pts, hs));
            }
        }
        out
    }
}

/// A point just inside a loop, off its edges.
fn inside_point(outer: &[DVec2], holes: &[Vec<DVec2>]) -> DVec2 {
    let n = outer.len();
    let size = outer
        .iter()
        .fold(0.0f64, |m, p| m.max(p.distance(outer[0])))
        .max(1e-6);
    for scale in [1e-4, 1e-3, 1e-2, 1e-5] {
        for i in 0..n {
            let (a, b) = (outer[i], outer[(i + 1) % n]);
            let d = b - a;
            if d.length() < EPS {
                continue;
            }
            // the left of an anticlockwise outline is inside
            let p = (a + b) / 2.0 + d.perp().normalize() * size * scale;
            if contains(outer, p) && !holes.iter().any(|h| contains(h, p)) {
                return p;
            }
        }
    }
    outer.iter().copied().sum::<DVec2>() / n.max(1) as f64
}

/// The faces of `loops`, filled or not, turned over by `flips`.
pub fn faces(loops: &[Loop], flips: &[DVec2]) -> Vec<Face> {
    let g = build(loops);
    let cycles = g.cycles(&|_| true);
    let mut faces: Vec<Face> = vec![];
    let mut holes: Vec<Vec<DVec2>> = vec![];
    for (pts, _) in cycles {
        let a = signed_area2(&pts);
        if a > EPS {
            faces.push(Face {
                outer: pts,
                holes: vec![],
                sample: DVec2::ZERO,
                filled: false,
            });
        } else if a < -EPS {
            holes.push(pts);
        }
    }
    // a loop drawn inside a face without touching it is its hole: in the
    // smallest face round it
    for h in holes {
        let p = h[0];
        let host = faces
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                contains(&f.outer, p)
                    && !f.outer.iter().any(|q| q.distance(p) < 1e-12)
                    && signed_area2(&f.outer) > -signed_area2(&h) + EPS
            })
            .min_by(|x, y| signed_area2(&x.1.outer).total_cmp(&signed_area2(&y.1.outer)))
            .map(|(i, _)| i);
        if let Some(i) = host {
            faces[i].holes.push(h);
        }
    }
    for f in &mut faces {
        f.sample = inside_point(&f.outer, &f.holes);
        let s = f.sample;
        let covered = loops
            .iter()
            .filter(|l| !l.solid && l.points.len() >= 3 && contains(&l.points, s))
            .count();
        let solid = loops
            .iter()
            .any(|l| l.solid && l.points.len() >= 3 && contains(&l.points, s));
        let flipped = flips.iter().filter(|q| f.holds(**q)).count();
        f.filled = (covered % 2 == 1 || solid) != (flipped % 2 == 1);
    }
    faces
}

/// What is filled, as outer loops (anticlockwise) and holes (clockwise):
/// the faces' edges with a filled face on one side only.
pub fn filled(loops: &[Loop], flips: &[DVec2]) -> Vec<Vec<DVec2>> {
    let fs = faces(loops, flips);
    if fs.is_empty() {
        return vec![];
    }
    let g = build(loops);
    // the face on each half-edge's left, by the point just left of it
    let side = |h: usize| -> bool {
        let (a, b) = (g.verts[g.half[h].0], g.verts[g.half[h].1]);
        let d = b - a;
        let span = d.length().max(1e-9);
        let p = (a + b) / 2.0 + d.perp() / span * (span * 1e-6).max(1e-9);
        fs.iter()
            .filter(|f| f.holds(p))
            .min_by(|x, y| signed_area2(&x.outer).total_cmp(&signed_area2(&y.outer)))
            .is_some_and(|f| f.filled)
    };
    let left: Vec<bool> = (0..g.half.len()).map(side).collect();
    let boundary = |h: usize| left[h] && !left[h ^ 1];
    g.cycles(&boundary).into_iter().map(|(p, _)| p).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<DVec2> {
        vec![
            DVec2::new(x0, y0),
            DVec2::new(x1, y0),
            DVec2::new(x1, y1),
            DVec2::new(x0, y1),
        ]
    }

    fn area(loops: &[Vec<DVec2>]) -> f64 {
        loops.iter().map(|l| signed_area2(l) / 2.0).sum()
    }

    #[test]
    fn a_shape_inside_another_is_a_hole_and_a_pick_fills_it() {
        let l = vec![
            Loop {
                points: rect(0.0, 0.0, 4.0, 4.0),
                solid: false,
            },
            Loop {
                points: rect(1.0, 1.0, 2.0, 2.0),
                solid: false,
            },
        ];
        assert!((area(&filled(&l, &[])) - 15.0).abs() < 1e-9);
        assert!((area(&filled(&l, &[DVec2::new(1.5, 1.5)])) - 16.0).abs() < 1e-9);
    }

    #[test]
    fn overlapping_shapes_leave_their_overlap_void_until_picked() {
        let l = vec![
            Loop {
                points: rect(0.0, 0.0, 4.0, 2.0),
                solid: false,
            },
            Loop {
                points: rect(1.0, -1.0, 2.0, 3.0),
                solid: false,
            },
        ];
        // 8 + 4 - 2 * 2 (the overlap, void)
        assert!((area(&filled(&l, &[])) - 8.0).abs() < 1e-9);
        assert!((area(&filled(&l, &[DVec2::new(1.5, 1.0)])) - 10.0).abs() < 1e-9);
        // a piece of a shape picked becomes a void
        assert!((area(&filled(&l, &[DVec2::new(3.0, 1.0)])) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn a_shape_drawn_clockwise_crosses_the_same() {
        let mut cw = rect(-0.0125, -0.014, 0.0125, 0.064);
        cw.reverse();
        let l = vec![
            Loop {
                points: rect(-0.05, 0.0, 0.05, 0.05),
                solid: false,
            },
            Loop {
                points: cw,
                solid: false,
            },
        ];
        let fs = faces(&l, &[]);
        let a = area(&filled(&l, &[]));
        // 0.005 + 0.025 * 0.078 - 2 * 0.025 * 0.05
        assert!(
            (a - (0.005 + 0.025 * 0.078 - 2.0 * 0.025 * 0.05)).abs() < 1e-9,
            "{a} {}",
            fs.len()
        );
    }

    #[test]
    fn a_solid_wire_fills_what_it_covers() {
        let l = vec![
            Loop {
                points: rect(0.0, 0.0, 4.0, 2.0),
                solid: false,
            },
            Loop {
                points: rect(3.0, 1.0, 5.0, 3.0),
                solid: true,
            },
        ];
        assert!((area(&filled(&l, &[])) - 11.0).abs() < 1e-9);
    }
}
