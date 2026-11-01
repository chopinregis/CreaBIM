# CreaBIM gaps found while recording the tour

These are things the Terrace House tour ran into in CreaBIM main (904ad06). Each says what happened, why it matters,
and what we did for the film. It's written to paste to the CreaBIM AI.

## Fixed on branch `claude/demo-player` (please review and merge)

1. **A Properties edit only changed the last element picked.**
   - **Problem:** with ten strip footings selected, setting "Top below wall base" changed one of them.
   - **Expected:** Revit and ArchiCAD apply the change to every selected element of the same kind.
   - **Fix:** commit ca9d7b1 does this. It leaves alone each element's own placement (ends, outline, host, offset along
     its wall, level).
2. **The view Properties' category list wrapped names mid-word** ("Wal ls", "Doo rs") when the panel was narrow.
   - **Fix:** commit d8b531a.

## Still open

3. **Strip footings overlap the floor slab.**
   - **Problem:** a footing put under a wall defaults to its top at the wall base (0 mm). The ground slab occupies
     −300…0, so the two overlap and their top faces flicker in 3D (z-fighting).
   - **Suggestion:** default the footing top to the underside of the floor slab on that level, or cut or merge
     coincident volumes in the 3D mesh.
4. **Picking in a plan prefers footings over doors.**
   - **Problem:** footings sit below the cut and are drawn dashed, yet a click on a door in a partition picked the
     strip footing (or the wall) instead of the door.
   - **Suggestion:** elements in the cut should win picks over elements shown below it.
5. **Snapping with an underlay pulls points onto the underlay's wall faces.**
   - **Problem:** walls traced over the level below landed 100 mm out of line, on the faces instead of the
     centrelines.
   - **Suggestion:** prefer centreline and end snaps of the elements below the faces, or offer a "trace by location
     line" option.
6. **Widening an opening grows it from its start edge.**
   - **Problem:** a window placed by its centre, then widened from 900 to 3600 mm, ran into the wall's corner join.
   - **Suggestion:** widen about the centre, as Revit does by default, or show which edge stays put.
7. **Multi-selection by box selects everything inside it**, including grid lines and slabs.
   - **Suggestion:** a "Filter selection by category" step (as in Revit), so "select the walls" is one action.
