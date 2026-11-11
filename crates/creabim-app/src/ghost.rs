//! The element a click would place, under the pointer before the click
//! (A7-4): a ghost in the accent colour with a dashed outline, in plans
//! and in 3D.
//!
//! The ghost is the click itself, run on a scratch copy of the model, so
//! what it shows is exactly what the click places: the type in the bar,
//! its size, the wall it goes into. Nothing reaches the real model; the
//! result is kept while the pointer, the tool and the model stay as they
//! were.

use crate::planview::{PlanState, Tool};
use creabim_core::document::Document;
use creabim_core::{ElementId, Model};
use eframe::egui::{self, Color32, Pos2, Shape, Stroke};
use glam::{DVec2, DVec3};
use std::cell::RefCell;
use std::collections::HashSet;

/// One part of the ghost: its corners (to fill) and its edges (to dash).
#[derive(Clone, Debug, Default)]
pub struct Part {
    pub points: Vec<DVec3>,
    pub edges: Vec<(DVec3, DVec3)>,
    /// Its triangles, as indices into `points`: filled as they are, so a
    /// curved wall tints as a band, not the disc its hull would make.
    pub tris: Vec<[u32; 3]>,
}

/// What a click would place.
#[derive(Clone, Debug, Default)]
pub struct Ghost {
    pub parts: Vec<Part>,
}

/// The tools a ghost is shown for: those a single click places.
pub fn shows(tool: Tool) -> bool {
    matches!(
        tool,
        Tool::Door
            | Tool::Window
            | Tool::Opening
            | Tool::Column
            | Tool::Footing
            | Tool::Stair
            | Tool::Mass
            | Tool::Object
            | Tool::Planting
    )
}

/// A plan state with only what placing reads: the types and sizes in the
/// bar and the host picked.
fn scratch_state(state: &PlanState) -> PlanState {
    let mut st = PlanState::default();
    st.tool = state.tool;
    st.types = state.types.clone();
    st.members = state.members.clone();
    st.parts = state.parts.clone();
    st.objects = state.objects.clone();
    st.host_pick = state.host_pick;
    st.host_cycle = state.host_cycle;
    st.boxed = state.boxed;
    st.wall_thickness = state.wall_thickness;
    st.wall_location = state.wall_location;
    st.wall_flip = state.wall_flip;
    st
}

/// What a click with `tool` at `p` on `level` would place: none when the
/// click would place nothing there (no wall for a window), or would only
/// start something (a strip footing's first point).
pub fn simulate(
    model: &Model,
    level: ElementId,
    tool: Tool,
    p: DVec2,
    state: &PlanState,
) -> Option<Ghost> {
    if !shows(tool) || state.pending.is_some() {
        return None;
    }
    let mut doc = Document::from_model(model.clone());
    let mut st = scratch_state(state);
    let mut sel = crate::selection::Selection::default();
    crate::planview::plan_tool(&mut doc, level, tool, p, &mut st, &mut sel).ok()?;
    if st.pending.is_some() {
        return None;
    }
    let before: HashSet<ElementId> = model.iter().map(|e| e.id).collect();
    let new: HashSet<ElementId> = doc
        .model()
        .iter()
        .map(|e| e.id)
        .filter(|id| !before.contains(id))
        .collect();
    if new.is_empty() {
        return None;
    }
    let part = |mesh: &creabim_geom::mesh::TriMesh| Part {
        points: mesh
            .positions
            .iter()
            .map(|v| glam::Vec3::from_array(*v).as_dvec3())
            .collect(),
        edges: crate::snaps::feature_edges(mesh),
        tris: mesh.indices.chunks(3).map(|t| [t[0], t[1], t[2]]).collect(),
    };
    let mut parts: Vec<Part> = creabim_geom::scene::build_scene_of(doc.model(), Some(&new))
        .into_iter()
        .filter(|i| !i.mesh.positions.is_empty() && !i.ghost)
        .map(|i| part(&i.mesh))
        .collect();
    // a door, window or hole shows the hole it cuts through the whole wall,
    // not only what sits in it (MU18)
    for id in &new {
        let m = doc.model();
        let Some(creabim_core::ElementKind::Opening(o)) = m.get(*id).map(|e| &e.kind) else {
            continue;
        };
        let Some(creabim_core::ElementKind::Wall(w)) = m.get(o.host).map(|e| &e.kind) else {
            continue;
        };
        let z = m.level(w.level).map_or(0.0, |l| l.elevation);
        if let Some(mesh) = creabim_geom::opening::cut_box_mesh(o, w, z) {
            parts.push(part(&mesh));
        }
    }
    (!parts.is_empty()).then_some(Ghost { parts })
}

