use crate::domain::{
    BlockState, CHUNK_WIDTH, GENERATOR_VERSION, WORLD_HEIGHT, chunk_is_representable,
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
        if previous.is_some_and(|value| value >= chunk.x) {
            return Err(SnapshotError(
                "chunks must be unique and sorted by x".into(),
            ));
        }
        previous = Some(chunk.x);
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
        player: PlayerSnapshot {
            chunk_x: (spawn.x.floor() as i64).div_euclid(i64::from(CHUNK_WIDTH)),
            local_x: spawn.x.rem_euclid(CHUNK_WIDTH as f32),
            y: spawn.y,
            selected_slot: 1,
        },
        chunks,
    }
}
