//! Detail components: 2D drafting for what the model does not show, drawn
//! in one view (most often a detail view, see [`crate::element::Callout`])
//! over the drawing the model gives it. A flashing, a damp-proof membrane,
//! a quilt of insulation, blocking, brick courses, a hatched region and a
//! break line. They are annotation: they are never in 3D, in schedules or
//! exported as building elements.
//!
//! Each component is a path of points in its view's drawing coordinates
//! (metres along and up a section, or across a plan) with a width across
//! it; how each kind is drawn lives in `creabim-geom`'s `detail` module.

use crate::{ElementId, Hatch};
use glam::DVec2;
use serde::{Deserialize, Serialize};

/// What a detail component is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum DetailKind {
    /// Soft insulation (a batt or quilt): a looping line filling a band
    /// `width` wide along the path.
    #[default]
    Batt,
    /// A membrane, damp-proof course or vapour control layer: a heavy
    /// line along the path.
    Membrane,
    /// A flashing or sheet-metal trim: a solid band `width` thick along
    /// the path.
    Flashing,
    /// Blocking or a timber seen in section: each stretch of the path is a
    /// piece `width` deep, crossed corner to corner.
    Blocking,
    /// A repeating detail (brick or block courses): units `module` long
    /// and `width` deep laid along the path with `joint` between them.
    Repeat { module: f64, joint: f64 },
    /// An area filled with a hatch (concrete, hardcore, earth), the path
    /// its outline.
    Region { hatch: Hatch },
    /// A break line across the path: where a drawing stops short.
    BreakLine,
}

impl DetailKind {
    /// The kinds the detail tool offers, with a repeat laid as brick.
    pub const ALL: [DetailKind; 7] = [
        DetailKind::Batt,
        DetailKind::Membrane,
        DetailKind::Flashing,
        DetailKind::Blocking,
        DetailKind::Repeat {
            module: 0.215,
            joint: 0.01,
        },
        DetailKind::Region {
            hatch: Hatch::Concrete,
        },
        DetailKind::BreakLine,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DetailKind::Batt => "Insulation batt",
            DetailKind::Membrane => "Membrane",
            DetailKind::Flashing => "Flashing",
            DetailKind::Blocking => "Blocking",
            DetailKind::Repeat { .. } => "Repeating detail",
            DetailKind::Region { .. } => "Filled region",
            DetailKind::BreakLine => "Break line",
        }
    }

    /// Is it the same kind, whatever its settings?
    pub fn same(self, other: DetailKind) -> bool {
        std::mem::discriminant(&self) == std::mem::discriminant(&other)
    }

    /// The width it is drawn when first placed, in metres.
    pub fn default_width(self) -> f64 {
        match self {
            DetailKind::Batt => 0.1,
            DetailKind::Membrane => 0.0,
            DetailKind::Flashing => 0.003,
            DetailKind::Blocking => 0.05,
            DetailKind::Repeat { .. } => 0.065,
            DetailKind::Region { .. } => 0.0,
            DetailKind::BreakLine => 0.0,
        }
    }

    /// Is the path closed into an outline?
    pub fn is_area(self) -> bool {
        matches!(self, DetailKind::Region { .. })
    }
}

/// One detail component in a view.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DetailItem {
    /// The view it is drawn in.
    pub view: ElementId,
    pub kind: DetailKind,
    /// Its path, in the view's drawing coordinates.
    pub points: Vec<DVec2>,
    /// Across the path, metres: a batt's thickness, a course's depth. The
    /// path runs along the middle of it.
    #[serde(default)]
    pub width: f64,
    /// A membrane lies to the right of the way it was drawn, not the left:
    /// the side of the line it was clicked on.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
}

/// How a membrane is drawn beside the line it follows (style pack,
/// Annotation): always a heavy line 2 mm off it on paper, so it never lies
/// on that line, with a partner further out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MembraneLook {
    /// A thin dashed partner.
    #[default]
    Dashed,
    /// A second, lighter solid line.
    Double,
    /// A zig-zag between the two.
    ZigZag,
}

impl MembraneLook {
    pub const ALL: [MembraneLook; 3] = [
        MembraneLook::Dashed,
        MembraneLook::Double,
        MembraneLook::ZigZag,
    ];

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn label(self) -> &'static str {
        match self {
            MembraneLook::Dashed => "Heavy line, dashed partner",
            MembraneLook::Double => "Double line",
            MembraneLook::ZigZag => "Zig-zag",
        }
    }
}

impl DetailItem {
    pub fn new(view: ElementId, kind: DetailKind, points: Vec<DVec2>) -> Self {
        Self {
            view,
            kind,
            points,
            width: kind.default_width(),
            flip: false,
        }
    }

