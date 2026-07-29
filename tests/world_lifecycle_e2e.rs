use bevy::prelude::Vec2;
use sidecraft::{
    adapters::storage::ScwRepository,
    application::{
        WorldRepository, WorldState, blank_snapshot, break_block, place_block, validate_snapshot,
    },
    domain::{
        BlockState, ChunkLayer, DEPTH_SLICES, DayCycle, LightVolume, VoxelPos, WORLD_HEIGHT,
        generated_voxel, spawn_for_seed, surface_height, world_to_chunk,
    },
};

#[test]
fn new_world_edit_save_and_reload_lifecycle() {
    let directory = tempfile::tempdir().unwrap();
    let repository = ScwRepository::new(directory.path());
    let seed = 0x5EED_CAFE;
    let spawn = spawn_for_seed(seed);
    let id = repository.next_available_id(seed).unwrap();
    let initial = blank_snapshot(seed, "End-to-end world".into(), Vec::new(), spawn, 1);

    repository.save(&id, &initial).unwrap();
    let loaded = repository.load(&id).unwrap();
    assert!(loaded.chunks.is_empty());

    let mut world = WorldState::from_snapshot(&loaded).state;
    let surface = VoxelPos::foreground(0, surface_height(seed, 0));
    let placed = VoxelPos {
        y: surface.y + 1,
        ..surface
    };
    assert!(!break_block(&mut world, surface).unwrap().report.is_empty());
    assert!(
        !place_block(&mut world, placed, BlockState::WOOD)
            .unwrap()
            .report
            .is_empty()
    );

    let mut updated = loaded;
    updated.day_time_ticks = 123_456_789;
    updated.player.local_x = spawn.x + 2.0;
    updated.player.y = spawn.y + 1.0;
    updated.player.selected_slot = 6;
    updated.chunks = world.snapshot_chunks();
    repository.save(&id, &updated).unwrap();

    let reloaded = repository.load(&id).unwrap();
    validate_snapshot(&reloaded).unwrap();
    let reconstructed = WorldState::from_snapshot(&reloaded).state;
    assert_eq!(reconstructed.view().block(surface), None);
    assert_eq!(reconstructed.view().block(placed), Some(BlockState::WOOD));
    assert_eq!(reloaded.player.selected_slot, 6);
    assert_eq!(
        Vec2::new(reloaded.player.local_x, reloaded.player.y),
        spawn + Vec2::new(2.0, 1.0)
    );
    assert_eq!(reloaded.day_time_ticks, 123_456_789);
    let view = reconstructed.view();
    let (min_x, max_x) = view.loaded_x_bounds().unwrap();
    let light = LightVolume::calculate(min_x, max_x, WORLD_HEIGHT, DEPTH_SLICES, |x, y, depth| {
        if depth == 0 && view.contains_chunk(ChunkLayer::foreground(world_to_chunk(x))) {
            view.block(VoxelPos::foreground(x, y))
        } else {
            generated_voxel(seed, x, y, depth)
        }
    });
    assert!(light.get(placed.global_x, placed.y + 1, 0).sky > 0);

    let listed = repository.list().unwrap();
    assert_eq!(listed.valid.len(), 1);
    assert!(listed.invalid.is_empty());
    assert_eq!(listed.valid[0].name, "End-to-end world");

    let cycle = DayCycle::from_ticks(reloaded.day_time_ticks);
    assert!((0.0..=1.0).contains(&cycle.daylight()));
    assert!((4..=15).contains(&cycle.sky_light_level()));
}
