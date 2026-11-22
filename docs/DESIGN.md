# Symphony design rules

These rules govern how Symphony looks, reads and behaves, for its whole
life. Every UI change is checked against them (see §15). Each rule has an
ID, such as **D3.2**, so reviews and commits can cite it.

The rules can change, but only on purpose: through a pull request to this
file that says what changed and why. Code doesn't get to drift away from
them quietly.

Software rules (data, undo, threading, files, releases) live in
[ENGINEERING.md](ENGINEERING.md). The two documents work together.

---

## 1. Who we design for

Professional and student 2D animators, most of them coming from Toon Boom
Harmony. They spend 8+ hours a day in the app, with a pen in one hand and
the other hand on the keyboard. They judge colour on the canvas all day.
They already have years of Harmony muscle memory.

## 2. Principles (in priority order)

When two rules conflict, the higher principle wins.

- **P1. Never lose work, never block the pen.** Crashes, freezes and lag
  while drawing are the worst failures. Nothing gets in front of a stroke.
- **P2. The artwork comes first.** The UI is a calm, neutral frame around
  the canvas. It never competes with the drawing for attention or tints
  how its colours look.
- **P3. Harmony muscle memory is respected.** In the Harmony profile,
  views, names, places, shortcuts and workflows match the latest Harmony.
  Someone switching over should be productive on day one.
- **P4. Improvements are a choice.** Better ways of working go into the
  Symphony profile (§11). They never silently replace Harmony behaviour.
- **P5. Calm, dense, legible.** This is a professional tool: compact,
  quiet and readable for hours. Colour and motion only appear when they
  mean something.
- **P6. Symphony looks like Symphony.** Harmony's structure, our own
  identity: our own icons, colours and type. We never copy Toon Boom
  artwork (see [CLEAN-ROOM.md](CLEAN-ROOM.md)).

- **P7. The art is the artist's.** This one is absolute: it never trades off
  against the others. No generative AI and no LLMs, ever.
  Assisted tools are rare, local, labelled and opt-in, and one switch turns
  them all off (§16, [AI-POLICY.md](AI-POLICY.md)).
---

## 3. Window and view anatomy

**D3.1** The main window keeps Harmony's *structure* with less chrome:
- **One top bar** (40 px). Menus on the left, the scene name in the
  centre, and the workspace switcher on the right. On macOS it *is* the
  title bar: content extends under it, the window buttons sit at its left,
  and dragging its empty space moves the window. Harmony stacks several
  toolbars here; Symphony folds them into this one bar and into view
  headers (D3.2).
- **The Tools toolbar**, docked to the left as its own card.
- **The dock area**, which holds the views.
- **A status bar** (24 px) along the bottom: frame, layer, tool and modes
  on the left; pen and profile on the right.

**D3.2** Every view has the same anatomy:
1. **Tab.** It shows the view's name. The active tab in each card sits on
   `bg.raised` with primary text, and inactive tabs show secondary text on
   the card. The tab of the *focused* view gets a 1 px cel-blue outline, so
   you always know where keystrokes go.
2. **View toolbar** (optional). One row of view-specific controls, always
   at the top of the view.
3. **Content.**

Nothing floats over content except transient feedback, such as the canvas
frame label or the eraser cursor.

**D3.3** View names and menu names match Harmony's (Camera, Drawing,
Timeline, Xsheet, Node, Tool Properties, Colour, Layer Properties,
Function, Library…), so tutorials and docs transfer over.

**D3.4** Views can be docked, tabbed, split, floated and maximised.
Tabs have **no close or collapse buttons**. Those actions live in the
tab's right-click menu and the Windows menu, so the chrome stays quiet.
Maximising the view under the focus is one keystroke and its own undo:
press it again to restore.

**D3.5** Workspaces are saved layouts. Layout changes auto-save into the
active workspace, and *Reset* returns it to its preset. The shipped
workspaces follow Harmony's.

**D3.5a** Camera view toolbars follow Harmony: a top toolbar for display
options (onion skin: show it, and add or remove earlier and later drawings),
and a bottom toolbar for navigation (zoom level, Reset Zoom, Reset Rotation,
Reset Pan, Reset View). Rotate View turns 30° like an animation table.

**D3.6** One canvas truth: the Camera and Drawing views use the same
renderer. They differ only in what they show (the composited scene, or the
selected drawing untransformed).

---

### 3.1 Surfaces (elevation by tone)
Symphony separates areas by **tone, not lines**:

