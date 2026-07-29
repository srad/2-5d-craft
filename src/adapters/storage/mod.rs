use crate::application::{
    ChunkSnapshot, InvalidWorldEntry, PlayerSnapshot, RepositoryError, SnapshotError, WorldCatalog,
    WorldId, WorldRepository, WorldSnapshot, WorldSummary, validate_snapshot,
};
use crate::domain::{BlockId, BlockState, CHUNK_WIDTH, WORLD_HEIGHT};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::io::{self, BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

const SCW_MAGIC: &[u8; 4] = b"SCW1";
const ZSTD_LEVEL: i32 = 3;
const MAX_FILE_BYTES: usize = 512 * 1024 * 1024;
const MAX_DECOMPRESSED_BYTES: usize = 512 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 4 * 1024 * 1024;
const CHUNK_AREA: usize = (CHUNK_WIDTH * WORLD_HEIGHT) as usize;
const MAX_SAVED_CHUNKS_PER_FILE: usize = 65_536;
const REGION_CHUNKS: i64 = 64;
const MANIFEST_FILE: &str = "manifest.scw";
const SAVE_SCHEMA_VERSION: u32 = 4;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ScwMetadataV1 {
    schema_version: u32,
    generator_version: u32,
    height: i32,
    name: String,
    seed: u64,
    created_at_unix_s: u64,
    last_played_unix_s: u64,
    day_time_ticks: u64,
    player_chunk_x: i64,
    player_local_x: f32,
    player_y: f32,
    selected_slot: u8,
}

#[derive(Debug, Serialize, Deserialize)]
struct ScwHeaderV1 {
    metadata: ScwMetadataV1,
    palette: Vec<ScwPaletteBlock>,
    chunk_xs: Vec<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct ScwPaletteBlock {
    id: u16,
    variant: u8,
}

impl ScwPaletteBlock {
    fn from_code(code: u8) -> Option<Self> {
        let state = BlockState::from_code(code)?;
        Some(Self {
            id: state.id().value(),
            variant: state.variant(),
        })
    }

    fn code(self) -> Option<u8> {
        BlockState::new(BlockId::new(self.id)?, self.variant).map(BlockState::code)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ScwPackageManifestV1 {
    metadata: ScwMetadataV1,
    generation: u64,
    regions: Vec<ScwRegionRefV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ScwRegionRefV1 {
    region_x: i64,
    content_hash: u64,
    file_name: String,
}

#[derive(Debug, Clone)]
pub struct ScwRepository {
    root: PathBuf,
}

#[derive(Debug)]
enum StoreError {
    Io(io::Error),
    Postcard(postcard::Error),
    Compression(io::Error),
    UnsupportedSchema(u32),
    Format(String),
    Validation(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Postcard(error) => write!(formatter, "invalid world metadata: {error}"),
            Self::Compression(error) => write!(formatter, "invalid compressed data: {error}"),
            Self::UnsupportedSchema(version) => {
                write!(formatter, "unsupported schema version {version}")
            }
            Self::Format(error) => write!(formatter, "invalid .scw file: {error}"),
            Self::Validation(error) => write!(formatter, "invalid world: {error}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl Default for ScwRepository {
    fn default() -> Self {
        Self::new("worlds")
    }
}

impl ScwRepository {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[cfg(test)]
    fn root(&self) -> &Path {
        &self.root
    }

    fn next_available_id(&self, seed: u64) -> Result<WorldId, StoreError> {
        fs::create_dir_all(&self.root)?;
        let base = format!("world-{seed:016x}");
        for suffix in 1_u32.. {
            let id = if suffix == 1 {
                base.clone()
            } else {
                format!("{base}-{suffix}")
            };
            let path = self.root.join(format!("{id}.scw"));
            if !path.exists() {
                return WorldId::new(id).map_err(|error| StoreError::Validation(error.to_string()));
            }
        }
        unreachable!("the numeric suffix space cannot be exhausted")
    }

    fn save(&self, id: &WorldId, save: &WorldSnapshot) -> Result<(), StoreError> {
        validate(save)?;
        fs::create_dir_all(&self.root)?;
        let path = self.path_for(id);
        if path.exists() && !path.is_dir() {
            return Err(StoreError::Validation(
                "world save path must be a package directory".into(),
            ));
        }
        save_package(&path, save)
    }

    fn load(&self, id: &WorldId) -> Result<WorldSnapshot, StoreError> {
        let path = self.path_for(id);
        if !path.is_dir() {
            return Err(StoreError::Validation(
                "world save path must be a package directory".into(),
            ));
        }
        load_package(&path)
    }

    fn list(&self) -> Result<WorldCatalog, StoreError> {
        fs::create_dir_all(&self.root)?;
        let mut list = WorldCatalog::default();
        for entry in fs::read_dir(&self.root)? {
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("scw") {
                continue;
            }
            let display_name = path
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
                list.invalid.push(InvalidWorldEntry {
                    display_name,
                    error: "world package name is not valid UTF-8".into(),
                });
                continue;
            };
            let id = match WorldId::new(stem) {
                Ok(id) => id,
                Err(error) => {
                    list.invalid.push(InvalidWorldEntry {
                        display_name,
                        error: error.to_string(),
                    });
                    continue;
                }
            };
            match self.load(&id) {
                Ok(snapshot) => list.valid.push(WorldSummary {
                    id,
                    name: snapshot.name,
                    last_played_unix_s: snapshot.last_played_unix_s,
                }),
                Err(error) => list.invalid.push(InvalidWorldEntry {
                    display_name,
                    error: error.to_string(),
                }),
            }
        }
        list.valid.sort_by(|a, b| {
            b.last_played_unix_s
                .cmp(&a.last_played_unix_s)
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.id.cmp(&b.id))
        });
        list.invalid
            .sort_by(|a, b| a.display_name.cmp(&b.display_name));
        Ok(list)
    }

    fn path_for(&self, id: &WorldId) -> PathBuf {
        self.root.join(format!("{}.scw", id.as_str()))
    }
}

impl WorldRepository for ScwRepository {
    fn next_available_id(&self, seed: u64) -> Result<WorldId, RepositoryError> {
        ScwRepository::next_available_id(self, seed).map_err(RepositoryError::from)
    }

    fn list(&self) -> Result<WorldCatalog, RepositoryError> {
        ScwRepository::list(self).map_err(RepositoryError::from)
    }

    fn load(&self, id: &WorldId) -> Result<WorldSnapshot, RepositoryError> {
        ScwRepository::load(self, id).map_err(RepositoryError::from)
    }

    fn save(&self, id: &WorldId, snapshot: &WorldSnapshot) -> Result<(), RepositoryError> {
        ScwRepository::save(self, id, snapshot).map_err(RepositoryError::from)
    }
}

impl From<StoreError> for RepositoryError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Io(error) => Self::Unavailable(error.to_string()),
            StoreError::Postcard(error) => Self::Corrupt(error.to_string()),
            StoreError::Compression(error) => Self::Corrupt(error.to_string()),
            StoreError::UnsupportedSchema(version) => Self::UnsupportedSchema(version),
            StoreError::Format(error) => Self::Corrupt(error),
            StoreError::Validation(error) => Self::InvalidSnapshot(SnapshotError(error)),
        }
    }
}

fn save_package(path: &Path, save: &WorldSnapshot) -> Result<(), StoreError> {
    fs::create_dir_all(path)?;
    let generation = rand::random::<u64>();
    let mut grouped = BTreeMap::<i64, Vec<ChunkSnapshot>>::new();
    for chunk in &save.chunks {
        grouped
            .entry(chunk.x.div_euclid(REGION_CHUNKS))
            .or_default()
            .push(chunk.clone());
    }

    let mut regions = Vec::with_capacity(grouped.len());
    for (region_x, chunks) in grouped {
        let content_hash = hash_region(&chunks);
        let file_name = format!("region-{region_x}-{content_hash:016x}.scw");
        let region_path = path.join(&file_name);
        if !region_path.exists() {
            let region_save = WorldSnapshot {
                last_played_unix_s: 0,
                day_time_ticks: 0,
                player: PlayerSnapshot {
                    chunk_x: 0,
                    local_x: 0.5,
                    y: 2.0,
                    selected_slot: 1,
                },
                chunks,
                ..save.clone()
            };
            atomic_write(path, &region_path, &encode_scw(&region_save)?)?;
        }
        regions.push(ScwRegionRefV1 {
            region_x,
            content_hash,
            file_name,
        });
    }

    let manifest = ScwPackageManifestV1 {
        metadata: metadata_from_save(save),
        generation,
        regions,
    };
    let manifest_bytes = encode_postcard_envelope(&manifest)?;
    atomic_write(path, &path.join(MANIFEST_FILE), &manifest_bytes)?;
    cleanup_obsolete_regions(path, &manifest)?;
    Ok(())
}

fn load_package(path: &Path) -> Result<WorldSnapshot, StoreError> {
    let manifest_bytes = fs::read(path.join(MANIFEST_FILE))?;
    let manifest: ScwPackageManifestV1 = decode_postcard_envelope(&manifest_bytes)?;
    validate_manifest(&manifest)?;
    let mut chunks = Vec::new();
    for region in &manifest.regions {
        let region_path = path.join(&region.file_name);
        let region_save = decode_scw(&fs::read(region_path)?)?;
        if !region_metadata_matches(&region_save, &manifest.metadata) {
            return Err(StoreError::Format(format!(
                "region {} metadata does not match the manifest",
                region.region_x
            )));
        }
        if hash_region(&region_save.chunks) != region.content_hash {
            return Err(StoreError::Format(format!(
                "region {} content hash does not match the manifest",
                region.region_x
            )));
        }
        if region_save
            .chunks
            .iter()
            .any(|chunk| chunk.x.div_euclid(REGION_CHUNKS) != region.region_x)
        {
            return Err(StoreError::Format(format!(
                "region {} contains an out-of-range chunk",
                region.region_x
            )));
        }
        chunks.extend(region_save.chunks);
    }
    chunks.sort_by_key(|chunk| chunk.x);
    let save = save_from_metadata(manifest.metadata, chunks)?;
    validate(&save)?;
    Ok(save)
}

fn validate_manifest(manifest: &ScwPackageManifestV1) -> Result<(), StoreError> {
    if manifest
        .regions
        .windows(2)
        .any(|pair| pair[0].region_x >= pair[1].region_x)
    {
        return Err(StoreError::Format(
            "manifest regions must be unique and sorted".into(),
        ));
    }
    for region in &manifest.regions {
        let expected = format!(
            "region-{}-{:016x}.scw",
            region.region_x, region.content_hash
        );
        if region.file_name != expected {
            return Err(StoreError::Format(
                "manifest contains an invalid region path".into(),
            ));
        }
    }
    Ok(())
}

fn hash_region(chunks: &[ChunkSnapshot]) -> u64 {
    hash_region_for_schema(chunks, SAVE_SCHEMA_VERSION)
}

fn hash_region_for_schema(chunks: &[ChunkSnapshot], schema_version: u32) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in schema_version.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    for chunk in chunks {
        for byte in chunk
            .x
            .to_le_bytes()
            .into_iter()
            .chain(chunk.blocks.iter().copied())
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

fn region_metadata_matches(region: &WorldSnapshot, metadata: &ScwMetadataV1) -> bool {
    region.generator_version == metadata.generator_version
        && region.height == metadata.height
        && region.seed == metadata.seed
        && region.created_at_unix_s == metadata.created_at_unix_s
}

fn cleanup_obsolete_regions(
    path: &Path,
    manifest: &ScwPackageManifestV1,
) -> Result<(), StoreError> {
    let active = manifest
        .regions
        .iter()
        .map(|region| region.file_name.as_str())
        .collect::<HashSet<_>>();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if file_name.starts_with("region-")
            && file_name.ends_with(".scw")
            && !active.contains(file_name)
        {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

fn atomic_write(parent: &Path, path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let mut temporary = NamedTempFile::new_in(parent)?;
    {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        writer.write_all(bytes)?;
        writer.flush()?;
    }
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| StoreError::Io(error.error))?;
    Ok(())
}

fn encode_postcard_envelope<T: Serialize>(value: &T) -> Result<Vec<u8>, StoreError> {
    let payload = postcard::to_allocvec(value).map_err(StoreError::Postcard)?;
    if payload.len() > MAX_HEADER_BYTES {
        return Err(StoreError::Format("metadata is too large".into()));
    }
    let compressed = zstd::stream::encode_all(Cursor::new(payload), ZSTD_LEVEL)
        .map_err(StoreError::Compression)?;
    let mut encoded = Vec::with_capacity(SCW_MAGIC.len() + compressed.len());
    encoded.extend_from_slice(SCW_MAGIC);
    encoded.extend_from_slice(&compressed);
    Ok(encoded)
}

fn decode_postcard_envelope<T>(file: &[u8]) -> Result<T, StoreError>
where
    T: for<'de> Deserialize<'de>,
{
    if file.len() > MAX_FILE_BYTES || file.get(..SCW_MAGIC.len()) != Some(SCW_MAGIC) {
        return Err(StoreError::Format(
            "unsupported or missing SCW1 header".into(),
        ));
    }
    let decoder = zstd::stream::read::Decoder::new(&file[SCW_MAGIC.len()..])
        .map_err(StoreError::Compression)?;
    let mut decoded = Vec::new();
    decoder
        .take((MAX_HEADER_BYTES + 1) as u64)
        .read_to_end(&mut decoded)
        .map_err(StoreError::Compression)?;
    if decoded.len() > MAX_HEADER_BYTES {
        return Err(StoreError::Format("metadata is too large".into()));
    }
    let (value, remaining) = postcard::take_from_bytes(&decoded).map_err(StoreError::Postcard)?;
    if !remaining.is_empty() {
        return Err(StoreError::Format("metadata contains trailing data".into()));
    }
    Ok(value)
}

fn encode_scw(save: &WorldSnapshot) -> Result<Vec<u8>, StoreError> {
    let (header, blocks) = parts_from_save(save)?;
    let encoded_header = postcard::to_allocvec(&header).map_err(StoreError::Postcard)?;
    if encoded_header.len() > MAX_HEADER_BYTES {
        return Err(StoreError::Format("metadata is too large".into()));
    }
    let header_length = u32::try_from(encoded_header.len())
        .map_err(|_| StoreError::Format("metadata length overflowed".into()))?;
    let mut payload = Vec::with_capacity(4 + encoded_header.len() + blocks.len());
    payload.extend_from_slice(&header_length.to_le_bytes());
    payload.extend_from_slice(&encoded_header);
    payload.extend_from_slice(&blocks);

    let compressed = zstd::stream::encode_all(Cursor::new(payload), ZSTD_LEVEL)
        .map_err(StoreError::Compression)?;
    if compressed.len() + SCW_MAGIC.len() > MAX_FILE_BYTES {
        return Err(StoreError::Format("compressed payload is too large".into()));
    }

    let mut file = Vec::with_capacity(SCW_MAGIC.len() + compressed.len());
    file.extend_from_slice(SCW_MAGIC);
    file.extend_from_slice(&compressed);
    Ok(file)
}

fn decode_scw(file: &[u8]) -> Result<WorldSnapshot, StoreError> {
    if file.len() > MAX_FILE_BYTES {
        return Err(StoreError::Format("file is too large".into()));
    }
    if file.get(..SCW_MAGIC.len()) != Some(SCW_MAGIC) {
        return Err(StoreError::Format(
            "unsupported or missing SCW1 header".into(),
        ));
    }

    let decoder = zstd::stream::read::Decoder::new(&file[SCW_MAGIC.len()..])
        .map_err(StoreError::Compression)?;
    let mut decoded = Vec::new();
    decoder
        .take((MAX_DECOMPRESSED_BYTES + 1) as u64)
        .read_to_end(&mut decoded)
        .map_err(StoreError::Compression)?;
    if decoded.len() > MAX_DECOMPRESSED_BYTES {
        return Err(StoreError::Format(
            "decompressed payload is too large".into(),
        ));
    }

    let Some(length_bytes) = decoded.get(..4) else {
        return Err(StoreError::Format("metadata length is missing".into()));
    };
    let header_length = u32::from_le_bytes(
        length_bytes
            .try_into()
            .expect("the metadata length slice has exactly four bytes"),
    ) as usize;
    if header_length > MAX_HEADER_BYTES {
        return Err(StoreError::Format("metadata is too large".into()));
    }
    let header_end = 4_usize
        .checked_add(header_length)
        .ok_or_else(|| StoreError::Format("metadata length overflowed".into()))?;
    let Some(encoded_header) = decoded.get(4..header_end) else {
        return Err(StoreError::Format("metadata is truncated".into()));
    };
    let (header, remaining): (ScwHeaderV1, &[u8]) =
        postcard::take_from_bytes(encoded_header).map_err(StoreError::Postcard)?;
    if !remaining.is_empty() {
        return Err(StoreError::Format("metadata contains trailing data".into()));
    }
    save_from_parts(header, decoded[header_end..].to_vec())
}

fn parts_from_save(save: &WorldSnapshot) -> Result<(ScwHeaderV1, Vec<u8>), StoreError> {
    validate(save)?;
    let palette = save
        .chunks
        .iter()
        .flat_map(|chunk| chunk.blocks.iter().copied())
        .filter_map(ScwPaletteBlock::from_code)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if palette.len() > u8::MAX as usize {
        return Err(StoreError::Format(
            "block palette contains more than 255 entries".into(),
        ));
    }

    let mut blocks = Vec::with_capacity(save.chunks.len() * CHUNK_AREA);
    for chunk in &save.chunks {
        for code in &chunk.blocks {
            let palette_code = match ScwPaletteBlock::from_code(*code) {
                None if *code == 0 => 0,
                Some(kind) => {
                    let palette_index = palette
                        .binary_search(&kind)
                        .expect("every chunk block kind was collected into the palette");
                    u8::try_from(palette_index + 1)
                        .map_err(|_| StoreError::Format("block palette index overflowed".into()))?
                }
                None => {
                    return Err(StoreError::Validation(format!(
                        "chunk {} contains invalid block code {code}",
                        chunk.x
                    )));
                }
            };
            blocks.push(palette_code);
        }
    }

    Ok((
        ScwHeaderV1 {
            metadata: metadata_from_save(save),
            palette,
            chunk_xs: save.chunks.iter().map(|chunk| chunk.x).collect(),
        },
        blocks,
    ))
}

fn save_from_parts(
    header: ScwHeaderV1,
    dense_blocks: Vec<u8>,
) -> Result<WorldSnapshot, StoreError> {
    if header.metadata.height != WORLD_HEIGHT {
        return Err(StoreError::Format(format!(
            "expected chunk height {WORLD_HEIGHT}"
        )));
    }
    if header.palette.len() > u8::MAX as usize
        || header.palette.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(StoreError::Format(
            "block palette must be unique and sorted".into(),
        ));
    }
    if header.chunk_xs.len() > MAX_SAVED_CHUNKS_PER_FILE
        || header.chunk_xs.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(StoreError::Format(
            "chunk coordinates must be unique, sorted, and within the save limit".into(),
        ));
    }
    let expected_blocks = header
        .chunk_xs
        .len()
        .checked_mul(CHUNK_AREA)
        .ok_or_else(|| StoreError::Format("dense chunk data length overflowed".into()))?;
    if dense_blocks.len() != expected_blocks {
        return Err(StoreError::Format(format!(
            "dense block array has {} entries; expected {expected_blocks}",
            dense_blocks.len()
        )));
    }

    let mut chunks = Vec::with_capacity(header.chunk_xs.len());
    for (chunk_x, encoded_chunk) in header
        .chunk_xs
        .iter()
        .copied()
        .zip(dense_blocks.chunks_exact(CHUNK_AREA))
    {
        let mut blocks = Vec::with_capacity(CHUNK_AREA);
        for palette_code in encoded_chunk {
            let code = if *palette_code == 0 {
                0
            } else {
                let palette_index = usize::from(*palette_code - 1);
                let Some(kind) = header.palette.get(palette_index).copied() else {
                    return Err(StoreError::Format(format!(
                        "dense block entry references missing palette index {palette_code}"
                    )));
                };
                kind.code().ok_or_else(|| {
                    StoreError::Format(format!(
                        "palette entry contains invalid block state {}:{}",
                        kind.id, kind.variant
                    ))
                })?
            };
            blocks.push(code);
        }
        chunks.push(ChunkSnapshot { x: chunk_x, blocks });
    }

    let save = save_from_metadata(header.metadata, chunks)?;
    validate(&save)?;
    Ok(save)
}

fn metadata_from_save(save: &WorldSnapshot) -> ScwMetadataV1 {
    ScwMetadataV1 {
        schema_version: SAVE_SCHEMA_VERSION,
        generator_version: save.generator_version,
        height: save.height,
        name: save.name.clone(),
        seed: save.seed,
        created_at_unix_s: save.created_at_unix_s,
        last_played_unix_s: save.last_played_unix_s,
        day_time_ticks: save.day_time_ticks,
        player_chunk_x: save.player.chunk_x,
        player_local_x: save.player.local_x,
        player_y: save.player.y,
        selected_slot: save.player.selected_slot,
    }
}

fn save_from_metadata(
    metadata: ScwMetadataV1,
    chunks: Vec<ChunkSnapshot>,
) -> Result<WorldSnapshot, StoreError> {
    if metadata.schema_version != SAVE_SCHEMA_VERSION {
        return Err(StoreError::UnsupportedSchema(metadata.schema_version));
    }
    Ok(WorldSnapshot {
        generator_version: metadata.generator_version,
        height: metadata.height,
        name: metadata.name,
        seed: metadata.seed,
        created_at_unix_s: metadata.created_at_unix_s,
        last_played_unix_s: metadata.last_played_unix_s,
        day_time_ticks: metadata.day_time_ticks,
        player: PlayerSnapshot {
            chunk_x: metadata.player_chunk_x,
            local_x: metadata.player_local_x,
            y: metadata.player_y,
            selected_slot: metadata.selected_slot,
        },
        chunks,
    })
}

fn validate(save: &WorldSnapshot) -> Result<(), StoreError> {
    validate_snapshot(save).map_err(|error| StoreError::Validation(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::{WorldRepository, blank_snapshot},
        domain::spawn_for_seed,
    };
    use glam::Vec2;
    use std::fs::File;
    use tempfile::tempdir;

    fn example_save() -> WorldSnapshot {
        let mut blocks = vec![0; CHUNK_AREA];
        blocks[0] = BlockState::BEDROCK.code();
        blocks[1] = BlockState::BEDROCK.code();
        blocks[(2 * CHUNK_WIDTH + 3) as usize] = BlockState::TORCH.code();
        blank_snapshot(
            7,
            "Example".into(),
            vec![ChunkSnapshot { x: 0, blocks }],
            Vec2::new(2.5, 4.0),
            10,
        )
    }

    #[test]
    fn round_trip_has_magic_compression_and_atomic_overwrite() {
        let directory = tempdir().unwrap();
        let store = ScwRepository::new(directory.path());
        let id = WorldId::new("example").unwrap();
        let path = store.path_for(&id);
        let mut save = example_save();
        WorldRepository::save(&store, &id, &save).unwrap();
        assert!(path.is_dir());
        let bytes = fs::read(path.join(MANIFEST_FILE)).unwrap();
        assert_eq!(&bytes[..SCW_MAGIC.len()], SCW_MAGIC);
        assert!(bytes.len() < CHUNK_AREA);
        assert_eq!(WorldRepository::load(&store, &id).unwrap(), save);

        save.name = "Updated".into();
        WorldRepository::save(&store, &id, &save).unwrap();
        assert_eq!(WorldRepository::load(&store, &id).unwrap().name, "Updated");
        let region_count = fs::read_dir(&path)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("region-"))
            .count();
        assert_eq!(region_count, 1);
    }

    #[test]
    fn payload_uses_a_dense_byte_array_and_palette() {
        let (header, blocks) = parts_from_save(&example_save()).unwrap();
        assert_eq!(blocks.len(), CHUNK_AREA);
        assert_eq!(
            header.palette,
            vec![
                ScwPaletteBlock::from_code(BlockState::TORCH.code()).unwrap(),
                ScwPaletteBlock::from_code(BlockState::BEDROCK.code()).unwrap(),
            ]
        );
        assert_eq!(blocks[0], 2);
        assert_eq!(blocks[(2 * CHUNK_WIDTH + 3) as usize], 1);
    }

    #[test]
    fn generated_paths_do_not_collide() {
        let directory = tempdir().unwrap();
        let store = ScwRepository::new(directory.path());
        let first = WorldRepository::next_available_id(&store, 7).unwrap();
        File::create(store.path_for(&first)).unwrap();
        let second = WorldRepository::next_available_id(&store, 7).unwrap();
        assert_eq!(second.as_str(), "world-0000000000000007-2");
    }

    #[test]
    fn list_is_sorted_reports_invalid_scw_and_ignores_json() {
        let directory = tempdir().unwrap();
        let store = ScwRepository::new(directory.path());
        let mut old = example_save();
        old.last_played_unix_s = 1;
        old.name = "Old".into();
        WorldRepository::save(&store, &WorldId::new("old").unwrap(), &old).unwrap();
        let mut new = example_save();
        new.last_played_unix_s = 2;
        new.name = "New".into();
        WorldRepository::save(&store, &WorldId::new("new").unwrap(), &new).unwrap();
        fs::write(store.root().join("broken.scw"), b"not-scw").unwrap();
        fs::write(store.root().join("legacy.json"), b"{}").unwrap();

        let listed = WorldRepository::list(&store).unwrap();
        assert_eq!(listed.valid[0].name, "New");
        assert_eq!(listed.invalid.len(), 1);
    }

    #[test]
    fn validation_rejects_duplicates_and_malformed_palettes() {
        let mut save = example_save();
        save.chunks.push(save.chunks[0].clone());
        assert!(validate(&save).is_err());

        let (mut header, blocks) = parts_from_save(&example_save()).unwrap();
        header.palette.push(header.palette[0]);
        assert!(save_from_parts(header, blocks).is_err());

        let (header, mut blocks) = parts_from_save(&example_save()).unwrap();
        blocks[10] = 200;
        assert!(save_from_parts(header, blocks).is_err());
    }

    #[test]
    fn bad_headers_and_truncated_compression_are_rejected() {
        assert!(decode_scw(b"JSON{}").is_err());
        let mut encoded = encode_scw(&example_save()).unwrap();
        encoded.truncate(encoded.len() / 2);
        assert!(decode_scw(&encoded).is_err());
    }

    #[test]
    fn package_partitions_chunks_into_regions_and_rejects_legacy_files() {
        let directory = tempdir().unwrap();
        let store = ScwRepository::new(directory.path());
        let mut save = example_save();
        let blocks = save.chunks[0].blocks.clone();
        save.chunks = [-65, -1, 0, 64]
            .into_iter()
            .map(|x| ChunkSnapshot {
                x,
                blocks: blocks.clone(),
            })
            .collect();
        let id = WorldId::new("package").unwrap();
        let package_path = store.path_for(&id);
        WorldRepository::save(&store, &id, &save).unwrap();
        assert_eq!(WorldRepository::load(&store, &id).unwrap(), save);
        let manifest: ScwPackageManifestV1 =
            decode_postcard_envelope(&fs::read(package_path.join(MANIFEST_FILE)).unwrap()).unwrap();
        assert_eq!(
            manifest
                .regions
                .iter()
                .map(|region| region.region_x)
                .collect::<Vec<_>>(),
            vec![-2, -1, 0, 1]
        );

        let legacy_path = store.root().join("legacy.scw");
        fs::write(&legacy_path, encode_scw(&save).unwrap()).unwrap();
        assert!(matches!(
            WorldRepository::load(&store, &WorldId::new("legacy").unwrap()),
            Err(RepositoryError::InvalidSnapshot(_))
        ));
    }

    #[test]
    fn large_horizontal_coordinates_round_trip_without_float_precision_loss() {
        let directory = tempdir().unwrap();
        let store = ScwRepository::new(directory.path());
        let mut save = example_save();
        save.player.chunk_x = 4_000_000_000_000;
        save.player.local_x = 31.75;
        save.chunks[0].x = 4_000_000_000_000;
        let id = WorldId::new("far-away").unwrap();

        WorldRepository::save(&store, &id, &save).unwrap();

        assert_eq!(WorldRepository::load(&store, &id).unwrap(), save);
    }

    #[test]
    fn large_absolute_day_time_round_trips_exactly() {
        let mut save = example_save();
        save.day_time_ticks = u64::MAX - 17;
        assert_eq!(
            decode_scw(&encode_scw(&save).unwrap())
                .unwrap()
                .day_time_ticks,
            u64::MAX - 17
        );
    }

    #[test]
    fn invalid_root_produces_an_io_error() {
        let directory = tempdir().unwrap();
        let file_root = directory.path().join("not-a-directory");
        File::create(&file_root).unwrap();
        let store = ScwRepository::new(&file_root);
        assert!(matches!(
            WorldRepository::next_available_id(&store, 1),
            Err(RepositoryError::Unavailable(_))
        ));
    }

    #[test]
    fn schema_four_is_exact_and_older_metadata_is_rejected() {
        let save = blank_snapshot(1, "World".into(), Vec::new(), spawn_for_seed(1), 1);
        let mut metadata = metadata_from_save(&save);
        assert_eq!(metadata.schema_version, 4);
        metadata.schema_version = 3;
        assert!(matches!(
            save_from_metadata(metadata, Vec::new()),
            Err(StoreError::UnsupportedSchema(3))
        ));
    }

    #[test]
    fn schema_four_palette_entries_encode_ids_and_variants() {
        let grass = ScwPaletteBlock::from_code(BlockState::GRASS.code()).unwrap();
        let torch = ScwPaletteBlock::from_code(BlockState::TORCH.code()).unwrap();
        let wall_torch = ScwPaletteBlock::from_code(BlockState::WALL_TORCH_LEFT.code()).unwrap();
        assert_eq!(postcard::to_allocvec(&grass).unwrap(), [1, 0]);
        assert_eq!(postcard::to_allocvec(&torch).unwrap(), [8, 0]);
        assert_eq!(postcard::to_allocvec(&wall_torch).unwrap(), [8, 1]);
    }

    #[test]
    fn region_hash_is_separated_by_schema() {
        let chunks = example_save().chunks;
        assert_ne!(
            hash_region_for_schema(&chunks, 3),
            hash_region_for_schema(&chunks, 4)
        );
    }
}
