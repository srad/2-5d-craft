use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::window::{Window, WindowPlugin, WindowResizeConstraints, WindowResolution};
use sidecraft::GamePlugin;
use sidecraft::adapters::logging;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                // The composition root is the only place that decides where log events go.
                .set(LogPlugin {
                    filter: logging::configured_filter(),
                    custom_layer: logging::file_layer,
                    ..default()
                })
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
