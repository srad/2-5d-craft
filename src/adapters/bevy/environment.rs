use bevy::{
    asset::RenderAssetUsages,
    camera::Projection,
    image::ImageSampler,
    light::{NotShadowCaster, NotShadowReceiver},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::adapters::bevy::{DayCycleResource, RuntimeSet, camera::GameCamera};

const SKY_BANDS: usize = 8;
const SKY_DEPTH: f32 = -80.0;
const STAR_DEPTH: f32 = -76.0;
const BODY_DEPTH: f32 = -72.0;
const CLOUD_DEPTH: f32 = -64.0;
const BODY_SIZE: f32 = 3.5;

#[derive(Component)]
struct EnvironmentVisual;

#[derive(Component)]
struct SkyBand(usize);

#[derive(Component)]
struct StarField;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum CelestialBody {
    Sun,
    Moon,
}

#[derive(Component)]
struct Cloud;

#[derive(Component)]
struct EnvironmentLight;

#[derive(Resource)]
struct MoonMaterials(Vec<Handle<StandardMaterial>>);

type EnvironmentVisualQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Transform,
        &'static mut MeshMaterial3d<StandardMaterial>,
        Option<&'static SkyBand>,
        Option<&'static StarField>,
        Option<&'static CelestialBody>,
        Option<&'static Cloud>,
    ),
    (With<EnvironmentVisual>, Without<EnvironmentLight>),
>;

pub(crate) struct EnvironmentPlugin;

impl Plugin for EnvironmentPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, setup_environment)
            .add_systems(Update, animate_environment.in_set(RuntimeSet::Derived));
    }
}

fn setup_environment(
    mut commands: Commands,
    camera: Single<Entity, With<GameCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let sky_mesh = meshes.add(Rectangle::new(512.0, 16.0));
    let mut sky_materials = Vec::with_capacity(SKY_BANDS);
    for _ in 0..SKY_BANDS {
        sky_materials.push(materials.add(unlit_material(None, Color::WHITE)));
    }

    let star_texture = images.add(star_image());
    let star_material = materials.add(unlit_material(
        Some(star_texture),
        Color::srgba(1.0, 1.0, 1.0, 0.0),
    ));
    let star_mesh = meshes.add(Rectangle::new(72.0, 40.0));

    let sun_texture = images.add(sun_image());
    let sun_material = materials.add(unlit_material(
        Some(sun_texture),
        Color::srgba(1.0, 1.0, 1.0, 1.0),
    ));
    let body_mesh = meshes.add(Rectangle::new(BODY_SIZE, BODY_SIZE));

    let moon_materials = (0..8)
        .map(|phase| {
            let texture = images.add(moon_image(phase));
            materials.add(unlit_material(
                Some(texture),
                Color::srgba(1.0, 1.0, 1.0, 1.0),
            ))
        })
        .collect::<Vec<_>>();
    let initial_moon = moon_materials[0].clone();
    commands.insert_resource(MoonMaterials(moon_materials));

    let cloud_texture = images.add(cloud_image());
    let cloud_mesh = meshes.add(Rectangle::new(8.0, 3.0));
    let cloud_material = materials.add(unlit_material(
        Some(cloud_texture),
        Color::srgba(0.92, 0.96, 1.0, 0.72),
    ));

    commands.entity(*camera).with_children(|parent| {
        for (index, material) in sky_materials.into_iter().enumerate() {
            parent.spawn((
                Mesh3d(sky_mesh.clone()),
                MeshMaterial3d(material),
                Transform::from_xyz(0.0, index as f32 * 16.0 - 56.0, SKY_DEPTH),
                EnvironmentVisual,
                SkyBand(index),
                NotShadowCaster,
                NotShadowReceiver,
            ));
        }
        parent.spawn((
            Mesh3d(star_mesh),
            MeshMaterial3d(star_material),
            Transform::from_xyz(0.0, 0.0, STAR_DEPTH),
            EnvironmentVisual,
            StarField,
            NotShadowCaster,
            NotShadowReceiver,
        ));
        parent.spawn((
            Mesh3d(body_mesh.clone()),
            MeshMaterial3d(sun_material),
            Transform::from_xyz(-20.0, 0.0, BODY_DEPTH),
            EnvironmentVisual,
            CelestialBody::Sun,
            NotShadowCaster,
            NotShadowReceiver,
        ));
        parent.spawn((
            Mesh3d(body_mesh),
            MeshMaterial3d(initial_moon),
            Transform::from_xyz(20.0, 0.0, BODY_DEPTH),
            EnvironmentVisual,
            CelestialBody::Moon,
            NotShadowCaster,
            NotShadowReceiver,
        ));
        for index in 0..7 {
            parent.spawn((
                Mesh3d(cloud_mesh.clone()),
                MeshMaterial3d(cloud_material.clone()),
                Transform::from_xyz(
                    index as f32 * 18.0 - 54.0,
                    7.0 + (index % 3) as f32 * 4.0,
                    CLOUD_DEPTH - (index % 2) as f32,
                ),
                EnvironmentVisual,
                Cloud,
                NotShadowCaster,
                NotShadowReceiver,
            ));
        }
    });

    commands.spawn((
        DirectionalLight {
            illuminance: 9_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, -0.4, 0.0)),
        EnvironmentLight,
    ));
}

