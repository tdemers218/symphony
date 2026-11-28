# Drawing

Everything you draw in Symphony is vector: lines stay editable after you
draw them, and textured brushes are redrawn rather than stretched when you
move or scale a line.

You draw on the **current drawing** of the **selected layer** at the
playhead, in the Camera or Drawing view. If the frame is empty, drawing
creates a new drawing there.

## Drawing tools

Keys are Symphony profile [Harmony profile]. A dash means search for it
with `Tab`.

| Tool | Key | What it does |
| --- | --- | --- |
| Brush | `B` [`Alt+B`] | Pressure-sensitive lines, drawn as filled shapes. Vector or Textured Vector. |
| Pencil | – [`Alt+/`] | A centre line with a thickness, with round or flat tips. In the Brush button's variants. |
| Line | `L` [–] | A straight line in the Brush's style. |
| Text | `T` [–] | Type text. It becomes vector letter shapes you can edit later. |
| Eraser | `E` [`Alt+E`] | Erases lines and paint. |
| Select | `S` [`Alt+S`] | Select, move, scale, rotate and cut artwork. |
| Line Editor | `N` [–] | Push, smooth, thicken or thin lines with a soft brush. |
| Paint, Close Gap, Dropper | see [Colour and paint](colour.md) | |
| Transform | `X` [`Shift+T`] | Move layers and pegs; see [Keys, pegs and the camera](animating.md). |
| Hand, Zoom | `H`, `Z` [–, `Alt+Z`] | Pan and zoom. Hold `Space` to pan with any tool. |

The pen's eraser end erases with any tool.

## Brush

**Tool Properties** for the Brush:

- **Vector / Textured Vector.** Vector is a solid line. Textured Vector
  adds a tip, paper grain, spacing, flow and opacity, and stays a vector
  line you can reshape.
- **Maximum size** and **Minimum size.** Pen pressure moves between the
  two. Set them equal for a line that ignores pressure.
- **Smoothing** filters small wobbles in the path.
- **Stabilization** (**Off**, **Light**, **Strong**) lets the pen lead and
  the line follow, for long steady curves.
- **Preserve sharp turns** uses less smoothing where the pen changes
  direction sharply.
