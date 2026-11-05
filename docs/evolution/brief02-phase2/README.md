# Brief 02, Phase 2: first-hour fixes

Reference screenshots for the first part of Phase 2 of *CreaBIM brief 02: consolidated* (2026-11-04): the brief's sections 1.4 to 1.8, the small things the release review and the tour films ran into in their first hour. They were taken in the running app on the starter house (`creabim sample house.creabim`, then `creabim-app house.creabim`). Every item has moved from [docs/KNOWN_GAPS.md](../../KNOWN_GAPS.md#brief-02-2026-11-04) to its *Recently fixed* list.

## Item by item

| Brief | What | Status | Commit |
| --- | --- | --- | --- |
| 1.4 | Footing top | **Done.** A new strip footing starts under the floor slab on its level, not at the wall base. | `7b4573b` |
| 1.4 | Partitions | **Done.** *Under selected* leaves out walls thinner than 150 mm and says how many. If only partitions are picked, they still get footings, so nothing is lost. | `7b4573b` |
| 1.4 | Picking | **Done.** In a plan, what is in the cut wins a click over what is drawn dashed below or above it. A click in a door's opening takes the door, not the footing under the wall. | `724c2e4` |
| 1.5 | Beam height | **Done.** A new beam's top starts under the slab above, not inside it. | `7b4573b` |
| 1.6 | Underlay snaps | **Done.** Over an underlay, snaps take the traced walls' faces and their centrelines. The mark says *Underlay face* or *Underlay centreline*, and ends say *Endpoint*. The underlay is now drawn over fills, so a footing or a slab no longer hides it. | `bee1196`, `cc88754` |
| 1.7 | Openings near corners | **Done.** Widening a door or window in Properties keeps it centred, or keeps the jamb nearest a wall end when that jamb is within 500 mm of it. With several selected, each widens the same way. The status bar warns when the opening then runs into a wall join. Grips still widen from the jamb dragged. | `bcb3142`, `cc88754` |
| 1.8 | Frame all | **Done.** Home frames only the pane under the pointer, and the 3D toolbar's *Frame all* only the 3D view. | `679902f` |
| 1.8 | Export | **Done.** Exports ask where to save, starting next to the project, and then the status bar offers *Open the folder*. Where no save window can open (no file picker on the system), the export goes next to the project as before. | `9a1647c`, `ed0c602` |
| 1.8 | Box selection | **Done.** A box that takes several kinds says what it took and offers *Keep only…* in the status bar, with each kind and its count. | `30a0a0b` |
| 1.8 | Guide lines | **Done.** Guide lines from resting on a point or edge now go after six seconds without use. While a snap follows one, or a shape is being drawn, they stay. The *guides* and *tracking* settings keep their defaults (on). | `75b7551` |
| 1.8 | To check by hand | **All three happened and are fixed.** Ctrl+A, H and the other letter shortcuts now stay in a text field. The Catalogue window opens over the other pane, or at the right edge, clear of where things are placed. The Materials window closes after *Assign*. | `147726d` |

## The logo (addendum A2)

| A2 | What | Status |
| --- | --- | --- |
| 1 | Top-left brand and the CreaBIM menu | **Done.** The small cut at 20 px, `creabim-mark` in `ui/icons.js`: sage `#4F6B4C` in the light themes (Light, Paper, Sage), light sage `#8FB08A` in the dark ones. |
| 2 | Window and taskbar icon | **Done.** `ViewportBuilder::with_icon` from `creabim-app-icon-256.png`. |
| 3 | Windows .exe icon | **Done, but not tried.** `build.rs` embeds `creabim.ico` through `winresource` when built on Windows. It can't be built here (Linux), so check the .exe's icon in Explorer on the first Windows build. The macOS bundle needs an `.icns` step when there is a bundle, which comes with the installer (section 6). |
| 4 | About, New project, README | **Done.** About shows the lockup (white in dark themes). New project and the README header show the full mark. |
| 5 | `.creabim` file icon | **Later,** with the installer (section 6). |

The files are in [crates/creabim-app/assets/logo](../../crates/creabim-app/assets/logo), with a README of their uses and rules.

## Addendum A3: the review of main 889c32c

