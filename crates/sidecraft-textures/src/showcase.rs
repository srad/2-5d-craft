use crate::BlockKind;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShowcaseSpec {
    pub min_x: i64,
    pub width: i32,
    pub height: i32,
    pub depth: u8,
    pub inspection_width: f32,
    pub inspection_height: f32,
    pub camera_target: [f32; 2],
    pub seed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShowcaseCell {
    pub block: BlockKind,
    pub torch_mount: Option<ShowcaseTorchMount>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShowcaseTorchMount {
    Floor,
    WallLeft,
    WallRight,
}

pub const SHOWCASE: ShowcaseSpec = ShowcaseSpec {
    min_x: -24,
    width: 48,
    height: 27,
    depth: 6,
    inspection_width: 36.0,
    inspection_height: 20.0,
    camera_target: [0.0, 11.5],
    seed: 0x51DE_CAFE,
};

impl ShowcaseSpec {
    pub fn cell(self, x: i64, y: i32, depth: i32) -> Option<ShowcaseCell> {
        if !(self.min_x..self.min_x + i64::from(self.width)).contains(&x)
            || !(0..self.height).contains(&y)
            || !(0..i32::from(self.depth)).contains(&depth)
        {
            return None;
        }
        if depth == 0 {
            foreground_cell(x, y)
        } else {
            rear_cell(x, y, depth)
        }
    }
}

const fn block(block: BlockKind) -> Option<ShowcaseCell> {
    Some(ShowcaseCell {
        block,
        torch_mount: None,
    })
}

const fn torch(mount: ShowcaseTorchMount) -> Option<ShowcaseCell> {
    Some(ShowcaseCell {
        block: BlockKind::Torch,
        torch_mount: Some(mount),
    })
}

fn foreground_cell(x: i64, y: i32) -> Option<ShowcaseCell> {
    match (x, y) {
        (0, 3) => return torch(ShowcaseTorchMount::Floor),
        (-7, 6) => return torch(ShowcaseTorchMount::WallRight),
        (7, 6) => return torch(ShowcaseTorchMount::WallLeft),
        (-8, 4 | 5) | (-9, 5) => return block(BlockKind::CoalOre),
        (8, 5 | 6) | (9, 5) => return block(BlockKind::IronOre),
        _ => {}
    }
    let surface = foreground_surface(x);
    for tree_x in [-14, 14] {
        let tree_surface = foreground_surface(tree_x);
        if x == tree_x && (tree_surface + 1..=tree_surface + 5).contains(&y) {
            return block(BlockKind::Wood);
        }
        let crown_y = tree_surface + 6;
        let dx = (x - tree_x).unsigned_abs();
        let dy = (y - crown_y).unsigned_abs();
        if dx <= 3 && dy <= 2 && dx + u64::from(dy) <= 4 {
            return block(BlockKind::Leaves);
        }
    }
    if (-7..=7).contains(&x) && (3..=8).contains(&y) {
        return None;
    }
    terrain_cell(surface, y)
}

fn rear_cell(x: i64, y: i32, depth: i32) -> Option<ShowcaseCell> {
    let shifted = x + i64::from(depth * 3);
    let surface =
        9 + match shifted {
            value if value < -14 => 4,
            value if value < -6 => 2,
            value if value < 8 => 0,
            value if value < 16 => 2,
            _ => 4,
        } + depth % 2;
    terrain_cell(surface, y)
}

fn foreground_surface(x: i64) -> i32 {
    match x {
        value if value < -18 => 16,
        value if value < -10 => 13,
        value if value < 10 => 11,
        value if value < 18 => 13,
        _ => 16,
    }
}

fn terrain_cell(surface: i32, y: i32) -> Option<ShowcaseCell> {
    if y > surface {
        None
    } else if y == 0 {
        block(BlockKind::Bedrock)
    } else if y == surface {
        block(BlockKind::Grass)
    } else if y >= surface - 3 {
        block(BlockKind::Dirt)
    } else {
        block(BlockKind::Stone)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn showcase_is_bounded_and_contains_every_material() {
        assert!(SHOWCASE.cell(SHOWCASE.min_x - 1, 0, 0).is_none());
        assert!(SHOWCASE.cell(SHOWCASE.min_x, -1, 0).is_none());
        assert!(
            SHOWCASE
                .cell(SHOWCASE.min_x, 0, i32::from(SHOWCASE.depth))
                .is_none()
        );

        let cells = (SHOWCASE.min_x..SHOWCASE.min_x + i64::from(SHOWCASE.width))
            .flat_map(|x| {
                (0..SHOWCASE.height).flat_map(move |y| {
                    (0..i32::from(SHOWCASE.depth))
                        .filter_map(move |depth| SHOWCASE.cell(x, y, depth))
                })
            })
            .collect::<Vec<_>>();
        let blocks = cells.iter().map(|cell| cell.block).collect::<HashSet<_>>();
        for block in BlockKind::ALL {
            assert!(blocks.contains(&block), "showcase is missing {block:?}");
        }
        let mounts = cells
            .iter()
            .filter_map(|cell| cell.torch_mount)
            .collect::<HashSet<_>>();
        assert_eq!(
            mounts,
            HashSet::from([
                ShowcaseTorchMount::Floor,
                ShowcaseTorchMount::WallLeft,
                ShowcaseTorchMount::WallRight,
            ])
        );
    }

    #[test]
    fn inspection_area_fits_inside_overscan() {
        for (width, height) in [(800.0, 450.0), (1024.0, 768.0), (2560.0, 1080.0)] {
            let scale =
                (SHOWCASE.inspection_width / width).max(SHOWCASE.inspection_height / height);
            assert!(width * scale <= SHOWCASE.width as f32);
            assert!(height * scale <= SHOWCASE.height as f32);
        }
    }
}
