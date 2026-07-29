use crate::{BlockKind, GenerateOptions, PalettePreset, pack::PartialPlayerPalette};

pub(crate) fn material_palette(block: BlockKind, options: &GenerateOptions) -> [[u8; 3]; 4] {
    adjusted_palette(base_palette(block, options.palette), options)
}

pub(crate) fn base_palette(block: BlockKind, preset: PalettePreset) -> [[u8; 3]; 4] {
    let mut palette = match block {
        BlockKind::Grass => [[42, 72, 25], [55, 91, 29], [70, 105, 34], [33, 59, 22]],
        BlockKind::Dirt => [[73, 45, 27], [91, 55, 30], [107, 65, 34], [58, 36, 23]],
        BlockKind::Stone | BlockKind::CoalOre | BlockKind::IronOre => {
            [[73, 74, 70], [88, 87, 79], [103, 100, 89], [57, 59, 57]]
        }
        BlockKind::Wood | BlockKind::Torch => {
            [[79, 46, 23], [102, 58, 27], [126, 75, 34], [59, 34, 20]]
        }
        BlockKind::Leaves => [[33, 67, 30], [43, 83, 34], [56, 98, 39], [25, 53, 26]],
        BlockKind::Bedrock => [[42, 43, 42], [57, 58, 55], [72, 70, 65], [29, 31, 31]],
    };
    if block == BlockKind::CoalOre {
        palette = [[28, 29, 29], [38, 39, 38], [51, 50, 46], [19, 20, 21]];
    } else if block == BlockKind::IronOre {
        palette = [[130, 68, 39], [159, 84, 47], [186, 106, 61], [99, 51, 34]];
    }
    match preset {
        PalettePreset::Earthy => palette,
        PalettePreset::DeepEarth => palette.map(|color| {
            [
                color[0].saturating_sub(12),
                color[1].saturating_sub(10),
                color[2].saturating_sub(8),
            ]
        }),
        PalettePreset::Classic => palette.map(|color| {
            [
                color[0].saturating_add(5),
                color[1].saturating_add(3),
                color[2],
            ]
        }),
    }
}

pub(crate) fn adjusted_palette(
    mut palette: [[u8; 3]; 4],
    options: &GenerateOptions,
) -> [[u8; 3]; 4] {
    for color in &mut palette {
        let gray = color[0] as f32 * 0.30 + color[1] as f32 * 0.59 + color[2] as f32 * 0.11;
        for channel in color {
            let saturated = gray + (*channel as f32 - gray) * options.saturation;
            let contrasted = 128.0 + (saturated - 128.0) * options.contrast;
            *channel = (contrasted + options.lightness * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8;
        }
    }
    palette
}

pub(crate) fn player_palette(preset: PalettePreset) -> PartialPlayerPalette {
    let palette = match preset {
        PalettePreset::Earthy => [
            [174, 111, 62],
            [25, 91, 105],
            [31, 40, 57],
            [55, 29, 16],
            [27, 22, 17],
        ],
        PalettePreset::DeepEarth => [
            [155, 94, 51],
            [18, 73, 81],
            [24, 30, 43],
            [43, 22, 13],
            [21, 17, 14],
        ],
        PalettePreset::Classic => [
            [184, 119, 68],
            [31, 102, 112],
            [36, 46, 65],
            [62, 33, 18],
            [30, 24, 18],
        ],
    };
    PartialPlayerPalette {
        skin: Some(palette[0]),
        shirt: Some(palette[1]),
        pants: Some(palette[2]),
        hair: Some(palette[3]),
        details: Some(palette[4]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palettes_remain_dark_and_material_specific() {
        let options = GenerateOptions::default();
        let dirt = material_palette(BlockKind::Dirt, &options);
        let grass = material_palette(BlockKind::Grass, &options);
        assert_ne!(dirt, grass);
        assert!(dirt.iter().flatten().copied().max().unwrap() < 180);
    }
}
