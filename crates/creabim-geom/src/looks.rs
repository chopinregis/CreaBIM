//! How a view's drawing looks on paper: its pens, and the object styles,
//! hatch patterns and graphic overrides of the project's style pack. They
//! are resolved per element once, when the view is placed, so drawing each
//! line is a lookup.

use crate::paper::{pen_stroke_with, Canvas, PaperMap, Stroke};
use crate::plan::{Hatch, Pen, PlanItem};
use creabim_core::styles::{self, HatchPattern, PatternUnits, ScalePattern, ScaleRule, Target};
use creabim_core::visibility::Category;
use creabim_core::{ElementId, ElementKind, LineWeights, Model, View, ViewKind};
use glam::DVec2;
use std::collections::{HashMap, HashSet};

/// The colour of an option a view ghosts.
pub const GHOST_BLUE: [f32; 3] = [0.17, 0.42, 0.65];

/// The look of one element in one view, where it differs from the pens.
#[derive(Clone, Debug, Default, PartialEq)]
struct Look {
    projection: Option<f64>,
    cut: Option<f64>,
    colour: Option<[f32; 3]>,
    /// Its line pattern: none keeps the pen's, `Some(None)` is solid.
    dashes: Option<Option<Vec<f64>>>,
    /// How far toward white it is drawn, 0..1: halftone, transparency,
    /// distance.
    fade: f32,
    cut_fill: Option<[f32; 3]>,
    /// A pattern its category's style names for its cut faces.
    category_pattern: Option<String>,
    /// A pattern its material names, or its category's style for that
    /// material, and the hatch of that material.
    material_pattern: Option<(Hatch, String)>,
    hatch_off: bool,
    /// The cut patterns its layers' materials name: (their hatch, their
    /// colour, the pattern).
    layer_patterns: Vec<(Hatch, [f32; 3], String)>,
    surface_fill: Option<[f32; 3]>,
    surface_pattern: Option<String>,
    order: i32,
}

/// A material drawn at its real size in large-scale drawings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrueKind {
    /// Grain along a member; a cross on one cut across.
    Timber,
    /// Batts, wool, cellulose: loops from face to face.
    Batt,
}

/// What `m` is drawn as at true size, if anything: soft insulation by its
/// words, timber by its hatch or category; rigid boards stay plain.
pub fn true_kind(m: &creabim_core::Material) -> Option<TrueKind> {
    let words = format!("{} {}", m.category, m.description).to_lowercase();
    let soft = [
        "batt",
        "wool",
        "cellulose",
        "fibre",
        "fiber",
        "dense-pack",
        "dense pack",
    ];
    if soft.iter().any(|w| words.contains(w)) {
        return Some(TrueKind::Batt);
    }
    let timber = m.hatch == Hatch::Timber
        || ["timber", "wood"]
            .iter()
            .any(|c| m.category.eq_ignore_ascii_case(c));
    timber.then_some(TrueKind::Timber)
}

fn mix(fade: &mut f32, by: f32) {
    *fade = 1.0 - (1.0 - *fade) * (1.0 - by.clamp(0.0, 1.0));
}

/// The pens and styles a drawing is drawn with.
#[derive(Clone, Debug)]
pub struct Looks {
    pub weights: LineWeights,
    /// The scale table's row for the view: how its cuts are patterned.
    /// None draws every pattern as it is (a drawing with no view).
    pub rule: Option<ScaleRule>,
    /// Each material drawn at true size in large-scale drawings, and as
    /// what.
    pub kinds: HashMap<ElementId, TrueKind>,
    by_id: HashMap<ElementId, Look>,
    patterns: HashMap<String, HatchPattern>,
    ordered: bool,
    /// Site areas whose surface a style fills or patterns: their boundary
    /// and holes, and the pattern drawn when the area has none of its own.
    areas: Vec<(ElementId, Vec<Vec<DVec2>>, Option<String>)>,
    /// A section, elevation or detail: courses stay level, whatever the
    /// shape they fill (a wall cut in section is taller than it is wide).
    upright: bool,
    /// The view's line style when it is not Ruled: its lines go through
    /// the hand-line engine (pens stage 4).
    pub hand: Option<creabim_core::linestyle::LineStyle>,
    /// The view's palette when it is not Ink: its colours are given on
    /// paper by a [`crate::palettecanvas::PaletteCanvas`] (pens stage 7).
    pub palette: Option<creabim_core::palette::Roles>,
}

impl Looks {
    /// Only the pens: no styles, the built-in patterns.
    pub fn plain(weights: LineWeights) -> Self {
        Self {
            weights,
            rule: None,
            kinds: HashMap::new(),
            by_id: HashMap::new(),
            patterns: HashMap::new(),
            ordered: false,
            areas: vec![],
            upright: false,
            hand: None,
            palette: None,
        }
    }

