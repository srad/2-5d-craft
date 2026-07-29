use crate::application::{SaveVersion, WorldId, WorldSnapshot};

#[derive(Debug, Clone)]
pub struct WorldSession {
    pub id: WorldId,
    pub name: String,
    pub seed: u64,
    pub generator_version: u32,
    pub created_at_unix_s: u64,
    pub saved_version: SaveVersion,
}

#[derive(Debug, Clone)]
pub struct PendingWorld {
    pub id: WorldId,
    pub snapshot: WorldSnapshot,
}
