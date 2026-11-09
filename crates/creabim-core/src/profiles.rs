//! Profiles (MU27 block 5, approved option A): one library of section
//! outlines for sweeps (and later extrusions and railings). Built-in
//! presets, and the project's own profiles, stored with the project in its
//! [`crate::ProjectInfo`]. A profile is a closed outline in metres, in a
//! sweep's own axes: `x` across the path (positive to the left, looking
//! along it), `y` up; its insertion point is the origin.

use crate::{ElementKind, Model};
use glam::DVec2;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// A profile of the project's own.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NamedProfile {
    pub name: String,
    pub points: Vec<DVec2>,
    /// An open line, not an outline (MU36: an open sketch was closed by
    /// force): swept, it is thickened to `thickness` on its left.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub open: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<f64>,
    /// More shapes in the same profile (MU37): a closed one inside the
    /// first is a hole (a frame, a hollow section), one beside it another
    /// piece, an open one a wire thickened (a flashing on a coping).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub more: Vec<ProfileShape>,
    /// Points picked to turn a face over, void to filled or back (MU37:
    /// "pick a void… or fill back a void"); see [`crate::regions`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flips: Vec<DVec2>,
}

/// One more shape of a profile.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProfileShape {
    pub points: Vec<DVec2>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub open: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<f64>,
}

impl ProfileShape {
    fn outline(&self) -> Vec<DVec2> {
        if self.open {
            thicken(&self.points, self.thickness.unwrap_or(OPEN_THICKNESS))
        } else {
            self.points.clone()
        }
    }
}

/// Several outlines as one: each joined to the first by a slit from its
/// nearest corner, a hole turned the other way, so one outline sweeps
/// them all (MU37).
pub fn merge(main: Vec<DVec2>, others: &[Vec<DVec2>]) -> Vec<DVec2> {
    let mut out = main;
    if out.len() >= 3 && crate::polygon::signed_area2(&out) < 0.0 {
        out.reverse();
    }
    for o in others {
        if o.len() < 3 {
            continue;
        }
        let inside = crate::polygon::contains(&out, o[0]);
        let mut o = o.clone();
        let ccw = crate::polygon::signed_area2(&o) > 0.0;
        // a hole runs clockwise, a piece beside it anticlockwise
        if inside == ccw {
            o.reverse();
        }
        out = join(out, &o);
    }
    out
}

/// Loops already turned the right way (outers anticlockwise, holes
/// clockwise, as [`crate::regions::filled`] gives them) as one outline.
pub fn merge_as_is(main: Vec<DVec2>, others: &[Vec<DVec2>]) -> Vec<DVec2> {
    let mut out = main;
    for o in others {
        if o.len() >= 3 {
            out = join(out, o);
        }
    }
    out
}

/// `o` joined to `out` by a slit between their nearest corners.
fn join(out: Vec<DVec2>, o: &[DVec2]) -> Vec<DVec2> {
    {
        let (mut bi, mut bj, mut bd) = (0, 0, f64::MAX);
        for (i, a) in out.iter().enumerate() {
            for (j, b) in o.iter().enumerate() {
                let d = a.distance_squared(*b);
                if d < bd {
                    (bi, bj, bd) = (i, j, d);
                }
            }
        }
        let mut next = out[..=bi].to_vec();
        next.extend(o[bj..].iter().copied());
        next.extend(o[..=bj].iter().copied());
        next.extend(out[bi..].iter().copied());
        next
    }
}

/// How thick an open profile is swept when it says nothing: 20 mm.
pub const OPEN_THICKNESS: f64 = 0.02;

impl NamedProfile {
    /// The outline a sweep takes: itself, or an open line thickened.
    pub fn outline(&self) -> Vec<DVec2> {
        let main = if self.open {
            thicken(&self.points, self.thickness.unwrap_or(OPEN_THICKNESS))
        } else {
            self.points.clone()
        };
        if self.more.is_empty() && self.flips.is_empty() {
            return main;
        }
        // the faces the shapes make, filled as picked, as one outline
        let mut parts = crate::regions::filled(&self.region_loops(), &self.flips);
        parts.sort_by(|a, b| crate::polygon::signed_area2(b).abs().total_cmp(&crate::polygon::signed_area2(a).abs()));
        let mut it = parts.into_iter();
        let Some(first) = it.next() else { return main };
        let rest: Vec<Vec<DVec2>> = it.collect();
        merge_as_is(first, &rest)
    }

