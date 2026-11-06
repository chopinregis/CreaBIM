//! An open way into CreaBIM: a documented, versioned JSON format that
//! describes a model (levels, grids, walls with their types and layers,
//! doors and windows by wall and offset, slabs, roofs, columns, beams,
//! stairs, railings, parts, site, furniture, rooms, views and sheets),
//! so any script can make a full model without compiling against
//! creabim-core. [`build`] turns a file into a model, [`export`] turns a
//! model back into a file. Elements refer to each other by ids the
//! file's author chooses, never by CreaBIM's internal ids.
//!
//! The format is declared once, in [`format`]; the reader, the writer,
//! the JSON Schema ([`schema`]) and the reference in the docs
//! ([`reference`]) all come from that declaration.

pub mod build;
pub mod export;
pub mod format;
pub mod json;

pub use build::{build, Built};
pub use export::{export, Exported};
pub use json::{Path, Problem};

use json::{Defs, Spec, TypeDoc, J};

/// Read a file of the format, checking its shape (every field known, of
/// the right type, every required one there) but not yet what it refers
/// to; [`build`] checks that.
pub fn parse(text: &str) -> Result<format::Model, Vec<Problem>> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| {
        vec![Problem {
            path: "$".into(),
            message: format!("not valid JSON: {e}"),
        }]
    })?;
    let mut out = vec![];
    if let Some(o) = value.as_object() {
        if o.contains_key("elements") && !o.contains_key("version") {
            return Err(vec![Problem {
                path: "$".into(),
                message: "this looks like a saved .creabim.json project, not a model \
                          description; open it in CreaBIM or convert it with `creabim save-db`"
                    .into(),
            }]);
        }
        // a newer file is refused for its version, not for fields this
        // version doesn't know yet
        if let Some(v) = o.get("version").and_then(|v| v.as_u64()) {
            if v > u64::from(format::VERSION) {
                return Err(vec![Problem {
                    path: "$.version".into(),
                    message: format!(
                        "the file is format version {v}; this CreaBIM reads up to version {}",
                        format::VERSION
                    ),
                }]);
            }
        }
    }
    match format::Model::read(&value, &Path::root(), &mut out) {
        Some(m) if out.is_empty() => {
            if m.version == 0 {
                return Err(vec![Problem {
                    path: "$.version".into(),
                    message: format!("the version starts at 1; write {}", format::VERSION),
                }]);
            }
            Ok(m)
        }
        _ => Err(out),
    }
}

/// Write a model description as JSON text.
pub fn to_text(m: &format::Model) -> String {
    m.write().pretty()
}

/// The format's JSON Schema (draft 2020-12), as `docs/creabim-model.schema.json` holds it.
pub fn schema() -> J {
    let mut defs = Defs::default();
    let root = format::Model::schema(&mut defs);
    J::obj(vec![
        (
            "$schema",
            J::str("https://json-schema.org/draft/2020-12/schema"),
        ),
        (
            "$id",
            J::str(
                "https://github.com/chopinregis/CreaBIM/blob/main/docs/creabim-model.schema.json",
            ),
        ),
        ("title", J::str("CreaBIM model")),
        (
            "description",
            J::Str(format!(
                "A CreaBIM model described in JSON, format version {}. Lengths are in metres \
                 unless `units` says otherwise. See docs/json-format.md.",
                format::VERSION
            )),
        ),
        (
            "$ref",
            match root {
                J::Obj(o) => o[0].1.clone(),
                _ => unreachable!(),
            },
        ),
        ("$defs", J::Obj(defs.schemas)),
    ])
}

/// The field-by-field reference, in Markdown, as `docs/json-format.md`
/// holds it between its reference markers.
pub fn reference() -> String {
    let mut defs = Defs::default();
    format::Model::schema(&mut defs);
    let mut out = String::new();
    let cell = |s: &str| s.replace('|', "\\|").replace("\n\n", "<br>");
    for (name, doc) in &defs.docs {
        out.push_str(&format!("### {name}\n\n"));
        match doc {
            TypeDoc::Struct { doc, fields } => {
                if !doc.is_empty() {
                    out.push_str(&format!("{doc}\n\n"));
                }
                out.push_str("| Field | Type | | Description |\n|---|---|---|---|\n");
                for f in fields {
                    let need = match (&f.default, f.required) {
                        (_, true) => "required".to_string(),
                        (Some(d), _) => format!("default `{d}`"),
                        (None, false) => String::new(),
                    };
                    out.push_str(&format!(
                        "| `{}` | {} | {} | {} |\n",
                        f.key,
                        f.label,
                        need,
                        cell(&f.doc)
                    ));
                }
            }
            TypeDoc::Enum { doc, values } => {
                if !doc.is_empty() {
                    out.push_str(&format!("{doc}\n\n"));
                }
                for (v, d) in values {
                    if d.is_empty() {
                        out.push_str(&format!("- `\"{v}\"`\n"));
                    } else {
                        out.push_str(&format!("- `\"{v}\"`: {d}\n"));
                    }
                }
            }
        }
        out.push('\n');
    }
    out
}
