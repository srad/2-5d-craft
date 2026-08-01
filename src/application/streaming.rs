use std::collections::HashSet;

use crate::domain::BlockChunk;

use super::{SessionInstanceId, WorldId, WorldSession};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamConfig {
    pub simulation_radius: u32,
    pub render_radius: u32,
    pub unload_radius: u32,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            simulation_radius: 3,
            render_radius: 5,
            unload_radius: 7,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamWindow {
    center_chunk: i64,
    config: StreamConfig,
}

impl StreamWindow {
    pub fn new(center_chunk: i64, config: StreamConfig) -> Self {
        assert!(config.simulation_radius <= config.render_radius);
        assert!(config.render_radius < config.unload_radius);
        Self {
            center_chunk,
            config,
        }
    }

    pub const fn center_chunk(self) -> i64 {
        self.center_chunk
    }

    pub const fn config(self) -> StreamConfig {
        self.config
    }

    pub fn contains_simulation(self, chunk_x: i64) -> bool {
        self.distance(chunk_x) <= u64::from(self.config.simulation_radius)
    }

    pub fn contains_render(self, chunk_x: i64) -> bool {
        self.distance(chunk_x) <= u64::from(self.config.render_radius)
    }

    pub fn should_unload(self, chunk_x: i64) -> bool {
        self.distance(chunk_x) > u64::from(self.config.unload_radius)
    }

    pub fn request_priority(self, chunk_x: i64) -> (u64, bool, i64) {
        (self.distance(chunk_x), chunk_x < self.center_chunk, chunk_x)
    }

    fn distance(self, chunk_x: i64) -> u64 {
        self.center_chunk.abs_diff(chunk_x)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenerationIdentity {
    pub session_instance: SessionInstanceId,
    pub world_id: WorldId,
    pub generator_version: u32,
    pub seed: u64,
}

impl From<&WorldSession> for GenerationIdentity {
    fn from(session: &WorldSession) -> Self {
        Self {
            session_instance: session.instance_id,
            world_id: session.id.clone(),
            generator_version: session.generator_version,
            seed: session.seed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenerationRequest {
    pub identity: GenerationIdentity,
    pub global_chunk_x: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationResult {
    pub request: GenerationRequest,
    pub chunk: BlockChunk,
}

pub fn plan_generation_requests(
    window: StreamWindow,
    identity: &GenerationIdentity,
    loaded: &HashSet<i64>,
    in_flight: &HashSet<i64>,
    request_budget: usize,
) -> Vec<GenerationRequest> {
    let mut requests = Vec::with_capacity(request_budget);
    for distance in 0..=i64::from(window.config.render_radius) {
        let candidates = if distance == 0 {
            [Some(window.center_chunk), None]
        } else {
            [
                window.center_chunk.checked_add(distance),
                window.center_chunk.checked_sub(distance),
            ]
        };
        for chunk_x in candidates.into_iter().flatten() {
            if requests.len() == request_budget {
                return requests;
            }
            if loaded.contains(&chunk_x) || in_flight.contains(&chunk_x) {
                continue;
            }
            requests.push(GenerationRequest {
                identity: identity.clone(),
                global_chunk_x: chunk_x,
            });
        }
    }
    requests
}

pub fn plan_unloads(window: StreamWindow, loaded: impl IntoIterator<Item = i64>) -> Vec<i64> {
    let mut result = loaded
        .into_iter()
        .filter(|chunk_x| window.should_unload(*chunk_x))
        .collect::<Vec<_>>();
    result.sort_unstable();
    result
}

pub fn result_is_still_requested(
    task_request: &GenerationRequest,
    result: &GenerationResult,
    current_identity: &GenerationIdentity,
    window: StreamWindow,
    already_loaded: bool,
) -> bool {
    task_request == &result.request
        && result.request.identity == *current_identity
        && result.chunk.x() == result.request.global_chunk_x
        && !already_loaded
        && window.contains_render(result.request.global_chunk_x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(instance: u64) -> GenerationIdentity {
        GenerationIdentity {
            session_instance: SessionInstanceId::new(instance),
            world_id: WorldId::new("test").unwrap(),
            generator_version: 2,
            seed: 11,
        }
    }

    #[test]
    fn default_window_has_strict_simulation_render_and_unload_bands() {
        let window = StreamWindow::new(10, StreamConfig::default());
        assert!(window.contains_simulation(7));
        assert!(!window.contains_simulation(6));
        assert!(window.contains_render(5));
        assert!(!window.contains_render(4));
        assert!(!window.should_unload(3));
        assert!(window.should_unload(2));
    }

    #[test]
    fn requests_are_nearest_first_and_capacity_bounded() {
        let loaded = HashSet::from([0]);
        let in_flight = HashSet::from([1]);
        let requests = plan_generation_requests(
            StreamWindow::new(0, StreamConfig::default()),
            &identity(1),
            &loaded,
            &in_flight,
            4,
        );
        assert_eq!(
            requests
                .iter()
                .map(|request| request.global_chunk_x)
                .collect::<Vec<_>>(),
            vec![-1, 2, -2, 3]
        );
    }

    #[test]
    fn planning_is_overflow_safe_and_budget_bounded() {
        let requests = plan_generation_requests(
            StreamWindow::new(i64::MAX, StreamConfig::default()),
            &identity(1),
            &HashSet::new(),
            &HashSet::new(),
            20,
        );
        assert_eq!(
            requests
                .iter()
                .map(|request| request.global_chunk_x)
                .collect::<Vec<_>>(),
            (i64::MAX - 5..=i64::MAX).rev().collect::<Vec<_>>()
        );
    }

    #[test]
    fn stale_or_malformed_results_are_rejected() {
        let request = GenerationRequest {
            identity: identity(1),
            global_chunk_x: 0,
        };
        let result = GenerationResult {
            request: request.clone(),
            chunk: crate::domain::generate_chunk(11, 0),
        };
        let window = StreamWindow::new(0, StreamConfig::default());
        assert!(result_is_still_requested(
            &request,
            &result,
            &identity(1),
            window,
            false,
        ));
        assert!(!result_is_still_requested(
            &request,
            &result,
            &identity(2),
            window,
            false,
        ));
        for stale_identity in [
            GenerationIdentity {
                world_id: WorldId::new("other").unwrap(),
                ..identity(1)
            },
            GenerationIdentity {
                generator_version: 3,
                ..identity(1)
            },
            GenerationIdentity {
                seed: 12,
                ..identity(1)
            },
        ] {
            assert!(!result_is_still_requested(
                &request,
                &result,
                &stale_identity,
                window,
                false,
            ));
        }
        let wrong_request = GenerationRequest {
            global_chunk_x: 1,
            ..request.clone()
        };
        assert!(!result_is_still_requested(
            &wrong_request,
            &result,
            &identity(1),
            window,
            false,
        ));
        assert!(!result_is_still_requested(
            &request,
            &result,
            &identity(1),
            window,
            true,
        ));

        let wrong_payload = GenerationResult {
            request: request.clone(),
            chunk: crate::domain::generate_chunk(11, 1),
        };
        assert!(!result_is_still_requested(
            &request,
            &wrong_payload,
            &identity(1),
            window,
            false,
        ));

        let out_of_range = GenerationRequest {
            global_chunk_x: 6,
            ..request
        };
        let out_of_range_result = GenerationResult {
            request: out_of_range.clone(),
            chunk: crate::domain::generate_chunk(11, 6),
        };
        assert!(!result_is_still_requested(
            &out_of_range,
            &out_of_range_result,
            &identity(1),
            window,
            false,
        ));
    }

    #[test]
    fn completion_priority_is_stable_independent_of_task_finish_order() {
        let window = StreamWindow::new(10, StreamConfig::default());
        let mut chunks = vec![8, 12, 9, 11, 10];

        chunks.sort_by_key(|chunk_x| window.request_priority(*chunk_x));

        assert_eq!(chunks, vec![10, 11, 9, 12, 8]);
    }

    #[test]
    fn unloads_are_sorted_and_begin_beyond_the_retention_band() {
        let unloads = plan_unloads(
            StreamWindow::new(0, StreamConfig::default()),
            [8, -7, -9, 7, 9],
        );
        assert_eq!(unloads, vec![-9, 8, 9]);
    }
}
