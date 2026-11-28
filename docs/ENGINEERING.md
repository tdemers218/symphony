# Symphony engineering rules

How Symphony works inside, and the rules that keep it working as it grows.
Each rule has an ID (such as **E4.2**) so reviews and commits can cite it.
Like [DESIGN.md](DESIGN.md), these rules change only through a pull request
that says what changed and why.

§16 lists where today's code doesn't follow these rules yet. Every gap
there is a task, not an exception.

---

## E1. Architecture

### E1.1 Layers
Each crate may only depend on the layers below it:

```
symphony-app        UI: views, tools, menus, workspaces, profiles  (egui)
   │
symphony-io         load/save, import/export, autosave, recovery
symphony-render     drawing → pixels (GPU via wgpu, CPU fallback for tests)
symphony-vector     strokes, contours, fills, tessellation, hit-testing
symphony-pen        tablet input, one backend per OS
   │
symphony-core       document model, commands, undo, evaluation. Pure Rust.
```

- **E1.2** `symphony-core` has **no** UI, GPU, OS or file IO
  dependencies. Given the same document and frame, it always gives the
  same result, so it can be tested and scripted headless.
- **E1.3** Engine crates never depend on `symphony-app`. Anything the UI
  needs from the engine goes through a public API in the engine.
- **E1.4** New crates are created when a part has its own reason to change
  (a new file format, a new backend). New crates aren't created for size
  alone.

---

## E2. Document model

- **E2.1 The document** is a project containing scenes. Each scene holds:
  - an Xsheet (columns of exposures),
  - a node graph,
  - elements (sets of drawings),
  - palettes,
  - scene settings (resolution, frame rate, length).

  This mirrors Harmony's concepts so they map one-to-one.
- **E2.2 Stable IDs.** Every persistent object (column, node, element,
  drawing, palette, swatch, stroke) has a stable 64-bit ID that never
  changes or gets reused. **Names are labels, not identity.** Renaming
  never breaks a link.
