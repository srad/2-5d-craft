use crate::AppState;
use crate::adapters::bevy::DayCycleResource;
use ::bevy::prelude::*;

pub(crate) struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DayCycleResource>().add_systems(
            Update,
            advance_day_cycle.run_if(in_state(AppState::Playing)),
        );
    }
}

fn advance_day_cycle(time: Res<Time>, mut day: ResMut<DayCycleResource>) {
    day.phase = (day.phase + time.delta_secs() / 180.0).rem_euclid(1.0);
}
