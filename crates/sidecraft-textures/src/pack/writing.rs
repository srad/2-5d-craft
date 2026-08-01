use super::{PackError, image_io::write_png, validate_pack};
use crate::{GeneratedPack, PackImage};
use std::{
    fs, io,
    path::{Path, PathBuf},
    thread::sleep,
    time::Duration,
};

pub fn write_generated_pack(
    pack: &GeneratedPack,
    output_root: &Path,
) -> Result<PathBuf, PackError> {
    fs::create_dir_all(output_root).map_err(at("create directory", output_root))?;
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
    fs::create_dir(&temporary).map_err(at("create directory", &temporary))?;
    let result = (|| {
        let manifest = temporary.join("pack.toml");
        fs::write(&manifest, toml::to_string_pretty(&pack.manifest)?)
            .map_err(at("write", &manifest))?;
        let generation = temporary.join("generation.toml");
        fs::write(&generation, toml::to_string_pretty(&pack.generation)?)
            .map_err(at("write", &generation))?;
        for (relative, image) in &pack.assets {
            write_png(&temporary.join(relative), image)?;
        }
        write_png(&temporary.join("preview.png"), &pack.preview)?;
        validate_pack(&temporary, true)?;
        move_into_place(&temporary, &target)?;
        Ok(target.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

/// Tags an IO failure with the call that produced it and the path it touched.
fn at(operation: &'static str, path: &Path) -> impl FnOnce(io::Error) -> PackError {
    let path = path.to_path_buf();
    move |source| PackError::IoAt {
        operation,
        path,
        source,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placement {
    Renamed,
    Copied,
}

/// Moves the finished pack directory into place.
///
/// `fs::rename` is `MoveFileExW` on Windows, which refuses to move a directory
/// while a handle inside it is still open without share-delete — which is what a
/// virus scanner does to the ~100 PNGs written moments earlier, and it surfaces
/// as a bare "Access is denied. (os error 5)". Cargo, npm and git absorb the same
/// race by retrying. Should the handles outlive the retry window we copy instead:
/// slower and not atomic, but the export lands rather than failing outright.
fn move_into_place(temporary: &Path, target: &Path) -> Result<Placement, PackError> {
    const ATTEMPTS: u32 = 10;
    const BACKOFF: Duration = Duration::from_millis(50);

    let mut last = match fs::rename(temporary, target) {
        Ok(()) => return Ok(Placement::Renamed),
        Err(error) => error,
    };
    for _ in 1..ATTEMPTS {
        // Only a denial is worth waiting on. An occupied target never clears, so
        // retrying that would postpone the failure by a second and nothing more.
        if last.kind() != io::ErrorKind::PermissionDenied {
            return Err(at("rename", temporary)(last));
        }
        sleep(BACKOFF);
        match fs::rename(temporary, target) {
            Ok(()) => return Ok(Placement::Renamed),
            Err(error) => last = error,
        }
    }
    if last.kind() != io::ErrorKind::PermissionDenied {
        return Err(at("rename", temporary)(last));
    }
    // Windows also reports a denial when the destination directory exists, which
    // is the one case the fallback must not paper over: copying would merge into
    // somebody else's pack. Exports never overwrite each other.
    if target.exists() {
        return Err(PackError::Invalid(format!(
            "export target already exists: {}",
            target.display()
        )));
    }
    copy_tree(temporary, target)?;
    // Whatever still holds the source open also blocks deleting it. The pack is
    // already in place by now, so a surviving temporary directory is not worth
    // failing an otherwise finished export over.
    let _ = fs::remove_dir_all(temporary);
    Ok(Placement::Copied)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), PackError> {
    fs::create_dir_all(destination).map_err(at("create directory", destination))?;
    for entry in fs::read_dir(source).map_err(at("read directory", source))? {
        let entry = entry.map_err(at("read directory", source))?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if entry.file_type().map_err(at("inspect", &from))?.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            fs::copy(&from, &to).map_err(at("copy", &from))?;
        }
    }
    Ok(())
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
        // The ID is a pack code, so it names the generation state rather than just the seed.
        let id = load_resolved_pack(&first, None).unwrap().manifest.id;
        assert_eq!(crate::decode_pack_code(&id).unwrap().seed, 41);
        assert!(fs::read_dir(directory.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with('.')
        }));
    }

    #[test]
    fn io_failures_name_the_operation_and_the_path() {
        let error = at("rename", Path::new("C:/packs/.pack.tmp"))(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Access is denied. (os error 5)",
        ));
        let text = error.to_string();
        assert!(text.starts_with("rename "), "{text}");
        assert!(text.contains(".pack.tmp"), "{text}");
        assert!(text.contains("Access is denied"), "{text}");
    }

    /// Windows refuses to move a directory that still holds an open handle, which
    /// is how a virus scanner turns a finished export into "os error 5". The copy
    /// fallback is the only reason such an export still lands.
    #[cfg(windows)]
    #[test]
    fn a_locked_file_falls_back_to_copying_the_pack_into_place() {
        use std::os::windows::fs::OpenOptionsExt;

        let directory = tempfile::tempdir().unwrap();
        let temporary = directory.path().join(".pack.tmp");
        fs::create_dir_all(temporary.join("blocks")).unwrap();
        fs::write(temporary.join("pack.toml"), "id = \"locked\"").unwrap();
        let locked_path = temporary.join("blocks/stone.png");
        fs::write(&locked_path, b"pretend PNG").unwrap();

        // FILE_SHARE_READ alone models a scanner exactly: it reads the file while
        // withholding FILE_SHARE_DELETE, and it is that missing delete share which
        // makes Windows refuse to move the containing directory. A plain
        // File::open grants delete sharing, so the rename would simply succeed and
        // leave the fallback untested; share mode 0 goes too far the other way and
        // blocks the fallback's own read.
        const FILE_SHARE_READ: u32 = 0x0000_0001;
        let locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&locked_path)
            .unwrap();

        let target = directory.path().join("locked");
        assert_eq!(
            move_into_place(&temporary, &target).unwrap(),
            Placement::Copied,
            "the rename should have been refused and the copy fallback used"
        );
        assert!(target.join("pack.toml").is_file());
        assert!(target.join("blocks/stone.png").is_file());
        drop(locked);
    }
}
