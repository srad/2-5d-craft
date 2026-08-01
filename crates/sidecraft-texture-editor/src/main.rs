mod document;
mod file_ops;
mod generation;
mod preview;
mod ui;

use bevy::{
    prelude::*,
    window::{Window, WindowPlugin, WindowResizeConstraints, WindowResolution},
};
use bevy_egui::{EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass};
use document::EditorDocument;
use file_ops::{Confirmation, EditorCommands, EditorStatus, ExportCoordinator};
use generation::GenerationCoordinator;
use preview::{PreviewLighting, PreviewViewport};
use ui::EditorUiState;

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::srgb_u8(24, 22, 20)))
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    close_when_requested: false,
                    primary_window: Some(Window {
                        title: "Sidecraft Texture Pack Editor".into(),
                        resolution: WindowResolution::new(1440, 810),
                        resize_constraints: WindowResizeConstraints {
                            min_width: 960.0,
                            min_height: 600.0,
                            ..default()
                        },
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(EguiPlugin::default())
        .insert_resource(editor_egui_settings())
        .init_resource::<EditorDocument>()
        .init_resource::<EditorCommands>()
        .init_resource::<EditorStatus>()
        .init_resource::<Confirmation>()
        .init_resource::<ExportCoordinator>()
        .init_resource::<GenerationCoordinator>()
        .init_resource::<PreviewViewport>()
        .init_resource::<PreviewLighting>()
        .init_resource::<EditorUiState>()
        .add_systems(Startup, (preview::setup_preview, generation::queue_initial))
        .add_systems(EguiPrimaryContextPass, ui::draw_editor)
        .add_systems(
            Update,
            (
                file_ops::handle_close_requests,
                file_ops::process_commands,
                generation::drive_generation,
                file_ops::poll_export,
                preview::update_preview,
                preview::apply_preview_viewport,
            )
                .chain(),
        )
        .run();
}

fn editor_egui_settings() -> EguiGlobalSettings {
    EguiGlobalSettings {
        auto_create_primary_context: false,
        ..default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_disables_automatic_egui_camera_selection() {
        assert!(!editor_egui_settings().auto_create_primary_context);
    }
}
