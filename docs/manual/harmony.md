# Coming from Harmony

Symphony follows Harmony's way of working, so most of what you know
carries over: the Camera, Drawing, Timeline, Xsheet and Node views; Line
Art and Colour Art; palettes where colours are linked by ID; the four
Paint modes and Close Gap; pegs, drawing substitution, and workspaces
that save their layout.

Symphony is built from Harmony 27's public documentation and isn't
affiliated with Toon Boom. It doesn't read or write Harmony's database.

## Pick the Harmony profile

**Edit → Preferences → Workflow and shortcuts → Harmony** gives you
Harmony 27's default shortcuts: `Alt+B` Brush, `Alt+E` Eraser, `Alt+O`
onion skin, `+` and `-` for exposure, `[` and `]` to substitute drawings,
`F` and `G` for the previous and next drawing, `K` to set a key, and so
on. Actions Harmony has no shortcut for are left free; use Search (`Tab`)
for them. The full list is in [Keyboard shortcuts](shortcuts.md).

## What the Symphony profile changes

If you'd like to try the Symphony profile, these are all the differences:

| Area | Harmony profile | Symphony profile |
| --- | --- | --- |
| Redo on a Mac | `Ctrl+Shift+Z`, as Harmony documents it | `Cmd+Shift+Z`, like other Mac apps (both profiles accept it) |
| Tool keys | Harmony's | `S` Select, `B` Brush, `E` Eraser, `L` Line, `T` Text, `P` Paint, `C` Cutter, `G` Close Gap, `D` Dropper, `N` Line Editor, `X` Transform, `H` Hand, `Z` Zoom, over Drawing and Camera |
| Zoom | Harmony's | `1` zooms in and `2` zooms out in Drawing, Camera and Node |
| `+` and `-` | Exposure | Brush size over Drawing and Camera; exposure over Timeline and Xsheet |
| Drawing and Camera | Harmony's | `R` Repaint, `U` Unpaint, `Y` Paint Unpainted, `O` onion skin, `M` mirror, `I` show selected points, `V` preview art layers, `Shift+R` render effects (Camera) |
| Timeline and Xsheet | Harmony's | `A` auto-key, `C` clone, `D` duplicate, `I` insert in-between, `K` set key, `Shift+K` delete key, `L` add layer, `N` new drawing frame, `V` layer visibility |
| Node view | Menus and Search | `A` arrange, `P` properties, `R` reset view, `V` render preview |
| Smart Charts | Search and the point bar | `Shift+B`, `Shift+H`, `[`, `]`, arrows, `Escape` while a chart is edited |

In the Symphony profile the same letter does different things depending
on the view under the pointer.

## Different names

| Harmony | Symphony |
| --- | --- |
| Layer Properties | **Node Properties** (one view for layers and nodes) |
| Animate mode | **Auto-key** |
| Display node | **Camera** node in the Node view |
| Toggle Full Screen | **Maximise view** (same `Ctrl+F`) |

## Only in Symphony

- **Search** (`Tab`) for every action, node, layer, colour and preset,
  with remappable shortcuts.
- **Smart Charts**: timing charts on the canvas that artwork follows. See
  [Smart Charts](smart-charts.md).
- A **Sketch** art layer under the others, left out of exports.
- **Line Editor** modes to push, smooth, thicken and thin lines.
- **Cutter**: split artwork along a lasso.
- **Onion-skin alignment**, to compare poses that moved.
- **Insert in-betweens** across the whole scene, moving keys and
  exposures together.
- **Bookmarks** on the Timeline.
- A separate **movie frame rate** on export, and exports that resume.
- **Brush import** from Photoshop, Krita, MyPaint, Procreate, Clip Studio
  and GIMP.
- **Points** mode for Mesh Warp, which bends artwork as rigidly as it can.

## Not in Symphony yet

These Harmony features are **Planned**:

- The Ink tool, gradient and texture swatches, palette lists.
- Curve, Envelope and other deformers; multi-bone weights.
- Drawing templates and the Library.
- Most effect nodes, and the Write node's full output options.
- Full `.xstage` conversion (artwork, palettes, rigs, effects). Today
  only the scene structure comes in; see
  [Harmony scenes](import-export.md#harmony-scenes).
- Scripting.
- Harmony Server-style shared databases (last, and optional).

See [Planned features](planned.md) for the full list.
