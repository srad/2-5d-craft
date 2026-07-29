use serde::Deserialize;
use sidecraft_textures::{
    ClusterShape, GenerateOptions, OrePattern, PackError, PalettePreset, PatternAlgorithm,
    PlacementAlgorithm, QualityPreset,
};
use std::{collections::BTreeMap, fs, path::Path, str::FromStr};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct Recipe {
    pub palette: Option<String>,
    pub pattern: Option<String>,
    pub placement: Option<String>,
    pub cluster_shape: Option<String>,
    pub cluster_size: Option<String>,
    pub cluster_density: Option<String>,
    pub smoothing_passes: Option<String>,
    pub contrast: Option<String>,
    pub saturation: Option<String>,
    pub lightness: Option<String>,
    pub variant_strength: Option<String>,
    pub ore_pattern: Option<String>,
    pub ore_coverage: Option<String>,
    pub ore_branches: Option<String>,
    pub ore_thickness: Option<String>,
    pub ore_center_bias: Option<String>,
    pub leaf_hole_density: Option<String>,
    pub grass_fringe_depth: Option<String>,
    pub quality: Option<String>,
    #[serde(default)]
    pub material: BTreeMap<String, BTreeMap<String, String>>,
}

impl Recipe {
    pub(crate) fn read(path: &Path) -> Result<Self, PackError> {
        toml::from_str(&fs::read_to_string(path)?).map_err(PackError::from)
    }

    pub(crate) fn merge(&mut self, overlay: Self) {
        macro_rules! replace {
            ($($field:ident),+ $(,)?) => {
                $(if overlay.$field.is_some() {
                    self.$field = overlay.$field;
                })+
            };
        }
        replace!(
            palette,
            pattern,
            placement,
            cluster_shape,
            cluster_size,
            cluster_density,
            smoothing_passes,
            contrast,
            saturation,
            lightness,
            variant_strength,
            ore_pattern,
            ore_coverage,
            ore_branches,
            ore_thickness,
            ore_center_bias,
            leaf_hole_density,
            grass_fringe_depth,
            quality,
        );
        for (material, fields) in overlay.material {
            self.material.entry(material).or_default().extend(fields);
        }
    }

    pub(crate) fn resolve(
        &self,
        seed: u64,
        set_overrides: &[String],
    ) -> Result<GenerateOptions, PackError> {
        let mut rng = ChoiceRng::new(seed);
        let mut options = randomized_defaults(seed, &mut rng);
        macro_rules! choose {
            ($field:ident, $kind:ident) => {
                if let Some(spec) = &self.$field {
                    options.$field = $kind(spec, &mut rng, stringify!($field))?;
                }
            };
        }
        choose!(palette, choose_enum);
        choose!(pattern, choose_enum);
        choose!(placement, choose_enum);
        choose!(cluster_shape, choose_enum);
        choose!(cluster_size, choose_usize);
        choose!(cluster_density, choose_f32);
        choose!(smoothing_passes, choose_usize);
        choose!(contrast, choose_f32);
        choose!(saturation, choose_f32);
        choose!(lightness, choose_f32);
        choose!(variant_strength, choose_i16);
        choose!(ore_pattern, choose_enum);
        choose!(ore_coverage, choose_f32);
        choose!(ore_branches, choose_usize);
        choose!(ore_thickness, choose_usize);
        choose!(ore_center_bias, choose_f32);
        choose!(leaf_hole_density, choose_f32);
        choose!(grass_fringe_depth, choose_usize);
        choose!(quality, choose_enum);
        for (material, fields) in &self.material {
            for (field, spec) in fields {
                let value = resolve_material_value(field, spec, &mut rng)?;
                options.apply_material_override(material, field, &value)?;
            }
        }
        for assignment in set_overrides {
            let (key, spec) = assignment.split_once('=').ok_or_else(|| {
                PackError::Invalid(format!("--set must use material.field=value: {assignment}"))
            })?;
            let (material, field) = key.split_once('.').ok_or_else(|| {
                PackError::Invalid(format!("--set must use material.field=value: {assignment}"))
            })?;
            let value = resolve_material_value(field, spec, &mut rng)?;
            options.apply_material_override(material, field, &value)?;
        }
        Ok(options)
    }
}

