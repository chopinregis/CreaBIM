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

On the plan's own level, a part that lies wholly above the cut, such as the coping on a low screen wall, is always drawn dashed as overhead.

Each height is a level and an offset from it, so it moves when the level moves.

A view depth can't show what the view hides: if a plan hides its footings, a note under the view depth says so, with a **Show footings** button.

## Plan ranges in sections

Turn on **Plan ranges** in a section's or elevation's header: each floor plan's cut shows as a solid line and its view depth as a dashed one, in a band as wide as the drawing. They stay quiet until you point at one. Drag the grip at a band's end to change the cut height or view depth, or click a cut line to type its height.

## Section box

Under *View range › Section box*, a plan cuts the 3D view as it sees it: **Below the cut** keeps what is below the plan's cut, **Above (ceilings)** keeps what is above it, to look up at the ceilings. A section's *Section box › In 3D* cuts the 3D view from its line to its depth. Each is a [crop box](crop-boxes) of the 3D view, updated when you ask again.

## From above

- The plan looks down from the *Top* (above everything by default) to the *Bottom* (under everything by default).
- Nothing is cut. Each surface hides what is under it: upper roofs hide lower roofs, roofs hide the walls and rooms beneath them.
- Roofs show their slope arrows. Grids, section marks, dimensions and notes show as in any plan.
- Its template can add sun shadows and distance fading, see [Graphic styles](graphic-styles#depth-cues).

With the *Light linework* pack, plans from above take the *Roof plan* template.
