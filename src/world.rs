use crate::camera::{CameraRig, GameCamera, center_camera};
use crate::lighting::{DayCycle, LightGrid};
use crate::persistence::{SavedChunk, WorldSaveV1};
use crate::player::{Player, RespawnPoint};
use crate::rendering::{RenderCatalog, build_chunk_collider, build_chunk_meshes};
use crate::{AppState, BlockKind, CHUNK_WIDTH, DEPTH_SLICES, WORLD_HEIGHT};
use avian2d::prelude::{Collider, RigidBody};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;

pub const LOAD_RADIUS_CHUNKS: i32 = 3;
pub const UNLOAD_RADIUS_CHUNKS: i32 = 5;
const MAX_GENERATION_TASKS: usize = 4;
const MAX_CHUNK_INTEGRATIONS_PER_FRAME: usize = 2;
const REBASE_THRESHOLD_CHUNKS: i32 = 8;
const CHUNK_AREA: usize = (CHUNK_WIDTH * WORLD_HEIGHT) as usize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockChunk {
    x: i32,
    blocks: Vec<u8>,
    dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockGrid {
    height: i32,
    chunks: HashMap<i32, BlockChunk>,
}

#[derive(Resource)]
pub struct WorldData {
    pub grid: BlockGrid,
    chunk_scenes: HashMap<i32, ChunkScene>,
    render_dirty: HashSet<i32>,
    pub persisted_chunks: BTreeMap<i64, Vec<u8>>,
    pub origin_chunk: i64,
    pub revision: u64,
    pub seed: u64,
    pub(crate) lighting_dirty: bool,
}

#[derive(Resource, Debug, Clone)]
pub struct WorldSession {
    pub path: PathBuf,
    pub name: String,
    pub seed: u64,
    pub generator_version: u32,
    pub created_at_unix_s: u64,
    pub saved_revision: u64,
}

#[derive(Resource)]
pub struct PendingWorld {
    pub path: PathBuf,
    pub save: WorldSaveV1,
}

#[derive(Component)]
pub struct WorldEntity;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkCoordinate(pub i32);

struct ChunkScene {
    root: Entity,
    layers: [Option<ChunkRenderLayer>; 3],
}

struct ChunkRenderLayer {
    entity: Entity,
    mesh: Handle<Mesh>,
}

#[derive(Resource, Default)]
struct RetiredMeshAssets {
    pending: Vec<Handle<Mesh>>,
    generations: VecDeque<Vec<Handle<Mesh>>>,
}

#[derive(Component)]
struct ChunkGenerationTask {
    chunk_x: i32,
    origin_chunk: i64,
    task: Task<BlockChunk>,
}

type RebaseEntityQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Transform, Option<&'static mut ChunkCoordinate>),
    (With<WorldEntity>, Without<Player>, Without<GameCamera>),
>;

impl BlockChunk {
    pub fn generated(seed: u64, chunk_x: i32) -> Self {
        generate_chunk(seed, chunk_x)
    }

    pub fn from_dense(chunk_x: i32, blocks: Vec<u8>) -> Option<Self> {
        if blocks.len() != CHUNK_AREA
            || blocks
                .iter()
                .any(|code| *code != 0 && BlockKind::from_code(*code).is_none())
        {
            return None;
        }
        Some(Self {
            x: chunk_x,
            blocks,
            dirty: false,
        })
    }

    pub fn x(&self) -> i32 {
        self.x
    }

    pub fn blocks(&self) -> &[u8] {
        &self.blocks
    }

    fn get(&self, local_x: i32, y: i32) -> Option<BlockKind> {
        BlockKind::from_code(self.blocks[chunk_index(local_x, y)])
    }

    fn set(&mut self, local_x: i32, y: i32, kind: Option<BlockKind>, dirty: bool) {
        self.blocks[chunk_index(local_x, y)] = kind.map_or(0, BlockKind::code);
        self.dirty |= dirty;
    }
}

impl BlockGrid {
    pub fn new(height: i32) -> Self {
        Self {
            height,
            chunks: HashMap::new(),
        }
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    pub fn in_bounds(&self, coordinate: IVec2) -> bool {
        (0..self.height).contains(&coordinate.y)
    }

    pub fn contains_chunk(&self, chunk_x: i32) -> bool {
        self.chunks.contains_key(&chunk_x)
    }

    pub fn loaded_chunks(&self) -> impl Iterator<Item = i32> + '_ {
        self.chunks.keys().copied()
    }

    pub fn loaded_x_bounds(&self) -> Option<(i32, i32)> {
        let minimum = self.chunks.keys().copied().min()? * CHUNK_WIDTH;
        let maximum = (self.chunks.keys().copied().max()? + 1) * CHUNK_WIDTH;
        Some((minimum, maximum))
    }

    pub fn insert_chunk(&mut self, chunk: BlockChunk) -> Option<BlockChunk> {
        self.chunks.insert(chunk.x, chunk)
    }

    pub fn remove_chunk(&mut self, chunk_x: i32) -> Option<BlockChunk> {
        self.chunks.remove(&chunk_x)
    }

    pub fn get(&self, coordinate: IVec2) -> Option<BlockKind> {
        if !self.in_bounds(coordinate) {
            return None;
        }
        let chunk_x = world_to_chunk(coordinate.x);
        let local_x = coordinate.x.rem_euclid(CHUNK_WIDTH);
        self.chunks.get(&chunk_x)?.get(local_x, coordinate.y)
    }

    pub fn set(&mut self, coordinate: IVec2, kind: BlockKind) -> Option<BlockKind> {
        assert!(
            self.in_bounds(coordinate),
            "tile coordinate is vertically out of bounds"
        );
        let chunk_x = world_to_chunk(coordinate.x);
        let local_x = coordinate.x.rem_euclid(CHUNK_WIDTH);
        let chunk = self
            .chunks
            .get_mut(&chunk_x)
            .expect("the target chunk must be loaded before editing");
        let previous = chunk.get(local_x, coordinate.y);
        chunk.set(local_x, coordinate.y, Some(kind), true);
        previous
    }

    pub fn remove(&mut self, coordinate: IVec2) -> Option<BlockKind> {
        if !self.in_bounds(coordinate) {
            return None;
        }
        let chunk_x = world_to_chunk(coordinate.x);
        let local_x = coordinate.x.rem_euclid(CHUNK_WIDTH);
        let chunk = self.chunks.get_mut(&chunk_x)?;
        let previous = chunk.get(local_x, coordinate.y);
        if previous.is_some() {
            chunk.set(local_x, coordinate.y, None, true);
        }
        previous
    }

    pub fn iter(&self) -> impl Iterator<Item = (IVec2, BlockKind)> + '_ {
        self.chunks.values().flat_map(|chunk| {
            chunk
                .blocks
                .iter()
                .enumerate()
                .filter_map(move |(index, code)| {
                    let kind = BlockKind::from_code(*code)?;
                    let local_x = index as i32 % CHUNK_WIDTH;
                    let y = index as i32 / CHUNK_WIDTH;
                    Some((IVec2::new(chunk.x * CHUNK_WIDTH + local_x, y), kind))
                })
        })
    }

    pub fn from_save(save: &WorldSaveV1) -> Self {
        let mut grid = Self::new(save.height);
        let origin = if save
            .chunks
            .iter()
            .all(|chunk| i32::try_from(chunk.x).is_ok())
        {
            0
        } else {
            save.chunks.first().map_or(0, |chunk| chunk.x)
        };
        for saved in &save.chunks {
            let local_chunk = i32::try_from(saved.x - origin)
                .expect("resident save span must fit local chunk coordinates");
            let chunk = BlockChunk::from_dense(local_chunk, saved.blocks.clone())
                .expect("validated saves contain valid dense chunks");
            grid.insert_chunk(chunk);
        }
        grid
    }

    pub fn safe_spawn(&self) -> Vec2 {
        for distance in 0..CHUNK_WIDTH {
            for x in [distance, -distance] {
                for y in (0..self.height - 2).rev() {
                    if self.get(IVec2::new(x, y)) == Some(BlockKind::Grass)
                        && self.get(IVec2::new(x, y + 1)).is_none()
                        && self.get(IVec2::new(x, y + 2)).is_none()
                    {
                        return Vec2::new(x as f32 + 0.5, y as f32 + 1.9);
                    }
                }
            }
        }
        Vec2::new(0.5, self.height as f32 - 2.0)
    }

    pub fn player_position_is_safe(&self, position: Vec2) -> bool {
        if !position.is_finite() || position.y < 0.9 || position.y > self.height as f32 {
            return false;
        }
        let min_tile = (position - Vec2::new(0.34, 0.88)).floor().as_ivec2();
        let max_tile = (position + Vec2::new(0.34, 0.88)).floor().as_ivec2();
        if !self.contains_chunk(world_to_chunk(min_tile.x))
            || !self.contains_chunk(world_to_chunk(max_tile.x))
        {
            return false;
        }
        for x in min_tile.x..=max_tile.x {
            for y in min_tile.y..=max_tile.y {
                if self
                    .get(IVec2::new(x, y))
                    .is_some_and(|kind| kind.def().solid)
                {
                    return false;
                }
            }
        }
        true
    }

    fn chunk_snapshot(&self, chunk_x: i32) -> Option<Vec<u8>> {
        self.chunks.get(&chunk_x).map(|chunk| chunk.blocks.clone())
    }

    fn rebase(&mut self, delta_chunks: i32) {
        self.chunks = self
            .chunks
            .drain()
            .map(|(chunk_x, mut chunk)| {
                let rebased = chunk_x - delta_chunks;
                chunk.x = rebased;
                (rebased, chunk)
            })
            .collect();
    }
}

