//! Style packs on drawings: the Standard pack draws as CreaBIM did before
//! packs, another pack restyles the whole set with no code change, and a
//! view's template and overrides change only its own drawing.

use creabim_core::styles::{self, LinePattern, Override, StylePack, Target};
use creabim_core::*;
use creabim_geom::looks::Looks;
use creabim_geom::plan::{Hatch, Pen};
use creabim_geom::sheet::{sheet_svg, sheets, SheetInfo};

fn info() -> SheetInfo {
    SheetInfo {
        project: "Test".into(),
        date: "2026-11-04".into(),
    }
}

fn with_pack(m: &Model, p: &StylePack) -> Model {
    let mut m = m.clone();
    styles::apply_pack(&m, p).apply(&mut m).unwrap();
    m
}

fn edit_view(m: &mut Model, view: ElementId, f: impl FnOnce(&mut View)) {
    let before = m.get(view).unwrap().clone();
    let mut e = before.clone();
    if let ElementKind::View(v) = &mut e.kind {
        f(v);
    }
    Transaction::new("t").replace(before, e).apply(m).unwrap();
}

fn first_plan(m: &Model) -> ElementId {
    m.iter()
        .find(
            |e| matches!(&e.kind, ElementKind::View(v) if matches!(v.kind, ViewKind::Plan { .. })),
        )
        .unwrap()
        .id
}

fn view_of(m: &Model, id: ElementId) -> View {
    match &m.get(id).unwrap().kind {
        ElementKind::View(v) => v.clone(),
        _ => unreachable!(),
    }
}

/// Stroke widths written in a PDF, with how often each is used.
fn pdf_widths(pdf: &[u8]) -> std::collections::BTreeMap<String, usize> {
    let text = String::from_utf8_lossy(pdf);
    let mut out = std::collections::BTreeMap::new();
    let words: Vec<&str> = text.split_whitespace().collect();
    for w in words.windows(2) {
        if w[1] == "w" && w[0].parse::<f64>().is_ok() {
            *out.entry(w[0].to_string()).or_insert(0) += 1;
        }
    }
    out
}

/// The ground floor plan sheet (1:50) as a PDF, as `sheets_pdf` makes it.
fn plan_pdf(m: &Model) -> Vec<u8> {
    let (id, s) = sheets(m)
        .into_iter()
        .find(|(_, s)| s.number == "A101")
        .unwrap();
    let views = creabim_geom::sheet::placed_views(m, id, &mut |v| {
        creabim_geom::section::view_drawing(m, v)
    });
    let mut pdf = creabim_geom::paper::PdfWriter::new();
    let mut page = pdf.page(glam::DVec2::new(s.width, s.height));
    creabim_geom::sheet::draw_sheet(&mut page, m, id, &views, &info());
    pdf.finish()
}

#[test]
fn the_standard_pack_draws_plans_and_schedules_as_before() {
    let house = starter::starter_house();
    let std = with_pack(&house, &StylePack::standard());
    // the same marks on paper; hidden-line runs may come in another order
    let lines = |m: &Model, id| {
        let mut v: Vec<String> = sheet_svg(m, id, &info())
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
        v.sort();
        v
    };
    // plans and schedules: a section's cut outlines can start at another
    // corner from one run to the next, so sections are left to the PDF test
    for (id, s) in sheets(&house) {
        if s.number.starts_with("A1") || s.number.starts_with("A6") {
            assert_eq!(lines(&house, id), lines(&std, id), "sheet {}", s.number);
        }
    }
}

#[test]
fn two_packs_give_two_sets_with_their_own_line_weights_in_pdf() {
    let house = starter::starter_house();
    let packs = StylePack::shipped();
    let a = plan_pdf(&with_pack(&house, &packs[0]));
    let b = plan_pdf(&with_pack(&house, &packs[1]));
    assert_ne!(a, b);
    let (wa, wb) = (pdf_widths(&a), pdf_widths(&b));
    // the plans at 1:50 cut with 0.70 mm in one, 0.50 mm in the other
    assert!(wa.contains_key("0.700"), "{wa:?}");
    assert!(!wb.contains_key("0.700"), "{wb:?}");
    assert!(wb.contains_key("0.500"));
    // the light pack's own fine pens are in its PDF, not the standard's
    assert!(
        wb.contains_key("0.100") && !wa.contains_key("0.100"),
        "{wa:?} {wb:?}"
    );
    // and it draws in colour: furniture grey, doors blue-grey
    let rgb = |pdf: &[u8]| String::from_utf8_lossy(pdf).matches(" RG ").count();
    assert!(rgb(&b) > rgb(&a) + 50, "{} {}", rgb(&b), rgb(&a));
}

