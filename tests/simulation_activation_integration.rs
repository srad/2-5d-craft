use sidecraft::{
    adapters::storage::ScwRepository,
    application::{
        PlayerSimulationRegions, ScheduleRejection, StreamConfig, StreamWindow, WorldRepository,
        WorldState, blank_snapshot, run_simulation, validate_snapshot,
    },
    domain::{
        CHUNK_WIDTH, ChunkLayer, MutationPriority, SECONDS_PER_STEP, SimulationClock, TickingArea,
        VoxelPos, generate_chunk, spawn_for_seed,
    },
};

const SEED: u64 = 0x51_4D_5F_01;
const FROZEN_CHUNK: i64 = 6;

fn regions(areas: Vec<TickingArea>) -> PlayerSimulationRegions {
    PlayerSimulationRegions::new(StreamWindow::new(0, StreamConfig::default()), areas)
}

fn block_in(chunk_x: i64) -> VoxelPos {
    VoxelPos::foreground(chunk_x * i64::from(CHUNK_WIDTH), 40)
}

/// Runs whole frames of exactly one logical step each.
fn step(world: &mut WorldState, clock: &mut SimulationClock, areas: &[TickingArea], frames: usize) {
    for _ in 0..frames {
        run_simulation(world, clock, &regions(areas.to_vec()), SECONDS_PER_STEP);
    }
}

#[test]
fn frozen_work_survives_streaming_save_and_reload_then_thaws_on_its_due_tick() {
    let directory = tempfile::tempdir().unwrap();
    let repository = ScwRepository::new(directory.path());
    let id = repository.next_available_id(SEED).unwrap();
    let snapshot = blank_snapshot(
        SEED,
        "Activation world".into(),
        Vec::new(),
        spawn_for_seed(SEED),
        1,
    );
    repository.save(&id, &snapshot).unwrap();

    let mut world = WorldState::from_snapshot(&repository.load(&id).unwrap()).state;
    world.integrate_chunk(generate_chunk(SEED, FROZEN_CHUNK));
    let mut clock = SimulationClock::default();

    let active = world
        .schedule_tick(block_in(0), MutationPriority::PLAYER, 3, None)
        .unwrap();
    let frozen = world
        .schedule_tick(block_in(FROZEN_CHUNK), MutationPriority::PLAYER, 3, None)
        .unwrap();

    // The frozen chunk is loaded and rendered, but outside the simulation radius.
    step(&mut world, &mut clock, &[], 5);
    assert_eq!(world.world_tick(), 5);
    assert_eq!(world.queued_ticks(), 1);
    assert!(
        world.take_due_ticks(&[0]).is_empty(),
        "{active:?} already ran"
    );

    // Streaming it out must not lose the work it still owes.
    world.unload_chunk(FROZEN_CHUNK);
    let mut saved = snapshot.clone();
    saved.chunks = world.snapshot_chunks();
    saved.world_tick = world.world_tick();
    saved.next_tick_sequence = 2;
    repository.save(&id, &saved).unwrap();

    let reloaded = repository.load(&id).unwrap();
    validate_snapshot(&reloaded).unwrap();
    assert_eq!(reloaded.world_tick, 5);
    assert_eq!(
        reloaded
            .chunks
            .iter()
            .find(|chunk| chunk.x == FROZEN_CHUNK)
            .map(|chunk| chunk.pending_ticks.as_slice()),
        Some([frozen].as_slice())
    );

    // Revisiting rejoins the normal bounded queue: overdue work runs on the next active tick.
    let mut revisited = WorldState::from_snapshot(&reloaded).state;
    let mut clock = SimulationClock::default();
    let area = [TickingArea::new(FROZEN_CHUNK, 0)];
    step(&mut revisited, &mut clock, &area, 1);
    assert_eq!(revisited.queued_ticks(), 1, "an unloaded area stays frozen");

    revisited.integrate_chunk(revisited.persisted_chunk(FROZEN_CHUNK).unwrap());
    step(&mut revisited, &mut clock, &area, 1);
    assert_eq!(revisited.queued_ticks(), 0);
    assert_eq!(revisited.world_tick(), 7);
}

#[test]
fn simulation_never_force_loads_a_frontier_chunk() {
    let mut world = WorldState::from_snapshot(&blank_snapshot(
        SEED,
        "Frontier world".into(),
        Vec::new(),
        spawn_for_seed(SEED),
        1,
    ))
    .state;

    assert_eq!(
        world.schedule_tick(block_in(FROZEN_CHUNK), MutationPriority::PLAYER, 1, None),
        Err(ScheduleRejection::Unloaded)
    );

    let mut clock = SimulationClock::default();
    step(
        &mut world,
        &mut clock,
        &[TickingArea::new(FROZEN_CHUNK, 1)],
        4,
    );

    assert_eq!(world.world_tick(), 4);
    for chunk_x in FROZEN_CHUNK - 1..=FROZEN_CHUNK + 1 {
        assert!(
            !world.view().contains_chunk(ChunkLayer::foreground(chunk_x)),
            "a ticking area must not pull chunk {chunk_x} into memory"
        );
    }
}