pub const fn world_to_chunk(world_x: i32) -> i32 {
    world_x.div_euclid(CHUNK_WIDTH)
}

fn chunk_index(local_x: i32, y: i32) -> usize {
    (y * CHUNK_WIDTH + local_x) as usize
}

pub fn stable_hash(seed: u64, x: i64, y: i32, salt: u64) -> u64 {
    let mut value = seed
        ^ (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (y as i64 as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9)
        ^ salt;
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

pub fn stable_hash_3d(seed: u64, x: i64, y: i32, depth: i32, salt: u64) -> u64 {
    stable_hash(
        seed ^ (depth as i64 as u64).wrapping_mul(0xd6e8_feb8_6659_fd93),
        x,
        y,
        salt,
    )
}

pub fn surface_height(seed: u64, x: i64) -> i32 {
    surface_height_at_depth(seed, x, 0)
}

fn smooth_step(value: f32) -> f32 {
    value * value * (3.0 - 2.0 * value)
}

fn lattice_noise(seed: u64, x: i64, depth: i32, salt: u64) -> f32 {
    let hash = stable_hash_3d(seed, x, 0, depth, salt);
    hash as f64 as f32 / u64::MAX as f32 * 2.0 - 1.0
}

fn value_noise_2d(
    seed: u64,
    x: i64,
    depth: i32,
    x_period: i64,
    depth_period: i32,
    salt: u64,
) -> f32 {
    let cell_x = x.div_euclid(x_period);
    let cell_depth = depth.div_euclid(depth_period);
    let fx = smooth_step(x.rem_euclid(x_period) as f32 / x_period as f32);
    let fz = smooth_step(depth.rem_euclid(depth_period) as f32 / depth_period as f32);
    let near_left = lattice_noise(seed, cell_x, cell_depth, salt);
    let near_right = lattice_noise(seed, cell_x + 1, cell_depth, salt);
    let far_left = lattice_noise(seed, cell_x, cell_depth + 1, salt);
    let far_right = lattice_noise(seed, cell_x + 1, cell_depth + 1, salt);
    let near = near_left + (near_right - near_left) * fx;
    let far = far_left + (far_right - far_left) * fx;
    near + (far - near) * fz
}

pub fn surface_height_at_depth(seed: u64, x: i64, depth: u8) -> i32 {
    let depth = i32::from(depth);
    let coarse = value_noise_2d(seed, x, depth, 16, 4, 11) * 7.0;
    let detail = value_noise_2d(seed, x, depth, 4, 2, 29) * 2.0;
    (38.0 + coarse + detail).round().clamp(20.0, 58.0) as i32
}

fn tree_root(seed: u64, x: i64, depth: i32) -> bool {
    x.abs() >= 5 && stable_hash_3d(seed, x, 0, depth, 313).is_multiple_of(29)
}

pub fn generated_voxel(seed: u64, x: i64, y: i32, depth: u8) -> Option<BlockKind> {
    if !(0..WORLD_HEIGHT).contains(&y) || depth >= DEPTH_SLICES {
        return None;
    }

    let depth_i32 = i32::from(depth);
    let surface = surface_height_at_depth(seed, x, depth);
    if y > surface {
        for root_depth in depth_i32 - 2..=depth_i32 + 2 {
            if root_depth < 0 {
                continue;
            }
            for root_x in x - 2..=x + 2 {
                if !tree_root(seed, root_x, root_depth) {
                    continue;
                }
                let root_y = surface_height_at_depth(seed, root_x, root_depth as u8) + 1;
                if x == root_x && depth_i32 == root_depth && (root_y..root_y + 3).contains(&y) {
                    return Some(BlockKind::Wood);
                }
                let canopy_distance = (x - root_x).abs()
                    + i64::from((depth_i32 - root_depth).abs())
                    + i64::from((y - (root_y + 3)).abs());
                if canopy_distance <= 3 && (root_y + 2..=root_y + 5).contains(&y) {
                    return Some(BlockKind::Leaves);
                }
            }
        }
        return None;
    }

    if y == 0 {
        return Some(BlockKind::Bedrock);
    }
    if y == surface {
        return Some(BlockKind::Grass);
    }
    if y >= surface - 4 {
        return Some(BlockKind::Dirt);
    }

    let cave_cell = stable_hash_3d(seed, x.div_euclid(4), y.div_euclid(4), depth_i32, 401) % 1000;
    if y > 4 && y < surface - 5 && cave_cell < 105 {
        return None;
    }

    let ore = stable_hash_3d(seed, x, y, depth_i32, 101) % 1000;
    if y < 28 && ore < 18 {
        Some(BlockKind::IronOre)
    } else if y < 36 && ore < 55 {
        Some(BlockKind::CoalOre)
    } else {
        Some(BlockKind::Stone)
    }
}

pub fn generate_chunk(seed: u64, chunk_x: i32) -> BlockChunk {
    generate_chunk_at(seed, i64::from(chunk_x), chunk_x)
}

fn generate_chunk_at(seed: u64, global_chunk_x: i64, local_chunk_x: i32) -> BlockChunk {
    let mut chunk = BlockChunk {
        x: local_chunk_x,
        blocks: vec![0; CHUNK_AREA],
        dirty: false,
    };
    let start_x = global_chunk_x * i64::from(CHUNK_WIDTH);
    let end_x = start_x + i64::from(CHUNK_WIDTH);
    for world_x in start_x..end_x {
        let local_x = world_x.rem_euclid(i64::from(CHUNK_WIDTH)) as i32;
        for y in 0..WORLD_HEIGHT {
            if let Some(kind) = generated_voxel(seed, world_x, y, 0) {
                chunk.set(local_x, y, Some(kind), false);
            }
        }
    }
    chunk
}

pub fn generate_world(seed: u64) -> BlockGrid {
    let mut grid = BlockGrid::new(WORLD_HEIGHT);
    for chunk_x in -1..=1 {
        grid.insert_chunk(generate_chunk(seed, chunk_x));
    }
    grid
}

pub fn spawn_for_seed(seed: u64) -> Vec2 {
    generate_world(seed).safe_spawn()
}

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RetiredMeshAssets>()
            .add_systems(OnEnter(AppState::LoadingWorld), spawn_pending_world)
            .add_systems(
                Update,
                finish_loading_world.run_if(in_state(AppState::LoadingWorld)),
            )
            .add_systems(
                Update,
                (
                    rebase_world,
                    request_chunk_generation,
                    integrate_generated_chunks,
                    unload_distant_chunks,
                    refresh_lighting,
                )
                    .chain()
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(PostUpdate, refresh_chunk_scenes)
            .add_systems(OnEnter(AppState::MainMenu), cleanup_world)
            .add_systems(Last, age_retired_meshes);
    }
}

fn spawn_pending_world(
    mut commands: Commands,
    pending: Option<Res<PendingWorld>>,
    catalog: Option<Res<RenderCatalog>>,
) {
    let (Some(pending), Some(_catalog)) = (pending, catalog) else {
        return;
    };
    let persisted_chunks = pending
        .save
        .chunks
        .iter()
        .map(|chunk| (chunk.x, chunk.blocks.clone()))
        .collect::<BTreeMap<_, _>>();
    let center_chunk = world_to_chunk(pending.save.player.local_x.floor() as i32);
    let origin_chunk = pending.save.player.chunk_x;
    let initial = persisted_chunks
        .get(&(origin_chunk + i64::from(center_chunk)))
        .cloned()
        .and_then(|blocks| BlockChunk::from_dense(center_chunk, blocks))
        .unwrap_or_else(|| {
            generate_chunk_at(
                pending.save.seed,
                origin_chunk + i64::from(center_chunk),
                center_chunk,
            )
        });
    let mut grid = BlockGrid::new(WORLD_HEIGHT);
    grid.insert_chunk(initial);
    let mut render_dirty = HashSet::new();
    render_dirty.insert(center_chunk);
    let world = WorldData {
        grid,
        chunk_scenes: HashMap::new(),
        render_dirty,
        persisted_chunks,
        origin_chunk,
        revision: 0,
        seed: pending.save.seed,
        lighting_dirty: false,
    };
    commands.insert_resource(LightGrid::calculate(&world.grid));
    commands.insert_resource(world);
    commands.insert_resource(WorldSession {
        path: pending.path.clone(),
        name: pending.save.name.clone(),
        seed: pending.save.seed,
        generator_version: pending.save.generator_version,
        created_at_unix_s: pending.save.created_at_unix_s,
        saved_revision: 0,
    });
}

fn finish_loading_world(
    world: Option<Res<WorldData>>,
    pending: Option<Res<PendingWorld>>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if world.is_some() && pending.is_some() {
        next_state.set(AppState::Playing);
    }
}

#[allow(clippy::too_many_arguments)]
fn rebase_world(
    mut commands: Commands,
    mut world: Option<ResMut<WorldData>>,
    player: Option<Single<(&mut Transform, &mut RespawnPoint), With<Player>>>,
    mut entities: RebaseEntityQuery,
    tasks: Query<Entity, With<ChunkGenerationTask>>,
    mut camera: Single<&mut Transform, (With<GameCamera>, Without<Player>)>,
    mut camera_rig: ResMut<CameraRig>,
) {
    let (Some(world), Some(mut player)) = (world.as_mut(), player) else {
        return;
    };
    let delta_chunks = world_to_chunk(player.0.translation.x.floor() as i32);
    if delta_chunks.abs() < REBASE_THRESHOLD_CHUNKS {
        return;
    }
    let offset = (delta_chunks * CHUNK_WIDTH) as f32;
    player.0.translation.x -= offset;
    player.1.0.x -= offset;
    for (mut transform, chunk) in &mut entities {
        transform.translation.x -= offset;
        if let Some(mut chunk) = chunk {
            chunk.0 -= delta_chunks;
        }
    }
    for entity in &tasks {
        commands.entity(entity).despawn();
    }

    world.grid.rebase(delta_chunks);
    world.chunk_scenes = std::mem::take(&mut world.chunk_scenes)
        .into_iter()
        .map(|(chunk_x, scene)| (chunk_x - delta_chunks, scene))
        .collect();
    world.render_dirty = std::mem::take(&mut world.render_dirty)
        .into_iter()
        .map(|chunk_x| chunk_x - delta_chunks)
        .collect();
    world.origin_chunk += i64::from(delta_chunks);
    world.lighting_dirty = true;
    center_camera(
        player.0.translation.truncate(),
        &mut camera,
        &mut camera_rig,
    );
}

fn request_chunk_generation(
    mut commands: Commands,
    world: Option<Res<WorldData>>,
    player: Option<Single<&Transform, With<Player>>>,
    tasks: Query<&ChunkGenerationTask>,
) {
    let (Some(world), Some(player)) = (world, player) else {
        return;
    };
    let mut running = tasks
        .iter()
        .filter(|task| task.origin_chunk == world.origin_chunk)
        .map(|task| task.chunk_x)
        .collect::<HashSet<_>>();
    let mut available = MAX_GENERATION_TASKS.saturating_sub(running.len());
    if available == 0 {
        return;
    }
    let center = world_to_chunk(player.translation.x.floor() as i32);
    let pool = AsyncComputeTaskPool::get();
    for distance in 0..=LOAD_RADIUS_CHUNKS {
        for chunk_x in [center + distance, center - distance] {
            if available == 0 {
                return;
            }
            if world.grid.contains_chunk(chunk_x) || running.contains(&chunk_x) {
                continue;
            }
            let seed = world.seed;
            let global_chunk_x = world.origin_chunk + i64::from(chunk_x);
            let persisted = world.persisted_chunks.get(&global_chunk_x).cloned();
            let task = pool.spawn(async move {
                persisted
                    .and_then(|blocks| BlockChunk::from_dense(chunk_x, blocks))
                    .unwrap_or_else(|| generate_chunk_at(seed, global_chunk_x, chunk_x))
            });
            commands.spawn(ChunkGenerationTask {
                chunk_x,
                origin_chunk: world.origin_chunk,
                task,
            });
            running.insert(chunk_x);
            available -= 1;
        }
    }
}

fn integrate_generated_chunks(
    mut commands: Commands,
    mut world: Option<ResMut<WorldData>>,
    player: Option<Single<&Transform, With<Player>>>,
    mut tasks: Query<(Entity, &mut ChunkGenerationTask)>,
) {
    let (Some(world), Some(player)) = (world.as_mut(), player) else {
        return;
    };
    let center = world_to_chunk(player.translation.x.floor() as i32);
    let mut integrated = 0;
    for (entity, mut generation) in &mut tasks {
        if integrated >= MAX_CHUNK_INTEGRATIONS_PER_FRAME {
            break;
        }
        let Some(chunk) = check_ready(&mut generation.task) else {
            continue;
        };
        commands.entity(entity).despawn();
        if generation.origin_chunk != world.origin_chunk {
            continue;
        }
        if (chunk.x - center).abs() > LOAD_RADIUS_CHUNKS || world.grid.contains_chunk(chunk.x) {
            continue;
        }
        let chunk_x = chunk.x;
        world.grid.insert_chunk(chunk);
        world.render_dirty.insert(chunk_x);
        world.lighting_dirty = true;
        integrated += 1;
    }
}

fn unload_distant_chunks(
    mut commands: Commands,
    mut world: Option<ResMut<WorldData>>,
    player: Option<Single<&Transform, With<Player>>>,
    mut retired: ResMut<RetiredMeshAssets>,
) {
    let (Some(world), Some(player)) = (world.as_mut(), player) else {
        return;
    };
    let center = world_to_chunk(player.translation.x.floor() as i32);
    let unload = world
        .grid
        .loaded_chunks()
        .filter(|chunk_x| (*chunk_x - center).abs() > UNLOAD_RADIUS_CHUNKS)
        .collect::<Vec<_>>();
    for chunk_x in unload {
        if let Some(chunk) = world.grid.remove_chunk(chunk_x)
            && chunk.dirty
        {
            let global_chunk_x = world.origin_chunk + i64::from(chunk_x);
            world.persisted_chunks.insert(global_chunk_x, chunk.blocks);
        }
        despawn_chunk_entities(&mut commands, world, &mut retired, chunk_x);
        world.lighting_dirty = true;
    }
}

fn refresh_lighting(mut commands: Commands, mut world: Option<ResMut<WorldData>>) {
    let Some(world) = world.as_mut() else {
        return;
    };
    if world.lighting_dirty {
        commands.insert_resource(LightGrid::calculate(&world.grid));
        world.lighting_dirty = false;
    }
}

fn refresh_chunk_scenes(
    mut commands: Commands,
    catalog: Option<Res<RenderCatalog>>,
    light: Option<Res<LightGrid>>,
    day: Res<DayCycle>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut retired: ResMut<RetiredMeshAssets>,
    mut world: Option<ResMut<WorldData>>,
) {
    let (Some(catalog), Some(light), Some(world)) = (catalog, light, world.as_mut()) else {
        return;
    };
    let chunks = world.render_dirty.drain().collect::<HashSet<_>>();
    let mut chunks = chunks
        .into_iter()
        .filter(|chunk_x| world.grid.contains_chunk(*chunk_x))
        .collect::<Vec<_>>();
    chunks.sort_unstable();

    for chunk_x in chunks {
        let rebuilt = build_chunk_meshes(
            &world.grid,
            chunk_x,
            world.origin_chunk + i64::from(chunk_x),
            world.seed,
            &light,
            &day,
        );
        let collider = build_chunk_collider(&world.grid, chunk_x);
        let chunk_transform = Transform::from_xyz((chunk_x * CHUNK_WIDTH) as f32, 0.0, 0.0);
        if let Some(scene) = world.chunk_scenes.get_mut(&chunk_x) {
            update_chunk_layer(
                &mut commands,
                &mut meshes,
                &mut retired,
                &mut scene.layers[0],
                rebuilt.opaque,
                &catalog.opaque_material,
                chunk_transform,
                chunk_x,
            );
            update_chunk_layer(
                &mut commands,
                &mut meshes,
                &mut retired,
                &mut scene.layers[1],
                rebuilt.cutout,
                &catalog.cutout_material,
                chunk_transform,
                chunk_x,
            );
            update_chunk_layer(
                &mut commands,
                &mut meshes,
                &mut retired,
                &mut scene.layers[2],
                rebuilt.emissive,
                &catalog.emissive_material,
                chunk_transform,
                chunk_x,
            );
            if let Some(collider) = collider {
                commands.entity(scene.root).insert(collider);
            } else {
                commands.entity(scene.root).remove::<Collider>();
            }
            continue;
        }

        let root = commands
            .spawn((
                Transform::from_xyz((chunk_x * CHUNK_WIDTH) as f32, 0.0, 0.0),
                RigidBody::Static,
                ChunkCoordinate(chunk_x),
                WorldEntity,
            ))
            .id();
        if let Some(collider) = collider {
            commands.entity(root).insert(collider);
        }
        let mut scene = ChunkScene {
            root,
            layers: [None, None, None],
        };
        update_chunk_layer(
            &mut commands,
            &mut meshes,
            &mut retired,
            &mut scene.layers[0],
            rebuilt.opaque,
            &catalog.opaque_material,
            chunk_transform,
            chunk_x,
        );
        update_chunk_layer(
            &mut commands,
            &mut meshes,
            &mut retired,
            &mut scene.layers[1],
            rebuilt.cutout,
            &catalog.cutout_material,
            chunk_transform,
            chunk_x,
        );
        update_chunk_layer(
            &mut commands,
            &mut meshes,
            &mut retired,
            &mut scene.layers[2],
            rebuilt.emissive,
            &catalog.emissive_material,
            chunk_transform,
            chunk_x,
        );
        world.chunk_scenes.insert(chunk_x, scene);
    }
}

#[allow(clippy::too_many_arguments)]
fn update_chunk_layer(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    retired: &mut RetiredMeshAssets,
    layer: &mut Option<ChunkRenderLayer>,
    mesh: Mesh,
    material: &Handle<StandardMaterial>,
    transform: Transform,
    chunk_x: i32,
) {
    if mesh.count_vertices() == 0 {
        if let Some(previous) = layer.take() {
            commands.entity(previous.entity).despawn();
            retired.pending.push(previous.mesh);
        }
        return;
    }

    let mesh = meshes.add(mesh);
    if let Some(previous) = layer {
        commands
            .entity(previous.entity)
            .insert(Mesh3d(mesh.clone()));
        retired
            .pending
            .push(std::mem::replace(&mut previous.mesh, mesh));
    } else {
        let entity = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                transform,
                ChunkCoordinate(chunk_x),
                WorldEntity,
            ))
            .id();
        *layer = Some(ChunkRenderLayer { entity, mesh });
    }
}