    /// The shapes as loops for [`crate::regions`]: closed ones as drawn,
    /// wires thickened and solid.
    pub fn region_loops(&self) -> Vec<crate::regions::Loop> {
        self.shapes()
            .iter()
            .filter(|s| s.points.len() >= 2)
            .map(|s| crate::regions::Loop { points: s.outline(), solid: s.open })
            .collect()
    }

    /// Its faces, filled or not (for the editor to show and pick).
    pub fn faces(&self) -> Vec<crate::regions::Face> {
        crate::regions::faces(&self.region_loops(), &self.flips)
    }

    /// Turn the face at `p` over: a void filled, a fill made void (MU37).
    /// False when no face is there.
    pub fn toggle_face(&mut self, p: DVec2) -> bool {
        let fs = self.faces();
        let Some(f) = fs
            .iter()
            .filter(|f| f.holds(p))
            .min_by(|a, b| crate::polygon::signed_area2(&a.outer).total_cmp(&crate::polygon::signed_area2(&b.outer)))
        else {
            return false;
        };
        let before = self.flips.len();
        self.flips.retain(|q| !f.holds(*q));
        if self.flips.len() == before {
            self.flips.push(f.sample);
        }
        true
    }

    /// Every shape: the first, then the rest, as drawn (for the editor).
    pub fn shapes(&self) -> Vec<ProfileShape> {
        let mut v = vec![ProfileShape { points: self.points.clone(), open: self.open, thickness: self.thickness }];
        v.extend(self.more.iter().cloned());
        v
    }
}

/// An open line as a thin closed outline: the line, then the line again
/// `t` to its left, back.
pub fn thicken(line: &[DVec2], t: f64) -> Vec<DVec2> {
    let n = line.len();
    if n < 2 {
        return line.to_vec();
    }
    let normal = |i: usize| -> DVec2 {
        let d = |a: usize, b: usize| (line[b] - line[a]).normalize_or_zero().perp();
        match i {
            0 => d(0, 1),
            i if i == n - 1 => d(n - 2, n - 1),
            i => {
                let m = (d(i - 1, i) + d(i, i + 1)).normalize_or_zero();
                // as far out as keeps the thickness square to both sides
                let c = m.dot(d(i, i + 1)).max(0.3);
                m / c
            }
        }
    };
    let mut out = line.to_vec();
    out.extend((0..n).rev().map(|i| line[i] + normal(i) * t));
    out
}

/// The built-in presets, by name, in the order the picker lists them.
pub const PRESETS: [&str; 12] = [
    "Coping",
    "Plinth",
    "Bar",
    "Cornice",
    "Skirting",
    "Gutter",
    "Handrail",
    "I-section",
    "Channel",
    "Angle",
    "T-section",
    "Tube",
];

fn circle(c: DVec2, r: f64, n: usize, from: f64, to: f64) -> Vec<DVec2> {
    (0..=n).map(|i| c + DVec2::from_angle(from + (to - from) * i as f64 / n as f64) * r).collect()
}

