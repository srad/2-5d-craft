use super::{
    camera::preview_layer,
    lighting::{LightField, PreviewLighting, VoxelFace, player_tint, torch_color},
};
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageSampler, ImageSamplerDescriptor},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use sidecraft_textures::{
    BLOCK_SIZE, BlockKind, Face as PackFace, PlayerPalette, ResolvedPack, SHOWCASE,
    ShowcaseTorchMount, VARIANT_COUNT,
};

const FACE_COUNT: u32 = 3;
const GUTTER: u32 = 2;
const ATLAS_CELL: u32 = BLOCK_SIZE + GUTTER * 2;
const ATLAS_TILES: u32 = BlockKind::ALL.len() as u32 * FACE_COUNT * VARIANT_COUNT as u32;

#[derive(Resource)]
pub(crate) struct PreviewScene {
    atlas: Handle<Image>,
    opaque_mesh: Handle<Mesh>,
    cutout_mesh: Handle<Mesh>,
    torch_mesh: Handle<Mesh>,
    player_materials: Vec<Handle<StandardMaterial>>,
}

pub(super) fn spawn_scene(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) {
    let atlas = images.add(empty_atlas_image());
    let opaque_material = materials.add(StandardMaterial {
        base_color_texture: Some(atlas.clone()),
        unlit: true,
        ..default()
    });
    let cutout_material = materials.add(StandardMaterial {
        base_color_texture: Some(atlas.clone()),
        alpha_mode: AlphaMode::Mask(0.38),
        unlit: true,
        cull_mode: None,
        ..default()
    });
    let opaque_mesh = meshes.add(MeshBuilder::default().finish());
    let cutout_mesh = meshes.add(MeshBuilder::default().finish());
    let torch_mesh = meshes.add(MeshBuilder::default().finish());
    commands.spawn((
        Mesh3d(opaque_mesh.clone()),
        MeshMaterial3d(opaque_material),
        preview_layer(),
    ));
    commands.spawn((
        Mesh3d(cutout_mesh.clone()),
        MeshMaterial3d(cutout_material.clone()),
        preview_layer(),
    ));
    commands.spawn((
        Mesh3d(torch_mesh.clone()),
        MeshMaterial3d(cutout_material),
        preview_layer(),
    ));

    let player_cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let player_materials = player_colors(PlayerPalette::default(), PreviewLighting::Day)
        .into_iter()
        .map(|base_color| {
            materials.add(StandardMaterial {
                base_color,
                unlit: true,
                ..default()
            })
        })
        .collect::<Vec<_>>();
    spawn_player(commands, &player_cube, &player_materials);

    commands.insert_resource(PreviewScene {
        atlas,
        opaque_mesh,
        cutout_mesh,
        torch_mesh,
        player_materials,
    });
}

pub(super) fn update_scene(
    scene: &PreviewScene,
    pack: &ResolvedPack,
    mode: PreviewLighting,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) {
    if let Some(mut image) = images.get_mut(&scene.atlas) {
        *image = atlas_image(pack);
    }
    let built = build_scene_meshes(mode);
    if let Some(mut mesh) = meshes.get_mut(&scene.opaque_mesh) {
        *mesh = built.opaque;
    }
    if let Some(mut mesh) = meshes.get_mut(&scene.cutout_mesh) {
        *mesh = built.cutout;
    }
    if let Some(mut mesh) = meshes.get_mut(&scene.torch_mesh) {
        *mesh = built.torch;
    }
    for (handle, color) in scene
        .player_materials
        .iter()
        .zip(player_colors(pack.player, mode))
    {
        if let Some(mut material) = materials.get_mut(handle) {
            material.base_color = color;
        }
    }
}

struct BuiltScene {
    opaque: Mesh,
    cutout: Mesh,
    torch: Mesh,
}

