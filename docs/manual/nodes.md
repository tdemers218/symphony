# Node view and effects

The **Node** view shows how the picture is built. Each drawing layer is a
**Drawing** node; its image flows down through pegs, effects and
composites to the **Camera** node, which is what the Camera view and
exports show.

## Add nodes

Click **Add node** in the Node view, or press `Tab` and search for the
node by name, such as **Add Peg** or **Add Blur**.

| Node | Does |
| --- | --- |
| Drawing | Shows the drawing its layer exposes on the current frame. Added with a layer. |
| Peg | Moves, rotates, scales and sets the depth of what's under it. See [Keys, pegs and the camera](animating.md). |
| Skeleton, Bone | A rig. See [Rigs and deformers](rigging.md). |
| Mesh Warp | Bends the artwork under it with points. |
| Smart Chart | Motion that pegs can follow. Draws nothing. See [Smart Charts](smart-charts.md). |
| Composite | Stacks its inputs into one image. |
| Mask | Keeps the part of its **Image** covered by its **Matte**. |
| Blur | Box, Gaussian, Motion, Radial Spin or Radial Zoom blur. |
| Grain | Film grain, with an amount and a seed. |
| Opacity | Fades its input. |
| Blend Modes | Blends its input onto what's behind it: Multiply, Screen, Overlay and 17 more, at an opacity. |
| Gradient | Makes a gradient image on its own, for backgrounds. |
| Camera | What the Camera view and exports show. Double-click it for the field of view. |

**Windows → Node Library** lists every node type, and a searchable
reference of Harmony 27's nodes, so you can see what a Harmony node does
and whether Symphony has it yet.

## Connect nodes

Outputs are at the bottom of a node, inputs at the top.

- Drag from an output to a compatible input to connect them.
- Drag from a connected input to another input to move the cable, or drop
  it on empty space to disconnect it. You can also right-click an input
  and choose **Disconnect input**.
- `Alt`-click a cable or a connected input to cut it. `Alt`-drag a single
  connected node away to disconnect all its cables.
- Hold `Alt` while dragging a node with free ports: drop it near an open
  socket to connect it, or onto a cable to insert it between two nodes.
  The cables it would make are highlighted while you drag.
- `Escape` cancels a drag. Undo covers placement, adding nodes and cable
  edits; one Alt-drag undoes as one step.

A peg's diamond **Motion** port takes a Smart Chart. It shows when the
scene has a chart.

## Arrange the graph

- Drag a node to move it. `Shift`-click adds nodes to the selection; drag
  one of them to move them all. Nearby nodes slide out of the way.
- Hold `Shift` while moving a connected node to line its ports up with
  the nodes above and below.
- **Arrange nodes** (`A` in the Symphony profile, or **View → Arrange
  Nodes**) lays the graph out by signal flow. Undo restores the old
  positions.
- Scroll to zoom around the pointer; drag empty space to pan. A minimap
  appears when the graph is bigger than the view. **Reset Node view**
  (`R`) brings everything back into view.
- Selecting a node highlights its cables and ports.

## Node settings

Double-click a node, or press `P` over the Node view, to open **Node
Properties**. Every node has a **Node name** and an **Enabled** switch
that can be keyed. Effect settings that are numbers can be keyed too.

## Composite order

A Composite stacks its inputs with the leftmost port in front. Depth on
pegs moves layers for parallax but doesn't change which one is in front;
the port order does.

## Masks and mattes

1. Add a **Mask**. Connect the artwork to its **Image** port and a drawing
   or other image to its **Matte** port.
2. Connect the Mask's output to your composite. The matte's alpha keeps
   the part of the image it covers. **Invert matte** cuts that part away
   instead.

To draw a matte, right-click a Drawing node and choose **Convert to Mask
Drawing**. It stays editable in the Drawing view and keeps its exposures
and cables, but only renders where it feeds a matte. **Convert to
Drawing** turns it back.

## Preview a node

**Render node preview** (`V` over the Node view in the Symphony profile)
renders the selected node's output on the current frame, to check one
branch of the graph.

The Camera view renders the full graph, effects included. Turn **Render
effects** off in the Camera toolbar (`Shift+R`) to position artwork
without effects; exports always include them.
