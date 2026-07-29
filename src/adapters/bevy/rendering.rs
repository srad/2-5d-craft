use crate::adapters::bevy::{DayCycleResource, RuntimeSet, world::WorldEntity};
use crate::domain::{
    BlockState, CHUNK_WIDTH, ChunkLayer, DEPTH_SLICES, LightCell, LightVolume, VoxelPos,
    WORLD_HEIGHT, WorldView, generated_voxel, stable_hash, world_to_chunk,
};
use avian2d::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::{Shader, ShaderRef};

const TEXTURE_SIZE: u32 = 32;
const LIGHTMAP_SIZE: u32 = 16;
const VOXEL_SHADER_HANDLE: Handle<Shader> =
    bevy::asset::uuid_handle!("a42fc7d0-7442-45b1-a797-bbefec330f96");
const ATLAS_VARIANTS: u32 = 3;
const FACE_VARIANTS: u32 = 3;
const GUTTER: u32 = 2;
const ATLAS_CELL: u32 = TEXTURE_SIZE + GUTTER * 2;
const ATLAS_TILES: u32 = BlockState::ALL.len() as u32 * ATLAS_VARIANTS * FACE_VARIANTS;

#[derive(Resource)]
pub struct RenderCatalog {
    pub opaque_material: Handle<VoxelMaterial>,
    pub cutout_material: Handle<VoxelMaterial>,
    pub emissive_material: Handle<StandardMaterial>,
    selection_mesh: Handle<Mesh>,
    selection_material: Handle<StandardMaterial>,
    pub player_cube: Handle<Mesh>,
    pub player_materials: Vec<Handle<StandardMaterial>>,
    pub player_palette: [LinearRgba; 5],
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct VoxelMaterial {
    #[texture(0)]
    #[sampler(1)]
    atlas: Handle<Image>,
    #[texture(2)]
    #[sampler(3)]
    lightmap: Handle<Image>,
    #[uniform(4)]
    haze_color: LinearRgba,
    alpha_mode: AlphaMode,
}

impl Material for VoxelMaterial {
    fn fragment_shader() -> ShaderRef {
        VOXEL_SHADER_HANDLE.clone().into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }
}

#[derive(Resource)]
pub(crate) struct VoxelLightmap {
    colors: [LinearRgba; (LIGHTMAP_SIZE * LIGHTMAP_SIZE) as usize],
    bytes: Vec<u8>,
    image: Handle<Image>,
}

impl VoxelLightmap {
    pub(crate) fn sample(&self, light: LightCell) -> LinearRgba {
        self.colors[lightmap_index(light.block, light.sky)]
    }
}

#[derive(Component)]
pub struct SelectionOutline;

pub struct RenderingPlugin;

impl Plugin for RenderingPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::load_internal_asset!(
            app,
            VOXEL_SHADER_HANDLE,
            "../../../assets/shaders/voxel_material.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins(MaterialPlugin::<VoxelMaterial>::default())
            .add_systems(Startup, build_catalog)
            .add_systems(Update, update_voxel_lightmap.in_set(RuntimeSet::Derived));
    }
}

