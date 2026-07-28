use bevy::prelude::*;
use bevy::window::{Window, WindowPlugin, WindowResizeConstraints, WindowResolution};
use sidecraft::GamePlugin;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    close_when_requested: false,
                    primary_window: Some(Window {
                        title: "Sidecraft".into(),
                        resolution: WindowResolution::new(1280, 720),
                        resize_constraints: WindowResizeConstraints {
                            min_width: 800.0,
                            min_height: 450.0,
                            ..default()
                        },
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(GamePlugin)
        .run();
}