| Level | Token | What sits there |
|---|---|---|
| 0 | `bg.base` | The window, the top bar, the status bar, and the gutters between cards |
| 1 | `bg.panel` | Cards: every docked view, and the Tools toolbar |
| 2 | `bg.raised` | Controls inside cards, rows, view headers |
| 3 | `bg.hover` | Hover, and popups or menus (plus a shadow) |

- **D3.7** Each docked view is a **card**: `bg.panel`, 8 px corner radius,
  separated from its neighbours by a 6 px `bg.base` gutter. Tabs and content share one continuous surface.
  Use the same gap beside the tool strip and between stacked inspectors.
  Reserve lines for timing grids, control edges, and interaction feedback.
- **D3.8** A line is used only where tone can't separate, such as grid
  lines in the Xsheet or the divider under a tab row. Never a box around a
  box.

### 3.2 Signature details
Spend boldness in one place (P5). Symphony's signature is **time**:
- **D3.9** The playhead is an amber line with an amber **frame tag** on the
  ruler, showing the current frame number. It's the same in every time
  view (Timeline, Xsheet, Function).
- **D3.10** The stage is **paper on a light table**: white, with a soft
  neutral shadow, on the grey surround. This is the one docked thing that
  casts a shadow (an exception to D6.4).

## 4. Colour

### 4.1 Tokens
The UI uses **only** these tokens. A colour literal in UI code is a bug.
Tokens live in `crates/symphony-ui/src/theme.rs`.

| Token | Hex | Use |
|---|---|---|
| `bg.base` | `#17181A` | Window background, gaps between docks |
| `bg.panel` | `#242628` | View content background |
| `bg.raised` | `#303336` | Toolbars, tab bars, headers, buttons at rest |
| `bg.hover` | `#3C4044` | Hovered buttons and rows |
| `bg.input` | `#1D1F21` | Text fields, slider rails, wells |
| `line.subtle` | `#393C40` | Dividers and grid lines |
| `line.strong` | `#595F66` | Outlines of containers |
| `line.control` | `#949CA4` | Edges of checkboxes and radios when off |
| `text.primary` | `#E8EAEC` | Labels, values, icons |
| `text.secondary` | `#B5BAC0` | Hints, units, secondary info. Never on `bg.hover`. |
| `text.disabled` | `#747980` | Disabled controls only, never information |
| `accent.select` | `#81B4F8` | "Cel blue": selection outlines, active tool, focused tab, focus ring |
| `accent.select.fill` | `#3D63B8` | Fill behind selected items (white text on it is 5.7:1) |
| `accent.time` | `#F0A63A` | "Light-table amber": playhead, current frame, keyframes, animate mode |
| `state.danger` | `#E86A62` | Eraser, destructive actions, errors. Icons and markers only, not body text. |
| `state.ok` | `#62C484` | Confirmed good states (tablet connected). Rare. |
| `canvas.surround` | `#464749` | Area around the stage. A neutral mid grey. |
| `canvas.stage` | `#D4D5D7` | The empty stage: light neutral grey, not white (Tristan). Exports still render on white. |

### Workspace glass

The wallpaper sits behind the editor chrome. Black is the default, with
Plain, Studio, Dusk and a custom PNG or JPEG available under Preferences → Interface.
Each docked panel and the tool strip have one 12 px outer contour, one
graphite fill and a 1 px inner rim. The top highlight follows that rim.
Inset corners use the outer radius minus the inset, so nested contours
share a centre. Rectangular view content sits inside the outside corners. The menu and status
bars use a slightly more transparent tint. The Camera and Drawing surfaces,
including the stage, stay opaque so the wallpaper cannot tint artwork.

Custom images are decoded, downsized, softened and darkened off the UI
thread. The editor uploads the result once and stretches the cached texture
when resized. Plain is shown while the picture loads or if reading fails.
Built-in coloured wallpapers use a single shaded mesh. Use solid surfaces
for controls that require reliable contrast over any wallpaper.

### 4.2 Rules

Use neutral graphite surfaces around artwork to limit perceived colour casts.
Blue identifies selection, keyboard focus and primary actions. Standard
controls use neutral fills through hover and press. Amber identifies time
and animation state.
- **D4.1 Colour carries meaning, never decoration.** Blue means
  *selected/active*. Amber means *time* (now, keys, animate mode). Red
  means *destructive*. Nothing else is coloured.
