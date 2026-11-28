# Colour and paint

Colours in Symphony come from the scene's **palette**. A line or fill
remembers which swatch it was painted with, not just the colour. Edit the
swatch and everything painted with it changes, in every drawing.

## The palette

The **Colour** view shows the scene's swatches.

- Click a colour's name to make it the current colour. Double-click the
  name to rename it.
- Click the colour chip to open the picker, **Square** or **Triangle**,
  with **Opacity**. Changes apply as you make them.
- The **+** and **−** buttons add a colour and remove the current one.
  Removing a colour that's in use asks which colour its strokes and fills
  should take instead.
- Right-click a name to **Rename** it, **Move up** or **Move down**, or
  **Protect Colour**. A red bar marks a protected colour.
- A swatch's **Opacity** is its maximum coverage. A pressure-sensitive
  brush's opacity multiplies it.

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
