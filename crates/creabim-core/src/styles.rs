//! Graphic styles as data: how each category of element is drawn (object
//! styles), the hatch patterns cut faces and surfaces are filled with, the
//! pen widths each scale draws with, view templates and per-view overrides.
//!
//! All of it travels as one [`StylePack`], a JSON file an office keeps its
//! drawing standard in. CreaBIM names no standard of its own: the look it
//! draws with when a project has no pack is the *Standard* pack shipped in
//! `packs/standard.json`, read like any other.

use crate::annotation::TextStyle;
use crate::graphics::LineWeights;
use crate::marks::MarkRules;
use crate::titleblock::SheetTemplate;
use crate::visibility::Category;
use crate::{Element, ElementId, ElementKind, Hatch, Model, Transaction, View, ViewStyle};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// An sRGB colour, 0..1. Packs write it as `"#rrggbb"`; a list of three
/// numbers reads too.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub [f32; 3]);

impl Rgb {
    pub fn hex(self) -> String {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!(
            "#{:02x}{:02x}{:02x}",
            c(self.0[0]),
            c(self.0[1]),
            c(self.0[2])
        )
    }

    pub fn parse(s: &str) -> Option<Self> {
        let h = s.strip_prefix('#')?;
        if h.len() != 6 {
            return None;
        }
        let v = |i: usize| {
            u8::from_str_radix(&h[i..i + 2], 16)
                .ok()
                .map(|b| b as f32 / 255.0)
        };
        Some(Rgb([v(0)?, v(2)?, v(4)?]))
    }
}

impl Serialize for Rgb {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.hex())
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Hex(String),
            List([f32; 3]),
        }
        match Raw::deserialize(d)? {
            Raw::Hex(s) => {
                Rgb::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("bad colour {s}")))
            }
            Raw::List(v) => Ok(Rgb(v)),
        }
    }
}

/// The pens of a view's [`LineWeights`], by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PenName {
    Cut,
    Projection,
    Symbol,
    Fine,
    Hatch,
    Hidden,
}

impl PenName {
    pub const ALL: [PenName; 6] = [
        PenName::Cut,
        PenName::Projection,
        PenName::Symbol,
        PenName::Fine,
        PenName::Hatch,
        PenName::Hidden,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PenName::Cut => "Cut pen",
            PenName::Projection => "Projection pen",
            PenName::Symbol => "Symbol pen",
            PenName::Fine => "Fine pen",
            PenName::Hatch => "Hatch pen",
            PenName::Hidden => "Hidden pen",
        }
    }

    pub fn width(self, w: &LineWeights) -> f64 {
        match self {
            PenName::Cut => w.cut,
            PenName::Projection => w.projection,
            PenName::Symbol => w.symbol,
            PenName::Fine => w.fine,
            PenName::Hatch => w.hatch,
            PenName::Hidden => w.hidden,
        }
    }
}

/// A line weight: one of the view's pens (so it follows the scale), or a
/// fixed width on paper in millimetres. Written `"Fine"` or `0.25`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Weight {
    Pen(PenName),
    Mm(f64),
}

impl Weight {
    pub fn mm(self, w: &LineWeights) -> f64 {
        match self {
            Weight::Pen(p) => p.width(w),
            Weight::Mm(v) => v,
        }
    }

    pub fn label(self) -> String {
        match self {
            Weight::Pen(p) => p.label().to_string(),
            Weight::Mm(v) => format!("{v:.2} mm"),
        }
    }
}

/// How a line is broken up on paper.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LinePattern {
    Solid,
    Dashed,
    Dotted,
    /// Long and short dashes, for centre lines.
    Centre,
    DashDot,
    /// Dash, gap, dash, gap… in paper millimetres; a dash of 0 is a dot.
    Custom(Vec<f64>),
}

