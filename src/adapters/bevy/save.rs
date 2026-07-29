use std::time::{SystemTime, UNIX_EPOCH};

use bevy::{
    app::AppExit,
    prelude::*,
    tasks::{IoTaskPool, Task, futures::check_ready},
    window::WindowCloseRequested,
};

use crate::{
    AppState,
    adapters::bevy::{
        DayCycleResource, RepositoryHandle, RuntimeSet, SaveCoordinatorResource,
        WorldSessionResource, WorldStateResource,
        player::{Hotbar, Player},
        ui::UiStatus,
    },
    application::{
        SaveCompletion, SaveDecision, SaveDestination, SaveTicket, SaveVersion, WorldSnapshot,
    },
};

const CLOCK_AUTOSAVE_TICKS: u64 = 1_200;

#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct SaveRequest(pub(crate) SaveDestination);

#[derive(Resource)]
struct SaveJob {
    task: Task<Result<(), String>>,
    version: SaveVersion,
}

#[derive(Resource)]
struct AutosaveTimer(Timer);

impl Default for AutosaveTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(10.0, TimerMode::Repeating))
    }
}

pub(crate) struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SaveRequest>()
            .init_resource::<SaveCoordinatorResource>()
            .init_resource::<AutosaveTimer>()
            .add_systems(OnEnter(AppState::Paused), save_when_paused)
            .add_systems(
                Update,
                (
                    autosave.run_if(in_state(AppState::Playing)),
                    handle_close_request,
                    handle_save_requests,
                    poll_save_job,
                )
                    .chain()
                    .in_set(RuntimeSet::Persistence),
            );
    }
}

fn save_when_paused(
    world: Option<Res<WorldStateResource>>,
    session: Option<Res<WorldSessionResource>>,
    day: Res<DayCycleResource>,
    mut requests: MessageWriter<SaveRequest>,
) {
    if let (Some(world), Some(session)) = (world, session)
        && current_version(&world, &day) != session.saved_version
    {
        requests.write(SaveRequest(SaveDestination::Background));
    }
}

fn autosave(
    time: Res<Time<Real>>,
    mut timer: ResMut<AutosaveTimer>,
    world: Option<Res<WorldStateResource>>,
    session: Option<Res<WorldSessionResource>>,
    day: Res<DayCycleResource>,
    mut requests: MessageWriter<SaveRequest>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    if let (Some(world), Some(session)) = (world, session)
        && needs_autosave(current_version(&world, &day), session.saved_version)
    {
        requests.write(SaveRequest(SaveDestination::Background));
    }
}

