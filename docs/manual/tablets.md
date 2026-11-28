# Pens and tablets

Symphony reads the pen directly: pressure, tilt, and the eraser end.

## What works where

| System | Pressure and tilt | Notes |
| --- | --- | --- |
| macOS | Yes | Any tablet whose driver reports pressure to macOS. |
| Windows | Not yet | The pen works as a mouse. Windows Ink and Wintab support is **Planned**. |
| Linux | Not yet | The pen works as a mouse. Wayland and X11 tablet support is **Planned**. |

**Tool Properties** shows the pen's state under **Tablet**: "Pen in
range" while the pen is near the tablet, or "No pen support on this system
yet. Using the mouse."

## Tune the pen

- **Pen pressure response** (**Tool Properties → Tablet**, or
  **Preferences → Drawing**): lower values make light strokes heavier; 1.0
  is linear. Lower it if you press lightly and lines come out thin.
- **Minimum size** and **Maximum size** on each brush set the width range
  that pressure moves through.
- **Smoothing** and **Stabilization** steady shaky lines; see
  [Brush](drawing.md#brush).
- Turn the pen over to erase with any tool.

## Tablets people have tested

| Tablet | System | Pressure | Tilt | Rate |
| --- | --- | --- | --- | --- |
| Veikk (model to be confirmed) | macOS | 244 levels | Yes | about 340 Hz |

The up-to-date table is in [TABLETS.md](../TABLETS.md).

## Report your tablet

Every report helps Symphony support more tablets.

1. Run the pen test. It needs a copy of the source code and Rust:
   `cargo run --release -p symphony-pentest`. Draw a few strokes, light
   and heavy, tilted and upright.
2. [Open a Tablet report](https://github.com/tdemers218/symphony/issues/new?template=tablet-report.yml)
   with the tablet's brand and model, your system and driver version, the
   pen test summary, and whether drawing felt right.
3. Attach `target/pen-log.csv` if you can. Logs from real tablets become
   tests, so the tablet keeps working in later versions.

## Drawing on a phone or tablet screen

Using an iPad with Apple Pencil, or an Android tablet with S Pen, as a
drawing surface and control panel for Symphony on your computer is
**Planned**. See [Planned features](planned.md#remote-workspace).
