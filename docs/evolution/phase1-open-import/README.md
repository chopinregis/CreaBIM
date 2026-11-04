# Phase 1: an open way in (JSON model format)

CreaBIM can now build a whole model from a plain JSON file that any script can write, without compiling against creabim-core. It can also export a project back to that format. The format is documented in [`docs/json-format.md`](../../json-format.md) and has a JSON Schema, [`docs/creabim-model.schema.json`](../../creabim-model.schema.json). The example used in these shots is [`examples/small-house.json`](../../../examples/small-house.json).

## Screenshots

| Shot | What it shows |
| --- | --- |
| `01-small-house-opened.png` | `examples/small-house.json` built with `creabim build` and opened in the app. It has three levels (site at −0.45, ground floor and roof), a grid, layered exterior walls tied to the roof level, partitions, doors and windows, a hip roof, a porch with oak posts, a beam, a flat canopy and concrete steps with a railing, a brick chimney, a garden wall with a swept coping, terrain, a gravel path and trees, furniture from the starter catalogue, rooms, tags and a note. |
| `02-wall-type-from-json.png` | The spine wall selected. The file names its type (`"type": "Partition"`), so in the app it is a real instance of the *Partition* wall type: 2 instances, everything from its type, and editable like a wall drawn by hand. |
| `03-sheet-plans.png` | Sheet A101 in the app, with its two viewports placed by the file: the ground floor plan at 1:50, cropped, and the site plan at 1:200. |
| `04-check-errors.png` | `creabim build --check` on files with mistakes. Each problem is given at its JSON path, with a suggestion when a name is close: an unknown field, a door naming a wall that doesn't exist, a window that runs past its wall's end, a catalogue key that is misspelt, and a field an elevation doesn't use. A file with problems builds nothing, and the command fails. |
| `05-sheet-a101-plans.png` | Sheet A101 *Plans* from `creabim pdf`: the plan with its door, window and room tags and the porch note, and the site plan. |
| `06-sheet-a201-section-elevations.png` | Sheet A201: Section A-A, the south and east elevations, all with their level heads. |
| `07-sheet-a301-3d-schedules.png` | Sheet A301: the 3D view, and the door, window and room schedules, read live from the built model. |

Shots 01 to 03 are of the app. Shot 04 is terminal output. Shots 05 to 07 are pages of the PDF the command line makes.

## How to try it

```sh
cargo run -p creabim-cli -- build examples/small-house.json small-house.creabim
cargo run -p creabim-cli -- build --check examples/small-house.json
cargo run -p creabim-cli -- pdf small-house.creabim small-house.pdf
cargo run -p creabim-cli -- export-json small-house.creabim small-house-again.json
cargo run -p creabim-cli -- json-schema
```

Then open `small-house.creabim` in the app (*File > Open…*).

## What was built

- **The format.** A versioned JSON format (`"version": 1`) describing materials (from the library or your own), assemblies with layers, element types, levels, grid groups and grids, walls, doors, windows and holes placed by wall and offset (or centre), slabs with holes and arcs, roofs, columns, beams, stairs, railings, parts (extrusions and sweeps, solid or void, including sweeps along a wall), site elements (terrain, hard landscape with patterns, plants), objects from the starter catalogue, rooms, views (plan, section, elevation, 3D, schedule, and callouts), sheets with viewports, tags and notes.
- **Ids you choose.** Elements refer to each other by user-chosen ids (a door names its wall, a plan its level, a viewport its view). References are checked by kind, and may point forward in the file.
- **Units.** Metres by default. A top-level `"units"` of `mm`, `cm`, `ft` or `in` scales every length in the file. Sheet positions are always paper millimetres.
- **Strict reading.** Unknown fields are errors, never ignored, with a "did you mean" suggestion. Type errors, missing fields, dangling or wrong-kind references, duplicate ids, openings that don't fit their wall, and fields a kind doesn't use are all reported at their JSON path. A mistake in one element doesn't cascade into messages about the elements that refer to it.
- **CLI.** `creabim build <spec.json> <out>` (to a `.creabim` project or a `.creabim.json` file), `creabim build --check <spec.json>`, `creabim export-json <project> <out.json>` and `creabim json-schema [out]`.
- **Export round-trips.** `export-json` names elements by their names (unique-ified), and writes typed elements' values only where they override their type. Building an exported file and exporting it again gives the same file. Anything the format can't describe is left out and listed. The Tropical Villa (REV00) exports 340 of its elements and round-trips. What it leaves out is 32 dimensions, 6 free meshes, some view settings and 9 disallowed wall joins.
- **One declaration, no drift.** Each object of the format is declared once, in `crates/creabim-spec/src/format.rs`, with its doc comments. The reader, the writer, the JSON Schema and the field reference in `docs/json-format.md` all come from that declaration. Tests fail if the committed schema or docs reference differ from what the declaration makes (`CREABIM_BLESS=1 cargo test -p creabim-spec` rewrites them).
- **Tests and CI.** The crate's tests do several checks. They build the example and check what it has. They round-trip the example, the starter, typed, layered and furnished houses through export, build and export again. They check that the example in the docs builds, and that each kind of error is reported with its path. CI builds the example and makes its PDF. It also exports the example to IFC and validates that in IfcOpenShell, round-trips the export with `cmp`, and validates the example and the export against the schema with Python's `jsonschema` (`tools/validate_model_json.py`).

## Known gaps

All open gaps, from every milestone, are kept up to date in [docs/KNOWN_GAPS.md](../../KNOWN_GAPS.md). The list below is as it was when this milestone landed; some of it may be fixed since.

- **Not yet in the format.** The following can't be described yet:
  - dimensions and spot levels
  - text styles and title block templates (sheets get the built-in title block)
  - mark rules
  - per-view visibility, graphics and underlays
  - level heads
  - crop boxes
  - curtain walls
  - footings, shafts, ceilings and room separators
  - sketches, guides and free meshes
  - booleans
  - join rules and detail components
  - objects that are not from the starter catalogue
  - disallowed wall joins

  `docs/json-format.md` keeps the list. `export-json` reports what it leaves out.
- **Shape problems first.** `--check` reports problems with the file's shape (unknown fields, wrong types, missing fields) first. Problems with references and geometry are reported only once the shape is right, so a file with both takes two runs to see everything.
- **Objects.** Only the starter catalogue can be named. A user catalogue folder can't be pointed to yet.
- **Wall lines.** Walls are always given by their centreline. The location line only says which line stays put when the wall changes in the app; it doesn't move the line the file gives.
- **Graphic styles and view ranges.** Elements and settings from work still in flight (graphic styles, view ranges) will join the format when they land. Their place is a new optional field or list, so version 1 files stay valid.
- **Help pages.** The app's help guide doesn't cover the format yet. It is a command-line feature, documented in `docs/json-format.md`.
