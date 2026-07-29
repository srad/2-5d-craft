use crate::adapters::bevy::{DayCycleResource, RuntimeSet, configured_autostart_u64};
use crate::{AppState, domain::DayCycle};
use ::bevy::prelude::*;

pub(crate) struct LightingPlugin;

#[derive(Resource)]
struct FixedTestDayCycle;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LightingPalette {
    pub skylight: [f32; 3],
    pub haze: [f32; 3],
    pub sky_bottom: [f32; 3],
    pub sky_top: [f32; 3],
}

impl LightingPalette {
    const KEYFRAMES: [Self; 4] = [
        Self {
            skylight: [1.00, 0.58, 0.30],
            haze: [0.30, 0.16, 0.20],
            sky_bottom: [0.82, 0.28, 0.18],
            sky_top: [0.16, 0.16, 0.42],
        },
        Self {
            skylight: [1.00, 0.98, 0.92],
            haze: [0.34, 0.46, 0.60],
            sky_bottom: [0.44, 0.73, 0.94],
            sky_top: [0.25, 0.58, 0.88],
        },
        Self {
            skylight: [1.00, 0.44, 0.26],
            haze: [0.34, 0.12, 0.18],
            sky_bottom: [0.88, 0.20, 0.14],
            sky_top: [0.28, 0.08, 0.34],
        },
        Self {
            skylight: [0.34, 0.46, 0.90],
            haze: [0.055, 0.075, 0.17],
            sky_bottom: [0.025, 0.020, 0.085],
            sky_top: [0.010, 0.015, 0.055],
        },
    ];

    pub fn from_day(day: &DayCycle) -> Self {
        let position = day.phase() * Self::KEYFRAMES.len() as f32;
        let index = position.floor() as usize % Self::KEYFRAMES.len();
        let next = (index + 1) % Self::KEYFRAMES.len();
        let amount = smoothstep(position.fract());
        Self::KEYFRAMES[index].mix(Self::KEYFRAMES[next], amount)
    }

    fn mix(self, other: Self, amount: f32) -> Self {
        Self {
            skylight: mix_array(self.skylight, other.skylight, amount),
            haze: mix_array(self.haze, other.haze, amount),
            sky_bottom: mix_array(self.sky_bottom, other.sky_bottom, amount),
            sky_top: mix_array(self.sky_top, other.sky_top, amount),
        }
    }
}

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        if configured_autostart_u64("SIDECRAFT_TEST_DAY_TICKS").is_some() {
            app.insert_resource(FixedTestDayCycle);
        }
        app.init_resource::<DayCycleResource>().add_systems(
            Update,
            advance_day_cycle
                .in_set(RuntimeSet::Clock)
                .run_if(in_state(AppState::Playing)),
        );
    }
}

fn advance_day_cycle(
    time: Res<Time>,
    mut day: ResMut<DayCycleResource>,
    fixed: Option<Res<FixedTestDayCycle>>,
) {
    if fixed.is_some() {
        return;
    }
    day.advance_seconds(time.delta_secs_f64());
}

pub(crate) fn configured_start_time(saved_ticks: u64) -> DayCycle {
    if let Some(ticks) = configured_autostart_u64("SIDECRAFT_TEST_DAY_TICKS") {
        return DayCycle::from_ticks(ticks);
    }
    DayCycle::from_ticks(saved_ticks)
}

fn mix_array(start: [f32; 3], end: [f32; 3], amount: f32) -> [f32; 3] {
    std::array::from_fn(|index| start[index] + (end[index] - start[index]) * amount)
}

fn smoothstep(value: f32) -> f32 {
    value * value * (3.0 - 2.0 * value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_times_select_exact_palette_keyframes() {
        for (ticks, expected) in [0, 6_000, 12_000, 18_000]
            .into_iter()
            .zip(LightingPalette::KEYFRAMES)
        {
            assert_eq!(
                LightingPalette::from_day(&DayCycle::from_ticks(ticks)),
                expected
            );
        }
    }

    #[test]
    fn palette_wraps_continuously_at_sunrise() {
        let before = LightingPalette::from_day(&DayCycle::from_ticks(23_999));
        let sunrise = LightingPalette::from_day(&DayCycle::from_ticks(24_000));
        assert!((before.skylight[0] - sunrise.skylight[0]).abs() < 0.001);
        assert!((before.sky_bottom[2] - sunrise.sky_bottom[2]).abs() < 0.001);
    }
}