    /// The looks of view `view` (`v`, as [`styles::resolved`] gives it)
    /// with the project's style pack.
    pub fn for_view(model: &Model, view: ElementId, v: &View) -> Self {
        let pack = styles::active_pack(model);
        let pens = styles::view_pens(model, v);
        let weights = pens.weights;
        let w = &weights;
        let cutters = creabim_core::visibility::cutters(model);
        let beyond = crate::ranges::beyond(model, v);
        let fades = distance_fades(model, v);
        let mut by_id = HashMap::new();
        let mut areas = vec![];
        let phase = creabim_core::phases::statuses(model, v.phase, &v.options, &v.ghost);
        let phase_look = creabim_core::phases::look(model);
        for e in model.iter() {
            if e.id == view {
                continue;
            }
            let Some(cat) = Category::of(e, &cutters) else {
                continue;
            };
            let material = creabim_core::material_of(model, e);
            let mut l = Look::default();
            let apply = |l: &mut Look, s: &styles::ObjectStyle| {
                if let Some(x) = s.projection_weight {
                    l.projection = Some(pens.mm(x));
                }
                if let Some(x) = s.cut_weight {
                    l.cut = Some(pens.mm(x));
                }
                if let Some(c) = s.colour {
                    l.colour = Some(c.0);
                }
                if let Some(p) = &s.pattern {
                    l.dashes = Some(p.dashes());
                }
                if let Some(c) = s.cut_fill {
                    l.cut_fill = Some(c.0);
                }
                if let Some(c) = s.surface_fill {
                    l.surface_fill = Some(c.0);
                }
                if let Some(p) = &s.surface_pattern {
                    l.surface_pattern = Some(p.clone());
                }
                if let Some(o) = s.draw_order {
                    l.order = o;
                }
                if let Some(t) = s.transparency {
                    mix(&mut l.fade, t);
                }
            };
            if let Some(s) = pack.object_style(cat, None) {
                apply(&mut l, s);
                l.category_pattern = s.cut_pattern.clone();
            }
            if let Some((me, m)) = material {
                if let Some(s) = pack
                    .object_style(cat, Some(&me.name))
                    .filter(|s| s.material.is_some())
                {
                    apply(&mut l, s);
                    if let Some(p) = &s.cut_pattern {
                        l.material_pattern = Some((m.hatch, p.clone()));
                    }
                }
                if l.material_pattern.is_none() {
                    if let Some(p) = &m.pattern {
                        l.material_pattern = Some((m.hatch, p.clone()));
                    }
                }
                if l.surface_pattern.is_none() {
                    l.surface_pattern = m.surface_pattern.clone();
                }
            }
            if let Some((_, a)) = creabim_core::assembly::assembly_of(model, e) {
                for layer in &a.layers {
                    if let Some(ElementKind::Material(m)) =
                        layer.material.and_then(|id| model.get(id)).map(|e| &e.kind)
                    {
                        if let Some(p) = &m.pattern {
                            l.layer_patterns.push((m.hatch, m.color, p.clone()));
                        }
                        if l.surface_pattern.is_none()
                            && m.surface_pattern.is_some()
                            && material.is_none()
                        {
                            // the outermost finish shows on its surfaces
                            l.surface_pattern = m.surface_pattern.clone();
                        }
                    }
                }
            }
            // the view's overrides: its category's, then its own
            let ours = |o: &&styles::Override| match o.target {
                Target::Category(c) => c == cat,
                Target::Element(id) => id == e.id,
            };
            let mut overrides: Vec<&styles::Override> =
                v.look.overrides.iter().filter(ours).collect();
            overrides.sort_by_key(|o| matches!(o.target, Target::Element(_)));
            for o in overrides {
                if let Some(x) = o.weight {
                    l.projection = Some(pens.mm(x));
                }
                if let Some(x) = o.cut_weight {
                    l.cut = Some(pens.mm(x));
                }
                if let Some(c) = o.colour {
                    l.colour = Some(c.0);
                }
                if let Some(p) = &o.pattern {
                    l.dashes = Some(p.dashes());
                }
                if o.halftone {
                    mix(&mut l.fade, 0.5);
                }
                if let Some(t) = o.transparency {
                    mix(&mut l.fade, t);
                }
                if let Some(h) = o.hatch {
                    l.hatch_off = !h;
                }
            }
            // from the phase the view stands at: what was there before
            // reads fainter and unhatched, what comes down in red dashes
            match phase.get(&e.id) {
                Some(creabim_core::phases::Status::Existing) if phase_look.existing_faint => {
                    mix(&mut l.fade, 0.5);
                    l.hatch_off = true;
                }
                Some(creabim_core::phases::Status::TakenDown) => {
                    l.colour = Some(phase_look.down);
                    if phase_look.down_dashed {
                        l.dashes = Some(styles::LinePattern::Dashed.dashes());
                    }
                    l.hatch_off = true;
                }
                Some(creabim_core::phases::Status::New) => {
                    if let Some(c) = phase_look.new {
                        l.colour = Some(c);
                    }
                }
                Some(creabim_core::phases::Status::Ghost) => {
                    l.colour = Some(GHOST_BLUE);
                    l.dashes = Some(styles::LinePattern::Dashed.dashes());
                    mix(&mut l.fade, 0.35);
                    l.hatch_off = true;
                }
                _ => {}
            }
            if beyond.contains(&e.id) {
                l.projection = Some(w.fine);
                mix(&mut l.fade, 0.45);
            }
            if let Some(f) = fades.get(&e.id) {
                mix(&mut l.fade, *f);
            }
            if let ElementKind::Site(creabim_core::site::Site {
                form:
                    creabim_core::site::SiteForm::Area {
                        boundary,
                        holes,
                        pattern,
                        ..
                    },
                ..
            }) = &e.kind
            {
                // an area's own pattern wins; else its style's or material's
                let named = (*pattern == creabim_core::site::Pattern::None)
                    .then(|| {
                        l.surface_pattern
                            .clone()
                            .or_else(|| l.material_pattern.as_ref().map(|(_, n)| n.clone()))
                    })
                    .flatten();
                if l.surface_fill.is_some() || named.is_some() {
                    let mut loops = vec![boundary.clone()];
                    loops.extend(holes.iter().cloned());
                    areas.push((e.id, loops, named));
                }
            }
            if l != Look::default() {
                by_id.insert(e.id, l);
            }
        }
        // what hangs on an element draws as it does, unless styled itself
        for e in model.iter() {
            let host = match &e.kind {
                ElementKind::Opening(o) if o.kind == creabim_core::OpeningKind::Hole => o.host,
                _ => continue,
            };
            if let (false, Some(l)) = (by_id.contains_key(&e.id), by_id.get(&host).cloned()) {
                by_id.insert(e.id, l);
            }
        }
        let patterns = pack
            .patterns
            .iter()
            .map(|p| (p.name.to_lowercase(), p.clone()))
            .collect();
        let ordered = by_id.values().any(|l| l.order != 0);
        Self {
            weights,
            rule: Some(styles::view_rule(model, v)),
            kinds: model
                .iter()
                .filter_map(|e| match &e.kind {
                    ElementKind::Material(m) => Some((e.id, true_kind(m)?)),
                    _ => None,
                })
                .collect(),
            by_id,
            patterns,
            ordered,
            areas,
            upright: matches!(v.kind, creabim_core::ViewKind::Section { .. }),
            hand: styles::view_line_style(model, v),
            palette: Some(styles::view_palette(model, v))
                .filter(|p| !p.is_ink())
                .map(|p| p.roles()),
        }
    }

    /// Whether any site area has a styled surface.
    pub fn has_surfaces(&self) -> bool {
        !self.areas.is_empty()
    }

    /// Draw the styled surfaces of the site areas in `drawn` (the elements
    /// a drawing shows), under everything else: the style's fill, and its
    /// pattern where the area has none of its own.
    pub fn surfaces(&self, canvas: &mut dyn Canvas, drawn: &HashSet<ElementId>, map: &PaperMap) {
        for (id, loops, named) in &self.areas {
            if !drawn.contains(id) || loops.is_empty() {
                continue;
            }
            let l = self.look(*id);
            let fade = l.map_or(0.0, |l| l.fade);
            let paper = |pts: &[DVec2]| pts.iter().map(|p| map.to_paper(*p)).collect::<Vec<_>>();
            if let Some(c) = l.and_then(|l| l.surface_fill) {
                canvas.fill_rgb(&paper(&loops[0]), c.map(|v| 1.0 - (1.0 - v) * (1.0 - fade)));
                for h in &loops[1..] {
                    canvas.fill_rgb(&paper(h), [1.0; 3]);
                }
            }
            let Some(p) = named.as_deref().and_then(|n| self.pattern(n)) else {
                continue;
            };
            let p = self.at_scale(p, map.k());
            let (s, c, _) = self.stroke(*id, Pen::Fine);
            let s = Stroke {
                width: self.weights.hatch,
                ..s
            };
            for (a, b) in pattern_lines(&p, &loops[0], map.k()) {
                for (a, b) in crate::parts::clip_line(loops, a, b) {
                    let seg = [map.to_paper(a), map.to_paper(b)];
                    match c {
                        Some(c) => canvas.line_rgb(&seg, s, c),
                        None => canvas.line(&seg, s),
                    }
                }
            }
        }
    }

    fn look(&self, id: ElementId) -> Option<&Look> {
        self.by_id.get(&id)
    }

    /// The pattern element `id`'s surfaces seen in projection carry
    /// (its style's or its material's), if any.
    pub fn surface_pattern(&self, id: ElementId) -> Option<HatchPattern> {
        self.look(id)
            .and_then(|l| l.surface_pattern.as_deref())
            .and_then(|n| self.pattern(n))
    }

    /// Whether the project's patterns could draw differently from the
    /// built-in hatches: its pack has patterns of its own.
    pub fn has_own_patterns(&self) -> bool {
        !self.patterns.is_empty()
    }

    /// A pattern by name: the pack's, else a built-in one.
    pub fn pattern(&self, name: &str) -> Option<HatchPattern> {
        if let Some(p) = self.patterns.get(&name.to_lowercase()) {
            return Some(p.clone());
        }
        Hatch::ALL
            .iter()
            .find(|h| h.label().eq_ignore_ascii_case(name))
            .map(|h| styles::builtin_pattern(*h))
    }

