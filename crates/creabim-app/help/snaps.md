# Snaps

While you draw or move, the pointer **snaps** to exact points on the model, so walls meet and things line up without guessing. A marker shows the kind of snap and a label names it, such as *Endpoint* or *Midpoint*.

## Kinds of snaps

Pick which are on in the list next to **Snap** in the status bar. Each can be ticked on its own; *Turn all on* and *Turn all off* set them all.

- **Endpoint**: the ends and corners of walls, slabs, lines.
- **Midpoint**: the middle of an edge.
- **Intersection**: where two edges cross.
- **Perpendicular**: square to an edge from the last point.
- **Nearest**: any point on an edge.
- **Centre** and **Quadrant**: circles and arcs.
- **Grid and level lines**.
- **Guides**: [guide lines](guides).
- **Axis tracking**: in line with the last point along X or Y.
- **Alignment with the shape's points**: level with or plumb under the corners you have clicked, with dashed lines.
- **Divisions**: every edge at its even parts: 3, 4, 5, 6, 8 or 10 (*Divisions* in the list).
- **Reference lines**: the ends, middles and lines of a [trace reference](trace-reference), in plans, sections and 3D.
- **On face** (3D): a grip sliding along a line stops on the face under the pointer, with a small blue square.

Snapping is **free** by default: points and drags go exactly where the pointer is, and the snaps catch them when the pointer comes near something.

## Switching snaps off

- Click **Snap** in the status bar, or press `F3`, to turn all snapping off or on.
- Hold `Alt` while clicking to suspend snapping for that click.

## Grid snap and distance marks

**Grid snap** (`Alt+S`, or its menu in the status bar) is off whenever the app starts. On, points land on its grid (10, 50 or 100 mm, or any spacing you type), and lengths along axes and guides round to it. *Distance marks*, in the same menu, show faint marks every spacing along a drag, and hold the drag when it comes within 10 px of one. See [Handles and grips](handles).

## Snapping to other levels

A plan's [underlay](levels#underlay) shows another level faintly, and its lines can be snapped to.
