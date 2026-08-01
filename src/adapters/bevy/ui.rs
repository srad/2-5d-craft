use avian2d::prelude::{Physics, PhysicsTime};
use bevy::prelude::*;

use crate::{
    AppState,
    adapters::bevy::{
        RuntimeSet, SimulationDebugVisible, SimulationDiagnostics, environment_flag,
        interaction::ActiveVoxelLayer,
        player::{Hotbar, Player},
        rendering::RenderCatalog,
        session::SessionCommand,
    },
    application::WorldId,
    domain::{BlockState, VoxelLayer},
};

mod menus;

#[derive(Component)]
struct LoadingRoot;

#[derive(Component)]
struct HudRoot;

#[derive(Component)]
struct PauseRoot;

#[derive(Component)]
struct SavingRoot;

#[derive(Component, Debug, Clone)]
pub(super) enum UiAction {
    NewWorld,
    ShowWorlds,
    ShowMainMenu,
    ShowSettings,
    ShowTexturePacks,
    SelectTexturePack(String),
    TexturePackPage(i32),
    ApplyTexturePack,
    LoadWorld(WorldId),
    Resume,
    SaveAndQuit,
    RetrySave,
    QuitWithoutSaving,
    Quit,
}

impl UiAction {
    fn session_command(&self) -> Option<SessionCommand> {
        match self {
            UiAction::NewWorld => Some(SessionCommand::NewWorld),
            UiAction::ShowWorlds => Some(SessionCommand::ShowWorlds),
            UiAction::LoadWorld(id) => Some(SessionCommand::LoadWorld(id.clone())),
            UiAction::Resume => Some(SessionCommand::Resume),
            UiAction::SaveAndQuit => Some(SessionCommand::SaveAndQuit),
            UiAction::RetrySave => Some(SessionCommand::RetrySave),
            UiAction::QuitWithoutSaving => Some(SessionCommand::QuitWithoutSaving),
            UiAction::Quit => Some(SessionCommand::Quit),
            UiAction::ShowMainMenu
            | UiAction::ShowSettings
            | UiAction::ShowTexturePacks
            | UiAction::SelectTexturePack(_)
            | UiAction::TexturePackPage(_)
            | UiAction::ApplyTexturePack => None,
        }
    }
}

#[derive(Component)]
struct StatusText;

#[derive(Component)]
struct HotbarSlot(u8);

#[derive(Component)]
struct SelectedItemText;

#[derive(Component)]
struct ActiveLayerText;

#[derive(Component)]
struct DebugOverlayRoot;

#[derive(Component)]
struct SimulationDebugText;

