use crate::application::WorldSnapshot;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct WorldId(String);

impl WorldId {
    pub fn new(value: impl Into<String>) -> Result<Self, RepositoryError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(RepositoryError::InvalidId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldSummary {
    pub id: WorldId,
    pub name: String,
    pub last_played_unix_s: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidWorldEntry {
    pub display_name: String,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorldCatalog {
    pub valid: Vec<WorldSummary>,
    pub invalid: Vec<InvalidWorldEntry>,
}

#[derive(Debug)]
pub enum RepositoryError {
    Unavailable(String),
    UnsupportedSchema(u32),
    Corrupt(String),
    InvalidId(String),
    InvalidSnapshot(crate::application::SnapshotError),
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(error) => write!(formatter, "storage unavailable: {error}"),
            Self::UnsupportedSchema(version) => {
                write!(formatter, "unsupported schema version {version}")
            }
            Self::Corrupt(error) => write!(formatter, "invalid .scw package: {error}"),
            Self::InvalidId(id) => write!(formatter, "invalid world identifier {id:?}"),
            Self::InvalidSnapshot(error) => write!(formatter, "invalid world: {error}"),
        }
    }
}

impl std::error::Error for RepositoryError {}

pub trait WorldRepository: Send + Sync {
    fn next_available_id(&self, seed: u64) -> Result<WorldId, RepositoryError>;
    fn list(&self) -> Result<WorldCatalog, RepositoryError>;
    fn load(&self, id: &WorldId) -> Result<WorldSnapshot, RepositoryError>;
    fn save(&self, id: &WorldId, snapshot: &WorldSnapshot) -> Result<(), RepositoryError>;
}