impl LinePattern {
    pub const NAMED: [LinePattern; 5] = [
        LinePattern::Solid,
        LinePattern::Dashed,
        LinePattern::Dotted,
        LinePattern::Centre,
        LinePattern::DashDot,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            LinePattern::Solid => "Solid",
            LinePattern::Dashed => "Dashed",
            LinePattern::Dotted => "Dotted",
            LinePattern::Centre => "Centre",
            LinePattern::DashDot => "Dash dot",
            LinePattern::Custom(_) => "Custom",
        }
    }

    /// Its dashes and gaps in paper mm; none when it is solid.
    pub fn dashes(&self) -> Option<Vec<f64>> {
        match self {
            LinePattern::Solid => None,
            LinePattern::Dashed => Some(vec![2.0, 1.0]),
            LinePattern::Dotted => Some(vec![0.0, 0.8]),
            LinePattern::Centre => Some(vec![6.0, 1.0, 1.0, 1.0]),
            LinePattern::DashDot => Some(vec![3.0, 1.0, 0.0, 1.0]),
            LinePattern::Custom(v) => {
                let ok = v.len() >= 2 && v.iter().all(|x| *x >= 0.0) && v.iter().sum::<f64>() > 0.0;
                ok.then(|| v.clone())
            }
        }
    }
}

/// How a category draws, model-wide: an object style. With `material`,
/// only where its elements are of that material (by name). Every field is
/// optional; what is left out draws as CreaBIM does without a style.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectStyle {
    pub category: Category,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// Lines seen in projection (below the cut, in elevation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection_weight: Option<Weight>,
    /// Outlines of what the view cuts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut_weight: Option<Weight>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<Rgb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<LinePattern>,
    /// Behind the cut hatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut_fill: Option<Rgb>,
    /// A [`HatchPattern`] by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut_pattern: Option<String>,
    /// The fill of its surfaces seen in projection (a roof from above).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_fill: Option<Rgb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_pattern: Option<String>,
    /// Lower draws first, under higher; 0 when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draw_order: Option<i32>,
    /// 0 opaque to 1 invisible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transparency: Option<f32>,
}

impl ObjectStyle {
    pub fn new(category: Category) -> Self {
        Self {
            category,
            material: None,
            projection_weight: None,
            cut_weight: None,
            colour: None,
            pattern: None,
            cut_fill: None,
            cut_pattern: None,
            surface_fill: None,
            surface_pattern: None,
            draw_order: None,
            transparency: None,
        }
    }
}

/// Whether a pattern's sizes are on paper (fixed-size dots and lines) or
/// in the model (true-size block coursing and tiles).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatternUnits {
    /// Millimetres on paper.
    #[default]
    Paper,
    /// Metres in the model.
    Model,
}

/// One family of parallel lines in a hatch, as in a CAD `.pat` file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HatchLine {
    /// Degrees counter-clockwise from the drawing's x axis.
    #[serde(default)]
    pub angle: f64,
    /// Distance between the lines.
    pub spacing: f64,
    /// The family moved this far across its lines.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset: f64,
    /// Each line's dashes slid this far along it from the one before,
    /// for staggered coursing.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub shift: f64,
    /// Dash, gap, dash, gap…; a dash of 0 is a dot. None is solid.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dashes: Vec<f64>,
    /// The angle is from the outline's longest side, not the x axis: grain
    /// running along a timber.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub along_longest: bool,
}

/// A small drawing repeated across a fill, for patterns lines can't make.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tile {
    /// Width and height of one tile.
    pub size: [f64; 2],
    /// Polylines inside the tile, from its lower left corner.
    pub strokes: Vec<Vec<[f64; 2]>>,
}

/// A named fill pattern: lines, tiles or both over a background.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HatchPattern {
    pub name: String,
    #[serde(default)]
    pub units: PatternUnits,
    /// Behind the lines; none leaves it white.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Rgb>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<HatchLine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tile: Option<Tile>,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// The paper spacing the built-in patterns are sized from, mm.
pub const BASE_SPACING: f64 = 1.2;

