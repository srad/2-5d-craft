use bevy::prelude::Resource;
use sidecraft_textures::{BlockKind, SHOWCASE};
use std::collections::VecDeque;

const MAX_LIGHT: u8 = 15;
const TORCH_LIGHT: u8 = 14;
const TORCH_TINT: [f32; 3] = [1.0, 0.42, 0.08];
const AO_SHADE: [f32; 4] = [0.72, 0.82, 0.91, 1.0];

type VoxelOffset = (i32, i32, i32);
type CornerTangents = [(VoxelOffset, VoxelOffset); 4];

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Resource)]
pub(crate) enum PreviewLighting {
    #[default]
    Day,
    Night,
}

#[derive(Debug, Clone, Copy, Default)]
struct LightCell {
    sky: u8,
    block: u8,
}

pub(super) struct LightField {
    cells: Vec<LightCell>,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum VoxelFace {
    Front,
    Back,
    Right,
    Left,
    Top,
    Bottom,
}

impl VoxelFace {
    pub const ALL: [Self; 6] = [
        Self::Front,
        Self::Back,
        Self::Right,
        Self::Left,
        Self::Top,
        Self::Bottom,
    ];

    pub const fn neighbor(self) -> (i32, i32, i32) {
        match self {
            Self::Front => (0, 0, -1),
            Self::Back => (0, 0, 1),
            Self::Right => (1, 0, 0),
            Self::Left => (-1, 0, 0),
            Self::Top => (0, 1, 0),
            Self::Bottom => (0, -1, 0),
        }
    }

    pub const fn normal(self) -> [f32; 3] {
        match self {
            Self::Front => [0.0, 0.0, 1.0],
            Self::Back => [0.0, 0.0, -1.0],
            Self::Right => [1.0, 0.0, 0.0],
            Self::Left => [-1.0, 0.0, 0.0],
            Self::Top => [0.0, 1.0, 0.0],
            Self::Bottom => [0.0, -1.0, 0.0],
        }
    }

    pub const fn shade(self) -> f32 {
        match self {
            Self::Top => 1.0,
            Self::Bottom => 0.5,
            Self::Front | Self::Back => 0.8,
            Self::Right | Self::Left => 0.6,
        }
    }

    fn tangents(self) -> CornerTangents {
        const LEFT: VoxelOffset = (-1, 0, 0);
        const RIGHT: VoxelOffset = (1, 0, 0);
        const DOWN: VoxelOffset = (0, -1, 0);
        const UP: VoxelOffset = (0, 1, 0);
        const FRONT: VoxelOffset = (0, 0, -1);
        const BACK: VoxelOffset = (0, 0, 1);
        match self {
            Self::Front | Self::Back => [(LEFT, DOWN), (RIGHT, DOWN), (RIGHT, UP), (LEFT, UP)],
            Self::Right | Self::Left => [(FRONT, DOWN), (BACK, DOWN), (BACK, UP), (FRONT, UP)],
            Self::Top | Self::Bottom => {
                [(LEFT, FRONT), (RIGHT, FRONT), (RIGHT, BACK), (LEFT, BACK)]
            }
        }
    }
}

impl LightField {
    pub fn calculate() -> Self {
        let mut field = Self {
            cells: vec![
                LightCell::default();
                SHOWCASE.width as usize * SHOWCASE.height as usize * SHOWCASE.depth as usize
            ],
        };
        let mut sky_queue = VecDeque::new();
        let mut block_queue = VecDeque::new();
        let top = SHOWCASE.height - 1;
        for x in SHOWCASE.min_x..SHOWCASE.min_x + i64::from(SHOWCASE.width) {
            for depth in 0..i32::from(SHOWCASE.depth) {
                if !opaque(x, top, depth) {
                    field.set_sky(x, top, depth, MAX_LIGHT);
                    sky_queue.push_back((x, top, depth));
                }
            }
        }
        for x in SHOWCASE.min_x..SHOWCASE.min_x + i64::from(SHOWCASE.width) {
            for y in 0..SHOWCASE.height {
                for depth in 0..i32::from(SHOWCASE.depth) {
                    if SHOWCASE
                        .cell(x, y, depth)
                        .is_some_and(|cell| cell.block == BlockKind::Torch)
                    {
                        field.set_block(x, y, depth, TORCH_LIGHT);
                        block_queue.push_back((x, y, depth));
                    }
                }
            }
        }
        field.propagate(&mut sky_queue, true);
        field.propagate(&mut block_queue, false);
        field
    }

