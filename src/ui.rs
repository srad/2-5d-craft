use crate::AppState;
use crate::lighting::DayCycle;
use crate::persistence::{StoreError, WorldSaveV1, WorldStore, blank_save};
use crate::player::{Hotbar, Player};
use crate::world::{PendingWorld, WorldData, WorldSession, spawn_for_seed};
use avian2d::prelude::{Physics, PhysicsTime};
use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, futures::check_ready};
use bevy::window::WindowCloseRequested;
use std::path::PathBuf;

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
    LoadWorld(PathBuf),
    Resume,
    SaveAndQuit,
    RetrySave,
    QuitWithoutSaving,
    Back,
    Quit,
}

#[derive(Component)]
struct StatusText;

#[derive(Component)]
struct HotbarSlot(u8);

#[derive(Component)]
struct SelectedItemText;

#[derive(Resource, Clone)]
struct UiFont(FontSource);

type ChangedButtonQuery<'w, 's> = Query<
    'w,
    's,
    (&'static Interaction, &'static mut BackgroundColor),
    (With<Button>, Changed<Interaction>),
>;

#[derive(Resource, Default)]
struct UiStatus(String);

#[derive(Resource)]
struct AutosaveTimer(Timer);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SaveDestination {
    None,
    MainMenu,
    Exit,
}

#[derive(Resource)]
struct SaveJob {
    task: Task<Result<(), StoreError>>,
    path: PathBuf,
    revision: u64,
    destination: SaveDestination,
}

impl Default for AutosaveTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(10.0, TimerMode::Repeating))
    }
}

pub struct GameUiPlugin;

impl Plugin for GameUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiStatus>()
            .init_resource::<AutosaveTimer>()
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
                (pause_physics, save_when_paused, spawn_pause_menu).chain(),
            )
            .add_systems(
                Update,
                (
                    poll_save_job,
                    handle_actions,
                    style_buttons,
                    update_status_text,
                    update_hotbar.run_if(in_state(AppState::Playing)),
                    autosave.run_if(in_state(AppState::Playing)),
                    handle_close_request,
                ),
            )
            .add_systems(Last, best_effort_exit_save);
    }

    fn finish(&self, app: &mut App) {
        let font = Font::from_bytes(include_bytes!("../assets/fonts/VT323-Regular.ttf").to_vec());
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

fn autostart_world(
    mut commands: Commands,
    store: Res<WorldStore>,
    mut status: ResMut<UiStatus>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if environment_flag("SIDECRAFT_AUTOSTART") {
        create_new_world(&mut commands, &store, &mut status, &mut next_state);
    }
}

fn environment_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| enabled_flag_value(&value))
}

fn enabled_flag_value(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
}

fn create_new_world(
    commands: &mut Commands,
    store: &WorldStore,
    status: &mut UiStatus,
    next_state: &mut NextState<AppState>,
) {
    status.0.clear();
    let seed = rand::random::<u64>();
    let spawn = spawn_for_seed(seed);
    let save = blank_save(
        seed,
        format!("World {:08X}", seed as u32),
        Vec::new(),
        spawn,
    );
    match store
        .new_path(seed)
        .and_then(|path| store.save(&path, &save).map(|()| path))
    {
        Ok(path) => {
            commands.insert_resource(PendingWorld { path, save });
            next_state.set(AppState::LoadingWorld);
        }
        Err(error) => status.0 = format!("Could not create world: {error}"),
    }
}

