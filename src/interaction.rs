use crate::AppState;
use crate::camera::GameCamera;
use crate::lighting::LightGrid;
use crate::player::{Hotbar, Player};
use crate::rendering::{RenderCatalog, SelectionOutline, spawn_selection_outline};
use crate::world::{WorldData, place_tile, remove_tile};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

const REACH: f32 = 5.0;

#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlockTarget {
    pub block: Option<IVec2>,
    pub adjacent: Option<IVec2>,
}

#[derive(Resource, Debug, Default)]
struct MiningState {
    target: Option<IVec2>,
    elapsed: f32,
}

pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BlockTarget>()
            .init_resource::<MiningState>()
            .add_systems(OnEnter(AppState::Playing), ensure_selection_outline)
            .add_systems(
                Update,
                (update_target, mine_target, place_selected, update_highlight)
                    .chain()
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
    world: Option<Res<WorldData>>,
    buttons: Query<&Interaction, With<Button>>,
    mut target: ResMut<BlockTarget>,
) {
    let Some(world) = world else {
        *target = BlockTarget::default();
        return;
    };
    if buttons
        .iter()
        .any(|interaction| *interaction != Interaction::None)
    {
        *target = BlockTarget::default();
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        *target = BlockTarget::default();
        return;
    };
    let Ok(ray) = camera.0.viewport_to_world(camera.1, cursor) else {
        *target = BlockTarget::default();
        return;
    };
    *target = target_from_ray(
        ray.origin,
        ray.direction.as_vec3(),
        player.translation.truncate(),
        &world.grid,
    );
}

#[allow(clippy::too_many_arguments)]
fn mine_target(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    target: Res<BlockTarget>,
    mut mining: ResMut<MiningState>,
    mut world: Option<ResMut<WorldData>>,
) {
    let Some(world) = world.as_mut() else {
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
        commands.insert_resource(LightGrid::calculate(&world.grid));
        *mining = MiningState::default();
    }
}

#[allow(clippy::too_many_arguments)]
fn place_selected(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    target: Res<BlockTarget>,
    player: Single<(&Transform, &Hotbar), With<Player>>,
    mut world: Option<ResMut<WorldData>>,
) {
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }
    let (Some(coordinate), Some(world)) = (target.adjacent, world.as_mut()) else {
        return;
    };
    if tile_overlaps_player(coordinate, player.0.translation.truncate()) {
        return;
    }
    if place_tile(world, coordinate, player.1.selected_kind()) {
        commands.insert_resource(LightGrid::calculate(&world.grid));
    }
}

fn update_highlight(
    target: Res<BlockTarget>,
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

fn clear_interaction(mut target: ResMut<BlockTarget>, mut mining: ResMut<MiningState>) {
    *target = BlockTarget::default();
    *mining = MiningState::default();
}

#[cfg(test)]
pub fn target_from_world(
    world_position: Vec2,
    player: Vec2,
    grid: &crate::BlockGrid,
) -> BlockTarget {
    let coordinate = world_position.floor().as_ivec2();
    if !grid.in_bounds(coordinate)
        || grid.get(coordinate).is_none()
        || world_position.distance(player) > REACH
    {
        return BlockTarget::default();
    }
    target_for_block(coordinate, world_position - coordinate.as_vec2())
}

pub fn target_from_ray(
    origin: Vec3,
    direction: Vec3,
    player: Vec2,
    grid: &crate::BlockGrid,
) -> BlockTarget {
    let mut nearest = None;
    for (coordinate, _) in grid.iter() {
        let center = coordinate.as_vec2() + Vec2::splat(0.5);
        if center.distance(player) > REACH + 1.0 {
            continue;
        }
        let minimum = Vec3::new(coordinate.x as f32, coordinate.y as f32, -0.5);
        let maximum = minimum + Vec3::ONE;
        let Some((distance, normal)) = ray_box_intersection(origin, direction, minimum, maximum)
        else {
            continue;
        };
        if distance < 0.0
            || nearest
                .as_ref()
                .is_some_and(|(nearest_distance, _, _)| distance >= *nearest_distance)
        {
            continue;
        }
        nearest = Some((distance, coordinate, normal));
    }
    let Some((distance, coordinate, normal)) = nearest else {
        return BlockTarget::default();
    };
    let hit = origin + direction * distance;
    let adjacent = if normal.z.abs() > 0.5 {
        target_for_block(coordinate, hit.truncate() - coordinate.as_vec2()).adjacent
    } else {
        Some(coordinate + IVec2::new(normal.x.round() as i32, normal.y.round() as i32))
    };
    BlockTarget {
        block: Some(coordinate),
        adjacent,
    }
}

fn target_for_block(coordinate: IVec2, local: Vec2) -> BlockTarget {
    let candidates = [
        (1.0 - local.y, IVec2::Y),
        (1.0 - local.x, IVec2::X),
        (local.y, -IVec2::Y),
        (local.x, -IVec2::X),
    ];
    let direction = candidates
        .into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, direction)| direction)
        .unwrap_or(IVec2::Y);
    BlockTarget {
        block: Some(coordinate),
        adjacent: Some(coordinate + direction),
    }
}

