pub(crate) mod camera;
pub(crate) mod environment;
pub(crate) mod interaction;
pub(crate) mod lighting;
pub(crate) mod player;
pub(crate) mod rendering;
pub(crate) mod save;
pub(crate) mod session;
pub(crate) mod showcase;
pub(crate) mod simulation;
pub(crate) mod textures;
pub(crate) mod ui;
pub(crate) mod world;

use crate::application::{
    PendingWorld, SaveCoordinator, SessionInstanceCounter, StreamConfig, StreamWindow,
    WorldCatalog, WorldRepository, WorldSession, WorldState,
};
use crate::domain::{DayCycle, LightVolume, SimulationClock, TickingArea};
use ::bevy::prelude::*;
use std::sync::Arc;

pub(crate) fn environment_flag(name: &str) -> bool {
    parse_environment_flag(std::env::var(name).ok().as_deref())
}

pub(crate) fn configured_autostart_u64(name: &str) -> Option<u64> {
    environment_flag("SIDECRAFT_AUTOSTART")
        .then(|| std::env::var(name).ok())
        .flatten()
        .and_then(|value| value.parse::<u64>().ok())
}

fn parse_environment_flag(value: Option<&str>) -> bool {
    value.is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RuntimeSet {
    Clock,
    CompletedWork,
    WorldMaintenance,
    Commands,
    Simulation,
    MutationDispatch,
    Derived,
    Persistence,
}

impl RuntimeSet {
    /// The single source of truth for `Update` ordering, mirroring the "Bevy ordering" contract in
    /// `ARCHITECTURE.md`. The composition root chains exactly this sequence.
    /// Chains [`Self::ORDER`] in `Update`. Equivalent to `.chain()` over the same sequence, but
    /// driven by the constant so ordering has exactly one declaration.
    pub(crate) fn configure_order(app: &mut App) {
        for pair in Self::ORDER.windows(2) {
            app.configure_sets(Update, pair[1].after(pair[0]));
        }
    }

    pub(crate) const ORDER: [Self; 8] = [
        Self::Clock,
        Self::CompletedWork,
        Self::WorldMaintenance,
        Self::Commands,
        Self::Simulation,
        Self::MutationDispatch,
        Self::Derived,
        Self::Persistence,
    ];
}

#[derive(Resource, Clone)]
pub(crate) struct RepositoryHandle(pub Arc<dyn WorldRepository>);

#[derive(Resource, Deref, DerefMut)]
pub(crate) struct PendingWorldResource(pub PendingWorld);

#[derive(Resource, Default, Deref, DerefMut)]
pub(crate) struct SaveCoordinatorResource(pub SaveCoordinator);

#[derive(Resource, Default, Deref, DerefMut)]
pub(crate) struct WorldCatalogResource(pub WorldCatalog);

#[derive(Resource, Deref, DerefMut)]
pub(crate) struct WorldSessionResource(pub WorldSession);

#[derive(Resource, Deref, DerefMut)]
pub(crate) struct WorldStateResource(pub WorldState);

#[derive(Resource, Default, Deref, DerefMut)]
pub(crate) struct SessionInstanceCounterResource(pub SessionInstanceCounter);

#[derive(Resource, Default, Deref, DerefMut)]
pub(crate) struct StreamConfigResource(pub StreamConfig);

#[derive(Resource, Deref, DerefMut)]
pub(crate) struct StreamWindowResource(pub StreamWindow);

#[derive(Resource, Default, Deref, DerefMut)]
pub(crate) struct DayCycleResource(pub DayCycle);

#[derive(Resource, Default, Deref, DerefMut)]
pub(crate) struct LightVolumeResource(pub LightVolume);

#[derive(Resource, Default, Deref, DerefMut)]
pub(crate) struct SimulationClockResource(pub SimulationClock);

/// Bounded areas that stay simulated regardless of player position. Empty by default; M3.1 gives
/// them no configuration source.
#[derive(Resource, Default, Deref, DerefMut)]
pub(crate) struct TickingAreasResource(pub Vec<TickingArea>);

/// Presentation-only counters for the debug HUD and session log. Never authoritative.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SimulationDiagnostics {
    pub(crate) world_tick: u64,
    pub(crate) steps_last_frame: usize,
    /// Highest step count any single frame reached since the world was entered.
    ///
    /// The HUD shows this rather than `steps_last_frame`, which reads zero on most frames at
    /// normal frame rates: what acceptance needs to see is whether the four-step budget was ever
    /// exceeded, not what the current frame happened to do.
    pub(crate) max_steps_per_frame: usize,
    pub(crate) processed_last_frame: usize,
    /// Scheduled ticks processed since the world was entered.
    ///
    /// Cumulative rather than per-frame because the log samples once per game second: work
    /// drained on any other tick would be invisible, and a counter that reads zero while the
    /// queue visibly shrinks is worse than no counter at all.
    pub(crate) processed_total: u64,
    pub(crate) queued_ticks: usize,
    pub(crate) simulated_chunks: usize,
    pub(crate) ticking_areas: usize,
}

/// Whether the debug HUD line is shown. Lives in a resource because the HUD is respawned on every
/// `OnEnter(AppState::Playing)`, so an entity-local flag would reset on each pause.
#[derive(Resource, Default, Deref, DerefMut)]
pub(crate) struct SimulationDebugVisible(pub bool);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_flags_accept_only_explicit_true_values() {
        for value in ["1", "true", "TRUE", "yes", "YeS"] {
            assert!(parse_environment_flag(Some(value)));
        }
        for value in ["", "0", "false", "no", "enabled"] {
            assert!(!parse_environment_flag(Some(value)));
        }
        assert!(!parse_environment_flag(None));
    }
}
