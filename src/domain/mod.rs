mod block;
mod generation;
mod lighting;
mod targeting;
mod world;

pub use block::{BlockDef, BlockId, BlockState};
pub use generation::{
    generate_chunk, generated_voxel, spawn_for_seed, stable_hash, surface_height,
    surface_height_at_depth,
};
pub use lighting::{DayCycle, LightCell, LightGrid};
pub use targeting::{BlockTarget, target_from_ray, tile_overlaps_player};
pub use world::{
    BlockChunk, BlockGrid, BlockPrecondition, BlockWrite, CellChange, ChunkChange, ChunkLayer,
    MutationBatchResult, MutationPriority, MutationProposal, MutationRejection, MutationReport,
    ProposalOutcome, VoxelCell, VoxelLayer, VoxelPos, WorldMutator, WorldView,
    chunk_is_representable, world_to_chunk,
};

pub const WORLD_HEIGHT: i32 = 80;
pub const CHUNK_WIDTH: i32 = 32;
pub const DEPTH_SLICES: u8 = 4;
pub const GENERATOR_VERSION: u32 = 2;

pub(crate) use generation::generate_chunk_at;
