# Keyboard shortcuts

<!-- Written by crates/symphony-app/src/manual_shortcuts.rs from the keymap. Don't edit by hand: run
SYMPHONY_UPDATE_MANUAL=1 cargo test -p symphony-app manual_shortcuts -->

Symphony has two shortcut profiles. Choose one in **Edit → Preferences → Workflow and shortcuts**.

- **Symphony** (the default) uses plain letters, and the same letter can do different things depending on the view under the pointer: `D` is the Dropper over a drawing and Duplicate Drawing over the Timeline.
- **Harmony** follows the default shortcuts Toon Boom publishes for Harmony 27. Actions Harmony has no shortcut for are left free; find them with Search (`Tab`).

On a Mac, read `Ctrl` as `Cmd` and `Alt` as `Option`. `Cmd+Shift+Z` also redoes in both profiles.

The tables list the profiles' own shortcuts. To change one, right-click the action in Search (`Tab`) and press the new keys; Preferences shows your current set.

Symphony also keeps a few keys that every tool understands:

| Key | What it does |
| --- | --- |
| Hold `Space` | Pan the view with any tool |
| Hold `Shift+Space` | Rotate the view |
| `Enter` | Finish a point-by-point selection or an onion-skin alignment |
| `Ctrl+Enter` | Finish typing with the Text tool (`Escape` cancels) |
| `Escape` | Cancel a drag in the Node view or an onion-skin alignment; finish editing a Smart Chart |

## Anywhere

| Action | Symphony | Harmony |
| --- | --- | --- |
| New scene | `Ctrl+N` | `Ctrl+N` |
| Open | `Ctrl+O` | `Ctrl+O` |
| Save | `Ctrl+S` | `Ctrl+S` |
| Save as | – | – |
| Import… | – | – |
| Undo | `Ctrl+Z` | `Ctrl+Z` |
| Redo | `Ctrl+Shift+Z` | `Ctrl+Shift+Z` (the Control key, also on a Mac) |
| Previous frame | `,` | `,` |
| Next frame | `.` | `.` |
| First frame | `Shift+,` | `Shift+,` |
| Last frame | `Shift+.` | `Shift+.` |
| Play / pause | `Shift+Enter` | `Shift+Enter` |
| Create empty drawing | `Alt+Shift+R` | `Alt+Shift+R` |
| Reset view | `Shift+M` | `Shift+M` |
| Playback starts here | – | – |
| Playback ends here | – | – |
| Play the whole scene | – | – |
| Loop playback | – | – |
| Rotate view CW | – | – |
| Rotate view CCW | – | – |
| Reset zoom | – | – |
| Reset rotation | `Shift+X` | `Shift+X` |
| Reset pan | `Shift+N` | `Shift+N` |
| Onion skin: add previous drawing | – | – |
| Onion skin: remove previous drawing | – | – |
| Onion skin: add next drawing | – | – |
| Onion skin: remove next drawing | – | – |
| Onion skin: 1 previous | `!` | `!` |
| Onion skin: 2 previous | `Shift+2` | `Shift+2` |
| Onion skin: 3 previous | `Shift+3` | `Shift+3` |
| Onion skin: no previous | `` Shift+` `` | `` Shift+` `` |
| Onion skin: 1 next | `Ctrl+1` | `Ctrl+1` |
| Onion skin: 2 next | `Ctrl+2` | `Ctrl+2` |
| Onion skin: 3 next | `Ctrl+3` | `Ctrl+3` |
| Onion skin: no next | `` Ctrl+` `` | `` Ctrl+` `` |
| Maximise view | `Ctrl+F` | `Ctrl+F` |
| Search everything | `Tab` | `Tab` |
| Smart Chart: rotate and scale | – | – |
| Smart Chart: even spacing | – | – |
| Stop following Smart Chart | – | – |
| Show or hide Smart Charts | – | – |
| Colour picker | `Q` | – |

## Drawing, Camera and Node

| Action | Symphony | Harmony |
| --- | --- | --- |
| Zoom in | `1` | `2` |
| Zoom out | `2` | `1` |