    pub fn face_color(
        &self,
        mode: PreviewLighting,
        x: i64,
        y: i32,
        depth: i32,
        face: VoxelFace,
    ) -> [f32; 4] {
        let (dx, dy, dd) = face.neighbor();
        let sample = self.sample(x + i64::from(dx), y + dy, depth + dd);
        let light = light_color(mode, sample);
        let shade = face.shade() * face_ao(x, y, depth, face);
        [
            (light[0] * shade).min(1.0),
            (light[1] * shade).min(1.0),
            (light[2] * shade).min(1.0),
            1.0,
        ]
    }

    fn propagate(&mut self, queue: &mut VecDeque<(i64, i32, i32)>, sky: bool) {
        while let Some((x, y, depth)) = queue.pop_front() {
            let current = if sky {
                self.get(x, y, depth).sky
            } else {
                self.get(x, y, depth).block
            };
            if current <= 1 {
                continue;
            }
            for (dx, dy, dd) in [
                (1, 0, 0),
                (-1, 0, 0),
                (0, 1, 0),
                (0, -1, 0),
                (0, 0, 1),
                (0, 0, -1),
            ] {
                let next = (x + i64::from(dx), y + dy, depth + dd);
                if !inside(next.0, next.1, next.2) || opaque(next.0, next.1, next.2) {
                    continue;
                }
                let level = current - 1;
                let existing = self.get(next.0, next.1, next.2);
                if (sky && existing.sky >= level) || (!sky && existing.block >= level) {
                    continue;
                }
                if sky {
                    self.set_sky(next.0, next.1, next.2, level);
                } else {
                    self.set_block(next.0, next.1, next.2, level);
                }
                queue.push_back(next);
            }
        }
    }

    fn sample(&self, x: i64, y: i32, depth: i32) -> LightCell {
        if y >= SHOWCASE.height
            || x < SHOWCASE.min_x
            || x >= SHOWCASE.min_x + i64::from(SHOWCASE.width)
        {
            return LightCell {
                sky: MAX_LIGHT,
                block: 0,
            };
        }
        if depth < 0 {
            return LightCell {
                sky: MAX_LIGHT,
                block: self.nearby_block_light(x, y, 0),
            };
        }
        if y < 0 || depth >= i32::from(SHOWCASE.depth) {
            return LightCell::default();
        }
        self.get(x, y, depth)
    }

    fn nearby_block_light(&self, x: i64, y: i32, depth: i32) -> u8 {
        [
            (x, y, depth),
            (x - 1, y, depth),
            (x + 1, y, depth),
            (x, y - 1, depth),
            (x, y + 1, depth),
        ]
        .into_iter()
        .filter(|position| inside(position.0, position.1, position.2))
        .map(|position| self.get(position.0, position.1, position.2).block)
        .max()
        .unwrap_or(0)
    }

    fn get(&self, x: i64, y: i32, depth: i32) -> LightCell {
        self.cells[index(x, y, depth)]
    }

    fn set_sky(&mut self, x: i64, y: i32, depth: i32, level: u8) {
        let index = index(x, y, depth);
        self.cells[index].sky = level;
    }

