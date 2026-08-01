use crate::{
    ClusterShape, GenerateOptions, OrePattern, PalettePreset, PatternAlgorithm, PlacementAlgorithm,
    QualityPreset,
};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn random_seed() -> u64 {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    time ^ u64::from(std::process::id()).rotate_left(17)
}

pub fn randomized_options(seed: u64) -> GenerateOptions {
    let mut rng = ChoiceRng::new(seed);
    let palettes = [
        PalettePreset::Earthy,
        PalettePreset::DeepEarth,
        PalettePreset::Classic,
    ];
    let patterns = [
        PatternAlgorithm::ClusterStamps,
        PatternAlgorithm::EvenlyVaried,
        PatternAlgorithm::CellularClumps,
        PatternAlgorithm::BrokenStrata,
        PatternAlgorithm::ShortWalks,
    ];
    let pattern = patterns[rng.index(patterns.len())];
    let (placement, cluster_shape, cluster_size, cluster_density, smoothing_passes) =
        randomized_pattern_profile(&mut rng, pattern);
    let ores = [
        OrePattern::CenterGrowth,
        OrePattern::BranchingWalk,
        OrePattern::CompactCellular,
    ];
    GenerateOptions {
        seed,
        palette: palettes[rng.index(palettes.len())],
        pattern,
        placement,
        cluster_shape,
        cluster_size,
        cluster_density,
        smoothing_passes,
        contrast: rng.float(1.02, 1.10),
        saturation: rng.float(0.95, 1.08),
        lightness: rng.float(-0.04, 0.01),
        variant_strength: rng.range(2, 5) as i16,
        ore_pattern: ores[rng.index(ores.len())],
        ore_coverage: rng.float(0.30, 0.35),
        ore_branches: rng.range(3, 6),
        ore_thickness: rng.range(2, 3),
        ore_center_bias: rng.float(0.75, 1.0),
        leaf_hole_density: rng.float(0.02, 0.06),
        grass_fringe_depth: rng.range(3, 5),
        quality: QualityPreset::Balanced,
        ..Default::default()
    }
}

fn randomized_pattern_profile(
    rng: &mut ChoiceRng,
    pattern: PatternAlgorithm,
) -> (PlacementAlgorithm, ClusterShape, usize, f32, usize) {
    match pattern {
        PatternAlgorithm::ClusterStamps => (
            pick(
                rng,
                &[
                    PlacementAlgorithm::JitteredGrid,
                    PlacementAlgorithm::PoissonDisc,
                ],
            ),
            pick(rng, &[ClusterShape::Mixed, ClusterShape::Polyomino]),
            rng.range(3, 5),
            rng.float(0.17, 0.23),
            rng.range(0, 1),
        ),
        PatternAlgorithm::EvenlyVaried => (
            PlacementAlgorithm::PoissonDisc,
            ClusterShape::Mixed,
            rng.range(1, 3),
            rng.float(0.17, 0.22),
            0,
        ),
        PatternAlgorithm::CellularClumps => (
            pick(
                rng,
                &[
                    PlacementAlgorithm::JitteredGrid,
                    PlacementAlgorithm::PoissonDisc,
                ],
            ),
            ClusterShape::Mixed,
            rng.range(3, 4),
            rng.float(0.17, 0.23),
            rng.range(0, 1),
        ),
        PatternAlgorithm::BrokenStrata => (
            PlacementAlgorithm::Uniform,
            ClusterShape::Rectangular,
            rng.range(4, 7),
            rng.float(0.14, 0.20),
            0,
        ),
        PatternAlgorithm::ShortWalks => (
            PlacementAlgorithm::Uniform,
            ClusterShape::Mixed,
            rng.range(3, 6),
            rng.float(0.15, 0.22),
            0,
        ),
    }
}

fn pick<T: Copy>(rng: &mut ChoiceRng, values: &[T]) -> T {
    values[rng.index(values.len())]
}

struct ChoiceRng(u64);

impl ChoiceRng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    fn index(&mut self, upper: usize) -> usize {
        (self.next() % upper as u64) as usize
    }

    fn range(&mut self, minimum: usize, maximum: usize) -> usize {
        minimum + self.index(maximum - minimum + 1)
    }

    fn float(&mut self, minimum: f32, maximum: f32) -> f32 {
        let unit = (self.next() >> 40) as f32 / (1_u32 << 24) as f32;
        minimum + (maximum - minimum) * unit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_random_defaults_replay_and_validate() {
        let first = randomized_options(42);
        let second = randomized_options(42);
        assert_eq!(first.material_overrides, second.material_overrides);
        assert_eq!(first.pattern, second.pattern);
        assert_eq!(first.cluster_size, second.cluster_size);
        crate::config::validate_options(&first).unwrap();
    }

    #[test]
    fn random_profiles_keep_pattern_specific_parameters_coherent() {
        let mut seen = [false; 5];
        for seed in 0..256 {
            let options = randomized_options(seed);
            crate::config::validate_options(&options).unwrap();
            match options.pattern {
                PatternAlgorithm::ClusterStamps => {
                    seen[0] = true;
                    assert!(matches!(
                        options.placement,
                        PlacementAlgorithm::JitteredGrid | PlacementAlgorithm::PoissonDisc
                    ));
                    assert!(matches!(
                        options.cluster_shape,
                        ClusterShape::Mixed | ClusterShape::Polyomino
                    ));
                    assert!((3..=5).contains(&options.cluster_size));
                    assert!((0.17..=0.23).contains(&options.cluster_density));
                }
                PatternAlgorithm::EvenlyVaried => {
                    seen[1] = true;
                    assert_eq!(options.placement, PlacementAlgorithm::PoissonDisc);
                    assert_eq!(options.cluster_shape, ClusterShape::Mixed);
                    assert!((1..=3).contains(&options.cluster_size));
                    assert!((0.17..=0.22).contains(&options.cluster_density));
                    assert_eq!(options.smoothing_passes, 0);
                }
                PatternAlgorithm::CellularClumps => {
                    seen[2] = true;
                    assert!(matches!(
                        options.placement,
                        PlacementAlgorithm::JitteredGrid | PlacementAlgorithm::PoissonDisc
                    ));
                    assert_eq!(options.cluster_shape, ClusterShape::Mixed);
                    assert!((3..=4).contains(&options.cluster_size));
                    assert!((0.17..=0.23).contains(&options.cluster_density));
                }
                PatternAlgorithm::BrokenStrata => {
                    seen[3] = true;
                    assert_eq!(options.placement, PlacementAlgorithm::Uniform);
                    assert_eq!(options.cluster_shape, ClusterShape::Rectangular);
                    assert!((4..=7).contains(&options.cluster_size));
                    assert!((0.14..=0.20).contains(&options.cluster_density));
                    assert_eq!(options.smoothing_passes, 0);
                }
                PatternAlgorithm::ShortWalks => {
                    seen[4] = true;
                    assert_eq!(options.placement, PlacementAlgorithm::Uniform);
                    assert_eq!(options.cluster_shape, ClusterShape::Mixed);
                    assert!((3..=6).contains(&options.cluster_size));
                    assert!((0.15..=0.22).contains(&options.cluster_density));
                    assert_eq!(options.smoothing_passes, 0);
                }
            }
        }
        assert!(seen.into_iter().all(|profile| profile));
    }
}
