//! Dimensions and tags: where they sit in a view's drawing, what they
//! read, and the linework they are drawn with. Both follow their hosts,
//! because their positions are worked out from the model every time.

use crate::annotation::{end_marks, measure, runs_past, Annotation, Ends, Shape, TextLook};
use crate::plan::PlanItem;
use crate::section::{AxoFrame, SectionFrame};
use creabim_core::{
    DimLeader, DimPlace, DimensionKind, ElementId, ElementKind, HostPoint, Model, Note,
    OpeningKind, RoomField, TagShape, Terminator, View, ViewKind, Wall,
};
use glam::{DAffine3, DMat3, DVec2, DVec3};

/// Maps world points into a view's drawing coordinates.
pub enum Projector {
    Plan,
    Section(SectionFrame),
    Axo(AxoFrame),
}

impl Projector {
    pub fn of(model: &Model, view: &View) -> Option<Self> {
        Some(match view.kind {
            ViewKind::Plan { .. } => Projector::Plan,
            ViewKind::Section {
                start, end, depth, ..
            } => Projector::Section(SectionFrame::new(start, end, depth)),
            ViewKind::Axonometric { azimuth, altitude } => {
                Projector::Axo(AxoFrame::new(model, azimuth, altitude))
            }
            ViewKind::Schedule { .. }
            | ViewKind::LongSection { .. }
            | ViewKind::BoreholeLog { .. }
            | ViewKind::WalkSection { .. } => return None,
        })
    }

    pub fn project(&self, p: DVec3) -> DVec2 {
        match self {
            Projector::Plan => p.truncate(),
            Projector::Section(f) => f.project(p).truncate(),
            Projector::Axo(f) => f.project(p).truncate(),
        }
    }
}

/// A wall's frame: a point `u` along it and `s` to its left.
fn on_wall(w: &Wall, u: f64, s: f64) -> DVec2 {
    w.start + w.direction() * u + w.direction().perp() * s
}

fn host_wall(model: &Model, id: ElementId) -> Option<(&Wall, f64)> {
    match &model.get(id)?.kind {
        ElementKind::Wall(w) => Some((w, 0.0)),
        ElementKind::Opening(o) => match &model.get(o.host)?.kind {
            ElementKind::Wall(w) => Some((w, o.offset)),
            _ => None,
        },
        _ => None,
    }
}

/// Where a dimension point sits in a view's drawing, if it can be drawn there.
pub fn resolve(model: &Model, view: &View, p: &HostPoint) -> Option<DVec2> {
    let Some(host) = p.host else {
        return Some(p.local);
    };
    match (&model.get(host)?.kind, &view.kind) {
        (ElementKind::Level(l), ViewKind::Section { .. }) => {
            Some(DVec2::new(p.local.x, l.elevation + p.local.y))
        }
        (ElementKind::Wall(_) | ElementKind::Opening(_), ViewKind::Plan { .. }) => {
            let (w, base) = host_wall(model, host)?;
            let u = if p.from_end {
                w.length() + p.local.x
            } else {
                base + p.local.x
            };
            Some(on_wall(w, u, p.local.y))
        }
        _ => {
            let body = p.body?;
            Some(
                Projector::of(model, view)?
                    .project(host_frame(model, host)?.transform_point3(body)),
            )
        }
    }
}

/// An element's own 3D axes in the model, for dimension points kept on it:
/// a mass's origin, turn and size; a slab's first corner and edge; a grid,
/// wall or guide's start and direction; a sketch's plane.
pub fn host_frame(model: &Model, id: ElementId) -> Option<DAffine3> {
    let elevation = |level: Option<ElementId>| {
        level
            .and_then(|l| model.level(l))
            .map_or(0.0, |l| l.elevation)
    };
    // axes along `x` (made level), across it and up, at `o`
    let along = |o: DVec3, x: DVec3| {
        let x = DVec3::new(x.x, x.y, 0.0)
            .try_normalize()
            .unwrap_or(DVec3::X);
        DAffine3::from_mat3_translation(DMat3::from_cols(x, DVec3::Z.cross(x), DVec3::Z), o)
    };
    Some(match &model.get(id)?.kind {
        ElementKind::Wall(w) => along(
            w.start.extend(elevation(Some(w.level)) + w.base_offset),
            (w.end - w.start).extend(0.0),
        ),
        ElementKind::Opening(o) => {
            let ElementKind::Wall(w) = &model.get(o.host)?.kind else {
                return None;
            };
            let z = elevation(Some(w.level)) + w.base_offset + o.sill;
            along(
                on_wall(w, o.offset, 0.0).extend(z),
                (w.end - w.start).extend(0.0),
            )
        }
        ElementKind::Slab(s) => {
            let a = *s.boundary.first()?;
            let b = s.boundary.get(1).copied().unwrap_or(a + DVec2::X);
            along(a.extend(elevation(Some(s.level))), (b - a).extend(0.0))
        }
        ElementKind::Grid(g) => along(g.start.extend(0.0), (g.end - g.start).extend(0.0)),
        ElementKind::Guide(g) => along(g.a + DVec3::Z * elevation(g.level), g.b - g.a),
        ElementKind::FreeMesh(m) => DAffine3::from_mat3_translation(
            DMat3::from_rotation_z(m.rotation.to_radians()) * m.scale,
            m.origin + DVec3::Z * elevation(m.level),
        ),
        ElementKind::Sketch(sk) => {
            let p = &sk.plane;
            DAffine3::from_mat3_translation(
                DMat3::from_cols(p.x, p.y, p.x.cross(p.y)),
                p.origin + DVec3::Z * elevation(sk.level),
            )
        }
        _ => return None,
    })
}

/// A drawing point of `view` back in the model: on the level in a plan, on
/// the cut plane in a section.
fn unproject(model: &Model, view: &View, at: DVec2) -> Option<DVec3> {
    Some(match view.kind {
        ViewKind::Plan { level, .. } => at.extend(model.level(level).map_or(0.0, |l| l.elevation)),
        ViewKind::Section {
            start, end, depth, ..
        } => {
            let f = SectionFrame::new(start, end, depth);
            (f.origin + f.along * at.x).extend(at.y)
        }
        _ => return None,
    })
}

/// A dimension point attached to `host`, expressed in the host's frame so
/// it follows the host. `at` is a drawing point in `view`.
pub fn attach(model: &Model, view: &View, host: Option<ElementId>, at: DVec2) -> HostPoint {
    let attached = host.and_then(|h| match (&model.get(h)?.kind, &view.kind) {
        (ElementKind::Level(l), ViewKind::Section { .. }) => Some(HostPoint {
            host: Some(h),
            local: DVec2::new(at.x, at.y - l.elevation),
            from_end: false,
            body: None,
        }),
        (ElementKind::Wall(_) | ElementKind::Opening(_), ViewKind::Plan { .. }) => {
            let (w, base) = host_wall(model, h)?;
            let d = w.direction();
            let u = (at - w.start).dot(d);
            let s = (at - w.start).dot(d.perp());
            let is_wall = matches!(model.get(h)?.kind, ElementKind::Wall(_));
            // points nearer a wall's end are kept relative to the end, so
            // they follow it when the wall is lengthened
            let from_end = is_wall && u > w.length() / 2.0;
            Some(HostPoint {
                host: Some(h),
                local: DVec2::new(if from_end { u - w.length() } else { u - base }, s),
                from_end,
                body: None,
            })
        }
        (ElementKind::Level(_), _) => None,
        _ => {
            let frame = host_frame(model, h)?;
            let world = unproject(model, view, at)?;
            Some(HostPoint {
                host: Some(h),
                local: at,
                from_end: false,
                body: Some(frame.inverse().transform_point3(world)),
            })
        }
    });
    attached.unwrap_or(HostPoint::free(at))
}

/// The centre of an element's body, in world coordinates.
pub fn centre(model: &Model, id: ElementId) -> Option<DVec3> {
    let e = model.get(id)?;
    let elevation = |level: ElementId| model.level(level).map_or(0.0, |l| l.elevation);
    Some(match &e.kind {
        ElementKind::Wall(w) => on_wall(w, w.length() / 2.0, 0.0)
            .extend(elevation(w.level) + w.base_offset + w.height / 2.0),
        ElementKind::Opening(o) => {
            let ElementKind::Wall(w) = &model.get(o.host)?.kind else {
                return None;
            };
            on_wall(w, o.offset + o.width / 2.0, 0.0)
                .extend(elevation(w.level) + w.base_offset + o.sill + o.height / 2.0)
        }
        ElementKind::Slab(s) => {
            let c = s.boundary.iter().copied().sum::<DVec2>() / s.boundary.len().max(1) as f64;
            c.extend(s.top(elevation(s.level)) - s.thickness / 2.0)
        }
        ElementKind::FreeMesh(m) => {
            let mesh = m.evaluated().mesh;
            let n = mesh.vertices.len().max(1) as f64;
            let c = mesh.vertices.iter().copied().sum::<DVec3>() / n;
            c + m.origin + DVec3::Z * m.level.map_or(0.0, elevation)
        }
        // furniture and fixtures: the middle of their parts
        ElementKind::Object(_) => {
            let mut pts = creabim_core::objects::model_parts(model, e)
                .into_iter()
                .flat_map(|(m, _)| m.vertices);
            let first = pts.next()?;
            let (lo, hi) = pts.fold((first, first), |(lo, hi), v| (lo.min(v), hi.max(v)));
            (lo + hi) / 2.0
        }
        // a room: its point on its floor
        ElementKind::Room(r) => r.at.extend(elevation(r.level)),
        // a ceiling: the middle of its outline, on its underside
        ElementKind::Ceiling(c) => {
            let o = c.outline(model)?;
            let middle = o.iter().copied().sum::<DVec2>() / o.len().max(1) as f64;
            middle.extend(c.span(model).0)
        }
        // columns, beams, roofs, stairs and railings: the middle of their shape
        k if creabim_core::members::is_member(k) => {
            let (mesh, _) = crate::members::member_mesh(model, e)?;
            let (lo, hi) = mesh.bounds()?;
            ((lo + hi) / 2.0).as_dvec3()
        }
        _ => return None,
    })
}

/// The size an element's tag would give it: a wall's or slab's thickness,
/// a column's profile, a beam's width × depth, a pad's three sizes. None
/// for what has no one size.
pub fn size_text(model: &Model, id: ElementId) -> Option<String> {
    let e = model.get(id)?;
    let units = creabim_core::units::number_format(model, creabim_core::NumberKind::Tags, None)
        .units(&model.units());
    let tag = |m: f64| creabim_core::write_number(model, creabim_core::NumberKind::Tags, None, m);
    Some(match &e.kind {
        ElementKind::Wall(w) => tag(w.thickness),
        ElementKind::Slab(s) => tag(s.thickness),
        ElementKind::Column(c) => c.profile.label_in(&units),
        ElementKind::Beam(b) => format!("{} × {}", tag(b.width), tag(b.depth)),
        // a door or window: width × height, as its tags write it
        ElementKind::Opening(o) => format!("{} × {}", tag(o.width), tag(o.height)),
        ElementKind::Roof(r) => format!("{} {:.0}°", tag(r.thickness), r.pitch),
        ElementKind::Footing(f) => match f.kind {
            creabim_core::FootingKind::Strip => format!("{} × {}", tag(f.width), tag(f.thickness)),
            creabim_core::FootingKind::Pad => {
                format!(
                    "{} × {} × {}",
                    tag(f.width),
                    tag(f.length),
                    tag(f.thickness)
                )
            }
        },
        _ => return None,
    })
}

/// One field a reading note shows of the element `id` (see
/// [`creabim_core::NOTE_FIELDS`]); none when it has no such value.
pub fn note_field(model: &Model, id: ElementId, key: &str) -> Option<String> {
    let e = model.get(id)?;
    let some = |s: String| (!s.trim().is_empty()).then_some(s);
    match key {
        "name" => some(e.name.clone()),
        "mark" => match &e.kind {
            ElementKind::Object(_) => creabim_core::marks::object_mark(model, e),
            _ => creabim_core::marks::mark_of(model, id),
        },
        "ifc" => some(e.ifc_class().to_string()),
        "type" => creabim_core::types::type_of(model, e).and_then(|(t, _)| some(t.name.clone())),
        // one material, or a layered build-up's, outside to inside
        "material" => creabim_core::material_of(model, e)
            .and_then(|(m, _)| some(m.name.clone()))
            .or_else(|| {
                let (_, a) = creabim_core::assembly_of(model, e)?;
                let mut names: Vec<String> = vec![];
                for l in &a.layers {
                    if let Some(m) = l.material.and_then(|m| model.get(m)) {
                        if !names.contains(&m.name) {
                            names.push(m.name.clone());
                        }
                    }
                }
                some(names.join(", "))
            }),
        "level" => creabim_core::levels::level_of(&e.kind)
            .and_then(|l| model.get(l))
            .and_then(|l| some(l.name.clone())),
        "size" => size_text(model, id),
        // a layered element's build-up, one line a layer with its
        // thickness, outside to inside, as a material tag writes it
        "layers" => creabim_core::assembly_of(model, e)
            .filter(|(_, a)| !a.layers.is_empty())
            .map(|_| material_text(model, id)),
        _ => None,
    }
}

