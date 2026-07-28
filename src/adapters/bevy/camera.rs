use crate::AppState;
use crate::{
    adapters::bevy::{DayCycleResource, player::Player},
    domain::WORLD_HEIGHT,
};
use bevy::camera::{ClearColorConfig, Hdr, ScalingMode};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::ui::IsDefaultUiCamera;

const CAMERA_SCALE: f32 = 1.0 / 32.0;
const CAMERA_DECAY: f32 = 8.0;
const CAMERA_OFFSET: Vec3 = Vec3::new(8.0, 6.0, 40.0);

#[derive(Component)]
pub struct GameCamera;

#[derive(Resource)]
pub(crate) struct CameraRig {
    logical_position: Vec2,
}

#[derive(Component)]
struct SkyBand {
    index: usize,
}

#[derive(Component)]
struct Cloud;

#[derive(Component)]
struct Sun;

#[derive(Resource)]
struct AutomaticScreenshot(Timer);

type BackgroundPlayerFilter = (With<Player>, Without<SkyBand>, Without<Cloud>);
type SkyBandFilter = (With<SkyBand>, Without<Player>, Without<Cloud>);
type CloudFilter = (With<Cloud>, Without<Player>, Without<SkyBand>);

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_scene)
            .add_systems(OnEnter(AppState::Playing), arm_automatic_screenshot)
            .add_systems(
                Update,
                (
                    follow_player.run_if(in_state(AppState::Playing)),
                    animate_environment,
                    follow_background.run_if(in_state(AppState::Playing)),
                    capture_screenshot,
                    capture_automatic_screenshot,
                    toggle_pause,
                ),
            );
    }
}

fn screenshot_path() -> String {
    std::env::var("SIDECRAFT_SCREENSHOT").unwrap_or_else(|_| "sidecraft-screenshot.png".into())
}

fn request_screenshot(commands: &mut Commands) {
    let path = screenshot_path();
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
}

fn capture_screenshot(keyboard: Res<ButtonInput<KeyCode>>, mut commands: Commands) {
    if !keyboard.just_pressed(KeyCode::F12) {
        return;
    }
    request_screenshot(&mut commands);
}

fn arm_automatic_screenshot(mut commands: Commands) {
    if std::env::var("SIDECRAFT_AUTOCAPTURE").is_ok() {
        commands.insert_resource(AutomaticScreenshot(Timer::from_seconds(
            3.0,
            TimerMode::Once,
        )));
    }
}

fn capture_automatic_screenshot(
    mut commands: Commands,
    time: Res<Time>,
    automatic: Option<ResMut<AutomaticScreenshot>>,
) {
    let Some(mut automatic) = automatic else {
        return;
    };
    if automatic.0.tick(time.delta()).just_finished() {
        request_screenshot(&mut commands);
        commands.remove_resource::<AutomaticScreenshot>();
    }
}

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.32, 0.66, 0.90)),
            ..default()
        },
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::WindowSize,
            scale: CAMERA_SCALE,
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_translation(Vec3::new(100.0, 40.0, 0.0) + CAMERA_OFFSET)
            .looking_at(Vec3::new(100.0, 40.0, 0.0), Vec3::Y),
        Hdr,
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        Msaa::Sample4,
        AmbientLight {
            color: Color::WHITE,
            brightness: 120.0,
            ..default()
        },
        IsDefaultUiCamera,
        GameCamera,
    ));
    commands.insert_resource(CameraRig {
        logical_position: Vec2::new(100.0, 40.0),
    });

    let sky_mesh = meshes.add(Rectangle::new(320.0, 28.0));
    for index in 0..8 {
        let fraction = index as f32 / 7.0;
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(
                0.32 + fraction * 0.25,
                0.62 + fraction * 0.20,
                0.88 + fraction * 0.08,
            ),
            unlit: true,
            cull_mode: None,
            ..default()
        });
        commands.spawn((
            Mesh3d(sky_mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_xyz(100.0, index as f32 * 28.0 - 30.0, -24.0),
            SkyBand { index },
        ));
    }

    let cloud_mesh = meshes.add(Rectangle::new(7.0, 2.2));
    let cloud_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.92, 0.96, 1.0, 0.72),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        ..default()
    });
    for index in 0..7 {
        commands.spawn((
            Mesh3d(cloud_mesh.clone()),
            MeshMaterial3d(cloud_material.clone()),
            Transform::from_xyz(
                index as f32 * 34.0 - 10.0,
                62.0 + (index % 3) as f32 * 6.0,
                -12.0 - (index % 2) as f32 * 2.0,
            ),
            Cloud,
        ));
    }

    commands.spawn((
        DirectionalLight {
            illuminance: 8_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, -0.4, 0.0)),
        Sun,
    ));
}

