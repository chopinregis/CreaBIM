# View range

A plan's **view range** says which levels it draws. Set it under *View range* in the plan's *Properties*.

- *Its level*: only the elements of the plan's own level, cut at its *Cut height*. This is how plans drew before view ranges.
- *Levels in range*: the elements of every level between the *Bottom* and the *Top*.
- *From above*: a roof or site plan, looking straight down with nothing cut.

## Levels in range

- **Bottom**: a level and an offset. Levels from here up to the cut plane are drawn as plans of their own: cut where the cut plane passes through them, in projection where they lie below it. A site level 0.45 m under the floor, with its boundary walls and gatehouse, shows in the floor plan with its walls cut.
- **Top**: levels starting above it are left out. *Next level up* by default.
- **View depth**: levels between it and the bottom are drawn lighter, in the *beyond* style.
- **Overhead dashed**: what lies above the cut, up to the top, is drawn in dashed lines: roof edges, an upper slab, a chimney cap.

Each height is a level and an offset from it, so it moves when the level moves.

## From above

- The plan looks down from the *Top* (above everything by default) to the *Bottom* (under everything by default).
- Nothing is cut. Each surface hides what is under it: upper roofs hide lower roofs, roofs hide the walls and rooms beneath them.
- Roofs show their slope arrows. Grids, section marks, dimensions and notes show as in any plan.
- Its template can add sun shadows and distance fading, see [Graphic styles](graphic-styles#depth-cues).

With the *Light linework* pack, plans from above take the *Roof plan* template.
