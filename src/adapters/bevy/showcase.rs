use bevy::{
    ecs::system::SystemParam,
    light::{NotShadowCaster, NotShadowReceiver},
    prelude::*,
    window::PrimaryWindow,
};

use crate::{
    AppState,
    adapters::bevy::{
        DayCycleResource,
        camera::{CameraRig, GAME_CAMERA_SCALE, GameCamera, frame_camera},
        rendering::{RenderCatalog, build_voxel_scene_meshes},
    },
    domain::{BlockState, DayCycle, LightVolume, NOON_TICKS},
};

const SCENE_MIN_X: i64 = -24;
const SCENE_WIDTH: i32 = 48;
const SCENE_HEIGHT: i32 = 27;
const SCENE_DEPTH: u8 = 6;
const SHOWCASE_SEED: u64 = 0x51DE_CAFE;
const INSPECTION_WIDTH: f32 = 36.0;
const INSPECTION_HEIGHT: f32 = 20.0;
const CAMERA_TARGET: Vec2 = Vec2::new(0.0, 11.5);

#[derive(Component)]
struct MenuShowcaseRoot;

#[derive(Resource)]
pub(crate) struct MenuShowcaseDay(pub(crate) DayCycle);

impl Default for MenuShowcaseDay {
    fn default() -> Self {
        Self(DayCycle::from_ticks(NOON_TICKS))
    }
}

#[derive(SystemParam)]
pub(crate) struct PresentationDay<'w> {
    state: Res<'w, State<AppState>>,
    gameplay: Res<'w, DayCycleResource>,
    showcase: Res<'w, MenuShowcaseDay>,
}

impl PresentationDay<'_> {
    pub(crate) fn get(&self) -> &DayCycle {
        if uses_showcase(*self.state.get()) {
            &self.showcase.0
        } else {
            &self.gameplay.0
        }
    }
}

pub(crate) fn uses_showcase(state: AppState) -> bool {
    matches!(
        state,
        AppState::MainMenu
            | AppState::Settings
            | AppState::TexturePacks
            | AppState::WorldSelect
            | AppState::LoadingWorld
    )
}

pub(crate) struct MenuShowcasePlugin;

impl Plugin for MenuShowcasePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuShowcaseDay>()
            .add_systems(PostStartup, spawn_showcase)
            .add_systems(Update, (sync_showcase_visibility, fit_showcase_camera))
            .add_systems(OnEnter(AppState::Playing), restore_game_camera_scale);
    }
}

fn spawn_showcase(
    mut commands: Commands,
    catalog: Res<RenderCatalog>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let light = LightVolume::calculate(
        SCENE_MIN_X,
        SCENE_MIN_X + i64::from(SCENE_WIDTH),
        SCENE_HEIGHT,
        SCENE_DEPTH,
        |x, y, depth| showcase_block(x, y, i32::from(depth)),
    );
    let scene = build_voxel_scene_meshes(
        SCENE_MIN_X,
        SCENE_WIDTH,
        SCENE_HEIGHT,
        SCENE_DEPTH,
        SHOWCASE_SEED,
        &light,
        showcase_block,
    );
    let opaque = meshes.add(scene.opaque);
    let cutout = meshes.add(scene.cutout);
    let emissive = meshes.add(scene.emissive);
    commands
        .spawn((
            MenuShowcaseRoot,
            Transform::from_xyz(SCENE_MIN_X as f32, 0.0, 0.0),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            root.spawn((
                Mesh3d(opaque),
                MeshMaterial3d(catalog.opaque_material.clone()),
                NotShadowCaster,
                NotShadowReceiver,
            ));
            root.spawn((
                Mesh3d(cutout),
                MeshMaterial3d(catalog.cutout_material.clone()),
                NotShadowCaster,
                NotShadowReceiver,
            ));
            root.spawn((
                Mesh3d(emissive),
                MeshMaterial3d(catalog.emissive_material.clone()),
                NotShadowCaster,
                NotShadowReceiver,
            ));
            spawn_showcase_player(root, &catalog);
        });
}

