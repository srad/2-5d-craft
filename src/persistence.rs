use crate::{BlockKind, CHUNK_WIDTH, GENERATOR_VERSION, SAVE_SCHEMA_VERSION, WORLD_HEIGHT};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::io::{self, BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
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

#[derive(Debug, Clone, PartialEq)]
pub struct WorldSaveV1 {
    pub schema_version: u32,
    pub generator_version: u32,
    pub height: i32,
    pub name: String,
    pub seed: u64,
    pub created_at_unix_s: u64,
    pub last_played_unix_s: u64,
    pub day_phase: f32,
    pub player: SavedPlayer,
    pub chunks: Vec<SavedChunk>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SavedPlayer {
    pub chunk_x: i64,
    pub local_x: f32,
    pub y: f32,
    pub selected_slot: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedChunk {
    pub x: i64,
    pub blocks: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ScwMetadataV1 {
    schema_version: u32,
    generator_version: u32,
    height: i32,
    name: String,
    seed: u64,
    created_at_unix_s: u64,
    last_played_unix_s: u64,
    day_phase: f32,
    player_chunk_x: i64,
    player_local_x: f32,
    player_y: f32,
    selected_slot: u8,
}

#[derive(Debug, Serialize, Deserialize)]
struct ScwHeaderV1 {
    metadata: ScwMetadataV1,
    palette: Vec<BlockKind>,
    chunk_xs: Vec<i64>,
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

#[derive(Resource, Debug, Clone)]
pub struct WorldStore {
    root: PathBuf,
}

#[derive(Debug)]
pub struct ListedWorld {
    pub path: PathBuf,
    pub save: WorldSaveV1,
}

#[derive(Debug)]
pub struct WorldList {
    pub valid: Vec<ListedWorld>,
    pub invalid: Vec<(PathBuf, String)>,
}

#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    Postcard(postcard::Error),
    Compression(io::Error),
    Format(String),
    Validation(String),
    Clock,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Postcard(error) => write!(formatter, "invalid world metadata: {error}"),
            Self::Compression(error) => write!(formatter, "invalid compressed data: {error}"),
            Self::Format(error) => write!(formatter, "invalid .scw file: {error}"),
            Self::Validation(error) => write!(formatter, "invalid world: {error}"),
            Self::Clock => write!(formatter, "system clock is before the Unix epoch"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl Default for WorldStore {
    fn default() -> Self {
        Self::new("worlds")
    }
}

impl WorldStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn now_unix_s() -> Result<u64, StoreError> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .map_err(|_| StoreError::Clock)
    }

    pub fn new_path(&self, seed: u64) -> Result<PathBuf, StoreError> {
        fs::create_dir_all(&self.root)?;
        let base = format!("world-{seed:016x}");
        for suffix in 1_u32.. {
            let filename = if suffix == 1 {
                format!("{base}.scw")
            } else {
                format!("{base}-{suffix}.scw")
            };
            let path = self.root.join(filename);
            if !path.exists() {
                return Ok(path);
            }
        }
        unreachable!("the numeric suffix space cannot be exhausted")
    }

    pub fn save(&self, path: &Path, save: &WorldSaveV1) -> Result<(), StoreError> {
        validate(save)?;
        fs::create_dir_all(&self.root)?;
        self.ensure_path(path)?;
        if path.is_file() {
            return atomic_write(&self.root, path, &encode_scw(save)?);
        }
        save_package(path, save)
    }

    pub fn load(&self, path: &Path) -> Result<WorldSaveV1, StoreError> {
        self.ensure_path(path)?;
        if path.is_dir() {
            load_package(path)
        } else {
            decode_scw(&fs::read(path)?)
        }
    }

    pub fn list(&self) -> Result<WorldList, StoreError> {
        fs::create_dir_all(&self.root)?;
        let mut list = WorldList {
            valid: Vec::new(),
            invalid: Vec::new(),
        };
        for entry in fs::read_dir(&self.root)? {
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("scw") {
                continue;
            }
            match self.load(&path) {
                Ok(save) => list.valid.push(ListedWorld { path, save }),
                Err(error) => list.invalid.push((path, error.to_string())),
            }
        }
        list.valid.sort_by(|a, b| {
            b.save
                .last_played_unix_s
                .cmp(&a.save.last_played_unix_s)
                .then_with(|| a.save.name.cmp(&b.save.name))
                .then_with(|| a.path.cmp(&b.path))
        });
        list.invalid.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(list)
    }

    fn ensure_path(&self, path: &Path) -> Result<(), StoreError> {
        if path.parent() != Some(self.root.as_path()) {
            return Err(StoreError::Validation(
                "save path is outside the configured world directory".into(),
            ));
        }
        if path.extension().and_then(|value| value.to_str()) != Some("scw") {
            return Err(StoreError::Validation(
                "world save path must use the .scw extension".into(),
            ));
        }
        Ok(())
    }
}

fn save_package(path: &Path, save: &WorldSaveV1) -> Result<(), StoreError> {
    fs::create_dir_all(path)?;
    let generation = rand::random::<u64>();
    let mut grouped = BTreeMap::<i64, Vec<SavedChunk>>::new();
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
            let region_save = WorldSaveV1 {
                last_played_unix_s: 0,
                day_phase: 0.20,
                player: SavedPlayer {
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

fn load_package(path: &Path) -> Result<WorldSaveV1, StoreError> {
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
    let save = save_from_metadata(manifest.metadata, chunks);
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

fn hash_region(chunks: &[SavedChunk]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
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

fn region_metadata_matches(region: &WorldSaveV1, metadata: &ScwMetadataV1) -> bool {
    region.schema_version == metadata.schema_version
        && region.generator_version == metadata.generator_version
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

fn encode_scw(save: &WorldSaveV1) -> Result<Vec<u8>, StoreError> {
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

fn decode_scw(file: &[u8]) -> Result<WorldSaveV1, StoreError> {
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

fn parts_from_save(save: &WorldSaveV1) -> Result<(ScwHeaderV1, Vec<u8>), StoreError> {
    validate(save)?;
    let palette = save
        .chunks
        .iter()
        .flat_map(|chunk| chunk.blocks.iter().copied())
        .filter_map(BlockKind::from_code)
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
            let palette_code = match BlockKind::from_code(*code) {
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

fn save_from_parts(header: ScwHeaderV1, dense_blocks: Vec<u8>) -> Result<WorldSaveV1, StoreError> {
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
                kind.code()
            };
            blocks.push(code);
        }
        chunks.push(SavedChunk { x: chunk_x, blocks });
    }

    let save = save_from_metadata(header.metadata, chunks);
    validate(&save)?;
    Ok(save)
}

fn metadata_from_save(save: &WorldSaveV1) -> ScwMetadataV1 {
    ScwMetadataV1 {
        schema_version: save.schema_version,
        generator_version: save.generator_version,
        height: save.height,
        name: save.name.clone(),
        seed: save.seed,
        created_at_unix_s: save.created_at_unix_s,
        last_played_unix_s: save.last_played_unix_s,
        day_phase: save.day_phase,
        player_chunk_x: save.player.chunk_x,
        player_local_x: save.player.local_x,
        player_y: save.player.y,
        selected_slot: save.player.selected_slot,
    }
}

fn save_from_metadata(metadata: ScwMetadataV1, chunks: Vec<SavedChunk>) -> WorldSaveV1 {
    WorldSaveV1 {
        schema_version: metadata.schema_version,
        generator_version: metadata.generator_version,
        height: metadata.height,
        name: metadata.name,
        seed: metadata.seed,
        created_at_unix_s: metadata.created_at_unix_s,
        last_played_unix_s: metadata.last_played_unix_s,
        day_phase: metadata.day_phase,
        player: SavedPlayer {
            chunk_x: metadata.player_chunk_x,
            local_x: metadata.player_local_x,
            y: metadata.player_y,
            selected_slot: metadata.selected_slot,
        },
        chunks,
    }
}

pub fn validate(save: &WorldSaveV1) -> Result<(), StoreError> {
    if save.schema_version != SAVE_SCHEMA_VERSION {
        return Err(StoreError::Validation(format!(
            "unsupported schema version {}",
            save.schema_version
        )));
    }
    if save.generator_version != GENERATOR_VERSION {
        return Err(StoreError::Validation(format!(
            "unsupported generator version {}",
            save.generator_version
        )));
    }
    if save.height != WORLD_HEIGHT {
        return Err(StoreError::Validation(format!(
            "expected chunk height {WORLD_HEIGHT}"
        )));
    }
    if save.name.trim().is_empty() || save.name.len() > 80 {
        return Err(StoreError::Validation(
            "world name must contain 1 to 80 characters".into(),
        ));
    }
    if !save.day_phase.is_finite() || !(0.0..1.0).contains(&save.day_phase) {
        return Err(StoreError::Validation(
            "day phase must be finite and in [0, 1)".into(),
        ));
    }
    if !save.player.local_x.is_finite()
        || !(0.0..CHUNK_WIDTH as f32).contains(&save.player.local_x)
        || !save.player.y.is_finite()
    {
        return Err(StoreError::Validation(
            "player position must be finite and local x must be inside its chunk".into(),
        ));
    }
    if !(1..=BlockKind::HOTBAR.len() as u8).contains(&save.player.selected_slot) {
        return Err(StoreError::Validation(
            "selected hotbar slot is invalid".into(),
        ));
    }
    let mut previous = None;
    for chunk in &save.chunks {
        if chunk.blocks.len() != CHUNK_AREA {
            return Err(StoreError::Validation(format!(
                "chunk {} has {} blocks; expected {CHUNK_AREA}",
                chunk.x,
                chunk.blocks.len()
            )));
        }
        if chunk
            .blocks
            .iter()
            .any(|code| *code != 0 && BlockKind::from_code(*code).is_none())
        {
            return Err(StoreError::Validation(format!(
                "chunk {} contains an invalid block code",
                chunk.x
            )));
        }
        if previous.is_some_and(|value| value >= chunk.x) {
            return Err(StoreError::Validation(
                "chunks must be unique and sorted by x".into(),
            ));
        }
        previous = Some(chunk.x);
    }
    Ok(())
}

pub fn blank_save(seed: u64, name: String, chunks: Vec<SavedChunk>, spawn: Vec2) -> WorldSaveV1 {
    let now = WorldStore::now_unix_s().unwrap_or(0);
    WorldSaveV1 {
        schema_version: SAVE_SCHEMA_VERSION,
        generator_version: GENERATOR_VERSION,
        height: WORLD_HEIGHT,
        name,
        seed,
        created_at_unix_s: now,
        last_played_unix_s: now,
        day_phase: 0.20,
        player: SavedPlayer {
            chunk_x: (spawn.x.floor() as i64).div_euclid(i64::from(CHUNK_WIDTH)),
            local_x: spawn.x.rem_euclid(CHUNK_WIDTH as f32),
            y: spawn.y,
            selected_slot: 1,
        },
        chunks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    fn example_save() -> WorldSaveV1 {
        let mut blocks = vec![0; CHUNK_AREA];
        blocks[0] = BlockKind::Bedrock.code();
        blocks[1] = BlockKind::Bedrock.code();
        blocks[(2 * CHUNK_WIDTH + 3) as usize] = BlockKind::Torch.code();
        blank_save(
            7,
            "Example".into(),
            vec![SavedChunk { x: 0, blocks }],
            Vec2::new(2.5, 4.0),
        )
    }

    #[test]
    fn round_trip_has_magic_compression_and_atomic_overwrite() {
        let directory = tempdir().unwrap();
        let store = WorldStore::new(directory.path());
        let path = store.new_path(7).unwrap();
        let mut save = example_save();
        store.save(&path, &save).unwrap();
        assert!(path.is_dir());
        let bytes = fs::read(path.join(MANIFEST_FILE)).unwrap();
        assert_eq!(&bytes[..SCW_MAGIC.len()], SCW_MAGIC);
        assert!(bytes.len() < CHUNK_AREA);
        assert_eq!(store.load(&path).unwrap(), save);

        save.name = "Updated".into();
        store.save(&path, &save).unwrap();
        assert_eq!(store.load(&path).unwrap().name, "Updated");
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
        assert_eq!(header.palette, vec![BlockKind::Torch, BlockKind::Bedrock]);
        assert_eq!(blocks[0], 2);
        assert_eq!(blocks[(2 * CHUNK_WIDTH + 3) as usize], 1);
    }

    #[test]
    fn generated_paths_do_not_collide() {
        let directory = tempdir().unwrap();
        let store = WorldStore::new(directory.path());
        let first = store.new_path(7).unwrap();
        File::create(&first).unwrap();
        let second = store.new_path(7).unwrap();
        assert_ne!(first, second);
        assert!(second.ends_with("world-0000000000000007-2.scw"));
    }

    #[test]
    fn list_is_sorted_reports_invalid_scw_and_ignores_json() {
        let directory = tempdir().unwrap();
        let store = WorldStore::new(directory.path());
        let mut old = example_save();
        old.last_played_unix_s = 1;
        old.name = "Old".into();
        store.save(&store.root().join("old.scw"), &old).unwrap();
        let mut new = example_save();
        new.last_played_unix_s = 2;
        new.name = "New".into();
        store.save(&store.root().join("new.scw"), &new).unwrap();
        fs::write(store.root().join("broken.scw"), b"not-scw").unwrap();
        fs::write(store.root().join("legacy.json"), b"{}").unwrap();

        let listed = store.list().unwrap();
        assert_eq!(listed.valid[0].save.name, "New");
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
    fn package_partitions_chunks_into_regions_and_loads_legacy_files() {
        let directory = tempdir().unwrap();
        let store = WorldStore::new(directory.path());
        let mut save = example_save();
        let blocks = save.chunks[0].blocks.clone();
        save.chunks = [-65, -1, 0, 64]
            .into_iter()
            .map(|x| SavedChunk {
                x,
                blocks: blocks.clone(),
            })
            .collect();
        let package_path = store.new_path(7).unwrap();
        store.save(&package_path, &save).unwrap();
        assert_eq!(store.load(&package_path).unwrap(), save);
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
        assert_eq!(store.load(&legacy_path).unwrap(), save);
    }

    #[test]
    fn large_horizontal_coordinates_round_trip_without_float_precision_loss() {
        let directory = tempdir().unwrap();
        let store = WorldStore::new(directory.path());
        let mut save = example_save();
        save.player.chunk_x = 4_000_000_000_000;
        save.player.local_x = 31.75;
        save.chunks[0].x = 4_000_000_000_000;
        let path = store.new_path(7).unwrap();

        store.save(&path, &save).unwrap();

        assert_eq!(store.load(&path).unwrap(), save);
    }

    #[test]
    fn invalid_root_produces_an_io_error() {
        let directory = tempdir().unwrap();
        let file_root = directory.path().join("not-a-directory");
        File::create(&file_root).unwrap();
        let store = WorldStore::new(&file_root);
        assert!(matches!(store.new_path(1), Err(StoreError::Io(_))));
    }
}
