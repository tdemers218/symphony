# The workspace

The window is made of **views**: panels you can dock, stack as tabs, or
float. A **workspace** is a saved arrangement of views.

## Views

Open any view from the **Windows** menu.

| View | What it's for |
| --- | --- |
| Camera | The scene as it renders, through the camera. Draw, pose and position artwork here. |
| Drawing | One drawing on its own, in its own coordinates, without pegs or the camera. Good for cleanup. |
| Timeline | Layers as rows and frames as columns: exposures, keys, sound and bookmarks. |
| Xsheet | The same timing as the Timeline, as columns of cells, like a paper exposure sheet. |
| Node | The node graph that builds the picture: drawings, pegs, effects, composites, the camera. |
| Tool Properties | Settings for the current tool: size, smoothing, presets, modes. |
| Colour | The scene's palette: swatches, colour pickers, protected colours. |
| Node Properties | Settings of the selected node or layer, with keys. |
| Function | Animation curves of the selected layer: keys and the interpolation between them. |
| 3D View | The scene from the side, to set depth for multiplane shots. |
| Node Library | Every node type, plus a reference list of Harmony 27's nodes. |
| Undo History | Every edit in this session. Click one to go back to it. |
| Library | **Planned**: templates and symbols. |

## Workspaces

Choose a workspace from the menu at the right of the top bar:

- **Default**: Camera, Drawing and Node in the middle; Timeline, Xsheet
  and Function below; Tool Properties, Node Properties and Colour on the
  right.
- **Hand-Drawn Animation**: Camera and Drawing large, Xsheet on the
  right, tools and colour on the left.
- **Compositing**: Node and 3D View, Camera, Node Properties and the
  Timeline.

Changes you make to a layout are saved into the active workspace, as in
Harmony. **Reset this workspace** puts it back to its preset.

## Arrange views

- Drag a tab to move the view. Drop it on the edge of another view to
  split that view, or on its tab strip to stack it as a tab.
- Right-click a tab for **Eject** (float it in its own window),
  **Remove**, or **Add Tab**. Right-click empty space in a tab strip to add
  a view there.
- **Maximise view** (`Ctrl+F`, or **Windows → Maximise view**) fills the
  window with the focused view. Press it again to go back.

## The Tools toolbar

- Drag the toolbar's grip to any edge of the window. At the top or bottom
  it turns horizontal.
- A small triangle in a tool's corner means it has variants. Rest the
  pointer on the button, or press and hold it, to see them: Brush has
  **Vector**, **Textured Vector** and **Pencil**; Select has **Lasso**,
  **Marquee** and **Cutter**; Paint has its four modes.
- Double-click a tool to open its properties, docked or in a floating
  panel.
- The toolbar only shows tools that work on what's selected. With a peg
  selected, that's Transform and the view tools.

## Search

Press `Tab` anywhere (both profiles). Type a few words and press `Enter`.

- Search finds **actions** ("onion skin", "flip horizontal"), **nodes to
  add** ("blur", "peg"), **layers**, **colours**, **brush presets**,
  **bookmarks**, and pages of this manual.
- Results that fit the view under the pointer come first, then the ones
  you pick most often. **Edit → Preferences → Interface → Forget search
  history** resets that.
- Some actions take a number: "insert 3 frames".
- Search understands everyday words, so "shadow" finds the Multiply blend
  mode. That comes from a hand-written word list.
- Right-click an action in the results to change its shortcut. If the
  keys are taken, Symphony tells you which action loses them.

## Navigate the canvas

These work in the Camera and Drawing views:

| Do this | To |
| --- | --- |
| Hold `Space` and drag | Pan, with any tool |
| Hold `Shift+Space` and drag | Rotate the view |
| Pinch, or `Ctrl` + scroll | Zoom |
| `1` / `2` (Symphony) or `2` / `1` (Harmony) | Zoom in / out |
| **View** menu | Reset zoom, rotation, pan or all of them, and Mirror view |

**Mirror view** (`M` in the Symphony profile) flips the view, not the
drawing, so you can check proportions with fresh eyes. "Mirrored" shows in
the toolbar while it's on.

## Preferences

**Edit → Preferences** has these pages:

| Page | Settings |
| --- | --- |
| Interface | Wallpaper, **Interface effects** (blur, glass and shadows; turn off on slow computers), interface animations, search history |
| Drawing | Stroke detail, pen pressure response, brush size cursor, whether brush size follows zoom, protected colours, Overlay and Underlay art layers |
| Canvas | Stage, surround and onion-skin colours; show strokes; preview all art layers |
| Workflow and shortcuts | The shortcut profile, and every shortcut, with a filter |
| Video | Whether movie export uses the OpenH264 codec, and downloading or removing it |
| AI-free | The **Use no AI** switch (see below) |

Symphony has no generative AI and no assisted tools, so nothing in it
uses AI today. The **Use no AI** switch is there so it stays that way for
you if optional assisted tools ever arrive. Read the
[AI policy](../AI-POLICY.md).
