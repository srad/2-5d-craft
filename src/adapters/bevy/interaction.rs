use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::{
    AppState,
    adapters::bevy::{
        RuntimeSet, WorldSessionResource, WorldStateResource,
        camera::GameCamera,
        player::{Hotbar, Player},
        rendering::{RenderCatalog, SelectionOutline, spawn_selection_outline},
        world::WorldMutationMessage,
    },
    application::{break_block, place_block},
    domain::{
        BlockState, BlockTarget, TorchMount, VoxelLayer, target_from_ray, tile_overlaps_player,
    },
};

#[derive(Resource, Debug, Default, Deref, DerefMut)]
struct BlockTargetResource(BlockTarget);

#[derive(Resource, Debug, Default)]
struct MiningState {
    target: Option<(VoxelLayer, IVec2)>,
    elapsed: f32,
}

#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq, Deref, DerefMut)]
pub(crate) struct ActiveVoxelLayer(pub(crate) VoxelLayer);

pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BlockTargetResource>()
            .init_resource::<MiningState>()
            .init_resource::<ActiveVoxelLayer>()
            .add_systems(OnEnter(AppState::LoadingWorld), reset_interaction_layer)
            .add_systems(OnEnter(AppState::Playing), ensure_selection_outline)
            .add_systems(
                Update,
                (
                    toggle_active_layer,
                    update_target,
                    mine_target,
                    place_selected,
                    update_highlight,
                )
                    .chain()
                    .in_set(RuntimeSet::Commands)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(OnEnter(AppState::Paused), clear_interaction);
    }
}

fn reset_interaction_layer(
    mut active: ResMut<ActiveVoxelLayer>,
    mut target: ResMut<BlockTargetResource>,
    mut mining: ResMut<MiningState>,
) {
    active.0 = VoxelLayer::Foreground;
    target.0 = BlockTarget::default();
    *mining = MiningState::default();
}

fn toggle_active_layer(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut active: ResMut<ActiveVoxelLayer>,
    mut target: ResMut<BlockTargetResource>,
    mut mining: ResMut<MiningState>,
) {
    if !keyboard.just_pressed(KeyCode::Tab) {
        return;
    }
    active.0 = toggled_layer(active.0);
    target.0 = BlockTarget {
        layer: active.0,
        ..Default::default()
    };
    *mining = MiningState::default();
}

