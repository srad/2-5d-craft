use bevy::{
    camera::{CameraOutputMode, ScalingMode, Viewport, visibility::RenderLayers},
    prelude::*,
    render::render_resource::BlendState,
    window::PrimaryWindow,
};
use bevy_egui::{EguiContext, PrimaryEguiContext, egui};
use sidecraft_textures::SHOWCASE;

const PREVIEW_LAYER: usize = 1;
const CAMERA_DISTANCE: f32 = 40.0;
const CAMERA_YAW: f32 = 10.0 * std::f32::consts::PI / 180.0;
const CAMERA_PITCH: f32 = 14.0 * std::f32::consts::PI / 180.0;

#[derive(Default, Resource)]
pub(crate) struct PreviewViewport {
    pub logical_rect: Option<egui::Rect>,
}

#[derive(Component)]
pub(crate) struct PreviewCamera;

type PreviewCameraQuery<'w, 's> = Single<
    'w,
    's,
    (&'static mut Camera, &'static mut Projection),
    (With<PreviewCamera>, Without<EguiContext>),
>;

pub(super) fn preview_layer() -> RenderLayers {
    RenderLayers::layer(PREVIEW_LAYER)
}

pub(super) fn spawn_cameras(commands: &mut Commands) -> Entity {
    let preview = commands
        .spawn((
            PreviewCamera,
            Camera3d::default(),
            Camera {
                clear_color: ClearColorConfig::Custom(Color::srgb(0.25, 0.58, 0.88)),
                ..default()
            },
            Projection::from(OrthographicProjection {
                scaling_mode: ScalingMode::WindowSize,
                scale: 1.0 / 32.0,
                ..OrthographicProjection::default_3d()
            }),
            camera_transform(),
            Msaa::Off,
            preview_layer(),
        ))
        .id();
    // The preview viewport must never own the primary Egui context. Otherwise Egui
    // uses the shrinking preview rectangle as its next full-window layout area.
    commands.spawn((
        PrimaryEguiContext,
        Camera2d,
        RenderLayers::none(),
        Camera {
            order: 1,
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::ALPHA_BLENDING),
                clear_color: ClearColorConfig::None,
            },
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
    ));
    preview
}

pub(crate) fn apply_preview_viewport(
    viewport: Res<PreviewViewport>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut camera: PreviewCameraQuery,
) {
    let Some(rect) = viewport.logical_rect else {
        camera.0.is_active = false;
        camera.0.viewport = None;
        return;
    };
    let physical = physical_viewport(
        rect,
        window.scale_factor(),
        UVec2::new(window.physical_width(), window.physical_height()),
    );
    camera.0.is_active = physical.is_some();
    camera.0.viewport = physical;
    if let Projection::Orthographic(projection) = &mut *camera.1 {
        projection.scale = preview_scale(rect.width(), rect.height());
    }
}

fn preview_scale(width: f32, height: f32) -> f32 {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return 1.0;
    }
    (SHOWCASE.inspection_width / width)
        .max(SHOWCASE.inspection_height / height)
        .max(0.000_1)
}

fn camera_transform() -> Transform {
    let target = Vec3::new(SHOWCASE.camera_target[0], SHOWCASE.camera_target[1], 0.0);
    let horizontal = CAMERA_DISTANCE * CAMERA_PITCH.cos();
    let offset = Vec3::new(
        horizontal * CAMERA_YAW.sin(),
        CAMERA_DISTANCE * CAMERA_PITCH.sin(),
        horizontal * CAMERA_YAW.cos(),
    );
    Transform::from_translation(target + offset).looking_at(target, Vec3::Y)
}

fn physical_viewport(rect: egui::Rect, scale: f32, target_size: UVec2) -> Option<Viewport> {
    if !scale.is_finite()
        || scale <= 0.0
        || !rect.min.x.is_finite()
        || !rect.min.y.is_finite()
        || !rect.max.x.is_finite()
        || !rect.max.y.is_finite()
        || rect.width() <= 0.0
        || rect.height() <= 0.0
        || target_size.x == 0
        || target_size.y == 0
    {
        return None;
    }

    let physical_position = UVec2::new(
        (rect.min.x * scale).round().max(0.0) as u32,
        (rect.min.y * scale).round().max(0.0) as u32,
    );
    if physical_position.x >= target_size.x || physical_position.y >= target_size.y {
        return None;
    }

    let requested_size = UVec2::new(
        (rect.width() * scale).round().max(0.0) as u32,
        (rect.height() * scale).round().max(0.0) as u32,
    );
    if requested_size.x == 0 || requested_size.y == 0 {
        return None;
    }

    Some(Viewport {
        physical_position,
        physical_size: requested_size.min(target_size - physical_position),
        ..default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_uses_the_game_showcase_inspection_area() {
        let widescreen = preview_scale(1_000.0, 740.0);
        assert_eq!(
            widescreen,
            (SHOWCASE.inspection_width / 1_000.0).max(SHOWCASE.inspection_height / 740.0)
        );
        assert_eq!(preview_scale(0.0, 740.0), 1.0);
    }

    #[test]
    fn logical_preview_rect_converts_at_common_dpi_scales() {
        let rect = egui::Rect::from_min_size(egui::pos2(440.0, 48.0), egui::vec2(1000.0, 730.0));
        let normal = physical_viewport(rect, 1.0, UVec2::new(1920, 1080)).unwrap();
        assert_eq!(normal.physical_position, UVec2::new(440, 48));
        assert_eq!(normal.physical_size, UVec2::new(1000, 730));
        let high_dpi = physical_viewport(rect, 1.5, UVec2::new(2560, 1440)).unwrap();
        assert_eq!(high_dpi.physical_position, UVec2::new(660, 72));
        assert_eq!(high_dpi.physical_size, UVec2::new(1500, 1095));
    }

    #[test]
    fn preview_viewport_rejects_empty_edge_rect_and_clamps_overflow() {
        let target_size = UVec2::new(1800, 1013);
        let empty_at_right_edge =
            egui::Rect::from_min_size(egui::pos2(1440.0, 168.8), egui::vec2(0.0, 530.4));
        assert!(physical_viewport(empty_at_right_edge, 1.25, target_size).is_none());

        let overflowing =
            egui::Rect::from_min_size(egui::pos2(1400.0, 168.8), egui::vec2(100.0, 700.0));
        let viewport = physical_viewport(overflowing, 1.25, target_size).unwrap();
        assert_eq!(viewport.physical_position, UVec2::new(1750, 211));
        assert_eq!(viewport.physical_size, UVec2::new(50, 802));
        assert!(viewport.physical_position.x + viewport.physical_size.x <= target_size.x);
        assert!(viewport.physical_position.y + viewport.physical_size.y <= target_size.y);
    }
}
