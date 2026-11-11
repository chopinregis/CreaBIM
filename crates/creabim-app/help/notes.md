# Notes

A **note** is free text on a drawing or a sheet, with or without a leader arrow pointing at something. The project also has a **notepad** of its own, for notes and to-dos that never print.

## Writing a note

1. Pick the **Note** tool (`N`, with nothing selected).
2. With **Leader** on (in the tool options), click what the note points at, then where its text goes. With *Leader* off, one click places the text.
3. Type the text in *Properties*: the text box is ready to type into. `Enter` adds a line.

If the leader's point snapped onto an element, it follows that element when it moves.

## Long text

- **Wrap at**: give the note a width on paper, and its lines wrap to it. Off, each line runs as long as it is.
- **Markdown**: the note reads `# heading` (in capitals, underlined), `- ` bullets, `1. ` numbered lines and `- [ ]` / `- [x]` checkboxes. A list item's later lines are set in under it. `**bold**` and `*italic*` marks are dropped on paper.
- The text box in *Properties* scrolls; the button at its corner pops it out into a large editor. Double-clicking a note on a sheet opens the large editor too.
- An area set to **Notes** (its view-type menu) shows the selected note's text large, as Blender's text editor does, with *Lists and headings*.

## On sheets

Pick the Note tool on a sheet, outside any viewport, to write on the paper itself: general notes, title notes. Its **X** and **Y** can be typed, in millimetres on the sheet (in a view, as lengths).

## Moving and styling

With the Select tool, drag a note's text to move it, its square grip at the arrow to move the leader's end, and its round grip to bend the leader at an elbow (**Straighten the leader** in *Properties* takes the elbow out). **Leader end** in *Properties* picks an arrow, a dot or nothing for this note. Pick a **Style**, or an *Own colour*, as for dimensions; the note's text style can be edited in its own section. See [Annotation styles](styles).

Notes are a visibility category: a view can hide them (see [Visibility](visibility)).

## The notepad

The project's **notepad** holds **To-dos** and **Notes**: what to check, what the client asked, what to draw next. It is kept in the project file and never printed. Open it from the **Notes and to-dos** tab of *Properties*, or show it large in an area: set the area's view-type menu to **Notes** with no note selected.

- Type in the line at the top and press `Enter` to add an item, as a to-do or a note.
- Each item is one row: a tick (to-do) or a dot (note), its first line, then its link, edit and delete at the right. Click a row's text to edit it in place; click away to close it.
- **Link to the selection** ties an item to the element selected, or else to the view in front. Its link button then selects the element or opens the view.
- The **⋯** menu: *Copy all*, *Save as text (.txt)…*, *Save as Markdown (.md)…* and *Remove the done to-dos*.
