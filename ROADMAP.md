# Sidecraft Roadmap

Sidecraft is a high-quality, low-poly living builder: a three-dimensionally
generated voxel world presented as a side-on 2.5D game. The player inhabits one
foreground slice, builds against an editable backwall, and sees additional
generated depth behind it.

The direction takes inspiration from the large simulated worlds, climate,
caves, water, plants, and technology progression of
[The Blockheads](https://theblockheads.net/), while preserving Sidecraft's own
3D voxel presentation.

## Status legend

- `[x]` complete
- `[~]` active; at most one item may use this status
- `[ ]` planned
- `[!]` caution or blocker
- `[-]` abandoned or failed

## Resume here

- Active item: none
- Next item: M2.2 — streaming
- Deferred item: M1.3 — add one local PowerShell quality-gate command
- Blocker: none
- Last completed work item: M2.1 persistent layers — added independent editable
  foreground/backwall storage, interaction, rendering, lighting, and exact SCW
  schema 5 persistence
- Verification baseline: confirmed 2026-08-01; formatting, Clippy with warnings
  denied, 168 automated tests, the release build, deterministic real-GPU
  midnight/noon gameplay captures, and a real-GPU editor launch passed locally

At the start of a work session, mark exactly one item `[~]`. At the end, change
it to `[x]`, `[!]`, or `[ ]`, record the next concrete item here, and record any
failed verification beside the affected milestone. Never mark work complete
from code inspection alone.

## Product decisions

- Gameplay is a living-builder loop: systemic terrain, homesteading,
  crafting/technology, exploration, and gentle survival.
- The world generator is three-dimensional but renders six bounded depth
  slices.
- Depth 0 is the colliding foreground and player plane.
- Depth 1 is an editable, persistent, non-colliding backwall.
- Depths 2 through 5 are deterministic render-only scenery.
- Simulation follows a Minecraft-like activation model: render distance is
  larger than simulation distance; inactive chunks freeze except for explicit
  bounded ticking areas.
- Performance matters, but deterministic serial behavior is the reference
  before parallel optimization.
- Prototype saves and internal APIs have no compatibility guarantee. Old
  implementations are deleted rather than migrated.
- Verification is local. The project does not use GitHub Actions or another
  hosted build server.

## M0 — Playable foundation `[x]`

- `[x]` State-driven main/settings/texture-pack menus, world selection,
  loading, gameplay HUD, pause, and saving.
- `[x]` Side-on player movement and interaction locked to the foreground.
- `[x]` Six correlated voxel depth slices rendered through Bevy's 3D stack.
- `[x]` Deterministic terrain with caves, ores, vegetation, and bedrock.
- `[x]` Lazy multi-threaded horizontal chunk generation and unloading.
- `[x]` Floating origin using global `i64` chunk identities.
- `[x]` Chunk-batched meshes, derived lighting, and merged 2D colliders.
- `[x]` Mining, block placement, a seven-slot block hotbar, and torches.
- `[x]` Regional Postcard/Zstandard SCW packages and atomic manifest writes.
- `[x]` Unit, integration, lifecycle, and rendered-smoke test foundations.
- `[x]` Domain, application, Bevy, UI, and persistence responsibilities were
  separated in M1.1.
- `[x]` The schema-compatible single-file SCW1 world reader was deleted in
  M1.1.
- `[x]` Rear depth slices include an editable, persistent backwall.
- `[!]` There is no bounded block-simulation engine.

## M1 — Architecture foundation `[ ]`

### M1.1 Boundaries and legacy deletion `[x]`

- `[x]` Separate plain-Rust domain code, application orchestration, Bevy and
  platform adapters, and the composition root.
- `[x]` Split existing modules where concerns have distinct invariants,
  dependencies, lifecycles, or reasons to change; keep cohesive code together.
- `[x]` Separate authoritative world storage, stream state, render state,
  persistence coordination, and UI session state.
- `[x]` Delete the single-file SCW loader, its data paths, compatibility
  branches, and compatibility tests.
- `[x]` Delete other unused APIs and old representations found during the
  boundary refactor; do not add wrappers or aliases.

### M1.2 Mutation and derived state `[x]`

- `[x]` Introduce `VoxelPos`, `VoxelLayer`, `BlockState`, and read-only
  `WorldView` domain types.
- `[x]` Make `WorldMutator` the sole write boundary for player, generation,
  loading, simulation, and debug changes.
- `[x]` Add atomic multi-cell proposals with preconditions and deterministic
  conflict resolution.
- `[x]` Dispatch one mutation report into independent render, lighting,
  collision, persistence, and simulation dirty sets.
- `[x]` Coalesce all derived rebuilds to once per chunk/layer per frame.

### M1.3 Local quality gate

- `[ ]` Add one PowerShell command that runs formatting checks, Clippy, all
  tests, and a release build.
- `[ ]` Ensure every resulting domain/application module has focused tests.
- `[ ]` Review every resulting module for one cohesive responsibility and a
  narrow public surface.
- `[ ]` Run the complete baseline locally and resolve or record every failure.

M1 is complete when no authoritative block write bypasses `WorldMutator`, the
legacy reader is absent, module boundaries follow `ARCHITECTURE.md`, and all
local gates pass.

## M2 — Layered world and fresh SCW schema `[ ]`

### M2.1 Persistent layers `[x]`

- `[x]` Store foreground and backwall as independent dense chunk layers.
- `[x]` Initialize both layers from depths 0 and 1 of the 3D generator.
- `[x]` Keep depths 2 through 5 generator-derived and render-only.
- `[x]` Make the backwall editable and persistent without creating colliders.
- `[x]` Connect authoritative backwall mutations to the existing six-depth
  light volume and depth-aware material presentation.
- `[x]` Default interaction to foreground; use `Tab` to select backwall and
  show the active layer in the HUD.
- `[x]` Test overlapping foreground/backwall blocks, negative coordinates,
  chunk edges, unload/reload, and save/reload.

### M2.2 Streaming

- `[ ]` Configure simulation radius 3, render radius 5, and unload radius 7 for
  32-block horizontal chunks.
- `[ ]` Prioritize load/generation jobs nearest-first.
- `[ ]` Limit in-flight work according to available worker capacity.
- `[ ]` Tag results with session and generator identity and discard stale or
  cancelled results before commit.
- `[ ]` Preserve deterministic generation regardless of task completion order.

### M2.3 SCW schema 6

- `[ ]` Replace current save structs with one current-schema representation;
  do not keep version-suffixed legacy Rust types.
- `[ ]` Require `SCW1` plus exact `schema_version = 6`.
- `[ ]` Encode metadata, palettes, region references, and pending ticks with
  Postcard, then compress SCW payloads with Zstandard.
- `[ ]` Reserve dense code 0 for air and codes 1–255 for the per-region
  non-air `BlockState` palette.
- `[ ]` Store dense one-byte foreground and backwall arrays for each changed
  chunk.
- `[ ]` Persist global `world_tick`, next tick sequence, player/day metadata,
  generator version, and per-region pending ticks.
- `[ ]` Write immutable content-addressed regions before atomically publishing
  the manifest.
- `[ ]` Serialize async save publication, coalesce requests, and reject stale
  completion.
- `[ ]` Reject wrong magic, schema, checksum, framing, compression, ordering,
  palette, dimensions, or runtime values without fallback.

M2 is complete when both editable layers survive long-distance streaming and a
fresh save/reload, unsupported worlds fail visibly, and interrupted/stale saves
cannot replace the last valid manifest.

## M3 — Deterministic living-world simulation `[ ]`

### M3.1 Clock and activation

- `[ ]` Add an independent 20 TPS simulation clock without changing Bevy's
  fixed clock or Avian physics schedule.
- `[ ]` Run at most four logical simulation steps per rendered frame and cap
  accumulated wall-time at 200 ms.
- `[ ]` Advance only while playing; pause, loading, and process downtime do not
  catch up.
- `[ ]` Simulate only the active radius from `SimulationRegionProvider`.
- `[ ]` Freeze inactive chunks and persist their pending scheduled ticks.
- `[ ]` Add optional bounded ticking areas without coupling rules to the
  current single-player region.

### M3.2 Deterministic rule engine

- `[ ]` Evaluate every rule against one immutable tick state.
- `[ ]` Sort atomic proposals by rule priority, source position, and stable
  sequence before resolving conflicts.
- `[ ]` Derive random samples from seed, world tick, global chunk, layer, and
  attempt; never depend on hash-map order or a mutable global RNG.
- `[ ]` Process at most 4,096 scheduled proposals plus three random samples per
  active foreground chunk per tick.
- `[ ]` Leave overflow queued in stable order and expose backlog diagnostics.
- `[ ]` Defer cross-frontier work instead of force-loading inactive chunks.

### M3.3 Initial rules

- `[ ]` Water flows down before sideways, uses levels 0–7, updates every five
  ticks, and renews a source between two valid sources.
- `[ ]` Lava flows down before sideways, updates every 30 ticks, and never
  renews sources.
- `[ ]` Water/lava contacts deterministically form obsidian, cobblestone, or
  stone according to source and flow state.
- `[ ]` Sand and gravel move down one cell per tick using atomic two-cell
  proposals and stack deterministically.
- `[ ]` Render falling blocks with interpolation without making physics
  entities authoritative.
- `[ ]` Plants use random ticks and require valid substrate, free space, and
  sufficient light.
- `[ ]` Exposed dirt can receive grass from nearby lit grass through
  deterministic random ticks; covered grass eventually returns to dirt.
- `[ ]` Remove floor and wall torches whose supports are removed by simulation
  proposals, using the same mount dependency rule as player-driven breaking.
- `[ ]` Schedule affected neighbors for no earlier than the next tick to avoid
  unbounded same-tick cascades.

### M3.4 Persistence and performance

- `[ ]` Snapshot block arrays and pending queues at one revision.
- `[ ]` Reload scheduled ticks in stable order and ignore stale expected-block
  ticks safely.
- `[ ]` Prove identical results for equal seed, commands, logical ticks, and
  activation history.
- `[ ]` Establish a serial benchmark baseline for default radius and maximum
  per-tick budget.
- `[ ]` Add chunk-parallel immutable evaluation only if release benchmarks show
  a material gain; retain deterministic single-threaded merge and commit.

M3 is complete when exact-tick tests cover fluids, gravity, growth, conflicts,
activation, persistence, overload, and physics-clock independence.

## M4 — Builder progression `[ ]`

- `[ ]` Add finite inventory stacks and world item drops.
- `[ ]` Consume blocks on placement and generate data-driven drops on mining.
- `[ ]` Add replaceable data-driven recipes and crafting stations.
- `[ ]` Add tool tiers, mining suitability, durability, and repair/replacement
  loops.
- `[ ]` Add workbench and furnace processing using scheduled simulation work.
- `[ ]` Persist inventory, equipment, stations, and processing state in a new
  exact save schema; invalidate schema 5 rather than migrating it.
- `[ ]` Test complete gather, craft, place, save, and reload journeys.

M4 is complete when a new player can gather resources, improve tools, build a
home, process materials, and resume that progression after reload.

## M5 — Ecology and exploration `[ ]`

- `[ ]` Add biome-dependent terrain, resources, vegetation, and surface color.
- `[ ]` Add temperature, precipitation, weather, and seasons.
- `[ ]` Expand plant rules into farming and self-propagating ecology.
- `[ ]` Add caves, deposits, ruins, and other deterministic exploration goals.
- `[ ]` Add gentle hunger, shelter, temperature, or equivalent survival
  pressures without making combat the primary loop.
- `[ ]` Test long-running ecology determinism, inactive-region behavior, and
  bounded workload.

M5 is complete when homesteading, exploration, climate, and the simulation
engine form a coherent living-builder loop.

## M6 — Presentation, usability, and scale `[ ]`

- `[x]` Add an exact 20-minute, 24,000-tick day/night presentation with a
  visible pixel-art sun, stars, eight persistent moon phases, celestial
  presentation, clock-aware saves, and lightmap-only clock refresh.
- `[x]` Establish side-on 2.5D voxel composition with a fixed 10-degree yaw,
  14-degree downward pitch, view-space camera snapping, directional face
  separation, and depth haze.
- `[x]` Replace smooth PBR world lighting with a six-depth, dual-channel 0-15
  light volume, six-neighbor sky and block propagation, linearly sampled voxel
  lightmaps with uniform per-face coordinates, targeted invalidation, and
  light-aware player materials.
- `[x]` Add neighborhood-filtered whole-face lighting, subtle quantized
  ambient occlusion, continuous player tinting, stronger cyclic palettes, and
  chunk-batched torch light without within-face gradients or real-time PBR
  world lights.
- `[x]` Establish 16-by-16-pixel block materials with restrained palettes,
  connected pixel-art clusters, semantic grass, wood, and ore faces, a
  cap-emissive low-poly torch, uniform depth exposure, and richer six-slice
  scenery.
- `[x]` Replace shared procedural masks with a separately publishable,
  deterministic texture generator supporting bounded cluster, placement, ore,
  palette, quality, recipe, batch, per-material CLI controls, and an evenly
  varied small-mark pattern for classic voxel block faces.
- `[x]` Add schema-1 full and partial texture packs, strict PNG/path validation,
  a checked-in default pack, authoritative previews, global atomic selection,
  live staged menu switching, stable runtime asset handles, environment assets,
  player colors, and hotbar icons.
- `[x]` Replace the runtime preview thumbnail with a responsive fixed-noon live
  voxel showcase across front-end states, including every block, six scenery
  depths, the player palette, shared environment art, and version chrome.
- `[!]` Add a standalone texture-pack editor with a resizable typed parameter
  panel, larger controls, coherent one-click candidate generation, debounced
  revision-safe generation, a Day/Night shaded 3D preview sharing the game
  showcase layout and framing, exact atomic project save/open, unsaved-change
  handling, and non-overwriting export through the publishable texture
  package; all workspace gates, the release build, and a real-GPU launch pass,
  but manual editor visual acceptance remains pending because the hidden
  hardware surface could not be captured through the Windows compositor.
- `[ ]` Add further material-specific face variation and restrained block
  silhouette cues without smoothing the pixel-art aesthetic.
- `[ ]` Add fluid surfaces, falling-block interpolation, plant animation,
  particles, weather effects, and responsive interaction feedback.
- `[ ]` Add music, ambient sound, interaction audio, and volume controls.
- `[ ]` Improve rebinding, controller support, accessibility, and UI scaling.
- `[ ]` Profile generation, simulation, mesh rebuilding, lighting, saving, and
  memory during sustained long-distance travel.
- `[ ]` Add stress tests for negative and positive `i64` coordinates, repeated
  origin rebasing, rapid streaming reversals, large save sets, and simulation
  backlogs.
- `[ ]` Tune parallelism and budgets from recorded release-mode measurements.

M6 is complete when the side-on plane is always readable, the rear depth is
visually rich, long sessions remain stable, and presentation supports the
living-builder identity.

## Deferred beyond the initial roadmap

- Multiplayer and network save compatibility
- Infinite playable depth or free 3D player movement
- Colony automation and large NPC settlements
- Boss-centric progression
- A public mod API or compatibility promises
- Touch-first/mobile UI
- Any legacy save, API, or behavior migration

## Milestone verification

Every milestone must run:

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo build --release
git diff --check
```

Testing is divided by responsibility:

- Unit tests cover every domain and application module containing logic.
- Headless integration tests cover Bevy state, adapter boundaries, streaming,
  mutation propagation, physics integration, and asynchronous lifecycle.
- End-to-end tests exercise menu-to-world, interaction, simulation, saving,
  reloading, long-distance travel, and visible failure handling.
- A real-GPU smoke test covers final rendering because headless tests cannot
  prove visual quality.

A milestone is not `[x]` until its implementation, tests, documentation,
architecture review, and relevant manual smoke test are complete.
