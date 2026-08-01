# Sidecraft Texture Pack Editor

The editor is a standalone desktop application for designing exact
`sidecraft-textures` projects. It depends on the Bevy-free
`sidecraft-textures` package, not on the Sidecraft game crate.

```powershell
cargo run -p sidecraft-texture-editor
```

The resizable left panel exposes pack metadata, the seed, every global
generator control, and material-specific overrides. The right side shows a
shaded 3D voxel scene using the same 48 by 27 showcase, six depth slices,
camera angle, and 36 by 20 inspection area as the game. Day is the default;
the view-only Day/Night switch changes the environment and static voxel
shading without changing the project. Textures remain nearest-neighbor, and
the preview uses no PBR lights, HDR, bloom, or smooth texture filtering.
Editing automatically regenerates after a 200 ms debounce. The last valid
preview remains visible while new work runs or when a parameter is invalid.

The toolbar supports:

- **New**: creates safe randomized parameters and clears material overrides.
- **Open**, **Save**, and **Save As**: read and write exact `*.sctex.toml`
  projects.
- **Regenerate**: repeats the current exact project.
- **New Seed**: changes only the seed.
- **Generate**: creates a coherent randomized parameter profile, clears material
  overrides, and immediately shows the newest candidate. The pack ID is derived,
  so every press yields a new one, and an untouched `Generated <seed>` name
  follows along. A name you typed yourself is kept, as is the author.
- **Export Pack**: writes the exact displayed result to a new validated pack
  folder without overwriting an existing pack. The dialog opens on `texture-packs/`
  — the only folder the game scans — creating it if it is missing, and afterwards
  returns to whichever folder the last export used.

Unsaved New, Open, and close actions offer Save, Discard, and Cancel. Project
replacement is blocked while an export is running. Save uses an atomic sibling
temporary file, and a failed or cancelled dialog leaves the document
unchanged. A failed open, save, or export raises a dialog naming the operation
and path, with the details available to copy; the status bar alone was too easy
to miss.

## Pack ID

The ID is not typed: it is a reversible pack code derived from the seed,
parameters, and material overrides, recomputed as you edit. Copy it to record a
result, and paste one back into the ID box to rebuild exactly the project it
names. Because it tracks content, exporting twice never collides unless the two
packs really are identical.

## Project files

`*.sctex.toml` is a deterministic, exact document rather than a probabilistic
generator recipe. It records both the project schema and generator version.
Unknown fields, unsupported versions, invalid metadata, non-finite numbers,
out-of-range values, unknown materials, and overrides that do not affect the
selected material are rejected.

The flexible recipe format remains a separate CLI feature and is intentionally
not imported by the editor.

## Architecture

The application is split by lifecycle and dependency:

```text
document.rs       project, variation, path, saved snapshot, and revision
generation.rs     debounced single-flight generation and stale-result rejection
file_ops.rs       native dialogs, save/open/close commands, and export task
preview/
  mod.rs          preview asset lifecycle and revision coordination
  camera.rs       isolated 3D/egui cameras, framing, and DPI-aware viewport
  scene.rs        voxel/player geometry, atlas, and stable asset updates
  lighting.rs     static day/night, torch, face, and ambient-occlusion shading
  environment.rs  camera-aligned sky and environment pack art
ui/
  mod.rs          editor layout, toolbar, status, and confirmation flow
  controls.rs     typed global and material controls
main.rs           plugin selection and system ordering
```

Control definitions live in `sidecraft-textures`; each definition owns its key,
label, semantic data type, numeric range and step, or predefined choices. The
editor consumes those definitions instead of duplicating generator bounds.
Pixel Pattern includes **Evenly Varied**, a restrained, evenly distributed
small-mark style suited to classic voxel block faces.
The shared showcase layout also lives in that Bevy-free package, so the editor
matches the game without depending on Sidecraft gameplay or renderer modules.
