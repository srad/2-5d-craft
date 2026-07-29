use crate::adapters::bevy::{DayCycleResource, RuntimeSet, configured_autostart_u64};
use crate::{AppState, domain::DayCycle};
use ::bevy::prelude::*;

pub(crate) struct LightingPlugin;

#[derive(Resource)]
struct FixedTestDayCycle;

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