fn build_catalog(
    mut commands: Commands,
    day: Res<DayCycleResource>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut voxel_materials: ResMut<Assets<VoxelMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let mut atlas_image = atlas_image();
    atlas_image.sampler = ImageSampler::nearest();
    let atlas = images.add(atlas_image);
    let lightmap_colors = generate_lightmap(day.daylight());
    let lightmap_bytes = encode_lightmap(&lightmap_colors);
    let lightmap_image = images.add(lightmap_image(lightmap_bytes.clone()));
    let haze_color = haze_color(day.daylight());
    let opaque_material = voxel_materials.add(VoxelMaterial {
        atlas: atlas.clone(),
        lightmap: lightmap_image.clone(),
        haze_color,
        alpha_mode: AlphaMode::Opaque,
    });
    let cutout_material = voxel_materials.add(VoxelMaterial {
        atlas: atlas.clone(),
        lightmap: lightmap_image.clone(),
        haze_color,
        alpha_mode: AlphaMode::Mask(0.38),
    });
    let emissive_material = materials.add(StandardMaterial {
        base_color_texture: Some(atlas),
        alpha_mode: AlphaMode::Mask(0.18),
        emissive: LinearRgba::rgb(4.5, 2.0, 0.35),
        unlit: true,
        cull_mode: None,
        ..default()
    });
    let player_cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let player_colors = [
        Color::srgb(0.78, 0.52, 0.32),
        Color::srgb(0.08, 0.38, 0.46),
        Color::srgb(0.12, 0.16, 0.25),
        Color::srgb(0.18, 0.09, 0.045),
        Color::srgb(0.10, 0.08, 0.06),
    ];
    let player_palette = player_colors.map(|color| color.to_linear());
    let player_materials = player_colors
        .into_iter()
        .map(|base_color| {
            materials.add(StandardMaterial {
                base_color,
                unlit: true,
                ..default()
            })
        })
        .collect();
    let selection_mesh = meshes.add(Cuboid::new(1.04, 1.04, 1.04));
    let selection_material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.92, 0.42, 0.18),
        emissive: LinearRgba::rgb(2.0, 1.4, 0.2),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        ..default()
    });

    commands.insert_resource(RenderCatalog {
        opaque_material,
        cutout_material,
        emissive_material,
        selection_mesh,
        selection_material,
        player_cube,
        player_materials,
        player_palette,
    });
    commands.insert_resource(VoxelLightmap {
        colors: lightmap_colors,
        bytes: lightmap_bytes,
        image: lightmap_image,
    });
}

pub(crate) fn update_voxel_lightmap(
    day: Res<DayCycleResource>,
    catalog: Option<Res<RenderCatalog>>,
    mut lightmap: Option<ResMut<VoxelLightmap>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<VoxelMaterial>>,
) {
    let (Some(catalog), Some(lightmap)) = (catalog, lightmap.as_mut()) else {
        return;
    };
    let colors = generate_lightmap(day.daylight());
    let bytes = encode_lightmap(&colors);
    if bytes == lightmap.bytes {
        return;
    }
    if let Some(mut image) = images.get_mut(&lightmap.image) {
        *image = lightmap_image(bytes.clone());
    }
    let haze_color = haze_color(day.daylight());
    for handle in [&catalog.opaque_material, &catalog.cutout_material] {
        if let Some(mut material) = materials.get_mut(handle) {
            material.haze_color = haze_color;
        }
    }
    lightmap.colors = colors;
    lightmap.bytes = bytes;
}

pub fn spawn_selection_outline(commands: &mut Commands, catalog: &RenderCatalog) {
    commands.spawn((
        Mesh3d(catalog.selection_mesh.clone()),
        MeshMaterial3d(catalog.selection_material.clone()),
        Transform::from_xyz(0.0, 0.0, 0.0),
        Visibility::Hidden,
        SelectionOutline,
        WorldEntity,
    ));
}

#[derive(Debug, Clone, Copy)]
enum Face {
    Front,
    Back,
    Right,
    Left,
    Top,
    Bottom,
}

impl Face {
    const ALL: [Self; 6] = [
        Self::Front,
        Self::Back,
        Self::Right,
        Self::Left,
        Self::Top,
        Self::Bottom,
    ];

    const fn normal(self) -> [f32; 3] {
        match self {
            Self::Front => [0.0, 0.0, 1.0],
            Self::Back => [0.0, 0.0, -1.0],
            Self::Right => [1.0, 0.0, 0.0],
            Self::Left => [-1.0, 0.0, 0.0],
            Self::Top => [0.0, 1.0, 0.0],
            Self::Bottom => [0.0, -1.0, 0.0],
        }
    }

    const fn neighbor(self) -> (i32, i32, i32) {
        match self {
            Self::Front => (0, 0, -1),
            Self::Back => (0, 0, 1),
            Self::Right => (1, 0, 0),
            Self::Left => (-1, 0, 0),
            Self::Top => (0, 1, 0),
            Self::Bottom => (0, -1, 0),
        }
    }

    const fn texture_class(self) -> u32 {
        match self {
            Self::Top => 1,
            Self::Right | Self::Left | Self::Bottom => 2,
            Self::Front | Self::Back => 0,
        }
    }

    const fn shade(self) -> f32 {
        match self {
            Self::Top => 1.0,
            Self::Bottom => 0.5,
            Self::Front | Self::Back => 0.8,
            Self::Right | Self::Left => 0.6,
        }
    }
}