- **E2.3 Strokes reference swatches by ID**, never by raw colour. Editing
  a swatch recolours every use (Harmony's colour-ID model).
- **E2.4 Frames are 1-based** everywhere a person sees them. Internal
  indexing may be 0-based, but the conversion happens in one place.
- **E2.5 Units.** Scene space is in scene units (the stage width equals
  the scene's horizontal resolution), y is down, and the origin is the
  stage centre. Conversions to screen happen only in views.
- **E2.6 Evaluation is pure.** Questions like "what's on screen at frame
  F?" are answered by `core` from the document alone, without UI state.
  Results may be cached, but caches are keyed by document version
  (E3.6), never invalidated by hand.

---

## E3. Commands and undo

- **E3.1 Every change to the document is a command**, an object that can
  apply and revert itself and has a user-facing name. Views and tools
  never mutate the document directly.
- **E3.2 One gesture is one undo step.** A whole stroke, a whole drag or a
  whole slider scrub is a single command. Commands from one gesture merge.
- **E3.3 Undo names** match the action's name everywhere (DESIGN D10.2),
  for example "Undo Brush stroke".
- **E3.4** The undo history is per scene and unlimited within a memory
  budget, with the oldest entries dropped first. It survives autosave
  recovery.
- **E3.5 Not part of the document, so not undoable:** selection, current
  frame, current tool, view zoom and pan, and workspace layout.
- **E3.6** Every committed command bumps the document version. Caches,
  autosave and "unsaved changes" all key off it.
- **E3.7** Scripting and plugins (later) work only through commands, so
  they're undoable and safe for free.

---

## E4. Input, tools and profiles

- **E4.1 Pen pipeline**, in this fixed order:
  raw samples (`symphony-pen`) → pressure curve (user setting) → tool.
  - Every sample is kept: nothing is coalesced or dropped.
  - Smoothing and stabilising belong to the *tool*, and settings for them
    show in Tool Properties.
- **E4.2 Device independence.** Tools see only `PenSample`, never anything
  specific to an OS or a tablet. The mouse produces `PenSample`s too.
- **E4.3 Actions registry.** Every user action has:
  - a stable ID (`"frame.next"`, `"edit.undo"`…),
  - a display name,
  - a menu location,
  - default bindings for **each profile** (Harmony, Symphony).

  Menus, shortcuts, the command palette and scripting all read the same
  registry. No shortcut is wired up anywhere else.
- **E4.4 Harmony profile defaults** come from the latest Harmony's
  published default shortcut list. Each binding notes where it came from.
  Unverified bindings are marked as such until checked.
- **E4.5 Symphony profile** starts as a copy of the Harmony bindings and
  changes only where we've found something better, and each change is
  logged in DESIGN §11.1.
- **E4.6 User overrides** are stored separately from profile defaults
  and survive switching profiles. Conflicting bindings are detected and
  shown, never silently resolved.
- **E4.7 Workflow differences** (not just shortcuts) are checked through
  one `Profile` value, never through scattered flags.

---

## E5. Threads and responsiveness

- **E5.1 The main thread** runs the UI, input and GPU submission only.
  It must never block for more than **4 ms**.
- **E5.2 Background work** (file IO, rendering an export, thumbnails,
  imports, vectorising, lip-sync analysis) runs on worker threads. Each
  job reports progress, can be cancelled, and sends its result back as a
  message, never by sharing mutable state.
  - **Exports lose nothing** (`symphony_render::checkpoint`). Each frame
    is kept on disk as soon as it's rendered: a PNG sequence's frames are
    its files, and a movie or GIF keeps PNGs in `name.mp4.partial/` and
    writes `name.mp4.part`, renamed only when whole. Every file is written
    under a temporary name and renamed when complete. A manifest holds a
    fingerprint of the scene, size and Symphony version. Exporting the
    same scene to the same place after a crash, a full disk or Cancel
    reuses every frame that reads back whole and renders the rest; a
    changed scene starts over. Tested with a real kill
    (`export_after_a_crash`) and in `an_interrupted_export_loses_nothing`.
- **E5.3 Snapshots.** Background readers get an immutable snapshot of the
  document (copy-on-write, `Arc`-shared), so the user can keep working
  while an export runs.
- **E5.4** A loading or progress indicator appears when work takes longer
  than 150 ms, and not before. DESIGN §17 sets what it shows at each stage.
- **E5.5 Every background job is visible.** Jobs (save, autosave, open,
  export, import) report a label, progress as done/total when countable,
  and a cancel flag. The app gathers them into the status bar's activity
  area (DESIGN D17.5), so new kinds of work get feedback for free.
- **E5.6 Startup work is deferred.** Anything slow at launch (scanning
  recovery files, loading large settings, warming caches) runs after the
  first frame, behind a loading screen if it blocks use (D17.7).

---

## E6. Performance budgets

Measured on a mid-range laptop at 60 Hz. Breaking a budget is a bug.

| What | Budget |
|---|---|
| Pen sample → ink on screen | ≤ 1 frame (16 ms) |
| Typical frame redraw (50 layers) | ≤ 8 ms |
| Playback | Hold the scene frame rate with 100 layers, with caching allowed |
| App start to usable | ≤ 2 s |
| Open a typical scene | ≤ 1 s, with a progress bar after 150 ms |
| Save | Never blocks the UI (snapshot, then write in the background) |

A CI benchmark tracks these once the renderer exists.

### E6.1 Rules that keep it smooth

Every feature follows these. A change that breaks one needs a written
reason in its commit.

- **The UI thread only paints.** Anything that can take over about 2 ms
  (stamping, tracing, decoding, rendering, file work) runs on a worker
  thread. The canvas shows a cheap stand-in until the result arrives:
  the last image, a low-resolution preview, or a faint plain line.
  Nothing waits on a result inside a frame.
- **Small synchronous budget.** A frame may do at most a few
  milliseconds of heavy work itself (`SYNC_BUDGET` in textured.rs), so
  small strokes appear at once. Everything past the budget queues.
- **Use every core for big jobs.** Heavy pixel work splits across cores
  with rayon:
  - one big stroke stamps in parallel horizontal bands;
  - a drawing's textured strokes stamp together;
  - the result must be byte-for-byte what one thread would make. A test
    checks it (`parallel_bands_match_one_pass_exactly`);
  - big outline merges (vector brushes, stroke outlines) merge in groups
    across the cores, then merge the groups (`shape::merge`; a union
    doesn't depend on order);
  - the eraser works out each stroke under it in parallel;
  - export renders a batch of frames at once and prepares them (GIF
    colours and compression, MP4 colour conversion) ahead of the writer.
  Spread work only when there is enough of it: one small job runs on
  the calling thread, since waking the other cores costs more.
- **Never rebuild geometry each frame.** Tessellated meshes, stamped
  images and traced outlines are cached by content key. A frame that
  re-tessellates a stroke it drew last frame is a bug. Stand-ins are
  cached too. Outlines that several parts need (canvas, eraser, export)
  live in one shared cache (`symphony_vector::recent`), so each is built
  once. The test `an_unchanged_frame_rebuilds_nothing` counts the work
  a redraw does (`Cache::work`, `VectorMeshes::built`) and fails if
  it's more than zero; add new kinds of cached work to those counters.
- **Cancel stale work.** A job whose stroke changed since it was queued
  is dropped, not finished.
- **Measure on real projects.** Synthetic scenes miss real problems. The
  first real scene showed one frame in three janking, while the
  generated heavy scene was smooth.

### E6.2 Measuring

Set these environment variables on a release build:

| Variable | Effect |
|---|---|
| `SYMPHONY_PERF=1` | Per-second timing of frame parts, GPU paint stages, and slow paints with their biggest mesh |
| `SYMPHONY_PERF_SECONDS=n` | Quit after n seconds and print CPU and between-frame percentiles and the jank count |
| `SYMPHONY_PERF_SPIN=1` | Redraw every frame |
| `SYMPHONY_PERF_OPEN=<project>` | Open a project on start |
| `SYMPHONY_PERF_SCENE=heavy` or `effects` | Start on a generated heavy scene |
| `SYMPHONY_PERF_VIEW=<tab>` | Bring a view to the front |
| `SYMPHONY_PERF_PLAY=1` | Start playing |
| `SYMPHONY_PERF_NAV=zoom` or `pan` | Keep navigating |

Example:

```
SYMPHONY_PERF_SECONDS=5 SYMPHONY_PERF_SPIN=1 SYMPHONY_PERF_OPEN=My.symphony ./target/release/symphony
```

"Between frames" is what smoothness feels like. Any frame over 16.7 ms
counts as a jank. It includes waiting for the GPU and the display, so
a busy GPU (training running on the same Mac) adds janks the app didn't
cause: compare against the previous commit on the same machine state.

**Timing checks.** `scripts/perf.sh` runs the release-build benchmarks
with `SYMPHONY_PERF_GATE=1`, and they fail past their budgets (about
twice today's time): eraser moves, the worst eraser move, and a long
vector-brush outline. Run it before merging work on drawing, erasing,
stamping or export. More benchmarks, not gated (they need a real
project in `SYMPHONY_PERF_OPEN`): `real_scene_stamp_cost` and
`real_scene_export_cost`.

---

## E7. Files

- **E7.1 Project format.** The project is a folder, `Name.symphony/`,
  shown as a single item on macOS.
  - `project.json`: manifest, format version, scenes list.
  - `scenes/<id>/scene.json`: Xsheet, graph and settings, human-readable.
  - `scenes/<id>/drawings/<id>.svd`: binary vector drawings.
  - `palettes/<id>.json`.
  - `media/`: imported sound, images and video.

  A *Pack* command makes a single `.symphonyz` zip for sharing.
- **E7.2 Format versioning.** Every file records `format_version`.
  - Symphony **reads every version it ever wrote**, through step-by-step
    migrations.
  - It writes only the newest version.
  - Opening an older file never changes it until the user saves.
- **E7.3 Atomic saves.** Each save writes to a temporary file, flushes it
  to disk (fsync), then renames it over the old one. A crash mid-save
  never corrupts the previous save.
- **E7.4 Backups.** The last 5 saves are kept in `.backups/` inside the
  project.
- **E7.5 Autosave and recovery.**
  - The app autosaves to a recovery folder every 2 minutes, and after
    every 50 commands.
  - On launch after a crash, it offers to recover.
  - Recovery data is deleted only after a successful normal save.
- **E7.6 Untrusted input.** Loaders treat every file as hostile. They
  never panic: bad data returns an error the user can read. Loaders are
  fuzz-tested.
- **E7.7 Harmony interop** (reading `.xstage`) is an *import*, never a
  native format, and follows [CLEAN-ROOM.md](CLEAN-ROOM.md).

---

## E8. Errors and crashes

- **E8.1 No panics on user data or user actions.** `unwrap`/`expect`
  are only for true programmer invariants, with a message explaining why
  the invariant holds.
- **E8.2** If a panic happens anyway, a panic hook:
  1. writes a recovery autosave,
  2. writes a crash report locally,
  3. tells the user on next launch, with the option to send the report.
  Sending is opt-in.
- **E8.3** Errors reach the user as plain messages (DESIGN D10.3), and the
  technical detail goes to the log.
- **E8.4 Logs** rotate in the app's data folder. *Help → Show logs* opens
  them.

---

## E9. Settings

- **E9.1** Settings live in the OS config folder as separate, versioned
  files:
  - `preferences.json`
  - `profile.json` (Harmony or Symphony, plus shortcut overrides)
  - `workspaces/`
  - `brushes/`
- **E9.2** Bad or unknown settings fall back to defaults with a log entry.
  They never crash the app and are never silently rewritten.
- **E9.3 Scenes don't depend on personal settings.** Opening a scene on
  another machine gives the same picture.

---

## E10. Rendering

- **E10.1** The renderer turns `core` evaluation results into pixels. The
  Camera view, Drawing view, Write node and thumbnails all use the same
  renderer (DESIGN D3.6), at different quality settings.
- **E10.2 The GPU path** uses wgpu. A CPU path (tiny-skia) renders the
  same output for tests and headless batch renders.
- **E10.3 Golden-image tests** compare renders with reference images,
  within a set tolerance.
- **E10.4 Colour.** Swatches are stored in sRGB. Blending and compositing
  are done in linear light. Export colour handling is explicit, never
  accidental.

---

## E11. Testing

- **E11.1** `core` logic is unit-tested. Every bug fix comes with a test
  that would have caught it.
- **E11.2 Pen logs as tests.** Recorded `pen-log.csv` files from real
  tablets are replayed through tools, and must produce the expected
  strokes.
- **E11.3 Both profiles are tested:** keymap completeness, no duplicate
  bindings, and every action reachable.
- **E11.4 CI** builds, tests and lints (`clippy -D warnings`) on macOS,
  Linux and Windows for every push and pull request.
- **E11.5 UI changes** come with before and after screenshots from
  `scripts/screenshot.sh` (every workspace at 1280×800 and 1024×640, plus
  Preferences and the loading screen), checked against DESIGN §15. Larger
  changes get a written review in `docs/reviews/`, using the
  *design-review* method: Must / Should / Could, plus what works.
- **E11.6 The manual changes with the feature.** A change people can see
  or use updates `docs/manual/` in the same commit: what it does, where it
  is, its keys. Write steps and facts, not praise ("Hold Shift to snap to
  22.5°", not "a powerful new snapping system"). Mark anything unbuilt
  **Planned**. The shortcuts page is generated from the keymap
  (`SYMPHONY_UPDATE_MANUAL=1 cargo test -p symphony-app manual_shortcuts`),
  and `cargo test -p symphony-manual` fails on broken links between pages.
- **E12.1** Stable Rust. `cargo fmt`. `clippy -D warnings` clean.
- **E12.2 `unsafe` is only allowed in platform backends** (pen, windowing
  glue), and every block has a `// SAFETY:` comment.
- **E12.3 Dependencies** must have GPL-3-compatible licences (MIT, Apache-2.0,
  BSD, Zlib, ISC, MPL-2.0; OFL for fonts), be actively maintained, and be
  worth their weight. Each new dependency is justified in its commit.
- **E12.4 Each module states its job** in a top-level `//!` comment.
  Comments explain *why*. The code shows *what*.
- **E12.5 Commits** have a short subject plus a body saying why. Each
  commit builds and passes tests on its own.

---

## E13. Releases, updates and the launcher

- **E13.1 Versions** are `MAJOR.MINOR.PATCH`.
  - `0.x` while in alpha.
  - Breaking the file format is forbidden at every version, because
    migrations handle old files (E7.2).
- **E13.2 Channels.**
  - **Stable**: tagged releases.
  - **Preview**: tagged pre-releases for testers.
  - Both are published as GitHub Releases on the public repo.
- **E13.3 Artifacts per release:** an archive of the app for each OS and
  CPU, a `SHA256SUMS` file, and release notes written for artists
  (plain words, what changed, what to test).
- **E13.4 The Symphony Launcher** is the one thing testers install.
  1. It checks GitHub Releases for the newest version on the chosen
     channel.
  2. It downloads that version in the background and **checks its
     SHA-256 checksum** before installing.
  3. Versions install side by side in the launcher's data folder. The
     previous version is kept so users can **roll back** with one click.
  4. It never updates a version that's currently running.
  5. It shows release notes before updating.
  6. **Launch** starts the selected version.
  7. It works offline with whatever is already installed.
- **E13.5 The launcher** updates itself through the same mechanism, but
  only when no Symphony version is running.
- **E13.6 Code signing.** Unsigned builds trigger OS warnings: macOS
  Gatekeeper and Windows SmartScreen. Until we sign builds (Apple Developer ID plus
  notarisation, and a Windows code-signing certificate), the install
  guide shows testers the one-time "Open anyway" step. Signing is required
  before any public stable release.
- **E13.7 No telemetry.** The launcher only contacts GitHub to check for
  releases and download them. Crash reports are opt-in (E8.2).

---

## E14. Scripting and plugins (later)
- **E14.1** One scripting API wraps the commands (E3.7) and the actions
  registry (E4.3). It stays as close to Harmony's scripting objects as
  the clean-room rules allow, so pipeline scripts can be ported.
- **E14.2** Plugins run behind a stable, versioned interface and can
  never corrupt the document outside commands.

---

## E15. Clean room and target version
Every feature follows [CLEAN-ROOM.md](CLEAN-ROOM.md). Behaviour is taken
from the **latest** Harmony release's public docs (currently 27.1).

---

## E17. AI

Policy: [AI-POLICY.md](AI-POLICY.md). Design rules: DESIGN §16.

- **E17.1 Forbidden:** generative models and LLMs, in any crate, build,
  feature flag or bundled plugin. Pull requests that add one are closed.
- **E17.2 Assisted tools are declared.** Every action that runs a trained
  model is marked `assisted` in the actions registry (E4.3). The registry
  refuses to run an assisted action while **Use no AI** is on. No other
  code path may call a model.
- **E17.3 Local only.** Models run on the device. Assisted tools make no
  network calls, and models never train or fine-tune on user content.
- **E17.4 Provenance.** Running an assisted action writes an entry to the
  scene's `provenance` record (tool, model name and version, date). It's
  saved in the scene file (E7) and survives copy, import and export. A
  scene with no entries reports **No AI used**. Provenance can't be edited
  by hand in the UI, and scripting can only add entries, never remove them.
- **E17.5 Model dependencies** are listed with their licence and training
  data source in AI-POLICY.md before they can be merged (alongside E12.3).
- **E17.6 What counts:** anything whose behaviour comes from a trained
  statistical model (neural nets and other ML). Fixed-rule algorithms
  don't count and must not be labelled as AI (DESIGN D16.5).

---

## E16. Where the code doesn't follow the rules yet

These are known gaps from early prototyping, and each one is a task.

| Rule | Gap today | Fix in |
|---|---|---|
| E7.1 | Drawings are stored inside `scene.json`, not as separate binary `drawings/<id>.svd` files. | A format migration once scenes get large |
| E5.3 | Background saves clone the whole document instead of sharing copy-on-write data. | When scenes get large |
| E10.2 | The canvas draws tessellated meshes through egui, not a shared GPU renderer. Strokes are re-tessellated every frame, with no cache. | Renderer milestone |
| E4.3 | Panels still edit a few UI settings directly (brush, onion skin). They're not document data, so that's allowed (E3.5). | n/a |
| E17.2, E17.4 | The "Use no AI" setting and provenance record aren't built yet. There are no assisted tools, so nothing is affected. | With the actions registry (M2 polish) and scene files (M12) |
| E13.6 | Builds aren't code-signed. Testers follow the "Open anyway" steps in TESTERS.md. | Before the first stable release |
| E13.4 | The macOS app is launched as a plain binary, not a `.app` bundle (it gets no Dock icon of its own). | With app icons |
