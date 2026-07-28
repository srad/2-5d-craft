use crate::domain::{
    BlockChunk, BlockGrid, BlockState, CHUNK_WIDTH, DEPTH_SLICES, WORLD_HEIGHT, WorldMutator,
};
use glam::Vec2;

pub fn stable_hash(seed: u64, x: i64, y: i32, salt: u64) -> u64 {
    let mut value = seed
        ^ (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (y as i64 as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9)
        ^ salt;
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn stable_hash_3d(seed: u64, x: i64, y: i32, depth: i32, salt: u64) -> u64 {
    stable_hash(
        seed ^ (depth as i64 as u64).wrapping_mul(0xd6e8_feb8_6659_fd93),
        x,
        y,
        salt,
    )
}

pub fn surface_height(seed: u64, x: i64) -> i32 {
    surface_height_at_depth(seed, x, 0)
}

fn smooth_step(value: f32) -> f32 {
    value * value * (3.0 - 2.0 * value)
}

fn lattice_noise(seed: u64, x: i64, depth: i32, salt: u64) -> f32 {
    let hash = stable_hash_3d(seed, x, 0, depth, salt);
    hash as f64 as f32 / u64::MAX as f32 * 2.0 - 1.0
}

fn value_noise_2d(
    seed: u64,
    x: i64,
    depth: i32,
    x_period: i64,
    depth_period: i32,
    salt: u64,
) -> f32 {
    let cell_x = x.div_euclid(x_period);
    let cell_depth = depth.div_euclid(depth_period);
    let fx = smooth_step(x.rem_euclid(x_period) as f32 / x_period as f32);
    let fz = smooth_step(depth.rem_euclid(depth_period) as f32 / depth_period as f32);
    let near_left = lattice_noise(seed, cell_x, cell_depth, salt);
    let near_right = lattice_noise(seed, cell_x + 1, cell_depth, salt);
    let far_left = lattice_noise(seed, cell_x, cell_depth + 1, salt);
    let far_right = lattice_noise(seed, cell_x + 1, cell_depth + 1, salt);
    let near = near_left + (near_right - near_left) * fx;
    let far = far_left + (far_right - far_left) * fx;
    near + (far - near) * fz
}

pub fn surface_height_at_depth(seed: u64, x: i64, depth: u8) -> i32 {
    let depth = i32::from(depth);
    let coarse = value_noise_2d(seed, x, depth, 16, 4, 11) * 7.0;
    let detail = value_noise_2d(seed, x, depth, 4, 2, 29) * 2.0;
    (38.0 + coarse + detail).round().clamp(20.0, 58.0) as i32
}

fn tree_root(seed: u64, x: i64, depth: i32) -> bool {
    x.abs() >= 5 && stable_hash_3d(seed, x, 0, depth, 313).is_multiple_of(29)
}

pub fn generated_voxel(seed: u64, x: i64, y: i32, depth: u8) -> Option<BlockState> {
    if !(0..WORLD_HEIGHT).contains(&y) || depth >= DEPTH_SLICES {
        return None;
    }
    let depth_i32 = i32::from(depth);
    let surface = surface_height_at_depth(seed, x, depth);
    if y > surface {
        for root_depth in depth_i32 - 2..=depth_i32 + 2 {
            if root_depth < 0 {
                continue;
            }
            for root_x in x - 2..=x + 2 {
                if !tree_root(seed, root_x, root_depth) {
                    continue;
                }
                let root_y = surface_height_at_depth(seed, root_x, root_depth as u8) + 1;
                if x == root_x && depth_i32 == root_depth && (root_y..root_y + 3).contains(&y) {
                    return Some(BlockState::WOOD);
                }
                let canopy_distance = (x - root_x).abs()
                    + i64::from((depth_i32 - root_depth).abs())
                    + i64::from((y - (root_y + 3)).abs());
                if canopy_distance <= 3 && (root_y + 2..=root_y + 5).contains(&y) {
                    return Some(BlockState::LEAVES);
                }
            }
        }
        return None;
    }
    if y == 0 {
        return Some(BlockState::BEDROCK);
    }
    if y == surface {
        return Some(BlockState::GRASS);
    }
    if y >= surface - 4 {
        return Some(BlockState::DIRT);
    }
    let cave_cell = stable_hash_3d(seed, x.div_euclid(4), y.div_euclid(4), depth_i32, 401) % 1000;
    if y > 4 && y < surface - 5 && cave_cell < 105 {
        return None;
    }
    let ore = stable_hash_3d(seed, x, y, depth_i32, 101) % 1000;
    if y < 28 && ore < 18 {
        Some(BlockState::IRON_ORE)
    } else if y < 36 && ore < 55 {
        Some(BlockState::COAL_ORE)
    } else {
        Some(BlockState::STONE)
    }
}

pub fn generate_chunk(seed: u64, chunk_x: i64) -> BlockChunk {
    generate_chunk_at(seed, chunk_x)
}

pub(crate) fn generate_chunk_at(seed: u64, global_chunk_x: i64) -> BlockChunk {
    let mut blocks = vec![0; (CHUNK_WIDTH * WORLD_HEIGHT) as usize];
    let start_x = global_chunk_x * i64::from(CHUNK_WIDTH);
    let end_x = start_x + i64::from(CHUNK_WIDTH);
    for world_x in start_x..end_x {
        let local_x = world_x.rem_euclid(i64::from(CHUNK_WIDTH)) as i32;
        for y in 0..WORLD_HEIGHT {
            if let Some(kind) = generated_voxel(seed, world_x, y, 0) {
                blocks[(y * CHUNK_WIDTH + local_x) as usize] = kind.code();
            }
        }
    }
    BlockChunk::from_dense(global_chunk_x, blocks).expect("generated chunks are valid")
}

fn generate_world(seed: u64) -> BlockGrid {
    let mut grid = BlockGrid::new(WORLD_HEIGHT);
    for chunk_x in -1..=1 {
        WorldMutator::new(&mut grid).integrate_chunk(generate_chunk(seed, chunk_x));
    }
    grid
}

pub fn spawn_for_seed(seed: u64) -> Vec2 {
    generate_world(seed).view().safe_spawn(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_repeatable_across_positive_and_negative_chunks() {
        for chunk_x in [-10, -1, 0, 1, 10] {
            assert_eq!(generate_chunk(123, chunk_x), generate_chunk(123, chunk_x));
            assert_ne!(generate_chunk(123, chunk_x), generate_chunk(124, chunk_x));
        }
    }

    #[test]
    fn generation_remains_deterministic_at_large_global_coordinates() {
        let left = generate_chunk_at(29, 1_000_000_000_000);
        let again = generate_chunk_at(29, 1_000_000_000_000);
        let right = generate_chunk_at(29, 1_000_000_000_001);
        assert_eq!(left, again);
        assert_ne!(left.blocks(), right.blocks());
    }

    #[test]
    fn depth_slices_are_correlated_but_not_identical() {
        let samples = (-64..64)
            .map(|x| {
                [
                    surface_height_at_depth(71, x, 0),
                    surface_height_at_depth(71, x, 1),
                    surface_height_at_depth(71, x, 2),
                    surface_height_at_depth(71, x, 3),
                ]
            })
            .collect::<Vec<_>>();
        assert!(samples.iter().any(|heights| heights[0] != heights[1]));
    }
}
