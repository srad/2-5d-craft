use super::{
    BlockKind, Face, PACK_SCHEMA_VERSION, PackError, PackImage, PackManifest, PlayerPalette,
    VARIANT_COUNT, block_all_path, block_path, environment_paths, icon_path, image_io::decode_png,
};
use std::{fs, path::Path};

pub fn validate_pack(root: &Path, require_complete: bool) -> Result<(), PackError> {
    reject_symlinks(root)?;
    let manifest = read_and_validate_manifest(root)?;
    for block in BlockKind::ALL {
        for face in Face::ALL {
            for variant in 0..VARIANT_COUNT {
                let exact = block_path(root, block, face, variant);
                let all = block_all_path(root, block, variant);
                if require_complete && !exact.is_file() && !all.is_file() {
                    return Err(PackError::Invalid(format!(
                        "default pack is missing {} {} variant {variant}",
                        block.slug(),
                        face.slug()
                    )));
                }
                for path in [exact, all] {
                    if path.is_file() {
                        let image = decode_png(&path, 16, 16)?;
                        validate_block_alpha(block, &path, &image)?;
                    }
                }
            }
        }
    }
    for (path, width, height) in environment_paths(root) {
        if path.is_file() {
            decode_png(&path, width, height)?;
        } else if require_complete {
            return Err(PackError::Invalid(format!(
                "default pack is missing {}",
                path.display()
            )));
        }
    }
    for block in BlockKind::HOTBAR {
        let path = icon_path(root, block);
        if path.is_file() {
            decode_png(&path, 16, 16)?;
        }
    }
    if require_complete {
        PlayerPalette::from_complete(&manifest.player)?;
    }
    let preview = root.join("preview.png");
    if preview.is_file() {
        decode_png(&preview, 320, 180)?;
    }
    Ok(())
}

pub(super) fn read_and_validate_manifest(root: &Path) -> Result<PackManifest, PackError> {
    let manifest: PackManifest = toml::from_str(&fs::read_to_string(root.join("pack.toml"))?)?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &PackManifest) -> Result<(), PackError> {
    if manifest.schema_version != PACK_SCHEMA_VERSION {
        return Err(PackError::Invalid(format!(
            "unsupported pack schema {}, expected {PACK_SCHEMA_VERSION}",
            manifest.schema_version
        )));
    }
    validate_id(&manifest.id)?;
    if manifest.name.trim().is_empty() || manifest.name.len() > 80 {
        return Err(PackError::Invalid(
            "pack name must contain 1 to 80 characters".into(),
        ));
    }
    if manifest.author.trim().is_empty() || manifest.author.len() > 80 {
        return Err(PackError::Invalid(
            "pack author must contain 1 to 80 characters".into(),
        ));
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<(), PackError> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || !id.as_bytes()[0].is_ascii_alphanumeric()
    {
        return Err(PackError::Invalid(format!(
            "invalid pack id {id:?}; use lowercase letters, digits, and hyphens"
        )));
    }
    Ok(())
}

fn validate_block_alpha(block: BlockKind, path: &Path, image: &PackImage) -> Result<(), PackError> {
    if !matches!(block, BlockKind::Leaves | BlockKind::Torch)
        && image.pixels.chunks_exact(4).any(|pixel| pixel[3] != 255)
    {
        return Err(PackError::Invalid(format!(
            "opaque block texture has transparency: {}",
            path.display()
        )));
    }
    Ok(())
}

fn reject_symlinks(root: &Path) -> Result<(), PackError> {
    if !root.is_dir() {
        return Err(PackError::Invalid(format!(
            "{} is not a texture-pack directory",
            root.display()
        )));
    }
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                return Err(PackError::Invalid(format!(
                    "texture packs may not contain symlinks: {}",
                    entry.path().display()
                )));
            }
            if file_type.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_ids_and_opaque_transparency_are_rejected() {
        assert!(validate_id("../bad").is_err());
        let image = PackImage::solid(16, 16, [1, 2, 3, 0]);
        assert!(validate_block_alpha(BlockKind::Stone, Path::new("stone.png"), &image).is_err());
    }
}
