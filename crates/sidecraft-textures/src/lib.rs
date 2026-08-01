mod algorithm;
mod code;
mod config;
mod controls;
mod environment;
mod generation;
mod material;
mod pack;
mod palette;
mod project;
mod rng;
mod showcase;

pub use algorithm::{ArtifactMetrics, artifact_metrics};
pub use code::{FORMAT_VERSION, GlobalMode, PackCodeState, decode_pack_code, encode_pack_code};
pub use config::{
    ClusterShape, GenerateOptions, OrePattern, PalettePreset, PatternAlgorithm, PlacementAlgorithm,
    QualityPreset,
};
pub use controls::{
    CLUSTER_DENSITY_LIMITS, CLUSTER_SIZE_LIMITS, CONTRAST_LIMITS, CONTROL_DEFINITIONS,
    ControlChoice, ControlChoiceValue, ControlDataType, ControlDefinition, ControlField, F32Limits,
    GRASS_FRINGE_DEPTH_LIMITS, LEAF_HOLE_DENSITY_LIMITS, LIGHTNESS_LIMITS, MaterialField,
    ORE_BRANCHES_LIMITS, ORE_CENTER_BIAS_LIMITS, ORE_COVERAGE_LIMITS, ORE_THICKNESS_LIMITS,
    SATURATION_LIMITS, SMOOTHING_PASSES_LIMITS, UsizeLimits, VARIANT_STRENGTH_LIMITS,
    control_definition, material_fields, snap_f32,
};
pub use generation::{GeneratedPack, generate_pack, resolve_generated_pack};
pub use pack::{
    BLOCK_SIZE, BlockKind, Face, GENERATOR_VERSION, GenerationManifest, PACK_SCHEMA_VERSION,
    PackError, PackImage, PackManifest, PartialPlayerPalette, PlayerPalette, ResolvedPack,
    TexturePackSummary, USER_PACK_ROOT, VARIANT_COUNT, compose_preview, discover_packs,
    initialize_recipe, load_resolved_pack, read_manifest, validate_pack, write_generated_pack,
    write_preview,
};
pub use project::{
    PROJECT_SCHEMA_VERSION, PackSettings, TextureParameters, TextureProject,
    TypedMaterialOverrides, auto_pack_name, load_texture_project, random_seed, randomized_options,
    save_texture_project, slugify_pack_id,
};
pub use showcase::{SHOWCASE, ShowcaseCell, ShowcaseSpec, ShowcaseTorchMount};
