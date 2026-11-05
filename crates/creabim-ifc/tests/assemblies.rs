//! Layered assemblies through IFC: walls and slabs go out with an
//! IfcMaterialLayerSetUsage on their assembly's IfcMaterialLayerSet (a
//! roof with the set itself), and come back built of the same assembly,
//! layer for layer, the exterior on the same side. IFCX carries them too.

use creabim_core::assembly::{assemblies, assembly_of};
use creabim_core::{
    sample::layered_house, Element, ElementKind, LayerFunction, Model, Transaction,
};
use creabim_ifc::{export_ifc4, export_ifcx, import_ifc, import_ifcx, ExportOptions};

/// A layer as (function, material name, thickness in mm).
type LayerRow = (LayerFunction, String, i64);

/// (element name, assembly name, layers, flip)
fn layered(m: &Model) -> Vec<(String, String, Vec<LayerRow>, bool)> {
    let mut out: Vec<_> = m
        .iter()
        .filter(|e| matches!(e.kind, ElementKind::Wall(_) | ElementKind::Slab(_)))
        .filter_map(|e| {
            let (ae, a) = assembly_of(m, e)?;
            let layers = a
                .layers
                .iter()
                .map(|l| {
                    let mat = l
                        .material
                        .and_then(|id| m.get(id))
                        .map_or(String::new(), |e| e.name.clone());
                    (l.function, mat, (l.thickness * 1000.0).round() as i64)
                })
                .collect();
            let flip = matches!(&e.kind, ElementKind::Wall(w) if w.flip);
            Some((e.name.clone(), ae.name.clone(), layers, flip))
        })
        .collect();
    out.sort();
    out
}

fn with_a_flipped_wall() -> Model {
    let m = layered_house();
    let wall = m
        .iter()
        .find(|e| e.name == "GF Wall 2")
        .expect("a ground floor wall")
        .clone();
    let mut after: Element = wall.clone();
    if let ElementKind::Wall(w) = &mut after.kind {
        w.flip = true;
    }
    let mut doc = creabim_core::Document::from_model(m);
    doc.commit(Transaction::new("Flip").replace(wall, after))
        .unwrap();
    doc.model().clone()
}

#[test]
fn layered_walls_and_slabs_round_trip_through_ifc() {
    let model = with_a_flipped_wall();
    let before = layered(&model);
    assert_eq!(before.len(), 11, "nine walls and two slabs");
    let ifc = export_ifc4(&model, &ExportOptions::default());
    assert_eq!(ifc.matches("IFCMATERIALLAYERSETUSAGE(").count(), 11);
    assert_eq!(ifc.matches("IFCMATERIALLAYERSET(").count(), 4);
    assert!(ifc.contains(".AXIS2.,.POSITIVE.") && ifc.contains(".AXIS2.,.NEGATIVE."));
    assert!(ifc.contains(".AXIS3.,.NEGATIVE."));
    let back = import_ifc(&ifc).unwrap().model;
    assert_eq!(layered(&back), before);
    // every assembly comes back, the roof's too, and walls are as thick as their layers
    let names = |m: &Model| {
        let mut v: Vec<String> = assemblies(m)
            .into_iter()
            .map(|(_, n, a)| format!("{n} {:?} {}", a.usage, a.layers.len()))
            .collect();
        v.sort();
        v
    };
    assert_eq!(names(&back), names(&model));
    for e in back.iter() {
        if let (ElementKind::Wall(w), Some((_, a))) = (&e.kind, assembly_of(&back, e)) {
            assert!((w.thickness - a.total()).abs() < 1e-6, "{}", e.name);
        }
    }
}

#[test]
fn layered_elements_round_trip_through_ifcx() {
    let model = with_a_flipped_wall();
    let back = import_ifcx(&export_ifcx(&model)).unwrap().model;
    assert_eq!(layered(&back), layered(&model));
    let roof = back
        .iter()
        .find(|e| matches!(e.kind, ElementKind::Roof(_)))
        .expect("the roof");
    assert!(assembly_of(&back, roof).is_some());
}

/// The layered house with one wall's core run 20 mm on at its end and its
/// interior finish held back 50 mm at its start.
fn with_a_layer_edit() -> (Model, usize) {
    let m = layered_house();
    let wall = m
        .iter()
        .find(|e| e.name == "GF Wall 2")
        .expect("a ground floor wall")
        .clone();
    let (_, a) = assembly_of(&m, &wall).unwrap();
    let n = a
        .layers
        .iter()
        .filter(|l| l.function != LayerFunction::AirGap)
        .count();
    let core = a.core().0;
    let last = a.layers.len() - 1;
    let mut after: Element = wall.clone();
    if let ElementKind::Wall(w) = &mut after.kind {
        w.set_layer_ext(core, true, 0.02);
        w.set_layer_ext(last, false, -0.05);
    }
    let mut doc = creabim_core::Document::from_model(m);
    doc.commit(Transaction::new("Layer ends").replace(wall, after))
        .unwrap();
    (doc.model().clone(), n)
}

#[test]
fn a_wall_with_layers_moved_by_hand_goes_out_with_its_layers_as_parts() {
    let (model, n) = with_a_layer_edit();
    let ifc = export_ifc4(&model, &ExportOptions::default());
    // only the edited wall has parts, one per solid layer
    assert_eq!(
        ifc.matches("IFCBUILDINGELEMENTPART(").count(),
        n,
        "{n} layers"
    );
    let part = ifc
        .lines()
        .find(|l| l.contains("IFCBUILDINGELEMENTPART("))
        .unwrap();
    assert!(part.contains("'GF Wall 2 "), "{part}");
    // the wall is made of them, and keeps its own body and layer set
    let wall = ifc
        .lines()
        .find(|l| l.contains("IFCWALL(") && l.contains("'GF Wall 2'"))
        .unwrap();
    let wall_ref = wall.split('=').next().unwrap();
    assert!(ifc
        .lines()
        .any(|l| l.contains("IFCRELAGGREGATES(") && l.contains(&format!(",{wall_ref},("))));
    assert_eq!(ifc.matches("IFCMATERIALLAYERSETUSAGE(").count(), 11);
    // read back, the parts are not extra elements: the wall has its layers
    let plain = import_ifc(&export_ifc4(&layered_house(), &ExportOptions::default()))
        .unwrap()
        .model;
    let back = import_ifc(&ifc).unwrap().model;
    let meshes = |m: &Model| count_free_meshes(m);
    assert_eq!(meshes(&back), meshes(&plain));
    assert_eq!(layered(&back), layered(&layered_house()));
    if let Some(dir) = std::env::var_os("CREABIM_IFC_OUT") {
        std::fs::write(std::path::Path::new(&dir).join("layer-parts.ifc"), &ifc).unwrap();
    }
}

fn count_free_meshes(m: &Model) -> usize {
    m.iter()
        .filter(|e| matches!(e.kind, ElementKind::FreeMesh(_)))
        .count()
}

#[test]
fn an_untouched_layered_house_has_no_parts() {
    let ifc = export_ifc4(&layered_house(), &ExportOptions::default());
    assert_eq!(ifc.matches("IFCBUILDINGELEMENTPART(").count(), 0);
}