- **D4.2 The canvas stays neutral.** Around the stage there is only neutral
  grey (`canvas.surround`), with no saturated colour except selection and
  time indicators drawn *on* the artwork, so colour judgement stays
  accurate.
- **D4.3 Artwork colours never appear in the UI** except as swatches.
- **D4.4 Contrast.** Text needs at least 4.5:1 against its background,
  and essential UI parts (icons, focus rings, control edges) at least 3:1.
  The values in §4.1 were measured. Any new pairing has to be measured
  before it's used.
- **D4.5 Never colour alone.** Every coloured meaning also has a shape or
  text. Keyframes are diamonds as well as amber. Errors have text as well
  as red.
- **D4.6 Themes are token swaps.** A future light or high-contrast theme
  only changes token values, never layouts or code.

### 4.3 Conventions on the canvas
- **Onion skin**: previous drawings and next drawings get two distinct
  tints, which the user can change. The defaults should match Harmony's
  convention. *(verify against Harmony 27)*
- **Selection on the canvas**: cel blue bounding box and handles.
- **Pivot**: amber, because it's tied to animation.

---

## 5. Typography

- **D5.1 One UI family: Inter** (OFL licence, bundled). Numbers in
  time-based grids (Xsheet frame numbers, Timeline ruler) use
  **JetBrains Mono** (OFL) so the digits line up. Mono is used for
  nothing else.
- **D5.2 Scale.**
  - 11 px: dense labels (ruler, cell text).
  - 13 px: body and default.
  - 15 px medium weight: view headings.
  - Nothing larger inside the app.
- **D5.3 Sentence case everywhere**, so "Tool properties" rather than
  "Tool Properties". The exception is **proper view and feature names
  that match Harmony** (D3.3), which keep Harmony's capitalisation so
  people can search for them. No ALL CAPS labels. No bold or colour to
  pick out a single word.
- **D5.4 Spelling:** Canadian/British, the same as Harmony ("Colour").
  Keep it consistent across UI and docs.
- **D5.5 Text that doesn't fit ends in an ellipsis, and hovering it shows
  the full text** (after the normal tooltip delay). Never let text run
  past its box or overlap other text. Use `widgets::fitted_text` for
  painted text and `Label::truncate()` for labels.

---

## 6. Space, size and shape

- **D6.1 4 px grid.** Every size and gap is a multiple of 4 (with 2 px
  allowed for hairline insets).
- **D6.2 Standard sizes.**
  - Top bar: 40 px. View header (tab row): 32 px. Status bar: 24 px.
  - Toolbar button: 24 px with a 16 px icon.
  - **Tool button (Tools toolbar): 32 px with a 20 px icon**, since it's
    picked with the pen all day. Tool groups are separated by 12 px.
  - Tools toolbar: 44 px wide.
  - List and Timeline row: 28 px. Xsheet row: 20 px.
  - Timeline frame cell: 16 px wide at default zoom.
  - Minimum pen target: 24 px.
  - Card inner padding: 12 px. Gutter between cards: 6 px.
- **D6.3 Corner radius.**
  - Controls, rows and exposure blocks: 4 px. Tool-strip buttons: 6 px.
  - Docked glass panels and the Tools toolbar: 12 px.
  - Popups and menus: 8 px. Floating windows: 12 px.
  - The stage: 0, because paper has square corners.
- **D6.4 Shadows** only on things that float above the dock (menus,
  popups, floating windows), and on the stage (D3.10). Cards are flat:
  their gutter separates them.
- **D6.5 Density over air.** We prefer showing more over padding. But
  related controls group together, and groups are separated by 8 px or
  a `line.subtle` divider, never both.
- **D6.6 UI scale** is a preference from 75% to 200%, and everything
  scales together.

- **D6.7 Shared controls.** Use `widgets::primary_button` for the main
  action in a dialog, such as Export. Use the standard button for secondary
  actions. Use `widgets::checkbox`, `widgets::radio_value` and
  `icons::button` for their respective controls so keyboard focus, disabled
  states and accessibility labels are consistent. Review new controls in the
  running app at 1280×800 and 1024×640, including a keyboard pass.

---

## 7. Icons

- **D7.1 Our own icon set**, drawn in code
  (`crates/symphony-ui/src/icons.rs`) so it stays sharp at any scale.
  **No emoji, and no Unicode symbols used as icons.** No third-party
  icon fonts mixed in.