| A3 | What | Status | Commit |
| --- | --- | --- | --- |
| 5 | Marker highlight | **Done.** Hovering a view mark, text, tag or dimension traces the mark itself in the hover colour at 1.5 times its line weight: bubble, wedge, line and text. There are no boxes round them. Selected, it is traced the same way in the accent, with its grips (the section line's end square and its depth, the move square on a room tag, whose dashed box is gone). The pick area is as generous as before but not drawn. A marker's text now has the extent of its own size on paper, so it fits inside the bubble (before, a 3 mm number counted as 3 m across). A selected note on a sheet is traced too, not framed. | see PR |
| 6 | Icons | **Done.** All 75 icons in the approved set are copied verbatim from `ui/approved/icons_final.json` (and `icons_actions.json`, the same 14 action icons) into `ui/icons.js`. The test `icons_js_matches_the_approved_set_verbatim` in `webui.rs` fails if one drifts, naming it. To change an icon, change the JSON first, with the owner's OK. | see PR |

## Screenshots

| Screenshot | What it shows |
| --- | --- |
| [01-box-selection-says-what-it-took.png](01-box-selection-says-what-it-took.png) | A box over the living room took 53 elements. The status bar lists them by kind and offers *Keep only…*. |
| [02-keep-only-menu.png](02-keep-only-menu.png) | *Keep only…* opened: each kind with its count. |
| [03-kept-only-the-walls.png](03-kept-only-the-walls.png) | After *Walls (3)*, only the three walls stay selected. |
| [04-underlay-centreline.png](04-underlay-centreline.png) | The Wall tool in the Foundation plan, over the Ground Floor underlay: the pointer is on a partition's centreline, and the mark says so. The ground floor walls now show over the footings. |
| [05-underlay-face.png](05-underlay-face.png) | The same pointer on an outer wall's face says *Underlay face*. |
| [06-home-frames-the-drawing-only.png](06-home-frames-the-drawing-only.png) | Home over the plan: "Framed the drawing." The 3D view keeps its camera. |
| [07-door-widened-about-its-centre.png](07-door-widened-about-its-centre.png) | The front door widened from 1000 to 1800 mm in Properties. Its offset went from 5300 to 4900 mm, so it grew 400 mm each way. |
| [08-door-runs-into-a-join.png](08-door-runs-into-a-join.png) | Widened to 3400 mm, it runs into the hall wall, and the status bar says so. |
| [09-export-then-open-the-folder.png](09-export-then-open-the-folder.png) | After *Sheets as PDF*, the status bar shows *Open the folder*. |
| [10-catalogue-beside-the-plan.png](10-catalogue-beside-the-plan.png) | The Catalogue opens at the right, leaving the plan free to click in. |
| [11-brand-mark-in-the-top-bar.png](11-brand-mark-in-the-top-bar.png) | The logo (addendum A2): the small cut of the weaver and its nest at the top left, at twice its size. |
| [12-new-project-full-mark.png](12-new-project-full-mark.png) | The New project window with the full mark at 48 px. |
| [13-about-with-the-lockup.png](13-about-with-the-lockup.png) | About CreaBIM with the lockup, and a line about the logo. |
| [14a-before-icon-drift-889c32c.png](14a-before-icon-drift-889c32c.png) | Before (A3-6): the owner's drift sheet of main 889c32c, with 35 icons that differ from the approved set. |
| [14b-after-icons-match-the-approved-set.png](14b-after-icons-match-the-approved-set.png) | After: each approved icon beside the one the app now draws; all 75 match. |
| [15-toolbar-with-the-approved-icons.png](15-toolbar-with-the-approved-icons.png) | The toolbar in the running app, at twice its size, with the approved icons. |
| [16-marker-hover-before-after.png](16-marker-hover-before-after.png) | A3-5: hovering an elevation mark. Before, two nested boxes far bigger than the bubble; after, the bubble, wedge and line themselves in blue. |
| [17-tag-hover-before-after.png](17-tag-hover-before-after.png) | Hovering a window tag: before boxed, after the tag itself traced. |
| [18-marker-selected-before-after.png](18-marker-selected-before-after.png) | The mark selected: traced in the accent, with its end square and its depth. |

There are no screenshots of the footing and beam heights, the click through a door, the guide lines fading or Ctrl+A in a text field. These are covered by tests, listed below. The machine the screenshots were taken on has no file picker, so the export's save window doesn't appear in them. It opens on Windows and on Linux desktops.

## How to try it

- **Footings:** on a level with a floor slab, select its walls and use the Footing tool's *Under N selected*. The strips start under the slab, and thin partitions are left out. Select only the partitions to give them footings too.
- **Underlay:** open the Foundation plan (its underlay is the Ground Floor) and draw a wall. The snap mark says face or centreline.
- **Openings:** select a door and change its width in Properties.
- **Box selection:** drag a box over a room in a plan, then use *Keep only…* in the status bar.
- **Export:** File › Export. Once it's written, *Open the folder* shows in the status bar.

## Tests

`cargo clippy --workspace --all-targets -- -D warnings` is clean, and `cargo test --workspace` passes. The new tests cover:

- default footing and beam heights (`members.rs`, `footing.rs`);
- *Under selected* and partitions (`levelcopy.rs`);
- a door's opening winning a click over a footing, and guide lines fading (`planview.rs`);
- underlay faces and centrelines (`snaps.rs`);
- widening about the centre or a corner jamb, and running into a join (`walls.rs`);
- Frame all in one pane, and export then *Open the folder* (`appmenu.rs`);
- *Keep only…* after a box (`selectsimilar.rs`).

## Known gaps

- Debug builds take about a minute to export the starter house's 13 sheets as PDF, and the window doesn't respond meanwhile. Release builds are much faster, but an export still holds the window until it's done.
- A partition is judged by its thickness alone (under 150 mm). There is no load-bearing flag on walls yet.
