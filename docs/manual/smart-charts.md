# Smart Charts

A Smart Chart is an animator's timing chart that moves artwork. You draw
the path and its spacing once, on the canvas: key points, breakdowns, and
a tick for every frame. Then a layer follows it. Smart Charts are a
Symphony feature; Harmony has nothing like them.

## Place a chart

1. Search **Add Smart Chart** (`Tab`). The chart is selected and the
   Transform tool is on.
2. Go to a key frame and click where the artwork is.
3. Go to the next key frame and click again. Do this for each key.

A smooth path runs through the points, with a tick for every frame between
them. Close ticks are slow, far-apart ticks are fast. A point between two
keys is a **breakdown**, drawn as a triangle; keys are circles. The point
on the current frame is filled.

## Shape and time it

- **Move a point**: drag it.
- **Curve handles**: bring the pointer near a point to show them. Drag a
  handle to change the curve; `Alt`-drag for a sharp turn.
- **Slide a tick** along the path to change where the chart is on that
  frame.

On a point's frame, a small bar appears next to the point:

| Control | Does |
| --- | --- |
| Key / Breakdown | Switches what the point is. |
| In, Out | Slow in to the point and slow out of it. Drag the number for more or less. |
| Hold | The chart stays on this point until the next one. |
| Line | A straight segment to the next point. |
| Pencil | Draw the path to the next point (see below). |
| Rotate | Turn and scale (see below). |
| More | Smooth / Corner / Handles, Even spacing, colour, following, and Done. |

**Even spacing** clears the eases and slid ticks around the point.
**Done**, or `Escape`, finishes editing. The chart stays, drawn quietly.

Node Properties has the same settings, under **Point on frame N**.

## Draw the path

Turn on the pencil in the bar, or hold `Shift`, and drag from one point
to the next. Your stroke is smoothed and becomes the path between them,
fitted with as few curves as it needs to stay close to what you drew. The
curves on either side leave the points in the drawn direction, so the
joints stay smooth. Moving either point stretches the drawn path with it.

- **Timing**: turn it on next to the pencil to record timing too. The
  chart then moves as fast as you drew: draw slowly near a key for an ease.
  Drawing from the later point back to the earlier one works the same.
- **Drawn** on the bar goes back to the automatic curve.

One stroke covers the stretch between two neighbouring points.

## Turn and scale

Click the rotate button on the bar. A needle appears from the point:

- Drag the round knob to turn it. `Shift` snaps to 15°.
- Drag the square along the needle to scale.
- **More** has exact **Rotation** and **Scale**, and **Tilt X** and
  **Tilt Y** for 3D. Rotation can go past a full turn: 720° is two turns.

## Make artwork follow it

Tick **<layer> follows this chart** under **More** or in Node Properties,
or search **Follow Smart Chart**. The layer's peg then moves along the
chart. On the chart's first frame the artwork stays where you drew it.

- In the peg's **Motion** section, untick **Rotation**, **Scale** or
  **Tilt** to take only part of the chart's motion. Position always
  follows.
- In the Node view, a chart connects to a peg's diamond **Motion** port.
  One chart can drive several pegs.
- **Stop following Smart Chart** (search) disconnects it.

## Retime it

- Drag the chart's coloured bar on its Timeline track to move the whole
  chart earlier or later. It can't go before frame 1.
- **Insert in-betweens** stretches it with the rest of the scene.
- While the chart is edited, **Copy**, **Paste**, **Cut** and **Delete**
  act on the point on the current frame. Copying between points copies
  where the chart is on that frame.

## Colours

Each new chart takes the next colour in a fixed order: lilac, mint, aqua,
rose, lime, periwinkle, orchid, sage. If you always chart the head first,
the head is lilac in every scene. Pick another colour under **More** or in
Node Properties.

Charts show in Camera and Drawing and never appear in exports. **Show or
hide Smart Charts** (search) hides them all.

## Keys

In the Symphony profile, while a chart is edited:

| Key | Does |
| --- | --- |
| `Shift+B` | Key or breakdown |
| `Shift+H` | Hold until the next point |
| `[` and `]` | Move the chart a frame earlier or later |
| Arrow keys | Nudge the point (`Shift`: 10 units) |
| `Escape` | Finish |

Search has these and more in both profiles: **Edit <chart name>**,
**Smart Chart: even spacing**, **Smart Chart: rotate and scale**.

## What's next for charts

Smart Charts are the base for Symphony's tracking. **Planned**: charts
that drive in-betweening, more consumers than pegs, and one stroke across
several points. See [Planned features](planned.md#smart-charts).
