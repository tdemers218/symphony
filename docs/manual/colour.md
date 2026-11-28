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

- **Wheel** (the default): drag the ring to choose the hue, and the square
  inside it for saturation (left to right) and brightness (bottom to
  top).
- **Square**: a larger square with the hue bar beside it.
- **Perceptual**: **Lightness** (up) and **Chroma** (right) for a hue,
  in the OKLCH colour model. Equal steps look like equal changes, and
  making a colour lighter or darker keeps its hue. The shape's right edge
  is the most colourful your screen can show at that hue; clicking past
  it picks the edge.

Under the picker:

- The split chip shows the new colour on the left and the colour you
  started with on the right. Click the right half to go back.
- The hex field shows the colour as `#RRGGBB`. Type or paste one;
  `#rgb` works too.
- The dropper button picks a colour from anywhere in the window: click
  it, then click a pixel. `Escape` cancels.
- **HSB** or **RGB** sliders, with exact values to type.
- **Opacity** is the paint's own transparency.
- The row of small chips is your recent colours. Click one to use it.
- **Colour Lab…** opens the [Colour Lab](#colour-lab) on this colour.
- **Preview with a blend mode** shows the paint over white, mid grey or
  near black, with a layer **Blend** mode, at the colour's opacity. It
  uses the same maths as the render.

Symphony remembers the layout and the slider type you used last.

### Quick colour picker

Press `Q` (Symphony profile) to open the editor for the current colour
right at the pointer, without going to the Colour view. Press `Q` again,
press `Escape` or click outside to close it; the click that closes it
doesn't draw. In the Harmony profile it has no shortcut: search (`Tab`)
for "Colour picker", or give it one.

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

## Colour Lab

The Colour Lab finds shading colours for a base colour by lighting it. Open
it with **Colour Lab…** in a colour editor, from **Windows**, or by
searching (`Tab`) for "Colour Lab". It opens as a floating window; dock it
like any view.

It shows a **Sphere** or a **Cube** in your colour on a floor, lit by:

- a **Main light** and an optional **Second light**, each with a colour
  and a strength. Drag the dot on the round dial to set where a light
  comes from, seen from above: you stand at the bottom of the dial, the
  middle is straight overhead, and the top edge is behind the object (a
  rim light). **Height** sets how high it is;
- **Sky light**, light from all around that fills the shadows;
- **Bounce**, light the floor throws back up onto the object, in the
  floor's colour;
- **Cast shadows** on the floor, and **Highlight** and **Gloss** for the
  shine.

**Smooth** shades gradually; **Cel** gives flat tones the way cel
animation paints them (**Tones** sets how many).

Click the picture to pick a colour. Then:

- **Add to palette** adds it as a flat colour, or
- paint it as a layer over the base colour: the list shows, for each
  blend mode that can make the colour, the colour to paint with at the
  **Opacity** you set. **Add** puts that colour in the palette. Modes
  that can't make it are listed underneath: Multiply only darkens and
  Screen only lightens, for example, and some need a higher opacity.

The recipes use the same blending as the render, so a Multiply layer
painted with the listed colour at that opacity gives the picked colour.

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
