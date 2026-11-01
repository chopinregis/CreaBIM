# Feature brief 01: the maintainer's list, checked against the code

Written 2026-11-02 against main 904ad06. It merges the maintainer's list with what viewers of the demo videos asked
for.

**The guiding rule: do it better, not bigger.** CreaBIM shouldn't copy Revit's or ArchiCAD's bloat. Each feature below
follows three tests:
- **One general mechanism over many special tools.** One profile-sweep engine, not separate beam, railing, moulding and
  gutter tools.
- **Direct manipulation first.** Click it in 3D or 2D and drag it, with the numbers editable in Properties.
- **Nothing a tester can't find in ten seconds.** Every control has an icon that says what it is.

---

## 1. True orthographic and standard views

**What happens:** in Ortho the "Top" view still shows diagonals; it doesn't look straight down.

**Why (found in the code):**
- `crates/creabim-app/src/camera.rs`, `Look::angles` sets **`Look::Top => (-π/2, 1.55)`**. A pitch of 1.55 rad is about
  88.8°, not 90°.
- This is a common workaround: `look_at_rh` with a Z-up vector breaks when looking exactly down.
- The result is a slight tilt, so vertical edges show as short diagonals.
- After any orbit, the camera is also never quite back on an axis.

**Fix:**
- Top and Bottom at exactly ±90°, with the up vector switched to +Y for those two cases (the standard fix).
- Snap to the nearest face when the camera comes within about 2° of an axis view.
- Ortho plus an axis view should give pixel-straight elevations, to check against the section and elevation drawings.

**Size:** small. **Priority:** do first; the view cube needs it.

## 2. A view cube (navigation cube)

**What exists:** a "View" dropdown (Top, Front, Right, Back, Left, Iso) in the 3D view's toolbar. It has no on-canvas
orientation indicator.

**Approach:**
- A cube drawn in the corner of the 3D view with the same orbit as the camera, using the existing wgpu or egui painter.
- It has 26 hot zones: 6 faces, 12 edges and 8 corners. Clicking one animates to that view.
- The faces are labelled in the project's own words (TOP, FRONT/SOUTH, …), with a compass ring under it to spin about Z.
- Two small arrows rotate the view 90° about the screen axis, and a home button gives "Frame all".

**Better than the others:** the cube's labels take their names from the model's north, so a building drawn at an
angle shows FRONT on its real front, not on world −Y.

**Size:** medium (about 1 week). **Depends on:** item 1.

## 3. Images as references: trace, scale, place on any plane

**What exists:** no image import at all.
- `PlanItem::Image` and `geom::raster` are used only for shaded surfaces in sections.
- CreaBIM *writes* PDF (sheets) but cannot *read* one. There is no PDF renderer. This corrects an earlier note that said
  it "already renders PDFs".

**Approach: one element, `ImageRef`, for every use.**
- **Its data:**
  - **file:** stored in the project, so it travels with the `.creabim` file;
  - **plane:** a level, a section or elevation view, or any workplane, which already exists for sketches;
  - **placement:** corner, rotation, size;
  - **appearance:** opacity, crop rectangle, lock, layer or category, and a "show in" setting (this view, every view
    of this level, 3D).
- **Calibrate:** click two points on the image, then type the real distance, and the image scales. Optionally click a
  third point to set the angle, which straightens a crooked scan.
- **Trace:** snaps reach the image's corners and edges if wanted, or none, so tracing stays free.
- **PDF:** use `pdfium-render` with a bundled pdfium. Pick a page, rasterise it once at a chosen DPI, and keep the vector
  text for later.
  - Local and free: no cloud service, in keeping with the project's offline-first rule.
- **In 3D:** an image on a vertical workplane works as a façade or elevation reference, and on a level as a site plan.
- **Better than the others:**
  - It's one element with one Properties panel, not Revit's split between "Import image", "Decal" and "Link CAD".
  - Calibration is built into placing, instead of being a separate Scale command.

**Size:** large (2–3 weeks), split into milestones:
1. Images on levels, with calibration.
2. PDF pages.
3. Images on any plane and in 3D.

## 4. Quantity take-off, priced as you build

**What exists:**
- Schedules for walls, slabs and roofs already compute **Length, Area and Count**, and can **export CSV**
  (`schedule::to_csv`).
- Layered elements are listed by their core layer only.

**Approach:**
- **Quantities by layer:** each assembly layer yields an area (m²) or volume (m³). Concrete, block, insulation and
  plaster each get their own row. That's what a cost estimator needs.
- **Rates live on materials:** a rate per unit on each material, kept in an optional rate table per project. No supplier
  names: rates are the user's own.