#[allow(clippy::too_many_arguments)]
fn animate_environment(
    time: Res<Time>,
    day: Res<DayCycleResource>,
    moon_materials: Res<MoonMaterials>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut camera: Single<(&mut AmbientLight, &Projection, &GlobalTransform), With<GameCamera>>,
    mut visuals: EnvironmentVisualQuery,
    mut light: Single<(&mut DirectionalLight, &mut Transform), With<EnvironmentLight>>,
) {
    let Projection::Orthographic(projection) = camera.1 else {
        return;
    };
    let half_view = projection.area.half_size();
    let (sun_position, moon_position) = celestial_positions(day.phase(), half_view);
    let (bottom, top) = sky_palette(day.daylight(), day.twilight());
    let night = day.night();

    for (mut transform, mut handle, band, stars, body, cloud) in &mut visuals {
        if let Some(band) = band {
            let fraction = band.0 as f32 / (SKY_BANDS - 1) as f32;
            if let Some(mut material) = materials.get_mut(&handle.0) {
                material.base_color = Color::srgb(
                    lerp(bottom[0], top[0], fraction),
                    lerp(bottom[1], top[1], fraction),
                    lerp(bottom[2], top[2], fraction),
                );
            }
        } else if stars.is_some() {
            transform.rotation = Quat::from_rotation_z(-day.phase() * std::f32::consts::TAU);
            if let Some(mut material) = materials.get_mut(&handle.0) {
                material.base_color = Color::srgba(0.82, 0.88, 1.0, night * 0.88);
            }
        } else if let Some(body) = body {
            let (position, elevation) = match body {
                CelestialBody::Sun => (sun_position, sun_position.y / half_view.y.max(0.01)),
                CelestialBody::Moon => {
                    *handle = MeshMaterial3d(moon_materials.0[day.moon_phase().index()].clone());
                    (moon_position, moon_position.y / half_view.y.max(0.01))
                }
            };
            transform.translation = Vec3::new(position.x, position.y, BODY_DEPTH);
            if let Some(mut material) = materials.get_mut(&handle.0) {
                material.base_color = Color::srgba(1.0, 1.0, 1.0, horizon_alpha(elevation));
            }
        } else if cloud.is_some() {
            transform.translation.x = wrap_cloud_x(
                transform.translation.x + time.delta_secs() * 0.7,
                half_view.x.max(70.0),
            );
            if let Some(mut material) = materials.get_mut(&handle.0) {
                material.base_color = Color::srgba(
                    lerp(0.42, 0.96, day.daylight()),
                    lerp(0.46, 0.98, day.daylight()),
                    lerp(0.58, 1.0, day.daylight()),
                    lerp(0.42, 0.72, day.daylight()),
                );
            }
        }
    }

    camera.0.brightness = lerp(25.0, 140.0, day.daylight());
    light.0.illuminance = lerp(300.0, 9_000.0, day.daylight());
    light.0.color = Color::srgb(
        lerp(0.46, 1.0, day.daylight()),
        lerp(0.56, 0.98, day.daylight()),
        lerp(0.84, 0.92, day.daylight()),
    );

    let active = if day.sun_elevation() >= 0.0 {
        sun_position
    } else {
        moon_position
    };
    let local_direction = Vec3::new(-active.x, -active.y, 24.0).normalize();
    let world_direction = camera.2.rotation() * local_direction;
    light.1.rotation = Quat::from_rotation_arc(Vec3::NEG_Z, world_direction);
}

