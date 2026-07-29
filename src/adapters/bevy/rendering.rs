use crate::{
    AppState,
    adapters::bevy::{
        DayCycleResource, RuntimeSet, configured_autostart_u64,
        lighting::LightingPalette,
        textures::{TexturePackCatalog, TexturePackChanged},
        world::WorldEntity,
    },
    domain::{
        BlockId, BlockState, CHUNK_WIDTH, ChunkLayer, DEPTH_SLICES, LightCell, LightVolume,
        MAX_LIGHT_LEVEL, TorchMount, VoxelPos, WORLD_HEIGHT, WorldView, generated_voxel,
        stable_hash, world_to_chunk,
    },
};
use avian2d::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::mesh::{Indices, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    TextureDimension, TextureFormat, VertexFormat,
};
use bevy::shader::{Shader, ShaderRef};
use sidecraft_textures::{
    BlockKind as PackBlock, Face as PackFace, PackImage, PlayerPalette, ResolvedPack,
};

const TEXTURE_SIZE: u32 = 16;
const LIGHTMAP_SIZE: u32 = 16;
const VOXEL_SHADER_HANDLE: Handle<Shader> =
    bevy::asset::uuid_handle!("a42fc7d0-7442-45b1-a797-bbefec330f96");
const TORCH_SHADER_HANDLE: Handle<Shader> =
    bevy::asset::uuid_handle!("dedfb6b1-5089-40e9-a4aa-f37684b33ad3");
const ATTRIBUTE_TORCH_EFFECT: MeshVertexAttribute =
    MeshVertexAttribute::new("TorchEffect", 2_134_867_501, VertexFormat::Float32x4);
const ATLAS_VARIANTS: u32 = 4;
const FACE_VARIANTS: u32 = 3;
const GUTTER: u32 = 2;
const ATLAS_CELL: u32 = TEXTURE_SIZE + GUTTER * 2;
const ATLAS_TILES: u32 = BlockId::ALL.len() as u32 * ATLAS_VARIANTS * FACE_VARIANTS;
const TORCH_LIGHT_TINT: [f32; 3] = [1.0, 0.42, 0.08];

#[derive(Resource)]
pub struct RenderCatalog {
    atlas: Handle<Image>,
    pub opaque_material: Handle<VoxelMaterial>,
    pub cutout_material: Handle<VoxelMaterial>,
    pub emissive_material: Handle<TorchMaterial>,
    selection_mesh: Handle<Mesh>,
    selection_material: Handle<StandardMaterial>,
    pub player_cube: Handle<Mesh>,
    pub player_materials: Vec<Handle<StandardMaterial>>,
    pub player_palette: [LinearRgba; 5],
    pub(crate) hotbar_icons: Vec<Handle<Image>>,
    pub(crate) pack_preview: Handle<Image>,
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

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct TorchMaterial {
    #[texture(0)]
    #[sampler(1)]
    atlas: Handle<Image>,
    #[uniform(2)]
    effects: Vec4,
}

impl Material for TorchMaterial {
    fn vertex_shader() -> ShaderRef {
        TORCH_SHADER_HANDLE.clone().into()
    }

