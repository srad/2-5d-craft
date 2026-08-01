use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use avian2d::prelude::{Collider, RigidBody};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};

use crate::{
    AppState,
    adapters::bevy::{
        DayCycleResource, LightVolumeResource, PendingWorldResource, RuntimeSet,
        WorldSessionResource, WorldStateResource,
        camera::{CameraRig, GameCamera, center_camera},
        environment_flag,
        lighting::configured_start_time,
        player::{Player, RespawnPoint},
        rendering::{RenderCatalog, build_chunk_collider, build_chunk_meshes},
    },
    application::{
        SaveVersion, StreamConfig, WorldId, WorldSession, WorldState, place_block,
        plan_generation_requests, plan_unloads, result_is_still_requested,
    },
    domain::{
        BlockChunk, BlockState, CHUNK_WIDTH, ChunkChange, ChunkLayer, DEPTH_SLICES, LightVolume,
        MAX_LIGHT_LEVEL, MutationReport, TorchMount, VoxelLayer, VoxelPos, WORLD_HEIGHT,
        generate_chunk_at, generated_voxel, world_to_chunk,
    },
};

const REBASE_THRESHOLD_CHUNKS: i32 = 8;

#[derive(Component)]
pub struct WorldEntity;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkCoordinate(pub i64);

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
    chunk_scenes: HashMap<i64, ChunkScene>,
}

#[derive(Resource, Default)]
struct WorldDirtySets {
    render: BTreeSet<i64>,
    lighting: BTreeSet<ChunkLayer>,
    collision: BTreeSet<i64>,
    simulation: BTreeSet<VoxelPos>,
}

#[derive(Message, Debug, Clone)]
pub(crate) struct WorldMutationMessage {
    pub(crate) world_id: WorldId,
    pub(crate) report: MutationReport,
}

#[derive(Resource, Default)]
struct RetiredMeshAssets {
    pending: Vec<Handle<Mesh>>,
    generations: VecDeque<Vec<Handle<Mesh>>>,
}

#[derive(Component)]
struct ChunkGenerationTask {
    chunk_x: i64,
    origin_chunk: i64,
    task: Task<BlockChunk>,
}

type RebaseEntityQuery<'w, 's> = Query<
    'w,
    's,
    &'static mut Transform,
    (With<WorldEntity>, Without<Player>, Without<GameCamera>),
>;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<WorldMutationMessage>()
            .init_resource::<RetiredMeshAssets>()
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
                dispatch_mutation_reports.in_set(RuntimeSet::MutationDispatch),
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
            .add_systems(Last, (clear_simulation_dirty, age_retired_meshes).chain());
    }
}

fn spawn_pending_world(
    mut commands: Commands,
    pending: Option<Res<PendingWorldResource>>,
    mut day: ResMut<DayCycleResource>,
    mut mutations: MessageWriter<WorldMutationMessage>,
) {
    let Some(pending) = pending else {
        return;
    };
    day.0 = configured_start_time(pending.snapshot.day_time_ticks);
    let mut initialized = WorldState::from_snapshot(&pending.snapshot);
    let fixture_report = (environment_flag("SIDECRAFT_AUTOSTART")
        && environment_flag("SIDECRAFT_TEST_TORCH_FIXTURE"))
    .then(|| {
        place_test_torches(
            &mut initialized.state,
            pending.snapshot.player.chunk_x,
            pending.snapshot.player.local_x,
            pending.snapshot.player.y,
        )
    })
    .flatten();
    commands.insert_resource(LightVolumeResource(calculate_light_volume(
        &initialized.state,
    )));
    commands.insert_resource(WorldStateResource(initialized.state));
    commands.insert_resource(WorldSessionResource(WorldSession {
        id: pending.id.clone(),
        name: pending.snapshot.name.clone(),
        seed: pending.snapshot.seed,
        generator_version: pending.snapshot.generator_version,
        created_at_unix_s: pending.snapshot.created_at_unix_s,
        saved_version: SaveVersion {
            world_revision: 0,
            day_time_ticks: pending.snapshot.day_time_ticks,
        },
    }));
    commands.insert_resource(WorldPresentation::default());
    commands.insert_resource(WorldDirtySets::default());
    mutations.write(WorldMutationMessage {
        world_id: pending.id.clone(),
        report: initialized.report,
    });
    if let Some(report) = fixture_report {
        mutations.write(WorldMutationMessage {
            world_id: pending.id.clone(),
            report,
        });
    }
}

