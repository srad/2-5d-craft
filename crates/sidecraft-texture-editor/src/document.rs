use bevy::prelude::Resource;
use sidecraft_textures::{TextureProject, random_seed};
use std::path::PathBuf;

#[derive(Debug, Resource)]
pub(crate) struct EditorDocument {
    pub project: TextureProject,
    pub path: Option<PathBuf>,
    saved_snapshot: TextureProject,
    pub revision: u64,
}

impl Default for EditorDocument {
    fn default() -> Self {
        let project = TextureProject::randomized(random_seed());
        Self {
            saved_snapshot: project.clone(),
            project,
            path: None,
            revision: 0,
        }
    }
}

impl EditorDocument {
    pub fn is_dirty(&self) -> bool {
        self.project != self.saved_snapshot
    }

    pub fn mark_changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn mark_saved(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.saved_snapshot = self.project.clone();
    }

    pub fn replace(&mut self, project: TextureProject, path: Option<PathBuf>) {
        self.project = project.clone();
        self.saved_snapshot = project;
        self.path = path;
        self.mark_changed();
    }

    pub fn generate_variation(&mut self, seed: u64) {
        let pack = self.project.pack.clone();
        self.project = TextureProject::randomized(seed);
        self.project.pack = pack;
        self.project.material.clear();
        self.mark_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_state_is_derived_from_saved_snapshot() {
        let mut document = EditorDocument::default();
        assert!(!document.is_dirty());
        document.project.pack.name.push_str(" changed");
        document.mark_changed();
        assert!(document.is_dirty());
        document.mark_saved(PathBuf::from("project.sctex.toml"));
        assert!(!document.is_dirty());
    }

    #[test]
    fn variation_preserves_metadata_and_clears_material_overrides() {
        let mut document = EditorDocument::default();
        document.project.pack.id = "kept-id".into();
        document.project.pack.name = "Kept Name".into();
        document.project.pack.author = "Kept Author".into();
        document.project.material.insert(
            "stone".into(),
            sidecraft_textures::TypedMaterialOverrides {
                contrast: Some(1.1),
                ..Default::default()
            },
        );
        let revision = document.revision;

        document.generate_variation(123);

        assert_eq!(document.project.seed, 123);
        assert_eq!(document.project.pack.id, "kept-id");
        assert_eq!(document.project.pack.name, "Kept Name");
        assert_eq!(document.project.pack.author, "Kept Author");
        assert!(document.project.material.is_empty());
        assert_eq!(document.revision, revision.wrapping_add(1));
        assert!(document.is_dirty());
    }
}