/// The built-in pattern a material's [`Hatch`] names: the same lines
/// CreaBIM drew before patterns were data, so every material still reads.
pub fn builtin_pattern(h: Hatch) -> HatchPattern {
    let s = BASE_SPACING;
    let grey = |g: f32| Some(Rgb([g; 3]));
    let line = |angle: f64, spacing: f64, offset: f64| HatchLine {
        angle,
        spacing,
        offset,
        shift: 0.0,
        dashes: vec![],
        along_longest: false,
    };
    let (fill, lines) = match h {
        Hatch::Empty => (None, vec![]),
        Hatch::Solid => (grey(0.6), vec![]),
        Hatch::Diagonal => (grey(0.88), vec![line(45.0, s, 0.0)]),
        Hatch::Cross => (
            grey(0.88),
            vec![line(45.0, s * 1.4, 0.0), line(-45.0, s * 1.4, 0.0)],
        ),
        Hatch::Brick => (
            grey(0.88),
            vec![line(45.0, s * 2.0, 0.0), line(45.0, s * 2.0, s * 0.5)],
        ),
        Hatch::Concrete => (
            grey(0.88),
            vec![
                line(45.0, s * 4.0, 0.0),
                HatchLine {
                    dashes: vec![0.0, s * 1.3],
                    shift: s * 0.65,
                    ..line(0.0, s * 1.3, 0.0)
                },
            ],
        ),
        Hatch::Timber => (
            grey(0.88),
            vec![HatchLine {
                along_longest: true,
                ..line(0.0, s * 1.2, s * 0.3)
            }],
        ),
    };
    HatchPattern {
        name: h.label().to_string(),
        units: PatternUnits::Paper,
        fill,
        lines,
        tile: None,
    }
}

/// The pens for drawings up to a scale: `up_to` 75 covers 1:75 and larger.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PenTable {
    pub up_to: f64,
    pub weights: LineWeights,
}

/// What a per-view override applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Target {
    Category(Category),
    Element(ElementId),
}

/// A view's change to how a category or one element draws in it, on top
/// of the object styles: "site elements halftone", "boundary wall dashed".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Override {
    pub target: Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<Weight>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut_weight: Option<Weight>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<Rgb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<LinePattern>,
    /// Drawn half-strength, mixed toward white.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub halftone: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transparency: Option<f32>,
    /// Cut hatching on or off; none keeps the style's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hatch: Option<bool>,
}

impl Override {
    pub fn new(target: Target) -> Self {
        Self {
            target,
            weight: None,
            cut_weight: None,
            colour: None,
            pattern: None,
            halftone: false,
            transparency: None,
            hatch: None,
        }
    }
}

/// Sun shadows: cast from each element onto the ground below it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shadows {
    /// The compass bearing shadows fall toward, degrees counter-clockwise
    /// from the drawing's x axis.
    pub direction: f64,
    /// How long a shadow is per metre of height.
    pub length: f64,
    /// 0 none to 1 black.
    pub opacity: f32,
}

/// Depth and shadow cues of a view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DepthCues {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadows: Option<Shadows>,
    /// Lines and fills further from the cut or the viewer get lighter,
    /// up to this much at the far end of the view (0..1).
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub fade: f32,
}

fn is_zero_f32(v: &f32) -> bool {
    *v == 0.0
}

impl DepthCues {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// A named bundle of how views draw, applied to many views: change it and
/// every view that uses it changes. Views name their template, so loading
/// another pack with templates of the same names restyles them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewTemplate {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ViewStyle>,
    /// Categories its views leave out, besides their own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden: Vec<Category>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<Override>,
    /// Pens of its own, whatever the scale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weights: Option<LineWeights>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub below_fill: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cues: Option<DepthCues>,
}

impl ViewTemplate {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            scale: None,
            style: None,
            hidden: vec![],
            overrides: vec![],
            weights: None,
            below_fill: None,
            cues: None,
        }
    }
}

/// An annotation style carried by a pack, by name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedTextStyle {
    pub name: String,
    pub style: TextStyle,
}

/// A title block carried by a pack, by name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedTitleBlock {
    pub name: String,
    pub template: SheetTemplate,
}

/// An office's drawing standard as one file: pens, object styles, hatch
/// patterns, view templates, text styles, title blocks and mark rules.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StylePack {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The project draws with it. One pack in a project is.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub default: bool,
    /// Pens by scale, smallest `up_to` first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pens: Vec<PenTable>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<ObjectStyle>,
    /// Patterns by name. One named like a built-in (*Brick*, *Concrete*…)
    /// replaces it for every material using that hatch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<HatchPattern>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<ViewTemplate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_styles: Vec<NamedTextStyle>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub title_blocks: Vec<NamedTitleBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark_rules: Option<MarkRules>,
}

