pub(crate) mod camera;
pub(crate) mod interaction;
pub(crate) mod lighting;
pub(crate) mod player;
pub(crate) mod rendering;
pub(crate) mod save;
pub(crate) mod session;
pub(crate) mod ui;
pub(crate) mod world;

use crate::application::{
    PendingWorld, SaveCoordinator, WorldCatalog, WorldRepository, WorldSession, WorldState,
};
use crate::domain::{DayCycle, LightGrid};
use ::bevy::prelude::*;
use std::sync::Arc;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RuntimeSet {
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

#[derive(Resource, Deref, DerefMut)]
pub(crate) struct LightGridResource(pub LightGrid);

impl Default for LightGridResource {
    fn default() -> Self {
        let grid = crate::domain::BlockGrid::new(crate::domain::WORLD_HEIGHT);
        Self(LightGrid::calculate(&grid.view()))
    }
}
