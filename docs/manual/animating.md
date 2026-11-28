# Keys, pegs and the camera

Drawings don't have to be redrawn to move. Put a **peg** above a drawing
and animate the peg: position, rotation, scale and depth, with **keys**
on the frames you choose and curves in between.

## Pegs

A peg is a node that moves everything connected under it. One peg can
carry several drawings, and pegs can sit under other pegs, so a head peg
can ride on a body peg.

1. Search **Add Peg** (`Tab`), or use **Add node → Peg** in the Node view.
2. In the Node view, connect the peg's output to the drawing's input. See
   [Node view and effects](nodes.md#connect-nodes).
3. Select the peg. Its track appears in the Timeline and its settings in
   **Node Properties**.

Peg settings in **Node Properties**: **Position X** and **Y**,
**Rotation**, and under **Scale, pivot and depth**: **Scale X**, **Scale
Y**, the **Pivot** and **Depth Z**. Each can be keyed.

## Move artwork with the Transform tool

Choose **Transform** (`X` [`Shift+T`]).

- Click artwork in Camera to pick its layer.
- Drag to move. Drag a corner handle to scale. Drag just outside a corner
  to rotate.
- With a peg selected, the change goes on the peg, and the drawing's
  lines stay as they were drawn.
- **3D rotation**: drag an axis ring on the canvas to turn the peg around
  X, Y or Z. **Face Camera** tilts the line's path but keeps its width
  facing the camera. Move in depth from the [3D View](#multiplane-and-depth).

The Transform tool also edits Mesh Warps, bones and Smart Charts when one
of those is selected; see [Rigs and deformers](rigging.md) and
[Smart Charts](smart-charts.md).

## Keys

A key stores a value on a frame. Between keys, Symphony interpolates.

- **Set a key**: select the node and click **Set key** in Node Properties,
  or press `K` over the Timeline, or click the diamond button in the
  Timeline. On a frame that already has a key on that track, the button
  removes it.
- **Auto-key** (`A` over the Timeline in the Symphony profile, or
  **Timing → Toggle auto-key**): every change you make is keyed on the
  current frame. With it off, a setting that isn't animated stays the same
  on every frame until you set a key.
- **Delete a key**: `Shift+K` over the Timeline (Symphony profile), or
  **Delete key** in the Function view.
- Every node can key its **Enabled** state, to switch it on or off at a
  frame. Peg transforms and numeric effect settings can be keyed too.

Keys appear on the node's track in the Timeline.

## The Function view

The **Function** view shows the selected layer's animation curves.

1. Tick a function on the left, such as **Position X**, to show its curve.
2. Click a key to select it. Drag it to change its frame or value.
3. Double-click a curve to add a key there, or click **Add key** to key
   the current frame.
4. Choose how the segment after a key moves:

| Interpolation | Does |
| --- | --- |
| Bezier | A curve shaped by handles. **Smooth** keeps the handles in line; **Corner** lets them break. |
| Linear | A straight line to the next key. |
| Stop-motion | Holds the value until the next key, then jumps (Harmony's stop-motion keyframe). |

For slow-ins and slow-outs, use Bezier keys and flatten the handles near
the key.

## The camera

The **Camera** node is what the Camera view and exports show.

- **Field of view**: double-click **Camera** in the Node view. Presets are
  **Tight** (35°), **Standard** (60°) and **Wide** (90°), with a slider
  for anything in between. Click or drag a slider, then press `Left` or
  `Right` for small steps.
- **Camera moves**: open **Camera properties** (in the 3D View) and key
  **Camera X** and **Camera Y** to pan, and **Dolly Z** to push in.
  Positive dolly moves the camera forward.

## Multiplane and depth

Give drawings depth and move the camera, and near artwork moves faster
than far artwork: parallax.

1. Open **3D View** (**Windows** menu, or search "parallax").
2. Choose a drawing layer that has a peg. Drag its plane in the side
   diagram, or type a **Peg Z**. Positive Z moves it away from the camera.
3. **Maintain Size** keeps the drawing's size and position on screen at
   the current frame while you change its depth, so you can stage depth
   without re-laying out the shot.
4. Key **Camera X**, **Camera Y** or **Dolly Z** over time.

In the 3D View, drag with the left or right button to orbit, middle-drag
to pan, and scroll to zoom. **Reset view** goes back.

Stacking order still comes from the Composite node's port order, not from
depth. See [Node view and effects](nodes.md#composite-order).

## Camera view controls

These are in the Camera view's toolbar:

- **Render effects** (`Shift+R` in the Symphony profile): turn off to
  position the plain artwork without blurs and other effects. Exports
  always render effects.
- **Bars**: black bars over the camera frame, as **Frame (rounded edges
  only)**, **Cinematic 1.85** or **Cinematic 2.39**, with a **Corner
  radius**. Bars are part of the picture, so exports draw them too.
- The dark area outside the stage hides artwork that's off-frame.
