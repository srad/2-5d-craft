use super::{PackError, TexturePackSummary, read_manifest, validate_pack};
use std::{fs, path::Path};

pub fn discover_packs(
    default_root: &Path,
    custom_root: &Path,
) -> Result<Vec<TexturePackSummary>, PackError> {
    let mut packs = vec![summarize_pack(default_root)];
    if custom_root.is_dir() {
        let mut entries = fs::read_dir(custom_root)?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| summarize_pack(&entry.path()))
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
        packs.extend(entries);
    }
    Ok(packs)
}

fn summarize_pack(root: &Path) -> TexturePackSummary {
    match read_manifest(root) {
        Ok(manifest) => TexturePackSummary {
            id: manifest.id,
            name: manifest.name,
            author: manifest.author,
            path: root.to_path_buf(),
            validation_error: validate_pack(root, false)
                .err()
                .map(|error| error.to_string()),
        },
        Err(error) => TexturePackSummary {
            id: root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("invalid")
                .to_owned(),
            name: "Invalid texture pack".into(),
            author: "Unknown".into(),
            path: root.to_path_buf(),
            validation_error: Some(error.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GenerateOptions, generate_pack, write_generated_pack};

    #[test]
    fn discovery_keeps_default_first() {
        let directory = tempfile::tempdir().unwrap();
        let default = generate_pack(&GenerateOptions {
            seed: 2,
            id: Some("default".into()),
            ..Default::default()
        })
        .unwrap();
        let default_path = write_generated_pack(&default, directory.path()).unwrap();
        let packs = discover_packs(&default_path, directory.path()).unwrap();
        assert_eq!(packs[0].id, "default");
    }
}
