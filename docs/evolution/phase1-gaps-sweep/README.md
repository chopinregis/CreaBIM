# Phase 1: gaps sweep

This round checked every known gap against `main` on 2026-11-04 and gathered them into one living list, [docs/KNOWN_GAPS.md](../../KNOWN_GAPS.md). It also fixed the small ones listed below. Every milestone README's *Known gaps* now points to that list.

## Screenshots

| Shot | What it shows |
| --- | --- |
| `01-narrow-window-before.png` | `main` at a window 1000 px wide. The view switcher at the right of the menu row and the last tools of the tool row are cut off. |
| `02-narrow-window-after.png` | The same window on this branch. Under about 1100 px the rows tighten (the logo without its name, narrower buttons), so the whole menu row fits. The tool row fades at its right end, where it has more. |
| `03-tool-row-scrolled.png` | The tool row scrolled to its end with the mouse wheel. It now fades at its left end. |
| `04-browser-search-by-mark.png` | The project browser's search finds the *Study side window* by its mark, `W05`. |
| `05-first-floor-stair-well.png` | The first floor plan: the stair is seen through its well with *DN*, and the well keeps its outline with no cross over the treads. |

## What changed

- **`Ctrl+S` saves.** After clicking a web pane (the browser, the menus), keys reach the views through the page. They lost their Ctrl, Shift and Alt on the way, so `Ctrl+S` over a view started Scale. They keep them now, so `Ctrl+Z`, `Shift+D` and `Alt+H` work too.
- **Narrow windows.** The menu and tool rows fit or scroll, as in the shots above.
- **The 3D view zooms toward the pointer**, in perspective and orthographic. The zoom buttons still zoom about the middle.
- **Walls attached under the cut.** A wall whose top is attached to a roof or slab below the plan's cut height is drawn in projection, not cut.
- **No cross over a stair.** A stair well, or a hole in a slab, that a stair arrives through keeps its outline but loses the cross.
- **Attach top to roof/slab** is greyed out in the right-click menu when nothing is overhead, as in Properties.
- **The distance in the 3D Align and Stretch preview** sits on a dark pill, so it reads over orange and brick.
- **The project browser's search** finds elements by mark (`D01`, `W05`) as well as by name.

## How to try it

1. Make the window about 1000 px wide. Roll the wheel over the tool row to scroll it.
2. Click a row in the project browser, move the pointer over the plan and press `Ctrl+S`: the project saves.
3. Roll the wheel over a corner of the house in the 3D view: that corner stays under the pointer.
4. Type `W05` in the browser's search box.
5. Open the *First Floor plan* and zoom to the stair.

## Tests

- `shell::tests::ctrl_s_typed_in_a_page_saves_and_does_not_scale`
- `shell::tests::browser_rows_carry_marks_for_the_search`
- `camera::tests::zooming_keeps_the_point_under_the_pointer`
- `wallsui::tests::attach_top_is_offered_only_with_a_roof_or_slab_overhead`
- `plan::tests::a_wall_attached_under_the_cut_is_drawn_in_projection`
- `plan::tests::the_first_floor_shows_the_stair_through_its_well` (now checks the well has no cross)

The narrow-window rows and the distance label are page styling and painting, with no test.

## Known gaps

All open gaps, from every milestone, are kept up to date in [docs/KNOWN_GAPS.md](../../KNOWN_GAPS.md). From this round:

- A wall whose attached top drops below the cut along only part of its length is still drawn cut along all of it.
- Under about 1000 px the menu row scrolls too, so the view switcher at its right end is reached by scrolling. The rows show no buttons to scroll; the wheel and the fades are the cue.
- The 3D view's buttons hide when the 3D view is narrower than 320 px.
