use std::time::{SystemTime, UNIX_EPOCH};

use bevy::app::AppExit;
use bevy::prelude::*;

use crate::{
    AppState,
    adapters::bevy::{
        PendingWorldResource, RepositoryHandle, WorldCatalogResource, configured_autostart_u64,
        save::SaveRequest, ui::UiStatus,
    },
    application::{SaveDestination, WorldId, blank_snapshot},
    domain::spawn_for_seed,
};

#[derive(Message, Debug, Clone)]
pub(crate) enum SessionCommand {
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

pub(crate) struct SessionPlugin;

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SessionCommand>()
            .add_systems(Update, handle_session_commands);
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_session_commands(
    mut commands: Commands,
    mut messages: MessageReader<SessionCommand>,
    repository: Res<RepositoryHandle>,
    mut catalog: ResMut<WorldCatalogResource>,
    mut status: ResMut<UiStatus>,
    mut next_state: ResMut<NextState<AppState>>,
    mut save_requests: MessageWriter<SaveRequest>,
    mut exits: MessageWriter<AppExit>,
) {
    for message in messages.read() {
        match message {
            SessionCommand::NewWorld => {
                let seed = configured_autostart_u64("SIDECRAFT_TEST_SEED")
                    .unwrap_or_else(rand::random::<u64>);
                let snapshot = blank_snapshot(
                    seed,
                    format!("World {:08X}", seed as u32),
                    Vec::new(),
                    spawn_for_seed(seed),
                    now_unix_s(),
                );
                match repository
                    .0
                    .next_available_id(seed)
                    .and_then(|id| repository.0.save(&id, &snapshot).map(|()| id))
                {
                    Ok(id) => {
                        status.0.clear();
                        commands.insert_resource(PendingWorldResource(
                            crate::application::PendingWorld { id, snapshot },
                        ));
                        next_state.set(AppState::LoadingWorld);
                    }
                    Err(error) => status.0 = format!("Could not create world: {error}"),
                }
            }
            SessionCommand::ShowWorlds => match repository.0.list() {
                Ok(list) => {
                    status.0 = if list.invalid.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "{} invalid save package(s) were ignored",
                            list.invalid.len()
                        )
                    };
                    catalog.0 = list;
                    next_state.set(AppState::WorldSelect);
                }
                Err(error) => status.0 = format!("Could not list worlds: {error}"),
            },
            SessionCommand::LoadWorld(id) => match repository.0.load(id) {
                Ok(snapshot) => {
                    status.0.clear();
                    commands.insert_resource(PendingWorldResource(
                        crate::application::PendingWorld {
                            id: id.clone(),
                            snapshot,
                        },
                    ));
                    next_state.set(AppState::LoadingWorld);
                }
                Err(error) => status.0 = format!("Could not load world: {error}"),
            },
            SessionCommand::Resume => next_state.set(AppState::Playing),
            SessionCommand::SaveAndQuit => {
                save_requests.write(SaveRequest(SaveDestination::MainMenu));
                next_state.set(AppState::Saving);
            }
            SessionCommand::RetrySave => {
                save_requests.write(SaveRequest(SaveDestination::Background));
            }
            SessionCommand::QuitWithoutSaving | SessionCommand::Quit => {
                exits.write(AppExit::Success);
            }
            SessionCommand::Back => next_state.set(AppState::MainMenu),
        }
    }
}

fn now_unix_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
