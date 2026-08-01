use bevy::prelude::Resource;
use sidecraft_textures::{PackError, TextureProject, auto_pack_name, random_seed};
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

    /// Rerolls every parameter from a new seed.
    ///
    /// Carrying the whole `pack` block across is what used to freeze the ID at the first seed the
    /// editor ever saw; the ID is derived now, so it follows the new state on its own. The name
    /// needs the same care: an untouched `Generated <seed>` is refreshed to match the new seed,
    /// while a name the user typed is theirs and is preserved.
    pub fn generate_variation(&mut self, seed: u64) {
        let author = self.project.pack.author.clone();
        let name = (!self.project.is_auto_named()).then(|| self.project.pack.name.clone());
        self.project = TextureProject::randomized(seed);
        self.project.pack.author = author;
        if let Some(name) = name {
            self.project.pack.name = name;
        }
        self.project.material.clear();
        self.mark_changed();
    }

    /// Changes only the seed, refreshing an untouched name to match.
    pub fn reseed(&mut self, seed: u64) {
        let rename = self.project.is_auto_named();
        self.project.seed = seed;
        if rename {
            self.project.pack.name = auto_pack_name(seed);
        }
        self.mark_changed();
    }

    /// Replaces the project with the one a pack code names, keeping the current labels.
    pub fn restore_from_pack_id(&mut self, code: &str) -> Result<(), PackError> {
        let restored =
            TextureProject::from_pack_id(code, &self.project.pack.name, &self.project.pack.author)?;
        self.project = restored;
        self.mark_changed();
        Ok(())
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

    /// The reported follow-up: the name stayed pinned to the first seed too.
    #[test]
    fn an_untouched_name_follows_every_reroll() {
        let mut document = EditorDocument::default();
        assert!(document.project.is_auto_named());
        let mut seen = std::collections::BTreeSet::new();
        for seed in 1..=10u64 {
            document.generate_variation(seed);
            assert_eq!(document.project.pack.name, format!("Generated {seed}"));
            assert!(seen.insert(document.project.pack.name.clone()));
        }
        document.reseed(777);
        assert_eq!(document.project.pack.name, "Generated 777");
    }

    #[test]
    fn a_name_the_user_typed_survives_a_reroll() {
        let mut document = EditorDocument::default();
        document.project.pack.name = "Mossy Caves".into();
        document.generate_variation(5);
        assert_eq!(document.project.pack.name, "Mossy Caves");
        document.reseed(6);
        assert_eq!(document.project.pack.name, "Mossy Caves");
    }

    #[test]
    fn variation_keeps_labels_and_clears_material_overrides() {
        let mut document = EditorDocument::default();
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
        assert_eq!(document.project.pack.name, "Kept Name");
        assert_eq!(document.project.pack.author, "Kept Author");
        assert!(document.project.material.is_empty());
        assert_eq!(document.revision, revision.wrapping_add(1));
        assert!(document.is_dirty());
    }

    /// The reported bug: every "Generate" reused the first seed's ID, so exports collided.
    #[test]
    fn every_variation_produces_a_different_pack_id() {
        let mut document = EditorDocument::default();
        let mut seen = std::collections::BTreeSet::new();
        for seed in 1..=25u64 {
            document.generate_variation(seed);
            let id = document
                .project
                .pack_id()
                .expect("a variation is encodable");
            assert!(seen.insert(id.clone()), "seed {seed} reused the id {id}");
        }
    }

    #[test]
    fn a_pack_id_restores_the_project_it_names() {
        let mut document = EditorDocument::default();
        document.generate_variation(4_242);
        document.project.parameters.contrast = 1.23;
        document.project.material.insert(
            "coal_ore".into(),
            sidecraft_textures::TypedMaterialOverrides {
                ore_branches: Some(7),
                ..Default::default()
            },
        );
        let code = document.project.pack_id().unwrap();
        let expected = document.project.clone();

        document.generate_variation(9_999);
        assert_ne!(document.project, expected);

        document.restore_from_pack_id(&code).unwrap();
        assert_eq!(document.project.seed, expected.seed);
        assert_eq!(document.project.parameters, expected.parameters);
        assert_eq!(document.project.material, expected.material);
        assert_eq!(document.project.pack_id().unwrap(), code);
    }

    #[test]
    fn a_mistyped_pack_id_leaves_the_project_alone() {
        let mut document = EditorDocument::default();
        let before = document.project.clone();
        assert!(document.restore_from_pack_id("not-a-code").is_err());
        assert_eq!(document.project, before);
    }
}
