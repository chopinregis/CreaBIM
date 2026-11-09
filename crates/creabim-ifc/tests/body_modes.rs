//! MU20: the three ways walls, slabs and roofs go out.
use creabim_ifc::{export_ifc4, ExportOptions, IfcBody};

fn count(text: &str, what: &str) -> usize {
    text.matches(&format!("={what}(")).count()
}

#[test]
fn layers_go_out_as_coverings_and_joined_bodies_as_shapes() {
    let m = creabim_core::starter::starter_house();
    let plain = export_ifc4(&m, &ExportOptions::default());
    let layers = export_ifc4(
        &m,
        &ExportOptions {
            body: IfcBody::Layers,
            ..Default::default()
        },
    );
    assert!(
        count(&layers, "IFCCOVERING") > count(&plain, "IFCCOVERING") + 10,
        "finishes and insulation as coverings"
    );
    assert!(count(&layers, "IFCRELCOVERSBLDGELEMENTS") > 0);
    // the walls and slabs are still there, as many as before
    for k in ["IFCWALL", "IFCSLAB"] {
        assert_eq!(count(&layers, k), count(&plain, k), "{k}");
    }
    let joined = export_ifc4(
        &m,
        &ExportOptions {
            body: IfcBody::Joined,
            ..Default::default()
        },
    );
    for k in ["IFCWALL", "IFCSLAB"] {
        assert_eq!(count(&joined, k), count(&plain, k), "{k}");
    }
    assert!(count(&joined, "IFCTRIANGULATEDFACESET") > count(&plain, "IFCTRIANGULATEDFACESET"));
}
