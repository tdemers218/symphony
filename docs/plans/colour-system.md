# Colour system and dynamic swatches

Tristan's brief (2026-11-28), turned into decisions against the real code.
Section numbers in brackets point back to the brief.

**Goal:** make it take less effort to reach the colour an artist means,
while they keep full control. Static colour stays the default and stays
fast. Dynamic behaviour is an extra on the same swatch, never a separate
system. A beginner picks green, turns on Natural variation and paints a
forest. An advanced artist sets that same green to respond to pressure and
direction, with fixed random results, linked to a scene lighting palette.
Both use the same swatch.

**Profiles:** both profiles get these features. The Harmony profile changes
nothing Harmony users rely on:
- Colour view in List mode, or Swatch mode (Colours > Swatch Mode in
  Harmony 27's Colour view menu);
- palettes, Protect Colour, and recolouring by swatch.

The Symphony additions sit behind a "Dynamics" entry on the swatch and in
Search. A swatch with no dynamics behaves exactly as a Harmony colour does.

**No AI** (docs/AI-POLICY.md). Every suggestion, palette and variation here
comes from deterministic colour maths that runs on the machine. "Natural-
language colour search" [9] means matching colour names against a built-in
word list ("warm dark red"), not a language model.

## Audit: what exists

| Need | Today | Plan |
| --- | --- | --- |
| Colour | `palette::Swatch { id, name, rgb: [u8;3], opacity: u8, protected }`. Strokes and fills point at a swatch by `Id`, so editing a swatch recolours its art (E2.3) | Keep `rgb`/`opacity` as the swatch's **base** colour, used as-is by every static path. Add `dynamics: Option<Box<Dynamics>>` (serde default `None`), so old projects load unchanged. |
| Colour opacity | `Swatch::opacity`, applied by the renderer (`swatch_opacity_limits_textured_brush_coverage`) | It stays the swatch's own transparency: how see-through the paint itself is. |
| Brush opacity and flow | `StrokeTexture { opacity, flow, opacity_by_pressure, opacity_curve, flow_curve, opacity_jitter, flow_jitter }` | Stay in the brush. Phase 1 writes down how the four opacities combine and adds a test of that order. |
| Layer opacity | Transparency on layers and effects (`effects.rs transparency_and_opacity_behave`) | Unchanged; composited last. |
| Input signals | `stroke::Point { x, y, pressure, pen: Pen { tilt, azimuth, rotation } }`. Speed isn't kept | Add one shared `input::Sample` derived from the points: pressure, tilt, azimuth, direction, distance along the stroke, speed (from point timing, which is kept from Phase 2 on), and a seeded random value. Brush dynamics and colour dynamics both read it. |
| Blending | `graph::BlendMode` (20 modes) and `render::effects::mix(mode, backdrop, source)` | Move `mix` into `symphony-core::colour`, so the picker preview, inverse blending and the renderer all run the same function. No second copy. |
| Picker | `views/colour_picker.rs`: an HSV square or triangle and a hue slider in a popup | Phase 1 adds an OKLCH picker with gamut mapping. The HSV square and triangle stay as alternative layouts. |
| Colour view | `views/panels.rs colour()`: a list of rows (28×18 chip and name) | Square swatches with concentric corners. List and Swatch modes, as in Harmony. **Done in step 0.** |
| Determinism | Strokes have stable `Id`s; there's no per-stroke seed | The seed is a hash of the stroke `Id` plus the swatch `Id`. It survives undo, save and reload, and nothing new is saved. "Reroll" stores an explicit seed offset. |

### How the four opacities combine (written into the code in Phase 1)
Each dab's coverage is brush **flow** × the pressure curve, building up to
at most brush **opacity**. That coverage is then multiplied by the
swatch's **colour opacity**, and the drawing is composited with the
layer's **transparency** and blend mode. Colour opacity is part of the
paint, so it travels with the swatch to every brush. Brush opacity and
flow are about how the paint goes down. That's [1] in the brief: three
systems, connected only through the shared input signals.

## The model (`crates/symphony-core/src/colour.rs`, new)

```
Oklch { l, c, h }            // conversions to and from sRGB, gamut mapping
Dynamics {
    behaviours: Vec<Behaviour>,      // composable, applied in order
    frequency: Frequency,            // PerStroke | PerDab | PerRegion | Continuous | Spatial | Time
    seed_offset: u32,                // "Reroll"
}
Behaviour {
    kind: Variation { hue, chroma, lightness, opacity }   // ranges in OKLCH
        | Pressure { to: Colour }       | Direction { colours, reference_angle }
        | Tilt { to }                    | Speed { to }
        | Multicolour { stops, interpolate: Oklab | Hue | Steps }
        | Spatial { scale }              | Distance { length, stops }
        | Time { per_frame }
    amount: f32,                     // the single "intensity" slider [7, level 2]
    curve: Option<PressureCurve>,    // response curve (level 3)
    smoothness: f32,                 // noise smoothness for Continuous
}
fn colour_at(base: Oklch, d: &Dynamics, input: &Sample, seed: u64) -> Rgba
```

- One evaluator for previews, canvas, export and baking [10]. Presets
  (Natural, Pressure darkens, Direction warm/cool…) are `Dynamics` values,
  not separate code [7, level 3].
- Static swatches never call it: `dynamics.is_none()` is the fast path
  [10]. This is the case that needs a benchmark.
- Variation works in OKLCH and maps back into the sRGB gamut by reducing
  chroma at fixed lightness and hue, so a variation never clips into a
  different hue [2, perceptual].
- Continuous variation uses seeded value noise along the stroke's length,
  smoothed so the colour drifts instead of jumping.

## Rendering dynamic colour

- **Textured Vector brush (stamped):** each dab already has a position,
  pressure and pen. `colour_at` runs per dab inside
  `symphony-vector::stamp`. Per-dab and continuous frequencies land here
  first.
- **Vector Pencil and Brush lines (one filled contour):** the per-stroke
  colour is exact. Continuous colour along the line uses vertex colours
  on the outline mesh. The egui canvas meshes support them; the
  tiny-skia export needs a per-segment gradient. Both are built from the
  same sampled colours, so the canvas matches the export.
- **Fills (paint zones):** per region. The seed is the zone's id, so a
  forest of leaves painted with one swatch gets slightly different greens
  that stay put.
- **Caches** are keyed by content (the perf rule). A dynamic swatch's
  colours depend on the swatch, the stroke and (for Time) the frame, so
  the key gains `swatch.version` and the frame only when a Time
  behaviour exists. No frame key otherwise, so there's no flicker and no
  re-stamping during playback.

## Interface (progressive disclosure [7])

- **Level 1, the picker popup:**
  - OKLCH wheel or square (with the HSV layouts kept);
  - opacity;
  - current and previous colour;
  - eyedropper;
  - a row of five suggestion chips: deep shadow, shadow, base, light,
    highlight;
  - one "Dynamics" button, which shows a dot when the swatch has
    dynamics.

  Picking a static colour takes as many clicks as it does today.
- **Level 2, Dynamics expanded:** cards for Natural, Pressure, Direction,
  Multicolour and Temperature. Each has an on/off switch, one Amount
  slider and a live mini-stroke drawn with the current brush through the
  real evaluator. Turning on Natural is all it takes to start painting.
- **Level 3, "Advanced" disclosure per card:**
  - hue, chroma and lightness ranges;
  - curve, frequency, smoothness and scale, seed and Reroll;
  - reference angle and interpolation.

  Every control has a Search entry (`search.rs`), so "variation
  frequency" finds it.
- **The swatch shows its behaviour:** a dynamic swatch draws its range
  inside the chip (a small gradient or scatter inside the same rounded
  square) with a corner mark. You can see it's dynamic at a glance,
  without a label.
- **Colour Lab [4]:** its own view (`View::ColourLab`), dockable or
  floating, and never part of the default picker.

## Phases and steps

Each step is one or more commits with tests and a manual update (E11.6).

### Step 0: swatches (done first, Tristan's ask) ✅ 605ab92
- Square chips with concentric corners: chip radius `RADIUS_CONTROL`, the
  row or tile highlight `around(chip, inset)`, the selection ring
  `around(chip, gap)`.
- Colour opacity is shown over a checkerboard, so a 40% swatch looks 40%.
- Colour view **List / Swatches** switch, matching Harmony's List and
  Swatch modes. Swatches is a grid of squares with the name on hover.

### Phase 1: foundation ✅ c4966f8, 241b6d2
1. `colour.rs`: sRGB ⇄ linear ⇄ OKLab/OKLCH, gamut mapping, tests
   (round-trips, known reference values, gamut edge).
2. Move `mix` / `BlendMode` blending maths into core, with the renderer
   calling it. Existing render tests must pass unchanged.
3. The four-opacity order (above) written down in code, with a test
   through the renderer.
4. The OKLCH picker layout in the popup, and current / previous colour.
5. Composite preview: in the picker, the colour as it will look over a
   chosen backdrop at a blend mode and opacity, using the core `mix`.
   White, grey and near black for now; sampling the canvas comes with
   the smart eyedropper.

### Phase 2: dynamic colours
1. ✅ `Dynamics` model, serde, the evaluator, the seeding rule, and tests
   (determinism, undo or reload give identical colours). In
   `symphony-core/src/dynamics.rs`; Variation and Pressure so far, other
   kinds join as their inputs arrive.
2. Shared input `Sample`: point timestamps for speed, and stroke distance.
3. Per-dab colour in the stamped brush, Natural variation and Pressure.
4. Per-stroke and per-region colour for vector lines and fills.
5. Direction, Tilt, Speed and Multicolour. Continuous noise and vertex
   colours.
6. Level 2 and Level 3 interface, the dynamic swatch chip look, and
   Search entries.

### Phase 3: contextual intelligence
1. Suggestions: shadow, highlight, warmer, cooler, complementary, in
   OKLCH, aware of blend mode and opacity (a Multiply shadow suggests
   the source colour that gives that shadow).
2. Inverse blending: pick the result you want over a backdrop, and get
   the source colour. When no source colour can give that result, say
   so and offer the closest one.
3. Palette generation: intent modes, lock a colour and regenerate the
   rest, perceptual ramps, extraction from an image (k-means in OKLab).
4. Colour Lab view: sphere, cube, cylinder, cloth, flat and stroke
   surfaces, lit in software (it's a small offscreen render, cached).
   Separate blend modes for light and shadow, smooth or stepped shading.
   An eyedropper on everything, and "Save ramp to palette".

### Phase 4: production integration
1. Semantic roles in a palette (Base, Shadow, Deep shadow, Reflected,
   Highlight), with optional links (`Derived { from, adjust }`). Changing
   the base re-derives the linked colours, and the art follows because it
   points at swatch ids.
2. Lighting variants per palette (Day, Sunset, Night, Interior), chosen
   per scene or animated.
3. Dynamics can reference role colours.
4. Keyframes on dynamics amounts. A Colour node with inputs and outputs
   in the node graph. Bake to static, and convert a dynamic swatch to
   static.

### Phase 5: extended utilities
- Smart eyedropper: pick from this layer, the composite, or an average.
- Value (greyscale) preview and colour-blindness simulation of the
  Camera view.
- Gamut warnings.
- Detecting nearly identical swatches.
- Scene colour distribution.
- Contrast hints.
- Pigment-style (Kubelka–Munk) mixing.
- Searching colours by name from a word list.
- Temporary colour constraints.

## Risks and rules

- **Never freeze the UI:** previews, the Colour Lab and palette extraction
  run off the UI thread and are cached by content; the UI thread only
  paints.
- **Rasterised art isn't editable:** bitmap layers keep pixels, not
  swatch ids, so palette changes and Phase 4 links don't reach them. The
  interface says so instead of pretending.
- **Saved files:** every new field defaults so older Symphony files load.
  Harmony scene import maps palette colours to static swatches.
- **Speed:** a benchmark guards the static path. Strokes with no dynamics
  must render exactly as fast as before.
