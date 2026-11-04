# Joins

Walls, slabs, roofs, columns, beams, footings and ceilings can meet. Where they do, plans and sections clean up the junction: the overlap is drawn once, with one outline, and the layers that run through are drawn running through.

## Automatic joins

You don't have to set anything. Each pair of layers that meet is compared, and the stronger layer cuts through the weaker one:

1. **Structure** (block, concrete, timber frame) is the strongest.
2. **Substrate** (sheathing, battens) comes next.
3. **Insulation**, **air gaps** and **membranes** come after.
4. **Finishes** (plaster, render, cladding) are the weakest.

A layer's function is set in its [assembly](assemblies). When two layers are equally strong, the kind of element decides: a column cuts a wall, and a wall cuts a slab. When the elements are of the same kind, the one drawn first wins. Two layers of the same material that meet merge into one, with no line between them.

For example, where a floor slab meets a wall, the slab's concrete runs into the wall's block and stops at its insulation. The wall's plaster stops at the slab.

## Wall junctions

Walls that meet at their ends join as a corner, a T or a cross:

- **Two walls** meet in a mitred corner.
- **Three or more walls** that end at the same point make a node. The two walls that run most nearly straight through the node continue through it as one wall. The other walls stop at those walls' faces, and their structure runs on into the through walls' structure. If no two walls run straight, the thickest pair goes through.
- A wall that ends on the side of another wall makes a T.

Walls join at a node only when they are drawn to the same point. Ends that stop short of each other are left unjoined.

## Setting a junction by hand

Select the two elements (Shift+click the second), then right-click:

- **Switch join order**: the other element now cuts through where the two overlap.
- **Don't clean up the junction**: both elements are drawn whole, each with its own outline. Pick it again to clean the junction up.
- **Join automatically**: goes back to the automatic join, by layer priority.

Junctions set by hand are listed under **Junctions** in each element's *Properties*, where you can also change their order. Each change is one step for Undo.