#[test]
fn a_pattern_named_like_a_built_in_restyles_every_material_with_that_hatch() {
    let house = starter::starter_house();
    let light = with_pack(&house, &StylePack::shipped()[1]);
    let view = first_plan(&light);
    let looks = Looks::for_view(&light, view, &view_of(&light, view));
    // the light pack's own Brick is true size, the built-in on paper
    let (_, p) = looks.cut_pattern(ElementId::NIL, Hatch::Brick);
    assert_eq!(p.unwrap().units, styles::PatternUnits::Model);
    let plain = Looks::plain(LineWeights::for_scale(100.0));
    let (fill, p) = plain.cut_pattern(ElementId::NIL, Hatch::Brick);
    assert_eq!(p.unwrap().units, styles::PatternUnits::Paper);
    assert!(fill.is_some());
    // a wall takes its category's style: a pale fill, no hatch lines
    let wall = light
        .iter()
        .find(|e| matches!(e.kind, ElementKind::Wall(_)))
        .unwrap();
    let hatch = creabim_geom::plan::hatch_of(&light, wall.id);
    let (fill, p) = looks.cut_pattern(wall.id, hatch);
    assert_eq!(styles::Rgb(fill.unwrap()).hex(), "#c9d3de");
    assert!(p.unwrap().lines.is_empty());
    // a material naming a pattern of the pack's takes it
    let mut m = light.clone();
    let mat = m
        .iter()
        .find(|e| matches!(e.kind, ElementKind::Material(_)))
        .unwrap()
        .clone();
    let mut after = mat.clone();
    if let ElementKind::Material(x) = &mut after.kind {
        x.pattern = Some("Concrete dots".into());
    }
    Transaction::new("t")
        .replace(mat, after)
        .apply(&mut m)
        .unwrap();
    assert!(styles::active_pack(&m).pattern("concrete dots").is_some());
}

#[test]
fn overrides_change_one_view_only() {
    let mut m = starter::starter_house();
    let plans: Vec<ElementId> = m
        .iter()
        .filter(
            |e| matches!(&e.kind, ElementKind::View(v) if matches!(v.kind, ViewKind::Plan { .. })),
        )
        .map(|e| e.id)
        .collect();
    let (a, b) = (plans[0], plans[1]);
    let wall = m
        .iter()
        .find(|e| matches!(e.kind, ElementKind::Wall(_)))
        .unwrap()
        .id;
    let looks = |m: &Model, v| Looks::for_view(m, v, &styles::resolved(m, &view_of(m, v)));
    let before = looks(&m, a).stroke(wall, Pen::Cut);
    assert_eq!(before.0.gray, 0.0);
    edit_view(&mut m, a, |v| {
        v.look
            .override_mut(Target::Category(Category::Walls))
            .halftone = true;
        let o: &mut Override = v.look.override_mut(Target::Element(wall));
        o.pattern = Some(LinePattern::Dashed);
    });
    let (s, rgb, _) = looks(&m, a).stroke(wall, Pen::Cut);
    assert_eq!(rgb, None);
    assert!((s.gray - 0.5).abs() < 1e-6, "{}", s.gray);
    assert_eq!(s.dash, Some([2.0, 1.0]));
    // the other plan is as it was
    assert_eq!(looks(&m, b).stroke(wall, Pen::Cut), before);
    // and the override is saved with the view
    let back = Model::from_json(&m.to_json()).unwrap();
    assert_eq!(view_of(&back, a).look, view_of(&m, a).look);
}