fn mark_chunk_render_dirty(world: &mut WorldData, world_x: i32) {
    let chunk_x = world_to_chunk(world_x);
    world.render_dirty.insert(chunk_x);
    let local_x = world_x.rem_euclid(CHUNK_WIDTH);
    if local_x == 0 {
        world.render_dirty.insert(chunk_x - 1);
    } else if local_x == CHUNK_WIDTH - 1 {
        world.render_dirty.insert(chunk_x + 1);
    }
}

fn despawn_chunk_entities(
    commands: &mut Commands,
    world: &mut WorldData,
    retired: &mut RetiredMeshAssets,
    chunk_x: i32,
) {
    if let Some(scene) = world.chunk_scenes.remove(&chunk_x) {
        commands.entity(scene.root).despawn();
        for layer in scene.layers.into_iter().flatten() {
            commands.entity(layer.entity).despawn();
            retired.pending.push(layer.mesh);
        }
    }
    world.render_dirty.remove(&chunk_x);
}

fn cleanup_world(
    mut commands: Commands,
    world: Option<Res<WorldData>>,
    mut retired: ResMut<RetiredMeshAssets>,
    entities: Query<Entity, With<WorldEntity>>,
    tasks: Query<Entity, With<ChunkGenerationTask>>,
) {
    if let Some(world) = world {
        for scene in world.chunk_scenes.values() {
            retired.pending.extend(
                scene
                    .layers
                    .iter()
                    .flatten()
                    .map(|layer| layer.mesh.clone()),
            );
        }
    }
    for entity in &entities {
        commands.entity(entity).despawn();
    }
    for entity in &tasks {
        commands.entity(entity).despawn();
    }
    commands.remove_resource::<WorldData>();
    commands.remove_resource::<WorldSession>();
}

