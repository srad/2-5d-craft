mod applicability;
mod choices;
mod definitions;
mod grid;

pub use applicability::{MaterialField, material_fields};
pub use choices::ControlChoiceValue;
pub use definitions::{CONTROL_DEFINITIONS, control_definition};
pub use grid::{control_cardinality, control_index, control_value, snap_f32};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct F32Limits {
    pub min: f32,
    pub max: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsizeLimits {
    pub min: usize,
    pub max: usize,
}

pub const CLUSTER_SIZE_LIMITS: UsizeLimits = UsizeLimits { min: 1, max: 12 };
pub const CLUSTER_DENSITY_LIMITS: F32Limits = F32Limits {
    min: 0.04,
    max: 0.48,
};
pub const SMOOTHING_PASSES_LIMITS: UsizeLimits = UsizeLimits { min: 0, max: 2 };
pub const CONTRAST_LIMITS: F32Limits = F32Limits {
    min: 0.65,
    max: 1.5,
};
pub const SATURATION_LIMITS: F32Limits = F32Limits { min: 0.5, max: 1.5 };
pub const LIGHTNESS_LIMITS: F32Limits = F32Limits {
    min: -0.25,
    max: 0.25,
};
pub const VARIANT_STRENGTH_LIMITS: (i16, i16) = (0, 16);
pub const ORE_COVERAGE_LIMITS: F32Limits = F32Limits {
    min: 0.20,
    max: 0.35,
};
pub const ORE_BRANCHES_LIMITS: UsizeLimits = UsizeLimits { min: 1, max: 10 };
pub const ORE_THICKNESS_LIMITS: UsizeLimits = UsizeLimits { min: 1, max: 4 };
pub const ORE_CENTER_BIAS_LIMITS: F32Limits = F32Limits { min: 0.0, max: 1.0 };
pub const LEAF_HOLE_DENSITY_LIMITS: F32Limits = F32Limits {
    min: 0.0,
    max: 0.25,
};
pub const GRASS_FRINGE_DEPTH_LIMITS: UsizeLimits = UsizeLimits { min: 1, max: 7 };

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlField {
    Palette,
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
    Quality,
}

impl ControlField {
    pub const ALL: [Self; 19] = [
        Self::Palette,
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
        Self::Quality,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlDataType {
    Choice,
    UnsignedInteger,
    SignedInteger,
    Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlChoice {
    pub value: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlDefinition {
    pub field: ControlField,
    pub key: &'static str,
    pub label: &'static str,
    pub data_type: ControlDataType,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub step: Option<f64>,
    pub choices: &'static [ControlChoice],
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_control_has_complete_authoritative_metadata() {
        assert_eq!(CONTROL_DEFINITIONS.len(), ControlField::ALL.len());
        let keys = CONTROL_DEFINITIONS
            .iter()
            .map(|definition| definition.key)
            .collect::<BTreeSet<_>>();
        assert_eq!(keys.len(), CONTROL_DEFINITIONS.len());
        for field in ControlField::ALL {
            let definition = control_definition(field);
            assert!(!definition.key.is_empty());
            assert!(!definition.label.is_empty());
            match definition.data_type {
                ControlDataType::Choice => {
                    assert!(!definition.choices.is_empty());
                    assert!(definition.minimum.is_none());
                    assert!(definition.maximum.is_none());
                    assert!(definition.step.is_none());
                }
                ControlDataType::UnsignedInteger
                | ControlDataType::SignedInteger
                | ControlDataType::Decimal => {
                    assert!(definition.choices.is_empty());
                    assert!(definition.minimum.unwrap() <= definition.maximum.unwrap());
                    assert!(definition.step.unwrap() > 0.0);
                }
            }
        }
    }

    #[test]
    fn typed_choice_values_match_their_definitions() {
        fn verify<T: ControlChoiceValue>() {
            let definition = control_definition(T::CONTROL_FIELD);
            assert_eq!(definition.choices.len(), T::values().len());
            for value in T::values() {
                assert!(
                    definition
                        .choices
                        .iter()
                        .any(|choice| choice.value == value.value())
                );
            }
        }
        verify::<crate::PalettePreset>();
        verify::<crate::PatternAlgorithm>();
        verify::<crate::PlacementAlgorithm>();
        verify::<crate::ClusterShape>();
        verify::<crate::OrePattern>();
        verify::<crate::QualityPreset>();
    }
}
