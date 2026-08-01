# Sidecraft Architecture

This document is the binding architecture contract for Sidecraft. The project
is a prototype, so obsolete implementations and data formats are removed
instead of preserved. `ROADMAP.md` records the work required to bring the
current code into compliance.

## Core principles

- Keep one responsibility per module and one reason for each component to
  change.
- Keep game rules independent from Bevy ECS, rendering, physics, storage, and
  operating-system APIs.
- Point dependencies inward: adapters depend on the application layer, and the
  application layer depends on the domain.
- Route every world change through one mutation boundary.
- Introduce interfaces only where an implementation is expected to be
  replaceable or where a side effect must be isolated.
- Prefer deterministic, bounded work over implicit global behavior.
- Return typed errors from runtime paths. Panics are reserved for tests and
  invariants that cannot be violated by world data or user input.
- Delete superseded APIs, readers, schemas, aliases, feature flags, and tests.
  There is no backwards-compatibility layer during the prototype phase.

## Dependency boundaries

The target dependency direction is:

```text
composition root
    |
    +-- Bevy and platform adapters
    |       |
    |       +-- application services
    |               |
    |               +-- domain model and rules
    |
    +-- storage adapter implementing application ports
```

The implemented M1.1 layout follows that direction:

```text
domain/
    block, world, generation, targeting, lighting, time
application/
    snapshot, repository port, session, streaming, world state, saving
adapters/
    bevy/       ECS and presentation integrations
    storage/    schema-5 SCW package repository
crates/
    sidecraft-texture-editor/
                Standalone Bevy/egui texture-project editor; depends on
                sidecraft-textures and never on the Sidecraft game package
    sidecraft-textures/
                Bevy-free texture-pack format, generator, validator, and CLI
lib.rs          composition root and schedule ordering
```

Domain snapshots are schema-neutral and world IDs are path-free. The storage
adapter maps those values to the current schema-5 package layout. The Bevy
adapter uses thin resource wrappers because Bevy resources must implement its
ECS component contract; the wrapped domain and application types remain
framework-independent.

### Domain

The domain is plain Rust. It owns:

- block identifiers, states, and gameplay properties;
- global coordinates, chunk coordinates, layers, and dense chunk storage;
- deterministic terrain generation;
- immutable world queries and atomic mutation proposals;
- simulation clocks, work queues, rules, and conflict resolution.

The domain must not depend on Bevy entities, assets, meshes, input, UI,
Avian, files, compression, or task pools.

### Application

The application layer owns use cases and lifecycle:

- the active world session and its composite saved world/day version;
- chunk requests, activation, loading, generation, and unloading;
- player/debug commands that request world mutations;
- simulation orchestration;
- save snapshot creation and save-job serialization;
- dirty-state propagation to adapters.

It may depend on domain types and small ports, but not on concrete rendering or
filesystem implementations.

### Adapters

Adapters translate between application/domain data and external systems:

- Bevy startup, schedules, state transitions, input, UI, and audio;
- Avian bodies, collision queries, and colliders;
- cameras, lighting, materials, meshes, particles, and animation;
- SCW files, Postcard, Zstandard, atomic filesystem operations, and task pools.

Adapters may cache derived state. They never become authoritative for block
state.

### Composition root

The composition root selects concrete adapters, registers systems, and defines
their ordering. It contains no game rules.

## World model

The persistent gameplay model uses these concepts:

```rust
enum VoxelLayer {
    Foreground,
    Backwall,
}

struct VoxelPos {
    global_x: i64,
    y: i32,
    layer: VoxelLayer,
}

struct BlockState {
    id: BlockId, // u16 newtype
    variant: u8,
}
```

- Foreground is editable, simulated, rendered, and colliding.
- Backwall is editable, persistent, rendered, and participates in lighting,
  but never creates player collision.
- Render depths 2 through 5 are deterministic generator output. They are not
  mutable world layers and are not saved.
- The generator samples a real three-dimensional voxel field using global
  horizontal position, vertical position, and depth. Depths 0 and 1 initialize
  the persistent layers; deeper samples provide scenery.
- Global horizontal identities use `i64`. Local `f32` coordinates are only
  temporary rendering and physics coordinates around the floating origin.
- An absent chunk means unmodified generator output. A chunk becomes persistent
  when its blocks or pending simulation state differ from that output.

## Presentation contract

