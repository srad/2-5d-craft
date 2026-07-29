use crate::domain::{CHUNK_WIDTH, WorldView};
use glam::{IVec2, Vec2, Vec3};

const REACH: f32 = 5.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlockTarget {
    pub block: Option<IVec2>,
    pub adjacent: Option<IVec2>,
}

pub fn target_from_ray(
    origin: Vec3,
    direction: Vec3,
    player: Vec2,
    view: &WorldView<'_>,
    origin_chunk: i64,
) -> BlockTarget {
    let mut nearest = None;
    let origin_x = origin_chunk * i64::from(CHUNK_WIDTH);
    for (position, _) in view.iter() {
        let Ok(local_x) = i32::try_from(position.global_x - origin_x) else {
            continue;
        };
        let coordinate = IVec2::new(local_x, position.y);
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
    use crate::domain::{
        BlockChunk, BlockGrid, BlockState, CHUNK_WIDTH, WORLD_HEIGHT, WorldMutator,
    };

    #[test]
    fn targeting_hits_only_loaded_playable_blocks() {
        let mut grid = BlockGrid::new(WORLD_HEIGHT);
        let mut blocks = vec![0; (CHUNK_WIDTH * WORLD_HEIGHT) as usize];
        blocks[(3 * CHUNK_WIDTH + 3) as usize] = BlockState::STONE.code();
        WorldMutator::new(&mut grid).integrate_chunk(BlockChunk::from_dense(0, blocks).unwrap());
        let target = target_from_ray(
            Vec3::new(3.5, 3.5, 10.0),
            Vec3::NEG_Z,
            Vec2::new(3.5, 2.0),
            &grid.view(),
            0,
        );
        assert_eq!(target.block, Some(IVec2::new(3, 3)));
    }

    #[test]
    fn oblique_rays_target_visible_foreground_faces() {
        let mut grid = BlockGrid::new(WORLD_HEIGHT);
        let mut blocks = vec![0; (CHUNK_WIDTH * WORLD_HEIGHT) as usize];
        blocks[(3 * CHUNK_WIDTH + 3) as usize] = BlockState::STONE.code();
        WorldMutator::new(&mut grid).integrate_chunk(BlockChunk::from_dense(0, blocks).unwrap());
        let offset = Vec3::new(16.0, 9.0, 36.0);
        let direction = -offset.normalize();
        let cases = [
            (Vec3::new(3.5, 3.5, 0.5), IVec2::new(3, 4)),
            (Vec3::new(3.5, 4.0, 0.0), IVec2::new(3, 4)),
            (Vec3::new(4.0, 3.5, 0.0), IVec2::new(4, 3)),
        ];

        for (hit, adjacent) in cases {
            let target = target_from_ray(
                hit + offset,
                direction,
                Vec2::new(3.5, 2.0),
                &grid.view(),
                0,
            );
            assert_eq!(target.block, Some(IVec2::new(3, 3)));
            assert_eq!(target.adjacent, Some(adjacent));
        }
    }

    #[test]
    fn player_overlap_uses_aligned_extents() {
        assert!(tile_overlaps_player(IVec2::new(3, 3), Vec2::new(3.5, 3.5)));
        assert!(!tile_overlaps_player(IVec2::new(5, 3), Vec2::new(3.5, 3.5)));
    }
}
