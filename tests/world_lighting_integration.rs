use sidecraft::{
    application::{ChunkSnapshot, blank_snapshot},
    domain::{
        BlockChunk, BlockGrid, CHUNK_WIDTH, ChunkLayer, DEPTH_SLICES, LightVolume, VoxelPos,
        WORLD_HEIGHT, WorldMutator, generate_chunk, generated_voxel, spawn_for_seed,
        world_to_chunk,
    },
};

fn calculate_light(seed: u64, grid: &BlockGrid) -> LightVolume {
    let view = grid.view();
    let (min_x, max_x) = view.loaded_x_bounds().unwrap();
    LightVolume::calculate(min_x, max_x, WORLD_HEIGHT, DEPTH_SLICES, |x, y, depth| {
        if depth == 0 && view.contains_chunk(ChunkLayer::foreground(world_to_chunk(x))) {
            view.block(VoxelPos::foreground(x, y))
        } else {
            generated_voxel(seed, x, y, depth)
        }
    })
}

#[test]
fn persisted_chunks_reconstruct_the_same_light_field() {
    let mut grid = BlockGrid::new(WORLD_HEIGHT);
    let chunks = (-1..=1).map(|x| generate_chunk(41, x)).collect::<Vec<_>>();
    for chunk in &chunks {
        WorldMutator::new(&mut grid).integrate_chunk(chunk.clone());
    }
    let before = calculate_light(41, &grid);
    let save = blank_snapshot(
        41,
        "Lighting integration".into(),
        chunks
            .iter()
            .map(|chunk| ChunkSnapshot {
                x: chunk.x(),
                blocks: chunk.blocks().to_vec(),
            })
            .collect(),
        spawn_for_seed(41),
        1,
    );
    let mut reconstructed = BlockGrid::new(WORLD_HEIGHT);
    for chunk in save.chunks {
        WorldMutator::new(&mut reconstructed)
            .integrate_chunk(BlockChunk::from_dense(chunk.x, chunk.blocks).unwrap());
    }
    let after = calculate_light(41, &reconstructed);

    for depth in 0..DEPTH_SLICES {
        for y in 0..WORLD_HEIGHT {
            for x in -CHUNK_WIDTH..CHUNK_WIDTH * 2 {
                assert_eq!(
                    after.get(i64::from(x), y, depth),
                    before.get(i64::from(x), y, depth)
                );
            }
        }
    }
}