    /// The items of a drawing in the order they draw: by their styles'
    /// draw order, lowest first, else as they come.
    pub fn order<'a>(&self, items: &'a [PlanItem]) -> Vec<&'a PlanItem> {
        let mut v: Vec<&PlanItem> = items.iter().collect();
        if self.ordered {
            v.sort_by_key(|it| self.look(it.id()).map_or(0, |l| l.order));
        }
        v
    }

    /// The stroke of a pen for element `id`, with its colour if it has one.
    pub fn stroke(&self, id: ElementId, pen: Pen) -> (Stroke, Option<[f32; 3]>, Option<Vec<f64>>) {
        let mut s = pen_stroke_with(pen, &self.weights);
        let Some(l) = self.look(id) else {
            return (s, None, None);
        };
        match pen {
            Pen::Cut => {
                if let Some(x) = l.cut {
                    s.width = x;
                }
            }
            Pen::Projection | Pen::Symbol | Pen::Fine | Pen::Pattern => {
                if let Some(x) = l.projection {
                    s.width = x;
                }
            }
            _ => {}
        }
        let mut long = None;
        if pen != Pen::Hidden {
            match &l.dashes {
                Some(None) => s.dash = None,
                Some(Some(d)) if d.len() == 2 && d[0] > 0.0 => s.dash = Some([d[0], d[1]]),
                Some(Some(d)) => {
                    s.dash = None;
                    long = Some(d.clone());
                }
                None => {}
            }
        }
        let rgb = l
            .colour
            .map(|c| c.map(|v| 1.0 - (1.0 - v) * (1.0 - l.fade)));
        if rgb.is_none() {
            s.gray += (1.0 - s.gray) * l.fade as f64;
        }
        (s, rgb, long)
    }

    /// Draw a polyline (paper mm) of element `id` in `pen`.
    pub fn line(&self, canvas: &mut dyn Canvas, id: ElementId, pen: Pen, pts: &[DVec2]) {
        let (s, rgb, long) = self.stroke(id, pen);
        let mut draw = |pts: &[DVec2]| match rgb {
            Some(c) => canvas.line_rgb(pts, s, c),
            None => canvas.line(pts, s),
        };
        match long {
            Some(d) => {
                for piece in dash_polyline(pts, &d, s.width) {
                    draw(&piece);
                }
            }
            None => draw(pts),
        }
    }

    /// The pattern and background a cut fill of element `id` takes, whose
    /// drawing says `hatch`.
    pub fn cut_pattern(
        &self,
        id: ElementId,
        hatch: Hatch,
    ) -> (Option<[f32; 3]>, Option<HatchPattern>) {
        self.cut_pattern_of(id, hatch, None)
    }

    /// As [`Self::cut_pattern`], for a cut whose material has colour
    /// `rgb` (a layer of a layered element is told apart by its hatch and
    /// its colour).
    pub fn cut_pattern_of(
        &self,
        id: ElementId,
        hatch: Hatch,
        rgb: Option<[f32; 3]>,
    ) -> (Option<[f32; 3]>, Option<HatchPattern>) {
        let builtin = || {
            // a layer's own material names a pattern
            let layer = self.look(id).and_then(|l| {
                let same = |c: &[f32; 3]| {
                    rgb.is_none_or(|r| r.iter().zip(c).all(|(a, b)| (a - b).abs() < 1e-3))
                };
                l.layer_patterns
                    .iter()
                    .find(|(h, c, _)| *h == hatch && same(c))
                    .or_else(|| l.layer_patterns.iter().find(|(h, _, _)| *h == hatch))
            });
            layer
                .and_then(|(_, _, n)| self.pattern(n))
                .or_else(|| self.pattern(hatch.label()))
        };
        let Some(l) = self.look(id) else {
            let p = builtin();
            return (p.as_ref().and_then(|p| p.fill.map(|c| c.0)), p);
        };
        let named = |n: &str| self.pattern(n);
        let pattern = match &l.material_pattern {
            // a layer of another material keeps its own hatch
            Some((h, name)) if *h == hatch => named(name),
            _ => None,
        }
        .or_else(|| match &l.category_pattern {
            Some(name) if l.material_pattern.as_ref().is_none_or(|(h, _)| *h == hatch) => {
                named(name)
            }
            _ => None,
        })
        .or_else(builtin);
        let fill = l
            .cut_fill
            .or_else(|| pattern.as_ref().and_then(|p| p.fill.map(|c| c.0)));
        let fill = fill.map(|c| c.map(|v| 1.0 - (1.0 - v) * (1.0 - l.fade)));
        let pattern = if l.hatch_off { None } else { pattern };
        (fill, pattern)
    }

    /// The colour behind cut piece `id`'s hatch, before any fade, as the
    /// sheets paint it: its material's colour where the drawing style
    /// gives one, else its style's Cut faces › Fill, else its pattern's
    /// Background, else white.
    pub fn cut_background(&self, id: ElementId, hatch: Hatch, rgb: Option<[f32; 3]>) -> [f32; 3] {
        let fade = self.look(id).map_or(0.0, |l| l.fade);
        let (fill, _) = self.cut_pattern_of(id, hatch, rgb);
        let unfade = |c: [f32; 3]| {
            if fade < 1.0 {
                c.map(|v| 1.0 - (1.0 - v) / (1.0 - fade))
            } else {
                c
            }
        };
        rgb.or(fill.map(unfade)).unwrap_or([1.0; 3])
    }

    /// True when a style sets a fill behind cut hatches.
    pub fn has_cut_fills(&self) -> bool {
        self.by_id.values().any(|l| l.cut_fill.is_some())
    }

    /// Draw a cut fill: its background, then its pattern's lines.
    #[allow(clippy::too_many_arguments)]
    pub fn fill(
        &self,
        canvas: &mut dyn Canvas,
        id: ElementId,
        points: &[DVec2],
        hatch: Hatch,
        rgb: Option<[f32; 3]>,
        material: Option<ElementId>,
        frame: Option<LayerFrame>,
        map: &PaperMap,
    ) {
        let (fill, pattern) = self.cut_pattern_of(id, hatch, rgb);
        let pts: Vec<DVec2> = points.iter().map(|p| map.to_paper(*p)).collect();
        // Colour and hatch: the material's colour under its hatch (MU32)
        canvas.fill_rgb(&pts, rgb.or(fill).unwrap_or([1.0; 3]));
        if let Some(p) = pattern {
            let p = self.at_scale(p, map.k());
            let fade = self.look(id).map_or(0.0, |l| l.fade);
            let s = Stroke {
                gray: 0.2 + 0.8 * fade as f64,
                ..Stroke::solid(self.weights.hatch, 0.2)
            };
            let (lines, plain) = self.cut_hatch(material, &p, points, map.k(), frame);
            if plain {
                draw_holes(canvas, &p, points, map, 0.2 + 0.8 * fade as f64);
            }
            for (a, b) in lines {
                canvas.line(&[map.to_paper(a), map.to_paper(b)], s);
            }
        }
    }

    /// The lines a cut piece is hatched with at `k` paper mm a drawing
    /// unit, by the view's row of the scale table: none at Fill or in a
    /// layer thinner than the row's limit, its material at true size where
    /// the row says so, else pattern `p` with its paper spacing opened up
    /// to the row's. True when they are `p`'s own lines (its holes go with
    /// them).
    pub fn cut_hatch(
        &self,
        material: Option<ElementId>,
        p: &HatchPattern,
        points: &[DVec2],
        k: f64,
        frame: Option<LayerFrame>,
    ) -> (Vec<(DVec2, DVec2)>, bool) {
        let Some(r) = self.rule else {
            return (pattern_lines(p, points, k), true);
        };
        if r.pattern == ScalePattern::Fill || thickness(points) * k < r.thin_layer {
            return (vec![], false);
        }
        if r.pattern == ScalePattern::TrueSize {
            if let Some(kind) = material.and_then(|m| self.kinds.get(&m).copied()) {
                let lines = true_size_lines(kind, points, k, r.hatch_spacing, frame);
                if !lines.is_empty() {
                    return (lines, false);
                }
            }
        }
        let open = (r.hatch_spacing / crate::paper::HATCH_SPACING).max(0.25);
        let k = if p.units == PatternUnits::Paper {
            k / open
        } else {
            k
        };
        (pattern_lines(p, points, k), true)
    }

    /// The pattern to draw at `k` paper mm a drawing unit: `p`, or the
    /// symbolic one it gives way to at small scales.
    pub fn at_scale(&self, p: HatchPattern, k: f64) -> HatchPattern {
        let mut p = match p
            .small_scale_name(1000.0 / k)
            .filter(|_| k > 0.0)
            .and_then(|n| self.pattern(n))
        {
            Some(small) => small,
            None => p,
        };
        // courses run level in sections and elevations; along the
        // outline only in plans
        if self.upright {
            p.along_longest = false;
        }
        p
    }

    /// Draw a surface seen in projection (a [`PlanItem::Shade`]): its
    /// style's fill and pattern, else `rgb`.
    pub fn shade(
        &self,
        canvas: &mut dyn Canvas,
        id: ElementId,
        points: &[DVec2],
        rgb: [f32; 3],
        map: &PaperMap,
    ) {
        let pts: Vec<DVec2> = points.iter().map(|p| map.to_paper(*p)).collect();
        let l = self.look(id);
        let fill = l.and_then(|l| l.surface_fill).unwrap_or(rgb);
        canvas.fill_rgb(&pts, fill);
        // a hairline of the same colour round it: a surface seen from above
        // comes in triangles, and viewers leave a pale seam between two
        // fills that only touch
        if pts.len() >= 3 {
            let mut ring = pts.clone();
            ring.push(pts[0]);
            let (s, _, _) = self.stroke(id, Pen::Fine);
            canvas.line_rgb(
                &ring,
                Stroke {
                    width: 0.05,
                    dash: None,
                    dots: 0,
                    ..s
                },
                fill,
            );
        }
        if let Some(p) = l
            .and_then(|l| l.surface_pattern.as_deref())
            .and_then(|n| self.pattern(n))
        {
            let p = self.at_scale(p, map.k());
            draw_holes(canvas, &p, points, map, 0.25);
            let (s, c, _) = self.stroke(id, Pen::Fine);
            let s = Stroke {
                width: self.weights.hatch,
                ..s
            };
            for (a, b) in pattern_lines(&p, points, map.k()) {
                let seg = [map.to_paper(a), map.to_paper(b)];
                match c {
                    Some(c) => canvas.line_rgb(&seg, s, c),
                    None => canvas.line(&seg, s),
                }
            }
        }
    }
}

