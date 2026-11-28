# Import and export

## Import files

Choose **File → Import…**, click **Import** in the top bar, or drop files
on the window. Symphony works out what each file is.

| What | Formats | Becomes |
| --- | --- | --- |
| Pictures | PNG, APNG, JPEG, WebP, BMP, TIFF, GIF, TGA, EXR, HDR, ICO, QOI, DDS, farbfeld, PBM/PGM/PPM/PNM/PAM | A layer holding the picture. |
| Numbered pictures | `walk_001.png`, `walk_002.png`… | One layer, a drawing per frame. |
| Layered files | Photoshop `.psd`, OpenRaster `.ora`, Krita `.kra` | A layer for each of the file's layers, stacked in file order. |
| Vector pictures | SVG | Vector drawings you can edit. |
| Sound | WAV, MP3, OGG, FLAC, M4A, AAC, AIFF | A sound track in the Timeline. |
| Video | MP4, MOV, M4V with H.264 video, up to 5 minutes | An image-sequence layer, a drawing per frame at the scene's frame rate, plus its sound. Needs the OpenH264 codec (below). |
| Brushes | See [Brushes](#brushes) | Brush presets. |
| Symphony presets | `.symphonypresets` | Brush presets. |
| Harmony scenes | `.xstage` | A new scene; see [Harmony scenes](#harmony-scenes). |

Imported files are copied into the project, so moving or deleting the
original later doesn't break the scene. Big imports run in the background
and you can keep working.

## Brushes

Drop a brush file or pack on Symphony. Compatible presets join your
brushes, and Symphony reports anything it couldn't read.

| From | Formats |
| --- | --- |
| Photoshop | Sampled tips from `.abr` versions 1, 2, 6 and 10 |
| Krita | `.kpp` presets and `.bundle` resource bundles |
| MyPaint | `.myb` presets and exported brush pack `.zip` archives |
| Procreate | `.brush` and `.brushset` |
| Clip Studio Paint | `.sut` |
| GIMP | `.gbr` tips and `.gih` image pipes |

What carries over:

- **Photoshop**: name, tip size, spacing, angle, roundness and the
  supported pressure controls.
- **Krita**: bitmap tips, and generated round and rectangular tips with
  their aspect and edge fades.
- **MyPaint**: pressure mapped to size and opacity.
- **GIMP**: each image-pipe cell becomes its own preset.

Imported brushes are Textured Vector brushes: lines stay editable even
with a bitmap tip or grain. Not mapped yet: Photoshop dual brushes and
scatter, Krita smudging, and MyPaint's time-based dabs. Re-import a file
to pick up importer improvements.

## Harmony scenes

**File → Import Harmony scene structure…** reads a standalone `.xstage`
file and makes a new scene from it. Save it as a new Symphony project.

It brings in the scene's resolution, frame rate and length, the drawing
layers and their exposures, and the order of drawings connected straight
to the display composite. Cells come in as empty drawings with the
original drawing names, ready to redraw or replace.

Not converted yet: linked artwork, palettes, keyframed rigs, nested
compositing and effects. The original scene XML is kept inside the
Symphony project, so later versions can convert more from it. See
[Coming from Harmony](harmony.md).

## Export

**File → Export** has three formats:

| Format | Gives |
| --- | --- |
| Animated GIF... | One `.gif`. |
| Image Sequence (PNG)... | Numbered files: `name_0001.png`, `name_0002.png`… |
| Movie (H.264 MP4)... | One `.mp4`. |

In the export window:

1. Click **Browse...** to choose where the file goes.
2. **Export range**: **All**, or a range of **Frames**.
3. **Resolution**: the scene's size, or a percentage of it.
4. **Include Sketch** also renders the Sketch art layer, which is left out
   by default.
5. Click **Export**. Symphony shows **Rendering frame N of M**; you can
   keep working or **Cancel**.

Exports render the same picture as the Camera view, through the node
graph, with effects on and Smart Charts and bones hidden. The stage
background exports as white.

### Movie frame rate

Movies use the scene's frame rate unless you choose another **Movie frame
rate**. Symphony samples the scene at that rate and keeps the shot's
duration to the nearest frame, so a 12 fps scene can deliver at 24 fps.

### Resume an export

If an export is interrupted by a crash or by **Cancel**, run the same
export again. Symphony reuses the frames it already finished. This works
for movies, GIFs and image sequences.

## The H.264 codec

H.264 is covered by patents, so Symphony doesn't include an H.264 encoder.
Movie export and video import use Cisco's free OpenH264 instead. The first
time you need it, click **Download codec and export** (or **Download
OpenH264 from Cisco** in **Preferences → Video**). Symphony downloads it
from Cisco, checks it against Cisco's published checksum, and keeps it in
its app data folder. **Remove OpenH264** deletes it.

Cisco's licence covers personal and unpaid use. Studios doing paid work
may need their own H.264 licence. Details are in [Codecs](../CODECS.md).