- Player movement and collision remain on depth 0. Targeting, mining, and
  placement may select the editable foreground at depth 0 or non-colliding
  backwall at depth 1; depths 2 through 5 remain render-only.
- A fixed orthographic camera looks at depth 0 with 10 degrees of horizontal
  yaw and 14 degrees of downward pitch. Camera setup, following, recentering,
  and floating-origin rebasing must share the same transform construction.
- Camera targets are snapped in camera-view space so the oblique projection
  remains stable at the nearest-neighbor pixel scale.
- Render depths keep their real voxel positions. Uniform exposure preserves
  their block shading, while a bounded multiplicative depth tint separates
  rear scenery without flattening texture contrast, changing generation, or
  making it interactive.
- Lighting is a derived six-depth volume with independent 0-15 sky and block
  channels. Opaque voxels stop propagation; open cells spread light through
  their six orthogonal neighbors without leaking beyond the rendered depth
  bounds.
- Voxel materials are unlit. The texture atlas remains nearest-neighbor, while
  neighborhood-filtered sky and block levels use a linearly sampled lightmap.
  Each block face receives one uniform shade; interpolation never creates
  gradients inside a face. Day/night changes update the lightmap rather than
  rebuilding chunk meshes.
- Block source art is 16 by 16 pixels with closely spaced material palettes and
  deliberate connected clusters rather than high-frequency procedural noise.
  Grass, wood, ores, leaves, bedrock, and torches retain material-specific face
  and motif rules.
- Ambient occlusion is a subtle quantized multiplier for an entire face.
  Low-poly floor and wall torches keep emission in a small textured cap within
  their chunk-batched material. Their propagated level stays static while one
  shared presentation-only lookup flickers the rendered block light; torches
  never create per-block render entities.
- Sun and moon visuals stay camera-aligned and communicate the clock state.
  They do not drive directional PBR lights or cast smooth real-time shadows.
- Texture packs are versioned data, never executable code. The Bevy-free
  `sidecraft-textures` package owns schema validation, partial-pack fallback,
  deterministic generation, preview composition, and the fixed Bevy-free
  showcase layout shared by the game and editor. The Bevy adapter owns global
  selection and replaces atlas, environment, hotbar, and player-color asset
  contents in place so runtime handles remain stable.
- Exact texture projects are owned by `sidecraft-textures`, not by a UI. Each
  project records its project schema and generator version and stores typed
  exact values. The package also owns every control's semantic data type,
  range, step, predefined choices, and per-material applicability. Flexible
  probabilistic recipes remain a separate CLI format.
- `sidecraft-texture-editor` is a separate composition root. Its document,
  generation, native-file, export, UI, and preview lifecycles remain separate
  modules. It consumes only public `sidecraft-textures` APIs and never imports
  Sidecraft game code. Its preview renders the shared showcase as actual
  nearest-neighbor 3D geometry with editor-local static face, ambient-occlusion,
  sky, and torch shading. Day/night selection is session-only presentation
  state and never changes project data. Preview generation is single-flight
  and revisioned: stale task results are discarded, the last valid scene
  remains visible, and export accepts only the exact revision currently
  displayed. Atlas, mesh, environment, and player-material handles remain
  stable while their asset contents are replaced.
- Front-end menus are a state-driven subsystem under `bevy::ui::menus`.
  Ordinary navigation owns no world lifecycle work: new/load/save actions are
  translated into session commands, while menu-local state changes remain
  inside the menu subsystem. Shared widgets, status text, transition overlays,
  and gameplay HUDs stay in the parent UI composition module. The texture-pack
  picker is a child module because preview staging and restoration have their
  own lifecycle and failure rules.
- Pack switching is allowed only from Settings > Texture Packs. Selecting a
  candidate fully resolves and validates it in memory, then stages its atlas,
  environment, and player palette on the live menu scene. Apply atomically
  replaces `sidecraft.toml` before the active resource changes; leaving without
  applying restores the active pack. A failed apply leaves the prior pack
  active, and an invalid saved selection falls back to the built-in default.
- Front-end states render one fixed voxel showcase through the same meshing,
  atlas, environment, player-palette, and derived-lighting paths as gameplay.
  Its camera framing and fixed-noon presentation state are isolated from the
  saved gameplay camera and day cycle. Loading keeps the showcase; Saving
  retains the actual world behind its overlay.

