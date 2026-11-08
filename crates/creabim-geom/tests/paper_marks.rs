//! North arrows and scale bars on a sheet (MU22): they draw on the sheet,
//! in its SVG and PDF. `CREABIM_SHOW` writes the SVG to look at.

use creabim_core::{Element, ElementKind, PaperMark, Transaction};
use glam::DVec2;

#[test]
fn a_sheet_draws_its_north_arrow_and_scale_bar() {
    let mut doc = creabim_core::Document::from_model(creabim_core::sample::sample_house());
    let (sheet, s) = creabim_geom::sheet::sheets(doc.model())
        .first()
        .map(|(id, s)| (*id, (*s).clone()))
        .expect("the sample has a sheet");
    let m = creabim_geom::sheet::MARGIN;
    let tb = creabim_geom::sheet::TITLE_BLOCK;
    let north = PaperMark::north_arrow(sheet, DVec2::new(s.width - m - 16.0, m + tb.y + 18.0));
    let bar = PaperMark::scale_bar(sheet, DVec2::new(s.width - m - tb.x + 6.0, m + tb.y + 10.0), 100.0);
    let info = creabim_geom::sheet::SheetInfo::default();
    let before = creabim_geom::sheet::sheet_svg(doc.model(), sheet, &info).unwrap();
    let (rev, drawn) = (doc.model().revision(), doc.model().drawing_revision());
    doc.commit(
        Transaction::new("marks")
            .insert(Element::new("North arrow", ElementKind::PaperMark(north)))
            .insert(Element::new("Scale bar", ElementKind::PaperMark(bar))),
    )
    .unwrap();
    // the model changed, but nothing a view draws (MU23): sheets keep
    // their views' drawings instead of redrawing them
    assert_ne!(doc.model().revision(), rev);
    assert_eq!(doc.model().drawing_revision(), drawn);
    let after = creabim_geom::sheet::sheet_svg(doc.model(), sheet, &info).unwrap();
    assert!(after.len() > before.len());
    assert!(after.contains(">N<"), "the north arrow's N");
    assert!(after.contains("5 m"), "the scale bar's last number");
    assert!(after.contains("1:100"));
    if let Some(dir) = std::env::var_os("CREABIM_SHOW") {
        std::fs::write(std::path::Path::new(&dir).join("paper_marks.svg"), after).unwrap();
    }
}
