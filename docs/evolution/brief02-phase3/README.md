# Brief 02, Phase 3: structure

Reference screenshots for Phase 3 of *CreaBIM brief 02: consolidated* (2026-11-04). They were taken in the running app on the starter house (`creabim sample house.creabim`, then `creabim-app house.creabim`). Items move from [docs/KNOWN_GAPS.md](../../KNOWN_GAPS.md#brief-02-2026-11-04) to its *Recently fixed* list as they land.

## Item by item

| Brief | What | Status |
| --- | --- | --- |
| 3.1 (2.6) | View areas and workspaces | **Done, with small gaps** (below). |
| 3.2 (2.10, part 3) | Temporary dimensions that drive, with the padlock | Not started. |
| 3.3 (2.13) | Choices with pictures, personal settings, style packs, the marker and tag designer, style pack export and import | Not started. |
| 3.4 (4.2) | View cube | Not started. |

## 3.1 View areas and workspaces

The middle of the window is now a set of **view areas**. Each area shows the 3D view, a view (plan, section, elevation, detail, schedule) or a sheet, and has its own header:

- the kind pill (3D, plan, section…) picks what the area shows, and the name next to it picks the view;
- the view's own controls follow: for 3D the render style, perspective, crop boxes, visibility, framing, X-ray and so on; for a plan its tags, underlay, visibility and cut height;
- on the right, *Show this area alone* and a ⋯ menu.

When an area is too narrow, the controls that don't fit go into its ⋯ menu, with *Split side by side*, *Split one above the other*, *Close this area* and *Show this area alone*. The 3D view's floating toolbar and the view controls in the options row are gone.

**Workspaces** are named layouts of areas, in the top-right menu and under *View → Workspace*:

| Workspace | Areas |
| --- | --- |
| Model | the 3D view alone |
| Drawing | 3D beside the current drawing |
| Document | 3D beside the current drawing, with its sheet under it |
| Sheets | the sheet that holds the current drawing, beside the drawing |

*The current drawing* is the plan or section you were last working in, so switching workspace keeps you on it. *Save current layout…* keeps your own layout under a name, and it can be deleted from the same menu.

**Keys:** Ctrl+Space shows the area under the pointer alone, and again brings the others back. Ctrl+PgUp and Ctrl+PgDn step through the workspaces. `/` isolates the selection, or shows everything again if anything is hidden.

**Splitting and joining:** drag an area's corner grip (the three dots at the left of its header) into the same area to split it there, or into the area beside it to join them. While dragging, the new half is tinted, or the area that would close is. The strips between areas resize them, and a double-click evens them out.

**The hint line** for where the pointer is (for example *Click to select, Shift+click to add…*) now sits in the status bar after the message, in grey, instead of over the plan.

![The four workspaces at 1366 px](46-workspaces-1366.png)

![The four workspaces at 1920 px](47-workspaces-1920.png)

![Before and after](48-areas-before-after.png)

![A narrow area and its menu](49-narrow-area-more.png)

![Split and join](50-split-and-join.png)

![One area alone](51-area-alone.png)

![The hint in the status bar](52-status-hint.png)

### Still to do in 3.1

- Saved workspaces live in your user settings, not in the project, so they follow you from project to project but not to another computer.
- Only one area can show the 3D view at a time.
- A number control (such as *Cut height*) that doesn't fit an area header has no place in its ⋯ menu yet; it comes back when the area is wider.
- A sheet has no *Open in area ▾* yet; a double-click on a sheet's view opens it in place.
- A corner join always closes the area dragged into.

## How to test 3.1

1. Open the starter house. The Drawing workspace shows 3D beside the ground floor plan.
2. Pick *Document* and then *Sheets* from the top-right menu; each keeps the same plan and shows its sheet.
3. Rest the pointer on the plan and press Ctrl+Space; press it again.
4. Press Ctrl+PgDn a few times, then Ctrl+PgUp.
5. Select a wall and press `/`; press `/` again.
6. Drag the plan's corner grip down inside the plan and let go; then drag the new area's grip into the one above it.
7. Narrow the plan area by dragging the strip between it and the 3D view, and open its ⋯ menu.
8. *Save current layout…*, name it, switch away and back to it.
