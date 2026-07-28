use avian2d::prelude::{Physics, PhysicsTime};
use bevy::prelude::*;

use crate::{
    AppState,
    adapters::bevy::{
        WorldCatalogResource,
        player::{Hotbar, Player},
        session::SessionCommand,
    },
    application::WorldId,
};

#[derive(Component)]
struct MainMenuRoot;

#[derive(Component)]
struct WorldSelectRoot;

#[derive(Component)]
struct LoadingRoot;

#[derive(Component)]
struct HudRoot;

#[derive(Component)]
struct PauseRoot;

#[derive(Component)]
struct SavingRoot;

#[derive(Component, Debug, Clone)]
enum UiAction {
    NewWorld,
    ShowWorlds,
    LoadWorld(WorldId),
    Resume,
    SaveAndQuit,
    RetrySave,
    QuitWithoutSaving,
    Back,
    Quit,
}

impl From<&UiAction> for SessionCommand {
    fn from(action: &UiAction) -> Self {
        match action {
            UiAction::NewWorld => Self::NewWorld,
            UiAction::ShowWorlds => Self::ShowWorlds,
            UiAction::LoadWorld(id) => Self::LoadWorld(id.clone()),
            UiAction::Resume => Self::Resume,
            UiAction::SaveAndQuit => Self::SaveAndQuit,
            UiAction::RetrySave => Self::RetrySave,
            UiAction::QuitWithoutSaving => Self::QuitWithoutSaving,
            UiAction::Back => Self::Back,
            UiAction::Quit => Self::Quit,
        }
    }
}

#[derive(Component)]
struct StatusText;

#[derive(Component)]
struct HotbarSlot(u8);

#[derive(Component)]
struct SelectedItemText;

#[derive(Resource, Clone)]
struct UiFont(FontSource);

#[derive(Resource, Default)]
pub(crate) struct UiStatus(pub(crate) String);

type ChangedButtonQuery<'w, 's> = Query<
    'w,
    's,
    (&'static Interaction, &'static mut BackgroundColor),
    (With<Button>, Changed<Interaction>),
>;

pub struct GameUiPlugin;

impl Plugin for GameUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiStatus>()
            .init_resource::<WorldCatalogResource>()
            .add_systems(
                OnEnter(AppState::MainMenu),
                (spawn_main_menu, autostart_world),
            )
            .add_systems(OnEnter(AppState::WorldSelect), spawn_world_select)
            .add_systems(OnEnter(AppState::LoadingWorld), spawn_loading_screen)
            .add_systems(OnEnter(AppState::Saving), spawn_saving_screen)
            .add_systems(OnEnter(AppState::Playing), (resume_physics, spawn_hud))
            .add_systems(
                OnEnter(AppState::Paused),
                (pause_physics, spawn_pause_menu).chain(),
            )
            .add_systems(
                Update,
                (
                    handle_actions,
                    style_buttons,
                    update_status_text,
                    update_hotbar.run_if(in_state(AppState::Playing)),
                ),
            );
    }

    fn finish(&self, app: &mut App) {
        let font =
            Font::from_bytes(include_bytes!("../../../assets/fonts/VT323-Regular.ttf").to_vec());
        let handle = app.world_mut().resource_mut::<Assets<Font>>().add(font);
        app.insert_resource(UiFont(handle.into()));
    }
}

fn spawn_main_menu(mut commands: Commands, ui_font: Res<UiFont>) {
    if environment_flag("SIDECRAFT_AUTOSTART") {
        return;
    }
    let font = ui_font.0.clone();
    commands
        .spawn((
            root_node(Color::srgb(0.035, 0.055, 0.085)),
            MainMenuRoot,
            DespawnOnExit(AppState::MainMenu),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("SIDECRAFT"),
                TextFont {
                    font: font.clone(),
                    font_size: FontSize::Px(76.0),
                    ..default()
                },
                TextColor(Color::srgb(0.90, 0.95, 1.0)),
            ));
            root.spawn((
                Text::new("A pixel 2.5D sandbox"),
                TextFont {
                    font: font.clone(),
                    font_size: FontSize::Px(28.0),
                    ..default()
                },
                TextColor(Color::srgb(0.55, 0.72, 0.82)),
            ));
            spawn_button(root, &font, "NEW WORLD", UiAction::NewWorld);
            spawn_button(root, &font, "LOAD WORLD", UiAction::ShowWorlds);
            spawn_button(root, &font, "QUIT", UiAction::Quit);
            spawn_status(root, &font);
        });
}