fn celestial_positions(phase: f32, half_view: Vec2) -> (Vec2, Vec2) {
    let angle = phase * std::f32::consts::TAU;
    let radii = Vec2::new(
        (half_view.x - BODY_SIZE).max(4.0) * 0.88,
        (half_view.y - BODY_SIZE * 0.5).max(3.0) * 0.82,
    );
    let sun = Vec2::new(-angle.cos() * radii.x, angle.sin() * radii.y);
    (sun, -sun)
}

fn horizon_alpha(elevation: f32) -> f32 {
    smoothstep(-0.10, 0.01, elevation)
}

fn wrap_cloud_x(x: f32, radius: f32) -> f32 {
    if x > radius {
        x - radius * 2.0
    } else if x < -radius {
        x + radius * 2.0
    } else {
        x
    }
}

fn sky_palette(daylight: f32, twilight: f32) -> ([f32; 3], [f32; 3]) {
    (
        [
            lerp(0.035, 0.44, daylight) + twilight * 0.28,
            lerp(0.025, 0.73, daylight) + twilight * 0.08,
            lerp(0.090, 0.94, daylight) + twilight * 0.12,
        ],
        [
            lerp(0.015, 0.25, daylight) + twilight * 0.20,
            lerp(0.020, 0.58, daylight) + twilight * 0.06,
            lerp(0.060, 0.88, daylight) + twilight * 0.16,
        ],
    )
}

fn unlit_material(texture: Option<Handle<Image>>, base_color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color,
        base_color_texture: texture,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        ..default()
    }
}

fn sun_image() -> Image {
    let mut pixels = vec![0; 32 * 32 * 4];
    for y in 3..29 {
        for x in 3..29 {
            let edge = !(6..26).contains(&x) || !(6..26).contains(&y);
            set_pixel(
                &mut pixels,
                32,
                x,
                y,
                if edge {
                    [255, 194, 55, 255]
                } else {
                    [255, 239, 139, 255]
                },
            );
        }
    }
    nearest_image(32, 32, pixels)
}

fn moon_image(phase: usize) -> Image {
    let mut pixels = vec![0; 32 * 32 * 4];
    let illumination = [1.0, 0.75, 0.5, 0.25, 0.0, 0.25, 0.5, 0.75][phase];
    let waxing = phase >= 5;
    for y in 0..32 {
        for x in 0..32 {
            let dx = x as f32 + 0.5 - 16.0;
            let dy = y as f32 + 0.5 - 16.0;
            let radius = (dx * dx + dy * dy).sqrt();
            if radius > 13.5 {
                continue;
            }
            let normalized_x = dx / 13.5;
            let lit_side = if waxing { normalized_x } else { -normalized_x };
            let threshold = 1.0 - illumination * 2.0;
            let lit = lit_side >= threshold;
            let crater = ((x * 17 + y * 31 + phase * 13) % 29) < 3;
            let color = if lit {
                if crater {
                    [166, 177, 190, 255]
                } else {
                    [221, 228, 229, 255]
                }
            } else {
                [42, 52, 75, if phase == 4 { 72 } else { 180 }]
            };
            set_pixel(&mut pixels, 32, x, y, color);
        }
    }
    nearest_image(32, 32, pixels)
}