const NIGHT_HAZE: [f32; 3] = [0.08, 0.11, 0.20];
const DAY_HAZE: [f32; 3] = [0.34, 0.46, 0.60];

pub struct ChunkMeshes {
    pub opaque: Mesh,
    pub cutout: Mesh,
    pub emissive: Mesh,
}

pub fn build_chunk_meshes(
    view: &WorldView<'_>,
    global_chunk_x: i64,
    seed: u64,
    light: &LightVolume,
) -> ChunkMeshes {
    let mut opaque = VoxelMeshBuilder::default();
    let mut cutout = VoxelMeshBuilder::default();
    let mut emissive = VoxelMeshBuilder::default();
    let voxel_at = |global_x: i64, y: i32, depth: i32| -> Option<BlockState> {
        if !(0..WORLD_HEIGHT).contains(&y) || !(0..i32::from(DEPTH_SLICES)).contains(&depth) {
            return None;
        }
        if depth == 0 && view.contains_chunk(ChunkLayer::foreground(world_to_chunk(global_x))) {
            view.block(VoxelPos::foreground(global_x, y))
        } else {
            generated_voxel(seed, global_x, y, depth as u8)
        }
    };

    for local_x in 0..CHUNK_WIDTH {
        let global_x = global_chunk_x * i64::from(CHUNK_WIDTH) + i64::from(local_x);
        for y in 0..WORLD_HEIGHT {
            for depth in 0..DEPTH_SLICES {
                let Some(state) = voxel_at(global_x, y, i32::from(depth)) else {
                    continue;
                };
                let variant = (stable_hash(
                    seed,
                    global_x,
                    y,
                    u64::from(state.id().value()) + u64::from(depth) * 97,
                ) % 3) as u32;
                if state == BlockState::TORCH {
                    if depth == 0 {
                        emissive.push_torch(local_x as f32, y as f32, variant);
                    }
                    continue;
                }
                let builder = if state == BlockState::LEAVES {
                    &mut cutout
                } else {
                    &mut opaque
                };
                for face in Face::ALL {
                    let (dx, dy, dd) = face.neighbor();
                    if voxel_at(global_x + i64::from(dx), y + dy, i32::from(depth) + dd)
                        .is_some_and(|neighbor| neighbor != BlockState::TORCH)
                    {
                        continue;
                    }
                    let sampled = face_light(light, global_x, y, depth, face, &voxel_at);
                    let color = [
                        f32::from(sampled.sky) / 15.0,
                        f32::from(sampled.block) / 15.0,
                        face.shade(),
                        f32::from(depth) / f32::from(DEPTH_SLICES - 1),
                    ];
                    builder.push_voxel_face(
                        local_x as f32,
                        y as f32,
                        depth,
                        state,
                        variant,
                        face,
                        color,
                    );
                }
            }
        }
    }

    ChunkMeshes {
        opaque: opaque.finish(),
        cutout: cutout.finish(),
        emissive: emissive.finish(),
    }
}

fn face_light(
    light: &LightVolume,
    global_x: i64,
    y: i32,
    depth: u8,
    face: Face,
    voxel_at: &impl Fn(i64, i32, i32) -> Option<BlockState>,
) -> LightCell {
    let (dx, dy, dd) = face.neighbor();
    let neighbor_depth = i32::from(depth) + dd;
    if !(0..i32::from(DEPTH_SLICES)).contains(&neighbor_depth) {
        return boundary_face_light(light, global_x, y, depth, voxel_at);
    }
    if y + dy >= WORLD_HEIGHT {
        return LightCell { sky: 15, block: 0 };
    }
    light.get(global_x + i64::from(dx), y + dy, neighbor_depth as u8)
}

fn boundary_face_light(
    light: &LightVolume,
    global_x: i64,
    y: i32,
    depth: u8,
    voxel_at: &impl Fn(i64, i32, i32) -> Option<BlockState>,
) -> LightCell {
    let mut visible = LightCell::default();
    for (dx, dy, dd) in [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ] {
        let x = global_x + i64::from(dx);
        let y = y + dy;
        let depth = i32::from(depth) + dd;
        if !(0..WORLD_HEIGHT).contains(&y)
            || !(0..i32::from(DEPTH_SLICES)).contains(&depth)
            || voxel_at(x, y, depth).is_some_and(|state| state.def().light_opacity >= 15)
        {
            continue;
        }
        let neighbor = light.get(x, y, depth as u8);
        visible.sky = visible.sky.max(neighbor.sky);
        visible.block = visible.block.max(neighbor.block);
    }
    visible
}

