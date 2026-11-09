//! Detail components and callouts drawn: the 2D drafting of
//! [`creabim_core::detail`], and the box a detail view is called out by on
//! its parent.

use crate::plan::{Pen, PlanItem};
use creabim_core::{DetailItem, DetailKind, ElementId, Hatch, MembraneLook};
use glam::DVec2;

/// How far off the line it follows a membrane's heavy line lies, in paper
/// millimetres, and its partner beyond it.
pub const MEMBRANE_OFFSET: f64 = 2.0;
pub const MEMBRANE_PARTNER: f64 = 3.5;

/// `pts` moved `by` to their left (right when negative), corners mitred.
pub fn offset_path(pts: &[DVec2], by: f64) -> Vec<DVec2> {
    creabim_core::offset::offset_path(pts, by)
}

/// The linework of a detail component, in its view's drawing coordinates;
/// `mm` is drawing units per paper millimetre (a membrane stands off its
/// line by paper millimetres), `look` how membranes are drawn.
pub fn detail_items(id: ElementId, d: &DetailItem, mm: f64, look: MembraneLook) -> Vec<PlanItem> {
    let mut out = Vec::new();
    let line = |out: &mut Vec<PlanItem>, pen: Pen, a: DVec2, b: DVec2| {
        out.push(PlanItem::Line { id, pen, a, b });
    };
    let segs: Vec<(DVec2, DVec2)> = d
        .points
        .windows(2)
        .map(|w| (w[0], w[1]))
        .filter(|(a, b)| a.distance(*b) > 1e-9)
        .collect();
    let w = d.width.max(0.0);
    match d.kind {
        DetailKind::Batt => {
            // a quilt: a wave from face to face, a loop every width along
            let half = w / 2.0;
            for &(a, b) in &segs {
                let len = a.distance(b);
                let dir = (b - a) / len;
                let n = dir.perp();
                for s in [-half, half] {
                    line(&mut out, Pen::Fine, a + n * s, b + n * s);
                }
                if half < 1e-6 {
                    continue;
                }
                let pitch = (w * 0.6).max(1e-3);
                let steps = ((len / pitch) * 16.0).ceil().max(2.0) as usize;
                let at = |t: f64| {
                    let u = t * len;
                    // a figure-of-eight-ish loop: the point runs along and
                    // swings across, doubling back at each face
                    let phase = u / pitch * std::f64::consts::TAU;
                    a + dir * (u - pitch * 0.3 * phase.sin()) + n * (half * 0.95 * phase.cos())
                };
                let mut prev = at(0.0);
                for i in 1..=steps {
                    let p = at(i as f64 / steps as f64);
                    line(&mut out, Pen::Symbol, prev, p);
                    prev = p;
                }
            }
        }
        DetailKind::Membrane => {
            // never on the line it follows: a heavy line just off it, on
            // the side it was clicked, and a partner beyond
            let pts: Vec<DVec2> = d.points.clone();
            let side = d.side() * mm;
            let heavy = offset_path(&pts, side * MEMBRANE_OFFSET);
            let partner = offset_path(&pts, side * MEMBRANE_PARTNER);
            for w in heavy.windows(2) {
                line(&mut out, Pen::Cut, w[0], w[1]);
            }
            match look {
                MembraneLook::Dashed => {
                    for w in partner.windows(2) {
                        line(&mut out, Pen::Hidden, w[0], w[1]);
                    }
                }
                MembraneLook::Double => {
                    for w in partner.windows(2) {
                        line(&mut out, Pen::Fine, w[0], w[1]);
                    }
                }
                MembraneLook::ZigZag => {
                    let pitch = 1.5 * mm;
                    for (h, p) in heavy.windows(2).zip(partner.windows(2)) {
                        let len = h[0].distance(h[1]);
                        let steps = ((len / pitch).ceil() as usize).max(1);
                        let mut prev = h[0];
                        for k in 1..=steps {
                            let t = k as f64 / steps as f64;
                            let q = if k % 2 == 1 {
                                p[0].lerp(p[1], t)
                            } else {
                                h[0].lerp(h[1], t)
                            };
                            line(&mut out, Pen::Symbol, prev, q);
                            prev = q;
                        }
                    }
                }
            }
        }
        DetailKind::Flashing => {
            let half = w.max(1e-4) / 2.0;
            for &(a, b) in &segs {
                let n = (b - a).normalize().perp() * half;
                out.push(PlanItem::Polygon {
                    id,
                    pen: Pen::Cut,
                    points: vec![a - n, b - n, b + n, a + n],
                });
            }
        }
        DetailKind::Blocking => {
            let half = w / 2.0;
            for &(a, b) in &segs {
                let n = (b - a).normalize().perp() * half;
                let c = [a - n, b - n, b + n, a + n];
                out.push(PlanItem::Polygon {
                    id,
                    pen: Pen::Projection,
                    points: c.to_vec(),
                });
                line(&mut out, Pen::Symbol, c[0], c[2]);
                line(&mut out, Pen::Symbol, c[1], c[3]);
            }
        }
        DetailKind::Repeat { module, joint } => {
            let half = w / 2.0;
            let step = (module + joint).max(1e-3);
            for &(a, b) in &segs {
                let len = a.distance(b);
                let dir = (b - a) / len;
                let n = dir.perp() * half;
                let mut u = 0.0;
                while u < len - 1e-6 {
                    let e = (u + module).min(len);
                    let (p, q) = (a + dir * u, a + dir * e);
                    out.push(PlanItem::Polygon {
                        id,
                        pen: Pen::Projection,
                        points: vec![p - n, q - n, q + n, p + n],
                    });
                    u += step;
                }
            }
        }
        DetailKind::Region { hatch } => {
            let pts = d.points.clone();
            if pts.len() >= 3 {
                // a region masks what is under it, then takes its hatch
                out.push(PlanItem::Shade {
                    id,
                    points: pts.clone(),
                    rgb: [1.0, 1.0, 1.0],
                });
                if hatch != Hatch::Empty {
                    out.push(PlanItem::Fill {
                        id,
                        points: pts.clone(),
                        hatch,
                        rgb: None,
                    });
                }
                for i in 0..pts.len() {
                    line(&mut out, Pen::Fine, pts[i], pts[(i + 1) % pts.len()]);
                }
            }
        }
        DetailKind::BreakLine => {
            for &(a, b) in &segs {
                let len = a.distance(b);
                let dir = (b - a) / len;
                let n = dir.perp();
                let h = (len * 0.06).clamp(0.02, 0.3);
                let m = (a + b) / 2.0;
                let zig = [
                    a,
                    m - dir * h * 0.5,
                    m - dir * h * 0.15 + n * h,
                    m + dir * h * 0.15 - n * h,
                    m + dir * h * 0.5,
                    b,
                ];
                for z in zig.windows(2) {
                    line(&mut out, Pen::Symbol, z[0], z[1]);
                }
            }
        }
    }
    out
}

