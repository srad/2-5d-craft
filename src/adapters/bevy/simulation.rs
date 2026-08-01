use ::bevy::prelude::*;

use crate::{
    AppState,
    adapters::bevy::{
        RuntimeSet, SimulationClockResource, SimulationDiagnostics, StreamWindowResource,
        TickingAreasResource, WorldStateResource, environment_flag,
    },
    application::{PlayerSimulationRegions, run_simulation},
    domain::{ChunkLayer, MutationPriority, SIMULATION_TICKS_PER_SECOND, VoxelPos, WORLD_HEIGHT},
};

/// Chunk distance of the fixture's deliberately frozen work: inside the render band, outside the
/// simulation radius, so it stays loaded, persistable, and visibly not ticking.
const FIXTURE_FROZEN_DISTANCE: i64 = 4;
const FIXTURE_HEIGHT: i32 = WORLD_HEIGHT / 2;

pub(crate) struct SimulationPlugin;

/// Marks that the manual-acceptance fixture still owes its seeded work.
///
/// The fixture cannot run when the world spawns: only the spawn chunk is integrated then, and its
/// neighbours arrive later through streaming, so a distant schedule would be refused as unloaded.
#[derive(Resource)]
struct PendingSimulationFixture;

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SimulationClockResource>()
            .init_resource::<TickingAreasResource>()
            .init_resource::<SimulationDiagnostics>()
            .add_systems(OnEnter(AppState::LoadingWorld), reset_simulation)
            .add_systems(OnEnter(AppState::MainMenu), reset_simulation)
            .add_systems(
                Update,
                seed_simulation_fixture
                    .in_set(RuntimeSet::Commands)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                Update,
                advance_simulation
                    .in_set(RuntimeSet::Simulation)
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

/// Clears logical time between worlds so no accumulator or stale tick count leaks across sessions.
fn reset_simulation(
    mut commands: Commands,
    mut clock: ResMut<SimulationClockResource>,
    mut diagnostics: ResMut<SimulationDiagnostics>,
) {
    clock.0 = crate::domain::SimulationClock::default();
    *diagnostics = SimulationDiagnostics::default();
    commands.remove_resource::<PendingSimulationFixture>();
    if environment_flag("SIDECRAFT_AUTOSTART")
        && environment_flag("SIDECRAFT_TEST_SIMULATION_FIXTURE")
    {
        commands.insert_resource(PendingSimulationFixture);
    }
}

fn advance_simulation(
    time: Res<Time>,
    mut world: Option<ResMut<WorldStateResource>>,
    window: Option<Res<StreamWindowResource>>,
    areas: Res<TickingAreasResource>,
    mut clock: ResMut<SimulationClockResource>,
    mut diagnostics: ResMut<SimulationDiagnostics>,
) {
    let (Some(world), Some(window)) = (world.as_mut(), window) else {
        return;
    };
    let regions = PlayerSimulationRegions::new(window.0, areas.0.clone());
    let steps = run_simulation(world, &mut clock.0, &regions, time.delta_secs_f64());
    diagnostics.steps_last_frame = steps.len();
    diagnostics.processed_last_frame = steps.iter().map(|step| step.processed.len()).sum();
    diagnostics.world_tick = world.world_tick();
    diagnostics.queued_ticks = world.queued_ticks();
    diagnostics.simulated_chunks = window.simulation_chunks().count();
    diagnostics.ticking_areas = areas.0.len();
}

/// Seeds observable work for the manual acceptance pass: some in the spawn chunk, which drains,
/// and some just outside the simulation radius, which must stay frozen and persisted.
fn seed_simulation_fixture(
    mut commands: Commands,
    pending: Option<Res<PendingSimulationFixture>>,
    mut world: Option<ResMut<WorldStateResource>>,
    window: Option<Res<StreamWindowResource>>,
) {
    let (Some(_), Some(world), Some(window)) = (pending, world.as_mut(), window) else {
        return;
    };
    let center = window.center_chunk();
    let Some(frozen_chunk) = center.checked_add(FIXTURE_FROZEN_DISTANCE) else {
        commands.remove_resource::<PendingSimulationFixture>();
        return;
    };
    if [center, frozen_chunk]
        .into_iter()
        .any(|chunk_x| !world.view().contains_chunk(ChunkLayer::foreground(chunk_x)))
    {
        return;
    }
    let width = i64::from(crate::domain::CHUNK_WIDTH);
    let now = world.world_tick();
    for (chunk_x, count) in [(center, 3), (frozen_chunk, 3)] {
        for index in 0..count {
            let position = VoxelPos::foreground(chunk_x * width + index, FIXTURE_HEIGHT);
            let due = now + (index as u64 + 1) * SIMULATION_TICKS_PER_SECOND;
            if let Err(rejection) =
                world.schedule_tick(position, MutationPriority::PLAYER, due, None)
            {
                warn!("simulation fixture could not schedule work: {rejection:?}");
            }
        }
    }
    commands.remove_resource::<PendingSimulationFixture>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{StreamConfig, StreamWindow, WorldState, blank_snapshot};
    use crate::domain::{SECONDS_PER_STEP, spawn_for_seed};
    use std::time::Duration;

    const TEST_SEED: u64 = 7;

    fn world_state() -> WorldState {
        WorldState::from_snapshot(&blank_snapshot(
            TEST_SEED,
            "Test".into(),
            vec![],
            spawn_for_seed(TEST_SEED),
            1,
        ))
        .state
    }

    /// A headless app with the runtime ordering and simulation wiring, but no rendering, assets,
    /// or physics. `Time` is inserted directly so frames advance by exact virtual deltas.
    fn app() -> App {
        let mut app = App::new();
        RuntimeSet::configure_order(&mut app);
        app.add_plugins(::bevy::state::app::StatesPlugin)
            .init_state::<AppState>()
            .init_resource::<Time>()
            .add_plugins(SimulationPlugin);
        app
    }

    fn enter_world(app: &mut App) {
        set_state(app, AppState::LoadingWorld);
        app.insert_resource(WorldStateResource(world_state()));
        app.insert_resource(StreamWindowResource(StreamWindow::new(
            0,
            StreamConfig::default(),
        )));
        set_state(app, AppState::Playing);
    }

    /// Transitions consume a frame, so zero the delta first: the real `TimePlugin` refreshes it
    /// every frame, and a leftover delta would fake a catch-up burst that cannot happen in game.
    fn set_state(app: &mut App, state: AppState) {
        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(state);
        advance(app, 0.0);
    }

    fn advance(app: &mut App, seconds: f64) {
        let mut time = app.world_mut().resource_mut::<Time>();
        // `advance_by` adds to elapsed and replaces delta, so a zero advance clears the frame.
        time.advance_by(Duration::from_secs_f64(seconds));
        app.update();
    }

    fn world_tick(app: &App) -> u64 {
        app.world().resource::<WorldStateResource>().world_tick()
    }

    #[test]
    fn logical_time_advances_only_while_playing() {
        let mut app = app();
        enter_world(&mut app);

        advance(&mut app, SECONDS_PER_STEP * 3.0);
        assert_eq!(world_tick(&app), 3);

        set_state(&mut app, AppState::Paused);
        advance(&mut app, 10.0);
        advance(&mut app, 10.0);
        assert_eq!(world_tick(&app), 3);

        // Resuming must not pay out the paused time as a catch-up burst.
        set_state(&mut app, AppState::Playing);
        advance(&mut app, SECONDS_PER_STEP);
        assert_eq!(world_tick(&app), 4);
    }

    #[test]
    fn a_stalled_frame_is_capped_at_the_step_budget() {
        let mut app = app();
        enter_world(&mut app);

        advance(&mut app, 30.0);

        assert_eq!(
            world_tick(&app),
            u64::from(crate::domain::MAX_STEPS_PER_FRAME)
        );
        assert_eq!(
            app.world()
                .resource::<SimulationDiagnostics>()
                .steps_last_frame,
            crate::domain::MAX_STEPS_PER_FRAME as usize
        );
    }

    #[test]
    fn logical_time_does_not_advance_without_a_world() {
        let mut app = app();
        set_state(&mut app, AppState::Playing);

        advance(&mut app, 1.0);

        assert_eq!(
            app.world().resource::<SimulationDiagnostics>().world_tick,
            0
        );
    }

    #[test]
    fn the_clock_and_diagnostics_reset_between_worlds() {
        let mut app = app();
        enter_world(&mut app);
        advance(&mut app, SECONDS_PER_STEP * 2.0);
        assert_eq!(
            app.world().resource::<SimulationDiagnostics>().world_tick,
            2
        );

        set_state(&mut app, AppState::MainMenu);
        assert_eq!(
            *app.world().resource::<SimulationDiagnostics>(),
            SimulationDiagnostics::default()
        );
        assert_eq!(
            app.world().resource::<SimulationClockResource>().0,
            crate::domain::SimulationClock::default()
        );

        enter_world(&mut app);
        assert_eq!(world_tick(&app), 0);
    }

    #[test]
    fn simulation_runs_after_player_commands_and_before_mutation_dispatch() {
        let commands = RuntimeSet::ORDER
            .iter()
            .position(|set| *set == RuntimeSet::Commands)
            .unwrap();
        let simulation = RuntimeSet::ORDER
            .iter()
            .position(|set| *set == RuntimeSet::Simulation)
            .unwrap();
        let dispatch = RuntimeSet::ORDER
            .iter()
            .position(|set| *set == RuntimeSet::MutationDispatch)
            .unwrap();

        assert!(commands < simulation);
        assert!(simulation < dispatch);
    }
}
