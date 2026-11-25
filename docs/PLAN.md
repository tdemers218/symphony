# Symphony: where we are and where we're going

[Feature direction and implementation audit](plans/feature-direction.md)

This is the master plan. It covers how the app works today, what's left,
and the full feature list: every feature Harmony has that we plan to match
first. Once the big chunk is done we'll improve some of them and add new
ones.

**Target: the latest Harmony, currently 27.1.** Build every feature from the
newest docs, not older versions (see [CLEAN-ROOM.md](CLEAN-ROOM.md)).

Status key: ✅ done · 🟡 started · ⬜ not started · ✖ deliberately left out

---

## 1. How Symphony works so far

Symphony is a Rust workspace with three crates. The engine never depends on
UI code, so the front end can change without touching it.

```
symphony-core     scene model: xsheet, node graph, pegs, keyframes, evaluation
symphony-pen      our own pen/tablet input, one backend per OS
symphony-pentest  test window: draws with pressure, logs every sample
```

### Scene model (`symphony-core`) ✅
- **Xsheet.** Columns of cells, where each cell names the drawing exposed on
  that frame ("1", "2", "a"…). Frames are 1-based, the way animators count.
  `expose("1", 1, 2)` means drawing 1 held for 2 frames (animating on twos).
  An empty cell shows nothing. The scene length is the longest column.
- **Node graph.** Nodes connected by links. Each link goes from a node's
  output into a numbered input port on another node.
  - *Drawing* shows whatever its column exposes on the current frame.
  - *Peg* holds a transform (position, rotation, scale, pivot). A peg feeds
    a drawing or another peg, and chained pegs make a rig hierarchy.
  - *Composite* stacks its inputs. Port 0 (leftmost) is in front.
  - *Display* is what the camera shows. *Write* is the render output.
- **Animatable values.** A value is either constant or keyframed. It
  interpolates linearly between keys and holds the first or last key
  outside them. Bézier and ease curves come later.
- **Evaluation.** For a given Display node and frame, the graph returns a
  back-to-front list of drawings, each with its full transform after all
  parent pegs are applied. Cycles are reported as errors instead of hanging.
- 6 unit tests cover exposure on twos, empty cells, render order, peg
  interpolation, transform maths and cycle detection.

### Pen input (`symphony-pen`) ✅ on macOS
- Every tablet produces one sample format: position, pressure (0–1), tilt
  (x/y, −1 to 1), barrel rotation, tangential pressure, phase
  (hover/down/move/up), tool (mouse/pen/eraser), pen buttons and a
  timestamp.
- **macOS** reads AppKit tablet events directly through a local event
  monitor. Input-event merging is switched off, so every sample arrives.
  Proximity events tell us whether the pen tip or the eraser end is in use.
  - AppKit crashes the app if tablet-only fields are read from a plain
    mouse event, so every read is checked against the event type first.
- **Verified 2026-11-21 on a Veikk tablet:** full pressure range with 244
  distinct levels, tilt on both axes, about 340 samples per second, and the
  tip button detected. (That pen has no barrel rotation, which is expected.)
- Windows and Linux backends: ⬜ (see §3).

### Pen test (`symphony-pentest`) ✅
- A winit window with a CPU canvas (softbuffer + tiny-skia).
- Line width follows pressure, and the eraser end erases.
- Shows a pressure bar, a tilt dial, a tool indicator and a hover cursor.
- Logs every sample to `target/pen-log.csv` and prints samples/sec and
  max pressure/tilt each second.
- Run it: `cargo run --release -p symphony-pentest`

---

## 2. What's left (big picture)

See [ROADMAP.md](ROADMAP.md) for milestone order. In short:

1. **App shell**: a real window with dockable views and workspaces.
2. **Drawing engine**: vector strokes, art layers, onion skin, undo.
3. **Timing**: Timeline and Xsheet views over the same data, plus playback.
4. **Ink & paint**: palettes with colour IDs, the paint tools, gap closing.
5. **Rigging**: Node view editing, pegs, Function view, substitution.
6. **Deformers**: bone, curve, envelope and more.
7. **Effects and compositing.**
8. **Render and export.**
9. **Import and interop**, including reading Harmony scenes from user files.
10. **Scripting.**
11. **Windows and Linux pen backends**, and wide tablet compatibility.
12. **Native file format**: save, load, autosave, crash recovery.

