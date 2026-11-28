# Remote Workspace: plan

Phones, tablets and other computers as extra panels for Symphony, through
a web browser on the local network: no app, no account, no cloud. This
plan comes from Tristan's brief (2026-11-28) and an audit of the code at
0e30d15.

## In short

- **Reuse, don't rebuild.** Search (`search_entries` → `search::rank` →
  `run_command`) is already a command registry. A phone search box can
  call it as it is, and every command is reachable from the phone on day
  one. Panels send the same commands and session changes the desktop
  makes.
- **Small server, no async runtime.** Symphony has no tokio today.
  Threads with `tiny_http` (pages) and `tungstenite` (WebSocket) are
  enough for a handful of devices and keep the build light. Axum would
  bring in tokio and hyper to serve a few phones.
- **The UI thread stays in charge.** Network threads never touch the
  scene. Commands arrive on a channel and the UI thread applies them at
  the start of a frame, like file loads do now. State goes out only when
  it changed and only while a device is connected. With the feature off,
  nothing runs: no thread, no socket, one `Option` check per frame.
- **Encrypted from the QR code.** There's no practical HTTPS on a home
  network without a domain or certificate warnings. Instead, the QR code
  carries a secret key in the URL's `#` part, which browsers never send
  over the network. Every message after the page loads is sealed with it
  (NaCl secretbox). Limits are in *Security* below.
- **Draw on a tablet.** An iPad with Apple Pencil or a Galaxy Tab with
  S Pen becomes a drawing surface: the Drawing view on the tablet, with
  pressure and tilt, going through Symphony's own stroke pipeline. See
  *Tablet drawing*.
- **First milestone:** "Connect device" in search → QR code → the phone
  shows a brush-size slider that stays in sync both ways. Tested on
  Tristan's iPhone and an Android phone before anything else is built.

## What the audit found

| Need | What exists | Use |
|---|---|---|
| Command registry | `search::Entry { title, keywords, context, command, key }`, built by `SymphonyApp::search_entries()`; `search::rank(entries, query, here, uses)`; `SymphonyApp::run_command(ctx, Command)` | Remote search ranks on the desktop and returns titles and keys. The phone runs an entry by its stable `key`, which is looked up again on the desktop and never sent as a `Command`. |
| Panels | `workspace::View` (13 views) in `egui_dock` layouts, kept per workspace | Move/Mirror means removing or keeping a `View` tab; the dock remembers where it was. |
| Brushes | `Session.brush / pencil / eraser: BrushSettings` (`min_width`, `max_width`, `smoothing`, …), presets through `Command::UsePreset { pencil, name }` | The Brushes panel reads `BrushSettings` and the preset list, and selects with `UsePreset`. |
| Colours | `doc.palette.swatches`, `Session.swatch`, `views/colour_picker.rs` | The Colours panel lists swatches and selects one. Colour edits go through the same palette edits (undoable). |
| Playback | `Session.frame`, `playing`, `play_range`, `loop_playback` | Playback panel. |
| Undo | `History::apply_merged(scene, name, key, edit)` merges a gesture into one step; `end_gesture()` seals it | Remote slider drags carry a gesture key and an end. A missed end seals itself after 0.5 s. |
| Background work | `std::thread::spawn` + `mpsc` + `ctx.request_repaint()` (recovery scan, updates, import) | The same pattern for the server. |
| Settings | eframe storage (Preferences) | Trusted devices and their layouts. |
| Rules | E6.1 (UI thread only paints, nothing rebuilt per frame), E13.7 (no telemetry), AI policy | The server only listens; it never calls out. Voice goes through the phone keyboard's own dictation, so Symphony never handles audio. |

## How it works

```
Phone browser ──HTTP──▶ tiny_http thread ──▶ the bundled page (one HTML file, include_str!)
Phone browser ◀─WebSocket─▶ one thread per device (max 8)
                     │ sealed messages, checked, numbered
                     ▼
               mpsc::Sender<RemoteIn> ──▶ UI thread, start of frame: apply (≤ 64 per frame)
               UI thread, end of frame: if a device subscribes and the state changed
                     └──▶ RemoteOut (only the changed panel's state) ──▶ device threads
```

- **Messages** are JSON inside a secretbox: `{seq, kind, …}`.
  - The phone sends `hello`, `subscribe(panel)`, `set(prop, value, gesture?)`, `end(gesture)`, `run(entry key)`, `search(query)` and `pick(panel action)`.
  - The desktop sends `snapshot(panel, state)`, `patch(panel, changes)`, `results(query, hits)`, `notice` and `scene(key)`.
