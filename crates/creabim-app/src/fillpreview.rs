//! Fill room, before the click (brief 02, 3.4): with the Room, Ceiling or
//! Slab tool in its Fill room mode, the space a click would fill is tinted
//! under the pointer with its area by it. Where the walls don't close the
//! space, the gaps are drawn in red and the pill says to draw the outline.

use crate::planview::{PlanState, Tool, Xf};
use creabim_core::{room, ElementId, ElementKind, Model};
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};
use glam::DVec2;

/// What a click would fill.
#[derive(Clone, Debug, PartialEq)]
pub enum Look {
    /// This outline, this many square metres.
    Fills { outline: Vec<DVec2>, area: f64 },
    /// A room is there already (for a room), or has a ceiling (for a ceiling).
    Taken { outline: Vec<DVec2>, note: String },
    /// Nothing closes the space: where the walls stop short.
    Open { gaps: Vec<(DVec2, DVec2)> },
}

/// The look, and what it was worked out for: the level, the pointer to the
/// centimetre, the tool and how many edits the model had then.
pub type Cached = ((ElementId, i64, i64, Tool, usize, usize), Look);

/// What a Fill room click at `p` on `level` would do with `tool`.
pub fn look(model: &Model, level: ElementId, tool: Tool, p: DVec2) -> Look {
    let room_there = crate::roomtool::room_at(model, level, p);
    match tool {
        Tool::Room => {
            if let Some(r) = room_there {
                let name = model.get(r).map_or(String::new(), |e| e.name.clone());
                return Look::Taken {
                    outline: room::outline(model, r).unwrap_or_default(),
                    note: format!("A room is here already ({name})"),
                };
            }
        }
        Tool::Ceiling => {
            if let Some(r) = room_there {
                let outline = room::outline(model, r).unwrap_or_default();
                let covered = model
                    .iter()
                    .any(|e| matches!(&e.kind, ElementKind::Ceiling(c) if c.room == Some(r)));
                if covered {
                    return Look::Taken {
                        outline,
                        note: "That room has a ceiling already".into(),
                    };
                }
                let area = creabim_core::polygon::signed_area2(&outline).abs() / 2.0;
                return Look::Fills { outline, area };
            }
        }
        _ => {}
    }
    let outline = if tool == Tool::Slab {
        creabim_core::room_outline(model, level, p)
    } else {
        room::enclosed(model, level, p).map(|s| s.outline)
    };
    match outline {
        Some(outline) => {
            let area = creabim_core::polygon::signed_area2(&outline).abs() / 2.0;
            Look::Fills { outline, area }
        }
        None => Look::Open {
            gaps: room::gaps(model, level, p),
        },
    }
}

/// Is the pane's tool in Fill room?
pub fn on(state: &PlanState) -> bool {
    matches!(state.tool, Tool::Room | Tool::Ceiling | Tool::Slab)
        && state.slab_mode == crate::slabtool::SlabMode::FillRoom
        && state.outline.is_none()
        && crate::partstool::make_for(state).is_none()
}

/// The look under the pointer, worked out again only when something changed.
pub fn cached<'a>(
    cache: &'a mut Option<Cached>,
    doc: &creabim_core::Document,
    level: ElementId,
    tool: Tool,
    p: DVec2,
) -> &'a Look {
    let key = (
        level,
        (p.x * 100.0).round() as i64,
        (p.y * 100.0).round() as i64,
        tool,
        doc.undo_steps().len(),
        doc.redo_steps().len(),
    );
    if cache.as_ref().map(|c| c.0) != Some(key) {
        *cache = Some((key, look(doc.model(), level, tool, p)));
    }
    &cache.as_ref().expect("just set").1
}

/// Draw it: the space tinted and outlined in the accent with its area by
/// the pointer, or the gaps in red.
pub fn draw(
    painter: &egui::Painter,
    xf: &Xf,
    look: &Look,
    at: Pos2,
    units: &creabim_core::Units,
    tool: Tool,
) {
    let palette = crate::theme::Palette::of(painter.ctx());
    let pill = crate::handles::PillLook::of(&palette);
    let accent = crate::planview::accent_selected();
    let outline = |pts: &[DVec2], c: Color32| {
        let mut s: Vec<Pos2> = pts.iter().map(|p| xf.to_screen(*p)).collect();
        if let Some(f) = s.first().copied() {
            s.push(f);
        }
        painter.add(egui::Shape::line(s, Stroke::new(2.0, c)));
    };
    let by = at + Vec2::new(16.0, 18.0);
    match look {
        Look::Fills { outline: pts, area } => {
            crate::planview::fill_polygon(painter, xf, pts, accent.gamma_multiply(0.18));
            outline(pts, accent);
            let what = match tool {
                Tool::Ceiling => "Ceiling",
                Tool::Slab => "Slab",
                _ => "Room",
            };
            crate::handles::plain_pill(
                painter,
                by,
                &format!("{what} {} · click to add", units.area(*area)),
                &pill,
            );
        }
        Look::Taken { outline: pts, note } => {
            outline(pts, Color32::GRAY);
            crate::handles::plain_pill(painter, by, note, &pill);
        }
        Look::Open { gaps } => {
            let red = Color32::from_rgb(214, 48, 49);
            for (a, b) in gaps {
                let (a, b) = (xf.to_screen(*a), xf.to_screen(*b));
                painter.line_segment([a, b], Stroke::new(3.0, red));
                painter.circle_filled(a, 3.5, red);
                painter.circle_filled(b, 3.5, red);
            }
            crate::handles::plain_pill(
                painter,
                by,
                "Not closed: draw its outline (1, 2, 4)",
                &pill,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use creabim_core::starter::starter_house;

    #[test]
    fn fill_room_says_what_a_click_would_fill() {
        let m = starter_house();
        let (lid, _) = m
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::Level(l) if l.elevation == 0.0 => Some((e.id, l.clone())),
                _ => None,
            })
            .unwrap();
        // inside a room the starter house already has
        let r = m
            .iter()
            .find(|e| matches!(&e.kind, ElementKind::Room(r) if r.level == lid))
            .unwrap();
        let o = room::outline(&m, r.id).unwrap();
        let inside = o.iter().copied().sum::<DVec2>() / o.len() as f64;
        assert!(matches!(
            look(&m, lid, Tool::Room, inside),
            Look::Taken { .. }
        ));
        // a slab fills it to the walls' centre lines
        assert!(
            matches!(look(&m, lid, Tool::Slab, inside), Look::Fills { area, .. } if area > 1.0)
        );
        // out in the garden nothing closes it
        assert!(matches!(
            look(&m, lid, Tool::Room, DVec2::new(-40.0, -40.0)),
            Look::Open { .. }
        ));
    }
}
