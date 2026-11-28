//! Furniture and fixtures in the app: the Catalogue window (the starter
//! objects and the user's own by category, with pictures; click one to
//! place it; import one from Blender, FreeCAD or glTF; save the selection
//! as one; the project's own object types), the user's catalogue kept as
//! one JSON file per object in `<settings>/CreaBIM/catalogue`, and the
//! Properties of objects and of their types.

use super::*;
use creabim_core::objects::{
    self, CatalogueObject, IfcKind, Mount, ObjectCategory, ObjectType, IFC_CLASSES,
};
use std::path::{Path, PathBuf};

/// Side of a catalogue picture, in points.
const THUMB: usize = 76;

/// The Catalogue window's state, for the session.
#[derive(Default)]
pub(super) struct CatalogueUi {
    pub open: bool,
    /// The category shown; none shows them all.
    category: Option<ObjectCategory>,
    filter: String,
    /// The user's own objects, read from their folder when first needed.
    user: Option<Vec<CatalogueObject>>,
    /// Pictures by catalogue key (and, for the project's types, revision).
    thumbs: HashMap<String, egui::TextureHandle>,
    /// The name the selection is saved under.
    save_name: String,
}

/// What a click in the window asks for.
enum CatAsk {
    Place(Box<CatalogueObject>, Option<ElementId>),
    Import,
    SaveSelection,
    Delete(String),
    Reveal,
}

/// The folder of the user's own catalogue.
pub(super) fn user_dir() -> Option<PathBuf> {
    crate::files::settings_dir().map(|d| d.join("catalogue"))
}

/// Every catalogue object in `dir`, by name.
pub(super) fn load_dir(dir: &Path) -> Vec<CatalogueObject> {
    let mut out: Vec<CatalogueObject> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path()).ok()?;
            let mut c = CatalogueObject::from_json(&text).ok()?;
            // the file is the object: its name is its key
            let stem = e.path().file_stem()?.to_str()?.to_string();
            c.key = format!("user/{stem}");
            c.object.catalogue = Some(c.key.clone());
            Some(c)
        })
        .collect();
    out.sort_by_key(|c| c.name.to_lowercase());
    out
}

/// A file name for an object called `name` in `dir`, not yet taken.
pub(super) fn free_key(dir: &Path, name: &str) -> String {
    let mut slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "object" } else { slug };
    let mut n = 1;
    loop {
        let stem = if n == 1 {
            slug.to_string()
        } else {
            format!("{slug}-{n}")
        };
        if !dir.join(format!("{stem}.json")).exists() {
            return format!("user/{stem}");
        }
        n += 1;
    }
}

/// Write `c` into `dir` under its key.
pub(super) fn save_to(dir: &Path, c: &CatalogueObject) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stem = c.key.strip_prefix("user/").unwrap_or(&c.key);
    let path = dir.join(format!("{stem}.json"));
    std::fs::write(&path, c.to_json())?;
    Ok(path)
}

/// Remove the object keyed `key` from `dir`.
pub(super) fn delete_from(dir: &Path, key: &str) -> std::io::Result<()> {
    let stem = key.strip_prefix("user/").unwrap_or(key);
    std::fs::remove_file(dir.join(format!("{stem}.json")))
}

/// A picture of `c`: its parts in their colours, seen from the front,
/// a little to the right and above, drawn in software.
pub(super) fn thumbnail(c: &CatalogueObject, size: usize) -> egui::ColorImage {
    let ss = 2;
    let n = size * ss;
    let d = DVec3::new(-0.55, 1.0, -0.75).normalize();
    let r = d.cross(DVec3::Z).normalize();
    let u = r.cross(d);
    let light = DVec3::new(0.35, -0.65, 0.85).normalize();
    let tris: Vec<([DVec3; 3], [f32; 3])> = c
        .object
        .parts
        .iter()
        .enumerate()
        .flat_map(|(i, p)| {
            let color = c
                .materials
                .get(i)
                .and_then(|m| m.as_ref())
                .map_or([0.8, 0.78, 0.74], |m| m.1.color);
            let v = &p.mesh.vertices;
            p.mesh.faces.iter().flat_map(move |f| {
                (1..f.len().saturating_sub(1)).filter_map(move |k| {
                    let t = [
                        *v.get(f[0] as usize)?,
                        *v.get(f[k] as usize)?,
                        *v.get(f[k + 1] as usize)?,
                    ];
                    Some((t, color))
                })
            })
        })
        .collect();
    let mut img = vec![[0u8; 4]; n * n];
    if tris.is_empty() {
        return egui::ColorImage::new([size, size], vec![egui::Color32::TRANSPARENT; size * size]);
    }
    let proj = |p: DVec3| DVec3::new(p.dot(r), p.dot(u), p.dot(d));
    let (mut lo, mut hi) = (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN));
    for (t, _) in &tris {
        for p in t {
            let q = proj(*p);
            lo = lo.min(q);
            hi = hi.max(q);
        }
    }
    let span = (hi.x - lo.x).max(hi.y - lo.y).max(1e-6);
    let scale = (n as f64 * 0.9) / span;
    let mid = (lo + hi) / 2.0;
    let to_px = |q: DVec3| {
        DVec3::new(
            (q.x - mid.x) * scale + n as f64 / 2.0,
            n as f64 / 2.0 - (q.y - mid.y) * scale,
            q.z,
        )
    };
    let mut depth = vec![f64::MAX; n * n];
    for (t, color) in &tris {
        let mut normal = (t[1] - t[0]).cross(t[2] - t[0]);
        if normal.length_squared() < 1e-18 {
            continue;
        }
        normal = normal.normalize();
        if normal.dot(d) > 0.0 {
            normal = -normal;
        }
        let shade = 0.5 + 0.5 * normal.dot(light).max(0.0) as f32;
        let rgb = color.map(|c| ((c * shade).clamp(0.0, 1.0) * 255.0) as u8);
        let s = t.map(|p| to_px(proj(p)));
        let x0 = s
            .iter()
            .map(|p| p.x)
            .fold(f64::MAX, f64::min)
            .floor()
            .max(0.0) as usize;
        let x1 = (s.iter().map(|p| p.x).fold(f64::MIN, f64::max).ceil() as usize).min(n - 1);
        let y0 = s
            .iter()
            .map(|p| p.y)
            .fold(f64::MAX, f64::min)
            .floor()
            .max(0.0) as usize;
        let y1 = (s.iter().map(|p| p.y).fold(f64::MIN, f64::max).ceil() as usize).min(n - 1);
        let area = (s[1].x - s[0].x) * (s[2].y - s[0].y) - (s[2].x - s[0].x) * (s[1].y - s[0].y);
        if area.abs() < 1e-12 || x0 > x1 || y0 > y1 {
            continue;
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let w0 = ((s[1].x - px) * (s[2].y - py) - (s[2].x - px) * (s[1].y - py)) / area;
                let w1 = ((s[2].x - px) * (s[0].y - py) - (s[0].x - px) * (s[2].y - py)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * s[0].z + w1 * s[1].z + w2 * s[2].z;
                let i = y * n + x;
                if z < depth[i] {
                    depth[i] = z;
                    img[i] = [rgb[0], rgb[1], rgb[2], 255];
                }
            }
        }
    }
    // down to size, averaging each block
    let mut px = Vec::with_capacity(size * size);
    for y in 0..size {
        for x in 0..size {
            let mut acc = [0u32; 4];
            for dy in 0..ss {
                for dx in 0..ss {
                    let p = img[(y * ss + dy) * n + x * ss + dx];
                    let a = p[3] as u32;
                    for k in 0..3 {
                        acc[k] += p[k] as u32 * a / 255;
                    }
                    acc[3] += a;
                }
            }
            let k = (ss * ss) as u32;
            px.push(egui::Color32::from_rgba_premultiplied(
                (acc[0] / k) as u8,
                (acc[1] / k) as u8,
                (acc[2] / k) as u8,
                (acc[3] / k) as u8,
            ));
        }
    }
    egui::ColorImage::new([size, size], px)
}

