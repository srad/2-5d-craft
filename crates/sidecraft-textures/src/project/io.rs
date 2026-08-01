use super::TextureProject;
use crate::PackError;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub fn load_texture_project(path: &Path) -> Result<TextureProject, PackError> {
    let project: TextureProject = toml::from_str(&fs::read_to_string(path)?)?;
    project.validate()?;
    Ok(project)
}

pub fn save_texture_project(project: &TextureProject, path: &Path) -> Result<PathBuf, PackError> {
    project.validate()?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let parent = parent.unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(toml::to_string_pretty(project)?.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| PackError::Io(error.error))?;
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TextureProject, generate_pack, random_seed};

    #[test]
    fn project_round_trip_replays_identical_assets() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("earth.sctex.toml");
        let project = TextureProject::randomized(random_seed());
        let before = generate_pack(&project.to_generate_options().unwrap()).unwrap();
        save_texture_project(&project, &path).unwrap();
        let reopened = load_texture_project(&path).unwrap();
        let after = generate_pack(&reopened.to_generate_options().unwrap()).unwrap();
        assert_eq!(project, reopened);
        assert_eq!(before, after);
    }

    #[test]
    fn unknown_fields_and_versions_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("bad.sctex.toml");
        fs::write(
            &path,
            r#"project-schema-version = 99
generator-version = 1
seed = 1
surprise = true

[pack]
id = "bad"
name = "Bad"
author = "Test"

[parameters]
palette = "earthy"
pattern = "cluster-stamps"
placement = "jittered-grid"
cluster-shape = "mixed"
cluster-size = 4
cluster-density = 0.2
smoothing-passes = 1
contrast = 1.0
saturation = 1.0
lightness = 0.0
variant-strength = 4
ore-pattern = "center-growth"
ore-coverage = 0.32
ore-branches = 4
ore-thickness = 2
ore-center-bias = 0.88
leaf-hole-density = 0.04
grass-fringe-depth = 4
quality = "balanced"
"#,
        )
        .unwrap();
        assert!(load_texture_project(&path).is_err());
    }

    #[test]
    fn atomic_save_replaces_an_existing_project() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("replace.sctex.toml");
        let mut project = TextureProject::randomized(1);
        save_texture_project(&project, &path).unwrap();
        project.pack.name = "Replacement".into();
        save_texture_project(&project, &path).unwrap();
        assert_eq!(
            load_texture_project(&path).unwrap().pack.name,
            "Replacement"
        );
    }
}
