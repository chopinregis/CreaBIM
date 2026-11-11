# Columns and beams

## Columns

1. Pick the **Column** tool.
2. In the tool options choose *Square* or *Round* and the *Size*, a profile from the [library](profiles) (an I, a T, a channel, your own), or a column **Type**.
3. Click where each column goes.

A column runs from its level up to the next one, unless you tie its top to a level or give it a fixed height in *Properties*. *Properties* also has its rotation and its **Profile**: *Its own (width × depth)*, or any profile of the [library](profiles), such as an I-section or a tube. Drag its centre handle to move it, its top handle to change its height and its base handle to change its base offset.

## Over several levels

A column's *Properties* has four rows that say where it starts and stops:

- **Base level** and **Base offset**: the level it stands on, and how far above that level its foot is.
- **Top level** and **Top offset**: the level its top is tied to, and how far above that level the top is. *Unconnected* means a fixed height, or up to the next level.

When the top is tied to a level, the column stretches when either level moves.

For example, say a column was drawn on Level 5 and needs to stand on Level 3 and run up to Level 5:

1. Select it and set its *Top level* to Level 5.
2. Set its *Base level* to Level 3. The top stays where it is, so it now runs through Level 4.

Changing the base level always keeps the top where it is. A column that ran up to the next level stays tied to that level.

In a section or elevation, drag its top or base handle up or down. When the top is tied to a level, dragging it changes the *Top offset*.

A column that runs through a level is drawn cut in that level's plan too.

Copying a column to other levels with **Copy to levels…** keeps its span in storeys. See [Levels](levels#copying-and-moving-to-other-levels).

## Beams

1. Pick the **Beam** tool. Set *Width* and *Depth* in the tool options, pick a **Profile** from the [library](profiles) (an I, a T, an L, your own), or a beam **Type**.
2. Click the beam's start, then its end. Keep clicking to chain beams. `Esc` stops, and picking another tool (or Beam again) starts a new chain.

By default a beam's top sits at the top of the storey (*Top offset* 0), hanging under the next level. Its handles move each end and its top.

### Beams on a face

Switch on **On face** in the tool options to draw in 3D on a face: a roof slope, a wall. The first click picks the face, and the beam lies in its plane, square to it and sitting on its surface, at any angle you draw. The next points stay on that face, so purlins, rafters and braces go where they belong. A beam drawn on a roof goes on the level under it. *Stand plumb* in *Properties* puts its section upright again.

### Chains and corners

**Corners** in the tool options sets how the beams of a chain meet:

| Corners | Gives |
| --- | --- |
| **Separate** | Pieces of their own, as concrete beams are |
| **Mitred** | Ends cut on the line between the two, as welded steel |
| **Butt** | One runs through, the next stops against it |

### Slopes, extensions and ends

*Properties* has a **Start rise** and an **End rise**, to slope a beam (a rafter, a stair stringer), and a **Start extension** and **End extension** to run an end on past where it was drawn, or a minus value to pull it back. **Ends** is *Square*, *One mitred* or *Both mitred*; *Square ends* takes the mitre cuts off.

## In drawings and schedules

Cut columns are filled solid in plans; beams above the cut are dashed, and members seen from above take their material's colour. A sloped beam is cut and drawn by its full height. For footings under columns, see [Footings](footings). *File › New schedule › Columns and beams* lists every one with its type, level, material, section and length.

A beam inside a wall is hidden in most drawings: use [X-ray](visibility#x-ray) or *Hidden* in a view to see it, and `Tab` to [pick it](select#picking-what-is-underneath).