/// The selection as a catalogue object called `name`: an object's type,
/// free meshes as they are, or any other elements as their 3D shapes.
pub(super) fn from_selection(
    model: &Model,
    ids: &[ElementId],
    name: &str,
    key: &str,
) -> Option<CatalogueObject> {
    if let [id] = ids {
        if let Some(e) = model.get(*id) {
            if let ElementKind::Object(_) = &e.kind {
                let (t, _) = creabim_core::types::type_of(model, e)?;
                let mut c = objects::from_type(model, t.id, key)?;
                c.name = name.to_string();
                return Some(c);
            }
            if let ElementKind::ElementType(_) = &e.kind {
                let mut c = objects::from_type(model, *id, key)?;
                c.name = name.to_string();
                return Some(c);
            }
        }
    }
    let meshes: Vec<ElementId> = ids
        .iter()
        .copied()
        .filter(|id| {
            model
                .get(*id)
                .is_some_and(|e| matches!(e.kind, ElementKind::FreeMesh(_)))
        })
        .collect();
    if meshes.len() == ids.len() {
        return objects::from_meshes(model, &meshes, name, key);
    }
    // anything else: its shape in 3D, each piece in its colour
    let items: Vec<_> = creabim_geom::scene::build_scene(model)
        .into_iter()
        .filter(|i| ids.contains(&i.id) && !i.ghost)
        .collect();
    let mut parts = vec![];
    let mut materials = vec![];
    for item in items {
        let mesh = creabim_core::EditMesh {
            vertices: item
                .mesh
                .positions
                .iter()
                .map(|p| DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64))
                .collect(),
            faces: item.mesh.indices.chunks(3).map(|t| t.to_vec()).collect(),
            edges: vec![],
        };
        let [r, g, b] = item.color.map(|c| (c * 255.0) as u8);
        let look = item
            .finish
            .map(str::to_string)
            .unwrap_or_else(|| format!("Colour #{r:02x}{g:02x}{b:02x}"));
        let mut m = creabim_core::Material::new(creabim_core::Hatch::Empty, item.color);
        m.roughness = item.surface[0];
        m.metallic = item.surface[1];
        m.opacity = item.surface[2];
        parts.push(objects::ObjectPart {
            name: model.get(item.id).map_or(String::new(), |e| e.name.clone()),
            mesh,
            material: None,
        });
        materials.push(Some((look, m)));
    }
    if parts.is_empty() {
        return None;
    }
    objects::recentre(&mut parts);
    let (category, ifc, mount) = creabim_exchange::catalogue::guess(name);
    Some(CatalogueObject {
        key: key.to_string(),
        name: name.to_string(),
        description: String::new(),
        object: ObjectType {
            category,
            ifc,
            mount,
            offset: 0.0,
            parts,
            symbol: vec![],
            catalogue: Some(key.to_string()),
            light: None,
        },
        materials,
    })
}

fn mount_label(m: Mount) -> &'static str {
    match m {
        Mount::Wall => "Against a wall",
        Mount::Floor => "Free-standing",
        Mount::Ceiling => "From the ceiling",
    }
}

/// A part's material: one of the project's, or none.
fn part_material(
    ui: &mut egui::Ui,
    model: &Model,
    salt: usize,
    current: &mut Option<ElementId>,
) -> bool {
    let shown = current
        .and_then(|c| model.get(c))
        .map_or("None".to_string(), |e| e.name.clone());
    let mut changed = false;
    egui::ComboBox::from_id_salt(("part_material", salt))
        .icon(crate::props::combo_icon)
        .selected_text(shown)
        .width(ui.available_width().min(170.0))
        .truncate()
        .show_ui(ui, |ui| {
            changed |= ui.selectable_value(current, None, "None").changed();
            for e in model
                .iter()
                .filter(|e| matches!(e.kind, ElementKind::Material(_)))
            {
                changed |= ui.selectable_value(current, Some(e.id), &e.name).changed();
            }
        });
    changed
}

