use crate::adapters::bevy::world::WorldEntity;
use crate::domain::{
    BlockGrid, BlockKind, CHUNK_WIDTH, DEPTH_SLICES, DayCycle, LightGrid, WORLD_HEIGHT,
    generated_voxel, stable_hash, surface_height_at_depth, world_to_chunk,
};
use avian2d::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

const TEXTURE_SIZE: u32 = 32;
const ATLAS_VARIANTS: u32 = 3;
const FACE_VARIANTS: u32 = 3;
const GUTTER: u32 = 2;
const ATLAS_CELL: u32 = TEXTURE_SIZE + GUTTER * 2;
const ATLAS_TILES: u32 = BlockKind::ALL.len() as u32 * ATLAS_VARIANTS * FACE_VARIANTS;

#[derive(Resource)]
pub struct RenderCatalog {
    pub opaque_material: Handle<StandardMaterial>,
    pub cutout_material: Handle<StandardMaterial>,
    pub emissive_material: Handle<StandardMaterial>,
    selection_mesh: Handle<Mesh>,
    selection_material: Handle<StandardMaterial>,
    pub player_cube: Handle<Mesh>,
    pub player_materials: Vec<Handle<StandardMaterial>>,
}

#[derive(Component)]
pub struct SelectionOutline;

pub struct RenderingPlugin;

impl Plugin for RenderingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, build_catalog);
    }
}