- **D7.2 Drawing grid.**
  - 16×16 units, with 2 units of padding.
  - Strokes are 1.5 units with round caps and joins. Check the 16 px toolbar glyph beside its 20 px tool-strip version. Keep enclosed spaces open at the smaller size.
  - Compare related actions side by side. Directional pairs share geometry, and each drawing tool has a recognizable working tip.
  - Outline style. Solid fill only where it *means* something: play,
    record, a set keyframe, an active state.
- **D7.3 Colour.**
  - At rest: `text.primary`.
  - Active or toggled on: white on `accent.select.fill`.
  - Disabled: `text.disabled`.
  - Destructive tools may tint red only while in use.
- **D7.4 One icon, one meaning.** If two actions share an icon, one of
  them is wrong. The same action uses the same icon everywhere.
- **D7.5 Every icon button has a tooltip:** "Name (shortcut)", plus a
  short line of what it does if the name isn't obvious.
- **D7.6 Metaphors from animation.** Prefer animation objects (pegs,
  light table, exposure sheet, pencil) over generic software symbols.
- **D7.7 New icons** are added to the icon sheet (debug view, *Help →
  Icon sheet*) and reviewed next to the existing ones before merging.

---

## 8. Interaction

- **D8.1 Pen first.** Every canvas action works with the pen alone. There
  is no hover-only information, because a pen hovering above a screen
  tablet isn't reliable.
- **D8.2 Instant feedback.** Ink appears within the same frame as the pen
  sample. Any UI response takes less than 100 ms. Anything slower runs in
  the background with visible progress and a Cancel button
  ([ENGINEERING.md](ENGINEERING.md) E5).
- **D8.3 No blocking dialogs** while drawing or playing back. Dialogs are
  for setup, not for work.
- **D8.4 Undo, don't ask.** Destructive actions are undoable and never ask
  "Are you sure?". Confirmation is only for things that *can't* be undone,
  such as deleting files on disk.
- **D8.5 Visible modes.** Any mode that changes what an action does
  (Animate mode, onion skin, a locked layer) shows on screen all the time
  while it's on, in its token colour.
- **D8.6 Escape cancels** the current gesture or popup, and never does
  anything destructive.
- **D8.7 Discoverable.** Every action is in a menu, and the menu shows its
  shortcut. Shortcuts are speed-ups, never the only way.
- **D8.8 Scrubbable numbers.** Every numeric field can be dragged to
  change it, typed into, and reset (double-click or a reset button).
- **D8.9 Selection is consistent.**
  - Click selects.
  - Shift extends.
  - Cmd/Ctrl toggles.
  - The selected item is the same across views: select a layer in the
    Timeline and it's selected in the Xsheet, the Node view and Layer
    Properties.
- **D8.10 Clicking an empty cell or empty canvas does the obvious thing.**
  For example, drawing on an empty cell creates the next drawing.

- **D8.11 Cursors show what will happen.** Over the canvas, Symphony
  draws its own cursor:
  - A crosshair at the exact hotspot, with a small icon of the active tool
    beside it. Both are drawn white over a dark outline, so they read on
    paper and on the grey surround.
  - When a shortcut or modifier changes what the pen does, the cursor
    changes at once, like Harmony's:
    - Space shows the Hand.
    - Shift+Space shows Rotate.
    - Option/Alt with the Zoom tool shows zoom out.
    - The pen's eraser end shows the Eraser.
    - A locked layer shows a "no" sign.
  - **Brush size cursor** (Preferences, as in Harmony): drawing tools add
    a circle the size of the brush. The Eraser always shows its circle.
  - Outside the canvas, the OS cursor is used as normal.

---

## 9. Motion

- **D9.1 No decorative animation.** Motion only answers an action, such as
  a panel opening or a drop target appearing, and lasts 120 ms or less.
- **D9.2 Motion never delays input.** An animation in progress never
  holds back a click or a stroke.
- **D9.3** Respect the OS "reduce motion" setting, which turns all UI
  motion off.

---

## 10. Words

- **D10.1 Harmony's vocabulary** for Harmony concepts: Peg, Drawing,
  Column, Exposure, Xsheet, Timeline, Composite, Display, Write, Onion
  Skin, Line Art, Colour Art, Palette, Swatch, Deformer, Pivot, Keyframe.
  New Symphony-only concepts get plain names. The project glossary lives
  in [RESEARCH.md](RESEARCH.md).