#[derive(Resource, Clone)]
pub(super) struct UiFont(pub(super) FontSource);

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
            // Acceptance runs need the overlay without a keypress, so a screenshot can be
            // compared against the session log.
            .insert_resource(SimulationDebugVisible(environment_flag(
                "SIDECRAFT_DEBUG_OVERLAY",
            )))
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
                    handle_session_actions,
                    style_buttons,
                    update_status_text,
                    update_hotbar.run_if(in_state(AppState::Playing)),
                    update_active_layer_text.run_if(in_state(AppState::Playing)),
                ),
            )
            // Derived presentation: the overlay reports the tick the simulation just advanced, so
            // it must be ordered after `RuntimeSet::Simulation` rather than race it.
            .add_systems(
                Update,
                (toggle_simulation_debug, update_simulation_debug_text)
                    .chain()
                    .in_set(RuntimeSet::Derived)
                    .run_if(in_state(AppState::Playing)),
            );
        menus::register(app);
    }

    fn finish(&self, app: &mut App) {
        let font =
            Font::from_bytes(include_bytes!("../../../assets/fonts/VT323-Regular.ttf").to_vec());
        let handle = app.world_mut().resource_mut::<Assets<Font>>().add(font);
        app.insert_resource(UiFont(handle.into()));
    }
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
    let font = ui_font.0.clone();
    commands
        .spawn((
            front_end_root_node(),
            LoadingRoot,
            DespawnOnExit(AppState::LoadingWorld),
        ))
        .with_children(|root| {
            root.spawn(menu_panel_node(420.0)).with_child((
                Text::new("GENERATING WORLD..."),
                TextFont {
                    font: font.clone(),
                    font_size: FontSize::Px(42.0),
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
            spawn_version(root, &font);
        });
}

fn spawn_saving_screen(mut commands: Commands, ui_font: Res<UiFont>) {
    let font = ui_font.0.clone();
    commands
        .spawn((
            front_end_root_node(),
            SavingRoot,
            DespawnOnExit(AppState::Saving),
        ))
        .with_children(|root| {
            root.spawn(menu_panel_node(420.0)).with_child((
                Text::new("SAVING WORLD..."),
                TextFont {
                    font: font.clone(),
                    font_size: FontSize::Px(42.0),
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
            spawn_version(root, &font);
        });
}

fn spawn_hud(
    mut commands: Commands,
    ui_font: Res<UiFont>,
    render_catalog: Res<RenderCatalog>,
    active_layer: Res<ActiveVoxelLayer>,
    debug_visible: Res<SimulationDebugVisible>,
) {
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
                    Text::new(active_layer_label(active_layer.0)),
                    TextFont {
                        font: font.clone(),
                        font_size: FontSize::Px(20.0),
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    ActiveLayerText,
                ));
                column.spawn((
                    Text::new("Dirt"),
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
                        for slot in 1..=BlockState::HOTBAR.len() as u8 {
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
                            .with_children(|slot_node| {
                                slot_node.spawn((
                                    ImageNode::new(
                                        render_catalog.hotbar_icons[usize::from(slot - 1)].clone(),
                                    ),
                                    Node {
                                        width: px(32),
                                        height: px(32),
                                        ..default()
                                    },
                                ));
                                slot_node.spawn((
                                    Text::new(slot.to_string()),
                                    TextFont {
                                        font: font.clone(),
                                        font_size: FontSize::Px(16.0),
                                        ..default()
                                    },
                                    TextColor(Color::WHITE),
                                    Node {
                                        position_type: PositionType::Absolute,
                                        right: px(2),
                                        bottom: px(0),
                                        ..default()
                                    },
                                ));
                            });
                        }
                    });
            });
        });
    spawn_debug_overlay(&mut commands, &font, debug_visible.0);
}