fn place_test_torches(
    world: &mut WorldState,
    origin_chunk: i64,
    local_player_x: f32,
    player_y: f32,
) -> Option<MutationReport> {
    let player_x = origin_chunk
        .saturating_mul(i64::from(CHUNK_WIDTH))
        .saturating_add(local_player_x.floor() as i64);
    let base_y = (player_y.ceil() as i32 + 1).clamp(1, WORLD_HEIGHT - 2);
    for offset in [3_i64, 4, 5, -3, -4, -5] {
        let x = player_x.saturating_add(offset);
        if !world
            .view()
            .contains_chunk(ChunkLayer::foreground(world_to_chunk(x)))
            || !world
                .view()
                .contains_chunk(ChunkLayer::foreground(world_to_chunk(x.saturating_add(2))))
        {
            continue;
        }
        for y in base_y..=(base_y + 3).min(WORLD_HEIGHT - 1) {
            let floor_support = VoxelPos::foreground(x, y - 1);
            let floor_torch = VoxelPos::foreground(x, y);
            let wall_torch = VoxelPos::foreground(x.saturating_add(1), y);
            let wall_support = VoxelPos::foreground(x.saturating_add(2), y);
            let placements = [
                (floor_support, BlockState::DIRT),
                (wall_support, BlockState::DIRT),
                (floor_torch, BlockState::TORCH),
                (wall_torch, BlockState::torch(TorchMount::WallLeft)),
            ];
            if placements
                .iter()
                .any(|(position, _)| world.view().block(*position).is_some())
            {
                continue;
            }
            let mut report = MutationReport::default();
            for (position, state) in placements {
                let placed = place_block(world, position, state)
                    .expect("validated torch fixture placement must succeed")
                    .report;
                report.cell_changes.extend(placed.cell_changes);
                report.chunk_changes.extend(placed.chunk_changes);
            }
            return Some(report);
        }
    }
    None
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
    player: Option<Single<(&mut Transform, &mut RespawnPoint), With<Player>>>,
    mut entities: RebaseEntityQuery,
    tasks: Query<Entity, With<ChunkGenerationTask>>,
    mut camera: Single<&mut Transform, (With<GameCamera>, Without<Player>)>,
    mut camera_rig: ResMut<CameraRig>,
) {
    let (Some(world), Some(mut player)) = (world.as_mut(), player) else {
        return;
    };
    let delta_chunks = (player.0.translation.x.floor() as i32).div_euclid(CHUNK_WIDTH);
    if delta_chunks.abs() < REBASE_THRESHOLD_CHUNKS {
        return;
    }
    let offset = (delta_chunks * CHUNK_WIDTH) as f32;
    player.0.translation.x -= offset;
    player.1.0.x -= offset;
    for mut transform in &mut entities {
        transform.translation.x -= offset;
    }
    for entity in &tasks {
        commands.entity(entity).despawn();
    }
    world.rebase(delta_chunks);
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
    let loaded = world.view().loaded_chunks().collect::<HashSet<_>>();
    let in_flight = tasks
        .iter()
        .filter(|task| task.origin_chunk == world.origin_chunk())
        .map(|task| task.chunk_x)
        .collect::<HashSet<_>>();
    let center = world.origin_chunk() + world_to_chunk(player.translation.x.floor() as i64);
    let pool = AsyncComputeTaskPool::get();
    for request in plan_generation_requests(
        center,
        world.origin_chunk(),
        &loaded,
        &in_flight,
        StreamConfig::default(),
    ) {
        let seed = world.seed();
        let persisted = world.persisted_chunk(request.global_chunk_x);
        let task = pool.spawn(async move {
            persisted.unwrap_or_else(|| generate_chunk_at(seed, request.global_chunk_x))
        });
        commands.spawn(ChunkGenerationTask {
            chunk_x: request.global_chunk_x,
            origin_chunk: request.origin_chunk,
            task,
        });
    }
}