type Key = (u64, ElementId, Tool, i64, i64, String);

thread_local! {
    static CACHE: RefCell<Option<(Key, Option<Ghost>)>> = const { RefCell::new(None) };
}

/// [`simulate`], kept while the model, the tool, the bar's settings and
/// the pointer (to the millimetre) stay the same.
pub fn cached(
    model: &Model,
    level: ElementId,
    tool: Tool,
    p: DVec2,
    state: &PlanState,
) -> Option<Ghost> {
    if !shows(tool) || state.pending.is_some() {
        return None;
    }
    let mut types: Vec<_> = state
        .types
        .iter()
        .map(|(c, t)| format!("{c:?}{t:?}"))
        .collect();
    types.sort();
    let settings = format!(
        "{types:?}{:?}{:?}{:?}{:?}",
        state.members, state.host_pick, state.objects, state.parts
    );
    let key: Key = (
        model.revision(),
        level,
        tool,
        (p.x * 1000.0).round() as i64,
        (p.y * 1000.0).round() as i64,
        settings,
    );
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((k, g)) = c.as_ref() {
            if *k == key {
                return g.clone();
            }
        }
        let _p = crate::app::Perf::new("ghost");
        let g = simulate(model, level, tool, p, state);
        *c = Some((key, g.clone()));
        g
    })
}

/// The accent at about 40 % for the fill, and the dashed outline.
pub fn draw(
    painter: &egui::Painter,
    project: &dyn Fn(DVec3) -> Option<Pos2>,
    ghost: &Ghost,
    accent: Color32,
) {
    let fill = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 90);
    // a closed body covers most of the screen it takes twice (its front
    // and its back), so each triangle at half: about the hull's tint
    let half = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 48);
    let line = Stroke::new(1.2, accent);
    for part in &ghost.parts {
        if part.tris.is_empty() {
            let pts: Vec<Pos2> = part.points.iter().filter_map(|p| project(*p)).collect();
            let hull = hull(&pts);
            if hull.len() >= 3 {
                painter.add(Shape::convex_polygon(hull, fill, Stroke::NONE));
            }
        } else {
            let scr: Vec<Option<Pos2>> = part.points.iter().map(|p| project(*p)).collect();
            let mut mesh = egui::Mesh::default();
            for t in &part.tris {
                let (Some(a), Some(b), Some(c)) =
                    (scr[t[0] as usize], scr[t[1] as usize], scr[t[2] as usize])
                else {
                    continue;
                };
                let i = mesh.vertices.len() as u32;
                for p in [a, b, c] {
                    mesh.colored_vertex(p, half);
                }
                mesh.add_triangle(i, i + 1, i + 2);
            }
            painter.add(Shape::mesh(mesh));
        }
        for (a, b) in &part.edges {
            if let (Some(a), Some(b)) = (project(*a), project(*b)) {
                if a.distance(b) < 0.5 {
                    continue;
                }
                painter.extend(Shape::dashed_line(&[a, b], line, 4.0, 3.0));
            }
        }
    }
}

/// The convex hull of screen points, anticlockwise on screen.
fn hull(pts: &[Pos2]) -> Vec<Pos2> {
    let mut p: Vec<Pos2> = pts.to_vec();
    p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    p.dedup_by(|a, b| a.distance(*b) < 0.01);
    if p.len() < 3 {
        return p;
    }
    let cross = |o: Pos2, a: Pos2, b: Pos2| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut lower: Vec<Pos2> = Vec::new();
    for &q in &p {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], q) <= 0.0 {
            lower.pop();
        }
        lower.push(q);
    }
    let mut upper: Vec<Pos2> = Vec::new();
    for &q in p.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], q) <= 0.0 {
            upper.pop();
        }
        upper.push(q);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

#[cfg(test)]
mod tests {
    use super::*;
    use creabim_core::{ElementKind, Opening};

    fn first_wall(m: &Model) -> (ElementId, ElementId, DVec2) {
        m.iter()
            .find_map(|e| match &e.kind {
                ElementKind::Wall(w) if w.curtain.is_none() => {
                    Some((e.id, w.level, (w.start + w.end) / 2.0))
                }
                _ => None,
            })
            .expect("a wall")
    }

