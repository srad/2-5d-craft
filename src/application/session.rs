use crate::application::{SaveVersion, WorldId, WorldSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct SessionInstanceId(u64);

impl SessionInstanceId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Default)]
pub struct SessionInstanceCounter(u64);

impl SessionInstanceCounter {
    pub fn next_id(&mut self) -> SessionInstanceId {
        self.0 = self
            .0
            .checked_add(1)
            .expect("world session instance counter exhausted");
        SessionInstanceId::new(self.0)
    }
}

#[derive(Debug, Clone)]
pub struct WorldSession {
    pub instance_id: SessionInstanceId,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_instance_ids_are_monotonic() {
        let mut counter = SessionInstanceCounter::default();
        assert_eq!(counter.next_id().get(), 1);
        assert_eq!(counter.next_id().get(), 2);
    }
}
