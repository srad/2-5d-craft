use crate::application::{ChunkSnapshot, PlayerSnapshot, WorldSession, WorldSnapshot};
use crate::domain::{
    BlockChunk, BlockGrid, BlockKind, WORLD_HEIGHT, generate_chunk_at, world_to_chunk,
};
use glam::IVec2;
use std::collections::{BTreeMap, HashSet};

#[derive(Debug)]
pub struct WorldState {
    pub grid: BlockGrid,
    persisted_chunks: BTreeMap<i64, Vec<u8>>,
    dirty_loaded_chunks: HashSet<i32>,
    pub origin_chunk: i64,
    pub revision: u64,
    pub seed: u64,
}

impl WorldState {
    pub fn from_snapshot(snapshot: &WorldSnapshot) -> Self {
        let persisted_chunks = snapshot
            .chunks
            .iter()
            .map(|chunk| (chunk.x, chunk.blocks.clone()))
            .collect::<BTreeMap<_, _>>();
        let requested = glam::Vec2::new(snapshot.player.local_x, snapshot.player.y);
        let center_chunk = world_to_chunk(requested.x.floor() as i32);
        let origin_chunk = snapshot.player.chunk_x;
        let global_chunk = origin_chunk + i64::from(center_chunk);
        let initial = persisted_chunks
            .get(&global_chunk)
            .cloned()
            .and_then(|blocks| BlockChunk::from_dense(center_chunk, blocks))
            .unwrap_or_else(|| generate_chunk_at(snapshot.seed, global_chunk, center_chunk));
        let mut grid = BlockGrid::new(WORLD_HEIGHT);
        grid.insert_chunk(initial);
        Self {
            grid,
            persisted_chunks,
            dirty_loaded_chunks: HashSet::new(),
            origin_chunk,
            revision: 0,
            seed: snapshot.seed,
        }
    }

    pub fn persisted_blocks(&self, global_chunk_x: i64) -> Option<Vec<u8>> {
        self.persisted_chunks.get(&global_chunk_x).cloned()
    }

    pub fn integrate_chunk(&mut self, chunk: BlockChunk) {
        self.grid.insert_chunk(chunk);
    }

    pub fn unload_chunk(&mut self, chunk_x: i32) {
        if let Some(chunk) = self.grid.remove_chunk(chunk_x)
            && self.dirty_loaded_chunks.remove(&chunk_x)
        {
            self.persisted_chunks.insert(
                self.origin_chunk + i64::from(chunk_x),
                chunk.blocks().to_vec(),
            );
        }
    }

    pub fn rebase(&mut self, delta_chunks: i32) {
        self.grid.rebase(delta_chunks);
        self.dirty_loaded_chunks = self
            .dirty_loaded_chunks
            .drain()
            .map(|chunk_x| chunk_x - delta_chunks)
            .collect();
        self.origin_chunk += i64::from(delta_chunks);
    }

    pub fn snapshot_chunks(&self) -> Vec<ChunkSnapshot> {
        let mut chunks = self.persisted_chunks.clone();
        for chunk_x in &self.dirty_loaded_chunks {
            if let Some(blocks) = self.grid.chunk_snapshot(*chunk_x) {
                chunks.insert(self.origin_chunk + i64::from(*chunk_x), blocks);
            }
        }
        chunks
            .into_iter()
            .map(|(x, blocks)| ChunkSnapshot { x, blocks })
            .collect()
    }

    pub fn snapshot(
        &self,
        session: &WorldSession,
        player_position: glam::Vec2,
        selected_slot: u8,
        day_phase: f32,
        last_played_unix_s: u64,
    ) -> WorldSnapshot {
        WorldSnapshot {
            generator_version: session.generator_version,
            height: self.grid.height(),
            name: session.name.clone(),
            seed: session.seed,
            created_at_unix_s: session.created_at_unix_s,
            last_played_unix_s,
            day_phase,
            player: PlayerSnapshot {
                chunk_x: self.origin_chunk
                    + (player_position.x.floor() as i64)
                        .div_euclid(i64::from(crate::domain::CHUNK_WIDTH)),
                local_x: player_position
                    .x
                    .rem_euclid(crate::domain::CHUNK_WIDTH as f32),
                y: player_position.y,
                selected_slot,
            },
            chunks: self.snapshot_chunks(),
        }
    }

    fn mark_changed(&mut self, coordinate: IVec2) {
        self.dirty_loaded_chunks
            .insert(world_to_chunk(coordinate.x));
        self.revision = self.revision.wrapping_add(1);
    }
}

pub fn remove_tile(world: &mut WorldState, coordinate: IVec2) -> Vec<(IVec2, BlockKind)> {
    let mut removed = Vec::new();
    if let Some(kind) = world.grid.remove(coordinate) {
        removed.push((coordinate, kind));
        let above = coordinate + IVec2::Y;
        if world.grid.get(above) == Some(BlockKind::Torch) {
            world.grid.remove(above);
            removed.push((above, BlockKind::Torch));
        }
        world.mark_changed(coordinate);
    }
    removed
}

pub fn place_tile(world: &mut WorldState, coordinate: IVec2, kind: BlockKind) -> bool {
    if !world.grid.in_bounds(coordinate)
        || !world.grid.contains_chunk(world_to_chunk(coordinate.x))
        || world.grid.get(coordinate).is_some()
    {
        return false;
    }
    if kind == BlockKind::Torch
        && !world
            .grid
            .get(coordinate - IVec2::Y)
            .is_some_and(|support| support.def().solid)
    {
        return false;
    }
    world.grid.set(coordinate, kind);
    world.mark_changed(coordinate);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{WorldId, blank_snapshot};
    use crate::domain::{BlockKind, spawn_for_seed};

    #[test]
    fn rebasing_preserves_global_dirty_chunk_identity() {
        let mut world = WorldState::from_snapshot(&blank_snapshot(
            11,
            "Test".into(),
            vec![],
            spawn_for_seed(11),
            1,
        ));
        let coordinate = IVec2::new(0, 70);
        assert!(place_tile(&mut world, coordinate, BlockKind::Dirt));
        world.rebase(8);
        assert_eq!(world.snapshot_chunks()[0].x, 0);
    }

    #[test]
    fn snapshot_converts_local_player_position_to_global_chunk_identity() {
        let mut world = WorldState::from_snapshot(&blank_snapshot(
            11,
            "Test".into(),
            vec![],
            spawn_for_seed(11),
            1,
        ));
        world.rebase(8);
        let session = WorldSession {
            id: WorldId::new("test").unwrap(),
            name: "Test".into(),
            seed: 11,
            generator_version: crate::domain::GENERATOR_VERSION,
            created_at_unix_s: 1,
            saved_revision: 0,
        };
        let snapshot = world.snapshot(&session, glam::Vec2::new(1.5, 40.0), 3, 0.75, 9);
        assert_eq!(snapshot.player.chunk_x, 8);
        assert_eq!(snapshot.player.local_x, 1.5);
        assert_eq!(snapshot.player.selected_slot, 3);
        assert_eq!(snapshot.day_phase, 0.75);
        assert_eq!(snapshot.last_played_unix_s, 9);
    }
}