fn integrate_generated_chunks(
    mut commands: Commands,
    mut world: Option<ResMut<WorldStateResource>>,
    session: Option<Res<WorldSessionResource>>,
    player: Option<Single<&Transform, With<Player>>>,
    mut tasks: Query<(Entity, &mut ChunkGenerationTask)>,
    mut mutations: MessageWriter<WorldMutationMessage>,
) {
    let (Some(world), Some(session), Some(player)) = (world.as_mut(), session, player) else {
        return;
    };
    let center = world.origin_chunk() + world_to_chunk(player.translation.x.floor() as i64);
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
            world.origin_chunk(),
            chunk.x(),
            center,
            world
                .view()
                .contains_chunk(ChunkLayer::foreground(chunk.x())),
            config,
        ) {
            continue;
        }
        let report = world.integrate_chunk(chunk);
        mutations.write(WorldMutationMessage {
            world_id: session.id.clone(),
            report,
        });
        integrated += 1;
    }
}

fn unload_distant_chunks(
    mut world: Option<ResMut<WorldStateResource>>,
    session: Option<Res<WorldSessionResource>>,
    player: Option<Single<&Transform, With<Player>>>,
    mut mutations: MessageWriter<WorldMutationMessage>,
) {
    let (Some(world), Some(session), Some(player)) = (world.as_mut(), session, player) else {
        return;
    };
    let center = world.origin_chunk() + world_to_chunk(player.translation.x.floor() as i64);
    let unload = plan_unloads(
        center,
        world.view().loaded_chunks(),
        StreamConfig::default(),
    );
    for chunk_x in unload {
        let report = world.unload_chunk(chunk_x);
        if !report.is_empty() {
            mutations.write(WorldMutationMessage {
                world_id: session.id.clone(),
                report,
            });
        }
    }
}

fn dispatch_mutation_reports(
    session: Option<Res<WorldSessionResource>>,
    mut dirty: Option<ResMut<WorldDirtySets>>,
    mut mutations: MessageReader<WorldMutationMessage>,
) {
    let (Some(session), Some(dirty)) = (session, dirty.as_mut()) else {
        return;
    };
    for mutation in mutations.read() {
        if mutation.world_id != session.id {
            continue;
        }
        apply_report_to_dirty(&mutation.report, dirty);
    }
}

fn apply_report_to_dirty(report: &MutationReport, dirty: &mut WorldDirtySets) {
    for change in &report.cell_changes {
        let chunk = ChunkLayer {
            chunk_x: world_to_chunk(change.position.global_x),
            layer: change.position.layer,
        };
        dirty.render.insert(chunk.chunk_x);
        dirty.lighting.insert(chunk);
        if change.position.layer == VoxelLayer::Foreground {
            dirty.collision.insert(chunk.chunk_x);
        }
        let local_x = change.position.global_x.rem_euclid(i64::from(CHUNK_WIDTH));
        if local_x == 0 {
            dirty.render.insert(chunk.chunk_x - 1);
        } else if local_x == i64::from(CHUNK_WIDTH - 1) {
            dirty.render.insert(chunk.chunk_x + 1);
        }
        for position in mutation_neighbors(change.position) {
            dirty.simulation.insert(position);
        }
    }
    for change in &report.chunk_changes {
        let chunk = match change {
            ChunkChange::Integrated(chunk) | ChunkChange::Unloaded(chunk) => *chunk,
        };
        dirty.render.insert(chunk.chunk_x);
        dirty.lighting.insert(chunk);
        if chunk.layer == VoxelLayer::Foreground {
            dirty.collision.insert(chunk.chunk_x);
        }
        dirty.render.insert(chunk.chunk_x - 1);
        dirty.render.insert(chunk.chunk_x + 1);
    }
}

fn mutation_neighbors(position: VoxelPos) -> [VoxelPos; 5] {
    [
        position,
        VoxelPos {
            global_x: position.global_x - 1,
            ..position
        },
        VoxelPos {
            global_x: position.global_x + 1,
            ..position
        },
        VoxelPos {
            y: position.y - 1,
            ..position
        },
        VoxelPos {
            y: position.y + 1,
            ..position
        },
    ]
}