/// The short reference a callout shows: the first word of the view's name
/// with a digit in it ("Detail 3 Eaves" shows 3), else its last word.
pub fn tag_of(name: &str) -> String {
    name.split_whitespace()
        .find(|w| w.chars().any(|c| c.is_ascii_digit()))
        .or_else(|| name.rsplit(' ').next())
        .unwrap_or(name)
        .to_string()
}

/// A callout's box on its parent view, and its number in a bubble at the
/// box's top right corner; `mm` is drawing units per paper millimetre.
pub fn callout_items(id: ElementId, name: &str, lo: DVec2, hi: DVec2, mm: f64) -> Vec<PlanItem> {
    let c = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
    let mut out: Vec<PlanItem> = (0..4)
        .map(|i| PlanItem::Line {
            id,
            pen: Pen::Symbol,
            a: c[i],
            b: c[(i + 1) % 4],
        })
        .collect();
    // a short leader out of the corner to the bubble
    let bubble = hi + DVec2::splat(4.5 * mm);
    out.push(PlanItem::Line {
        id,
        pen: Pen::Symbol,
        a: hi,
        b: bubble - DVec2::splat(2.5 * mm * std::f64::consts::FRAC_1_SQRT_2),
    });
    out.push(PlanItem::Label {
        id,
        at: bubble,
        text: tag_of(name),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: DetailKind, pts: &[(f64, f64)]) -> DetailItem {
        DetailItem::new(
            ElementId::new(),
            kind,
            pts.iter().map(|&(x, y)| DVec2::new(x, y)).collect(),
        )
    }

    #[test]
    fn each_kind_draws_within_its_band() {
        for kind in DetailKind::ALL {
            let pts: &[(f64, f64)] = if kind.is_area() {
                &[(0.0, 0.0), (1.0, 0.0), (1.0, 0.5)]
            } else {
                &[(0.0, 0.0), (1.0, 0.0)]
            };
            let d = item(kind, pts);
            let items = detail_items(ElementId::new(), &d, 0.01, MembraneLook::default());
            assert!(!items.is_empty(), "{kind:?} draws something");
            let (lo, hi) = crate::plan::PlanView { items }.bounds().unwrap();
            let reach = d.width.max(0.06) + 0.6;
            assert!(lo.y > -reach && hi.y < reach, "{kind:?}: {lo} {hi}");
            assert!(lo.x > -0.2 && hi.x < 1.2, "{kind:?}: {lo} {hi}");
        }
    }

    #[test]
    fn a_membrane_never_lies_on_the_line_it_follows() {
        let mut d = item(DetailKind::Membrane, &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]);
        // 1:10, a paper millimetre is 10 mm
        for look in MembraneLook::ALL {
            let items = detail_items(ElementId::new(), &d, 0.01, look);
            let lines: Vec<(Pen, DVec2, DVec2)> = items
                .iter()
                .filter_map(|i| match i {
                    PlanItem::Line { pen, a, b, .. } => Some((*pen, *a, *b)),
                    _ => None,
                })
                .collect();
            // the heavy line is 2 mm on paper to the left, mitred round the corner
            let heavy: Vec<_> = lines.iter().filter(|l| l.0 == Pen::Cut).collect();
            assert_eq!(heavy.len(), 2);
            assert!(heavy[0].1.distance(DVec2::new(0.0, 0.02)) < 1e-9);
            assert!(heavy[0].2.distance(DVec2::new(0.98, 0.02)) < 1e-9);
            assert!(
                lines.iter().all(|l| l.1.y > 1e-6 || l.1.x < 1.0 - 1e-6),
                "{look:?}"
            );
            assert!(
                lines.iter().any(|l| l.0 != Pen::Cut),
                "{look:?} has a partner"
            );
        }
        // flipped, it goes the other way
        d.flip = true;
        let items = detail_items(ElementId::new(), &d, 0.01, MembraneLook::Dashed);
        assert!(items.iter().any(|i| matches!(i,
            PlanItem::Line { pen: Pen::Cut, a, .. } if a.distance(DVec2::new(0.0, -0.02)) < 1e-9)));
    }

    #[test]
    fn bricks_repeat_along_the_path() {
        let d = item(
            DetailKind::Repeat {
                module: 0.215,
                joint: 0.01,
            },
            &[(0.0, 0.0), (0.9, 0.0)],
        );
        let units = detail_items(ElementId::new(), &d, 0.01, MembraneLook::default())
            .iter()
            .filter(|i| matches!(i, PlanItem::Polygon { .. }))
            .count();
        assert_eq!(units, 4);
    }
}