- **Duplicates:** each device's `seq` only goes up; the desktop drops anything at or below the last one it saw. After a dropped connection the phone discards everything it hadn't sent and asks for fresh snapshots. Nothing destructive is ever replayed.
- **Project switching:** messages carry the scene `key`. A change makes the phone reload its panels, and commands addressed to the old scene are dropped.
- **Panel registry** (`remote/panels.rs`): each panel has a stable id, a title, an icon name, and three functions:
  - `state(&Session) -> json`, for the subscription;
  - `fingerprint(&Session) -> u64`, a cheap test for "did it change";
  - `apply(&mut Session, action)`.

  Simple property panels describe their controls as data (slider, toggle, choice) and the phone draws them generically. Complex panels (node editor, timeline) get their own phone code later. None of this touches the desktop panels.

## Pairing and devices

1. Search "Connect device", or open Preferences > Devices. Symphony starts the server on the computer's local address and a fixed port (kept between runs, so trusted devices find it again).
2. The window shows a QR code for `http://192.168.1.20:7313/#p=<one-time code>&k=<key>`, plus the same link to type or copy. The code is good for 2 minutes and one device.
3. The phone camera opens it. The page pairs and stores its own device secret on the phone, then asks which panels to show.
4. The desktop lists the device ("iPhone", which can be renamed), its status and its panels, with **Forget** (which revokes it) and **Stop server**.

Reconnecting: a trusted device's page reconnects by itself while the server runs. Browsers keep a page's storage per address, though. If the computer's local address changes (a new router, another network), the phone has a different address and has to scan again. Symphony says so in the devices window instead of failing silently. A `.local` name would avoid this, but Android support for those names is uneven, so it's a later improvement.

The server starts only when you ask for it. "Let trusted devices reconnect when Symphony opens" is a setting, off by default.

## Security

- **Explicit start.** The server binds only to the local network address, never `0.0.0.0` on all interfaces, and never sets up port forwarding.
- **Pairing codes** are 128-bit random, single use, and expire after 2 minutes. More than 10 bad attempts a minute close pairing until a new code is shown.
- **Device secrets** are random and stored hashed on the desktop. Forgetting a device closes its socket at once.
- **WebSocket checks:** the `Origin` must be the page Symphony served, and the first message must be a valid sealed `hello`, or the socket closes. Limits:
  - 8 sockets;
  - 64 KB per message;
  - 200 messages a second per device, with slider moves merged first.
- **Every command is checked on the desktop.** Values are clamped, ids must exist, and only search entries and panel actions can run. There's no file access, no shell and no add-ons from a phone.
- **Destructive or unclear commands** ask for confirmation on the phone. Search entries get a `confirm` flag (delete, clear, revert…).
- **What the encryption covers.** Every message after load is secret and tamper-proof against anyone who never saw the QR code. What it doesn't cover: the page itself (HTML and JavaScript) loads over plain HTTP. Someone already on your network and actively attacking at the moment you open it could swap the page. Proper HTTPS on a home network means either a certificate warning on every phone or a public domain, which needs the internet. Symphony will say this in one line in the devices window. A "local certificate" option for advanced users can come later.
- **Why not encrypt with the browser's own tools:** `crypto.subtle` only exists on HTTPS pages. The page therefore bundles TweetNaCl-js (public domain, about 7 KB), and the desktop uses the matching Rust crate (`crypto_secretbox`).

## Voice

Search on the phone is an ordinary text field. The phone keyboard's microphone button (iOS and Android dictation) types into it, and the words go to Symphony's normal search. There's no separate command language, and no microphone permission, which a plain HTTP page couldn't get anyway.

The phone's own operating system does the speech recognition. Symphony never receives audio, so this stays inside the AI policy; the devices window says so. Values in a request ("opacity to forty percent") are a search feature to add later, and they'll work the same from the desktop.

## Tablet drawing

Tristan's addition: the Drawing view works on tablets with Apple Pencil
and Samsung S Pen, so people can draw in Symphony on their tablet.

