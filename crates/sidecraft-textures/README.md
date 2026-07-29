# sidecraft-textures

`sidecraft-textures` is a standalone Rust library and CLI for generating,
validating, resolving, and previewing small pixel-art texture packs. It uses a
fixed SplitMix64-based random stream and records `generator_version = 1`, so an
explicit seed and resolved recipe replay exactly across supported platforms.

The package is independent from Bevy and Sidecraft gameplay types. Sidecraft
consumes its public pack API, but the generator can later be versioned and
published on its own.

## Generate

With no arguments, the CLI chooses a random seed and safe random values for
every control. It creates a new folder and never overwrites an existing pack:

```powershell
cargo run -p sidecraft-textures --
cargo run -p sidecraft-textures -- generate
```

Replay or tune a pack with explicit controls:

```powershell
cargo run -p sidecraft-textures -- generate `
  --seed 42 `
  --count 3 `
  --palette earthy `
  --pattern cluster-stamps,cellular-clumps `
  --placement poisson-disc `
  --cluster-size 2..5 `
  --ore-pattern center-growth,branching-walk `
  --set stone.contrast=1.04..1.10 `
  --set dirt.pattern=cluster-stamps,short-walks
```

Comma-separated values are sampled as choices. Inclusive `a..b` ranges are
sampled per generated pack. Precedence is safe randomized defaults, an
optional recipe, global CLI controls, then repeatable material `--set`
overrides.

Other commands:

```powershell
cargo run -p sidecraft-textures -- init my-recipe.toml
cargo run -p sidecraft-textures -- validate texture-packs/my-pack
cargo run -p sidecraft-textures -- validate assets/texture-packs/default --complete
cargo run -p sidecraft-textures -- preview texture-packs/my-pack
```

Generation writes into a hidden sibling directory, validates the complete
pack, writes the resolved recipe and preview, and only then renames it into
place. Each tile has at most 32 quality attempts and a deterministic best
candidate fallback.

## Pack schema 1

Every pack contains `pack.toml`. Generated packs also contain
`generation.toml` and a cached 320 by 180 `preview.png`.

Block assets are 16 by 16 RGBA PNGs:

```text
blocks/<material>/<face>_<variant>.png
blocks/<material>/all_<variant>.png
icons/<material>.png
```

Materials are `grass`, `dirt`, `stone`, `coal_ore`, `iron_ore`, `wood`,
`leaves`, `torch`, and `bedrock`. Faces are `side`, `top`, and `bottom`;
variants are `0` through `3`. A face-specific image overrides `all`. Missing
custom images resolve from the complete default pack. Missing icons derive
from the resolved side texture.

Environment paths are fixed:

```text
environment/sun.png          # 32x32
environment/moon_0.png       # 32x32, phases 0..7
environment/stars.png        # 512x256
environment/cloud.png        # 64x24
```

`pack.toml` may override any player color independently:

```toml
schema_version = 1
id = "my-pack"
name = "My Pack"
author = "Author"
description = "Optional description"

[player]
skin = [174, 111, 62]
shirt = [25, 91, 105]
```

IDs, dimensions, file sizes, PNG decoding, transparency, schema versions, and
symlinks are validated. Only leaves, torches, and environment/UI images may
contain transparent pixels.
