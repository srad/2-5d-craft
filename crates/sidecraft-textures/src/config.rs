use crate::{BlockKind, PackError};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PalettePreset {
    #[default]
    Earthy,
    DeepEarth,
    Classic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PatternAlgorithm {
    #[default]
    ClusterStamps,
    CellularClumps,
    BrokenStrata,
    ShortWalks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PlacementAlgorithm {
    Uniform,
    #[default]
    JitteredGrid,
    PoissonDisc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ClusterShape {
    #[default]
    Mixed,
    Polyomino,
    Rectangular,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "kebab-case")]
pub enum OrePattern {
    #[default]
    CenterGrowth,
    BranchingWalk,
    CompactCellular,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "kebab-case")]
pub enum QualityPreset {
    Relaxed,
    #[default]
    Balanced,
    Strict,
}

#[derive(Debug, Clone)]
pub struct GenerateOptions {
    pub seed: u64,
    pub id: Option<String>,
    pub name: Option<String>,
    pub author: String,
    pub palette: PalettePreset,
    pub pattern: PatternAlgorithm,
    pub placement: PlacementAlgorithm,
    pub cluster_shape: ClusterShape,
    pub cluster_size: usize,
    pub cluster_density: f32,
    pub smoothing_passes: usize,
    pub contrast: f32,
    pub saturation: f32,
    pub lightness: f32,
    pub variant_strength: i16,
    pub ore_pattern: OrePattern,
    pub ore_coverage: f32,
    pub ore_branches: usize,
    pub ore_thickness: usize,
    pub ore_center_bias: f32,
    pub leaf_hole_density: f32,
    pub grass_fringe_depth: usize,
    pub quality: QualityPreset,
    pub material_overrides: BTreeMap<String, BTreeMap<String, String>>,
}

impl Default for GenerateOptions {
    fn default() -> Self {
        Self {
            seed: 0,
            id: None,
            name: None,
            author: "Sidecraft texture generator".into(),
            palette: PalettePreset::Earthy,
            pattern: PatternAlgorithm::ClusterStamps,
            placement: PlacementAlgorithm::JitteredGrid,
            cluster_shape: ClusterShape::Mixed,
            cluster_size: 4,
            cluster_density: 0.20,
            smoothing_passes: 1,
            contrast: 1.06,
            saturation: 1.0,
            lightness: -0.02,
            variant_strength: 4,
            ore_pattern: OrePattern::CenterGrowth,
            ore_coverage: 0.32,
            ore_branches: 4,
            ore_thickness: 2,
            ore_center_bias: 0.88,
            leaf_hole_density: 0.04,
            grass_fringe_depth: 4,
            quality: QualityPreset::Balanced,
            material_overrides: BTreeMap::new(),
        }
    }
}

impl GenerateOptions {
    pub fn apply_material_override(
        &mut self,
        material: &str,
        field: &str,
        value: &str,
    ) -> Result<(), PackError> {
        if !BlockKind::ALL.iter().any(|block| block.slug() == material) {
            return Err(PackError::Invalid(format!(
                "unknown material in --set: {material}"
            )));
        }
        if !MATERIAL_FIELDS.contains(&field) {
            return Err(PackError::Invalid(format!(
                "unknown material field in --set: {field}"
            )));
        }
        self.material_overrides
            .entry(material.to_owned())
            .or_default()
            .insert(field.to_owned(), value.to_owned());
        Ok(())
    }
}

const MATERIAL_FIELDS: &[&str] = &[
    "pattern",
    "placement",
    "cluster-shape",
    "cluster-size",
    "cluster-density",
    "smoothing-passes",
    "contrast",
    "saturation",
    "lightness",
    "variant-strength",
    "ore-pattern",
    "ore-coverage",
    "ore-branches",
    "ore-thickness",
    "ore-center-bias",
    "leaf-hole-density",
    "grass-fringe-depth",
];

pub(crate) fn validate_options(options: &GenerateOptions) -> Result<(), PackError> {
    let checks = [
        (
            (1..=12).contains(&options.cluster_size),
            "cluster-size must be in 1..=12",
        ),
        (
            (0.04..=0.48).contains(&options.cluster_density),
            "cluster-density must be in 0.04..=0.48",
        ),
        (
            options.smoothing_passes <= 2,
            "smoothing-passes must be in 0..=2",
        ),
        (
            (0.65..=1.5).contains(&options.contrast),
            "contrast must be in 0.65..=1.5",
        ),
        (
            (0.5..=1.5).contains(&options.saturation),
            "saturation must be in 0.5..=1.5",
        ),
        (
            (-0.25..=0.25).contains(&options.lightness),
            "lightness must be in -0.25..=0.25",
        ),
        (
            (0..=16).contains(&options.variant_strength),
            "variant-strength must be in 0..=16",
        ),
        (
            (0.30..=0.35).contains(&options.ore_coverage),
            "ore-coverage must be in 0.30..=0.35",
        ),
        (
            (1..=10).contains(&options.ore_branches),
            "ore-branches must be in 1..=10",
        ),
        (
            (1..=4).contains(&options.ore_thickness),
            "ore-thickness must be in 1..=4",
        ),
        (
            (0.0..=1.0).contains(&options.ore_center_bias),
            "ore-center-bias must be in 0..=1",
        ),
        (
            (0.0..=0.2).contains(&options.leaf_hole_density),
            "leaf-hole-density must be in 0..=0.2",
        ),
        (
            (1..=7).contains(&options.grass_fringe_depth),
            "grass-fringe-depth must be in 1..=7",
        ),
    ];
    if let Some((_, message)) = checks.into_iter().find(|(valid, _)| !valid) {
        return Err(PackError::Invalid(message.into()));
    }
    Ok(())
}

pub(crate) fn options_for_material(
    global: &GenerateOptions,
    block: BlockKind,
) -> Result<GenerateOptions, PackError> {
    let mut resolved = global.clone();
    let Some(overrides) = global.material_overrides.get(block.slug()) else {
        return Ok(resolved);
    };
    for (field, value) in overrides {
        match field.as_str() {
            "pattern" => resolved.pattern = parse_pattern(value)?,
            "placement" => resolved.placement = parse_placement(value)?,
            "cluster-shape" => resolved.cluster_shape = parse_shape(value)?,
            "cluster-size" => resolved.cluster_size = parse(value, field)?,
            "cluster-density" => resolved.cluster_density = parse(value, field)?,
            "smoothing-passes" => resolved.smoothing_passes = parse(value, field)?,
            "contrast" => resolved.contrast = parse(value, field)?,
            "saturation" => resolved.saturation = parse(value, field)?,
            "lightness" => resolved.lightness = parse(value, field)?,
            "variant-strength" => resolved.variant_strength = parse(value, field)?,
            "ore-pattern" => resolved.ore_pattern = parse_ore(value)?,
            "ore-coverage" => resolved.ore_coverage = parse(value, field)?,
            "ore-branches" => resolved.ore_branches = parse(value, field)?,
            "ore-thickness" => resolved.ore_thickness = parse(value, field)?,
            "ore-center-bias" => resolved.ore_center_bias = parse(value, field)?,
            "leaf-hole-density" => resolved.leaf_hole_density = parse(value, field)?,
            "grass-fringe-depth" => resolved.grass_fringe_depth = parse(value, field)?,
            _ => unreachable!("override field was validated"),
        }
    }
    validate_options(&resolved)?;
    Ok(resolved)
}

fn parse<T: std::str::FromStr>(value: &str, field: &str) -> Result<T, PackError> {
    value
        .parse()
        .map_err(|_| PackError::Invalid(format!("invalid {field} value: {value}")))
}

fn parse_pattern(value: &str) -> Result<PatternAlgorithm, PackError> {
    match value {
        "cluster-stamps" => Ok(PatternAlgorithm::ClusterStamps),
        "cellular-clumps" => Ok(PatternAlgorithm::CellularClumps),
        "broken-strata" => Ok(PatternAlgorithm::BrokenStrata),
        "short-walks" => Ok(PatternAlgorithm::ShortWalks),
        _ => Err(PackError::Invalid(format!("invalid pattern: {value}"))),
    }
}

fn parse_placement(value: &str) -> Result<PlacementAlgorithm, PackError> {
    match value {
        "uniform" => Ok(PlacementAlgorithm::Uniform),
        "jittered-grid" => Ok(PlacementAlgorithm::JitteredGrid),
        "poisson-disc" => Ok(PlacementAlgorithm::PoissonDisc),
        _ => Err(PackError::Invalid(format!("invalid placement: {value}"))),
    }
}

fn parse_shape(value: &str) -> Result<ClusterShape, PackError> {
    match value {
        "mixed" => Ok(ClusterShape::Mixed),
        "polyomino" => Ok(ClusterShape::Polyomino),
        "rectangular" => Ok(ClusterShape::Rectangular),
        _ => Err(PackError::Invalid(format!(
            "invalid cluster-shape: {value}"
        ))),
    }
}

fn parse_ore(value: &str) -> Result<OrePattern, PackError> {
    match value {
        "center-growth" => Ok(OrePattern::CenterGrowth),
        "branching-walk" => Ok(OrePattern::BranchingWalk),
        "compact-cellular" => Ok(OrePattern::CompactCellular),
        _ => Err(PackError::Invalid(format!("invalid ore-pattern: {value}"))),
    }
}

pub(crate) fn resolved_values(options: &GenerateOptions) -> BTreeMap<String, String> {
    let mut values = BTreeMap::from([
        ("palette".into(), format!("{:?}", options.palette)),
        ("pattern".into(), format!("{:?}", options.pattern)),
        ("placement".into(), format!("{:?}", options.placement)),
        (
            "cluster-shape".into(),
            format!("{:?}", options.cluster_shape),
        ),
        ("cluster-size".into(), options.cluster_size.to_string()),
        (
            "cluster-density".into(),
            options.cluster_density.to_string(),
        ),
        (
            "smoothing-passes".into(),
            options.smoothing_passes.to_string(),
        ),
        ("contrast".into(), options.contrast.to_string()),
        ("saturation".into(), options.saturation.to_string()),
        ("lightness".into(), options.lightness.to_string()),
        (
            "variant-strength".into(),
            options.variant_strength.to_string(),
        ),
        ("ore-pattern".into(), format!("{:?}", options.ore_pattern)),
        ("ore-coverage".into(), options.ore_coverage.to_string()),
        ("ore-branches".into(), options.ore_branches.to_string()),
        ("ore-thickness".into(), options.ore_thickness.to_string()),
        (
            "ore-center-bias".into(),
            options.ore_center_bias.to_string(),
        ),
        (
            "leaf-hole-density".into(),
            options.leaf_hole_density.to_string(),
        ),
        (
            "grass-fringe-depth".into(),
            options.grass_fringe_depth.to_string(),
        ),
        ("quality".into(), format!("{:?}", options.quality)),
    ]);
    for (material, fields) in &options.material_overrides {
        for (field, value) in fields {
            values.insert(format!("{material}.{field}"), value.clone());
        }
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_keys_and_bounds_are_validated() {
        let mut options = GenerateOptions::default();
        assert!(
            options
                .apply_material_override("stone", "pattern", "short-walks")
                .is_ok()
        );
        assert!(
            options
                .apply_material_override("unknown", "pattern", "short-walks")
                .is_err()
        );
        options.ore_coverage = 0.5;
        assert!(validate_options(&options).is_err());
    }
}
