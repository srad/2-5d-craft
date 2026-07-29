pub mod adapters;
pub mod application;
pub mod domain;

use adapters::bevy::{RepositoryHandle, RuntimeSet};
use adapters::storage::ScwRepository;
use avian2d::prelude::*;
use bevy::prelude::*;
use std::sync::Arc;

#[derive(States, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppState {
    #[default]
    Boot,
    MainMenu,
    TexturePacks,
    WorldSelect,
    LoadingWorld,
    Playing,
    Paused,
    Saving,
}

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>()
            .insert_resource(Time::<Fixed>::from_hz(64.0))
            .insert_resource(Gravity(Vec2::NEG_Y * 25.0))
            .insert_resource(RepositoryHandle(Arc::new(ScwRepository::default())))
            .configure_sets(
                Update,
                (
                    RuntimeSet::Clock,
                    RuntimeSet::CompletedWork,
                    RuntimeSet::WorldMaintenance,
                    RuntimeSet::Commands,
                    RuntimeSet::MutationDispatch,
                    RuntimeSet::Derived,
                    RuntimeSet::Persistence,
                )
                    .chain(),
            )
            .add_systems(PostStartup, finish_boot)
            .add_plugins((
                adapters::bevy::textures::TexturePackPlugin,
                PhysicsPlugins::default().with_length_unit(1.0),
                adapters::bevy::rendering::RenderingPlugin,
                adapters::bevy::world::WorldPlugin,
                adapters::bevy::lighting::LightingPlugin,
                adapters::bevy::player::PlayerPlugin,
                adapters::bevy::interaction::InteractionPlugin,
                adapters::bevy::camera::CameraPlugin,
                adapters::bevy::environment::EnvironmentPlugin,
                adapters::bevy::save::SavePlugin,
                adapters::bevy::session::SessionPlugin,
                adapters::bevy::ui::GameUiPlugin,
            ));
    }
}

fn finish_boot(mut next_state: ResMut<NextState<AppState>>) {
    next_state.set(AppState::MainMenu);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_always_transitions_to_main_menu() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .init_state::<AppState>()
            .add_systems(PostStartup, finish_boot);
        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<State<AppState>>().get(),
            &AppState::MainMenu
        );
    }
}
