# Sidecraft

Sidecraft is a 2.5D, side-view living-builder prototype built with Rust, Bevy
0.19, and Avian2D. It turns the original non-working skeleton into a playable
foundation with generated terrain, movement, mining, placement, lighting, and
durable world saves.

The world is generated and rendered as real 3D voxels, while movement and
interaction remain side-on. A fixed orthographic camera uses a restrained
10-degree yaw and 14-degree downward pitch to retain narrow top and right-face
depth cues without losing the side-view composition. Depth-aware haze keeps the
editable foreground visually dominant. The intended result is a polished,
low-poly world with an editable foreground, an editable backwall, and generated
scenery behind them—not a flat 2D tile map or a free-camera Minecraft clone.

The product direction is a systemic living-builder focused on homesteading,
crafting and technology, exploration, climate, fluids, gravity, and plant
growth. [The Blockheads](https://theblockheads.net/) and its
[early mobile presentation](https://toucharcade.com/2012/04/23/chopper-developer-majic-jungle-announces-2d-minecraft-inspired-ios-title-the-blockheads/)
are important references.

## Project status

The current baseline is playable but remains a foundation. The foreground and
backwall are editable persistent layers; four deeper slices remain generated
visual scenery. Block simulation, finite inventory, crafting, ecology, audio,
and final presentation are not implemented yet.

- [`ROADMAP.md`](ROADMAP.md) is the cross-session implementation plan.
- [`ARCHITECTURE.md`](ARCHITECTURE.md) is the binding architecture contract.

Prototype saves and internal APIs may be invalidated at any time. Obsolete
formats and implementations are deleted rather than migrated.

## Play

```powershell
cargo run
```

The first build is large because Bevy's rendering stack must be compiled.
Worlds are stored under `worlds/` relative to the working directory.
The global texture-pack selection is stored in schema-1 `sidecraft.toml`.

### Controls

| Input | Action |
| --- | --- |
| `A` / `D` or arrow keys | Move |
| `Space` | Jump |
| Left mouse, held | Mine the targeted block |
| Right mouse | Place the selected block |
| `Tab` | Toggle foreground/backwall editing |
| `1` through `8` | Select a hotbar block |
| `Escape` | Pause or resume |
| `F12` | Save a gameplay screenshot |

The placement cursor has a five-block reach and refuses occupied cells and
unsupported torches. Foreground placement also refuses blocks overlapping the
player; backwall blocks never collide with the player.

## What works

- Deterministic, horizontally unbounded terrain in dense 32 by 80 chunks. Six
  correlated depth slices form a real voxel volume with grass, dirt, stone,
  ores, caves, trees, leaves, and an unbreakable bedrock floor.
- The player and collision remain locked to the foreground. Interaction can
  switch between the foreground and persistent non-colliding backwall while
  four generated rear slices supply additional 2.5D depth.
- An `i64` global chunk coordinate plus a frequently rebased local `f32`
  simulation origin, so rendering and physics remain precise far from spawn.
- Lazy multi-threaded chunk generation through Bevy's async compute pool.
  Nearby chunks are prioritized; distant chunks and their render/physics
  entities are unloaded.
- Chunk-batched opaque, cutout, and emissive six-face meshes plus merged
  two-dimensional compound colliders instead of one render and physics entity
  per block. Empty render layers are omitted.
- Avian2D kinematic movement, acceleration, jumping, gravity, grounded checks,
  collision sliding, world bounds, and fall recovery.
- Hardness-based mining and adjacent-face placement.
- Seven block hotbar, including warm, slowly flickering level-14 torches that
  mount on floors or either side of solid blocks.
- Minecraft-style six-depth lighting with separate 0-15 sky and block
  channels, six-neighbor propagation, opaque-cell occlusion, neighborhood-
  filtered whole-face shading, subtle quantized ambient occlusion, light-aware
  player colors, and targeted chunk refresh after world changes.
- Moving clouds, bloom, HDR tonemapping, four-sample MSAA, and an exact
  20-minute, 24,000-tick day/night cycle with a visible pixel-art sun, stars,
  eight persistent moon phases, stronger cyclic color moods, and cap-emissive
  chunk-batched low-poly floor and wall torches.
- A versioned texture-pack pipeline with four 16-by-16 variants per face,
  closely spaced earthy palettes, connected pixel-art clusters, semantic
  grass, bark, ring, ore, leaf, bedrock, and torch motifs, environment art,
  hotbar icons, player colors, partial-pack fallback, validation, generator
  previews, and live in-game previewing. Nearest-neighbor textures retain
  linearly sampled lighting, directional face shading, uniform depth exposure,
  and bounded depth haze.
- A state-driven Main Menu > Settings > Texture Packs flow, world selection,
  loading/saving overlays, HUD, and pause controls. Front-end screens share a
  live fixed voxel showcase, use translucent left-side panels, and show the
  game version in the lower-right corner. Button actions are triggered only by
  Bevy's `Interaction::Pressed` state.
- Autosave after changed world data, save on pause, and save-aware window
  closing.

## Save format

Each world is a versioned `.scw` package directory:

1. An atomically replaced `manifest.scw` containing Postcard metadata and the
   sorted content-addressed region index.
2. Region `.scw` files containing at most 64 horizontal chunks each.
3. A four-byte `SCW1` magic header and Zstandard compression on every file.
4. A sorted Postcard block palette and chunk coordinates followed by paired
   foreground/backwall dense row-major arrays; zero means air and non-zero
   values index the palette.

The loader rejects wrong headers, malformed compression, trailing metadata,
invalid dimensions, unsorted or duplicate palettes and indexes, invalid block
indices, mismatched region hashes, oversized data, and invalid runtime fields.
Changed region files are flushed and synced before the manifest is atomically
replaced; unchanged content-addressed regions are reused. Only chunks changed
by the player are persisted, while untouched terrain is regenerated exactly
from the seed. Player position is stored as an `i64` chunk plus local `f32`
offset, alongside hotbar selection, timestamps, and absolute day ticks.
Clock-only progress autosaves once per real minute of active play (1,200 day
ticks); pause and exit preserve any whole-tick change. Compression and I/O run
on Bevy's I/O task pool.

Schema 5 is intentionally a fresh two-layer format. Schema 4, legacy JSON
metadata saves, and top-level single-file SCW1 worlds are neither loaded nor
migrated; schema 5 worlds are package directories only. M2.3 will replace this
format directly with exact schema 6 when persisted simulation state is added.

## Texture packs

The complete built-in pack is under `assets/texture-packs/default`. User packs
are folders under `texture-packs/`; partial packs inherit each missing face,
icon, environment image, or player color from the default. Select packs from
`SETTINGS > TEXTURE PACKS` to preview them immediately on the live menu scene.
`APPLY` validates and persists the selection before updating stable render
handles; `BACK` restores the active pack. The generator's `preview.png` remains
an external pack-development artifact rather than a runtime thumbnail.

The generator is a separate publishable package:

```powershell
cargo run -p sidecraft-textures --
cargo run -p sidecraft-textures -- generate --seed 42 --pattern short-walks
cargo run -p sidecraft-textures -- init texture-generator.toml
cargo run -p sidecraft-textures -- validate texture-packs/generated-42
```

No generator options produces one randomized safe pack and prints its seed and
resolved choices. Exact values, comma-separated choices, inclusive ranges,
recipes, batches, and repeatable `--set material.field=value` overrides are
supported. See the
[`sidecraft-textures` package guide](crates/sidecraft-textures/README.md) and
the [earthy example recipe](crates/sidecraft-textures/recipes/earthy.toml).

The standalone visual editor exposes every typed generator parameter beside a
shaded 3D game-scene preview with Day/Night switching, coherent one-click
variation generation, in-game-equivalent framing, automatic regeneration,
exact versioned `*.sctex.toml` projects, and export of the displayed result:

```powershell
cargo run -p sidecraft-texture-editor
```

On Windows, `.\editor.ps1` launches the release editor directly.

It depends only on the publishable texture package and Bevy/egui presentation,
not on Sidecraft gameplay. See the
[`sidecraft-texture-editor` guide](crates/sidecraft-texture-editor/README.md).

## Architecture

The library points dependencies inward from adapters to application services
and then to a Bevy-free domain:

```text
src/
  domain/          Blocks, dense chunks, generation, targeting, lighting, time
  application/     World/session state, streaming plans, snapshots, save policy
  adapters/
    bevy/           ECS, Avian, rendering, showcase, menu/UI, session and save
                    systems
    storage/        SCW packages, validation, compression, atomic manifests
  lib.rs            AppState, concrete adapter selection and system ordering
  main.rs           Desktop window and Bevy startup
```

`WorldState` owns authoritative resident chunks, global persisted chunk
identity, and revision tracking. All block changes pass through
`WorldMutator`, which atomically commits proposals and reports effective cell
and chunk changes. The application records persistence dirtiness from those
reports; Bevy coalesces independent render, lighting, collision, and simulation
dirty sets before rebuilding derived state. `WorldPresentation` owns only scene
entities. `WorldRepository` isolates logical world identity from filesystem
paths, while `SaveCoordinator` serializes and escalates save intent. The menu
subsystem owns front-end navigation and staged texture previews; it emits
session commands for world lifecycle work and never loads or saves worlds
directly.

The dependency direction, mutation boundary, simulation contract, threading
rules, SCW schema policy, and module-boundary rules are defined in
[`ARCHITECTURE.md`](ARCHITECTURE.md). Their staged implementation is tracked in
[`ROADMAP.md`](ROADMAP.md).

## Verification

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo build --release
git diff --check
```

The test suite has three levels:

- Unit tests in every gameplay module cover block definitions, atomic world
  mutations, derived-state dispatch, generation, persistence validation,
  lighting, controller math/ECS behavior, targeting, pixel assets, camera
  behavior, and menu button semantics.
- `tests/world_lighting_integration.rs` verifies that world reconstruction
  preserves the complete lighting field.
- `tests/parallel_chunk_generation.rs` generates positive and negative chunks
  concurrently and verifies deterministic edge data.
- `tests/world_lifecycle_e2e.rs` creates a world, saves it, reloads it, edits
  blocks and player state, overwrites it atomically, lists it, and reconstructs
  it again.

Automated tests are headless. A deterministic rendered smoke-test hook can
bypass the menu, enter a newly created world, wait for chunk streaming, and
capture the gameplay frame:

```powershell
$env:SIDECRAFT_AUTOSTART = "1"
$env:SIDECRAFT_AUTOCAPTURE = "1"
$env:SIDECRAFT_TEST_SEED = "9"
$env:SIDECRAFT_TEST_DAY_TICKS = "18000"
$env:SIDECRAFT_TEST_TORCH_FIXTURE = "1"
$env:SIDECRAFT_TEST_TORCH_INTENSITY_PERCENT = "108"
$env:SIDECRAFT_SCREENSHOT = "sidecraft-e2e.png"
cargo run
```

`SIDECRAFT_TEST_SEED` and `SIDECRAFT_TEST_DAY_TICKS` are honored only with
autostart and accept decimal `u64` values. The seed makes terrain and spawn
repeatable. The requested clock remains frozen for the test session, so
captures can target sunrise (`0`), noon (`6000`), sunset (`12000`), midnight
(`18000`), or a later moon phase exactly. Invalid or missing overrides retain
the normal random seed and advancing clock. Without autostart,
`SIDECRAFT_AUTOCAPTURE` captures the live main menu instead. The rendered smoke
test still requires a real window and graphics adapter.
`SIDECRAFT_TEST_TORCH_FIXTURE` is also autostart-only; it places supported floor
and wall torches through the normal mutation boundary.
`SIDECRAFT_TEST_TORCH_INTENSITY_PERCENT` is autostart-only and accepts `92..=108`
to freeze the visual flicker at a deterministic intensity for GPU comparison.

## Current scope

This is a playable prototype foundation, not a feature-complete game. The
initial roadmap covers architecture, a persistent backwall, deterministic
fluids/gravity/plants, builder progression, ecology, presentation, and
large-world performance. Multiplayer, free 3D movement, colony automation,
boss-centric progression, a public mod API, touch-first UI, and all legacy
migration paths are deferred beyond that roadmap.

## License

MIT
