use std::collections::{HashMap, HashSet, VecDeque};

use avian2d::prelude::{Collider, RigidBody};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};

use crate::{
    AppState,
    adapters::bevy::{
        DayCycleResource, LightGridResource, PendingWorldResource, RuntimeSet,
        WorldSessionResource, WorldStateResource,
        camera::{CameraRig, GameCamera, center_camera},
        player::{Player, RespawnPoint},
        rendering::{RenderCatalog, build_chunk_collider, build_chunk_meshes},
    },
    application::{
        StreamConfig, WorldSession, WorldState, plan_generation_requests, plan_unloads,
        result_is_still_requested,
    },
    domain::{BlockChunk, CHUNK_WIDTH, LightGrid, generate_chunk_at, world_to_chunk},
};

const REBASE_THRESHOLD_CHUNKS: i32 = 8;

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
pub(crate) struct WorldPresentation {
    chunk_scenes: HashMap<i32, ChunkScene>,
    render_dirty: HashSet<i32>,
    lighting_dirty: bool,
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
                integrate_generated_chunks
                    .in_set(RuntimeSet::CompletedWork)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                Update,
                (
                    rebase_world,
                    request_chunk_generation,
                    unload_distant_chunks,
                )
                    .chain()
                    .in_set(RuntimeSet::WorldMaintenance)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                Update,
                refresh_lighting
                    .in_set(RuntimeSet::Derived)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                PostUpdate,
                refresh_chunk_scenes.run_if(in_state(AppState::Playing)),
            )
            .add_systems(OnEnter(AppState::MainMenu), cleanup_world)
            .add_systems(Last, age_retired_meshes);
    }
}

fn spawn_pending_world(
    mut commands: Commands,
    pending: Option<Res<PendingWorldResource>>,
    mut day: ResMut<DayCycleResource>,
) {
    let Some(pending) = pending else {
        return;
    };
    day.phase = pending.snapshot.day_phase;
    day.previous_light_level = day.light_level();
    let world = WorldState::from_snapshot(&pending.snapshot);
    let mut presentation = WorldPresentation::default();
    presentation.render_dirty.extend(world.grid.loaded_chunks());
    commands.insert_resource(LightGridResource(LightGrid::calculate(&world.grid)));
    commands.insert_resource(WorldStateResource(world));
    commands.insert_resource(WorldSessionResource(WorldSession {
        id: pending.id.clone(),
        name: pending.snapshot.name.clone(),
        seed: pending.snapshot.seed,
        generator_version: pending.snapshot.generator_version,
        created_at_unix_s: pending.snapshot.created_at_unix_s,
        saved_revision: 0,
    }));
    commands.insert_resource(presentation);
}

fn finish_loading_world(
    world: Option<Res<WorldStateResource>>,
    pending: Option<Res<PendingWorldResource>>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if world.is_some() && pending.is_some() {
        next_state.set(AppState::Playing);
    }
}