- **Pen input.** Browsers report pens through Pointer Events: `pointerType`
  is `"pen"`, with `pressure`, `tiltX` and `tiltY`, and the extra
  in-between samples (`getCoalescedEvents`) where the browser offers them.
  The page turns each sample into the fields of `symphony_pen::PenSample`
  (position, pressure, tilt, phase, time).
  - On the desktop they go through the same stroke pipeline as a Wacom or
    Veikk: pressure curve, smoothing, stabilization, the active brush.
    The tablet is then simply one more pen backend, so the "one sample
    format for every tablet" design in `symphony-pen` already covers it.
  - Which browsers report which fields is to be checked on real devices
    and recorded in `docs/TABLETS.md`, not assumed.
- **Pen draws, fingers navigate.** Pen strokes draw. One finger does
  nothing by default, which avoids marks from a resting palm. Two fingers
  pan, pinch zooms and twist rotates the view. Two-finger tap undoes and
  three-finger tap redoes, as in tablet drawing apps (both can be changed).
  The page turns off the browser's own scrolling, zoom, text selection and
  long-press menus on the canvas.
- **No waiting for the network.** The tablet draws the stroke under the
  pen tip itself, at once, as a plain preview. Samples go to the desktop
  in small batches, about every 16 ms. When the desktop finishes the
  stroke with the real brush, the finished stroke replaces the preview.
  Wi-Fi delay then never shows under the pen.
- **What the tablet shows.** No video. The tablet receives the current
  drawing's strokes as data (vector lines and fills, with the light table
  and onion skins when on) and draws them itself. After that it gets only
  the strokes that changed. Bitmap layers come as image tiles, only for
  what is on screen.
  - The tablet's look is a close preview: textured brushes are simplified
    at first.
  - Building Symphony's own renderer for the browser (WebAssembly) would
    make it exact; that's a later option, chosen once the simple version
    has been tried.
- **Tools on the tablet.** A compact toolbar: brush, pencil, eraser, the
  colour, size, undo/redo and frame stepping. Everything else goes
  through search. The tablet can show the Drawing panel next to Brushes
  and Colours, or on its own, full screen.
- **Speed checks:**
  - pen-to-ink delay on the tablet;
  - samples per second that reach the desktop;
  - the desktop's frame time while a tablet draws (`scripts/perf.sh`).
- **Tests:**
  - samples become correct `PenSample`s (pressure, tilt, phase);
  - a stroke drawn from a simulated tablet equals the same stroke drawn
    locally;
  - a dropped connection mid-stroke never leaves a half-stroke or a stuck
    pen;
  - undo/redo from the tablet;
  - then Tristan on an iPad with Apple Pencil and a Samsung tablet with
    S Pen.

## Milestones

1. **Connection prototype.**
   - `remote/` module: server, pairing, sealing, devices.
   - "Connect device" in search; Preferences > Devices with the QR code and the device list.
   - The bundled page, with one synced brush-size slider.
   - Tests:
     - pairing and expiry;
     - a wrong code, a wrong origin or a bad seal is rejected;
     - forgetting a device;
     - duplicate `seq`;
     - two devices;
     - server shutdown frees the port and its threads.
   - **Then Tristan tries it on iPhone Safari and Android Chrome, and with the Wi-Fi offline, before Milestone 2.**
2. **Useful.** Brushes (presets with small previews, made on a worker and cached by preset), Colours (palette, picker, current colour), Search (with confirmation), Playback (play, step, scrub, range). Reconnection and resync, undo grouping for slider drags, timings in `scripts/perf.sh` with two devices connected.
3. **Flexible.** Panels assigned per device, Mirror / Move from a panel's menu and from search, a "bring back" notice when a device leaves, layouts remembered per device, tablet layouts, tool properties that follow the active tool.
4. **Tablet drawing.** The Drawing view on tablets with Apple Pencil and S Pen, as above. It comes after Milestone 2 because it needs the connection, sync and undo work done there.
5. **Later.** Push-to-talk, custom control surfaces (dials, XY pads, macros bound to commands and properties, changing with context: brush size while drawing, chart curvature in Smart Charts), node editor, Xsheet, Smart Chart controls, layers.

## New dependencies (Milestone 1)

All are small, pure Rust and GPL-compatible:

- `tiny_http` (MIT/Apache)
- `tungstenite` (MIT/Apache)
- `qrcode` (MIT/Apache; drawn as an egui texture)
- `crypto_secretbox` (MIT/Apache)

`getrandom` is already in the tree. The local address comes from the standard library (the address of a UDP socket "connected" to a private test address; nothing is sent).