    #[test]
    fn a_window_ghost_is_what_the_click_places_and_changes_nothing() {
        let m = creabim_core::sample::sample_house();
        let (_, level, mid) = first_wall(&m);
        let mut state = PlanState::default();
        state.tool = Tool::Window;
        let rev = m.revision();
        let ghost = simulate(&m, level, Tool::Window, mid, &state).expect("a ghost over a wall");
        assert!(!ghost.parts.is_empty());
        assert_eq!(m.revision(), rev, "the real model is untouched");
        // the click itself makes a window whose box holds the ghost
        let mut doc = Document::from_model(m.clone());
        let mut sel = crate::selection::Selection::default();
        crate::planview::plan_tool(&mut doc, level, Tool::Window, mid, &mut state, &mut sel)
            .unwrap();
        let placed: Vec<_> = doc
            .model()
            .iter()
            .filter(|e| {
                matches!(e.kind, ElementKind::Opening(Opening { .. })) && m.get(e.id).is_none()
            })
            .map(|e| e.id)
            .collect();
        assert_eq!(placed.len(), 1);
        let only: HashSet<_> = placed.into_iter().collect();
        let items = creabim_geom::scene::build_scene_of(doc.model(), Some(&only));
        let real: Vec<DVec3> = items
            .iter()
            .flat_map(|i| {
                i.mesh
                    .positions
                    .iter()
                    .map(|v| glam::Vec3::from_array(*v).as_dvec3())
            })
            .collect();
        let ghost_pts: Vec<DVec3> = ghost.parts.iter().flat_map(|p| p.points.clone()).collect();
        let bounds = |v: &[DVec3]| {
            v.iter().fold(
                (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN)),
                |(lo, hi), p| (lo.min(*p), hi.max(*p)),
            )
        };
        let (a, b) = (bounds(&real), bounds(&ghost_pts));
        // the ghost holds the window, and the cut through the whole wall
        assert!(
            b.0.cmple(a.0 + 1e-6).all() && b.1.cmpge(a.1 - 1e-6).all(),
            "{a:?} in {b:?}"
        );
    }

    #[test]
    fn a_hole_ghost_shows_its_cut_through_the_wall() {
        let m = creabim_core::sample::sample_house();
        let (_, level, mid) = first_wall(&m);
        let mut state = PlanState::default();
        state.tool = Tool::Opening;
        let g = simulate(&m, level, Tool::Opening, mid, &state).expect("a hole ghost");
        assert_eq!(g.parts.len(), 1, "the cut box");
        let mut state = PlanState::default();
        state.tool = Tool::Window;
        let g = simulate(&m, level, Tool::Window, mid, &state).unwrap();
        assert!(g.parts.len() >= 2, "the window and its cut");
    }

    #[test]
    fn furniture_shows_a_ghost_where_it_will_land() {
        let m = creabim_core::sample::sample_house();
        let (_, level, _) = first_wall(&m);
        let mut state = PlanState::default();
        state.tool = Tool::Object;
        state.objects.current = creabim_core::catalogue::starters().into_iter().next();
        assert!(
            state.objects.current.is_some(),
            "a catalogue object to place"
        );
        let g = simulate(&m, level, Tool::Object, DVec2::new(2.0, 2.0), &state)
            .expect("an object ghost");
        let lo = g
            .parts
            .iter()
            .flat_map(|p| p.points.iter())
            .fold(f64::MAX, |a, p| {
                a.min(p.truncate().distance(DVec2::new(2.0, 2.0)))
            });
        assert!(lo < 2.0, "it sits by the pointer");
    }

    #[test]
    fn no_ghost_away_from_a_wall() {
        let m = creabim_core::sample::sample_house();
        let (_, level, _) = first_wall(&m);
        let state = PlanState::default();
        assert!(simulate(&m, level, Tool::Window, DVec2::new(500.0, 500.0), &state).is_none());
    }
}

#[cfg(test)]
mod timing {
    #[test]
    #[ignore = "timing probe"]
    fn ghost_cost() {
        use super::*;
        let m = creabim_core::sample::sample_house();
        let (level, mid) = m
            .iter()
            .find_map(|e| match &e.kind {
                creabim_core::ElementKind::Wall(w) => Some((w.level, (w.start + w.end) / 2.0)),
                _ => None,
            })
            .unwrap();
        let mut st = PlanState::default();
        st.tool = Tool::Window;
        let t = std::time::Instant::now();
        for i in 0..50 {
            let _ = simulate(
                &m,
                level,
                Tool::Window,
                mid + DVec2::X * (i as f64 * 0.001),
                &st,
            );
        }
        println!(
            "PROBE ghost {:.2} ms each",
            t.elapsed().as_secs_f64() * 1000.0 / 50.0
        );
    }
}
