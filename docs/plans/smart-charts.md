# Smart Charts

A Smart Chart is the animator's timing and spacing chart, made live: points
placed on key poses, a smooth path through them, spacing marks that show
speed, and motion that other nodes can reuse. **Define motion once,
visualize it directly, reuse it anywhere.**

This is a Symphony feature with no Harmony equivalent (Harmony has motion
paths on pegs and the static Field Chart node, neither of which is this).
It is available in both profiles; it changes no Harmony-profile shortcut.
Deterministic maths only, no AI (docs/AI-POLICY.md).

The full brief (from Tristan, 2026-11-27) is summarised here as decisions
against the real code. Section numbers in brackets refer to the brief.

## Audit: what exists and what charts reuse

| Need | Existing system | Use for charts |
| --- | --- | --- |
| Node | `symphony_core::graph::NodeKind`, positional ports, kind-checked in `Graph::can_connect` | New `NodeKind::Chart(Box<Chart>)`. No inputs; its output connects to nothing until Milestone 4. |
| Keyed channels | `anim::Value` (Linear / Bezier / Stop keys, handles, continuity, `at_f` fractional frames) | Scale X/Y and rotation are plain `Value`s, so the existing key tools, Bézier eases, holds and >360° rotation all apply as-is. |
| Undo | `history::Edit::SetNodeProperties`, which merges a live drag into one entry | Every chart edit replaces the chart node through it. No new edit type. |
| Save | serde JSON scene; enums gain variants compatibly | `Chart` is serde with defaults; derived curves are never saved. Old projects load unchanged. |
| Search | `search.rs` entries, `Command::AddNode`, `node::add_from_search` (auto-inserts effects and warps for the selected layer) | "Smart Chart" entry plus chart actions. |
| Canvas editing of a selected node | `warp_edit::ui` / `rig_edit::ui` in `camera.rs`: active with Transform when that node is selected | `chart_edit::ui`, same pattern: select a chart, use Transform. No new toolbar button. |
| Timeline | `timing.rs` node tracks show a node's keyed frames; `reveal_node_track` | Chart points and channel keys show on the chart's node track. |
| Retiming | `Session::insert_inbetweens` and friends shift xsheet, bookmarks, camera, node keys | Charts join that list: points and channel keys at or after the insert shift too. |
| Caching | `doc.version` edit counter; `VectorMeshes` / `bent_drawing` pattern | Built curves cached per (node, `doc.version`); the overlay keeps its own screen cache. |

## Model (`crates/symphony-core/src/chart.rs`)

```
Chart { points: Vec<ChartPoint>, scale: [Value; 2], rotation: Value,
        colour, visible, locked, next_point }
ChartPoint { id, frame, pos: [x, y], role: Key | Breakdown,
             tangent: Auto | Manual { before, after } | Corner,
             path: Curve | Line,        // the segment to the next point
             timing: Timing }           // the segment to the next point
Timing { hold, ease_out, ease_in, spacing: Vec<[t, s]> }
```

- **Space** (where it goes, [6]): a cubic Bézier per segment.
  - Auto tangents come from **centripetal Catmull-Rom**: the curve passes through every point, doesn't loop or cusp, and tolerates uneven spacing.
  - A tangent depends only on the neighbouring points, so adding a point reshapes the two segments beside it and nothing else ([7]).
  - Manual tangents are never touched by automatic updates. Corner gives a sharp turn; Line gives a straight segment. Coincident points are guarded, so there's no divide-by-zero ([22]).