---

## 3. Tablet compatibility plan

The goal is **every tablet**. One person can't own every tablet, so
compatibility will be a community effort.

### Simulating tablets
| OS | How to fake a tablet | What it tests |
|---|---|---|
| macOS | Post synthetic `CGEvent`s with tablet fields (pressure, tilt, proximity). | Our AppKit backend end to end, in CI. |
| Windows | `InjectSyntheticPointerInput` with `PT_PEN` (Windows 10 1809+). | The Windows Ink path in CI. Wintab can't be faked this way and needs real hardware or a community report. |
| Linux | A virtual tablet through `uinput` (evdev `ABS_PRESSURE`, `ABS_TILT_X/Y`, `BTN_TOOL_PEN`/`RUBBER`). | XInput2 and Wayland tablet-v2, in CI under a headless compositor. |
| All | **Replay recorded logs.** `pen-log.csv` files from real tablets are fed straight into the drawing pipeline. | Stroke smoothing, pressure curves and drawing behaviour, independent of the device. |

### Community testing
- A "Tablet report" issue template asks for brand, model, OS, driver
  version and the pen test summary, with `pen-log.csv` attached.
- A compatibility table (`docs/TABLETS.md`) gets updated from those reports.
- Real logs from reports become regression tests through log replay.

### Backends still to write
- **Windows**: Windows Ink (`WM_POINTER`) plus Wintab for older Wacom,
  Huion and XP-Pen drivers, with a switch in Preferences, like other
  professional apps have.
- **Linux**: Wayland `tablet-unstable-v2`, and XInput2 valuators on X11.

---

## 4. Full Harmony feature list (target: all of it)

Built clean-room from Toon Boom's public documentation
([CLEAN-ROOM.md](CLEAN-ROOM.md)). Items marked *(verify)* still need
checking against the docs or a video. This list will grow as research
continues.

### 4.1 Workspace and interface
- ⬜ Dockable, tabbable, floating views
- ⬜ Saved workspaces (Drawing, Animation, Rigging, Compositing…) and fast switching
- ⬜ Camera view (rendered scene, animate mode)
- ⬜ Drawing view (one drawing at a time, no transforms)
- ⬜ Timeline view
- ⬜ Xsheet view
- ⬜ Node view and Node Library
- ⬜ Tool Properties view
- 🟨 Colour view: scene palette and named solid swatches; multiple palettes pending
- ⬜ Layer Properties view
- 🟡 Function view (curve editor): peg functions as curves, select/drag keys, Linear/Bezier/Stop-motion segments, Bézier handles with Smooth/Straight/Corner continuity, add/delete keys, double-click to add. Still to do: Ease functions, zoom/pan of the graph, multiple key selection, Timeline keyframe editing, other node parameters.
- ⬜ Library view (templates and symbols)
- ⬜ Top, Side and Perspective views (3D scene planning)
- ⬜ Reference view (a second, frozen camera view), with Mirror mode (27)
- ⬜ Model view (character reference)
- ⬜ Coordinates and Control Points view
- ⬜ Script Editor view
- ⬜ Message log
- ⬜ Customisable toolbars
- 🟡 Keyboard shortcut sets: a Harmony-style set built from the published
  default list, plus custom sets (Harmony 27 defaults verified for every built action; custom sets later)
- ⬜ [Contextual keybindings](plans/contextual-keybindings.md) with Symphony as the default, Harmony and Photoshop presets, and a choice of hover or interaction to activate a panel.
- [Icon redesign inventory](plans/icon-inventory.md)
- ⬜ Preferences dialog (all tabs)
- 🟡 Rotate, flip and zoom the canvas. Reset view. (Harmony toolbars and resets done; flipping the view still to do)
- ⬜ Field guide, grid, rulers, safe areas, guides (including perspective guides)
- ⬜ Light table and Shift & Trace

