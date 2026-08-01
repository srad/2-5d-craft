use crate::{
    BlockKind, ClusterShape, GENERATOR_VERSION, GenerateOptions, MaterialField, OrePattern,
    PackError, PalettePreset, PatternAlgorithm, PlacementAlgorithm, QualityPreset,
    config::validate_options, controls::material_fields, pack::validate_metadata,
    randomized_options,
};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PROJECT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TextureProject {
    pub project_schema_version: u32,
    pub generator_version: u32,
    pub seed: u64,
    pub pack: PackSettings,
    pub parameters: TextureParameters,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub material: BTreeMap<String, TypedMaterialOverrides>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct PackSettings {
    pub id: String,
    pub name: String,
    pub author: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TextureParameters {
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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TypedMaterialOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<PatternAlgorithm>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement: Option<PlacementAlgorithm>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_shape: Option<ClusterShape>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_size: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_density: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoothing_passes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contrast: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saturation: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lightness: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant_strength: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ore_pattern: Option<OrePattern>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ore_coverage: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ore_branches: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ore_thickness: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ore_center_bias: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leaf_hole_density: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grass_fringe_depth: Option<usize>,
}

impl TextureProject {
    pub fn randomized(seed: u64) -> Self {
        let mut options = randomized_options(seed);
        options.id = Some(format!("generated-{seed}"));
        options.name = Some(format!("Generated {seed}"));
        Self::from_generate_options(&options).expect("safe randomized options form a project")
    }

    pub fn validate(&self) -> Result<(), PackError> {
        if self.project_schema_version != PROJECT_SCHEMA_VERSION {
            return Err(PackError::Invalid(format!(
                "unsupported project schema {}, expected {PROJECT_SCHEMA_VERSION}",
                self.project_schema_version
            )));
        }
        if self.generator_version != GENERATOR_VERSION {
            return Err(PackError::Invalid(format!(
                "unsupported generator version {}, expected {GENERATOR_VERSION}",
                self.generator_version
            )));
        }
        validate_metadata(&self.pack.id, &self.pack.name, &self.pack.author)?;
        self.to_generate_options().map(|_| ())
    }

    pub fn to_generate_options(&self) -> Result<GenerateOptions, PackError> {
        let mut options = self.parameters.to_options(self.seed);
        options.id = Some(self.pack.id.clone());
        options.name = Some(self.pack.name.clone());
        options.author = self.pack.author.clone();
        for (material, overrides) in &self.material {
            let block = BlockKind::ALL
                .into_iter()
                .find(|block| block.slug() == material)
                .ok_or_else(|| PackError::Invalid(format!("unknown material: {material}")))?;
            overrides.apply(block, &mut options)?;
        }
        validate_options(&options)?;
        Ok(options)
    }

    pub fn from_generate_options(options: &GenerateOptions) -> Result<Self, PackError> {
        validate_options(options)?;
        let mut material = BTreeMap::new();
        for (name, fields) in &options.material_overrides {
            let block = BlockKind::ALL
                .into_iter()
                .find(|block| block.slug() == name)
                .ok_or_else(|| PackError::Invalid(format!("unknown material: {name}")))?;
            let overrides = TypedMaterialOverrides::from_fields(fields)?;
            overrides.validate_applicability(block)?;
            if !overrides.is_empty() {
                material.insert(name.clone(), overrides);
            }
        }
        let project = Self {
            project_schema_version: PROJECT_SCHEMA_VERSION,
            generator_version: GENERATOR_VERSION,
            seed: options.seed,
            pack: PackSettings {
                id: options
                    .id
                    .clone()
                    .unwrap_or_else(|| format!("generated-{}", options.seed)),
                name: options
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("Generated {}", options.seed)),
                author: options.author.clone(),
            },
            parameters: TextureParameters::from_options(options),
            material,
        };
        validate_metadata(&project.pack.id, &project.pack.name, &project.pack.author)?;
        Ok(project)
    }
}

impl TextureParameters {
    fn from_options(options: &GenerateOptions) -> Self {
        Self {
            palette: options.palette,
            pattern: options.pattern,
            placement: options.placement,
            cluster_shape: options.cluster_shape,
            cluster_size: options.cluster_size,
            cluster_density: options.cluster_density,
            smoothing_passes: options.smoothing_passes,
            contrast: options.contrast,
            saturation: options.saturation,
            lightness: options.lightness,
            variant_strength: options.variant_strength,
            ore_pattern: options.ore_pattern,
            ore_coverage: options.ore_coverage,
            ore_branches: options.ore_branches,
            ore_thickness: options.ore_thickness,
            ore_center_bias: options.ore_center_bias,
            leaf_hole_density: options.leaf_hole_density,
            grass_fringe_depth: options.grass_fringe_depth,
            quality: options.quality,
        }
    }

    fn to_options(&self, seed: u64) -> GenerateOptions {
        GenerateOptions {
            seed,
            palette: self.palette,
            pattern: self.pattern,
            placement: self.placement,
            cluster_shape: self.cluster_shape,
            cluster_size: self.cluster_size,
            cluster_density: self.cluster_density,
            smoothing_passes: self.smoothing_passes,
            contrast: self.contrast,
            saturation: self.saturation,
            lightness: self.lightness,
            variant_strength: self.variant_strength,
            ore_pattern: self.ore_pattern,
            ore_coverage: self.ore_coverage,
            ore_branches: self.ore_branches,
            ore_thickness: self.ore_thickness,
            ore_center_bias: self.ore_center_bias,
            leaf_hole_density: self.leaf_hole_density,
            grass_fringe_depth: self.grass_fringe_depth,
            quality: self.quality,
            ..Default::default()
        }
    }
}

impl TypedMaterialOverrides {
    pub fn is_empty(&self) -> bool {
        MaterialField::ALL
            .into_iter()
            .all(|field| !self.contains(field))
    }

    pub fn contains(&self, field: MaterialField) -> bool {
        match field {
            MaterialField::Pattern => self.pattern.is_some(),
            MaterialField::Placement => self.placement.is_some(),
            MaterialField::ClusterShape => self.cluster_shape.is_some(),
            MaterialField::ClusterSize => self.cluster_size.is_some(),
            MaterialField::ClusterDensity => self.cluster_density.is_some(),
            MaterialField::SmoothingPasses => self.smoothing_passes.is_some(),
            MaterialField::Contrast => self.contrast.is_some(),
            MaterialField::Saturation => self.saturation.is_some(),
            MaterialField::Lightness => self.lightness.is_some(),
            MaterialField::VariantStrength => self.variant_strength.is_some(),
            MaterialField::OrePattern => self.ore_pattern.is_some(),
            MaterialField::OreCoverage => self.ore_coverage.is_some(),
            MaterialField::OreBranches => self.ore_branches.is_some(),
            MaterialField::OreThickness => self.ore_thickness.is_some(),
            MaterialField::OreCenterBias => self.ore_center_bias.is_some(),
            MaterialField::LeafHoleDensity => self.leaf_hole_density.is_some(),
            MaterialField::GrassFringeDepth => self.grass_fringe_depth.is_some(),
        }
    }

    fn apply(&self, block: BlockKind, options: &mut GenerateOptions) -> Result<(), PackError> {
        self.validate_applicability(block)?;
        macro_rules! set {
            ($field:ident, $slug:literal) => {
                if let Some(value) = self.$field {
                    options.apply_material_override(
                        block.slug(),
                        $slug,
                        &serialize_value(value)?,
                    )?;
                }
            };
        }
        set!(pattern, "pattern");
        set!(placement, "placement");
        set!(cluster_shape, "cluster-shape");
        set!(cluster_size, "cluster-size");
        set!(cluster_density, "cluster-density");
        set!(smoothing_passes, "smoothing-passes");
        set!(contrast, "contrast");
        set!(saturation, "saturation");
        set!(lightness, "lightness");
        set!(variant_strength, "variant-strength");
        set!(ore_pattern, "ore-pattern");
        set!(ore_coverage, "ore-coverage");
        set!(ore_branches, "ore-branches");
        set!(ore_thickness, "ore-thickness");
        set!(ore_center_bias, "ore-center-bias");
        set!(leaf_hole_density, "leaf-hole-density");
        set!(grass_fringe_depth, "grass-fringe-depth");
        Ok(())
    }

    fn validate_applicability(&self, block: BlockKind) -> Result<(), PackError> {
        let allowed = material_fields(block);
        if let Some(field) = MaterialField::ALL
            .into_iter()
            .find(|field| self.contains(*field) && !allowed.contains(field))
        {
            return Err(PackError::Invalid(format!(
                "{} does not affect {}",
                field.slug(),
                block.slug()
            )));
        }
        Ok(())
    }

    fn from_fields(fields: &BTreeMap<String, String>) -> Result<Self, PackError> {
        let mut result = Self::default();
        for (field, value) in fields {
            match field.as_str() {
                "pattern" => result.pattern = Some(parse_enum(value, field)?),
                "placement" => result.placement = Some(parse_enum(value, field)?),
                "cluster-shape" => result.cluster_shape = Some(parse_enum(value, field)?),
                "cluster-size" => result.cluster_size = Some(parse(value, field)?),
                "cluster-density" => result.cluster_density = Some(parse(value, field)?),
                "smoothing-passes" => result.smoothing_passes = Some(parse(value, field)?),
                "contrast" => result.contrast = Some(parse(value, field)?),
                "saturation" => result.saturation = Some(parse(value, field)?),
                "lightness" => result.lightness = Some(parse(value, field)?),
                "variant-strength" => result.variant_strength = Some(parse(value, field)?),
                "ore-pattern" => result.ore_pattern = Some(parse_enum(value, field)?),
                "ore-coverage" => result.ore_coverage = Some(parse(value, field)?),
                "ore-branches" => result.ore_branches = Some(parse(value, field)?),
                "ore-thickness" => result.ore_thickness = Some(parse(value, field)?),
                "ore-center-bias" => result.ore_center_bias = Some(parse(value, field)?),
                "leaf-hole-density" => result.leaf_hole_density = Some(parse(value, field)?),
                "grass-fringe-depth" => result.grass_fringe_depth = Some(parse(value, field)?),
                _ => {
                    return Err(PackError::Invalid(format!(
                        "unknown material field: {field}"
                    )));
                }
            }
        }
        Ok(result)
    }
}

pub fn slugify_pack_id(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_hyphen = false;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
            previous_hyphen = false;
        } else if !previous_hyphen && !slug.is_empty() {
            slug.push('-');
            previous_hyphen = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "generated".into()
    } else {
        slug
    }
}