M4 interaction presentation follows the same separation:

- Mining cracks, target recoil, held-item motion, chips, break and placement
  particles, item interpolation, pickup motion, and crafting pulses are
  presentation only. They never change blocks, inventories, drops, stations,
  simulation queues, or propagated lighting.
- Completion effects consume typed accepted action or rule outcomes. Raw cell
  changes are insufficient because integration, unloading, simulation
  movement, and replacement mutations must not appear as player actions.
- Transient interaction effects use a bounded reusable pool. Exhaustion may
  recycle only cosmetic effects; authoritative world drops are never pooled or
  discarded by presentation limits.
- Natural terrain keeps a cubic silhouette. Crafted and special blocks may
  use restrained chunk-batched geometry while retaining full-cell collision,
  opacity, whole-face light coordinates, and ambient occlusion.
- Texture-pack schema 2 adds current block-item, material-item, and tool
  visuals. Partial current-schema packs may fall back per asset, but schema-1
  packs and projects are rejected rather than adapted.

## Items, inventory, crafting, and stations

M4 introduces Bevy-free `ItemId`, `ItemStack`, `Inventory`, `ToolClass`,
`ToolTier`, `Recipe`, `CraftingGrid`, `DropId`, `WorldDrop`, and
`GameplayCatalog` domain types.

- The composition root embeds checked-in schema-1 gameplay TOML and validates
  it into an immutable catalog before entering a world. This catalog is
  internal content data, not a runtime mod API.
- Render and lighting properties remain in the block definition. Item limits,
  mining hardness, preferred tools, harvest tiers, drops, fuel, repair, and
  recipes belong to the gameplay catalog.
- Catalog validation rejects duplicate or invalid IDs and slugs, dangling
  references, invalid stack limits or durability, malformed recipes, invalid
  counts, and missing block/item visual mappings.
- The application owns the single-player inventory, selected hotbar slot,
  crafting session, world-drop store, and station store. Bevy resources are
  thin wrappers around those types; UI components are never authoritative.
- `BlockState::HOTBAR`, `BlockId::HOTBAR`, block `hotbar_slot` metadata, and the
  infinite Bevy hotbar are deleted when the finite inventory arrives. No
  compatibility aliases remain.

Player actions are atomic across their authoritative owners:

- One application action owner prevalidates the selected stack, inventory,
  station, target, reach, layer, and expected block state.
- It commits the voxel proposal through `WorldMutator` and performs item
  consumption, durability, station, and drop side effects only after the
  proposal is accepted. Those post-commit operations are infallible because
  the same owner retains exclusive mutable access for the whole action.
- Rejected placement consumes nothing. A final durability point still
  completes an accepted break and its harvest before the tool is removed.
- Crafting first simulates the complete cursor/grid/inventory transfer, then
  commits it as one inventory transaction. Failed recipe fill, output
  collection, repair, or close leaves every slot unchanged.
- Typed action and simulation outcomes carry accepted side effects. Loot is
  never inferred from `MutationReport`: player harvest, support-removal torch
  drops, station contents, block movement, and fluid replacement have distinct
  causes and drop policies.

The inventory overlay remains inside `AppState::Playing`. It captures player
movement and gameplay commands, clears mining state, and owns pointer input,
while day time, Avian physics, M3 simulation, item aging, automatic pickup, and
presentation continue. Outside an overlay, `E` opens personal crafting or the
targeted in-reach workbench/furnace; inside an overlay, `E` closes it. `Escape`
closes the overlay before a second press may pause. A workbench or furnace is
revalidated against the current player position and block state. If it
disappears or leaves reach, grid and cursor stacks are returned atomically and
the UI falls back to
personal crafting; genuine overflow becomes a foreground drop with a pickup
delay.

World drops are deterministic simulation state:

- Each drop has a stable ID, validated stack, global foreground cell anchor,
  pickup delay, and remaining active lifetime. Backwall actions project their
  collectible drops to the foreground while keeping visual effects at the
  source depth.
- M3 scheduled work moves unsupported drops downward one cell at a time,
  merges identical compatible stacks in stable order, and expires them after
  6,000 active ticks. Inactive chunks freeze this work.
- Placement never consumes or silently deletes a drop. A stable bounded search
  relocates a covered anchor; if no free cell exists, the authoritative record
  remains collectible.