/// How much lighter each element draws for its distance from the cut or
/// the viewer, in a view with distance fading.
fn distance_fades(model: &Model, v: &View) -> HashMap<ElementId, f32> {
    let mut out = HashMap::new();
    let fade = v.look.cues.fade;
    if fade <= 0.0 {
        return out;
    }
    // each element's nearest point, as a share of the view's depth
    let near: Box<dyn Fn(glam::DVec3, glam::DVec3) -> Option<f64>> = match &v.kind {
        ViewKind::Section {
            start, end, depth, ..
        } => {
            let frame = crate::section::SectionFrame::new(*start, *end, *depth);
            let depth = *depth;
            Box::new(move |lo, hi| {
                let d = corners(lo, hi)
                    .map(|c| frame.project(c).z)
                    .fold(f64::MAX, f64::min);
                Some((d.max(0.0) / depth.max(1e-6)).min(1.0))
            })
        }
        ViewKind::Plan { level, cut_height } if v.graphics.range.from_above => {
            let (top, bottom) = crate::ranges::from_above_span(model, v, *level, *cut_height);
            Box::new(move |_, hi| Some(((top - hi.z) / (top - bottom).max(1e-6)).clamp(0.0, 1.0)))
        }
        _ => return out,
    };
    for s in crate::scene::drawn_scene(model) {
        let Some((lo, hi)) = s.mesh.bounds() else {
            continue;
        };
        if let Some(t) = near(lo.as_dvec3(), hi.as_dvec3()) {
            let f = fade * t as f32;
            let e = out.entry(s.id).or_insert(f);
            *e = e.min(f);
        }
    }
    out
}

fn corners(lo: glam::DVec3, hi: glam::DVec3) -> impl Iterator<Item = glam::DVec3> {
    (0..8).map(move |i| {
        glam::DVec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        )
    })
}

/// Fill the holes of a drawn pattern (units cut right through) that lie
/// inside `polygon`, dark.
fn draw_holes(
    canvas: &mut dyn Canvas,
    p: &HatchPattern,
    polygon: &[DVec2],
    map: &PaperMap,
    grey: f64,
) {
    let has_holes = matches!(&p.recipe, Some(creabim_core::pattern::Recipe::Drawn(t))
        if t.strokes.iter().any(|s| s.closed && s.job == creabim_core::pattern::Job::Hole));
    if !has_holes {
        return;
    }
    let Some(l) = recipe_layout(p, polygon, map.k()) else {
        return;
    };
    let g = grey as f32;
    for (pts, job) in &l.shapes {
        if *job == creabim_core::pattern::Job::Hole && pts.iter().all(|q| point_in(*q, polygon)) {
            let paper: Vec<DVec2> = pts.iter().map(|q| map.to_paper(*q)).collect();
            canvas.fill_rgb(&paper, [g, g, g]);
        }
    }
}

/// The most lines one pattern draws in one fill; finer than that it
/// would only be grey, so it is left as its background.
const MAX_PATTERN_LINES: usize = 20_000;

/// The lines of a pattern clipped to `polygon` (drawing units), with
/// `k` paper millimetres per drawing unit. Dots come back as very short
/// segments.
pub fn pattern_lines(p: &HatchPattern, polygon: &[DVec2], k: f64) -> Vec<(DVec2, DVec2)> {
    let mut out = Vec::new();
    if polygon.len() < 3 || k <= 0.0 {
        return out;
    }
    let unit = match p.units {
        PatternUnits::Paper => 1.0 / k,
        PatternUnits::Model => 1.0,
    };
    let dot = 0.15 / k;
    for l in &p.lines {
        let mut angle = l.angle.to_radians();
        if l.along_longest {
            let n = polygon.len();
            angle += (0..n)
                .map(|i| polygon[(i + 1) % n] - polygon[i])
                .max_by(|a, b| a.length().total_cmp(&b.length()))
                .map_or(0.0, |d| d.to_angle());
        }
        let spacing = l.spacing * unit;
        let dashes: Vec<f64> = l.dashes.iter().map(|d| d * unit).collect();
        if !family(
            polygon,
            angle,
            spacing,
            l.offset * unit,
            l.shift * unit,
            l.phase * unit,
            &dashes,
            dot,
            &mut out,
        ) {
            return vec![];
        }
    }
    if let Some(r) = &p.recipe {
        recipe_lines(p, r, polygon, unit, &mut out);
    }
    if let Some(t) = &p.tile {
        let (w, h) = (t.size[0] * unit, t.size[1] * unit);
        let (lo, hi) = bounds(polygon);
        let cells = ((hi.x - lo.x) / w).ceil() * ((hi.y - lo.y) / h).ceil();
        if cells.is_nan() || cells >= MAX_PATTERN_LINES as f64 {
            return out;
        }
        let (i0, j0) = ((lo.x / w).floor() as i64, (lo.y / h).floor() as i64);
        let (i1, j1) = ((hi.x / w).ceil() as i64, (hi.y / h).ceil() as i64);
        for i in i0..i1 {
            for j in j0..j1 {
                let o = DVec2::new(i as f64 * w, j as f64 * h);
                for s in &t.strokes {
                    for pair in s.windows(2) {
                        let a = o + DVec2::new(pair[0][0], pair[0][1]) * unit;
                        let b = o + DVec2::new(pair[1][0], pair[1][1]) * unit;
                        for (u0, u1) in inside(a, b, polygon) {
                            out.push((a.lerp(b, u0), a.lerp(b, u1)));
                        }
                    }
                }
            }
        }
    }
    out
}

