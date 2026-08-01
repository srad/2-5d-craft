use super::ControlField;
use crate::BlockKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialField {
    Pattern,
    Placement,
    ClusterShape,
    ClusterSize,
    ClusterDensity,
    SmoothingPasses,
    Contrast,
    Saturation,
    Lightness,
    VariantStrength,
    OrePattern,
    OreCoverage,
    OreBranches,
    OreThickness,
    OreCenterBias,
    LeafHoleDensity,
    GrassFringeDepth,
}

impl MaterialField {
    pub const ALL: [Self; 17] = [
        Self::Pattern,
        Self::Placement,
        Self::ClusterShape,
        Self::ClusterSize,
        Self::ClusterDensity,
        Self::SmoothingPasses,
        Self::Contrast,
        Self::Saturation,
        Self::Lightness,
        Self::VariantStrength,
        Self::OrePattern,
        Self::OreCoverage,
        Self::OreBranches,
        Self::OreThickness,
        Self::OreCenterBias,
        Self::LeafHoleDensity,
        Self::GrassFringeDepth,
    ];

    pub const fn slug(self) -> &'static str {
        match self {
            Self::Pattern => "pattern",
            Self::Placement => "placement",
            Self::ClusterShape => "cluster-shape",
            Self::ClusterSize => "cluster-size",
            Self::ClusterDensity => "cluster-density",
            Self::SmoothingPasses => "smoothing-passes",
            Self::Contrast => "contrast",
            Self::Saturation => "saturation",
            Self::Lightness => "lightness",
            Self::VariantStrength => "variant-strength",
            Self::OrePattern => "ore-pattern",
            Self::OreCoverage => "ore-coverage",
            Self::OreBranches => "ore-branches",
            Self::OreThickness => "ore-thickness",
            Self::OreCenterBias => "ore-center-bias",
            Self::LeafHoleDensity => "leaf-hole-density",
            Self::GrassFringeDepth => "grass-fringe-depth",
        }
    }

    pub const fn control_field(self) -> ControlField {
        match self {
            Self::Pattern => ControlField::Pattern,
            Self::Placement => ControlField::Placement,
            Self::ClusterShape => ControlField::ClusterShape,
            Self::ClusterSize => ControlField::ClusterSize,
            Self::ClusterDensity => ControlField::ClusterDensity,
            Self::SmoothingPasses => ControlField::SmoothingPasses,
            Self::Contrast => ControlField::Contrast,
            Self::Saturation => ControlField::Saturation,
            Self::Lightness => ControlField::Lightness,
            Self::VariantStrength => ControlField::VariantStrength,
            Self::OrePattern => ControlField::OrePattern,
            Self::OreCoverage => ControlField::OreCoverage,
            Self::OreBranches => ControlField::OreBranches,
            Self::OreThickness => ControlField::OreThickness,
            Self::OreCenterBias => ControlField::OreCenterBias,
            Self::LeafHoleDensity => ControlField::LeafHoleDensity,
            Self::GrassFringeDepth => ControlField::GrassFringeDepth,
        }
    }
}

const COMMON_FIELDS: &[MaterialField] = &[
    MaterialField::Pattern,
    MaterialField::Placement,
    MaterialField::ClusterShape,
    MaterialField::ClusterSize,
    MaterialField::ClusterDensity,
    MaterialField::SmoothingPasses,
    MaterialField::Contrast,
    MaterialField::Saturation,
    MaterialField::Lightness,
    MaterialField::VariantStrength,
];
const GRASS_FIELDS: &[MaterialField] = &[
    MaterialField::Pattern,
    MaterialField::Placement,
    MaterialField::ClusterShape,
    MaterialField::ClusterSize,
    MaterialField::ClusterDensity,
    MaterialField::SmoothingPasses,
    MaterialField::Contrast,
    MaterialField::Saturation,
    MaterialField::Lightness,
    MaterialField::VariantStrength,
    MaterialField::GrassFringeDepth,
];
const ORE_FIELDS: &[MaterialField] = &[
    MaterialField::Pattern,
    MaterialField::Placement,
    MaterialField::ClusterShape,
    MaterialField::ClusterSize,
    MaterialField::ClusterDensity,
    MaterialField::SmoothingPasses,
    MaterialField::Contrast,
    MaterialField::Saturation,
    MaterialField::Lightness,
    MaterialField::VariantStrength,
    MaterialField::OrePattern,
    MaterialField::OreCoverage,
    MaterialField::OreBranches,
    MaterialField::OreThickness,
    MaterialField::OreCenterBias,
];
const LEAF_FIELDS: &[MaterialField] = &[
    MaterialField::Pattern,
    MaterialField::Placement,
    MaterialField::ClusterShape,
    MaterialField::ClusterSize,
    MaterialField::ClusterDensity,
    MaterialField::SmoothingPasses,
    MaterialField::Contrast,
    MaterialField::Saturation,
    MaterialField::Lightness,
    MaterialField::VariantStrength,
    MaterialField::LeafHoleDensity,
];
const PALETTE_ADJUSTMENT_FIELDS: &[MaterialField] = &[
    MaterialField::Contrast,
    MaterialField::Saturation,
    MaterialField::Lightness,
];

pub const fn material_fields(block: BlockKind) -> &'static [MaterialField] {
    match block {
        BlockKind::Grass => GRASS_FIELDS,
        BlockKind::CoalOre | BlockKind::IronOre => ORE_FIELDS,
        BlockKind::Leaves => LEAF_FIELDS,
        BlockKind::Wood | BlockKind::Torch => PALETTE_ADJUSTMENT_FIELDS,
        BlockKind::Dirt | BlockKind::Stone | BlockKind::Bedrock => COMMON_FIELDS,
    }
}
