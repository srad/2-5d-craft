mod discovery;
mod format;
mod image_io;
mod loading;
mod preview;
mod validation;
mod writing;

pub use discovery::discover_packs;
pub use format::{
    BLOCK_SIZE, BlockKind, Face, GENERATOR_VERSION, GenerationManifest, PACK_SCHEMA_VERSION,
    PackError, PackImage, PackManifest, PartialPlayerPalette, PlayerPalette, ResolvedPack,
    TexturePackSummary, VARIANT_COUNT,
};
pub use loading::{load_resolved_pack, read_manifest};
pub use preview::compose_preview;
pub(crate) use validation::validate_metadata;
pub use validation::validate_pack;
pub use writing::{initialize_recipe, write_generated_pack, write_preview};

use std::path::{Path, PathBuf};

/// Folder the game scans for user-installed packs, relative to the working
/// directory. The editor points its export dialog here so a pack lands where the
/// game will actually find it: the built-in pack sits under
/// `assets/texture-packs/default`, which is the folder people otherwise pick, and
/// nothing there is ever scanned.
pub const USER_PACK_ROOT: &str = "texture-packs";

fn block_path(root: &Path, block: BlockKind, face: Face, variant: usize) -> PathBuf {
    root.join(format!(
        "blocks/{}/{}_{}.png",
        block.slug(),
        face.slug(),
        variant
    ))
}

fn block_all_path(root: &Path, block: BlockKind, variant: usize) -> PathBuf {
    root.join(format!("blocks/{}/all_{variant}.png", block.slug()))
}

fn icon_path(root: &Path, block: BlockKind) -> PathBuf {
    root.join(format!("icons/{}.png", block.slug()))
}

fn environment_paths(root: &Path) -> Vec<(PathBuf, u32, u32)> {
    let mut paths = vec![
        (root.join("environment/sun.png"), 32, 32),
        (root.join("environment/stars.png"), 512, 256),
        (root.join("environment/cloud.png"), 64, 24),
    ];
    paths.extend((0..8).map(|phase| (root.join(format!("environment/moon_{phase}.png")), 32, 32)));
    paths
}