fn follow_player(
    time: Res<Time>,
    player: Single<&Transform, (With<Player>, Without<GameCamera>)>,
    mut camera: Single<(&mut Transform, &Projection), With<GameCamera>>,
    mut rig: ResMut<CameraRig>,
) {
    let Projection::Orthographic(projection) = camera.1 else {
        return;
    };
    let target = player.translation.truncate();
    rig.logical_position = smooth_position(
        rig.logical_position,
        target,
        CAMERA_DECAY,
        time.delta_secs(),
    );
    rig.logical_position = clamp_camera_vertical(
        rig.logical_position,
        projection.area.half_size().y,
        WORLD_HEIGHT as f32,
    );
    let target = Vec3::new(
        snap(rig.logical_position.x, projection.scale),
        snap(rig.logical_position.y, projection.scale),
        0.0,
    );
    camera.0.translation = target + CAMERA_OFFSET;
    camera.0.look_at(target, Vec3::Y);
}

pub(crate) fn center_camera(target: Vec2, camera: &mut Transform, rig: &mut CameraRig) {
    rig.logical_position = target;
    let target = target.extend(0.0);
    camera.translation = target + CAMERA_OFFSET;
    camera.look_at(target, Vec3::Y);
}

fn animate_environment(
    day: Res<DayCycleResource>,
    mut ambient: Single<&mut AmbientLight, With<GameCamera>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    bands: Query<(&SkyBand, &MeshMaterial3d<StandardMaterial>)>,
    mut sun: Single<(&mut DirectionalLight, &mut Transform), With<Sun>>,
) {
    let (bottom, top) = sky_palette(day.phase);
    for (band, handle) in &bands {
        if let Some(mut material) = materials.get_mut(&handle.0) {
            let fraction = band.index as f32 / 7.0;
            material.base_color = Color::srgb(
                lerp(bottom[0], top[0], fraction),
                lerp(bottom[1], top[1], fraction),
                lerp(bottom[2], top[2], fraction),
            );
        }
    }
    let daylight = day.daylight();
    ambient.brightness = lerp(25.0, 140.0, daylight);
    sun.0.illuminance = lerp(250.0, 9_000.0, daylight);
    sun.1.rotation = Quat::from_euler(EulerRot::XYZ, -0.7, day.phase * std::f32::consts::TAU, 0.0);
}

fn follow_background(
    time: Res<Time>,
    player: Single<&Transform, BackgroundPlayerFilter>,
    mut bands: Query<&mut Transform, SkyBandFilter>,
    mut clouds: Query<&mut Transform, CloudFilter>,
) {
    for mut transform in &mut bands {
        transform.translation.x = player.translation.x;
    }
    for mut transform in &mut clouds {
        transform.translation.x = wrap_cloud_x(
            transform.translation.x + time.delta_secs() * 0.7,
            player.translation.x,
        );
    }
}

fn toggle_pause(
    keyboard: Res<ButtonInput<KeyCode>>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
) {
    if !keyboard.just_pressed(KeyCode::Escape) {
        return;
    }
    match state.get() {
        AppState::Playing => next.set(AppState::Paused),
        AppState::Paused => next.set(AppState::Playing),
        _ => {}
    }
}

