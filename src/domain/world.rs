use crate::domain::{BlockKind, CHUNK_WIDTH};
use glam::{IVec2, Vec2};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockChunk {
    x: i32,
    blocks: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockGrid {
    height: i32,
    chunks: HashMap<i32, BlockChunk>,
}

impl BlockChunk {
    pub fn from_dense(chunk_x: i32, blocks: Vec<u8>) -> Option<Self> {
        let expected = (CHUNK_WIDTH * crate::domain::WORLD_HEIGHT) as usize;
        if blocks.len() != expected
            || blocks
                .iter()
                .any(|code| *code != 0 && BlockKind::from_code(*code).is_none())
        {
            return None;
        }
        Some(Self { x: chunk_x, blocks })
    }

    pub fn x(&self) -> i32 {
        self.x
    }

    pub fn blocks(&self) -> &[u8] {
        &self.blocks
    }

    fn get(&self, local_x: i32, y: i32) -> Option<BlockKind> {
        BlockKind::from_code(self.blocks[chunk_index(local_x, y)])
    }

    fn set(&mut self, local_x: i32, y: i32, kind: Option<BlockKind>) {
        self.blocks[chunk_index(local_x, y)] = kind.map_or(0, BlockKind::code);
    }
}

impl BlockGrid {
    pub fn new(height: i32) -> Self {
        Self {
            height,
            chunks: HashMap::new(),
        }
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    pub fn in_bounds(&self, coordinate: IVec2) -> bool {
        (0..self.height).contains(&coordinate.y)
    }

    pub fn contains_chunk(&self, chunk_x: i32) -> bool {
        self.chunks.contains_key(&chunk_x)
    }

    pub fn loaded_chunks(&self) -> impl Iterator<Item = i32> + '_ {
        self.chunks.keys().copied()
    }

    pub fn loaded_x_bounds(&self) -> Option<(i32, i32)> {
        let minimum = self.chunks.keys().copied().min()? * CHUNK_WIDTH;
        let maximum = (self.chunks.keys().copied().max()? + 1) * CHUNK_WIDTH;
        Some((minimum, maximum))
    }

    pub fn insert_chunk(&mut self, chunk: BlockChunk) -> Option<BlockChunk> {
        self.chunks.insert(chunk.x, chunk)
    }

    pub fn remove_chunk(&mut self, chunk_x: i32) -> Option<BlockChunk> {
        self.chunks.remove(&chunk_x)
    }

    pub fn get(&self, coordinate: IVec2) -> Option<BlockKind> {
        if !self.in_bounds(coordinate) {
            return None;
        }
        let chunk_x = world_to_chunk(coordinate.x);
        let local_x = coordinate.x.rem_euclid(CHUNK_WIDTH);
        self.chunks.get(&chunk_x)?.get(local_x, coordinate.y)
    }

    pub fn set(&mut self, coordinate: IVec2, kind: BlockKind) -> Option<BlockKind> {
        assert!(
            self.in_bounds(coordinate),
            "tile coordinate is vertically out of bounds"
        );
        let chunk_x = world_to_chunk(coordinate.x);
        let local_x = coordinate.x.rem_euclid(CHUNK_WIDTH);
        let chunk = self
            .chunks
            .get_mut(&chunk_x)
            .expect("the target chunk must be loaded before editing");
        let previous = chunk.get(local_x, coordinate.y);
        chunk.set(local_x, coordinate.y, Some(kind));
        previous
    }

    pub fn remove(&mut self, coordinate: IVec2) -> Option<BlockKind> {
        if !self.in_bounds(coordinate) {
            return None;
        }
        let chunk_x = world_to_chunk(coordinate.x);
        let local_x = coordinate.x.rem_euclid(CHUNK_WIDTH);
        let chunk = self.chunks.get_mut(&chunk_x)?;
        let previous = chunk.get(local_x, coordinate.y);
        if previous.is_some() {
            chunk.set(local_x, coordinate.y, None);
        }
        previous
    }

    pub fn iter(&self) -> impl Iterator<Item = (IVec2, BlockKind)> + '_ {
        self.chunks.values().flat_map(|chunk| {
            chunk
                .blocks
                .iter()
                .enumerate()
                .filter_map(move |(index, code)| {
                    let kind = BlockKind::from_code(*code)?;
                    let local_x = index as i32 % CHUNK_WIDTH;
                    let y = index as i32 / CHUNK_WIDTH;
                    Some((IVec2::new(chunk.x * CHUNK_WIDTH + local_x, y), kind))
                })
        })
    }

    pub fn safe_spawn(&self) -> Vec2 {
        for distance in 0..CHUNK_WIDTH {
            for x in [distance, -distance] {
                for y in (0..self.height - 2).rev() {
                    if self.get(IVec2::new(x, y)) == Some(BlockKind::Grass)
                        && self.get(IVec2::new(x, y + 1)).is_none()
                        && self.get(IVec2::new(x, y + 2)).is_none()
                    {
                        return Vec2::new(x as f32 + 0.5, y as f32 + 1.9);
                    }
                }
            }
        }
        Vec2::new(0.5, self.height as f32 - 2.0)
    }

    pub fn player_position_is_safe(&self, position: Vec2) -> bool {
        if !position.is_finite() || position.y < 0.9 || position.y > self.height as f32 {
            return false;
        }
        let min_tile = (position - Vec2::new(0.34, 0.88)).floor().as_ivec2();
        let max_tile = (position + Vec2::new(0.34, 0.88)).floor().as_ivec2();
        if !self.contains_chunk(world_to_chunk(min_tile.x))
            || !self.contains_chunk(world_to_chunk(max_tile.x))
        {
            return false;
        }
        for x in min_tile.x..=max_tile.x {
            for y in min_tile.y..=max_tile.y {
                if self
                    .get(IVec2::new(x, y))
                    .is_some_and(|kind| kind.def().solid)
                {
                    return false;
                }
            }
        }
        true
    }

    pub(crate) fn chunk_snapshot(&self, chunk_x: i32) -> Option<Vec<u8>> {
        self.chunks.get(&chunk_x).map(|chunk| chunk.blocks.clone())
    }

    pub(crate) fn rebase(&mut self, delta_chunks: i32) {
        self.chunks = self
            .chunks
            .drain()
            .map(|(chunk_x, mut chunk)| {
                let rebased = chunk_x - delta_chunks;
                chunk.x = rebased;
                (rebased, chunk)
            })
            .collect();
    }
}

