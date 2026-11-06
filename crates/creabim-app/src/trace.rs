//! Trace and reference (MU18, after ArchiCAD's): something shown faintly
//! under a drawing to draw over or compare against. Another view, a
//! placed image (a scanned plan, a photo of a sketch) or a DXF drawing,
//! in a colour and strength of its own, moved aside or turned to compare,
//! shown and hidden with one key (Alt+F2).
//!
//! It is the viewer's, not the model's: it is kept with the view's pane
//! state, never in the project file, the undo history or an export.

use creabim_core::{ElementId, Model};
use creabim_geom::plan::PlanItem;
use creabim_geom::raster::Raster;
use eframe::egui::{self, Color32, Pos2, Stroke};
use glam::DVec2;
use std::sync::Arc;

/// What is traced over.
#[derive(Clone, Debug)]
pub enum Source {
    /// Another plan, section or elevation, drawn at the same place.
    View(ElementId),
    /// A picture: `width` metres wide, its lower left corner at the origin.
    Image {
        path: String,
        raster: Arc<Raster>,
        width: f64,
    },
    /// A DXF drawing's lines, in metres (its units read from the file).
    Dxf {
        path: String,
        lines: Vec<Vec<DVec2>>,
    },
}

impl Source {
    pub fn label(&self, model: &Model) -> String {
        let file = |p: &str| {
            std::path::Path::new(p)
                .file_name()
                .map_or(p.to_string(), |n| n.to_string_lossy().into_owned())
        };
        match self {
            Source::View(v) => model.get(*v).map_or("a view".into(), |e| e.name.clone()),
            Source::Image { path, .. } | Source::Dxf { path, .. } => file(path),
        }
    }
}

/// The colours a reference can be drawn in; the first is the default.
pub const COLOURS: [(&str, Color32); 4] = [
    ("Red", Color32::from_rgb(214, 64, 52)),
    ("Blue", Color32::from_rgb(52, 104, 196)),
    ("Grey", Color32::from_rgb(120, 124, 130)),
    ("Green", Color32::from_rgb(70, 130, 70)),
];

/// A reference shown under one view.
#[derive(Clone, Debug)]
pub struct Trace {
    pub source: Source,
    pub colour: usize,
    /// 0 (gone) to 1 (as strong as the drawing).
    pub opacity: f32,
    /// Moved aside by, metres.
    pub offset: DVec2,
    /// Turned by, degrees anticlockwise, about the origin.
    pub rotation: f64,
    pub shown: bool,
    /// A view's linework, and the model revision it was taken at.
    cache: Option<(u64, Arc<Vec<Vec<DVec2>>>)>,
}

impl Trace {
    pub fn new(source: Source) -> Self {
        Self {
            source,
            colour: 0,
            opacity: 0.45,
            offset: DVec2::ZERO,
            rotation: 0.0,
            shown: true,
            cache: None,
        }
    }

    /// From the reference's own place to where it is shown.
    pub fn place(&self, p: DVec2) -> DVec2 {
        DVec2::from_angle(self.rotation.to_radians()).rotate(p) + self.offset
    }

    fn colour(&self) -> Color32 {
        COLOURS[self.colour.min(COLOURS.len() - 1)]
            .1
            .gamma_multiply(self.opacity.clamp(0.0, 1.0))
    }

    /// Its linework, for a view or a DXF: polylines in model metres.
    fn lines(&mut self, model: &Model) -> Arc<Vec<Vec<DVec2>>> {
        match &self.source {
            Source::Dxf { lines, .. } => Arc::new(lines.clone()),
            Source::View(v) => {
                let rev = model.revision();
                if let Some((r, l)) = &self.cache {
                    if *r == rev {
                        return l.clone();
                    }
                }
                let l = Arc::new(
                    creabim_geom::section::view_drawing(model, *v)
                        .map(|d| linework(&d.items))
                        .unwrap_or_default(),
                );
                self.cache = Some((rev, l.clone()));
                l
            }
            Source::Image { .. } => Arc::new(vec![]),
        }
    }

