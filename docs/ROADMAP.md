# Roadmap

See the [reconciled feature direction](plans/feature-direction.md) for workflow, architecture and production milestones beyond this table.

Each milestone ends in something an animator can try.

| # | Milestone | Done when |
|---|---|---|
| M0 | Core model | xsheet, node graph, pegs, keyframes and evaluation, all tested. **(done, 6 tests)** |
| M1 | Pen spike | A test window shows pressure and tilt on Mac, Windows and Linux. The UI-stack decision gets made here (see LANGUAGE.md). **(macOS done: Veikk gave 244 pressure levels, tilt, ~340 Hz. Windows and Linux to do)** |
| L | Launcher and releases | Testers install one launcher, which installs, updates and rolls back Symphony from GitHub Releases. **(done; preview 1 published)** |
| M2 | Shell | Window with dockable Camera, Timeline, Xsheet, Node and Tool Properties views. Workspace switching. **(done)** |
| M3 | Draw | Pencil (centreline + thickness) and Brush (contour). Line Art and Colour Art layers. Onion skin. Undo. **(done: brush, pencil, eraser, smoothing, pressure, onion skin, undo/redo, navigation, Select and Transform tools, presets, textured vector brushes. Art layers and a cutting Eraser done.)** |
| M4 | Time | Timeline and Xsheet editing the same data. Exposures and holds. Playback at scene fps. **(done for the basics)** |
| M5 | Paint | Palettes with colour IDs, Paint tool, auto-fill, close gap, swatch edits recolour everything. **(mostly done: Paint modes, Close Gap, Dropper, swatch editing, Protect Colour. Still to do: palette lists, Ink tool, gradients.)** |
| M6 | Rig | Node view editing. Pegs, the Transform tool, Function view (Bézier/ease), drawing substitution. |
| M7 | Deform | Bone, Curve and Envelope deformers. |
| M8 | Output | PNG sequence, video via ffmpeg, SVG. Write node. |
| M9 | Interop | Import from Harmony exports. Then `.xstage` scene import (from user files only). |
| M10 | Scripting | Harmony-like JS/Python scripting API. |
