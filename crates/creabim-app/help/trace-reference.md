# Trace and reference

A **reference** is something shown faintly under (or over) a plan or a section, to draw over or compare against:

- **another view**: the floor below, under the floor you are drawing;
- **a picture**: a scanned plan, a photo of a sketch (PNG or JPEG);
- **a DXF drawing**: a survey or a consultant's plan. A DWG opens once saved as DXF: any CAD program, or the free ODA File Converter, does it offline.

A reference is saved with the project. One reference can show in **several views** (a site survey under every floor). Pictures and drawings stay where they are on disk, as links: *Reload* reads them again after they change.

## Adding one

*View › Reference* opens *Properties › Trace* for the view in the focused area. Pick another view from *Add*, or click **Picture…** or **DXF drawing…**. It is added in place and selected.

## Placing it

- Drag the **square grip** in its middle to move it.
- Drag a **corner** to scale it about its middle.
- Drag the **round grip** above it to turn it about its middle (5° steps; `Alt` for any angle).
- Or type *X*, *Y*, *Rotation* and *Scale* in *Properties*.
- **Calibrate…**: click two points on it a known distance apart (a dimensioned wall on a scan), then type that distance. It is scaled to fit; the first point stays put.
- **Back in place**: where it was drawn, unmoved, unturned, at its own size.

Its dashed outline shows when it is selected or under the pointer; clicking the outline selects it. **Locked** keeps the grips off.

## How it looks

- **Colour**: red, blue, grey or green, or *its own colours* (a picture's).
- **Strength**: how faint it is.
- **Under** or **Over** the model.
- **Hidden**, or `Alt+F2` to hide or show every reference in the view.

References are never in 3D, exports, schedules or IFC.
