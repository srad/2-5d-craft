pub(crate) mod camera;
pub(crate) mod environment;
pub(crate) mod interaction;
pub(crate) mod lighting;
pub(crate) mod player;
pub(crate) mod rendering;
pub(crate) mod save;
pub(crate) mod session;
pub(crate) mod showcase;
pub(crate) mod textures;
pub(crate) mod ui;
pub(crate) mod world;

use crate::application::{
    PendingWorld, SaveCoordinator, WorldCatalog, WorldRepository, WorldSession, WorldState,
};
use crate::domain::{DayCycle, LightVolume};
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
    MutationDispatch,
    Derived,
    Persistence,
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
pub(crate) struct DayCycleResource(pub DayCycle);

#[derive(Resource, Default, Deref, DerefMut)]
pub(crate) struct LightVolumeResource(pub LightVolume);

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
