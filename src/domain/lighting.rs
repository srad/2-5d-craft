use crate::domain::BlockState;
use std::collections::{BTreeSet, VecDeque};

pub const MAX_LIGHT_LEVEL: u8 = 15;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LightCell {
    pub sky: u8,
    pub block: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightVolume {
    min_x: i64,
    width: usize,
    height: i32,
    depth_slices: u8,
    cells: Vec<LightCell>,
}

impl Default for LightVolume {
    fn default() -> Self {
        Self::empty()
    }
}

impl LightVolume {
    pub const fn empty() -> Self {
        Self {
            min_x: 0,
            width: 0,
            height: 0,
            depth_slices: 0,
            cells: Vec::new(),
        }
    }

    pub fn calculate(
        min_x: i64,
        max_x_exclusive: i64,
        height: i32,
        depth_slices: u8,
        mut sample_voxel: impl FnMut(i64, i32, u8) -> Option<BlockState>,
    ) -> Self {
        let width = max_x_exclusive
            .checked_sub(min_x)
            .and_then(|width| usize::try_from(width).ok())
            .unwrap_or(0);
        let height_usize = usize::try_from(height).unwrap_or(0);
        let cell_count = width
            .checked_mul(height_usize)
            .and_then(|count| count.checked_mul(usize::from(depth_slices)))
            .unwrap_or(0);
        if cell_count == 0 {
            return Self::empty();
        }
        let mut result = Self {
            min_x,
            width,
            height,
            depth_slices,
            cells: vec![LightCell::default(); cell_count],
        };
        let mut opacity = vec![0; cell_count];
        let mut emission = vec![0; cell_count];
        for depth in 0..depth_slices {
            for y in 0..height {
                for offset in 0..width {
                    let x = min_x + offset as i64;
                    let index = result.index(x, y, depth).unwrap();
                    if let Some(state) = sample_voxel(x, y, depth) {
                        opacity[index] = state.def().light_opacity;
                        emission[index] = state.def().emitted_light.min(MAX_LIGHT_LEVEL);
                    }
                }
            }
        }
        result.propagate_sky(&opacity);
        result.propagate_blocks(&opacity, &emission);
        result
    }

    pub fn get(&self, x: i64, y: i32, depth: u8) -> LightCell {
        self.index(x, y, depth)
            .map(|index| self.cells[index])
            .unwrap_or_default()
    }

    pub fn changed_x_columns(&self, other: &Self) -> BTreeSet<i64> {
        let min_x = self.min_x.min(other.min_x);
        let max_x = self.max_x_exclusive().max(other.max_x_exclusive());
        let height = self.height.max(other.height);
        let depth_slices = self.depth_slices.max(other.depth_slices);
        let mut changed = BTreeSet::new();
        for x in min_x..max_x {
            'column: for depth in 0..depth_slices {
                for y in 0..height {
                    if self.get(x, y, depth) != other.get(x, y, depth) {
                        changed.insert(x);
                        break 'column;
                    }
                }
            }
        }
        changed
    }

    pub fn max_x_exclusive(&self) -> i64 {
        self.min_x.saturating_add(self.width as i64)
    }

    fn index(&self, x: i64, y: i32, depth: u8) -> Option<usize> {
        if x < self.min_x
            || x >= self.max_x_exclusive()
            || y < 0
            || y >= self.height
            || depth >= self.depth_slices
        {
            None
        } else {
            let x = usize::try_from(x - self.min_x).ok()?;
            let y = usize::try_from(y).ok()?;
            Some((usize::from(depth) * self.height as usize + y) * self.width + x)
        }
    }

    fn propagate_sky(&mut self, opacity: &[u8]) {
        let mut queue = VecDeque::new();
        for depth in 0..self.depth_slices {
            for x in self.min_x..self.max_x_exclusive() {
                let mut level = MAX_LIGHT_LEVEL;
                for y in (0..self.height).rev() {
                    let position = LightPosition { x, y, depth };
                    let index = self.index(x, y, depth).unwrap();
                    level = level.saturating_sub(opacity[index]);
                    if level == 0 {
                        break;
                    }
                    self.cells[index].sky = level;
                    queue.push_back((position, level));
                }
            }
        }
        self.flood_channel(opacity, queue, |cell| &mut cell.sky);
    }

    fn propagate_blocks(&mut self, opacity: &[u8], emission: &[u8]) {
        let mut queue = VecDeque::new();
        for (index, &level) in emission.iter().enumerate() {
            if level > 0 {
                self.cells[index].block = level;
                queue.push_back((self.position(index), level));
            }
        }
        self.flood_channel(opacity, queue, |cell| &mut cell.block);
    }

    fn flood_channel(
        &mut self,
        opacity: &[u8],
        mut queue: VecDeque<(LightPosition, u8)>,
        channel: impl Copy + Fn(&mut LightCell) -> &mut u8,
    ) {
        while let Some((position, level)) = queue.pop_front() {
            if level <= 1 {
                continue;
            }
            for next in neighbors(position) {
                let Some(index) = self.index(next.x, next.y, next.depth) else {
                    continue;
                };
                let propagated = level.saturating_sub(1_u8.saturating_add(opacity[index]));
                let target = channel(&mut self.cells[index]);
                if propagated > *target {
                    *target = propagated;
                    queue.push_back((next, propagated));
                }
            }
        }
    }

    fn position(&self, index: usize) -> LightPosition {
        let plane = self.width * self.height as usize;
        let depth = index / plane;
        let within_plane = index % plane;
        let y = within_plane / self.width;
        let x = within_plane % self.width;
        LightPosition {
            x: self.min_x + x as i64,
            y: y as i32,
            depth: depth as u8,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct LightPosition {
    x: i64,
    y: i32,
    depth: u8,
}

fn neighbors(position: LightPosition) -> [LightPosition; 6] {
    [
        LightPosition {
            x: position.x.saturating_add(1),
            ..position
        },
        LightPosition {
            x: position.x.saturating_sub(1),
            ..position
        },
        LightPosition {
            y: position.y + 1,
            ..position
        },
        LightPosition {
            y: position.y - 1,
            ..position
        },
        LightPosition {
            depth: position.depth.saturating_add(1),
            ..position
        },
        LightPosition {
            depth: position.depth.wrapping_sub(1),
            ..position
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn volume(
        width: i64,
        height: i32,
        depth_slices: u8,
        blocks: &HashMap<(i64, i32, u8), BlockState>,
    ) -> LightVolume {
        LightVolume::calculate(0, width, height, depth_slices, |x, y, depth| {
            blocks.get(&(x, y, depth)).copied()
        })
    }

    #[test]
    fn direct_sky_stays_full_through_vertical_air_and_stops_at_opaque_blocks() {
        let blocks = HashMap::from([((0, 2, 0), BlockState::STONE)]);
        let light = volume(1, 4, 1, &blocks);
        assert_eq!(light.get(0, 3, 0).sky, 15);
        assert_eq!(light.get(0, 2, 0).sky, 0);
        assert_eq!(light.get(0, 1, 0).sky, 0);
    }

    #[test]
    fn block_light_falls_off_across_all_six_neighbors() {
        let blocks = HashMap::from([((1, 1, 1), BlockState::TORCH)]);
        let light = volume(3, 3, 3, &blocks);
        assert_eq!(light.get(1, 1, 1).block, 12);
        for position in [
            (0, 1, 1),
            (2, 1, 1),
            (1, 0, 1),
            (1, 2, 1),
            (1, 1, 0),
            (1, 1, 2),
        ] {
            assert_eq!(light.get(position.0, position.1, position.2).block, 11);
        }
    }

    #[test]
    fn opacity_attenuates_flooded_light() {
        let blocks = HashMap::from([
            ((0, 1, 0), BlockState::TORCH),
            ((1, 1, 0), BlockState::STONE),
        ]);
        let light = volume(3, 3, 1, &blocks);
        assert_eq!(light.get(1, 1, 0).block, 0);
    }

    #[test]
    fn depth_boundaries_do_not_seed_or_wrap_light() {
        let blocks = HashMap::from([
            ((0, 1, 0), BlockState::TORCH),
            ((0, 1, 1), BlockState::STONE),
        ]);
        let light = volume(1, 3, 2, &blocks);
        assert_eq!(light.get(0, 1, 0).block, 12);
        assert_eq!(light.get(0, 1, 1).block, 0);
        assert_eq!(light.get(0, 1, u8::MAX).block, 0);
    }

    #[test]
    fn changed_columns_include_added_and_removed_bounds() {
        let empty = LightVolume::empty();
        let light = volume(2, 2, 1, &HashMap::new());
        assert_eq!(empty.changed_x_columns(&light), BTreeSet::from([0, 1]));
        assert_eq!(light.changed_x_columns(&light), BTreeSet::new());
    }
}