/// The frame a recipe is laid in over `polygon`: its origin and its x
/// direction, in the drawing.
pub fn recipe_frame(p: &HatchPattern, polygon: &[DVec2]) -> (DVec2, DVec2) {
    let mut origin = DVec2::ZERO;
    let mut angle = p.angle.to_radians();
    if p.along_longest && polygon.len() >= 2 {
        let n = polygon.len();
        if let Some(i) = (0..n).max_by(|a, b| {
            let la = (polygon[(a + 1) % n] - polygon[*a]).length();
            let lb = (polygon[(b + 1) % n] - polygon[*b]).length();
            la.total_cmp(&lb)
        }) {
            let d = polygon[(i + 1) % n] - polygon[i];
            angle += d.to_angle();
            origin = polygon[i];
        }
    }
    (origin, DVec2::from_angle(angle))
}

/// The layout of a pattern's recipe over `polygon` (drawing units), in the
/// drawing, or none when there is none or it is too fine to draw.
pub fn recipe_layout(
    p: &HatchPattern,
    polygon: &[DVec2],
    k: f64,
) -> Option<creabim_core::pattern::Layout> {
    let r = p.recipe.as_ref()?;
    let unit = match p.units {
        PatternUnits::Paper => 1.0 / k,
        PatternUnits::Model => 1.0,
    };
    let (origin, dir) = recipe_frame(p, polygon);
    let to_local = |q: DVec2| {
        let d = q - origin;
        DVec2::new(d.dot(dir), d.dot(dir.perp())) / unit
    };
    let local: Vec<DVec2> = polygon.iter().map(|q| to_local(*q)).collect();
    let (lo, hi) = bounds(&local);
    let mut l = r.layout(lo, hi, MAX_PATTERN_LINES)?;
    let back = |q: DVec2| origin + (dir * q.x + dir.perp() * q.y) * unit;
    for (pts, _) in l.shapes.iter_mut().chain(l.lines.iter_mut()) {
        for q in pts.iter_mut() {
            *q = back(*q);
        }
    }
    Some(l)
}

/// The outlines of a recipe's units and its lines, clipped to `polygon`.
fn recipe_lines(
    p: &HatchPattern,
    _r: &creabim_core::pattern::Recipe,
    polygon: &[DVec2],
    unit: f64,
    out: &mut Vec<(DVec2, DVec2)>,
) {
    let Some(l) = recipe_layout(p, polygon, 1.0 / unit) else {
        return;
    };
    let (lo, hi) = bounds(polygon);
    for (a, b) in l.edges() {
        // most units lie wholly inside or outside: skip far ones cheaply
        if a.max(b).cmplt(lo).any() || a.min(b).cmpgt(hi).any() {
            continue;
        }
        for (u0, u1) in inside(a, b, polygon) {
            out.push((a.lerp(b, u0), a.lerp(b, u1)));
        }
        if out.len() > MAX_PATTERN_LINES * 4 {
            return;
        }
    }
}

/// A layer's own axes in a drawing: along it (`u`), and its faces across
/// it (`v0`, `v1` along `u.perp()`), over every piece it was cut into, so
/// a true-size pattern runs on across the pieces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerFrame {
    pub u: DVec2,
    pub v0: f64,
    pub v1: f64,
}

/// The axes of one cut piece: along its longest side, and its extent
/// across.
pub fn piece_frame(poly: &[DVec2]) -> Option<LayerFrame> {
    let n = poly.len();
    if n < 3 {
        return None;
    }
    let u = (0..n)
        .map(|i| poly[(i + 1) % n] - poly[i])
        .max_by(|a, b| a.length().total_cmp(&b.length()))
        .and_then(|d| d.try_normalize())?;
    // one direction for both ways along a side
    let u = if u.x < -1e-9 || (u.x.abs() <= 1e-9 && u.y < 0.0) {
        -u
    } else {
        u
    };
    let v = u.perp();
    let (v0, v1) = poly.iter().fold((f64::MAX, f64::MIN), |(a, b), p| {
        (a.min(p.dot(v)), b.max(p.dot(v)))
    });
    Some(LayerFrame { u, v0, v1 })
}

/// The frame of each layer of a drawing's cut pieces drawn at true size:
/// pieces of one element and one material, running the same way, share
/// the extent of the largest of them (a layer cut in two by a member
/// through it keeps one row of loops).
pub fn layer_frames<'a>(
    items: impl IntoIterator<Item = &'a PlanItem>,
    looks: &Looks,
) -> HashMap<(ElementId, ElementId, i64), LayerFrame> {
    let mut out: HashMap<(ElementId, ElementId, i64), (f64, LayerFrame)> = HashMap::new();
    for it in items {
        let PlanItem::Fill {
            id,
            points,
            material: Some(m),
            ..
        } = it
        else {
            continue;
        };
        if !looks.kinds.contains_key(m) {
            continue;
        }
        let Some(f) = piece_frame(points) else {
            continue;
        };
        let key = (*id, *m, frame_bucket(f.u));
        let area = creabim_core::polygon::signed_area2(points).abs();
        out.entry(key)
            .and_modify(|(a, g)| {
                // the largest piece sets the faces; others reach past them only
                // where they really are wider (a piece split along its length)
                if area > *a {
                    *a = area;
                    *g = LayerFrame { u: f.u, ..*g };
                }
                g.v0 = g.v0.min(f.v0);
                g.v1 = g.v1.max(f.v1);
            })
            .or_insert((area, f));
    }
    out.into_iter().map(|(k, (_, f))| (k, f)).collect()
}

/// The frame `frames` holds for a cut piece of `id` in `material`.
pub fn frame_of(
    frames: &HashMap<(ElementId, ElementId, i64), LayerFrame>,
    id: ElementId,
    material: Option<ElementId>,
    points: &[DVec2],
) -> Option<LayerFrame> {
    let m = material?;
    let f = piece_frame(points)?;
    frames.get(&(id, m, frame_bucket(f.u))).copied()
}

/// Which way a piece runs, to the degree.
pub fn frame_bucket(u: DVec2) -> i64 {
    (u.y.atan2(u.x).to_degrees().rem_euclid(180.0)).round() as i64 % 180
}

