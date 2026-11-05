//! View marks in the Classic family: a split bubble with the view's number
//! over the sheet it is placed on, a solid wedge pointing the way the view
//! looks, heavy line ends and a dash-dot line between them; callouts are a
//! dashed rounded box (a circle in a section) with a leader to the bubble.
//!
//! Every size here is on paper, so the marks read the same at every scale:
//! `mm` is drawing units per paper millimetre.

use crate::plan::{Pen, PlanItem};
use creabim_core::{ElementId, ElementKind, Model};
use glam::DVec2;
use std::f64::consts::{PI, TAU};

/// The bubble's radius: a 10 mm bubble.
pub const BUBBLE: f64 = 5.0;
/// The wedge's dark fill.
const INK: [f32; 3] = [0.17, 0.19, 0.16];

/// The number of the sheet `view` is placed on, or "–" until it is.
pub fn sheet_ref(model: &Model, view: ElementId) -> String {
    model
        .iter()
        .find_map(|e| match &e.kind {
            ElementKind::Viewport(vp) if vp.view == view => {
                crate::sheet::sheet_of(model, vp.sheet).map(|s| s.number.clone())
            }
            _ => None,
        })
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "–".into())
}

/// The number a view's marks carry: a section's letter ("Section A-A"
/// shows A), a detail's number, and an elevation's place among the
/// project's elevations.
pub fn number_of(model: &Model, view: ElementId) -> String {
    let Some(e) = model.get(view) else {
        return "–".into();
    };
    let elevation = |v: &creabim_core::View| {
        v.callout.is_none() && matches!(v.kind, creabim_core::ViewKind::Section { cut: false, .. })
    };
    if let ElementKind::View(v) = &e.kind {
        if elevation(v) {
            let n = model
                .iter()
                .filter(|x| matches!(&x.kind, ElementKind::View(w) if elevation(w)))
                .position(|x| x.id == view)
                .unwrap_or(0);
            return (n + 1).to_string();
        }
    }
    let tag = crate::detail::tag_of(&e.name);
    match tag.split_once('-') {
        Some((a, b)) if a == b && !a.is_empty() => a.to_string(),
        _ => tag,
    }
}

fn circle(id: ElementId, pen: Pen, c: DVec2, r: f64) -> PlanItem {
    PlanItem::Arc {
        id,
        pen,
        center: c,
        radius: r,
        start: 0.0,
        end: TAU,
    }
}

fn text(id: ElementId, at: DVec2, text: &str, size: f64) -> PlanItem {
    PlanItem::Text {
        id,
        at,
        text: text.to_string(),
        size,
        align: crate::paper::Anchor::Middle,
    }
}

/// A split bubble at `c`: `number` above the line, `sheet` below, and a
/// wedge pointing `look` when there is one.
pub fn bubble(
    id: ElementId,
    c: DVec2,
    number: &str,
    sheet: &str,
    look: Option<DVec2>,
    mm: f64,
) -> Vec<PlanItem> {
    let r = BUBBLE * mm;
    let mut out = vec![];
    if let Some(look) = look.and_then(|l| l.try_normalize()) {
        // the wedge: from where its sides touch the bubble out to its tip,
        // following the bubble round so it never covers the text
        let tip = c + look * r * 2.0;
        let a0 = look.y.atan2(look.x);
        let half = (r / (2.0 * r)).acos(); // the tangents from the tip
        let mut pts = vec![tip];
        let n = 10;
        for i in 0..=n {
            let t = a0 - half + 2.0 * half * i as f64 / n as f64;
            pts.push(c + DVec2::from_angle(t) * r);
        }
        pts.reverse();
        out.push(PlanItem::Shade {
            id,
            points: pts,
            rgb: INK,
        });
    }
    out.push(circle(id, Pen::Symbol, c, r));
    out.push(PlanItem::Line {
        id,
        pen: Pen::Symbol,
        a: c - DVec2::X * r,
        b: c + DVec2::X * r,
    });
    let big = if number.chars().count() > 2 { 2.6 } else { 3.2 };
    out.push(text(id, c + DVec2::Y * 2.1 * mm, number, big));
    out.push(text(id, c - DVec2::Y * 2.1 * mm, sheet, 1.9));
    out
}

/// A section line from `a` to `b` looking to its left (or an elevation
/// mark at its middle when `cut` is off), in the Classic family.
pub fn section(
    id: ElementId,
    number: &str,
    sheet: &str,
    a: DVec2,
    b: DVec2,
    cut: bool,
    mm: f64,
) -> Vec<PlanItem> {
    let along = (b - a).normalize_or_zero();
    let look = along.perp();
    let mut out = vec![];
    if !cut {
        out.extend(bubble(id, (a + b) / 2.0, number, sheet, Some(look), mm));
        return out;
    }
    let r = BUBBLE * mm;
    let stub = 6.0 * mm;
    let len = a.distance(b);
    // heavy ends, and a dash-dot line between them
    for (p, d) in [(a, along), (b, -along)] {
        out.push(PlanItem::Line {
            id,
            pen: Pen::Cut,
            a: p,
            b: p + d * stub.min(len / 2.0),
        });
    }
    if len > stub * 2.0 {
        out.push(PlanItem::Line {
            id,
            pen: Pen::Grid,
            a: a + along * stub,
            b: b - along * stub,
        });
    }
    for (p, d) in [(a, -along), (b, along)] {
        out.extend(bubble(id, p + d * r, number, sheet, Some(look), mm));
    }
    out
}