fn toggled_layer(layer: VoxelLayer) -> VoxelLayer {
    match layer {
        VoxelLayer::Foreground => VoxelLayer::Backwall,
        VoxelLayer::Backwall => VoxelLayer::Foreground,
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
    active: Res<ActiveVoxelLayer>,
    buttons: Query<&Interaction, With<Button>>,
    mut target: ResMut<BlockTargetResource>,
) {
    let Some(world) = world else {
        target.0 = BlockTarget {
            layer: active.0,
            ..Default::default()
        };
        return;
    };
    if buttons
        .iter()
        .any(|interaction| *interaction != Interaction::None)
    {
        target.0 = BlockTarget {
            layer: active.0,
            ..Default::default()
        };
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        target.0 = BlockTarget {
            layer: active.0,
            ..Default::default()
        };
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
        &world.view(),
        world.origin_chunk(),
        active.0,
    );
}

fn mine_target(
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    target: Res<BlockTargetResource>,
    mut mining: ResMut<MiningState>,
    mut world: Option<ResMut<WorldStateResource>>,
    session: Option<Res<WorldSessionResource>>,
    mut mutations: MessageWriter<WorldMutationMessage>,
) {
    let (Some(world), Some(session)) = (world.as_mut(), session) else {
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
    if mining.target != Some((target.layer, coordinate)) {
        mining.target = Some((target.layer, coordinate));
        mining.elapsed = 0.0;
    }
    let position = world.local_to_voxel(coordinate, target.layer);
    let Some(state) = world.view().block(position) else {
        *mining = MiningState::default();
        return;
    };
    if !state.breakable() {
        mining.elapsed = 0.0;
        return;
    }
    mining.elapsed += time.delta_secs();
    if mining.elapsed >= state.def().hardness_seconds {
        if let Ok(result) = break_block(world, position)
            && !result.report.is_empty()
        {
            mutations.write(WorldMutationMessage {
                world_id: session.id.clone(),
                report: result.report,
            });
        }
        *mining = MiningState::default();
    }
}

fn place_selected(
    mouse: Res<ButtonInput<MouseButton>>,
    target: Res<BlockTargetResource>,
    player: Single<(&Transform, &Hotbar), With<Player>>,
    mut world: Option<ResMut<WorldStateResource>>,
    session: Option<Res<WorldSessionResource>>,
    mut mutations: MessageWriter<WorldMutationMessage>,
) {
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }
    let (Some(coordinate), Some(world), Some(session)) = (target.adjacent, world.as_mut(), session)
    else {
        return;
    };
    if target.layer == VoxelLayer::Foreground
        && tile_overlaps_player(coordinate, player.0.translation.truncate())
    {
        return;
    }
    let position = world.local_to_voxel(coordinate, target.layer);
    let Some(state) = placement_state(player.1.selected_state(), target.0) else {
        return;
    };
    if let Ok(result) = place_block(world, position, state)
        && !result.report.is_empty()
    {
        mutations.write(WorldMutationMessage {
            world_id: session.id.clone(),
            report: result.report,
        });
    }
}

fn placement_state(selected: BlockState, target: BlockTarget) -> Option<BlockState> {
    if selected.torch_mount().is_none() {
        return Some(selected);
    }
    let (Some(support), Some(adjacent)) = (target.block, target.adjacent) else {
        return None;
    };
    let offset = adjacent - support;
    TorchMount::from_placement_offset(offset.x, offset.y).map(BlockState::torch)
}

fn update_highlight(
    target: Res<BlockTargetResource>,
    mut outline: Single<(&mut Transform, &mut Visibility), With<SelectionOutline>>,
) {
    if let Some(coordinate) = target.block {
        outline.0.translation.x = coordinate.x as f32 + 0.5;
        outline.0.translation.y = coordinate.y as f32 + 0.5;
        outline.0.translation.z = -f32::from(target.layer.depth());
        *outline.1 = Visibility::Visible;
    } else {
        *outline.1 = Visibility::Hidden;
    }
}

fn clear_interaction(
    active: Res<ActiveVoxelLayer>,
    mut target: ResMut<BlockTargetResource>,
    mut mining: ResMut<MiningState>,
    mut outlines: Query<&mut Visibility, With<SelectionOutline>>,
) {
    target.0 = BlockTarget {
        layer: active.0,
        ..Default::default()
    };
    *mining = MiningState::default();
    for mut visibility in &mut outlines {
        *visibility = Visibility::Hidden;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn torch_mount_follows_the_targeted_face() {
        for (adjacent, expected) in [
            (IVec2::new(4, 5), Some(BlockState::TORCH)),
            (IVec2::new(3, 4), Some(BlockState::WALL_TORCH_LEFT)),
            (IVec2::new(5, 4), Some(BlockState::WALL_TORCH_RIGHT)),
            (IVec2::new(4, 3), None),
        ] {
            assert_eq!(
                placement_state(
                    BlockState::TORCH,
                    BlockTarget {
                        layer: VoxelLayer::Foreground,
                        block: Some(IVec2::new(4, 4)),
                        adjacent: Some(adjacent),
                    },
                ),
                expected
            );
        }
    }

    #[test]
    fn ordinary_blocks_keep_their_selected_state() {
        assert_eq!(
            placement_state(BlockState::DIRT, BlockTarget::default()),
            Some(BlockState::DIRT)
        );
    }

    #[test]
    fn layer_toggle_round_trips_to_foreground() {
        assert_eq!(toggled_layer(VoxelLayer::Foreground), VoxelLayer::Backwall);
        assert_eq!(toggled_layer(VoxelLayer::Backwall), VoxelLayer::Foreground);
    }
}