    fn set_block(&mut self, x: i64, y: i32, depth: i32, level: u8) {
        let index = index(x, y, depth);
        self.cells[index].block = level;
    }
}

pub(super) fn torch_color(mode: PreviewLighting, cap: bool) -> [f32; 4] {
    if cap {
        [1.0, 0.72, 0.24, 1.0]
    } else {
        let base = match mode {
            PreviewLighting::Day => [0.90, 0.82, 0.68],
            PreviewLighting::Night => [1.0, 0.58, 0.20],
        };
        [base[0], base[1], base[2], 1.0]
    }
}

pub(super) fn player_tint(mode: PreviewLighting) -> [f32; 3] {
    match mode {
        PreviewLighting::Day => [0.96, 0.94, 0.88],
        PreviewLighting::Night => [0.46, 0.50, 0.72],
    }
}

fn light_color(mode: PreviewLighting, cell: LightCell) -> [f32; 3] {
    let (global_sky, sky_tint) = match mode {
        PreviewLighting::Day => (15.0, [1.0, 0.98, 0.92]),
        PreviewLighting::Night => (4.0, [0.34, 0.46, 0.90]),
    };
    let effective_sky = (f32::from(cell.sky) - (15.0 - global_sky)).max(0.0);
    let sky = classic_brightness(effective_sky);
    let block = if cell.block == 0 {
        0.0
    } else {
        classic_brightness(f32::from(cell.block))
    };
    std::array::from_fn(|channel| {
        (sky * sky_tint[channel])
            .max(block * TORCH_TINT[channel])
            .min(1.0)
    })
}

fn classic_brightness(level: f32) -> f32 {
    let normalized = (level / 15.0).clamp(0.0, 1.0);
    0.05 + 0.95 * normalized / (4.0 - 3.0 * normalized)
}

fn face_ao(x: i64, y: i32, depth: i32, face: VoxelFace) -> f32 {
    let (nx, ny, nd) = face.neighbor();
    let base = (x + i64::from(nx), y + ny, depth + nd);
    let total = face
        .tangents()
        .into_iter()
        .map(|(a, b)| {
            let side_a = offset(base, a);
            let side_b = offset(base, b);
            let corner = offset(side_a, b);
            vertex_ao(
                opaque(side_a.0, side_a.1, side_a.2),
                opaque(side_b.0, side_b.1, side_b.2),
                opaque(corner.0, corner.1, corner.2),
            )
        })
        .sum::<usize>();
    AO_SHADE[(total + 2) / 4]
}

fn vertex_ao(side_a: bool, side_b: bool, corner: bool) -> usize {
    if side_a && side_b {
        0
    } else {
        3 - usize::from(side_a) - usize::from(side_b) - usize::from(corner)
    }
}

fn offset(position: (i64, i32, i32), delta: VoxelOffset) -> (i64, i32, i32) {
    (
        position.0 + i64::from(delta.0),
        position.1 + delta.1,
        position.2 + delta.2,
    )
}

fn opaque(x: i64, y: i32, depth: i32) -> bool {
    SHOWCASE
        .cell(x, y, depth)
        .is_some_and(|cell| !matches!(cell.block, BlockKind::Leaves | BlockKind::Torch))
}

fn inside(x: i64, y: i32, depth: i32) -> bool {
    (SHOWCASE.min_x..SHOWCASE.min_x + i64::from(SHOWCASE.width)).contains(&x)
        && (0..SHOWCASE.height).contains(&y)
        && (0..i32::from(SHOWCASE.depth)).contains(&depth)
}

fn index(x: i64, y: i32, depth: i32) -> usize {
    let local_x = (x - SHOWCASE.min_x) as usize;
    (depth as usize * SHOWCASE.height as usize + y as usize) * SHOWCASE.width as usize + local_x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_defaults_to_day_and_day_is_brighter_than_night() {
        assert_eq!(PreviewLighting::default(), PreviewLighting::Day);
        let field = LightField::calculate();
        let day = field.face_color(PreviewLighting::Day, 20, 16, 0, VoxelFace::Front);
        let night = field.face_color(PreviewLighting::Night, 20, 16, 0, VoxelFace::Front);
        assert!(day[0] > night[0]);
        assert!(day[1] > night[1]);
    }

    #[test]
    fn torch_light_is_warm_and_propagates_to_neighbors() {
        let field = LightField::calculate();
        let lit = field.sample(1, 3, 0);
        assert!(lit.block > 0);
        let color = light_color(PreviewLighting::Night, lit);
        assert!(color[0] > color[1]);
        assert!(color[1] > color[2]);
    }

    #[test]
    fn directional_face_shades_match_the_game_presentation() {
        assert_eq!(VoxelFace::Top.shade(), 1.0);
        assert_eq!(VoxelFace::Bottom.shade(), 0.5);
        assert_eq!(VoxelFace::Front.shade(), 0.8);
        assert_eq!(VoxelFace::Right.shade(), 0.6);
    }
}
