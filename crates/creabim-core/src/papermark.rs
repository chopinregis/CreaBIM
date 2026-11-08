//! Paper marks (MU22): what belongs to a sheet rather than to a view. A
//! north arrow, turned to the project's north, and a scale bar for a
//! drawing scale, placed in sheet millimetres. Text on a sheet is a
//! [`crate::Note`] written on it.

use crate::ElementId;
use glam::DVec2;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PaperMarkKind {
    /// Points to north: the project's true north when `rotation` is none,
    /// else turned this many degrees anticlockwise from the sheet's up.
    NorthArrow {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rotation: Option<f64>,
    },
    /// A bar `length` metres long at 1:`scale`, in `parts` alternating
    /// black and white parts, numbered in metres.
    ScaleBar { scale: f64, length: f64, parts: u8 },
    /// A revision cloud round a change, `w` by `h` mm about `at`, with its
    /// revision's letter, date and what changed (Annotate board step 4).
    Cloud {
        w: f64,
        h: f64,
        rev: String,
        #[serde(default)]
        date: String,
        #[serde(default)]
        what: String,
    },
    /// The sheet's revisions: one row per revision cloud on it, from `at`
    /// (its lower left), `size` mm text.
    RevisionTable,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaperMark {
    /// The sheet it is on.
    pub sheet: ElementId,
    /// Where it is, sheet millimetres from the lower left: a north arrow's
    /// middle, a scale bar's left end.
    pub at: DVec2,
    /// A north arrow's height, a scale bar's thickness, millimetres.
    pub size: f64,
    pub kind: PaperMarkKind,
}

impl PaperMark {
    pub fn north_arrow(sheet: ElementId, at: DVec2) -> Self {
        Self {
            sheet,
            at,
            size: 18.0,
            kind: PaperMarkKind::NorthArrow { rotation: None },
        }
    }

    /// A scale bar for 1:`scale`, its length a round number of metres
    /// about 60 mm long on paper.
    pub fn scale_bar(sheet: ElementId, at: DVec2, scale: f64) -> Self {
        let scale = if scale > 0.0 { scale } else { 100.0 };
        Self {
            sheet,
            at,
            size: 2.0,
            kind: PaperMarkKind::ScaleBar {
                scale,
                length: round_length(0.06 * scale),
                parts: 5,
            },
        }
    }

    /// A revision cloud `w` by `h` about `at`.
    pub fn cloud(sheet: ElementId, at: DVec2, rev: &str) -> Self {
        Self {
            sheet,
            at,
            size: 4.0,
            kind: PaperMarkKind::Cloud {
                w: 60.0,
                h: 40.0,
                rev: rev.into(),
                date: String::new(),
                what: String::new(),
            },
        }
    }

    /// The revisions table, from `at`.
    pub fn revision_table(sheet: ElementId, at: DVec2) -> Self {
        Self {
            sheet,
            at,
            size: 2.5,
            kind: PaperMarkKind::RevisionTable,
        }
    }

    /// Its extent on the sheet, millimetres (lower left, upper right).
    pub fn bounds(&self) -> (DVec2, DVec2) {
        match &self.kind {
            PaperMarkKind::Cloud { w, h, .. } => {
                let half = DVec2::new(w / 2.0, h / 2.0) + DVec2::splat(self.size);
                (self.at - half, self.at + half + DVec2::new(8.0, 8.0))
            }
            PaperMarkKind::RevisionTable => {
                (self.at, self.at + DVec2::new(110.0, self.size * 2.4 * 5.0))
            }
            PaperMarkKind::NorthArrow { .. } => {
                let r = self.size * 0.6;
                (self.at - DVec2::splat(r), self.at + DVec2::new(r, r + 4.0))
            }
            PaperMarkKind::ScaleBar { scale, length, .. } => {
                let w = length * 1000.0 / scale.max(1.0);
                (
                    self.at - DVec2::new(1.0, 5.0),
                    self.at + DVec2::new(w + 8.0, self.size + 5.0),
                )
            }
        }
    }

    /// The angle a north arrow points, degrees anticlockwise from up.
    pub fn north_angle(&self, true_north: f64) -> f64 {
        match self.kind {
            PaperMarkKind::NorthArrow { rotation } => rotation.unwrap_or(true_north),
            _ => 0.0,
        }
    }
}

/// A length as 1, 2 or 5 times a power of ten, the nearest at or under it.
pub fn round_length(m: f64) -> f64 {
    if m <= 0.0 || !m.is_finite() {
        return 1.0;
    }
    let p = 10f64.powf(m.log10().floor());
    let f = m / p;
    let step = if f >= 5.0 {
        5.0
    } else if f >= 2.0 {
        2.0
    } else {
        1.0
    };
    step * p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scale_bar_is_a_round_length_about_six_centimetres_long() {
        let m = PaperMark::scale_bar(ElementId::new(), DVec2::ZERO, 100.0);
        let PaperMarkKind::ScaleBar { length, .. } = m.kind else {
            panic!()
        };
        assert_eq!(length, 5.0);
        assert_eq!(round_length(12.0), 10.0);
        assert_eq!(round_length(3.0), 2.0);
        let (lo, hi) = m.bounds();
        assert!(hi.x - lo.x > 50.0 && hi.x - lo.x < 70.0);
    }
}