/// How thick a cut piece is: twice its area over its perimeter, which is
/// its width for a long strip (a layer, a stud), in drawing units.
pub fn thickness(poly: &[DVec2]) -> f64 {
    let n = poly.len();
    if n < 3 {
        return 0.0;
    }
    let area = creabim_core::polygon::signed_area2(poly).abs() / 2.0;
    let perimeter: f64 = (0..n).map(|i| poly[i].distance(poly[(i + 1) % n])).sum();
    if perimeter <= 0.0 {
        0.0
    } else {
        2.0 * area / perimeter
    }
}

/// The parts of polyline `pts` inside `poly`, as segments: each kept where
/// its middle is inside (the steps are short).
fn inside_parts(pts: &[DVec2], poly: &[DVec2], out: &mut Vec<(DVec2, DVec2)>) {
    for w in pts.windows(2) {
        if creabim_core::polygon::contains(poly, (w[0] + w[1]) / 2.0) {
            out.push((w[0], w[1]));
        }
    }
}

/// A material's lines at its real size over cut piece `poly` (drawing
/// units, metres), `k` paper mm a unit: timber grain along a long member
/// and a cross on one cut across; batts as loops from face to face, sized
/// to the layer. `spacing` (paper mm) keeps grain lines apart at 1:20.
pub fn true_size_lines(
    kind: TrueKind,
    poly: &[DVec2],
    k: f64,
    spacing: f64,
    frame: Option<LayerFrame>,
) -> Vec<(DVec2, DVec2)> {
    let n = poly.len();
    if n < 3 || k <= 0.0 {
        return vec![];
    }
    // the layer's axes (or the piece's own): along it, and across it
    let Some(own) = piece_frame(poly) else {
        return vec![];
    };
    let f = frame
        .filter(|f| frame_bucket(f.u) == frame_bucket(own.u))
        .unwrap_or(own);
    let (u, v) = (f.u, f.u.perp());
    let (mut u0, mut u1) = (f64::MAX, f64::MIN);
    for p in poly {
        u0 = u0.min(p.dot(u));
        u1 = u1.max(p.dot(u));
    }
    // the loops' phase from the line, not the piece, so pieces of one
    // layer meet
    let (v0, v1) = (f.v0, f.v1);
    let (len, wide) = (u1 - u0, v1 - v0);
    if wide <= 0.0 {
        return vec![];
    }
    let at = |a: f64, b: f64| u * a + v * b;
    // short steps on paper, so a clipped end is never far out
    let step = 0.4 / k;
    let mut out = vec![];
    match kind {
        TrueKind::Timber if (own.v1 - own.v0) * 6.0 > len && len < 0.4 => {
            // cut across the grain: a cross corner to corner
            for (a, b) in [((u0, v0), (u1, v1)), ((u0, v1), (u1, v0))] {
                let d = DVec2::new(b.0 - a.0, b.1 - a.1);
                let m = ((d.length() / step).ceil() as usize).max(1);
                let pts: Vec<DVec2> = (0..=m)
                    .map(|i| {
                        let t = i as f64 / m as f64;
                        at(a.0 + d.x * t, a.1 + d.y * t)
                    })
                    .collect();
                inside_parts(&pts, poly, &mut out);
            }
        }
        TrueKind::Timber => {
            // grain along the member, gently waving
            let g = 0.011_f64.max(spacing * 0.8 / k);
            let amp = g * 0.15;
            let mut b = v0 + g / 2.0;
            while b < v1 {
                let m = ((len / step).ceil() as usize).max(1);
                let pts: Vec<DVec2> = (0..=m)
                    .map(|i| {
                        let a = u0 + len * i as f64 / m as f64;
                        at(
                            a,
                            b + amp * ((a + b * 7.0) / 0.26 * std::f64::consts::TAU).sin(),
                        )
                    })
                    .collect();
                inside_parts(&pts, poly, &mut out);
                b += g;
                if out.len() > MAX_PATTERN_LINES * 4 {
                    return vec![];
                }
            }
        }
        TrueKind::Batt => {
            // a prolate cycloid: loops a little narrower than the layer,
            // started on a whole number of loops so the pieces line up
            let period = wide * 0.75;
            let swing = 0.33 * period;
            let start = (u0 / period).floor() * period;
            let turns = (u1 - start) / period + 2.0;
            let dth = 0.12;
            let mut pts = vec![];
            let mut th = -std::f64::consts::TAU;
            while th <= turns * std::f64::consts::TAU {
                let a = start + period / std::f64::consts::TAU * th - swing * th.sin();
                let b = v0 + wide / 2.0 - wide / 2.0 * 0.86 * th.cos();
                pts.push(at(a, b));
                th += dth;
                if pts.len() > MAX_PATTERN_LINES * 4 {
                    return vec![];
                }
            }
            inside_parts(&pts, poly, &mut out);
        }
    }
    out
}

fn bounds(poly: &[DVec2]) -> (DVec2, DVec2) {
    poly.iter()
        .fold((DVec2::MAX, DVec2::MIN), |(a, b), p| (a.min(*p), b.max(*p)))
}

/// One family of parallel lines, `spacing` apart across them, into `out`.
/// False when it would be too many lines to draw.
#[allow(clippy::too_many_arguments)]
fn family(
    polygon: &[DVec2],
    angle: f64,
    spacing: f64,
    offset: f64,
    shift: f64,
    phase0: f64,
    dashes: &[f64],
    dot: f64,
    out: &mut Vec<(DVec2, DVec2)>,
) -> bool {
    if spacing <= 0.0 {
        return true;
    }
    let (dir, nrm) = (DVec2::from_angle(angle), DVec2::from_angle(angle).perp());
    let local: Vec<DVec2> = polygon
        .iter()
        .map(|p| DVec2::new(p.dot(dir), p.dot(nrm) - offset))
        .collect();
    let (lo, hi) = bounds(&local);
    if (hi.y - lo.y) / spacing > MAX_PATTERN_LINES as f64 {
        return false;
    }
    let period: f64 = dashes.iter().sum();
    // a corner this close to a line counts as on it, and on counts as
    // below: a line along an edge, as along a wall's mitre, is then drawn
    // by the one polygon above it, the same whatever the last bits of sin
    // and cos are on this machine (Windows and Linux differ there)
    let eps = spacing * 1e-9;
    let above = |v: f64, y: f64| v > y + eps;
    let mut k = (lo.y / spacing).ceil() as i64;
    while (k as f64) * spacing < hi.y {
        let y = k as f64 * spacing;
        let mut xs: Vec<f64> = Vec::new();
        for i in 0..local.len() {
            let (a, b) = (local[i], local[(i + 1) % local.len()]);
            if above(a.y, y) != above(b.y, y) {
                xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
            }
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        let at = |x: f64| dir * x + nrm * (y + offset);
        for pair in xs.chunks(2) {
            let [x0, x1] = pair else { continue };
            // a line through a corner alone has no length to draw
            if x1 - x0 <= eps {
                continue;
            }
            if dashes.is_empty() || period <= 0.0 {
                out.push((at(*x0), at(*x1)));
                continue;
            }
            if (x1 - x0) / period > MAX_PATTERN_LINES as f64 {
                return false;
            }
            // dashes start at x = shift * k, so rows stagger
            let phase = (k as f64 * shift + phase0).rem_euclid(period);
            let mut x = ((x0 - phase) / period).floor() * period + phase;
            while x < *x1 {
                let mut u = x;
                for (i, d) in dashes.iter().enumerate() {
                    if i % 2 == 0 {
                        let (a, b) = if *d <= 0.0 { (u, u + dot) } else { (u, u + d) };
                        let (a, b) = (a.max(*x0), b.min(*x1));
                        if b > a {
                            out.push((at(a), at(b)));
                        }
                    }
                    u += d;
                }
                x += period;
            }
        }
        k += 1;
        if out.len() > MAX_PATTERN_LINES * 4 {
            return false;
        }
    }
    true
}

/// The stretches of `a`→`b` inside a polygon (even-odd), as fractions.
fn inside(a: DVec2, b: DVec2, poly: &[DVec2]) -> Vec<(f64, f64)> {
    let d = b - a;
    let mut ts = vec![0.0, 1.0];
    let n = poly.len();
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        let e = q - p;
        let den = d.perp_dot(e);
        if den.abs() < 1e-12 {
            continue;
        }
        let t = (p - a).perp_dot(e) / den;
        let u = (p - a).perp_dot(d) / den;
        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
            ts.push(t);
        }
    }
    ts.sort_by(|x, y| x.total_cmp(y));
    ts.windows(2)
        .filter(|w| w[1] - w[0] > 1e-9 && point_in(a.lerp(b, (w[0] + w[1]) / 2.0), poly))
        .map(|w| (w[0], w[1]))
        .collect()
}