fn spawn_showcase_player(root: &mut ChildSpawnerCommands, catalog: &RenderCatalog) {
    let cube = catalog.player_cube.clone();
    root.spawn((Transform::from_xyz(28.0, 13.1, 0.0), Visibility::default()))
        .with_children(|player| {
            player
                .spawn((
                    Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_3)),
                    Visibility::default(),
                ))
                .with_children(|parts| {
                    spawn_player_part(
                        parts,
                        &cube,
                        &catalog.player_materials[1],
                        Vec3::new(0.0, 0.10, 0.0),
                        Vec3::new(0.62, 0.68, 0.38),
                    );
                    spawn_player_part(
                        parts,
                        &cube,
                        &catalog.player_materials[0],
                        Vec3::new(0.0, 0.67, 0.0),
                        Vec3::new(0.56, 0.56, 0.50),
                    );
                    spawn_player_part(
                        parts,
                        &cube,
                        &catalog.player_materials[3],
                        Vec3::new(0.0, 0.91, -0.01),
                        Vec3::new(0.60, 0.15, 0.54),
                    );
                    for x in [-0.42, 0.42] {
                        spawn_player_part(
                            parts,
                            &cube,
                            &catalog.player_materials[0],
                            Vec3::new(x, 0.08, 0.0),
                            Vec3::new(0.20, 0.64, 0.24),
                        );
                    }
                    for x in [-0.18, 0.18] {
                        spawn_player_part(
                            parts,
                            &cube,
                            &catalog.player_materials[2],
                            Vec3::new(x, -0.55, 0.0),
                            Vec3::new(0.25, 0.58, 0.28),
                        );
                        spawn_player_part(
                            parts,
                            &cube,
                            &catalog.player_materials[4],
                            Vec3::new(x, -0.84, 0.07),
                            Vec3::new(0.27, 0.13, 0.40),
                        );
                    }
                    for x in [-0.14, 0.14] {
                        spawn_player_part(
                            parts,
                            &cube,
                            &catalog.player_materials[4],
                            Vec3::new(x, 0.72, 0.27),
                            Vec3::splat(0.055),
                        );
                    }
                });
        });
}

fn spawn_player_part(
    parts: &mut ChildSpawnerCommands,
    mesh: &Handle<Mesh>,
    material: &Handle<StandardMaterial>,
    translation: Vec3,
    scale: Vec3,
) {
    parts.spawn((
        Mesh3d(mesh.clone()),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(translation).with_scale(scale),
        NotShadowCaster,
        NotShadowReceiver,
    ));
}

