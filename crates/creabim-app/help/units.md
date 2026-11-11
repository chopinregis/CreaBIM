# Units and number formats

## Project units

*File › Project units…*, or the **Units** menu in the status bar, sets how lengths are shown and typed in this project:

- **Metric**: millimetres, centimetres or metres, with the number of decimals;
- **Imperial**: feet and fractional inches, decimal feet, or inches, with the rounding.

A preview shows the result. Every field, readout, dimension, tag, schedule and sheet follows at once. The choice is saved with the project and can be undone.

## Number formats

Each kind of annotation can write its numbers its own way: **dimensions**, **level heads**, **spot levels**, **tags** and **schedules**. The status bar's **Units** menu lists each kind with a sample of how it writes now; click one, or *Number formats…*, to open the **Number formats** window. *Project* comes first, then each kind.

For a kind, every setting starts as *Use the project's*, and can be set on its own:

| Setting | Choices |
| --- | --- |
| **Unit** | any length unit, metric or imperial |
| **Decimals** (or **Rounding**) | 0 to 4 decimals; feet and inches round to a fraction |
| **Unit symbol** | show it, hide it, or as the unit writes it |
| **Zeros** | a leading zero (`0.450` or `.450`) and trailing zeros (`3.000` or `3`) |
| **Sign** (levels) | `+3` and `±0`, or the bare number |

*Looks like* shows a sample as you change them. *Reset to the project's* puts every field back. The number formats are kept in the [style pack](graphic-styles), so a set of drawing standards carries them.

A single dimension can override its kind's format in *Properties*. See [Dimensions](dimensions).

## Typing lengths

Any length field takes a number in the project's units, or any unit written out:

| You type | In a metric project (mm) |
| --- | --- |
| `1200` | 1200 mm |
| `1.2 m` | 1200 mm |
| `120 cm` | 1200 mm |
| `4'` | 1219 mm |
| `3'-6 1/2"` | 1080 mm |

In a feet-and-inches project, `3 6` reads as 3′-6″ and `42"` as 3′-6″.

> **Note:** Paper sizes, text sizes and line weights on sheets stay in millimetres, as is usual for drawings.