- **Presets.** Save the current settings as a preset, and import or
  export preset files (`.symphonypresets`). Brush packs from other apps
  import too; see [Import and export](import-export.md#brushes).

Textured Vector adds: **Spacing**, **Flow** (below 1, each stamp lays down
a little and overlaps build up), **Hardness**, **Pressure fades
opacity**, **Tip angle**, **Turn tip with the stroke** and **Paper grain
scale**.

`+` and `-` change the brush size while the pointer is over the canvas
(Symphony profile).

## Pencil

The Pencil draws a centre line with a thickness, like Harmony's Pencil.
Choose it from the Brush button's variants. **Start tip** and **End tip**
are **Round** or **Flat**.

A **thickness stencil** fixes the thickness along a line regardless of
pressure. Select a pencil line you like and choose **New thickness
stencil** to save its thickness as a Pencil preset. **Remove thickness
stencil** goes back to pressure.

## Line

Drag to draw a straight line in the current Brush style.

- Hold `Shift` while dragging to snap the angle to 22.5° steps.
- Hold `Alt` while dragging to bend the line into a curve between its
  ends.

## Text

Click to place text and type. `Ctrl+Enter` finishes, `Escape` cancels.
Click existing text on the drawing to change its wording or typography.
Filter fonts by name in **Tool Properties**. Bundled fonts: Bebas Neue,
Fraunces, Josefin Sans, Patrick Hand, Space Grotesk and Space Mono.

## Eraser

The Eraser removes the lines and paint it passes over, on the current art
layer. Pen pressure sets the size between **Minimum** and **Maximum
size**.

- **Tip style** (**Round** or **Flat**) shapes the cut end of a pencil
  line. Brush lines and paint lose exactly the eraser's shape.
- **Apply to all art layers** erases Line Art, Colour Art, Overlay and
  Underlay together.
- **Erase all layers** erases through every visible layer under the
  eraser.
- **Erase all frames** erases the same place in every drawing of the
  layer.

## Select

With **Select**:

- Click a line to select it. `Shift` adds to the selection; `Control`
  subtracts (the Control key on a Mac too).
- **Lasso**: drag on empty space to draw a freehand boundary. Or click to
  place straight anchors, drag an anchor to curve it, and click the first
  anchor or press `Enter` to close it.
- **Marquee**: drag a rectangle. Hold `Alt` to use the other mode for one
  selection.
- Drag the selection to move it. Corner handles scale it; just outside a
  corner rotates it. **Easy drag** lets you drag from anywhere inside the
  box.
- `Backspace` or `Delete` deletes the selected lines. **Flip horizontal** and **Flip
  vertical** are in Tool Properties and the **Edit** menu.
- `F` paints the selected lines, and the areas they close, with the
  current colour.
- **Reduce stroke points** (Tool Properties or search) simplifies the
  selected lines. Raise **Tolerance** for fewer points.

**Cutter** (`C` in the Symphony profile): draw a boundary to split the
artwork along it and select the pieces inside. The traced outline stays
visible while you move them.

## Line Editor

Drag over lines to reshape them with a soft brush. Set **Radius** and
**Strength**.

| Mode | Does |
| --- | --- |
| Push | Moves the line under the brush. |
| Smooth | Evens out the **Path**, or the **Pressure** along it. |
| Thicken / Thin | Changes the local width without moving the line. |

`Shift`-drag paints over points to select them; then dragging moves only
those points. **Show selected points** (`I` in the Symphony profile)
shows them. **Clear point selection** deselects them.

## Art layers

Each drawing has separate art layers, so lines and colour stay apart:

| Art layer | Use |
| --- | --- |
| Line Art | Ink lines. |
| Colour Art | Colour fills under the lines. |
| Overlay, Underlay | Extra layers above and below, turned on in **Preferences → Drawing → Support Overlay and Underlay Arts**. |
| Sketch | Rough drawing under everything. It shows faintly while you ink and colour, and stays out of exports unless you tick **Include Sketch**. |

Choose the art layer with the buttons in the canvas toolbar.
`Shift+L` [`L`] switches between Line Art and Colour Art. **Drawing →
Create Colour Art from Line Art** (`*` in the Harmony profile) copies
the lines to Colour Art for painting. **Preview All Art Layers** (`V`
[`P`]) shows the other art layers as reference.

## Onion skin

`O` [`Alt+O`] turns the onion skin on. The onion-skin button in the
canvas toolbar sets how many **Previous drawings** and **Next drawings**
to show. Their colours are in **Preferences → Canvas**.

- **Light table** (Drawing view) shows the other layers' drawings
  faintly.
- **Align onion skins** lets you move or rotate the previous or next
  ghosts, to compare poses that moved across the screen. Drag to move,
  `Shift`-drag to rotate, `Enter` or **Done** to finish, `Escape` to
  cancel. **Reset alignment** puts them back. Alignment changes only the
  ghosts, never the drawings.

## Guides

The **Guides** menu in the canvas toolbar shows reference guides on the
stage in Camera and Drawing: **Centre lines**, a **Horizon**, and **One
point** or **Two points** perspective with adjustable spread. Guides are
for reference; they don't snap or constrain lines yet (**Planned**).

## Drawing preferences

In **Edit → Preferences → Drawing**:

- **Stroke detail**: how many of the pen's points a line keeps. Lines are
  always smooth curves; lower detail makes them lighter and steadier.
- **Pen pressure response**: lower makes light strokes heavier; 1.0 is
  linear.
- **Brush size cursor**: shows the brush size as a circle.
- **Brush size follows zoom**: on, a brush is the same size on screen at
  any zoom. Off (the default), sizes belong to the drawing, as in
  Harmony.