/// The two packs that ship with CreaBIM, as data.
pub const STANDARD_PACK: &str = include_str!("../packs/standard.json");
pub const LIGHT_PACK: &str = include_str!("../packs/light-linework.json");

impl StylePack {
    /// The look of a project without a pack: the *Standard* pack.
    pub fn standard() -> Self {
        Self::from_json(STANDARD_PACK).expect("the standard pack reads")
    }

    /// The packs that come with CreaBIM.
    pub fn shipped() -> Vec<Self> {
        [STANDARD_PACK, LIGHT_PACK]
            .iter()
            .map(|s| Self::from_json(s).expect("shipped packs read"))
            .collect()
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let p: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        p.validate()?;
        Ok(p)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a pack always serializes")
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("a style pack needs a name".into());
        }
        for p in &self.patterns {
            for l in &p.lines {
                if !(l.spacing > 0.0) {
                    return Err(format!("pattern {}: spacing must be above 0", p.name));
                }
            }
            if let Some(t) = &p.tile {
                if !(t.size[0] > 0.0 && t.size[1] > 0.0) {
                    return Err(format!("pattern {}: tile size must be above 0", p.name));
                }
            }
        }
        for t in &self.pens {
            if !(t.up_to > 0.0) {
                return Err("pen tables need a scale above 0".into());
            }
        }
        if let Some(r) = &self.mark_rules {
            r.validate()?;
        }
        Ok(())
    }

    /// The pens a drawing at 1:`scale` takes.
    pub fn weights_at(&self, scale: f64) -> LineWeights {
        let mut tables: Vec<&PenTable> = self.pens.iter().collect();
        tables.sort_by(|a, b| a.up_to.total_cmp(&b.up_to));
        tables
            .iter()
            .find(|t| scale <= t.up_to)
            .or(tables.last())
            .map_or_else(|| LineWeights::for_scale(scale), |t| t.weights)
    }

    /// A pattern by name: the pack's own, else a built-in one.
    pub fn pattern(&self, name: &str) -> Option<Cow<'_, HatchPattern>> {
        if let Some(p) = self
            .patterns
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
        {
            return Some(Cow::Borrowed(p));
        }
        Hatch::ALL
            .iter()
            .find(|h| h.label().eq_ignore_ascii_case(name))
            .map(|h| Cow::Owned(builtin_pattern(*h)))
    }

    /// Every pattern name a material or style can pick: built-ins, then
    /// the pack's own.
    pub fn pattern_names(&self) -> Vec<String> {
        let mut v: Vec<String> = Hatch::ALL.iter().map(|h| h.label().to_string()).collect();
        for p in &self.patterns {
            if !v.iter().any(|n| n.eq_ignore_ascii_case(&p.name)) {
                v.push(p.name.clone());
            }
        }
        v
    }

    pub fn template(&self, name: &str) -> Option<&ViewTemplate> {
        self.templates.iter().find(|t| t.name == name)
    }

    /// The object style of a category, the one for its material first.
    pub fn object_style(&self, c: Category, material: Option<&str>) -> Option<&ObjectStyle> {
        let of = |m: Option<&str>| {
            self.objects.iter().find(|s| {
                s.category == c
                    && match (&s.material, m) {
                        (None, None) => true,
                        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                        _ => false,
                    }
            })
        };
        material.and_then(|m| of(Some(m))).or_else(|| of(None))
    }
}

/// The style packs a project holds.
pub fn packs(model: &Model) -> Vec<(ElementId, &StylePack)> {
    model
        .iter()
        .filter_map(|e| match &e.kind {
            ElementKind::StylePack(p) => Some((e.id, p.as_ref())),
            _ => None,
        })
        .collect()
}

/// The pack a project draws with: the one marked default, else its first,
/// else the *Standard* pack.
pub fn active_pack(model: &Model) -> Cow<'_, StylePack> {
    let all = packs(model);
    all.iter()
        .find(|(_, p)| p.default)
        .or(all.first())
        .map_or_else(
            || Cow::Owned(StylePack::standard()),
            |(_, p)| Cow::Borrowed(*p),
        )
}