pub const fn world_to_chunk(world_x: i32) -> i32 {
    world_x.div_euclid(CHUNK_WIDTH)
}

fn chunk_index(local_x: i32, y: i32) -> usize {
    (y * CHUNK_WIDTH + local_x) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{BlockKind, WORLD_HEIGHT, generate_chunk};

    #[test]
    fn negative_coordinates_use_euclidean_chunks() {
        assert_eq!(world_to_chunk(-33), -2);
        assert_eq!(world_to_chunk(-32), -1);
        assert_eq!(world_to_chunk(-1), -1);
        assert_eq!(world_to_chunk(0), 0);
        assert_eq!(world_to_chunk(31), 0);
        assert_eq!(world_to_chunk(32), 1);
    }

    #[test]
    fn dense_chunk_editing_affects_loaded_data() {
        let mut grid = BlockGrid::new(WORLD_HEIGHT);
        grid.insert_chunk(generate_chunk(1, -1));
        let coordinate = IVec2::new(-1, 70);
        assert_eq!(grid.set(coordinate, BlockKind::Torch), None);
        assert_eq!(grid.get(coordinate), Some(BlockKind::Torch));
        assert_eq!(grid.remove(coordinate), Some(BlockKind::Torch));
        assert_eq!(grid.get(coordinate), None);
    }
}
