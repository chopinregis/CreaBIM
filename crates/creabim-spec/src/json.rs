//! The machinery the format is written in: each type of the format reads
//! itself from JSON with exact paths in its errors, writes itself back,
//! and describes itself as JSON Schema and as a reference table, all from
//! one declaration ([`spec_struct!`], [`spec_enum!`]). The schema and the
//! docs can't drift from what the reader accepts because they are made
//! from the same field list.

use glam::{DVec2, DVec3};
use serde_json::Value;
use std::collections::BTreeMap;

/// JSON with its object keys kept in order, for what CreaBIM writes (the
/// export and the schema): `id` first, then the fields in the order the
/// docs give them.
#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Bool(bool),
    Num(f64),
    Int(i64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn obj(fields: Vec<(&str, J)>) -> J {
        J::Obj(
            fields
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }

    pub fn str(s: impl Into<String>) -> J {
        J::Str(s.into())
    }

    /// The value as serde_json's, to read it back.
    pub fn to_value(&self) -> Value {
        match self {
            J::Bool(b) => Value::Bool(*b),
            J::Num(n) => Value::from(*n),
            J::Int(i) => Value::from(*i),
            J::Str(s) => Value::String(s.clone()),
            J::Arr(a) => Value::Array(a.iter().map(J::to_value).collect()),
            J::Obj(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), v.to_value())).collect()),
        }
    }

    /// Indented JSON; short arrays and small flat objects stay on one line.
    pub fn pretty(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out.push('\n');
        out
    }

    fn scalar(&self) -> String {
        match self {
            J::Bool(b) => b.to_string(),
            J::Num(n) => Value::from(*n).to_string(),
            J::Int(i) => i.to_string(),
            J::Str(s) => Value::String(s.clone()).to_string(),
            _ => unreachable!("not a scalar"),
        }
    }

    /// A scalar, or an array of scalars (a point).
    fn flat(&self) -> bool {
        match self {
            J::Arr(a) => a.iter().all(|v| !matches!(v, J::Arr(_) | J::Obj(_))),
            J::Obj(o) => o.is_empty(),
            _ => true,
        }
    }

    fn compact(&self) -> String {
        match self {
            J::Arr(a) => format!(
                "[{}]",
                a.iter().map(J::compact).collect::<Vec<_>>().join(", ")
            ),
            J::Obj(o) if o.is_empty() => "{}".into(),
            J::Obj(o) => format!(
                "{{ {} }}",
                o.iter()
                    .map(|(k, v)| format!("{}: {}", J::Str(k.clone()).scalar(), v.compact()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            v => v.scalar(),
        }
    }

    /// One line, when it is short: points, lists of points, and objects
    /// of scalars and points.
    fn inline(&self) -> Option<String> {
        let fits = match self {
            J::Arr(a) => a.iter().all(J::flat),
            J::Obj(o) => o.iter().all(|(_, v)| v.flat()),
            _ => true,
        };
        let s = self.compact();
        (fits && s.len() <= 76).then_some(s)
    }

    fn write(&self, out: &mut String, indent: usize) {
        if let Some(s) = self.inline() {
            out.push_str(&s);
            return;
        }
        let pad = "  ".repeat(indent + 1);
        match self {
            J::Arr(a) => {
                out.push_str("[\n");
                for (i, v) in a.iter().enumerate() {
                    out.push_str(&pad);
                    v.write(out, indent + 1);
                    out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
                }
            }
            J::Obj(o) => {
                out.push_str("{\n");
                for (i, (k, v)) in o.iter().enumerate() {
                    out.push_str(&pad);
                    out.push_str(&J::Str(k.clone()).scalar());
                    out.push_str(": ");
                    v.write(out, indent + 1);
                    out.push_str(if i + 1 < o.len() { ",\n" } else { "\n" });
                }
            }
            v => {
                out.push_str(&v.scalar());
                return;
            }
        }
        out.push_str(&"  ".repeat(indent));
        out.push(if matches!(self, J::Arr(_)) { ']' } else { '}' });
    }
}

/// Where a value is in the file, as a JSON path: `$.walls[3].start`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Path(pub String);

impl Path {
    pub fn root() -> Self {
        Path("$".into())
    }
    pub fn key(&self, k: &str) -> Self {
        Path(format!("{}.{k}", self.0))
    }
    pub fn index(&self, i: usize) -> Self {
        Path(format!("{}[{i}]", self.0))
    }
}

impl std::fmt::Display for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One thing wrong with a file, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub path: String,
    pub message: String,
}

impl Problem {
    pub fn new(at: &Path, message: impl Into<String>) -> Self {
        Self {
            path: at.0.clone(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

/// What a JSON value is, for messages.
pub fn what(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "true or false",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// The closest of `options` to a misspelt `word`, if one is close.
pub fn suggest<'a>(word: &str, options: &[&'a str]) -> Option<&'a str> {
    fn distance(a: &str, b: &str) -> usize {
        let b: Vec<char> = b.chars().collect();
        let mut row: Vec<usize> = (0..=b.len()).collect();
        for (i, ca) in a.chars().enumerate() {
            let mut prev = row[0];
            row[0] = i + 1;
            for (j, cb) in b.iter().enumerate() {
                let cur = row[j + 1];
                row[j + 1] = (prev + usize::from(ca != *cb)).min(row[j] + 1).min(cur + 1);
                prev = cur;
            }
        }
        row[b.len()]
    }
    let lower = word.to_lowercase();
    options
        .iter()
        .map(|o| (distance(&lower, &o.to_lowercase()), *o))
        .filter(|(d, o)| *d <= 2.max(o.len() / 3))
        .min_by_key(|(d, _)| *d)
        .map(|(_, o)| o)
}

/// "unknown field `thikness` in a wall: did you mean `thickness`?"
pub fn unknown(what: &str, word: &str, options: &[&str]) -> String {
    match suggest(word, options) {
        Some(s) => format!("unknown {what} `{word}`: did you mean `{s}`?"),
        None => format!(
            "unknown {what} `{word}`; expected one of {}",
            options.join(", ")
        ),
    }
}

/// One field of a struct, for the reference.
#[derive(Clone, Debug)]
pub struct FieldDoc {
    pub key: &'static str,
    pub label: String,
    pub required: bool,
    pub default: Option<String>,
    pub doc: String,
}

/// One named type of the format, for the reference.
#[derive(Clone, Debug)]
pub enum TypeDoc {
    Struct {
        doc: String,
        fields: Vec<FieldDoc>,
    },
    Enum {
        doc: String,
        values: Vec<(&'static str, String)>,
    },
}

/// The named types met while describing the format: their schemas (the
/// `$defs`) and their reference entries, in the order first met.
#[derive(Default)]
pub struct Defs {
    pub schemas: Vec<(String, J)>,
    pub docs: Vec<(String, TypeDoc)>,
}

impl Defs {
    pub fn has(&self, name: &str) -> bool {
        self.schemas.iter().any(|(n, _)| n == name)
    }
    /// Claim `name` before describing its fields, so a type met again
    /// inside itself refers to it instead of recursing.
    pub fn reserve(&mut self, name: &str) {
        self.schemas.push((name.into(), J::Obj(vec![])));
    }
    pub fn define(&mut self, name: &str, schema: J, doc: TypeDoc) {
        if let Some(s) = self.schemas.iter_mut().find(|(n, _)| n == name) {
            s.1 = schema;
        }
        self.docs.push((name.into(), doc));
    }
}

/// A `$ref` to a named type.
pub fn reference(name: &str) -> J {
    J::obj(vec![("$ref", J::Str(format!("#/$defs/{name}")))])
}

/// Doc comment lines as one text: lines joined, blank lines kept as breaks.
pub fn doc_text(lines: &[&str]) -> String {
    let mut out = String::new();
    for l in lines {
        let l = l.trim();
        if l.is_empty() {
            out.push_str("\n\n");
        } else {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push(' ');
            }
            out.push_str(l);
        }
    }
    out.trim().to_string()
}

/// The JSON key of a field: its own name, or the one given for it.
pub fn key_of(ident: &'static str, given: &[&'static str]) -> &'static str {
    given
        .first()
        .copied()
        .unwrap_or_else(|| ident.trim_start_matches("r#"))
}

/// A value of the format: read from JSON, written back, described.
pub trait Spec: Sized {
    /// Its JSON Schema, registering named types in `defs`.
    fn schema(defs: &mut Defs) -> J;
    /// How the reference names its type: "number", "[x, y]", "[Wall](#wall)".
    fn label() -> String;
    /// Read it from `v`, at `at`; problems go to `out`.
    fn read(v: &Value, at: &Path, out: &mut Vec<Problem>) -> Option<Self>;
    fn write(&self) -> J;
    /// What a field of it is when left out; none when it must be given.
    fn absent() -> Option<Self> {
        None
    }
}

impl Spec for f64 {
    fn schema(_: &mut Defs) -> J {
        J::obj(vec![("type", J::str("number"))])
    }
    fn label() -> String {
        "number".into()
    }
    fn read(v: &Value, at: &Path, out: &mut Vec<Problem>) -> Option<Self> {
        match v.as_f64() {
            Some(n) => Some(n),
            None => {
                out.push(Problem::new(
                    at,
                    format!("expected a number, got {}", show(v)),
                ));
                None
            }
        }
    }
    fn write(&self) -> J {
        // float noise (0.30000000000000004) written as what was meant
        let r = (self * 1e9).round() / 1e9;
        J::Num(if (r - self).abs() < 1e-12 { r } else { *self })
    }
}

impl Spec for u32 {
    fn schema(_: &mut Defs) -> J {
        J::obj(vec![("type", J::str("integer")), ("minimum", J::Int(0))])
    }
    fn label() -> String {
        "integer".into()
    }
    fn read(v: &Value, at: &Path, out: &mut Vec<Problem>) -> Option<Self> {
        match v.as_u64().and_then(|n| u32::try_from(n).ok()) {
            Some(n) => Some(n),
            None => {
                out.push(Problem::new(
                    at,
                    format!("expected a whole number 0 or more, got {}", show(v)),
                ));
                None
            }
        }
    }
    fn write(&self) -> J {
        J::Int(i64::from(*self))
    }
}

/// A short form of a value for messages.
fn show(v: &Value) -> String {
    match v {
        Value::Number(_) | Value::Bool(_) | Value::Null => v.to_string(),
        Value::String(s) if s.len() < 40 => format!("\"{s}\""),
        _ => what(v).into(),
    }
}

impl Spec for bool {
    fn schema(_: &mut Defs) -> J {
        J::obj(vec![("type", J::str("boolean"))])
    }
    fn label() -> String {
        "boolean".into()
    }
    fn read(v: &Value, at: &Path, out: &mut Vec<Problem>) -> Option<Self> {
        match v.as_bool() {
            Some(b) => Some(b),
            None => {
                out.push(Problem::new(
                    at,
                    format!("expected true or false, got {}", show(v)),
                ));
                None
            }
        }
    }
    fn write(&self) -> J {
        J::Bool(*self)
    }
}

impl Spec for String {
    fn schema(_: &mut Defs) -> J {
        J::obj(vec![("type", J::str("string"))])
    }
    fn label() -> String {
        "string".into()
    }
    fn read(v: &Value, at: &Path, out: &mut Vec<Problem>) -> Option<Self> {
        match v.as_str() {
            Some(s) => Some(s.to_string()),
            None => {
                out.push(Problem::new(
                    at,
                    format!("expected a string, got {}", show(v)),
                ));
                None
            }
        }
    }
    fn write(&self) -> J {
        J::Str(self.clone())
    }
}

/// An optional field. `null` is refused rather than read as "left out",
/// so a file means one thing.
impl<T: Spec> Spec for Option<T> {
    fn schema(defs: &mut Defs) -> J {
        T::schema(defs)
    }
    fn label() -> String {
        T::label()
    }
    fn read(v: &Value, at: &Path, out: &mut Vec<Problem>) -> Option<Self> {
        if v.is_null() {
            out.push(Problem::new(
                at,
                "null is not allowed: leave the field out instead",
            ));
            return None;
        }
        T::read(v, at, out).map(Some)
    }
    fn write(&self) -> J {
        self.as_ref()
            .expect("an absent field is not written")
            .write()
    }
    fn absent() -> Option<Self> {
        Some(None)
    }
}

impl<T: Spec> Spec for Vec<T> {
    fn schema(defs: &mut Defs) -> J {
        J::obj(vec![("type", J::str("array")), ("items", T::schema(defs))])
    }
    fn label() -> String {
        format!("array of {}", T::label())
    }
    fn read(v: &Value, at: &Path, out: &mut Vec<Problem>) -> Option<Self> {
        let Some(a) = v.as_array() else {
            out.push(Problem::new(
                at,
                format!("expected an array, got {}", what(v)),
            ));
            return None;
        };
        let mut ok = true;
        let mut items = Vec::with_capacity(a.len());
        for (i, x) in a.iter().enumerate() {
            match T::read(x, &at.index(i), out) {
                Some(t) => items.push(t),
                None => ok = false,
            }
        }
        ok.then_some(items)
    }
    fn write(&self) -> J {
        J::Arr(self.iter().map(Spec::write).collect())
    }
    fn absent() -> Option<Self> {
        Some(vec![])
    }
}

/// Free text by name: a sheet's title block fields.
impl Spec for BTreeMap<String, String> {
    fn schema(_: &mut Defs) -> J {
        J::obj(vec![
            ("type", J::str("object")),
            (
                "additionalProperties",
                J::obj(vec![("type", J::str("string"))]),
            ),
        ])
    }
    fn label() -> String {
        "object of strings".into()
    }
    fn read(v: &Value, at: &Path, out: &mut Vec<Problem>) -> Option<Self> {
        let Some(o) = v.as_object() else {
            out.push(Problem::new(
                at,
                format!("expected an object, got {}", what(v)),
            ));
            return None;
        };
        let mut ok = true;
        let mut map = BTreeMap::new();
        for (k, x) in o {
            match String::read(x, &at.key(k), out) {
                Some(s) => {
                    map.insert(k.clone(), s);
                }
                None => ok = false,
            }
        }
        ok.then_some(map)
    }
    fn write(&self) -> J {
        J::Obj(
            self.iter()
                .map(|(k, v)| (k.clone(), J::Str(v.clone())))
                .collect(),
        )
    }
    fn absent() -> Option<Self> {
        Some(BTreeMap::new())
    }
}

fn numbers(v: &Value, n: usize, at: &Path, out: &mut Vec<Problem>) -> Option<Vec<f64>> {
    let shape = if n == 2 { "[x, y]" } else { "[x, y, z]" };
    match v.as_array() {
        Some(a) if a.len() == n => {
            let mut ok = true;
            let mut xs = vec![];
            for (i, x) in a.iter().enumerate() {
                match f64::read(x, &at.index(i), out) {
                    Some(f) => xs.push(f),
                    None => ok = false,
                }
            }
            ok.then_some(xs)
        }
        Some(a) => {
            out.push(Problem::new(
                at,
                format!("expected a point {shape}, got {} numbers", a.len()),
            ));
            None
        }
        None => {
            out.push(Problem::new(
                at,
                format!("expected a point {shape}, got {}", what(v)),
            ));
            None
        }
    }
}

fn point_schema(n: i64) -> J {
    J::obj(vec![
        ("type", J::str("array")),
        ("items", J::obj(vec![("type", J::str("number"))])),
        ("minItems", J::Int(n)),
        ("maxItems", J::Int(n)),
    ])
}

/// A point in plan, `[x, y]`.
impl Spec for DVec2 {
    fn schema(_: &mut Defs) -> J {
        point_schema(2)
    }
    fn label() -> String {
        "[x, y]".into()
    }
    fn read(v: &Value, at: &Path, out: &mut Vec<Problem>) -> Option<Self> {
        numbers(v, 2, at, out).map(|x| DVec2::new(x[0], x[1]))
    }
    fn write(&self) -> J {
        J::Arr(vec![self.x.write(), self.y.write()])
    }
}

/// A point in space, `[x, y, z]`.
impl Spec for DVec3 {
    fn schema(_: &mut Defs) -> J {
        point_schema(3)
    }
    fn label() -> String {
        "[x, y, z]".into()
    }
    fn read(v: &Value, at: &Path, out: &mut Vec<Problem>) -> Option<Self> {
        numbers(v, 3, at, out).map(|x| DVec3::new(x[0], x[1], x[2]))
    }
    fn write(&self) -> J {
        J::Arr(vec![self.x.write(), self.y.write(), self.z.write()])
    }
}

/// A colour: `"#rrggbb"`, or `[r, g, b]` from 0 to 1 for one that needs
/// more than 8 bits a channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colour(pub [f32; 3]);

impl Spec for Colour {
    fn schema(_: &mut Defs) -> J {
        J::obj(vec![(
            "oneOf",
            J::Arr(vec![
                J::obj(vec![
                    ("type", J::str("string")),
                    ("pattern", J::str("^#[0-9a-fA-F]{6}$")),
                ]),
                J::obj(vec![
                    ("type", J::str("array")),
                    (
                        "items",
                        J::obj(vec![
                            ("type", J::str("number")),
                            ("minimum", J::Int(0)),
                            ("maximum", J::Int(1)),
                        ]),
                    ),
                    ("minItems", J::Int(3)),
                    ("maxItems", J::Int(3)),
                ]),
            ]),
        )])
    }
    fn label() -> String {
        "colour".into()
    }
    fn read(v: &Value, at: &Path, out: &mut Vec<Problem>) -> Option<Self> {
        let bad = |out: &mut Vec<Problem>| {
            out.push(Problem::new(
                at,
                format!(
                    "expected a colour, \"#rrggbb\" or [r, g, b] from 0 to 1, got {}",
                    show(v)
                ),
            ));
            None
        };
        match v {
            Value::String(s) => {
                let hex = s.strip_prefix('#').filter(|h| h.len() == 6);
                let Some(c) = hex.and_then(|h| {
                    let ch = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
                    Some([ch(0)?, ch(2)?, ch(4)?])
                }) else {
                    return bad(out);
                };
                Some(Colour(c.map(|c| f32::from(c) / 255.0)))
            }
            Value::Array(a)
                if a.len() == 3
                    && a.iter()
                        .all(|x| x.as_f64().is_some_and(|f| (0.0..=1.0).contains(&f))) =>
            {
                let c = |i: usize| a[i].as_f64().unwrap_or(0.0) as f32;
                Some(Colour([c(0), c(1), c(2)]))
            }
            _ => bad(out),
        }
    }
    fn write(&self) -> J {
        let bytes = self.0.map(|c| (c * 255.0).round());
        let exact = self
            .0
            .iter()
            .zip(bytes)
            .all(|(c, b)| (c * 255.0 - b).abs() < 1e-3);
        if exact {
            J::Str(format!(
                "#{:02x}{:02x}{:02x}",
                bytes[0] as u8, bytes[1] as u8, bytes[2] as u8
            ))
        } else {
            J::Arr(
                self.0
                    .iter()
                    .map(|c| J::Num(c.to_string().parse().unwrap_or(f64::from(*c))))
                    .collect(),
            )
        }
    }
}

/// Declare a struct of the format: the Rust type, its reader, writer,
/// schema and reference entry, from one field list. A field is
/// `/// doc` `name: Type`, optionally `as "key"` for a key that is not a
/// Rust name, and `= default` for one that may be left out. `Option`
/// and `Vec` fields may always be left out.
#[macro_export]
macro_rules! spec_struct {
    (@absent $t:ty) => { <$t as $crate::json::Spec>::absent() };
    (@absent $t:ty, $def:expr) => { Some::<$t>($def) };
    (
        $(#[doc = $doc:literal])*
        pub struct $name:ident {
            $(
                $(#[doc = $fdoc:literal])*
                $f:ident $(as $key:literal)? : $t:ty $(= $def:expr)?
            ),* $(,)?
        }
    ) => {
        $(#[doc = $doc])*
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name {
            $( $(#[doc = $fdoc])* pub $f: $t, )*
        }

        impl $crate::json::Spec for $name {
            fn schema(defs: &mut $crate::json::Defs) -> $crate::json::J {
                use $crate::json::{J, Spec};
                let name = stringify!($name);
                if !defs.has(name) {
                    defs.reserve(name);
                    let mut props = vec![];
                    let mut required = vec![];
                    let mut fields = vec![];
                    $(
                        let key = $crate::json::key_of(stringify!($f), &[$($key)?]);
                        let doc = $crate::json::doc_text(&[$($fdoc),*]);
                        let absent: Option<$t> = $crate::spec_struct!(@absent $t $(, $def)?);
                        // a default worth stating: not just "none" or "empty"
                        let default = absent.as_ref().and_then(|a| {
                            let empty = <$t as Spec>::absent();
                            (empty.as_ref() != Some(a)).then(|| a.write())
                        });
                        let mut s = match <$t as Spec>::schema(defs) {
                            J::Obj(o) => o,
                            _ => unreachable!("schemas are objects"),
                        };
                        if !doc.is_empty() {
                            s.push(("description".into(), J::Str(doc.clone())));
                        }
                        if let Some(d) = &default {
                            s.push(("default".into(), d.clone()));
                        }
                        props.push((key.to_string(), J::Obj(s)));
                        if absent.is_none() {
                            required.push(J::str(key));
                        }
                        fields.push($crate::json::FieldDoc {
                            key,
                            label: <$t as Spec>::label(),
                            required: absent.is_none(),
                            default: default.map(|d| d.pretty().trim().to_string()),
                            doc,
                        });
                    )*
                    let doc = $crate::json::doc_text(&[$($doc),*]);
                    let mut schema = vec![("type", J::str("object"))];
                    if !doc.is_empty() {
                        schema.push(("description", J::Str(doc.clone())));
                    }
                    schema.push(("properties", J::Obj(props)));
                    if !required.is_empty() {
                        schema.push(("required", J::Arr(required)));
                    }
                    schema.push(("additionalProperties", J::Bool(false)));
                    defs.define(
                        name,
                        J::obj(schema),
                        $crate::json::TypeDoc::Struct { doc, fields },
                    );
                }
                $crate::json::reference(name)
            }

            fn label() -> String {
                let name = stringify!($name);
                format!("[{name}](#{})", name.to_lowercase())
            }

            #[allow(unused_mut)]
            fn read(
                v: &serde_json::Value,
                at: &$crate::json::Path,
                out: &mut Vec<$crate::json::Problem>,
            ) -> Option<Self> {
                use $crate::json::Spec;
                let Some(obj) = v.as_object() else {
                    out.push($crate::json::Problem::new(
                        at,
                        format!("expected an object, got {}", $crate::json::what(v)),
                    ));
                    return None;
                };
                let keys: &[&str] = &[$($crate::json::key_of(stringify!($f), &[$($key)?])),*];
                let mut ok = true;
                for k in obj.keys() {
                    if !keys.contains(&k.as_str()) {
                        out.push($crate::json::Problem::new(
                            &at.key(k),
                            $crate::json::unknown("field", k, keys),
                        ));
                        ok = false;
                    }
                }
                $(
                    let key = $crate::json::key_of(stringify!($f), &[$($key)?]);
                    let $f: Option<$t> = match obj.get(key) {
                        Some(x) => <$t as Spec>::read(x, &at.key(key), out),
                        None => {
                            let a: Option<$t> = $crate::spec_struct!(@absent $t $(, $def)?);
                            if a.is_none() {
                                out.push($crate::json::Problem::new(
                                    at,
                                    format!("missing field `{key}`"),
                                ));
                            }
                            a
                        }
                    };
                    ok &= $f.is_some();
                )*
                if !ok {
                    return None;
                }
                Some(Self { $($f: $f?),* })
            }

            fn write(&self) -> $crate::json::J {
                #[allow(unused_imports)]
                use $crate::json::Spec;
                let mut o: Vec<(String, $crate::json::J)> = vec![];
                $(
                    let key = $crate::json::key_of(stringify!($f), &[$($key)?]);
                    let a: Option<$t> = $crate::spec_struct!(@absent $t $(, $def)?);
                    if a.as_ref() != Some(&self.$f) {
                        o.push((key.to_string(), self.$f.write()));
                    }
                )*
                $crate::json::J::Obj(o)
            }
        }
    };
}

/// Declare a set of named values of the format, written as strings.
#[macro_export]
macro_rules! spec_enum {
    (
        $(#[doc = $doc:literal])*
        pub enum $name:ident {
            $( $(#[doc = $vdoc:literal])* $v:ident = $s:literal ),* $(,)?
        }
    ) => {
        $(#[doc = $doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name { $( $(#[doc = $vdoc])* $v, )* }

        impl $name {
            pub const ALL: &[$name] = &[$($name::$v),*];
            pub fn key(self) -> &'static str {
                match self { $($name::$v => $s,)* }
            }
        }

        impl $crate::json::Spec for $name {
            fn schema(defs: &mut $crate::json::Defs) -> $crate::json::J {
                use $crate::json::J;
                let name = stringify!($name);
                if !defs.has(name) {
                    defs.reserve(name);
                    let doc = $crate::json::doc_text(&[$($doc),*]);
                    let mut schema = vec![("type", J::str("string"))];
                    if !doc.is_empty() {
                        schema.push(("description", J::Str(doc.clone())));
                    }
                    schema.push(("enum", J::Arr(vec![$(J::str($s)),*])));
                    let values = vec![$(($s, $crate::json::doc_text(&[$($vdoc),*]))),*];
                    defs.define(name, J::obj(schema), $crate::json::TypeDoc::Enum { doc, values });
                }
                $crate::json::reference(name)
            }
            fn label() -> String {
                let name = stringify!($name);
                format!("[{name}](#{})", name.to_lowercase())
            }
            fn read(
                v: &serde_json::Value,
                at: &$crate::json::Path,
                out: &mut Vec<$crate::json::Problem>,
            ) -> Option<Self> {
                let keys: &[&str] = &[$($s),*];
                match v.as_str() {
                    $(Some($s) => Some($name::$v),)*
                    Some(other) => {
                        out.push($crate::json::Problem::new(
                            at,
                            $crate::json::unknown("value", other, keys),
                        ));
                        None
                    }
                    None => {
                        out.push($crate::json::Problem::new(
                            at,
                            format!(
                                "expected one of {}, got {}",
                                keys.join(", "),
                                $crate::json::what(v)
                            ),
                        ));
                        None
                    }
                }
            }
            fn write(&self) -> $crate::json::J {
                $crate::json::J::str(self.key())
            }
        }
    };
}
