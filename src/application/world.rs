use crate::application::{ChunkSnapshot, PlayerSnapshot, WorldSession, WorldSnapshot};
use crate::domain::{
    BlockChunk, BlockGrid, BlockPrecondition, BlockState, BlockWrite, ChunkLayer,
    MutationBatchResult, MutationPriority, MutationProposal, MutationReport, ScheduledTick,
    ScheduledTickQueue, TorchMount, VoxelCell, VoxelLayer, VoxelPos, WORLD_HEIGHT, WorldMutator,
    WorldView, generate_chunk_at, world_to_chunk,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub struct WorldState {
    grid: BlockGrid,
    persisted_chunks: BTreeMap<i64, BlockChunk>,
    dirty_chunks: BTreeSet<i64>,
    pending_ticks: ScheduledTickQueue,
    origin_chunk: i64,
    revision: u64,
    seed: u64,
    world_tick: u64,
}

#[derive(Debug)]
pub struct InitializedWorld {
    pub state: WorldState,
    pub report: MutationReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldCommandError {
    Unavailable,
    Occupied,
    Unsupported,
    Unbreakable,
}

/// Why the world refused to queue simulation work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleRejection {
    /// The target chunk is not loaded. Simulation never force-loads a frontier chunk, so callers
    /// defer this work until the chunk activates.
    Unloaded,
    /// The target cell lies outside the world height.
    OutOfBounds,
}

impl WorldState {
    pub fn from_snapshot(snapshot: &WorldSnapshot) -> InitializedWorld {
        let persisted_chunks = snapshot
            .chunks
            .iter()
            .filter_map(|chunk| {
                BlockChunk::from_dense(chunk.x, chunk.foreground.clone(), chunk.backwall.clone())
                    .map(|blocks| (chunk.x, blocks))
            })
            .collect::<BTreeMap<_, _>>();
        let pending_ticks = ScheduledTickQueue::from_chunks(
            snapshot
                .chunks
                .iter()
                .filter(|chunk| !chunk.pending_ticks.is_empty())
                .map(|chunk| (chunk.x, chunk.pending_ticks.clone())),
            snapshot.next_tick_sequence,
        );
        let center_local_chunk = world_to_chunk(snapshot.player.local_x.floor() as i64);
        let origin_chunk = snapshot.player.chunk_x;
        let global_chunk = origin_chunk + center_local_chunk;
        let initial = persisted_chunks
            .get(&global_chunk)
            .cloned()
            .unwrap_or_else(|| generate_chunk_at(snapshot.seed, global_chunk));
        let mut grid = BlockGrid::new(WORLD_HEIGHT);
        let report = WorldMutator::new(&mut grid).integrate_chunk(initial);
        InitializedWorld {
            state: Self {
                grid,
                persisted_chunks,
                dirty_chunks: BTreeSet::new(),
                pending_ticks,
                origin_chunk,
                revision: 0,
                seed: snapshot.seed,
                world_tick: snapshot.world_tick,
            },
            report,
        }
    }

    pub fn view(&self) -> WorldView<'_> {
        self.grid.view()
    }

    pub fn origin_chunk(&self) -> i64 {
        self.origin_chunk
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn world_tick(&self) -> u64 {
        self.world_tick
    }

    /// Completes one logical simulation step and returns the new tick.
    pub fn advance_world_tick(&mut self) -> u64 {
        self.world_tick += 1;
        self.world_tick
    }

    pub fn queued_ticks(&self) -> usize {
        self.pending_ticks.queued_len()
    }

    /// Queues simulation work for a later tick.
    ///
    /// Scheduling requires a loaded chunk and marks that chunk dirty, because [`Self::snapshot_chunks`]
    /// can only attach pending ticks to a chunk that owns a dense block array. Work for an
    /// unloaded chunk is refused so callers defer it instead of creating unpersistable state.
    pub fn schedule_tick(
        &mut self,
        position: VoxelPos,
        priority: MutationPriority,
        due_world_tick: u64,
        expected: Option<BlockState>,
    ) -> Result<ScheduledTick, ScheduleRejection> {
        if !(0..self.view().height()).contains(&position.y) {
            return Err(ScheduleRejection::OutOfBounds);
        }
        let chunk_x = world_to_chunk(position.global_x);
        if !self
            .view()
            .contains_chunk(ChunkLayer::new(chunk_x, position.layer))
        {
            return Err(ScheduleRejection::Unloaded);
        }
        self.dirty_chunks.insert(chunk_x);
        Ok(self
            .pending_ticks
            .schedule(position, priority, due_world_tick, expected))
    }

    /// Removes the work due at the current tick for the given sorted active chunks.
    pub fn take_due_ticks(&mut self, active: &[i64]) -> Vec<ScheduledTick> {
        self.pending_ticks.take_due(self.world_tick, active)
    }

    pub fn persisted_chunk(&self, global_chunk_x: i64) -> Option<BlockChunk> {
        self.persisted_chunks.get(&global_chunk_x).cloned()
    }

    pub fn integrate_chunk(&mut self, chunk: BlockChunk) -> MutationReport {
        WorldMutator::new(&mut self.grid).integrate_chunk(chunk)
    }

    pub fn unload_chunk(&mut self, chunk_x: i64) -> MutationReport {
        let (removed, report) = WorldMutator::new(&mut self.grid).unload_chunk(chunk_x);
        // A chunk that owes simulation work must keep its blocks even when nothing was edited,
        // otherwise the next save has no dense array to attach those ticks to.
        if let Some(chunk) = removed
            && (self.dirty_chunks.remove(&chunk_x) || self.pending_ticks.owes_work(chunk_x))
        {
            self.persisted_chunks.insert(chunk_x, chunk);
        }
        report
    }

    pub fn rebase(&mut self, delta_chunks: i32) {
        self.origin_chunk += i64::from(delta_chunks);
    }

    pub fn commit_batch(&mut self, proposals: Vec<MutationProposal>) -> MutationBatchResult {
        let result = WorldMutator::new(&mut self.grid).commit_batch(proposals);
        if !result.report.cell_changes.is_empty() {
            self.dirty_chunks.extend(
                result
                    .report
                    .cell_changes
                    .iter()
                    .map(|change| world_to_chunk(change.position.global_x)),
            );
            self.revision = self.revision.wrapping_add(1);
        }
        result
    }

    pub fn snapshot_chunks(&self) -> Vec<ChunkSnapshot> {
        let mut chunks = self.persisted_chunks.clone();
        for chunk_x in &self.dirty_chunks {
            if let Some(chunk) = self.grid.chunk_snapshot(*chunk_x) {
                chunks.insert(*chunk_x, chunk);
            }
        }
        debug_assert!(
            self.pending_ticks
                .chunks_owing_work()
                .all(|chunk_x| chunks.contains_key(&chunk_x)),
            "scheduling marks its chunk dirty and unload_chunk retains it, so owed work always \
             has a dense array to attach to"
        );
        chunks
            .into_iter()
            .map(|(x, chunk)| ChunkSnapshot {
                x,
                foreground: chunk.blocks(VoxelLayer::Foreground).to_vec(),
                backwall: chunk.blocks(VoxelLayer::Backwall).to_vec(),
                pending_ticks: self.pending_ticks.pending_for_chunk(x).collect(),
            })
            .collect()
    }

    pub fn snapshot(
        &self,
        session: &WorldSession,
        player_position: glam::Vec2,
        selected_slot: u8,
        day_time_ticks: u64,
        last_played_unix_s: u64,
    ) -> WorldSnapshot {
        WorldSnapshot {
            generator_version: session.generator_version,
            height: self.view().height(),
            name: session.name.clone(),
            seed: session.seed,
            created_at_unix_s: session.created_at_unix_s,
            last_played_unix_s,
            day_time_ticks,
            world_tick: self.world_tick,
            next_tick_sequence: self.pending_ticks.next_sequence(),
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

    pub fn local_to_voxel(&self, coordinate: glam::IVec2, layer: VoxelLayer) -> VoxelPos {
        VoxelPos::new(
            self.origin_chunk * i64::from(crate::domain::CHUNK_WIDTH) + i64::from(coordinate.x),
            coordinate.y,
            layer,
        )
    }
}

pub fn place_block(
    world: &mut WorldState,
    position: VoxelPos,
    state: BlockState,
) -> Result<MutationBatchResult, WorldCommandError> {
    if world.view().cell(position) != VoxelCell::Air {
        return Err(match world.view().cell(position) {
            VoxelCell::Block(_) => WorldCommandError::Occupied,
            _ => WorldCommandError::Unavailable,
        });
    }
    if let Some(mount) = state.torch_mount() {
        let support_position = offset_position(position, mount.support_offset());
        if !world
            .view()
            .block(support_position)
            .is_some_and(|support| support.def().solid)
        {
            return Err(WorldCommandError::Unsupported);
        }
    }
    Ok(world.commit_batch(vec![MutationProposal {
        preconditions: vec![BlockPrecondition {
            position,
            expected: None,
        }],
        writes: vec![BlockWrite {
            position,
            state: Some(state),
        }],
        priority: MutationPriority::PLAYER,
        source: position,
        sequence: 0,
    }]))
}

pub fn break_block(
    world: &mut WorldState,
    position: VoxelPos,
) -> Result<MutationBatchResult, WorldCommandError> {
    let Some(state) = world.view().block(position) else {
        return Err(WorldCommandError::Unavailable);
    };
    if !state.breakable() {
        return Err(WorldCommandError::Unbreakable);
    }
    let mut preconditions = vec![BlockPrecondition {
        position,
        expected: Some(state),
    }];
    let mut writes = vec![BlockWrite {
        position,
        state: None,
    }];
    for (offset, mount) in [
        ((0, 1), TorchMount::Floor),
        ((-1, 0), TorchMount::WallLeft),
        ((1, 0), TorchMount::WallRight),
    ] {
        let torch_position = offset_position(position, offset);
        let torch = BlockState::torch(mount);
        if world.view().block(torch_position) != Some(torch) {
            continue;
        }
        preconditions.push(BlockPrecondition {
            position: torch_position,
            expected: Some(torch),
        });
        writes.push(BlockWrite {
            position: torch_position,
            state: None,
        });
    }
    Ok(world.commit_batch(vec![MutationProposal {
        preconditions,
        writes,
        priority: MutationPriority::PLAYER,
        source: position,
        sequence: 0,
    }]))
}

fn offset_position(position: VoxelPos, offset: (i32, i32)) -> VoxelPos {
    VoxelPos {
        global_x: position.global_x.saturating_add(i64::from(offset.0)),
        y: position.y.saturating_add(offset.1),
        ..position
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{WorldId, blank_snapshot};
    use crate::domain::{BlockState, spawn_for_seed};

    fn world() -> WorldState {
        WorldState::from_snapshot(&blank_snapshot(
            11,
            "Test".into(),
            vec![],
            spawn_for_seed(11),
            1,
        ))
        .state
    }

    #[test]
    fn rebasing_preserves_global_dirty_chunk_identity() {
        let mut world = world();
        let position = VoxelPos::foreground(0, 70);
        place_block(&mut world, position, BlockState::DIRT).unwrap();
        world.rebase(8);
        assert_eq!(world.snapshot_chunks()[0].x, 0);
        assert_eq!(world.view().block(position), Some(BlockState::DIRT));
    }

    #[test]
    fn snapshot_converts_local_player_position_to_global_chunk_identity() {
        let mut world = world();
        world.rebase(8);
        let session = WorldSession {
            instance_id: crate::application::SessionInstanceId::new(1),
            id: WorldId::new("test").unwrap(),
            name: "Test".into(),
            seed: 11,
            generator_version: crate::domain::GENERATOR_VERSION,
            created_at_unix_s: 1,
            saved_version: crate::application::SaveVersion::default(),
        };
        let snapshot = world.snapshot(&session, glam::Vec2::new(1.5, 40.0), 3, 18_000, 9);
        assert_eq!(snapshot.player.chunk_x, 8);
        assert_eq!(snapshot.player.local_x, 1.5);
        assert_eq!(snapshot.player.selected_slot, 3);
        assert_eq!(snapshot.day_time_ticks, 18_000);
        assert_eq!(snapshot.last_played_unix_s, 9);
    }

    #[test]
    fn failed_command_does_not_increment_revision() {
        let mut world = world();
        let position = VoxelPos::foreground(0, 0);
        assert_eq!(
            break_block(&mut world, position),
            Err(WorldCommandError::Unbreakable)
        );
        assert_eq!(world.revision(), 0);
    }

    #[test]
    fn backwall_changes_survive_streaming_unload_and_reload() {
        let mut world = world();
        let position = (1..WORLD_HEIGHT)
            .rev()
            .map(|y| VoxelPos::backwall(0, y))
            .find(|position| world.view().cell(*position) == VoxelCell::Air)
            .unwrap();
        place_block(&mut world, position, BlockState::WOOD).unwrap();

        world.unload_chunk(0);
        let persisted = world.persisted_chunk(0).unwrap();
        assert_eq!(
            BlockState::from_code(
                persisted.blocks(VoxelLayer::Backwall)
                    [(position.y * crate::domain::CHUNK_WIDTH) as usize]
            ),
            Some(BlockState::WOOD)
        );

        world.integrate_chunk(persisted);
        assert_eq!(world.view().block(position), Some(BlockState::WOOD));
    }

    #[test]
    fn scheduling_requires_a_loaded_chunk_inside_the_world() {
        let mut world = world();
        assert_eq!(
            world.schedule_tick(
                VoxelPos::foreground(5_000, 40),
                MutationPriority::PLAYER,
                10,
                None,
            ),
            Err(ScheduleRejection::Unloaded)
        );
        assert_eq!(
            world.schedule_tick(
                VoxelPos::foreground(0, WORLD_HEIGHT),
                MutationPriority::PLAYER,
                10,
                None,
            ),
            Err(ScheduleRejection::OutOfBounds)
        );
        assert_eq!(world.queued_ticks(), 0);
    }

    #[test]
    fn scheduled_work_is_persisted_even_when_no_block_changed() {
        let mut world = world();
        let position = VoxelPos::foreground(3, 40);
        let scheduled = world
            .schedule_tick(
                position,
                MutationPriority::PLAYER,
                7,
                Some(BlockState::DIRT),
            )
            .unwrap();

        let chunks = world.snapshot_chunks();
        let chunk = chunks.iter().find(|chunk| chunk.x == 0).unwrap();
        assert_eq!(chunk.pending_ticks, vec![scheduled]);
        assert_eq!(scheduled.sequence, 0);
    }

    #[test]
    fn owed_work_survives_unload_and_reload() {
        let mut world = world();
        let position = VoxelPos::foreground(3, 40);
        let scheduled = world
            .schedule_tick(position, MutationPriority::PLAYER, 7, None)
            .unwrap();

        world.unload_chunk(0);
        let chunks = world.snapshot_chunks();
        assert_eq!(
            chunks
                .iter()
                .find(|chunk| chunk.x == 0)
                .map(|chunk| chunk.pending_ticks.as_slice()),
            Some([scheduled].as_slice())
        );

        world.integrate_chunk(world.persisted_chunk(0).unwrap());
        world.advance_world_tick();
        assert!(world.take_due_ticks(&[0]).is_empty());
        for _ in 1..7 {
            world.advance_world_tick();
        }
        assert_eq!(world.take_due_ticks(&[0]), vec![scheduled]);
        assert_eq!(world.queued_ticks(), 0);
    }

    #[test]
    fn scheduling_allocates_sequences_the_snapshot_validator_accepts() {
        let mut world = world();
        let position = VoxelPos::foreground(3, 40);
        world
            .schedule_tick(position, MutationPriority::PLAYER, 1, None)
            .unwrap();
        world
            .schedule_tick(position, MutationPriority::PLAYER, 2, None)
            .unwrap();

        let session = WorldSession {
            instance_id: crate::application::SessionInstanceId::new(1),
            id: WorldId::new("test").unwrap(),
            name: "Test".into(),
            seed: 11,
            generator_version: crate::domain::GENERATOR_VERSION,
            created_at_unix_s: 1,
            saved_version: crate::application::SaveVersion::default(),
        };
        let snapshot = world.snapshot(&session, glam::Vec2::new(1.5, 40.0), 1, 0, 9);

        assert_eq!(snapshot.next_tick_sequence, 2);
        crate::application::validate_snapshot(&snapshot).unwrap();
    }

    #[test]
    fn torch_mounts_require_their_designated_support() {
        let mut world = world();
        let support = VoxelPos::foreground(5, 70);
        place_block(&mut world, support, BlockState::DIRT).unwrap();

        for (position, mount) in [
            (VoxelPos::foreground(5, 71), TorchMount::Floor),
            (VoxelPos::foreground(4, 70), TorchMount::WallLeft),
            (VoxelPos::foreground(6, 70), TorchMount::WallRight),
        ] {
            place_block(&mut world, position, BlockState::torch(mount)).unwrap();
            assert_eq!(world.view().block(position), Some(BlockState::torch(mount)));
        }

        assert_eq!(
            place_block(
                &mut world,
                VoxelPos::foreground(7, 70),
                BlockState::WALL_TORCH_LEFT,
            ),
            Err(WorldCommandError::Unsupported)
        );
    }

    #[test]
    fn breaking_support_atomically_removes_all_attached_torches() {
        let mut world = world();
        let support = VoxelPos::foreground(5, 70);
        place_block(&mut world, support, BlockState::DIRT).unwrap();
        let torches = [
            (
                VoxelPos::foreground(5, 71),
                BlockState::torch(TorchMount::Floor),
            ),
            (
                VoxelPos::foreground(4, 70),
                BlockState::torch(TorchMount::WallLeft),
            ),
            (
                VoxelPos::foreground(6, 70),
                BlockState::torch(TorchMount::WallRight),
            ),
        ];
        for (position, torch) in torches {
            place_block(&mut world, position, torch).unwrap();
        }

        let result = break_block(&mut world, support).unwrap();

        assert_eq!(result.report.cell_changes.len(), 4);
        assert_eq!(world.view().block(support), None);
        for (position, _) in torches {
            assert_eq!(world.view().block(position), None);
        }
    }
}
