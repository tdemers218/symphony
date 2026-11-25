# Symphony feature direction

This reconciles the six-phase feature plan with the current repository.
[PLAN.md](../PLAN.md) tracks current features, [ROADMAP.md](../ROADMAP.md)
sequences releases, and [DESIGN.md](../DESIGN.md),
[ENGINEERING.md](../ENGINEERING.md) and [AI-POLICY.md](../AI-POLICY.md)
govern implementation. Keep the Rust document and rendering layers
portable, with frontends handling input and presentation.

## A. Artist workflow

| Proposal | Status and existing foundation | Next milestone and acceptance check |
|---|---|---|
| Temporary shortcuts | Partial. Contextual remapping and temporary Space navigation exist in the app. | Add tap/hold handling to the action registry. Activate on press, retain quick taps, and restore the prior state after a hold of about 500 ms. Make the threshold configurable. Test nesting, release across panels, focus loss and cancellation. Limit this to reversible modes. |
| Unified guides | Partial. Drawing and Camera share reference-only one- and two-point perspective and centre lines. The orbitable Multiplane stage shows those guides on the scene camera’s reference plane. | Add reusable scene guide data and active horizontal and vertical symmetry. Validate camera calibration and depth compositing before connecting guides to tilted drawings or imported 3D props. Extend to arbitrary and radial symmetry, one- to three-point, isometric, curved and fisheye perspective, and custom paths. Verify snapping and independent guide visibility before constraining drawing, erasing and artwork changes. |
| Sculpt brush modes | Partial. The Line Editor pushes and smooths points with brush falloff and strength. | Add inflate, shrink, pinch and relax. Reuse the spatial selection and undo path. Test pressure and falloff, stroke identity, texture and linked fills. Benchmark large drawings. |
| Local pressure editing | Partial. The Line Editor includes Thicken, Thin, and Smooth Pressure to edit pressure without changing position. | Improve editing on sparsely sampled paths. Preview before committing a single undo step. Add a curve editor and other compatible pressure channels after width edits work through save, reload and render. |
| Brush sandbox | Partial. Tool Properties has a generated brush preview. | Expand it into an isolated drawing surface that accepts pressure and tilt. Support clearing and comparison. Test that live adjustments, marks and clearing leave the project and its history untouched. |
| Timeline bookmarks | Implemented. Named frames and ranges have optional colour and notes, a pinned Timeline lane, and Search navigation. Whole-scene frame insertion retimes them. | Follow up with denser layouts for overlapping bookmarks if needed. Playback and exports ignore bookmarks. |
| Onion skin alignment | Partial, with independent previous/next translation, rotation and scale, reset and Search in this pass. | Add direct canvas handles if numeric controls prove limiting. Test that alignment affects tints only and stays out of scene saves, artwork edits and exports. |
| Undo history | Partial, with a dockable or floating view with named history and live navigation in this pass. | Add thumbnails or isolated previews only after measuring reconstruction and memory. Reuse existing grouped commands, and keep active history branches and redo ordering predictable. |

## B. Nodes and performance

| Proposal | Status and existing foundation | Next milestone and acceptance check |
|---|---|---|
| Folders, groups and bundles | New. Graph layout, selection, Node Properties and the node Library already exist. | Start with folders that leave evaluation untouched. Then implement nested groups with typed exposed ports and internal graph editing. Package groups with named, ranged controls, defaults, dependencies and compatibility metadata. Test ungrouping, cycles and bundle round trips. |
| Coded addons | Depends on a versioned boundary for graph evaluation and property schemas. | Specify a portable contract for typed ports, serialized properties, migrations and permissions. Evaluate isolated WebAssembly and constrained shaders with resource limits. Consider native loading separately. Validate incompatible versions and failing addons without jeopardizing the scene. |
| Performance heat map | Partial. Timing benchmarks and render caches exist. | Instrument per-node CPU time, evaluation counts and cache hits. Add optional GPU timing, memory estimates, historical samples and critical paths where measurable. Benchmark with profiling disabled and enabled. |
| Adaptive quality | Partial. Camera can already bypass effects for editing. | Reuse that preview path for per-node bypass, resolution or sampling reductions, cached or simpler outputs and essential full-quality nodes. Integrate it with profiling after instrumentation. Verify scene properties and final exports are unchanged. |
| Integrated mattes | Partial. The graph already supports masks and reversible Drawing-to-Matte conversion. | Share those operations with effect-owned mattes, retaining external ports. Provide one contextual editing tool for drawing, selection and transforms. Add invert, feather, animation and multiple operations through the shared renderer. Test existing manual graphs and export parity. |

## C. Artist-directed motion

| Proposal | Status and existing foundation | Next milestone and acceptance check |
|---|---|---|
| Smart animation charts | New. Timeline already owns frame and key timing. | Start with an editable trajectory and frame range, markers distributed by arc length, and a current-frame reference point. Add easing, custom curves, manually placed markers and holds. Make multiple named charts reusable and optionally visible on playback. Progress from reference to peg/rig driver and in-between constraint. Test straight, curved and held trajectories. |
| Interactive in-betweening | Partial foundation. Insert In-betweens allocates frames and retimes exposures and keys. | Build deterministic correspondence with editable manual matches and landmarks. Add suggestions, multiple chart constraints, adjustable intensity, preview and volume preservation. Produce editable drawings and handle appearing or disappearing strokes where possible. Test difficult correspondences and scene round trips. |
| Temporal selection | Partial. Select operates on artwork, and bones support multi-selection within one rig. | Extend Select to explicit drawings, layers, frame ranges and compatible rig components. Add an affected-frame preview, optional strength falloff and explicit correspondence for independent drawings. Test undo, cloned exposures and uncertain matches. |

