//! Sheet templates: a named title block (and, if it has one, its own
//! border) that any number of sheets share. A template's linework and text
//! are in paper millimetres measured from one corner of the sheet, so the
//! same template fits every paper size. Text can hold fields in braces,
//! `{sheet}`, `{number}`, `{project}`, `{scale}`, `{date}` or a field of
//! the template's own, filled in per sheet.
//!
//! Templates are elements like text styles: saved, undone and listed in
//! the model. A sheet without one draws the built-in title block.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use glam::DVec2;

/// The sheet corner a template's coordinates are measured from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Corner {
    BottomLeft,
    /// Where title blocks usually sit.
    #[default]
    BottomRight,
    TopLeft,
    TopRight,
}

impl Corner {
    pub const ALL: [Corner; 4] = [
        Corner::BottomLeft,
        Corner::BottomRight,
        Corner::TopLeft,
        Corner::TopRight,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Corner::BottomLeft => "Bottom left",
            Corner::BottomRight => "Bottom right",
            Corner::TopLeft => "Top left",
            Corner::TopRight => "Top right",
        }
    }

    /// The corner of a `width` × `height` sheet, mm from its lower left.
    pub fn at(self, width: f64, height: f64) -> DVec2 {
        match self {
            Corner::BottomLeft => DVec2::ZERO,
            Corner::BottomRight => DVec2::new(width, 0.0),
            Corner::TopLeft => DVec2::new(0.0, height),
            Corner::TopRight => DVec2::new(width, height),
        }
    }
}

/// How a piece of title block text lines up with its point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TextAlign {
    #[default]
    Start,
    Middle,
    End,
}

/// One piece of a title block, in paper mm from the template's corner.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TitleItem {
    /// An open polyline (closed when its last point is its first).
    Line {
        points: Vec<DVec2>,
        /// Pen width, mm.
        width: f64,
    },
    /// A filled polygon; 0 is black, 1 white.
    Fill { points: Vec<DVec2>, gray: f64 },
    /// Text centred vertically on `at`; may hold `{field}`s.
    Text {
        at: DVec2,
        /// Letter height, mm.
        size: f64,
        text: String,
        #[serde(default)]
        align: TextAlign,
        /// Radians counter-clockwise.
        #[serde(default)]
        angle: f64,
    },
    /// A drawn shape in colour (MU40): an outline, a filled face, or both,
    /// see-through by `opacity`.
    Shape {
        points: Vec<DVec2>,
        #[serde(default)]
        closed: bool,
        /// Its pen, mm; none draws no outline.
        #[serde(default)]
        pen: Option<f64>,
        /// The pen's colour, sRGB 0..1.
        #[serde(default)]
        colour: [f32; 3],
        /// Its fill colour; none leaves it empty.
        #[serde(default)]
        fill: Option<[f32; 3]>,
        /// 1 solid, 0 gone.
        #[serde(default = "one")]
        opacity: f64,
    },
    /// A picture (a logo, a stamp, a background), spanning `lo`..`hi` mm:
    /// its pixels kept in the master, RGBA rows from the top, deflated and
    /// in base64, so the project carries it wherever it goes (MU40).
    Image {
        lo: DVec2,
        hi: DVec2,
        width: u32,
        height: u32,
        rgba: String,
    },
}

fn one() -> f64 {
    1.0
}

impl TitleItem {
    /// Its points, mm from the template's corner (a text's place, a
    /// picture's two corners).
    pub fn points(&self) -> Vec<DVec2> {
        match self {
            TitleItem::Line { points, .. }
            | TitleItem::Fill { points, .. }
            | TitleItem::Shape { points, .. } => points.clone(),
            TitleItem::Text { at, .. } => vec![*at],
            TitleItem::Image { lo, hi, .. } => vec![*lo, *hi],
        }
    }

    /// Move it by `d` mm.
    pub fn shift(&mut self, d: DVec2) {
        match self {
            TitleItem::Line { points, .. }
            | TitleItem::Fill { points, .. }
            | TitleItem::Shape { points, .. } => points.iter_mut().for_each(|p| *p += d),
            TitleItem::Text { at, .. } => *at += d,
            TitleItem::Image { lo, hi, .. } => {
                *lo += d;
                *hi += d;
            }
        }
    }
}

/// A named, reusable title block. The element's name is the template's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetTemplate {
    #[serde(default)]
    pub corner: Corner,
    pub items: Vec<TitleItem>,
    /// Draw the usual border inside the sheet's margins as well. Off for
    /// a template that brings its own frame.
    #[serde(default = "yes")]
    pub border: bool,
    /// Fields of its own (client, drawn by, revision…) and the value a
    /// sheet shows until it is given one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<(String, String)>,
    /// The paper size new sheets made from it get, mm; none keeps A3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper: Option<DVec2>,
}

