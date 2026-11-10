# Masses, modifiers and booleans

A **mass** is a free 3D shape you can model like in Blender: for early design studies, or anything that isn't a wall, slab or roof.

## Making a mass

- Pick the **Mass** tool and click to place a box.
- Or extrude a closed [sketch](sketches) with `E`.

## Editing its shape

- Click a face in the 3D view to select it, then press `E` to extrude it, or move it with `G`.
- Use [Move face](move#move-a-face-push-and-pull) to push or pull a face by a typed distance.
- Rotate, tilt and scale it with `R` and `S` (see [Move, rotate and scale](move)).

Selected, a mass shows only its main grips: its base point (moves it), its turn grip and the clicked face's push and pull arrow. Every face's arrow, its corners, its edges and the lift arrow come in [Edit mode](edit-mode) (`Tab`).

*Properties* shows its **Location**, **Rotation (XYZ Euler)** and **Scale**, as Blender's Transform panel: a mass turns about X, Y and Z, so a shape drawn on a wall turns about the wall's axes.

Give a mass an IFC class in *Properties* (such as `IfcRoof`) and it is exported as that.

## Modifiers

Modifiers change a mass's shape without touching it, so you can change or remove them later. Select a mass and open the **Modifiers** tab of *Properties* (the wrench), then **Add modifier**:

- **Array**: copies in a row (a pergola's rafters);
- **Mirror**: a mirrored copy;
- **Solidify**: gives a surface a thickness;
- **Subdivision**: smooths it.

Each modifier is a card, as in Blender: fold it, rename it, show or hide it, and its menu has **Apply** (make it part of the shape), **Move up** and **Move down**; the cross removes it. Open, its settings are rows. The *Element* tab says how many modifiers the mass has.

## Booleans: cutting one shape from another

Select a wall, slab or mass, then under *Booleans* pick *Subtract…* and the element to cut away. The cutter stays as a see-through ghost you can select and move to change the cut. Switch it to *Union* or *Intersect*, or *Remove* it.