fn sync_showcase_visibility(
    state: Res<State<AppState>>,
    mut roots: Query<&mut Visibility, With<MenuShowcaseRoot>>,
) {
    if !state.is_changed() {
        return;
    }
    let visibility = if uses_showcase(*state.get()) {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut current in &mut roots {
        *current = visibility;
    }
}

type ShowcaseCameraQuery<'w, 's> = Single<
    'w,
    's,
    (&'static mut Transform, &'static mut Projection),
    (With<GameCamera>, Without<MenuShowcaseRoot>),
>;

fn fit_showcase_camera(
    state: Res<State<AppState>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: ShowcaseCameraQuery,
    mut rig: ResMut<CameraRig>,
) {
    if !uses_showcase(*state.get()) {
        return;
    }
    let scale = (INSPECTION_WIDTH / window.width())
        .max(INSPECTION_HEIGHT / window.height())
        .max(0.000_1);
    let (mut camera_transform, mut camera_projection) = camera.into_inner();
    frame_camera(
        CAMERA_TARGET,
        scale,
        &mut camera_transform,
        &mut camera_projection,
        &mut rig,
    );
}

fn restore_game_camera_scale(mut camera: Single<&mut Projection, With<GameCamera>>) {
    if let Projection::Orthographic(projection) = &mut **camera {
        projection.scale = GAME_CAMERA_SCALE;
    }
}

fn showcase_block(x: i64, y: i32, depth: i32) -> Option<BlockState> {
    if !(SCENE_MIN_X..SCENE_MIN_X + i64::from(SCENE_WIDTH)).contains(&x)
        || !(0..SCENE_HEIGHT).contains(&y)
        || !(0..i32::from(SCENE_DEPTH)).contains(&depth)
    {
        return None;
    }
    if depth == 0 {
        foreground_block(x, y)
    } else {
        rear_block(x, y, depth)
    }
}

fn foreground_block(x: i64, y: i32) -> Option<BlockState> {
    match (x, y) {
        (0, 3) => return Some(BlockState::TORCH),
        (-7, 6) => return Some(BlockState::WALL_TORCH_RIGHT),
        (7, 6) => return Some(BlockState::WALL_TORCH_LEFT),
        (-8, 4 | 5) | (-9, 5) => return Some(BlockState::COAL_ORE),
        (8, 5 | 6) | (9, 5) => return Some(BlockState::IRON_ORE),
        _ => {}
    }
    let surface = foreground_surface(x);
    for tree_x in [-14, 14] {
        let tree_surface = foreground_surface(tree_x);
        if x == tree_x && (tree_surface + 1..=tree_surface + 5).contains(&y) {
            return Some(BlockState::WOOD);
        }
        let crown_y = tree_surface + 6;
        let dx = (x - tree_x).unsigned_abs();
        let dy = (y - crown_y).unsigned_abs();
        if dx <= 3 && dy <= 2 && dx + u64::from(dy) <= 4 {
            return Some(BlockState::LEAVES);
        }
    }
    if (-7..=7).contains(&x) && (3..=8).contains(&y) {
        return None;
    }
    terrain_block(surface, y)
}

fn rear_block(x: i64, y: i32, depth: i32) -> Option<BlockState> {
    let shifted = x + i64::from(depth * 3);
    let surface =
        9 + match shifted {
            value if value < -14 => 4,
            value if value < -6 => 2,
            value if value < 8 => 0,
            value if value < 16 => 2,
            _ => 4,
        } + depth % 2;
    terrain_block(surface, y)
}

fn foreground_surface(x: i64) -> i32 {
    match x {
        value if value < -18 => 16,
        value if value < -10 => 13,
        value if value < 10 => 11,
        value if value < 18 => 13,
        _ => 16,
    }
}

fn terrain_block(surface: i32, y: i32) -> Option<BlockState> {
    if y > surface {
        None
    } else if y == 0 {
        Some(BlockState::BEDROCK)
    } else if y == surface {
        Some(BlockState::GRASS)
    } else if y >= surface - 3 {
        Some(BlockState::DIRT)
    } else {
        Some(BlockState::STONE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn showcase_contains_every_block_state() {
        let states = (SCENE_MIN_X..SCENE_MIN_X + i64::from(SCENE_WIDTH))
            .flat_map(|x| {
                (0..SCENE_HEIGHT).flat_map(move |y| {
                    (0..i32::from(SCENE_DEPTH)).filter_map(move |depth| showcase_block(x, y, depth))
                })
            })
            .collect::<HashSet<_>>();
        for state in BlockState::ALL {
            assert!(states.contains(&state), "showcase is missing {state:?}");
        }
    }

    #[test]
    fn common_aspect_ratios_keep_the_inspection_area_inside_overscan() {
        for (width, height) in [(800.0, 450.0), (1024.0, 768.0), (2560.0, 1080.0)] {
            let scale = (INSPECTION_WIDTH / width).max(INSPECTION_HEIGHT / height);
            assert!(width * scale <= SCENE_WIDTH as f32);
            assert!(height * scale <= SCENE_HEIGHT as f32);
        }
    }
}
