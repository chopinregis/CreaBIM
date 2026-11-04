//! The open JSON format: the example builds, the export round-trips, the
//! schema and the docs match the format, and mistakes are reported where
//! they are.

use creabim_core::{ElementKind, Model};
use std::path::PathBuf;

fn repo(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

fn read(path: &str) -> String {
    std::fs::read_to_string(repo(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn build(text: &str) -> Model {
    let spec = creabim_spec::parse(text).unwrap_or_else(|e| panic!("{e:#?}"));
    match creabim_spec::build(&spec) {
        Ok(b) => b.model,
        Err(e) => panic!("{e:#?}"),
    }
}

/// The problems a file has, as "path: message" lines.
fn problems(text: &str) -> Vec<String> {
    let r = creabim_spec::parse(text).and_then(|s| creabim_spec::build(&s).map(|_| ()));
    match r {
        Ok(()) => vec![],
        Err(e) => e.iter().map(|p| p.to_string()).collect(),
    }
}

fn count(m: &Model, f: impl Fn(&ElementKind) -> bool) -> usize {
    m.iter().filter(|e| f(&e.kind)).count()
}

/// Export, build what was exported, export again: the same text.
fn round_trips(m: &Model) -> String {
    let first = creabim_spec::to_text(&creabim_spec::export(m).spec);
    let again = build(&first);
    let second = creabim_spec::to_text(&creabim_spec::export(&again).spec);
    assert_eq!(first, second, "export → build → export changed the file");
    first
}

#[test]
fn the_example_builds_a_small_house_with_sheets() {
    let m = build(&read("examples/small-house.json"));
    assert_eq!(count(&m, |k| matches!(k, ElementKind::Level(_))), 3);
    assert_eq!(count(&m, |k| matches!(k, ElementKind::Wall(_))), 7);
    assert_eq!(count(&m, |k| matches!(k, ElementKind::Opening(_))), 9);
    assert_eq!(count(&m, |k| matches!(k, ElementKind::Object(_))), 9);
    assert_eq!(count(&m, |k| matches!(k, ElementKind::Site(_))), 5);
    assert_eq!(count(&m, |k| matches!(k, ElementKind::Part(_))), 2);
    let sheets = creabim_geom::sheet::sheets(&m);
    assert_eq!(sheets.len(), 3);
    assert_eq!(count(&m, |k| matches!(k, ElementKind::Viewport(_))), 9);
    // the exterior walls are of the layered type, as thick as its layers,
    // and run up to the roof level
    let south = m.iter().find(|e| e.name == "South wall").unwrap();
    let ElementKind::Wall(w) = &south.kind else {
        panic!()
    };
    assert!((w.thickness - 0.25).abs() < 1e-9);
    assert!((w.height - 2.7).abs() < 1e-9);
    assert!(south.of_type.is_some());
    // nothing stands in a doorway, and every sheet draws
    assert!(creabim_core::checks::objects_in_openings(&m).is_empty());
    let pdf = creabim_geom::sheet::sheets_pdf(
        &m,
        &creabim_geom::sheet::SheetInfo {
            project: "Small house".into(),
            date: "2026-11-04".into(),
        },
    );
    assert!(pdf.starts_with(b"%PDF"));
}

#[test]
fn the_example_round_trips_through_export_json() {
    let m = build(&read("examples/small-house.json"));
    let text = round_trips(&m);
    // the export names elements by what they were called
    assert!(text.contains("\"id\": \"South wall\""));
    assert!(text.contains("\"wall\": \"Spine wall\""));
    assert!(text.contains("\"catalogue\": \"starter/sofa-3\""));
    let x = creabim_spec::export(&m);
    assert!(x.skipped.is_empty(), "{:?}", x.skipped);
    // and the rebuilt model has as much in it
    let again = build(&text);
    assert_eq!(again.len(), m.len());
}

#[test]
fn the_sample_houses_round_trip_for_what_the_format_covers() {
    for (name, m) in [
        ("starter", creabim_core::starter::starter_house()),
        ("typed", creabim_core::sample::typed_house()),
        ("layered", creabim_core::sample::layered_house()),
        ("furnished", creabim_core::sample::furnished_house()),
    ] {
        let x = creabim_spec::export(&m);
        let text = round_trips(&m);
        let again = build(&text);
        // the walls, doors and windows the format covers all come back
        for kind in ["Wall", "Opening", "Slab", "Roof", "Level", "View", "Sheet"] {
            let n = |m: &Model| {
                m.iter()
                    .filter(|e| e.kind.name() == kind)
                    .filter(|e| !matches!(&e.kind, ElementKind::Wall(w) if w.curtain.is_some()))
                    .count()
            };
            let skipped_hosts = x.skipped.iter().any(|s| s.contains("curtain"));
            if kind == "Opening" && skipped_hosts {
                continue;
            }
            assert_eq!(n(&again), n(&m), "{name}: {kind}");
        }
    }
}

#[test]
fn the_schema_file_is_the_one_the_format_makes() {
    let made = creabim_spec::schema().pretty();
    let path = repo("docs/creabim-model.schema.json");
    if std::env::var_os("CREABIM_BLESS").is_some() {
        std::fs::write(&path, &made).unwrap();
    }
    let file = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        file == made,
        "docs/creabim-model.schema.json is out of date: run \
         `CREABIM_BLESS=1 cargo test -p creabim-spec` to rewrite it"
    );
    // and it is JSON that describes every list of the format
    let v: serde_json::Value = serde_json::from_str(&file).unwrap();
    let props = &v["$defs"]["Model"]["properties"];
    for key in ["walls", "openings", "views", "sheets", "parts", "site"] {
        assert!(props.get(key).is_some(), "{key}");
    }
}

const START: &str =
    "<!-- reference:start (made by `CREABIM_BLESS=1 cargo test -p creabim-spec`) -->";
const END: &str = "<!-- reference:end -->";

#[test]
fn the_docs_reference_is_the_one_the_format_makes() {
    let path = repo("docs/json-format.md");
    let doc = std::fs::read_to_string(&path).unwrap();
    let (Some(a), Some(b)) = (doc.find(START), doc.find(END)) else {
        panic!("docs/json-format.md has no reference markers");
    };
    let made = format!("{START}\n\n{}{END}", creabim_spec::reference());
    let now = &doc[a..b + END.len()];
    if std::env::var_os("CREABIM_BLESS").is_some() && now != made {
        std::fs::write(&path, doc.replace(now, &made)).unwrap();
        return;
    }
    assert!(
        now == made,
        "the reference in docs/json-format.md is out of date: run \
         `CREABIM_BLESS=1 cargo test -p creabim-spec` to rewrite it"
    );
}

#[test]
fn the_example_in_the_docs_builds() {
    let doc = read("docs/json-format.md");
    let start = doc.find("```json\n").expect("a json example") + 8;
    let end = start + doc[start..].find("```").unwrap();
    let m = build(&doc[start..end]);
    assert!(count(&m, |k| matches!(k, ElementKind::Sheet(_))) >= 1);
    assert!(count(&m, |k| matches!(k, ElementKind::Opening(_))) >= 2);
}

const SMALL: &str = r#"{
  "version": 1,
  "levels": [{ "id": "L0", "elevation": 0 }],
  "walls": [
    { "id": "W1", "level": "L0", "start": [0, 0], "end": [5, 0], "thickness": 0.2 }
  ],
  "openings": [
    { "id": "D1", "kind": "door", "wall": "W1", "offset": 1, "width": 0.9, "height": 2.1 }
  ]
}"#;

#[test]
fn a_small_file_builds() {
    let m = build(SMALL);
    assert_eq!(m.len(), 3);
}

#[test]
fn unknown_fields_are_errors_with_their_path_and_a_suggestion() {
    let p = problems(&SMALL.replace("\"thickness\"", "\"thikness\""));
    assert!(
        p.iter().any(
            |p| p == "$.walls[0].thikness: unknown field `thikness`: did you mean `thickness`?"
        ),
        "{p:?}"
    );
    let p = problems(&SMALL.replace("\"door\"", "\"dor\""));
    assert!(
        p.iter()
            .any(|p| p.starts_with("$.openings[0].kind: unknown value `dor`: did you mean `door`")),
        "{p:?}"
    );
}

#[test]
fn wrong_types_and_missing_fields_say_where() {
    let p = problems(&SMALL.replace("[5, 0]", "[5, \"0\"]"));
    assert_eq!(p, ["$.walls[0].end[1]: expected a number, got \"0\""]);
    let p = problems(&SMALL.replace("\"start\": [0, 0], ", ""));
    assert_eq!(p, ["$.walls[0]: missing field `start`"]);
    let p = problems(&SMALL.replace("\"version\": 1", "\"version\": 7"));
    assert!(p[0].contains("version 7"), "{p:?}");
    let p = problems("{\"version\": 1, \"walls\": {}}");
    assert_eq!(p, ["$.walls: expected an array, got an object"]);
}

#[test]
fn references_are_checked_by_kind() {
    let p = problems(&SMALL.replace("\"wall\": \"W1\"", "\"wall\": \"W2\""));
    assert_eq!(
        p,
        ["$.openings[0].wall: there is no wall with the id \"W2\"; did you mean \"W1\"?"]
    );
    let p = problems(&SMALL.replace("\"wall\": \"W1\"", "\"wall\": \"L0\""));
    assert_eq!(p, ["$.openings[0].wall: \"L0\" is a level, not a wall"]);
    let p = problems(&SMALL.replace("\"id\": \"D1\"", "\"id\": \"W1\""));
    assert_eq!(
        p,
        ["$.openings[0].id: the id \"W1\" is already used by $.walls[0]"]
    );
    // a mistake in a wall isn't repeated by the doors in it
    let p = problems(&SMALL.replace("\"level\": \"L0\"", "\"level\": \"L9\""));
    assert_eq!(p.len(), 1, "{p:?}");
}

#[test]
fn elements_that_dont_fit_are_reported() {
    let p = problems(&SMALL.replace("\"offset\": 1", "\"offset\": 4.5"));
    assert_eq!(p.len(), 1);
    assert!(
        p[0].starts_with("$.openings[0]: it runs from 4.500 to 5.400 m"),
        "{p:?}"
    );
    let p = problems(&SMALL.replace(", \"thickness\": 0.2", ""));
    assert_eq!(
        p,
        ["$.walls[0]: a wall needs `thickness`, an `assembly` or a `type`"]
    );
    let p = problems(&SMALL.replace("\"thickness\": 0.2", "\"thickness\": -0.2"));
    assert_eq!(p, ["$.walls[0]: wall thickness must be positive, got -0.2"]);
}

#[test]
fn lengths_follow_the_units_field() {
    let mm = SMALL
        .replace("\"version\": 1", "\"version\": 1, \"units\": \"mm\"")
        .replace("[5, 0]", "[5000, 0]")
        .replace("\"thickness\": 0.2", "\"thickness\": 200")
        .replace("\"elevation\": 0", "\"elevation\": 0, \"height\": 3000")
        .replace(
            "\"offset\": 1, \"width\": 0.9, \"height\": 2.1",
            "\"offset\": 1000, \"width\": 900, \"height\": 2100",
        );
    let a = build(SMALL);
    let b = build(&mm);
    let text = |m: &Model| creabim_spec::to_text(&creabim_spec::export(m).spec);
    assert_eq!(text(&a), text(&b));
}

#[test]
fn fields_a_kind_does_not_use_are_errors() {
    let text = r#"{ "version": 1, "levels": [{ "id": "L0", "elevation": 0 }],
        "views": [{ "id": "S", "kind": "section", "level": "L0",
                    "start": [0, 0], "end": [1, 0], "depth": 5 }] }"#;
    assert_eq!(
        problems(text),
        ["$.views[0].level: `level` is not used by a section view"]
    );
}
