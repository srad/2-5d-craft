use crate::adapters::bevy::{DayCycleResource, RuntimeSet};
use crate::{AppState, domain::DayCycle};
use ::bevy::prelude::*;

#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SkyLightLevelChanged {
    pub(crate) previous: u8,
    pub(crate) current: u8,
}

pub(crate) struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DayCycleResource>()
            .add_message::<SkyLightLevelChanged>()
            .add_systems(
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
    mut changes: MessageWriter<SkyLightLevelChanged>,
) {
    let previous = day.sky_light_level();
    if day.advance_seconds(time.delta_secs_f64()) {
        let current = day.sky_light_level();
        if previous != current {
            changes.write(SkyLightLevelChanged { previous, current });
        }
    }
}

pub(crate) fn configured_start_time(saved_ticks: u64) -> DayCycle {
    let requested = std::env::var("SIDECRAFT_TEST_DAY_TICKS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok());
    if std::env::var("SIDECRAFT_AUTOSTART").is_ok()
        && let Some(ticks) = requested
    {
        return DayCycle::from_ticks(ticks);
    }
    DayCycle::from_ticks(saved_ticks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_change_message_preserves_both_levels() {
        let change = SkyLightLevelChanged {
            previous: 15,
            current: 14,
        };
        assert_eq!((change.previous, change.current), (15, 14));
    }
}