## D. Reusable character animation

| Proposal | Status and existing foundation | Next milestone and acceptance check |
|---|---|---|
| Independent rig assets | Partial. Bone rest poses, animated properties and drawing bindings already exist in the graph. | Separate reusable topology, proportions, animation, drawing libraries, bindings, weights and constraints while migrating existing scenes. Test sharing the same motion between distinct rest poses. |
| Skeleton templates | Depends on portable rig serialization. Unbound bones already work in scenes. | Save templates that preserve structural bones and constraints even when no drawing is attached. Test adjusting proportions, binding only needed parts, and reusing animation. |
| Drawing-library substitution | Partial. Exposure substitution and linked clones already work. | Add semantic roles such as limbs and expression sets, persistent mappings and artist overrides. Identify ambiguous matches and allow missing artwork or extra bones. |
| Multi-bone binding | Partial. Bone-driven deformation and mesh warps already exist. | Add multiple normalized influences, weight editing and visualization, and rigid regions. Test deformed rendering, weight normalization and compatibility with single-bone scenes. |
| Retargeting | Depends on independent rest poses and semantic bindings. | Transfer rest-relative motion across proportions, handling optional bones, root motion and IK contact correction. Preview mapping and motion before committing. Test changing limb length and preserving planted hands and feet. |

## E. Lighting and materials

| Proposal | Status | Next milestone and acceptance check |
|---|---|---|
| Height/normal reconstruction research | New research. | Survey relevant Blender addons and deterministic methods. Record inputs, stylized failure cases, runtime and licences before choosing an approach. |
| Material data | New. Existing render and node architecture is the integration point. | Define optional height, normals, roughness, specular response, emission and practical occlusion for vector and raster artwork on rigs, frame-by-frame art, props and backgrounds. Test scene round trips and transform/deformation alignment. |
| Manual and procedural height | Depends on material data. | Start with imported maps, assigned heights and gradients. Explore painting, bevels, inflation, presets and silhouette/region-derived height. Test detailed background work and propagation between drawings. |
| Live lighting | Depends on material data and render integration. | Prototype GPU directional, point and spot lights with animated intensity and colour. Add softness, ramps, cel and smooth shading, and painterly controls. Check temporal consistency, camera/export agreement and performance. |
| Optional assisted material generation | Research proposal, subject to [AI-POLICY.md](../AI-POLICY.md). | Keep deterministic/manual workflows independent. Any eligible local assisted tool must be off by default, require explicit enablement and download approval, disclose size and hardware needs, be uninstallable, keep outputs editable and never send artwork away. Generative AI remains excluded by project policy. |

## F. Production and shared resources

| Proposal | Status and existing foundation | Next milestone and acceptance check |
|---|---|---|
| Reference board | New. Docking, image import and viewport navigation already exist. | Add a dockable, pannable board with clipboard/drop import, groups, transforms and notes. Support project and reusable boards. Investigate supported PureRef and Pinterest interoperability after the offline workflow works. |
| Shared libraries | Partial foundations. Brush libraries, palettes and scene assets already exist. | Define versioned stable IDs for drawings, rigs, brushes, palettes, node bundles and addons, materials, reference boards and applicable preferences. Preserve per-project overrides and pinning. |
| Project synchronization | Depends on stable IDs and versioned libraries. | Diff upstream updates, allow accept/reject/postpone, surface conflicts and incompatible changes, and support rollback. Verify offline opening and unchanged completed scenes. |
| Audio scratching | Partial. Timeline already shows waveform tracks and supports timed sound clips. | Add short scrub previews, then variable-rate forward/reverse scratching and optional pitch preservation. Test frame-accurate sync and keep decoding off the Timeline interaction path. |
| Diagnostics | Partial. Graph evaluation and import already report errors. | Collect issues into a navigable report for missing assets, invalid connections, rig bindings, schemas, addon/library versions, unused resources and measurable bottlenecks. Distinguish errors from warnings and require confirmation for destructive repair. |

## Order and handoff

Onion alignment and history navigation move ahead of the larger Phase A
work because they reuse current rendering and history without a file
format change. Land each remaining Phase A workflow independently.
Instrument nodes before adaptive quality, and build container semantics
before bundles or coded addons. Prove chart references and explicit
correspondence before chart-assisted in-betweening. Establish reusable
rig data before retargeting and material data before live lighting.
Deliver offline reference boards before shared libraries, then add
synchronization. Audio and diagnostics can proceed independently.

For each milestone verify its normal UI and Search entry points,
undo/redo, save and reopen where relevant, and compatibility with older
scenes. Review pen and touch interaction and use workspace tests and
performance gates for changes to drawing or rendering.
