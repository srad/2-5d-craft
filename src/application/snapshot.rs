use crate::domain::{
    BlockState, CHUNK_WIDTH, GENERATOR_VERSION, ScheduledTick, WORLD_HEIGHT,
    chunk_is_representable, world_to_chunk,
};
use glam::Vec2;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct WorldSnapshot {
    pub generator_version: u32,
    pub height: i32,
    pub name: String,
    pub seed: u64,
    pub created_at_unix_s: u64,
    pub last_played_unix_s: u64,
    pub day_time_ticks: u64,
    pub world_tick: u64,
    pub next_tick_sequence: u64,
    pub player: PlayerSnapshot,
    pub chunks: Vec<ChunkSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerSnapshot {
    pub chunk_x: i64,
    pub local_x: f32,
    pub y: f32,
    pub selected_slot: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkSnapshot {
    pub x: i64,
    pub foreground: Vec<u8>,
    pub backwall: Vec<u8>,
    /// Simulation work owed by this chunk, strictly ordered by [`ScheduledTick::key`].
    pub pending_ticks: Vec<ScheduledTick>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotError(pub String);

impl fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SnapshotError {}

pub fn validate_snapshot(snapshot: &WorldSnapshot) -> Result<(), SnapshotError> {
    if snapshot.generator_version != GENERATOR_VERSION {
        return Err(SnapshotError(format!(
            "unsupported generator version {}",
            snapshot.generator_version
        )));
    }
    if snapshot.height != WORLD_HEIGHT {
        return Err(SnapshotError(format!(
            "expected chunk height {WORLD_HEIGHT}"
        )));
    }
    if snapshot.name.trim().is_empty() || snapshot.name.len() > 80 {
        return Err(SnapshotError(
            "world name must contain 1 to 80 characters".into(),
        ));
    }
    if !snapshot.player.local_x.is_finite()
        || !(0.0..CHUNK_WIDTH as f32).contains(&snapshot.player.local_x)
        || !snapshot.player.y.is_finite()
    {
        return Err(SnapshotError(
            "player position must be finite and local x must be inside its chunk".into(),
        ));
    }
    if !(1..=BlockState::HOTBAR.len() as u8).contains(&snapshot.player.selected_slot) {
        return Err(SnapshotError("selected hotbar slot is invalid".into()));
    }
    if !chunk_is_representable(snapshot.player.chunk_x) {
        return Err(SnapshotError(
            "player chunk cannot be represented as global block coordinates".into(),
        ));
    }
    let chunk_area = (CHUNK_WIDTH * WORLD_HEIGHT) as usize;
    let mut previous = None;
    for chunk in &snapshot.chunks {
        if !chunk_is_representable(chunk.x) {
            return Err(SnapshotError(format!(
                "chunk {} cannot be represented as global block coordinates",
                chunk.x
            )));
        }
        for (layer, blocks) in [
            ("foreground", &chunk.foreground),
            ("backwall", &chunk.backwall),
        ] {
            if blocks.len() != chunk_area {
                return Err(SnapshotError(format!(
                    "chunk {} {layer} has {} blocks; expected {chunk_area}",
                    chunk.x,
                    blocks.len()
                )));
            }
            if blocks
                .iter()
                .any(|code| *code != 0 && BlockState::from_code(*code).is_none())
            {
                return Err(SnapshotError(format!(
                    "chunk {} {layer} contains an invalid block code",
                    chunk.x
                )));
            }
        }
        validate_pending_ticks(chunk, snapshot.next_tick_sequence)?;
        if previous.is_some_and(|value| value >= chunk.x) {
            return Err(SnapshotError(
                "chunks must be unique and sorted by x".into(),
            ));
        }
        previous = Some(chunk.x);
    }
    Ok(())
}

fn validate_pending_ticks(
    chunk: &ChunkSnapshot,
    next_tick_sequence: u64,
) -> Result<(), SnapshotError> {
    let mut previous = None;
    for tick in &chunk.pending_ticks {
        if world_to_chunk(tick.position.global_x) != chunk.x {
            return Err(SnapshotError(format!(
                "chunk {} holds a pending tick for block {}",
                chunk.x, tick.position.global_x
            )));
        }
        if !(0..WORLD_HEIGHT).contains(&tick.position.y) {
            return Err(SnapshotError(format!(
                "chunk {} holds a pending tick outside the world height",
                chunk.x
            )));
        }
        if tick.sequence >= next_tick_sequence {
            return Err(SnapshotError(format!(
                "chunk {} holds pending tick sequence {}; the next sequence is {next_tick_sequence}",
                chunk.x, tick.sequence
            )));
        }
        if previous.is_some_and(|value| value >= tick.key()) {
            return Err(SnapshotError(format!(
                "chunk {} pending ticks must be unique and sorted",
                chunk.x
            )));
        }
        previous = Some(tick.key());
    }
    Ok(())
}

pub fn blank_snapshot(
    seed: u64,
    name: String,
    chunks: Vec<ChunkSnapshot>,
    spawn: Vec2,
    now_unix_s: u64,
) -> WorldSnapshot {
    WorldSnapshot {
        generator_version: GENERATOR_VERSION,
        height: WORLD_HEIGHT,
        name,
        seed,
        created_at_unix_s: now_unix_s,
        last_played_unix_s: now_unix_s,
        day_time_ticks: crate::domain::SUNRISE_TICKS,
        world_tick: 0,
        next_tick_sequence: 0,
        player: PlayerSnapshot {
            chunk_x: (spawn.x.floor() as i64).div_euclid(i64::from(CHUNK_WIDTH)),
            local_x: spawn.x.rem_euclid(CHUNK_WIDTH as f32),
            y: spawn.y,
            selected_slot: 1,
        },
        chunks,
    }
}