/// A preset's outline, at its usual size.
pub fn preset(name: &str) -> Option<Vec<DVec2>> {
    let p = |v: &[(f64, f64)]| v.iter().map(|(x, y)| DVec2::new(*x, *y)).collect::<Vec<_>>();
    Some(match name {
        "Coping" => crate::parts::coping(0.2, 0.04, 0.06),
        "Plinth" => crate::parts::plinth(0.2, 0.03, 0.3, false),
        "Bar" => crate::parts::rectangle(0.15, 0.15),
        "Cornice" => p(&[(0.0, 0.0), (0.05, 0.0), (0.05, 0.03), (0.09, 0.05), (0.12, 0.09), (0.15, 0.15), (0.15, 0.2), (0.0, 0.2)]),
        "Skirting" => p(&[(0.0, 0.0), (0.018, 0.0), (0.018, 0.08), (0.012, 0.095), (0.012, 0.1), (0.0, 0.1)]),
        "Gutter" => {
            // a half-round, open at its top, 3 mm thick
            let (r, t) = (0.06, 0.003);
            let c = DVec2::new(0.0, r);
            let mut v = circle(c, r, 16, PI, 2.0 * PI);
            let mut inner = circle(c, r - t, 16, PI, 2.0 * PI);
            inner.reverse();
            v.extend(inner);
            v
        }
        "Handrail" => {
            let mut v = circle(DVec2::new(0.0, 0.0225), 0.0225, 24, -PI / 2.0, 1.5 * PI);
            v.pop();
            v
        }
        "I-section" => {
            let (h, b, tf, tw) = (0.2, 0.1, 0.0085, 0.0056);
            let (x, w) = (b / 2.0, tw / 2.0);
            p(&[(-x, 0.0), (x, 0.0), (x, tf), (w, tf), (w, h - tf), (x, h - tf), (x, h), (-x, h), (-x, h - tf), (-w, h - tf), (-w, tf), (-x, tf)])
        }
        "Channel" => {
            let (h, b, t) = (0.15, 0.075, 0.007);
            p(&[(0.0, 0.0), (b, 0.0), (b, t), (t, t), (t, h - t), (b, h - t), (b, h), (0.0, h)])
        }
        "Angle" => {
            let (a, t) = (0.08, 0.008);
            p(&[(0.0, 0.0), (a, 0.0), (a, t), (t, t), (t, a), (0.0, a)])
        }
        "T-section" => {
            let (h, b, t) = (0.1, 0.1, 0.01);
            let (x, w) = (b / 2.0, t / 2.0);
            p(&[(-w, 0.0), (w, 0.0), (w, h - t), (x, h - t), (x, h), (-x, h), (-x, h - t), (-w, h - t)])
        }
        "Tube" => {
            // a round tube, its wall a thin ring cut at the top to stay one outline
            let (r, t) = (0.024, 0.003);
            let c = DVec2::new(0.0, r);
            let mut v = circle(c, r, 24, PI / 2.0 + 0.02, 2.5 * PI - 0.02);
            let mut inner = circle(c, r - t, 24, PI / 2.0 + 0.02, 2.5 * PI - 0.02);
            inner.reverse();
            v.extend(inner);
            v
        }
        _ => return None,
    })
}

/// The project's own profiles.
pub fn project_profiles(model: &Model) -> Vec<NamedProfile> {
    model.project_info().map(|(_, p)| p.profiles.clone()).unwrap_or_default()
}

/// A profile by name: the project's first, then the presets.
pub fn find(model: &Model, name: &str) -> Option<Vec<DVec2>> {
    project_profiles(model)
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.outline())
        .or_else(|| preset(name))
}

/// Write the project's profiles (one undo step).
pub fn set_profiles(model: &Model, profiles: Vec<NamedProfile>, label: &str) -> crate::Transaction {
    let mut info = model.project_info().map(|(_, p)| p.clone()).unwrap_or_default();
    info.profiles = profiles;
    let mut tx = crate::project::set_project(model, info);
    tx.label = label.into();
    tx
}

/// The parts made with library profile `name`, set to `outline`: what
/// editing a profile adds to its undo step (MU36).
pub fn follow(model: &Model, name: &str, outline: &[DVec2], tx: crate::Transaction) -> crate::Transaction {
    let mut tx = tx;
    for e in users(model, name) {
        let mut now = e.clone();
        match &mut now.kind {
            ElementKind::Part(q) => match &mut q.form {
                crate::parts::Form::Sweep { profile, .. } => *profile = outline.to_vec(),
                // an extrusion placed from a profile keeps its own outline
                crate::parts::Form::Extrusion { .. } => continue,
            },
            ElementKind::Column(c) => {
                if let Some(s) = &mut c.section {
                    s.points = outline.to_vec();
                }
            }
            ElementKind::Beam(b) => {
                if let Some(s) = &mut b.section {
                    s.points = outline.to_vec();
                }
            }
            ElementKind::Railing(_) => {}
            _ => continue,
        }
        if now != *e {
            tx = tx.replace(e.clone(), now);
        }
    }
    tx
}

