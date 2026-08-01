use super::{camera::preview_layer, lighting::PreviewLighting};
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use sidecraft_textures::{PackImage, ResolvedPack};

const BACKDROP_WIDTH: u32 = 384;
const BACKDROP_HEIGHT: u32 = 216;

#[derive(Resource)]
pub(crate) struct PreviewEnvironment {
    image: Handle<Image>,
}

pub(super) fn spawn_environment(
    commands: &mut Commands,
    camera: Entity,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) {
    let image = images.add(to_bevy_image(&PackImage::solid(
        BACKDROP_WIDTH,
        BACKDROP_HEIGHT,
        [64, 148, 224, 255],
    )));
    let material = materials.add(StandardMaterial {
        base_color_texture: Some(image.clone()),
        unlit: true,
        cull_mode: None,
        ..default()
    });
    let backdrop = commands
        .spawn((
            Mesh3d(meshes.add(Rectangle::new(64.0, 36.0))),
            MeshMaterial3d(material),
            Transform::from_xyz(0.0, 0.0, -60.0),
            preview_layer(),
        ))
        .id();
    commands.entity(camera).add_child(backdrop);
    commands.insert_resource(PreviewEnvironment { image });
}

pub(super) fn update_environment(
    environment: &PreviewEnvironment,
    pack: &ResolvedPack,
    mode: PreviewLighting,
    images: &mut Assets<Image>,
) {
    if let Some(mut image) = images.get_mut(&environment.image) {
        *image = to_bevy_image(&compose_environment(pack, mode));
    }
}

fn compose_environment(pack: &ResolvedPack, mode: PreviewLighting) -> PackImage {
    let (bottom, top) = match mode {
        PreviewLighting::Day => ([112, 186, 240], [64, 148, 224]),
        PreviewLighting::Night => ([6, 5, 22], [3, 4, 14]),
    };
    let mut image = PackImage::solid(BACKDROP_WIDTH, BACKDROP_HEIGHT, [0, 0, 0, 255]);
    for y in 0..BACKDROP_HEIGHT {
        let band = (y / 12) * 12;
        let amount = band as f32 / (BACKDROP_HEIGHT - 1) as f32;
        let color = mix(bottom, top, amount);
        for x in 0..BACKDROP_WIDTH {
            image.set_pixel(x, y, [color[0], color[1], color[2], 255]);
        }
    }
    match mode {
        PreviewLighting::Day => {
            blit_scaled(&mut image, &pack.sun, 310, 24, 48, 48, 1.0);
            blit_scaled(&mut image, &pack.cloud, 36, 34, 128, 48, 0.86);
            blit_scaled(&mut image, &pack.cloud, 212, 84, 96, 36, 0.72);
        }
        PreviewLighting::Night => {
            blit_scaled(
                &mut image,
                &pack.stars,
                0,
                0,
                BACKDROP_WIDTH,
                BACKDROP_HEIGHT,
                0.92,
            );
            blit_scaled(&mut image, &pack.moons[4], 306, 28, 42, 42, 1.0);
            blit_scaled(&mut image, &pack.cloud, 44, 48, 128, 48, 0.42);
        }
    }
    image
}

fn mix(start: [u8; 3], end: [u8; 3], amount: f32) -> [u8; 3] {
    std::array::from_fn(|channel| {
        (f32::from(start[channel]) + (f32::from(end[channel]) - f32::from(start[channel])) * amount)
            .round() as u8
    })
}

#[allow(clippy::too_many_arguments)]
fn blit_scaled(
    destination: &mut PackImage,
    source: &PackImage,
    left: u32,
    top: u32,
    width: u32,
    height: u32,
    opacity: f32,
) {
    for y in 0..height {
        for x in 0..width {
            let source_color = source.pixel(x * source.width / width, y * source.height / height);
            let alpha = f32::from(source_color[3]) / 255.0 * opacity;
            if alpha <= 0.0 {
                continue;
            }
            let target_x = left + x;
            let target_y = top + y;
            if target_x >= destination.width || target_y >= destination.height {
                continue;
            }
            let target = destination.pixel(target_x, target_y);
            let mut blended = [0, 0, 0, 255];
            for channel in 0..3 {
                blended[channel] = (f32::from(target[channel]) * (1.0 - alpha)
                    + f32::from(source_color[channel]) * alpha)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
            destination.set_pixel(target_x, target_y, blended);
        }
    }
}

fn to_bevy_image(image: &PackImage) -> Image {
    let mut output = Image::new(
        Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        image.pixels.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    output.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::nearest());
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use sidecraft_textures::{GenerateOptions, generate_pack, resolve_generated_pack};

    #[test]
    fn day_and_night_environments_are_distinct_and_stable() {
        let pack =
            resolve_generated_pack(&generate_pack(&GenerateOptions::default()).unwrap()).unwrap();
        let day = compose_environment(&pack, PreviewLighting::Day);
        let replay = compose_environment(&pack, PreviewLighting::Day);
        let night = compose_environment(&pack, PreviewLighting::Night);
        assert_eq!(day, replay);
        assert_ne!(day, night);
        assert_eq!((day.width, day.height), (BACKDROP_WIDTH, BACKDROP_HEIGHT));
    }
}