fn yes() -> bool {
    true
}

/// The fields every sheet fills in itself.
pub const BUILT_IN_FIELDS: [(&str, &str); 5] = [
    ("sheet", "The sheet's title"),
    ("number", "The sheet number"),
    ("project", "The project name"),
    ("scale", "The scale of its views"),
    ("date", "Today's date"),
];

impl SheetTemplate {
    pub fn validate(&self) -> Result<(), String> {
        let finite = |p: &DVec2| p.is_finite();
        for it in &self.items {
            let ok = match it {
                TitleItem::Line { points, width } => {
                    points.iter().all(finite) && width.is_finite() && *width >= 0.0
                }
                TitleItem::Fill { points, gray } => {
                    points.iter().all(finite) && (0.0..=1.0).contains(gray)
                }
                TitleItem::Text {
                    at, size, angle, ..
                } => at.is_finite() && size.is_finite() && *size > 0.0 && angle.is_finite(),
                TitleItem::Shape {
                    points,
                    pen,
                    opacity,
                    ..
                } => {
                    points.iter().all(finite)
                        && pen.is_none_or(|w| w.is_finite() && w >= 0.0)
                        && (0.0..=1.0).contains(opacity)
                }
                TitleItem::Image {
                    lo,
                    hi,
                    width,
                    height,
                    ..
                } => lo.is_finite() && hi.is_finite() && *width > 0 && *height > 0,
            };
            if !ok {
                return Err("title block items must be finite, with positive text sizes".into());
            }
        }
        if self
            .paper
            .is_some_and(|p| !(p.is_finite() && p.min_element() > 10.0))
        {
            return Err("a template's paper must be over 10 mm each way".into());
        }
        Ok(())
    }

    /// The corners of everything in it, from its corner, mm.
    pub fn bounds(&self) -> Option<(DVec2, DVec2)> {
        let mut pts = vec![];
        for it in &self.items {
            match it {
                TitleItem::Text { at, size, .. } => pts.push(*at + DVec2::Y * size * 0.5),
                _ => pts.extend(it.points()),
            }
        }
        let first = *pts.first()?;
        Some(
            pts.iter()
                .fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))),
        )
    }

    /// Move everything so its outline's far corner sits on the template's
    /// corner: imported drawings come at any position, and a title block
    /// belongs in its corner.
    pub fn fit_to_corner(&mut self) {
        let Some((lo, hi)) = self.bounds() else {
            return;
        };
        let target = match self.corner {
            Corner::BottomLeft => lo,
            Corner::BottomRight => DVec2::new(hi.x, lo.y),
            Corner::TopLeft => DVec2::new(lo.x, hi.y),
            Corner::TopRight => hi,
        };
        let shift = -target;
        for it in &mut self.items {
            it.shift(shift);
        }
    }

    /// The names of the fields its text uses, in order, once each.
    pub fn used_fields(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for it in &self.items {
            if let TitleItem::Text { text, .. } = it {
                for f in fields_in(text) {
                    if !out.contains(&f) {
                        out.push(f);
                    }
                }
            }
        }
        out
    }
}

/// The masters a project can start from (MU40): the Standard strip down
/// the right, and the Minimal band along the foot. A3, from the bottom
/// right corner.
pub fn styles() -> Vec<(&'static str, SheetTemplate)> {
    vec![
        ("Standard A3", standard_strip()),
        ("Minimal A3", minimal_band()),
    ]
}

fn rect(lo: DVec2, hi: DVec2, width: f64) -> TitleItem {
    TitleItem::Line {
        points: vec![lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y), lo],
        width,
    }
}

fn rule(a: DVec2, b: DVec2) -> TitleItem {
    TitleItem::Line {
        points: vec![a, b],
        width: 0.18,
    }
}

fn text(at: DVec2, size: f64, t: &str, align: TextAlign) -> TitleItem {
    TitleItem::Text {
        at,
        size,
        text: t.to_string(),
        align,
        angle: 0.0,
    }
}

/// CreaBIM's woven mark, `s` mm across, centred on `c`: two threads each
/// way, over and under, in a ring.
pub fn woven_mark(c: DVec2, s: f64) -> Vec<TitleItem> {
    let r = s / 2.0;
    let ring: Vec<DVec2> = (0..=32)
        .map(|k| c + DVec2::from_angle(k as f64 / 32.0 * std::f64::consts::TAU) * r)
        .collect();
    let mut out = vec![TitleItem::Line {
        points: ring,
        width: 0.25,
    }];
    let t = r * 0.42;
    for k in [-1.0, 1.0] {
        // across, broken where it passes under
        out.push(TitleItem::Line {
            points: vec![
                c + DVec2::new(-r * 0.8, k * t),
                c + DVec2::new(-t * 0.35, k * t),
            ],
            width: 0.35,
        });
        out.push(TitleItem::Line {
            points: vec![
                c + DVec2::new(t * 0.35, k * t),
                c + DVec2::new(r * 0.8, k * t),
            ],
            width: 0.35,
        });
        // up, whole
        out.push(TitleItem::Line {
            points: vec![
                c + DVec2::new(k * t, -r * 0.8),
                c + DVec2::new(k * t, r * 0.8),
            ],
            width: 0.35,
        });
    }
    out
}