- Bevy drop entities interpolate pop, fall, bob, attraction, and pickup around
  the authoritative anchor. Avian entities never own item position or expiry.

## World mutation boundary

`WorldMutator` is the only component allowed to change authoritative chunks.
Direct writes by input, UI, simulation, generation, loading, rendering, or
debug systems are forbidden.

Mutations use atomic proposals:

```rust
struct MutationProposal {
    preconditions: Vec<BlockPrecondition>,
    writes: Vec<BlockWrite>,
    priority: MutationPriority,
    source: VoxelPos,
    sequence: u64,
}
```

M3 extends accepted proposals with scheduled tick requests once the logical
clock and persisted queue exist; M1.2 proposals contain only atomic block
writes.

- A proposal is accepted or rejected as a unit. A two-cell sand movement can
  never apply only its source or destination write.
- Simulation evaluates an immutable tick state and produces proposals. It does
  not mutate the world while rules are running.
- Proposals are ordered by priority, source position, then sequence. A proposal
  is accepted only when all preconditions still match and none of its writes
  conflict with an already accepted proposal.
- Player and application commands use the same commit path as simulation.
- A successful commit increments the world revision and returns a
  `MutationReport`.

The report is dispatched into separate dirty sets for rendering, lighting,
collision, persistence, and simulation-neighbor scheduling. The application
owns persistence dirtiness; Bevy owns the remaining derived sets. Each owner
drains its set, and a chunk/layer is rebuilt at most once per rendered frame
regardless of how many cells changed.

## Simulation

`SimulationEngine` evaluates registered `SimulationRule` implementations
against a `WorldView`. `SimulationRegionProvider` supplies the active horizontal
chunks. These are replaceable application/domain boundaries; individual helper
types do not need traits.

### Clock

- World simulation runs at a logical maximum of 20 ticks per second.
- The simulation clock is independent from Bevy's global fixed timestep and
  Avian physics.
- The application accumulates virtual frame time only while `Playing`.
- It executes at most four simulation steps per rendered frame.
- Accumulated wall-time is capped at 200 ms. Under sustained overload, game
  time slows instead of skipping logical ticks or attempting unbounded
  catch-up.
- `world_tick` increments only when a complete simulation step executes.
- Pausing, loading, and time spent outside the process do not advance
  simulation.

### Determinism and activation

Simulation is deterministic for the same seed, commands, logical tick count,
and chunk-activation history.

- Random samples derive from world seed, world tick, global chunk, layer, and
  attempt index. Hash-map iteration order and a mutable global RNG cannot affect
  results.
- Scheduled ticks use the stable key `(due_world_tick, priority,
  global_chunk_x, position, sequence)`.
- Default simulation radius is 3 chunks, render radius is 5, and unload radius
  is 7. All are configuration values.
- Inactive chunks freeze. They receive no random ticks and no wall-clock
  catch-up.
- Pending ticks remain persisted. Overdue work rejoins the normal bounded queue
  when its chunk activates.
- Simulation never force-loads an inactive frontier chunk. Boundary work is
  deferred until that chunk becomes active.
- Optional bounded ticking areas are supplied by the same region provider.
  Future multiplayer support can use the union of player regions without
  changing rule code.

Each tick processes at most 4,096 scheduled proposals plus three deterministic
random samples per active foreground chunk. Excess work stays queued in stable
order. Initial evaluation is serial and serves as the correctness reference.
Chunk-parallel evaluation is allowed only after profiling, uses immutable
inputs, and performs one deterministic merge before committing.

## Threading and asynchronous work

- Generation requests contain the world session identity, generator version,
  seed, and global chunk coordinate.
- Workers return owned chunk data and never access ECS or live world storage.
- Results are committed only if the session still matches and the chunk remains
  requested.
- Requested chunks are prioritized nearest-first. Render and unload hysteresis
  prevent task churn.
- Save workers operate on immutable snapshots captured after a complete
  mutation commit and a whole presentation-clock tick.
- One save coordinator serializes publication, coalesces queued requests, and
  prevents an older completion from replacing a newer manifest.
- The save version covers every authoritative block, simulation, inventory,
  drop, station, and day revision. An escalated pause/exit save completes only
  when the full version still equals the completed snapshot.

## SCW persistence

Only the current SCW schema is accepted. A structural change bumps
`schema_version`, deletes the previous reader and tests, and intentionally
invalidates existing prototype worlds.

The current schema is exactly version 5:

