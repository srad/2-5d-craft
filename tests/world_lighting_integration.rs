use sidecraft::{
    application::{ChunkSnapshot, blank_snapshot},
    domain::{
        BlockChunk, BlockGrid, CHUNK_WIDTH, LightGrid, VoxelPos, WORLD_HEIGHT, WorldMutator,
        generate_chunk, spawn_for_seed,
    },
};

#[test]
fn persisted_chunks_reconstruct_the_same_light_field() {
    let mut grid = BlockGrid::new(WORLD_HEIGHT);
    let chunks = (-1..=1).map(|x| generate_chunk(41, x)).collect::<Vec<_>>();
    for chunk in &chunks {
        WorldMutator::new(&mut grid).integrate_chunk(chunk.clone());
    }
    let before = LightGrid::calculate(&grid.view());
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
    let after = LightGrid::calculate(&reconstructed.view());

    for y in 0..WORLD_HEIGHT {
        for x in -CHUNK_WIDTH..CHUNK_WIDTH * 2 {
            let position = VoxelPos::foreground(i64::from(x), y);
            assert_eq!(after.get(position), before.get(position));
        }
    }
}
