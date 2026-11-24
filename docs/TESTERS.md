# Testing Symphony

Thanks for helping test Symphony. You install one thing, the **Symphony
Launcher**. It installs Symphony, keeps it up to date, and can switch back
to the previous version if an update has a problem.

## 1. Download the launcher
Go to [the Symphony site](https://tdemers218.github.io/symphony/) and click
**Download**. It picks the right file for your computer. (Every file is
also on the [releases page](https://github.com/tdemers218/symphony/releases):
`symphony-launcher-macos.zip` for any Mac, and the Windows or Linux
launcher.)

## 2. Open it for the first time
Symphony isn't code-signed yet, so your computer shows a warning the first
time. This happens once.

- **Mac**:
  1. Unzip the file and move **Symphony Launcher** to Applications.
  2. Double-click it. When macOS says it can't verify the app, click
     **Done**.
  3. Open **System Settings → Privacy & Security**. Scroll down and click
     **Open Anyway** next to Symphony Launcher. Then confirm.
- **Windows**:
  1. Unzip the file and run `symphony-launcher.exe`.
  2. If SmartScreen appears, click **More info → Run anyway**.
- **Linux**: extract the file and run `./symphony-launcher`.

## 3. Install and launch
1. Click **Install Symphony**. The launcher downloads it and checks the
   download before installing.
2. Click **Launch Symphony**.

Next time, open the launcher and click **Launch**. When an update is out,
the launcher shows what changed and an **Update** button.

## If an update breaks something
The launcher keeps the version you had before. Pick it under
**Installed versions**, launch it, and tell us what went wrong.

## Reporting
- Bugs and ideas: [open an issue](https://github.com/tdemers218/symphony/issues).
- Tablets: run the pen test and file a *Tablet report* (see
  [CONTRIBUTING.md](../CONTRIBUTING.md)).

## Update from the editor

Click **Update** beside Help. Save any edits when prompted. A release build
checks your selected release channel and installs an available update.
Click **Launch** when it finishes. Symphony Dev instead rebuilds the
current source checkout and opens the resulting app, including local edits.

## Edit the Node view

Use **Add node** or press **Tab** and search for a node by name, such as
**Add Peg** or **Add Box Blur**. Search still includes actions from other
views. Double-click a node to open its properties. Drag a node card
to place it or drag blank space to pan. Drag from a bottom output to a
compatible top input to connect them.

Drag from a connected input to another input to move the cable, or drop
it on blank space to disconnect it. You can also right-click a connected
input and choose **Disconnect input**. Press Escape to cancel an active
drag. Undo and redo cover placement, node creation, and cable edits.

Node positions and cables are saved with the project. After a manual
cable edit, composite input order controls stacking in the graph.

Scroll in the Node view to zoom around the pointer. Drag empty space
to pan. Hold Shift while moving a connected node to line up nearby
ports vertically. A two-rectangle minimap appears when nodes extend
beyond the viewport. Use **View → Arrange Nodes** or find **Arrange
Nodes** in Tab search to lay the graph out by signal flow. Undo restores
the previous positions.

Selecting a node reveals its Timeline track and updates **Layer
Properties**. The floating properties window uses the same controls.
Pegs have their own tracks. Select a connected peg and use **Transform**
in Camera to move the artwork through that peg. The drawing's strokes
stay in drawing coordinates.

Choose a frame and click **Set key** in node properties. Every node can
key its enabled state. Peg transforms and numeric effect properties can
also change over time. Keys appear on the node's Timeline track.

Hold **Alt** (Option on Mac) while dragging a node with free ports.
Drop it near a compatible open socket to connect it, or onto a cable
to insert it between the connected nodes. Proposed wires are highlighted
during the drag. Undo restores the old wiring and node position together.

Option-click a cable or a connected input to cut that wire. Option-drag
a single connected node away to disconnect its wires. Shift-click to
add a node to the selection, then drag a selected node to move the group.

Double-click **Camera** in the Node view to change the horizontal field
of view. The presets are **Tight** (35°), **Standard** (60°) and **Wide**
(90°), with a custom slider. The framing affects Camera and exports.
Click or drag a slider and press Left or Right for small adjustments.

**Auto-key** records changes at the current frame. With it off, an
unanimated setting remains constant until you set a key. The diamond
button sets a key at the playhead or removes it if that track already
has one there.

Drag the Tools grip to any window edge. Top and bottom make the toolbar
horizontal. Double-click a tool to open its properties in the docked
tab or in a floating panel. Right-click a tab for **Eject**, **Remove**,
or **Add Tab**, and right-click the empty tab strip to add a view to that
section.

## Stage a multiplane shot

Open **Multiplane** from the **Windows** menu or search **parallax**.
Choose a drawing layer and move its peg in **Z**. Drag its plane in the
side diagram or type a value in **Peg Z**. Positive values move it away
from Camera. **Maintain Size** keeps that drawing's screen position
and size at the current frame while you set depth.

Open **Camera properties** and key **Camera X**, **Camera Y**, or
**Dolly Z** over time. Camera X and Y pan the shot. Positive dolly moves
the camera forward. Near artwork moves faster than far artwork, and
Camera and exports use the same projection. Composite port order still
controls stacking.

## Pose a skeleton rig

In Node View or Tab Search, choose **Add Skeleton** or **Add Bone** to
start with a connected Skeleton and Bone. Select a Bone and choose
**Add Bone** again to extend the chain. The Skeleton moves the whole rig.

In **Bone properties → Rest pose and deformation**, set the length and
rest angle over the artwork. Set the root origin on the first bone.
Connect a Bone’s output to one or more Drawing inputs. To keep an
existing drawing peg, connect the Bone to that peg instead. Artwork is
editable in Drawing view and bones are visible as controls in Camera.

With a Bone selected, drag its tip in Camera to pose it. Right-click the
joint where two bones meet and choose **Make smart joint**. Drag the
square target at the end of the second bone to pose both bones together.
Use **Flip bend** in Bone properties to choose the direction of the
elbow or knee. Use **Auto-key** or the Timeline’s **K** to animate the rig.
Bones can be animated without artwork and are excluded from exports.
Vector artwork bends at the joint. Imported pictures follow their bone
as rigid artwork.

## Timeline keys and frames

In Timeline, press **K** or click the diamond-plus button to set a
keyframe on the selected node at the playhead. The **Frames** menu and
**Tab** search let you insert a blank drawing, duplicate the current
drawing, extend its exposure, or reuse an existing drawing from the
layer. Insert and duplicate place the new exposure after the playhead
and shift later exposures on that layer.

A duplicate is independent artwork. Reuse exposes the same drawing
again, so edits appear in each of its exposures. To shift timing across
all layers, search for **Insert In-between Frame** or type a quantity
such as **insert 3 frames**.

## Select and colour drawings

With **Select**, click an existing stroke to select it. On empty space,
click to place straight anchors, then click and drag an anchor to make a
curve. Click the first anchor or press Enter to finish. Drag from empty
space before placing any anchors to draw a freehand lasso.

Shift adds to the current selection and Control subtracts. Use the
selection handles to move, scale, or rotate it. Backspace deletes selected
strokes. **F** applies the active swatch to selected lines and the closed
regions they enclose.

Click a swatch to open the **Square** or **Triangle** colour picker. The
swatch's **Opacity** sets its maximum coverage. A pressure-sensitive
brush's opacity multiplies that coverage.

## Render simple effects

In **Node view → Add node**, choose **Box Blur**, **Grain**, **Gradient**,
or **Opacity**. Route an image through one or more effects before it
reaches Display. Gradient generates a background on its own. Connect it
to a Composite input behind your drawings.

Select a node and click **Properties**. Set blur radius, grain amount
and seed, gradient colors and angle, or opacity. Click **Apply** to
commit the change as one undo step.

**Camera** renders the current frame from the Camera node, including
compositing and effects. The black area outside the stage masks artwork. **Drawing** shows the current exposure of the
selected drawing node in its own coordinates. Use **Preview All Art
Layers** to see the entire drawing while you edit.

Exports render the same Display graph. From the Node view, **Render
preview** also lets you check an individual frame.

## Import brush packs

Drop a brush file or pack onto Symphony. The importer adds compatible
presets to your brushes and reports entries it could not read.

| Source | Formats |
| --- | --- |
| Photoshop | Sampled tips from `.abr` versions 1, 2, 6 and 10 |
| Krita | `.kpp` presets and `.bundle` resource bundles |
| MyPaint | `.myb` presets and exported brush pack `.zip` archives |
| Procreate | `.brush` and `.brushset` |
| Clip Studio Paint | `.sut` |
| GIMP | `.gbr` tips and `.gih` image pipes |

Photoshop sampled presets can carry their name, tip size, spacing, angle,
roundness, and supported pressure controls into Symphony. Krita imports
bitmap tips and recreates generated round and rectangular tips with their
aspect and edge fades. MyPaint maps pressure to diameter and opacity.
Each GIMP image pipe cell imports as an individual preset.

Re-import a source file to apply improvements to its presets. Photoshop
presets previously imported under numbered names may appear alongside
their newly named versions. Imported presets appear alongside the other
vector brush presets and retain editable points even when they use a
bitmap tip or grain. Pixel painting is planned as a separate brush
category. Some source behaviours, such as Photoshop dual brushes and
scatter, Krita smudging, and MyPaint’s time-based dabs, are still outside
the importer’s mapping.

### Compare brush rendering

Generate a repeatable sheet and a tab-separated report for each paint
preset in a file.

```sh
SYMPHONY_BRUSH_SHEET_IN=/path/to/brush-pack.abr \
SYMPHONY_BRUSH_SHEET_OUT=/tmp/symphony-brush-sheets \
cargo test -p symphony-app brush_sheet -- --ignored
```

Each 960 × 480 PNG shows a full-pressure tap at pixel (100, 80), a
full-pressure stroke at y=240, and a 5%–100% pressure ramp at y=400.
Both strokes run from x=100 to x=800. Nominal diameter is 64 px and
the imported width ratio, tip, texture and pressure curves are retained.
The report includes original diameter, spacing, flow, and measured
coverage on a 0–255 scale.

To compare renders from a source app, align black-on-opaque-white
960 × 480 PNGs to the same positions and use the generated filenames.
Set `SYMPHONY_BRUSH_SHEET_REFERENCE` to their directory and run again.
The report adds mean absolute coverage error separately for the tap,
steady stroke and ramp, excluding pixels where both sheets are white.
A zero means identical coverage. A blank comparison means the reference
or drawn area is missing.

## Mask artwork

Add **Mask** in Node view or Tab search. Connect the artwork to its
**Image** port, and a drawing or other image to its **Matte** port.
Connect the Mask output to your composite. The matte's alpha keeps
the covered portion of the image. Use **Invert matte** to cut it away.

Right-click a Drawing card and choose **Convert to Mask Drawing**.
It remains editable in Drawing and retains its exposures and wiring.
The drawing renders only in matte branches while converted. Choose
**Convert to Drawing** to restore its ordinary image output.

## Harmony scene structure

Choose **File → Import Harmony scene structure** and select a standalone
`.xstage` file. Save the result as a new Symphony project. The importer
reads scene resolution, frame rate, frame count, drawing layers and exposure
ranges. Direct drawing inputs to the display composite follow source port
order. Cells are editable empty drawings with the source drawing names.

**Windows → Node Library** lists native node types, the imported scene’s
node counts, and a searchable [Harmony 27 Premium node reference](https://docs.toonboom.com/help/harmony-27/premium/nodes/about-node-view.html).
The original scene XML is retained in the native save. Linked artwork,
palettes, keyframed rigs, nested compositing, and effects still require
conversion. The reference list describes Harmony’s catalogue.

## Scene and movie timing

Choose 12, 24, 30 or 48 fps in the welcome screen. Change the scene rate
later in the Timeline or the Timing menu. The rate changes how fast scene
frames play.

**Timing → Insert in-betweens** inserts frames after the playhead at any
scene rate. Existing drawings hold through the new frames and later
exposures and peg keys move together. Use **Create Empty Drawing** to
start a fresh drawing in one of those frames. Undo restores the previous
timing.

**File → Export → Movie** uses the scene rate by default. Choose an
output rate to sample the scene at a different cadence while preserving
its duration to the nearest output frame.