#[test]
fn a_template_sets_the_scale_and_pens_of_every_view_using_it() {
    let house = starter::starter_house();
    let mut pack = StylePack::standard();
    pack.templates[0].scale = Some(200.0);
    let mut m = with_pack(&house, &pack);
    let view = first_plan(&m);
    let name = pack.templates[0].name.clone();
    styles::set_template(&m, &[view], Some(&name))
        .apply(&mut m)
        .unwrap();
    let v = view_of(&m, view);
    assert_eq!(styles::resolved(&m, &v).scale, 200.0);
    let sheet = sheets(&m)
        .into_iter()
        .find(|(s, _)| {
            m.iter().any(|e| {
                matches!(&e.kind, ElementKind::Viewport(vp) if vp.sheet == *s && vp.view == view)
            })
        })
        .map(|s| s.0)
        .unwrap();
    let placed = creabim_geom::sheet::placed_views(&m, sheet, &mut |v| {
        creabim_geom::section::view_drawing(&m, v)
    });
    let p = placed.iter().find(|p| p.view == view).unwrap();
    assert_eq!(p.map.scale, 200.0);
    assert_eq!(p.weights, LineWeights::for_scale(200.0));
}

#[test]
fn a_style_fills_and_patterns_site_areas_that_have_no_pattern_of_their_own() {
    use creabim_core::site::{Pattern, Site};
    use creabim_geom::paper::{draw_looks, PaperMap, SvgCanvas};
    use creabim_geom::plan::PlanView;
    use glam::DVec2;
    let lv = Element::new(
        "L0",
        ElementKind::Level(Level {
            elevation: 0.0,
            height: 3.0,
        }),
    );
    let sq = vec![
        DVec2::ZERO,
        DVec2::new(4.0, 0.0),
        DVec2::new(4.0, 4.0),
        DVec2::new(0.0, 4.0),
    ];
    let bare = Element::new(
        "Bare",
        ElementKind::Site(Site::area(lv.id, sq.clone(), 0.1, Pattern::None)),
    );
    let paved = Element::new(
        "Paved",
        ElementKind::Site(Site::area(
            lv.id,
            sq.iter().map(|p| *p + DVec2::X * 6.0).collect(),
            0.1,
            Pattern::Lines {
                spacing: 0.5,
                angle: 0.0,
            },
        )),
    );
    let view = Element::new("Plan", ElementKind::View(View::plan(lv.id)));
    let m = Model::from_elements([lv, bare.clone(), paved.clone(), view.clone()]).unwrap();
    let svg = |m: &Model, only: ElementId| {
        let v = view_of(m, view.id);
        let looks = Looks::for_view(m, view.id, &styles::resolved(m, &v));
        let e = m.get(only).unwrap();
        let drawing = PlanView {
            items: creabim_geom::parts::plan_items(m, e, 1.2),
        };
        let mut c = SvgCanvas::new(DVec2::new(200.0, 200.0));
        let map = PaperMap {
            model: DVec2::ZERO,
            paper: DVec2::new(10.0, 10.0),
            scale: 50.0,
        };
        draw_looks(&mut c, &drawing, &map, &looks);
        c.finish()
    };
    let lines = |s: &str| s.matches("<polyline").count() + s.matches("<line").count();
    let plain_bare = svg(&m, bare.id);
    let plain_paved = svg(&m, paved.id);
    // a pack that fills site areas and gives them a pattern
    let mut pack = StylePack::standard();
    let mut st = styles::ObjectStyle::new(visibility::Category::Site);
    st.surface_fill = Some(styles::Rgb([0.9, 0.85, 0.8]));
    st.surface_pattern = Some(Hatch::Diagonal.label().into());
    pack.objects.push(st);
    let m = with_pack(&m, &pack);
    let styled_bare = svg(&m, bare.id);
    assert!(
        lines(&styled_bare) > lines(&plain_bare) + 4,
        "{styled_bare}"
    );
    assert!(styled_bare.len() > plain_bare.len());
    // an area's own pattern stays: only the fill is added
    let styled_paved = svg(&m, paved.id);
    assert_eq!(lines(&styled_paved), lines(&plain_paved));
    assert!(styled_paved.len() > plain_paved.len());
}
