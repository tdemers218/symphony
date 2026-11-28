# Symphony

Symphony is a free, open-source 2D animation app for drawing, timing, and
node-based compositing.

[Presentation page](https://tdemers218.github.io/symphony/) ·
[Manual](https://tdemers218.github.io/symphony/manual/) ·
[Preview builds](https://github.com/tdemers218/symphony/releases)

![Symphony’s default workspace: a red ball mid-bounce following a Smart Chart in the Camera view, with the tools, palette and Timeline around it](docs/presentation/workspace.png)

## Key features

- **Plan motion with Smart Charts.** Click where the artwork is on each key frame: a path runs through the points with a tick for every frame, and a layer follows it. Set slow-ins, holds and breakdowns in a small bar by the point, slide ticks to change the spacing, or draw the path by hand and record its timing as you draw.
- **Import brushes as editable lines.** Bring in brushes from Photoshop, Krita, MyPaint, Procreate, Clip Studio Paint and GIMP. They become Textured Vector brushes: the tip’s texture on a line you can still reshape and recolour. The presentation compares Textured Vector and Vector strokes rendered in Symphony with Ramón Miranda’s [Sketch V2 for Krita 5](https://krita-artists.org/t/updated-to-include-v3-sketch-v2-sk2-pleasure-to-draw-including-colored-pencils-and-blenders/59552).
- **Build and pose smart rigs.** Assign drawings to bones in Node Properties. Lock anchor points to keep hands or feet planted, use Aim targets to guide the pose, and tune spring stiffness or stretch on several bones at once.
- **Sketch in each drawing.** Rough construction stays on a separate art layer and can be included in an export when needed.
- **Cut along a lasso.** Split strokes or fills along a freehand or point-by-point boundary and move the selected pieces with the traced outline still visible.
- **Reuse shortcuts across panels.** In the Symphony preset, `D` picks the Dropper over Drawing and duplicates a drawing over the Timeline. Tab Search shows the shortcuts and lets you remap them.
- **Clone or duplicate a drawing.** Clone to share future edits, or duplicate to create an independent version. Frame numbers stay visible while the Timeline tracks scroll.
- **Bend a line as you draw.** Hold Alt while drawing with the Line tool to bend the stroke between its endpoints. Hold Shift to snap the angle in 22.5° steps.
- **Recolour drawings from one swatch.** Edit a scene palette colour once and every stroke or fill linked to it updates together.
- **Stage a multiplane camera move.** Set depth on drawing pegs and animate the camera for parallax. Maintain Size keeps the drawing in place while you set its depth.
- **Position artwork with effects paused.** Switch off Render effects while positioning artwork in Camera; exports always render them.
- **Resume an interrupted export.** Restart the same export to reuse finished frames after a crash or cancellation. This works for movies, GIFs, and image sequences.
- **Untangle a node graph.** Arrange Nodes lays out the graph by signal flow. Select a node to highlight its cables and ports, or drag it to make nearby cards slide out of the way.
- **Make room for in-betweens.** Insert frames after the playhead. Current exposures hold through the gap, and later exposures and animation keys shift together.
- **Choose a separate movie frame rate.** Animate at your scene’s frame rate and choose a different rate for the movie export. Symphony resamples the timing to keep the shot’s duration.
- **Turn a drawing into an editable matte.** Switch a drawing to a mask and back while keeping its artwork, exposures, and node connections.
- **Find actions with Tab Search.** Type what you want to do. Results prioritize the panel you’re working in, from adding an effect to running a timing command.

Not affiliated with Toon Boom. Built clean-room from public documentation
(see [docs/CLEAN-ROOM.md](docs/CLEAN-ROOM.md)).

- **[Plan: status, what's left, full feature list](docs/PLAN.md)**
- **[Design rules](docs/DESIGN.md)** and **[engineering rules](docs/ENGINEERING.md)**: every change follows these.
- [Research](docs/RESEARCH.md) · [Language choice](docs/LANGUAGE.md) · [Roadmap](docs/ROADMAP.md) · [Tablets](docs/TABLETS.md)
- [AI in Symphony](docs/AI-POLICY.md): no generative AI, ever.
- [Codecs](docs/CODECS.md): movie export uses Cisco's OpenH264, downloaded only when you ask.
- [Manual](docs/manual/index.md): how to use Symphony, also on the [website](https://tdemers218.github.io/symphony/manual/) and in the app (**Help → Manual**).
- [Testing Symphony](docs/TESTERS.md): install the launcher.
- [Contributing](CONTRIBUTING.md): please read the clean-room rules first.

Licensed under GPL-3.0-or-later.

## Build
```sh
cargo test
cargo run --release -p symphony-pentest   # pen test window
```

## Layout
- `crates/symphony-core`: scene model (xsheet, node graph, pegs, evaluation).
  No UI.
- `crates/symphony-pen`: our own pen/tablet input layer (pressure, tilt,
  rotation, eraser), one backend per OS. macOS done; Windows and Linux next.
- `crates/symphony-pentest`: test window that draws with pressure and logs
  every sample to `target/pen-log.csv`.