fn build_scene_meshes(mode: PreviewLighting) -> BuiltScene {
    let light = LightField::calculate();
    let mut opaque = MeshBuilder::default();
    let mut cutout = MeshBuilder::default();
    let mut torch = MeshBuilder::default();
    for x in SHOWCASE.min_x..SHOWCASE.min_x + i64::from(SHOWCASE.width) {
        for y in 0..SHOWCASE.height {
            for depth in 0..i32::from(SHOWCASE.depth) {
                let Some(cell) = SHOWCASE.cell(x, y, depth) else {
                    continue;
                };
                let variant = variant(x, y, depth, cell.block);
                if let Some(mount) = cell.torch_mount {
                    torch.push_torch(x as f32, y as f32, variant, mount, mode);
                    continue;
                }
                let builder = if cell.block == BlockKind::Leaves {
                    &mut cutout
                } else {
                    &mut opaque
                };
                for face in VoxelFace::ALL {
                    let (dx, dy, dd) = face.neighbor();
                    if SHOWCASE
                        .cell(x + i64::from(dx), y + dy, depth + dd)
                        .is_some_and(|neighbor| neighbor.block != BlockKind::Torch)
                    {
                        continue;
                    }
                    builder.push_voxel_face(
                        x as f32,
                        y as f32,
                        depth,
                        cell.block,
                        variant,
                        face,
                        light.face_color(mode, x, y, depth, face),
                    );
                }
            }
        }
    }
    BuiltScene {
        opaque: opaque.finish(),
        cutout: cutout.finish(),
        torch: torch.finish(),
    }
}