/// An object type's fields. Returns the undo label of what changed, or "".
pub(super) fn type_fields(ui: &mut egui::Ui, model: &Model, o: &mut ObjectType) -> &'static str {
    let mut label = "";
    let s = o.size();
    crate::props::worked_out(
        ui,
        "object-type",
        &[
            crate::props::Worked::new("Width", crate::units::len(s.x)),
            crate::props::Worked::new("Depth", crate::units::len(s.y)),
            crate::props::Worked::new("Height", crate::units::len(s.z)),
        ],
    );
    // one setting a row, as the rest of Properties (MU23)
    crate::props::row(ui, "Category", |ui| {
        egui::ComboBox::from_id_salt("object_category")
            .icon(crate::props::combo_icon)
            // as wide as the row and no wider: at its text's width it ran
            // past the panel's edge (MU119)
            .width(ui.available_width())
            .truncate()
            .selected_text(o.category.label())
            .show_ui(ui, |ui| {
                for c in ObjectCategory::ALL {
                    if ui.selectable_value(&mut o.category, c, c.label()).changed() {
                        label = "Object category";
                    }
                }
            });
    });
    crate::props::row(ui, "Placed", |ui| {
        egui::ComboBox::from_id_salt("object_mount")
            .icon(crate::props::combo_icon)
            // as wide as the row and no wider: at its text's width it ran
            // past the panel's edge (MU119)
            .width(ui.available_width())
            .truncate()
            .selected_text(mount_label(o.mount))
            .show_ui(ui, |ui| {
                for m in Mount::ALL {
                    if ui
                        .selectable_value(&mut o.mount, m, mount_label(m))
                        .changed()
                    {
                        label = "Object placement";
                    }
                }
            })
            .response
            .on_hover_text(
                "Against a wall: the Object tool backs it onto the nearest wall face. \
                 From the ceiling: it hangs below the ceiling over it, centred where clicked",
            );
    });
    // a label short enough to read whole; which way it measures is said
    // in the hover (MU119: "New ones below the ceiling" was cut to "New on…")
    let new_at = if o.mount == Mount::Ceiling {
        "New ones below the ceiling: how far under the ceiling the Object tool hangs new ones of this type, to their top"
    } else {
        "New ones above the floor: how high the Object tool places new ones of this type, to their underside"
    };
    crate::props::row(ui, "New ones", |ui| {
        if crate::props::fill(ui, crate::units::drag(&mut o.offset).range(-10.0..=100.0))
            .on_hover_text(new_at)
            .changed()
        {
            label = "Object mounting height";
        }
    });
    crate::props::row(ui, "IFC class", |ui| {
        let mut class = o.ifc.class.clone();
        egui::ComboBox::from_id_salt("object_class")
            .icon(crate::props::combo_icon)
            // as wide as the row and no wider: at its text's width it ran
            // past the panel's edge (MU119)
            .width(ui.available_width())
            .truncate()
            .selected_text(&class)
            .show_ui(ui, |ui| {
                for (c, _, _) in IFC_CLASSES {
                    ui.selectable_value(&mut class, c.to_string(), *c);
                }
            });
        if class != o.ifc.class {
            o.ifc = IfcKind::new(&class, "NOTDEFINED").normalized();
            label = "Object IFC class";
        }
    });
    crate::props::row(ui, "IFC type", |ui| {
        let mut pre = o.ifc.predefined.clone();
        egui::ComboBox::from_id_salt("object_predefined")
            .icon(crate::props::combo_icon)
            // as wide as the row and no wider: at its text's width it ran
            // past the panel's edge (MU119)
            .width(ui.available_width())
            .truncate()
            .selected_text(&pre)
            .show_ui(ui, |ui| {
                for p in objects::predefined_types(&o.ifc.class) {
                    ui.selectable_value(&mut pre, p.to_string(), *p);
                }
            });
        if pre != o.ifc.predefined {
            o.ifc.predefined = pre;
            label = "Object IFC type";
        }
    });
    if let Some(k) = &o.catalogue {
        crate::props::row(ui, "Catalogue", |ui| {
            ui.add(egui::Label::new(k.as_str()).truncate())
                .on_hover_text(format!("It came from the catalogue: {k}"))
        });
    }
    if !o.parts.is_empty() {
        crate::props::sub_box(
            ui,
            "object.parts",
            "Parts",
            Some(o.parts.len()),
            true,
            |ui| {
                for (i, p) in o.parts.iter_mut().enumerate() {
                    let name = if p.name.is_empty() {
                        format!("Part {}", i + 1)
                    } else {
                        p.name.clone()
                    };
                    crate::props::row(ui, &name, |ui| {
                        ui.spacing_mut().combo_width = ui.available_width();
                        if part_material(ui, model, i, &mut p.material) {
                            label = "Part material";
                        }
                    });
                }
            },
        );
    }
    label
}

