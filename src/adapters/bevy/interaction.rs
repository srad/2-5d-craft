use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::{
    AppState,
    adapters::bevy::{
        RuntimeSet, WorldStateResource,
        camera::GameCamera,
        player::{Hotbar, Player},
        rendering::{RenderCatalog, SelectionOutline, spawn_selection_outline},
        world::{WorldPresentation, mark_world_visuals_dirty},
    },
    application::{place_tile, remove_tile},
    domain::{BlockTarget, target_from_ray, tile_overlaps_player},
};

#[derive(Resource, Debug, Default, Deref, DerefMut)]
struct BlockTargetResource(BlockTarget);

#[derive(Resource, Debug, Default)]
struct MiningState {
    target: Option<IVec2>,
    elapsed: f32,
}

pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BlockTargetResource>()
            .init_resource::<MiningState>()
            .add_systems(OnEnter(AppState::Playing), ensure_selection_outline)
            .add_systems(
                Update,
                (update_target, mine_target, place_selected, update_highlight)
                    .chain()
                    .in_set(RuntimeSet::Commands)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(OnEnter(AppState::Paused), clear_interaction);
    }
}

fn ensure_selection_outline(
    mut commands: Commands,
    catalog: Option<Res<RenderCatalog>>,
    existing: Query<(), With<SelectionOutline>>,
) {
    if existing.is_empty()
        && let Some(catalog) = catalog
    {
        spawn_selection_outline(&mut commands, &catalog);
    }
}

fn update_target(
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<GameCamera>>,
    player: Single<&Transform, With<Player>>,
    world: Option<Res<WorldStateResource>>,
    buttons: Query<&Interaction, With<Button>>,
    mut target: ResMut<BlockTargetResource>,
) {
    let Some(world) = world else {
        target.0 = BlockTarget::default();
        return;
    };
    if buttons
        .iter()
        .any(|interaction| *interaction != Interaction::None)
    {
        target.0 = BlockTarget::default();
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        target.0 = BlockTarget::default();
        return;
    };
    let Ok(ray) = camera.0.viewport_to_world(camera.1, cursor) else {
        target.0 = BlockTarget::default();
        return;
    };
    target.0 = target_from_ray(
        ray.origin,
        ray.direction.as_vec3(),
        player.translation.truncate(),
        &world.grid,
    );
}

fn mine_target(
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    target: Res<BlockTargetResource>,
    mut mining: ResMut<MiningState>,
    mut world: Option<ResMut<WorldStateResource>>,
    mut presentation: Option<ResMut<WorldPresentation>>,
) {
    let (Some(world), Some(presentation)) = (world.as_mut(), presentation.as_mut()) else {
        return;
    };
    if !mouse.pressed(MouseButton::Left) {
        *mining = MiningState::default();
        return;
    }
    let Some(coordinate) = target.block else {
        *mining = MiningState::default();
        return;
    };
    if mining.target != Some(coordinate) {
        mining.target = Some(coordinate);
        mining.elapsed = 0.0;
    }
    let Some(kind) = world.grid.get(coordinate) else {
        *mining = MiningState::default();
        return;
    };
    if !kind.breakable() {
        mining.elapsed = 0.0;
        return;
    }
    mining.elapsed += time.delta_secs();
    if mining.elapsed >= kind.def().hardness_seconds {
        remove_tile(world, coordinate);
        mark_world_visuals_dirty(presentation, coordinate.x);
        *mining = MiningState::default();
    }
}

fn place_selected(
    mouse: Res<ButtonInput<MouseButton>>,
    target: Res<BlockTargetResource>,
    player: Single<(&Transform, &Hotbar), With<Player>>,
    mut world: Option<ResMut<WorldStateResource>>,
    mut presentation: Option<ResMut<WorldPresentation>>,
) {
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }
    let (Some(coordinate), Some(world), Some(presentation)) =
        (target.adjacent, world.as_mut(), presentation.as_mut())
    else {
        return;
    };
    if tile_overlaps_player(coordinate, player.0.translation.truncate()) {
        return;
    }
    if place_tile(world, coordinate, player.1.selected_kind()) {
        mark_world_visuals_dirty(presentation, coordinate.x);
    }
}

fn update_highlight(
    target: Res<BlockTargetResource>,
    mut outline: Single<(&mut Transform, &mut Visibility), With<SelectionOutline>>,
) {
    if let Some(coordinate) = target.block {
        outline.0.translation.x = coordinate.x as f32 + 0.5;
        outline.0.translation.y = coordinate.y as f32 + 0.5;
        *outline.1 = Visibility::Visible;
    } else {
        *outline.1 = Visibility::Hidden;
    }
}

fn clear_interaction(mut target: ResMut<BlockTargetResource>, mut mining: ResMut<MiningState>) {
    target.0 = BlockTarget::default();
    *mining = MiningState::default();
}
