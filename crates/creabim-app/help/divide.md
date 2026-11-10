# Divide

**Divide** splits an element into even parts: a wall into panels or courses, a beam into bays, a shelf unit into shelves. It can cut the element, put guides at the divisions, or place a copy of something in each part.

## Dividing an element

1. Select a wall, a beam, a guide, or an extrusion drawn from a plan outline.
2. In *Properties*, click **Divide › Into even parts…**.
3. In the **Divide** window, set:
   - **Parts**: how many;
   - **Which way**: **Along** (its length, in plan) or **Up** (its height: courses, bands, shelves);
   - **Make**: what goes at the divisions (below), and a **Gap** between the parts.
4. Orange marks in the 3D view show where the divisions fall. Click **Divide**.

The whole division is one undo step.

## What it makes

| Make | Gives |
| --- | --- |
| **Cuts** | The element cut into parts: a wall into walls side by side or stacked (a base course, bands), a part into bays or shelves |
| **Guides** | A guide across it at each division, to snap to (going up, a level guide along its face) |
| **Copies** | What you `Shift`+click, one in each part |

For **Copies**, select the element to divide first, then `Shift`+click what goes in each part. A sketch has to be extruded first, or draw a [guide](guides) along it.

> **Tip:** **Divisions** in the [snaps](snaps) list snaps to the even parts of any edge (its thirds, quarters, fifths…) without changing anything.
