use crate::{document::EditorDocument, generation::GenerationCoordinator};
use bevy::{
    app::AppExit,
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
    window::WindowCloseRequested,
};
use rfd::FileDialog;
use sidecraft_textures::{
    TextureProject, USER_PACK_ROOT, load_texture_project, random_seed, save_texture_project,
    slugify_pack_id, write_generated_pack,
};
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
};

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
    pub error: Option<String>,
    pub last_export: Option<PathBuf>,
}

impl EditorStatus {
    /// Reports a failed action.
    ///
    /// The status bar alone was not enough: it is one uncoloured line that the
    /// next message overwrites, so a failed export read almost exactly like a
    /// successful one. Failures now also raise a dialog and reach the console.
    pub fn fail(&mut self, detail: String) {
        error!("{detail}");
        self.message = Some(detail.clone());
        self.error = Some(detail);
    }

    /// Reports a finished action, retiring any dialog the last one raised.
    pub fn succeed(&mut self, message: String) {
        self.message = Some(message);
        self.error = None;
    }
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
            status.succeed("Created a new randomized project.".into());
        }
        EditorCommand::Perform(PendingAction::Open) => {
            let Some(path) = project_dialog().pick_file() else {
                return;
            };
            match load_texture_project(&path) {
                Ok(project) => {
                    document.replace(project, Some(path.clone()));
                    generation.request_now(document.revision);
                    status.succeed(format!("Opened {}.", path.display()));
                }
                Err(error) => status.fail(format!("Open failed: {error}")),
            }
        }
        EditorCommand::Perform(PendingAction::Exit) => {
            exits.write(AppExit::Success);
        }
        EditorCommand::Save { choose_path, then } => {
            let path = if choose_path || document.path.is_none() {
                project_dialog()
                    .set_file_name(format!(
                        "{}.sctex.toml",
                        slugify_pack_id(&document.project.pack.name)
                    ))
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
                    status.succeed(format!("Saved {}.", path.display()));
                    if let Some(action) = then {
                        commands.0.push_front(EditorCommand::Perform(action));
                    }
                }
                Err(error) => status.fail(format!("Save failed: {error}")),
            }
        }
        EditorCommand::Export => {
            let Some(pack) = generation.generated.clone() else {
                status.fail("Generate the current project before exporting.".into());
                return;
            };
            let mut dialog = FileDialog::new().set_title("Export texture pack into folder");
            if let Some(start) = export_start_dir(status.last_export.as_deref()) {
                // A folder that does not exist yet cannot be opened, and on a fresh
                // checkout the user pack folder is exactly that.
                if let Err(error) = fs::create_dir_all(&start) {
                    warn!("could not prepare {}: {error}", start.display());
                }
                dialog = dialog.set_directory(start);
            }
            let Some(folder) = dialog.pick_folder() else {
                return;
            };
            status.message = Some("Exporting texture pack…".into());
            // A fresh attempt retires the dialog the previous one may have raised.
            status.error = None;
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
            status.succeed(format!("Exported {}.", path.display()));
        }
        Err(error) => status.fail(format!("Export failed: {error}")),
    }
}

/// Where the export dialog should open.
///
/// Left to itself the dialog opens nowhere in particular, and the folder that
/// looks right — `assets/texture-packs`, the one holding the built-in pack — is
/// the one the game never scans, so exports quietly went missing. Packs belong in
/// [`USER_PACK_ROOT`], and after the first export the folder actually used wins.
///
/// The result is absolute on purpose: rfd feeds the value to
/// `SHCreateItemFromParsingName`, which only accepts absolute parsing names and
/// whose failure rfd discards, so a relative path would silently do nothing.
fn export_start_dir(last_export: Option<&Path>) -> Option<PathBuf> {
    let preferred = last_export
        .and_then(Path::parent)
        .map_or_else(|| PathBuf::from(USER_PACK_ROOT), Path::to_path_buf);
    std::path::absolute(&preferred).ok()
}

fn project_dialog() -> FileDialog {
    FileDialog::new()
        .set_title("Sidecraft texture project")
        .add_filter("Sidecraft texture project", &["toml"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_first_export_is_steered_at_the_folder_the_game_scans() {
        let start = export_start_dir(None).expect("an absolute starting directory");
        assert!(start.is_absolute(), "{}", start.display());
        assert!(start.ends_with(USER_PACK_ROOT), "{}", start.display());
    }

    #[test]
    fn a_later_export_returns_to_the_folder_last_used() {
        let previous = std::path::absolute(Path::new(USER_PACK_ROOT))
            .unwrap()
            .join("some-pack");
        let start = export_start_dir(Some(&previous)).expect("an absolute starting directory");
        assert_eq!(start, previous.parent().unwrap());
    }
}
