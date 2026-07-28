use crate::domain::{BlockState, CHUNK_WIDTH};
use glam::Vec2;
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum VoxelLayer {
    Foreground,
    Backwall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct VoxelPos {
    pub global_x: i64,
    pub y: i32,
    pub layer: VoxelLayer,
}

impl VoxelPos {
    pub const fn foreground(global_x: i64, y: i32) -> Self {
        Self {
            global_x,
            y,
            layer: VoxelLayer::Foreground,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoxelCell {
    LayerUnavailable,
    OutOfBounds,
    Unloaded,
    Air,
    Block(BlockState),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct ChunkLayer {
    pub chunk_x: i64,
    pub layer: VoxelLayer,
}

impl ChunkLayer {
    pub const fn foreground(chunk_x: i64) -> Self {
        Self {
            chunk_x,
            layer: VoxelLayer::Foreground,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockChunk {
    x: i64,
    blocks: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockGrid {
    height: i32,
    chunks: HashMap<i64, BlockChunk>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct MutationPriority(pub u16);

impl MutationPriority {
    pub const PLAYER: Self = Self(0);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockPrecondition {
    pub position: VoxelPos,
    pub expected: Option<BlockState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockWrite {
    pub position: VoxelPos,
    pub state: Option<BlockState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationProposal {
    pub preconditions: Vec<BlockPrecondition>,
    pub writes: Vec<BlockWrite>,
    pub priority: MutationPriority,
    pub source: VoxelPos,
    pub sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellChange {
    pub position: VoxelPos,
    pub before: Option<BlockState>,
    pub after: Option<BlockState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkChange {
    Integrated(ChunkLayer),
    Unloaded(ChunkLayer),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MutationReport {
    pub cell_changes: Vec<CellChange>,
    pub chunk_changes: Vec<ChunkChange>,
}

impl MutationReport {
    pub fn is_empty(&self) -> bool {
        self.cell_changes.is_empty() && self.chunk_changes.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationRejection {
    EmptyWrites,
    DuplicatePrecondition(VoxelPos),
    DuplicateWrite(VoxelPos),
    DuplicateOrderingKey,
    Unavailable(VoxelPos),
    PreconditionFailed {
        position: VoxelPos,
        expected: Option<BlockState>,
        actual: Option<BlockState>,
    },
    WriteConflict(VoxelPos),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalOutcome {
    Applied {
        sequence: u64,
    },
    Rejected {
        sequence: u64,
        reason: MutationRejection,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MutationBatchResult {
    pub outcomes: Vec<ProposalOutcome>,
    pub report: MutationReport,
}

pub struct WorldView<'a> {
    grid: &'a BlockGrid,
}

pub struct WorldMutator<'a> {
    grid: &'a mut BlockGrid,
}

impl BlockChunk {
    pub fn from_dense(chunk_x: i64, blocks: Vec<u8>) -> Option<Self> {
        let expected = (CHUNK_WIDTH * crate::domain::WORLD_HEIGHT) as usize;
        if !chunk_is_representable(chunk_x)
            || blocks.len() != expected
            || blocks
                .iter()
                .any(|code| *code != 0 && BlockState::from_code(*code).is_none())
        {
            return None;
        }
        Some(Self { x: chunk_x, blocks })
    }

    pub fn x(&self) -> i64 {
        self.x
    }

    pub fn blocks(&self) -> &[u8] {
        &self.blocks
    }

    fn get(&self, local_x: i32, y: i32) -> Option<BlockState> {
        BlockState::from_code(self.blocks[chunk_index(local_x, y)])
    }

    fn set(&mut self, local_x: i32, y: i32, state: Option<BlockState>) {
        self.blocks[chunk_index(local_x, y)] = state.map_or(0, BlockState::code);
    }
}

impl BlockGrid {
    pub fn new(height: i32) -> Self {
        Self {
            height,
            chunks: HashMap::new(),
        }
    }

    pub fn view(&self) -> WorldView<'_> {
        WorldView { grid: self }
    }

    fn insert_chunk(&mut self, chunk: BlockChunk) -> Option<BlockChunk> {
        self.chunks.insert(chunk.x, chunk)
    }

    fn remove_chunk(&mut self, chunk_x: i64) -> Option<BlockChunk> {
        self.chunks.remove(&chunk_x)
    }

    fn set(&mut self, position: VoxelPos, state: Option<BlockState>) {
        let chunk_x = world_to_chunk(position.global_x);
        let local_x = position.global_x.rem_euclid(i64::from(CHUNK_WIDTH)) as i32;
        self.chunks
            .get_mut(&chunk_x)
            .expect("mutation targets are validated before commit")
            .set(local_x, position.y, state);
    }

    pub(crate) fn chunk_snapshot(&self, chunk_x: i64) -> Option<Vec<u8>> {
        self.chunks.get(&chunk_x).map(|chunk| chunk.blocks.clone())
    }
}

impl<'a> WorldView<'a> {
    pub fn height(&self) -> i32 {
        self.grid.height
    }

    pub fn cell(&self, position: VoxelPos) -> VoxelCell {
        if position.layer != VoxelLayer::Foreground {
            return VoxelCell::LayerUnavailable;
        }
        if !(0..self.grid.height).contains(&position.y) {
            return VoxelCell::OutOfBounds;
        }
        let chunk_x = world_to_chunk(position.global_x);
        let Some(chunk) = self.grid.chunks.get(&chunk_x) else {
            return VoxelCell::Unloaded;
        };
        let local_x = position.global_x.rem_euclid(i64::from(CHUNK_WIDTH)) as i32;
        match chunk.get(local_x, position.y) {
            Some(state) => VoxelCell::Block(state),
            None => VoxelCell::Air,
        }
    }

    pub fn block(&self, position: VoxelPos) -> Option<BlockState> {
        match self.cell(position) {
            VoxelCell::Block(state) => Some(state),
            _ => None,
        }
    }

    pub fn contains_chunk(&self, chunk: ChunkLayer) -> bool {
        chunk.layer == VoxelLayer::Foreground && self.grid.chunks.contains_key(&chunk.chunk_x)
    }

    pub fn loaded_chunks(&self) -> impl Iterator<Item = i64> + '_ {
        self.grid.chunks.keys().copied()
    }

    pub fn loaded_x_bounds(&self) -> Option<(i64, i64)> {
        let minimum = self.grid.chunks.keys().copied().min()? * i64::from(CHUNK_WIDTH);
        let maximum = (self.grid.chunks.keys().copied().max()? + 1) * i64::from(CHUNK_WIDTH);
        Some((minimum, maximum))
    }

    pub fn iter(&self) -> impl Iterator<Item = (VoxelPos, BlockState)> + '_ {
        self.grid.chunks.values().flat_map(|chunk| {
            chunk
                .blocks
                .iter()
                .enumerate()
                .filter_map(move |(index, code)| {
                    let state = BlockState::from_code(*code)?;
                    let local_x = index as i32 % CHUNK_WIDTH;
                    let y = index as i32 / CHUNK_WIDTH;
                    Some((
                        VoxelPos::foreground(
                            chunk.x * i64::from(CHUNK_WIDTH) + i64::from(local_x),
                            y,
                        ),
                        state,
                    ))
                })
        })
    }

    pub fn safe_spawn(&self, origin_chunk: i64) -> Vec2 {
        let origin_x = origin_chunk * i64::from(CHUNK_WIDTH);
        for distance in 0..i64::from(CHUNK_WIDTH) {
            for x in [origin_x + distance, origin_x - distance] {
                for y in (0..self.grid.height - 2).rev() {
                    if self.block(VoxelPos::foreground(x, y)) == Some(BlockState::GRASS)
                        && self.block(VoxelPos::foreground(x, y + 1)).is_none()
                        && self.block(VoxelPos::foreground(x, y + 2)).is_none()
                    {
                        return Vec2::new((x - origin_x) as f32 + 0.5, y as f32 + 1.9);
                    }
                }
            }
        }
        Vec2::new(0.5, self.grid.height as f32 - 2.0)
    }

    pub fn player_position_is_safe(&self, position: Vec2, origin_chunk: i64) -> bool {
        if !position.is_finite() || position.y < 0.9 || position.y > self.grid.height as f32 {
            return false;
        }
        let min_tile = (position - Vec2::new(0.34, 0.88)).floor().as_ivec2();
        let max_tile = (position + Vec2::new(0.34, 0.88)).floor().as_ivec2();
        let origin_x = origin_chunk * i64::from(CHUNK_WIDTH);
        for local_x in min_tile.x..=max_tile.x {
            for y in min_tile.y..=max_tile.y {
                match self.cell(VoxelPos::foreground(origin_x + i64::from(local_x), y)) {
                    VoxelCell::Air => {}
                    VoxelCell::Block(state) if !state.def().solid => {}
                    _ => return false,
                }
            }
        }
        true
    }
}

impl<'a> WorldMutator<'a> {
    pub fn new(grid: &'a mut BlockGrid) -> Self {
        Self { grid }
    }

    pub fn commit_batch(&mut self, mut proposals: Vec<MutationProposal>) -> MutationBatchResult {
        let duplicate_keys = duplicate_ordering_keys(&proposals);
        proposals.sort_by_key(|proposal| (proposal.priority, proposal.source, proposal.sequence));

        let mut result = MutationBatchResult::default();
        let mut overlay = BTreeMap::<VoxelPos, Option<BlockState>>::new();
        let mut written = BTreeSet::<VoxelPos>::new();

        for proposal in proposals {
            let reject = |reason| ProposalOutcome::Rejected {
                sequence: proposal.sequence,
                reason,
            };
            if duplicate_keys.contains(&(proposal.priority, proposal.source, proposal.sequence)) {
                result
                    .outcomes
                    .push(reject(MutationRejection::DuplicateOrderingKey));
                continue;
            }
            if proposal.writes.is_empty() {
                result.outcomes.push(reject(MutationRejection::EmptyWrites));
                continue;
            }
            if let Some(position) = first_duplicate(
                proposal
                    .preconditions
                    .iter()
                    .map(|precondition| precondition.position),
            ) {
                result
                    .outcomes
                    .push(reject(MutationRejection::DuplicatePrecondition(position)));
                continue;
            }
            if let Some(position) =
                first_duplicate(proposal.writes.iter().map(|write| write.position))
            {
                result
                    .outcomes
                    .push(reject(MutationRejection::DuplicateWrite(position)));
                continue;
            }
            let mut unavailable = None;
            for position in std::iter::once(proposal.source)
                .chain(
                    proposal
                        .preconditions
                        .iter()
                        .map(|precondition| precondition.position),
                )
                .chain(proposal.writes.iter().map(|write| write.position))
            {
                if cell_state(self.grid.view().cell(position)).is_none() {
                    unavailable = Some(position);
                    break;
                }
            }
            if let Some(position) = unavailable {
                result
                    .outcomes
                    .push(reject(MutationRejection::Unavailable(position)));
                continue;
            }
            let mut failed = None;
            for precondition in &proposal.preconditions {
                let actual = overlay
                    .get(&precondition.position)
                    .copied()
                    .unwrap_or_else(|| {
                        cell_state(self.grid.view().cell(precondition.position))
                            .expect("availability was validated")
                    });
                if actual != precondition.expected {
                    failed = Some(MutationRejection::PreconditionFailed {
                        position: precondition.position,
                        expected: precondition.expected,
                        actual,
                    });
                    break;
                }
            }
            if let Some(reason) = failed {
                result.outcomes.push(reject(reason));
                continue;
            }
            if let Some(position) = proposal
                .writes
                .iter()
                .map(|write| write.position)
                .find(|position| written.contains(position))
            {
                result
                    .outcomes
                    .push(reject(MutationRejection::WriteConflict(position)));
                continue;
            }

            for write in &proposal.writes {
                let before = overlay.get(&write.position).copied().unwrap_or_else(|| {
                    cell_state(self.grid.view().cell(write.position))
                        .expect("availability was validated")
                });
                written.insert(write.position);
                overlay.insert(write.position, write.state);
                if before != write.state {
                    result.report.cell_changes.push(CellChange {
                        position: write.position,
                        before,
                        after: write.state,
                    });
                }
            }
            result.outcomes.push(ProposalOutcome::Applied {
                sequence: proposal.sequence,
            });
        }

        for change in &result.report.cell_changes {
            self.grid.set(change.position, change.after);
        }
        result
    }

    pub fn integrate_chunk(&mut self, chunk: BlockChunk) -> MutationReport {
        let chunk_x = chunk.x;
        self.grid.insert_chunk(chunk);
        MutationReport {
            chunk_changes: vec![ChunkChange::Integrated(ChunkLayer::foreground(chunk_x))],
            ..Default::default()
        }
    }

    pub fn unload_chunk(&mut self, chunk_x: i64) -> (Option<BlockChunk>, MutationReport) {
        let removed = self.grid.remove_chunk(chunk_x);
        let report = if removed.is_some() {
            MutationReport {
                chunk_changes: vec![ChunkChange::Unloaded(ChunkLayer::foreground(chunk_x))],
                ..Default::default()
            }
        } else {
            MutationReport::default()
        };
        (removed, report)
    }
}

pub const fn world_to_chunk(world_x: i64) -> i64 {
    world_x.div_euclid(CHUNK_WIDTH as i64)
}

pub const fn chunk_is_representable(chunk_x: i64) -> bool {
    let width = CHUNK_WIDTH as i64;
    let Some(start) = chunk_x.checked_mul(width) else {
        return false;
    };
    start.checked_add(width).is_some()
}

fn chunk_index(local_x: i32, y: i32) -> usize {
    (y * CHUNK_WIDTH + local_x) as usize
}

fn cell_state(cell: VoxelCell) -> Option<Option<BlockState>> {
    match cell {
        VoxelCell::Air => Some(None),
        VoxelCell::Block(state) => Some(Some(state)),
        VoxelCell::LayerUnavailable | VoxelCell::OutOfBounds | VoxelCell::Unloaded => None,
    }
}

fn first_duplicate(values: impl IntoIterator<Item = VoxelPos>) -> Option<VoxelPos> {
    let mut seen = BTreeSet::new();
    values.into_iter().find(|value| !seen.insert(*value))
}

fn duplicate_ordering_keys(
    proposals: &[MutationProposal],
) -> BTreeSet<(MutationPriority, VoxelPos, u64)> {
    let mut counts = BTreeMap::new();
    for proposal in proposals {
        *counts
            .entry((proposal.priority, proposal.source, proposal.sequence))
            .or_insert(0_usize) += 1;
    }
    counts
        .into_iter()
        .filter_map(|(key, count)| (count > 1).then_some(key))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{WORLD_HEIGHT, generate_chunk};

    fn loaded_grid() -> BlockGrid {
        let mut grid = BlockGrid::new(WORLD_HEIGHT);
        WorldMutator::new(&mut grid).integrate_chunk(generate_chunk(1, -1));
        grid
    }

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
    fn chunk_representation_requires_both_bounds_to_fit() {
        let width = i64::from(CHUNK_WIDTH);
        let minimum = i64::MIN.div_euclid(width);
        let maximum = i64::MAX.div_euclid(width);

        assert!(chunk_is_representable(minimum));
        assert!(!chunk_is_representable(minimum - 1));
        assert!(chunk_is_representable(maximum - 1));
        assert!(!chunk_is_representable(maximum));
    }

    #[test]
    fn view_distinguishes_air_unloaded_bounds_and_layers() {
        let grid = loaded_grid();
        let view = grid.view();
        assert_eq!(view.cell(VoxelPos::foreground(-1, 70)), VoxelCell::Air);
        assert_eq!(
            view.cell(VoxelPos::foreground(100, 70)),
            VoxelCell::Unloaded
        );
        assert_eq!(
            view.cell(VoxelPos::foreground(-1, WORLD_HEIGHT)),
            VoxelCell::OutOfBounds
        );
        assert_eq!(
            view.cell(VoxelPos {
                global_x: -1,
                y: 70,
                layer: VoxelLayer::Backwall,
            }),
            VoxelCell::LayerUnavailable
        );
    }

    #[test]
    fn atomic_proposal_rolls_back_when_one_precondition_fails() {
        let mut grid = loaded_grid();
        let source = VoxelPos::foreground(-1, 70);
        let occupied = VoxelPos::foreground(-1, 0);
        let proposal = MutationProposal {
            preconditions: vec![
                BlockPrecondition {
                    position: source,
                    expected: None,
                },
                BlockPrecondition {
                    position: occupied,
                    expected: None,
                },
            ],
            writes: vec![BlockWrite {
                position: source,
                state: Some(BlockState::TORCH),
            }],
            priority: MutationPriority::PLAYER,
            source,
            sequence: 1,
        };
        let result = WorldMutator::new(&mut grid).commit_batch(vec![proposal]);
        assert!(result.report.is_empty());
        assert_eq!(grid.view().block(source), None);
        assert!(matches!(
            result.outcomes[0],
            ProposalOutcome::Rejected {
                reason: MutationRejection::PreconditionFailed { .. },
                ..
            }
        ));
    }

    #[test]
    fn conflicts_resolve_independently_of_input_order() {
        let source = VoxelPos::foreground(-1, 70);
        let proposal = |sequence, state| MutationProposal {
            preconditions: vec![BlockPrecondition {
                position: source,
                expected: None,
            }],
            writes: vec![BlockWrite {
                position: source,
                state: Some(state),
            }],
            priority: MutationPriority::PLAYER,
            source,
            sequence,
        };
        let run = |proposals| {
            let mut grid = loaded_grid();
            let result = WorldMutator::new(&mut grid).commit_batch(proposals);
            (grid.view().block(source), result.outcomes)
        };
        let forward = run(vec![
            proposal(2, BlockState::DIRT),
            proposal(1, BlockState::STONE),
        ]);
        let reverse = run(vec![
            proposal(1, BlockState::STONE),
            proposal(2, BlockState::DIRT),
        ]);
        assert_eq!(forward, reverse);
        assert_eq!(forward.0, Some(BlockState::STONE));
    }
}
