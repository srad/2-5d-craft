# sidecraft-textures

`sidecraft-textures` is a standalone Rust library and CLI for generating,
validating, resolving, and previewing small pixel-art texture packs. It uses a
fixed SplitMix64-based random stream and records `generator_version = 2`, so an
explicit seed and resolved recipe replay exactly across supported platforms.

The package is independent from Bevy and Sidecraft gameplay types. Sidecraft
consumes its public pack API, but the generator can later be versioned and
published on its own.

## Generate

With no arguments, the CLI chooses a random seed and one coherent safe profile
for the selected pixel-pattern algorithm. It creates a new folder and never
overwrites an existing pack. A pack is assembled in a hidden temporary folder and
moved into place; on Windows a scanner still holding the freshly written PNGs can
refuse that move, so the writer retries briefly and then copies rather than
failing, and every filesystem error names the operation and path that produced
it:

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
  --pattern evenly-varied,cluster-stamps `
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

All surface algorithms produce a Minecraft-inspired full-field patchwork using
the material's four-color ramp. They grow deliberate orthogonal clusters and
reject dominant flat fields, checkerboards, long bands, obvious seams, and
high-frequency noise. `evenly-varied` uses the smallest, most widely distributed
motifs; the other choices favor stamps, cellular areas, broken strata, or short
walks. Ores use separated multitone clusters over the stone field, while leaf
cutouts preserve a connected canopy.

Other commands:

```powershell
cargo run -p sidecraft-textures -- init my-recipe.toml
cargo run -p sidecraft-textures -- validate texture-packs/my-pack
cargo run -p sidecraft-textures -- validate assets/texture-packs/default --complete
cargo run -p sidecraft-textures -- preview texture-packs/my-pack
```

Generate from an exact editor project:

```powershell
cargo run -p sidecraft-textures -- generate `
  --project my-pack.sctex.toml `
  --output texture-packs
```

In project mode, only `--output` may accompany `--project`. The project already
contains the seed, metadata, and every exact control, so recipe, batch,
metadata, seed, global-control, and material override arguments are rejected.

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

## Exact project schema 1

The visual editor and CLI share a human-readable `*.sctex.toml` format:

```toml
project-schema-version = 1
generator-version = 1
seed = 42

[pack]
id = "earth-42"
name = "Earth 42"
author = "Author"

[parameters]
palette = "earthy"
pattern = "cluster-stamps"
placement = "jittered-grid"
cluster-shape = "mixed"
cluster-size = 4
cluster-density = 0.2
smoothing-passes = 1
contrast = 1.06
saturation = 1.0
lightness = -0.02
variant-strength = 4
ore-pattern = "center-growth"
ore-coverage = 0.25
ore-branches = 4
ore-thickness = 2
ore-center-bias = 0.88
leaf-hole-density = 0.22
grass-fringe-depth = 4
quality = "balanced"

[material.stone]
pattern = "broken-strata"
contrast = 1.1
```

This format stores exact typed values and is separate from range/choice recipe
files. Reads deny unknown fields and validate both versions, metadata, finite
numbers, bounds, materials, and whether each material override has an effect.
Writes atomically replace the target file.

Library users can inspect `CONTROL_DEFINITIONS`; every generator control
declares its stable key, label, semantic data type, numeric range and step, or
predefined choice values. `material_fields` declares which controls actually
affect each material. The same definitions drive validation and the editor UI.
`SHOWCASE` supplies a deterministic, Bevy-free fixed-scene contract—including
dimensions, inspection area, block cells, and torch mounts—that the game and
standalone editor render through their own presentation adapters.

Run the standalone editor with:

```powershell
cargo run -p sidecraft-texture-editor
```

See the
[`sidecraft-texture-editor` guide](../sidecraft-texture-editor/README.md).

## Pack codes

A generated pack's ID is a *pack code*: a reversible, compressed encoding of
everything that determined its pixels — the seed, all 19 global controls, and
every material override. Decoding one rebuilds that exact state, so an ID names
a recipe rather than merely labelling a folder.

```powershell
cargo run -p sidecraft-textures -- generate --seed 1785602118
# generated texture-packs/t8cetw9w9d

cargo run -p sidecraft-textures -- generate --code t8cetw9w9d
# byte-identical assets, same ID
```

Codes stay short by never storing what is already implied. An untouched
randomized pack is fully described by its seed, so its code carries nothing
else and runs about 10 characters; changing one control adds a 19-bit mask and
just that value. Every decimal control is quantized onto the step grid its UI
slider already uses, which is what makes the round trip exact rather than
approximate. The largest possible project — every block overriding every
applicable field — still fits well inside the 160-character ID limit.

Because codes are content-derived, two packs share an ID only when they
generate identical pixels, and any edit produces a new one. Passing `--id` or
`--name-prefix` opts back out to a fixed folder name.