- **D10.2 Buttons start with a verb** and say exactly what happens:
  "Save scene", not "OK". An action keeps the same name everywhere: the
  menu item, button, tooltip, undo entry and message all match.
- **D10.3 Errors** say what happened and how to fix it, in plain words,
  with no apology and no vagueness. For example: "Couldn't save
  MyScene: the disk is full. Free some space and save again."
- **D10.4 Empty states invite action.** For example: "Draw here to create
  drawing 1."
- **D10.5 No filler, no jokes, no exclamation marks** in the UI.

---

## 11. Profiles: Harmony and Symphony

- **D11.1** Two profiles, chosen at first launch and changeable in
  *Preferences → Workflow*:
  - **Harmony**: shortcuts and workflows as the latest Harmony does them.
  - **Symphony**: our improvements.
- **D11.2** Both profiles are always complete and fully working. A
  Symphony improvement never removes the Harmony way.
- **D11.3** Every Symphony-only difference is listed in §11.1 below, with
  its reason, so there's one place that shows how the two profiles
  differ.
- **D11.4** The profile affects **behaviour** (shortcuts, gestures, tool
  defaults, workflows). It never changes **how the app looks**, so both
  profiles share one visual identity.
- **D11.5** Users can override single shortcuts on top of either profile.
  Their overrides are stored separately and survive profile switches.

### 11.1 Symphony profile differences
| Area | Harmony profile | Symphony profile | Why |
|---|---|---|---|
| Redo on macOS | Control+Shift+Z (as Harmony 27 documents) | Cmd+Shift+Z | Matches every other Mac app |

---

## 12. One look on every OS

- **D12.1 Same pixels everywhere.** Symphony draws its whole UI itself
  with bundled fonts and its own icons. It never uses OS widgets, OS
  fonts or OS themes inside the window, so macOS, Windows and Linux look
  identical.
- **D12.2 Differences are deliberate and listed here:**
  - Modifier names and keys: Cmd and Option on macOS, Ctrl and Alt
    elsewhere. Each profile maps Cmd to Ctrl automatically.
  - Window buttons: left on macOS, inside our top bar. Right on Windows and
    Linux, in the OS frame.
  - File open and save dialogs are the OS's own, because people know
    them.
  - Scrolling, pointer acceleration and display scaling follow OS settings.
- **D12.3 The window frame is always dark.** We ask the OS for its dark
  frame, so there's never a white title bar on Windows or Linux next to the
  dark UI.
- **D12.4** Screenshots for design review come from the app's own
  screenshot mode (`scripts/screenshot.sh`), and the same scene renders
  the same on every OS.

## 13. Accessibility

- **D13.1** Contrast as in D4.4. Colour is never the only signal (D4.5).
- **D13.2** Everything can be done from the keyboard outside the canvas,
  with a visible focus ring (`accent.select`, 2 px).
- **D13.3** UI scale from 75% to 200% (D6.6).
- **D13.4** Left-handed setups work: the Tools toolbar can dock right,
  and nothing assumes a side.

---

## 14. Performance as design

Speed is part of the design (P1). Budgets are in
[ENGINEERING.md](ENGINEERING.md) E6. A feature that breaks a budget isn't
finished.

---

## 15. Review checklist (every UI change)

- [ ] Only theme tokens are used, with no colour literals (D4.1).
- [ ] Any new colour pairing is contrast-checked (D4.4).
- [ ] Sizes are on the 4 px grid and use the standard sizes (D6).
- [ ] Areas separated by tone, not boxes (D3.7, D3.8). No new chrome buttons.
- [ ] Looks the same on all three OSes, or the difference is listed in D12.2.
- [ ] Icons are from our set, on the grid and tooltipped. No emoji (D7).
- [ ] Works with the pen alone. Nothing is hover-only (D8.1).
- [ ] Undoable, and named the same everywhere (D8.4, D10.2).
- [ ] The action is in a menu, with its shortcut shown (D8.7).
- [ ] Shortcuts are defined for both profiles. Any Symphony difference is
      logged in §11.1.
- [ ] Words: sentence case, Harmony vocabulary, verb-first buttons (D5.3, D10).
- [ ] Nothing blocks input, and slow work runs in the background (D8.2).
- [ ] Waits follow §17: acknowledged at once, spinner past 150 ms, words and progress past 1 s, Cancel past 3 s.
- [ ] No generative AI. Any assisted tool is labelled, opt-in, and behind the switch (§16).
- [ ] Checked against the latest Harmony behaviour
      ([CLEAN-ROOM.md](CLEAN-ROOM.md)).

