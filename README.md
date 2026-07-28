# Sidecraft

Sidecraft is a 2.5D, side-view living-builder prototype built with Rust, Bevy
0.19, and Avian2D. It turns the original non-working skeleton into a playable
foundation with generated terrain, movement, mining, placement, lighting, and
durable world saves.

The world is generated and rendered as real 3D voxels, while movement and
interaction remain side-on. The intended result is a polished, low-poly world
with an editable foreground, an editable backwall, and generated scenery behind
them—not a flat 2D tile map or a free-camera Minecraft clone.

The product direction is a systemic living-builder focused on homesteading,
crafting and technology, exploration, climate, fluids, gravity, and plant
growth. [The Blockheads](https://theblockheads.net/) and its
[early mobile presentation](https://toucharcade.com/2012/04/23/chopper-developer-majic-jungle-announces-2d-minecraft-inspired-ios-title-the-blockheads/)
are important references.

## Project status

The current baseline is playable but remains a foundation. Only the foreground
slice is editable; the rear slices are generated visual depth. Block
simulation, a persistent backwall, finite inventory, crafting, ecology, audio,
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

### Controls

| Input | Action |
| --- | --- |
| `A` / `D` or arrow keys | Move |
| `Space` | Jump |
| Left mouse, held | Mine the targeted block |
| Right mouse | Place the selected block |
| `1` through `8` | Select a hotbar block |
| `Escape` | Pause or resume |
| `F12` | Save a gameplay screenshot |

The placement cursor has a five-block reach and refuses occupied cells,
unsupported torches, and blocks overlapping the player.

## What works

- Deterministic, horizontally unbounded terrain in dense 32 by 80 chunks. Four
  correlated depth slices form a real voxel volume with grass, dirt, stone,
  ores, caves, trees, leaves, and an unbreakable bedrock floor.
- The player and all interaction remain locked to the front slice while the
  generated rear slices supply the visible 2.5D depth.
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
- Eight block hotbar, including placeable light-emitting torches.
- Sky and torch flood lighting, directional sunlight, moving clouds, bloom,
  HDR tonemapping, four-sample MSAA, and a 180-second day/night cycle.
- A runtime-built, guttered texture atlas with deterministic material variants,
  nearest-neighbor sampling, face shading, and depth shading.
- Main menu, world selection, HUD, and pause/save controls. Button actions are
  triggered only by Bevy's `Interaction::Pressed` state.
- Autosave after changed world data, save on pause, and save-aware window
  closing.

## Save format

Each world is a versioned `.scw` package directory:

1. An atomically replaced `manifest.scw` containing Postcard metadata and the
   sorted content-addressed region index.
2. Region `.scw` files containing at most 64 horizontal chunks each.
3. A four-byte `SCW1` magic header and Zstandard compression on every file.
4. A sorted Postcard block palette and chunk coordinates followed by dense,
   row-major, one-byte-per-block arrays; zero means air and non-zero values
   index the palette.

The loader rejects wrong headers, malformed compression, trailing metadata,
invalid dimensions, unsorted or duplicate palettes and indexes, invalid block
indices, mismatched region hashes, oversized data, and invalid runtime fields.
Changed region files are flushed and synced before the manifest is atomically
replaced; unchanged content-addressed regions are reused. Only chunks changed
by the player are persisted, while untouched terrain is regenerated exactly
from the seed. Player position is stored as an `i64` chunk plus local `f32`
offset, alongside hotbar selection, timestamps, and day phase. Autosave
compression and I/O run on Bevy's I/O task pool.

Schema 2 is intentionally a fresh format. Legacy JSON metadata saves are
neither loaded nor migrated. Top-level single-file SCW1 worlds are also
rejected: schema 2 worlds are package directories only. M2 replaces the
current save model with exact-schema SCW version 3.

## Architecture

The library points dependencies inward from adapters to application services
and then to a Bevy-free domain:

```text
src/
  domain/          Blocks, dense chunks, generation, targeting, lighting
  application/     World/session state, streaming plans, snapshots, save policy
  adapters/
    bevy/           ECS, Avian, rendering, input, UI, session and save systems
    storage/        SCW packages, validation, compression, atomic manifests
  lib.rs            AppState, concrete adapter selection and system ordering
  main.rs           Desktop window and Bevy startup
```

`WorldState` owns authoritative resident chunks, global persisted chunk
identity, and revision tracking. Bevy's `WorldPresentation` owns only scene and
derived-state dirtiness. `WorldRepository` isolates logical world identity from
filesystem paths, while `SaveCoordinator` serializes and escalates save intent.
Menus emit session commands; they do not load or save worlds directly.

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

- Unit tests in every gameplay module cover block definitions, generation,
  persistence validation, lighting, controller math/ECS behavior, targeting,
  pixel assets, camera behavior, and menu button semantics.
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
$env:SIDECRAFT_SCREENSHOT = "sidecraft-e2e.png"
cargo run
```

Without those environment variables, startup follows the normal main-menu
flow. The rendered smoke test still requires a real window and graphics
adapter.

## Current scope

This is a playable prototype foundation, not a feature-complete game. The
initial roadmap covers architecture, a persistent backwall, deterministic
fluids/gravity/plants, builder progression, ecology, presentation, and
large-world performance. Multiplayer, free 3D movement, colony automation,
boss-centric progression, a public mod API, touch-first UI, and all legacy
migration paths are deferred beyond that roadmap.

## License

MIT