/// The Light block of an object type (Visuals REV02): shown for a light
/// fitting, and for any type given a light. Returns the undo label, or "".
pub(super) fn light_fields(ui: &mut egui::Ui, o: &mut objects::ObjectType) -> &'static str {
    let fitting = o.is_light_fitting();
    let mut label = "";
    let Some(mut l) = o.light_given() else {
        // not a fitting: it can be made to give light (a screen, a sign)
        let mut on = false;
        if crate::props::check(ui, "On", &mut on, "Gives light").changed() && on {
            o.light = Some(objects::Light::typical(o.mount, false));
            label = "Object light";
        }
        return label;
    };
    let was = l.clone();
    crate::props::check(ui, "On", &mut l.on, "Gives light")
        .on_hover_text("Unticked, it is drawn and gives no light in a render");
    ui.add_enabled_ui(l.on, |ui| {
        crate::props::row(ui, "Output", |ui| {
            crate::props::fill(
                ui,
                egui::DragValue::new(&mut l.lumens).range(10.0..=50_000.0).speed(10.0).suffix(" lm"),
            )
            .on_hover_text("How much light it gives, in lumens, as a lamp's box says: about 800 for a 60 W bulb of old, 1500 for a bright ceiling light")
        });
        crate::props::row(ui, "Colour", |ui| {
            crate::props::fill(
                ui,
                egui::DragValue::new(&mut l.kelvin).range(1800.0..=8000.0).speed(25.0).suffix(" K"),
            )
            .on_hover_text("Its light's colour, in kelvin: 2700 warm white, 4000 neutral, 6500 daylight")
        });
        crate::props::row(ui, "Beam", |ui| {
            egui::ComboBox::from_id_salt("object.light.beam")
                .icon(crate::props::combo_icon)
                .selected_text(l.beam.label())
                .width(ui.available_width())
                .truncate()
                .show_ui(ui, |ui| {
                    for (b, tip) in [
                        (objects::Spread::Narrow, "A spot or a downlight: a tight cone"),
                        (objects::Spread::Medium, "A wider cone, as over a worktop"),
                        (objects::Spread::Wide, "A pendant or a ceiling light: across the room, with a glow round it"),
                        (objects::Spread::AllRound, "A bare bulb or a globe: the same every way"),
                    ] {
                        ui.selectable_value(&mut l.beam, b, b.label()).on_hover_text(tip);
                    }
                });
        });
        crate::props::row(ui, "Group", |ui| {
            crate::props::text_field(
                ui,
                egui::TextEdit::singleline(&mut l.group).hint_text("its type's name"),
                f32::INFINITY,
            )
            .on_hover_text("The group it is dimmed with in Visuals, Light: \"Kitchen downlights\". Give several types the same group to dim them together. Empty: its type's name")
        });
    });
    if l != was {
        // a fitting set back to exactly what its kind gives keeps none of
        // its own, so its file reads as before
        let usual = objects::Light::typical(o.mount, o.ifc.predefined == "DIRECTIONSOURCE");
        o.light = (!(fitting && l == usual)).then_some(l);
        label = "Object light";
    }
    label
}

/// Turn `o` by `by` degrees (anticlockwise) about its footprint's middle
/// `mid` (in its own axes), so it turns in place (MU142: the quarter turn
/// was about its back corner, and the object moved as it turned).
pub(crate) fn turn_about_centre(o: &mut objects::Object, mid: glam::DVec2, by: f64) {
    let c = o.to_plan(mid);
    o.rotation = (o.rotation + by).rem_euclid(360.0);
    let now = o.to_plan(mid);
    o.position += c - now;
}

/// A placed object's own fields; `hung` when its type hangs from the
/// ceiling; `mid` its footprint's middle in its own axes, what it turns
/// about. Returns the undo label, or "".
pub(super) fn object_fields(
    ui: &mut egui::Ui,
    o: &mut objects::Object,
    hung: bool,
    mid: glam::DVec2,
) -> &'static str {
    let mut label = "";
    // short labels, the whole meaning in the hover (MU119: "Below the
    // ceiling" was cut to "Below t…")
    let (height, tip) = if hung {
        (
            "Below",
            "Below the ceiling: how far its top hangs under the ceiling over it",
        )
    } else {
        (
            "Height",
            "Above its level: how high its underside is over its level's floor",
        )
    };
    crate::props::row(ui, height, |ui| {
        if crate::props::fill(ui, crate::units::drag(&mut o.offset).range(-10.0..=100.0))
            .on_hover_text(tip)
            .changed()
        {
            label = "Object height";
        }
    });
    // the angle in its field, whole; every turn about its middle, so it
    // turns in place (MU142)
    let was = o.rotation;
    let mut angle = o.rotation;
    crate::props::row(ui, "Rotation", |ui| {
        if crate::props::fill(
            ui,
            egui::DragValue::new(&mut angle)
                .speed(1.0)
                .suffix("°")
                .range(-360.0..=720.0)
                // typed, it turns the object once the number is whole
                // (Enter or leaving the field): turned at every key, the
                // panel was drawn again under the cursor and typing 140
                // took 1, then 14 (MU141)
                .update_while_editing(false),
        )
        .changed()
        {
            label = "Turn object";
        }
    });
    if label == "Turn object" {
        turn_about_centre(o, mid, angle - was);
    }
    // the turns, a row of their own: three buttons and the angle did not
    // fit the panel at its narrowest (MU142 mock-up, part 2)
    let mut by = None;
    crate::props::row(ui, "Turn", |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            let h = ui.spacing().interact_size.y;
            let w = ((ui.available_width() - 2.0) / 3.0).floor().max(26.0);
            // drawn by hand: egui's buttons cut their text to "…" in a
            // narrow panel (MU142: "…", "1…", "…")
            // plain angles, no arrows (MU142: they "don't belong"); + is
            // anticlockwise, as Rotation counts
            let none: Option<&str> = None;
            for (icon, text, deg, tip) in [
                (
                    none,
                    "−90°",
                    -90.0,
                    "A quarter turn clockwise, about its middle",
                ),
                (none, "180°", 180.0, "A half turn, about its middle"),
                (
                    none,
                    "+90°",
                    90.0,
                    "A quarter turn anticlockwise, about its middle",
                ),
            ] {
                let (r, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
                let v = ui.style().interact(&resp);
                ui.painter().rect(
                    r,
                    v.corner_radius,
                    v.weak_bg_fill,
                    v.bg_stroke,
                    egui::StrokeKind::Inside,
                );
                let font = egui::FontId::proportional(12.0);
                let galley = ui
                    .painter()
                    .layout_no_wrap(text.into(), font, v.text_color());
                let icon_w = if icon.is_some() { 13.0 } else { 0.0 };
                let x = r.center().x - (icon_w + galley.size().x) / 2.0;
                if let Some(name) = icon {
                    let ir = egui::Rect::from_min_size(
                        egui::pos2(x, r.center().y - 6.5),
                        egui::vec2(13.0, 13.0),
                    );
                    crate::svgicon::paint(ui.painter(), ir, name, v.text_color());
                }
                let at = egui::pos2(x + icon_w, r.center().y - galley.size().y / 2.0);
                ui.painter().galley(at, galley, v.text_color());
                if resp.on_hover_text(tip).clicked() {
                    by = Some(deg);
                }
            }
        });
    });
    if let Some(deg) = by {
        turn_about_centre(o, mid, deg);
        label = "Turn object";
    }
    label
}

