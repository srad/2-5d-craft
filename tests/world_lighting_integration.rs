use bevy::prelude::IVec2;
use sidecraft::{
    BlockGrid, CHUNK_WIDTH, LightGrid, SavedChunk, WORLD_HEIGHT, blank_save, generate_chunk,
};

#[test]
fn persisted_chunks_reconstruct_the_same_light_field() {
    let mut grid = BlockGrid::new(WORLD_HEIGHT);
    let chunks = (-1..=1).map(|x| generate_chunk(41, x)).collect::<Vec<_>>();
    for chunk in &chunks {
        grid.insert_chunk(chunk.clone());
    }
    let before = LightGrid::calculate(&grid);
    let save = blank_save(
        41,
        "Lighting integration".into(),
        chunks
            .iter()
            .map(|chunk| SavedChunk {
                x: i64::from(chunk.x()),
                blocks: chunk.blocks().to_vec(),
            })
            .collect(),
        grid.safe_spawn(),
    );
    let reconstructed = BlockGrid::from_save(&save);
    let after = LightGrid::calculate(&reconstructed);

    for y in 0..WORLD_HEIGHT {
        for x in -CHUNK_WIDTH..CHUNK_WIDTH * 2 {
            let coordinate = IVec2::new(x, y);
            assert_eq!(after.get(coordinate), before.get(coordinate));
        }
    }
}