/// The words a note shows and whether they read as markdown: a notes
/// section's text, its own text with `{Field}`s filled from what it
/// reads, or its own text.
pub fn note_shown_text(model: &Model, n: &Note) -> (String, bool) {
    if let Some(t) = n
        .section
        .as_deref()
        .and_then(|k| creabim_core::project::linked_text(model, k))
    {
        return (t, true);
    }
    if let Some(r) = &n.reads {
        let from = n.reads_from();
        let filled = creabim_core::fill_note_fields(&n.text, |k| {
            from.and_then(|id| note_field(model, id, k))
        });
        let filled = if r.caps {
            filled.to_uppercase()
        } else {
            filled
        };
        return (filled, n.markdown);
    }
    (n.text.clone(), n.markdown)
}

/// What a material tag on `id` reads: its layers, one line each with
/// their thickness, outside to inside, or its one material's name.
pub fn material_text(model: &Model, id: ElementId) -> String {
    let Some(e) = model.get(id) else {
        return String::new();
    };
    let tag = |m: f64| creabim_core::write_number(model, creabim_core::NumberKind::Tags, None, m);
    if let Some((_, a)) = creabim_core::assembly_of(model, e) {
        if !a.layers.is_empty() {
            return a
                .layers
                .iter()
                .map(|l| {
                    let name = l
                        .material
                        .and_then(|m| model.get(m))
                        .map_or("(no material)".to_string(), |m| m.name.clone());
                    format!("{} {name}", tag(l.thickness))
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
    }
    creabim_core::material_of(model, e).map_or("(no material)".into(), |(m, _)| m.name.clone())
}

/// What a tag on `id` reads, and whether it sits in a circle (doors) or a box.
pub fn tag_text(model: &Model, id: ElementId) -> (String, bool) {
    let Some(e) = model.get(id) else {
        return (String::new(), false);
    };
    let material = creabim_core::material_of(model, e).map(|(m, _)| m.name.clone());
    let units = creabim_core::units::number_format(model, creabim_core::NumberKind::Tags, None)
        .units(&model.units());
    let tag = |m: f64| creabim_core::write_number(model, creabim_core::NumberKind::Tags, None, m);
    let with_material = |t: f64| match &material {
        Some(m) => format!("{m} {}", tag(t)),
        None => tag(t),
    };
    // a mark, typed or numbered by the project's rules, is what it reads
    if !matches!(e.kind, ElementKind::Object(_)) {
        if let Some(m) = creabim_core::marks::mark_of(model, id) {
            let round = matches!(&e.kind, ElementKind::Opening(o) if o.kind == OpeningKind::Door);
            return (m, round);
        }
    }
    match &e.kind {
        ElementKind::Opening(o) => (e.name.clone(), o.kind == OpeningKind::Door),
        ElementKind::Wall(w) => (with_material(w.thickness), false),
        ElementKind::Slab(s) => (with_material(s.thickness), false),
        ElementKind::Column(c) => (c.profile.label_in(&units), false),
        ElementKind::Beam(b) => (format!("{} × {}", tag(b.width), tag(b.depth)), false),
        ElementKind::Roof(r) => (
            format!("{} {:.0}°", with_material(r.thickness), r.pitch),
            false,
        ),
        // furniture and fixtures: their type's mark, in a circle like doors
        ElementKind::Object(_) => match creabim_core::marks::object_mark(model, e) {
            Some(m) => (m, true),
            None => (e.name.clone(), false),
        },
        ElementKind::Footing(f) => (
            match f.kind {
                creabim_core::FootingKind::Strip => {
                    format!("{} × {}", tag(f.width), tag(f.thickness))
                }
                creabim_core::FootingKind::Pad => format!(
                    "{} × {} × {}",
                    tag(f.width),
                    tag(f.length),
                    tag(f.thickness)
                ),
            },
            false,
        ),
        ElementKind::Room(_) => (room_tag_text(model, id, &[]), false),
        // a ceiling: its height above the floor, as its tag in a ceiling
        // plan writes it (MU142: one tag language)
        ElementKind::Ceiling(c) => (crate::rooms::ceiling_height_text(model, "CH", c), false),
        _ => (e.name.clone(), false),
    }
}

/// What a room's tag reads: each of `fields` on its own line (the room's
/// name, number and area when there are none), its name in capitals and
/// its number and area side by side; fields the room has no value for are
/// left out.
pub fn room_tag_text(model: &Model, id: ElementId, fields: &[RoomField]) -> String {
    let Some(e) = model.get(id) else {
        return String::new();
    };
    if !matches!(e.kind, ElementKind::Room(_)) {
        return e.name.clone();
    }
    let fields = if fields.is_empty() {
        &RoomField::DEFAULT[..]
    } else {
        fields
    };
    let mut lines: Vec<String> = Vec::new();
    let mut joined = false;
    for (i, f) in fields.iter().enumerate() {
        let value = f.value(model, id);
        if value.is_empty() {
            continue;
        }
        let value = if *f == RoomField::Name {
            value.to_uppercase()
        } else {
            value
        };
        // the area goes beside the number written just before it
        if *f == RoomField::Area && joined && i > 0 && fields[i - 1] == RoomField::Number {
            if let Some(last) = lines.last_mut() {
                last.push_str("  ");
                last.push_str(&value);
                continue;
            }
        }
        joined = *f == RoomField::Number;
        lines.push(value);
    }
    lines.join("\n")
}

/// A room's tag as drawing text: its lines stacked and centred on `at`.
pub fn room_tag_item(id: ElementId, text: String, at: DVec2, look: TextLook, mm: f64) -> PlanItem {
    // its lines centred by their real widths (MU29)
    let mut look = look;
    if look.align == creabim_core::TextAlign::Usual {
        look.align = creabim_core::TextAlign::Centre;
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let width = lines.iter().map(|l| look.width(l)).fold(0.0, f64::max) * mm;
    let step = look.size * 1.5 * mm;
    let start = at + DVec2::new(-width / 2.0, step * (lines.len() - 1) as f64 / 2.0);
    PlanItem::Annotation {
        id,
        item: Box::new(Annotation {
            shape: Shape::Note {
                at: start,
                leader: None,
                elbow: None,
            },
            text,
            look,
            mm,
        }),
    }
}

/// A room's tag laid out as `style` says, centred on `at`: each line its
/// own drawing text. Stacked writes the name in capitals and semibold, the
/// area below it and the number small and last; Inline is
/// [`room_tag_text`] in one block. A tag moved out of its room (`room` is
/// its outline in the drawing) gets a leader back to `centre`.
#[allow(clippy::too_many_arguments)]
/// `name` broken at the space nearest its middle, on two lines.
pub fn two_lines(name: &str) -> String {
    let mid = name.len() / 2;
    match name
        .match_indices(' ')
        .map(|(i, _)| i)
        .min_by_key(|i| i.abs_diff(mid))
    {
        Some(i) => format!("{}\n{}", &name[..i], &name[i + 1..]),
        None => name.to_string(),
    }
}

/// The fields a room tag shows in a view: what it asks for (name, area,
/// number when nothing), less what the scale table's row leaves out at
/// the view's scale (MU131: at 1:100 tags said as much as at 1:50 and ran
/// into each other).
pub fn room_fields_at(
    fields: &[RoomField],
    says: creabim_core::styles::RoomTagSays,
) -> Vec<RoomField> {
    use creabim_core::styles::RoomTagSays as S;
    let all: Vec<RoomField> = if fields.is_empty() {
        vec![RoomField::Name, RoomField::Area, RoomField::Number]
    } else {
        fields.to_vec()
    };
    let keep = |f: &RoomField| match says {
        S::All => true,
        S::NameNumber => matches!(f, RoomField::Name | RoomField::Number),
        S::Number => matches!(f, RoomField::Number),
    };
    let some: Vec<RoomField> = all.iter().copied().filter(keep).collect();
    if some.is_empty() {
        all
    } else {
        some
    }
}

/// The room tags of a plan's `items` too big for their room even fitted
/// (broken and shrunk): the label pass may take them outside the plan.
pub fn tags_too_big(
    model: &Model,
    view: ElementId,
    items: &[PlanItem],
    mm: f64,
) -> std::collections::HashSet<ElementId> {
    let mut out = std::collections::HashSet::new();
    for e in model.iter() {
        let ElementKind::Tag(t) = &e.kind else {
            continue;
        };
        if t.view != view {
            continue;
        }
        let Some(outline) = area_outline(model, t.host) else {
            continue;
        };
        let (lo, hi) = outline
            .iter()
            .fold((DVec2::MAX, DVec2::MIN), |(a, b), p| (a.min(*p), b.max(*p)));
        let own: Vec<PlanItem> = items.iter().filter(|i| i.id() == e.id).cloned().collect();
        if let Some((a, b)) = tag_extent(&own, mm) {
            if b.x - a.x > hi.x - lo.x - 1.0 * mm || b.y - a.y > hi.y - lo.y - 1.0 * mm {
                out.insert(e.id);
            }
        }
    }
    out
}

/// The outline in plan of what an area tag reads: a room's, or a
/// ceiling's. None for anything else.
fn area_outline(model: &Model, host: ElementId) -> Option<Vec<DVec2>> {
    match &model.get(host)?.kind {
        ElementKind::Room(r) => creabim_core::room::shape_of(model, r).map(|s| s.outline),
        ElementKind::Ceiling(c) => c.outline(model),
        _ => None,
    }
    .filter(|o| o.len() > 2)
}

/// The room and ceiling tags of plan `view`, each with its room's or
/// ceiling's outline: a tag the label pass nudges but keeps inside it
/// needs no leader back.
pub fn room_tag_rooms(
    model: &Model,
    view: ElementId,
) -> std::collections::HashMap<ElementId, Vec<DVec2>> {
    model
        .iter()
        .filter_map(|e| {
            let ElementKind::Tag(t) = &e.kind else {
                return None;
            };
            if t.view != view {
                return None;
            }
            Some((e.id, area_outline(model, t.host)?))
        })
        .collect()
}

/// The box a room tag's lines cover, in drawing units.
fn tag_extent(items: &[PlanItem], mm: f64) -> Option<(DVec2, DVec2)> {
    items
        .iter()
        .filter_map(|it| crate::declutter::text_box(it, mm).map(|(b, _)| (b.lo, b.hi)))
        .reduce(|(a, b), (c, d)| (a.min(c), b.max(d)))
}

/// A room tag that fits its room: as it is, else its name on two lines,
/// else smaller, down to 1.8 mm (MU131). `make` draws it with a size
/// factor and the name wrapped or not; `room` is the room's outline.
fn fitted_tag(
    make: &dyn Fn(f64, bool) -> Vec<PlanItem>,
    room: Option<&[DVec2]>,
    size: f64,
    mm: f64,
) -> Vec<PlanItem> {
    let first = make(1.0, false);
    let Some(room) = room.filter(|r| r.len() > 2) else {
        return first;
    };
    let (lo, hi) = room
        .iter()
        .fold((DVec2::MAX, DVec2::MIN), |(a, b), p| (a.min(*p), b.max(*p)));
    let room_w = (hi.x - lo.x) - 1.0 * mm;
    let room_h = (hi.y - lo.y) - 1.0 * mm;
    let fits = |items: &[PlanItem]| {
        tag_extent(items, mm).is_none_or(|(a, b)| b.x - a.x <= room_w && b.y - a.y <= room_h)
    };
    if fits(&first) {
        return first;
    }
    let least = (1.8 / size.max(1e-9)).min(1.0);
    for (k, wrap) in [(1.0, true), (0.85, false), (0.85, true), (least, true)] {
        let k = k.max(least);
        let items = make(k, wrap);
        if fits(&items) {
            return items;
        }
    }
    // still too big: as small as it goes; the label pass moves it out
    make(least, true)
}

#[allow(clippy::too_many_arguments)]
pub fn room_tag_items(
    model: &Model,
    id: ElementId,
    room: ElementId,
    fields: &[RoomField],
    at: DVec2,
    centre: DVec2,
    outline: Option<&[DVec2]>,
    look: TextLook,
    mm: f64,
    style: creabim_core::styles::RoomTagStyle,
    wrap_name: bool,
) -> Vec<PlanItem> {
    use creabim_core::styles::RoomTagStyle;
    let outside = away_from(at, centre, outline, mm);
    let mut out = match style {
        RoomTagStyle::Inline => {
            vec![room_tag_item(
                id,
                room_tag_text(model, room, fields),
                at,
                look,
                mm,
            )]
        }
        RoomTagStyle::Stacked => {
            let order: &[RoomField] = if fields.is_empty() {
                &[RoomField::Name, RoomField::Area, RoomField::Number]
            } else {
                fields
            };
            let lines: Vec<(String, TextLook)> = order
                .iter()
                .filter_map(|f| {
                    let v = f.value(model, room);
                    if v.is_empty() {
                        return None;
                    }
                    let mut l = look;
                    Some(match f {
                        RoomField::Name => {
                            l.bold = true;
                            (
                                if wrap_name {
                                    two_lines(&v.to_uppercase())
                                } else {
                                    v.to_uppercase()
                                },
                                l,
                            )
                        }
                        RoomField::Number => {
                            l.size *= 0.72;
                            (v, l)
                        }
                        _ => {
                            l.size *= 0.9;
                            (v, l)
                        }
                    })
                })
                .collect();
            // a name on two lines takes two steps
            let steps: Vec<f64> = lines
                .iter()
                .map(|(t, l)| l.size * 1.45 * mm * t.split('\n').count() as f64)
                .collect();
            let total: f64 = steps.iter().sum::<f64>() - steps.last().copied().unwrap_or(0.0);
            let mut y = at.y + total / 2.0;
            let mut items = vec![];
            // left or right aligned: every line against the block's edge
            let block = lines
                .iter()
                .map(|(t, l)| l.width(t) * mm)
                .fold(0.0, f64::max);
            for ((text, l), step) in lines.into_iter().zip(steps) {
                let w = l.width(&text) * mm;
                let x = match look.align {
                    creabim_core::TextAlign::Left => at.x - block / 2.0 + w / 2.0,
                    creabim_core::TextAlign::Right => at.x + block / 2.0 - w / 2.0,
                    _ => at.x,
                };
                items.push(room_tag_item(id, text, DVec2::new(x, y), l, mm));
                y -= step;
            }
            items
        }
        RoomTagStyle::NumberBadge | RoomTagStyle::Boxed => {
            let value = |f: RoomField| {
                let shown = fields.is_empty() || fields.contains(&f);
                Some(f.value(model, room)).filter(|v| shown && !v.is_empty())
            };
            let (name, number, area) = (
                value(RoomField::Name),
                value(RoomField::Number),
                value(RoomField::Area),
            );
            let mut bold = look;
            bold.bold = true;
            let mut small = look;
            small.size *= 0.72;
            let mut mid = look;
            mid.size *= 0.9;
            let pen = crate::plan::Pen::Symbol;
            let rect = |lo: DVec2, hi: DVec2| PlanItem::Polygon {
                id,
                pen,
                points: vec![lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)],
            };
            let mut items = vec![];
            if style == RoomTagStyle::NumberBadge {
                // the number in a small outline on top, the name, the area
                let step = look.size * 1.45 * mm;
                let rows = [number.is_some(), name.is_some(), area.is_some()]
                    .iter()
                    .filter(|r| **r)
                    .count();
                let mut y = at.y + step * (rows.saturating_sub(1)) as f64 / 2.0;
                if let Some(n) = number {
                    let w = small.width(&n) * mm + 1.6 * mm;
                    let h = small.size * 1.5 * mm;
                    let c = DVec2::new(at.x, y);
                    items.push(PlanItem::Polygon {
                        id,
                        pen,
                        points: (0..20)
                            .map(|i| {
                                // a stadium round the number
                                let t = i as f64 / 20.0 * std::f64::consts::TAU;
                                let (cx, cy) = (t.cos(), t.sin());
                                let half = (w - h).max(0.0) / 2.0;
                                c + DVec2::new(cx * h / 2.0 + half * cx.signum(), cy * h / 2.0)
                            })
                            .collect(),
                    });
                    items.push(room_tag_item(id, n, c, small, mm));
                    y -= step;
                }
                if let Some(n) = name {
                    items.push(room_tag_item(
                        id,
                        n.to_uppercase(),
                        DVec2::new(at.x, y),
                        bold,
                        mm,
                    ));
                    y -= step;
                }
                if let Some(a) = area {
                    items.push(room_tag_item(id, a, DVec2::new(at.x, y), mid, mm));
                }
            } else {
                // a box: the name, and a footer with the number and area
                let pad = 1.2 * mm;
                let top = name
                    .as_ref()
                    .map_or(0.0, |n| bold.width(&n.to_uppercase()) * mm);
                // text drawn on screen runs a little wider than the
                // estimate, so the footer leaves room and a divider
                let foot_gap = 2.0 * pad;
                let nw = number.as_ref().map_or(0.0, |n| small.width(n) * mm * 1.15);
                let aw = area.as_ref().map_or(0.0, |a| mid.width(a) * mm * 1.15);
                let foot = nw + aw + foot_gap;
                let w = top.max(foot) + 2.0 * pad;
                let h_top = look.size * 1.6 * mm;
                let h_foot = look.size * 1.4 * mm;
                let lo = DVec2::new(at.x - w / 2.0, at.y - (h_top + h_foot) / 2.0);
                let hi = DVec2::new(at.x + w / 2.0, at.y + (h_top + h_foot) / 2.0);
                let split = hi.y - h_top;
                items.push(rect(lo, hi));
                items.push(PlanItem::Line {
                    id,
                    pen,
                    a: DVec2::new(lo.x, split),
                    b: DVec2::new(hi.x, split),
                });
                if let Some(n) = name {
                    items.push(room_tag_item(
                        id,
                        n.to_uppercase(),
                        DVec2::new(at.x, split + h_top / 2.0),
                        bold,
                        mm,
                    ));
                }
                let fy = lo.y + h_foot / 2.0;
                if number.is_some() && area.is_some() {
                    let x = lo.x + 2.0 * pad + nw;
                    items.push(PlanItem::Line {
                        id,
                        pen,
                        a: DVec2::new(x, lo.y),
                        b: DVec2::new(x, split),
                    });
                }
                if let Some(n) = number {
                    items.push(room_tag_item(
                        id,
                        n,
                        DVec2::new(lo.x + pad + nw / 2.0, fy),
                        small,
                        mm,
                    ));
                }
                if let Some(a) = area {
                    items.push(room_tag_item(
                        id,
                        a,
                        DVec2::new(hi.x - pad - aw / 2.0, fy),
                        mid,
                        mm,
                    ));
                }
            }
            items
        }
    };
    if outside {
        lead_back(model, id, &mut out, at, centre, look, mm);
    }
    out
}

/// Is a tag at `at` away from what it tags, whose middle is `centre` and
/// outline `outline` (a room, a ceiling): out of it, or put by hand well
/// away from its spot inside it. Such a tag draws a leader back (MU142: a
/// tag dragged inside its room lost the leader every other tag had).
fn away_from(at: DVec2, centre: DVec2, outline: Option<&[DVec2]>, mm: f64) -> bool {
    let away = (at - centre).length() > crate::declutter::LEADER_PAST * mm;
    outline.is_some_and(|o| o.len() > 2 && (away || !creabim_core::polygon::contains(o, at)))
}

/// A leader from area tag `id`'s first line back to what it tags: to
/// where its end was dragged, through its bend, both kept from the middle
/// `centre` (MU142: the leader's grips showed but did not move it).
fn lead_back(
    model: &Model,
    id: ElementId,
    out: &mut [PlanItem],
    at: DVec2,
    centre: DVec2,
    look: TextLook,
    mm: f64,
) {
    let (end, elbow) = match model.get(id).map(|e| &e.kind) {
        Some(ElementKind::Tag(t)) => (t.end, t.elbow),
        _ => (None, None),
    };
    let centre_mid = centre;
    let centre = centre + end.unwrap_or(DVec2::ZERO);
    let gap = look.size * mm;
    let d = centre - at;
    if d.length() > gap * 2.0 {
        // the tag's own leader, from its first line, in its colour and
        // with its end, as the label pass gives a tag it nudges (MU142:
        // a white symbol line with a dot read apart from the others)
        if let Some(PlanItem::Annotation { item, .. }) = out
            .iter_mut()
            .find(|i| matches!(i, PlanItem::Annotation { .. }))
        {
            if let Shape::Note {
                leader,
                elbow: bend,
                ..
            } = &mut item.shape
            {
                *leader = Some(centre);
                *bend = elbow.map(|p| centre_mid + p);
            }
        }
    }
}

/// A ceiling's height tag in a ceiling plan: one line, "CH 7'-11 1/2"", in
/// the tag's style and the annotation colour, as a room's tag is drawn,
/// with a leader back once it is moved out of its ceiling (MU142, owner:
/// one tag language for everything that labels the model).
#[allow(clippy::too_many_arguments)]
pub fn ceiling_tag_items(
    model: &Model,
    id: ElementId,
    t: &creabim_core::Tag,
    c: &creabim_core::Ceiling,
    centre: DVec2,
    outline: Option<&[DVec2]>,
    look: TextLook,
    mm: f64,
) -> Vec<PlanItem> {
    let at = centre + t.offset;
    // a material tag reads the ceiling's material, as any tag set so does
    let text = if t.material {
        material_text(model, t.host)
    } else {
        crate::rooms::ceiling_height_text(model, t.prefix(), c)
    };
    let mut out = vec![room_tag_item(id, text, at, look, mm)];
    if !t.no_leader && away_from(at, centre, outline, mm) {
        lead_back(model, id, &mut out, at, centre, look, mm);
    }
    out
}

/// A dimension's value as it is written in `units`.
pub fn dimension_text(units: &creabim_core::Units, a: DVec2, b: DVec2) -> String {
    units.dimension(a.distance(b))
}

/// A linear dimension from `a` to `b` measured along `axis`: where `b`
/// lands on the line through `a` that way, and the true points its
/// witness lines start from (none when aligned). None when the points
/// coincide along it, so there is nothing to measure.
pub fn squared(
    axis: creabim_core::DimAxis,
    a: DVec2,
    b: DVec2,
) -> Option<(DVec2, Option<[DVec2; 2]>)> {
    let u = axis.direction(a, b)?;
    if axis.is_aligned() {
        return (a.distance(b) > 1e-6).then_some((b, None));
    }
    let b2 = a + u * (b - a).dot(u);
    (a.distance(b2) > 1e-6).then_some((b2, Some([a, b])))
}

/// How an annotation with `style` and its own `color` looks.
pub fn look_of(model: &Model, style: Option<ElementId>, color: Option<[f32; 3]>) -> TextLook {
    creabim_core::text_style(model, style, color).into()
}

/// A dimension about a centre (angle, radius, diameter, arc length) as a
/// drawing item, if its points can be drawn in the view.
fn measured(
    model: &Model,
    view: &View,
    id: ElementId,
    d: &creabim_core::Dimension,
) -> Option<PlanItem> {
    let c = resolve(model, view, d.center.as_ref()?)?;
    let a = resolve(model, view, &d.a)?;
    let b = resolve(model, view, &d.b)?;
    if c.distance(a) < 1e-6 {
        return None;
    }
    let shape = match d.kind {
        DimensionKind::Linear | DimensionKind::Spot => return None,
        DimensionKind::Angular => {
            if c.distance(b) < 1e-6 {
                return None;
            }
            Shape::Angle {
                vertex: c,
                a,
                b,
                radius: d.offset,
            }
        }
        DimensionKind::Radius | DimensionKind::Diameter => Shape::Radius {
            center: c,
            at: a,
            extra: d.offset,
            diameter: d.kind == DimensionKind::Diameter,
        },
        DimensionKind::ArcLength => Shape::ArcLength {
            center: c,
            a,
            b,
            offset: d.offset,
        },
    };
    Some(PlanItem::Annotation {
        id,
        item: Box::new(Annotation {
            shape,
            text: measure(
                &creabim_core::units::number_format(
                    model,
                    creabim_core::NumberKind::Dimensions,
                    d.format,
                )
                .units(&model.units()),
                d.kind,
                c,
                a,
                b,
            ),
            look: look_of(model, d.style, d.color),
            mm: view.scale / 1000.0,
        }),
    })
}

/// The height a spot level reads at `at`: in a section or elevation the
/// point's own height; in a plan the top of the slab it was put on, or
/// else the plan's level (a level marker).
pub fn spot_height(
    model: &Model,
    view: &View,
    d: &creabim_core::Dimension,
    at: DVec2,
) -> Option<f64> {
    match view.kind {
        ViewKind::Section { .. } => Some(at.y),
        ViewKind::Plan { level, .. } => {
            let elevation = |l: ElementId| model.level(l).map(|l| l.elevation);
            match d.a.host.and_then(|h| model.get(h)).map(|e| &e.kind) {
                Some(ElementKind::Slab(s)) => Some(s.top(elevation(s.level)?)),
                _ => elevation(level),
            }
        }
        _ => None,
    }
}

/// A spot level as a drawing item, if it can be drawn in the view.
fn spot(
    model: &Model,
    view: &View,
    id: ElementId,
    d: &creabim_core::Dimension,
) -> Option<PlanItem> {
    let at = resolve(model, view, &d.a)?;
    let text_at = resolve(model, view, &d.b)?;
    let z = spot_height(model, view, d, at)?;
    let pattern = d
        .pattern
        .as_deref()
        .unwrap_or(creabim_core::annotation::SPOT_PATTERN);
    Some(PlanItem::Annotation {
        id,
        item: Box::new(Annotation {
            shape: Shape::Spot {
                at,
                text_at,
                plan: matches!(view.kind, ViewKind::Plan { .. }),
            },
            text: creabim_core::annotation::spot_text(model, pattern, z),
            look: look_of(model, d.style, d.color),
            mm: view.scale / 1000.0,
        }),
    })
}

/// A note as a drawing item; `leader` resolves its leader point.
pub fn note_item(
    model: &Model,
    id: ElementId,
    n: &Note,
    leader: Option<DVec2>,
    mm: f64,
) -> PlanItem {
    if let Some(p) = &n.symbol {
        let view = match model.get(n.view).map(|e| &e.kind) {
            Some(ElementKind::View(v)) => Some(v),
            _ => None,
        };
        if let Some(s) = creabim_core::symbols::placed_style(model, view, p) {
            return crate::symbol::placed(id, s, n.at, n.text.clone(), p.angle, mm);
        }
    }
    // a block of a long note shows its share of the lines, laid out with
    // the first block's look so the parts read as one text
    if let Some((lines, _)) = chain_share(model, id) {
        let first = chain_first(model, id).unwrap_or(n);
        let mut look = look_of(model, first.style, first.color);
        look.leader_end = n.leader_end.or(look.leader_end);
        return PlanItem::Annotation {
            id,
            item: Box::new(Annotation {
                shape: Shape::Note {
                    at: n.at,
                    leader,
                    elbow: leader.and(n.elbow),
                },
                text: lines.join("\n"),
                look,
                mm,
            }),
        };
    }
    let mut look = look_of(model, n.style, n.color);
    look.leader_end = n.leader_end.or(look.leader_end);
    // a notes section's text as Markdown, or its own text with what it
    // reads from an object filled in
    let (text, markdown) = note_shown_text(model, n);
    let text = text.as_str();
    PlanItem::Annotation {
        id,
        item: Box::new(Annotation {
            shape: Shape::Note {
                at: n.at,
                leader,
                elbow: leader.and(n.elbow),
            },
            text: crate::annotation::lay_out_note(text, &look, n.width, markdown),
            look,
            mm,
        }),
    }
}

/// The first block of the long note `id` belongs to.
fn chain_first(model: &Model, id: ElementId) -> Option<&Note> {
    let n = match &model.get(id)?.kind {
        ElementKind::Note(n) => n,
        _ => return None,
    };
    let first = n.follows.map_or(id, |f| f.first);
    match &model.get(first)?.kind {
        ElementKind::Note(f) => Some(f),
        _ => None,
    }
}

/// The blocks of the long note whose first block is `first`, in order:
/// the first, then those that carry it on by their part number.
pub fn chain_blocks(model: &Model, first: ElementId) -> Vec<ElementId> {
    let mut rest: Vec<(u32, ElementId)> = model
        .iter()
        .filter_map(|e| match &e.kind {
            ElementKind::Note(n) => n
                .follows
                .filter(|f| f.first == first)
                .map(|f| (f.part, e.id)),
            _ => None,
        })
        .collect();
    rest.sort();
    std::iter::once(first)
        .chain(rest.into_iter().map(|(_, id)| id))
        .collect()
}

/// A long note's blocks share out its lines (Mockup_Long_Notes_REV00):
/// the block `id`'s own lines, laid out, and how many are left over after
/// the last block. None when `id` is a note of its own or a first block
/// shown Whole; a later block of a Whole note gets no lines.
pub fn chain_share(model: &Model, id: ElementId) -> Option<(Vec<String>, usize)> {
    let n = match &model.get(id)?.kind {
        ElementKind::Note(n) => n,
        _ => return None,
    };
    let first_id = n.follows.map_or(id, |f| f.first);
    let first = chain_first(model, id)?;
    if !first.continued {
        return n.follows.map(|_| (vec![], 0));
    }
    let (text, markdown) = note_shown_text(model, first);
    let look = look_of(model, first.style, first.color);
    let laid: Vec<String> = crate::annotation::lay_out_note(&text, &look, first.width, markdown)
        .split('\n')
        .map(String::from)
        .collect();
    let pitch = (look.size * 1.5).max(0.1);
    let name = first
        .section
        .as_deref()
        .and_then(|k| model.project_info().and_then(|(_, p)| p.section_name(k)))
        .unwrap_or_default();
    let mut i = 0;
    let mut mine = None;
    for (k, block) in chain_blocks(model, first_id).into_iter().enumerate() {
        let Some(ElementKind::Note(b)) = model.get(block).map(|e| &e.kind) else {
            continue;
        };
        let mut room = b
            .height
            .map_or(usize::MAX, |h| ((h / pitch).floor() as usize).max(1));
        let mut out = vec![];
        if k > 0 {
            // a later block starts on a line with words, under its title
            while laid.get(i).is_some_and(|l| l.trim().is_empty()) {
                i += 1;
            }
            if let Some(t) = b.follows.and_then(|f| f.title.line(&name)) {
                out.push(t);
                room = room.saturating_sub(1).max(1);
            }
        }
        let take = room.min(laid.len() - i);
        out.extend(laid[i..i + take].iter().cloned());
        i += take;
        if block == id {
            mine = Some(out);
        }
    }
    Some((mine?, laid.len() - i))
}

/// Lines that run past the block `id`'s height: for a Whole note, those
/// below its foot; for the last block of a Continued one, those no block
/// holds. Shown on screen as a red mark, never printed.
pub fn note_runs_past(model: &Model, id: ElementId) -> usize {
    let Some(ElementKind::Note(n)) = model.get(id).map(|e| &e.kind) else {
        return 0;
    };
    if n.follows.is_some() || n.continued {
        return chain_overflow(model, id);
    }
    let Some(h) = n.height else { return 0 };
    let (text, markdown) = note_shown_text(model, n);
    let look = look_of(model, n.style, n.color);
    let lines = crate::annotation::lay_out_note(&text, &look, n.width, markdown)
        .split('\n')
        .count();
    let room = ((h / (look.size * 1.5).max(0.1)).floor() as usize).max(1);
    lines.saturating_sub(room)
}

/// Lines a long note could not fit, when `id` is its last block.
pub fn chain_overflow(model: &Model, id: ElementId) -> usize {
    let Some(first) = chain_first(model, id) else {
        return 0;
    };
    if !first.continued {
        return 0;
    }
    let first_id = match &model.get(id).map(|e| &e.kind) {
        Some(ElementKind::Note(n)) => n.follows.map_or(id, |f| f.first),
        _ => return 0,
    };
    if chain_blocks(model, first_id).last() != Some(&id) {
        return 0;
    }
    chain_share(model, id).map_or(0, |(_, left)| left)
}

/// The notes written straight on a sheet, in paper millimetres.
pub fn sheet_notes(model: &Model, sheet: ElementId) -> Vec<PlanItem> {
    model
        .iter()
        .filter_map(|e| match &e.kind {
            ElementKind::Note(n) if n.view == sheet => {
                Some(note_item(model, e.id, n, n.leader.map(|p| p.local), 1.0))
            }
            _ => None,
        })
        .collect()
}

/// The dimensions, tags, notes and detail components placed in a view,
/// and the boxes of the detail views called out from it, as drawing items.
pub fn annotations(model: &Model, view_id: ElementId, view: &View) -> Vec<PlanItem> {
    let Some(project) = Projector::of(model, view) else {
        return vec![];
    };
    let rule = creabim_core::styles::view_rule(model, view);
    let mut out = annotation_items(model, view_id, view, &project, &rule);
    to_scale(&mut out, &rule);
    out
}

/// A length rounded to the scale table's step for its row: a millimetre
/// in a detail, ten in a plan; 1/16" to 1" in feet and inches.
pub fn rounded_to_scale(model: &Model, rule: &creabim_core::styles::ScaleRule, metres: f64) -> f64 {
    let step = if model.units().length.is_imperial() {
        0.0254 / f64::from(rule.round_in.max(1))
    } else {
        rule.round_mm.max(0.001) / 1000.0
    };
    (metres / step).round() * step
}

/// Ticks and lettering at the sizes the scale table gives the view's row,
/// where a text style left them as they come (a style's own sizes stay).
fn to_scale(items: &mut [PlanItem], rule: &creabim_core::styles::ScaleRule) {
    use creabim_core::annotation::DEFAULT_TEXT_SIZE;
    let size = |look: &mut crate::annotation::TextLook, to: f64| {
        if (look.size - DEFAULT_TEXT_SIZE).abs() < 1e-9 {
            look.size = to;
        }
    };
    let tick = |look: &mut crate::annotation::TextLook| {
        if (look.end_size - crate::annotation::END_SIZE).abs() < 1e-9 {
            look.end_size = rule.dim_tick;
        }
    };
    for it in items.iter_mut() {
        match it {
            PlanItem::Dimension { look, .. } => {
                size(look, rule.dim_text);
                tick(look);
            }
            PlanItem::Tag { look, .. } => size(look, rule.note_text),
            PlanItem::Annotation { item, .. } => {
                let note = matches!(item.shape, crate::annotation::Shape::Note { .. });
                size(
                    &mut item.look,
                    if note { rule.note_text } else { rule.dim_text },
                );
                if !note {
                    tick(&mut item.look);
                }
            }
            _ => {}
        }
    }
}

fn annotation_items(
    model: &Model,
    view_id: ElementId,
    view: &View,
    project: &Projector,
    rule: &creabim_core::styles::ScaleRule,
) -> Vec<PlanItem> {
    let mut out = Vec::new();
    for e in model.iter() {
        match &e.kind {
            ElementKind::Detail(d) if d.view == view_id => {
                let look = creabim_core::styles::active_pack(model).membranes;
                out.extend(crate::detail::detail_items(
                    e.id,
                    d,
                    view.scale / 1000.0,
                    look,
                ));
            }
            ElementKind::View(v) => {
                if let Some(c) = v.callout.filter(|c| c.parent == view_id) {
                    let mm = view.scale / 1000.0;
                    let round = matches!(view.kind, ViewKind::Section { .. });
                    if let Some(s) = creabim_core::symbols::pick(model, view, "callouts") {
                        // the callout's symbol picked: the box, the symbol
                        // on its corner (Mockup_Symbols_REV00)
                        let parts = creabim_core::styles::active_pack(model)
                            .marker_parts()
                            .unwrap_or_default();
                        let (bx, corner, _) =
                            crate::markers::callout_box(e.id, c.lo, c.hi, round, &parts, mm);
                        out.extend(bx);
                        out.push(crate::symbol::callout_head(
                            e.id,
                            s,
                            corner,
                            crate::markers::number_of(model, e.id),
                            mm,
                        ));
                        continue;
                    }
                    match creabim_core::styles::active_pack(model).marker_parts() {
                        Some(parts) => {
                            // a detail called out of a section is ringed
                            let round = matches!(view.kind, ViewKind::Section { .. });
                            out.extend(crate::markers::callout(
                                e.id,
                                &crate::markers::number_of(model, e.id),
                                &crate::markers::sheet_ref(model, e.id),
                                c.lo,
                                c.hi,
                                round,
                                &parts,
                                mm,
                            ));
                        }
                        None => {
                            out.extend(crate::detail::callout_items(e.id, &e.name, c.lo, c.hi, mm))
                        }
                    }
                }
            }
            ElementKind::Dimension(d) if d.view == view_id && d.kind == DimensionKind::Spot => {
                out.extend(spot(model, view, e.id, d));
            }
            ElementKind::Dimension(d) if d.view == view_id && !d.kind.is_linear() => {
                out.extend(measured(model, view, e.id, d));
            }
            ElementKind::Note(n) if n.view == view_id => {
                let leader = n.leader.and_then(|p| resolve(model, view, &p));
                out.push(note_item(model, e.id, n, leader, view.scale / 1000.0));
            }
            ElementKind::Dimension(d) if d.view == view_id => {
                let (Some(a), Some(b)) = (resolve(model, view, &d.a), resolve(model, view, &d.b))
                else {
                    continue;
                };
                let Some((b2, witness)) = squared(d.axis, a, b) else {
                    continue;
                };
                // rounded to the scale table's step, unless the dimension
                // carries a number format of its own
                let length = match d.format {
                    Some(_) => a.distance(b2),
                    None => rounded_to_scale(model, rule, a.distance(b2)),
                };
                let measured = creabim_core::write_number(
                    model,
                    creabim_core::NumberKind::Dimensions,
                    d.format,
                    length,
                );
                out.push(PlanItem::Dimension {
                    id: e.id,
                    a,
                    b: b2,
                    offset: d.offset,
                    text: d.label.compose(&measured),
                    look: look_of(model, d.style, d.color),
                    witness,
                    breaks: vec![],
                    label: DimLabel::of(&d.label),
                });
            }
            ElementKind::Tag(t) if t.view == view_id => {
                let Some(c) = centre(model, t.host) else {
                    continue;
                };
                let centre = project.project(c);
                // a room's tag is its name, number and area, without a bubble
                if matches!(
                    model.get(t.host).map(|h| &h.kind),
                    Some(ElementKind::Room(_))
                ) {
                    if !view.shows_room_tags() {
                        continue;
                    }
                    let outline = match model.get(t.host).map(|h| &h.kind) {
                        Some(ElementKind::Room(r)) => {
                            creabim_core::room::shape_of(model, r).map(|s| {
                                s.outline
                                    .iter()
                                    .map(|p| project.project(p.extend(c.z)))
                                    .collect::<Vec<_>>()
                            })
                        }
                        _ => None,
                    };
                    let fields = room_fields_at(&t.fields, rule.room_tag);
                    let look = look_of(model, t.style, t.color);
                    let mm = view.scale / 1000.0;
                    let style = creabim_core::styles::active_pack(model).room_tags;
                    let make = |k: f64, wrap: bool| {
                        let mut l = look;
                        l.size *= k;
                        room_tag_items(
                            model,
                            e.id,
                            t.host,
                            &fields,
                            centre + t.offset,
                            centre,
                            // its outline tells a tag out of its room, which
                            // draws a leader back unless it is turned off
                            outline.as_deref().filter(|_| !t.no_leader),
                            l,
                            mm,
                            style,
                            wrap,
                        )
                    };
                    // a tag moved by hand stays as it was put (MU131 d4)
                    if t.offset.length() > 1e-9 {
                        out.extend(make(1.0, false));
                    } else {
                        out.extend(fitted_tag(&make, outline.as_deref(), look.size, mm));
                    }
                    continue;
                }
                // a ceiling's height, in a ceiling plan that shows them
                if let Some(ElementKind::Ceiling(cl)) = model.get(t.host).map(|h| &h.kind) {
                    if !view.ceiling_plan || view.graphics.hide_ceiling_heights {
                        continue;
                    }
                    let outline: Option<Vec<DVec2>> = cl
                        .outline(model)
                        .map(|o| o.iter().map(|p| project.project(p.extend(c.z))).collect());
                    out.extend(ceiling_tag_items(
                        model,
                        e.id,
                        t,
                        cl,
                        centre,
                        outline.as_deref(),
                        look_of(model, t.style, t.color),
                        view.scale / 1000.0,
                    ));
                    continue;
                }
                // a material tag reads the build-up (Annotate step 5)
                let (text, round) = if t.material {
                    (material_text(model, t.host), false)
                } else {
                    tag_text(model, t.host)
                };
                let mut look = look_of(model, t.style, t.color);
                look.leader_end = t.leader_end.or(look.leader_end);
                out.push(PlanItem::Tag {
                    id: e.id,
                    anchor: centre + t.end.unwrap_or(DVec2::ZERO),
                    at: centre + t.offset,
                    elbow: t.elbow.map(|p| centre + p),
                    text,
                    round,
                    look,
                });
            }
            _ => {}
        }
    }
    out
}

/// Where a dimension line `p` → `q` crosses the box `corners` (grown by
/// `pad`), as fractions of the way along it.
fn crossing(p: DVec2, q: DVec2, corners: &[DVec2], pad: f64) -> Option<[f64; 2]> {
    let lo = corners.iter().copied().reduce(DVec2::min)? - DVec2::splat(pad);
    let hi = corners.iter().copied().reduce(DVec2::max)? + DVec2::splat(pad);
    let d = q - p;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (o, v, a, b) in [(p.x, d.x, lo.x, hi.x), (p.y, d.y, lo.y, hi.y)] {
        if v.abs() < 1e-12 {
            if o < a || o > b {
                return None;
            }
            continue;
        }
        let (mut s0, mut s1) = ((a - o) / v, (b - o) / v);
        if s0 > s1 {
            std::mem::swap(&mut s0, &mut s1);
        }
        t0 = t0.max(s0);
        t1 = t1.min(s1);
    }
    (t1 - t0 > 1e-9).then_some([t0, t1])
}

/// Give each dimension whose style breaks at tags the gaps where the
/// tags' bubbles and the notes' text sit across its line; `mm` is drawing
/// units per paper mm.
pub fn break_dimensions(items: &mut [PlanItem], mm: f64) {
    let mut boxes: Vec<Vec<DVec2>> = Vec::new();
    for it in items.iter() {
        match it {
            PlanItem::Tag {
                anchor,
                at,
                elbow,
                text,
                round,
                look,
                ..
            } => boxes.push(tag_geometry(*anchor, *at, *elbow, text, *round, look, mm).outline),
            PlanItem::Annotation { item, .. } if matches!(item.shape, Shape::Note { .. }) => {
                boxes.extend(item.text_boxes().iter().map(|b| b.to_vec()))
            }
            _ => {}
        }
    }
    boxes.retain(|b| !b.is_empty());
    for it in items.iter_mut() {
        let PlanItem::Dimension {
            a,
            b,
            offset,
            look,
            breaks,
            ..
        } = it
        else {
            continue;
        };
        breaks.clear();
        if !look.breaks {
            continue;
        }
        let n = (*b - *a).normalize_or_zero().perp() * *offset;
        let (p, q) = (*a + n, *b + n);
        let mut gaps: Vec<[f64; 2]> = boxes
            .iter()
            .filter_map(|c| crossing(p, q, c, 0.8 * mm))
            .collect();
        gaps.sort_by(|x, y| x[0].total_cmp(&y[0]));
        for g in gaps {
            match breaks.last_mut() {
                Some(last) if g[0] <= last[1] => last[1] = last[1].max(g[1]),
                _ => breaks.push(g),
            }
        }
    }
}

impl DimensionGeometry {
    /// Leave the stretches `breaks` (fractions of the way from `pa` to
    /// `pb`, the dimension line's measured ends) out of its line.
    pub fn break_line(&mut self, breaks: &[[f64; 2]]) {
        if breaks.is_empty() || self.lines.is_empty() {
            return;
        }
        let (s, e) = self.lines[0];
        let (pa, pb) = self.ends_at;
        let len = pa.distance(pb);
        if len < 1e-12 {
            return;
        }
        let u = (pb - pa) / len;
        let t = |p: DVec2| (p - pa).dot(u) / len;
        let at = |f: f64| pa + (pb - pa) * f;
        let mut pieces = vec![];
        let mut from = t(s);
        for br in breaks {
            if br[0] > from {
                pieces.push((at(from), at(br[0].min(t(e)))));
            }
            from = from.max(br[1]);
        }
        if t(e) > from {
            pieces.push((at(from), e));
        }
        pieces.retain(|(p, q)| p.distance(*q) > 1e-9);
        self.lines.splice(0..1, pieces);
    }
}

/// Linework of a dimension, in drawing units; `mm` is drawing units per
/// paper millimetre, so ticks and gaps are a fixed size on paper.
pub struct DimensionGeometry {
    pub lines: Vec<(DVec2, DVec2)>,
    /// The ends of the dimension line: ticks (drawn heavier than the
    /// line), arrows, dots, as its style says.
    pub ends: Ends,
    pub text_at: DVec2,
    /// Text angle in radians, kept readable (never upside down).
    pub angle: f64,
    /// Where the dimension line meets its witness lines.
    pub ends_at: (DVec2, DVec2),
    /// Where a note below the line sits (MU80), centred, at `angle`.
    pub below_at: DVec2,
}

/// A linear dimension's text beyond its number, as drawn (MU80): the note
/// below its line, its place, and whether the number is its own text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DimLabel {
    pub below: String,
    pub place: DimPlace,
    /// Moved text's shift from its centred place, drawing units.
    pub moved: Option<DVec2>,
    pub leader: DimLeader,
    /// The number is the user's text: marked on screen, never on paper.
    pub overridden: bool,
    /// Its text has a grip to drag it by (Move text, MU81).
    pub movable: bool,
}

impl DimLabel {
    pub fn of(t: &creabim_core::DimText) -> Self {
        DimLabel {
            below: t.below.trim().to_string(),
            place: t.place,
            moved: t.moved.map(DVec2::from),
            leader: t.leader,
            overridden: t.overridden(),
            movable: t.movable,
        }
    }
}

/// Size of a dimension's note below its line, against its text's.
pub const BELOW_SIZE: f64 = 0.8;

/// A linear dimension laid out with its text `text` placed as `label`
/// says: past either end (the line runs on under it), or moved with a
/// leader back to the line's middle when it sits outside the witness lines
/// (or always, or never). The note below follows the text.
#[allow(clippy::too_many_arguments)]
pub fn dimension_layout(
    a: DVec2,
    b: DVec2,
    offset: f64,
    witness: Option<[DVec2; 2]>,
    look: &TextLook,
    mm: f64,
    text: &str,
    label: &DimLabel,
) -> DimensionGeometry {
    let mut g = dimension_geometry(a, b, offset, witness, look, mm);
    let (pa, pb) = g.ends_at;
    let r = DVec2::from_angle(g.angle);
    let up = r.perp();
    let half = look.width(text) * mm / 2.0;
    let mid = (pa + pb) / 2.0;
    let lift = g.text_at - mid;
    match label.place {
        DimPlace::Centre => {}
        DimPlace::Left | DimPlace::Right => {
            // the line's start and end as the text reads
            let (s, e) = if (pb - pa).dot(r) >= 0.0 {
                (pa, pb)
            } else {
                (pb, pa)
            };
            let gap = 1.5 * mm;
            let (from, dir) = if label.place == DimPlace::Left {
                (s, -r)
            } else {
                (e, r)
            };
            g.text_at = from + dir * (gap + half) + lift;
            g.lines.push((from, from + dir * (gap + 2.0 * half)));
        }
        DimPlace::Moved => {
            g.text_at += label.moved.unwrap_or_default();
            let len = pa.distance(pb).max(1e-12);
            let u = (pb - pa) / len;
            let t = (g.text_at - pa).dot(u) / len;
            let off_line = (g.text_at - mid).dot(u.perp()).abs();
            let outside = !(0.0..=1.0).contains(&t) || off_line > (0.55 + look.size * 1.6) * mm;
            let draw = match label.leader {
                DimLeader::Always => true,
                DimLeader::Never => false,
                DimLeader::WhenOutside => outside,
            };
            if draw {
                // a landing under the text, then back to the line's middle
                let foot = g.text_at - up * (0.55 + look.size / 2.0) * mm;
                let (near, far) = if (mid - foot).dot(r) < 0.0 {
                    (foot - r * half, foot + r * half)
                } else {
                    (foot + r * half, foot - r * half)
                };
                g.lines.push((near, far));
                g.lines.push((mid, near));
                g.ends
                    .extend(end_marks(Terminator::Dot, mid, mid - near, 1.0, mm));
            }
        }
    }
    let down = (0.55 + look.size / 2.0) + (0.55 + look.size * BELOW_SIZE / 2.0);
    g.below_at = g.text_at - up * down * mm;
    g
}

/// A linear dimension's linework: its line `offset` to the left of `a` →
/// `b`, witness lines back to the measured points (`witness` when they are
/// not `a` and `b`, as for a horizontal or vertical dimension), and its ends.
pub fn dimension_geometry(
    a: DVec2,
    b: DVec2,
    offset: f64,
    witness: Option<[DVec2; 2]>,
    look: &TextLook,
    mm: f64,
) -> DimensionGeometry {
    let u = (b - a).normalize_or_zero();
    let n = u.perp();
    let off = n * offset;
    let (pa, pb) = (a + off, b + off);
    let end = look.end.unwrap_or(Terminator::Tick);
    let run = if runs_past(end) { 1.5 * mm } else { 0.0 };
    let mut lines = vec![(pa - u * run, pb + u * run)];
    let [wa, wb] = witness.unwrap_or([a, b]);
    // witness lines from just off the measured points to just past the line
    for (from, to) in [(wa, pa), (wb, pb)] {
        let reach = (to - from).dot(n);
        if reach.abs() > 2.0 * mm {
            let s = n * reach.signum();
            lines.push((from + s * 1.0 * mm, to + s * 1.5 * mm));
        }
    }
    let mut ends = end_marks(end, pa, -u, look.end_size, mm);
    ends.extend(end_marks(end, pb, u, look.end_size, mm));
    let mut angle = u.to_angle();
    let mut up = n;
    if angle > std::f64::consts::FRAC_PI_2 + 1e-6 || angle <= -std::f64::consts::FRAC_PI_2 + 1e-6 {
        angle += std::f64::consts::PI;
        up = -n;
    }
    if angle > std::f64::consts::PI {
        angle -= std::f64::consts::TAU;
    }
    DimensionGeometry {
        lines,
        ends,
        // 1.8 mm off the line for the default 2.5 mm text
        text_at: (pa + pb) / 2.0 + up * (0.55 + look.size / 2.0) * mm,
        angle,
        ends_at: (pa, pb),
        below_at: (pa + pb) / 2.0 - up * (0.55 + look.size * BELOW_SIZE / 2.0) * mm,
    }
}

/// A tag's bubble and leader, in drawing units.
pub struct TagGeometry {
    pub outline: Vec<DVec2>,
    /// The outline is a closed shape, filled white under the text (else an
    /// underline, or nothing for text alone).
    pub closed: bool,
    /// The leader from the bubble's edge (through its elbow) to the host,
    /// as a polyline; empty when the tag sits on its host.
    pub leader: Vec<DVec2>,
    /// The mark at the leader's end: an arrow, a dot, or nothing.
    pub ends: Ends,
    /// The tag drawn as its kind's symbol, in place of the outline.
    pub symbol: Ends,
}

/// Text height of tags and dimensions, paper mm.
pub const TEXT: f64 = 2.5;

/// A tag's outline of `shape` about `at`, its text half `half` wide and high.
fn tag_outline(shape: TagShape, at: DVec2, half: DVec2) -> (Vec<DVec2>, bool) {
    use std::f64::consts::{FRAC_PI_2, PI, TAU};
    let ring = |n: usize, r: DVec2, turn: f64| -> Vec<DVec2> {
        (0..n)
            .map(|i| {
                let t = turn + i as f64 / n as f64 * TAU;
                at + DVec2::new(t.cos() * r.x, t.sin() * r.y)
            })
            .collect()
    };
    // rounded corners: quarter circles of `r` inside a box `h`
    let rounded = |h: DVec2, r: f64| -> Vec<DVec2> {
        let r = r.min(h.x).min(h.y);
        let mut pts = vec![];
        for (k, c) in [
            DVec2::new(h.x - r, h.y - r),
            DVec2::new(-h.x + r, h.y - r),
            DVec2::new(-h.x + r, -h.y + r),
            DVec2::new(h.x - r, -h.y + r),
        ]
        .into_iter()
        .enumerate()
        {
            for i in 0..=6 {
                let t = k as f64 * FRAC_PI_2 + FRAC_PI_2 * i as f64 / 6.0;
                pts.push(at + c + DVec2::from_angle(t) * r);
            }
        }
        pts
    };
    let boxed = |h: DVec2| {
        vec![
            at - h,
            at + DVec2::new(h.x, -h.y),
            at + h,
            at + DVec2::new(-h.x, h.y),
        ]
    };
    match shape {
        TagShape::Auto | TagShape::Box => (boxed(half), true),
        TagShape::Circle => (ring(24, DVec2::splat(half.x.max(half.y)), 0.0), true),
        TagShape::RoundedBox => (rounded(half, half.y * 0.45), true),
        TagShape::Pill => (rounded(half + DVec2::X * half.y * 0.4, half.y), true),
        TagShape::Hexagon => {
            // flat top and bottom, points left and right
            let w = half.x + half.y * 0.6;
            (
                vec![
                    at + DVec2::new(w, 0.0),
                    at + DVec2::new(half.x, half.y),
                    at + DVec2::new(-half.x, half.y),
                    at + DVec2::new(-w, 0.0),
                    at + DVec2::new(-half.x, -half.y),
                    at + DVec2::new(half.x, -half.y),
                ],
                true,
            )
        }
        TagShape::Octagon => {
            let r = half.x.max(half.y) / (PI / 8.0).cos();
            (ring(8, DVec2::splat(r), PI / 8.0), true)
        }
        TagShape::Diamond => {
            let h = half * 1.45;
            (
                vec![
                    at + DVec2::new(h.x, 0.0),
                    at + DVec2::new(0.0, h.y),
                    at + DVec2::new(-h.x, 0.0),
                    at + DVec2::new(0.0, -h.y),
                ],
                true,
            )
        }
        TagShape::Underline => (
            vec![
                at + DVec2::new(-half.x, -half.y * 0.75),
                at + DVec2::new(half.x, -half.y * 0.75),
            ],
            false,
        ),
        TagShape::TextOnly => (vec![], false),
    }
}

/// A tag's outline and leader. `round` is the tag's usual shape (doors
/// and windows) when its style leaves the shape to the tag.
pub fn tag_geometry(
    anchor: DVec2,
    at: DVec2,
    elbow: Option<DVec2>,
    text: &str,
    round: bool,
    look: &TextLook,
    mm: f64,
) -> TagGeometry {
    // the default 2.5 mm text sits in a bubble 4.4 mm tall
    let k = look.size / TEXT;
    let half = DVec2::new((look.width(text) / 2.0 + 1.2 * k).max(2.2 * k), 2.2 * k) * mm;
    let shape = match (look.shape, round) {
        (TagShape::Auto, true) => TagShape::Circle,
        (s, _) => s,
    };
    let (mut outline, mut closed) = tag_outline(shape, at, half);
    // the tag's symbol picked (Mockup_Symbols_REV00): its box is the edge
    // the leader starts from
    let mut symbol = Ends::default();
    let mut symbol_box = None;
    if let Some(s) = look.symbol {
        let sh = crate::symbol::tag_shapes(s, at, look.width(text), look.size, mm);
        symbol_box = sh.bounds();
        symbol = sh.into_ends();
        outline.clear();
        closed = false;
    }
    // a leader back to the host, from the edge of the bubble (or of the
    // text's box when there is no bubble), towards its elbow if it has one
    let first = elbow.unwrap_or(anchor);
    let d = first - at;
    let edge = if let Some((lo, hi)) = symbol_box {
        vec![lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)]
    } else if closed {
        outline.clone()
    } else {
        tag_outline(TagShape::Box, at, half).0
    };
    let mut t = 1.0f64;
    for i in 0..edge.len() {
        let (p, q) = (edge[i], edge[(i + 1) % edge.len()]);
        let e = q - p;
        let den = d.perp_dot(e);
        if den.abs() < 1e-18 {
            continue;
        }
        let s = (p - at).perp_dot(e) / den;
        let v = (p - at).perp_dot(d) / den;
        if s > 0.0 && (-1e-9..=1.0 + 1e-9).contains(&v) {
            t = t.min(s);
        }
    }
    let mut leader = vec![];
    if t < 1.0 && d.length() * (1.0 - t) > 1.0 * mm {
        leader.push(at + d * t);
        leader.extend(elbow);
        leader.push(anchor);
    } else if elbow.is_some() {
        // the elbow is inside the bubble: run straight on from it
        leader = vec![first, anchor];
    }
    leader.dedup_by(|a, b| a.distance(*b) < 1e-9);
    let mut ends = Ends::default();
    if let (Some(s), [.., from, tip]) = (look.leader_symbol, leader.as_slice()) {
        ends = crate::symbol::leader_end(s, *tip, *tip - *from, look.end_size, mm);
    } else if let (Some(end), [.., from, tip]) = (look.leader_end, leader.as_slice()) {
        ends = crate::annotation::leader_marks(end, *tip, *tip - *from, look.end_size, mm);
    }
    if leader.len() < 2 {
        leader.clear();
    }
    TagGeometry {
        outline,
        closed,
        leader,
        ends,
        symbol,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::section::view_drawing;
    use creabim_core::sample::sample_house;

    /// A note's {Size} reads a door's or window's width × height
    /// (GitHub discussion #57: it read "—" on openings).
    #[test]
    fn a_notes_size_reads_an_openings_width_and_height() {
        let m = sample_house();
        let (id, o) = m
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::Opening(o) => Some((e.id, o.clone())),
                _ => None,
            })
            .expect("the sample house has an opening");
        let tag = |v: f64| creabim_core::write_number(&m, creabim_core::NumberKind::Tags, None, v);
        assert_eq!(
            note_field(&m, id, "size"),
            Some(format!("{} × {}", tag(o.width), tag(o.height))),
            "an opening's Size is its width × height"
        );
    }

    /// A room tag put by hand well away from its spot has one leader back,
    /// in its room or out of it, as every tag the label pass nudges has;
    /// put a hair off its spot, none (MU142: dragged out it got a second
    /// leader; dragged inside its room it lost the one it had).
    #[test]
    fn a_room_tag_put_by_hand_has_one_leader() {
        let mut m = creabim_core::ridgeline::ridgeline_house();
        let view = m.iter().find(|e| e.name == "Ground Floor plan").unwrap().id;
        let tags: Vec<creabim_core::Element> = m
            .iter()
            .filter(|e| {
                matches!(&e.kind, ElementKind::Tag(t) if t.view == view
                && matches!(m.get(t.host).map(|h| &h.kind), Some(ElementKind::Room(_))))
            })
            .cloned()
            .collect();
        assert!(tags.len() > 5, "Ridgeline's rooms are tagged");
        let leaders = |m: &Model, id: ElementId| {
            view_drawing(m, view)
                .unwrap()
                .items
                .iter()
                .filter(|i| match i {
                    PlanItem::Line {
                        id: l,
                        pen: crate::plan::Pen::Symbol | crate::plan::Pen::Fine,
                        ..
                    } => *l == id,
                    PlanItem::Annotation { id: a, item } => {
                        *a == id
                            && matches!(
                                item.shape,
                                Shape::Note {
                                    leader: Some(_),
                                    ..
                                }
                            )
                    }
                    _ => false,
                })
                .count()
        };
        // well away (out of most rooms, then inside them), and a hair off
        for (off, want) in [
            (DVec2::new(2.5, 0.3), 1),
            (DVec2::new(-0.6, 2.8), 1),
            (DVec2::new(0.6, 0.0), 1),
            (DVec2::new(0.05, 0.02), 0),
        ] {
            for tag in &tags {
                let mut e = m.get(tag.id).unwrap().clone();
                if let ElementKind::Tag(t) = &mut e.kind {
                    t.offset = off;
                }
                creabim_core::Transaction::new("tag")
                    .replace(m.get(tag.id).unwrap().clone(), e)
                    .apply(&mut m)
                    .unwrap();
            }
            for tag in &tags {
                let n = leaders(&m, tag.id);
                assert_eq!(n, want, "{} put at {off:?}", tag.name);
                // and it is the tag's own leader, in its colour, not a
                // symbol line of another pen (MU142)
                let symbol = view_drawing(&m, view)
                    .unwrap()
                    .items
                    .iter()
                    .filter(|i| matches!(i, PlanItem::Line { id, pen: crate::plan::Pen::Symbol, .. } if *id == tag.id))
                    .count();
                assert_eq!(
                    symbol, 0,
                    "{} put at {off:?}: a symbol-pen leader",
                    tag.name
                );
            }
        }
    }

    /// MU142 (owner): "CH" heights were plain text that took neither the
    /// tag style nor the annotation colour and could not be moved. In a
    /// ceiling plan each ceiling's height is its tag, one line, drawn as a
    /// room tag: no leader in its ceiling, one back once moved out (none
    /// with its leader off), and none at all with Heights hidden.
    #[test]
    fn a_ceiling_height_is_a_tag_drawn_as_a_room_tag() {
        let mut m = creabim_core::ridgeline::ridgeline_house();
        let view = m
            .iter()
            .find(|e| e.name == "Ground Floor ceiling plan")
            .unwrap()
            .id;
        #[allow(clippy::type_complexity)]
        let notes = |m: &Model| -> Vec<(ElementId, String, Option<DVec2>, Option<[f32; 3]>)> {
            view_drawing(m, view)
                .unwrap()
                .items
                .iter()
                .filter_map(|i| match i {
                    PlanItem::Annotation { id, item } => match (&m.get(*id)?.kind, &item.shape) {
                        (ElementKind::Tag(t), Shape::Note { leader, .. })
                            if matches!(
                                m.get(t.host).map(|h| &h.kind),
                                Some(ElementKind::Ceiling(_))
                            ) =>
                        {
                            Some((*id, item.text.clone(), *leader, item.look.rgb))
                        }
                        _ => None,
                    },
                    _ => None,
                })
                .collect()
        };
        // no height left as plain text
        assert!(!view_drawing(&m, view)
            .unwrap()
            .items
            .iter()
            .any(|i| matches!(i, PlanItem::Text { text, .. } if text.starts_with("CH "))));
        let ceilings = m
            .iter()
            .filter(|e| {
                matches!(&e.kind, ElementKind::Ceiling(c)
                if Some(c.level) == m.iter().find(|l| l.name == "Ground Floor").map(|l| l.id))
            })
            .count();
        let tags = notes(&m);
        assert!(ceilings > 0);
        assert_eq!(tags.len(), ceilings, "{tags:?}");
        for (_, text, leader, colour) in &tags {
            assert!(text.starts_with("CH ") && !text.contains('\n'), "{text}");
            assert!(leader.is_none(), "{text}: a leader in its own ceiling");
            // the annotation colour: none of its own
            assert!(colour.is_none());
        }
        // moved well out of its ceiling: a leader back, unless turned off
        let tag = m.get(tags[0].0).unwrap().clone();
        let mut out = tag.clone();
        if let ElementKind::Tag(t) = &mut out.kind {
            t.offset = DVec2::new(0.0, -30.0);
        }
        creabim_core::Transaction::new("out")
            .replace(tag, out.clone())
            .apply(&mut m)
            .unwrap();
        let out_id = out.id;
        let lead = |m: &Model| notes(m).iter().find(|n| n.0 == out_id).and_then(|n| n.2);
        assert!(lead(&m).is_some(), "moved out, it leads back");
        let mut off = out.clone();
        if let ElementKind::Tag(t) = &mut off.kind {
            t.no_leader = true;
        }
        creabim_core::Transaction::new("off")
            .replace(out, off)
            .apply(&mut m)
            .unwrap();
        assert!(lead(&m).is_none(), "its leader turned off");
        // Heights hidden: no tags
        let v = m.get(view).unwrap().clone();
        let mut hidden = v.clone();
        if let ElementKind::View(x) = &mut hidden.kind {
            x.graphics.hide_ceiling_heights = true;
        }
        creabim_core::Transaction::new("hide")
            .replace(v.clone(), hidden.clone())
            .apply(&mut m)
            .unwrap();
        assert!(notes(&m).is_empty());
        // shown again, a tag set to read the material reads the ceiling's
        // (its Properties offer the switch, as on every tag)
        creabim_core::Transaction::new("show")
            .replace(hidden, v)
            .apply(&mut m)
            .unwrap();
        let first = m.get(tags[1].0).unwrap().clone();
        let mut reads = first.clone();
        if let ElementKind::Tag(t) = &mut reads.kind {
            t.material = true;
        }
        let host = match &first.kind {
            ElementKind::Tag(t) => t.host,
            _ => unreachable!(),
        };
        creabim_core::Transaction::new("reads")
            .replace(first, reads)
            .apply(&mut m)
            .unwrap();
        let text = notes(&m).into_iter().find(|n| n.0 == tags[1].0).unwrap().1;
        assert_eq!(text, material_text(&m, host));
    }

    /// MU142 (owner): "make sure even those light fixture tags work like
    /// other tags". A tag on a ceiling light in a ceiling plan is drawn as
    /// any tag: in the annotation colour (none of its own), in its style
    /// once given one, its leader from where its end was put through its
    /// bend.
    #[test]
    fn a_light_fixtures_tag_is_drawn_as_any_tag() {
        use creabim_core::objects::{self, Mount};
        let mut m = creabim_core::ridgeline::ridgeline_house();
        let view = m
            .iter()
            .find(|e| e.name == "Ground Floor ceiling plan")
            .unwrap()
            .id;
        let level = m.iter().find(|e| e.name == "Ground Floor").unwrap().id;
        let light = m
            .iter()
            .find(|e| {
                matches!(&e.kind, ElementKind::Object(o) if o.level == level)
                    && objects::type_of(&m, e).is_some_and(|t| t.mount == Mount::Ceiling)
            })
            .expect("a ceiling light on the ground floor")
            .id;
        let style = m
            .iter()
            .find(|e| matches!(e.kind, ElementKind::TextStyle(_)))
            .unwrap()
            .id;
        let mut t = creabim_core::Tag::new(view, light, DVec2::new(0.6, 0.4));
        t.end = Some(DVec2::new(0.05, 0.0));
        t.elbow = Some(DVec2::new(0.3, 0.4));
        let tag = creabim_core::Element::new("Tag", ElementKind::Tag(t));
        let tid = tag.id;
        creabim_core::Transaction::new("tag")
            .insert(tag.clone())
            .apply(&mut m)
            .unwrap();
        let drawn = |m: &Model| {
            view_drawing(m, view)
                .unwrap()
                .items
                .into_iter()
                .find_map(|i| match i {
                    PlanItem::Tag {
                        id,
                        anchor,
                        elbow,
                        look,
                        ..
                    } if id == tid => Some((anchor, elbow, look)),
                    _ => None,
                })
        };
        let (anchor, elbow, look) = drawn(&m).expect("drawn as a tag in the ceiling plan");
        assert!(look.rgb.is_none(), "the annotation colour");
        let c = centre(&m, light).unwrap().truncate();
        assert!(
            anchor.distance(c + DVec2::new(0.05, 0.0)) < 1e-9,
            "its end: {anchor} vs {c}"
        );
        assert!(elbow.is_some_and(|e| e.distance(c + DVec2::new(0.3, 0.4)) < 1e-9));
        // given a style, in it
        let mut styled = tag.clone();
        if let ElementKind::Tag(t) = &mut styled.kind {
            t.style = Some(style);
        }
        creabim_core::Transaction::new("style")
            .replace(tag, styled)
            .apply(&mut m)
            .unwrap();
        let (_, _, styled_look) = drawn(&m).unwrap();
        assert_eq!(styled_look, look_of(&m, Some(style), None));
    }

    /// A room tag's leader goes where its end was dragged, through its bend
    /// (MU142: "why can't I move this?", the grips moved nothing).
    #[test]
    fn a_room_tags_leader_end_and_bend_are_its_own() {
        let mut m = creabim_core::ridgeline::ridgeline_house();
        let view = m.iter().find(|e| e.name == "Ground Floor plan").unwrap().id;
        let kitchen = m.iter().find(|e| e.name == "Kitchen").unwrap().id;
        let tag = m
            .iter()
            .find(|e| matches!(&e.kind, ElementKind::Tag(t) if t.host == kitchen && t.view == view))
            .unwrap()
            .clone();
        let note = |m: &Model| {
            view_drawing(m, view)
                .unwrap()
                .items
                .iter()
                .find_map(|i| match i {
                    PlanItem::Annotation { id, item } if *id == tag.id => match item.shape {
                        Shape::Note {
                            leader: Some(l),
                            elbow,
                            ..
                        } => Some((l, elbow)),
                        _ => None,
                    },
                    _ => None,
                })
        };
        let mut e = tag.clone();
        if let ElementKind::Tag(t) = &mut e.kind {
            t.offset = DVec2::new(0.0, -4.0);
        }
        creabim_core::Transaction::new("out")
            .replace(tag.clone(), e.clone())
            .apply(&mut m)
            .unwrap();
        let (centre, bend) = note(&m).expect("moved out, it has a leader");
        assert!(bend.is_none());
        let mut e2 = e.clone();
        if let ElementKind::Tag(t) = &mut e2.kind {
            t.end = Some(DVec2::new(0.5, 0.25));
            t.elbow = Some(DVec2::new(1.0, -2.0));
        }
        creabim_core::Transaction::new("leader")
            .replace(e, e2)
            .apply(&mut m)
            .unwrap();
        let (tip, bend) = note(&m).expect("still a leader");
        assert!(
            tip.distance(centre + DVec2::new(0.5, 0.25)) < 1e-9,
            "its end: {tip:?}"
        );
        assert!(
            bend.is_some_and(|b| b.distance(centre + DVec2::new(1.0, -2.0)) < 1e-9),
            "its bend: {bend:?}"
        );
    }

    /// A room's tag moved out of its room draws a leader back to it; with
    /// its leader turned off it draws none (MU141: the owner could not turn
    /// it off).
    #[test]
    fn a_room_tags_leader_can_be_turned_off() {
        let mut m = creabim_core::ridgeline::ridgeline_house();
        let view = m.iter().find(|e| e.name == "Ground Floor plan").unwrap().id;
        let kitchen = m.iter().find(|e| e.name == "Kitchen").unwrap().id;
        let tag = m
            .iter()
            .find(|e| matches!(&e.kind, ElementKind::Tag(t) if t.host == kitchen && t.view == view))
            .unwrap()
            .clone();
        let leader_lines = |m: &Model| {
            view_drawing(m, view)
                .unwrap()
                .items
                .iter()
                .filter(|i| match i {
                    PlanItem::Line {
                        id,
                        pen: crate::plan::Pen::Symbol,
                        ..
                    } => *id == tag.id,
                    PlanItem::Annotation { id, item } => {
                        *id == tag.id
                            && matches!(
                                item.shape,
                                Shape::Note {
                                    leader: Some(_),
                                    ..
                                }
                            )
                    }
                    _ => false,
                })
                .count()
        };
        // moved well out of the kitchen, south into the dining room
        let set = |m: &mut Model, off: bool| {
            let mut e = m.get(tag.id).unwrap().clone();
            if let ElementKind::Tag(t) = &mut e.kind {
                t.offset = DVec2::new(0.0, -4.0);
                t.no_leader = off;
            }
            creabim_core::Transaction::new("tag")
                .replace(m.get(tag.id).unwrap().clone(), e)
                .apply(m)
                .unwrap();
        };
        set(&mut m, false);
        assert_eq!(leader_lines(&m), 1, "moved out, it points back to its room");
        set(&mut m, true);
        assert_eq!(leader_lines(&m), 0, "turned off, no leader");
    }

    fn named(m: &Model, name: &str) -> ElementId {
        m.iter().find(|e| e.name == name).unwrap().id
    }

    #[test]
    fn a_material_tag_reads_the_layers_outside_to_inside() {
        // Annotate step 5: a layered wall reads one line a layer
        let m = creabim_core::starter::starter_house();
        let wall = m
            .iter()
            .find(|e| {
                matches!(e.kind, ElementKind::Wall(_))
                    && creabim_core::assembly_of(&m, e).is_some_and(|(_, a)| a.layers.len() > 1)
            })
            .expect("a layered wall");
        let (_, a) = creabim_core::assembly_of(&m, wall).unwrap();
        let text = material_text(&m, wall.id);
        assert_eq!(text.lines().count(), a.layers.len(), "{text}");
    }

    /// A reading note fills its fields from what its arrow points at; the
    /// arrow moved onto another object reads that one; Follow off keeps
    /// the first; a deleted object's fields read "—" (MU135).
    #[test]
    fn a_reading_note_follows_its_arrow() {
        let m = creabim_core::starter::starter_house();
        let walls: Vec<&creabim_core::Element> = m
            .iter()
            .filter(|e| matches!(e.kind, ElementKind::Wall(_)) && !e.name.is_empty())
            .take(2)
            .collect();
        let (a, b) = (walls[0], walls[1]);
        assert_ne!(a.name, b.name, "two walls of different names");
        let view = m
            .iter()
            .find(|e| matches!(e.kind, ElementKind::View(_)))
            .unwrap()
            .id;
        let mut n = Note::new(view, glam::DVec2::ZERO, "{Name} · {IFC class} · {Level}");
        let on = |id: ElementId| creabim_core::element::HostPoint {
            host: Some(id),
            ..creabim_core::element::HostPoint::free(glam::DVec2::ZERO)
        };
        n.leader = Some(on(a.id));
        // its own text while it does not read
        assert_eq!(note_shown_text(&m, &n).0, n.text);
        n.reads = Some(creabim_core::NoteReads::default());
        let level = note_field(&m, a.id, "level").expect("a wall's level");
        assert_eq!(
            note_shown_text(&m, &n).0,
            format!("{} · IfcWall · {level}", a.name)
        );
        // the arrow moved onto the other wall reads it
        n.leader = Some(on(b.id));
        assert!(note_shown_text(&m, &n).0.starts_with(&b.name));
        // follow off keeps reading the first, wherever the arrow goes
        n.reads = Some(creabim_core::NoteReads {
            follow: false,
            fixed: Some(a.id),
            caps: true,
        });
        assert_eq!(
            note_shown_text(&m, &n).0,
            format!(
                "{} · IFCWALL · {}",
                a.name.to_uppercase(),
                level.to_uppercase()
            )
        );
        // a layered wall's material is its layers', outside to inside
        // (it read — with only the one-material lookup)
        let layered = m
            .iter()
            .find(|e| {
                matches!(e.kind, ElementKind::Wall(_))
                    && creabim_core::material_of(&m, e).is_none()
                    && creabim_core::assembly_of(&m, e).is_some_and(|(_, a)| a.layers.len() > 1)
            })
            .expect("a layered wall");
        let mat = note_field(&m, layered.id, "material").expect("its layers' materials");
        assert!(mat.contains(", "), "{mat}");
        // the object gone: every field reads —
        n.reads = Some(creabim_core::NoteReads {
            follow: false,
            fixed: Some(ElementId::new()),
            caps: false,
        });
        assert_eq!(note_shown_text(&m, &n).0, "— · — · —");
    }

    #[test]
    fn room_tags_show_the_fields_they_list() {
        let m = creabim_core::starter::starter_house();
        let study = named(&m, "Study");
        let text = |f: &[RoomField]| room_tag_text(&m, study, f);
        let area = RoomField::Area.value(&m, study);
        assert_eq!(text(&[]), format!("STUDY\n0.01  {area}"));
        assert_eq!(
            text(&[
                RoomField::Name,
                RoomField::Area,
                RoomField::FloorFinish,
                RoomField::Level
            ]),
            format!("STUDY\n{area}\nOak boards\nGround Floor")
        );
        // a field the room has nothing for is left out
        assert_eq!(text(&[RoomField::Department, RoomField::Number]), "0.01");
        // and the tag's own list is what its drawing writes
        let mut doc = creabim_core::Document::from_model(m.clone());
        let tag = m
            .iter()
            .find(|e| matches!(&e.kind, ElementKind::Tag(t) if t.host == study))
            .unwrap()
            .id;
        doc.edit(tag, "Fields", |e| {
            if let ElementKind::Tag(t) = &mut e.kind {
                t.fields = vec![RoomField::Name, RoomField::CeilingFinish];
            }
        })
        .unwrap();
        let ElementKind::Tag(t) = doc.model().get(tag).unwrap().kind.clone() else {
            unreachable!()
        };
        let ElementKind::View(view) = doc.model().get(t.view).unwrap().kind.clone() else {
            unreachable!()
        };
        let items = annotations(doc.model(), t.view, &view);
        let shown: Vec<String> = items
            .iter()
            .filter_map(|i| match i {
                PlanItem::Annotation { id, item } if *id == tag => Some(item.text.clone()),
                _ => None,
            })
            .collect();
        let ceiling = RoomField::CeilingFinish.value(doc.model(), study);
        let want: Vec<String> = ["STUDY".to_string(), ceiling]
            .into_iter()
            .filter(|l| !l.is_empty())
            .collect();
        assert_eq!(shown, want);
        // with no list, Stacked: the name in semibold, the area, the number small
        doc.edit(tag, "Fields", |e| {
            if let ElementKind::Tag(t) = &mut e.kind {
                t.fields = vec![];
            }
        })
        .unwrap();
        let items = annotations(doc.model(), t.view, &view);
        let lines: Vec<(String, f64, bool)> = items
            .iter()
            .filter_map(|i| match i {
                PlanItem::Annotation { id, item } if *id == tag => {
                    Some((item.text.clone(), item.look.size, item.look.bold))
                }
                _ => None,
            })
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].0, "STUDY");
        assert!(lines[0].2);
        assert_eq!(lines[1].0, area);
        assert_eq!(lines[2].0, "0.01");
        assert!(lines[2].1 < lines[1].1 && lines[1].1 < lines[0].1);
    }

    #[test]
    fn a_room_tag_says_less_at_smaller_scales_and_breaks_a_long_name() {
        use creabim_core::styles::RoomTagSays as S;
        let all = room_fields_at(&[], S::All);
        assert_eq!(
            all,
            vec![RoomField::Name, RoomField::Area, RoomField::Number]
        );
        assert_eq!(
            room_fields_at(&[], S::NameNumber),
            vec![RoomField::Name, RoomField::Number]
        );
        assert_eq!(room_fields_at(&[], S::Number), vec![RoomField::Number]);
        // a tag asking only for its area keeps it, rather than saying nothing
        assert_eq!(
            room_fields_at(&[RoomField::Area], S::Number),
            vec![RoomField::Area]
        );
        assert_eq!(two_lines("PRIMARY BEDROOM"), "PRIMARY\nBEDROOM");
        assert_eq!(two_lines("LANDING"), "LANDING");
    }

    #[test]
    fn a_dimension_rounds_and_ticks_by_the_scale_of_its_view() {
        let mut model = sample_house();
        let plan = named(&model, "Ground Floor plan");
        let ElementKind::View(v) = &model.get(plan).unwrap().kind else {
            panic!()
        };
        let v = v.clone();
        let a = attach(&model, &v, None, DVec2::new(0.0, -3.0));
        let b = attach(&model, &v, None, DVec2::new(1.2343, -3.0));
        let dim = creabim_core::Element::new(
            "Dimension",
            ElementKind::Dimension(creabim_core::Dimension::linear(plan, a, b, 0.5)),
        );
        let id = dim.id;
        creabim_core::Transaction::new("dim")
            .insert(dim)
            .apply(&mut model)
            .unwrap();
        let at = |model: &mut Model, scale: f64| {
            let before = model.get(plan).unwrap().clone();
            let mut e = before.clone();
            if let ElementKind::View(v) = &mut e.kind {
                v.scale = scale;
            }
            creabim_core::Transaction::new("scale")
                .replace(before, e)
                .apply(model)
                .unwrap();
            view_drawing(model, plan)
                .unwrap()
                .items
                .into_iter()
                .find_map(|i| match i {
                    PlanItem::Dimension {
                        id: d, text, look, ..
                    } if d == id => Some((text, look.end_size)),
                    _ => None,
                })
                .unwrap()
        };
        // a millimetre in a detail, ten in a plan; ticks by the row
        let (detail, tick5) = at(&mut model, 5.0);
        let (plan_text, tick100) = at(&mut model, 100.0);
        assert!(detail.starts_with("1234"), "{detail}");
        assert!(plan_text.starts_with("1230"), "{plan_text}");
        assert!(tick5 > tick100, "{tick5} {tick100}");
    }

    #[test]
    fn dimensions_follow_their_walls() {
        let mut doc = creabim_core::Document::from_model(sample_house());
        let plan = named(doc.model(), "Ground Floor plan");
        let overall = |m: &Model| {
            view_drawing(m, plan)
                .unwrap()
                .items
                .into_iter()
                .filter_map(|i| match i {
                    PlanItem::Dimension { text, .. } => text.parse::<f64>().ok(),
                    _ => None,
                })
                .fold(0.0, f64::max)
        };
        assert_eq!(overall(doc.model()), 10300.0);
        // stretch the house: move the east walls out by a metre
        for name in ["GF Wall 1", "GF Wall 2", "GF Wall 3"] {
            let id = named(doc.model(), name);
            doc.edit(id, "Stretch", |e| {
                if let ElementKind::Wall(w) = &mut e.kind {
                    for p in [&mut w.start, &mut w.end] {
                        if p.x > 9.0 {
                            p.x += 1.0;
                        }
                    }
                }
            })
            .unwrap();
        }
        assert_eq!(overall(doc.model()), 11300.0);
    }

    #[test]
    fn dimension_points_follow_the_masses_they_sit_on() {
        use creabim_core::transform::transformed;
        use creabim_core::{EditMesh, Element, FreeMesh, Transaction, Xform};
        let mut model = sample_house();
        let level = model.levels().next().unwrap().0.id;
        let mass = Element::new(
            "Mass",
            ElementKind::FreeMesh(FreeMesh::new(
                Some(level),
                DVec3::new(20.0, 5.0, 0.0),
                EditMesh::cuboid(DVec3::new(2.0, 4.0, 3.0)),
            )),
        );
        let id = mass.id;
        Transaction::new("Add mass")
            .insert(mass)
            .apply(&mut model)
            .unwrap();
        let plan = View::plan(level);
        let section = View::section(DVec2::new(15.0, 0.0), DVec2::new(25.0, 0.0), 20.0, true);
        // a plan corner and a section corner (top of the near face)
        let corner = DVec2::new(21.0, 7.0);
        let top = DVec2::new(6.0, 3.0);
        let in_plan = attach(&model, &plan, Some(id), corner);
        let in_section = attach(&model, &section, Some(id), top);
        assert_eq!(in_plan.host, Some(id));
        assert!(in_plan.body.is_some() && in_section.body.is_some());
        assert!(resolve(&model, &plan, &in_plan).unwrap().distance(corner) < 1e-9);
        assert!(
            resolve(&model, &section, &in_section)
                .unwrap()
                .distance(top)
                < 1e-9
        );
        // move the mass, then turn it a quarter about a point
        let moves = [
            Xform::Move(DVec3::new(1.0, -2.0, 0.5)),
            Xform::Rotate {
                center: DVec2::new(22.0, 5.0),
                degrees: 90.0,
            },
        ];
        let mut e = model.get(id).unwrap().clone();
        for t in &moves {
            e = transformed(&model, &e, t).unwrap();
        }
        let before = model.get(id).unwrap().clone();
        Transaction::new("Move mass")
            .replace(before, e)
            .apply(&mut model)
            .unwrap();
        let expect = moves.iter().fold(corner, |p, t| t.point2(p));
        let now = resolve(&model, &plan, &in_plan).unwrap();
        assert!(now.distance(expect) < 1e-9, "{now} vs {expect}");
        // the section point rose with the mass
        let now = resolve(&model, &section, &in_section).unwrap();
        assert!((now.y - 3.5).abs() < 1e-9, "{now}");
    }

    #[test]
    fn tags_read_marks_and_follow_doors() {
        let mut doc = creabim_core::Document::from_model(sample_house());
        let plan = named(doc.model(), "Ground Floor plan");
        let door = named(doc.model(), "Front Door");
        let tag_at = |m: &Model| {
            view_drawing(m, plan)
                .unwrap()
                .items
                .into_iter()
                .find_map(|i| match i {
                    // its leader's end, which follows the door (the text
                    // may be nudged clear of other lines)
                    PlanItem::Tag { anchor, text, .. } if text == "D01" => Some(anchor),
                    _ => None,
                })
                .expect("front door is tagged D01")
        };
        let before = tag_at(doc.model());
        doc.edit(door, "Move door", |e| {
            if let ElementKind::Opening(o) = &mut e.kind {
                o.offset += 2.0;
            }
        })
        .unwrap();
        let after = tag_at(doc.model());
        assert!((after.x - before.x - 2.0).abs() < 1e-9);
    }

    #[test]
    fn dimension_text_goes_aside_and_moves_with_a_leader() {
        let look = TextLook::default();
        let (a, b, mm) = (DVec2::ZERO, DVec2::X * 1.0, 0.01);
        let text = "0'-2 3/4\"";
        let centred = dimension_geometry(a, b, 0.5, None, &look, mm);
        let plain = dimension_layout(a, b, 0.5, None, &look, mm, text, &DimLabel::default());
        assert_eq!(plain.text_at, centred.text_at);
        assert_eq!(plain.lines.len(), centred.lines.len());
        // Right: past the line's end, the line running on under it
        let right = DimLabel {
            place: DimPlace::Right,
            ..Default::default()
        };
        let g = dimension_layout(a, b, 0.5, None, &look, mm, text, &right);
        assert!(g.text_at.x > 1.0, "{:?}", g.text_at);
        assert_eq!(g.lines.len(), centred.lines.len() + 1);
        // moved well clear: a landing and a leader back to the middle
        let moved = DimLabel {
            place: DimPlace::Moved,
            moved: Some(DVec2::new(0.9, 0.3)),
            ..Default::default()
        };
        let g = dimension_layout(a, b, 0.5, None, &look, mm, text, &moved);
        assert_eq!(g.lines.len(), centred.lines.len() + 2);
        let never = DimLabel {
            leader: DimLeader::Never,
            ..moved.clone()
        };
        let g = dimension_layout(a, b, 0.5, None, &look, mm, text, &never);
        assert_eq!(g.lines.len(), centred.lines.len());
        // moved only a little, still between the witness lines: no leader
        let near = DimLabel {
            moved: Some(DVec2::new(0.1, 0.0)),
            ..moved
        };
        let g = dimension_layout(a, b, 0.5, None, &look, mm, text, &near);
        assert_eq!(g.lines.len(), centred.lines.len());
        // the note below sits under the line
        assert!(centred.below_at.y < 0.5 && centred.text_at.y > 0.5);
    }

    #[test]
    fn dimension_text_reads_upright() {
        let g = dimension_geometry(
            DVec2::new(0.0, 5.0),
            DVec2::ZERO,
            1.0,
            None,
            &TextLook::default(),
            0.1,
        );
        assert!(g.angle.abs() <= std::f64::consts::FRAC_PI_2 + 1e-9);
    }

    #[test]
    fn a_horizontal_dimension_measures_square_across() {
        use creabim_core::{DimAxis, Dimension, Element, Transaction};
        let mut doc = creabim_core::Document::from_model(sample_house());
        let plan = named(doc.model(), "Ground Floor plan");
        let (a, b) = (DVec2::new(1.0, 1.0), DVec2::new(1.47, 0.4));
        let mut d = Dimension::linear(plan, HostPoint::free(a), HostPoint::free(b), 0.5);
        d.axis = DimAxis::Horizontal;
        let e = Element::new("Dimension", ElementKind::Dimension(d));
        let id = e.id;
        doc.commit(Transaction::new("Add").insert(e)).unwrap();
        let item = view_drawing(doc.model(), plan)
            .unwrap()
            .items
            .into_iter()
            .find(|i| i.id() == id)
            .unwrap();
        let PlanItem::Dimension {
            a: pa,
            b: pb,
            text,
            witness,
            offset,
            look,
            ..
        } = item
        else {
            panic!("a linear dimension")
        };
        // 470 across, not the 760 along the diagonal
        assert_eq!(text, "470");
        assert!((pb - DVec2::new(1.47, 1.0)).length() < 1e-9 && pa == a);
        assert_eq!(witness, Some([a, b]));
        // the line is level, and the witness from the lower point reaches it
        let g = dimension_geometry(pa, pb, offset, witness, &look, 0.01);
        let (p, q) = g.lines[0];
        assert!((p.y - q.y).abs() < 1e-9);
        assert!(g.lines.iter().any(|(s, t)| (s.x - 1.47).abs() < 1e-9
            && (t.x - 1.47).abs() < 1e-9
            && s.y < 0.45
            && t.y > 1.5));
        // points level with each other measure nothing vertically
        assert!(squared(DimAxis::Vertical, a, DVec2::new(3.0, 1.0)).is_none());
    }

    #[test]
    fn styles_change_dimension_ends_and_tag_shapes() {
        let mut look = TextLook::default();
        let tick = dimension_geometry(DVec2::ZERO, DVec2::X * 3.0, 1.0, None, &look, 0.01);
        assert_eq!(tick.ends.heavy.len(), 2, "two ticks by default");
        look.end = Some(Terminator::Dot);
        let dots = dimension_geometry(DVec2::ZERO, DVec2::X * 3.0, 1.0, None, &look, 0.01);
        assert!(dots.ends.heavy.is_empty() && dots.ends.fills.len() == 2);
        // a tick.s line runs on past it; a dot.s or an arrow.s stops on its point
        assert!(tick.lines[0].0.x < 0.0);
        assert_eq!(dots.lines[0].0.x, 0.0);
        look.end = Some(Terminator::ArrowOpen);
        let open = dimension_geometry(DVec2::ZERO, DVec2::X * 3.0, 1.0, None, &look, 0.01);
        assert_eq!(open.lines[0].0.x, 0.0);
        assert_eq!(open.ends.thin.len(), 2);
        // every end draws something but None
        for t in Terminator::ALL {
            let e = end_marks(t, DVec2::ZERO, DVec2::X, 2.5, 0.01);
            let n = e.thin.len() + e.heavy.len() + e.fills.len();
            assert_eq!(n == 0, t == Terminator::None, "{t:?}");
        }
        // tags: shapes, and the leader leaving each from its edge
        let anchor = DVec2::new(0.0, -1.0);
        for shape in TagShape::ALL {
            look.shape = shape;
            let g = tag_geometry(anchor, DVec2::ZERO, None, "D01", false, &look, 0.01);
            assert_eq!(
                g.closed,
                !matches!(shape, TagShape::Underline | TagShape::TextOnly)
            );
            let [from, to] = g.leader[..] else {
                panic!("a leader to the far anchor")
            };
            assert_eq!(to, anchor);
            assert!(from.y < 0.0 && from.y > -0.05, "{shape:?} {from}");
        }
        look.shape = TagShape::Auto;
        let round = tag_geometry(anchor, DVec2::ZERO, None, "D01", true, &look, 0.01);
        assert_eq!(round.outline.len(), 24, "openings stay round");
    }
}
