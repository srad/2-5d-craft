# Sidecraft

Sidecraft is a small 2.5D, side-view block sandbox built with Rust, Bevy 0.19,
and Avian2D. It turns the original non-working skeleton into a playable
foundation with generated terrain, movement, mining, placement, lighting, and
durable world saves.

The visual direction is a compact, pixel-art world with orthographic depth,
inspired by early mobile side-view building games rather than a full 3D
Minecraft camera.

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
neither loaded nor migrated. The loader still accepts schema-compatible
single-file SCW1 worlds.

## Architecture

The library is split into focused Bevy plugins and pure data layers:

```text
src/
  block.rs        Block catalog and gameplay properties
  world.rs        BlockGrid, generation, floating origin, chunk scenes
  persistence.rs  SCW1 regions, validation, compression, atomic manifests
  lighting.rs     Sky and torch flood lighting, day cycle
  rendering.rs    Procedural atlas, PBR materials, depth-slice voxel meshes
  player.rs       Avian controller, hotbar, animation
  interaction.rs  Cursor targeting, mining, placement
  camera.rs       Oblique orthographic camera and environment presentation
  ui.rs           State-driven menus, HUD, autosave, exit handling
  lib.rs          AppState and plugin composition
  main.rs         Desktop window and Bevy startup
```

`BlockGrid` contains only resident dense chunks and is authoritative for active
gameplay. Chunk meshes, merged colliders, and light data are derived from it.
Modified chunks retain global `i64` identities when their local simulation
coordinates are rebased or their live scenes unload.

## Verification

```powershell
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
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

This is a solid prototype foundation, not a feature-complete game. Crafting,
finite inventory stacks, mobs, fluids, audio, touch controls, and multiplayer
are deliberately out of scope for now.

## License

MIT