fn star_image() -> Image {
    const WIDTH: usize = 512;
    const HEIGHT: usize = 256;
    let mut pixels = vec![0; WIDTH * HEIGHT * 4];
    let mut state = 0x5eed_fade_u32;
    for index in 0..160 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let x = (state as usize % (WIDTH - 4)) + 2;
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let y = (state as usize % (HEIGHT - 4)) + 2;
        let brightness = 170 + (state % 86) as u8;
        set_pixel(&mut pixels, WIDTH, x, y, [brightness, brightness, 255, 255]);
        if index % 9 == 0 {
            set_pixel(
                &mut pixels,
                WIDTH,
                x + 1,
                y,
                [brightness, brightness, 255, 190],
            );
        }
    }
    nearest_image(WIDTH as u32, HEIGHT as u32, pixels)
}

fn cloud_image() -> Image {
    const WIDTH: usize = 64;
    const HEIGHT: usize = 24;
    let mut pixels = vec![0; WIDTH * HEIGHT * 4];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let base = (7..17).contains(&y) && (4..60).contains(&x);
            let puff_left = ellipse_contains(x, y, 20, 10, 14, 9);
            let puff_center = ellipse_contains(x, y, 34, 8, 17, 11);
            let puff_right = ellipse_contains(x, y, 49, 11, 12, 8);
            if base || puff_left || puff_center || puff_right {
                let shade = if y > 14 { 210 } else { 255 };
                set_pixel(&mut pixels, WIDTH, x, y, [shade, shade, shade, 255]);
            }
        }
    }
    nearest_image(WIDTH as u32, HEIGHT as u32, pixels)
}

fn ellipse_contains(
    x: usize,
    y: usize,
    center_x: usize,
    center_y: usize,
    radius_x: usize,
    radius_y: usize,
) -> bool {
    let dx = (x as f32 - center_x as f32) / radius_x as f32;
    let dy = (y as f32 - center_y as f32) / radius_y as f32;
    dx * dx + dy * dy <= 1.0
}

fn set_pixel(pixels: &mut [u8], width: usize, x: usize, y: usize, color: [u8; 4]) {
    let offset = (y * width + x) * 4;
    pixels[offset..offset + 4].copy_from_slice(&color);
}

fn nearest_image(width: u32, height: u32, pixels: Vec<u8>) -> Image {
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
}

fn lerp(start: f32, end: f32, amount: f32) -> f32 {
    start + (end - start) * amount
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let amount = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    amount * amount * (3.0 - 2.0 * amount)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sun_and_moon_are_opposite_at_key_times() {
        for ticks in [0, 6_000, 12_000, 18_000] {
            let phase = ticks as f32 / crate::domain::TICKS_PER_DAY as f32;
            let (sun, moon) = celestial_positions(phase, Vec2::new(20.0, 12.0));
            assert!(sun.distance(-moon) < 0.0001);
        }
    }

    #[test]
    fn bodies_fade_only_below_the_horizon() {
        assert!(horizon_alpha(0.0) > 0.9);
        assert_eq!(horizon_alpha(-0.1), 0.0);
        assert_eq!(horizon_alpha(0.01), 1.0);
    }

    #[test]
    fn generated_celestial_textures_have_visible_pixels() {
        assert!(
            sun_image()
                .data
                .as_ref()
                .unwrap()
                .chunks_exact(4)
                .any(|pixel| pixel[3] > 0)
        );
        for phase in 0..8 {
            assert!(
                moon_image(phase)
                    .data
                    .as_ref()
                    .unwrap()
                    .chunks_exact(4)
                    .any(|pixel| pixel[3] > 0)
            );
        }
        assert!(
            star_image()
                .data
                .as_ref()
                .unwrap()
                .chunks_exact(4)
                .filter(|pixel| pixel[3] > 0)
                .count()
                >= 160
        );
        assert!(
            cloud_image()
                .data
                .as_ref()
                .unwrap()
                .chunks_exact(4)
                .any(|pixel| pixel[3] > 0)
        );
    }

    #[test]
    fn cloud_wrap_is_bounded() {
        assert_eq!(wrap_cloud_x(71.0, 70.0), -69.0);
        assert_eq!(wrap_cloud_x(-71.0, 70.0), 69.0);
    }
}