impl CreaBimApp {
    /// The user's own catalogue, read when first wanted.
    fn user_catalogue(&mut self) -> &[CatalogueObject] {
        if self.catalogue.user.is_none() {
            self.catalogue.user = Some(user_dir().map(|d| load_dir(&d)).unwrap_or_default());
        }
        self.catalogue.user.as_deref().unwrap_or(&[])
    }

    /// Run a catalogue action. False when the action isn't one.
    pub(super) fn objects_action(&mut self, head: &str, _arg: &str) -> bool {
        match head {
            "catalogue" => self.catalogue.open = true,
            "catalogue.entourage" => {
                self.catalogue.open = true;
                self.catalogue.category = Some(ObjectCategory::Entourage);
            }
            "catalogue.import" => self.run_catalogue(CatAsk::Import),
            "catalogue.save" => self.run_catalogue(CatAsk::SaveSelection),
            _ => return false,
        }
        true
    }

    /// Properties of an object's type, under the object's own.
    pub(super) fn object_type_section(&mut self, ui: &mut egui::Ui, id: ElementId) {
        let model = self.doc.model();
        let Some((t, _)) = model
            .get(id)
            .and_then(|e| creabim_core::types::type_of(model, e))
        else {
            return;
        };
        let ElementKind::ElementType(ty) = &t.kind else {
            return;
        };
        let (tid, tname) = (t.id, t.name.clone());
        let mut ty = ty.clone();
        let creabim_core::TypeParams::Object(o) = &mut ty.params else {
            return;
        };
        // a block of its own, as every other part of Properties is: loose
        // under the last block its rows ran wider than the panel and lined
        // up with nothing (owner, MU119)
        let open = self.prop_header(ui, "object-its-type", "Its type", true, None);
        let model = self.doc.model();
        let mut label = "";
        let mut save = false;
        crate::props::fold(ui, open, |ui| {
            ui.vertical(|ui| {
                crate::props::row(ui, "Type", |ui| {
                    ui.add(egui::Label::new(tname.as_str()).truncate())
                        .on_hover_text(format!("{tname}: what every object of this type shares. A change here changes them all"))
                });
                label = type_fields(ui, model, o);
                // what it gives as light, on the fitting itself
                let lit = crate::props::sub_box(ui, "object.light", "Light", None, true, |ui| light_fields(ui, o))
                    .unwrap_or("");
                if !lit.is_empty() {
                    label = lit;
                }
                save = crate::props::action(
                    ui,
                    "",
                    "To catalogue",
                    "Keep this object in your own catalogue, for every project",
                    true,
                )
                .clicked();
            });
        });
        if save {
            self.catalogue.save_name = tname.clone();
            self.run_catalogue(CatAsk::SaveSelection);
        }
        if !label.is_empty() {
            self.run_types(typesui::TypeAsk::Edit(tid, label, ty));
        }
    }

