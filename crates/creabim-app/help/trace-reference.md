# Trace and reference

A **reference** is something shown faintly under (or over) a plan or a section, to draw over or compare against:

- **another view**: the floor below, under the floor you are drawing;
- **a picture**: a scanned plan, a photo of a sketch (PNG or JPEG);
- **a DXF drawing**: a survey or a consultant's plan. A DWG opens once saved as DXF: any CAD program, or the free ODA File Converter, does it offline.

A reference is saved with the project. One reference can show in **several views** (a site survey under every floor). Pictures and drawings stay where they are on disk, as links: *Reload* reads them again after they change.

## Adding one

**Reference** on the *View* tab opens the **Trace and reference** tab of *Properties* for the view in the area you last used. Pick another view from *Add*, or click **Picture…** or **DXF drawing…**. It is added in place and selected. A picture or DXF file dropped on a plan or section becomes a reference too.

## Placing it

- Drag the **square grip** in its middle to move it.
- Drag a **corner** to scale it about its middle.
- Drag the **round grip** above it to turn it about its middle (5° steps; `Alt` for any angle).
- Or type *X*, *Y*, *Rotation* and *Scale* in *Properties*.
- **Calibrate…**: click two points on it a known distance apart (a dimensioned wall on a scan), then type that distance. It is scaled to fit; the first point stays put.
- **Align…**: click two points on the reference, then the two points on the model they belong on: it is moved, turned and scaled onto them.
- **Back in place**: where it was drawn, unmoved, unturned, at its own size.
- **Move**, **Rotate** and **Scale** from the toolbar work on references as on any element.

In a section, a reference stands on the section's plane, and its grips and moves stay on that plane. Its lines snap: ends, middles and anywhere along them.

Its dashed outline shows when it is selected or under the pointer, in plans and in 3D; clicking the outline selects it. **Locked** keeps the grips off.

**Copy to view…** puts a separate copy of the same drawing in another view, with its own place, size and settings.

A trace placed in a [Profile view](profiles#the-profile-view) is kept to that profile: **Only in this profile** is ticked, and it is not drawn in plans, sections or 3D. Untick it to choose the views it shows in, as for any other reference.

## How it looks

- **Colour**: red, blue, grey or green, or *its own colours* (a picture's).
- **Strength**: how faint it is.
- **Under** or **Over** the model.
- **Hidden**, or `Alt+F2` to hide or show every reference in the view.

## In 3D

Under **In 3D** in its *Properties*: **Shown** draws the reference in the 3D view too, on its level or its section's plane (a picture shows its image), where it can be picked and snapped to. It can have its **Own strength** there, and be drawn **In perspective** or flat.

References are never in exports, schedules or IFC.