    fn fragment_shader() -> ShaderRef {
        TORCH_SHADER_HANDLE.clone().into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Mask(0.18)
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            ATTRIBUTE_TORCH_EFFECT.at_shader_location(2),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[derive(Resource)]
pub(crate) struct VoxelLightmap {
    colors: [LinearRgba; (LIGHTMAP_SIZE * LIGHTMAP_SIZE) as usize],
    bytes: Vec<u8>,
    image: Handle<Image>,
}

#[derive(Resource)]
struct TorchFlicker {
    elapsed: f32,
    intensity: f32,
}

impl Default for TorchFlicker {
    fn default() -> Self {
        Self {
            elapsed: 0.0,
            intensity: 1.0,
        }
    }
}

#[derive(Resource)]
struct FixedTorchIntensity(f32);

impl VoxelLightmap {
    pub(crate) fn sample_levels(&self, block: f32, sky: f32) -> LinearRgba {
        sample_lightmap(&self.colors, block, sky)
    }
}

#[derive(Component)]
pub struct SelectionOutline;

pub struct RenderingPlugin;

impl Plugin for RenderingPlugin {
    fn build(&self, app: &mut App) {
        if let Some(percent) = configured_autostart_u64("SIDECRAFT_TEST_TORCH_INTENSITY_PERCENT")
            && (92..=108).contains(&percent)
        {
            app.insert_resource(FixedTorchIntensity(percent as f32 / 100.0));
        }
        bevy::asset::load_internal_asset!(
            app,
            VOXEL_SHADER_HANDLE,
            "../../../assets/shaders/voxel_material.wgsl",
            Shader::from_wgsl
        );
        bevy::asset::load_internal_asset!(
            app,
            TORCH_SHADER_HANDLE,
            "../../../assets/shaders/torch_material.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins((
            MaterialPlugin::<VoxelMaterial>::default(),
            MaterialPlugin::<TorchMaterial>::default(),
        ))
        .init_resource::<TorchFlicker>()
        .add_systems(Startup, build_catalog)
        .add_systems(Update, apply_texture_pack)
        .add_systems(
            Update,
            advance_torch_flicker
                .in_set(RuntimeSet::Clock)
                .run_if(in_state(AppState::Playing)),
        )
        .add_systems(
            Update,
            (update_voxel_lightmap, update_torch_material)
                .in_set(RuntimeSet::Derived)
                .run_if(in_state(AppState::Playing)),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn build_catalog(
    mut commands: Commands,
    day: Res<DayCycleResource>,
    texture_packs: Res<TexturePackCatalog>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut voxel_materials: ResMut<Assets<VoxelMaterial>>,
    mut torch_materials: ResMut<Assets<TorchMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let mut atlas_image = atlas_image(&texture_packs.active);
    atlas_image.sampler = ImageSampler::nearest();
    let atlas = images.add(atlas_image);
    let palette = LightingPalette::from_day(&day);
    let lightmap_colors = generate_lightmap(day.daylight(), palette, 1.0);
    let lightmap_bytes = encode_lightmap(&lightmap_colors);
    let lightmap_image = images.add(lightmap_image(lightmap_bytes.clone()));
    let haze_color = LinearRgba::rgb(palette.haze[0], palette.haze[1], palette.haze[2]);
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
    let emissive_material = torch_materials.add(TorchMaterial {
        atlas: atlas.clone(),
        effects: Vec4::new(1.0, 0.0, 0.0, 0.0),
    });
    let player_cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let player_colors = player_colors(texture_packs.active.player);
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
        base_color: Color::srgba(1.0, 0.92, 0.42, 0.08),
        emissive: LinearRgba::rgb(0.08, 0.05, 0.01),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        ..default()
    });
    let hotbar_icons = PackBlock::HOTBAR
        .into_iter()
        .map(|block| images.add(pack_image(&texture_packs.active.icons[&block])))
        .collect();
    let pack_preview = images.add(pack_image(&texture_packs.active.preview));

    commands.insert_resource(RenderCatalog {
        atlas,
        opaque_material,
        cutout_material,
        emissive_material,
        selection_mesh,
        selection_material,
        player_cube,
        player_materials,
        player_palette,
        hotbar_icons,
        pack_preview,
    });
    commands.insert_resource(VoxelLightmap {
        colors: lightmap_colors,
        bytes: lightmap_bytes,
        image: lightmap_image,
    });
}

fn update_voxel_lightmap(
    day: Res<DayCycleResource>,
    catalog: Option<Res<RenderCatalog>>,
    mut lightmap: Option<ResMut<VoxelLightmap>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<VoxelMaterial>>,
    flicker: Res<TorchFlicker>,
) {
    let (Some(catalog), Some(lightmap)) = (catalog, lightmap.as_mut()) else {
        return;
    };
    let palette = LightingPalette::from_day(&day);
    let colors = generate_lightmap(day.daylight(), palette, flicker.intensity);
    let bytes = encode_lightmap(&colors);
    if bytes != lightmap.bytes {
        if let Some(mut image) = images.get_mut(&lightmap.image) {
            *image = lightmap_image(bytes.clone());
        }
        lightmap.bytes = bytes;
    }
    let haze_color = LinearRgba::rgb(palette.haze[0], palette.haze[1], palette.haze[2]);
    for handle in [&catalog.opaque_material, &catalog.cutout_material] {
        if let Some(mut material) = materials.get_mut(handle) {
            material.haze_color = haze_color;
        }
    }
    lightmap.colors = colors;
}

fn advance_torch_flicker(
    time: Res<Time>,
    fixed: Option<Res<FixedTorchIntensity>>,
    mut flicker: ResMut<TorchFlicker>,
) {
    if let Some(fixed) = fixed {
        flicker.intensity = fixed.0;
        return;
    }
    flicker.elapsed = (flicker.elapsed + time.delta_secs()) % 1_024.0;
    flicker.intensity = torch_flicker(flicker.elapsed);
}

fn update_torch_material(
    flicker: Res<TorchFlicker>,
    catalog: Option<Res<RenderCatalog>>,
    mut materials: ResMut<Assets<TorchMaterial>>,
) {
    let Some(catalog) = catalog else {
        return;
    };
    if let Some(mut material) = materials.get_mut(&catalog.emissive_material) {
        material.effects.x = flicker.intensity;
    }
}

fn torch_flicker(elapsed: f32) -> f32 {
    let primary = (std::f32::consts::TAU * elapsed / 2.8).sin() * 0.055;
    let secondary = (std::f32::consts::TAU * elapsed / 4.6 + 1.3).sin() * 0.025;
    (1.0 + primary + secondary).clamp(0.92, 1.08)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
enum TextureFace {
    Side,
    Top,
    Bottom,
}

impl TextureFace {
    const ALL: [Self; FACE_VARIANTS as usize] = [Self::Side, Self::Top, Self::Bottom];

    const fn index(self) -> u32 {
        self as u32
    }
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

    const fn texture_face(self) -> TextureFace {
        match self {
            Self::Top => TextureFace::Top,
            Self::Bottom => TextureFace::Bottom,
            Self::Front | Self::Back | Self::Right | Self::Left => TextureFace::Side,
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

    const fn corner_tangents(self) -> [(VoxelOffset, VoxelOffset); 4] {
        const LEFT: VoxelOffset = VoxelOffset::new(-1, 0, 0);
        const RIGHT: VoxelOffset = VoxelOffset::new(1, 0, 0);
        const DOWN: VoxelOffset = VoxelOffset::new(0, -1, 0);
        const UP: VoxelOffset = VoxelOffset::new(0, 1, 0);
        const FRONT: VoxelOffset = VoxelOffset::new(0, 0, -1);
        const BACK: VoxelOffset = VoxelOffset::new(0, 0, 1);
        match self {
            Self::Front => [(LEFT, DOWN), (RIGHT, DOWN), (RIGHT, UP), (LEFT, UP)],
            Self::Back => [(RIGHT, DOWN), (LEFT, DOWN), (LEFT, UP), (RIGHT, UP)],
            Self::Right => [(FRONT, DOWN), (BACK, DOWN), (BACK, UP), (FRONT, UP)],
            Self::Left => [(BACK, DOWN), (FRONT, DOWN), (FRONT, UP), (BACK, UP)],
            Self::Top => [(LEFT, FRONT), (RIGHT, FRONT), (RIGHT, BACK), (LEFT, BACK)],
            Self::Bottom => [(LEFT, BACK), (RIGHT, BACK), (RIGHT, FRONT), (LEFT, FRONT)],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VoxelOffset {
    x: i32,
    y: i32,
    depth: i32,
}

impl VoxelOffset {
    const fn new(x: i32, y: i32, depth: i32) -> Self {
        Self { x, y, depth }
    }

    const fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.depth + other.depth)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FaceVertexLight {
    sky: f32,
    block: f32,
    shade: f32,
}

impl FaceVertexLight {
    fn encoded(self, depth: u8) -> [f32; 4] {
        [
            self.sky / f32::from(MAX_LIGHT_LEVEL),
            self.block / f32::from(MAX_LIGHT_LEVEL),
            self.shade,
            f32::from(depth),
        ]
    }
}

const AO_SHADE: [f32; 4] = [0.82, 0.88, 0.94, 1.0];

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
    let mut emissive = TorchMeshBuilder::default();
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
                ) % u64::from(ATLAS_VARIANTS)) as u32;
                if let Some(mount) = state.torch_mount() {
                    if depth == 0 {
                        emissive.push_torch(local_x as f32, y as f32, variant, mount);
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
                        .is_some_and(|neighbor| neighbor.torch_mount().is_none())
                    {
                        continue;
                    }
                    let lighting = face_vertex_lighting(light, global_x, y, depth, face, &voxel_at);
                    builder.push_voxel_face(
                        local_x as f32,
                        y as f32,
                        depth,
                        state,
                        variant,
                        face,
                        lighting,
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

fn face_vertex_lighting(
    light: &LightVolume,
    global_x: i64,
    y: i32,
    depth: u8,
    face: Face,
    voxel_at: &impl Fn(i64, i32, i32) -> Option<BlockState>,
) -> [FaceVertexLight; 4] {
    let (normal_x, normal_y, normal_depth) = face.neighbor();
    let face_offset = VoxelOffset::new(normal_x, normal_y, normal_depth);
    let face_position = offset_position(global_x, y, i32::from(depth), face_offset);
    let base_light = sample_face_light(light, face_position, depth, voxel_at);
    let mut sky_total = 0.0;
    let mut block_total = 0.0;
    let mut ao_total = 0;
    for (side_a, side_b) in face.corner_tangents() {
        let side_a_position = offset_position_tuple(face_position, side_a);
        let side_b_position = offset_position_tuple(face_position, side_b);
        let corner_position = offset_position_tuple(face_position, side_a.add(side_b));
        let side_a_opaque = fully_opaque(side_a_position, voxel_at);
        let side_b_opaque = fully_opaque(side_b_position, voxel_at);
        let corner_opaque = fully_opaque(corner_position, voxel_at);
        let samples = [
            base_light,
            light_or_base(
                light,
                side_a_position,
                depth,
                side_a_opaque,
                base_light,
                voxel_at,
            ),
            light_or_base(
                light,
                side_b_position,
                depth,
                side_b_opaque,
                base_light,
                voxel_at,
            ),
            light_or_base(
                light,
                corner_position,
                depth,
                corner_opaque,
                base_light,
                voxel_at,
            ),
        ];
        sky_total += samples
            .iter()
            .map(|sample| f32::from(sample.sky))
            .sum::<f32>()
            / 4.0;
        block_total += samples
            .iter()
            .map(|sample| f32::from(sample.block))
            .sum::<f32>()
            / 4.0;
        ao_total += vertex_ao(side_a_opaque, side_b_opaque, corner_opaque);
    }
    let face_ao = (ao_total + 2) / 4;
    [FaceVertexLight {
        sky: sky_total / 4.0,
        block: block_total / 4.0,
        shade: face.shade() * AO_SHADE[face_ao],
    }; 4]
}

fn offset_position(x: i64, y: i32, depth: i32, offset: VoxelOffset) -> (i64, i32, i32) {
    (
        x.saturating_add(i64::from(offset.x)),
        y.saturating_add(offset.y),
        depth.saturating_add(offset.depth),
    )
}

fn offset_position_tuple(position: (i64, i32, i32), offset: VoxelOffset) -> (i64, i32, i32) {
    offset_position(position.0, position.1, position.2, offset)
}

fn fully_opaque(
    position: (i64, i32, i32),
    voxel_at: &impl Fn(i64, i32, i32) -> Option<BlockState>,
) -> bool {
    voxel_at(position.0, position.1, position.2)
        .is_some_and(|state| state.def().light_opacity >= MAX_LIGHT_LEVEL)
}

fn light_or_base(
    light: &LightVolume,
    position: (i64, i32, i32),
    surface_depth: u8,
    opaque: bool,
    base: LightCell,
    voxel_at: &impl Fn(i64, i32, i32) -> Option<BlockState>,
) -> LightCell {
    if opaque {
        base
    } else {
        sample_face_light(light, position, surface_depth, voxel_at)
    }
}

fn sample_face_light(
    light: &LightVolume,
    position: (i64, i32, i32),
    surface_depth: u8,
    voxel_at: &impl Fn(i64, i32, i32) -> Option<BlockState>,
) -> LightCell {
    if position.1 >= WORLD_HEIGHT {
        return LightCell {
            sky: MAX_LIGHT_LEVEL,
            block: 0,
        };
    }
    if position.1 < 0 {
        return LightCell::default();
    }
    if !(0..i32::from(DEPTH_SLICES)).contains(&position.2) {
        return boundary_face_light(light, position.0, position.1, surface_depth, voxel_at);
    }
    light.get(position.0, position.1, position.2 as u8)
}

fn vertex_ao(side_a: bool, side_b: bool, corner: bool) -> usize {
    if side_a && side_b {
        0
    } else {
        3 - usize::from(side_a) - usize::from(side_b) - usize::from(corner)
    }
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
        lighting: [FaceVertexLight; 4],
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
        let tile = ((u32::from(state.id().value()) - 1) * FACE_VARIANTS
            + face.texture_face().index())
            * ATLAS_VARIANTS
            + variant;
        let colors = lighting.map(|sample| sample.encoded(depth));
        self.push_quad(corners, face.normal(), tile_uvs(tile), colors);
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

#[derive(Default)]
struct TorchMeshBuilder {
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    effects: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl TorchMeshBuilder {
    fn push_torch(&mut self, x: f32, y: f32, variant: u32, mount: TorchMount) {
        let tile = ((u32::from(BlockState::TORCH.id().value()) - 1) * FACE_VARIANTS)
            * ATLAS_VARIANTS
            + variant;
        let (base, direction) = match mount {
            TorchMount::Floor => (Vec2::new(x + 0.5, y + 0.05), Vec2::new(0.0, 0.625)),
            TorchMount::WallLeft => {
                let angle = std::f32::consts::FRAC_PI_8;
                (
                    Vec2::new(x + 0.94, y + 0.25),
                    Vec2::new(-angle.sin(), angle.cos()) * 0.625,
                )
            }
            TorchMount::WallRight => {
                let angle = std::f32::consts::FRAC_PI_8;
                (
                    Vec2::new(x + 0.06, y + 0.25),
                    Vec2::new(angle.sin(), angle.cos()) * 0.625,
                )
            }
        };
        let top = base + direction;
        let perpendicular = Vec2::new(-direction.y, direction.x).normalize() * 0.0625;
        let lower_left = base + perpendicular;
        let lower_right = base - perpendicular;
        let upper_left = top + perpendicular;
        let upper_right = top - perpendicular;
        let front = 0.1225;
        let back = -0.0025;
        let side_uvs = tile_uvs(tile);
        let vertical_data = [
            [0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
        ];

        self.push_quad(
            [
                [lower_left.x, lower_left.y, front],
                [lower_right.x, lower_right.y, front],
                [upper_right.x, upper_right.y, front],
                [upper_left.x, upper_left.y, front],
            ],
            side_uvs,
            vertical_data,
        );
        self.push_quad(
            [
                [lower_right.x, lower_right.y, back],
                [lower_left.x, lower_left.y, back],
                [upper_left.x, upper_left.y, back],
                [upper_right.x, upper_right.y, back],
            ],
            side_uvs,
            vertical_data,
        );
        self.push_quad(
            [
                [lower_right.x, lower_right.y, front],
                [lower_right.x, lower_right.y, back],
                [upper_right.x, upper_right.y, back],
                [upper_right.x, upper_right.y, front],
            ],
            side_uvs,
            vertical_data,
        );
        self.push_quad(
            [
                [lower_left.x, lower_left.y, back],
                [lower_left.x, lower_left.y, front],
                [upper_left.x, upper_left.y, front],
                [upper_left.x, upper_left.y, back],
            ],
            side_uvs,
            vertical_data,
        );
        self.push_quad(
            [
                [upper_left.x, upper_left.y, front],
                [upper_right.x, upper_right.y, front],
                [upper_right.x, upper_right.y, back],
                [upper_left.x, upper_left.y, back],
            ],
            tile_region_uvs(tile, 0, 4),
            [[1.0, 0.0, 0.0, 0.0]; 4],
        );
    }

    fn push_quad(&mut self, corners: [[f32; 3]; 4], uvs: [[f32; 2]; 4], effects: [[f32; 4]; 4]) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&corners);
        self.uvs.extend_from_slice(&uvs);
        self.effects.extend_from_slice(&effects);
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn finish(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
        .with_inserted_attribute(ATTRIBUTE_TORCH_EFFECT, self.effects)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

fn tile_uvs(tile: u32) -> [[f32; 2]; 4] {
    tile_region_uvs(tile, 0, TEXTURE_SIZE)
}

fn tile_region_uvs(tile: u32, top: u32, bottom: u32) -> [[f32; 2]; 4] {
    let atlas_width = (ATLAS_TILES * ATLAS_CELL) as f32;
    let start = tile * ATLAS_CELL + GUTTER;
    let u0 = start as f32 / atlas_width;
    let u1 = (start + TEXTURE_SIZE) as f32 / atlas_width;
    let v0 = (GUTTER + top) as f32 / ATLAS_CELL as f32;
    let v1 = (GUTTER + bottom) as f32 / ATLAS_CELL as f32;
    [[u0, v1], [u1, v1], [u1, v0], [u0, v0]]
}

fn lightmap_index(block: u8, sky: u8) -> usize {
    usize::from(sky.min(15)) * LIGHTMAP_SIZE as usize + usize::from(block.min(15))
}

fn classic_brightness(level: f32) -> f32 {
    let normalized = (level / 15.0).clamp(0.0, 1.0);
    0.05 + 0.95 * normalized / (4.0 - 3.0 * normalized)
}

fn generate_lightmap(
    daylight: f32,
    palette: LightingPalette,
    block_intensity: f32,
) -> [LinearRgba; (LIGHTMAP_SIZE * LIGHTMAP_SIZE) as usize] {
    let daylight = daylight.clamp(0.0, 1.0);
    let global_sky_level = 4.0 + daylight * 11.0;
    let sky_tint = palette.skylight;
    let block_tint = TORCH_LIGHT_TINT;
    std::array::from_fn(|index| {
        let block = (index % LIGHTMAP_SIZE as usize) as u8;
        let sky = (index / LIGHTMAP_SIZE as usize) as u8;
        let effective_sky = (f32::from(sky) - (15.0 - global_sky_level)).max(0.0);
        let sky_brightness = classic_brightness(effective_sky);
        let block_brightness = if block == 0 {
            0.0
        } else {
            classic_brightness(f32::from(block)) * block_intensity
        };
        LinearRgba::rgb(
            (sky_brightness * sky_tint[0]).max(block_brightness * block_tint[0]),
            (sky_brightness * sky_tint[1]).max(block_brightness * block_tint[1]),
            (sky_brightness * sky_tint[2]).max(block_brightness * block_tint[2]),
        )
    })
}

fn sample_lightmap(
    colors: &[LinearRgba; (LIGHTMAP_SIZE * LIGHTMAP_SIZE) as usize],
    block: f32,
    sky: f32,
) -> LinearRgba {
    let block = block.clamp(0.0, 15.0);
    let sky = sky.clamp(0.0, 15.0);
    let block_low = block.floor() as u8;
    let block_high = block.ceil() as u8;
    let sky_low = sky.floor() as u8;
    let sky_high = sky.ceil() as u8;
    let lower = mix_color(
        colors[lightmap_index(block_low, sky_low)],
        colors[lightmap_index(block_high, sky_low)],
        block.fract(),
    );
    let upper = mix_color(
        colors[lightmap_index(block_low, sky_high)],
        colors[lightmap_index(block_high, sky_high)],
        block.fract(),
    );
    mix_color(lower, upper, sky.fract())
}

fn mix_color(start: LinearRgba, end: LinearRgba, amount: f32) -> LinearRgba {
    LinearRgba::rgb(
        start.red + (end.red - start.red) * amount,
        start.green + (end.green - start.green) * amount,
        start.blue + (end.blue - start.blue) * amount,
    )
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
    image.sampler = ImageSampler::linear();
    image
}

fn atlas_image(pack: &ResolvedPack) -> Image {
    let width = ATLAS_TILES * ATLAS_CELL;
    let mut pixels = vec![0; (width * ATLAS_CELL * 4) as usize];
    for (state_index, id) in BlockId::ALL.into_iter().enumerate() {
        for face in TextureFace::ALL {
            for variant in 0..ATLAS_VARIANTS {
                let tile =
                    (state_index as u32 * FACE_VARIANTS + face.index()) * ATLAS_VARIANTS + variant;
                let source = &pack
                    .block(pack_block(id), pack_face(face), variant as usize)
                    .pixels;
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

fn apply_texture_pack(
    mut changes: MessageReader<TexturePackChanged>,
    texture_packs: Res<TexturePackCatalog>,
    mut catalog: Option<ResMut<RenderCatalog>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if changes.read().next().is_none() {
        return;
    }
    let Some(catalog) = catalog.as_mut() else {
        return;
    };
    let mut atlas = atlas_image(&texture_packs.active);
    atlas.sampler = ImageSampler::nearest();
    if let Some(mut image) = images.get_mut(&catalog.atlas) {
        *image = atlas;
    }
    for (handle, block) in catalog.hotbar_icons.iter().zip(PackBlock::HOTBAR) {
        if let Some(mut image) = images.get_mut(handle) {
            *image = pack_image(&texture_packs.active.icons[&block]);
        }
    }
    if let Some(mut image) = images.get_mut(&catalog.pack_preview) {
        *image = pack_image(&texture_packs.active.preview);
    }
    let colors = player_colors(texture_packs.active.player);
    catalog.player_palette = colors.map(|color| color.to_linear());
    for (handle, color) in catalog.player_materials.iter().zip(colors) {
        if let Some(mut material) = materials.get_mut(handle) {
            material.base_color = color;
        }
    }
}

pub(crate) fn set_pack_preview(
    catalog: &RenderCatalog,
    images: &mut Assets<Image>,
    preview: &PackImage,
) {
    replace_pack_image(images, &catalog.pack_preview, preview);
}

fn replace_pack_image(images: &mut Assets<Image>, handle: &Handle<Image>, source: &PackImage) {
    if let Some(mut image) = images.get_mut(handle) {
        *image = pack_image(source);
    }
}

fn pack_image(source: &PackImage) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: source.width,
            height: source.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        source.pixels.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
}

fn player_colors(palette: PlayerPalette) -> [Color; 5] {
    [
        srgb8(palette.skin),
        srgb8(palette.shirt),
        srgb8(palette.pants),
        srgb8(palette.hair),
        srgb8(palette.details),
    ]
}

fn srgb8(color: [u8; 3]) -> Color {
    Color::srgb(
        f32::from(color[0]) / 255.0,
        f32::from(color[1]) / 255.0,
        f32::from(color[2]) / 255.0,
    )
}

fn pack_block(id: BlockId) -> PackBlock {
    match id {
        BlockId::GRASS => PackBlock::Grass,
        BlockId::DIRT => PackBlock::Dirt,
        BlockId::STONE => PackBlock::Stone,
        BlockId::COAL_ORE => PackBlock::CoalOre,
        BlockId::IRON_ORE => PackBlock::IronOre,
        BlockId::WOOD => PackBlock::Wood,
        BlockId::LEAVES => PackBlock::Leaves,
        BlockId::TORCH => PackBlock::Torch,
        BlockId::BEDROCK => PackBlock::Bedrock,
        _ => unreachable!("all renderable block ids have texture-pack entries"),
    }
}

fn pack_face(face: TextureFace) -> PackFace {
    match face {
        TextureFace::Side => PackFace::Side,
        TextureFace::Top => PackFace::Top,
        TextureFace::Bottom => PackFace::Bottom,
    }
}

#[cfg(test)]
const GRASS_COLORS: [[u8; 3]; 4] = [[48, 101, 35], [60, 116, 38], [75, 132, 43], [91, 146, 49]];
#[cfg(test)]
const DIRT_COLORS: [[u8; 3]; 4] = [[94, 62, 41], [109, 73, 47], [125, 87, 58], [140, 101, 70]];
#[cfg(test)]
const STONE_COLORS: [[u8; 3]; 4] = [
    [88, 90, 93],
    [102, 104, 107],
    [116, 118, 120],
    [132, 133, 135],
];
#[cfg(test)]
const COAL_COLORS: [[u8; 3]; 3] = [[35, 36, 39], [48, 50, 54], [63, 65, 70]];
#[cfg(test)]
const IRON_COLORS: [[u8; 3]; 3] = [[126, 82, 57], [155, 105, 76], [184, 133, 98]];
#[cfg(test)]
const BARK_COLORS: [[u8; 3]; 4] = [[73, 43, 23], [91, 54, 28], [111, 69, 36], [134, 87, 46]];
#[cfg(test)]
const WOOD_RING_COLORS: [[u8; 3]; 4] = [[96, 59, 29], [118, 76, 38], [143, 98, 52], [164, 119, 67]];
#[cfg(test)]
const LEAF_COLORS: [[u8; 3]; 4] = [[30, 79, 31], [39, 96, 36], [49, 113, 41], [62, 130, 48]];
#[cfg(test)]
const BEDROCK_COLORS: [[u8; 3]; 4] = [[29, 30, 33], [42, 44, 48], [56, 58, 63], [72, 74, 79]];

#[cfg(test)]
const CLUSTER_PATTERN: [[usize; 8]; 8] = [
    [2, 2, 2, 1, 1, 2, 2, 3],
    [2, 1, 1, 1, 2, 2, 3, 3],
    [2, 1, 2, 2, 2, 2, 3, 2],
    [0, 0, 2, 2, 3, 3, 2, 2],
    [0, 2, 2, 2, 3, 2, 2, 1],
    [0, 0, 2, 2, 2, 2, 1, 1],
    [2, 2, 2, 1, 1, 2, 2, 2],
    [3, 3, 2, 1, 2, 2, 2, 3],
];

#[cfg(test)]
fn clustered_color(
    colors: &[[u8; 3]; 4],
    seed: u64,
    x: u32,
    y: u32,
    variant: u8,
    face: TextureFace,
    scale: u32,
) -> [u8; 3] {
    let shifted_x = (x + u32::from(variant) * 3 + face.index() * 5) % TEXTURE_SIZE;
    let shifted_y = (y + u32::from(variant) * 5 + face.index() * 3) % TEXTURE_SIZE;
    let mut pattern_x = ((shifted_x / scale) % 8) as usize;
    let mut pattern_y = ((shifted_y / scale) % 8) as usize;
    if seed & 1 != 0 {
        std::mem::swap(&mut pattern_x, &mut pattern_y);
    }
    if seed & 2 != 0 {
        pattern_x = 7 - pattern_x;
    }
    if seed & 4 != 0 {
        pattern_y = 7 - pattern_y;
    }
    colors[CLUSTER_PATTERN[pattern_y][pattern_x]]
}

#[cfg(test)]
fn grass_color(x: u32, y: u32, variant: u8, face: TextureFace) -> [u8; 3] {
    clustered_color(&GRASS_COLORS, 0x6a51, x, y, variant, face, 2)
}

#[cfg(test)]
fn dirt_color(x: u32, y: u32, variant: u8, face: TextureFace) -> [u8; 3] {
    clustered_color(&DIRT_COLORS, 0xd17, x, y, variant, face, 2)
}

#[cfg(test)]
fn stone_color(x: u32, y: u32, variant: u8, face: TextureFace) -> [u8; 3] {
    clustered_color(&STONE_COLORS, 0x570, x, y, variant, face, 2)
}

#[cfg(test)]
fn ore_color(
    stone: [u8; 3],
    ore_colors: &[[u8; 3]; 3],
    x: u32,
    y: u32,
    variant: u8,
    face: TextureFace,
) -> [u8; 3] {
    const ORE_PIXELS: [(u32, u32, usize); 20] = [
        (2, 2, 0),
        (3, 2, 1),
        (3, 3, 2),
        (4, 3, 0),
        (10, 1, 0),
        (11, 1, 1),
        (11, 2, 2),
        (12, 2, 0),
        (7, 7, 0),
        (8, 7, 1),
        (8, 8, 2),
        (9, 8, 1),
        (9, 9, 0),
        (12, 11, 0),
        (13, 11, 1),
        (12, 12, 2),
        (13, 13, 1),
        (14, 13, 0),
        (2, 12, 0),
        (3, 12, 1),
    ];
    let shifted_x = (x + u32::from(variant) * 5 + face.index() * 3) % TEXTURE_SIZE;
    let shifted_y = (y + u32::from(variant) * 3 + face.index() * 5) % TEXTURE_SIZE;

    for (ore_x, ore_y, index) in ORE_PIXELS {
        if shifted_x == ore_x && shifted_y == ore_y {
            return ore_colors[index];
        }
    }
    stone
}

#[cfg(test)]
fn wood_color(x: u32, y: u32, variant: u8, face: TextureFace) -> [u8; 3] {
    if matches!(face, TextureFace::Top | TextureFace::Bottom) {
        let center = (TEXTURE_SIZE as i32 - 1) / 2;
        let ring = (x as i32 - center).abs().max((y as i32 - center).abs()) as u32;
        let index = ((ring + u32::from(variant)) % 4) as usize;
        return WOOD_RING_COLORS[index];
    }

    let shifted_x = (x + u32::from(variant) * 3) % TEXTURE_SIZE;
    let shifted_y = (y + u32::from(variant) * 5) % TEXTURE_SIZE;
    let mut index = match shifted_x {
        0..=1 => 1,
        2..=4 => 2,
        5 => 0,
        6..=8 => 2,
        9..=10 => 3,
        11 => 1,
        12..=14 => 2,
        _ => 1,
    };
    if matches!(
        (shifted_x, shifted_y),
        (2, 6) | (3, 6) | (9, 3) | (10, 3) | (12, 11) | (13, 11)
    ) {
        index = 1;
    }
    let knot_x = 5 + (u32::from(variant) * 4) % 7;
    let knot_y = 4 + (u32::from(variant) * 5) % 8;
    let knot_distance = (x as i32 - knot_x as i32).abs() + (y as i32 - knot_y as i32).abs();
    if knot_distance <= 1 {
        index = 0;
    }
    BARK_COLORS[index]
}

#[cfg(test)]
fn leaf_pixel(x: u32, y: u32, variant: u8, face: TextureFace) -> ([u8; 3], u8) {
    let color = clustered_color(&LEAF_COLORS, 0x1eaf, x, y, variant, face, 2);
    let shifted_x = (x + u32::from(variant) * 3 + face.index() * 2) % TEXTURE_SIZE;
    let shifted_y = (y + u32::from(variant) * 5 + face.index() * 3) % TEXTURE_SIZE;
    let transparent = matches!(
        (shifted_x, shifted_y),
        (1, 2) | (2, 2) | (10, 3) | (6, 10) | (13, 13) | (4, 14)
    );
    (color, if transparent { 0 } else { 255 })
}

#[cfg(test)]
fn torch_color(x: u32, y: u32) -> [u8; 3] {
    if y < TEXTURE_SIZE / 4 {
        let center_distance = (x as i32 - 7).abs();
        match (y, center_distance, (x + y) % 4) {
            (0, 0..=2, _) => [255, 238, 135],
            (_, 0..=3, 0..=1) => [255, 204, 67],
            (_, _, 0) => [229, 87, 20],
            _ => [247, 139, 26],
        }
    } else {
        let band = (x / 3 + y / 5) % 4;
        match (x == 0 || x == TEXTURE_SIZE - 1, band) {
            (true, _) => [67, 34, 18],
            (false, 0) => [92, 48, 23],
            (false, 1) => [119, 66, 29],
            (false, 2) => [147, 85, 36],
            (false, _) => [104, 55, 25],
        }
    }
}

#[cfg(test)]
pub fn block_pixels(state: BlockState, variant: u8) -> Vec<u8> {
    block_face_pixels(state, variant, TextureFace::Side)
}

#[cfg(test)]
fn block_face_pixels(state: BlockState, variant: u8, face: TextureFace) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((TEXTURE_SIZE * TEXTURE_SIZE * 4) as usize);
    for y in 0..TEXTURE_SIZE {
        for x in 0..TEXTURE_SIZE {
            let (color, alpha) = match state {
                BlockState::GRASS => match face {
                    TextureFace::Top => (grass_color(x, y, variant, face), 255),
                    TextureFace::Bottom => (dirt_color(x, y, variant, face), 255),
                    TextureFace::Side => {
                        const GRASS_FRINGE: [u32; 16] =
                            [4, 4, 3, 5, 4, 3, 6, 4, 4, 5, 3, 4, 6, 3, 5, 4];
                        let fringe_x = (x + u32::from(variant) * 3) as usize % GRASS_FRINGE.len();
                        let grass_depth = GRASS_FRINGE[fringe_x];
                        if y < grass_depth {
                            (grass_color(x, y, variant, face), 255)
                        } else {
                            (dirt_color(x, y, variant, face), 255)
                        }
                    }
                },
                BlockState::DIRT => (dirt_color(x, y, variant, face), 255),
                BlockState::STONE => (stone_color(x, y, variant, face), 255),
                BlockState::COAL_ORE => {
                    let stone = stone_color(x, y, variant, face);
                    (ore_color(stone, &COAL_COLORS, x, y, variant, face), 255)
                }
                BlockState::IRON_ORE => {
                    let stone = stone_color(x, y, variant, face);
                    (ore_color(stone, &IRON_COLORS, x, y, variant, face), 255)
                }
                BlockState::WOOD => (wood_color(x, y, variant, face), 255),
                BlockState::LEAVES => leaf_pixel(x, y, variant, face),
                BlockState::BEDROCK => (
                    clustered_color(&BEDROCK_COLORS, 0xbed, x, y, variant, face, 2),
                    255,
                ),
                BlockState::TORCH => (torch_color(x, y), 255),
                _ => unreachable!("all valid block states have generated texture art"),
            };
            pixels.extend_from_slice(&[color[0], color[1], color[2], alpha]);
        }
    }
    pixels
}

#[cfg(test)]
pub fn torch_pixels() -> Vec<u8> {
    let mut pixels = Vec::with_capacity((TEXTURE_SIZE * TEXTURE_SIZE * 4) as usize);
    for y in 0..TEXTURE_SIZE {
        for x in 0..TEXTURE_SIZE {
            let color = torch_color(x, y);
            pixels.extend_from_slice(&[color[0], color[1], color[2], 255]);
        }
    }
    pixels
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
    fn pack_image_replacement_preserves_the_asset_handle() {
        let mut images = Assets::<Image>::default();
        let handle = images.add(pack_image(&PackImage::solid(16, 16, [1, 2, 3, 255])));
        let id = handle.id();
        replace_pack_image(
            &mut images,
            &handle,
            &PackImage::solid(16, 16, [9, 8, 7, 255]),
        );
        assert_eq!(handle.id(), id);
        assert_eq!(
            images.get(&handle).unwrap().data.as_ref().unwrap()[..4],
            [9, 8, 7, 255]
        );
    }

    #[test]
    fn generated_texture_tiles_are_low_resolution_and_deterministic() {
        assert_eq!(block_pixels(BlockState::DIRT, 1).len(), 16 * 16 * 4);
        assert_eq!(
            block_pixels(BlockState::DIRT, 1),
            block_pixels(BlockState::DIRT, 1)
        );
        assert_ne!(
            block_pixels(BlockState::DIRT, 1),
            block_pixels(BlockState::DIRT, 2)
        );
        let alphas: Vec<_> = torch_pixels().iter().skip(3).step_by(4).copied().collect();
        assert!(alphas.iter().all(|alpha| *alpha == 255));
        let leaf_alphas: Vec<_> = block_pixels(BlockState::LEAVES, 1)
            .iter()
            .skip(3)
            .step_by(4)
            .copied()
            .collect();
        assert!(leaf_alphas.contains(&0));
        assert!(leaf_alphas.contains(&255));
        assert_ne!(
            &torch_pixels()[..4],
            &torch_pixels()[TEXTURE_SIZE as usize * 8 * 4..][..4]
        );
    }

    #[test]
    fn grass_uses_grass_top_dirt_bottom_and_quarter_height_side_cap() {
        let top = block_face_pixels(BlockState::GRASS, 0, TextureFace::Top);
        let side = block_face_pixels(BlockState::GRASS, 0, TextureFace::Side);
        let bottom = block_face_pixels(BlockState::GRASS, 0, TextureFace::Bottom);
        let pixel = |pixels: &[u8], x: u32, y: u32| {
            let index = ((y * TEXTURE_SIZE + x) * 4) as usize;
            [pixels[index], pixels[index + 1], pixels[index + 2]]
        };
        let uses_palette = |color: [u8; 3], colors: &[[u8; 3]; 4]| colors.contains(&color);

        for y in 0..TEXTURE_SIZE {
            for x in 0..TEXTURE_SIZE {
                assert!(uses_palette(pixel(&top, x, y), &GRASS_COLORS));
                assert!(uses_palette(pixel(&bottom, x, y), &DIRT_COLORS));
                let side_color = pixel(&side, x, y);
                if y < 3 {
                    assert!(uses_palette(side_color, &GRASS_COLORS));
                } else if y < 6 {
                    assert!(
                        uses_palette(side_color, &GRASS_COLORS)
                            || uses_palette(side_color, &DIRT_COLORS)
                    );
                } else {
                    assert!(uses_palette(side_color, &DIRT_COLORS));
                }
            }
        }
    }

    #[test]
    fn generated_materials_use_clustered_multi_tone_art() {
        let unique_colors = |pixels: Vec<u8>| {
            pixels
                .chunks_exact(4)
                .filter(|pixel| pixel[3] != 0)
                .map(|pixel| [pixel[0], pixel[1], pixel[2]])
                .collect::<std::collections::HashSet<_>>()
        };

        assert!(unique_colors(block_pixels(BlockState::DIRT, 0)).len() >= 4);
        assert!(unique_colors(block_pixels(BlockState::STONE, 0)).len() >= 4);
        assert!(unique_colors(block_pixels(BlockState::COAL_ORE, 0)).len() >= 6);
        assert!(unique_colors(block_pixels(BlockState::IRON_ORE, 0)).len() >= 6);
        assert!(unique_colors(block_pixels(BlockState::WOOD, 0)).len() >= 4);
        assert!(unique_colors(block_pixels(BlockState::LEAVES, 0)).len() >= 4);
        assert!(unique_colors(block_pixels(BlockState::BEDROCK, 0)).len() >= 4);

        assert_ne!(
            block_face_pixels(BlockState::WOOD, 0, TextureFace::Top),
            block_face_pixels(BlockState::WOOD, 0, TextureFace::Side)
        );
    }

    #[test]
    fn a_single_voxel_emits_six_faces() {
        let mut builder = VoxelMeshBuilder::default();
        for face in Face::ALL {
            builder.push_voxel_face(
                0.0,
                0.0,
                0,
                BlockState::STONE,
                0,
                face,
                [FaceVertexLight {
                    sky: 15.0,
                    block: 0.0,
                    shade: face.shade(),
                }; 4],
            );
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
    fn corner_ambient_occlusion_uses_classic_side_rule() {
        assert_eq!(vertex_ao(false, false, false), 3);
        assert_eq!(vertex_ao(false, false, true), 2);
        assert_eq!(vertex_ao(true, false, true), 1);
        assert_eq!(vertex_ao(true, true, false), 0);
        assert_eq!(vertex_ao(true, true, true), 0);

        let voxel_at = |_: i64, _: i32, depth: i32| match depth {
            0 => Some(BlockState::STONE),
            1 => Some(BlockState::LEAVES),
            _ => None,
        };
        assert!(fully_opaque((0, 0, 0), &voxel_at));
        assert!(!fully_opaque((0, 0, 1), &voxel_at));
    }

    #[test]
    fn filtered_face_lighting_stays_uniform_after_ambient_occlusion() {
        let voxel_at = |x: i64, y: i32, depth: i32| {
            matches!((x, y, depth), (2, 1, 1) | (1, 2, 1) | (2, 2, 0)).then_some(BlockState::STONE)
        };
        let light = LightVolume::calculate(0, 5, 5, DEPTH_SLICES, |x, y, depth| {
            voxel_at(x, y, i32::from(depth))
        });
        let samples = face_vertex_lighting(&light, 2, 1, 1, Face::Top, &voxel_at);

        assert!(samples[0].shade < 1.0);
        assert!(samples.iter().all(|sample| *sample == samples[0]));
    }

    #[test]
    fn lightmap_keeps_sky_and_block_channels_distinct() {
        let noon_day = crate::domain::DayCycle::from_ticks(6_000);
        let midnight_day = crate::domain::DayCycle::from_ticks(18_000);
        let noon = generate_lightmap(1.0, LightingPalette::from_day(&noon_day), 1.0);
        let midnight = generate_lightmap(0.0, LightingPalette::from_day(&midnight_day), 1.0);
        let full_sky = noon[lightmap_index(0, 15)];
        let night_sky = midnight[lightmap_index(0, 15)];
        let torch = midnight[lightmap_index(14, 0)];
        assert!(full_sky.red > night_sky.red);
        assert!(torch.red > torch.green);
        assert!(torch.green > torch.blue);
        assert_eq!(encode_lightmap(&noon).len(), 16 * 16 * 4);
    }

    #[test]
    fn lightmap_sampling_interpolates_both_channels() {
        let colors = std::array::from_fn(|index| {
            let block = (index % LIGHTMAP_SIZE as usize) as f32;
            let sky = (index / LIGHTMAP_SIZE as usize) as f32;
            LinearRgba::rgb(block / 15.0, sky / 15.0, 0.0)
        });
        let sampled = sample_lightmap(&colors, 7.5, 3.25);
        assert!((sampled.red - 0.5).abs() < 0.0001);
        assert!((sampled.green - 3.25 / 15.0).abs() < 0.0001);
    }

    #[test]
    fn torch_mesh_is_a_compact_cuboid_with_an_emissive_cap() {
        let mut first = TorchMeshBuilder::default();
        first.push_torch(2.0, 3.0, 0, TorchMount::Floor);
        let first = first.finish();
        let mut second = TorchMeshBuilder::default();
        second.push_torch(2.0, 3.0, 0, TorchMount::Floor);
        let second = second.finish();

        assert_eq!(first.count_vertices(), 20);
        assert_eq!(first.indices().unwrap().len(), 30);
        let Some(bevy::mesh::VertexAttributeValues::Float32x4(first_effects)) =
            first.attribute(ATTRIBUTE_TORCH_EFFECT)
        else {
            panic!("torch effect attribute must be float32x4");
        };
        let Some(bevy::mesh::VertexAttributeValues::Float32x4(second_effects)) =
            second.attribute(ATTRIBUTE_TORCH_EFFECT)
        else {
            panic!("torch effect attribute must be float32x4");
        };
        assert_eq!(first_effects, second_effects);
        assert!(first_effects.iter().any(|effect| effect[0] == 0.0));
        assert!(first_effects.iter().any(|effect| effect[0] == 1.0));
    }

    #[test]
    fn wall_torch_meshes_are_mirrored_and_lean_away_from_support() {
        let mut left = TorchMeshBuilder::default();
        left.push_torch(0.0, 0.0, 0, TorchMount::WallLeft);
        let left = left.finish();
        let mut right = TorchMeshBuilder::default();
        right.push_torch(0.0, 0.0, 0, TorchMount::WallRight);
        let right = right.finish();
        let left_positions = left
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        let right_positions = right
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();

        for left in left_positions {
            assert!(right_positions.iter().any(|right| {
                (left[0] + right[0] - 1.0).abs() < 0.0001
                    && (left[1] - right[1]).abs() < 0.0001
                    && (left[2] - right[2]).abs() < 0.0001
            }));
        }
        let left_base_x = (left_positions[0][0] + left_positions[1][0]) * 0.5;
        let left_top_x = (left_positions[2][0] + left_positions[3][0]) * 0.5;
        assert!(left_top_x < left_base_x);
    }

    #[test]
    fn flicker_changes_only_block_light_and_stays_bounded() {
        for step in 0..1_000 {
            let intensity = torch_flicker(step as f32 / 60.0);
            assert!((0.92..=1.08).contains(&intensity));
        }
        let midnight_day = crate::domain::DayCycle::from_ticks(18_000);
        let palette = LightingPalette::from_day(&midnight_day);
        let low = generate_lightmap(0.0, palette, 0.92);
        let high = generate_lightmap(0.0, palette, 1.08);
        assert!(high[lightmap_index(14, 0)].red > low[lightmap_index(14, 0)].red);
        assert_eq!(high[lightmap_index(0, 15)], low[lightmap_index(0, 15)]);
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
        assert!(positions.iter().any(|position| position[2] <= -4.5));
        assert!(built.cutout.count_vertices() > 0);
        assert!(built.emissive.count_vertices() > 0);
        assert!(build_chunk_collider(&grid.view(), 0).is_some());
    }
}
