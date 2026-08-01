use crate::application::{
    ChunkSnapshot, InvalidWorldEntry, PlayerSnapshot, RepositoryError, SnapshotError, WorldCatalog,
    WorldId, WorldRepository, WorldSnapshot, WorldSummary, validate_snapshot,
};
use crate::domain::{
    BlockId, BlockState, CHUNK_WIDTH, MutationPriority, ScheduledTick, VoxelLayer, VoxelPos,
    WORLD_HEIGHT,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::io::{self, BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

const SCW_MAGIC: &[u8; 4] = b"SCW1";
const SAVE_SCHEMA_VERSION: u32 = 6;
const ENVELOPE_BYTES: usize = SCW_MAGIC.len() + 4 + 8;
const ZSTD_LEVEL: i32 = 3;
const MAX_FILE_BYTES: usize = 512 * 1024 * 1024;
const MAX_DECOMPRESSED_BYTES: usize = 512 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 4 * 1024 * 1024;
const CHUNK_AREA: usize = (CHUNK_WIDTH * WORLD_HEIGHT) as usize;
const REGION_CHUNKS: i64 = 64;
/// One region holds at most `REGION_CHUNKS` chunks, each bounded by the per-tick proposal
/// budget documented in `ARCHITECTURE.md`.
const MAX_PENDING_TICKS_PER_REGION: usize = REGION_CHUNKS as usize * 4_096;
const MANIFEST_FILE: &str = "manifest.scw";
const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ScwMetadata {
    generator_version: u32,
    height: i32,
    name: String,
    seed: u64,
    created_at_unix_s: u64,
    last_played_unix_s: u64,
    day_time_ticks: u64,
    world_tick: u64,
    next_tick_sequence: u64,
    player_chunk_x: i64,
    player_local_x: f32,
    player_y: f32,
    selected_slot: u8,
}

#[derive(Debug, Serialize, Deserialize)]
struct ScwManifest {
    metadata: ScwMetadata,
    regions: Vec<ScwRegionRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ScwRegionRef {
    region_x: i64,
    content_hash: u64,
    file_name: String,
}

/// The Postcard part of a region file. The dense block arrays follow it as raw bytes so that
/// megabytes of block data never fall under the metadata size bound.
#[derive(Debug, Serialize, Deserialize)]
struct ScwRegionHeader {
    region_x: i64,
    seed: u64,
    generator_version: u32,
    palette: Vec<ScwPaletteBlock>,
    chunk_xs: Vec<i64>,
    pending_ticks: Vec<Vec<ScwScheduledTick>>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct ScwScheduledTick {
    due_world_tick: u64,
    priority: u16,
    global_x: i64,
    y: i32,
    layer: u8,
    sequence: u64,
    /// Block code the tick expects to find, or `0` for "no expectation". Code `0` is air and is
    /// never a valid `BlockState`.
    expected: u8,
}

impl ScwScheduledTick {
    fn from_tick(tick: ScheduledTick) -> Self {
        Self {
            due_world_tick: tick.due_world_tick,
            priority: tick.priority.0,
            global_x: tick.position.global_x,
            y: tick.position.y,
            layer: tick.position.layer.depth(),
            sequence: tick.sequence,
            expected: tick.expected.map_or(0, BlockState::code),
        }
    }

    fn tick(self) -> Result<ScheduledTick, StoreError> {
        let Some(layer) = VoxelLayer::from_persistent_depth(self.layer) else {
            return Err(StoreError::Format(format!(
                "pending tick references unknown layer {}",
                self.layer
            )));
        };
        let expected = match self.expected {
            0 => None,
            code => Some(BlockState::from_code(code).ok_or_else(|| {
                StoreError::Format(format!("pending tick expects invalid block code {code}"))
            })?),
        };
        Ok(ScheduledTick {
            due_world_tick: self.due_world_tick,
            priority: MutationPriority(self.priority),
            position: VoxelPos::new(self.global_x, self.y, layer),
            sequence: self.sequence,
            expected,
        })
    }

    fn hash_into(self, hash: &mut Fnv1a) {
        hash.write(&self.due_world_tick.to_le_bytes());
        hash.write(&self.priority.to_le_bytes());
        hash.write(&self.global_x.to_le_bytes());
        hash.write(&self.y.to_le_bytes());
        hash.write(&[self.layer, self.expected]);
        hash.write(&self.sequence.to_le_bytes());
    }
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
    let metadata = metadata_from_save(save);
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
            atomic_write(
                path,
                &region_path,
                &encode_region(region_x, &metadata, &chunks)?,
            )?;
        }
        regions.push(ScwRegionRef {
            region_x,
            content_hash,
            file_name,
        });
    }

    let manifest = ScwManifest { metadata, regions };
    atomic_write(
        path,
        &path.join(MANIFEST_FILE),
        &encode_manifest(&manifest)?,
    )?;
    cleanup_obsolete_regions(path, &manifest)?;
    Ok(())
}

fn load_package(path: &Path) -> Result<WorldSnapshot, StoreError> {
    let manifest = decode_manifest(&fs::read(path.join(MANIFEST_FILE))?)?;
    validate_manifest(&manifest)?;
    let mut chunks = Vec::new();
    for region in &manifest.regions {
        let region_chunks = decode_region(
            &fs::read(path.join(&region.file_name))?,
            region.region_x,
            &manifest.metadata,
        )?;
        if hash_region(&region_chunks) != region.content_hash {
            return Err(StoreError::Format(format!(
                "region {} content hash does not match the manifest",
                region.region_x
            )));
        }
        chunks.extend(region_chunks);
    }
    chunks.sort_by_key(|chunk| chunk.x);
    let save = save_from_metadata(manifest.metadata, chunks);
    validate(&save)?;
    Ok(save)
}

fn validate_manifest(manifest: &ScwManifest) -> Result<(), StoreError> {
    if manifest.metadata.height != WORLD_HEIGHT {
        return Err(StoreError::Format(format!(
            "expected chunk height {WORLD_HEIGHT}"
        )));
    }
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

struct Fnv1a(u64);

impl Fnv1a {
    fn new() -> Self {
        Self(FNV_OFFSET_BASIS)
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(FNV_PRIME);
        }
    }

    fn finish(self) -> u64 {
        self.0
    }
}

fn checksum(bytes: &[u8]) -> u64 {
    let mut hash = Fnv1a::new();
    hash.write(bytes);
    hash.finish()
}

fn hash_region(chunks: &[ChunkSnapshot]) -> u64 {
    hash_region_for_schema(chunks, SAVE_SCHEMA_VERSION)
}

/// Hashes region content from field bytes rather than an encoded payload, so the content
/// address never depends on an encoding library's stability.
fn hash_region_for_schema(chunks: &[ChunkSnapshot], schema_version: u32) -> u64 {
    let mut hash = Fnv1a::new();
    hash.write(&schema_version.to_le_bytes());
    for chunk in chunks {
        hash.write(&chunk.x.to_le_bytes());
        hash.write(&chunk.foreground);
        hash.write(&chunk.backwall);
        hash.write(&(chunk.pending_ticks.len() as u64).to_le_bytes());
        for tick in &chunk.pending_ticks {
            ScwScheduledTick::from_tick(*tick).hash_into(&mut hash);
        }
    }
    hash.finish()
}

fn cleanup_obsolete_regions(path: &Path, manifest: &ScwManifest) -> Result<(), StoreError> {
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

/// `SCW1` | schema version | payload checksum | Zstandard payload.
fn encode_envelope(payload: &[u8]) -> Result<Vec<u8>, StoreError> {
    let compressed = zstd::stream::encode_all(Cursor::new(payload), ZSTD_LEVEL)
        .map_err(StoreError::Compression)?;
    if ENVELOPE_BYTES + compressed.len() > MAX_FILE_BYTES {
        return Err(StoreError::Format("compressed payload is too large".into()));
    }
    let mut file = Vec::with_capacity(ENVELOPE_BYTES + compressed.len());
    file.extend_from_slice(SCW_MAGIC);
    file.extend_from_slice(&SAVE_SCHEMA_VERSION.to_le_bytes());
    file.extend_from_slice(&checksum(&compressed).to_le_bytes());
    file.extend_from_slice(&compressed);
    Ok(file)
}

/// Rejects size, magic, schema, and checksum before anything is decompressed.
fn decode_envelope(file: &[u8], max_decompressed: usize) -> Result<Vec<u8>, StoreError> {
    if file.len() > MAX_FILE_BYTES {
        return Err(StoreError::Format("file is too large".into()));
    }
    if file.get(..SCW_MAGIC.len()) != Some(SCW_MAGIC.as_slice()) {
        return Err(StoreError::Format(
            "unsupported or missing SCW1 header".into(),
        ));
    }
    let Some(header) = file.get(SCW_MAGIC.len()..ENVELOPE_BYTES) else {
        return Err(StoreError::Format("SCW1 envelope is truncated".into()));
    };
    let (schema, expected_checksum) = header.split_at(4);
    let schema = u32::from_le_bytes(
        schema
            .try_into()
            .expect("the schema slice has exactly four bytes"),
    );
    if schema != SAVE_SCHEMA_VERSION {
        return Err(StoreError::UnsupportedSchema(schema));
    }
    let expected_checksum = u64::from_le_bytes(
        expected_checksum
            .try_into()
            .expect("the checksum slice has exactly eight bytes"),
    );
    let compressed = &file[ENVELOPE_BYTES..];
    if checksum(compressed) != expected_checksum {
        return Err(StoreError::Format(
            "payload checksum does not match the envelope".into(),
        ));
    }
    let mut decoded = Vec::new();
    zstd::stream::read::Decoder::new(compressed)
        .map_err(StoreError::Compression)?
        .take((max_decompressed + 1) as u64)
        .read_to_end(&mut decoded)
        .map_err(StoreError::Compression)?;
    if decoded.len() > max_decompressed {
        return Err(StoreError::Format(
            "decompressed payload is too large".into(),
        ));
    }
    Ok(decoded)
}

fn encode_manifest(manifest: &ScwManifest) -> Result<Vec<u8>, StoreError> {
    let payload = postcard::to_allocvec(manifest).map_err(StoreError::Postcard)?;
    if payload.len() > MAX_HEADER_BYTES {
        return Err(StoreError::Format("metadata is too large".into()));
    }
    encode_envelope(&payload)
}

fn decode_manifest(file: &[u8]) -> Result<ScwManifest, StoreError> {
    let decoded = decode_envelope(file, MAX_HEADER_BYTES)?;
    let (manifest, remaining) =
        postcard::take_from_bytes(&decoded).map_err(StoreError::Postcard)?;
    if !remaining.is_empty() {
        return Err(StoreError::Format("metadata contains trailing data".into()));
    }
    Ok(manifest)
}

fn encode_region(
    region_x: i64,
    metadata: &ScwMetadata,
    chunks: &[ChunkSnapshot],
) -> Result<Vec<u8>, StoreError> {
    let palette = chunks
        .iter()
        .flat_map(|chunk| chunk.foreground.iter().chain(&chunk.backwall).copied())
        .filter_map(ScwPaletteBlock::from_code)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if palette.len() > u8::MAX as usize {
        return Err(StoreError::Format(
            "block palette contains more than 255 entries".into(),
        ));
    }

    let mut blocks = Vec::with_capacity(chunks.len() * CHUNK_AREA * 2);
    for chunk in chunks {
        for layer in [&chunk.foreground, &chunk.backwall] {
            for code in layer {
                let palette_code = match ScwPaletteBlock::from_code(*code) {
                    None if *code == 0 => 0,
                    Some(kind) => {
                        let palette_index = palette
                            .binary_search(&kind)
                            .expect("every chunk block kind was collected into the palette");
                        u8::try_from(palette_index + 1).map_err(|_| {
                            StoreError::Format("block palette index overflowed".into())
                        })?
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
    }

    let header = ScwRegionHeader {
        region_x,
        seed: metadata.seed,
        generator_version: metadata.generator_version,
        palette,
        chunk_xs: chunks.iter().map(|chunk| chunk.x).collect(),
        pending_ticks: chunks
            .iter()
            .map(|chunk| {
                chunk
                    .pending_ticks
                    .iter()
                    .map(|tick| ScwScheduledTick::from_tick(*tick))
                    .collect()
            })
            .collect(),
    };
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
    encode_envelope(&payload)
}

fn decode_region(
    file: &[u8],
    region_x: i64,
    metadata: &ScwMetadata,
) -> Result<Vec<ChunkSnapshot>, StoreError> {
    let decoded = decode_envelope(file, MAX_DECOMPRESSED_BYTES)?;
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
    let (header, remaining): (ScwRegionHeader, &[u8]) =
        postcard::take_from_bytes(encoded_header).map_err(StoreError::Postcard)?;
    if !remaining.is_empty() {
        return Err(StoreError::Format("metadata contains trailing data".into()));
    }

    if header.region_x != region_x
        || header.seed != metadata.seed
        || header.generator_version != metadata.generator_version
    {
        return Err(StoreError::Format(format!(
            "region {region_x} identity does not match the manifest"
        )));
    }
    if header.palette.len() > u8::MAX as usize
        || header.palette.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(StoreError::Format(
            "block palette must be unique and sorted".into(),
        ));
    }
    if header.chunk_xs.is_empty() || header.chunk_xs.len() > REGION_CHUNKS as usize {
        return Err(StoreError::Format(format!(
            "region {region_x} must hold 1 to {REGION_CHUNKS} chunks"
        )));
    }
    if header.chunk_xs.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(StoreError::Format(
            "chunk coordinates must be unique and sorted".into(),
        ));
    }
    if header
        .chunk_xs
        .iter()
        .any(|chunk_x| chunk_x.div_euclid(REGION_CHUNKS) != region_x)
    {
        return Err(StoreError::Format(format!(
            "region {region_x} contains an out-of-range chunk"
        )));
    }
    if header.pending_ticks.len() != header.chunk_xs.len() {
        return Err(StoreError::Format(format!(
            "region {region_x} has {} pending tick lists for {} chunks",
            header.pending_ticks.len(),
            header.chunk_xs.len()
        )));
    }
    if header.pending_ticks.iter().map(Vec::len).sum::<usize>() > MAX_PENDING_TICKS_PER_REGION {
        return Err(StoreError::Format(format!(
            "region {region_x} holds more than {MAX_PENDING_TICKS_PER_REGION} pending ticks"
        )));
    }

    let expected_blocks = header
        .chunk_xs
        .len()
        .checked_mul(CHUNK_AREA)
        .and_then(|size| size.checked_mul(2))
        .ok_or_else(|| StoreError::Format("dense chunk data length overflowed".into()))?;
    let dense_blocks = &decoded[header_end..];
    if dense_blocks.len() != expected_blocks {
        return Err(StoreError::Format(format!(
            "dense block array has {} entries; expected {expected_blocks}",
            dense_blocks.len()
        )));
    }

    let decode_layer = |encoded: &[u8]| -> Result<Vec<u8>, StoreError> {
        let mut blocks = Vec::with_capacity(CHUNK_AREA);
        for palette_code in encoded {
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
        Ok(blocks)
    };

    let mut chunks = Vec::with_capacity(header.chunk_xs.len());
    for ((chunk_x, encoded_ticks), encoded_chunk) in header
        .chunk_xs
        .iter()
        .copied()
        .zip(&header.pending_ticks)
        .zip(dense_blocks.chunks_exact(CHUNK_AREA * 2))
    {
        let (foreground, backwall) = encoded_chunk.split_at(CHUNK_AREA);
        chunks.push(ChunkSnapshot {
            x: chunk_x,
            foreground: decode_layer(foreground)?,
            backwall: decode_layer(backwall)?,
            pending_ticks: encoded_ticks
                .iter()
                .map(|tick| tick.tick())
                .collect::<Result<Vec<_>, _>>()?,
        });
    }
    Ok(chunks)
}

fn metadata_from_save(save: &WorldSnapshot) -> ScwMetadata {
    ScwMetadata {
        generator_version: save.generator_version,
        height: save.height,
        name: save.name.clone(),
        seed: save.seed,
        created_at_unix_s: save.created_at_unix_s,
        last_played_unix_s: save.last_played_unix_s,
        day_time_ticks: save.day_time_ticks,
        world_tick: save.world_tick,
        next_tick_sequence: save.next_tick_sequence,
        player_chunk_x: save.player.chunk_x,
        player_local_x: save.player.local_x,
        player_y: save.player.y,
        selected_slot: save.player.selected_slot,
    }
}

fn save_from_metadata(metadata: ScwMetadata, chunks: Vec<ChunkSnapshot>) -> WorldSnapshot {
    WorldSnapshot {
        generator_version: metadata.generator_version,
        height: metadata.height,
        name: metadata.name,
        seed: metadata.seed,
        created_at_unix_s: metadata.created_at_unix_s,
        last_played_unix_s: metadata.last_played_unix_s,
        day_time_ticks: metadata.day_time_ticks,
        world_tick: metadata.world_tick,
        next_tick_sequence: metadata.next_tick_sequence,
        player: PlayerSnapshot {
            chunk_x: metadata.player_chunk_x,
            local_x: metadata.player_local_x,
            y: metadata.player_y,
            selected_slot: metadata.selected_slot,
        },
        chunks,
    }
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

    fn tick(global_x: i64, due: u64, sequence: u64) -> ScheduledTick {
        ScheduledTick {
            due_world_tick: due,
            priority: MutationPriority::PLAYER,
            position: VoxelPos::foreground(global_x, 12),
            sequence,
            expected: Some(BlockState::STONE),
        }
    }

    fn example_chunk(x: i64) -> ChunkSnapshot {
        let mut foreground = vec![0; CHUNK_AREA];
        foreground[0] = BlockState::BEDROCK.code();
        foreground[1] = BlockState::BEDROCK.code();
        foreground[(2 * CHUNK_WIDTH + 3) as usize] = BlockState::TORCH.code();
        let mut backwall = vec![0; CHUNK_AREA];
        backwall[4] = BlockState::STONE.code();
        ChunkSnapshot {
            x,
            foreground,
            backwall,
            pending_ticks: Vec::new(),
        }
    }

    fn example_save() -> WorldSnapshot {
        blank_snapshot(
            7,
            "Example".into(),
            vec![example_chunk(0)],
            Vec2::new(2.5, 4.0),
            10,
        )
    }

    fn ticking_save() -> WorldSnapshot {
        let mut save = example_save();
        save.world_tick = 4_096;
        save.next_tick_sequence = 9;
        save.chunks[0].pending_ticks = vec![tick(5, 100, 3), tick(5, 100, 8), tick(9, 400, 1)];
        save
    }

    fn repository() -> (tempfile::TempDir, ScwRepository) {
        let directory = tempdir().unwrap();
        let store = ScwRepository::new(directory.path());
        (directory, store)
    }

    fn region_file_names(path: &Path) -> Vec<String> {
        let mut names = fs::read_dir(path)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("region-"))
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    #[test]
    fn round_trip_has_magic_compression_and_atomic_overwrite() {
        let (_directory, store) = repository();
        let id = WorldId::new("example").unwrap();
        let path = store.path_for(&id);
        let mut save = example_save();
        WorldRepository::save(&store, &id, &save).unwrap();
        assert!(path.is_dir());
        let bytes = fs::read(path.join(MANIFEST_FILE)).unwrap();
        assert_eq!(&bytes[..SCW_MAGIC.len()], SCW_MAGIC);
        assert!(bytes.len() < CHUNK_AREA);
        assert_eq!(WorldRepository::load(&store, &id).unwrap(), save);

        let regions = region_file_names(&path);
        save.name = "Updated".into();
        WorldRepository::save(&store, &id, &save).unwrap();
        assert_eq!(WorldRepository::load(&store, &id).unwrap().name, "Updated");
        assert_eq!(region_file_names(&path), regions, "blocks did not change");
    }

    #[test]
    fn pending_ticks_and_world_tick_round_trip() {
        let (_directory, store) = repository();
        let id = WorldId::new("ticking").unwrap();
        let save = ticking_save();
        WorldRepository::save(&store, &id, &save).unwrap();
        assert_eq!(WorldRepository::load(&store, &id).unwrap(), save);
    }

    #[test]
    fn a_second_save_of_a_reloaded_world_keeps_its_pending_ticks() {
        use crate::application::{SaveVersion, SessionInstanceId, WorldSession, WorldState};

        let (_directory, store) = repository();
        let id = WorldId::new("reloaded").unwrap();
        WorldRepository::save(&store, &id, &ticking_save()).unwrap();

        let reloaded = WorldRepository::load(&store, &id).unwrap();
        let world = WorldState::from_snapshot(&reloaded).state;
        let session = WorldSession {
            instance_id: SessionInstanceId::new(1),
            id: id.clone(),
            name: reloaded.name.clone(),
            seed: reloaded.seed,
            generator_version: reloaded.generator_version,
            created_at_unix_s: reloaded.created_at_unix_s,
            saved_version: SaveVersion::default(),
        };
        let resaved = world.snapshot(
            &session,
            Vec2::new(2.5, 4.0),
            1,
            reloaded.day_time_ticks,
            10,
        );
        WorldRepository::save(&store, &id, &resaved).unwrap();

        let twice = WorldRepository::load(&store, &id).unwrap();
        assert_eq!(
            twice.chunks[0].pending_ticks,
            ticking_save().chunks[0].pending_ticks
        );
        assert_eq!(twice.world_tick, 4_096);
        assert_eq!(twice.next_tick_sequence, 9);
    }

    #[test]
    fn payload_uses_two_dense_arrays_and_one_palette() {
        let save = example_save();
        let metadata = metadata_from_save(&save);
        let encoded = encode_region(0, &metadata, &save.chunks).unwrap();
        let decoded = decode_region(&encoded, 0, &metadata).unwrap();
        assert_eq!(decoded, save.chunks);

        let payload = decode_envelope(&encoded, MAX_DECOMPRESSED_BYTES).unwrap();
        let header_length = u32::from_le_bytes(payload[..4].try_into().unwrap()) as usize;
        let (header, _): (ScwRegionHeader, &[u8]) =
            postcard::take_from_bytes(&payload[4..4 + header_length]).unwrap();
        assert_eq!(payload.len() - 4 - header_length, CHUNK_AREA * 2);
        let mut expected = vec![
            ScwPaletteBlock::from_code(BlockState::TORCH.code()).unwrap(),
            ScwPaletteBlock::from_code(BlockState::STONE.code()).unwrap(),
            ScwPaletteBlock::from_code(BlockState::BEDROCK.code()).unwrap(),
        ];
        expected.sort();
        assert_eq!(header.palette, expected);
    }

    #[test]
    fn encoding_is_deterministic() {
        let (_first_directory, first) = repository();
        let (_second_directory, second) = repository();
        let id = WorldId::new("twice").unwrap();
        WorldRepository::save(&first, &id, &ticking_save()).unwrap();
        WorldRepository::save(&second, &id, &ticking_save()).unwrap();

        for file in region_file_names(&first.path_for(&id))
            .into_iter()
            .chain([MANIFEST_FILE.to_owned()])
        {
            assert_eq!(
                fs::read(first.path_for(&id).join(&file)).unwrap(),
                fs::read(second.path_for(&id).join(&file)).unwrap(),
                "{file} differs between identical saves"
            );
        }
    }

    #[test]
    fn generated_paths_do_not_collide() {
        let (_directory, store) = repository();
        let first = WorldRepository::next_available_id(&store, 7).unwrap();
        File::create(store.path_for(&first)).unwrap();
        let second = WorldRepository::next_available_id(&store, 7).unwrap();
        assert_eq!(second.as_str(), "world-0000000000000007-2");
    }

    #[test]
    fn list_is_sorted_reports_invalid_scw_and_ignores_json() {
        let (_directory, store) = repository();
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
    fn schema_six_is_exact_and_neighbouring_versions_are_rejected() {
        let save = blank_snapshot(1, "World".into(), Vec::new(), spawn_for_seed(1), 1);
        let encoded = encode_manifest(&ScwManifest {
            metadata: metadata_from_save(&save),
            regions: Vec::new(),
        })
        .unwrap();
        assert_eq!(
            u32::from_le_bytes(encoded[4..8].try_into().unwrap()),
            SAVE_SCHEMA_VERSION
        );

        for version in [5_u32, 7] {
            let mut stamped = encoded.clone();
            stamped[4..8].copy_from_slice(&version.to_le_bytes());
            assert!(matches!(
                decode_manifest(&stamped),
                Err(StoreError::UnsupportedSchema(found)) if found == version
            ));
        }
    }

    #[test]
    fn a_package_rejects_an_old_schema_before_reading_regions() {
        let (_directory, store) = repository();
        let id = WorldId::new("old-schema").unwrap();
        let package = store.path_for(&id);
        fs::create_dir_all(&package).unwrap();
        let mut manifest = encode_manifest(&ScwManifest {
            metadata: metadata_from_save(&example_save()),
            regions: vec![ScwRegionRef {
                region_x: 0,
                content_hash: 0,
                file_name: "region-0-0000000000000000.scw".into(),
            }],
        })
        .unwrap();
        manifest[4..8].copy_from_slice(&5_u32.to_le_bytes());
        fs::write(package.join(MANIFEST_FILE), manifest).unwrap();

        assert!(matches!(
            WorldRepository::load(&store, &id),
            Err(RepositoryError::UnsupportedSchema(5))
        ));
    }

    #[test]
    fn corrupted_envelopes_are_rejected() {
        assert!(matches!(
            decode_manifest(b"JSON{}"),
            Err(StoreError::Format(_))
        ));

        let save = example_save();
        let metadata = metadata_from_save(&save);
        let encoded = encode_region(0, &metadata, &save.chunks).unwrap();

        let mut flipped = encoded.clone();
        *flipped.last_mut().unwrap() ^= 0xff;
        assert!(matches!(
            decode_region(&flipped, 0, &metadata),
            Err(StoreError::Format(message)) if message.contains("checksum")
        ));

        let mut truncated = encoded.clone();
        truncated.truncate(encoded.len() / 2);
        assert!(decode_region(&truncated, 0, &metadata).is_err());

        let mut header_only = encoded;
        header_only.truncate(ENVELOPE_BYTES - 1);
        assert!(matches!(
            decode_region(&header_only, 0, &metadata),
            Err(StoreError::Format(message)) if message.contains("truncated")
        ));
    }

    #[test]
    fn region_identity_and_ordering_are_validated() {
        let save = example_save();
        let metadata = metadata_from_save(&save);
        let chunks = vec![example_chunk(0), example_chunk(1)];
        let encoded = encode_region(0, &metadata, &chunks).unwrap();

        assert!(decode_region(&encoded, 1, &metadata).is_err());
        let foreign = ScwMetadata {
            seed: metadata.seed + 1,
            ..metadata.clone()
        };
        assert!(decode_region(&encoded, 0, &foreign).is_err());

        let mut unsorted = chunks.clone();
        unsorted.swap(0, 1);
        let encoded = encode_region(0, &metadata, &unsorted).unwrap();
        assert!(decode_region(&encoded, 0, &metadata).is_err());

        let encoded = encode_region(0, &metadata, &[example_chunk(64)]).unwrap();
        assert!(decode_region(&encoded, 0, &metadata).is_err());

        let encoded = encode_region(0, &metadata, &[]).unwrap();
        assert!(decode_region(&encoded, 0, &metadata).is_err());
    }

    #[test]
    fn malformed_palettes_and_dense_arrays_are_rejected() {
        let mut save = example_save();
        save.chunks.push(save.chunks[0].clone());
        assert!(validate(&save).is_err());

        let save = example_save();
        let metadata = metadata_from_save(&save);
        let encoded = encode_region(0, &metadata, &save.chunks).unwrap();
        let payload = decode_envelope(&encoded, MAX_DECOMPRESSED_BYTES).unwrap();
        let header_length = u32::from_le_bytes(payload[..4].try_into().unwrap()) as usize;

        let mut broken = payload.clone();
        broken[4 + header_length + 10] = 200;
        assert!(decode_region(&encode_envelope(&broken).unwrap(), 0, &metadata).is_err());

        let mut short = payload;
        short.truncate(short.len() - 1);
        assert!(decode_region(&encode_envelope(&short).unwrap(), 0, &metadata).is_err());
    }

    #[test]
    fn invalid_pending_ticks_are_rejected() {
        let (_directory, store) = repository();
        let id = WorldId::new("bad-ticks").unwrap();

        let mut outside = ticking_save();
        outside.chunks[0].pending_ticks = vec![tick(64, 1, 0)];
        assert!(matches!(
            WorldRepository::save(&store, &id, &outside),
            Err(RepositoryError::InvalidSnapshot(_))
        ));

        let mut unsorted = ticking_save();
        unsorted.chunks[0].pending_ticks = vec![tick(5, 100, 8), tick(5, 100, 3)];
        assert!(WorldRepository::save(&store, &id, &unsorted).is_err());

        let mut ahead = ticking_save();
        ahead.next_tick_sequence = 1;
        assert!(WorldRepository::save(&store, &id, &ahead).is_err());

        let mut too_high = ticking_save();
        too_high.chunks[0].pending_ticks = vec![ScheduledTick {
            position: VoxelPos::foreground(5, WORLD_HEIGHT),
            ..tick(5, 1, 0)
        }];
        assert!(WorldRepository::save(&store, &id, &too_high).is_err());

        let unknown_layer = ScwScheduledTick {
            layer: 2,
            ..ScwScheduledTick::from_tick(tick(5, 1, 0))
        };
        assert!(unknown_layer.tick().is_err());
        let unknown_block = ScwScheduledTick {
            expected: 250,
            ..ScwScheduledTick::from_tick(tick(5, 1, 0))
        };
        assert!(unknown_block.tick().is_err());
    }

    #[test]
    fn package_partitions_chunks_into_regions_and_rejects_bare_region_files() {
        let (_directory, store) = repository();
        let mut save = example_save();
        save.chunks = [-65, -1, 0, 64].into_iter().map(example_chunk).collect();
        let id = WorldId::new("package").unwrap();
        let package_path = store.path_for(&id);
        WorldRepository::save(&store, &id, &save).unwrap();
        assert_eq!(WorldRepository::load(&store, &id).unwrap(), save);
        let manifest =
            decode_manifest(&fs::read(package_path.join(MANIFEST_FILE)).unwrap()).unwrap();
        assert_eq!(
            manifest
                .regions
                .iter()
                .map(|region| region.region_x)
                .collect::<Vec<_>>(),
            vec![-2, -1, 0, 1]
        );

        let bare = store.root().join("bare.scw");
        fs::write(
            &bare,
            encode_region(0, &metadata_from_save(&save), &[example_chunk(0)]).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            WorldRepository::load(&store, &WorldId::new("bare").unwrap()),
            Err(RepositoryError::InvalidSnapshot(_))
        ));
    }

    #[test]
    fn a_pending_tick_change_alone_republishes_the_region() {
        let (_directory, store) = repository();
        let id = WorldId::new("retick").unwrap();
        let mut save = ticking_save();
        WorldRepository::save(&store, &id, &save).unwrap();
        let before = region_file_names(&store.path_for(&id));

        save.chunks[0].pending_ticks.push(tick(9, 500, 2));
        WorldRepository::save(&store, &id, &save).unwrap();
        let after = region_file_names(&store.path_for(&id));

        assert_ne!(before, after);
        assert_eq!(after.len(), 1, "the superseded region was cleaned up");
        assert_eq!(WorldRepository::load(&store, &id).unwrap(), save);
    }

    #[test]
    fn a_tampered_region_fails_its_manifest_hash() {
        let (_directory, store) = repository();
        let id = WorldId::new("tampered").unwrap();
        let save = example_save();
        WorldRepository::save(&store, &id, &save).unwrap();

        let package = store.path_for(&id);
        let name = region_file_names(&package).remove(0);
        let mut chunks = save.chunks.clone();
        chunks[0].foreground[7] = BlockState::DIRT.code();
        fs::write(
            package.join(&name),
            encode_region(0, &metadata_from_save(&save), &chunks).unwrap(),
        )
        .unwrap();

        assert!(matches!(
            WorldRepository::load(&store, &id),
            Err(RepositoryError::Corrupt(message)) if message.contains("content hash")
        ));
    }

    #[test]
    fn large_horizontal_coordinates_round_trip_without_float_precision_loss() {
        let (_directory, store) = repository();
        let mut save = ticking_save();
        save.player.chunk_x = 4_000_000_000_000;
        save.player.local_x = 31.75;
        let far = 4_000_000_000_000 * i64::from(CHUNK_WIDTH);
        save.chunks[0].x = -4_000_000_000_000;
        save.chunks[0].pending_ticks = vec![tick(-far, 3, 0)];
        let id = WorldId::new("far-away").unwrap();

        WorldRepository::save(&store, &id, &save).unwrap();

        assert_eq!(WorldRepository::load(&store, &id).unwrap(), save);
    }

    #[test]
    fn large_absolute_day_time_round_trips_exactly() {
        let (_directory, store) = repository();
        let id = WorldId::new("late").unwrap();
        let mut save = example_save();
        save.day_time_ticks = u64::MAX - 17;
        save.world_tick = u64::MAX - 3;
        WorldRepository::save(&store, &id, &save).unwrap();
        let loaded = WorldRepository::load(&store, &id).unwrap();
        assert_eq!(loaded.day_time_ticks, u64::MAX - 17);
        assert_eq!(loaded.world_tick, u64::MAX - 3);
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
    fn palette_entries_encode_ids_and_variants() {
        let grass = ScwPaletteBlock::from_code(BlockState::GRASS.code()).unwrap();
        let torch = ScwPaletteBlock::from_code(BlockState::TORCH.code()).unwrap();
        let wall_torch = ScwPaletteBlock::from_code(BlockState::WALL_TORCH_LEFT.code()).unwrap();
        assert_eq!(postcard::to_allocvec(&grass).unwrap(), [1, 0]);
        assert_eq!(postcard::to_allocvec(&torch).unwrap(), [8, 0]);
        assert_eq!(postcard::to_allocvec(&wall_torch).unwrap(), [8, 1]);
    }

    #[test]
    fn region_hash_is_separated_by_schema() {
        let chunks = ticking_save().chunks;
        assert_ne!(
            hash_region_for_schema(&chunks, 5),
            hash_region_for_schema(&chunks, 6)
        );
    }
}
