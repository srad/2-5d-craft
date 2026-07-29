use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use sidecraft_textures::{
    PackError, ResolvedPack, TexturePackSummary, discover_packs, load_resolved_pack,
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const DEFAULT_PACK_ROOT: &str = "assets/texture-packs/default";
const CUSTOM_PACK_ROOT: &str = "texture-packs";
const CONFIG_PATH: &str = "sidecraft.toml";
const CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SidecraftConfig {
    schema_version: u32,
    texture_pack: String,
}

impl Default for SidecraftConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            texture_pack: "default".into(),
        }
    }
}

#[derive(Resource)]
pub(crate) struct TexturePackCatalog {
    pub(crate) active: ResolvedPack,
    pub(crate) packs: Vec<TexturePackSummary>,
    pub(crate) diagnostic: String,
    default_root: PathBuf,
    custom_root: PathBuf,
}

#[derive(Resource)]
pub(crate) struct TexturePackPreview(pub(crate) ResolvedPack);

impl TexturePackCatalog {
    pub(crate) fn active_id(&self) -> &str {
        &self.active.manifest.id
    }

    pub(crate) fn refresh(&mut self) {
        match discover_packs(&self.default_root, &self.custom_root) {
            Ok(packs) => self.packs = deduplicate_default(packs, &self.default_root),
            Err(error) => self.diagnostic = format!("Could not scan texture packs: {error}"),
        }
    }

    pub(crate) fn resolve(&self, id: &str) -> Result<ResolvedPack, PackError> {
        if id == "default" {
            return load_resolved_pack(&self.default_root, None);
        }
        let summary = self
            .packs
            .iter()
            .find(|pack| pack.id == id)
            .ok_or_else(|| PackError::Invalid(format!("texture pack {id:?} was not found")))?;
        if let Some(error) = &summary.validation_error {
            return Err(PackError::Invalid(error.clone()));
        }
        load_resolved_pack(&self.default_root, Some(&summary.path))
    }

    pub(crate) fn apply(&mut self, id: &str) -> Result<(), PackError> {
        let resolved = self.resolve(id)?;
        write_config(&SidecraftConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            texture_pack: id.to_owned(),
        })?;
        self.active = resolved;
        self.diagnostic.clear();
        Ok(())
    }
}

#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct TexturePackChanged;

pub(crate) struct TexturePackPlugin;

impl Plugin for TexturePackPlugin {
    fn build(&self, app: &mut App) {
        let catalog = load_initial_catalog().unwrap_or_else(|error| {
            panic!("built-in texture pack failed to load: {error}");
        });
        let preview = TexturePackPreview(catalog.active.clone());
        app.insert_resource(catalog)
            .insert_resource(preview)
            .add_message::<TexturePackChanged>();
    }
}

fn load_initial_catalog() -> Result<TexturePackCatalog, PackError> {
    let default_root = PathBuf::from(DEFAULT_PACK_ROOT);
    let custom_root = PathBuf::from(CUSTOM_PACK_ROOT);
    let packs = deduplicate_default(discover_packs(&default_root, &custom_root)?, &default_root);
    let config = read_config().unwrap_or_default();
    let mut diagnostic = String::new();
    let selected = if config.schema_version == CONFIG_SCHEMA_VERSION {
        config.texture_pack
    } else {
        diagnostic = format!(
            "Unsupported sidecraft.toml schema {}; using the default texture pack.",
            config.schema_version
        );
        "default".into()
    };
    let selected_root = packs
        .iter()
        .find(|pack| pack.id == selected && pack.path != default_root)
        .map(|pack| pack.path.as_path());
    let active = if selected == "default" {
        load_resolved_pack(&default_root, None)?
    } else if let Some(root) = selected_root {
        match load_resolved_pack(&default_root, Some(root)) {
            Ok(pack) => pack,
            Err(error) => {
                diagnostic =
                    format!("Texture pack {selected:?} is invalid ({error}); using the default.");
                load_resolved_pack(&default_root, None)?
            }
        }
    } else {
        diagnostic = format!("Texture pack {selected:?} was not found; using the default.");
        load_resolved_pack(&default_root, None)?
    };
    Ok(TexturePackCatalog {
        active,
        packs,
        diagnostic,
        default_root,
        custom_root,
    })
}

fn deduplicate_default(
    packs: Vec<TexturePackSummary>,
    default_root: &Path,
) -> Vec<TexturePackSummary> {
    packs
        .into_iter()
        .enumerate()
        .filter(|(index, pack)| *index == 0 || (pack.path != default_root && pack.id != "default"))
        .map(|(_, pack)| pack)
        .collect()
}

fn read_config() -> Result<SidecraftConfig, PackError> {
    read_config_at(Path::new(CONFIG_PATH))
}

fn read_config_at(path: &Path) -> Result<SidecraftConfig, PackError> {
    match fs::read_to_string(path) {
        Ok(contents) => toml::from_str(&contents).map_err(PackError::from),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(SidecraftConfig::default())
        }
        Err(error) => Err(error.into()),
    }
}

fn write_config(config: &SidecraftConfig) -> Result<(), PackError> {
    write_config_at(Path::new(CONFIG_PATH), config)
}

fn write_config_at(path: &Path, config: &SidecraftConfig) -> Result<(), PackError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(toml::to_string_pretty(config)?.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| PackError::Io(error.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_defaults_to_the_builtin_pack() {
        let config = SidecraftConfig::default();
        assert_eq!(config.schema_version, 1);
        assert_eq!(config.texture_pack, "default");
    }

    #[test]
    fn default_deduplication_keeps_first_entry() {
        let path = PathBuf::from("default");
        let summary = TexturePackSummary {
            id: "default".into(),
            name: "Default".into(),
            author: "Test".into(),
            path: path.clone(),
            validation_error: None,
        };
        assert_eq!(
            deduplicate_default(vec![summary.clone(), summary], &path).len(),
            1
        );
    }

    #[test]
    fn configuration_is_atomically_replaceable() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sidecraft.toml");
        write_config_at(&path, &SidecraftConfig::default()).unwrap();
        let changed = SidecraftConfig {
            schema_version: 1,
            texture_pack: "alternate".into(),
        };
        write_config_at(&path, &changed).unwrap();
        assert_eq!(read_config_at(&path).unwrap().texture_pack, "alternate");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