/// Load `pack` into the project and make it the default: a pack of the
/// same name is replaced, and the pack's text styles, title blocks and
/// mark rules replace the project's of the same names. One undo step.
pub fn apply_pack(model: &Model, pack: &StylePack) -> Transaction {
    let mut t = Transaction::new(format!("Apply style pack {}", pack.name));
    let mut pack = pack.clone();
    pack.default = true;
    let mut placed = false;
    for (id, p) in packs(model) {
        let before = model.get(id).expect("listed");
        let mut e = before.clone();
        if p.name == pack.name && !placed {
            e.kind = ElementKind::StylePack(Box::new(pack.clone()));
            e.name = pack.name.clone();
            placed = true;
        } else if p.default {
            let mut q = p.clone();
            q.default = false;
            e.kind = ElementKind::StylePack(Box::new(q));
        } else {
            continue;
        }
        t = t.replace(before.clone(), e);
    }
    if !placed {
        t = t.insert(Element::new(
            &pack.name,
            ElementKind::StylePack(Box::new(pack.clone())),
        ));
    }
    for s in &pack.text_styles {
        let found = model
            .iter()
            .find(|e| e.name == s.name && matches!(e.kind, ElementKind::TextStyle(_)));
        match found {
            Some(before) => {
                let mut e = before.clone();
                e.kind = ElementKind::TextStyle(s.style.clone());
                t = t.replace(before.clone(), e);
            }
            None => {
                t = t.insert(Element::new(
                    &s.name,
                    ElementKind::TextStyle(s.style.clone()),
                ))
            }
        }
    }
    for b in &pack.title_blocks {
        let found = model
            .iter()
            .find(|e| e.name == b.name && matches!(e.kind, ElementKind::SheetTemplate(_)));
        match found {
            Some(before) => {
                let mut e = before.clone();
                e.kind = ElementKind::SheetTemplate(b.template.clone());
                t = t.replace(before.clone(), e);
            }
            None => {
                t = t.insert(Element::new(
                    &b.name,
                    ElementKind::SheetTemplate(b.template.clone()),
                ))
            }
        }
    }
    if let Some(r) = &pack.mark_rules {
        for op in crate::marks::set_rules(model, r.clone()).ops {
            t.ops.push(op);
        }
    }
    t
}

/// Make the pack `id` the project's default.
pub fn make_default(model: &Model, id: ElementId) -> Transaction {
    let mut t = Transaction::new("Default style pack");
    for (pid, p) in packs(model) {
        if (pid == id) != p.default {
            let before = model.get(pid).expect("listed");
            let mut e = before.clone();
            let mut q = p.clone();
            q.default = pid == id;
            e.kind = ElementKind::StylePack(Box::new(q));
            t = t.replace(before.clone(), e);
        }
    }
    t
}

/// What a view adds to the pack's styles: its template, its own overrides
/// and its depth cues.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewLook {
    /// A [`ViewTemplate`] of the active pack, by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// Applied after the template's, so the view's own win.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<Override>,
    #[serde(default, skip_serializing_if = "DepthCues::is_default")]
    pub cues: DepthCues,
}

impl ViewLook {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The view's override of `target`, made if it has none.
    pub fn override_mut(&mut self, target: Target) -> &mut Override {
        if let Some(i) = self.overrides.iter().position(|o| o.target == target) {
            return &mut self.overrides[i];
        }
        self.overrides.push(Override::new(target));
        self.overrides.last_mut().expect("just pushed")
    }

    /// Drop overrides that change nothing.
    pub fn tidy(&mut self) {
        self.overrides.retain(|o| *o != Override::new(o.target));
    }
}

