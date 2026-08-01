use std::collections::BTreeSet;

use crate::application::{StreamWindow, WorldState};
use crate::domain::{
    ChunkLayer, ScheduledTick, SimulationClock, SimulationRegionProvider, TickingArea,
};

/// The active region of a single-player session: the streaming window's simulation band plus any
/// bounded ticking areas.
///
/// Rules never see this type. They receive whatever [`SimulationRegionProvider`] supplies, so a
/// future host can pass the union of several player regions without changing rule code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerSimulationRegions {
    window: StreamWindow,
    areas: Vec<TickingArea>,
}

impl PlayerSimulationRegions {
    pub fn new(window: StreamWindow, areas: Vec<TickingArea>) -> Self {
        Self { window, areas }
    }
}

impl SimulationRegionProvider for PlayerSimulationRegions {
    fn active_chunks(&self) -> Vec<i64> {
        self.window
            .simulation_chunks()
            .chain(self.areas.iter().flat_map(|area| area.chunks()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

/// One completed logical simulation step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimulationStep {
    pub world_tick: u64,
    pub processed: Vec<ScheduledTick>,
}

/// Advances zero or more logical simulation steps for one rendered frame.
///
/// M3.1 registers no rules, so due work is consumed and reported rather than turned into
/// proposals. The step boundary, activation, and bounded drain are what this establishes.
pub fn run_simulation(
    world: &mut WorldState,
    clock: &mut SimulationClock,
    regions: &impl SimulationRegionProvider,
    delta_seconds: f64,
) -> Vec<SimulationStep> {
    clock.accumulate(delta_seconds);
    let steps = clock.take_steps();
    if steps == 0 {
        return Vec::new();
    }
    // The active set cannot change between steps of one frame: the player has not moved and no
    // chunk has loaded. Computing it once also keeps the loaded intersection off the step path.
    let active = active_loaded_chunks(world, regions);
    (0..steps)
        .map(|_| {
            let world_tick = world.advance_world_tick();
            SimulationStep {
                world_tick,
                processed: world.take_due_ticks(&active),
            }
        })
        .collect()
}

/// The active chunks that are actually loaded.
///
/// A ticking area over an unloaded chunk is skipped rather than force-loading a frontier chunk;
/// its queued work stays queued until streaming brings the chunk back.
fn active_loaded_chunks(world: &WorldState, regions: &impl SimulationRegionProvider) -> Vec<i64> {
    let view = world.view();
    regions
        .active_chunks()
        .into_iter()
        .filter(|chunk_x| view.contains_chunk(ChunkLayer::foreground(*chunk_x)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{StreamConfig, blank_snapshot};
    use crate::domain::{
        BlockChunk, MutationPriority, SECONDS_PER_STEP, VoxelPos, generate_chunk, spawn_for_seed,
    };

    const SEED: u64 = 11;

    fn world() -> WorldState {
        WorldState::from_snapshot(&blank_snapshot(
            SEED,
            "Test".into(),
            vec![],
            spawn_for_seed(SEED),
            1,
        ))
        .state
    }

    fn regions(areas: Vec<TickingArea>) -> PlayerSimulationRegions {
        PlayerSimulationRegions::new(StreamWindow::new(0, StreamConfig::default()), areas)
    }

    fn load(world: &mut WorldState, chunk_x: i64) -> BlockChunk {
        let chunk = generate_chunk(SEED, chunk_x);
        world.integrate_chunk(chunk.clone());
        chunk
    }

    #[test]
    fn active_region_is_the_sorted_union_of_the_window_and_ticking_areas() {
        assert_eq!(
            regions(vec![]).active_chunks(),
            vec![-3, -2, -1, 0, 1, 2, 3]
        );
        assert_eq!(
            regions(vec![TickingArea::new(20, 1), TickingArea::new(3, 1)]).active_chunks(),
            vec![-3, -2, -1, 0, 1, 2, 3, 4, 19, 20, 21]
        );
    }

    #[test]
    fn frames_advance_exactly_the_logical_ticks_they_paid_for() {
        let mut world = world();
        let mut clock = SimulationClock::default();
        for _ in 0..60 {
            run_simulation(&mut world, &mut clock, &regions(vec![]), 1.0 / 60.0);
        }
        assert_eq!(world.world_tick(), 20);
    }

    #[test]
    fn a_single_long_frame_is_capped_and_never_catches_up() {
        let mut world = world();
        let mut clock = SimulationClock::default();
        let steps = run_simulation(&mut world, &mut clock, &regions(vec![]), 30.0);
        assert_eq!(steps.len(), 4);
        assert_eq!(world.world_tick(), 4);
        assert_eq!(
            steps.iter().map(|step| step.world_tick).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
    }

    #[test]
    fn only_active_chunks_process_their_due_work() {
        let mut world = world();
        load(&mut world, 1);
        let inactive_chunk = 5;
        load(&mut world, inactive_chunk);
        let active_tick = world
            .schedule_tick(
                VoxelPos::foreground(33, 40),
                MutationPriority::PLAYER,
                1,
                None,
            )
            .unwrap();
        world
            .schedule_tick(
                VoxelPos::foreground(inactive_chunk * 32, 40),
                MutationPriority::PLAYER,
                1,
                None,
            )
            .unwrap();

        let mut clock = SimulationClock::default();
        let steps = run_simulation(
            &mut world,
            &mut clock,
            &regions(vec![]),
            SECONDS_PER_STEP * 2.0,
        );

        assert_eq!(steps[0].processed, vec![active_tick]);
        assert!(steps[1].processed.is_empty());
        // The frozen chunk keeps its work for whenever it becomes active again.
        assert_eq!(world.queued_ticks(), 1);
    }

    #[test]
    fn a_ticking_area_over_an_unloaded_chunk_drains_nothing_and_loads_nothing() {
        let mut world = world();
        let distant = 40;
        load(&mut world, distant);
        let queued = world
            .schedule_tick(
                VoxelPos::foreground(distant * 32, 40),
                MutationPriority::PLAYER,
                1,
                None,
            )
            .unwrap();
        world.unload_chunk(distant);

        let mut clock = SimulationClock::default();
        let steps = run_simulation(
            &mut world,
            &mut clock,
            &regions(vec![TickingArea::new(distant, 1)]),
            SECONDS_PER_STEP,
        );

        assert!(steps[0].processed.is_empty());
        assert_eq!(world.queued_ticks(), 1);
        assert!(!world.view().contains_chunk(ChunkLayer::foreground(distant)));

        // It thaws once streaming brings the chunk back, without losing its place in the queue.
        world.integrate_chunk(world.persisted_chunk(distant).unwrap());
        let steps = run_simulation(
            &mut world,
            &mut clock,
            &regions(vec![TickingArea::new(distant, 1)]),
            SECONDS_PER_STEP,
        );
        assert_eq!(steps[0].processed, vec![queued]);
    }

    #[test]
    fn identical_histories_produce_identical_ticks_regardless_of_frame_pacing() {
        let run = |deltas: &[f64]| {
            let mut world = world();
            load(&mut world, 1);
            for due in 1..=6 {
                world
                    .schedule_tick(
                        VoxelPos::foreground(33, 40 + due as i32),
                        MutationPriority::PLAYER,
                        due,
                        None,
                    )
                    .unwrap();
            }
            let mut clock = SimulationClock::default();
            let mut processed = Vec::new();
            for delta in deltas {
                for step in run_simulation(&mut world, &mut clock, &regions(vec![]), *delta) {
                    processed.push((step.world_tick, step.processed));
                }
            }
            (world.world_tick(), processed)
        };

        let steady = run(&[SECONDS_PER_STEP; 6]);
        let jittery = run(&[
            SECONDS_PER_STEP * 0.5,
            SECONDS_PER_STEP * 2.5,
            SECONDS_PER_STEP * 1.5,
            SECONDS_PER_STEP * 1.5,
        ]);

        assert_eq!(steady.0, 6);
        assert_eq!(steady, jittery);
    }

    #[test]
    fn a_frame_without_a_whole_step_changes_nothing() {
        let mut world = world();
        let mut clock = SimulationClock::default();
        assert!(
            run_simulation(
                &mut world,
                &mut clock,
                &regions(vec![]),
                SECONDS_PER_STEP / 2.0
            )
            .is_empty()
        );
        assert_eq!(world.world_tick(), 0);
    }
}