    /// The Catalogue window, when open.
    pub(super) fn catalogue_window(&mut self, ctx: &egui::Context) {
        if !self.catalogue.open {
            return;
        }
        // pictures of the starters, the user's own and the project's types
        let mut shown: Vec<(CatalogueObject, Option<ElementId>, bool)> =
            creabim_core::catalogue::starters()
                .into_iter()
                .map(|c| (c, None, false))
                .collect();
        let user: Vec<CatalogueObject> = self.user_catalogue().to_vec();
        shown.extend(user.into_iter().map(|c| (c, None, true)));
        let model = self.doc.model();
        let project: Vec<(CatalogueObject, Option<ElementId>, bool)> =
            creabim_core::types::types(model)
                .into_iter()
                .filter(|(_, _, t)| matches!(t.params, creabim_core::TypeParams::Object(_)))
                .filter_map(|(id, name, _)| {
                    let mut c = objects::from_type(model, id, &format!("project/{}", id.0))?;
                    c.name = name.to_string();
                    Some((c, Some(id), false))
                })
                .collect();
        let rev = self.doc.model().revision();
        for (c, own, _) in shown.iter().chain(&project) {
            let key = match own {
                Some(_) => format!("{}@{rev}", c.key),
                None => c.key.clone(),
            };
            self.catalogue.thumbs.entry(key).or_insert_with_key(|key| {
                ctx.load_texture(key, thumbnail(c, THUMB), egui::TextureOptions::LINEAR)
            });
        }
        let selected = self.selection.all();
        let mut open = true;
        let mut asks = vec![];
        let area = self.mat.area.unwrap_or_else(|| ctx.content_rect());
        let current = self.plan.objects.current.as_ref().map(|c| c.key.clone());
        // over the other pane than the one things are placed in, so the
        // plan (or the 3D view) stays clear to click in; with one view
        // shown, against its right edge
        let other = if self.last_in_2d {
            self.rect_3d
        } else {
            self.rect_2d
        };
        let thumb = THUMB as f32 + 26.0;
        let (width, pos) = match other {
            Some(r) if r.width() > 3.0 * thumb => {
                let w = (r.width() - 24.0).min(5.0 * thumb + 170.0);
                (w, egui::pos2(r.max.x - w - 12.0, r.min.y + 12.0))
            }
            _ => {
                let w = 3.0 * thumb + 194.0;
                (w, egui::pos2(area.max.x - w - 16.0, area.min.y + 64.0))
            }
        };
        crate::app::shell::window("Catalogue")
            .resizable(true)
            .open(&mut open)
            .default_pos(pos)
            .constrain_to(area)
            .default_size([width, (area.height() - 90.0).clamp(360.0, 640.0)])
            .collapsible(false)
            .show(ctx, |ui| {
                let cat = &mut self.catalogue;
                search_row(ui, &mut cat.filter);
                let filter = cat.filter.to_lowercase();
                let matches = |c: &CatalogueObject| {
                    filter.is_empty()
                        || c.name.to_lowercase().contains(&filter)
                        || c.description.to_lowercase().contains(&filter)
                };
                let all: Vec<&(CatalogueObject, Option<ElementId>, bool)> =
                    shown.iter().chain(&project).filter(|r| matches(&r.0)).collect();
                let count = |k: Option<ObjectCategory>| {
                    all.iter()
                        .filter(|r| k.is_none_or(|k| k == r.0.object.category))
                        .count()
                };
                let mut state = (cat, &mut asks);
                crate::library::frame(
                    ui,
                    "catalogue",
                    150.0,
                    &mut state,
                    |ui, (cat, _)| {
                        // categories as a list, with how many each holds
                        let mut pick = |ui: &mut egui::Ui, k: Option<ObjectCategory>, label: &str| {
                            let n = count(k);
                            if crate::library::item(ui, cat.category == k, label, &n.to_string(), None)
                                .clicked()
                            {
                                cat.category = k;
                            }
                        };
                        pick(ui, None, "All");
                        for c in ObjectCategory::ALL {
                            pick(ui, Some(c), c.label());
                        }
                    },
                    |ui, (_, asks)| {
                        if ui
                            .button("Import…")
                            .on_hover_text("A .blend, .FCStd, .gltf or .glb file becomes one object of your catalogue, each mesh a part in its material")
                            .clicked()
                        {
                            asks.push(CatAsk::Import);
                        }
                        if ui
                            .button("Folder")
                            .on_hover_text("Where your catalogue is kept: one .json file per object")
                            .clicked()
                        {
                            asks.push(CatAsk::Reveal);
                        }
                    },
                    |ui, (cat, asks)| {
                        let wanted = |c: &CatalogueObject| {
                            cat.category.is_none_or(|k| k == c.object.category) && matches(c)
                        };
                        let tile = |ui: &mut egui::Ui,
                                    cat: &CatalogueUi,
                                    c: &CatalogueObject,
                                    own: Option<ElementId>,
                                    user: bool,
                                    asks: &mut Vec<CatAsk>| {
                            let key = match own {
                                Some(_) => format!("{}@{rev}", c.key),
                                None => c.key.clone(),
                            };
                            let tex = cat.thumbs.get(&key);
                            let picked = current.as_deref() == Some(c.key.as_str());
                            let s = c.object.size();
                            ui.vertical(|ui| {
                                ui.set_width(THUMB as f32 + 16.0);
                                let r = match tex {
                                    Some(t) => ui.add(
                                        egui::Button::image(
                                            egui::Image::new(t)
                                                .fit_to_exact_size(Vec2::splat(THUMB as f32)),
                                        )
                                        .selected(picked),
                                    ),
                                    None => ui.add(egui::Button::new(&c.name).selected(picked)),
                                };
                                let r = r.on_hover_text(format!(
                                    "{}\n{} × {} × {}\n{} {}\n{}",
                                    c.name,
                                    crate::units::len(s.x),
                                    crate::units::len(s.y),
                                    crate::units::len(s.z),
                                    c.object.ifc.class,
                                    c.object.ifc.predefined,
                                    if c.description.is_empty() {
                                        "Click to place it"
                                    } else {
                                        &c.description
                                    }
                                ));
                                if r.clicked() {
                                    asks.push(CatAsk::Place(Box::new(c.clone()), own));
                                }
                                if user {
                                    r.context_menu(|ui| {
                                        if ui.button("Delete from the catalogue").clicked() {
                                            asks.push(CatAsk::Delete(c.key.clone()));
                                            ui.close();
                                        }
                                    });
                                }
                                ui.add(egui::Label::new(egui::RichText::new(&c.name).size(11.5)).wrap());
                            });
                        };
                        for (heading, rows) in [
                            ("Starters", shown.iter().filter(|r| !r.2).collect::<Vec<_>>()),
                            ("Yours", shown.iter().filter(|r| r.2).collect()),
                            ("In this project", project.iter().collect()),
                        ] {
                            let rows: Vec<_> = rows.into_iter().filter(|r| wanted(&r.0)).collect();
                            if rows.is_empty() {
                                if heading == "Yours" && cat.category.is_none() && filter.is_empty() {
                                    crate::props::empty(ui, "Yours: nothing yet. Import one from Blender, FreeCAD or glTF, or save a selection.");
                                }
                                continue;
                            }
                            let open = crate::library::group(ui, "catalogue", heading, rows.len());
                            // the rows fold smoothly (AMD-017)
                            crate::props::fold(ui, open, |ui| {
                            // rows of as many pictures as fit across
                            let per = ((ui.available_width() / (THUMB as f32 + 26.0)) as usize).max(1);
                            for chunk in rows.chunks(per) {
                                ui.horizontal_top(|ui| {
                                    for (c, own, user) in chunk {
                                        tile(ui, cat, c, *own, *user, asks);
                                    }
                                });
                            }
                            });
                        }
                        crate::library::foot(
                            ui,
                            "Click an object, then click in a view. Space turns it.",
                            |ui| {
                                let r = ui.add_enabled(!selected.is_empty(), egui::Button::new("Save selection"));
                                if r.on_hover_text("An object's type, or masses and other elements as one new object, kept for every project")
                                    .on_disabled_hover_text("Select an object, masses or other elements first")
                                    .clicked()
                                {
                                    asks.push(CatAsk::SaveSelection);
                                }
                                crate::props::text_field(ui, egui::TextEdit::singleline(&mut cat.save_name)
                                        .hint_text("Its name"), 140.0);
                            },
                        );
                    },
                );
            });
        self.catalogue.open = open;
        for a in asks {
            self.run_catalogue(a);
        }
    }