    /// Draw it under a drawing; `to_screen` places a model point.
    pub fn draw(
        &mut self,
        painter: &egui::Painter,
        model: &Model,
        to_screen: &dyn Fn(DVec2) -> Pos2,
    ) {
        if !self.shown || self.opacity <= 0.0 {
            return;
        }
        if let Source::Image { raster, width, .. } = &self.source {
            let h = width * raster.height as f64 / raster.width.max(1) as f64;
            // a turned picture is drawn by its corners
            let corners = [
                DVec2::ZERO,
                DVec2::new(*width, 0.0),
                DVec2::new(*width, h),
                DVec2::new(0.0, h),
            ]
            .map(|c| to_screen(self.place(c)));
            let id = crate::textures::texture(painter.ctx(), raster);
            let tint = Color32::WHITE.gamma_multiply(self.opacity.clamp(0.0, 1.0));
            let mut mesh = egui::Mesh::with_texture(id);
            let uv = [
                egui::pos2(0.0, 1.0),
                egui::pos2(1.0, 1.0),
                egui::pos2(1.0, 0.0),
                egui::pos2(0.0, 0.0),
            ];
            for (c, t) in corners.iter().zip(uv) {
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: *c,
                    uv: t,
                    color: tint,
                });
            }
            mesh.indices.extend([0, 1, 2, 0, 2, 3]);
            painter.add(egui::Shape::mesh(mesh));
            return;
        }
        let stroke = Stroke::new(1.0, self.colour());
        for line in self.lines(model).iter() {
            let pts: Vec<Pos2> = line.iter().map(|p| to_screen(self.place(*p))).collect();
            if pts.len() >= 2 {
                painter.add(egui::Shape::line(pts, stroke));
            }
        }
    }
}

/// A drawing's linework as polylines: lines, outlines and arcs, no text
/// or fills.
pub fn linework(items: &[PlanItem]) -> Vec<Vec<DVec2>> {
    let mut out = Vec::new();
    for it in items {
        match it {
            PlanItem::Line { a, b, .. } => out.push(vec![*a, *b]),
            PlanItem::Polygon { points, .. } | PlanItem::Fill { points, .. } => {
                if let Some(first) = points.first() {
                    let mut p = points.clone();
                    p.push(*first);
                    out.push(p);
                }
            }
            PlanItem::Arc {
                center,
                radius,
                start,
                end,
                ..
            } => {
                let sweep = end - start;
                let n = ((sweep.abs() / 0.15).ceil() as usize).clamp(4, 64);
                out.push(
                    (0..=n)
                        .map(|i| {
                            let a = start + sweep * i as f64 / n as f64;
                            *center + DVec2::from_angle(a) * *radius
                        })
                        .collect(),
                );
            }
            _ => {}
        }
    }
    out
}

/// A picture file as a reference `width` metres wide.
pub fn image(path: &str, width: f64) -> Result<Source, String> {
    let img = image::open(path).map_err(|e| format!("couldn't read {path}: {e}"))?;
    let rgba = img.to_rgba8();
    let raster = Raster {
        width: rgba.width() as usize,
        height: rgba.height() as usize,
        rgba: rgba.into_raw(),
        key: path_key(path),
    };
    Ok(Source::Image {
        path: path.to_string(),
        raster: Arc::new(raster),
        width,
    })
}

/// A DXF file's lines as a reference.
pub fn dxf(path: &str) -> Result<Source, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("couldn't read {path}: {e}"))?;
    let items = creabim_exchange::titleblock::dxf_items(&text)?;
    let lines = items
        .into_iter()
        .filter_map(|it| match it {
            creabim_core::titleblock::TitleItem::Line { points, .. } => {
                Some(points.into_iter().map(|p| p / 1000.0).collect())
            }
            _ => None,
        })
        .collect();
    Ok(Source::Dxf {
        path: path.to_string(),
        lines,
    })
}

fn path_key(path: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    h.finish() | 1 << 63
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_view_traced_under_another_is_its_linework_moved_and_turned() {
        let m = creabim_core::sample::sample_house();
        let v = m
            .iter()
            .find(|e| matches!(&e.kind, creabim_core::ElementKind::View(v) if matches!(v.kind, creabim_core::ViewKind::Plan { .. })))
            .unwrap()
            .id;
        let mut t = Trace::new(Source::View(v));
        let lines = t.lines(&m);
        assert!(!lines.is_empty(), "a plan has linework");
        t.offset = DVec2::new(10.0, 0.0);
        t.rotation = 90.0;
        let p = t.place(DVec2::new(1.0, 0.0));
        assert!(p.distance(DVec2::new(10.0, 1.0)) < 1e-9, "{p}");
    }

    #[test]
    fn a_dxf_reference_reads_its_lines_in_metres() {
        let dir = std::env::temp_dir().join("creabim-trace-test.dxf");
        let text = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n5000\n21\n0\n0\nENDSEC\n0\nEOF\n";
        std::fs::write(&dir, text).unwrap();
        let Source::Dxf { lines, .. } = dxf(&dir.to_string_lossy()).unwrap() else {
            panic!()
        };
        assert_eq!(lines.len(), 1);
        assert!((lines[0][1].x - 5.0).abs() < 1e-9, "{:?}", lines[0]);
    }
}