- Every file begins with an `SCW1` envelope.
- Postcard encodes metadata, palettes, and region references.
- Zstandard compresses each payload.
- Palette value `0` means air. Values `1..=255` index at most 255 non-air
  `BlockState` entries.
- Each saved chunk has independent dense row-major, one-byte-per-block
  foreground and backwall arrays.
- Region files are immutable and content-addressed. Unchanged references are
  reused, and the schema version is a domain separator in every region hash.
- Region files are flushed and synced before an atomically replaced manifest
  publishes the new snapshot.
- Obsolete regions are removed only after manifest publication. Failed cleanup
  is harmless and retryable.
- The manifest stores seed, generator version, world dimensions, player state,
  absolute day ticks, and sorted region references.

M2.3 replaces this prototype layout with exact schema version 6, adding
`world_tick`, the next tick sequence, and region-local pending scheduled ticks.
Schema 5 will be invalidated rather than migrated.

M4.1 replaces schema 6 with exact schema 7. Player metadata gains the 36 fixed
inventory slots, selected slot, durability, and transient crafting/cursor
stacks. Regions gain sorted world drops and their pending work, and the
manifest gains the next drop sequence plus all authoritative revisions. A
region exists when it contains drops or scheduled work even if its generated
blocks are unchanged.

Background saves may snapshot an open crafting session exactly. Loading
normalizes transient grid/cursor stacks back into inventory; overflow becomes
world drops and the normalization emits an initial dirty report so it is saved.
Pause, main-menu, and exit transitions close and normalize the overlay before
capturing their escalated snapshot.

M4.2 replaces schema 7 with exact schema 8. Regions additionally store sorted
station positions, slots, fuel, processing progress, and scheduled work. A
station record without its matching block is invalid. Both schemas reject
unknown item IDs, invalid counts or durability, duplicate drop IDs, invalid
anchors or lifetimes, bad ordering, and malformed or stale pending work.

Magic, schema, framing, decompression, palette, dimensions, ordering, checksum,
and runtime fields are validated before data enters the domain. Invalid or
incompatible data produces a visible typed error. It is never silently
regenerated or passed to an older reader.

## Bevy ordering

The application schedule preserves this order:

1. Advance the 20 TPS presentation clock while `Playing`.
2. Accept completed load/generation tasks.
3. Translate input and UI actions into application commands.
4. Commit player inventory, crafting, station, and external world actions.
5. Apply automatic pickups after explicit player actions.
6. Advance zero or more independent world-simulation ticks.
7. Dispatch accepted mutations, side effects, and daylight changes.
8. Rebuild lighting, meshes, colliders, and presentation effects from
   authoritative data and typed outcomes.
9. Capture or queue immutable save snapshots.

The presentation clock stores absolute ticks, wraps its visual phase every
24,000 ticks, and advances moon phase every day. A discrete skylight-level
change dirties loaded foreground render meshes only; it does not rebuild the
light grid, colliders, persistence state, or simulation queues.

Avian continues to use its independently configured fixed physics schedule.
Changing simulation TPS must not change player movement, collision, or camera
behavior.

## Module and interface rules

- A module has one primary responsibility and a narrow public surface.
- Split code when concerns have distinct invariants, dependencies, lifecycles,
  or reasons to change.
- Keep cohesive code together; file length alone is not a reason to split it.
- Tests larger than the unit under test move to a dedicated test module.
- Prefer concrete types inside a boundary. Use a trait for storage,
  simulation-region selection, or another implementation that tests replace.
- Do not create pass-through managers, catch-all utility modules, global
  mutable state, or cyclic plugin dependencies.
- Public domain values validate invariants at construction. Raw indices and
  coordinate conversions stay private to their owning module.
- Performance-sensitive collections and allocation behavior are measured
  before being made more complex.

## Verification contract

- Every domain and application module with logic has unit tests.
- Cross-boundary behavior has headless Bevy integration tests.
- Complete user journeys have end-to-end tests.
- Rendering has automated data/asset tests plus a real-GPU smoke test; brittle
  pixel-perfect snapshots are not required.
- Deterministic tests use explicit seeds and logical tick counts.
- Persistence tests cover interrupted writes, stale async completion, corrupt
  input, negative coordinates, region boundaries, and reload equivalence.
- No milestone is complete while a relevant module is untested or any required
  local verification gate fails.