    fn run_catalogue(&mut self, ask: CatAsk) {
        match ask {
            CatAsk::Place(c, own) => {
                self.status = format!(
                    "{}: click in a plan or the 3D view to place it. Space turns it; Esc ends.",
                    c.name
                );
                self.plan.objects.current = Some(*c);
                self.plan.objects.offset = None;
                self.plan.objects.project_type = own;
                self.plan.cancel();
                self.plan.tool = Tool::Object;
            }
            CatAsk::Import => {
                let Some(path) = rfd::FileDialog::new()
                    .set_title("Import an object")
                    .add_filter(
                        "Objects (Blender, FreeCAD, glTF)",
                        &["blend", "FCStd", "fcstd", "gltf", "glb"],
                    )
                    .pick_file()
                else {
                    return;
                };
                self.import_object(&path);
            }
            CatAsk::SaveSelection => {
                let Some(dir) = user_dir() else {
                    self.status = "There is no settings folder to keep the catalogue in.".into();
                    return;
                };
                let ids = self.selection.all();
                let model = self.doc.model();
                let name = match self.catalogue.save_name.trim() {
                    "" => ids
                        .first()
                        .and_then(|i| model.get(*i))
                        .and_then(|e| match &e.kind {
                            ElementKind::Object(_) => {
                                creabim_core::types::type_of(model, e).map(|t| t.0.name.clone())
                            }
                            _ => Some(e.name.clone()),
                        })
                        .unwrap_or_else(|| "Object".into()),
                    n => n.to_string(),
                };
                let key = free_key(&dir, &name);
                let Some(c) = from_selection(model, &ids, &name, &key) else {
                    self.status = "Nothing in the selection has a shape to keep.".into();
                    return;
                };
                match save_to(&dir, &c) {
                    Ok(_) => {
                        self.status = format!("Saved {} to your catalogue.", c.name);
                        self.catalogue.user = None;
                        self.catalogue.open = true;
                        self.catalogue.save_name.clear();
                    }
                    Err(e) => self.status = format!("Couldn't save it: {e}"),
                }
            }
            CatAsk::Delete(key) => {
                if let Some(dir) = user_dir() {
                    match delete_from(&dir, &key) {
                        Ok(()) => {
                            self.status =
                                "Deleted from your catalogue. Objects placed from it stay.".into()
                        }
                        Err(e) => self.status = format!("Couldn't delete it: {e}"),
                    }
                }
                self.catalogue.user = None;
                self.catalogue.thumbs.remove(&key);
            }
            CatAsk::Reveal => {
                if let Some(dir) = user_dir() {
                    let _ = std::fs::create_dir_all(&dir);
                    self.status = format!("Your catalogue is in {}", dir.display());
                }
            }
        }
    }

    /// Read a file as a new object of the user's catalogue.
    pub(super) fn import_object(&mut self, path: &Path) {
        let Some(dir) = user_dir() else {
            self.status = "There is no settings folder to keep the catalogue in.".into();
            return;
        };
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("object")
            .to_string();
        let key = free_key(&dir, &stem);
        match creabim_exchange::catalogue::read_file(path, &key) {
            Ok(c) => match save_to(&dir, &c) {
                Ok(_) => {
                    let s = c.object.size();
                    self.status = format!(
                        "Added {} to your catalogue: {} parts, {} × {} × {}, as {}. Click it to place it; set its category and IFC class in Properties once placed.",
                        c.name,
                        c.object.parts.len(),
                        crate::units::len(s.x),
                        crate::units::len(s.y),
                        crate::units::len(s.z),
                        c.object.ifc.class
                    );
                    self.catalogue.user = None;
                    self.catalogue.open = true;
                    self.catalogue.category = None;
                }
                Err(e) => self.status = format!("Couldn't save it: {e}"),
            },
            Err(e) => self.status = format!("Couldn't read it: {e}"),
        }
    }
}

#[cfg(test)]
mod typing_tests {
    use super::*;

    /// A turn from Properties is about the object's middle: its middle
    /// stays where it was, whatever its first angle (MU142: the quarter
    /// turn was about its back corner, and the object moved).
    #[test]
    fn a_turn_keeps_the_objects_middle_in_place() {
        let mid = glam::DVec2::new(0.4, -0.35);
        for start in [0.0, 35.0, 147.0] {
            let mut o = objects::Object::new(ElementId::NIL, glam::DVec2::new(3.0, 2.0), start);
            let c = o.to_plan(mid);
            for by in [90.0, 180.0, -90.0] {
                turn_about_centre(&mut o, mid, by);
                assert!(
                    o.to_plan(mid).distance(c) < 1e-9,
                    "moved by turning {by} from {start}"
                );
            }
            // 90 + 180 - 90: a half turn on from where it started
            assert!(
                (o.rotation - (start + 180.0).rem_euclid(360.0)).abs() < 1e-9,
                "{}",
                o.rotation
            );
        }
    }

