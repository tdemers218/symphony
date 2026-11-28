# Projects, saving and recovery

## What a project is

A Symphony project is a folder named `Name.symphony`:

```text
Name.symphony/
  project.json          the project's format version and its scenes
  scenes/main/scene.json
  .backups/             the last 5 saves of each scene
```

Keep the folder together: copy, move or back up the whole `.symphony`
folder. Open it with **File → Open...** or from **Recent projects** on
the welcome screen.

Projects record their format version. When the format changes, newer
Symphony versions upgrade older projects as they open them.

## Save

- **Save** (`Ctrl+S`) writes the project.
- **Save as...** writes a copy under a new name and keeps working in the
  copy.
- "(edited)" next to the scene name in the top bar means there are
  unsaved changes.

Saving never damages the previous save, even if the computer crashes
partway: Symphony writes a temporary file, flushes it to disk, then
replaces the old file in one step.

## Go back to an earlier save

Each save puts the previous version of the scene in the project's
`.backups/` folder, which keeps the last five. To go back to one, close
Symphony, then copy a backup over `scenes/main/scene.json` (keep a copy of
the current file first).

## Autosave and recovery

Every two minutes, if you have unsaved changes, Symphony autosaves them
to a recovery folder. It doesn't overwrite your project; only **Save**
does that.

If Symphony closes without you saving (a crash, a power cut), the next
time it opens it asks **Recover unsaved work?** **Recover** opens what it
autosaved; **Discard** throws it away.

## Undo

Undo (`Ctrl+Z`) and redo (`Ctrl+Shift+Z`) cover drawing, timing, node and
property edits. The **Edit** menu names what will be undone. **Edit → Undo
History** opens a list of this session's edits: click one to go back to
that point, and click a later one to go forward again. The history
starts fresh each time you open a scene.

## Where Symphony keeps its files

Outside your projects, Symphony uses its app data folder:

| System | Folder |
| --- | --- |
| macOS | `~/Library/Application Support/Symphony` |
| Windows | `%APPDATA%\Symphony` |
| Linux | `~/.local/share/Symphony` |

Inside it: `recovery/` (autosaves), `media/` (imported files waiting for
a project to be saved), and `codecs/` (OpenH264, if you downloaded it).
Your preferences, workspaces and shortcut changes are saved with the app's
settings.