/// The view as it draws: its template's scale, style, hidden categories,
/// pens and cues laid under its own settings. A template the pack lacks
/// is ignored.
pub fn resolved<'a>(model: &Model, v: &'a View) -> Cow<'a, View> {
    let Some(name) = &v.look.template else {
        return Cow::Borrowed(v);
    };
    let pack = active_pack(model);
    let Some(t) = pack.template(name) else {
        return Cow::Borrowed(v);
    };
    let mut out = v.clone();
    if let Some(s) = t.scale {
        if !matches!(v.kind, crate::ViewKind::Schedule { .. }) {
            out.scale = s;
        }
    }
    if let Some(s) = t.style {
        out.style = s;
    }
    for c in &t.hidden {
        if !out.hidden.hides_category(*c) {
            out.hidden.set_category(*c, true);
        }
    }
    let mut overrides = t.overrides.clone();
    overrides.extend(v.look.overrides.iter().cloned());
    out.look.overrides = overrides;
    if out.graphics.weights.is_none() {
        out.graphics.weights = t.weights;
    }
    if let Some(b) = t.below_fill {
        out.graphics.below_fill = b;
    }
    if let (Some(c), true) = (t.cues, v.look.cues.is_default()) {
        out.look.cues = c;
    }
    Cow::Owned(out)
}

/// The pens a view draws with: its own, its template's, else its pack's
/// for its scale.
pub fn view_weights(model: &Model, v: &View) -> LineWeights {
    let v = resolved(model, v);
    let scale = v.paper_scale();
    v.graphics
        .weights
        .unwrap_or_else(|| active_pack(model).weights_at(scale))
}

