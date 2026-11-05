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

## Phase 2 item 2.3: the tracker and steps

| Brief | What | Status | Commit |
| --- | --- | --- | --- |
| 2.10 (1) | Tracker | **Done for lines drawn in plans.** While a wall, beam, grid line, section, elevation, railing, footing or room separator is being drawn, a pill by the pointer shows its Length, its Angle and the step. Typing fills Length; Tab moves to Angle; Enter places the point and the chain goes on; Backspace takes a character back and Esc clears what was typed. A field left empty follows the pointer. Grip drags already took Tab or a digit for a length. *Partly:* Move, Rotate and Copy don't show Δx / Δy in the pill yet, and slab and region edges don't use it. | see PR |
| 2.10 (2) | Steps | **Done.** The status bar has *Step* (Off, 10, 50 or 100 mm, or Custom…; in imperial projects 1/8", 1/2", 1" or 6"), and the Snap menu lists the same choices. The step in use always shows there and in the tracker. Free points and grip drags round to it, and Alt ignores it for one move. | see PR |
| 2.14 | Live dimensions | **Partly.** Old → new and the change show while dragging (A3-3), and D hides or shows them. The distance to the nearest neighbour and the "Widened D03 by 300 mm" wording are not done yet. | see PR |

## Phase 2 item 2.5 with A3-7: detail components and plants

