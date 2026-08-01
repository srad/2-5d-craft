use crate::{
    BlockKind, Face, GenerateOptions, PackImage,
    algorithm::{PIXELS, best_pattern, leaf_hole_mask, ore_mask},
    palette::{adjusted_palette, base_palette, material_palette},
    rng::{FixedRng, mix_seed},
};

pub(crate) fn generate_block(
    block: BlockKind,
    face: Face,
    variant: usize,
    options: &GenerateOptions,
) -> PackImage {
    match block {
        BlockKind::Grass => grass_image(face, variant, options),
        BlockKind::CoalOre | BlockKind::IronOre => ore_image(block, face, variant, options),
        BlockKind::Wood => wood_image(face, variant, options),
        BlockKind::Leaves => leaves_image(face, variant, options),
        BlockKind::Torch => torch_image(face, variant, options),
        BlockKind::Dirt | BlockKind::Stone | BlockKind::Bedrock => {
            patterned_image(block, face, variant, options)
        }
    }
}

fn patterned_image(
    block: BlockKind,
    face: Face,
    variant: usize,
    options: &GenerateOptions,
) -> PackImage {
    let label = format!("{}-{}", block.slug(), face.slug());
    let mut rng = FixedRng::new(mix_seed(options.seed, &label, variant as u64));
    let indices = best_pattern(&mut rng, options);
    indices_to_image(
        &indices,
        material_palette(block, options),
        &mut rng,
        options.variant_strength,
    )
}

fn grass_image(face: Face, variant: usize, options: &GenerateOptions) -> PackImage {
    if face == Face::Bottom {
        return patterned_image(BlockKind::Dirt, face, variant, options);
    }
    let label = format!("grass-{}", face.slug());
    let mut rng = FixedRng::new(mix_seed(options.seed, &label, variant as u64));
    if face == Face::Top {
        let indices = best_pattern(&mut rng, options);
        return indices_to_image(
            &indices,
            material_palette(BlockKind::Grass, options),
            &mut rng,
            options.variant_strength,
        );
    }
    let dirt_indices = best_pattern(&mut rng, options);
    let mut image = indices_to_image(
        &dirt_indices,
        material_palette(BlockKind::Dirt, options),
        &mut rng,
        options.variant_strength,
    );
    let grass = adjusted_palette(base_palette(BlockKind::Grass, options.palette), options);
    let grass_indices = best_pattern(&mut rng, options);
    for x in 0..16 {
        let depth = options.grass_fringe_depth.saturating_sub(rng.index(3)) as u32;
        for y in 0..depth {
            let color = grass[grass_indices[(y * 16 + x) as usize] as usize];
            image.set_pixel(x, y, [color[0], color[1], color[2], 255]);
        }
    }
    image
}

fn ore_image(block: BlockKind, face: Face, variant: usize, options: &GenerateOptions) -> PackImage {
    let mut base = patterned_image(BlockKind::Stone, face, variant, options);
    let label = format!("{}-ore-{}", block.slug(), face.slug());
    let mut rng = FixedRng::new(mix_seed(options.seed, &label, variant as u64));
    let mask = ore_mask(&mut rng, options);
    let ore = material_palette(block, options);
    for (index, occupied) in mask.into_iter().enumerate() {
        if occupied {
            let color = ore[ore_role(&mask, index)];
            base.pixels[index * 4..index * 4 + 4]
                .copy_from_slice(&[color[0], color[1], color[2], 255]);
        }
    }
    base
}

fn wood_image(face: Face, variant: usize, options: &GenerateOptions) -> PackImage {
    let colors = material_palette(BlockKind::Wood, options);
    let mut rng = FixedRng::new(mix_seed(
        options.seed,
        &format!("wood-{}", face.slug()),
        variant as u64,
    ));
    let mut image = PackImage::solid(16, 16, [colors[0][0], colors[0][1], colors[0][2], 255]);
    if face == Face::Top || face == Face::Bottom {
        for y in 0..16_i32 {
            for x in 0..16_i32 {
                let distance = (x - 7).abs().max((y - 7).abs());
                if matches!(distance, 2 | 5 | 7) && rng.chance(0.78) {
                    let color = colors[2];
                    image.set_pixel(x as u32, y as u32, [color[0], color[1], color[2], 255]);
                }
            }
        }
    } else {
        for x in [2_u32, 6, 11, 14] {
            let offset = rng.index(3) as u32;
            for y in 0..16 {
                if !(y + offset).is_multiple_of(5) {
                    let color = colors[1 + ((y as usize + variant) % 2)];
                    image.set_pixel(x, y, [color[0], color[1], color[2], 255]);
                }
            }
        }
    }
    image
}

fn leaves_image(face: Face, variant: usize, options: &GenerateOptions) -> PackImage {
    let mut image = patterned_image(BlockKind::Leaves, face, variant, options);
    let mut rng = FixedRng::new(mix_seed(
        options.seed,
        &format!("leaf-holes-{}", face.slug()),
        variant as u64,
    ));
    for (index, hole) in leaf_hole_mask(&mut rng, options.leaf_hole_density)
        .into_iter()
        .enumerate()
    {
        if hole {
            image.set_pixel((index % 16) as u32, (index / 16) as u32, [0, 0, 0, 0]);
        }
    }
    image
}

