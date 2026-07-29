use bevy::prelude::*;

use super::{
    UiAction, UiFont, front_end_root_node, menu_panel_node, spawn_button, spawn_status,
    spawn_version,
};
use crate::{
    AppState,
    adapters::bevy::{WorldCatalogResource, environment_flag, session::SessionCommand},
};

mod texture_packs;

#[derive(Component)]
struct MainMenuRoot;

#[derive(Component)]
struct SettingsRoot;

#[derive(Component)]
struct WorldSelectRoot;

pub(super) fn register(app: &mut App) {
    app.init_resource::<WorldCatalogResource>()
        .add_systems(
            OnEnter(AppState::MainMenu),
            (spawn_main_menu, autostart_world),
        )
        .add_systems(OnEnter(AppState::Settings), spawn_settings)
        .add_systems(OnEnter(AppState::WorldSelect), spawn_world_select)
        .add_systems(Update, handle_navigation);
    texture_packs::register(app);
}

fn spawn_main_menu(mut commands: Commands, ui_font: Res<UiFont>) {
    if environment_flag("SIDECRAFT_AUTOSTART") {
        return;
    }
    let font = ui_font.0.clone();
    commands
        .spawn((
            front_end_root_node(),
            MainMenuRoot,
            DespawnOnExit(AppState::MainMenu),
        ))
        .with_children(|root| {
            root.spawn(menu_panel_node(420.0)).with_children(|panel| {
                spawn_title(panel, &font, "SIDECRAFT", 72.0);
                panel.spawn((
                    Text::new("A pixel 2.5D sandbox"),
                    TextFont {
                        font: font.clone(),
                        font_size: FontSize::Px(27.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.62, 0.78, 0.72)),
                ));
                spawn_button(panel, &font, "NEW WORLD", UiAction::NewWorld);
                spawn_button(panel, &font, "LOAD WORLD", UiAction::ShowWorlds);
                spawn_button(panel, &font, "SETTINGS", UiAction::ShowSettings);
                spawn_button(panel, &font, "QUIT", UiAction::Quit);
                spawn_status(panel, &font);
            });
            spawn_version(root, &font);
        });
}

fn autostart_world(mut messages: MessageWriter<SessionCommand>) {
    if environment_flag("SIDECRAFT_AUTOSTART") {
        messages.write(SessionCommand::NewWorld);
    }
}

fn spawn_settings(mut commands: Commands, ui_font: Res<UiFont>) {
    let font = ui_font.0.clone();
    commands
        .spawn((
            front_end_root_node(),
            SettingsRoot,
            DespawnOnExit(AppState::Settings),
        ))
        .with_children(|root| {
            root.spawn(menu_panel_node(420.0)).with_children(|panel| {
                spawn_title(panel, &font, "SETTINGS", 52.0);
                spawn_button(panel, &font, "TEXTURE PACKS", UiAction::ShowTexturePacks);
                spawn_button(panel, &font, "BACK", UiAction::ShowMainMenu);
                spawn_status(panel, &font);
            });
            spawn_version(root, &font);
        });
}

fn spawn_world_select(
    mut commands: Commands,
    ui_font: Res<UiFont>,
    catalog: Res<WorldCatalogResource>,
) {
    let font = ui_font.0.clone();
    commands
        .spawn((
            front_end_root_node(),
            WorldSelectRoot,
            DespawnOnExit(AppState::WorldSelect),
        ))
        .with_children(|root| {
            root.spawn(menu_panel_node(480.0)).with_children(|panel| {
                spawn_title(panel, &font, "SELECT WORLD", 52.0);
                if catalog.valid.is_empty() {
                    panel.spawn((
                        Text::new("No saved worlds yet"),
                        TextFont {
                            font: font.clone(),
                            font_size: FontSize::Px(27.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.68, 0.74, 0.70)),
                    ));
                }
                for world in &catalog.valid {
                    spawn_button(
                        panel,
                        &font,
                        &world.name,
                        UiAction::LoadWorld(world.id.clone()),
                    );
                }
                spawn_button(panel, &font, "BACK", UiAction::ShowMainMenu);
                spawn_status(panel, &font);
            });
            spawn_version(root, &font);
        });
}

fn spawn_title(parent: &mut ChildSpawnerCommands, font: &FontSource, title: &str, size: f32) {
    parent.spawn((
        Text::new(title),
        TextFont {
            font: font.clone(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(Color::srgb(0.92, 0.96, 0.88)),
    ));
}

fn handle_navigation(
    interactions: Query<(&Interaction, &UiAction), Changed<Interaction>>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    for (interaction, action) in &interactions {
        if *interaction == Interaction::Pressed
            && let Some(target) = navigation_target(action)
        {
            next_state.set(target);
        }
    }
}

fn navigation_target(action: &UiAction) -> Option<AppState> {
    match action {
        UiAction::ShowMainMenu => Some(AppState::MainMenu),
        UiAction::ShowSettings => Some(AppState::Settings),
        UiAction::ShowTexturePacks => Some(AppState::TexturePacks),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_end_navigation_has_explicit_state_targets() {
        assert_eq!(
            navigation_target(&UiAction::ShowMainMenu),
            Some(AppState::MainMenu)
        );
        assert_eq!(
            navigation_target(&UiAction::ShowSettings),
            Some(AppState::Settings)
        );
        assert_eq!(
            navigation_target(&UiAction::ShowTexturePacks),
            Some(AppState::TexturePacks)
        );
        assert_eq!(navigation_target(&UiAction::NewWorld), None);
    }

    #[test]
    fn menu_actions_do_not_enter_the_world_session_command_path() {
        assert!(UiAction::ShowMainMenu.session_command().is_none());
        assert!(UiAction::ShowSettings.session_command().is_none());
        assert!(UiAction::ShowTexturePacks.session_command().is_none());
    }
}
