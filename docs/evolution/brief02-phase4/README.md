# Brief 02, Phase 4: heavy modelling

Reference screenshots for Phase 4 of *CreaBIM brief 02: consolidated* (2026-11-04), as the owner amended it on 2026-11-05. The items come one at a time, in this order:

1. layer editing in plan, section, 3D and any other useful view, such as callouts;
2. drawing on any face, with align, mirror, offset, move, rotate and the other manipulations there;
3. edge offset in 3D and 2D;
4. take-off;
5. image and PDF references;
6. profiles;
7. curved walls.

Items move from [docs/KNOWN_GAPS.md](../../KNOWN_GAPS.md#brief-02-2026-11-04) to its *Recently fixed* list as they land.

The screenshots were taken in the running app on a small L corner cut out of the starter house: two 400 mm walls of *Exterior wall 400, rendered* (render 20, mineral wool 150, block 215, plaster 15), 1.5 m each, with the plan and the 3D view side by side.

## Item by item

| Item | Part | Status |
| --- | --- | --- |
| 1 (3.7) | Wall layer ends: grips in Layers, drag or type, a dot on edited ends, reset one or all, kept through move, stretch and retype | **Done** (below). |
| 1 (3.7) | Slab and roof layer edges: a grip on each edge of each layer, drag or type, reset | **Done** (below). |
| 1 (3.7) | Corner join presets: Auto, Cores lap and finishes stop, This or the other wall runs through, Mitre every layer, Not joined | **Done** (below). |
| 1 (3.7) | *Draw by this line* on any layer boundary, without the wall moving | **Done** (below). |
| 1 (3.7) | Check model warnings for gaps and overlaps; IFC parts where layers differ | **Done** (below). |
| 1 (3.7) | Acceptance: an L corner, core 20 mm past, finishes stopping, in plan, section and 3D, the IFC in a second reader, moved with the edit kept | **Done** (below). |
| 2 (3.5) | Extrusion and void drawn on any face, keeping their plane; a void on a sloped roof cuts square to it | **Done** (below). |
| 2 (3.5) | Sweep path drawn on a face, its profile standing out of the face | **Done** (below). |
| 2 (3.5) | Slab on a level face; wall opening drawn on a wall face | **Done** (below). |
| 2 (3.5) | Room outline and detail region on a face | Not started: a detail region is drawn in its section or detail view already, and a room follows its walls. |
| 2 | Align, mirror, offset, move, rotate on the face | **Done** (below): new Mirror and Offset commands, in plan and on a face; Move, Rotate and Align follow a face workplane; push/pull on an extrusion drawn on a face. |
| 3–7 | | Not started. |

## 1. Layer editing

### Wall layer ends

Select a layered wall, press `Enter` for Edit mode, then `Tab`: **Layers**. Its own grips give way to one grip at each end of each layer, where its join puts that end. `Tab` again goes back to the wall's grips, and leaving Edit mode leaves Layers too.

![Layers grips in plan and 3D](01-layers-grips.png)

The grip under the pointer names its layer and end ("Block, concrete: end"), and once moved, by how much.

![The grip names its layer](02-layer-grip-name.png)

Drag a grip along the wall to run the layer on past its join, or to hold it back. While dragging, `Tab` (or a digit) types the distance, as with every grip; `Enter` applies it.

![Typing 20 mm](03-typed-run-on.png)

After the drag, the moved end's grip is filled. Here the block runs 20 mm past the wall's free end, and the plaster is held back 50 mm from the corner. Plans, sections, callouts and 3D all draw the layers from the same pieces, so they all follow:

![Plan and 3D after](04-after-plan-3d.png)

![3D grips](05-3d-grips.png)

![Section](06-section.png)

![Callout](07-callout.png)

**How it is kept.** Each edit is an offset from where the join puts that layer's end, stored on the wall (`layer_ends` in the file, left out when there are none). So it stays through a move, a stretch, a slide, and a change of the other wall. Changing the wall's type keeps the edits of layers the new type also has; the others are dropped, and the status bar says how many.

![Slid 300 mm afterwards](08-moved-kept.png)

**Reset.** Right-click a layer grip: *Reset layer end* puts that end back on its join, *Reset all layers* every end of the wall. Each is one undo step.

### Slab and roof layer edges

On a layered slab or roof, `Tab` in Edit mode shows a grip on each edge of each layer, at the middle of the edge and of the layer's depth. Drag one square to the edge, or type how far, to push that layer out past the element's edge or pull it in: a slate run on past the fascia, insulation stopped short of a slab edge. A roof's edges are its eaves and verges, and a pushed-out layer runs on down its own slope. In plan, where a slab's layers sit on one another, each layer's grip is drawn a step further out; in sections and 3D each is at its own depth.

![Roof layer grips](09-roof-layer-grips.png)

![Before](10-roof-eave-before.png)

![After](11-roof-eave-after.png)

Each edit is kept on the slab or roof (`layer_edges` in the file) as a distance square to that edge, so it moves with the edge when the outline is reshaped. *Reset layer edge* and *Reset all layers* are in the grip's right-click menu, and a change of type drops the edits of layers the new type lacks, as for walls.

**Gaps still open** (in [KNOWN_GAPS](../../KNOWN_GAPS.md)):

- A slab's arc edge is moved by its corners, so its bulge keeps its ratio rather than staying parallel.
- Adding or removing a corner of the outline renumbers its edges, and edits on edges after it move to the next edge.

- A layer run on by hand into the wall it meets overlaps that wall's layers, and the plan draws one over the other.
- While a grip is dragged, the preview is drawn without the joins of the walls around it.
- Grips of thin layers sit close together in plan at small scales: zoom in, or use the 3D view, where they are stepped up the end face.

### Corner join presets

Where a wall's end meets one other wall at a corner, right-click the wall near that end and pick *Corner join*, or use the row for that end at the top of *Properties › Junctions*. *Cores lap, finishes stop* is the corner the brief asks for: the finishes meet on the mitre, the stronger wall's core runs on to the far face of the other's core, and the other's core stops square against it. The preset is kept with the two walls, so it stays when either moves, and layer ends moved by hand are offsets from where the preset puts them.

![Corner join menu](12-corner-menu.png)

![Each preset](13-corner-presets.png)

### Draw by this line

Any line between two layers can be the wall's location line: the line it is drawn and dimensioned by, and that stays put when the wall gets thicker or its layers change. In Layers, right-click a layer's end grip and pick *Draw by its exterior face* or *Draw by its interior face*. *Properties › Location line* also lists every boundary by the layers it lies between. Only the location line changes: the wall does not move. The choice is saved in the file (`"location": "layer-boundary"` with `"boundary"`, counted from the exterior face) and written to IFC as the location line `LayerBoundary`.

![Draw by menu](14-draw-by-menu.png)

![Drawn by the line](15-drawn-by-boundary.png)

![Location line choices](16-location-boundaries.png)

### Check layer joins

*Create › Check layer joins* lists the walls with a layer moved by hand at an end where the wall meets another: held back, it leaves a gap; run on, it overlaps the wall it meets. It selects them and names each layer in the status bar. A layer stopped short or run on at an end that meets nothing is left alone. `creabim check <model>` prints the same lines and fails when there are any.

![Check layer joins](17-check-layers-menu.png)

![What it found](18-check-layers-result.png)

### IFC parts

Where an element's layers are not all as its layer set puts them (a layer end or edge moved by hand, or cores lapped at a corner), the IFC export also writes each layer as an `IfcBuildingElementPart`: its own solid, named after the element and the layer, with the layer's material, aggregated by the wall, slab or roof. The element keeps its body and its layer set, for software that reads only those. An untouched model has no parts. `tools/validate_ifc.py` (IfcOpenShell) passes on the export, and builds every part's geometry.

The acceptance corner, exported and drawn from IfcOpenShell's own geometry (a second reader, not CreaBIM): the cores lap at the corner, the plaster stops 50 mm short of it, and the block runs 20 mm past the free end.

![IFC parts](19-ifc-parts.png)

Reading the file back into CreaBIM takes each wall's layers from its layer set and skips its parts, so they don't come in twice; the edits they show are not read back yet (in KNOWN_GAPS).

### Acceptance

| Asked | Where |
| --- | --- |
| An L corner, the core 20 mm past, the finishes stopping | 03, 04, 13 (*Cores lap, finishes stop*), 19 |
| Shown in plan, section and 3D | 04, 05, 06, 07 |
| The IFC opened in a second reader | 19 (IfcOpenShell) |
| The wall moved afterwards, the edit kept | 08 |

## 2. Drawing on any face

In the 3D view, the Extrusion, Void, Sweep, Slab and Wall opening tools now draw on the face under the pointer. The first click starts on that face (or on the workplane, if one is set), and later corners stay on the same plane. Snaps only count when they lie on that plane. Before this change, a snap could jump to a corner hidden behind the face.

An extrusion or void drawn this way keeps its plane: `"plane"` in the file, with `base` and `height` measured along the plane's normal. A void cuts square into the face, as deep as *Depth on a face* (default 500 mm). An extrusion stands square out of the face.

![Void on the roof](20-void-on-roof.png)

![Close up](21-void-close.png)

A sweep drawn on a face stores the face's normal with its path (`"face_normal"` in the file). The profile's height runs along that normal, so a band can run up a wall and across it.

![Sweep on a wall face](22-sweep-on-wall.png)

A slab can only go on a level face, and its top sits on that face. A wall opening is drawn on a wall face once the workplane is on that face. It becomes the rectangle around the outline, measured along the wall and up from its base. Without a workplane, a click places an opening as before.

![Opening on a wall face](23-opening-on-wall.png)

Edit mode moves an extrusion's or sweep's corners on its own plane. Move and rotate take the plane with them, and a scale scales the outline on it.

### Mirror, offset, move, rotate and align on the face

Two new commands sit in the Edit menu, the right-click menu and the modify bar: **Mirror** and **Offset**. Both make a copy by default; Ctrl turns the copy off, so the original changes instead.

- **Mirror**: click two points for the mirror line. With a workplane on a face, the line lies on that face and the selection flips across it, so a part on a wall still stands out of the wall afterwards. Shift keeps the line along the plane's axes, Ctrl snaps it to 15° steps, and a typed angle sets it. In plan, it mirrors across a line in the level. Walls keep their outside out, and columns and objects are turned to match.
- **Offset**: the side of the pointer and its distance from the original set the offset; type a distance and press Enter, and Ctrl snaps to 50 mm. Walls, beams, grids, room separators, strip footings, railings, slabs, roofs, ceilings, shafts and extrusions offset in plan; a sketch, an extrusion drawn on a face and a sweep offset on their own plane. A closed outline grows or shrinks; a line moves sideways.
- **Move** with a workplane on a face slides along the face. **Rotate** turns about the face's normal, by the pointer or by typed degrees.
- **Align** with a workplane on a face turns the part on that face, so its edge runs parallel to the reference edge.
- **Push/pull** now works on an extrusion drawn on a face: its outer face makes it deeper or shallower square to the face, and a side face makes it wider.

![Mirror on a face](24-mirror-on-face.png)

![Mirrored copy](25-mirror-placed.png)

![Rotate on a face](26-rotate-on-face.png)

![Push/pull on a face](27-pushpull-on-face.png)

![Offset preview](28-offset-preview.png)

![Offset placed](29-offset-placed.png)

### Acceptance

| Check | Result |
| --- | --- |
| A part drawn on the south wall face, mirrored across a line on the face, copy stands out of the wall | Pass (24, 25; test `a_part_on_a_face_mirrors_square_to_it_and_still_stands_out_of_it`). |
| A wall mirrored across a slanted line keeps its outside out | Pass (test). |
| Rotate 30° on the face workplane turns about the face normal | Pass (26; test). |
| Push/pull of a part on a sloped face gets deeper square to it | Pass (27; test). |
| Offset a slab by a typed 600 mm in plan | Pass (28, 29; test with a typed "1.5m"). |
| Align a part on a wall face to a sloped edge | Pass (test `on_a_wall_face_workplane_a_part_turns_on_it_to_a_sloped_edge`). |

**Gaps still open** (in [KNOWN_GAPS](../../KNOWN_GAPS.md)):

- Room outline and detail region on a face.
- The plan's 2D tools still draw in the level's plane, as before.
- An object (furniture, fixture) mirrored keeps its own shape: only its position and facing are mirrored. An asymmetric column is not mirrored either, only turned.
- Mirror and Offset reuse two existing icons for now; new icons wait for the owner's OK.
- Thin grey slivers can show in the 3D view while a workplane sits on a face.
