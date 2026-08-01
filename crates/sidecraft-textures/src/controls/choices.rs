use super::ControlField;
use crate::{
    ClusterShape, OrePattern, PalettePreset, PatternAlgorithm, PlacementAlgorithm, QualityPreset,
};

pub trait ControlChoiceValue: Copy + PartialEq + 'static {
    const CONTROL_FIELD: ControlField;

    fn values() -> &'static [Self];
    fn value(self) -> &'static str;
}

macro_rules! choice_values {
    ($type:ty, $field:ident, [$($variant:ident => $value:literal),+ $(,)?]) => {
        impl ControlChoiceValue for $type {
            const CONTROL_FIELD: ControlField = ControlField::$field;

            fn values() -> &'static [Self] {
                &[$(<$type>::$variant),+]
            }

            fn value(self) -> &'static str {
                match self {
                    $(<$type>::$variant => $value),+
                }
            }
        }
    };
}

choice_values!(
    PalettePreset,
    Palette,
    [
        Earthy => "earthy",
        DeepEarth => "deep-earth",
        Classic => "classic",
    ]
);
choice_values!(
    PatternAlgorithm,
    Pattern,
    [
        ClusterStamps => "cluster-stamps",
        EvenlyVaried => "evenly-varied",
        CellularClumps => "cellular-clumps",
        BrokenStrata => "broken-strata",
        ShortWalks => "short-walks",
    ]
);
choice_values!(
    PlacementAlgorithm,
    Placement,
    [
        Uniform => "uniform",
        JitteredGrid => "jittered-grid",
        PoissonDisc => "poisson-disc",
    ]
);
choice_values!(
    ClusterShape,
    ClusterShape,
    [
        Mixed => "mixed",
        Polyomino => "polyomino",
        Rectangular => "rectangular",
    ]
);
choice_values!(
    OrePattern,
    OrePattern,
    [
        CenterGrowth => "center-growth",
        BranchingWalk => "branching-walk",
        CompactCellular => "compact-cellular",
    ]
);
choice_values!(
    QualityPreset,
    Quality,
    [
        Relaxed => "relaxed",
        Balanced => "balanced",
        Strict => "strict",
    ]
);