fn spawn_player(
    commands: &mut Commands,
    cube: &Handle<Mesh>,
    materials: &[Handle<StandardMaterial>],
) {
    commands
        .spawn((Transform::from_xyz(4.0, 13.1, 0.0), Visibility::default()))
        .with_children(|player| {
            player
                .spawn((
                    Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_3)),
                    Visibility::default(),
                ))
                .with_children(|parts| {
                    spawn_player_part(
                        parts,
                        cube,
                        &materials[1],
                        Vec3::new(0.0, 0.10, 0.0),
                        Vec3::new(0.62, 0.68, 0.38),
                    );
                    spawn_player_part(
                        parts,
                        cube,
                        &materials[0],
                        Vec3::new(0.0, 0.67, 0.0),
                        Vec3::new(0.56, 0.56, 0.50),
                    );
                    spawn_player_part(
                        parts,
                        cube,
                        &materials[3],
                        Vec3::new(0.0, 0.91, -0.01),
                        Vec3::new(0.60, 0.15, 0.54),
                    );
                    for x in [-0.42, 0.42] {
                        spawn_player_part(
                            parts,
                            cube,
                            &materials[0],
                            Vec3::new(x, 0.08, 0.0),
                            Vec3::new(0.20, 0.64, 0.24),
                        );
                    }
                    for x in [-0.18, 0.18] {
                        spawn_player_part(
                            parts,
                            cube,
                            &materials[2],
                            Vec3::new(x, -0.55, 0.0),
                            Vec3::new(0.25, 0.58, 0.28),
                        );
                        spawn_player_part(
                            parts,
                            cube,
                            &materials[4],
                            Vec3::new(x, -0.84, 0.07),
                            Vec3::new(0.27, 0.13, 0.40),
                        );
                    }
                    for x in [-0.14, 0.14] {
                        spawn_player_part(
                            parts,
                            cube,
                            &materials[4],
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
        preview_layer(),
    ));
}

fn player_colors(palette: PlayerPalette, mode: PreviewLighting) -> [Color; 5] {
    let tint = player_tint(mode);
    [
        palette.skin,
        palette.shirt,
        palette.pants,
        palette.hair,
        palette.details,
    ]
    .map(|color| {
        Color::srgb(
            f32::from(color[0]) / 255.0 * tint[0],
            f32::from(color[1]) / 255.0 * tint[1],
            f32::from(color[2]) / 255.0 * tint[2],
        )
    })
}

#[derive(Default)]
struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl MeshBuilder {
    #[allow(clippy::too_many_arguments)]
    fn push_voxel_face(
        &mut self,
        x: f32,
        y: f32,
        depth: i32,
        block: BlockKind,
        variant: usize,
        face: VoxelFace,
        color: [f32; 4],
    ) {
        let front = -(depth as f32) + 0.5;
        let back = front - 1.0;
        let corners = face_corners([x, y, back], [x + 1.0, y + 1.0, front], face);
        self.push_quad(
            corners,
            face.normal(),
            tile_uvs(tile(block, pack_face(face), variant)),
            [color; 4],
        );
    }

    fn push_torch(
        &mut self,
        x: f32,
        y: f32,
        variant: usize,
        mount: ShowcaseTorchMount,
        mode: PreviewLighting,
    ) {
        let (center, angle) = match mount {
            ShowcaseTorchMount::Floor => (Vec3::new(x + 0.5, y + 0.39, 0.30), 0.0),
            ShowcaseTorchMount::WallLeft => {
                (Vec3::new(x + 0.28, y + 0.48, 0.30), 25.0_f32.to_radians())
            }
            ShowcaseTorchMount::WallRight => {
                (Vec3::new(x + 0.72, y + 0.48, 0.30), -25.0_f32.to_radians())
            }
        };
        let transform =
            Transform::from_translation(center).with_rotation(Quat::from_rotation_z(angle));
        let min = [-0.09, -0.34, -0.08];
        let max = [0.09, 0.34, 0.08];
        for face in VoxelFace::ALL {
            let corners = face_corners(min, max, face)
                .map(|point| transform.transform_point(Vec3::from(point)).to_array());
            let color = torch_color(mode, matches!(face, VoxelFace::Top));
            self.push_quad(
                corners,
                (transform.rotation * Vec3::from(face.normal())).to_array(),
                tile_uvs(tile(BlockKind::Torch, pack_face(face), variant)),
                [color; 4],
            );
        }
    }

    fn push_quad(
        &mut self,
        corners: [[f32; 3]; 4],
        normal: [f32; 3],
        uvs: [[f32; 2]; 4],
        colors: [[f32; 4]; 4],
    ) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&corners);
        self.normals.extend_from_slice(&[normal; 4]);
        self.uvs.extend_from_slice(&uvs);
        self.colors.extend_from_slice(&colors);
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn finish(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

fn face_corners(min: [f32; 3], max: [f32; 3], face: VoxelFace) -> [[f32; 3]; 4] {
    let [left, bottom, back] = min;
    let [right, top, front] = max;
    match face {
        VoxelFace::Front => [
            [left, bottom, front],
            [right, bottom, front],
            [right, top, front],
            [left, top, front],
        ],
        VoxelFace::Back => [
            [right, bottom, back],
            [left, bottom, back],
            [left, top, back],
            [right, top, back],
        ],
        VoxelFace::Right => [
            [right, bottom, front],
            [right, bottom, back],
            [right, top, back],
            [right, top, front],
        ],
        VoxelFace::Left => [
            [left, bottom, back],
            [left, bottom, front],
            [left, top, front],
            [left, top, back],
        ],
        VoxelFace::Top => [
            [left, top, front],
            [right, top, front],
            [right, top, back],
            [left, top, back],
        ],
        VoxelFace::Bottom => [
            [left, bottom, back],
            [right, bottom, back],
            [right, bottom, front],
            [left, bottom, front],
        ],
    }
}

fn pack_face(face: VoxelFace) -> PackFace {
    match face {
        VoxelFace::Top => PackFace::Top,
        VoxelFace::Bottom => PackFace::Bottom,
        VoxelFace::Front | VoxelFace::Back | VoxelFace::Right | VoxelFace::Left => PackFace::Side,
    }
}

fn tile(block: BlockKind, face: PackFace, variant: usize) -> u32 {
    let block_index = BlockKind::ALL
        .iter()
        .position(|candidate| *candidate == block)
        .expect("showcase blocks belong to the texture-pack catalog") as u32;
    (block_index * FACE_COUNT + face_index(face)) * VARIANT_COUNT as u32
        + variant as u32 % VARIANT_COUNT as u32
}

fn face_index(face: PackFace) -> u32 {
    match face {
        PackFace::Side => 0,
        PackFace::Top => 1,
        PackFace::Bottom => 2,
    }
}

fn tile_uvs(tile: u32) -> [[f32; 2]; 4] {
    let atlas_width = (ATLAS_TILES * ATLAS_CELL) as f32;
    let start = tile * ATLAS_CELL + GUTTER;
    let u0 = start as f32 / atlas_width;
    let u1 = (start + BLOCK_SIZE) as f32 / atlas_width;
    let v0 = GUTTER as f32 / ATLAS_CELL as f32;
    let v1 = (GUTTER + BLOCK_SIZE) as f32 / ATLAS_CELL as f32;
    [[u0, v1], [u1, v1], [u1, v0], [u0, v0]]
}

fn variant(x: i64, y: i32, depth: i32, block: BlockKind) -> usize {
    let block_index = BlockKind::ALL
        .iter()
        .position(|candidate| *candidate == block)
        .expect("showcase block");
    let mut value = SHOWCASE.seed
        ^ (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (y as u64).rotate_left(19)
        ^ (depth as u64).rotate_left(37)
        ^ (block_index as u64).rotate_left(51);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    ((value ^ (value >> 31)) % VARIANT_COUNT as u64) as usize
}

fn empty_atlas_image() -> Image {
    let width = ATLAS_TILES * ATLAS_CELL;
    bevy_image(
        width,
        ATLAS_CELL,
        vec![0; (width * ATLAS_CELL * 4) as usize],
    )
}

fn atlas_image(pack: &ResolvedPack) -> Image {
    let width = ATLAS_TILES * ATLAS_CELL;
    let mut pixels = vec![0; (width * ATLAS_CELL * 4) as usize];
    for block in BlockKind::ALL {
        for face in PackFace::ALL {
            for variant in 0..VARIANT_COUNT {
                let tile = tile(block, face, variant);
                let source = pack.block(block, face, variant);
                for cell_y in 0..ATLAS_CELL {
                    let source_y = cell_y.saturating_sub(GUTTER).min(BLOCK_SIZE - 1);
                    for cell_x in 0..ATLAS_CELL {
                        let source_x = cell_x.saturating_sub(GUTTER).min(BLOCK_SIZE - 1);
                        let source_index = ((source_y * BLOCK_SIZE + source_x) * 4) as usize;
                        let target_x = tile * ATLAS_CELL + cell_x;
                        let target_index = ((cell_y * width + target_x) * 4) as usize;
                        pixels[target_index..target_index + 4]
                            .copy_from_slice(&source.pixels[source_index..source_index + 4]);
                    }
                }
            }
        }
    }
    bevy_image(width, ATLAS_CELL, pixels)
}

fn bevy_image(width: u32, height: u32, pixels: Vec<u8>) -> Image {
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
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::nearest());
    image
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;
    use sidecraft_textures::{GenerateOptions, generate_pack, resolve_generated_pack};

    #[test]
    fn fixed_scene_builds_real_opaque_cutout_and_torch_geometry() {
        let built = build_scene_meshes(PreviewLighting::Day);
        assert!(built.opaque.count_vertices() > 0);
        assert!(built.cutout.count_vertices() > 0);
        assert_eq!(built.torch.count_vertices(), 3 * 6 * 4);
        assert!(built.opaque.indices().is_some());
        assert!(built.opaque.attribute(Mesh::ATTRIBUTE_UV_0).is_some());
        assert!(built.opaque.attribute(Mesh::ATTRIBUTE_COLOR).is_some());
    }

    #[test]
    fn lighting_mode_changes_vertex_colors_without_changing_geometry() {
        let day = build_scene_meshes(PreviewLighting::Day).opaque;
        let night = build_scene_meshes(PreviewLighting::Night).opaque;
        assert_eq!(day.count_vertices(), night.count_vertices());
        assert_eq!(day.indices().unwrap().len(), night.indices().unwrap().len());
        let Some(VertexAttributeValues::Float32x4(day_colors)) =
            day.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("day mesh must contain float vertex colors");
        };
        let Some(VertexAttributeValues::Float32x4(night_colors)) =
            night.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("night mesh must contain float vertex colors");
        };
        assert_ne!(day_colors, night_colors);
    }

    #[test]
    fn atlas_has_guttered_nearest_neighbor_tiles_for_every_pack_face() {
        let pack =
            resolve_generated_pack(&generate_pack(&GenerateOptions::default()).unwrap()).unwrap();
        let image = atlas_image(&pack);
        assert_eq!(
            image.texture_descriptor.size.width,
            ATLAS_TILES * ATLAS_CELL
        );
        assert_eq!(image.texture_descriptor.size.height, ATLAS_CELL);
        assert_eq!(
            image.data.as_ref().unwrap().len(),
            (ATLAS_TILES * ATLAS_CELL * ATLAS_CELL * 4) as usize
        );
    }
}