fn refresh_lighting(
    mut light: Option<ResMut<LightVolumeResource>>,
    world: Option<Res<WorldStateResource>>,
    mut dirty: Option<ResMut<WorldDirtySets>>,
) {
    let (Some(light), Some(world), Some(dirty)) = (light.as_mut(), world, dirty.as_mut()) else {
        return;
    };
    if !dirty.lighting.is_empty() {
        let next = calculate_light_volume(&world);
        let changed = light.0.changed_x_columns(&next);
        light.0 = next;
        mark_light_columns_dirty(changed, dirty);
        dirty.lighting.clear();
    }
}

fn calculate_light_volume(world: &WorldState) -> LightVolume {
    let view = world.view();
    let Some((loaded_min_x, loaded_max_x)) = view.loaded_x_bounds() else {
        return LightVolume::empty();
    };
    let halo = i64::from(MAX_LIGHT_LEVEL);
    let min_x = loaded_min_x.saturating_sub(halo);
    let max_x = loaded_max_x.saturating_add(halo);
    let seed = world.seed();
    LightVolume::calculate(min_x, max_x, view.height(), DEPTH_SLICES, |x, y, depth| {
        if let Some(layer) = VoxelLayer::from_persistent_depth(depth)
            && view.contains_chunk(ChunkLayer::new(world_to_chunk(x), layer))
        {
            view.block(VoxelPos::new(x, y, layer))
        } else {
            generated_voxel(seed, x, y, depth)
        }
    })
}

