# Sketches and the workplane

A **sketch** is a flat drawing of lines, arcs and circles on a plane. A closed sketch can be extruded into a mass, or used as the outline of a slab, roof or shaft.

## Drawing a sketch

1. Pick the **Sketch** tool.
2. In 3D, click a face to sketch on it (or the ground). In a plan or section, click on the drawing.
3. Click the corners. `L`, `A`, `C` and `R` switch between lines, arcs (click its end, then a point it passes through), circles (click the centre, then a point on it) and rectangles (two opposite corners, square to the sketch plane, with its width × height shown while you draw). A circle or a rectangle is a sketch of its own.
4. Click the first corner to close the sketch, or press `Enter` or right-click to leave it open. `Esc` cancels.

`Shift` keeps lines square.

## Constraints

Select a sketch to add constraints in *Properties*: length, equal, horizontal, vertical, parallel, perpendicular, coincident and radius. The sketch moves to meet them, and you can drag a constraint's value to change it.

## Extruding

A **closed** sketch is a face: it has a light wash inside its outline, in 3D, plans and sections, and *Properties* says *Closed: a face. Extruding it makes a solid.* An **open** one is a wire, and extrudes into a surface. A closed sketch that crosses itself has no inside: no wash, and *Extrude into a mass* is greyed out until no lines cross.

Select a closed sketch and press `E`: it becomes a mass, extruded by *Extrude by* in *Properties*. An open sketch extrudes into a surface, which a Solidify [modifier](masses#modifiers) can thicken.

## The workplane

The **workplane** is the plane that sketches and guides drawn in 3D go on. Pick it in the status bar's *Workplane* list:

- *Level (follows the plan)*: the working level, the usual choice;
- *Pick a face…*: click a face in 3D;
- *Through three points…*: click three points;
- *Level plane you can offset*: a level plane you slide up and down.

Or right-click a face and choose *Set workplane here*.
