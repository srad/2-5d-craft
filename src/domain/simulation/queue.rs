use std::collections::{BTreeMap, BTreeSet};

use crate::domain::{BlockState, MutationPriority, ScheduledTick, VoxelPos, world_to_chunk};

/// Scheduled proposals one active chunk may process in one logical tick. Overflow stays queued in
/// stable order rather than extending the tick.
pub const MAX_SCHEDULED_PER_CHUNK_PER_TICK: usize = 4_096;

/// Simulation work owed by the world, grouped by global chunk.
///
/// Grouping is what makes activation cheap: an inactive chunk is frozen simply by never being
/// inspected, and its entry is exactly what persistence writes back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScheduledTickQueue {
    by_chunk: BTreeMap<i64, BTreeSet<ScheduledTick>>,
    next_sequence: u64,
}

impl ScheduledTickQueue {
    /// Rebuilds the queue from persisted per-chunk work.
    ///
    /// Callers supply data that already passed snapshot validation, which rejects duplicate keys,
    /// unsorted runs, foreign chunks, and sequences at or beyond `next_sequence`.
    pub fn from_chunks(
        chunks: impl IntoIterator<Item = (i64, Vec<ScheduledTick>)>,
        next_sequence: u64,
    ) -> Self {
        let mut queue = Self {
            by_chunk: BTreeMap::new(),
            next_sequence,
        };
        for (chunk_x, ticks) in chunks {
            for tick in ticks {
                debug_assert_eq!(
                    world_to_chunk(tick.position.global_x),
                    chunk_x,
                    "validated snapshots only group a tick under its own chunk"
                );
                debug_assert!(
                    tick.sequence < next_sequence,
                    "validated snapshots only carry allocated sequences"
                );
                queue.insert(chunk_x, tick);
            }
        }
        queue
    }

    /// Queues work for a later tick and allocates its stable sequence.
    ///
    /// The caller is responsible for the chunk being loaded; see `WorldState::schedule_tick`.
    pub fn schedule(
        &mut self,
        position: VoxelPos,
        priority: MutationPriority,
        due_world_tick: u64,
        expected: Option<BlockState>,
    ) -> ScheduledTick {
        let tick = ScheduledTick {
            due_world_tick,
            priority,
            position,
            sequence: self.next_sequence,
            expected,
        };
        self.next_sequence += 1;
        self.insert(world_to_chunk(position.global_x), tick);
        tick
    }

    /// Removes the work due at `world_tick` for the given active chunks.
    ///
    /// `active` must be sorted and deduplicated. Chunks outside it are never inspected, which is
    /// how inactive chunks freeze. Each chunk yields at most
    /// [`MAX_SCHEDULED_PER_CHUNK_PER_TICK`] ticks; the rest stay queued in stable order.
    pub fn take_due(&mut self, world_tick: u64, active: &[i64]) -> Vec<ScheduledTick> {
        debug_assert!(
            active.windows(2).all(|pair| pair[0] < pair[1]),
            "active chunks must be sorted and deduplicated for deterministic drain order"
        );
        let mut due = Vec::new();
        for chunk_x in active {
            let Some(ticks) = self.by_chunk.get_mut(chunk_x) else {
                continue;
            };
            for _ in 0..MAX_SCHEDULED_PER_CHUNK_PER_TICK {
                let Some(tick) = ticks.first().copied() else {
                    break;
                };
                if tick.due_world_tick > world_tick {
                    break;
                }
                ticks.remove(&tick);
                due.push(tick);
            }
            if ticks.is_empty() {
                self.by_chunk.remove(chunk_x);
            }
        }
        due
    }