fn handle_close_request(
    mut close_requests: MessageReader<WindowCloseRequested>,
    world: Option<Res<WorldStateResource>>,
    session: Option<Res<WorldSessionResource>>,
    day: Res<DayCycleResource>,
    mut save_requests: MessageWriter<SaveRequest>,
    mut next_state: ResMut<NextState<AppState>>,
    mut exits: MessageWriter<AppExit>,
) {
    if close_requests.read().next().is_none() {
        return;
    }
    if let (Some(world), Some(session)) = (world, session)
        && current_version(&world, &day) != session.saved_version
    {
        save_requests.write(SaveRequest(SaveDestination::Exit));
        next_state.set(AppState::Saving);
    } else {
        exits.write(AppExit::Success);
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_save_requests(
    mut commands: Commands,
    mut requests: MessageReader<SaveRequest>,
    mut coordinator: ResMut<SaveCoordinatorResource>,
    repository: Res<RepositoryHandle>,
    world: Option<Res<WorldStateResource>>,
    session: Option<Res<WorldSessionResource>>,
    player: Query<(&Transform, &Hotbar), With<Player>>,
    day: Res<DayCycleResource>,
    mut status: ResMut<UiStatus>,
    mut next_state: ResMut<NextState<AppState>>,
    mut exits: MessageWriter<AppExit>,
) {
    let Some(destination) = requests.read().map(|request| request.0).max() else {
        return;
    };
    let (Some(world), Some(session)) = (world, session) else {
        finish_destination(destination, &mut next_state, &mut exits);
        return;
    };
    let version = current_version(&world, &day);
    if destination == SaveDestination::Background
        && version == session.saved_version
        && coordinator.ticket().is_none()
    {
        finish_destination(destination, &mut next_state, &mut exits);
        return;
    }
    let decision = coordinator.request(session.id.clone(), version, destination);
    if let SaveDecision::Start(ticket) = decision
        && let Err(error) = start_save(
            &mut commands,
            &repository,
            &world,
            &session,
            &player,
            &day,
            &ticket,
        )
    {
        coordinator.complete(SaveCompletion::Failed, version);
        status.0 = format!("Save failed: {error}");
        if destination != SaveDestination::Background {
            next_state.set(AppState::Paused);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn poll_save_job(
    mut commands: Commands,
    mut job: Option<ResMut<SaveJob>>,
    mut coordinator: ResMut<SaveCoordinatorResource>,
    repository: Res<RepositoryHandle>,
    world: Option<Res<WorldStateResource>>,
    mut session: Option<ResMut<WorldSessionResource>>,
    player: Query<(&Transform, &Hotbar), With<Player>>,
    day: Res<DayCycleResource>,
    mut status: ResMut<UiStatus>,
    mut next_state: ResMut<NextState<AppState>>,
    mut exits: MessageWriter<AppExit>,
) {
    let Some(job) = job.as_mut() else {
        return;
    };
    let Some(result) = check_ready(&mut job.task) else {
        return;
    };
    let version = job.version;
    commands.remove_resource::<SaveJob>();
    let current_version = world
        .as_ref()
        .map_or(version, |world| current_version(world, &day));
    let completion = if result.is_ok() {
        if let Some(session) = session.as_mut() {
            session.saved_version = version;
        }
        SaveCompletion::Succeeded
    } else {
        SaveCompletion::Failed
    };
    match coordinator.complete(completion, current_version) {
        SaveDecision::Start(ticket) => {
            let (Some(world), Some(session)) = (world, session.as_deref()) else {
                return;
            };
            if let Err(error) = start_save(
                &mut commands,
                &repository,
                &world,
                session,
                &player,
                &day,
                &ticket,
            ) {
                coordinator.complete(SaveCompletion::Failed, current_version);
                status.0 = format!("Save failed: {error}");
                next_state.set(AppState::Paused);
            } else {
                status.0 = "Saving latest changes...".into();
            }
        }
        SaveDecision::Finish(destination) => {
            status.0 = "World saved".into();
            finish_destination(destination, &mut next_state, &mut exits);
        }
        SaveDecision::Failed(destination) => {
            status.0 = format!(
                "Save failed{}",
                result
                    .err()
                    .map_or(String::new(), |error| format!(": {error}"))
            );
            if destination != SaveDestination::Background {
                next_state.set(AppState::Paused);
            }
        }
        SaveDecision::Idle => {}
    }
}

fn start_save(
    commands: &mut Commands,
    repository: &RepositoryHandle,
    world: &WorldStateResource,
    session: &WorldSessionResource,
    player: &Query<(&Transform, &Hotbar), With<Player>>,
    day: &DayCycleResource,
    ticket: &SaveTicket,
) -> Result<(), String> {
    let Some((transform, hotbar)) = player.iter().next() else {
        return Err("active world has no player".into());
    };
    let snapshot = snapshot_world(world, session, transform, *hotbar, day);
    let repository = repository.0.clone();
    let id = ticket.id.clone();
    let task = IoTaskPool::get().spawn(async move {
        repository
            .save(&id, &snapshot)
            .map_err(|error| error.to_string())
    });
    commands.insert_resource(SaveJob {
        task,
        version: ticket.version,
    });
    Ok(())
}

fn snapshot_world(
    world: &WorldStateResource,
    session: &WorldSessionResource,
    player: &Transform,
    hotbar: Hotbar,
    day: &DayCycleResource,
) -> WorldSnapshot {
    world.snapshot(
        session,
        player.translation.truncate(),
        hotbar.selected_slot,
        day.day_time_ticks(),
        now_unix_s(),
    )
}

fn current_version(world: &WorldStateResource, day: &DayCycleResource) -> SaveVersion {
    SaveVersion {
        world_revision: world.revision(),
        day_time_ticks: day.day_time_ticks(),
    }
}

fn needs_autosave(current: SaveVersion, saved: SaveVersion) -> bool {
    current.world_revision != saved.world_revision
        || current.day_time_ticks.saturating_sub(saved.day_time_ticks) >= CLOCK_AUTOSAVE_TICKS
}

fn finish_destination(
    destination: SaveDestination,
    next_state: &mut NextState<AppState>,
    exits: &mut MessageWriter<AppExit>,
) {
    match destination {
        SaveDestination::Background => {}
        SaveDestination::MainMenu => next_state.set(AppState::MainMenu),
        SaveDestination::Exit => {
            exits.write(AppExit::Success);
        }
    }
}

fn now_unix_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_only_autosave_waits_one_minute() {
        let saved = SaveVersion {
            world_revision: 8,
            day_time_ticks: 50,
        };
        assert!(!needs_autosave(
            SaveVersion {
                day_time_ticks: 1_249,
                ..saved
            },
            saved
        ));
        assert!(needs_autosave(
            SaveVersion {
                day_time_ticks: 1_250,
                ..saved
            },
            saved
        ));
    }

    #[test]
    fn block_change_requests_autosave_immediately() {
        assert!(needs_autosave(
            SaveVersion {
                world_revision: 2,
                day_time_ticks: 100,
            },
            SaveVersion {
                world_revision: 1,
                day_time_ticks: 100,
            }
        ));
    }
}
