//! Timing probe: how much a mass on a level costs a plan and the 3D scene.
use creabim_core::{Element, ElementKind, Transaction};

#[test]
#[ignore = "timing probe"]
fn mass_cost() {
    let m = creabim_core::starter::starter_house();
    let (plan, level) = m
        .iter()
        .find_map(|e| match &e.kind {
            ElementKind::View(v) => match v.kind {
                creabim_core::ViewKind::Plan { level, .. } => Some((e.id, level)),
                _ => None,
            },
            _ => None,
        })
        .unwrap();
    let time = |m: &creabim_core::Model, what: &str| {
        let v = m.get(plan).unwrap().clone();
        let ElementKind::View(view) = &v.kind else {
            panic!()
        };
        let t = std::time::Instant::now();
        for _ in 0..5 {
            let _ = creabim_geom::section::view_drawing(m, plan);
        }
        let p = t.elapsed().as_secs_f64() * 200.0;
        let t = std::time::Instant::now();
        for _ in 0..5 {
            let _ = creabim_geom::scene::build_scene_of(m, None);
        }
        let s = t.elapsed().as_secs_f64() * 200.0;
        let _ = view;
        println!("PROBE {what}: plan {p:.1} ms, scene {s:.1} ms");
    };
    time(&m, "no mass");
    let mut doc = creabim_core::document::Document::from_model(m);
    let mass = creabim_core::FreeMesh::new(
        Some(level),
        glam::DVec3::new(-6.0, -10.0, 0.0),
        creabim_core::editmesh::EditMesh::cuboid(glam::DVec3::new(2.0, 1.0, 3.0)),
    );
    doc.commit(Transaction::new("m").insert(Element::new("Mass", ElementKind::FreeMesh(mass))))
        .unwrap();
    time(doc.model(), "one mass");
}