    /// Typing 140 into an object's Rotation turns it once, when Enter is
    /// pressed, and the field keeps every key typed (MU141: each key turned
    /// the object and the panel threw the cursor out: 1, then 14).
    #[test]
    fn a_rotation_is_typed_whole_and_taken_on_enter() {
        let ctx = egui::Context::default();
        let mut o = objects::Object::new(ElementId::NIL, glam::DVec2::ZERO, 0.0);
        let taken = std::cell::RefCell::new(vec![]);
        let mut t = 0.0;
        let mut frame = |o: &mut objects::Object, events: Vec<egui::Event>| -> Option<egui::Rect> {
            t += 0.1;
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(300.0, 200.0),
                )),
                events,
                time: Some(t),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                ui.set_max_width(232.0);
                if !object_fields(ui, o, false, glam::DVec2::ZERO).is_empty() {
                    taken.borrow_mut().push(o.rotation);
                }
            });
            out.textures_delta.clear();
            out.shapes.iter().find_map(|c| match &c.shape {
                egui::Shape::Text(s)
                    if s.galley.text().ends_with('°') && !s.galley.text().contains('9') =>
                {
                    Some(c.shape.visual_bounding_rect())
                }
                _ => None,
            })
        };
        let rect = frame(&mut o, vec![]).expect("the rotation field");
        let at = rect.center();
        let press = |down| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: down,
            modifiers: Default::default(),
        };
        frame(&mut o, vec![egui::Event::PointerMoved(at)]);
        frame(&mut o, vec![press(true)]);
        frame(&mut o, vec![press(false)]);
        // select what is there and type over it, key by key
        frame(
            &mut o,
            vec![egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
        );
        for k in ["1", "4", "0"] {
            frame(&mut o, vec![egui::Event::Text(k.into())]);
            assert_eq!(
                o.rotation,
                0.0,
                "turned before Enter, at {k}: {:?}",
                taken.borrow()
            );
        }
        frame(
            &mut o,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        frame(&mut o, vec![]);
        assert_eq!(o.rotation, 140.0, "taken: {:?}", taken.borrow());
        // turned once, to 140: no 1 or 14 on the way
        assert!(
            taken.borrow().iter().all(|r| *r == 0.0 || *r == 140.0),
            "turned on the way: {:?}",
            taken.borrow()
        );
    }
}

/// The Catalogue's search, at the top right as in the other library
/// windows; returns where its field is.
fn search_row(ui: &mut egui::Ui, filter: &mut String) -> egui::Rect {
    // one row, as the Materials and Patterns windows put theirs: a
    // right-to-left layout straight in the window took its whole height
    // and centred the field in it (chopinregis/CreaBIM#113)
    let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(
        row,
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            crate::props::text_field(
                ui,
                egui::TextEdit::singleline(filter).hint_text("Find an object"),
                200.0,
            )
            .rect
        },
    )
    .inner
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_catalogue_search_takes_one_row_not_the_window() {
        // chopinregis/CreaBIM#113: in a tall window the search sat halfway
        // down, a huge empty band above the catalogue
        let ctx = egui::Context::default();
        let mut row = egui::Rect::NOTHING;
        let mut field = 0.0;
        let mut top = 0.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 1000.0),
            )),
            ..Default::default()
        };
        // as the Catalogue opens: a resizable window of its default size
        for _ in 0..5 {
            let mut out = ctx.run_ui(input.clone(), |ui| {
                let ctx = ui.ctx().clone();
                egui::Window::new("Catalogue")
                    .resizable(true)
                    .collapsible(false)
                    .default_pos(egui::pos2(100.0, 64.0))
                    .default_size([700.0, 640.0])
                    .show(&ctx, |ui| {
                        let mut filter = String::new();
                        field = ui.spacing().interact_size.y;
                        top = ui.cursor().top();
                        row = super::search_row(ui, &mut filter);
                        // the catalogue below takes the rest, as in the app
                        crate::library::fit(ui, |ui| ui.label("the catalogue"));
                    });
            });
            out.textures_delta.clear();
        }
        assert!(
            row.top() - top <= 1.0,
            "the search sits {} below the top of the window, a field is {field} tall",
            row.top() - top
        );
    }

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("creabim-cat-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn the_users_catalogue_keeps_objects_as_files() {
        let dir = scratch("keep");
        let sofa = creabim_core::catalogue::starter("starter/sofa-3").unwrap();
        let key = free_key(&dir, "My sofa!");
        assert_eq!(key, "user/my-sofa");
        let mut mine = sofa.clone();
        mine.key = key.clone();
        mine.name = "My sofa".into();
        save_to(&dir, &mine).unwrap();
        assert_eq!(free_key(&dir, "my sofa"), "user/my-sofa-2");
        let back = load_dir(&dir);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].key, key);
        assert_eq!(back[0].object.catalogue.as_deref(), Some(key.as_str()));
        assert_eq!(back[0].object.parts.len(), sofa.object.parts.len());
        delete_from(&dir, &key).unwrap();
        assert!(load_dir(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn thumbnails_show_the_object() {
        let c = creabim_core::catalogue::starter("starter/bathtub").unwrap();
        let img = thumbnail(&c, 48);
        assert_eq!(img.size, [48, 48]);
        let solid = img.pixels.iter().filter(|p| p.a() > 200).count();
        assert!(solid > 48 * 48 / 8, "{solid}");
        // and leaves the background clear
        assert_eq!(img.pixels[0].a(), 0);
    }

    #[test]
    fn a_selection_becomes_a_catalogue_object() {
        let model = creabim_core::sample::furnished_house();
        let obj = model
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Object(_)))
            .unwrap();
        let c = from_selection(&model, &[obj.id], "Mine", "user/mine").unwrap();
        assert_eq!(c.name, "Mine");
        assert_eq!(c.object.catalogue.as_deref(), Some("user/mine"));
        assert!(!c.object.parts.is_empty());
        // a wall, as its shape
        let wall = model
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Wall(_)))
            .unwrap();
        let w = from_selection(&model, &[wall.id], "Partition", "user/p").unwrap();
        let (lo, hi) = w.object.bounds();
        assert!(lo.z.abs() < 1e-6 && hi.y.abs() < 1e-6, "{lo} {hi}");
        assert!(w.materials.iter().all(|m| m.is_some()));
    }
}