fn point_in(p: DVec2, poly: &[DVec2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}

/// A polyline broken into the dashes of `dashes` (paper mm; a dash of 0
/// is a dot as long as the line is wide), the pattern running on round
/// corners.
pub fn dash_polyline(pts: &[DVec2], dashes: &[f64], width: f64) -> Vec<Vec<DVec2>> {
    let period: f64 = dashes.iter().sum();
    if pts.len() < 2 || period <= 0.0 {
        return vec![pts.to_vec()];
    }
    let mut out: Vec<Vec<DVec2>> = Vec::new();
    let (mut i, mut left) = (0usize, dashes[0].max(width));
    let mut on = true;
    let mut cur: Vec<DVec2> = vec![pts[0]];
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = a.distance(b);
        let mut t = 0.0;
        while len - t > left {
            t += left;
            let p = a.lerp(b, t / len);
            if on {
                cur.push(p);
                out.push(std::mem::take(&mut cur));
            } else {
                cur = vec![p];
            }
            on = !on;
            i = (i + 1) % dashes.len();
            left = if on { dashes[i].max(width) } else { dashes[i] };
            if left <= 0.0 {
                left = 1e-6;
            }
        }
        left -= len - t;
        if on {
            cur.push(b);
        }
    }
    if on && cur.len() > 1 {
        out.push(cur);
    }
    out
}

/// The ids of the elements a view draws with a style of their own.
pub fn styled_ids(looks: &Looks) -> HashSet<ElementId> {
    looks.by_id.keys().copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use creabim_core::styles::HatchLine;

    fn square(s: f64) -> Vec<DVec2> {
        vec![
            DVec2::ZERO,
            DVec2::new(s, 0.0),
            DVec2::new(s, s),
            DVec2::new(0.0, s),
        ]
    }

    /// The cut hatch of 1.2's walls: a wall's mitred corner lies along a
    /// 45° line of the pattern. Linux drew a stroke along the mitre that
    /// Windows didn't (their sin and cos differ in the last bit), one more
    /// line in every plan. Nudged a ten-trillionth either way across the
    /// line, the pattern must come out the same.
    #[test]
    fn a_pattern_line_along_an_edge_is_cut_the_same_whatever_the_last_bits() {
        let mut p = styles::builtin_pattern(Hatch::Diagonal);
        p.lines = vec![HatchLine {
            angle: 45.0,
            spacing: 1.0,
            offset: 0.0,
            shift: 0.0,
            dashes: vec![],
            along_longest: false,
            ..Default::default()
        }];
        p.units = PatternUnits::Model;
        let wall = |nudge: f64| {
            vec![
                DVec2::new(0.0, 0.0),
                // the mitre: (0, 0) to (3, 3), along the line through (0, 0)
                DVec2::new(3.0 - nudge, 3.0 + nudge),
                DVec2::new(3.0, 20.0),
                DVec2::new(0.0, 20.0),
            ]
        };
        let lines = |nudge: f64| {
            let mut v: Vec<[i64; 4]> = pattern_lines(&p, &wall(nudge), 1.0)
                .into_iter()
                .map(|(a, b)| {
                    let r = |v: f64| (v * 1e6).round() as i64;
                    [r(a.x), r(a.y), r(b.x), r(b.y)]
                })
                .collect();
            v.sort();
            v
        };
        let exact = lines(0.0);
        assert_eq!(lines(1e-13), exact, "a corner a hair above the line");
        assert_eq!(lines(-1e-13), exact, "a corner a hair below the line");
        assert!(pattern_lines(&p, &wall(1e-13), 1.0)
            .iter()
            .all(|(a, b)| a.distance(*b) > 1e-6));
    }

    #[test]
    fn model_patterns_keep_their_true_size_and_paper_ones_do_not() {
        let mut p = styles::builtin_pattern(Hatch::Diagonal);
        p.lines = vec![HatchLine {
            angle: 0.0,
            spacing: 0.1,
            offset: 0.0,
            shift: 0.0,
            dashes: vec![],
            along_longest: false,
            ..Default::default()
        }];
        p.units = PatternUnits::Model;
        // ten courses a metre whatever the scale
        assert_eq!(pattern_lines(&p, &square(1.0), 10.0).len(), 10);
        assert_eq!(pattern_lines(&p, &square(1.0), 20.0).len(), 10);
        // on paper: 0.1 mm apart, so 100 a metre at 1:1000 (k = 1)
        p.units = PatternUnits::Paper;
        assert_eq!(pattern_lines(&p, &square(1.0), 1.0).len(), 10);
        assert_eq!(pattern_lines(&p, &square(1.0), 2.0).len(), 20);
        // far too fine to draw: nothing, the fill stays
        assert!(pattern_lines(&p, &square(100.0), 1000.0).is_empty());
    }

    #[test]
    fn dashes_and_dots_follow_the_pattern() {
        let mut p = styles::builtin_pattern(Hatch::Diagonal);
        p.units = PatternUnits::Model;
        p.lines = vec![HatchLine {
            angle: 0.0,
            spacing: 0.5,
            offset: 0.25,
            shift: 0.0,
            dashes: vec![0.1, 0.1],
            along_longest: false,
            ..Default::default()
        }];
        let lines = pattern_lines(&p, &square(1.0), 10.0);
        // two rows, five dashes each
        assert_eq!(lines.len(), 10);
        assert!(lines
            .iter()
            .all(|(a, b)| (a.distance(*b) - 0.1).abs() < 1e-9));
        let pieces = dash_polyline(
            &[DVec2::ZERO, DVec2::new(10.0, 0.0)],
            &[6.0, 1.0, 1.0, 1.0],
            0.1,
        );
        assert_eq!(pieces.len(), 3);
        assert!((pieces[0][1].x - 6.0).abs() < 1e-9);
        assert!((pieces[1][0].x - 7.0).abs() < 1e-9);
    }

    fn total(lines: &[(DVec2, DVec2)]) -> f64 {
        lines.iter().map(|(a, b)| a.distance(*b)).sum()
    }

    #[test]
    fn a_bond_draws_its_brick_outlines_inside_the_fill() {
        use creabim_core::pattern::{Bond, Recipe};
        let p = HatchPattern::from_recipe(
            "Brick",
            Recipe::Bond {
                bond: Bond::Stretcher,
                length: 0.215,
                height: 0.065,
                joint: 0.01,
            },
        );
        let lines = pattern_lines(&p, &square(1.0), 50.0);
        assert!(!lines.is_empty());
        for (a, b) in &lines {
            for q in [a, b] {
                assert!(q.x > -1e-9 && q.x < 1.0 + 1e-9 && q.y > -1e-9 && q.y < 1.0 + 1e-9);
            }
        }
        // the same bond written as .pat line families draws the same lines
        let fam = HatchPattern {
            name: "Brick lines".into(),
            units: PatternUnits::Model,
            lines: creabim_core::patfile::lines_of(&p).unwrap(),
            ..Default::default()
        };
        let (a, b) = (
            total(&lines),
            total(&pattern_lines(&fam, &square(1.0), 50.0)),
        );
        assert!((a - b).abs() / a < 0.02, "{a} {b}");
    }

    #[test]
    fn along_the_longest_side_the_courses_follow_a_wall() {
        use creabim_core::pattern::{Bond, Recipe};
        let mut p = HatchPattern::from_recipe(
            "Brick",
            Recipe::Bond {
                bond: Bond::Stack,
                length: 0.5,
                height: 0.1,
                joint: 0.0,
            },
        );
        p.along_longest = true;
        // a wall 2 m long, 0.2 thick, running up the drawing
        let wall = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(0.0, 2.0),
            DVec2::new(-0.2, 2.0),
            DVec2::new(-0.2, 0.0),
        ];
        let lines = pattern_lines(&p, &wall, 50.0);
        // the long joints run up the wall
        let up: f64 = lines
            .iter()
            .filter(|(a, b)| (a.x - b.x).abs() < 1e-9)
            .map(|(a, b)| a.distance(*b))
            .sum();
        let across: f64 = lines
            .iter()
            .filter(|(a, b)| (a.y - b.y).abs() < 1e-9)
            .map(|(a, b)| a.distance(*b))
            .sum();
        assert!(up > across * 2.0, "{up} {across}");
    }

    #[test]
    fn a_pattern_gives_way_to_its_symbolic_one_at_small_scales() {
        use creabim_core::pattern::{Bond, Recipe};
        let mut p = HatchPattern::from_recipe(
            "Coursing",
            Recipe::Bond {
                bond: Bond::Stretcher,
                length: 0.215,
                height: 0.065,
                joint: 0.01,
            },
        );
        p.small_scale = Some(styles::SmallScale {
            below: 50.0,
            pattern: "Brick".into(),
        });
        let looks = Looks::plain(LineWeights::for_scale(50.0));
        // 1:20 keeps it, 1:100 takes the built-in brick hatch
        assert_eq!(looks.at_scale(p.clone(), 50.0).name, "Coursing");
        assert_eq!(looks.at_scale(p, 10.0).name, "Brick");
    }

    #[test]
    fn tiles_repeat_and_clip() {
        let p = HatchPattern {
            name: "t".into(),
            units: PatternUnits::Model,
            fill: None,
            lines: vec![],
            tile: Some(styles::Tile {
                size: [0.5, 0.5],
                strokes: vec![vec![[0.0, 0.25], [0.5, 0.25]]],
            }),
            ..Default::default()
        };
        let lines = pattern_lines(&p, &square(1.0), 10.0);
        assert_eq!(lines.len(), 4);
    }
}