fn age_retired_meshes(mut retired: ResMut<RetiredMeshAssets>) {
    let pending = std::mem::take(&mut retired.pending);
    retired.generations.push_back(pending);
    while retired.generations.len() > 16 {
        retired.generations.pop_front();
    }
}

impl WorldData {
    #[cfg(test)]
    pub(crate) fn for_test(grid: BlockGrid, seed: u64) -> Self {
        Self {
            grid,
            chunk_scenes: HashMap::new(),
            render_dirty: HashSet::new(),
            persisted_chunks: BTreeMap::new(),
            origin_chunk: 0,
            revision: 0,
            seed,
            lighting_dirty: false,
        }
    }

    pub fn saved_chunks(&self) -> Vec<SavedChunk> {
        let mut chunks = self.persisted_chunks.clone();
        for chunk_x in self.grid.loaded_chunks() {
            if self
                .grid
                .chunks
                .get(&chunk_x)
                .is_some_and(|chunk| chunk.dirty)
                && let Some(blocks) = self.grid.chunk_snapshot(chunk_x)
            {
                chunks.insert(self.origin_chunk + i64::from(chunk_x), blocks);
            }
        }
        chunks
            .into_iter()
            .map(|(x, blocks)| SavedChunk { x, blocks })
            .collect()
    }
}

