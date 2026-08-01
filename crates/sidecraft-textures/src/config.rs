use crate::{
    BlockKind, ControlField, PackError,
    controls::{
        CLUSTER_DENSITY_LIMITS, CLUSTER_SIZE_LIMITS, CONTRAST_LIMITS, GRASS_FRINGE_DEPTH_LIMITS,
        LEAF_HOLE_DENSITY_LIMITS, LIGHTNESS_LIMITS, ORE_BRANCHES_LIMITS, ORE_CENTER_BIAS_LIMITS,
        ORE_COVERAGE_LIMITS, ORE_THICKNESS_LIMITS, SATURATION_LIMITS, SMOOTHING_PASSES_LIMITS,
        VARIANT_STRENGTH_LIMITS, snap_f32,
    },
};
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
    EvenlyVaried,
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
            ore_coverage: 0.25,
            ore_branches: 4,
            ore_thickness: 2,
            ore_center_bias: 0.88,
            leaf_hole_density: 0.22,
            grass_fringe_depth: 4,
            quality: QualityPreset::Balanced,
            material_overrides: BTreeMap::new(),
        }
    }
}

impl GenerateOptions {
    /// Snaps every decimal parameter onto its control's step grid.
    ///
    /// The generator draws continuous floats and the CLI accepts arbitrary strings, but a pack
    /// code addresses each decimal by grid index. Quantizing here makes the value space finite so
    /// a code reproduces a pack exactly rather than approximately. Integer controls have a step of
    /// 1 and are already on-grid once `validate_options` has bounded them, so call this *after*
    /// validating — it quantizes, it does not range-check.
    pub fn snap_to_control_grid(&mut self) {
        for (field, value) in [
            (ControlField::ClusterDensity, &mut self.cluster_density),
            (ControlField::Contrast, &mut self.contrast),
            (ControlField::Saturation, &mut self.saturation),
            (ControlField::Lightness, &mut self.lightness),
            (ControlField::OreCoverage, &mut self.ore_coverage),
            (ControlField::OreCenterBias, &mut self.ore_center_bias),
            (ControlField::LeafHoleDensity, &mut self.leaf_hole_density),
        ] {
            *value = snap_f32(field, *value);
        }
    }

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

pub fn validate_options(options: &GenerateOptions) -> Result<(), PackError> {
    let finite = [
        options.cluster_density,
        options.contrast,
        options.saturation,
        options.lightness,
        options.ore_coverage,
        options.ore_center_bias,
        options.leaf_hole_density,
    ]
    .into_iter()
    .all(f32::is_finite);
    if !finite {
        return Err(PackError::Invalid(
            "generator parameters must contain only finite numbers".into(),
        ));
    }
    let checks = [
        (
            (CLUSTER_SIZE_LIMITS.min..=CLUSTER_SIZE_LIMITS.max).contains(&options.cluster_size),
            "cluster-size must be in 1..=12",
        ),
        (
            (CLUSTER_DENSITY_LIMITS.min..=CLUSTER_DENSITY_LIMITS.max)
                .contains(&options.cluster_density),
            "cluster-density must be in 0.04..=0.48",
        ),
        (
            (SMOOTHING_PASSES_LIMITS.min..=SMOOTHING_PASSES_LIMITS.max)
                .contains(&options.smoothing_passes),
            "smoothing-passes must be in 0..=2",
        ),
        (
            (CONTRAST_LIMITS.min..=CONTRAST_LIMITS.max).contains(&options.contrast),
            "contrast must be in 0.65..=1.5",
        ),
        (
            (SATURATION_LIMITS.min..=SATURATION_LIMITS.max).contains(&options.saturation),
            "saturation must be in 0.5..=1.5",
        ),
        (
            (LIGHTNESS_LIMITS.min..=LIGHTNESS_LIMITS.max).contains(&options.lightness),
            "lightness must be in -0.25..=0.25",
        ),
        (
            (VARIANT_STRENGTH_LIMITS.0..=VARIANT_STRENGTH_LIMITS.1)
                .contains(&options.variant_strength),
            "variant-strength must be in 0..=16",
        ),
        (
            (ORE_COVERAGE_LIMITS.min..=ORE_COVERAGE_LIMITS.max).contains(&options.ore_coverage),
            "ore-coverage must be in 0.20..=0.35",
        ),
        (
            (ORE_BRANCHES_LIMITS.min..=ORE_BRANCHES_LIMITS.max).contains(&options.ore_branches),
            "ore-branches must be in 1..=10",
        ),
        (
            (ORE_THICKNESS_LIMITS.min..=ORE_THICKNESS_LIMITS.max).contains(&options.ore_thickness),
            "ore-thickness must be in 1..=4",
        ),
        (
            (ORE_CENTER_BIAS_LIMITS.min..=ORE_CENTER_BIAS_LIMITS.max)
                .contains(&options.ore_center_bias),
            "ore-center-bias must be in 0..=1",
        ),
        (
            (LEAF_HOLE_DENSITY_LIMITS.min..=LEAF_HOLE_DENSITY_LIMITS.max)
                .contains(&options.leaf_hole_density),
            "leaf-hole-density must be in 0..=0.25",
        ),
        (
            (GRASS_FRINGE_DEPTH_LIMITS.min..=GRASS_FRINGE_DEPTH_LIMITS.max)
                .contains(&options.grass_fringe_depth),
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
    // Overrides are parsed from free-form strings after the global snap, so quantize again once
    // they are folded in.
    resolved.snap_to_control_grid();
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
        "evenly-varied" => Ok(PatternAlgorithm::EvenlyVaried),
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
