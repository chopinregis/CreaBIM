# Phase 1: graphic styles and view ranges

Shots of the style packs, view templates and view ranges. The sheets come from the Tropical Villa (`TropicalVilla_REV00.creabim`) and the starter house, exported with `creabim apply-style <project> <pack> --pdf`. The app shots use the starter house.

For the site and roof plan shots (03 to 08), the villa was re-levelled first:

- A *Site ±0.00* level was added under the floor. The boundary walls, gatehouse walls, site slabs and copings were moved onto it.
- The R1 roofs moved to the R1 level, and the hearth cap moved to *Hearth top +6.35*. Each element kept its height in the world.
- *Site and floor plan* now has a view range from *Site* to R2 + 2 m, with overhead dashed.
- *Roof plan* is set to *From above*.

No code changed between the Standard and Light linework sets. Only the pack differs.

| Screenshot | What it shows |
| --- | --- |
| [01-villa-A101-standard.png](01-villa-A101-standard.png) | Villa A-101 under **Standard**, the pack that holds CreaBIM's own look as data. The drawing is the same as before this branch: a test compares the starter house's plan and schedule sheets line for line. |
| [02-villa-A101-light-linework.png](02-villa-A101-light-linework.png) | The same sheet under **Light linework**. Pens are finer, walls are cut in pale blue-grey poché with no hatch, columns are darker, doors and windows are blue-grey, furniture is grey and grids are lighter. |
| [03-villa-sections-standard.png](03-villa-sections-standard.png) / [04-villa-sections-light-linework.png](04-villa-sections-light-linework.png) | Sections under each pack. Light linework's Section template draws in hidden line, and what lies further behind the cut fades. |
| [05-site-plan-all-levels.png](05-site-plan-all-levels.png) | One plan with *Levels in range*. The site level's boundary walls and gatehouse are cut at their own height, and the floor level is cut at its own. The roofs on R1 and R2, and the hearth cap, are drawn dashed as overhead. |
| [06-roof-plan-standard.png](06-roof-plan-standard.png) / [07-roof-plan-light-linework.png](07-roof-plan-light-linework.png) | The roof plan *From above*. Nothing is cut, and each surface hides what lies under it: upper roofs hide lower roofs, and roofs hide the walls and rooms beneath them. Slope arrows, grids and section marks still show. Under Light linework, roofs take the pack's *Roofing* surface pattern, and sun shadows fall on the ground. |
| [08-elevations-light-linework.png](08-elevations-light-linework.png) | Elevations under Light linework, in hidden line with distance fade. |
| [10-graphic-styles-object-styles.png](10-graphic-styles-object-styles.png) | *File › Graphic styles…*, Object styles page, with Light linework just added from *Add a shipped pack*. Each category can set a projection pen, cut pen, colour, line pattern, cut fill, cut pattern and surface fill. |
| [11-house-A101-standard.png](11-house-A101-standard.png) / [12-house-A101-light-linework.png](12-house-A101-light-linework.png) | The starter house ground floor sheet under both packs. |
| [13-house-A501-details-light-linework.png](13-house-A501-details-light-linework.png) | The starter house details sheet (from #43) under Light linework. The detail views and their filled regions take the pack's poché and patterns like any other view. |
| [15-graphic-styles-patterns.png](15-graphic-styles-patterns.png) | The Patterns page. Patterns are data: line families (angle, spacing, offset, stagger, dashes), on paper or at true size, with a background fill and a live preview. Light linework's *Brick* has true-size coursing, and because it is named like the built-in Brick hatch, it replaces that hatch. |
| [16-graphic-styles-view-templates.png](16-graphic-styles-view-templates.png) | The View templates page. A template can set the scale, style, pens, the categories left out, overrides and depth cues. *Views without a template take* gives a default template for each kind of view, now including detail views. |
| [17-properties-view-range-and-template.png](17-properties-view-range-and-template.png) | A plan's Properties. *View range* has Its level, Levels in range (Top, Bottom, View depth, Overhead dashed) or From above. *Template and overrides* has the template, Fade with distance, Sun shadows and per-category overrides. Selecting an element adds an *In <view>* section for that one element's override. |

## How to try it

- **Restyle a project:** open *File › Graphic styles…*, then *Add a shipped pack › Light linework*. One undo goes back. From the command line, run `creabim apply-style house.creabim "Light linework" --pdf out.pdf`. `creabim style-packs <dir>` writes the shipped packs as JSON, ready to edit as an office pack. You can then load the pack with *Load pack…* or pass its path to `apply-style`.
- **Site plan:** in a plan's Properties, choose *View range › Levels in range* and set *Bottom* to the site level.
- **Roof plan:** choose *From above*.
- **One view differently:** add a category under *Template and overrides › Add a category…*. To change one element, select it and use its *In <view>* section.
- **Site areas:** the Site style's surface fill goes under every hard landscape area. Its surface pattern is drawn only on areas whose own pattern is *None*, so a paving pattern set on an area stays.

## Known gaps

- The plan and section panes on screen still draw with CreaBIM's own pens. Styles show on sheets, in the sheet view and in PDF/SVG export.
- Sun shadows are cast only on the ground, from from-above plans. Shadows don't fall on lower roofs and aren't drawn in elevations or sections.
- Distance fade doesn't lighten shaded raster images (3D views placed on sheets).
- The editor doesn't yet cover object styles for one material, surface patterns, draw order or transparency. Those are set in the pack's JSON. There's no per-material pattern picker in the Materials window yet; a material's `pattern` is set in the file.
- A template's overrides and depth cues are edited in the pack file. A view's own overrides are edited in Properties.
- There is no coarse, medium or fine detail level.
- Element categories cover what Category::of knows. Detail items (filled regions, components) have no category, so they follow pack patterns by hatch name but take no object style or override.
- Packs carry mark rules, title blocks and text styles by name, but the editor doesn't show them.
