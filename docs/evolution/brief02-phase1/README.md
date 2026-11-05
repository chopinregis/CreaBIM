# Brief 02, Phase 1: quick wins

Reference screenshots for Phase 1 of *CreaBIM brief 02: consolidated* (2026-11-04), taken in the running app on the starter house (`creabim sample house.creabim`, then `creabim-app house.creabim`), plus two drawings exported from it. Everything in the brief was first put in [docs/KNOWN_GAPS.md](../../KNOWN_GAPS.md#brief-02-2026-11-04); what landed here has moved to its *Recently fixed* list.

## Item by item

| Brief | What | Status | Commit |
| --- | --- | --- | --- |
| 1.1 | The waiting branches | **Partly.** The three branches were never pushed, so their work was rebuilt: a Properties change with several elements selected goes to all of the same kind, and `creabim-app <file>` opens a file at launch. Category names that wrap mid-word are not done. | `23f4bf3`, `b14bb97`, `48118d1` |
| 1.2 | Duplicate, callouts, help cards | **Done.** Duplicate goes into a base-point move and Esc leaves the copy in place. Drawing a callout stays in the parent view (double-click the mark to open it). Help cards sit under the 3D toolbar and close on Esc or when the pointer leaves. | `0059015` |
| 1.3 | Final icon set | **Done.** 60 icons drawn to the IconSetFinal board, with a FreeCAD icon (an F with a gear) and a units icon instead of the ruler. The board's JSON files weren't attached, so the icons were drawn from the picture. | `3d07963` |
| 1.4 | Properties rows | **Done for every panel built on the shared rows** (walls, slabs, roofs, openings, columns, beams, stairs, railings, rooms, ceilings, footings, objects, parts, sketches, guides, views, crop boxes, grids, styles and the Settings window): label first, controls in one column, dropdowns for four or more choices (wall pattern and form, roof shape with pictures), switches for on/off, full-width headers with a turning chevron. Category tick lists in the visibility panel stay checkboxes. | `48118d1`, `25443d4` |
| 1.5 | Room tags and view markers | **Done for the defaults.** Stacked room tags and Classic markers, sized on paper, with the sheet reference or "–". The first tag (Inline) and the first markers (Plain) are kept under Graphic styles › Tags and marks. Number badge, Boxed, Minimal and Bold come with 2.13 in Phase 3. | `ab7ebae` |
| 1.6 | Application menu and Units | **Done.** | `48118d1` |
| 1.7 | New project window | **Done.** File has one *New project…* (Ctrl+N); the six samples are under Help › Samples to learn from. | `3d07963` |
| 1.8 | Quiet grips and their states | **Done.** Squares, bars, no circles on stalks, no flip circles; the four states. Live dimensions while dragging are Phase 2 (2.10, 2.14). | `cb96757` |
| 1.9 | A true top view | **Done.** | `f88b8d3` |

## Screenshots

| Screenshot | What it shows |
| --- | --- |
| [01-true-top-view.png](01-true-top-view.png) | View › Top in the 3D view: it looks straight down with north up, so walls show no vertical edges and the grid bubbles line up. Orbiting near an axis view snaps to it within 2°. |
| [02-app-menu.png](02-app-menu.png) | The CreaBIM menu at the left of the top bar: About, the version and build, Check for updates, Settings (Ctrl+,), Keyboard shortcuts (Ctrl+/) and Exit. The file name now sits at the right. |
| [03-units-switch.png](03-units-switch.png) | The Units switch in the status bar: metric and imperial units, each with an example, and *Rounding and more…*. It is saved with the project and Ctrl+Z undoes it. |
| [04-icon-sheet-100.png](04-icon-sheet-100.png), [04b-icon-sheet-200.png](04b-icon-sheet-200.png) | The new icons at 100% and 200%. |
| [05-markers-and-room-tags.png](05-markers-and-room-tags.png) | The Ground Floor plan in the app: Section A's split bubble (A over A201) with its wedge and heavy line end, North Elevation's mark (3 over A301), and Stacked room tags (LIVING / DINING, the area, then the number small). |
| [05b-plan-export-markers-tags.png](05b-plan-export-markers-tags.png) | The same plan exported to SVG and zoomed in. |
| [05c-detail-callouts-in-a-section.png](05c-detail-callouts-in-a-section.png) | Section C-C: its three detail callouts are dashed circles with a leader to a split bubble (1, 2, 3 over A501). In plans a callout is a dashed rounded box. |
| [06-properties-rows.png](06-properties-rows.png) | A room's Properties: every label at the left, every field in one column, and *Own height* as a switch. |
| [07-new-project.png](07-new-project.png) | The New project window (Ctrl+N): Empty project (two levels, in your units) and Starter house, then six samples that each show one thing. A sample opens as a new, unsaved project. |
| [08-quiet-grips-slab.png](08-quiet-grips-slab.png) | The ground floor slab selected: small squares at the corners and short bars on the edges, nothing outside the outline. |
| [09-grip-under-the-pointer.png](09-grip-under-the-pointer.png) | Under the pointer an edge bar grows and names itself, "Edge: push". A door's jamb says "Jamb: widen". |
| [10-edit-mode-plus-on-hovered-edge.png](10-edit-mode-plus-on-hovered-edge.png) | Edit mode (Enter): the corners are filled squares and the "+" that adds a corner shows only on the edge under the pointer. |
| [11-tags-and-marks-choices.png](11-tags-and-marks-choices.png) | File › Graphic styles › Tags and marks: the room tag layout and the marker family, one choice for the whole project. |
| [12-two-walls-selected.png](12-two-walls-selected.png) | Two walls selected (Shift+click): Properties says so and shows the last one picked. Their grips are bars. |
| [13-one-edit-both-walls.png](13-one-edit-both-walls.png) | Base offset set to 150 mm once: the status bar says "Wall base offset: 2 elements", and both walls have it. One Ctrl+Z undoes both. |

## How to try it

- **Several at once:** select walls (or doors, slabs…) with Shift+click, then change a value in Properties. It goes to each selected element of the same kind. Where each one is (its ends, its host, its place along the wall) stays its own.
- **Grips:** hover a grip to see its name and the pointer it takes. Hold it to see the halo and the old place; let go to see the new value for a moment. Space flips a selected door, window, wall, section or piece of furniture; Shift+Space flips a door's hand.
- **Markers and tags:** File › Graphic styles › Tags and marks switches the whole project between Stacked and Inline tags, and Classic and Plain markers. A room tag dragged out of its room gets a leader back to it.
- **Units:** the status bar's Units switch, or the Settings window (Ctrl+,).

## Tests

`cargo test --workspace` passes. New tests cover the top view and axis snap (`camera.rs`), the app menu, the Units switch and its undo, samples opening as new projects, the launch file (`appmenu.rs`), Duplicate and Esc (`modal.rs`), multi-select edits on walls and the starter house's hall walls (`multiedit.rs`), Stacked tag lines and sizes (`annotate.rs` in creabim-geom), and Classic markers and sheet references (`markers.rs`).

## Known gaps

These are in [docs/KNOWN_GAPS.md](../../KNOWN_GAPS.md#brief-02-2026-11-04):

- Category names still wrap mid-word in the visibility list.
- Room tags: no Number badge or Boxed layout yet; tags don't keep level in rotated views.
- Markers: no Minimal or Bold family yet. A single click on a bubble doesn't open its view; a double-click on the selected mark does. A grid line passing a bubble shows through it.
- No live dimensions while dragging, and the status bar says "Change width of Loggia door", not "Widened D03 by 300 mm".
- Bubble text is hidden when the plan is zoomed far out, like other paper-sized text.
