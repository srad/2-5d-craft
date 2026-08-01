use crate::{
    BlockKind, Face, GENERATOR_VERSION, GenerateOptions, GenerationManifest, PACK_SCHEMA_VERSION,
    PackError, PackImage, PackManifest, PlayerPalette, ResolvedPack, VARIANT_COUNT,
    compose_preview,
    config::{options_for_material, resolved_values, validate_options},
    environment::generate_environment,
    material::generate_block,
    palette::player_palette,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedPack {
    pub manifest: PackManifest,
    pub generation: GenerationManifest,
    pub assets: BTreeMap<String, PackImage>,
    pub preview: PackImage,
}

pub fn generate_pack(options: &GenerateOptions) -> Result<GeneratedPack, PackError> {
    validate_options(options)?;
    // Pixels must only ever come from on-grid values, or a pack code would name a pack the
    // generator cannot reproduce. Validation runs first so this only quantizes in-range values.
    let mut options = options.clone();
    options.snap_to_control_grid();
    let options = &options;
    let mut assets = BTreeMap::new();
    for block in BlockKind::ALL {
        let material_options = options_for_material(options, block)?;
        for face in Face::ALL {
            let mut variants = Vec::with_capacity(VARIANT_COUNT);
            for variant in 0..VARIANT_COUNT {
                let mut candidate = generate_block(block, face, variant, &material_options);
                for attempt in 1..32 {
                    if !variants
                        .iter()
                        .any(|prior| transform_equivalent(prior, &candidate))
                    {
                        break;
                    }
                    candidate = generate_block(
                        block,
                        face,
                        variant + attempt * VARIANT_COUNT,
                        &material_options,
                    );
                }
                assets.insert(
                    format!("blocks/{}/{}_{}.png", block.slug(), face.slug(), variant),
                    candidate.clone(),
                );
                variants.push(candidate);
            }
        }
    }
    for block in BlockKind::HOTBAR {
        let source = assets[&format!("blocks/{}/side_0.png", block.slug())].clone();
        assets.insert(format!("icons/{}.png", block.slug()), source);
    }
    generate_environment(options.seed, &mut assets);

    let manifest = PackManifest {
        schema_version: PACK_SCHEMA_VERSION,
        id: match options.id.clone() {
            Some(id) => id,
            // Falling back to the pack code keeps a CLI-generated pack identified by what it
            // actually contains, exactly as an editor-exported one is.
            None => crate::TextureProject::from_generate_options(options)?.pack_id()?,
        },
        name: options
            .name
            .clone()
            .unwrap_or_else(|| format!("Generated {}", options.seed)),
        author: options.author.clone(),
        description: "Deterministic procedural Sidecraft texture pack.".into(),
        player: player_palette(options.palette),
    };
    let generation = GenerationManifest {
        generator_version: GENERATOR_VERSION,
        seed: options.seed,
        resolved: resolved_values(options),
    };
    let mut pack = GeneratedPack {
        manifest,
        generation,
        assets,
        preview: PackImage::solid(320, 180, [0, 0, 0, 255]),
    };
    pack.preview = compose_preview(&resolve_generated_pack(&pack)?);
    Ok(pack)
}

pub fn resolve_generated_pack(pack: &GeneratedPack) -> Result<ResolvedPack, PackError> {
    let mut blocks = BTreeMap::new();
    for block in BlockKind::ALL {
        for face in Face::ALL {
            for variant in 0..VARIANT_COUNT {
                let path = format!("blocks/{}/{}_{}.png", block.slug(), face.slug(), variant);
                blocks.insert(
                    (block, face, variant),
                    pack.assets.get(&path).cloned().ok_or_else(|| {
                        PackError::Invalid(format!("generated pack is missing {path}"))
                    })?,
                );
            }
        }
    }
    let mut icons = BTreeMap::new();
    for block in BlockKind::HOTBAR {
        let path = format!("icons/{}.png", block.slug());
        icons.insert(
            block,
            pack.assets
                .get(&path)
                .cloned()
                .unwrap_or_else(|| blocks[&(block, Face::Side, 0)].clone()),
        );
    }
    let required = |path: &str| {
        pack.assets
            .get(path)
            .cloned()
            .ok_or_else(|| PackError::Invalid(format!("generated pack is missing {path}")))
    };
    Ok(ResolvedPack {
        manifest: pack.manifest.clone(),
        player: PlayerPalette::from_complete(&pack.manifest.player)?,
        blocks,
        icons,
        sun: required("environment/sun.png")?,
        moons: (0..8)
            .map(|phase| required(&format!("environment/moon_{phase}.png")))
            .collect::<Result<Vec<_>, _>>()?,
        stars: required("environment/stars.png")?,
        cloud: required("environment/cloud.png")?,
        preview: pack.preview.clone(),
    })
}

fn transform_equivalent(first: &PackImage, second: &PackImage) -> bool {
    if first.width != second.width || first.height != second.height || first.width != first.height {
        return false;
    }
    if color_histogram(first) != color_histogram(second) {
        return false;
    }
    let side = first.width as i32;
    for transform in 0..8 {
        for shift_y in 0..side {
            for shift_x in 0..side {
                let equal = (0..side).all(|y| {
                    (0..side).all(|x| {
                        let (mut source_x, source_y) = match transform % 4 {
                            0 => (x, y),
                            1 => (side - 1 - y, x),
                            2 => (side - 1 - x, side - 1 - y),
                            _ => (y, side - 1 - x),
                        };
                        if transform >= 4 {
                            source_x = side - 1 - source_x;
                        }
                        first.pixel(x as u32, y as u32)
                            == second.pixel(
                                (source_x + shift_x).rem_euclid(side) as u32,
                                (source_y + shift_y).rem_euclid(side) as u32,
                            )
                    })
                });
                if equal {
                    return true;
                }
            }
        }
    }
    false
}

fn color_histogram(image: &PackImage) -> BTreeMap<[u8; 4], usize> {
    let mut histogram = BTreeMap::new();
    for pixel in image.pixels.chunks_exact(4) {
        *histogram
            .entry([pixel[0], pixel[1], pixel[2], pixel[3]])
            .or_insert(0) += 1;
    }
    histogram
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PatternAlgorithm, load_resolved_pack, write_generated_pack};

    #[test]
    fn fixed_seed_replays_every_algorithm() {
        for pattern in [
            PatternAlgorithm::ClusterStamps,
            PatternAlgorithm::EvenlyVaried,
            PatternAlgorithm::CellularClumps,
            PatternAlgorithm::BrokenStrata,
            PatternAlgorithm::ShortWalks,
        ] {
            let options = GenerateOptions {
                seed: 91,
                pattern,
                ..Default::default()
            };
            assert_eq!(
                generate_pack(&options).unwrap().assets,
                generate_pack(&options).unwrap().assets
            );
        }
    }

    #[test]
    fn generated_contract_is_complete() {
        let pack = generate_pack(&GenerateOptions::default()).unwrap();
        assert_eq!(
            pack.assets
                .keys()
                .filter(|path| path.starts_with("blocks/"))
                .count(),
            BlockKind::ALL.len() * Face::ALL.len() * VARIANT_COUNT
        );
        assert_eq!(pack.preview.width, 320);
        assert_eq!(pack.assets["icons/dirt.png"].width, crate::BLOCK_SIZE);
    }

    #[test]
    fn in_memory_resolution_matches_written_pack_loading() {
        let directory = tempfile::tempdir().unwrap();
        let generated = generate_pack(&GenerateOptions {
            seed: 81,
            ..Default::default()
        })
        .unwrap();
        let in_memory = resolve_generated_pack(&generated).unwrap();
        let path = write_generated_pack(&generated, directory.path()).unwrap();
        let from_disk = load_resolved_pack(&path, None).unwrap();
        assert_eq!(in_memory, from_disk);
    }

    #[test]
    fn variants_reject_translation_reflection_and_transpose_equivalence() {
        let pack = generate_pack(&GenerateOptions {
            seed: 27,
            variant_strength: 0,
            ..Default::default()
        })
        .unwrap();
        let variants = (0..VARIANT_COUNT)
            .map(|variant| &pack.assets[&format!("blocks/stone/side_{variant}.png")])
            .collect::<Vec<_>>();
        for left in 0..variants.len() {
            for right in left + 1..variants.len() {
                assert!(!transform_equivalent(variants[left], variants[right]));
            }
        }
    }
}