pub fn build_chunk_collider(view: &WorldView<'_>, chunk_x: i64) -> Option<Collider> {
    let start_x = chunk_x * i64::from(CHUNK_WIDTH);
    let mut shapes = Vec::new();
    for y in 0..WORLD_HEIGHT {
        let mut run_start = None;
        for local_x in 0..=CHUNK_WIDTH {
            let solid = local_x < CHUNK_WIDTH
                && view
                    .block(VoxelPos::foreground(start_x + i64::from(local_x), y))
                    .is_some_and(|state| state.def().solid);
            match (run_start, solid) {
                (None, true) => run_start = Some(local_x),
                (Some(start), false) => {
                    let length = (local_x - start) as f32;
                    shapes.push((
                        Vec2::new(start as f32 + length * 0.5, y as f32 + 0.5),
                        0.0,
                        Collider::rectangle(length, 1.0),
                    ));
                    run_start = None;
                }
                _ => {}
            }
        }
    }
    (!shapes.is_empty()).then(|| Collider::compound(shapes))
}

#[derive(Default)]
struct VoxelMeshBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl VoxelMeshBuilder {
    #[allow(clippy::too_many_arguments)]
    fn push_voxel_face(
        &mut self,
        x: f32,
        y: f32,
        depth: u8,
        state: BlockState,
        variant: u32,
        face: Face,
        color: [f32; 4],
    ) {
        let front = -f32::from(depth) + 0.5;
        let back = front - 1.0;
        let corners = match face {
            Face::Front => [
                [x, y, front],
                [x + 1.0, y, front],
                [x + 1.0, y + 1.0, front],
                [x, y + 1.0, front],
            ],
            Face::Back => [
                [x + 1.0, y, back],
                [x, y, back],
                [x, y + 1.0, back],
                [x + 1.0, y + 1.0, back],
            ],
            Face::Right => [
                [x + 1.0, y, front],
                [x + 1.0, y, back],
                [x + 1.0, y + 1.0, back],
                [x + 1.0, y + 1.0, front],
            ],
            Face::Left => [
                [x, y, back],
                [x, y, front],
                [x, y + 1.0, front],
                [x, y + 1.0, back],
            ],
            Face::Top => [
                [x, y + 1.0, front],
                [x + 1.0, y + 1.0, front],
                [x + 1.0, y + 1.0, back],
                [x, y + 1.0, back],
            ],
            Face::Bottom => [
                [x, y, back],
                [x + 1.0, y, back],
                [x + 1.0, y, front],
                [x, y, front],
            ],
        };
        let tile = ((u32::from(state.id().value()) - 1) * FACE_VARIANTS + face.texture_class())
            * ATLAS_VARIANTS
            + variant;
        self.push_quad(corners, face.normal(), tile_uvs(tile), color);
    }

    fn push_torch(&mut self, x: f32, y: f32, variant: u32) {
        let tile = ((u32::from(BlockState::TORCH.id().value()) - 1) * FACE_VARIANTS)
            * ATLAS_VARIANTS
            + variant;
        let uvs = tile_uvs(tile);
        let color = [1.0; 4];
        self.push_quad(
            [
                [x + 0.18, y + 0.02, 0.06],
                [x + 0.82, y + 0.02, 0.06],
                [x + 0.82, y + 0.98, 0.06],
                [x + 0.18, y + 0.98, 0.06],
            ],
            [0.0, 0.0, 1.0],
            uvs,
            color,
        );
        self.push_quad(
            [
                [x + 0.5, y + 0.02, -0.26],
                [x + 0.5, y + 0.02, 0.38],
                [x + 0.5, y + 0.98, 0.38],
                [x + 0.5, y + 0.98, -0.26],
            ],
            [1.0, 0.0, 0.0],
            uvs,
            color,
        );
    }

    fn push_quad(
        &mut self,
        corners: [[f32; 3]; 4],
        normal: [f32; 3],
        uvs: [[f32; 2]; 4],
        color: [f32; 4],
    ) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&corners);
        self.normals.extend_from_slice(&[normal; 4]);
        self.uvs.extend_from_slice(&uvs);
        self.colors.extend_from_slice(&[color; 4]);
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

