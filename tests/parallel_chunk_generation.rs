use bevy::tasks::{AsyncComputeTaskPool, TaskPoolBuilder, block_on};
use sidecraft::{BlockKind, CHUNK_WIDTH, generate_chunk, surface_height};

#[test]
fn chunks_generate_concurrently_without_seams_or_shared_state() {
    let pool = AsyncComputeTaskPool::get_or_init(|| {
        TaskPoolBuilder::new()
            .num_threads(4)
            .thread_name("chunk-test".into())
            .build()
    });
    let seed = 0x0A11_CE55;
    let tasks = (-12..=12)
        .map(|chunk_x| pool.spawn(async move { generate_chunk(seed, chunk_x) }))
        .collect::<Vec<_>>();
    let chunks = tasks.into_iter().map(block_on).collect::<Vec<_>>();

    for chunk in chunks {
        let left_x = chunk.x() * CHUNK_WIDTH;
        let right_x = left_x + CHUNK_WIDTH - 1;
        for world_x in [left_x, right_x] {
            let local_x = world_x.rem_euclid(CHUNK_WIDTH);
            let surface = surface_height(seed, i64::from(world_x));
            let index = (surface * CHUNK_WIDTH + local_x) as usize;
            assert_eq!(
                BlockKind::from_code(chunk.blocks()[index]),
                Some(BlockKind::Grass)
            );
        }
    }
}
