# Dimensions

Dimensions measure the model. Their points stick to what they measure (wall corners, door and window edges, levels, grids, columns, masses), so they update when it moves.

## Temporary dimensions

Select one wall, grid line, door, window or column in a plan. Blue dimensions show the distance to its nearest neighbours: the parallel walls and grid lines beside a wall or grid line, the nearest grid lines or walls to a column, and the wall's ends or the next opening beside a door or window.

- **Click a value and type a new one**, then `Enter`. The element moves to make it true. A wall takes its doors and windows and the walls joined to it along; a grid line takes the walls and columns standing on it. `Tab` moves on to the next value, `Backspace` takes a character back and `Esc` stops.
- **Click the padlock** beside a value to keep it as a dimension that holds the spacing. Moving either end then moves the other by as much. Click a closed padlock to let it go; the dimension stays.

The typing works like typing a length while drawing a wall. Nothing is added to the drawing unless you lock a value.

## Linear dimensions

1. Pick the **Dimension** tool.
2. Click two points to measure. They snap to corners and edges.
3. Click where the dimension line goes.

In the tool options choose the direction:

- **Auto**: from where you drag the line. Beside both points it measures vertically, above or below them horizontally, otherwise along the two points. Hold `Shift` to keep the one shown.
- **Aligned**: the true distance between the points.
- **Horizontal** or **Vertical**: the distance across or up the drawing.

A placed dimension's *Measures* in *Properties* changes it later. Drag a selected dimension to move its line.

## Angles

Pick the **Angle** tool. Click two lines (wall faces, edges), then where the arc goes. Or click the corner and one point on each arm. The side you place the arc on picks the inside or outside angle.

## Radius and diameter

Pick **Radius** or **Diameter**. Click an arc or circle (a door swing, a sketch arc), or three points on one, then where the dimension goes.

## Arc length

Pick **Arc length**. Click an arc, or its start, a point on it and its end, then where the dimension goes.

![A radius and an arc length on a door swing](img/dimensions.jpg)

## How they look

Pick a **Style** in the tool options for new dimensions; change a placed one's style or *Own colour* in *Properties*. See [Annotation styles](styles).

## Breaking around tags

A style with **Break dimension lines at tags** on leaves a gap in its dimension lines wherever a tag's bubble or a note's text sits across them, so a chain of dimensions stays readable past the door tags on it. Dimensions in other styles run straight through.

To write the height of a point rather than a distance, use a [spot level](spot-levels).

Dimensions work in plans, sections and elevations, show on sheets and print in PDF and SVG.