/// The Standard master: a strip down the right of an A3 sheet. The
/// project's name has the head of it to itself; the mark signs the foot,
/// small, beside "Drawn in CreaBIM" (MU40: the mark fought the name).
pub fn standard_strip() -> SheetTemplate {
    use TextAlign::{End, Start};
    let (l, r) = (-76.0, -10.0);
    let x = l + 3.0;
    let mut items = vec![
        rect(DVec2::new(l, 10.0), DVec2::new(r, 287.0), 0.35),
        text(DVec2::new(x, 282.0), 2.0, "PROJECT", Start),
        text(DVec2::new(x, 274.0), 4.5, "{project}", Start),
        text(DVec2::new(x, 267.0), 2.5, "{client}", Start),
        rule(DVec2::new(l, 262.0), DVec2::new(r, 262.0)),
        text(DVec2::new(x, 257.0), 2.0, "DRAWING", Start),
        text(DVec2::new(x, 250.0), 4.0, "{sheet}", Start),
        rule(DVec2::new(l, 80.0), DVec2::new(r, 80.0)),
        text(DVec2::new(x, 75.0), 2.0, "REV", Start),
        text(DVec2::new(l + 14.0, 75.0), 2.0, "DATE", Start),
        text(DVec2::new(l + 34.0, 75.0), 2.0, "DESCRIPTION", Start),
        rule(DVec2::new(l, 71.0), DVec2::new(r, 71.0)),
        rule(DVec2::new(l, 60.0), DVec2::new(r, 60.0)),
        text(DVec2::new(x, 55.0), 2.0, "SCALE", Start),
        text(DVec2::new(x, 47.5), 3.5, "{scale}", Start),
        text(DVec2::new(l + 36.0, 55.0), 2.0, "DATE", Start),
        text(DVec2::new(l + 36.0, 47.5), 3.0, "{date}", Start),
        rule(DVec2::new(l + 33.0, 40.0), DVec2::new(l + 33.0, 60.0)),
        rule(DVec2::new(l, 40.0), DVec2::new(r, 40.0)),
        text(DVec2::new(x, 35.0), 2.0, "SHEET", Start),
        text(DVec2::new(x, 25.0), 9.0, "{number}", Start),
        text(DVec2::new(r - 3.0, 35.0), 2.0, "REV", End),
        text(DVec2::new(r - 3.0, 25.0), 4.0, "{rev}", End),
        rule(DVec2::new(l, 17.0), DVec2::new(r, 17.0)),
        text(DVec2::new(l + 9.0, 13.5), 2.0, "Drawn in CreaBIM", Start),
    ];
    items.extend(woven_mark(DVec2::new(l + 5.0, 13.5), 4.6));
    SheetTemplate {
        corner: Corner::BottomRight,
        items,
        border: true,
        fields: vec![("client".into(), String::new()), ("rev".into(), "-".into())],
        paper: Some(DVec2::new(420.0, 297.0)),
    }
}

/// The Minimal master: one band along the foot of an A3 sheet.
pub fn minimal_band() -> SheetTemplate {
    use TextAlign::{End, Start};
    let (l, r, lo, hi) = (-390.0, -10.0, 10.0, 24.0);
    let y = (lo + hi) / 2.0;
    let mut items = vec![rect(DVec2::new(l, lo), DVec2::new(r, hi), 0.35)];
    for x in [-240.0, -130.0, -90.0, -40.0] {
        items.push(rule(DVec2::new(x, lo), DVec2::new(x, hi)));
    }
    items.extend([
        text(DVec2::new(l + 4.0, y), 4.0, "{project}", Start),
        text(DVec2::new(-236.0, y), 3.0, "{sheet}", Start),
        text(DVec2::new(-126.0, y), 2.5, "{scale}", Start),
        text(DVec2::new(-86.0, y), 2.5, "{date}", Start),
        text(DVec2::new(r - 4.0, y), 6.0, "{number}", End),
    ]);
    SheetTemplate {
        corner: Corner::BottomRight,
        items,
        border: true,
        fields: vec![],
        paper: Some(DVec2::new(420.0, 297.0)),
    }
}

/// The `{name}`s in `text`.
pub fn fields_in(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut rest = text;
    while let Some(i) = rest.find('{') {
        let after = &rest[i + 1..];
        let Some(j) = after.find('}') else { break };
        let name = after[..j].trim();
        if !name.is_empty() && !name.contains('{') {
            out.push(name.to_string());
        }
        rest = &after[j + 1..];
    }
    out
}

