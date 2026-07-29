use super::{
    BLOCK_SIZE, BlockKind, Face, PackError, PackImage, PackManifest, PlayerPalette, ResolvedPack,
    VARIANT_COUNT, block_all_path, block_path, compose_preview, image_io::decode_png,
    validate_pack, validation::read_and_validate_manifest,
};
use std::{collections::BTreeMap, path::Path};

pub fn read_manifest(root: &Path) -> Result<PackManifest, PackError> {
    read_and_validate_manifest(root)
}

pub fn load_resolved_pack(
    default_root: &Path,
    selected_root: Option<&Path>,
) -> Result<ResolvedPack, PackError> {
    validate_pack(default_root, true)?;
    let default_manifest = read_manifest(default_root)?;
    let default_player = PlayerPalette::from_complete(&default_manifest.player)?;
    let (manifest, player) = if let Some(root) = selected_root {
        validate_pack(root, false)?;
        let manifest = read_manifest(root)?;
        (
            manifest.clone(),
            PlayerPalette::resolve(default_player, &manifest.player),
        )
    } else {
        (default_manifest, default_player)
    };

    let mut blocks = BTreeMap::new();
    for block in BlockKind::ALL {
        for face in Face::ALL {
            for variant in 0..VARIANT_COUNT {
                blocks.insert(
                    (block, face, variant),
                    resolve_block(default_root, selected_root, block, face, variant)?,
                );
            }
        }
    }
    let mut icons = BTreeMap::new();
    for block in BlockKind::HOTBAR {
        let image = resolve_optional(
            default_root,
            selected_root,
            &format!("icons/{}.png", block.slug()),
            BLOCK_SIZE,
            BLOCK_SIZE,
        )?
        .unwrap_or_else(|| blocks[&(block, Face::Side, 0)].clone());
        icons.insert(block, image);
    }
    let mut resolved = ResolvedPack {
        manifest,
        player,
        blocks,
        icons,
        sun: resolve_required(default_root, selected_root, "environment/sun.png", 32, 32)?,
        moons: (0..8)
            .map(|phase| {
                resolve_required(
                    default_root,
                    selected_root,
                    &format!("environment/moon_{phase}.png"),
                    32,
                    32,
                )
            })
            .collect::<Result<Vec<_>, _>>()?,
        stars: resolve_required(
            default_root,
            selected_root,
            "environment/stars.png",
            512,
            256,
        )?,
        cloud: resolve_required(default_root, selected_root, "environment/cloud.png", 64, 24)?,
        preview: PackImage::solid(320, 180, [20, 18, 16, 255]),
    };
    resolved.preview = compose_preview(&resolved);
    Ok(resolved)
}

fn resolve_block(
    default_root: &Path,
    selected_root: Option<&Path>,
    block: BlockKind,
    face: Face,
    variant: usize,
) -> Result<PackImage, PackError> {
    if let Some(root) = selected_root {
        for path in [
            block_path(root, block, face, variant),
            block_all_path(root, block, variant),
        ] {
            if path.is_file() {
                return decode_png(&path, BLOCK_SIZE, BLOCK_SIZE);
            }
        }
    }
    for path in [
        block_path(default_root, block, face, variant),
        block_all_path(default_root, block, variant),
    ] {
        if path.is_file() {
            return decode_png(&path, BLOCK_SIZE, BLOCK_SIZE);
        }
    }
    Err(PackError::Invalid(format!(
        "default pack cannot resolve {} {} variant {variant}",
        block.slug(),
        face.slug()
    )))
}

fn resolve_required(
    default_root: &Path,
    selected_root: Option<&Path>,
    relative: &str,
    width: u32,
    height: u32,
) -> Result<PackImage, PackError> {
    resolve_optional(default_root, selected_root, relative, width, height)?.ok_or_else(|| {
        PackError::Invalid(format!("default pack cannot resolve required {relative}"))
    })
}

fn resolve_optional(
    default_root: &Path,
    selected_root: Option<&Path>,
    relative: &str,
    width: u32,
    height: u32,
) -> Result<Option<PackImage>, PackError> {
    if let Some(root) = selected_root {
        let path = root.join(relative);
        if path.is_file() {
            return Ok(Some(decode_png(&path, width, height)?));
        }
    }
    let path = default_root.join(relative);
    if path.is_file() {
        return Ok(Some(decode_png(&path, width, height)?));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GenerateOptions, generate_pack, write_generated_pack};
    use std::fs;

    #[test]
    fn partial_pack_falls_back_per_asset_and_field() {
        let directory = tempfile::tempdir().unwrap();
        let default = generate_pack(&GenerateOptions {
            seed: 7,
            ..Default::default()
        })
        .unwrap();
        let default_path = write_generated_pack(&default, directory.path()).unwrap();
        let partial = directory.path().join("partial");
        fs::create_dir(&partial).unwrap();
        let manifest = PackManifest {
            schema_version: 1,
            id: "partial".into(),
            name: "Partial".into(),
            author: "Test".into(),
            description: String::new(),
            player: Default::default(),
        };
        fs::write(
            partial.join("pack.toml"),
            toml::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let resolved = load_resolved_pack(&default_path, Some(&partial)).unwrap();
        let baseline = load_resolved_pack(&default_path, None).unwrap();
        assert_eq!(resolved.manifest.id, "partial");
        assert_eq!(
            resolved.block(BlockKind::Stone, Face::Side, 0),
            baseline.block(BlockKind::Stone, Face::Side, 0)
        );
        assert_eq!(resolved.player, baseline.player);
    }
}