pub fn remove_tile(world: &mut WorldData, coordinate: IVec2) -> Vec<(IVec2, BlockKind)> {
    let mut removed = Vec::new();
    if let Some(kind) = world.grid.remove(coordinate) {
        removed.push((coordinate, kind));
        let above = coordinate + IVec2::Y;
        if world.grid.get(above) == Some(BlockKind::Torch) {
            world.grid.remove(above);
            removed.push((above, BlockKind::Torch));
        }
        mark_chunk_render_dirty(world, coordinate.x);
        world.revision = world.revision.wrapping_add(1);
        world.lighting_dirty = true;
    }
    removed
}

pub fn place_tile(world: &mut WorldData, coordinate: IVec2, kind: BlockKind) -> bool {
    if !world.grid.in_bounds(coordinate)
        || !world.grid.contains_chunk(world_to_chunk(coordinate.x))
        || world.grid.get(coordinate).is_some()
    {
        return false;
    }
    if kind == BlockKind::Torch
        && !world
            .grid
            .get(coordinate - IVec2::Y)
            .is_some_and(|support| support.def().solid)
    {
        return false;
    }
    world.grid.set(coordinate, kind);
    mark_chunk_render_dirty(world, coordinate.x);
    world.revision = world.revision.wrapping_add(1);
    world.lighting_dirty = true;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_repeatable_across_positive_and_negative_chunks() {
        for chunk_x in [-10, -1, 0, 1, 10] {
            assert_eq!(generate_chunk(123, chunk_x), generate_chunk(123, chunk_x));
            assert_ne!(generate_chunk(123, chunk_x), generate_chunk(124, chunk_x));
        }
    }

    #[test]
    fn adjacent_chunk_edges_are_seamless() {
        let seed = 5;
        let left = generate_chunk(seed, -1);
        let right = generate_chunk(seed, 0);
        assert_eq!(left.x(), -1);
        assert_eq!(right.x(), 0);
        for world_x in -CHUNK_WIDTH..CHUNK_WIDTH {
            let chunk = if world_x < 0 { &left } else { &right };
            let local_x = world_x.rem_euclid(CHUNK_WIDTH);
            assert_eq!(chunk.get(local_x, 0), Some(BlockKind::Bedrock));
            assert_eq!(
                chunk.get(local_x, surface_height(seed, i64::from(world_x))),
                Some(BlockKind::Grass)
            );
        }
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
    fn dense_chunk_editing_marks_only_loaded_data() {
        let mut grid = BlockGrid::new(WORLD_HEIGHT);
        grid.insert_chunk(generate_chunk(1, -1));
        let coordinate = IVec2::new(-1, 70);
        assert_eq!(grid.set(coordinate, BlockKind::Torch), None);
        assert_eq!(grid.get(coordinate), Some(BlockKind::Torch));
        assert_eq!(grid.remove(coordinate), Some(BlockKind::Torch));
        assert_eq!(grid.get(coordinate), None);
    }

    #[test]
    fn spawn_is_safe_in_generated_origin_chunks() {
        let grid = generate_world(5);
        assert!(grid.player_position_is_safe(grid.safe_spawn()));
    }

    #[test]
    fn stable_hash_has_no_iteration_state() {
        let expected = stable_hash(1, -4, 9, 3);
        for _ in 0..100 {
            assert_eq!(stable_hash(1, -4, 9, 3), expected);
        }
    }

    #[test]
    fn rebasing_preserves_global_chunk_identity() {
        let mut grid = BlockGrid::new(WORLD_HEIGHT);
        grid.insert_chunk(generate_chunk_at(11, 108, 8));
        grid.set(IVec2::new(8 * CHUNK_WIDTH, 1), BlockKind::Torch);
        let mut world = WorldData::for_test(grid, 11);
        world.origin_chunk = 100;

        world.grid.rebase(8);
        world.origin_chunk += 8;

        assert!(world.grid.contains_chunk(0));
        assert_eq!(world.saved_chunks()[0].x, 108);
    }

    #[test]
    fn generation_remains_deterministic_at_large_global_coordinates() {
        let left = generate_chunk_at(29, 1_000_000_000_000, 0);
        let again = generate_chunk_at(29, 1_000_000_000_000, 0);
        let right = generate_chunk_at(29, 1_000_000_000_001, 1);
        assert_eq!(left, again);
        assert_ne!(left.blocks(), right.blocks());
    }

    #[test]
    fn depth_slices_are_correlated_but_not_identical() {
        let seed = 71;
        let samples = (-64..64)
            .map(|x| {
                [
                    surface_height_at_depth(seed, x, 0),
                    surface_height_at_depth(seed, x, 1),
                    surface_height_at_depth(seed, x, 2),
                    surface_height_at_depth(seed, x, 3),
                ]
            })
            .collect::<Vec<_>>();
        assert!(
            samples
                .iter()
                .all(|heights| heights.iter().all(|height| (20..=58).contains(height)))
        );
        assert!(samples.iter().any(|heights| heights[0] != heights[1]));
        assert_eq!(samples, {
            (-64..64)
                .map(|x| {
                    [
                        surface_height_at_depth(seed, x, 0),
                        surface_height_at_depth(seed, x, 1),
                        surface_height_at_depth(seed, x, 2),
                        surface_height_at_depth(seed, x, 3),
                    ]
                })
                .collect::<Vec<_>>()
        });
    }

    #[test]
    fn generated_volume_has_bedrock_on_every_depth_slice() {
        for depth in 0..DEPTH_SLICES {
            for x in [-1_000_000_000_000, -1, 0, 1, 1_000_000_000_000] {
                assert_eq!(generated_voxel(91, x, 0, depth), Some(BlockKind::Bedrock));
            }
        }
    }
}