pub fn smooth_position(current: Vec2, target: Vec2, decay: f32, delta_seconds: f32) -> Vec2 {
    let blend = 1.0 - (-decay * delta_seconds).exp();
    current.lerp(target, blend)
}

pub fn clamp_camera_vertical(position: Vec2, half_view: f32, world_height: f32) -> Vec2 {
    let y = if half_view * 2.0 >= world_height {
        world_height * 0.5
    } else {
        position.y.clamp(half_view, world_height - half_view)
    };
    Vec2::new(position.x, y)
}

pub fn snap(value: f32, pixel_size: f32) -> f32 {
    (value / pixel_size).round() * pixel_size
}

pub fn wrap_cloud_x(x: f32, center: f32) -> f32 {
    if x > center + 130.0 {
        x - 260.0
    } else if x < center - 130.0 {
        x + 260.0
    } else {
        x
    }
}

fn sky_palette(phase: f32) -> ([f32; 3], [f32; 3]) {
    let daylight = (0.15 + 0.85 * (phase * std::f32::consts::TAU).sin().max(0.0)).clamp(0.15, 1.0);
    let dusk = (1.0 - ((phase - 0.50).abs() * 8.0).min(1.0)) * (1.0 - daylight);
    (
        [
            lerp(0.035, 0.44, daylight) + dusk * 0.20,
            lerp(0.025, 0.73, daylight) + dusk * 0.08,
            lerp(0.090, 0.94, daylight) + dusk * 0.18,
        ],
        [
            lerp(0.015, 0.25, daylight) + dusk * 0.18,
            lerp(0.020, 0.58, daylight) + dusk * 0.08,
            lerp(0.060, 0.88, daylight) + dusk * 0.20,
        ],
    )
}

fn lerp(start: f32, end: f32, amount: f32) -> f32 {
    start + (end - start) * amount
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothing_is_frame_rate_independent_within_tolerance() {
        let target = Vec2::new(10.0, 5.0);
        let one_step = smooth_position(Vec2::ZERO, target, 8.0, 1.0);
        let mut many_steps = Vec2::ZERO;
        for _ in 0..60 {
            many_steps = smooth_position(many_steps, target, 8.0, 1.0 / 60.0);
        }
        assert!(one_step.distance(many_steps) < 0.0001);
    }

    #[test]
    fn camera_clamps_or_centers_each_axis() {
        assert_eq!(
            clamp_camera_vertical(Vec2::new(-5.0, 90.0), 10.0, 80.0),
            Vec2::new(-5.0, 70.0)
        );
        assert_eq!(
            clamp_camera_vertical(Vec2::new(400.0, 0.0), 100.0, 40.0),
            Vec2::new(400.0, 20.0)
        );
    }

    #[test]
    fn snapping_uses_projection_pixel_size() {
        assert_eq!(snap(1.02, 1.0 / 32.0), 1.03125);
    }

    #[test]
    fn cloud_wrap_is_bounded() {
        assert_eq!(wrap_cloud_x(131.0, 0.0), -129.0);
        assert_eq!(wrap_cloud_x(-131.0, 0.0), 129.0);
        assert_eq!(wrap_cloud_x(50.0, 0.0), 50.0);
    }

    #[test]
    fn centering_resets_camera_and_smoothing_origin() {
        let mut camera = Transform::from_xyz(100.0, 40.0, 100.0);
        let mut rig = CameraRig {
            logical_position: Vec2::new(100.0, 40.0),
        };
        center_camera(Vec2::new(-12.5, 34.0), &mut camera, &mut rig);
        assert_eq!(
            camera.translation,
            Vec3::new(-12.5, 34.0, 0.0) + CAMERA_OFFSET
        );
        assert_eq!(rig.logical_position, Vec2::new(-12.5, 34.0));
    }
}