#[allow(clippy::too_many_arguments)]
fn rebase_world(
    mut commands: Commands,
    mut world: Option<ResMut<WorldStateResource>>,
    mut presentation: Option<ResMut<WorldPresentation>>,
    player: Option<Single<(&mut Transform, &mut RespawnPoint), With<Player>>>,
    mut entities: RebaseEntityQuery,
    tasks: Query<Entity, With<ChunkGenerationTask>>,
    mut camera: Single<&mut Transform, (With<GameCamera>, Without<Player>)>,
    mut camera_rig: ResMut<CameraRig>,
) {
    let (Some(world), Some(presentation), Some(mut player)) =
        (world.as_mut(), presentation.as_mut(), player)
    else {
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
    world.rebase(delta_chunks);
    presentation.chunk_scenes = std::mem::take(&mut presentation.chunk_scenes)
        .into_iter()
        .map(|(chunk_x, scene)| (chunk_x - delta_chunks, scene))
        .collect();
    presentation.render_dirty = std::mem::take(&mut presentation.render_dirty)
        .into_iter()
        .map(|chunk_x| chunk_x - delta_chunks)
        .collect();
    presentation.lighting_dirty = true;
    center_camera(
        player.0.translation.truncate(),
        &mut camera,
        &mut camera_rig,
    );
}

fn request_chunk_generation(
    mut commands: Commands,
    world: Option<Res<WorldStateResource>>,
    player: Option<Single<&Transform, With<Player>>>,
    tasks: Query<&ChunkGenerationTask>,
) {
    let (Some(world), Some(player)) = (world, player) else {
        return;
    };
    let loaded = world.grid.loaded_chunks().collect::<HashSet<_>>();
    let in_flight = tasks
        .iter()
        .filter(|task| task.origin_chunk == world.origin_chunk)
        .map(|task| task.chunk_x)
        .collect::<HashSet<_>>();
    let center = world_to_chunk(player.translation.x.floor() as i32);
    let pool = AsyncComputeTaskPool::get();
    for request in plan_generation_requests(
        center,
        world.origin_chunk,
        &loaded,
        &in_flight,
        StreamConfig::default(),
    ) {
        let seed = world.seed;
        let persisted = world.persisted_blocks(request.global_chunk_x);
        let task = pool.spawn(async move {
            persisted
                .and_then(|blocks| BlockChunk::from_dense(request.local_chunk_x, blocks))
                .unwrap_or_else(|| {
                    generate_chunk_at(seed, request.global_chunk_x, request.local_chunk_x)
                })
        });
        commands.spawn(ChunkGenerationTask {
            chunk_x: request.local_chunk_x,
            origin_chunk: request.origin_chunk,
            task,
        });
    }
}

fn integrate_generated_chunks(
    mut commands: Commands,
    mut world: Option<ResMut<WorldStateResource>>,
    mut presentation: Option<ResMut<WorldPresentation>>,
    player: Option<Single<&Transform, With<Player>>>,
    mut tasks: Query<(Entity, &mut ChunkGenerationTask)>,
) {
    let (Some(world), Some(presentation), Some(player)) =
        (world.as_mut(), presentation.as_mut(), player)
    else {
        return;
    };
    let center = world_to_chunk(player.translation.x.floor() as i32);
    let config = StreamConfig::default();
    let mut integrated = 0;
    for (entity, mut generation) in &mut tasks {
        if integrated >= config.max_integrations_per_frame {
            break;
        }
        let Some(chunk) = check_ready(&mut generation.task) else {
            continue;
        };
        commands.entity(entity).despawn();
        if !result_is_still_requested(
            generation.origin_chunk,
            world.origin_chunk,
            chunk.x(),
            center,
            world.grid.contains_chunk(chunk.x()),
            config,
        ) {
            continue;
        }
        let chunk_x = chunk.x();
        world.integrate_chunk(chunk);
        presentation.render_dirty.insert(chunk_x);
        presentation.lighting_dirty = true;
        integrated += 1;
    }
}

fn unload_distant_chunks(
    mut commands: Commands,
    mut world: Option<ResMut<WorldStateResource>>,
    mut presentation: Option<ResMut<WorldPresentation>>,
    player: Option<Single<&Transform, With<Player>>>,
    mut retired: ResMut<RetiredMeshAssets>,
) {
    let (Some(world), Some(presentation), Some(player)) =
        (world.as_mut(), presentation.as_mut(), player)
    else {
        return;
    };
    let center = world_to_chunk(player.translation.x.floor() as i32);
    let unload = plan_unloads(center, world.grid.loaded_chunks(), StreamConfig::default());
    for chunk_x in unload {
        world.unload_chunk(chunk_x);
        despawn_chunk_entities(&mut commands, presentation, &mut retired, chunk_x);
        presentation.lighting_dirty = true;
    }
}

fn refresh_lighting(
    mut light: Option<ResMut<LightGridResource>>,
    world: Option<Res<WorldStateResource>>,
    mut presentation: Option<ResMut<WorldPresentation>>,
) {
    let (Some(light), Some(world), Some(presentation)) =
        (light.as_mut(), world, presentation.as_mut())
    else {
        return;
    };
    if presentation.lighting_dirty {
        light.0 = LightGrid::calculate(&world.grid);
        presentation.lighting_dirty = false;
    }
}

#[allow(clippy::too_many_arguments)]
fn refresh_chunk_scenes(
    mut commands: Commands,
    catalog: Option<Res<RenderCatalog>>,
    light: Option<Res<LightGridResource>>,
    day: Res<DayCycleResource>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut retired: ResMut<RetiredMeshAssets>,
    world: Option<Res<WorldStateResource>>,
    mut presentation: Option<ResMut<WorldPresentation>>,
) {
    let (Some(catalog), Some(light), Some(world), Some(presentation)) =
        (catalog, light, world, presentation.as_mut())
    else {
        return;
    };
    let mut chunks = presentation
        .render_dirty
        .drain()
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
        let transform = Transform::from_xyz((chunk_x * CHUNK_WIDTH) as f32, 0.0, 0.0);
        if let Some(scene) = presentation.chunk_scenes.get_mut(&chunk_x) {
            update_chunk_layer(
                &mut commands,
                &mut meshes,
                &mut retired,
                &mut scene.layers[0],
                rebuilt.opaque,
                &catalog.opaque_material,
                transform,
                chunk_x,
            );
            update_chunk_layer(
                &mut commands,
                &mut meshes,
                &mut retired,
                &mut scene.layers[1],
                rebuilt.cutout,
                &catalog.cutout_material,
                transform,
                chunk_x,
            );
            update_chunk_layer(
                &mut commands,
                &mut meshes,
                &mut retired,
                &mut scene.layers[2],
                rebuilt.emissive,
                &catalog.emissive_material,
                transform,
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
                transform,
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
            transform,
            chunk_x,
        );
        update_chunk_layer(
            &mut commands,
            &mut meshes,
            &mut retired,
            &mut scene.layers[1],
            rebuilt.cutout,
            &catalog.cutout_material,
            transform,
            chunk_x,
        );
        update_chunk_layer(
            &mut commands,
            &mut meshes,
            &mut retired,
            &mut scene.layers[2],
            rebuilt.emissive,
            &catalog.emissive_material,
            transform,
            chunk_x,
        );
        presentation.chunk_scenes.insert(chunk_x, scene);
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

pub(crate) fn mark_world_visuals_dirty(presentation: &mut WorldPresentation, world_x: i32) {
    let chunk_x = world_to_chunk(world_x);
    presentation.render_dirty.insert(chunk_x);
    let local_x = world_x.rem_euclid(CHUNK_WIDTH);
    if local_x == 0 {
        presentation.render_dirty.insert(chunk_x - 1);
    } else if local_x == CHUNK_WIDTH - 1 {
        presentation.render_dirty.insert(chunk_x + 1);
    }
    presentation.lighting_dirty = true;
}

fn despawn_chunk_entities(
    commands: &mut Commands,
    presentation: &mut WorldPresentation,
    retired: &mut RetiredMeshAssets,
    chunk_x: i32,
) {
    if let Some(scene) = presentation.chunk_scenes.remove(&chunk_x) {
        commands.entity(scene.root).despawn();
        for layer in scene.layers.into_iter().flatten() {
            commands.entity(layer.entity).despawn();
            retired.pending.push(layer.mesh);
        }
    }
    presentation.render_dirty.remove(&chunk_x);
}

fn cleanup_world(
    mut commands: Commands,
    presentation: Option<Res<WorldPresentation>>,
    mut retired: ResMut<RetiredMeshAssets>,
    entities: Query<Entity, With<WorldEntity>>,
    tasks: Query<Entity, With<ChunkGenerationTask>>,
) {
    if let Some(presentation) = presentation {
        for scene in presentation.chunk_scenes.values() {
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
    commands.remove_resource::<WorldStateResource>();
    commands.remove_resource::<WorldSessionResource>();
    commands.remove_resource::<WorldPresentation>();
    commands.remove_resource::<LightGridResource>();
    commands.remove_resource::<PendingWorldResource>();
}

fn age_retired_meshes(mut retired: ResMut<RetiredMeshAssets>) {
    let pending = std::mem::take(&mut retired.pending);
    retired.generations.push_back(pending);
    while retired.generations.len() > 16 {
        retired.generations.pop_front();
    }
}
