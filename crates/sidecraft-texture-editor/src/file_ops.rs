use crate::{document::EditorDocument, generation::GenerationCoordinator};
use bevy::{
    app::AppExit,
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
    window::WindowCloseRequested,
};
use rfd::FileDialog;
use sidecraft_textures::{
    TextureProject, load_texture_project, random_seed, save_texture_project, write_generated_pack,
};
use std::{collections::VecDeque, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PendingAction {
    New,
    Open,
    Exit,
}

#[derive(Debug)]
pub(crate) enum EditorCommand {
    Perform(PendingAction),
    Save {
        choose_path: bool,
        then: Option<PendingAction>,
    },
    Export,
}

#[derive(Default, Resource)]
pub(crate) struct EditorCommands(pub VecDeque<EditorCommand>);

#[derive(Default, Resource)]
pub(crate) struct Confirmation(pub Option<PendingAction>);

#[derive(Default, Resource)]
pub(crate) struct EditorStatus {
    pub message: Option<String>,
    pub last_export: Option<PathBuf>,
}

#[derive(Default, Resource)]
pub(crate) struct ExportCoordinator {
    active: Option<Task<Result<PathBuf, String>>>,
}

impl ExportCoordinator {
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }
}

pub(crate) fn handle_close_requests(
    mut requests: MessageReader<WindowCloseRequested>,
    document: Res<EditorDocument>,
    export: Res<ExportCoordinator>,
    mut confirmation: ResMut<Confirmation>,
    mut commands: ResMut<EditorCommands>,
    mut status: ResMut<EditorStatus>,
) {
    if requests.read().next().is_none() {
        return;
    }
    if export.is_active() {
        status.message = Some("Finish exporting before closing the editor.".into());
    } else if document.is_dirty() {
        confirmation.0 = Some(PendingAction::Exit);
    } else {
        commands
            .0
            .push_back(EditorCommand::Perform(PendingAction::Exit));
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn process_commands(
    mut commands: ResMut<EditorCommands>,
    mut document: ResMut<EditorDocument>,
    mut generation: ResMut<GenerationCoordinator>,
    mut export: ResMut<ExportCoordinator>,
    mut status: ResMut<EditorStatus>,
    mut exits: MessageWriter<AppExit>,
) {
    if export.is_active() {
        return;
    }
    let Some(command) = commands.0.pop_front() else {
        return;
    };
    match command {
        EditorCommand::Perform(PendingAction::New) => {
            document.replace(TextureProject::randomized(random_seed()), None);
            generation.request_now(document.revision);
            status.message = Some("Created a new randomized project.".into());
        }
        EditorCommand::Perform(PendingAction::Open) => {
            let Some(path) = project_dialog().pick_file() else {
                return;
            };
            match load_texture_project(&path) {
                Ok(project) => {
                    document.replace(project, Some(path.clone()));
                    generation.request_now(document.revision);
                    status.message = Some(format!("Opened {}.", path.display()));
                }
                Err(error) => status.message = Some(format!("Open failed: {error}")),
            }
        }
        EditorCommand::Perform(PendingAction::Exit) => {
            exits.write(AppExit::Success);
        }
        EditorCommand::Save { choose_path, then } => {
            let path = if choose_path || document.path.is_none() {
                project_dialog()
                    .set_file_name(format!("{}.sctex.toml", document.project.pack.id))
                    .save_file()
            } else {
                document.path.clone()
            };
            let Some(path) = path else {
                return;
            };
            match save_texture_project(&document.project, &path) {
                Ok(path) => {
                    document.mark_saved(path.clone());
                    status.message = Some(format!("Saved {}.", path.display()));
                    if let Some(action) = then {
                        commands.0.push_front(EditorCommand::Perform(action));
                    }
                }
                Err(error) => status.message = Some(format!("Save failed: {error}")),
            }
        }
        EditorCommand::Export => {
            let Some(pack) = generation.generated.clone() else {
                status.message = Some("Generate the current project before exporting.".into());
                return;
            };
            let Some(folder) = FileDialog::new()
                .set_title("Export texture pack into folder")
                .pick_folder()
            else {
                return;
            };
            status.message = Some("Exporting texture pack…".into());
            export.active = Some(AsyncComputeTaskPool::get().spawn(async move {
                write_generated_pack(&pack, &folder).map_err(|error| error.to_string())
            }));
        }
    }
}

pub(crate) fn poll_export(mut export: ResMut<ExportCoordinator>, mut status: ResMut<EditorStatus>) {
    let Some(task) = export.active.as_mut() else {
        return;
    };
    let Some(result) = check_ready(task) else {
        return;
    };
    export.active = None;
    match result {
        Ok(path) => {
            status.last_export = Some(path.clone());
            status.message = Some(format!("Exported {}.", path.display()));
        }
        Err(error) => status.message = Some(format!("Export failed: {error}")),
    }
}

fn project_dialog() -> FileDialog {
    FileDialog::new()
        .set_title("Sidecraft texture project")
        .add_filter("Sidecraft texture project", &["toml"])
}
