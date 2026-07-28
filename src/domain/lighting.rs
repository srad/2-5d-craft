use crate::domain::BlockGrid;
use glam::IVec2;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LightCell {
    pub sky: u8,
    pub torch: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightGrid {
    min_x: i32,
    width: i32,
    height: i32,
    cells: Vec<LightCell>,
}

#[derive(Debug, Clone)]
pub struct DayCycle {
    pub phase: f32,
    pub previous_light_level: u8,
}

impl Default for DayCycle {
    fn default() -> Self {
        Self {
            phase: 0.20,
            previous_light_level: 15,
        }
    }
}

impl LightGrid {
    pub fn calculate(grid: &BlockGrid) -> Self {
        let (min_x, max_x) = grid.loaded_x_bounds().unwrap_or((0, 0));
        let mut result = Self {
            min_x,
            width: max_x - min_x,
            height: grid.height(),
            cells: vec![LightCell::default(); ((max_x - min_x) * grid.height()) as usize],
        };
        result.propagate_sky(grid);
        result.propagate_torches(grid);
        result
    }

    pub fn get(&self, coordinate: IVec2) -> LightCell {
        self.index(coordinate)
            .map(|index| self.cells[index])
            .unwrap_or_default()
    }

    pub fn visible_at(&self, grid: &BlockGrid, coordinate: IVec2) -> LightCell {
        let mut visible = self.get(coordinate);
        if grid
            .get(coordinate)
            .is_some_and(|kind| kind.def().light_opacity >= 15)
        {
            for direction in [IVec2::X, -IVec2::X, IVec2::Y, -IVec2::Y] {
                let neighbor = self.get(coordinate + direction);
                visible.sky = visible.sky.max(neighbor.sky);
                visible.torch = visible.torch.max(neighbor.torch);
            }
        }
        visible
    }

    fn index(&self, coordinate: IVec2) -> Option<usize> {
        if coordinate.x < self.min_x
            || coordinate.x >= self.min_x + self.width
            || coordinate.y < 0
            || coordinate.y >= self.height
        {
            None
        } else {
            Some((coordinate.y * self.width + coordinate.x - self.min_x) as usize)
        }
    }

    fn propagate_sky(&mut self, grid: &BlockGrid) {
        let mut queue = VecDeque::new();
        for x in self.min_x..self.min_x + self.width {
            let mut level = 15_u8;
            for y in (0..self.height).rev() {
                let coordinate = IVec2::new(x, y);
                let opacity = grid
                    .get(coordinate)
                    .map(|kind| kind.def().light_opacity)
                    .unwrap_or(0);
                level = level.saturating_sub(opacity);
                if level == 0 {
                    break;
                }
                let index = self.index(coordinate).unwrap();
                self.cells[index].sky = level;
                queue.push_back((coordinate, level));
            }
        }
        self.flood_channel(grid, queue, |cell| &mut cell.sky);
    }

    fn propagate_torches(&mut self, grid: &BlockGrid) {
        let mut queue = VecDeque::new();
        for (coordinate, kind) in grid.iter() {
            let level = kind.def().emitted_light;
            if level > 0 {
                let index = self.index(coordinate).unwrap();
                self.cells[index].torch = level;
                queue.push_back((coordinate, level));
            }
        }
        self.flood_channel(grid, queue, |cell| &mut cell.torch);
    }

    fn flood_channel(
        &mut self,
        grid: &BlockGrid,
        mut queue: VecDeque<(IVec2, u8)>,
        channel: impl Copy + Fn(&mut LightCell) -> &mut u8,
    ) {
        while let Some((coordinate, level)) = queue.pop_front() {
            if level <= 1 {
                continue;
            }
            for direction in [IVec2::X, -IVec2::X, IVec2::Y, -IVec2::Y] {
                let next = coordinate + direction;
                let Some(index) = self.index(next) else {
                    continue;
                };
                let opacity = grid
                    .get(next)
                    .map(|kind| kind.def().light_opacity)
                    .unwrap_or(0);
                let propagated = level.saturating_sub(1 + opacity);
                let target = channel(&mut self.cells[index]);
                if propagated > *target {
                    *target = propagated;
                    queue.push_back((next, propagated));
                }
            }
        }
    }
}

impl DayCycle {
    pub fn daylight(&self) -> f32 {
        let angle = self.phase * std::f32::consts::TAU;
        (0.15 + 0.85 * angle.sin().max(0.0)).clamp(0.15, 1.0)
    }

    pub fn light_level(&self) -> u8 {
        (self.daylight() * 15.0).round() as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{BlockChunk, BlockKind, CHUNK_WIDTH, WORLD_HEIGHT};

    fn empty_grid() -> BlockGrid {
        let mut grid = BlockGrid::new(WORLD_HEIGHT);
        grid.insert_chunk(
            BlockChunk::from_dense(0, vec![0; (CHUNK_WIDTH * WORLD_HEIGHT) as usize]).unwrap(),
        );
        grid
    }

    #[test]
    fn torch_light_falls_off_by_distance() {
        let mut grid = empty_grid();
        grid.set(IVec2::new(4, 4), BlockKind::Torch);
        let light = LightGrid::calculate(&grid);
        assert_eq!(light.get(IVec2::new(4, 4)).torch, 12);
        assert_eq!(light.get(IVec2::new(5, 4)).torch, 11);
    }

    #[test]
    fn day_cycle_has_bounded_light() {
        for phase in [0.0, 0.25, 0.5, 0.75, 0.999] {
            let cycle = DayCycle {
                phase,
                ..Default::default()
            };
            assert!((0.15..=1.0).contains(&cycle.daylight()));
            assert!(cycle.light_level() <= 15);
        }
    }
}
