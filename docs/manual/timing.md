# Timing

Timing is which drawing shows on which frame, and for how long. The
**Timeline** and **Xsheet** show the same timing two ways; an edit in one
shows in the other.

Words used on this page:

- An **exposure** is a drawing shown on a run of frames. Holding a
  drawing for two frames is "on twos".
- A **cell** is one frame of one layer.
- A **key** stores a value, such as a peg's position, on a frame.

## Timeline and Xsheet

- **Timeline**: layers are rows and frames run left to right. It shows
  exposures, node tracks with their keys, sound and bookmarks.
- **Xsheet**: layers are columns and frames run down, like a paper
  exposure sheet. Each cell shows the drawing's name.

Click a frame to move the playhead. `.` and `,` step one frame; `Shift+.`
and `Shift+,` jump to the last and first frames.

Selecting a node in the Node view shows its track in the Timeline. **Show
all node tracks** shows the tracks of nodes that aren't animated too.

## Change how long a drawing shows

With the pointer over the Timeline or Xsheet:

- `+` (**Increase exposure**) holds the current drawing one frame longer.
  Later frames on that layer move along.
- `-` (**Decrease exposure**) shortens it by a frame.
- **Extend Exposure** holds it one more frame too, from the **Frames**
  menu.

Right-click a cell for **Clear exposure** (empties the cell; the drawing
stays in the layer) and **Remove frame** (takes the cell out and pulls
the rest of the layer one frame earlier).

## Add drawings to a layer

From the Timeline's **Frames** menu, the **Timing** menu, or search:

| Action | Symphony key (over Timeline/Xsheet) | Does |
| --- | --- | --- |
| Insert New Drawing Frame | `N` | A new, empty drawing after the playhead. Later exposures on the layer move along. |
| Duplicate Drawing | `D` | A copy of the current drawing after the playhead. The copy is independent. |
| Clone Drawing | `C` | A linked copy: drawing on one changes both. |
| Reuse Existing Drawing | – | Exposes a drawing the layer already has. Edits show in every exposure of it. |
| Copy Drawing, Paste Drawing | `Ctrl+C`, `Ctrl+V` | Paste puts an independent copy in the current cell. |
| Create Empty Drawing | `Alt+Shift+R` (both profiles) | Replaces the current cell with a new, empty drawing. |

Use **Duplicate** when you want to start from a drawing and change it, and
**Clone** or **Reuse** when the same drawing should come back unchanged,
such as a blink or a cycle.

## Swap a drawing in a cell

- Drag sideways on the drawing name in a cell to swap it for another
  drawing in the layer, or double-click the name and type one.
- `[` and `]` (**Substitute Drawing Previous / Next**) step through the
  layer's drawings in the current cell.
- `F` and `G` (**Go to Previous / Next Drawing**) move the playhead to
  where the previous or next drawing starts.

In the Symphony profile these four keys work with the pointer over the
Timeline or Xsheet; in the Harmony profile they work anywhere.

## Layers

- **Add drawing layer**: `L` over the Timeline [`Ctrl+R`].
- Right-click a layer for **Rename layer**, **Hide layer** / **Show layer**
  and **Delete layer**. `V` over the Timeline hides or shows the selected
  layer (Symphony profile).

## Insert in-betweens across the scene

**Timing → Insert in-betweens** inserts frames after the playhead on
every layer at once. Choose how many and click **Insert after playhead**.

- Current drawings hold through the new frames.
- Later exposures, peg keys, bookmarks and Smart Charts move together.
- In search, type "insert 3 frames".
- **Insert In-between Frame** (`I` over the Timeline) inserts one frame.

Then put the new in-between drawings in: **Create Empty Drawing** on a
new frame starts a fresh one.

## Keys

- **Set Keyframe**: `K` over the Timeline, or the diamond button. On a
  frame that already has a key on that track, the button removes it.
- **Delete Keyframe**: `Shift+K` (Symphony profile).
- **Auto-key** (`A` over the Timeline, or **Timing → Toggle auto-key**)
  records changes as keys at the current frame. With it off, a setting
  that isn't animated stays the same on every frame until you set a key.

Shape what happens between keys in the [Function view](animating.md#the-function-view).

## Bookmarks

Bookmarks name a frame or a range, such as "contact" or "anticipation".

- **Bookmarks → Add bookmark here** adds one at the playhead. Give it a
  **Name**, a **Start** and **End**, a **Custom colour** and **Notes**,
  then **Save bookmark**.
- Bookmarks sit in a lane pinned to the top of the Timeline. Search finds
  them by name and jumps there.
- Playback and exports ignore bookmarks.

## Sound

Import a sound file (**File → Import…**, or drop it on the window) to add
it as a track with its waveform. Supported: WAV, MP3, OGG, FLAC, M4A, AAC
and AIFF. Right-click the track to **Mute sound** or **Delete sound**.
Scrubbing sound is **Planned**.

## Playback

| Action | Key | Menu |
| --- | --- | --- |
| Play / pause | `Shift+Enter` | Play |
| Loop playback | – | Play → Loop playback |
| Play a range | – | Play → Playback starts here, Playback ends here |
| Play the whole scene | – | Play → Play the whole scene |

## Frame rate

The scene's frame rate is set when you create it (12, 24, 30 or 48 fps are
the presets). Change it in **Timing → Scene frame rate**. Changing it
changes how fast frames play, not how many there are.

To deliver at another rate, choose a **Movie frame rate** when you
export. Symphony samples the scene at that rate and keeps the shot's
duration. See [Import and export](import-export.md#movie-frame-rate).