    /// This chunk's queued work in stable order, as persistence writes it.
    pub fn pending_for_chunk(&self, chunk_x: i64) -> impl Iterator<Item = ScheduledTick> + '_ {
        self.by_chunk
            .get(&chunk_x)
            .into_iter()
            .flat_map(|ticks| ticks.iter().copied())
    }

    /// Whether this chunk must stay persisted even if none of its blocks were edited.
    pub fn owes_work(&self, chunk_x: i64) -> bool {
        self.by_chunk.contains_key(&chunk_x)
    }

    pub fn chunks_owing_work(&self) -> impl Iterator<Item = i64> + '_ {
        self.by_chunk.keys().copied()
    }

    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    pub fn queued_len(&self) -> usize {
        self.by_chunk.values().map(BTreeSet::len).sum()
    }

    fn insert(&mut self, chunk_x: i64, tick: ScheduledTick) {
        let ticks = self.by_chunk.entry(chunk_x).or_default();
        debug_assert!(
            !ticks.iter().any(|queued| queued.key() == tick.key()),
            "scheduled tick keys are unique: runtime sequences are allocated and loaded ones validated"
        );
        ticks.insert(tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue() -> ScheduledTickQueue {
        ScheduledTickQueue::default()
    }

    fn schedule_at(queue: &mut ScheduledTickQueue, global_x: i64, due: u64) -> ScheduledTick {
        queue.schedule(
            VoxelPos::foreground(global_x, 40),
            MutationPriority::PLAYER,
            due,
            None,
        )
    }

    #[test]
    fn drain_order_is_stable_regardless_of_insertion_order() {
        let drain = |order: [(i64, u64); 4]| {
            let mut queue = queue();
            for (global_x, due) in order {
                schedule_at(&mut queue, global_x, due);
            }
            queue.take_due(10, &[0, 1])
        };
        let forward = drain([(40, 9), (1, 10), (0, 9), (33, 10)]);
        let reverse = drain([(33, 10), (0, 9), (1, 10), (40, 9)]);

        assert_eq!(
            forward
                .iter()
                .map(|tick| (tick.position.global_x, tick.due_world_tick))
                .collect::<Vec<_>>(),
            vec![(0, 9), (1, 10), (40, 9), (33, 10)]
        );
        assert_eq!(
            forward
                .iter()
                .map(|tick| (tick.position, tick.due_world_tick))
                .collect::<Vec<_>>(),
            reverse
                .iter()
                .map(|tick| (tick.position, tick.due_world_tick))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn inactive_chunks_freeze_and_stay_persisted() {
        let mut queue = queue();
        schedule_at(&mut queue, 0, 1);
        let frozen = schedule_at(&mut queue, 200, 1);

        assert_eq!(queue.take_due(5, &[0]).len(), 1);
        assert!(queue.owes_work(world_to_chunk(200)));
        assert_eq!(
            queue
                .pending_for_chunk(world_to_chunk(200))
                .collect::<Vec<_>>(),
            vec![frozen]
        );
        assert_eq!(queue.queued_len(), 1);
    }

    #[test]
    fn future_work_is_not_drained_early() {
        let mut queue = queue();
        schedule_at(&mut queue, 0, 7);
        assert!(queue.take_due(6, &[0]).is_empty());
        assert_eq!(queue.take_due(7, &[0]).len(), 1);
        assert!(!queue.owes_work(0));
    }

    #[test]
    fn a_chunk_never_exceeds_its_per_tick_budget() {
        let mut queue = queue();
        let scheduled = MAX_SCHEDULED_PER_CHUNK_PER_TICK + 3;
        for _ in 0..scheduled {
            schedule_at(&mut queue, 0, 1);
        }

        let first = queue.take_due(1, &[0]);
        assert_eq!(first.len(), MAX_SCHEDULED_PER_CHUNK_PER_TICK);
        assert_eq!(queue.queued_len(), 3);
        // Overflow keeps its stable order and resumes exactly where the budget stopped.
        let second = queue.take_due(1, &[0]);
        assert_eq!(
            second.iter().map(|tick| tick.sequence).collect::<Vec<_>>(),
            (MAX_SCHEDULED_PER_CHUNK_PER_TICK as u64..scheduled as u64).collect::<Vec<_>>()
        );
    }

    #[test]
    fn sequences_are_allocated_monotonically_for_snapshot_validation() {
        let mut queue = ScheduledTickQueue::from_chunks([], 4);
        assert_eq!(schedule_at(&mut queue, 0, 1).sequence, 4);
        assert_eq!(schedule_at(&mut queue, 0, 1).sequence, 5);
        assert_eq!(queue.next_sequence(), 6);
        assert!(
            queue
                .pending_for_chunk(0)
                .all(|tick| tick.sequence < queue.next_sequence())
        );
    }

    #[test]
    fn reloaded_work_round_trips_through_chunk_grouping() {
        let mut original = queue();
        schedule_at(&mut original, 0, 3);
        schedule_at(&mut original, 200, 4);
        let chunks = original
            .chunks_owing_work()
            .map(|chunk_x| (chunk_x, original.pending_for_chunk(chunk_x).collect()))
            .collect::<Vec<(i64, Vec<_>)>>();

        let reloaded = ScheduledTickQueue::from_chunks(chunks, original.next_sequence());

        assert_eq!(reloaded, original);
    }
}
