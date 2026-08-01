use super::*;

const PALETTE_CHOICES: &[ControlChoice] = &[
    ControlChoice {
        value: "earthy",
        label: "Earthy",
    },
    ControlChoice {
        value: "deep-earth",
        label: "Deep Earth",
    },
    ControlChoice {
        value: "classic",
        label: "Classic",
    },
];
const PATTERN_CHOICES: &[ControlChoice] = &[
    ControlChoice {
        value: "cluster-stamps",
        label: "Cluster Stamps",
    },
    ControlChoice {
        value: "evenly-varied",
        label: "Evenly Varied",
    },
    ControlChoice {
        value: "cellular-clumps",
        label: "Cellular Clumps",
    },
    ControlChoice {
        value: "broken-strata",
        label: "Broken Strata",
    },
    ControlChoice {
        value: "short-walks",
        label: "Short Walks",
    },
];
const PLACEMENT_CHOICES: &[ControlChoice] = &[
    ControlChoice {
        value: "uniform",
        label: "Uniform",
    },
    ControlChoice {
        value: "jittered-grid",
        label: "Jittered Grid",
    },
    ControlChoice {
        value: "poisson-disc",
        label: "Poisson Disc",
    },
];
const CLUSTER_SHAPE_CHOICES: &[ControlChoice] = &[
    ControlChoice {
        value: "mixed",
        label: "Mixed",
    },
    ControlChoice {
        value: "polyomino",
        label: "Polyomino",
    },
    ControlChoice {
        value: "rectangular",
        label: "Rectangular",
    },
];
const ORE_PATTERN_CHOICES: &[ControlChoice] = &[
    ControlChoice {
        value: "center-growth",
        label: "Center Growth",
    },
    ControlChoice {
        value: "branching-walk",
        label: "Branching Walk",
    },
    ControlChoice {
        value: "compact-cellular",
        label: "Compact Cellular",
    },
];
const QUALITY_CHOICES: &[ControlChoice] = &[
    ControlChoice {
        value: "relaxed",
        label: "Relaxed",
    },
    ControlChoice {
        value: "balanced",
        label: "Balanced",
    },
    ControlChoice {
        value: "strict",
        label: "Strict",
    },
];
const NO_CHOICES: &[ControlChoice] = &[];

macro_rules! choice_control {
    ($field:ident, $key:literal, $label:literal, $choices:ident) => {
        ControlDefinition {
            field: ControlField::$field,
            key: $key,
            label: $label,
            data_type: ControlDataType::Choice,
            minimum: None,
            maximum: None,
            step: None,
            choices: $choices,
        }
    };
}

macro_rules! numeric_control {
    ($field:ident, $key:literal, $label:literal, $kind:ident, $min:expr, $max:expr, $step:expr) => {
        ControlDefinition {
            field: ControlField::$field,
            key: $key,
            label: $label,
            data_type: ControlDataType::$kind,
            minimum: Some($min as f64),
            maximum: Some($max as f64),
            step: Some($step as f64),
            choices: NO_CHOICES,
        }
    };
}

pub const CONTROL_DEFINITIONS: &[ControlDefinition] = &[
    choice_control!(Palette, "palette", "Palette", PALETTE_CHOICES),
    choice_control!(Pattern, "pattern", "Pixel pattern", PATTERN_CHOICES),
    choice_control!(
        Placement,
        "placement",
        "Pattern placement",
        PLACEMENT_CHOICES
    ),
    choice_control!(
        ClusterShape,
        "cluster-shape",
        "Cluster shape",
        CLUSTER_SHAPE_CHOICES
    ),
    numeric_control!(
        ClusterSize,
        "cluster-size",
        "Cluster size",
        UnsignedInteger,
        CLUSTER_SIZE_LIMITS.min,
        CLUSTER_SIZE_LIMITS.max,
        1
    ),
    numeric_control!(
        ClusterDensity,
        "cluster-density",
        "Cluster density",
        Decimal,
        CLUSTER_DENSITY_LIMITS.min,
        CLUSTER_DENSITY_LIMITS.max,
        0.01
    ),
    numeric_control!(
        SmoothingPasses,
        "smoothing-passes",
        "Smoothing passes",
        UnsignedInteger,
        SMOOTHING_PASSES_LIMITS.min,
        SMOOTHING_PASSES_LIMITS.max,
        1
    ),
    numeric_control!(
        Contrast,
        "contrast",
        "Contrast",
        Decimal,
        CONTRAST_LIMITS.min,
        CONTRAST_LIMITS.max,
        0.01
    ),
    numeric_control!(
        Saturation,
        "saturation",
        "Saturation",
        Decimal,
        SATURATION_LIMITS.min,
        SATURATION_LIMITS.max,
        0.01
    ),
    numeric_control!(
        Lightness,
        "lightness",
        "Lightness",
        Decimal,
        LIGHTNESS_LIMITS.min,
        LIGHTNESS_LIMITS.max,
        0.01
    ),
    numeric_control!(
        VariantStrength,
        "variant-strength",
        "Variant strength",
        SignedInteger,
        VARIANT_STRENGTH_LIMITS.0,
        VARIANT_STRENGTH_LIMITS.1,
        1
    ),
    choice_control!(
        OrePattern,
        "ore-pattern",
        "Ore pattern",
        ORE_PATTERN_CHOICES
    ),
    numeric_control!(
        OreCoverage,
        "ore-coverage",
        "Ore coverage",
        Decimal,
        ORE_COVERAGE_LIMITS.min,
        ORE_COVERAGE_LIMITS.max,
        0.005
    ),
    numeric_control!(
        OreBranches,
        "ore-branches",
        "Ore branches",
        UnsignedInteger,
        ORE_BRANCHES_LIMITS.min,
        ORE_BRANCHES_LIMITS.max,
        1
    ),
    numeric_control!(
        OreThickness,
        "ore-thickness",
        "Ore thickness",
        UnsignedInteger,
        ORE_THICKNESS_LIMITS.min,
        ORE_THICKNESS_LIMITS.max,
        1
    ),
    numeric_control!(
        OreCenterBias,
        "ore-center-bias",
        "Ore center bias",
        Decimal,
        ORE_CENTER_BIAS_LIMITS.min,
        ORE_CENTER_BIAS_LIMITS.max,
        0.01
    ),
    numeric_control!(
        LeafHoleDensity,
        "leaf-hole-density",
        "Leaf hole density",
        Decimal,
        LEAF_HOLE_DENSITY_LIMITS.min,
        LEAF_HOLE_DENSITY_LIMITS.max,
        0.01
    ),
    numeric_control!(
        GrassFringeDepth,
        "grass-fringe-depth",
        "Grass fringe depth",
        UnsignedInteger,
        GRASS_FRINGE_DEPTH_LIMITS.min,
        GRASS_FRINGE_DEPTH_LIMITS.max,
        1
    ),
    choice_control!(Quality, "quality", "Quality", QUALITY_CHOICES),
];

pub fn control_definition(field: ControlField) -> &'static ControlDefinition {
    CONTROL_DEFINITIONS
        .iter()
        .find(|definition| definition.field == field)
        .expect("every control field has a definition")
}
