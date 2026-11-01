# CreaBIM: what to fix before the first public test build

Found while scripting the tutorial and the three-part Terrace House tour (2026-10-29 to 31) on main 904ad06. Every item
here is something the films had to work around. The ranking is "what a first tester would hit in their first hour".
It's written to paste to the CreaBIM AI.

## A. Must fix: first-hour friction

### 1. Foundations

- **The default footing depth collides with the floor.** "Under selected" puts each strip footing's top at the wall
  base (0 mm). A ground slab sits at −300…0, so the two overlap, and their faces fight in 3D.
  - Default the top to the underside of the slab on that level, when there is one.
- **Strip footings go under every selected wall, partitions included.**
  - Skip walls typed as partitions and non-load-bearing walls, or ask.
- **Footings win picks in plan.** They are drawn dashed below the cut, yet a click on a door in a partition selected
  the footing (or the wall) instead of the door.
  - Elements in the cut should win over elements below it.

### 2. Wall joins and openings near joins

- **Corners of layered walls need layer-priority joins.** Currently the wall simply wins: the slab stops at the inside
  face.
  - Cores should join cores, finishes should stop at faces, with an override per junction (as Revit and ArchiCAD do).
- **T-junctions** between a 365 cavity wall and a 126 block or 100 stud partition need the partition to run into the
  core, not stop at the finish.
- **Widening an opening grows it from its start edge.** A window widened from 0.9 to 3.6 m ran into the corner join,
  and the film had to slide it back by hand.
  - Widen about the centre, or keep the edge nearest the corner fixed, and warn when an opening crosses a join.

### 3. Structure defaults

- **Beams default to their top at the storey top**, which is inside the floor slab above. Every beam had to be dropped
  300 mm by hand.
  - Default to the underside of the slab above.
- **A new roof leaves the walls below it unattached**, so gables stay open until "Attach top" is run.
  - Offer to attach the walls under a roof when it is drawn.
- **Roof overhang is one value for every edge.** A flat roof against a taller wall pokes its 500 mm overhang into the
  room above.
  - Allow overhang per edge, or zero automatically where the roof meets a wall.

### 4. Stairs

- **The stair and its well aren't linked.** The hole in the floor above is drawn by hand, and nothing checks it.
  - Offer "cut a well for this stair" on the level above, sized to the flight plus headroom.
- **Changing a level's height re-flows the stair** (more risers), so the flight can end under the slab edge with no
  warning.
  - Add a headroom check, and landing-length warnings (less than 900 mm).

### 5. Snapping, picking, selection

- **Underlay snapping prefers the faces of the walls below over their centrelines.** Walls traced over an underlay
  landed 100 mm out of line.
- **Resting the cursor on a point pins guide lines to it**, which stay until Esc twice. New users will find this
  noisy.
  - Make it opt-in, or show it more faintly.
- **Box selection takes everything inside the box**, grids and slabs included.
  - Add "filter selection by category".
- **A Properties edit applies only to the last element picked.**
  - Fixed on branch `claude/demo-player` (ca9d7b1): it now applies to every selected element of the same kind.
- **Ctrl+A inside a text field selects every element** in the model instead of the field's text.

### 6. Windows and panels

- The **Catalogue window** opens over the plan where you place things.
- The **Materials window** stays open after "Assign material".
- **Frame all also reframes the 2D view.** Surprising when you only meant the 3D one.
- The single-view tab remembers whether it last showed 3D or the plan, across sessions.

### 7. Feedback on export

- PDF and IFC export write next to the project file with no dialog, and only the status bar says where.
  - Ask where to save, or offer "Open the folder".

## B. Must have: release basics (not features)

- **A Windows build people can run:** an installer or a portable zip, signed if possible, plus a short "first five
  minutes" page.
- **A file format version** in `.creabim` files, and a promise about opening older files.
- **Known limitations** stated plainly in the README, and "not for real projects".
- **Issue templates** (bug, feature, "I'm a tester"), and how to send a crash or log.
- **Sample projects:** the starter house, and the Terrace House from the tour (the scripts save `.creabim` files).
- **A CONTRIBUTING guide:** how to build, test and send a PR. The workspace already has a strong test suite.

## C. Nice to have for the first build

- Per-edge roof overhang, and a stair-well tool (see A).
- A "check model" panel: openings crossing joins, beams inside slabs, footings inside slabs, missing wells.
- PDF import as an underlay, with scale calibration: the most requested feature in comparable tools.
