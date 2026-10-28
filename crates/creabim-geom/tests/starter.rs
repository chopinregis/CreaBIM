//! Drawing checks on the starter house: what a new user sees first.

use creabim_core::starter::starter_house;
use creabim_core::*;
use creabim_geom::plan::Pen;
use creabim_geom::{build_scene, plan_view, PlanItem};
use glam::DVec2;

fn level(m: &Model, name: &str) -> ElementId {
    m.iter()
        .find(|e| e.name == name && matches!(e.kind, ElementKind::Level(_)))
        .unwrap_or_else(|| panic!("no level {name}"))
        .id
}

fn first(m: &Model, name: &str) -> ElementId {
    m.iter().find(|e| e.name == name).unwrap().id
}

#[test]
fn the_first_floor_shows_the_stair_coming_up_through_its_well() {
    let m = starter_house();
    let plan = plan_view(&m, level(&m, "First Floor"), 1.2);
    let stair = first(&m, "Stair");
    let well = first(&m, "Stair Well");
    // the stair's treads, seen from above, marked down
    let treads = plan
        .items
        .iter()
        .filter(|i| matches!(i, PlanItem::Line { id, pen: Pen::Projection, .. } if *id == stair))
        .count();
    assert!(treads > 10, "only {treads} stair lines upstairs");
    assert!(plan
        .items
        .iter()
        .any(|i| matches!(i, PlanItem::Label { id, text, .. } if *id == stair && text == "DN")));
    // and the well round it, with no cross over the stair
    assert!(!plan
        .items
        .iter()
        .any(|i| matches!(i, PlanItem::Line { id, pen: Pen::Symbol, .. } if *id == well)));
}

#[test]
fn footing_outlines_never_run_inside_the_footings() {
    let m = starter_house();
    let plan = plan_view(&m, level(&m, "Foundation"), 0.2);
    let fills: Vec<&Vec<DVec2>> = plan
        .items
        .iter()
        .filter_map(|i| match i {
            PlanItem::Fill { points, .. } => Some(points),
            _ => None,
        })
        .collect();
    let mut inside = 0;
    for i in &plan.items {
        if let PlanItem::Line {
            pen: Pen::Cut,
            a,
            b,
            ..
        } = i
        {
            let mid = (*a + *b) / 2.0;
            // a little either side of the line: both in footing means the
            // line runs through the middle of the concrete
            let n = (*b - *a).normalize_or_zero().perp() * 0.02;
            let filled = |p: DVec2| fills.iter().any(|f| polygon::contains(f, p));
            if filled(mid + n) && filled(mid - n) {
                inside += 1;
            }
        }
    }
    assert_eq!(
        inside, 0,
        "{inside} footing outlines run inside the footings"
    );
}

#[test]
fn the_first_floor_slab_stops_at_the_inner_face_of_the_walls_it_sits_in() {
    let m = starter_house();
    let slab = m
        .iter()
        .find(|e| matches!(&e.kind, ElementKind::Slab(s) if s.level == level(&m, "First Floor")))
        .unwrap();
    let ElementKind::Slab(s) = &slab.kind else {
        unreachable!()
    };
    // the street wall runs the whole south edge, its outer face on the
    // slab's edge: the slab stops at its inner face, 0.4 in
    let south = s.outline().iter().map(|p| p.y).fold(f64::MAX, f64::min);
    for item in build_scene(&m).iter().filter(|i| i.id == slab.id) {
        for p in &item.mesh.positions {
            assert!(
                p[1] as f64 > south + 0.39,
                "the slab reaches y = {} in the street wall",
                p[1]
            );
        }
    }
}