fn autostart_world(mut messages: MessageWriter<SessionCommand>) {
    if environment_flag("SIDECRAFT_AUTOSTART") {
        messages.write(SessionCommand::NewWorld);
    }
}

fn environment_flag(name: &str) -> bool {
    std::env::var(name)
        .is_ok_and(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

fn spawn_world_select(
    mut commands: Commands,
    ui_font: Res<UiFont>,
    catalog: Res<WorldCatalogResource>,
) {
    let font = ui_font.0.clone();
    commands
        .spawn((
            root_node(Color::srgb(0.04, 0.055, 0.075)),
            WorldSelectRoot,
            DespawnOnExit(AppState::WorldSelect),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("SELECT WORLD"),
                TextFont {
                    font: font.clone(),
                    font_size: FontSize::Px(56.0),
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
            if catalog.valid.is_empty() {
                root.spawn((
                    Text::new("No saved worlds yet"),
                    TextFont {
                        font: font.clone(),
                        font_size: FontSize::Px(28.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.65, 0.70, 0.75)),
                ));
            }
            for world in &catalog.valid {
                spawn_button(
                    root,
                    &font,
                    &world.name,
                    UiAction::LoadWorld(world.id.clone()),
                );
            }
            spawn_button(root, &font, "BACK", UiAction::Back);
            spawn_status(root, &font);
        });
}

fn spawn_pause_menu(mut commands: Commands, ui_font: Res<UiFont>) {
    let font = ui_font.0.clone();
    commands
        .spawn((
            root_node(Color::srgba(0.015, 0.020, 0.030, 0.82)),
            PauseRoot,
            DespawnOnExit(AppState::Paused),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("PAUSED"),
                TextFont {
                    font: font.clone(),
                    font_size: FontSize::Px(62.0),
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
            spawn_button(root, &font, "RESUME", UiAction::Resume);
            spawn_button(root, &font, "SAVE & QUIT", UiAction::SaveAndQuit);
            spawn_button(root, &font, "RETRY SAVE", UiAction::RetrySave);
            spawn_button(
                root,
                &font,
                "QUIT WITHOUT SAVING",
                UiAction::QuitWithoutSaving,
            );
            spawn_status(root, &font);
        });
}

fn spawn_loading_screen(mut commands: Commands, ui_font: Res<UiFont>) {
    commands
        .spawn((
            root_node(Color::srgb(0.035, 0.055, 0.085)),
            LoadingRoot,
            DespawnOnExit(AppState::LoadingWorld),
        ))
        .with_child((
            Text::new("GENERATING WORLD..."),
            TextFont {
                font: ui_font.0.clone(),
                font_size: FontSize::Px(42.0),
                ..default()
            },
            TextColor(Color::WHITE),
        ));
}

fn spawn_saving_screen(mut commands: Commands, ui_font: Res<UiFont>) {
    commands
        .spawn((
            root_node(Color::srgba(0.015, 0.020, 0.030, 0.90)),
            SavingRoot,
            DespawnOnExit(AppState::Saving),
        ))
        .with_child((
            Text::new("SAVING WORLD..."),
            TextFont {
                font: ui_font.0.clone(),
                font_size: FontSize::Px(42.0),
                ..default()
            },
            TextColor(Color::WHITE),
        ));
}

fn spawn_hud(mut commands: Commands, ui_font: Res<UiFont>) {
    let font = ui_font.0.clone();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::End,
                padding: UiRect::bottom(px(20)),
                ..default()
            },
            HudRoot,
            DespawnOnExit(AppState::Playing),
        ))
        .with_children(|root| {
            root.spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|column| {
                column.spawn((
                    Text::new("Grass"),
                    TextFont {
                        font: font.clone(),
                        font_size: FontSize::Px(24.0),
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    SelectedItemText,
                ));
                column
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        ..default()
                    })
                    .with_children(|bar| {
                        for slot in 1..=8 {
                            bar.spawn((
                                Node {
                                    width: px(48),
                                    height: px(48),
                                    margin: UiRect::all(px(2)),
                                    border: UiRect::all(px(2)),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(0.03, 0.04, 0.05, 0.72)),
                                BorderColor::all(if slot == 1 {
                                    Color::WHITE
                                } else {
                                    Color::srgb(0.25, 0.28, 0.31)
                                }),
                                HotbarSlot(slot),
                            ))
                            .with_child((
                                Text::new(slot.to_string()),
                                TextFont {
                                    font: font.clone(),
                                    font_size: FontSize::Px(24.0),
                                    ..default()
                                },
                                TextColor(Color::WHITE),
                            ));
                        }
                    });
            });
        });
}