/// A callout's dashed rounded box from `lo` to `hi` (a dashed circle when
/// `round`), with a leader from its corner to its bubble.
pub fn callout(
    id: ElementId,
    number: &str,
    sheet: &str,
    lo: DVec2,
    hi: DVec2,
    round: bool,
    mm: f64,
) -> Vec<PlanItem> {
    let mut out = vec![];
    let corner = if round {
        let c = (lo + hi) / 2.0;
        let rad = (hi - lo).abs().min_element() / 2.0;
        // a dashed circle, in short arcs
        let n = 24;
        for i in (0..n).step_by(2) {
            out.push(PlanItem::Arc {
                id,
                pen: Pen::Symbol,
                center: c,
                radius: rad,
                start: TAU * i as f64 / n as f64,
                end: TAU * (i + 1) as f64 / n as f64,
            });
        }
        c + DVec2::from_angle(PI / 4.0) * rad
    } else {
        let size = hi - lo;
        let k = (3.0 * mm).min(size.min_element() / 3.0);
        let c = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
        for i in 0..4 {
            let (p, q) = (c[i], c[(i + 1) % 4]);
            let d = (q - p).normalize_or_zero();
            out.push(PlanItem::Line {
                id,
                pen: Pen::Hidden,
                a: p + d * k,
                b: q - d * k,
            });
        }
        // rounded corners
        let centres = [
            lo + DVec2::splat(k),
            DVec2::new(hi.x - k, lo.y + k),
            hi - DVec2::splat(k),
            DVec2::new(lo.x + k, hi.y - k),
        ];
        for (i, cc) in centres.into_iter().enumerate() {
            let start = PI + i as f64 * PI / 2.0;
            out.push(PlanItem::Arc {
                id,
                pen: Pen::Symbol,
                center: cc,
                radius: k,
                start,
                end: start + PI / 2.0,
            });
        }
        hi - DVec2::splat(k) + DVec2::splat(k * std::f64::consts::FRAC_1_SQRT_2)
    };
    let r = BUBBLE * mm;
    let at = corner + DVec2::new(9.0, 6.0) * mm;
    let d = (at - corner).normalize_or_zero();
    out.push(PlanItem::Line {
        id,
        pen: Pen::Symbol,
        a: corner,
        b: at - d * r,
    });
    out.extend(bubble(id, at, number, sheet, None, mm));
    out
}

/// In a plan's items, the section and elevation marks redrawn in the
/// Classic family: each section view's own items are taken out and its
/// Classic marks put in their place.
pub fn restyle_sections(model: &Model, items: &mut Vec<PlanItem>, mm: f64) {
    let mut seen: Vec<ElementId> = vec![];
    items.retain(|it| {
        let id = it.id();
        let is_section = matches!(
            model.get(id).map(|e| &e.kind),
            Some(ElementKind::View(v)) if v.callout.is_none()
                && matches!(v.kind, creabim_core::ViewKind::Section { .. })
        );
        if is_section && !seen.contains(&id) {
            seen.push(id);
        }
        !is_section
    });
    for id in seen {
        let Some(e) = model.get(id) else { continue };
        let ElementKind::View(v) = &e.kind else {
            continue;
        };
        if let creabim_core::ViewKind::Section {
            start, end, cut, ..
        } = v.kind
        {
            let number = number_of(model, id);
            let sheet = sheet_ref(model, id);
            items.extend(section(id, &number, &sheet, start, end, cut, mm));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_classic_section_has_two_split_bubbles_and_heavy_ends() {
        let id = ElementId::new();
        let items = section(
            id,
            "A",
            "A301",
            DVec2::ZERO,
            DVec2::new(10.0, 0.0),
            true,
            0.1,
        );
        let texts: Vec<&str> = items
            .iter()
            .filter_map(|i| match i {
                PlanItem::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["A", "A301", "A", "A301"]);
        let heavy = items
            .iter()
            .filter(|i| matches!(i, PlanItem::Line { pen: Pen::Cut, .. }))
            .count();
        assert_eq!(heavy, 2);
        // the wedges point the way the view looks: to the left of the line
        for i in &items {
            if let PlanItem::Shade { points, .. } = i {
                assert!(points.iter().any(|p| p.y > 0.9), "{points:?}");
                assert!(points.iter().all(|p| p.y > -1e-9));
            }
        }
    }

    #[test]
    fn the_sheet_reference_waits_for_a_sheet() {
        let m = creabim_core::sample::sample_house();
        let placed = m.iter().find_map(|e| match &e.kind {
            ElementKind::Viewport(vp) => Some(vp.view),
            _ => None,
        });
        if let Some(v) = placed {
            assert_ne!(sheet_ref(&m, v), "–");
        }
        assert_eq!(sheet_ref(&m, ElementId::new()), "–");
    }
}