### 4.2 Drawing
- 🟡 Pressure and tilt input (macOS ✅)
- 🟡 **Pencil**: centreline with thickness, pressure-driven thickness, editable afterwards (draws; editing comes with the Contour/Pencil editors)
- 🟡 Pencil line textures and pencil presets (presets, Round/Flat tips and thickness stencils done; textures next)
- 🟡 **Brush**: filled contour, pressure size and opacity, smoothing (size and smoothing done; opacity later)
- 🟡 **Textured vector brushes** retain editable points and use bitmap tips
  or grain for appearance. Converted imported presets belong in the vector
  brush list.
- ⬜ **Bitmap painting** stores paint as pixels and uses a separate brush
  category. Implement a pixel canvas with undoable dabs and erasing, then
  map imported brush dynamics and blending for familiar source-app results.
  Existing vector imports stay editable.
- 🟡 Eraser (vector and bitmap): brush strokes and paint lose exactly the eraser's shape (outlines, polygon difference); pencil lines cut along the centreline with Round/Flat tips; pressure size; options for all art layers, all Timeline layers and all frames (done). Still to do: Bevel tip, eraser presets, bitmap.
- ⬜ Line, Rectangle, Ellipse and Polyline tools
- 🟡 Text tool (✅ type in the Camera view; font, size, bold, italic and
  alignment; the letters become vector shapes, like Harmony's text after
  Break Apart. Still to do: re-editing committed text, Justify.)
- 🟡 Stroke tool (invisible lines used for painting zones): invisible strokes exist (Close Gap makes them); the drawing tool is next
- ⬜ Contour Editor (edit vector points and Béziers)
- ⬜ Centerline Editor
- ⬜ Pencil Editor (reshape thickness)
- ⬜ Smooth Editor
- ✅ Cutter (lasso cut)
- ✅ Select tool (lasso, marquee, transform selection)
- ✅ **Cutter mode** for Select. Use the Select tool’s existing boundary
  gestures to split artwork at the selection edge. Select the enclosed
  pieces for moving, transforming, or deleting, with the outside pieces
  left in place. Preserve editable vectors and textured brush appearance.
  Expose the mode in Tool Properties and Search, and make the split
  undoable in one step.
- ⬜ Perspective tool
- ⬜ Envelope tool (warp a selection)
- ⬜ Morphing tool and morph sequences (Premium)
- ✅ Stabiliser, smoothing and pressure curve settings
- ⬜ Auto-flatten, auto-merge, draw behind, and "Draw on Colour Art" options
- ⬜ Auto close gap
- ⬜ Snap to contour, snap to grid, snap to alignment guides
- ✅ Art layers: Overlay, Line Art, Colour Art, Underlay (toolbar, L, :, P, preference)
- ✅ **Sketch art layer** on each Drawing node. Edit sketches in their own
  mode. Show them at reduced opacity while working in Line Art or Colour
  Art. Exclude them from exports by default and provide an explicit
  **Include Sketch** export option. (Under the other art layers; faded as
  a whole in renders.)
- ⬜ Art Layer Manager (25.2) *(verify)*
- ✅ Create Colour Art from Line Art (*)
- ✅ Onion skin (previous and next drawings, colours, opacity, range)
- ⬜ Reposition All Drawings
- ⬜ Rotary table
- ✅ Hand and Zoom tools
- 🟡 Undo/redo history view (undo and redo work everywhere; the history view itself is later)