fn build_catalog(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let atlas = images.add(atlas_image());
    let opaque_material = materials.add(StandardMaterial {
        base_color_texture: Some(atlas.clone()),
        perceptual_roughness: 0.94,
        reflectance: 0.15,
        ..default()
    });
    let cutout_material = materials.add(StandardMaterial {
        base_color_texture: Some(atlas.clone()),
        alpha_mode: AlphaMode::Mask(0.38),
        perceptual_roughness: 0.98,
        reflectance: 0.08,
        ..default()
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
    let player_materials = [
        Color::srgb(0.78, 0.52, 0.32),
        Color::srgb(0.08, 0.38, 0.46),
        Color::srgb(0.12, 0.16, 0.25),
        Color::srgb(0.18, 0.09, 0.045),
        Color::srgb(0.10, 0.08, 0.06),
    ]
    .into_iter()
    .map(|base_color| {
        materials.add(StandardMaterial {
            base_color,
            perceptual_roughness: 0.9,
            reflectance: 0.12,
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
    });
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
            Self::Top => 1.08,
            Self::Front => 0.96,
            Self::Right => 0.82,
            Self::Left => 0.68,
            Self::Back => 0.62,
            Self::Bottom => 0.55,
        }
    }
}

pub struct ChunkMeshes {
    pub opaque: Mesh,
    pub cutout: Mesh,
    pub emissive: Mesh,
}

pub fn build_chunk_meshes(
    grid: &BlockGrid,
    chunk_x: i32,
    global_chunk_x: i64,
    seed: u64,
    light: &LightGrid,
    day: &DayCycle,
) -> ChunkMeshes {
    let mut opaque = VoxelMeshBuilder::default();
    let mut cutout = VoxelMeshBuilder::default();
    let mut emissive = VoxelMeshBuilder::default();
    let local_start_x = chunk_x * CHUNK_WIDTH;
    let global_origin_x = (global_chunk_x - i64::from(chunk_x)) * i64::from(CHUNK_WIDTH);

    let voxel_at = |local_x: i32, y: i32, depth: i32| -> Option<BlockKind> {
        if !(0..WORLD_HEIGHT).contains(&y) || !(0..i32::from(DEPTH_SLICES)).contains(&depth) {
            return None;
        }
        let global_x = global_origin_x + i64::from(local_x);
        if depth == 0 && grid.contains_chunk(world_to_chunk(local_x)) {
            grid.get(IVec2::new(local_x, y))
        } else {
            generated_voxel(seed, global_x, y, depth as u8)
        }
    };

    for local_x in 0..CHUNK_WIDTH {
        let world_x = local_start_x + local_x;
        let global_x = global_chunk_x * i64::from(CHUNK_WIDTH) + i64::from(local_x);
        for y in 0..WORLD_HEIGHT {
            for depth in 0..DEPTH_SLICES {
                let Some(kind) = voxel_at(world_x, y, i32::from(depth)) else {
                    continue;
                };
                let variant = (stable_hash(seed, global_x, y, kind as u64 + u64::from(depth) * 97)
                    % 3) as u32;
                if kind == BlockKind::Torch {
                    if depth == 0 {
                        emissive.push_torch(local_x as f32, y as f32, variant);
                    }
                    continue;
                }
                let builder = if kind == BlockKind::Leaves {
                    &mut cutout
                } else {
                    &mut opaque
                };
                for face in Face::ALL {
                    let (dx, dy, dd) = face.neighbor();
                    if voxel_at(world_x + dx, y + dy, i32::from(depth) + dd)
                        .is_some_and(|neighbor| neighbor != BlockKind::Torch)
                    {
                        continue;
                    }
                    let color = face_color(
                        grid,
                        light,
                        day,
                        global_x,
                        IVec2::new(world_x, y),
                        depth,
                        face,
                        seed,
                    );
                    builder.push_voxel_face(
                        local_x as f32,
                        y as f32,
                        depth,
                        kind,
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

#[allow(clippy::too_many_arguments)]
fn face_color(
    grid: &BlockGrid,
    light: &LightGrid,
    day: &DayCycle,
    global_x: i64,
    coordinate: IVec2,
    depth: u8,
    face: Face,
    seed: u64,
) -> [f32; 4] {
    let daylight = day.daylight();
    let (level, torch) = if depth == 0 {
        let visible = light.visible_at(grid, coordinate);
        (
            f32::from(
                visible
                    .sky
                    .saturating_mul(day.light_level())
                    .saturating_div(15)
                    .max(visible.torch),
            ) / 15.0,
            f32::from(visible.torch) / 15.0,
        )
    } else {
        let surface = surface_height_at_depth(seed, global_x, depth);
        let exposure = if coordinate.y >= surface - 4 {
            1.0
        } else {
            0.42
        };
        (daylight * exposure, 0.0)
    };
    let depth_factor = 1.0 - f32::from(depth) * 0.075;
    let brightness = (0.28 + level * 0.72) * face.shade() * depth_factor;
    let warmth = torch * 0.16;
    [
        (brightness + warmth).min(1.15),
        (brightness + warmth * 0.55).min(1.08),
        (brightness + f32::from(depth) * 0.015).min(1.0),
        1.0,
    ]
}

pub fn build_chunk_collider(grid: &BlockGrid, chunk_x: i32) -> Option<Collider> {
    let start_x = chunk_x * CHUNK_WIDTH;
    let mut shapes = Vec::new();
    for y in 0..WORLD_HEIGHT {
        let mut run_start = None;
        for local_x in 0..=CHUNK_WIDTH {
            let solid = local_x < CHUNK_WIDTH
                && grid
                    .get(IVec2::new(start_x + local_x, y))
                    .is_some_and(|kind| kind.def().solid);
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
        kind: BlockKind,
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
        let tile = (kind as u32 * FACE_VARIANTS + face.texture_class()) * ATLAS_VARIANTS + variant;
        self.push_quad(corners, face.normal(), tile_uvs(tile), color);
    }

    fn push_torch(&mut self, x: f32, y: f32, variant: u32) {
        let tile = (BlockKind::Torch as u32 * FACE_VARIANTS) * ATLAS_VARIANTS + variant;
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

fn atlas_image() -> Image {
    let width = ATLAS_TILES * ATLAS_CELL;
    let mut pixels = vec![0; (width * ATLAS_CELL * 4) as usize];
    for (kind_index, kind) in BlockKind::ALL.into_iter().enumerate() {
        for face in 0..FACE_VARIANTS {
            for variant in 0..ATLAS_VARIANTS {
                let tile = (kind_index as u32 * FACE_VARIANTS + face) * ATLAS_VARIANTS + variant;
                let source = if kind == BlockKind::Torch {
                    torch_pixels()
                } else {
                    block_face_pixels(kind, variant as u8, face)
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

fn palette(kind: BlockKind) -> ([u8; 3], [u8; 3]) {
    match kind {
        BlockKind::Grass => ([86, 142, 48], [45, 92, 32]),
        BlockKind::Dirt => ([128, 83, 48], [82, 48, 29]),
        BlockKind::Stone => ([110, 114, 119], [66, 70, 76]),
        BlockKind::CoalOre => ([93, 96, 101], [22, 24, 29]),
        BlockKind::IronOre => ([112, 104, 94], [197, 124, 78]),
        BlockKind::Wood => ([137, 88, 40], [72, 43, 22]),
        BlockKind::Leaves => ([54, 132, 52], [25, 76, 35]),
        BlockKind::Bedrock => ([55, 58, 64], [20, 22, 27]),
        BlockKind::Torch => ([224, 126, 28], [255, 221, 91]),
    }
}

#[cfg(test)]
pub fn block_pixels(kind: BlockKind, variant: u8) -> Vec<u8> {
    block_face_pixels(kind, variant, 0)
}

fn block_face_pixels(kind: BlockKind, variant: u8, face: u32) -> Vec<u8> {
    let (base, accent) = palette(kind);
    let mut pixels = Vec::with_capacity((TEXTURE_SIZE * TEXTURE_SIZE * 4) as usize);
    for y in 0..TEXTURE_SIZE {
        for x in 0..TEXTURE_SIZE {
            let hash = stable_hash(
                u64::from(variant) + u64::from(face) * 131,
                i64::from(x),
                y as i32,
                kind as u64 + 700,
            );
            let edge = x == 0 || y == 0 || x == TEXTURE_SIZE - 1 || y == TEXTURE_SIZE - 1;
            let grass_blade = kind == BlockKind::Grass
                && ((face == 1 && hash % 7 < 3) || (face != 1 && y < 6 && hash % 5 < 3));
            let wood_grain = kind == BlockKind::Wood
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
            let alpha = if kind == BlockKind::Leaves && hash.is_multiple_of(19) {
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
    use crate::domain::BlockChunk;

    fn empty_grid() -> BlockGrid {
        let mut grid = BlockGrid::new(WORLD_HEIGHT);
        grid.insert_chunk(
            BlockChunk::from_dense(0, vec![0; (CHUNK_WIDTH * WORLD_HEIGHT) as usize]).unwrap(),
        );
        grid
    }

    #[test]
    fn generated_texture_tiles_are_deterministic_and_detailed() {
        assert_eq!(block_pixels(BlockKind::Dirt, 1).len(), 32 * 32 * 4);
        assert_eq!(
            block_pixels(BlockKind::Dirt, 1),
            block_pixels(BlockKind::Dirt, 1)
        );
        assert_ne!(
            block_pixels(BlockKind::Dirt, 1),
            block_pixels(BlockKind::Dirt, 2)
        );
        let alphas: Vec<_> = torch_pixels().iter().skip(3).step_by(4).copied().collect();
        assert!(alphas.contains(&0));
        assert!(alphas.contains(&255));
    }

    #[test]
    fn a_single_voxel_emits_six_faces() {
        let mut builder = VoxelMeshBuilder::default();
        for face in Face::ALL {
            builder.push_voxel_face(0.0, 0.0, 0, BlockKind::Stone, 0, face, [1.0; 4]);
        }
        let mesh = builder.finish();
        assert_eq!(mesh.count_vertices(), 24);
        assert_eq!(mesh.indices().unwrap().len(), 36);
    }

    #[test]
    fn chunk_mesh_contains_real_depth_and_keeps_collider_two_dimensional() {
        let mut grid = empty_grid();
        grid.set(IVec2::new(0, 0), BlockKind::Bedrock);
        grid.set(IVec2::new(1, 1), BlockKind::Leaves);
        grid.set(IVec2::new(2, 1), BlockKind::Torch);
        let light = LightGrid::calculate(&grid);
        let built = build_chunk_meshes(&grid, 0, 0, 7, &light, &DayCycle::default());
        let positions = built
            .opaque
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        assert!(positions.iter().any(|position| position[2] <= -2.5));
        assert!(built.cutout.count_vertices() > 0);
        assert!(built.emissive.count_vertices() > 0);
        assert!(build_chunk_collider(&grid, 0).is_some());
    }
}