fn serialize_value<T: Serialize>(value: T) -> Result<String, PackError> {
    let encoded = toml::to_string(&ValueWrapper { value })?;
    Ok(encoded
        .trim()
        .strip_prefix("value = ")
        .unwrap_or(encoded.trim())
        .trim_matches('"')
        .to_owned())
}

#[derive(Serialize)]
struct ValueWrapper<T> {
    value: T,
}

fn parse<T: std::str::FromStr>(value: &str, field: &str) -> Result<T, PackError> {
    value
        .parse()
        .map_err(|_| PackError::Invalid(format!("invalid {field}: {value}")))
}

fn parse_enum<T: ValueEnum>(value: &str, field: &str) -> Result<T, PackError> {
    T::from_str(value, true).map_err(|_| PackError::Invalid(format!("invalid {field}: {value}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_material_overrides_reject_no_op_fields() {
        let mut project = TextureProject::randomized(7);
        project.material.insert(
            "torch".into(),
            TypedMaterialOverrides {
                ore_coverage: Some(0.32),
                ..Default::default()
            },
        );
        assert!(project.validate().is_err());
    }

    #[test]
    fn non_finite_values_are_rejected() {
        let mut project = TextureProject::randomized(7);
        project.parameters.contrast = f32::NAN;
        assert!(project.validate().is_err());
    }

    #[test]
    fn typed_material_values_round_trip_through_generate_options() {
        let mut project = TextureProject::randomized(7);
        project.material.insert(
            "stone".into(),
            TypedMaterialOverrides {
                pattern: Some(PatternAlgorithm::BrokenStrata),
                contrast: Some(1.12),
                ..Default::default()
            },
        );
        let options = project.to_generate_options().unwrap();
        let replay = TextureProject::from_generate_options(&options).unwrap();
        assert_eq!(replay, project);
    }
}