### 4.3 Ink and paint, colour
- ✅ **Scene palette with colour IDs**: edit a swatch and everything using it recolours
- ⬜ Solid, linear gradient, radial gradient and bitmap texture swatches
- ⬜ Palette levels: scene, element, job and environment *(verify naming)*
- ⬜ Palette lists, palette browser and master palettes
- ⬜ Palette cloning (for example day and night versions of the same palette)
- ✅ Paint tool modes: Paint, Paint Unpainted, Repaint, Unpaint
- ⬜ Ink tool (recolour lines)
- ✅ Dropper
- ✅ Close Gap tool
- ⬜ Edit Gradient/Texture tool
- ✅ Protect colour (swatch lock, Respect Protected Colours, Shift+S)
- 🟡 Show Strokes, Show Unpainted Zones (Show Strokes done)
- ⬜ Colour override
- ✖ Generative fill (Harmony 25.2's AI fill): **won't be added.** Symphony has no
  generative AI (see [AI-POLICY.md](AI-POLICY.md)).

### 4.4 Timing
- ⬜ **Timeline**: layers, exposure bars, keyframes, layer parenting,
  extend and reduce exposure, drag timing
- ⬜ **Xsheet**: columns, cells with drawing names, Increase/Decrease
  Exposure, Fill Empty Cells, Sequence Fill, holds, keyframe markers
- ✅ Timeline and Xsheet editing the same data, live
- 🟡 Drawing substitution: [ and ], Go to Next/Previous Drawing (G, F), Timeline Parameters field (drag or type) done. Still to do: the Drawing Picker and Library substitution panels, range selections.
- ⬜ Annotation column (notes and drawings in the Xsheet)
- ⬜ Function columns in the Xsheet (Bézier, Ease, Expression, 3D Path, Quaternion)
- ⬜ Timing column (drawings from another element)
- ⬜ Scene markers and ranges
- ⬜ Frame rate setting, scene length, start and stop
- ⬜ Playback: real-time playback, loop, range, playback speed, sound
  scrubbing, jog/shuttle, real-time cache
- ⬜ Sound: sound column, waveform, scrubbing
- ⬜ **Lip-sync**: automatic mouth-shape detection (A–H, X chart) and mapping to drawings

### 4.5 Animation
- ⬜ Animate mode (on/off)
- 🟡 Transform tool (move, rotate, scale, skew, pivot): move, rotate, scale and flip done; skew and pivot dragging next
- ⬜ Keyframes: motion and stop-motion keyframes, ease in/out, constant segments
- ⬜ Motion paths (on-screen editing)
- ⬜ 3D paths and 3D transforms
- ⬜ Function editor: Bézier, ease, velocity graphs, expressions
- 🟡 Linear keyframe interpolation (core ✅)
- ⬜ Pivots: permanent pivot, temporary pivot, pivot per drawing
- ⬜ Copy, paste and Paste Special (with keyframe options)
- ⬜ Inverse kinematics (IK tool, IK constraints, nail/hold)
- ⬜ Animate Current Drawing Only, plus the other animate modes
- ⬜ Show/hide manipulators and control points

### 4.6 Rigging and node view
- 🟡 **Skeleton setup view** (✅ double-click a Skeleton (or right-click,
  Open skeleton setup): rest-pose diamonds, drag a base or tip, hold an
  endpoint then drag to grow a bone, a thumbnail of each bone's drawing,
  Back; Layer Properties is now Node Properties. Still to do: thumbnails
  in Node Properties, connecting new artwork straight to its bone from the
  view, showing the posed rig in Camera when an attached drawing is
  selected). Open the Skeleton node in the Node View panel
  and use Back to return to the graph. Show diamond-shaped bones in their
  rest pose. Drag a base or tip to reshape the rig, keep shared endpoints
  together, and hold an endpoint to grow a new bone from it. Rename Layer
  Properties to Node Properties and show a single thumbnail of each bone’s
  directly attached drawings at the current frame. New artwork can connect
  straight to its bone. Preserve existing pegs that carry transforms or keys.
  Show the posed rig in Camera when a bone or its attached drawing is selected.
- 🟡 **Smart rigs** (✅ Locked and Aim anchors on any bone tip (A cycles,
  Bone properties), drawn and draggable in Camera; dragging a joint solves
  the chain to the nearest locked joint (FABRIK) and re-plants locked
  chains below it; with no anchors, bones pose one at a time; F flips a
  smart joint, arrows nudge, Shift snaps to 22.5°. Anchors on the root's
  base and dragging the rig by a free point with no anchors are still to
  do). Toggle any joint or free endpoint into an editable
  anchor. Locked anchors pin joints to their targets. Aim anchors let joints
  move while their bones aim at the targets. With anchors active, dragging
  a free point solves the rig around them. With no anchors, dragging a free
  point moves the rig. Add contextual shortcuts for toggling anchors and
  flipping bends, arrow-key nudging, and Shift-drag along straight lines
  snapped to 22.5° increments. Defer dedicated controller objects.
- ✅ **Bone flexibility and stretch limits** (Bend radius; Rigid, Simple
  Stretch, Squash & Stretch; optional shortest/longest with resistance;
  the solver stays in range). Give bones separate bend and
  stretch controls. Simple Stretch changes length at constant width.
  Squash & Stretch adjusts width in the opposite direction to suggest
  volume. A stretchable bone can be unlimited or have a minimum and maximum
  length. Resistance increases as it approaches either configured limit.
  Keep locked anchors planted and constrain a drag to the rig’s reachable
  range, including the stretch permitted on that chain.
- ⬜ **Sticky Timeline ruler**. Keep frame numbers visible at the top while
  the track rows scroll vertically.
- ⬜ **Find Stroke Node**. Tap a stroke in Camera view to select its
  source Drawing node across visible layers. Reveal the node in Node View
  and select its Timeline row at the current frame. Explore a modifier
  in the Select tool and make the same action available through Search.
  Add optional pick-through filters for generated gradients, branches with
  an Opacity node below 100% at the current frame, and effect overlays that
  can obscure a stroke. Skip the overlays for picking while keeping their
  rendered appearance and allowing the artist to target them explicitly.
- 🟡 Pegs and peg hierarchy (core model ✅, no UI)
- 🟡 Drawing nodes and Composite stacking order (core model ✅)
- ⬜ Node view editing: add, connect, disconnect, reorder ports, groups,
  enable/disable, colours, navigation
- ⬜ Groups with multi-port in/out nodes, and nested groups
- ⬜ **Attribute Controllers** (27): non-destructive pose offsets on rig
  attributes, Setup Mode, train from keyframes, pose indicators (27.1)
- ⬜ **Skeleton Controllers** (27.1 beta): animate once, reuse on any
  character with the same skeleton
- ⬜ Master controllers (sliders, point controllers, wizards). Partly
  superseded by Attribute Controllers, but still needed for old rigs.
- ⬜ Selection Lock node (27): make rig layers unselectable in the Camera view
- ⬜ Cut-out rig templates and the library
- ⬜ Symbols
- ⬜ Rig lock / kinematic output *(verify)*
- ⬜ Auto-patch
- ⬜ Backdrops in the Node view

### 4.7 Deformation
- 🟡 **Mesh Warp**. Non-destructive deformation for a drawing or group (✅
  node in the peg chain; points dragged in the Camera view with the
  Transform tool, keyed in Animate mode; bends in the canvas and renders).
  Kinds: ✅ Grid (Rows, Columns, (x)/(y) Axis, Width, Height, Deformation
  Quality) and ✅ Points (Harmony 24 Shape-Aware style: place points on the
  art, Build mesh over its shape, as-rigid-as-possible bending, only
  connected art follows). Lines: ✅ Paths (keep thickness) or ✅ Whole shape
  (thickness follows the warp). Still to do: rotating/scaling single points,
  bones and cages, per-point Weight, rigidity maps, Bézier handles on grid
  points, bending imported pictures.
- ⬜ Bone deformer (bias, regions of influence, chains)
- ⬜ Game Bone deformer (game export, no bias or regions)
- ⬜ Curve deformer (Bézier chain)
- ⬜ Envelope deformer (closed loop)
- ⬜ Free-form deformer *(verify)*
- ⬜ Weighted deformation *(verify)*
- ⬜ Deformation Composite node
- ⬜ Rigging tool and deformation animation tool
- ⬜ Deformation on bitmap and textured drawings (with the quality caveats Harmony documents)

### 4.8 Camera and 3D scene
- ⬜ Camera node, camera framing, multiple cameras, camera switching
- ⬜ Multiplane (Z depth), with Top and Side view editing
- ⬜ 3D rotation of 2D layers
- ⬜ Import 3D models (OBJ, FBX, Alembic, 3DS) *(verify list)*
- ⬜ Camera shake (Quake effect)

### 4.9 Effects (nodes)
Every effect is a node in the graph.
- ⬜ Blur: Gaussian, Directional, Radial, Variable
- ⬜ Glow, Bloom
- ⬜ Shadow and Highlight (tone shading on drawings)
- ⬜ Drop Shadow
- ⬜ Light shading and surface normals (2D lighting)
- ⬜ Colour Card, Colour Override, Colour Scale, Colour Fade
- ⬜ Brightness-Contrast, Hue-Saturation, Gradient, Grain, Noise
- ⬜ Transparency
- ⬜ Cutter and Inverse Cutter (matte cutting)
- ⬜ Matte Composite, Matte Resize
- ⬜ Glare, Refract *(verify)*
- ⬜ **Turbulence**. Use one noise generator with two modes. Distort warps
  connected artwork. Generate Noise outputs a pattern for a texture or
  matte without an image input. Share Scale, Evolution, and Seed, and show
  mode-specific controls where relevant. Let Tab Search open the appropriate
  mode for “Turbulence” or “Turbulent Noise”.
- ⬜ Motion blur
- ⬜ Particle system (generators, forces, baking)
- ⬜ Line Art / Colour Art separation in effects
- ⬜ Effect SDK / plugins (Premium)

### 4.10 Compositing and render
- 🟡 Composite node render order (core ✅)
- ⬜ Composite modes: bitmap, pass-through, vector
- ⬜ GPU preview render (OpenGL-style) and full-quality Render view
- ⬜ Write node with output settings
- 🟡 Image sequences: PNG, TGA, TIFF, PSD, OpenEXR, JPEG (PNG done)
- 🟡 Movies: MP4/H.264, MOV (ProRes), GIF (GIF and H.264 MP4 done, through Cisco's OpenH264; ProRes later)
- ⬜ Command-line and batch rendering
- ⬜ Sprite sheet export
- ⬜ Game engine export (Unity / Godot)

### 4.11 Import and export
- 🟡 Import bitmaps (as bitmaps, or vectorised) (picture ✅, vector
  drawing ✅ (Line art / Colour), editable bitmap drawing ⬜). Formats: PNG,
  JPEG, WebP, BMP, TIFF, GIF, TGA, EXR, HDR, ICO, QOI, PBM/PGM/PPM, DDS,
  Farbfeld ✅; animated GIF/PNG/WebP as timed sequences ✅
- 🟡 PSD import as separate layers ✅ (also OpenRaster layers, Krita
  flattened), PSD export ⬜
- 🟡 Import AI, PDF and SVG (SVG as an editable vector drawing ✅). Export SVG.
- ⬜ Import sound (WAV, MP3, AIFF) and video
- 🟡 Vectorise (pix2vec-style line extraction) (on import ✅)
- ⬜ Library audio bin with detail view (27.1)
- ⬜ Templates: save and load (`.tpl`-style folders, our own format)
- ⬜ Harmony interop: read `.xstage` scenes from user-made files (clean-room),
  including the compact single-file scene format (25+)
- ⬜ Exchange formats: OpenToonz, Krita and Blender Grease Pencil where practical

### 4.12 Scripting and automation
- ⬜ Scripting engine (JavaScript-like API: `scene`, `node`, `column`, `frame`, `selection`…)
- ⬜ Python API
- ⬜ Script Editor, script toolbar buttons, script UIs
- ⬜ Command-line utilities (render, scene inspection, conversion)

### 4.13 Production
- ✅ Native scene format: save, autosave, crash recovery, versions
- ⬜ Project and scene browser
- ⬜ Scene setup (resolution, frame rate, alignment)
- ⬜ Multi-user database mode (like Harmony Server) — last, and optional
- ⬜ Scan/paper workflow (import scanned drawings, registration)

### 4.14 Symphony extras (after the main chunk)
- ⬜ **Use no AI** switch and per-scene "No AI used" provenance (DESIGN §16, ENGINEERING E17)
These are improvements and new features. They come after the Harmony list
above is in place.
- ⬜ **Showcase demos**. Replace the current GIFs with clearer footage of real artist workflows. Make playback loop on the page and keep the still previews for reduced motion.

---

*Last updated: 2026-11-21 (M3 drawing, undo, onion skin, saving)*