fn ore_role(mask: &[bool; PIXELS], index: usize) -> usize {
    let x = (index % 16) as i32;
    let y = (index / 16) as i32;
    let occupied = |x: i32, y: i32| mask[(y.rem_euclid(16) * 16 + x.rem_euclid(16)) as usize];
    let light_edges = usize::from(!occupied(x - 1, y)) + usize::from(!occupied(x, y - 1));
    let dark_edges = usize::from(!occupied(x + 1, y)) + usize::from(!occupied(x, y + 1));
    if light_edges > dark_edges {
        2
    } else if dark_edges > light_edges {
        3
    } else {
        1
    }
}

fn torch_image(face: Face, variant: usize, options: &GenerateOptions) -> PackImage {
    let mut image = PackImage::solid(16, 16, [0, 0, 0, 0]);
    let mut rng = FixedRng::new(mix_seed(
        options.seed,
        &format!("torch-{}", face.slug()),
        variant as u64,
    ));
    let shaft = material_palette(BlockKind::Torch, options);
    for y in 4..15 {
        for x in 6..10 {
            let color = shaft[((x + y + variant as u32) % 2) as usize + 1];
            image.set_pixel(x, y, [color[0], color[1], color[2], 255]);
        }
    }
    let flame = [[255, 205, 62], [242, 126, 25], [177, 55, 16]];
    for y in 1..5 {
        let inset = u32::from(y == 1);
        for x in 5 + inset..11 - inset {
            let color = flame[(y as usize + x as usize + rng.index(3)) % 3];
            image.set_pixel(x, y, [color[0], color[1], color[2], 255]);
        }
    }
    image
}

fn indices_to_image(
    indices: &[u8; PIXELS],
    mut colors: [[u8; 3]; 4],
    rng: &mut FixedRng,
    variation: i16,
) -> PackImage {
    for color in &mut colors {
        for channel in color {
            *channel = (i16::from(*channel) + rng.signed(variation)).clamp(0, 255) as u8;
        }
    }
    let mut pixels = Vec::with_capacity(PIXELS * 4);
    for &index in indices {
        let base = colors[index.min(3) as usize];
        pixels.extend_from_slice(&base);
        pixels.push(255);
    }
    PackImage::new(16, 16, pixels).expect("fixed-size generated image")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algorithm::CARDINALS;
    use std::collections::HashSet;

    #[test]
    fn variants_are_distinct() {
        let options = GenerateOptions {
            seed: 27,
            variant_strength: 0,
            ..Default::default()
        };
        let variants = (0..4)
            .map(|variant| generate_block(BlockKind::Stone, Face::Side, variant, &options).pixels)
            .collect::<HashSet<_>>();
        assert_eq!(variants.len(), 4);
    }

    #[test]
    fn ores_use_clustered_multitone_masks_over_matching_stone() {
        let options = GenerateOptions {
            seed: 14,
            ..Default::default()
        };
        let stone = generate_block(BlockKind::Stone, Face::Side, 0, &options);
        let ore = generate_block(BlockKind::IronOre, Face::Side, 0, &options);
        let changed = (0..PIXELS)
            .filter(|&index| {
                stone.pixels[index * 4..index * 4 + 3] != ore.pixels[index * 4..index * 4 + 3]
            })
            .collect::<HashSet<_>>();
        assert!((56..=72).contains(&changed.len()));
        let ore_colors = changed
            .iter()
            .map(|index| ore.pixels[index * 4..index * 4 + 3].to_vec())
            .collect::<HashSet<_>>();
        assert!(ore_colors.len() >= 3);

        let mut remaining = changed.clone();
        let mut components = Vec::new();
        while let Some(&start) = remaining.iter().next() {
            remaining.remove(&start);
            let mut pending = vec![start];
            let mut size = 0;
            while let Some(index) = pending.pop() {
                size += 1;
                let x = (index % 16) as i32;
                let y = (index / 16) as i32;
                for (dx, dy) in CARDINALS {
                    let next = ((y + dy).rem_euclid(16) * 16 + (x + dx).rem_euclid(16)) as usize;
                    if remaining.remove(&next) {
                        pending.push(next);
                    }
                }
            }
            components.push(size);
        }
        assert!((6..=12).contains(&components.len()), "{components:?}");
        assert!(components.iter().all(|size| (2..=16).contains(size)));
    }

    #[test]
    fn grass_side_has_a_cap_and_dirt_body() {
        let image = grass_image(Face::Side, 0, &GenerateOptions::default());
        assert_ne!(image.pixel(4, 1), image.pixel(4, 12));
        assert_eq!(image.pixel(4, 12)[3], 255);
    }

    #[test]
    fn leaves_and_torches_use_transparency_only_where_intended() {
        let options = GenerateOptions::default();
        let leaves = generate_block(BlockKind::Leaves, Face::Side, 0, &options);
        assert_eq!(
            leaves
                .pixels
                .chunks_exact(4)
                .filter(|pixel| pixel[3] == 0)
                .count(),
            56
        );
        assert!(
            generate_block(BlockKind::Torch, Face::Side, 0, &options)
                .pixels
                .chunks_exact(4)
                .any(|pixel| pixel[3] == 0)
        );
    }
}