/// `text` with each `{name}` replaced by its value; unknown fields are
/// left as written so a missing value shows.
pub fn fill_fields(text: &str, values: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('}') {
            Some(j) => {
                let name = after[..j].trim();
                match values.get(name) {
                    Some(v) => out.push_str(v),
                    None => {
                        out.push('{');
                        out.push_str(&after[..=j]);
                    }
                }
                rest = &after[j + 1..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, Element, ElementKind, Sheet, Transaction};

    fn block() -> SheetTemplate {
        SheetTemplate {
            corner: Corner::BottomRight,
            items: vec![
                TitleItem::Line {
                    points: vec![
                        DVec2::new(-120.0, 10.0),
                        DVec2::new(-10.0, 10.0),
                        DVec2::new(-10.0, 40.0),
                        DVec2::new(-120.0, 40.0),
                        DVec2::new(-120.0, 10.0),
                    ],
                    width: 0.35,
                },
                TitleItem::Text {
                    at: DVec2::new(-115.0, 30.0),
                    size: 5.0,
                    text: "{sheet} for {client}".into(),
                    align: TextAlign::Start,
                    angle: 0.0,
                },
            ],
            border: true,
            fields: vec![("client".into(), "Client name".into())],
            paper: None,
        }
    }

    #[test]
    fn fields_fill_in_and_unknown_ones_show() {
        let mut v = BTreeMap::new();
        v.insert("sheet".to_string(), "Ground floor".to_string());
        assert_eq!(
            fill_fields("{sheet} for {client}", &v),
            "Ground floor for {client}"
        );
        v.insert("client".to_string(), "Ana".to_string());
        assert_eq!(
            fill_fields("{ sheet } / {client}", &v),
            "Ground floor / Ana"
        );
        assert_eq!(fill_fields("a { b", &v), "a { b");
        assert_eq!(block().used_fields(), vec!["sheet", "client"]);
    }

    #[test]
    fn templates_round_trip_and_sheets_from_older_files_have_none() {
        let mut doc = Document::new();
        let t = Element::new("Office A3", ElementKind::SheetTemplate(block()));
        let tid = t.id;
        let mut sheet = Sheet::a("A101", "A3");
        sheet.template = Some(tid);
        sheet.fields.insert("client".into(), "Ana".into());
        let s = Element::new("Plans", ElementKind::Sheet(sheet.clone()));
        doc.commit(Transaction::new("Template").insert(t).insert(s))
            .unwrap();
        let text = serde_json::to_string(doc.model()).unwrap();
        let back: crate::Model = serde_json::from_str(&text).unwrap();
        assert_eq!(
            back.get(tid).unwrap().kind,
            ElementKind::SheetTemplate(block())
        );
        // a sheet from before templates reads with none, and writes none
        let old: Sheet =
            serde_json::from_str(r#"{"number":"A1","width":420.0,"height":297.0}"#).unwrap();
        assert_eq!(old, Sheet::a("A1", "A3"));
        let text = serde_json::to_string(&old).unwrap();
        assert!(
            !text.contains("template") && !text.contains("fields"),
            "{text}"
        );
        // a template without a border flag draws the border
        let t: SheetTemplate = serde_json::from_str(r#"{"items":[]}"#).unwrap();
        assert!(t.border);
    }

    #[test]
    fn the_masters_to_start_from_are_valid_and_fit_their_paper() {
        for (name, t) in styles() {
            assert!(t.validate().is_ok(), "{name}");
            let (lo, hi) = t.bounds().unwrap();
            let paper = t.paper.unwrap();
            // from the bottom right corner: inside the sheet's margins
            assert!(hi.x <= -9.9 && lo.x >= -paper.x + 9.9, "{name}: {lo} {hi}");
            assert!(lo.y >= 9.9 && hi.y <= paper.y - 9.9, "{name}: {lo} {hi}");
            let used = t.used_fields();
            for f in ["project", "sheet", "number", "scale"] {
                assert!(used.iter().any(|u| u == f), "{name} has no {{{f}}}");
            }
        }
    }

    #[test]
    fn imported_blocks_move_into_their_corner() {
        let mut t = block();
        t.fit_to_corner();
        let (lo, hi) = t.bounds().unwrap();
        assert!(hi.x.abs() < 1e-9 && lo.y.abs() < 1e-9, "{lo} {hi}");
        let mut bad = block();
        bad.items.push(TitleItem::Text {
            at: DVec2::ZERO,
            size: 0.0,
            text: "x".into(),
            align: TextAlign::Start,
            angle: 0.0,
        });
        assert!(bad.validate().is_err());
    }
}