fn mark_light_columns_dirty(changed_x: impl IntoIterator<Item = i64>, dirty: &mut WorldDirtySets) {
    for x in changed_x {
        for sample_x in [x.saturating_sub(1), x, x.saturating_add(1)] {
            dirty.render.insert(world_to_chunk(sample_x));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn refresh_chunk_scenes(
    mut commands: Commands,
    catalog: Option<Res<RenderCatalog>>,
    light: Option<Res<LightVolumeResource>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut retired: ResMut<RetiredMeshAssets>,
    world: Option<Res<WorldStateResource>>,
    mut presentation: Option<ResMut<WorldPresentation>>,
    mut dirty: Option<ResMut<WorldDirtySets>>,
) {
    let (Some(catalog), Some(light), Some(world), Some(presentation), Some(dirty)) =
        (catalog, light, world, presentation.as_mut(), dirty.as_mut())
    else {
        return;
    };
    let work = dirty
        .render
        .iter()
        .chain(dirty.collision.iter())
        .copied()
        .collect::<BTreeSet<_>>();
    for chunk_x in work {
        let rebuild_render = dirty.render.contains(&chunk_x);
        let rebuild_collision = dirty.collision.contains(&chunk_x);
        if !world.view().contains_chunk(ChunkLayer::foreground(chunk_x)) {
            despawn_chunk_entities(&mut commands, presentation, &mut retired, chunk_x);
            continue;
        }
        let local_chunk = chunk_x - world.origin_chunk();
        let Ok(local_chunk) = i32::try_from(local_chunk) else {
            continue;
        };
        let transform = Transform::from_xyz((local_chunk * CHUNK_WIDTH) as f32, 0.0, 0.0);
        let is_new = !presentation.chunk_scenes.contains_key(&chunk_x);
        let rebuilt = (rebuild_render || is_new)
            .then(|| build_chunk_meshes(&world.view(), chunk_x, world.seed(), &light));
        let collider =
            (rebuild_collision || is_new).then(|| build_chunk_collider(&world.view(), chunk_x));
        if let Some(scene) = presentation.chunk_scenes.get_mut(&chunk_x) {
            if let Some(rebuilt) = rebuilt {
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
            }
            if let Some(collider) = collider {
                if let Some(collider) = collider {
                    commands.entity(scene.root).insert(collider);
                } else {
                    commands.entity(scene.root).remove::<Collider>();
                }
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
        if let Some(Some(collider)) = collider {
            commands.entity(root).insert(collider);
        }
        let mut scene = ChunkScene {
            root,
            layers: [None, None, None],
        };
        if let Some(rebuilt) = rebuilt {
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
        }
        presentation.chunk_scenes.insert(chunk_x, scene);
    }
    dirty.render.clear();
    dirty.collision.clear();
}

#[allow(clippy::too_many_arguments)]
fn update_chunk_layer<M: Material>(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    retired: &mut RetiredMeshAssets,
    layer: &mut Option<ChunkRenderLayer>,
    mesh: Mesh,
    material: &Handle<M>,
    transform: Transform,
    chunk_x: i64,
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

fn despawn_chunk_entities(
    commands: &mut Commands,
    presentation: &mut WorldPresentation,
    retired: &mut RetiredMeshAssets,
    chunk_x: i64,
) {
    if let Some(scene) = presentation.chunk_scenes.remove(&chunk_x) {
        commands.entity(scene.root).despawn();
        for layer in scene.layers.into_iter().flatten() {
            commands.entity(layer.entity).despawn();
            retired.pending.push(layer.mesh);
        }
    }
}

fn clear_simulation_dirty(mut dirty: Option<ResMut<WorldDirtySets>>) {
    if let Some(dirty) = dirty.as_mut() {
        dirty.simulation.clear();
    }
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
    commands.remove_resource::<WorldDirtySets>();
    commands.remove_resource::<LightVolumeResource>();
    commands.remove_resource::<PendingWorldResource>();
}

fn age_retired_meshes(mut retired: ResMut<RetiredMeshAssets>) {
    let pending = std::mem::take(&mut retired.pending);
    retired.generations.push_back(pending);
    while retired.generations.len() > 16 {
        retired.generations.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{BlockState, CellChange};

    #[test]
    fn cell_reports_coalesce_and_only_cross_the_edited_edge() {
        let position = VoxelPos::foreground(0, 70);
        let report = MutationReport {
            cell_changes: vec![
                CellChange {
                    position,
                    before: None,
                    after: Some(BlockState::DIRT),
                },
                CellChange {
                    position,
                    before: Some(BlockState::DIRT),
                    after: Some(BlockState::STONE),
                },
            ],
            chunk_changes: Vec::new(),
        };
        let mut dirty = WorldDirtySets::default();

        apply_report_to_dirty(&report, &mut dirty);

        assert_eq!(dirty.render, BTreeSet::from([-1, 0]));
        assert_eq!(dirty.lighting, BTreeSet::from([ChunkLayer::foreground(0)]));
        assert_eq!(dirty.collision, BTreeSet::from([0]));
        assert_eq!(dirty.simulation.len(), 5);
    }

    #[test]
    fn chunk_lifecycle_reports_invalidate_both_render_neighbors() {
        let report = MutationReport {
            cell_changes: Vec::new(),
            chunk_changes: vec![ChunkChange::Integrated(ChunkLayer::foreground(4))],
        };
        let mut dirty = WorldDirtySets::default();

        apply_report_to_dirty(&report, &mut dirty);

        assert_eq!(dirty.render, BTreeSet::from([3, 4, 5]));
        assert_eq!(dirty.lighting, BTreeSet::from([ChunkLayer::foreground(4)]));
        assert_eq!(dirty.collision, BTreeSet::from([4]));
        assert!(dirty.simulation.is_empty());
    }

    #[test]
    fn changed_light_columns_dirty_faces_across_chunk_boundaries() {
        let mut dirty = WorldDirtySets::default();

        mark_light_columns_dirty([31], &mut dirty);

        assert_eq!(dirty.render, BTreeSet::from([0, 1]));
        assert!(dirty.lighting.is_empty());
        assert!(dirty.collision.is_empty());
        assert!(dirty.simulation.is_empty());
    }

    #[test]
    fn backwall_reports_rebuild_rendering_and_lighting_without_collision() {
        let position = VoxelPos::backwall(31, 12);
        let report = MutationReport {
            cell_changes: vec![CellChange {
                position,
                before: None,
                after: Some(BlockState::STONE),
            }],
            chunk_changes: Vec::new(),
        };
        let mut dirty = WorldDirtySets::default();

        apply_report_to_dirty(&report, &mut dirty);

        assert_eq!(dirty.render, BTreeSet::from([0, 1]));
        assert_eq!(dirty.lighting, BTreeSet::from([ChunkLayer::backwall(0)]));
        assert!(dirty.collision.is_empty());
        assert!(dirty.simulation.contains(&position));
    }
}