    /// The side a membrane is drawn on: +1 left of the way it was drawn,
    /// -1 right.
    pub fn side(&self) -> f64 {
        if self.flip {
            -1.0
        } else {
            1.0
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let need = if self.kind.is_area() { 3 } else { 2 };
        if self.points.len() < need {
            return Err(format!(
                "a {} needs at least {need} points",
                self.kind.label().to_lowercase()
            ));
        }
        if !self.points.iter().all(|p| p.is_finite()) || !self.width.is_finite() {
            return Err("detail points must be finite".into());
        }
        if self.width < 0.0 {
            return Err("a detail's width can't be negative".into());
        }
        if let DetailKind::Repeat { module, joint } = self.kind {
            if !(module > 1e-3 && joint >= 0.0) {
                return Err("a repeating detail needs a unit length".into());
            }
        }
        Ok(())
    }

    /// The path's length.
    pub fn length(&self) -> f64 {
        self.points.windows(2).map(|w| w[0].distance(w[1])).sum()
    }
}

/// How a view's drawing coordinates stand in the model: a plan lies level
/// at its level's height, a section (or a detail called out of one)
/// stands on its line, x along it and y up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrawingFrame {
    Plan { z: f64 },
    Section { start: DVec2, end: DVec2 },
}

impl DrawingFrame {
    /// The frame of view `view`, if it is a plan or a section.
    pub fn of(model: &crate::Model, view: ElementId) -> Option<DrawingFrame> {
        let crate::ElementKind::View(v) = &model.get(view)?.kind else {
            return None;
        };
        match v.kind {
            crate::ViewKind::Plan { level, .. } => Some(DrawingFrame::Plan {
                z: model.level(level).map_or(0.0, |l| l.elevation),
            }),
            crate::ViewKind::Section { start, end, .. } => {
                Some(DrawingFrame::Section { start, end })
            }
            _ => None,
        }
    }

    /// A drawing point in the model.
    pub fn to_model(self, p: DVec2) -> glam::DVec3 {
        match self {
            DrawingFrame::Plan { z } => p.extend(z),
            DrawingFrame::Section { start, end } => {
                (start + (end - start).normalize_or_zero() * p.x).extend(p.y)
            }
        }
    }

    /// A model point on the drawing (dropped square onto its plane).
    pub fn to_drawing(self, p: glam::DVec3) -> DVec2 {
        match self {
            DrawingFrame::Plan { .. } => p.truncate(),
            DrawingFrame::Section { start, end } => DVec2::new(
                (p.truncate() - start).dot((end - start).normalize_or_zero()),
                p.z,
            ),
        }
    }

    /// Square to the drawing: up out of a plan, the way a section looks.
    pub fn normal(self) -> glam::DVec3 {
        match self {
            DrawingFrame::Plan { .. } => glam::DVec3::Z,
            DrawingFrame::Section { start, end } => {
                (end - start).normalize_or_zero().perp().extend(0.0)
            }
        }
    }
}

impl DetailItem {
    /// Its points in the model, in its view's frame.
    pub fn model_points(&self, frame: DrawingFrame) -> Vec<glam::DVec3> {
        self.points.iter().map(|p| frame.to_model(*p)).collect()
    }

    /// The direction across its first stretch (left of the way it was
    /// drawn), on the drawing; the side its width grip stands.
    pub fn across(&self) -> DVec2 {
        match self.points.as_slice() {
            [a, b, ..] => (*b - *a).normalize_or_zero().perp(),
            _ => DVec2::Y,
        }
    }

    /// A repeating detail's unit and joint, from one unit's start to the
    /// next: where its spacing grip sits along the path.
    pub fn spacing(&self) -> Option<f64> {
        match self.kind {
            DetailKind::Repeat { module, joint } => Some(module + joint),
            _ => None,
        }
    }

    /// The point `d` along the path.
    pub fn at_length(&self, d: f64) -> DVec2 {
        let mut left = d;
        for w in self.points.windows(2) {
            let l = w[0].distance(w[1]);
            if left <= l || l <= 0.0 {
                return w[0] + (w[1] - w[0]).normalize_or_zero() * left.max(0.0);
            }
            left -= l;
        }
        self.points.last().copied().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_check_their_paths() {
        let v = ElementId::new();
        let p = |x, y| DVec2::new(x, y);
        assert!(DetailItem::new(v, DetailKind::Batt, vec![p(0.0, 0.0)])
            .validate()
            .is_err());
        assert!(
            DetailItem::new(v, DetailKind::Batt, vec![p(0.0, 0.0), p(1.0, 0.0)])
                .validate()
                .is_ok()
        );
        let region = DetailKind::Region {
            hatch: Hatch::Concrete,
        };
        assert!(DetailItem::new(v, region, vec![p(0.0, 0.0), p(1.0, 0.0)])
            .validate()
            .is_err());
        // a saved detail without a width still reads
        let json = format!(
            r#"{{"view":{},"kind":"Membrane","points":[[0.0,0.0],[1.0,0.0]]}}"#,
            serde_json::to_string(&v).unwrap()
        );
        let d: DetailItem = serde_json::from_str(&json).unwrap();
        assert_eq!(d.width, 0.0);
    }
}
