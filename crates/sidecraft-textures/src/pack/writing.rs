use super::{PackError, image_io::write_png, validate_pack};
use crate::{GeneratedPack, PackImage};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn write_generated_pack(
    pack: &GeneratedPack,
    output_root: &Path,
) -> Result<PathBuf, PackError> {
    fs::create_dir_all(output_root)?;
    let target = unique_target(output_root, &pack.manifest.id);
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| PackError::Invalid("invalid output folder".into()))?;
    let temporary = output_root.join(format!(".{file_name}.tmp-{}", std::process::id()));
    if temporary.exists() {
        return Err(PackError::Invalid(format!(
            "temporary output already exists: {}",
            temporary.display()
        )));
    }
    fs::create_dir(&temporary)?;
    let result = (|| {
        fs::write(
            temporary.join("pack.toml"),
            toml::to_string_pretty(&pack.manifest)?,
        )?;
        fs::write(
            temporary.join("generation.toml"),
            toml::to_string_pretty(&pack.generation)?,
        )?;
        for (relative, image) in &pack.assets {
            write_png(&temporary.join(relative), image)?;
        }
        write_png(&temporary.join("preview.png"), &pack.preview)?;
        validate_pack(&temporary, true)?;
        fs::rename(&temporary, &target)?;
        Ok(target.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

fn unique_target(output_root: &Path, id: &str) -> PathBuf {
    let direct = output_root.join(id);
    if !direct.exists() {
        return direct;
    }
    for suffix in 2.. {
        let candidate = output_root.join(format!("{id}-{suffix}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

pub fn write_preview(path: &Path, preview: &PackImage) -> Result<(), PackError> {
    if preview.width != 320 || preview.height != 180 {
        return Err(PackError::Invalid(
            "pack previews must be exactly 320x180".into(),
        ));
    }
    write_png(path, preview)
}

pub fn initialize_recipe(path: &Path) -> Result<(), PackError> {
    if path.exists() {
        return Err(PackError::Invalid(format!(
            "{} already exists",
            path.display()
        )));
    }
    fs::write(
        path,
        r#"palette = "earthy"
pattern = "cluster-stamps"
placement = "jittered-grid"
cluster-shape = "mixed"
cluster-size = "2..5"
cluster-density = "0.16..0.24"
smoothing-passes = "0..1"
contrast = "1.02..1.10"
saturation = "0.95..1.08"
lightness = "-0.04..0.01"
variant-strength = "2..5"
ore-pattern = "center-growth,branching-walk,compact-cellular"
ore-coverage = "0.30..0.35"
ore-branches = "3..6"
ore-thickness = "2..3"
ore-center-bias = "0.75..1.0"
leaf-hole-density = "0.02..0.06"
grass-fringe-depth = "3..5"
quality = "balanced"

[material.stone]
pattern = "cluster-stamps,cellular-clumps"

[material.dirt]
pattern = "cluster-stamps,short-walks"
"#,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GenerateOptions, generate_pack, load_resolved_pack};

    #[test]
    fn atomic_writer_never_overwrites_and_round_trips() {
        let directory = tempfile::tempdir().unwrap();
        let pack = generate_pack(&GenerateOptions {
            seed: 41,
            ..Default::default()
        })
        .unwrap();
        let first = write_generated_pack(&pack, directory.path()).unwrap();
        let second = write_generated_pack(&pack, directory.path()).unwrap();
        assert_ne!(first, second);
        validate_pack(&first, true).unwrap();
        assert_eq!(
            load_resolved_pack(&first, None).unwrap().manifest.id,
            "generated-41"
        );
        assert!(fs::read_dir(directory.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with('.')
        }));
    }
}