fn spawn_world_select(
    mut commands: Commands,
    ui_font: Res<UiFont>,
    store: Res<WorldStore>,
    mut status: ResMut<UiStatus>,
) {
    let font = ui_font.0.clone();
    let listed = store.list();
    status.0 = match &listed {
        Ok(list) if !list.invalid.is_empty() => {
            format!("{} invalid save file(s) were ignored", list.invalid.len())
        }
        Ok(_) => String::new(),
        Err(error) => error.to_string(),
    };
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
            if let Ok(list) = &listed {
                if list.valid.is_empty() {
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
                for listed_world in &list.valid {
                    spawn_button(
                        root,
                        &font,
                        &listed_world.save.name,
                        UiAction::LoadWorld(listed_world.path.clone()),
                    );
                }
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

#[allow(clippy::too_many_arguments)]
fn handle_actions(
    mut commands: Commands,
    interactions: Query<(&Interaction, &UiAction), Changed<Interaction>>,
    store: Res<WorldStore>,
    mut status: ResMut<UiStatus>,
    mut next_state: ResMut<NextState<AppState>>,
    mut exits: MessageWriter<AppExit>,
    world: Option<Res<WorldData>>,
    session: Option<Res<WorldSession>>,
    player: Query<(&Transform, &Hotbar), With<Player>>,
    day: Res<DayCycle>,
    mut save_job: Option<ResMut<SaveJob>>,
) {
    for (interaction, action) in &interactions {
        if !button_was_activated(*interaction) {
            continue;
        }
        match action {
            UiAction::NewWorld => {
                create_new_world(&mut commands, &store, &mut status, &mut next_state);
            }
            UiAction::ShowWorlds => next_state.set(AppState::WorldSelect),
            UiAction::LoadWorld(path) => match store.load(path) {
                Ok(save) => {
                    status.0.clear();
                    commands.insert_resource(PendingWorld {
                        path: path.clone(),
                        save,
                    });
                    next_state.set(AppState::LoadingWorld);
                }
                Err(error) => status.0 = format!("Could not load world: {error}"),
            },
            UiAction::Resume => next_state.set(AppState::Playing),
            UiAction::SaveAndQuit => {
                if let Some(job) = save_job.as_deref_mut() {
                    job.destination = SaveDestination::MainMenu;
                    next_state.set(AppState::Saving);
                } else if let (Some(world), Some(session)) = (world.as_deref(), session.as_deref())
                {
                    match queue_save(
                        &mut commands,
                        &store,
                        world,
                        session,
                        &player,
                        &day,
                        SaveDestination::MainMenu,
                    ) {
                        Ok(()) => next_state.set(AppState::Saving),
                        Err(error) => status.0 = format!("Save failed: {error}"),
                    }
                }
            }
            UiAction::RetrySave => {
                if save_job.is_none()
                    && let (Some(world), Some(session)) = (world.as_deref(), session.as_deref())
                {
                    status.0 = match queue_save(
                        &mut commands,
                        &store,
                        world,
                        session,
                        &player,
                        &day,
                        SaveDestination::None,
                    ) {
                        Ok(()) => "Saving...".into(),
                        Err(error) => format!("Save failed: {error}"),
                    }
                }
            }
            UiAction::QuitWithoutSaving | UiAction::Quit => {
                exits.write(AppExit::Success);
            }
            UiAction::Back => next_state.set(AppState::MainMenu),
        }
    }
}

fn button_was_activated(interaction: Interaction) -> bool {
    interaction == Interaction::Pressed
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

#[allow(clippy::too_many_arguments)]
fn save_when_paused(
    mut commands: Commands,
    store: Res<WorldStore>,
    world: Option<Res<WorldData>>,
    session: Option<Res<WorldSession>>,
    player: Query<(&Transform, &Hotbar), With<Player>>,
    day: Res<DayCycle>,
    save_job: Option<Res<SaveJob>>,
    mut status: ResMut<UiStatus>,
) {
    let (Some(world), Some(session)) = (world.as_deref(), session.as_deref()) else {
        return;
    };
    if save_job.is_some() || world.revision == session.saved_revision {
        return;
    }
    status.0 = match queue_save(
        &mut commands,
        &store,
        world,
        session,
        &player,
        &day,
        SaveDestination::None,
    ) {
        Ok(()) => "Saving...".into(),
        Err(error) => format!("Save failed: {error}"),
    };
}

#[allow(clippy::too_many_arguments)]
fn autosave(
    time: Res<Time<Real>>,
    mut timer: ResMut<AutosaveTimer>,
    store: Res<WorldStore>,
    world: Option<Res<WorldData>>,
    session: Option<Res<WorldSession>>,
    player: Query<(&Transform, &Hotbar), With<Player>>,
    day: Res<DayCycle>,
    save_job: Option<Res<SaveJob>>,
    mut status: ResMut<UiStatus>,
    mut commands: Commands,
) {
    if !timer.0.tick(time.delta()).just_finished() || save_job.is_some() {
        return;
    }
    let (Some(world), Some(session)) = (world.as_deref(), session.as_deref()) else {
        return;
    };
    if world.revision == session.saved_revision {
        return;
    }
    match queue_save(
        &mut commands,
        &store,
        world,
        session,
        &player,
        &day,
        SaveDestination::None,
    ) {
        Ok(()) => status.0 = "Autosaving...".into(),
        Err(error) => status.0 = format!("Autosave failed: {error}"),
    }
}

#[allow(clippy::too_many_arguments)]
fn poll_save_job(
    mut commands: Commands,
    mut save_job: Option<ResMut<SaveJob>>,
    store: Res<WorldStore>,
    world: Option<Res<WorldData>>,
    mut session: Option<ResMut<WorldSession>>,
    player: Query<(&Transform, &Hotbar), With<Player>>,
    day: Res<DayCycle>,
    mut status: ResMut<UiStatus>,
    mut next_state: ResMut<NextState<AppState>>,
    mut exits: MessageWriter<AppExit>,
) {
    let Some(job) = save_job.as_deref_mut() else {
        return;
    };
    let Some(result) = check_ready(&mut job.task) else {
        return;
    };
    let path = job.path.clone();
    let revision = job.revision;
    let destination = job.destination;
    commands.remove_resource::<SaveJob>();

    match result {
        Ok(()) => {
            if let Some(session) = session.as_deref_mut()
                && session.path == path
            {
                session.saved_revision = session.saved_revision.max(revision);
            }
            if destination != SaveDestination::None
                && let (Some(world), Some(session)) = (world.as_deref(), session.as_deref())
                && world.revision != revision
            {
                match queue_save(
                    &mut commands,
                    &store,
                    world,
                    session,
                    &player,
                    &day,
                    destination,
                ) {
                    Ok(()) => status.0 = "Saving latest changes...".into(),
                    Err(error) => {
                        status.0 = format!("Save failed: {error}");
                        next_state.set(AppState::Paused);
                    }
                }
                return;
            }
            status.0 = "World saved".into();
            match destination {
                SaveDestination::None => {}
                SaveDestination::MainMenu => next_state.set(AppState::MainMenu),
                SaveDestination::Exit => {
                    exits.write(AppExit::Success);
                }
            }
        }
        Err(error) => {
            status.0 = match destination {
                SaveDestination::Exit => format!("Save failed; close cancelled: {error}"),
                _ => format!("Save failed: {error}"),
            };
            if destination != SaveDestination::None {
                next_state.set(AppState::Paused);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_close_request(
    mut requests: MessageReader<WindowCloseRequested>,
    mut exits: MessageWriter<AppExit>,
    store: Res<WorldStore>,
    world: Option<Res<WorldData>>,
    mut session: Option<ResMut<WorldSession>>,
    player: Query<(&Transform, &Hotbar), With<Player>>,
    day: Res<DayCycle>,
    mut status: ResMut<UiStatus>,
    mut next_state: ResMut<NextState<AppState>>,
    mut save_job: Option<ResMut<SaveJob>>,
) {
    if requests.read().next().is_none() {
        return;
    }
    if let Some(job) = save_job.as_deref_mut() {
        job.destination = SaveDestination::Exit;
        next_state.set(AppState::Saving);
        return;
    }
    let result = match (world.as_deref(), session.as_deref_mut()) {
        (Some(world), Some(session)) if world.revision != session.saved_revision => {
            save_active(&store, world, session, &player, &day)
        }
        _ => Ok(()),
    };
    match result {
        Ok(()) => {
            exits.write(AppExit::Success);
        }
        Err(error) => {
            status.0 = format!("Save failed; close cancelled: {error}");
            next_state.set(AppState::Paused);
        }
    }
}

fn best_effort_exit_save(
    mut exits: MessageReader<AppExit>,
    store: Res<WorldStore>,
    world: Option<Res<WorldData>>,
    mut session: Option<ResMut<WorldSession>>,
    player: Query<(&Transform, &Hotbar), With<Player>>,
    day: Res<DayCycle>,
) {
    if exits.read().next().is_none() {
        return;
    }
    if let (Some(world), Some(session)) = (world.as_deref(), session.as_deref_mut())
        && world.revision != session.saved_revision
    {
        let _ = save_active(&store, world, session, &player, &day);
    }
}

fn save_active(
    store: &WorldStore,
    world: &WorldData,
    session: &mut WorldSession,
    player: &Query<(&Transform, &Hotbar), With<Player>>,
    day: &DayCycle,
) -> Result<(), StoreError> {
    let Some((transform, hotbar)) = player.iter().next() else {
        return Err(StoreError::Validation("active world has no player".into()));
    };
    let save = snapshot_world(world, session, transform, *hotbar, day)?;
    store.save(&session.path, &save)?;
    session.saved_revision = world.revision;
    Ok(())
}

fn queue_save(
    commands: &mut Commands,
    store: &WorldStore,
    world: &WorldData,
    session: &WorldSession,
    player: &Query<(&Transform, &Hotbar), With<Player>>,
    day: &DayCycle,
    destination: SaveDestination,
) -> Result<(), StoreError> {
    let Some((transform, hotbar)) = player.iter().next() else {
        return Err(StoreError::Validation("active world has no player".into()));
    };
    let save = snapshot_world(world, session, transform, *hotbar, day)?;
    let path = session.path.clone();
    let task_store = store.clone();
    let task_path = path.clone();
    let task = IoTaskPool::get().spawn(async move { task_store.save(&task_path, &save) });
    commands.insert_resource(SaveJob {
        task,
        path,
        revision: world.revision,
        destination,
    });
    Ok(())
}

fn snapshot_world(
    world: &WorldData,
    session: &WorldSession,
    player: &Transform,
    hotbar: Hotbar,
    day: &DayCycle,
) -> Result<WorldSaveV1, StoreError> {
    Ok(WorldSaveV1 {
        schema_version: crate::SAVE_SCHEMA_VERSION,
        generator_version: session.generator_version,
        height: world.grid.height(),
        name: session.name.clone(),
        seed: session.seed,
        created_at_unix_s: session.created_at_unix_s,
        last_played_unix_s: WorldStore::now_unix_s()?,
        day_phase: day.phase,
        player: crate::SavedPlayer {
            chunk_x: world.origin_chunk
                + (player.translation.x.floor() as i64).div_euclid(i64::from(crate::CHUNK_WIDTH)),
            local_x: player.translation.x.rem_euclid(crate::CHUNK_WIDTH as f32),
            y: player.translation.y,
            selected_slot: hotbar.selected_slot,
        },
        chunks: world.saved_chunks(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlockKind;
    use crate::world::{BlockChunk, BlockGrid};

    #[test]
    fn button_states_have_distinct_colors() {
        assert_ne!(
            button_color(Interaction::None),
            button_color(Interaction::Hovered)
        );
        assert_ne!(
            button_color(Interaction::Hovered),
            button_color(Interaction::Pressed)
        );
    }

    #[test]
    fn actions_only_fire_for_pressed_buttons() {
        assert!(!button_was_activated(Interaction::None));
        assert!(!button_was_activated(Interaction::Hovered));
        assert!(button_was_activated(Interaction::Pressed));
    }

    #[test]
    fn e2e_flags_accept_only_explicit_enabled_values() {
        for value in ["1", "true", "TRUE", "yes", "Yes"] {
            assert!(enabled_flag_value(value));
        }
        for value in ["", "0", "false", "on", "anything"] {
            assert!(!enabled_flag_value(value));
        }
    }

    #[test]
    fn snapshot_contains_runtime_state_and_dense_chunks() {
        let mut grid = BlockGrid::new(crate::WORLD_HEIGHT);
        grid.insert_chunk(
            BlockChunk::from_dense(
                0,
                vec![0; (crate::CHUNK_WIDTH * crate::WORLD_HEIGHT) as usize],
            )
            .unwrap(),
        );
        grid.set(IVec2::new(2, 1), BlockKind::Dirt);
        grid.set(IVec2::new(0, 0), BlockKind::Bedrock);
        let mut world = WorldData::for_test(grid, 9);
        world.revision = 4;
        let session = WorldSession {
            path: PathBuf::from("worlds/test.scw"),
            name: "Test".into(),
            seed: 9,
            generator_version: crate::GENERATOR_VERSION,
            created_at_unix_s: 1,
            saved_revision: 3,
        };
        let transform = Transform::from_xyz(3.0, 4.0, 0.0);
        let day = DayCycle {
            phase: 0.75,
            ..default()
        };
        let save = snapshot_world(
            &world,
            &session,
            &transform,
            Hotbar { selected_slot: 8 },
            &day,
        )
        .unwrap();
        assert_eq!(save.player.selected_slot, 8);
        assert_eq!(save.day_phase, 0.75);
        assert_eq!(save.chunks.len(), 1);
        assert_eq!(save.chunks[0].x, 0);
        assert_eq!(save.chunks[0].blocks[0], BlockKind::Bedrock.code());
    }
}
