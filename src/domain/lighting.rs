use crate::domain::{VoxelPos, WorldView};
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LightCell {
    pub sky: u8,
    pub torch: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightGrid {
    min_x: i64,
    width: i64,
    height: i32,
    cells: Vec<LightCell>,
}

impl LightGrid {
    pub fn calculate(view: &WorldView<'_>) -> Self {
        let (min_x, max_x) = view.loaded_x_bounds().unwrap_or((0, 0));
        let mut result = Self {
            min_x,
            width: max_x - min_x,
            height: view.height(),
            cells: vec![
                LightCell::default();
                ((max_x - min_x) * i64::from(view.height())) as usize
            ],
        };
        result.propagate_sky(view);
        result.propagate_torches(view);
        result
    }

    pub fn get(&self, position: VoxelPos) -> LightCell {
        self.index(position)
            .map(|index| self.cells[index])
            .unwrap_or_default()
    }

    pub fn visible_at(&self, view: &WorldView<'_>, position: VoxelPos) -> LightCell {
        let mut visible = self.get(position);
        if view
            .block(position)
            .is_some_and(|state| state.def().light_opacity >= 15)
        {
            for neighbor_position in neighbors(position) {
                let neighbor = self.get(neighbor_position);
                visible.sky = visible.sky.max(neighbor.sky);
                visible.torch = visible.torch.max(neighbor.torch);
            }
        }
        visible
    }

    fn index(&self, position: VoxelPos) -> Option<usize> {
        if position.global_x < self.min_x
            || position.global_x >= self.min_x + self.width
            || position.y < 0
            || position.y >= self.height
        {
            None
        } else {
            Some((i64::from(position.y) * self.width + position.global_x - self.min_x) as usize)
        }
    }

    fn propagate_sky(&mut self, view: &WorldView<'_>) {
        let mut queue = VecDeque::new();
        for x in self.min_x..self.min_x + self.width {
            let mut level = 15_u8;
            for y in (0..self.height).rev() {
                let position = VoxelPos::foreground(x, y);
                let opacity = view
                    .block(position)
                    .map(|state| state.def().light_opacity)
                    .unwrap_or(0);
                level = level.saturating_sub(opacity);
                if level == 0 {
                    break;
                }
                let index = self.index(position).unwrap();
                self.cells[index].sky = level;
                queue.push_back((position, level));
            }
        }
        self.flood_channel(view, queue, |cell| &mut cell.sky);
    }

    fn propagate_torches(&mut self, view: &WorldView<'_>) {
        let mut queue = VecDeque::new();
        for (position, state) in view.iter() {
            let level = state.def().emitted_light;
            if level > 0 {
                let index = self.index(position).unwrap();
                self.cells[index].torch = level;
                queue.push_back((position, level));
            }
        }
        self.flood_channel(view, queue, |cell| &mut cell.torch);
    }

    fn flood_channel(
        &mut self,
        view: &WorldView<'_>,
        mut queue: VecDeque<(VoxelPos, u8)>,
        channel: impl Copy + Fn(&mut LightCell) -> &mut u8,
    ) {
        while let Some((position, level)) = queue.pop_front() {
            if level <= 1 {
                continue;
            }
            for next in neighbors(position) {
                let Some(index) = self.index(next) else {
                    continue;
                };
                let opacity = view
                    .block(next)
                    .map(|state| state.def().light_opacity)
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

fn neighbors(position: VoxelPos) -> [VoxelPos; 4] {
    [
        VoxelPos {
            global_x: position.global_x + 1,
            ..position
        },
        VoxelPos {
            global_x: position.global_x - 1,
            ..position
        },
        VoxelPos {
            y: position.y + 1,
            ..position
        },
        VoxelPos {
            y: position.y - 1,
            ..position
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        BlockChunk, BlockGrid, BlockState, CHUNK_WIDTH, WORLD_HEIGHT, WorldMutator,
    };

    fn empty_grid() -> BlockGrid {
        let mut grid = BlockGrid::new(WORLD_HEIGHT);
        WorldMutator::new(&mut grid).integrate_chunk(
            BlockChunk::from_dense(0, vec![0; (CHUNK_WIDTH * WORLD_HEIGHT) as usize]).unwrap(),
        );
        grid
    }

    #[test]
    fn torch_light_falls_off_by_distance() {
        let mut grid = empty_grid();
        let position = VoxelPos::foreground(4, 4);
        WorldMutator::new(&mut grid).commit_batch(vec![crate::domain::MutationProposal {
            preconditions: vec![crate::domain::BlockPrecondition {
                position,
                expected: None,
            }],
            writes: vec![crate::domain::BlockWrite {
                position,
                state: Some(BlockState::TORCH),
            }],
            priority: crate::domain::MutationPriority::PLAYER,
            source: position,
            sequence: 0,
        }]);
        let light = LightGrid::calculate(&grid.view());
        assert_eq!(light.get(position).torch, 12);
        assert_eq!(light.get(VoxelPos::foreground(5, 4)).torch, 11);
    }
}
