# Phase 1: parametric parts, screen walls and site

CreaBIM now has **parametric parts**, **screen walls** and **site** elements.

- **Parts.** An Extrusion is an outline pulled up. A Sweep is a profile run along a path, or along the top or base of a wall. A Void is an extrusion that cuts what it meets. Each part has a material and an IFC class you pick, and parts show in plan, section and 3D.
- **Screen walls.** A screen wall is a wall type made of a repeating unit with holes in it, such as breeze blocks or a perforated panel. The holes go right through the wall in 3D and in plan.
- **Site.** The site tools add terrain with spot heights and contours, hard landscape areas (paving, driveway, gravel, decking, asphalt) with their materials and patterns, planting (tree, conifer, palm, shrub) as a plan symbol and a simple 3D form, and cars and people from the catalogue.

*File › New sample house in its garden* opens a demo with all of these. The CLI does the same with `creabim sample-garden garden.creabim.json`. The demo is the sample house with:

- a brick chimney stack, a firebox void that cuts both the stack and the house wall, and a hearth
- a screen wall with a coping sweep along its top
- ground and a mound with spot heights
- a patio, a path and a driveway
- trees and shrubs, a car and a person
- three new views: *Fireplace plan*, *Chimney section* and *Garden section*

| Screenshot | What it shows |
| --- | --- |
| [01-garden-3d.png](01-garden-3d.png) | The garden in 3D. The ground and the mound are in grass, and the patio, path and driveway sit in it. Next to the house are the brick chimney stack and the screen wall with its coping. Around them are conifers, round trees, a palm and shrubs. |
| [02-screen-wall-coping.png](02-screen-wall-coping.png) | Close up on the screen wall. Its repeating unit is pierced right through, and the coping is a sweep that follows the top of the wall. |
| [03-fireplace-plan.png](03-fireplace-plan.png) | The *Fireplace plan* at 1:50, cut at 500. The firebox void cuts a niche through the chimney stack and into the house wall. Also visible: the screen wall's holes, the stippled path, a tree symbol and the shrubs. |
| [04-chimney-section.png](04-chimney-section.png) | The *Chimney section* at 1:50. The stack is cut and hatched, with the firebox niche at its foot. The ground is cut too, and the hearth sits on the slab. |
| [05-garden-section.png](05-garden-section.png) | The *Garden section* at 1:200, across the whole site. The ground and the mound are cut, and the screen wall stands behind them with its holes. Trees, a palm, shrubs and the house are drawn beyond. |
| [06-site-plan.png](06-site-plan.png) | The Ground Floor plan at 1:100 shows the whole site: the ground outline, the mound's contours, the patio in units, the path in stipple, the driveway, and planting symbols for each form. |
| [07-create-menu.png](07-create-menu.png) | *Create* now lists Extrusion, Sweep, Void and Screen wall under Building elements. A new *Site* group holds Terrain, Hard landscape, Planting and Cars and people…. |
| [08-hard-landscape-tool.png](08-hard-landscape-tool.png) | The Hard landscape tool. Its bar has the presets (Paving, Driveway, Gravel, Decking, Asphalt) and the drawing modes (polygon, rectangle, fill room, from sketch). Here a decking area has just been drawn, and *Properties* shows its top, depth, pattern, spacing, angle and material. |
| [09-terrain-spot-heights.png](09-terrain-spot-heights.png) | The mound selected, with its outline grips (the same edit-mode grips as slabs). *Properties* shows its spot heights (x, y, height), *Add spot height* and the contour interval. |
| [10-planting.png](10-planting.png) | A birch selected in plan and in 3D. *Properties* shows its form, canopy, height, base and position. The person from the catalogue stands on the lawn, and the car is parked on the driveway. |

## Trying it

1. Open *File › New sample house in its garden*, or run `creabim sample-garden garden.creabim.json` and open the file.
2. Open *Fireplace plan*, *Chimney section* or *Garden section* from the project tree.
3. Select *Firebox* (search for it) and change its base or height in *Properties*. The cut in the stack and the wall follows, in plan, section and 3D.
4. Select the mound and drag a spot height, or add one. The contours and the 3D surface follow.
5. Use *Create › Screen wall* to draw a wall with a screen type. Its holes come from the type, so changing the type changes every wall of that type.
6. Export with *File › Export › IFC4 (.ifc)*. The parts are written as the class you picked, the voids as `IfcOpeningElement`, and the terrain as `IfcGeographicElement` (TERRAIN). An *IFC5 (.ifcx)* export reads back with every part and site element native.

## Known gaps

All open gaps, from every milestone, are kept up to date in [docs/KNOWN_GAPS.md](../../KNOWN_GAPS.md). The list below is as it was when this milestone landed; some of it may be fixed since.

- **IFC STEP import of parts and site comes back as plain shapes.** It does not bring them back as parametric elements, because main's STEP reader keeps no CreaBIM property set for them. IFCX round-trips them natively.
- **"Make a type of this" on a screen wall loses the screen pattern.** The new type is a solid wall type.
- **Parts that lie wholly above the plan cut are not drawn in plan.** For example, the coping on the screen wall. There is no overhead line style for them yet.
- **Sloped terrain does not leave holes for areas on its surface.** Areas cut the ground only when the terrain is flat. Over a mound, an area sits on the surface at its own top.
- **New hard landscape areas start at top 0.** The demo ground is at -150, so an area drawn on it needs its top set by hand.
- **Entourage (cars, people) is exported as `IfcFurniture`.** It has no visibility category of its own and is grouped with objects.
- **There is no preview of a sweep path while drawing.**
- **Wall-hosted sweeps have no corner grips.** They follow their wall, so to reshape one, reshape the wall.
- **Fine patterns (block paving) fill solid when zoomed far out.** Their lines do not thin out with the scale.
- **Plants in section are drawn with the facets of their 3D form**, not as a soft outline.
- **Voids are hidden in presentation 3D views**, together with openings. Framing a selected void there now explains why it can't be framed; turn on *Openings, shafts and void cutters* to see and frame it.
