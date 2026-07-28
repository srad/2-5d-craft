mod block;
mod camera;
mod interaction;
mod lighting;
mod persistence;
mod player;
mod rendering;
mod ui;
mod world;

use avian2d::prelude::*;
use bevy::prelude::*;

pub use block::{BlockDef, BlockKind};
pub use lighting::{DayCycle, LightCell, LightGrid};
pub use persistence::{SavedChunk, SavedPlayer, WorldSaveV1, WorldStore, blank_save, validate};
pub use world::{
    BlockChunk, BlockGrid, WorldSession, generate_chunk, generate_world, generated_voxel,
    spawn_for_seed, surface_height, surface_height_at_depth, world_to_chunk,
};

pub const WORLD_HEIGHT: i32 = 80;
pub const CHUNK_WIDTH: i32 = 32;
pub const DEPTH_SLICES: u8 = 4;
pub const GENERATOR_VERSION: u32 = 2;
pub const SAVE_SCHEMA_VERSION: u32 = 2;

#[derive(States, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppState {
    #[default]
    Boot,
    MainMenu,
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
            .init_resource::<WorldStore>()
            .add_systems(PostStartup, finish_boot)
            .add_plugins((
                PhysicsPlugins::default().with_length_unit(1.0),
                rendering::RenderingPlugin,
                world::WorldPlugin,
                lighting::LightingPlugin,
                player::PlayerPlugin,
                interaction::InteractionPlugin,
                camera::CameraPlugin,
                ui::GameUiPlugin,
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
