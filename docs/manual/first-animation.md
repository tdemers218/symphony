# Your first animation

This walks through a short drawn animation, a ball falling and landing,
from a new scene to an exported GIF. It takes about ten minutes. The keys
are for the default **Symphony** profile; the Harmony-profile key is in
brackets where it differs.

## Create a scene

When Symphony opens, the welcome screen offers **New scene**,
**Open project…** and your **Recent projects**.

1. Click **New scene**.
2. Pick a preset. **HD 1080p** is 1920 × 1080 at 24 fps for 10 seconds.
   **Storyboard** is 12 fps for 30 seconds. You can change the size,
   frame rate and length here too.
3. Type a **Name** and choose a **Save location**. Symphony makes a
   project folder called `Name.symphony` there.
4. Click **Create scene**.

The scene starts with one drawing layer called **Drawing**, and the
Brush tool is selected.

## Draw the first pose

1. The playhead is on frame 1. Draw a ball near the top of the Camera
   view. Drawing on an empty frame creates a new drawing there.
2. Adjust the brush in **Tool Properties**: **Maximum size**, **Minimum
   size** (pen pressure moves between the two), and **Smoothing**.
   `+` and `-` resize the brush while the pointer is over the canvas.
3. Made a mistake? `Ctrl+Z` undoes and `Ctrl+Shift+Z` redoes (`Cmd` on a
   Mac). `E` picks the Eraser [`Alt+E`]; `B` goes back to the Brush
   [`Alt+B`].

## Add the next drawings

Most animation is drawn "on twos": each drawing shows for two frames.

1. Put the pointer over the Timeline and press `+` (**Increase
   exposure**). The ball now shows on frames 1 and 2. `-` shortens it
   again.
2. Press `.` twice to go to frame 3 (`,` goes back; `Shift+.` and
   `Shift+,` jump to the last and first frames). The frame is empty.
3. Turn on the onion skin with `O` [`Alt+O`]. The previous drawing shows
   faintly, so you can place the next one against it. The onion-skin
   button in the canvas toolbar sets how many drawings before and after
   are shown.
4. Draw the ball lower, then press `+` over the Timeline again. Repeat
   on frames 5 and 7, squashing the ball where it hits the ground on
   frame 7.

To start a new drawing on a frame that already has one, press
`Alt+Shift+R` (**Create Empty Drawing**). It replaces that frame's
drawing with a new, empty one; the old drawing stays in the layer.

## Play it back

- `Shift+Enter` plays and pauses.
- **Play → Loop playback** repeats the animation.
- To play part of the scene, go to a frame and choose **Play → Playback
  starts here**, then **Playback ends here** on a later frame. **Play the
  whole scene** clears the range.

## Fix the timing

Watch the fall. A falling ball speeds up, so the drawings should be
close together at the top and far apart near the ground. If it looks
even:

- Redraw a pose: go to its frame and draw over it, or erase and redraw.
- Add a drawing between two others: put the playhead on the first and
  choose **Timing → Insert in-betweens**. The new frames come after the
  playhead and the later drawings move along. See [Timing](timing.md).

## Save and export

1. `Ctrl+S` saves. Symphony also autosaves every two minutes in case it
   closes unexpectedly.
2. Choose **File → Export → Animated GIF...**.
3. Click **Browse...** to choose where the file goes, check the **Export
   range** and **Resolution**, and click **Export**.

The export renders the same picture as the Camera view, through the node
graph. See [Import and export](import-export.md) for movies and image
sequences.

## Where to go next

- Ink and colour the ball: [Colour and paint](colour.md).
- Move a drawing without redrawing it, with keys and pegs:
  [Keys, pegs and the camera](animating.md).
- Plan the arc and the spacing first, then follow it:
  [Smart Charts](smart-charts.md).
