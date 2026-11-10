# CreaBIM roadmap

What is planned for CreaBIM, in rough order. [KNOWN_GAPS.md](KNOWN_GAPS.md) lists what doesn't work yet; this page lists what is coming. Plans change with feedback: open a Discussion or an issue to argue for something.

## Principles

- **Free, open source and offline:** CreaBIM runs on your computer, with no account, no subscription and no paid services.
- **One model, every drawing:** plans, sections, elevations, sheets, schedules, quantities and IFC all come from the same model.
- **Open formats first:** IFC in and out; DXF, glTF and FreeCAD exchange.

## First public release

- Downloadable builds on GitHub Releases: Windows (portable and installer) and Linux, with checksums.
- A project website with downloads and an introduction film.

## Next

- A rewritten help guide with new screenshots.
- **Parametric and freeform profiles:** standard sections (I, channel, tube, angle, rectangle) driven by their dimensions, alongside profiles you draw.
- **Direct 3D modelling:** inference markers, typed distances, push/pull, follow-me, copy while moving, and a context toolbar at the selection. A setting chooses the modelling style.
- **Stairs:** more shapes, railing styles, and a check against stair rules.
- **Components from sketches:** windows, doors and objects from drawn outlines.

## Later

- **Renovation and phasing:** existing, demolished and new elements, with views and schedules by status.
- **Site:** survey coordinates, terrain from surveys, roads, paths and building pads.
- **DWG** import and export, through an open-source library.
- **3D annotations and sketching:** dimensions and notes in 3D, and sketch layers.
- **Rendering and studies:** textures; sun, sky and location settings; shadow and sunlight studies.
- **Parametric trees:** light trees that read well in plan and section.
- **Hatch patterns:** a parametric hatch editor.
- **Walk mode:** a first-person walk through the model.
- **Fit-out:** kitchen and bathroom runs, and detailed wall build-ups.

## Being considered

- A browser version (WebAssembly), still working on local files.