fn root_node(color: Color) -> (Node, BackgroundColor) {
    (
        Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: px(10),
            ..default()
        },
        BackgroundColor(color),
    )
}

fn spawn_button(
    parent: &mut ChildSpawnerCommands,
    font: &FontSource,
    label: &str,
    action: UiAction,
) {
    parent
        .spawn((
            Button,
            Node {
                width: px(300),
                height: px(56),
                border: UiRect::all(px(2)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(button_color(Interaction::None)),
            BorderColor::all(Color::srgb(0.40, 0.48, 0.52)),
            action,
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font: font.clone(),
                font_size: FontSize::Px(31.0),
                ..default()
            },
            TextColor(Color::WHITE),
        ));
}

fn spawn_status(parent: &mut ChildSpawnerCommands, font: &FontSource) {
    parent.spawn((
        Text::new(""),
        TextFont {
            font: font.clone(),
            font_size: FontSize::Px(24.0),
            ..default()
        },
        TextColor(Color::srgb(1.0, 0.66, 0.36)),
        StatusText,
    ));
}

fn handle_actions(
    interactions: Query<(&Interaction, &UiAction), Changed<Interaction>>,
    mut messages: MessageWriter<SessionCommand>,
) {
    for (interaction, action) in &interactions {
        if *interaction == Interaction::Pressed {
            messages.write(action.into());
        }
    }
}

fn style_buttons(mut buttons: ChangedButtonQuery) {
    for (interaction, mut background) in &mut buttons {
        background.0 = button_color(*interaction);
    }
}

fn button_color(interaction: Interaction) -> Color {
    match interaction {
        Interaction::Pressed => Color::srgb(0.23, 0.46, 0.50),
        Interaction::Hovered => Color::srgb(0.16, 0.31, 0.35),
        Interaction::None => Color::srgb(0.09, 0.16, 0.19),
    }
}

fn update_status_text(status: Res<UiStatus>, mut texts: Query<&mut Text, With<StatusText>>) {
    if !status.is_changed() {
        return;
    }
    for mut text in &mut texts {
        text.0.clone_from(&status.0);
    }
}

fn update_hotbar(
    player: Single<&Hotbar, With<Player>>,
    mut slots: Query<(&HotbarSlot, &mut BorderColor)>,
    mut labels: Query<&mut Text, With<SelectedItemText>>,
) {
    for (slot, mut border) in &mut slots {
        *border = BorderColor::all(if slot.0 == player.selected_slot {
            Color::WHITE
        } else {
            Color::srgb(0.25, 0.28, 0.31)
        });
    }
    for mut label in &mut labels {
        label.0 = player.selected_kind().display_name().into();
    }
}

fn pause_physics(mut physics_time: ResMut<Time<Physics>>) {
    physics_time.pause();
}

fn resume_physics(mut physics_time: ResMut<Time<Physics>>) {
    physics_time.unpause();
}

#[cfg(test)]
mod tests {
    #[test]
    fn environment_flags_accept_common_enabled_values() {
        for value in ["1", "true", "TRUE", "yes"] {
            assert!(matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            ));
        }
    }
}
