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
        .map(|p| p.points)
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

/// A name not yet taken: `base`, else `base 2`, `base 3`…
pub fn free_name(model: &Model, base: &str) -> String {
    let taken: Vec<String> = project_profiles(model).into_iter().map(|p| p.name).collect();
    if !taken.iter().any(|t| t == base) && preset(base).is_none() {
        return base.into();
    }
    (2..).map(|n| format!("{base} {n}")).find(|n| !taken.contains(n)).unwrap_or_default()
}

/// The outline of a shape to take a profile from: a closed sketch, a
/// slab's, a part's (pulled up from an outline) or a roof's. Moved so its
/// insertion point (the middle of its bottom edge) is the origin.
pub fn from_shape(model: &Model, id: crate::ElementId) -> Option<Vec<DVec2>> {
    let e = model.get(id)?;
    let pts: Vec<DVec2> = match &e.kind {
        ElementKind::Slab(s) => s.boundary.clone(),
        ElementKind::Part(p) => match &p.form {
            crate::parts::Form::Extrusion { outline, .. } => outline.clone(),
            crate::parts::Form::Sweep { profile, .. } => profile.clone(),
        },
        ElementKind::Sketch(s) => {
            let pts = s.points.clone();
            if pts.len() < 3 {
                return None;
            }
            pts
        }
        _ => return None,
    };
    if pts.len() < 3 {
        return None;
    }
    let lo = pts.iter().fold(DVec2::splat(f64::MAX), |a, p| a.min(*p));
    let hi = pts.iter().fold(DVec2::splat(f64::MIN), |a, p| a.max(*p));
    let o = DVec2::new((lo.x + hi.x) / 2.0, lo.y);
    Some(pts.into_iter().map(|p| p - o).collect())
}

#[cfg(test)]
mod tests {
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
        let p = NamedProfile { name: "Sill".into(), points: crate::parts::rectangle(0.2, 0.05) };
        doc.commit(set_profiles(doc.model(), vec![p.clone()], "Add profile")).unwrap();
        assert_eq!(find(doc.model(), "Sill"), Some(p.points));
        assert!(find(doc.model(), "Cornice").is_some());
        assert_eq!(free_name(doc.model(), "Sill"), "Sill 2");
        doc.undo();
        assert!(find(doc.model(), "Sill").is_none());
    }
}
