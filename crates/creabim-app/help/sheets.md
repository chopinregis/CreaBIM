# Sheets and title blocks

A **sheet** is a page for printing, from A0 to A4, with a title block. Views placed on it are drawn from the current model, so they are never out of date.

## Making a sheet

*File › New sheet*. Open sheets from the browser's *Sheets* group or the view switcher.

## Placing views

1. Open the sheet.
2. Click **Place view…** in the top bar and pick a view: plan, section, elevation, 3D view or schedule.
3. Drag it where it belongs on the paper.

Each placed view (a **viewport**) shows its view at the view's scale, with what the view hides left out.

- **Scale** a viewport: select it and pick a scale in *Properties* (1:1 to 1:500, or type any other after *1:*), or right-click it and choose **Scale**. The viewport grows or shrinks about its middle. The scale is the view's own, so the view shows at that scale on every sheet it is placed on.
- **Crop** a viewport: select it and drag its dashed crop frame's edge handles, or set the crop in *Properties*.
- **Work through it**: double-click a viewport to edit the model right there, with every tool; drag to orbit a 3D view. `Esc` finishes.

![A sheet beside the live 3D view](img/sheet.jpg)

## Notes on the paper

Pick the [Note](notes) tool on a sheet, outside any viewport, to write on the paper itself.

## Title blocks and templates

Fill in the title block in the sheet's *Properties*. To change the title block, open *File › Sheet templates…*:

- *Copy of the built-in block*, to change the usual one;
- *Import DXF or SVG…*, to use your own drawing;
- or import another project's templates and styles.

Any `{name}` in the template's text becomes a field. `{sheet}`, `{number}`, `{project}`, `{scale}` and `{date}` are filled in by each sheet; other fields get a value per sheet in its *Properties*. Use *Use on this sheet*, *Use on every sheet* or *New sheet from it*, or pick the template in a sheet's *Title block*.

## Printing

*File › Export › Sheets as PDF* opens **Export sheets** (below): all sheets, chosen ones or this one, as one PDF or a PDF each. *File › This view or sheet as SVG* writes the open sheet. See [PDF, SVG and DXF](dxf-svg).

## Text, north arrow and scale bar

What belongs to the paper, not to a view, goes on the sheet itself, from the sheet's header:

- **Text** (`N`): click where it starts, then type. It is a note on the sheet.
- **North arrow**: placed above the title block, turned to the project's true north (*Project information*). Give it an angle of its own in *Properties*.
- **Scale bar**: placed above the title block at the scale of the sheet's views. Set its scale, length, parts and thickness in *Properties*.

Drag either one to where it goes; `Delete` removes it. Dimensions, tags and notes about the building stay in the views, so they show on every sheet a view is placed on.

## Exporting sheets

- **Ctrl+P**, or the PDF button in the sheet's header: the sheet in front, as a PDF named by its number and name.
- **Ctrl+Shift+P**, or *File › Export › Sheets as PDF*: the **Export sheets** window. Choose **All**, **Chosen** (the sheets selected in the project browser, or ticked in the list) or **This one**, and **One PDF** or **A PDF each** (into a folder, each named by number and name). Sheets print on white at their own sizes.