fn tile_uvs(tile: u32) -> [[f32; 2]; 4] {
    let atlas_width = (ATLAS_TILES * ATLAS_CELL) as f32;
    let start = tile * ATLAS_CELL + GUTTER;
    let u0 = start as f32 / atlas_width;
    let u1 = (start + TEXTURE_SIZE) as f32 / atlas_width;
    let v0 = GUTTER as f32 / ATLAS_CELL as f32;
    let v1 = (GUTTER + TEXTURE_SIZE) as f32 / ATLAS_CELL as f32;
    [[u0, v1], [u1, v1], [u1, v0], [u0, v0]]
}

fn lightmap_index(block: u8, sky: u8) -> usize {
    usize::from(sky.min(15)) * LIGHTMAP_SIZE as usize + usize::from(block.min(15))
}

fn classic_brightness(level: f32) -> f32 {
    let normalized = (level / 15.0).clamp(0.0, 1.0);
    0.05 + 0.95 * normalized / (4.0 - 3.0 * normalized)
}

fn generate_lightmap(daylight: f32) -> [LinearRgba; (LIGHTMAP_SIZE * LIGHTMAP_SIZE) as usize] {
    let daylight = daylight.clamp(0.0, 1.0);
    let global_sky_level = 4.0 + daylight * 11.0;
    let sky_tint = [
        mix(0.46, 1.0, daylight),
        mix(0.56, 0.98, daylight),
        mix(0.84, 0.92, daylight),
    ];
    let block_tint = [1.0, 0.58, 0.22];
    std::array::from_fn(|index| {
        let block = (index % LIGHTMAP_SIZE as usize) as u8;
        let sky = (index / LIGHTMAP_SIZE as usize) as u8;
        let effective_sky = (f32::from(sky) - (15.0 - global_sky_level)).max(0.0);
        let sky_brightness = classic_brightness(effective_sky);
        let block_brightness = if block == 0 {
            0.0
        } else {
            classic_brightness(f32::from(block))
        };
        LinearRgba::rgb(
            (sky_brightness * sky_tint[0]).max(block_brightness * block_tint[0]),
            (sky_brightness * sky_tint[1]).max(block_brightness * block_tint[1]),
            (sky_brightness * sky_tint[2]).max(block_brightness * block_tint[2]),
        )
    })
}

fn encode_lightmap(colors: &[LinearRgba; (LIGHTMAP_SIZE * LIGHTMAP_SIZE) as usize]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(colors.len() * 4);
    for color in colors {
        bytes.extend_from_slice(&[
            linear_channel_byte(color.red),
            linear_channel_byte(color.green),
            linear_channel_byte(color.blue),
            255,
        ]);
    }
    bytes
}