## Drawing and Camera

The Smart Chart keys work only while a chart is being edited; see [Smart Charts](smart-charts.md#keys).

| Action | Symphony | Harmony |
| --- | --- | --- |
| Select all | `Ctrl+A` | `Ctrl+A` |
| Deselect all | `Escape` | `Escape` |
| Delete | `Delete` (Backspace on Mac) | `Delete` (Backspace on Mac) |
| Cut | `Ctrl+X` | `Ctrl+X` |
| Copy | `Ctrl+C` | `Ctrl+C` |
| Paste | `Ctrl+V` | `Ctrl+V` |
| Flip horizontal | `4` | `4` |
| Flip vertical | `5` | `5` |
| Increase brush size | `+` | – |
| Decrease brush size | `−` | – |
| Onion skin | `O` | `Alt+O` |
| Mirror view | `M` | – |
| Select | `S` | `Alt+S` |
| Transform | `X` | `Shift+T` |
| Brush | `B` | `Alt+B` |
| Line | `L` | – |
| Text | `T` | – |
| Pencil | – | `Alt+/` |
| Eraser | `E` | `Alt+E` |
| Paint | `P` | `Alt+I` |
| Paint Unpainted | `Y` | `Alt+Y` |
| Repaint | `R` | `Alt+R` |
| Unpaint | `U` | `Alt+U` |
| Close Gap | `G` | `Alt+C` |
| Show Strokes | `K` | `K` |
| Respect Protected Colours | `Shift+S` | `Shift+S` |
| Switch to Line Art/Colour Art | `Shift+L` | `L` |
| Switch to Overlay/Underlay Art | `:` | `:` |
| Preview All Art Layers | `V` | `P` |
| Create Colour Art from Line Art | – | `*` |
| Dropper | `D` | `Alt+D` |
| Line Editor | `N` | – |
| Hand | `H` | – |
| Zoom | `Z` | `Alt+Z` |
| Cutter | `C` | – |
| Show selected points | `I` | – |
| Smart Chart: key or breakdown | `Shift+B` | – |
| Smart Chart: hold until the next point | `Shift+H` | – |
| Move Smart Chart one frame earlier | `[` | – |
| Move Smart Chart one frame later | `]` | – |

## Camera

| Action | Symphony | Harmony |
| --- | --- | --- |
| Toggle camera effects | `Shift+R` | – |

## Timeline and Xsheet

| Action | Symphony | Harmony |
| --- | --- | --- |
| Increase exposure | `+` | `+` |
| Decrease exposure | `−` | `−` |
| Add drawing layer | `L` | `Ctrl+R` |
| Substitute Drawing Next | `]` | `]` |
| Substitute Drawing Previous | `[` | `[` |
| Go to Next Drawing | `G` | `G` |
| Go to Previous Drawing | `F` | `F` |
| Toggle auto-key | `A` | – |
| Hide or show the selected layer | `V` | – |
| Set Keyframe | `K` | `K` |
| Delete Keyframe | `Shift+K` | – |
| Insert New Drawing Frame | `N` | – |
| Duplicate Drawing | `D` | – |
| Copy Drawing | `Ctrl+C` | `Ctrl+C` |
| Paste Drawing | `Ctrl+V` | `Ctrl+V` |
| Clone Drawing | `C` | – |
| Insert In-between Frame | `I` | – |
| Delete Layer | – | – |

## Node view

| Action | Symphony | Harmony |
| --- | --- | --- |
| Arrange nodes | `A` | – |
| Open Node Properties | `P` | – |
| Render node preview | `V` | – |
| Reset Node view | `R` | – |

## Where Harmony-profile shortcuts work

In the Harmony profile, shortcuts work in every view, as they do in Harmony. These are the exceptions:

- Copy: Anywhere but the Timeline and Xsheet
- Paste: Anywhere but the Timeline and Xsheet
- Show Strokes: Anywhere but the Timeline
- Set Keyframe: Timeline
- Copy Drawing: Timeline and Xsheet
- Paste Drawing: Timeline and Xsheet
