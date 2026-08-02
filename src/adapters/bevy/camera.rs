use crate::{
    AppState,
    adapters::bevy::{environment_flag, player::Player},
    domain::WORLD_HEIGHT,
};
use bevy::camera::{ClearColorConfig, Hdr, ScalingMode};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::ui::IsDefaultUiCamera;
use bevy::ui_render::UiAntiAlias;

pub(crate) const GAME_CAMERA_SCALE: f32 = 1.0 / 32.0;
const CAMERA_DECAY: f32 = 8.0;
const CAMERA_DISTANCE: f32 = 40.0;
const CAMERA_YAW: f32 = 10.0 * std::f32::consts::PI / 180.0;
const CAMERA_PITCH: f32 = 14.0 * std::f32::consts::PI / 180.0;

#[derive(Component)]
pub struct GameCamera;

#[derive(Resource)]
pub(crate) struct CameraRig {
    logical_position: Vec2,
}

#[derive(Resource)]
struct AutomaticScreenshot(Timer);

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_scene)
            .add_systems(OnEnter(AppState::Playing), arm_automatic_screenshot)
            .add_systems(OnEnter(AppState::MainMenu), arm_menu_automatic_screenshot)
            .add_systems(
                Update,
                (
                    follow_player.run_if(in_state(AppState::Playing)),
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

fn arm_menu_automatic_screenshot(commands: Commands) {
    if !environment_flag("SIDECRAFT_AUTOSTART") {
        arm_automatic_screenshot(commands);
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

fn setup_scene(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.32, 0.66, 0.90)),
            ..default()
        },
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::WindowSize,
            scale: GAME_CAMERA_SCALE,
            ..OrthographicProjection::default_3d()
        }),
        camera_transform(Vec3::new(100.0, 40.0, 0.0)),
        Hdr,
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        // Msaa stays on: it is the voxel world's antialiasing. `UiAntiAlias` is
        // the UI-only control, and switching it off is half of Bevy's recipe for
        // a pixel-art look — the other half, `FontSmoothing::None`, lives in the
        // UI theme. Without this, every bevel and border renders soft-edged.
        Msaa::Sample4,
        UiAntiAlias::Off,
        IsDefaultUiCamera,
        GameCamera,
    ));
    commands.insert_resource(CameraRig {
        logical_position: Vec2::new(100.0, 40.0),
    });
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
        projection.area.half_size().y / projected_world_y_scale(),
        WORLD_HEIGHT as f32,
    );
    let target = snap_camera_target(rig.logical_position.extend(0.0), projection.scale);
    *camera.0 = camera_transform(target);
}

pub(crate) fn center_camera(target: Vec2, camera: &mut Transform, rig: &mut CameraRig) {
    rig.logical_position = target;
    *camera = camera_transform(snap_camera_target(target.extend(0.0), GAME_CAMERA_SCALE));
}

pub(crate) fn frame_camera(
    target: Vec2,
    scale: f32,
    camera: &mut Transform,
    projection: &mut Projection,
    rig: &mut CameraRig,
) {
    rig.logical_position = target;
    if let Projection::Orthographic(orthographic) = projection {
        orthographic.scale = scale;
    }
    *camera = camera_transform(snap_camera_target(target.extend(0.0), scale));
}

fn camera_offset() -> Vec3 {
    let horizontal = CAMERA_DISTANCE * CAMERA_PITCH.cos();
    Vec3::new(
        horizontal * CAMERA_YAW.sin(),
        CAMERA_DISTANCE * CAMERA_PITCH.sin(),
        horizontal * CAMERA_YAW.cos(),
    )
}

pub(crate) fn camera_rotation() -> Quat {
    camera_transform(Vec3::ZERO).rotation
}

fn camera_transform(target: Vec3) -> Transform {
    Transform::from_translation(target + camera_offset()).looking_at(target, Vec3::Y)
}

fn snap_camera_target(target: Vec3, pixel_size: f32) -> Vec3 {
    let rotation = camera_rotation();
    let right = rotation * Vec3::X;
    let up = rotation * Vec3::Y;
    let back = rotation * Vec3::Z;
    right * snap(target.dot(right), pixel_size)
        + up * snap(target.dot(up), pixel_size)
        + back * target.dot(back)
}

fn projected_world_y_scale() -> f32 {
    Vec3::Y.dot(camera_rotation() * Vec3::Y).abs()
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
    fn oblique_projection_exposes_depth_without_collapsing_the_foreground() {
        let inverse = camera_rotation().inverse();
        let foreground_x = inverse * Vec3::X;
        let foreground_y = inverse * Vec3::Y;
        let depth = inverse * Vec3::NEG_Z;
        let slope = foreground_x.y.abs().atan2(foreground_x.x.abs());

        assert!(foreground_x.x.abs() >= 0.90);
        assert!(foreground_y.y.abs() >= 0.95);
        assert!(depth.x.abs() >= 0.16);
        assert!(depth.y.abs() >= 0.20);
        assert!(slope <= 6.5_f32.to_radians());
    }

    #[test]
    fn camera_target_snaps_in_view_space() {
        let snapped = snap_camera_target(Vec3::new(1.017, 2.013, 0.0), GAME_CAMERA_SCALE);
        let local = camera_rotation().inverse() * snapped;
        assert!(
            (local.x / GAME_CAMERA_SCALE - (local.x / GAME_CAMERA_SCALE).round()).abs() < 0.0001
        );
        assert!(
            (local.y / GAME_CAMERA_SCALE - (local.y / GAME_CAMERA_SCALE).round()).abs() < 0.0001
        );
    }

    #[test]
    fn centering_resets_camera_and_smoothing_origin() {
        let mut camera = Transform::from_xyz(100.0, 40.0, 100.0);
        let mut rig = CameraRig {
            logical_position: Vec2::new(100.0, 40.0),
        };
        center_camera(Vec2::new(-12.5, 34.0), &mut camera, &mut rig);
        let expected_target = snap_camera_target(Vec3::new(-12.5, 34.0, 0.0), GAME_CAMERA_SCALE);
        assert_eq!(
            camera.translation,
            camera_transform(expected_target).translation
        );
        assert_eq!(rig.logical_position, Vec2::new(-12.5, 34.0));
    }
}
