use bevy::prelude::{IVec2, Vec2};
use sidecraft::{
    BlockGrid, BlockKind, DayCycle, LightGrid, SavedChunk, WORLD_HEIGHT, WorldStore, blank_save,
    generate_chunk, spawn_for_seed, surface_height, validate,
};

#[test]
fn new_world_edit_save_and_reload_lifecycle() {
    let directory = tempfile::tempdir().unwrap();
    let store = WorldStore::new(directory.path());
    let seed = 0x5EED_CAFE;
    let spawn = spawn_for_seed(seed);
    let path = store.new_path(seed).unwrap();
    let initial = blank_save(seed, "End-to-end world".into(), Vec::new(), spawn);

    store.save(&path, &initial).unwrap();
    let loaded = store.load(&path).unwrap();
    assert!(loaded.chunks.is_empty());

    let generated = generate_chunk(seed, 0);
    let mut edited = BlockGrid::new(WORLD_HEIGHT);
    edited.insert_chunk(generated);
    let surface = IVec2::new(0, surface_height(seed, 0));
    let placed = surface + IVec2::Y;
    assert!(edited.remove(surface).is_some());
    assert_eq!(edited.set(placed, BlockKind::Wood), None);

    let mut updated = loaded;
    updated.day_phase = 0.82;
    updated.player.local_x = spawn.x + 2.0;
    updated.player.y = spawn.y + 1.0;
    updated.player.selected_slot = 6;
    let chunk = generate_saved_chunk(&edited, 0);
    updated.chunks = vec![chunk];
    store.save(&path, &updated).unwrap();

    let reloaded = store.load(&path).unwrap();
    validate(&reloaded).unwrap();
    let reconstructed = BlockGrid::from_save(&reloaded);
    assert_eq!(reconstructed.get(surface), None);
    assert_eq!(reconstructed.get(placed), Some(BlockKind::Wood));
    assert_eq!(reloaded.player.selected_slot, 6);
    assert_eq!(
        Vec2::new(reloaded.player.local_x, reloaded.player.y),
        spawn + Vec2::new(2.0, 1.0)
    );
    assert_eq!(reloaded.day_phase, 0.82);
    assert!(
        LightGrid::calculate(&reconstructed)
            .visible_at(&reconstructed, placed)
            .sky
            > 0
    );

    let listed = store.list().unwrap();
    assert_eq!(listed.valid.len(), 1);
    assert!(listed.invalid.is_empty());
    assert_eq!(listed.valid[0].save.name, "End-to-end world");

    let cycle = DayCycle {
        phase: reloaded.day_phase,
        ..Default::default()
    };
    assert!(cycle.daylight() >= 0.15);
    assert!(cycle.light_level() <= 15);
}

fn generate_saved_chunk(grid: &BlockGrid, chunk_x: i32) -> SavedChunk {
    let mut blocks = vec![0; (sidecraft::CHUNK_WIDTH * WORLD_HEIGHT) as usize];
    let start_x = chunk_x * sidecraft::CHUNK_WIDTH;
    for y in 0..WORLD_HEIGHT {
        for local_x in 0..sidecraft::CHUNK_WIDTH {
            if let Some(kind) = grid.get(IVec2::new(start_x + local_x, y)) {
                blocks[(y * sidecraft::CHUNK_WIDTH + local_x) as usize] = kind.code();
            }
        }
    }
    SavedChunk {
        x: i64::from(chunk_x),
        blocks,
    }
}