/// Debug readouts live in their own top-left column, away from the gameplay HUD, so they stay
/// legible and future diagnostics can stack under the same anchor.
fn spawn_debug_overlay(commands: &mut Commands, font: &FontSource, visible: bool) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(12),
                left: px(12),
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            },
            DebugOverlayRoot,
            DespawnOnExit(AppState::Playing),
        ))
        .with_children(|column| {
            column.spawn((
                Text::new(String::new()),
                TextFont {
                    font: font.clone(),
                    font_size: FontSize::Px(24.0),
                    ..default()
                },
                TextColor(Color::srgb(0.86, 0.91, 0.95)),
                if visible {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
                SimulationDebugText,
            ));
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

pub(super) fn front_end_root_node() -> (Node, BackgroundColor) {
    (
        Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::FlexStart,
            padding: UiRect::horizontal(px(40)),
            ..default()
        },
        BackgroundColor(Color::NONE),
    )
}

pub(super) fn menu_panel_node(width: f32) -> (Node, BackgroundColor, BorderColor) {
    (
        Node {
            width: px(width),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(10),
            padding: UiRect::all(px(24)),
            border: UiRect::all(px(2)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.025, 0.045, 0.055, 0.88)),
        BorderColor::all(Color::srgba(0.42, 0.54, 0.48, 0.82)),
    )
}

pub(super) fn spawn_button(
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

pub(super) fn spawn_compact_button(
    parent: &mut ChildSpawnerCommands,
    font: &FontSource,
    label: &str,
    action: UiAction,
) {
    parent
        .spawn((
            Button,
            Node {
                width: px(440),
                height: px(38),
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
                font_size: FontSize::Px(22.0),
                ..default()
            },
            TextColor(Color::WHITE),
        ));
}

pub(super) fn spawn_status(parent: &mut ChildSpawnerCommands, font: &FontSource) {
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

pub(super) fn spawn_version(parent: &mut ChildSpawnerCommands, font: &FontSource) {
    parent.spawn((
        Text::new(format!("SIDECRAFT v{}", env!("CARGO_PKG_VERSION"))),
        TextFont {
            font: font.clone(),
            font_size: FontSize::Px(20.0),
            ..default()
        },
        TextColor(Color::srgba(0.86, 0.90, 0.84, 0.88)),
        Node {
            position_type: PositionType::Absolute,
            right: px(18),
            bottom: px(12),
            ..default()
        },
    ));
}

fn handle_session_actions(
    interactions: Query<(&Interaction, &UiAction), Changed<Interaction>>,
    mut messages: MessageWriter<SessionCommand>,
) {
    for (interaction, action) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if let Some(command) = action.session_command() {
            messages.write(command);
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
        label.0 = player.selected_state().display_name().into();
    }
}

fn update_active_layer_text(
    active_layer: Res<ActiveVoxelLayer>,
    mut labels: Query<&mut Text, With<ActiveLayerText>>,
) {
    if !active_layer.is_changed() {
        return;
    }
    for mut label in &mut labels {
        label.0 = active_layer_label(active_layer.0).into();
    }
}

fn toggle_simulation_debug(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut visible: ResMut<SimulationDebugVisible>,
    mut texts: Query<&mut Visibility, With<SimulationDebugText>>,
) {
    if !keyboard.just_pressed(KeyCode::F3) {
        return;
    }
    visible.0 = !visible.0;
    for mut visibility in &mut texts {
        *visibility = if visible.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

fn update_simulation_debug_text(
    visible: Res<SimulationDebugVisible>,
    diagnostics: Res<SimulationDiagnostics>,
    mut texts: Query<&mut Text, With<SimulationDebugText>>,
) {
    if !visible.0 {
        return;
    }
    for mut text in &mut texts {
        text.0 = simulation_debug_label(&diagnostics);
    }
}

fn simulation_debug_label(diagnostics: &SimulationDiagnostics) -> String {
    format!(
        "tick {} | peak steps {}/4 | processed {} | queued {} | active {}+{} [F3]",
        diagnostics.world_tick,
        diagnostics.max_steps_per_frame,
        diagnostics.processed_total,
        diagnostics.queued_ticks,
        diagnostics.simulated_chunks,
        diagnostics.ticking_areas,
    )
}

fn active_layer_label(layer: VoxelLayer) -> &'static str {
    match layer {
        VoxelLayer::Foreground => "Layer: Foreground [Tab]",
        VoxelLayer::Backwall => "Layer: Backwall [Tab]",
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
    use super::*;

    #[test]
    fn environment_flags_accept_common_enabled_values() {
        for value in ["1", "true", "TRUE", "yes"] {
            assert!(matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            ));
        }
    }

    #[test]
    fn texture_pack_actions_stay_out_of_the_world_session() {
        assert!(UiAction::ShowTexturePacks.session_command().is_none());
        assert!(
            UiAction::SelectTexturePack("default".into())
                .session_command()
                .is_none()
        );
        assert!(UiAction::ApplyTexturePack.session_command().is_none());
        assert!(UiAction::ShowSettings.session_command().is_none());
    }

    #[test]
    fn active_layer_labels_expose_the_tab_binding() {
        assert_eq!(
            active_layer_label(VoxelLayer::Foreground),
            "Layer: Foreground [Tab]"
        );
        assert_eq!(
            active_layer_label(VoxelLayer::Backwall),
            "Layer: Backwall [Tab]"
        );
    }
}