fn linear_channel_byte(channel: f32) -> u8 {
    (channel.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn lightmap_image(bytes: Vec<u8>) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: LIGHTMAP_SIZE,
            height: LIGHTMAP_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        bytes,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
}

fn haze_color(daylight: f32) -> LinearRgba {
    LinearRgba::rgb(
        mix(NIGHT_HAZE[0], DAY_HAZE[0], daylight),
        mix(NIGHT_HAZE[1], DAY_HAZE[1], daylight),
        mix(NIGHT_HAZE[2], DAY_HAZE[2], daylight),
    )
}

fn mix(start: f32, end: f32, amount: f32) -> f32 {
    start + (end - start) * amount.clamp(0.0, 1.0)
}

fn atlas_image() -> Image {
    let width = ATLAS_TILES * ATLAS_CELL;
    let mut pixels = vec![0; (width * ATLAS_CELL * 4) as usize];
    for (state_index, state) in BlockState::ALL.into_iter().enumerate() {
        for face in 0..FACE_VARIANTS {
            for variant in 0..ATLAS_VARIANTS {
                let tile = (state_index as u32 * FACE_VARIANTS + face) * ATLAS_VARIANTS + variant;
                let source = if state == BlockState::TORCH {
                    torch_pixels()
                } else {
                    block_face_pixels(state, variant as u8, face)
                };
                for cell_y in 0..ATLAS_CELL {
                    let source_y = cell_y.saturating_sub(GUTTER).min(TEXTURE_SIZE - 1);
                    for cell_x in 0..ATLAS_CELL {
                        let source_x = cell_x.saturating_sub(GUTTER).min(TEXTURE_SIZE - 1);
                        let source_index = ((source_y * TEXTURE_SIZE + source_x) * 4) as usize;
                        let target_x = tile * ATLAS_CELL + cell_x;
                        let target_index = ((cell_y * width + target_x) * 4) as usize;
                        pixels[target_index..target_index + 4]
                            .copy_from_slice(&source[source_index..source_index + 4]);
                    }
                }
            }
        }
    }
    Image::new(
        Extent3d {
            width,
            height: ATLAS_CELL,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
}

fn palette(state: BlockState) -> ([u8; 3], [u8; 3]) {
    match state {
        BlockState::GRASS => ([86, 142, 48], [45, 92, 32]),
        BlockState::DIRT => ([128, 83, 48], [82, 48, 29]),
        BlockState::STONE => ([110, 114, 119], [66, 70, 76]),
        BlockState::COAL_ORE => ([93, 96, 101], [22, 24, 29]),
        BlockState::IRON_ORE => ([112, 104, 94], [197, 124, 78]),
        BlockState::WOOD => ([137, 88, 40], [72, 43, 22]),
        BlockState::LEAVES => ([54, 132, 52], [25, 76, 35]),
        BlockState::BEDROCK => ([55, 58, 64], [20, 22, 27]),
        BlockState::TORCH => ([224, 126, 28], [255, 221, 91]),
        _ => unreachable!("all valid block states have a palette"),
    }
}

#[cfg(test)]
pub fn block_pixels(state: BlockState, variant: u8) -> Vec<u8> {
    block_face_pixels(state, variant, 0)
}

fn block_face_pixels(state: BlockState, variant: u8, face: u32) -> Vec<u8> {
    let (base, accent) = palette(state);
    let mut pixels = Vec::with_capacity((TEXTURE_SIZE * TEXTURE_SIZE * 4) as usize);
    for y in 0..TEXTURE_SIZE {
        for x in 0..TEXTURE_SIZE {
            let hash = stable_hash(
                u64::from(variant) + u64::from(face) * 131,
                i64::from(x),
                y as i32,
                u64::from(state.id().value()) + 700,
            );
            let edge = x == 0 || y == 0 || x == TEXTURE_SIZE - 1 || y == TEXTURE_SIZE - 1;
            let grass_blade = state == BlockState::GRASS
                && ((face == 1 && hash % 7 < 3) || (face != 1 && y < 6 && hash % 5 < 3));
            let wood_grain = state == BlockState::WOOD
                && if face == 1 {
                    ((x as i32 - 16).pow(2) + (y as i32 - 16).pow(2)) % 17 < 4
                } else {
                    (x + u32::from(variant) * 3) % 9 < 2
                };
            let use_accent = edge || hash.is_multiple_of(13) || grass_blade || wood_grain;
            let mut color = if use_accent { accent } else { base };
            let variation = (hash % 17) as i16 - 8;
            for channel in &mut color {
                *channel = (i16::from(*channel) + variation).clamp(0, 255) as u8;
            }
            let alpha = if state == BlockState::LEAVES && hash.is_multiple_of(19) {
                0
            } else {
                255
            };
            pixels.extend_from_slice(&[color[0], color[1], color[2], alpha]);
        }
    }
    pixels
}

pub fn torch_pixels() -> Vec<u8> {
    let mut pixels = vec![0; (TEXTURE_SIZE * TEXTURE_SIZE * 4) as usize];
    for y in 8..31 {
        for x in 14..18 {
            set_pixel(&mut pixels, x, y, [120, 69, 28, 255]);
        }
    }
    for y in 1..12 {
        let half_width = 2 + (11 - y) / 4;
        for x in 16 - half_width..=16 + half_width {
            let color = if y < 5 {
                [255, 235, 122, 255]
            } else if x % 2 == 0 {
                [255, 170, 42, 255]
            } else {
                [238, 82, 24, 255]
            };
            set_pixel(&mut pixels, x, y, color);
        }
    }
    pixels
}

fn set_pixel(pixels: &mut [u8], x: u32, y: u32, color: [u8; 4]) {
    let index = ((y * TEXTURE_SIZE + x) * 4) as usize;
    pixels[index..index + 4].copy_from_slice(&color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        BlockChunk, BlockGrid, BlockPrecondition, BlockWrite, MutationPriority, MutationProposal,
        WorldMutator,
    };

    fn empty_grid() -> BlockGrid {
        let mut grid = BlockGrid::new(WORLD_HEIGHT);
        WorldMutator::new(&mut grid).integrate_chunk(
            BlockChunk::from_dense(0, vec![0; (CHUNK_WIDTH * WORLD_HEIGHT) as usize]).unwrap(),
        );
        grid
    }

    #[test]
    fn generated_texture_tiles_are_deterministic_and_detailed() {
        assert_eq!(block_pixels(BlockState::DIRT, 1).len(), 32 * 32 * 4);
        assert_eq!(
            block_pixels(BlockState::DIRT, 1),
            block_pixels(BlockState::DIRT, 1)
        );
        assert_ne!(
            block_pixels(BlockState::DIRT, 1),
            block_pixels(BlockState::DIRT, 2)
        );
        let alphas: Vec<_> = torch_pixels().iter().skip(3).step_by(4).copied().collect();
        assert!(alphas.contains(&0));
        assert!(alphas.contains(&255));
    }

    #[test]
    fn a_single_voxel_emits_six_faces() {
        let mut builder = VoxelMeshBuilder::default();
        for face in Face::ALL {
            builder.push_voxel_face(0.0, 0.0, 0, BlockState::STONE, 0, face, [1.0; 4]);
        }
        let mesh = builder.finish();
        assert_eq!(mesh.count_vertices(), 24);
        assert_eq!(mesh.indices().unwrap().len(), 36);
    }

    #[test]
    fn face_shades_follow_minecraft_directional_values() {
        assert_eq!(Face::Top.shade(), 1.0);
        assert_eq!(Face::Bottom.shade(), 0.5);
        assert_eq!(Face::Front.shade(), 0.8);
        assert_eq!(Face::Back.shade(), 0.8);
        assert_eq!(Face::Right.shade(), 0.6);
        assert_eq!(Face::Left.shade(), 0.6);
    }

    #[test]
    fn lightmap_keeps_sky_and_block_channels_distinct() {
        let noon = generate_lightmap(1.0);
        let midnight = generate_lightmap(0.0);
        let full_sky = noon[lightmap_index(0, 15)];
        let night_sky = midnight[lightmap_index(0, 15)];
        let torch = midnight[lightmap_index(12, 0)];
        assert!(full_sky.red > night_sky.red);
        assert!(torch.red > torch.green);
        assert!(torch.green > torch.blue);
        assert_eq!(encode_lightmap(&noon).len(), 16 * 16 * 4);
    }

    #[test]
    fn chunk_mesh_contains_real_depth_and_keeps_collider_two_dimensional() {
        let mut grid = empty_grid();
        let writes = [
            (VoxelPos::foreground(0, 0), BlockState::BEDROCK),
            (VoxelPos::foreground(1, 1), BlockState::LEAVES),
            (VoxelPos::foreground(2, 1), BlockState::TORCH),
        ];
        let proposals = writes
            .into_iter()
            .enumerate()
            .map(|(sequence, (position, state))| MutationProposal {
                preconditions: vec![BlockPrecondition {
                    position,
                    expected: None,
                }],
                writes: vec![BlockWrite {
                    position,
                    state: Some(state),
                }],
                priority: MutationPriority::PLAYER,
                source: position,
                sequence: sequence as u64,
            })
            .collect();
        WorldMutator::new(&mut grid).commit_batch(proposals);
        let view = grid.view();
        let light = LightVolume::calculate(-15, 47, WORLD_HEIGHT, DEPTH_SLICES, |x, y, depth| {
            if depth == 0 && view.contains_chunk(ChunkLayer::foreground(world_to_chunk(x))) {
                view.block(VoxelPos::foreground(x, y))
            } else {
                generated_voxel(7, x, y, depth)
            }
        });
        let built = build_chunk_meshes(&view, 0, 7, &light);
        let positions = built
            .opaque
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        assert!(positions.iter().any(|position| position[2] <= -2.5));
        assert!(built.cutout.count_vertices() > 0);
        assert!(built.emissive.count_vertices() > 0);
        assert!(build_chunk_collider(&grid.view(), 0).is_some());
    }
}
