use bevy::prelude::{IVec2, Vec2};
use sidecraft::{
    adapters::storage::ScwRepository,
    application::{
        WorldRepository, WorldState, blank_snapshot, place_tile, remove_tile, validate_snapshot,
    },
    domain::{BlockKind, DayCycle, LightGrid, spawn_for_seed, surface_height},
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

    let mut world = WorldState::from_snapshot(&loaded);
    let surface = IVec2::new(0, surface_height(seed, 0));
    let placed = surface + IVec2::Y;
    assert!(!remove_tile(&mut world, surface).is_empty());
    assert!(place_tile(&mut world, placed, BlockKind::Wood));

    let mut updated = loaded;
    updated.day_phase = 0.82;
    updated.player.local_x = spawn.x + 2.0;
    updated.player.y = spawn.y + 1.0;
    updated.player.selected_slot = 6;
    updated.chunks = world.snapshot_chunks();
    repository.save(&id, &updated).unwrap();

    let reloaded = repository.load(&id).unwrap();
    validate_snapshot(&reloaded).unwrap();
    let reconstructed = WorldState::from_snapshot(&reloaded);
    assert_eq!(reconstructed.grid.get(surface), None);
    assert_eq!(reconstructed.grid.get(placed), Some(BlockKind::Wood));
    assert_eq!(reloaded.player.selected_slot, 6);
    assert_eq!(
        Vec2::new(reloaded.player.local_x, reloaded.player.y),
        spawn + Vec2::new(2.0, 1.0)
    );
    assert_eq!(reloaded.day_phase, 0.82);
    assert!(
        LightGrid::calculate(&reconstructed.grid)
            .visible_at(&reconstructed.grid, placed)
            .sky
            > 0
    );

    let listed = repository.list().unwrap();
    assert_eq!(listed.valid.len(), 1);
    assert!(listed.invalid.is_empty());
    assert_eq!(listed.valid[0].name, "End-to-end world");

    let cycle = DayCycle {
        phase: reloaded.day_phase,
        ..Default::default()
    };
    assert!(cycle.daylight() >= 0.15);
    assert!(cycle.light_level() <= 15);
}
