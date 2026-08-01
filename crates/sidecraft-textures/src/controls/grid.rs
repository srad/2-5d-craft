use super::{ControlDataType, ControlField, control_definition};

/// Number of distinct values a control can hold.
///
/// Choice controls count their variants; numeric controls count the points on their step grid.
/// This is the alphabet size the pack-code encoder packs against, so it must stay derived from
/// [`CONTROL_DEFINITIONS`](super::CONTROL_DEFINITIONS) rather than restated anywhere.
pub fn control_cardinality(field: ControlField) -> u64 {
    let definition = control_definition(field);
    match definition.data_type {
        ControlDataType::Choice => definition.choices.len() as u64,
        _ => {
            let (minimum, maximum, step) = numeric_shape(field);
            ((maximum - minimum) / step).round() as u64 + 1
        }
    }
}

/// Position of `value` on the control's step grid, clamped into range.
pub fn control_index(field: ControlField, value: f64) -> u64 {
    let (minimum, maximum, step) = numeric_shape(field);
    let clamped = value.clamp(minimum, maximum);
    let index = ((clamped - minimum) / step).round() as u64;
    index.min(control_cardinality(field) - 1)
}

/// Canonical value at grid position `index`.
///
/// Every producer of a control value routes through here, so the editor, the decoder and the
/// randomizer all yield bit-identical floats. Reconstructing as `minimum + index * step` in one
/// place and rounding as `(value / step).round() * step` in another differs by an ULP and would
/// break exact round-tripping.
pub fn control_value(field: ControlField, index: u64) -> f64 {
    let (minimum, maximum, step) = numeric_shape(field);
    let last = control_cardinality(field) - 1;
    // The limits are declared as `f32` and widened, so they land either side of the value they
    // read as: `0.65f32` is 0.6499999761581421 and `0.48f32` is 0.47999998927116394. Walking the
    // grid from `minimum` therefore drifts off both ends. Pin the endpoints to the declared
    // limits so every produced value stays inside the range `validate_options` enforces.
    match index.min(last) {
        0 => minimum,
        index if index == last => maximum,
        index => minimum + index as f64 * step,
    }
}

/// Snap a value onto the control's step grid.
pub fn snap_to_step(field: ControlField, value: f64) -> f64 {
    control_value(field, control_index(field, value))
}

/// Snap an `f32` control value, as stored in [`TextureParameters`](crate::TextureParameters).
pub fn snap_f32(field: ControlField, value: f32) -> f32 {
    snap_to_step(field, f64::from(value)) as f32
}

fn numeric_shape(field: ControlField) -> (f64, f64, f64) {
    let definition = control_definition(field);
    let (Some(minimum), Some(maximum), Some(step)) =
        (definition.minimum, definition.maximum, definition.step)
    else {
        unreachable!("{:?} is a choice control and has no step grid", field);
    };
    (minimum, maximum, step)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numeric_fields() -> impl Iterator<Item = ControlField> {
        ControlField::ALL
            .into_iter()
            .filter(|field| control_definition(*field).data_type != ControlDataType::Choice)
    }

    #[test]
    fn every_numeric_range_lands_exactly_on_its_grid() {
        for field in numeric_fields() {
            let (minimum, maximum, step) = numeric_shape(field);
            // The limits are `f32` widened to `f64`, so allow for that noise while still
            // catching a range that genuinely is not a whole number of steps wide.
            let span = (maximum - minimum) / step;
            assert!(
                (span - span.round()).abs() < 1e-4,
                "{field:?} spans {span} steps, which is not a whole number"
            );
            assert_eq!(
                control_value(field, control_cardinality(field) - 1),
                maximum
            );
            assert_eq!(control_value(field, 0), minimum);
        }
    }

    #[test]
    fn snapping_is_idempotent_and_round_trips_through_indices() {
        for field in numeric_fields() {
            let cardinality = control_cardinality(field);
            for index in 0..cardinality {
                let value = control_value(field, index);
                assert_eq!(control_index(field, value), index, "{field:?} at {index}");
                assert_eq!(snap_to_step(field, value), value, "{field:?} at {index}");
                let nudged = snap_to_step(field, value + f64::EPSILON);
                assert_eq!(nudged, value, "{field:?} not stable against float noise");
            }
        }
    }

    #[test]
    fn out_of_range_values_clamp_into_the_grid() {
        for field in numeric_fields() {
            let (minimum, maximum, _) = numeric_shape(field);
            assert_eq!(
                snap_to_step(field, minimum - 1000.0),
                minimum,
                "{field:?} low"
            );
            assert_eq!(
                snap_to_step(field, maximum + 1000.0),
                maximum,
                "{field:?} high"
            );
        }
    }

    #[test]
    fn cardinalities_match_the_documented_value_space() {
        assert_eq!(control_cardinality(ControlField::Palette), 3);
        assert_eq!(control_cardinality(ControlField::Pattern), 5);
        assert_eq!(control_cardinality(ControlField::ClusterSize), 12);
        assert_eq!(control_cardinality(ControlField::ClusterDensity), 45);
        assert_eq!(control_cardinality(ControlField::Contrast), 86);
        assert_eq!(control_cardinality(ControlField::Saturation), 101);
        assert_eq!(control_cardinality(ControlField::Lightness), 51);
        assert_eq!(control_cardinality(ControlField::VariantStrength), 17);
        assert_eq!(control_cardinality(ControlField::OreCoverage), 31);
        assert_eq!(control_cardinality(ControlField::OreCenterBias), 101);
        assert_eq!(control_cardinality(ControlField::LeafHoleDensity), 26);
        assert_eq!(control_cardinality(ControlField::GrassFringeDepth), 7);
    }
}