- **A live "Take-off" pane:** totals by material and by level, updating as you model, as a schedule does now.
- **Export:**
  - **Excel:** `.xlsx` through `rust_xlsxwriter`, with one sheet per category and a summary, offline and free.
  - **CSV:** stays as it is.
- **Better than the others:** Revit needs a separate add-in for live cost. Here it's the same schedule engine plus a rate
  column.

**Size:** medium. The Excel export alone is small. **Priority:** high: pricing as you model is a frequent request.

## 5. Curved walls

**What exists:** walls are straight lines (start → end). Slab outlines can have arcs.

**Approach:**
- Give the wall an optional arc: three-point or start/end/radius. It reuses the slab's arc code.
- Layers offset as concentric arcs.
- Openings are placed by distance along the arc, and rendered flat or as a segmented chord.
- Joins to straight walls are handled through the existing join code.
- **The hard parts:**
  - plan cut and hatch of curved layers;
  - IFC export as `IfcWall` with an arc axis (supported by IFC4);
  - doors in curved walls (keep them flat to start, as Revit does).

**Size:** large (2–3 weeks).

## 6. Profiles and sweeps: one engine for beams, columns, railings and trims

**What exists:**
- Column and beam profiles are only `Rectangle` and `Round` (`creabim-core/src/members.rs`).
- Sketch → extrude exists (`sketch.rs`).
- There is no sweep along a path.

**Approach: a `Profile` library plus one `Sweep` element.**
- **Profiles are closed 2D shapes:** I, H, C, L, T, tubes, timber sections, or any user sketch saved as a profile.
- **A `Sweep` is a profile plus a path (the rail):**
  - the path can be a line, polyline, arc or spline;
  - it can be drawn in plan, in an elevation, or in 3D on any workplane;
  - options: profile rotation, the profile's justification point, and mitred or rounded corners.
- **Beams, columns, railings, gutters, cornices and trims all become sweeps** with a category.
  - Today's beam and column tools become presets of the same element, so the feature set shrinks while the power
    grows.
- **Direct editing:** grips on the path in 3D and 2D. Swapping the profile in Properties changes every sweep of that
  type.
- **IFC:** `IfcBeam` or `IfcColumn` with `IfcExtrudedAreaSolid`, or `IfcSweptDiskSolid` / `IfcFixedReferenceSweptAreaSolid`
  for curved rails.

**Size:** large, but it retires several special cases. **Priority:** high. It is also how "a balustrade, a gutter, a
cornice" get done without new tools for each.

## 7. Edge offset and snap in 3D (ArchiCAD's "pet palette")

Several viewers asked for this, ArchiCAD users among them.

**Approach:**
- Select an element in 3D and hover an edge of its outline (wall end, slab edge, roof contour).
- A small palette appears with three choices:
  - **offset edge:** moves the edge parallel;
  - **offset all:** offsets the whole outline;
  - **insert point.**
- Drag, with snapping to other elements' faces and edges in 3D, or type a distance.
- This builds on the existing grips (`handles.rs`, `editmode.rs`) and the push/pull code (`pushpull.rs`).

**Size:** medium. **Priority:** high. It's the most-requested modelling feature.

## 8. The roof tool's shape buttons

**What happens:**
- Flat, Mono-pitch, Gable and Hip all show the same "tent" icon in the top bar.
- That comes from `memberstool.rs`: `toggle(…, "tent", …)` for every `RoofShape`.

**Fix:**
- **Icons:** draw four distinct icons in `ui/icons.js`: a flat slab, a single slope, a gable (two slopes with an end
  triangle) and a hip (four slopes).
- **Layout:**
  - Group the four as one segmented control under a "Shape" label, with the pitch field beside it.
  - Hide the pitch field for Flat.
  - Add per-edge overhang (see the release list): the flat roof poked into the room above in the tour.
- **Audit the rest of the top bar:** the same one-icon-for-all pattern may exist in other tools' option groups. Every
  segmented choice needs icons that differ.

**Size:** small.

---

## Suggested order

1. **True ortho and view snapping** (small). Unblocks the view cube.
2. **Roof shape icons and grouping** (small). It's visible in every demo.
3. **Excel take-off export** (small). This is phase 1 of item 4.
4. **View cube** (medium).
5. **Edge offset and snap in 3D** (medium). The most-requested modelling feature.
6. **Image references, milestone 1:** levels and calibration (medium).
7. **Quantities by layer, with rates** (medium). This completes item 4.
8. **Profiles and sweeps** (large).
9. **Images, milestones 2–3:** PDF pages, then any plane (large).
10. **Curved walls** (large).

Items 1–3 are worth doing before the first public test build; the rest can follow it.
