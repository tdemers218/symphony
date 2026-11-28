# Colour and paint

Colours in Symphony come from the scene's **palette**. A line or fill
remembers which swatch it was painted with, not just the colour. Edit the
swatch and everything painted with it changes, in every drawing.

## The palette

The **Colour** view shows the scene's swatches, as a **List** (a chip and
a name per colour) or as **Swatches** (a grid of squares, the name when you
hover). The switch is at the top right of the view, and Symphony remembers
your choice.

- In the list, click a row to make that colour the current colour.
  Double-click its name to rename it. Click the chip to open the colour
  editor.
- In the grid, click a square to make it the current colour, and
  double-click it to open the picker. The current colour has a ring
  around it.
- A see-through colour shows a checkerboard behind it, so a 40% swatch
  looks 40%.
- The **+** and **−** buttons add a colour and remove the current one.
  Removing a colour that's in use asks which colour its strokes and fills
  should take instead.
- Right-click a colour to **Rename** it, **Move up** or **Move down**,
  or **Protect Colour**. A red bar marks a protected colour.

## The colour editor

Changes apply as you make them, to everything painted with the swatch.

- **Square** and **Triangle** pick saturation and brightness for a **Hue**.
- **Perceptual** picks **Lightness** (up) and **Chroma** (right) for a hue
  (the OKLCH colour model). Equal steps look like equal changes, and
  making a colour lighter or darker keeps its hue, which Square and
  Triangle don't. The shape's right edge is the most colourful your
  screen can show at that hue; clicking past it picks the edge.
- **Before · Now** shows the colour the editor opened with next to the
  current one. Click **Before** to go back to it.
- **Opacity** is the paint's own transparency.
- The square at the bottom previews the paint as it will look: over
  white, mid grey or near black, with a layer **Blend** mode, at the
  colour's opacity. It uses the same maths as the render.

Symphony remembers the layout you used last.

### Three kinds of opacity

- **Colour opacity** (in the colour editor) belongs to the swatch. Every
  brush, and fills, paint it at that strength.
- **The brush's own strength** belongs to the brush: a Textured Vector
  brush's **Flow** and **Pressure fades opacity** in Tool Properties, and
  the opacity an imported brush brings with it.
- **Opacity** and **Blend Modes** nodes (see [Nodes](nodes.md)) apply to
  the whole layer, after everything in it is drawn.

They multiply: a brush at 50% with a colour at 50% paints at 25%.

To recolour a character, edit the swatch rather than repainting: every
line and fill linked to it updates together.

## Paint

The **Paint** tool fills closed areas with the current colour. Click a
line to recolour the line.

| Mode | Symphony | Harmony | Does |
| --- | --- | --- | --- |
| Paint | `P` | `Alt+I` | Fills an area, or recolours it if it's already painted. |
| Paint Unpainted | `Y` | `Alt+Y` | Fills only empty areas and leaves painted ones alone. |
| Repaint | `R` | `Alt+R` | Recolours only areas that are already painted. |
| Unpaint | `U` | `Alt+U` | Removes the colour from an area. |

Options in **Tool Properties**:

- **Keep new fills linked to lines**: new fills follow their lines when
  you move or reshape them with the Line Editor. If an edit opens the
  area, the fill keeps its last closed shape.
- **Respect protected colours** (`Shift+S`): painting leaves protected
  colours alone.

Paint fills on the current art layer. Paint on **Colour Art** to keep
fills separate from the ink on **Line Art**; see
[Art layers](drawing.md#art-layers).

## Close gaps

An area with a gap in its outline can't be filled. With **Close Gap**
(`G` [`Alt+C`]), draw a rough line near the gap. Symphony joins the two
nearest line ends with an invisible stroke, and the area can be painted.

**Show strokes** (`K`) shows those invisible strokes. It's also in
**Preferences → Canvas**.

## Pick a colour

- **Dropper** (`D` [`Alt+D`]): click a line or painted area to make its
  swatch the current colour.
- **Pick colour from screen** (macOS, in the Dropper's Tool Properties)
  opens the system colour sampler. Symphony selects the matching swatch,
  or adds one.

## Protect colours

Protect a swatch (right-click it → **Protect Colour**) to keep paint off
it, for example the line colour while you fill. Protection applies while
**Respect protected colours** is on (`Shift+S`, or **Drawing → Colour
Protection**).

## Still to come

- Palette lists and palette files you can share between scenes.
  **Planned**.
- Gradient and texture swatches, and the Ink tool. **Planned**.
