use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamConfig {
    pub load_radius: i32,
    pub unload_radius: i32,
    pub max_in_flight: usize,
    pub max_integrations_per_frame: usize,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            load_radius: 3,
            unload_radius: 5,
            max_in_flight: 4,
            max_integrations_per_frame: 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenerationRequest {
    pub local_chunk_x: i32,
    pub global_chunk_x: i64,
    pub origin_chunk: i64,
}

pub fn plan_generation_requests(
    center: i32,
    origin_chunk: i64,
    loaded: &HashSet<i32>,
    in_flight: &HashSet<i32>,
    config: StreamConfig,
) -> Vec<GenerationRequest> {
    let available = config.max_in_flight.saturating_sub(in_flight.len());
    let mut requests = Vec::with_capacity(available);
    for distance in 0..=config.load_radius {
        for chunk_x in [center + distance, center - distance] {
            if requests.len() == available {
                return requests;
            }
            if loaded.contains(&chunk_x)
                || in_flight.contains(&chunk_x)
                || requests
                    .iter()
                    .any(|request: &GenerationRequest| request.local_chunk_x == chunk_x)
            {
                continue;
            }
            requests.push(GenerationRequest {
                local_chunk_x: chunk_x,
                global_chunk_x: origin_chunk + i64::from(chunk_x),
                origin_chunk,
            });
        }
    }
    requests
}

pub fn plan_unloads(
    center: i32,
    loaded: impl IntoIterator<Item = i32>,
    config: StreamConfig,
) -> Vec<i32> {
    let mut result = loaded
        .into_iter()
        .filter(|chunk_x| (*chunk_x - center).abs() > config.unload_radius)
        .collect::<Vec<_>>();
    result.sort_unstable();
    result
}

pub fn result_is_still_requested(
    result_origin: i64,
    current_origin: i64,
    chunk_x: i32,
    center: i32,
    already_loaded: bool,
    config: StreamConfig,
) -> bool {
    result_origin == current_origin
        && !already_loaded
        && (chunk_x - center).abs() <= config.load_radius
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_nearest_first_and_capacity_bounded() {
        let requests = plan_generation_requests(
            0,
            100,
            &HashSet::new(),
            &HashSet::new(),
            StreamConfig::default(),
        );
        assert_eq!(
            requests
                .iter()
                .map(|request| request.local_chunk_x)
                .collect::<Vec<_>>(),
            vec![0, 1, -1, 2]
        );
    }

    #[test]
    fn stale_origin_results_are_rejected() {
        assert!(!result_is_still_requested(
            4,
            5,
            0,
            0,
            false,
            StreamConfig::default()
        ));
    }
}