| Brief | What | Status | Commit |
| --- | --- | --- | --- |
| 3.1 / A3-7 | Grips on detail components | **Done.** A selected component shows a square on each of its points and, on a batt, flashing, blocking or repeating detail, a width bar off the middle of its first stretch (both sides move). A repeating detail has a unit bar at the end of its first unit. A filled region has corner squares and edge bars like a slab's, and Edit mode (double-click, or Enter) adds and removes corners and points. The grips show only in the view the component is drawn in. M, G, R, mirror, Shift+D and Ctrl+D work on components in plans and sections, each laid back on its view's paper, and the pill shows the length, width or unit as it changes. Properties shows the length (with the point count) and the width. | see PR |
| A3-7 | Finishing a path | **Done.** Enter, a double-click or a right-click finishes a path. Esc, or picking another tool, keeps a path that has enough points ("Membrane kept with its 2 points. Ctrl+Z takes it away."), and drops it only when it has too few. The hint says how many points there are and "Enter to finish". | see PR |
| A3-7 | Membrane look | **Done.** A membrane is a heavy line 2 mm on paper off the line it follows, so it never draws over that line, with a thin dashed partner 3.5 mm off. It goes on the side of the line the pointer was on when the points were clicked; Space while drawing, or *On the other side* in Properties, swaps it. The style pack (Annotation › Membranes in details) chooses the partner: dashed (the default), a double line or a zig-zag. Membranes already in a project move off their line by the same 2 mm. | see PR |
| 3.2 | Grips on plants | **Done.** A selected tree or shrub has a canopy bar at the edge of its crown in plan (dragging it changes the canopy's width, both sides), and a height bar at its top in 3D and sections. | see PR |
| 3.3 | Callout boxes | **Partly.** A selected callout shows, on the view it was called out from, a square on each corner of its box and a bar on each edge. Dragging one resizes the box and the detail view follows; a ringed callout (on a section) stays round. Its body moves the box with G or M, no longer the line its view was cut on. The pill shows the box's width or height as it changes. *Not yet:* the linked *Open in 3D* crop box for callouts, sections, elevations and plans. | see PR |
| 3.4 | Fill room preview | **Done.** With the Room, Ceiling or Slab tool in *Fill room* (3), the space a click would fill is tinted and outlined under the pointer, with "Room 21.60 m² · click to add" by it (a slab's to the walls' centre lines, as it fills). Over a room that is already there it says so. Where the walls and room separators don't close the space, the gaps are drawn in red, wall end to wall end (or to the nearest face), and the pill says "Not closed: draw its outline (1, 2, 4)"; a click says the same in the status bar. | see PR |

## Phase 2 items 2.1 and 2.2: tool tabs and the element action bar

| Brief | What | Status | Commit |
| --- | --- | --- | --- |
| 2.1 | Tabs | **Done.** The tools row starts with four tabs: *Model*, *Annotate*, *View* and *Manage*. Each tab shows its own tools, plus the commands that belong with them: *Catalogue* on Model, *Annotation styles* and *Mark numbering* on Annotate, *Crop box*, *New sheet*, *New schedule ▾* and *Graphic styles* on View, and *Levels*, *Types*, *Materials*, *Style packs*, *Units* and *IFC ▾* on Manage. Select and Align stay at the left on every tab, and Move, Undo and Redo stay in the row above. The callout is on both Annotate and View. When a tool is picked from a menu, the palette or a key, and it isn't on the tab shown, its own tab comes up. The tab chosen is kept between sessions. | see PR |
| 2.1 | Icons only | **Done.** The tools row has icons only, with each tool's name and what it does in its tooltip. The tool in use shows as a pressed icon, with its name in the options row below. Nothing wraps, and nothing scrolls sideways at 1366 px: the fullest tab, Model, ends at about 1310 px. | see PR |
| 2.1 | View controls out of the options row | **Not started.** This goes with 2.6, where each view gets its own header. Until then the view's settings stay at the right of the options row. | |
| 2.2 (brief 2.7) | Action bar | **Done.** A row of icon buttons sits under the element's name (and its mark) in Properties. The element's own actions come first, tinted: *Edit outline* (Enter) for anything Edit mode opens; *Flip*, *Attach top* (greyed when nothing is overhead) and *Make a type* for walls; *Flip* and *Flip hand* for doors; *Flip* for windows; *Open view* and *Flip* for sections and elevations; *Select tagged* and *Straighten leader* (greyed when the leader is straight) for tags. The common actions follow in a fixed order: *Duplicate*, *Select similar*, *Hide*, *Isolate* and *Frame*. Isolate is left out for annotation. Then come *More* (⋯) and *Delete*, set apart at the right, in red. Tooltips give each action's name and key. With several elements selected, only the actions that apply to all of them show: walls with a door keep *Flip*. When the panel is too narrow, actions move into More, the common ones first. More also holds Move, Rotate, Scale, Copy, Cut and *select similar in the project*. The right-click menu is unchanged. The old *Edit* and *Delete* text buttons are gone. | see PR |

## Addendum A3: the review of main 889c32c

| A3 | What | Status | Commit |
| --- | --- | --- | --- |
| 1 | Footing visibility | **Mostly done.** While the Footing tool is on, any plan shows its level's footings dashed, with a light tint in the accent. The hint line starts "Showing footings while the tool is on". Before the click, the pill says what a click makes ("Strip 600 × 300 under FF East", or that the wall already has one). After placing, the footing is selected, and a selected footing is always drawn, even below the view's range. The status says "Strip footing placed under FF East", and the status bar offers *Show in Foundation plan*. In a project without one it offers *Make a Foundation plan*, which adds a level 600 mm below the lowest with a plan that shows the floor above as its underlay. *Not yet:* the x-ray outline of a selected footing in 3D, and the per-view "Footings in this plan: Hidden / Dashed / Shown" choice. | see PR |
| 2 | Host picking | **Done.** `walls_at` measures across the wall's whole thickness about its centreline, whatever line the wall was drawn on (a test checks every wall of the starter house). Before the first click, the Footing, Door, Window and Opening tools let the wall (or, for a pad, the column) under the pointer win. It is highlighted, and the snap marks and guide lines stay away from it. Tab goes through overlapping hosts ("1/2 · Tab"), and the click goes on the host shown. Where there is no host, and once a free strip has its first point, snaps work as before. *Not yet:* Railing on a stair, Tag, and Attach top use their own picking still. | see PR |
| 3 | Value pills | **Done.** While a grip is dragged, a pill by the grip shows only the value that is changing: "Length 5000 → 5707 +707", or "Moved 750" for a slide or a move. On release it stays about a second, then fades. It shows only in the pane the drag is in. It sits on the theme's raised surface with a 1 px line round it, 11.5 px type and 6 px corners, in the theme's text colour with the changing number in the accent. A test checks both are at least 4.5:1 in all seven themes. The full summary (length, height, thickness) goes to the status bar. D hides or shows the pills. | see PR |
| 4 | Slide grip | **Done.** The grip that floated 300 mm off a wall's face is gone, so every grip sits on the wall. Dragging the wall's body slides it square to itself and its joined walls stretch. Shift frees the direction, and Tab types the distance. A wall under 14 px across on screen shows its thickness grip only when the pointer comes near its face. | see PR |
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
| [19-no-floating-slide-grip.png](19-no-floating-slide-grip.png) | A3-4: before, a slide grip off the kitchen wall's face; after, the grips on the wall, and the wall slid 750 mm by its body, the north wall stretching with it. |
| [20-live-value-pill.png](20-live-value-pill.png) | A3-3: dragging the wall's end. Before, nothing until release; after, "Length 5000 → 5707 +707" by the grip. |
| [21-full-summary-in-the-status-bar.png](21-full-summary-in-the-status-bar.png) | On release, the status bar has the full summary. |
| [22-tracker-length-and-angle.png](22-tracker-length-and-angle.png) | 2.10: drawing a wall, 3000, Tab, 90 typed into the tracker; Enter places it, 3000 mm long and straight up. |
| [23-step-in-the-status-bar.png](23-step-in-the-status-bar.png) | *Step* in the status bar, with its choices. |
| [24-footing-tool-host-wins.png](24-footing-tool-host-wins.png) | A3-2: the Footing tool over the First Floor's east wall. Before, an *Intersection* snap and guide lines; after, the wall highlighted and "Strip 600 × 300 under FF East". |
| [25-footing-placed-and-shown.png](25-footing-placed-and-shown.png) | A3-1: before, the click started a free strip and nothing showed; after, the footing is placed under the wall, selected and drawn. |
| [26-footings-shown-while-the-tool-is-on.png](26-footings-shown-while-the-tool-is-on.png) | The Ground Floor plan with the Footing tool on: the level's footings dashed and tinted, and a wall that already has one says so. |
| [27-show-in-foundation-plan.png](27-show-in-foundation-plan.png) | After placing: *Show in Foundation plan* in the status bar. |
| [28-foundation-plan-opened.png](28-foundation-plan-opened.png) | It opens the Foundation plan with the new footing selected. |
| [29-detail-region-grips.png](29-detail-region-grips.png) | 3.1: the perimeter insulation (a filled region) in Detail 1 Wall base. Before, selected but no grips; after, corner squares and edge bars. |
| [30-membrane-off-its-line.png](30-membrane-off-its-line.png) | A3-7: a membrane drawn along the slab's top. Before, it lay on the slab's line and couldn't be seen; after, a heavy line just above it with its dashed partner. The DPC under the wall is drawn the same way. |
| [31-esc-or-right-click-keeps-the-path.png](31-esc-or-right-click-keeps-the-path.png) | A right-click finished the batt's path, and the status bar says so. |
| [32-batt-width-grip.png](32-batt-width-grip.png) | A batt selected, then its width bar dragged: "Width 100 → 190 +90". |
| [33-detail-moves-with-g.png](33-detail-moves-with-g.png) | G on a detail component: before, "can't be moved this way"; after, it moves. |
| [34-plant-canopy-grip.png](34-plant-canopy-grip.png) | 3.2: a tree in the garden sample. Before, no grips; after, its canopy bar dragged, "Canopy 6000 → 8450 +2450". |
| [35-callout-box-grips.png](35-callout-box-grips.png) | 3.3: Detail 2 Window's ring in Section C-C. Before, selected without grips; after, its top edge dragged ("Width 1200 → 2700": it stays round), then let go. |
| [36-fill-room-preview.png](36-fill-room-preview.png) | 3.4: the Room tool in *Fill room* over the simple house's east room. Before, nothing until the click; after, the room it would make and its area. |
| [37-fill-room-not-closed.png](37-fill-room-not-closed.png) | The west wall deleted: before, nothing shows why a click fails; after, the gap in red and "Not closed: draw its outline (1, 2, 4)". |
| [38-tabs-1366.png](38-tabs-1366.png) | 2.1 at 1366 px. Before, one long row of every tool that ran off the right edge. After, each of the four tabs. |
| [39-tabs-1600.png](39-tabs-1600.png) | The same at 1600 px. |
| [40-tabs-1920.png](40-tabs-1920.png) | The same at 1920 px. |
| [41-tab-menu.png](41-tab-menu.png) | The Manage tab's *IFC* button and its menu. |
| [42-tab-follows-tool.png](42-tab-follows-tool.png) | Wall picked from the Create menu while the Manage tab showed: the Model tab comes up with it. |
| [43-action-bar.png](43-action-bar.png) | 2.2. Before, a window's Properties with *Edit* as a text button (Delete was at the very bottom). After, the bar for a wall, a section, a tag, a room and a mixed box selection. |
| [44-action-bar-zoom.png](44-action-bar-zoom.png) | The wall's bar at three times its size. |
| [45-action-bar-more.png](45-action-bar-more.png) | The bar's More menu. |

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
- *Keep only…* after a box (`selectsimilar.rs`);
- every tool on a tab, and the tab each tool is on (`tooltabs.rs`);
- the action bar for a wall alone and for walls with a door, and every action having an icon (`actionbar.rs`);
- the line icons turned into strokes for the Properties panel, circles staying round (`svgicon.rs`).

## Known gaps

- Debug builds take about a minute to export the starter house's 13 sheets as PDF, and the window doesn't respond meanwhile. Release builds are much faster, but an export still holds the window until it's done.
- A partition is judged by its thickness alone (under 150 mm). There is no load-bearing flag on walls yet.