/// The elements made with library profile `name`: sweeps, extrusions,
/// columns, beams and railings.
pub fn users<'a>(model: &'a Model, name: &str) -> Vec<&'a crate::Element> {
    model
        .iter()
        .filter(|e| match &e.kind {
            ElementKind::Part(p) => p.profile.as_deref() == Some(name),
            ElementKind::Column(c) => c.section.as_ref().is_some_and(|s| s.name == name),
            ElementKind::Beam(b) => b.section.as_ref().is_some_and(|s| s.name == name),
            ElementKind::Railing(r) => r.rail_profile.as_deref() == Some(name),
            _ => false,
        })
        .collect()
}

/// How many elements made with `name` don't have its current shape yet.
pub fn stale(model: &Model, name: &str) -> usize {
    let Some(now) = find(model, name) else { return 0 };
    users(model, name)
        .into_iter()
        .filter(|e| match &e.kind {
            ElementKind::Part(p) => match &p.form {
                crate::parts::Form::Sweep { profile, .. } => *profile != now,
                _ => false,
            },
            ElementKind::Column(c) => c.section.as_ref().is_some_and(|s| s.points != now),
            ElementKind::Beam(b) => b.section.as_ref().is_some_and(|s| s.points != now),
            _ => false,
        })
        .count()
}

/// A name not yet taken: `base`, else `base 2`, `base 3`…
pub fn free_name(model: &Model, base: &str) -> String {
    let taken: Vec<String> = project_profiles(model).into_iter().map(|p| p.name).collect();
    if !taken.iter().any(|t| t == base) && preset(base).is_none() {
        return base.into();
    }
    (2..).map(|n| format!("{base} {n}")).find(|n| !taken.contains(n)).unwrap_or_default()
}

/// The id trace references of profile `name` are shown in (MU37): not an
/// element, a stable key made from its name, as a view's id would be.
pub fn trace_view(name: &str) -> crate::ElementId {
    // FNV-1a, twice over, for 128 bits that stay the same between runs
    let mut h: [u64; 2] = [0xcbf2_9ce4_8422_2325, 0x8422_2325_cbf2_9ce4];
    for (k, seed) in h.iter_mut().enumerate() {
        for b in name.bytes().chain(std::iter::once(k as u8)) {
            *seed ^= b as u64;
            *seed = seed.wrapping_mul(0x0100_0000_01b3);
        }
    }
    crate::ElementId(uuid::Uuid::from_u128(((h[0] as u128) << 64) | h[1] as u128))
}

/// A profile from several shapes at once (MU37: a fill and its frame, a
/// coping and its flashing): the first selected is the main shape, the
/// insertion point the middle of their bottom together.
pub fn from_shapes(model: &Model, ids: &[crate::ElementId]) -> Option<NamedProfile> {
    let raw: Vec<(Vec<DVec2>, bool)> = ids.iter().filter_map(|id| raw_shape(model, *id)).collect();
    let first = raw.first()?;
    let all = raw.iter().flat_map(|(p, _)| p.iter().copied());
    let (lo, hi) = all.fold((DVec2::splat(f64::MAX), DVec2::splat(f64::MIN)), |(a, b), p| (a.min(p), b.max(p)));
    let o = DVec2::new((lo.x + hi.x) / 2.0, lo.y);
    let mv = |v: &Vec<DVec2>| v.iter().map(|p| *p - o).collect::<Vec<_>>();
    Some(NamedProfile {
        name: String::new(),
        points: mv(&first.0),
        open: first.1,
        thickness: None,
        more: raw[1..].iter().map(|(p, open)| ProfileShape { points: mv(p), open: *open, thickness: None }).collect(),
        flips: vec![],
    })
}

