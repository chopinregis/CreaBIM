# Phase 1: details and joins

This round cleans up the junctions where walls, slabs and other cut elements meet, and adds detail drawing: callouts that make larger-scale detail views, and detail components to draw over them. The starter house gets a detail sheet.

The project holds the conventions as data:

- how strong each layer function is where elements meet, which a layer can override;
- which element wins at a given junction, and whether that junction is cleaned up;
- the kinds of detail component and their sizes.

The code only applies them.

## Screenshots

| Shot | What it shows |
| --- | --- |
| `01-three-way-join-before.png` | Before (main): three partitions drawn to one point, the case in the markup. Two partitions are selected. The wall square to the others and one of the in-line walls form an L corner, the other in-line wall stops at the centre line, and their outlines overlap in the junction. |
| `02-three-way-join-after.png` | After: the same three walls. The two in-line walls run through as one wall, and the third stops at their face. The fill is clean and there are no stray outlines. |
| `03-slab-wall-layered-join.png` | A floor slab meeting a rendered wall, in section. **Before:** the wall is drawn over the floor. **Automatic:** layer by layer, so the plaster stops at the floor and the floor's finishes stop at the block. **Set by hand:** the floor cuts into the wall up to the block's outer face, and the insulation and render run on past it. |
| `04-junction-menu.png` | Two walls selected and right-clicked: **Switch join order** and **Don't clean up the junction**. Once a junction has a rule, **Join automatically** removes it. |
| `05-callouts-on-section.png` | Section C-C in the app, with the boxes and numbered bubbles of its three callouts. Hovering a callout highlights it. |
| `06-detail-view.png` | Detail 3 Eaves, opened from the new **Details** group of the project browser. It is at 1:10, cropped to its box, with its detail components (batt, soffit, fascia, gutter) and notes. |
| `07-detail-components-tool.png` | The **Detail component** tool. The bar has its kinds and a width. A repeating detail (bricks) has just been drawn, and its *Properties* show its unit and joint. A batt is being drawn below it. |
| `08-a501-details-sheet.png` | The starter house's new sheet **A501 Details**: section C-C at 1:25 with the wall base, window and eaves details at 1:10. |

Shots 01, 02 and 04 to 07 are of the app. Shots 03 and 08 are drawn by the command line (`creabim view` and `creabim sheet`). The model for shots 01 to 04 is the starter house with a demo storey added high above it. The same model was opened in a build of main for the "before" pictures.

## What was built

- **Three-way and four-way wall joins.** When three or more walls end at the same point, they make a node. The two walls that run most nearly straight through it pass through as one wall; if no two run straight, the thickest pair goes through. The other walls stop at those walls' faces, and their structure runs on into the through walls' structure. Ends that only come close to each other are left alone, so walls that only butt are not trimmed.
- **Joins by layer priority.** Where cut elements overlap in plans and sections (walls, slabs, roofs, columns, beams, footings, ceilings), the stronger layer cuts the weaker one. The strength order is structure, then substrate, then insulation, air gaps and membranes, then finishes. A layer can set its own priority in the assembly file (`priority`), which overrides its function's. When two layers are equally strong, the element kind decides: a column beats a wall, and a wall beats a slab. Between elements of the same kind, the one drawn first wins. Layers of the same material merge instead of cutting each other, so they have no line between them.
- **Per-junction control.** Select two elements and right-click. **Switch join order** stores a junction rule saying which element wins. **Don't clean up the junction** draws both elements whole. **Join automatically** removes the rule again. The rules are listed under **Junctions** in each element's *Properties*, where you can also change the winner. A rule is a model element of its own (`Join`), and every change is one transaction, so it undoes. Deleting either element deletes its rules.
- **Callouts.** The **Callout** tool (in the Views group) works on a plan or section. Two clicks make a box, and a new detail view is created at the scale picked in the bar (1:2, 1:5, 1:10 or 1:20). The detail draws what its parent draws, cropped to the box. The parent shows the box and a bubble with the detail's number, taken from its name. Details are listed in their own **Details** group of the project browser. Their section lines are not drawn in plans, because the box on the parent marks them.
- **Detail components.** The **Detail component** tool (in the Annotation group) draws seven kinds: batt insulation, membrane, flashing, blocking, repeating detail (bricks or blocks, with a unit and a joint), filled region (any material hatch, masking what is under it) and break line. They are drafting that belongs to one view. They are not in 3D, schedules or IFC.
- **Starter house details.** Section C-C runs through the south wall and the study window at 1:25. It has three callouts at 1:10: Detail 1 Wall base, Detail 2 Window and Detail 3 Eaves. Each is drawn over with components and notes, and all four views are on sheet **A501 Details**, which the cover sheet lists. The first floor slab now bears on the block of the walls under it, inside their insulation and render, and junction rules let it cut into the block.
- **Help pages** *Joins* and *Details*, with links from *Walls* and *The starter house*. The shortcuts page lists Enter for finishing a detail component. The new icons (callout, detail component, join) are in `make_icons.py` and `icons.js`.
- **Known gaps closed.** Slabs running into walls no longer show both fills overlapping in sections: they now join by priority. The strip footings at the loggia corner no longer show an overlap square, because footings of the same material merge.

## Files and exchange

- New fields read with defaults from older files, and a test checks it: `View.callout`, `Layer.priority`, and a junction rule's `winner` and `cleanup`.
- IFC export leaves out detail components and junction rules, like other annotation. The starter house still round-trips through IFC4.

## Known gaps

All open gaps, from every milestone, are kept up to date in [docs/KNOWN_GAPS.md](../../KNOWN_GAPS.md). The list below is as it was when this milestone landed; some of it may be fixed since.

- Detail components can't be moved with G or M, or edited with grips. To change one, delete it and draw it again, or change its kind and width in *Properties*.
- A detail stores a copy of its parent's view settings when it is made. If the parent's section line or depth later changes, the detail follows the parent's line when it draws, but the copy stored in the file goes stale. If the parent is deleted, the detail draws from that old copy.
- A junction rule changes how overlapping pieces are cut. It doesn't change how wall ends are trimmed, so at a wall-to-wall node **Switch join order** may make no visible difference.
- A slab drawn past the wall's structure into its insulation is cut by the block where they overlap. The part of the slab in the insulation is left as a separate island of concrete. Draw the slab to the block's outer face, as the starter house does, or set the junction by hand.
- Layer priorities can only be overridden in the assembly file. *Edit layers…* has no priority column yet.
- Where three walls meet and no two of them are in line (a Y), the thickest pair goes through, and the third wall's trim can leave a small overlap.
- Elevations still draw each wall's own outline at corners. Joins clean up plans and sections only.
- A callout box is always a rectangle aligned with the view. A detail of a detail is possible but has had little testing.
