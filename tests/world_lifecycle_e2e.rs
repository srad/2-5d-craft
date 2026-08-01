use bevy::prelude::Vec2;
use sidecraft::{
    adapters::storage::ScwRepository,
    application::{
        WorldRepository, WorldState, blank_snapshot, break_block, place_block, validate_snapshot,
    },
    domain::{
        BlockState, CHUNK_WIDTH, ChunkLayer, DEPTH_SLICES, DayCycle, LightVolume, MutationPriority,
        ScheduledTick, VoxelLayer, VoxelPos, WORLD_HEIGHT, generate_chunk, generated_voxel,
        spawn_for_seed, surface_height, world_to_chunk,
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
    let overlap_y = (1..WORLD_HEIGHT)
        .find(|y| {
            generated_voxel(seed, 0, *y, 0).is_some_and(BlockState::breakable)
                && generated_voxel(seed, 0, *y, 1).is_some_and(BlockState::breakable)
        })
        .unwrap();
    let overlapping_foreground = VoxelPos::foreground(0, overlap_y);
    let overlapping_backwall = VoxelPos::backwall(0, overlap_y);
    break_block(&mut world, overlapping_foreground).unwrap();
    break_block(&mut world, overlapping_backwall).unwrap();
    place_block(&mut world, overlapping_foreground, BlockState::WOOD).unwrap();
    place_block(&mut world, overlapping_backwall, BlockState::DIRT).unwrap();

    let mut updated = loaded;
    updated.day_time_ticks = 123_456_789;
    updated.player.local_x = spawn.x + 2.0;
    updated.player.y = spawn.y + 1.0;
    updated.player.selected_slot = 6;
    updated.chunks = world.snapshot_chunks();
    updated.world_tick = 24_601;
    updated.next_tick_sequence = 3;
    let owed = [
        ScheduledTick {
            due_world_tick: 24_605,
            priority: MutationPriority::PLAYER,
            position: placed,
            sequence: 1,
            expected: Some(BlockState::WOOD),
        },
        ScheduledTick {
            due_world_tick: 24_700,
            priority: MutationPriority::PLAYER,
            position: overlapping_backwall,
            sequence: 2,
            expected: None,
        },
    ];
    updated
        .chunks
        .iter_mut()
        .find(|chunk| chunk.x == 0)
        .expect("the edited chunk is saved")
        .pending_ticks = owed.to_vec();
    repository.save(&id, &updated).unwrap();

    let reloaded = repository.load(&id).unwrap();
    validate_snapshot(&reloaded).unwrap();
    assert_eq!(reloaded.world_tick, 24_601);
    assert_eq!(reloaded.next_tick_sequence, 3);
    assert_eq!(
        reloaded
            .chunks
            .iter()
            .find(|chunk| chunk.x == 0)
            .map(|chunk| chunk.pending_ticks.as_slice()),
        Some(owed.as_slice())
    );
    let reconstructed = WorldState::from_snapshot(&reloaded).state;
    assert_eq!(reconstructed.view().block(surface), None);
    assert_eq!(reconstructed.view().block(placed), Some(BlockState::WOOD));
    assert_eq!(
        reconstructed.view().block(overlapping_foreground),
        Some(BlockState::WOOD)
    );
    assert_eq!(
        reconstructed.view().block(overlapping_backwall),
        Some(BlockState::DIRT)
    );
    assert_eq!(reloaded.player.selected_slot, 6);
    assert_eq!(
        Vec2::new(reloaded.player.local_x, reloaded.player.y),
        spawn + Vec2::new(2.0, 1.0)
    );
    assert_eq!(reloaded.day_time_ticks, 123_456_789);
    let view = reconstructed.view();
    let (min_x, max_x) = view.loaded_x_bounds().unwrap();
    let light = LightVolume::calculate(min_x, max_x, WORLD_HEIGHT, DEPTH_SLICES, |x, y, depth| {
        if let Some(layer) = VoxelLayer::from_persistent_depth(depth)
            && view.contains_chunk(ChunkLayer::new(world_to_chunk(x), layer))
        {
            view.block(VoxelPos::new(x, y, layer))
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

#[test]
fn distant_edits_survive_unload_save_reload_and_revisit() {
    let directory = tempfile::tempdir().unwrap();
    let repository = ScwRepository::new(directory.path());
    let seed = 0x0D15_7A17;
    let id = repository.next_available_id(seed).unwrap();
    let spawn = spawn_for_seed(seed);
    let mut snapshot = blank_snapshot(seed, "Streaming world".into(), Vec::new(), spawn, 1);
    let mut world = WorldState::from_snapshot(&snapshot).state;

    let origin = VoxelPos::foreground(0, surface_height(seed, 0));
    break_block(&mut world, origin).unwrap();
    place_block(&mut world, origin, BlockState::WOOD).unwrap();

    let far_chunk_x = 20;
    let far_x = far_chunk_x * i64::from(CHUNK_WIDTH);
    world.integrate_chunk(generate_chunk(seed, far_chunk_x));
    let distant = VoxelPos::foreground(far_x, surface_height(seed, far_x));
    break_block(&mut world, distant).unwrap();
    place_block(&mut world, distant, BlockState::DIRT).unwrap();
    world.unload_chunk(far_chunk_x);

    snapshot.player.chunk_x = far_chunk_x;
    snapshot.player.local_x = 0.0;
    snapshot.chunks = world.snapshot_chunks();
    repository.save(&id, &snapshot).unwrap();

    let reloaded = repository.load(&id).unwrap();
    validate_snapshot(&reloaded).unwrap();
    let mut revisited = WorldState::from_snapshot(&reloaded).state;
    assert_eq!(revisited.view().block(distant), Some(BlockState::DIRT));

    let origin_chunk = revisited.persisted_chunk(0).unwrap();
    revisited.integrate_chunk(origin_chunk);
    assert_eq!(revisited.view().block(origin), Some(BlockState::WOOD));
}
