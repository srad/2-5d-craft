use crate::document::EditorDocument;
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use sidecraft_textures::{GeneratedPack, ResolvedPack, generate_pack, resolve_generated_pack};
use std::time::{Duration, Instant};

const DEBOUNCE: Duration = Duration::from_millis(200);

struct GenerationResult {
    revision: u64,
    result: Result<(GeneratedPack, ResolvedPack), String>,
}

#[derive(Resource)]
pub(crate) struct GenerationCoordinator {
    pending_revision: Option<u64>,
    requested_at: Instant,
    active: Option<Task<GenerationResult>>,
    pub generated: Option<GeneratedPack>,
    pub resolved: Option<ResolvedPack>,
    pub generated_revision: Option<u64>,
    pub error: Option<String>,
}

impl Default for GenerationCoordinator {
    fn default() -> Self {
        Self {
            pending_revision: None,
            requested_at: Instant::now(),
            active: None,
            generated: None,
            resolved: None,
            generated_revision: None,
            error: None,
        }
    }
}

impl GenerationCoordinator {
    pub fn request(&mut self, revision: u64) {
        self.pending_revision = Some(revision);
        self.requested_at = Instant::now();
    }

    pub fn request_now(&mut self, revision: u64) {
        self.pending_revision = Some(revision);
        self.requested_at = Instant::now() - DEBOUNCE;
    }

    pub fn is_generating(&self) -> bool {
        self.active.is_some() || self.pending_revision.is_some()
    }

    pub fn can_export(&self, revision: u64) -> bool {
        self.generated.is_some()
            && self.generated_revision == Some(revision)
            && !self.is_generating()
            && self.error.is_none()
    }

    fn accept_completion(&mut self, current_revision: u64, completed: GenerationResult) {
        if completed.revision != current_revision {
            return;
        }
        match completed.result {
            Ok((pack, resolved)) => {
                self.generated = Some(pack);
                self.resolved = Some(resolved);
                self.generated_revision = Some(completed.revision);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
}

pub(crate) fn queue_initial(
    document: Res<EditorDocument>,
    mut generation: ResMut<GenerationCoordinator>,
) {
    generation.request_now(document.revision);
}

pub(crate) fn drive_generation(
    document: Res<EditorDocument>,
    mut generation: ResMut<GenerationCoordinator>,
) {
    if let Some(task) = generation.active.as_mut()
        && let Some(completed) = check_ready(task)
    {
        generation.active = None;
        generation.accept_completion(document.revision, completed);
    }

    if generation.active.is_some() {
        return;
    }
    let Some(revision) = generation.pending_revision else {
        return;
    };
    if generation.requested_at.elapsed() < DEBOUNCE {
        return;
    }
    generation.pending_revision = None;
    let project = document.project.clone();
    generation.active = Some(AsyncComputeTaskPool::get().spawn(async move {
        let result = (|| {
            let options = project
                .to_generate_options()
                .map_err(|error| error.to_string())?;
            let pack = generate_pack(&options).map_err(|error| error.to_string())?;
            let resolved = resolve_generated_pack(&pack).map_err(|error| error.to_string())?;
            Ok((pack, resolved))
        })();
        GenerationResult { revision, result }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_requests_replace_pending_work_and_gate_export() {
        let mut coordinator = GenerationCoordinator::default();
        coordinator.request(1);
        coordinator.request(2);
        assert_eq!(coordinator.pending_revision, Some(2));
        assert!(!coordinator.can_export(2));
    }

    #[test]
    fn stale_completions_cannot_replace_current_status() {
        let mut coordinator = GenerationCoordinator::default();
        coordinator.accept_completion(
            2,
            GenerationResult {
                revision: 1,
                result: Err("stale".into()),
            },
        );
        assert!(coordinator.error.is_none());
        coordinator.accept_completion(
            2,
            GenerationResult {
                revision: 2,
                result: Err("current".into()),
            },
        );
        assert_eq!(coordinator.error.as_deref(), Some("current"));
    }
}