- **Time** (how fast, [6]): each segment maps time (0→1, between its two points' frames) to the fraction of the segment's **arc length** covered (0→1).
  - That map is a monotone cubic through the spacing knots (Fritsch–Carlson), with end slopes set by ease out / ease in. Hold stays put until the next point.
  - Because timing is stored as fractions of arc length, reshaping the path keeps the spacing, and changing the spacing never moves the path.
  - Even spacing is the default, like a fresh chart before the animator marks favours.
- **Scale and rotation** ([6], transform channels): independent `Value`s keyed by frame, evaluated against the same timeline. Rotation is in degrees, so multi-turn rotation is just a bigger number. 3D orientation (a quaternion internally, X/Y/Z in the UI) is added in Milestone 3 as an optional field; consumers decide what it means ([5]).
- **Keys versus in-betweens** ([8]):
  - A **Key** point is a key pose.
  - Moving the chart off its path on an in-between frame adds a **Breakdown** point. The path passes through it, but it's drawn differently and creates no drawing or exposure.
  - Sliding a frame marker *along* the path adds a spacing knot instead. Both are authored data, never temporary samples.
- **Space of positions:** stage coordinates (what Camera shows). Drawing view shows the chart through the selected layer's inverse transform. A point can remember the drawing exposed when it was placed (`drawing: Option<Id>`), for retiming decisions later ([14]); nothing tracks artwork automatically ([11]).
- **Output** ([13]): `ChartCurve::sample(frame: f32) -> MotionSample`. It works at any fractional frame and returns:
  - position, velocity, direction, speed
  - scale, rotation, angular velocity
  - progress (0→1 over the chart's range), the transform matrix, and the valid range

  It is pure and independent of the UI: hiding or selecting a chart never changes it.

## Milestones (the brief's order, [21])

1. **Motion foundation.** *Done.*
   - `chart.rs`: model, curve and timing maths, arc-length tables, `MotionSample`.
   - `NodeKind::Chart`, serialization, undo through `SetNodeProperties`.
   - Tests from [22]: anchors exact, uneven spacing, extension, manual tangents kept, insertion, coincident points, duplicate frames, holds, space/time independence, scale, rotation wrap and multi-turn, fractional frames, round trip, old projects.
2. **Canvas workflow.** *Done (`views/chart_edit.rs`).* Charts show in Camera and (since Milestone 3) in the Drawing view, through the inverse of the selected drawing's stage transform, where they can be edited the same way.
   - Search "chart" + Enter creates a chart, selects it and switches to Transform.
   - Clicking the canvas adds a point at the current frame; dragging points works.
   - Path overlay with frame marks (spacing shows speed), the current-frame mark, direction, frame numbers on keys.
   - Several charts, each with name, colour, visibility and lock.
   - Simplified when not selected; overlays never reach export.
3. **Advanced editing.** *Done.*
   - Timing (ease out / ease in, spacing knots by sliding marks), Breakdown points, manual tangent handles, Corner / Line / Hold.
   - Scale and rotation handles on the selected point.
     *Done:* on a point's frame, a needle points up from the point at 0°. Its round knob turns: keys rotation, the short way, and Shift snaps 15°. A square along it scales: uniform, distance = size.
     The card has exact Rotation (720° = two turns) and Scale fields.
   - Timeline track, retiming with insert and delete frames, copy and paste points, shift the whole chart in time.
     *Done:* drag a chart's span on its Timeline track to move it in time (`Chart::shift_frames`, never before frame 1, one undo step).
     While a chart is edited (selected, Transform), Copy / Cut / Paste / Delete act on its point on the current frame. Copy between points takes where the chart is.
4. **Node integration.**
   - A typed Motion output. Consumers: Peg first (follow a chart: position, plus optional rotation and scale), then others.
     *Peg done:* a peg's port 1 (`graph::MOTION_PORT`, a diamond socket, shown once the scene has a chart) takes a chart.
     The motion is `translate(pos) · rotate · scale · translate(-start)` in the peg's parent space, so artwork stays where it was drawn on the chart's first frame and turns around the chart's current point.
     Peg properties have Rotation / Scale toggles; the chart card has "<layer> follows this chart"; search has "Follow Smart Chart".
     Curves are shared through `chart::shared_curve`, keyed by chart content.
     Open: under a moving parent the chart overlay (drawn in stage space) and the motion (parent space) differ; show it through the parent then.
   - One chart can drive several consumers; contextual connections from search; curves cached per `doc.version`.
     *Done for pegs:* several pegs can follow one chart. Curves are cached by content (`chart::shared_curve`) rather than by `doc.version`, so an edit elsewhere in the scene doesn't rebuild them.
5. **Polish.** Shortcuts, search coverage, large-project timings in `scripts/perf.sh`, user documentation, a manual workflow test (three key poses; head, torso and hand charts).
   *Done:*
   - `chart_cost` in `scripts/perf.sh`, with 40 layers on 10 charts of 60 points (release build, 2026-11-28):
     - about 40 µs to build a chart (budget 1 ms);
     - about 53 µs per playback frame (budget 1 ms);
     - about 111 µs for the first frame after an edit (budget 2 ms).
   - User steps in `docs/TESTERS.md` ("Animate with Smart Charts").

   *Open:* the manual workflow test, which needs Tristan.

In-betweening, lighting, texture tracking and motion-processing nodes ([15]–[17]) come after; the Motion output is designed so they can consume or transform it.

## Look (v2, Tristan's direction, 2026-11-28)

A paper timing chart in a glass-lite finish:
- **Path and points:** a thin line with a soft glow of its own colour, and short ticks. A key is its frame number in a small glass circle; a breakdown is a glass triangle. The point on the current frame is filled with the chart's colour, and between points the current frame is an amber ring.
- **Only what's being worked on shows.** Curve handles appear when the pointer nears the point (or it has handles of its own). The rotate and scale needle shows only when switched on from the bar.
- **Controls:** a glass pill bar next to the point, placed where it hides least of the chart (above, below, or beside the point), with More for the rest. The same settings are in Node Properties. Esc finishes editing.
- **Colours:** soft tones from the interface's own family (the Function view's curves), away from its meanings (selection blue, time amber, danger red).
  New charts take them in a fixed order: lilac, mint, aqua, rose, lime, periwinkle, orchid, sage. Each takes the first colour no chart uses yet, so a habit like charting the head first gives the same colours in every scene. Pick any other colour from the swatches in More or in Node Properties.
- **Shadow, not glow:** the line and ticks sit on a soft drop shadow, like the glass chips. Tristan chose it over a coloured glow after comparing the two.

## Performance ([18])

- A built curve is cached per (chart node, scene version). An edit to one chart rebuilds that chart only.
- Sampling is a binary search in a 64-entry-per-segment arc-length table.
- The overlay builds its marks once per chart change and view, not every frame.
- Charts off-screen or hidden are skipped.
- `an_unchanged_frame_rebuilds_nothing` must keep passing.