fn ray_box_intersection(
    origin: Vec3,
    direction: Vec3,
    minimum: Vec3,
    maximum: Vec3,
) -> Option<(f32, Vec3)> {
    let mut near = f32::NEG_INFINITY;
    let mut far = f32::INFINITY;
    let mut normal = Vec3::ZERO;
    for axis in 0..3 {
        let origin_axis = origin[axis];
        let direction_axis = direction[axis];
        if direction_axis.abs() < 1.0e-6 {
            if origin_axis < minimum[axis] || origin_axis > maximum[axis] {
                return None;
            }
            continue;
        }
        let (axis_near, axis_far, axis_normal) = if direction_axis > 0.0 {
            (
                (minimum[axis] - origin_axis) / direction_axis,
                (maximum[axis] - origin_axis) / direction_axis,
                -1.0,
            )
        } else {
            (
                (maximum[axis] - origin_axis) / direction_axis,
                (minimum[axis] - origin_axis) / direction_axis,
                1.0,
            )
        };
        if axis_near > near {
            near = axis_near;
            normal = Vec3::ZERO;
            normal[axis] = axis_normal;
        }
        far = far.min(axis_far);
        if near > far {
            return None;
        }
    }
    (far >= 0.0).then_some((near.max(0.0), normal))
}

pub fn tile_overlaps_player(tile: IVec2, player_center: Vec2) -> bool {
    let tile_min = tile.as_vec2();
    let tile_max = tile_min + Vec2::ONE;
    let player_min = player_center - Vec2::new(0.35, 0.90);
    let player_max = player_center + Vec2::new(0.35, 0.90);
    tile_min.x < player_max.x
        && tile_max.x > player_min.x
        && tile_min.y < player_max.y
        && tile_max.y > player_min.y
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlockGrid;
    use crate::BlockKind;

    fn target_grid() -> BlockGrid {
        let mut grid = BlockGrid::new(crate::WORLD_HEIGHT);
        grid.insert_chunk(
            crate::world::BlockChunk::from_dense(
                0,
                vec![0; (crate::CHUNK_WIDTH * crate::WORLD_HEIGHT) as usize],
            )
            .unwrap(),
        );
        grid.set(IVec2::new(3, 3), BlockKind::Stone);
        grid
    }

    #[test]
    fn targeting_uses_nearest_face_with_fixed_tie_order() {
        let grid = target_grid();
        assert_eq!(
            target_from_world(Vec2::new(3.5, 3.9), Vec2::new(3.5, 2.0), &grid).adjacent,
            Some(IVec2::new(3, 4))
        );
        assert_eq!(
            target_from_world(Vec2::new(3.5, 3.5), Vec2::new(3.5, 2.0), &grid).adjacent,
            Some(IVec2::new(3, 4))
        );
    }

    #[test]
    fn targeting_enforces_reach_and_occupancy() {
        let grid = target_grid();
        assert!(
            target_from_world(Vec2::new(3.2, 3.2), Vec2::ZERO, &grid)
                .block
                .is_some()
        );
        assert!(
            target_from_world(Vec2::new(3.2, 3.2), Vec2::new(9.0, 9.0), &grid)
                .block
                .is_none()
        );
        assert!(
            target_from_world(Vec2::new(2.2, 2.2), Vec2::new(2.0, 2.0), &grid)
                .block
                .is_none()
        );
    }

    #[test]
    fn ray_targeting_hits_only_the_playable_cube() {
        let grid = target_grid();
        let target = target_from_ray(
            Vec3::new(3.5, 3.5, 10.0),
            Vec3::NEG_Z,
            Vec2::new(3.5, 2.0),
            &grid,
        );
        assert_eq!(target.block, Some(IVec2::new(3, 3)));
        assert_eq!(target.adjacent, Some(IVec2::new(3, 4)));
    }

    #[test]
    fn player_overlap_uses_aligned_collider_extents() {
        assert!(tile_overlaps_player(IVec2::new(3, 3), Vec2::new(3.5, 3.5)));
        assert!(!tile_overlaps_player(IVec2::new(5, 3), Vec2::new(3.5, 3.5)));
    }
}