#[cfg(test)]
mod scale_table_hatch_tests {
    use super::*;

    fn strip(w: f64, h: f64) -> Vec<DVec2> {
        vec![
            DVec2::ZERO,
            DVec2::new(w, 0.0),
            DVec2::new(w, h),
            DVec2::new(0.0, h),
        ]
    }

    #[test]
    fn a_strip_is_as_thick_as_it_is_wide() {
        assert!((thickness(&strip(3.0, 0.145)) - 0.145).abs() < 0.01);
    }

    #[test]
    fn timber_and_batts_draw_at_their_real_size() {
        // a 45 x 145 stud cut across: one cross, two lines' worth
        let stud = true_size_lines(TrueKind::Timber, &strip(0.045, 0.145), 200.0, 3.0, None);
        assert!(!stud.is_empty());
        // a rafter along its length: grain lines, the same at 1:5 and 1:10
        // in metres (true size), more lines on paper at the larger scale
        let rafter = strip(2.0, 0.195);
        let at5 = true_size_lines(TrueKind::Timber, &rafter, 200.0, 3.0, None);
        let grain_rows = |v: &[(DVec2, DVec2)]| {
            let mut ys: Vec<i64> = v
                .iter()
                .map(|(a, _)| (a.y * 1000.0 / 11.0).round() as i64)
                .collect();
            ys.sort();
            ys.dedup();
            ys.len()
        };
        assert!(grain_rows(&at5) >= 12, "{}", grain_rows(&at5));
        // batts: loops between the faces, every point inside the layer
        let batt = strip(1.0, 0.145);
        let loops = true_size_lines(TrueKind::Batt, &batt, 200.0, 3.0, None);
        assert!(!loops.is_empty());
        for (a, b) in &loops {
            for p in [a, b] {
                assert!(p.y > -0.01 && p.y < 0.155, "{p:?}");
            }
        }
    }

    #[test]
    fn a_row_fills_thin_layers_and_opens_plain_hatches() {
        let mut looks = Looks::plain(LineWeights::for_scale(100.0));
        let p = creabim_core::styles::builtin_pattern(Hatch::Diagonal);
        let wall = strip(4.0, 0.2);
        let membrane = strip(4.0, 0.0015);
        let old = looks.cut_hatch(None, &p, &wall, 10.0, None).0.len();
        // the 1:100 row: its spacing; a 1.5 mm membrane is filled, no lines
        looks.rule = Some(creabim_core::styles::shipped_scale_rules()[4]);
        assert!(looks
            .cut_hatch(None, &p, &membrane, 10.0, None)
            .0
            .is_empty());
        // the 1:50 row opens the same hatch up
        looks.rule = Some(creabim_core::styles::shipped_scale_rules()[3]);
        let open = looks.cut_hatch(None, &p, &wall, 20.0, None).0.len();
        let dense = Looks::plain(LineWeights::for_scale(50.0))
            .cut_hatch(None, &p, &wall, 20.0, None)
            .0
            .len();
        assert!(open < dense, "{open} {dense}");
        let _ = old;
        // 1:200: fill only
        looks.rule = Some(creabim_core::styles::shipped_scale_rules()[5]);
        assert!(looks.cut_hatch(None, &p, &wall, 5.0, None).0.is_empty());
    }

    #[test]
    fn the_pieces_of_one_layer_share_its_frame() {
        let m = ElementId::new();
        let mut looks = Looks::plain(LineWeights::for_scale(10.0));
        looks.kinds.insert(m, TrueKind::Batt);
        let id = ElementId::new();
        let piece = |pts: Vec<DVec2>| PlanItem::Fill {
            id,
            points: pts,
            hatch: Hatch::Cross,
            rgb: None,
            material: Some(m),
        };
        // a 0.4 m layer cut along its length into a 0.3 and a 0.1 m piece
        let items = vec![
            piece(strip(3.0, 0.3)),
            piece(vec![
                DVec2::new(0.0, 0.3),
                DVec2::new(3.0, 0.3),
                DVec2::new(3.0, 0.4),
                DVec2::new(0.0, 0.4),
            ]),
        ];
        let frames = layer_frames(&items, &looks);
        assert_eq!(frames.len(), 1);
        let f = frames.values().next().unwrap();
        assert!((f.v1 - f.v0 - 0.4).abs() < 1e-9, "{f:?}");
    }
}