fn raw_shape(model: &Model, id: crate::ElementId) -> Option<(Vec<DVec2>, bool)> {
    let e = model.get(id)?;
    let (pts, open): (Vec<DVec2>, bool) = match &e.kind {
        ElementKind::Slab(s) => (s.boundary.clone(), false),
        ElementKind::Part(p) => match &p.form {
            crate::parts::Form::Extrusion { outline, .. } => (outline.clone(), false),
            crate::parts::Form::Sweep { profile, .. } => (profile.clone(), false),
        },
        ElementKind::Sketch(s) => (s.points.clone(), !s.closed),
        _ => return None,
    };
    (pts.len() >= if open { 2 } else { 3 }).then_some((pts, open))
}

/// The outline of a shape to take a profile from: a closed sketch, a
/// slab's, a part's (pulled up from an outline) or a roof's. Moved so its
/// insertion point (the middle of its bottom edge) is the origin.
pub fn from_shape(model: &Model, id: crate::ElementId) -> Option<(Vec<DVec2>, bool)> {
    let e = model.get(id)?;
    let (pts, open): (Vec<DVec2>, bool) = match &e.kind {
        ElementKind::Slab(s) => (s.boundary.clone(), false),
        ElementKind::Part(p) => match &p.form {
            crate::parts::Form::Extrusion { outline, .. } => (outline.clone(), false),
            crate::parts::Form::Sweep { profile, .. } => (profile.clone(), false),
        },
        // a sketch in its own plane, open or closed as drawn
        ElementKind::Sketch(s) => (s.points.clone(), !s.closed),
        _ => return None,
    };
    if pts.len() < if open { 2 } else { 3 } {
        return None;
    }
    let lo = pts.iter().fold(DVec2::splat(f64::MAX), |a, p| a.min(*p));
    let hi = pts.iter().fold(DVec2::splat(f64::MIN), |a, p| a.max(*p));
    let o = DVec2::new((lo.x + hi.x) / 2.0, lo.y);
    Some((pts.into_iter().map(|p| p - o).collect(), open))
}

#[cfg(test)]
mod tests {
    #[test]
    fn overlapping_shapes_make_one_outline_of_their_faces() {
        let mut cw = crate::parts::rectangle(0.025, 0.078);
        for p in &mut cw {
            p.y -= 0.014;
        }
        cw.reverse();
        let p = NamedProfile {
            name: "X".into(),
            points: crate::parts::rectangle(0.1, 0.05),
            more: vec![ProfileShape { points: cw, open: false, thickness: None }],
            ..Default::default()
        };
        let o = p.outline();
        let a = crate::polygon::signed_area2(&o) / 2.0;
        assert!((a.abs() - (0.005 + 0.025 * 0.078 - 2.0 * 0.025 * 0.05)).abs() < 1e-9, "{a}");
    }

    use super::*;

    #[test]
    fn every_preset_is_a_closed_outline_with_an_area() {
        for name in PRESETS {
            let p = preset(name).unwrap_or_else(|| panic!("{name}"));
            assert!(p.len() >= 3, "{name}");
            assert!(crate::polygon::signed_area2(&p).abs() > 1e-7, "{name}");
        }
    }

    #[test]
    fn project_profiles_are_kept_with_the_project_and_found_by_name() {
        let mut doc = crate::Document::new();
        let p = NamedProfile { name: "Sill".into(), points: crate::parts::rectangle(0.2, 0.05), ..Default::default() };
        doc.commit(set_profiles(doc.model(), vec![p.clone()], "Add profile")).unwrap();
        assert_eq!(find(doc.model(), "Sill"), Some(p.points));
        // an open line sweeps as a thin outline
        let t = thicken(&[DVec2::ZERO, DVec2::new(0.1, 0.0), DVec2::new(0.1, 0.1)], 0.02);
        assert_eq!(t.len(), 6);
        assert!(crate::polygon::signed_area2(&t).abs() > 1e-6);
        assert!(find(doc.model(), "Cornice").is_some());
        assert_eq!(free_name(doc.model(), "Sill"), "Sill 2");
        doc.undo();
        assert!(find(doc.model(), "Sill").is_none());
    }
}