---

## 17. Feedback and waiting

Waiting is fine. Waiting *without knowing anything is happening* is not.
Every action shows that it was received, and every wait shows what's
happening and roughly how long is left.

### 17.1 Response times
| Time since the action | What the user must see |
|---|---|
| ≤ 100 ms | The direct result, or a visible acknowledgement where they acted (button pressed, row highlighted, cursor changed). |
| > 150 ms | An activity indicator **where the work was triggered**: a spinner on the button, in the view, or in the status bar. Never earlier, so fast actions don't flicker (E5.4). |
| > 1 s | **What** is happening, in words ("Opening Walk", "Rendering frame 12 of 48"), and determinate progress whenever the work can be counted. |
| > 3 s | A **Cancel** button, and elapsed time or time remaining. |
| Startup | The window shows something within 200 ms. A launch that will take longer shows a loading screen with its current step. Never a blank or frozen window. |

### 17.2 Rules
- **D17.1 Never freeze.** The UI keeps answering while work runs. Slow
  work runs in the background (ENGINEERING E5.2). If something has to
  block, it shows a loading screen first, never a frozen frame.
- **D17.2 Say what's happening, in words.** A spinner alone isn't enough
  past one second. Name the step with the verb the user chose ("Opening",
  "Saving", "Exporting"). Mention counts if there are any.
- **D17.3 Determinate when possible.** If the work can be counted
  (frames, files, crates), show a progress bar that fills. Only use an
  indeterminate spinner when the total really can't be known.
- **D17.4 One look for waiting.** There is one Symphony spinner and one
  progress bar (in `symphony-ui`). They're drawn in **cel blue** (active),
  never amber (amber is time, D4.1). The spinner turns at a steady,
  calm speed.
- **D17.5 Background work stays visible.** Saving, autosave, export and
  imports show in the **status bar's activity area**, with spinner, label
  and progress, for as long as they run, even while the user keeps
  drawing.
- **D17.6 Confirm the finish.** When background work ends, the status bar
  says so briefly ("Saved Walk", "Exported 48 frames"), then quietly
  clears. Failures stay until read and use plain words (D10.3).
- **D17.7 Loading screens are designed screens,** not empty windows: the
  Symphony name, the current step in words, a progress bar or spinner,
  and elapsed time once it passes 3 s. Same tokens, fonts and spacing as
  the rest of the app.
- **D17.8 Placeholders over blank space.** While a view's content loads
  (a thumbnail, a big drawing), show its frame with a quiet placeholder
  and a spinner, so the layout doesn't jump when it arrives.
- **D17.9 Reduced motion.** With the OS "reduce motion" setting, spinners
  become a static "Working" label with pulsing removed. Progress bars
  still fill (D9.3).
- **D17.10 Motion here is allowed.** Loading indicators are the one
  continuous motion in the app (an exception to D9.1), because they
  answer the user's action and show it's still alive.

## 16. AI

The full policy for artists is in [AI-POLICY.md](AI-POLICY.md).

- **D16.1 No generative AI and no LLMs.** Nothing in Symphony generates
  images, drawings, colours, poses, in-betweens or text. This rule can't be
  overridden by a profile, a setting or a plugin shipped with Symphony.
- **D16.2 Assisted tools are opt-in.** A tool that uses a trained model is
  off until the artist turns it on, and **Preferences → AI → Use no AI** (on
  by default) disables all of them at once.
- **D16.3 Always labelled.** An assisted tool carries an "Assisted" tag in
  its tooltip, its Tool Properties header and its menu item, and is listed
  in AI-POLICY.md with what the model does.
- **D16.4 Provenance is visible.** Scene info shows **No AI used**, or lists
  the assisted tools used on that scene. Artists never have to dig for it.
- **D16.5 Plain algorithms aren't labelled as AI.** Flood fill, gap
  closing, smoothing, tracing and interpolation work with the switch on,
  and are never marketed as "smart" or "AI".

---

## Sources and notes
- Process inspired by Anthropic's *frontend-design* skill (a small token
  system decided before building, restraint, writing rules), adapted from
  web pages to a desktop creative tool.
- Dark UI practice: avoid pure black and pure white text, meet WCAG
  contrast, use few accents, keep the content first.
- Contrast ratios in §4.1 were computed with the WCAG 2.x relative
  luminance formula.
