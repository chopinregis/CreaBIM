//! A reference (Trace & Reference v2, MU19): something placed in the
//! project to trace over or compare against, shown faintly under (or over)
//! the views it is shown in. Another view's linework, a picture or a DXF
//! drawing, placed, turned about its own centre and scaled, as Blender's
//! reference images are. It has no body: it is never in 3D, drawings'
//! exports, schedules or IFC.

use crate::ElementId;
use glam::DVec2;
use serde::{Deserialize, Serialize};

/// What a reference shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RefSource {
    /// Another plan or section, drawn at the same place.
    View(ElementId),
    /// A picture file (PNG, JPEG); `width` metres wide at scale 1.
    Image { path: String, width: f64 },
    /// A DXF drawing's lines, in metres from its units.
    Dxf { path: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reference {
    pub source: RefSource,
    /// The views it shows in; one survey can sit under several floors.
    #[serde(default)]
    pub shown_in: Vec<ElementId>,
    /// Where its centre is put, in the views' coordinates.
    #[serde(default)]
    pub at: DVec2,
    /// Turned about its centre, degrees anticlockwise.
    #[serde(default)]
    pub rotation: f64,
    /// Made larger or smaller about its centre (Calibrate sets it).
    #[serde(default = "one")]
    pub scale: f64,
    /// Its colour, by the index of [`REFERENCE_COLOURS`]; none keeps its
    /// own colours (a picture's, a drawing's ink).
    #[serde(default)]
    pub colour: Option<u8>,
    /// 0 (gone) to 1.
    #[serde(default = "strength")]
    pub opacity: f32,
    /// Drawn over the model rather than under it.
    #[serde(default)]
    pub over: bool,
    /// Can't be moved by its grips.
    #[serde(default)]
    pub locked: bool,
    /// Hidden for now (Alt+F2 in a view it shows in).
    #[serde(default)]
    pub hidden: bool,
    /// What it shows spans, in its own coordinates (lower left, upper
    /// right), once known: its middle is what it turns and scales about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<[DVec2; 2]>,
}

fn one() -> f64 {
    1.0
}

fn strength() -> f32 {
    0.45
}

/// The colours a reference can be drawn in: red, blue, grey, green.
pub const REFERENCE_COLOURS: [(&str, [u8; 3]); 4] = [
    ("Red", [214, 64, 52]),
    ("Blue", [52, 104, 196]),
    ("Grey", [120, 124, 130]),
    ("Green", [70, 130, 70]),
];

impl Reference {
    pub fn new(source: RefSource, shown_in: Vec<ElementId>) -> Self {
        Self {
            source,
            shown_in,
            at: DVec2::ZERO,
            rotation: 0.0,
            scale: 1.0,
            colour: Some(0),
            opacity: strength(),
            over: false,
            locked: false,
            hidden: false,
            bounds: None,
        }
    }

    /// Shown where it was drawn: its middle at its own middle, upright,
    /// at its own size.
    pub fn in_place(&mut self, bounds: [DVec2; 2]) {
        self.bounds = Some(bounds);
        self.at = (bounds[0] + bounds[1]) / 2.0;
        self.rotation = 0.0;
        self.scale = 1.0;
    }

    /// The middle it turns and scales about, in its own coordinates.
    pub fn centre(&self) -> DVec2 {
        self.bounds.map_or(DVec2::ZERO, |[a, b]| (a + b) / 2.0)
    }

    /// Its outline's corners where the views show it (lower left, then
    /// anticlockwise), once its bounds are known.
    pub fn outline(&self) -> Option<[DVec2; 4]> {
        let [a, b] = self.bounds?;
        Some([a, DVec2::new(b.x, a.y), b, DVec2::new(a.x, b.y)].map(|p| self.place(p)))
    }

    /// A point of what it shows (its own coordinates) where the views show
    /// it: about its centre, scaled, turned, then put at `at`.
    pub fn place(&self, p: DVec2) -> DVec2 {
        let c = self.centre();
        self.at + DVec2::from_angle(self.rotation.to_radians()).rotate((p - c) * self.scale)
    }

    /// The other way: a view's point in the reference's own coordinates.
    pub fn unplace(&self, q: DVec2) -> DVec2 {
        let c = self.centre();
        c + DVec2::from_angle(-self.rotation.to_radians()).rotate(q - self.at) / self.scale
    }

    /// Scaled so two points picked on it, `a` and `b` where the views
    /// show them, lie `distance` apart; `a` stays where it is (Calibrate,
    /// as FreeCAD BIM's image scaling).
    pub fn calibrate(&mut self, a: DVec2, b: DVec2, distance: f64) -> Result<(), String> {
        let d = a.distance(b);
        if d < 1e-9 || distance <= 0.0 || !distance.is_finite() {
            return Err("Pick two different points and a distance more than zero.".into());
        }
        let own = self.unplace(a);
        self.scale *= distance / d;
        self.at += a - self.place(own);
        Ok(())
    }

    /// Turned and moved so the line `a1`–`a2` picked on it (where the
    /// views show it) lies along `b1`–`b2`, `a1` on `b1`; its size stays
    /// (Align, MU23).
    pub fn align(&mut self, a1: DVec2, a2: DVec2, b1: DVec2, b2: DVec2) -> Result<(), String> {
        let (da, db) = (a2 - a1, b2 - b1);
        if da.length() < 1e-9 || db.length() < 1e-9 {
            return Err("Pick two different points on each line.".into());
        }
        let own = self.unplace(a1);
        let turn = db.y.atan2(db.x) - da.y.atan2(da.x);
        self.rotation = (self.rotation + turn.to_degrees()).rem_euclid(360.0);
        self.at += b1 - self.place(own);
        Ok(())
    }

    pub fn shows_in(&self, view: ElementId) -> bool {
        !self.hidden && self.shown_in.contains(&view)
    }

    /// The elements it needs: the view it shows, if any.
    pub fn refs(&self) -> Vec<ElementId> {
        match &self.source {
            RefSource::View(v) => vec![*v],
            _ => vec![],
        }
    }
}

/// The references shown in `view`, under or over it, in the model's
/// order.
pub fn in_view(model: &crate::Model, view: ElementId) -> Vec<(ElementId, &Reference)> {
    model
        .iter()
        .filter_map(|e| match &e.kind {
            crate::ElementKind::Reference(r) if r.shown_in.contains(&view) => Some((e.id, r)),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reference_turns_and_scales_about_its_centre_and_calibrates() {
        let mut r = Reference::new(
            RefSource::Image {
                path: "plan.png".into(),
                width: 10.0,
            },
            vec![],
        );
        r.in_place([DVec2::new(0.0, 0.0), DVec2::new(10.0, 4.0)]);
        // untouched, it is where it was
        assert!(r.place(DVec2::new(1.0, 1.0)).distance(DVec2::new(1.0, 1.0)) < 1e-12);
        // its centre stays put when it turns
        r.rotation = 90.0;
        assert!(r.place(DVec2::new(5.0, 2.0)).distance(DVec2::new(5.0, 2.0)) < 1e-12);
        let p = DVec2::new(7.0, 2.0);
        assert!(r.place(p).distance(DVec2::new(5.0, 4.0)) < 1e-12);
        assert!(r.unplace(r.place(p)).distance(p) < 1e-12);
        // two points 4 apart made 12 apart: three times as large, the
        // first point staying put
        let a = r.place(DVec2::new(1.0, 1.0));
        let b = r.place(DVec2::new(5.0, 1.0));
        r.calibrate(a, b, 12.0).unwrap();
        assert!((r.scale - 3.0).abs() < 1e-12);
        assert!(r.place(DVec2::new(1.0, 1.0)).distance(a) < 1e-9);
        assert!((r.place(DVec2::new(5.0, 1.0)).distance(a) - 12.0).abs() < 1e-9);
        assert_eq!(r.outline().unwrap().len(), 4);
        // a line on it laid along another: its first point lands on the
        // other's, and it turns to follow it
        let (a1, a2) = (r.place(DVec2::new(1.0, 1.0)), r.place(DVec2::new(3.0, 1.0)));
        let (b1, b2) = (DVec2::new(20.0, 5.0), DVec2::new(20.0, 9.0));
        r.align(a1, a2, b1, b2).unwrap();
        assert!(r.place(DVec2::new(1.0, 1.0)).distance(b1) < 1e-9);
        let d = r.place(DVec2::new(3.0, 1.0)) - b1;
        assert!(d.x.abs() < 1e-9 && d.y > 0.0, "{d}");
    }
}