fn randomized_defaults(seed: u64, rng: &mut ChoiceRng) -> GenerateOptions {
    let palettes = [
        PalettePreset::Earthy,
        PalettePreset::DeepEarth,
        PalettePreset::Classic,
    ];
    let patterns = [
        PatternAlgorithm::ClusterStamps,
        PatternAlgorithm::CellularClumps,
        PatternAlgorithm::BrokenStrata,
        PatternAlgorithm::ShortWalks,
    ];
    let placements = [
        PlacementAlgorithm::Uniform,
        PlacementAlgorithm::JitteredGrid,
        PlacementAlgorithm::PoissonDisc,
    ];
    let shapes = [
        ClusterShape::Mixed,
        ClusterShape::Polyomino,
        ClusterShape::Rectangular,
    ];
    let ores = [
        OrePattern::CenterGrowth,
        OrePattern::BranchingWalk,
        OrePattern::CompactCellular,
    ];
    GenerateOptions {
        seed,
        palette: palettes[rng.index(palettes.len())],
        pattern: patterns[rng.index(patterns.len())],
        placement: placements[rng.index(placements.len())],
        cluster_shape: shapes[rng.index(shapes.len())],
        cluster_size: rng.range(2, 5),
        cluster_density: rng.float(0.16, 0.24),
        smoothing_passes: rng.range(0, 1),
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

fn choose_enum<T>(spec: &str, rng: &mut ChoiceRng, field: &str) -> Result<T, PackError>
where
    T: clap::ValueEnum + Clone,
{
    let choice = choose_list(spec, rng);
    T::from_str(choice, true).map_err(|_| PackError::Invalid(format!("invalid {field}: {choice}")))
}

fn choose_usize(spec: &str, rng: &mut ChoiceRng, field: &str) -> Result<usize, PackError> {
    choose_number(spec, rng, field, |left, right, rng| rng.range(left, right))
}

fn choose_i16(spec: &str, rng: &mut ChoiceRng, field: &str) -> Result<i16, PackError> {
    choose_number(spec, rng, field, |left, right, rng| {
        let span = i32::from(right) - i32::from(left);
        left + rng.index(span as usize + 1) as i16
    })
}

fn choose_f32(spec: &str, rng: &mut ChoiceRng, field: &str) -> Result<f32, PackError> {
    let choice = choose_list(spec, rng);
    if let Some((left, right)) = choice.split_once("..") {
        let left = parse_number(left, field)?;
        let right = parse_number(right, field)?;
        if right < left {
            return Err(PackError::Invalid(format!(
                "{field} range is reversed: {choice}"
            )));
        }
        Ok(rng.float(left, right))
    } else {
        parse_number(choice, field)
    }
}

fn choose_number<T>(
    spec: &str,
    rng: &mut ChoiceRng,
    field: &str,
    range: impl Fn(T, T, &mut ChoiceRng) -> T,
) -> Result<T, PackError>
where
    T: FromStr + PartialOrd + Copy,
{
    let choice = choose_list(spec, rng);
    if let Some((left, right)) = choice.split_once("..") {
        let left = parse_number(left, field)?;
        let right = parse_number(right, field)?;
        if right < left {
            return Err(PackError::Invalid(format!(
                "{field} range is reversed: {choice}"
            )));
        }
        Ok(range(left, right, rng))
    } else {
        parse_number(choice, field)
    }
}

fn parse_number<T: FromStr>(value: &str, field: &str) -> Result<T, PackError> {
    value
        .trim()
        .parse()
        .map_err(|_| PackError::Invalid(format!("invalid {field}: {value}")))
}

fn choose_list<'a>(spec: &'a str, rng: &mut ChoiceRng) -> &'a str {
    let choices = spec.split(',').map(str::trim).collect::<Vec<_>>();
    choices[rng.index(choices.len())]
}

fn resolve_material_value(
    field: &str,
    spec: &str,
    rng: &mut ChoiceRng,
) -> Result<String, PackError> {
    match field {
        "pattern" | "placement" | "cluster-shape" | "ore-pattern" => {
            Ok(choose_list(spec, rng).to_owned())
        }
        "cluster-size" | "smoothing-passes" | "ore-branches" | "ore-thickness"
        | "grass-fringe-depth" => Ok(choose_usize(spec, rng, field)?.to_string()),
        "variant-strength" => Ok(choose_i16(spec, rng, field)?.to_string()),
        "cluster-density" | "contrast" | "saturation" | "lightness" | "ore-coverage"
        | "ore-center-bias" | "leaf-hole-density" => Ok(choose_f32(spec, rng, field)?.to_string()),
        _ => Err(PackError::Invalid(format!(
            "unknown material field: {field}"
        ))),
    }
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
    fn choices_ranges_and_overrides_replay() {
        let recipe: Recipe = toml::from_str(
            r#"
pattern = "cluster-stamps,short-walks"
cluster-size = "2..5"
[material.stone]
contrast = "1.1..1.2"
"#,
        )
        .unwrap();
        let first = recipe
            .resolve(9, &["stone.pattern=broken-strata".into()])
            .unwrap();
        let second = recipe
            .resolve(9, &["stone.pattern=broken-strata".into()])
            .unwrap();
        assert_eq!(first.pattern, second.pattern);
        assert_eq!(first.cluster_size, second.cluster_size);
        assert_eq!(first.material_overrides, second.material_overrides);
    }
}
