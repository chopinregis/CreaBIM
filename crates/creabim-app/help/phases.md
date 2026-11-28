# Phases

One model, three moments: **what is there**, **what comes down**, **what is new**. A view stands at one phase and shows the building at that moment.

![The same house at Existing, Demolition and Proposed: two rooms as surveyed; the partition and part of the east wall in red dashes; the partition gone and an extension built, the existing walls fainter](img/phases-plans.jpg)

| Phase | What the view shows |
| --- | --- |
| **Existing** (grey tag) | the building as surveyed |
| **Demolition** (red tag) | what comes down, in red dashes |
| **Proposed** (green tag) | the new work, with the existing fainter behind it |

**Where to find it**

- **The phase tag** in an area's header, beside the view's name: click it to step the view through time.
- **Properties › Phases**: the phases, what came down and how they draw.
- **Properties › Element › Phase**: when one element is built and taken down.

## Start phases

![The Phases tab of a project with no phases yet: It is the existing building, It is the proposed building](img/phases-start.jpg)

1. Open the **Phases** tab of Properties (its icon is the stack of layers on the tab rail).
2. Say what you have drawn so far, usually **It is the existing building**. **It is the proposed building** makes it new work instead.

The project gets three phases, first to last: **Existing**, **Demolition** (a phase that takes things down) and **Proposed**.

## Step a view through time

![The phase tag on the Existing plan, its menu open: Stands at Existing, Demolition, Proposed; Make it the view's phase; Phases…](img/phases-tag.jpg)

Click the tag beside the view's name and pick a phase under *Stands at*. Its colour tells you where you stand: grey at the first phase, red at a phase that takes things down, green at one that builds.

- The tag moves **this area only**, as its zoom is its own: the view, its other areas and its sheets stay where they are. So the existing and the proposed plan can sit side by side, one view in two areas.
- **Make it the view's phase** moves the view too, and its sheets with it. **Back to the view's phase** returns the area. The view's own phase is also *View* tab › **Phase** › **View at**.
- **Phases…** opens the Phases tab.

> **Tip:** The 3D view stands at a phase of its own, from its own tag.

![The house in 3D at Existing, Demolition and Proposed](img/phases-3d.jpg)

## The three steps

1. **Draw what is there.** Stand at **Existing** and draw the building as surveyed. What you draw is built in Existing.
2. **Take things down.** Stand at **Demolition**, select what goes and press `Delete`. It turns to red dashes and stays in the model. A wall takes its doors and windows with it, and openings left in walls that stay are listed as *making good*, with the area to fill.
3. **Draw what is new.** Stand at **Proposed** and draw the new work. What came down is left out; the existing draws fainter behind it.

Deleting existing work while standing at Proposed takes it down in the latest phase before it that takes things down (Demolition).

### No red dashes? Look at the tag.

![Left: the partition deleted at Existing, gone with no trace. Right: the partition deleted at Demolition, in red dashes](img/phases-mistake.jpg)

A `Delete` at **Existing** really deletes: the element was built in that very phase, so it was never there. The status bar says so. To take it down instead: `Ctrl+Z`, step the tag to **Demolition**, then `Delete` again.

## The Phases tab

![The Phases tab for the Demolition plan, its sections numbered 1 to 5, and its icon on the tab rail](img/phases-tab.jpg)

The tab speaks for the view you last clicked (its **View** line).

1. **Phases**: each with what it builds or takes down. Click one to stand the view at it.
2. The picked phase's **Name** and **Takes down**; **+ Phase** adds one after it, **Earlier** and **Later** move it in time, **Remove** takes away an empty one.
3. **Existing work** › **Changes**: what moving an existing element from a later phase means (below).
4. **Options**: design questions with two or more answers (below).
5. **Taken down**: everything that came down. Click rows and **Bring back**, or **Bring all**. Doors and windows come back with their wall, and nothing drawn since is undone.

Further down, **How they draw** sets the office's look for every view, sheet and the 3D view: the colour of what comes **Down** and **Dashed**; **Existing** › **Fainter**; **New** › **One colour**. **Reset** goes back to red dashes, existing fainter, new in its own colours.

### Correct or Alter

Moving or editing an existing wall from a view at Proposed:

| Changes | Means | What happens |
| --- | --- | --- |
| **Correct** | the survey was wrong | the element changes in every phase |
| **Alter** | the building is being changed | the old one comes down, the changed one is built in the phase the view stands at; `Ctrl+Z` twice puts it back |

## One element's phases

![The Element tab of a partition: its Phase block reads Built in Existing, Down in Demolition, In view to take down, with Bring back](img/phases-element.jpg)

Select an element: *Element* tab › **Phase**.

- **Built in** and **Down in** (or *Never*) move it in time without redrawing it.
- **In view** says what it is in the view in front: *as surveyed*, *existing*, *to take down*, *new in …* or *not here*.
- **Bring back** stands it again in every phase, with what came down with it.

## Sheets for a planning application

Duplicate the floor plan for each phase and stand each copy at one: *Existing*, *Demolition*, *Proposed*. Place the three on one sheet side by side.

## In schedules and IFC

- A phased project's [schedules](schedules) end with their totals split by status: *Existing kept*, *Taken down* and *New*.
- An IFC export writes each element's standard **Status** (EXISTING, DEMOLISH or NEW) in its common property set, and the phases by name, in order, in *CreaBIM_Phasing*.
- An IFC import reads them back: CreaBIM's own phases first, then the phase names other programs write, then the standard Status, which comes in as *Existing*, *Demolition* and *Proposed*. A file that says nothing stays without phases.

## Options

**Options** in the Phases tab hold design questions with two or more answers (*Option A*, *Option B*). **Hold shared parts** keeps what all options share from being moved or changed by accident while you work in one. A view shows one option and works in it, from its tag's menu. See the tag's **Working in** list and **Ghost the other options**.