/// Give `views` the template `name` (none takes it off). One undo step.
pub fn set_template(model: &Model, views: &[ElementId], name: Option<&str>) -> Transaction {
    let mut t = Transaction::new(match name {
        Some(n) => format!("View template {n}"),
        None => "No view template".into(),
    });
    for id in views {
        let Some(e) = model.get(*id) else { continue };
        let ElementKind::View(v) = &e.kind else {
            continue;
        };
        if v.look.template.as_deref() == name {
            continue;
        }
        let mut after = e.clone();
        if let ElementKind::View(v) = &mut after.kind {
            v.look.template = name.map(str::to_string);
        }
        t = t.replace(e.clone(), after);
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;

    #[test]
    fn shipped_packs_read_and_differ() {
        let packs = StylePack::shipped();
        assert_eq!(packs.len(), 2);
        let (a, b) = (&packs[0], &packs[1]);
        assert_eq!(a.name, "Standard");
        assert_eq!(b.name, "Light linework");
        assert_ne!(a.weights_at(100.0), b.weights_at(100.0));
        // the standard pack draws as CreaBIM did before packs
        assert_eq!(a.weights_at(50.0), LineWeights::for_scale(50.0));
        assert_eq!(a.weights_at(100.0), LineWeights::for_scale(100.0));
        assert_eq!(a.weights_at(500.0), LineWeights::for_scale(500.0));
        // a pack writes and reads back the same
        for p in &packs {
            assert_eq!(&StylePack::from_json(&p.to_json()).unwrap(), p);
        }
        // the light pack's templates are named like the standard's
        for t in &a.templates {
            assert!(b.template(&t.name).is_some(), "{}", t.name);
        }
    }

    #[test]
    fn every_old_hatch_names_a_pattern() {
        let p = StylePack::standard();
        for h in Hatch::ALL {
            let pat = p.pattern(h.label()).unwrap();
            assert_eq!(pat.name, h.label());
        }
        assert_eq!(builtin_pattern(Hatch::Empty).fill, None);
        assert!(builtin_pattern(Hatch::Solid).lines.is_empty());
        assert_eq!(builtin_pattern(Hatch::Cross).lines.len(), 2);
        assert!(p.pattern("No such pattern").is_none());
    }

    #[test]
    fn values_read_in_the_short_forms_packs_use() {
        let w: Weight = serde_json::from_str("\"Fine\"").unwrap();
        assert_eq!(w, Weight::Pen(PenName::Fine));
        let w: Weight = serde_json::from_str("0.35").unwrap();
        assert_eq!(w, Weight::Mm(0.35));
        let c: Rgb = serde_json::from_str("\"#ff8000\"").unwrap();
        assert_eq!(c.hex(), "#ff8000");
        let c: Rgb = serde_json::from_str("[0.0, 1.0, 0.0]").unwrap();
        assert_eq!(c.hex(), "#00ff00");
        assert!(serde_json::from_str::<Rgb>("\"red\"").is_err());
        let p: LinePattern = serde_json::from_str(r#"{"Custom":[3.0,1.0]}"#).unwrap();
        assert_eq!(p.dashes(), Some(vec![3.0, 1.0]));
        assert_eq!(LinePattern::Custom(vec![-1.0, 2.0]).dashes(), None);
        assert!(StylePack::from_json(r#"{"name":""}"#).is_err());
        let bad = r#"{"name":"x","patterns":[{"name":"p","lines":[{"spacing":0}]}]}"#;
        assert!(StylePack::from_json(bad).is_err());
    }

    #[test]
    fn applying_a_pack_makes_it_the_default_and_undoes() {
        let mut doc = Document::from_model(crate::starter::starter_house());
        assert_eq!(active_pack(doc.model()).name, "Standard");
        let light = StylePack::shipped().remove(1);
        doc.commit(apply_pack(doc.model(), &light)).unwrap();
        assert_eq!(active_pack(doc.model()).name, "Light linework");
        // back to the standard one: both are kept, one is the default
        let standard = StylePack::standard();
        doc.commit(apply_pack(doc.model(), &standard)).unwrap();
        assert_eq!(packs(doc.model()).len(), 2);
        assert_eq!(active_pack(doc.model()).name, "Standard");
        let light_id = packs(doc.model())
            .iter()
            .find(|p| p.1.name == "Light linework")
            .unwrap()
            .0;
        doc.commit(make_default(doc.model(), light_id)).unwrap();
        assert_eq!(active_pack(doc.model()).name, "Light linework");
        doc.undo();
        doc.undo();
        doc.undo();
        assert!(packs(doc.model()).is_empty());
        // a pack survives saving
        doc.commit(apply_pack(doc.model(), &light)).unwrap();
        let back = Model::from_json(&doc.model().to_json()).unwrap();
        assert_eq!(active_pack(&back).as_ref(), &{
            let mut l = light.clone();
            l.default = true;
            l
        });
    }

    #[test]
    fn templates_restyle_every_view_that_names_them() {
        let mut doc = Document::from_model(crate::starter::starter_house());
        let plans: Vec<ElementId> = doc
            .model()
            .iter()
            .filter(|e| {
                matches!(&e.kind, ElementKind::View(v) if matches!(v.kind, crate::ViewKind::Plan { .. }))
            })
            .map(|e| e.id)
            .collect();
        assert!(plans.len() >= 2);
        let mut pack = StylePack::standard();
        let mut t = ViewTemplate::new("Plan test");
        t.hidden = vec![Category::Objects];
        t.scale = Some(200.0);
        let mut o = Override::new(Target::Category(Category::Walls));
        o.halftone = true;
        t.overrides.push(o);
        pack.templates.push(t);
        doc.commit(apply_pack(doc.model(), &pack)).unwrap();
        doc.commit(set_template(doc.model(), &plans, Some("Plan test")))
            .unwrap();
        let view = |doc: &Document, id| match &doc.model().get(id).unwrap().kind {
            ElementKind::View(v) => v.clone(),
            _ => unreachable!(),
        };
        for id in &plans {
            let v = view(&doc, *id);
            let r = resolved(doc.model(), &v);
            assert_eq!(r.scale, 200.0);
            assert!(r.hidden.hides_category(Category::Objects));
            assert!(r.look.overrides[0].halftone);
        }
        // changing the template changes them all
        pack.templates.last_mut().unwrap().scale = Some(50.0);
        doc.commit(apply_pack(doc.model(), &pack)).unwrap();
        for id in &plans {
            let v = view(&doc, *id);
            assert_eq!(resolved(doc.model(), &v).scale, 50.0);
            assert_eq!(view_weights(doc.model(), &v), LineWeights::for_scale(50.0));
        }
    }

    #[test]
    fn old_views_and_materials_read_with_no_look() {
        let old = r#"{"kind":{"Plan":{"level":"00000000-0000-0000-0000-000000000001","cut_height":1.2}},"scale":50.0}"#;
        let v: View = serde_json::from_str(old).unwrap();
        assert!(v.look.is_default());
        assert!(!serde_json::to_string(&v).unwrap().contains("look"));
        let m: crate::Material =
            serde_json::from_str(r#"{"hatch":"Brick","color":[1,0,0]}"#).unwrap();
        assert_eq!(m.pattern, None);
        assert!(!serde_json::to_string(&m).unwrap().contains("pattern"));
    }
}
