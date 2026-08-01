//! Reversible pack codes.
//!
//! A pack code is the pack's identity *and* its recipe: decoding one reconstructs the exact
//! generation state it names — seed, all 19 global controls, and every material override. That is
//! what makes it worth putting in the ID, where `generated-<seed>` only described a pack until
//! someone moved a slider.
//!
//! Codes are short because they never spend bits on what is already implied. A freshly randomized
//! project is fully determined by its seed, so [`GlobalMode::Seed`] stores nothing else and lands
//! around 11 characters — shorter than the `generated-1785602118763815816` it replaces. Editing
//! one control moves to [`GlobalMode::Delta`], which stores a 19-field mask and only what changed.
//! [`GlobalMode::Full`] is the self-contained fallback.
//!
//! ## Compatibility
//!
//! [`GlobalMode::Seed`] and [`GlobalMode::Delta`] replay [`randomized_options`] at decode time, so
//! that function is part of this format. Changing what it produces changes what every existing
//! code of those modes means. Such a change must bump [`FORMAT_VERSION`] and keep the old
//! generator reachable for decoding older codes.

mod digits;

use crate::{
    BlockKind, ControlField, MaterialField, PackError, TextureParameters, TypedMaterialOverrides,
    controls::{ControlChoiceValue, control_cardinality, control_index, control_value},
    project::randomized_options,
};
use digits::Digits;
use std::collections::BTreeMap;

/// Bumped whenever the meaning of an existing encoding changes.
pub const FORMAT_VERSION: u64 = 1;
const VERSION_SPACE: u64 = 8;
const SEED_LENGTH_SPACE: u64 = 8;
const BYTE_SPACE: u64 = 256;
const FLAG_SPACE: u64 = 2;
const CHECKSUM_SPACE: u64 = 32;
const PREFIX: &str = "t";

/// How the 19 global controls are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalMode {
    /// Nothing stored — replayed from the seed.
    Seed = 0,
    /// A presence mask plus only the controls that differ from the seed's own randomization.
    Delta = 1,
    /// Every control, self-contained.
    Full = 2,
}

impl GlobalMode {
    const ALL: [Self; 3] = [Self::Seed, Self::Delta, Self::Full];

    fn from_index(index: u64) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| *mode as u64 == index)
    }
}

/// The generation state a pack code carries.
///
/// Deliberately excludes `name` and `author`: they never reach a pixel, and the ID is derived
/// from this state, so anything the ID is stored beside cannot also be an input to it.
#[derive(Debug, Clone, PartialEq)]
pub struct PackCodeState {
    pub seed: u64,
    pub parameters: TextureParameters,
    pub material: BTreeMap<String, TypedMaterialOverrides>,
}

/// Encodes generation state into the shortest code that reproduces it.
pub fn encode_pack_code(state: &PackCodeState) -> Result<String, PackError> {
    verify_material_is_encodable(&state.material)?;
    let shortest = GlobalMode::ALL
        .into_iter()
        .filter_map(|mode| fields_for(mode, state))
        .map(|fields| render(&fields))
        .min_by_key(String::len)
        .ok_or_else(|| PackError::Invalid("no pack-code mode can represent this project".into()))?;
    // A code that does not decode to what it was given is worse than no code: it would name a
    // pack nobody can rebuild. Pay one decode to guarantee the promise the ID makes.
    let decoded = decode_pack_code(&shortest)?;
    if decoded != *state {
        return Err(PackError::Invalid(
            "pack code did not round-trip; the encoder and decoder disagree".into(),
        ));
    }
    Ok(shortest)
}

/// Reconstructs generation state from a code.
pub fn decode_pack_code(code: &str) -> Result<PackCodeState, PackError> {
    let body = code
        .strip_prefix(PREFIX)
        .ok_or_else(|| invalid(code, "codes start with 't'"))?;
    let mut value = Digits::from_base36(body)
        .ok_or_else(|| invalid(code, "contains a non-base36 character"))?;
    let mut popped = Vec::new();
    let mut pop = |value: &mut Digits, cardinality: u64| {
        let index = value.div_rem(cardinality);
        popped.push((index, cardinality));
        index
    };

    let version = pop(&mut value, VERSION_SPACE);
    if version != FORMAT_VERSION {
        return Err(invalid(
            code,
            &format!("unsupported pack-code version {version}, expected {FORMAT_VERSION}"),
        ));
    }
    let mode = GlobalMode::from_index(pop(&mut value, GlobalMode::ALL.len() as u64))
        .ok_or_else(|| invalid(code, "unknown global mode"))?;
    let has_material = pop(&mut value, FLAG_SPACE) == 1;
    let seed_bytes = pop(&mut value, SEED_LENGTH_SPACE) + 1;
    let mut seed = 0u64;
    for _ in 0..seed_bytes {
        seed = (seed << 8) | pop(&mut value, BYTE_SPACE);
    }

    // Every mode starts from the seed's own randomization; Full simply overwrites all of it.
    let mut parameters = baseline_parameters(seed);
    match mode {
        GlobalMode::Seed => {}
        GlobalMode::Delta => {
            let changed = ControlField::ALL
                .map(|field| (field, pop(&mut value, FLAG_SPACE) == 1))
                .into_iter()
                .filter(|(_, changed)| *changed)
                .map(|(field, _)| field)
                .collect::<Vec<_>>();
            for field in changed {
                let index = pop(&mut value, control_cardinality(field));
                write_parameter(&mut parameters, field, index);
            }
        }
        GlobalMode::Full => {
            for field in ControlField::ALL {
                let index = pop(&mut value, control_cardinality(field));
                write_parameter(&mut parameters, field, index);
            }
        }
    }

    let mut material = BTreeMap::new();
    if has_material {
        let blocks = BlockKind::ALL
            .map(|block| (block, pop(&mut value, FLAG_SPACE) == 1))
            .into_iter()
            .filter(|(_, present)| *present)
            .map(|(block, _)| block)
            .collect::<Vec<_>>();
        for block in blocks {
            let fields = crate::controls::material_fields(block);
            let present = fields
                .iter()
                .map(|field| (*field, pop(&mut value, FLAG_SPACE) == 1))
                .filter(|(_, present)| *present)
                .map(|(field, _)| field)
                .collect::<Vec<_>>();
            let mut overrides = TypedMaterialOverrides::default();
            for field in present {
                let index = pop(&mut value, control_cardinality(field.control_field()));
                write_override(&mut overrides, field, index);
            }
            material.insert(block.slug().to_owned(), overrides);
        }
    }

    let stated = pop(&mut value, CHECKSUM_SPACE);
    let expected = checksum(&popped[..popped.len() - 1]);
    if stated != expected {
        return Err(invalid(code, "checksum mismatch; the code is mistyped"));
    }
    if !value.is_zero() {
        return Err(invalid(code, "trailing data after the checksum"));
    }
    Ok(PackCodeState {
        seed,
        parameters,
        material,
    })
}

/// Builds the field schedule in *pop* order, or `None` when the mode cannot represent the state.
fn fields_for(mode: GlobalMode, state: &PackCodeState) -> Option<Vec<(u64, u64)>> {
    let baseline = baseline_parameters(state.seed);
    if mode == GlobalMode::Seed && state.parameters != baseline {
        return None;
    }
    let mut fields = vec![
        (FORMAT_VERSION, VERSION_SPACE),
        (mode as u64, GlobalMode::ALL.len() as u64),
        (u64::from(!state.material.is_empty()), FLAG_SPACE),
    ];

    let seed_bytes = (8 - (state.seed.leading_zeros() / 8)).max(1) as usize;
    fields.push((seed_bytes as u64 - 1, SEED_LENGTH_SPACE));
    for byte in state.seed.to_be_bytes().into_iter().skip(8 - seed_bytes) {
        fields.push((u64::from(byte), BYTE_SPACE));
    }

    match mode {
        GlobalMode::Seed => {}
        GlobalMode::Delta => {
            let changed = ControlField::ALL
                .into_iter()
                .filter(|field| {
                    read_parameter(&state.parameters, *field) != read_parameter(&baseline, *field)
                })
                .collect::<Vec<_>>();
            for field in ControlField::ALL {
                fields.push((u64::from(changed.contains(&field)), FLAG_SPACE));
            }
            for field in changed {
                fields.push((
                    read_parameter(&state.parameters, field),
                    control_cardinality(field),
                ));
            }
        }
        GlobalMode::Full => {
            for field in ControlField::ALL {
                fields.push((
                    read_parameter(&state.parameters, field),
                    control_cardinality(field),
                ));
            }
        }
    }

    if !state.material.is_empty() {
        let present = BlockKind::ALL
            .into_iter()
            .filter(|block| {
                state
                    .material
                    .get(block.slug())
                    .is_some_and(|overrides| !overrides.is_empty())
            })
            .collect::<Vec<_>>();
        for block in BlockKind::ALL {
            fields.push((u64::from(present.contains(&block)), FLAG_SPACE));
        }
        for block in present {
            let overrides = &state.material[block.slug()];
            let applicable = crate::controls::material_fields(block);
            for field in applicable {
                fields.push((u64::from(overrides.contains(*field)), FLAG_SPACE));
            }
            for field in applicable
                .iter()
                .filter(|field| overrides.contains(**field))
            {
                fields.push((
                    read_override(overrides, *field)?,
                    control_cardinality(field.control_field()),
                ));
            }
        }
    }

    fields.push((checksum(&fields), CHECKSUM_SPACE));
    Some(fields)
}

/// Folds the schedule into one integer. Pushed in reverse so the header pops out first — the
/// decoder needs the mode before it knows how many fields follow.
fn render(fields: &[(u64, u64)]) -> String {
    let mut value = Digits::default();
    for (index, cardinality) in fields.iter().rev() {
        value.mul_add(*cardinality, *index);
    }
    format!("{PREFIX}{}", value.to_base36())
}

fn checksum(fields: &[(u64, u64)]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for (index, cardinality) in fields {
        for value in [*index, *cardinality] {
            hash ^= value;
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
    }
    hash % CHECKSUM_SPACE
}

fn baseline_parameters(seed: u64) -> TextureParameters {
    TextureParameters::from_options(&randomized_options(seed))
}

fn verify_material_is_encodable(
    material: &BTreeMap<String, TypedMaterialOverrides>,
) -> Result<(), PackError> {
    for (slug, overrides) in material {
        let block = BlockKind::ALL
            .into_iter()
            .find(|block| block.slug() == slug)
            .ok_or_else(|| PackError::Invalid(format!("unknown material: {slug}")))?;
        overrides.validate_applicability(block)?;
    }
    Ok(())
}

fn invalid(code: &str, reason: &str) -> PackError {
    PackError::Invalid(format!("invalid pack code {code:?}: {reason}"))
}

fn choice_index<T: ControlChoiceValue>(value: T) -> u64 {
    T::values()
        .iter()
        .position(|candidate| *candidate == value)
        .expect("a choice is always one of its own values") as u64
}

fn choice_at<T: ControlChoiceValue>(index: u64) -> T {
    let values = T::values();
    values[(index as usize).min(values.len() - 1)]
}

fn numeric_index(field: ControlField, value: f64) -> u64 {
    control_index(field, value)
}

fn read_parameter(parameters: &TextureParameters, field: ControlField) -> u64 {
    match field {
        ControlField::Palette => choice_index(parameters.palette),
        ControlField::Pattern => choice_index(parameters.pattern),
        ControlField::Placement => choice_index(parameters.placement),
        ControlField::ClusterShape => choice_index(parameters.cluster_shape),
        ControlField::OrePattern => choice_index(parameters.ore_pattern),
        ControlField::Quality => choice_index(parameters.quality),
        ControlField::ClusterSize => numeric_index(field, parameters.cluster_size as f64),
        ControlField::SmoothingPasses => numeric_index(field, parameters.smoothing_passes as f64),
        ControlField::VariantStrength => {
            numeric_index(field, f64::from(parameters.variant_strength))
        }
        ControlField::OreBranches => numeric_index(field, parameters.ore_branches as f64),
        ControlField::OreThickness => numeric_index(field, parameters.ore_thickness as f64),
        ControlField::GrassFringeDepth => {
            numeric_index(field, parameters.grass_fringe_depth as f64)
        }
        ControlField::ClusterDensity => numeric_index(field, f64::from(parameters.cluster_density)),
        ControlField::Contrast => numeric_index(field, f64::from(parameters.contrast)),
        ControlField::Saturation => numeric_index(field, f64::from(parameters.saturation)),
        ControlField::Lightness => numeric_index(field, f64::from(parameters.lightness)),
        ControlField::OreCoverage => numeric_index(field, f64::from(parameters.ore_coverage)),
        ControlField::OreCenterBias => numeric_index(field, f64::from(parameters.ore_center_bias)),
        ControlField::LeafHoleDensity => {
            numeric_index(field, f64::from(parameters.leaf_hole_density))
        }
    }
}

fn write_parameter(parameters: &mut TextureParameters, field: ControlField, index: u64) {
    let integer = || control_value(field, index).round();
    let decimal = || control_value(field, index) as f32;
    match field {
        ControlField::Palette => parameters.palette = choice_at(index),
        ControlField::Pattern => parameters.pattern = choice_at(index),
        ControlField::Placement => parameters.placement = choice_at(index),
        ControlField::ClusterShape => parameters.cluster_shape = choice_at(index),
        ControlField::OrePattern => parameters.ore_pattern = choice_at(index),
        ControlField::Quality => parameters.quality = choice_at(index),
        ControlField::ClusterSize => parameters.cluster_size = integer() as usize,
        ControlField::SmoothingPasses => parameters.smoothing_passes = integer() as usize,
        ControlField::VariantStrength => parameters.variant_strength = integer() as i16,
        ControlField::OreBranches => parameters.ore_branches = integer() as usize,
        ControlField::OreThickness => parameters.ore_thickness = integer() as usize,
        ControlField::GrassFringeDepth => parameters.grass_fringe_depth = integer() as usize,
        ControlField::ClusterDensity => parameters.cluster_density = decimal(),
        ControlField::Contrast => parameters.contrast = decimal(),
        ControlField::Saturation => parameters.saturation = decimal(),
        ControlField::Lightness => parameters.lightness = decimal(),
        ControlField::OreCoverage => parameters.ore_coverage = decimal(),
        ControlField::OreCenterBias => parameters.ore_center_bias = decimal(),
        ControlField::LeafHoleDensity => parameters.leaf_hole_density = decimal(),
    }
}

fn read_override(overrides: &TypedMaterialOverrides, field: MaterialField) -> Option<u64> {
    let control = field.control_field();
    Some(match field {
        MaterialField::Pattern => choice_index(overrides.pattern?),
        MaterialField::Placement => choice_index(overrides.placement?),
        MaterialField::ClusterShape => choice_index(overrides.cluster_shape?),
        MaterialField::OrePattern => choice_index(overrides.ore_pattern?),
        MaterialField::ClusterSize => numeric_index(control, overrides.cluster_size? as f64),
        MaterialField::SmoothingPasses => {
            numeric_index(control, overrides.smoothing_passes? as f64)
        }
        MaterialField::VariantStrength => {
            numeric_index(control, f64::from(overrides.variant_strength?))
        }
        MaterialField::OreBranches => numeric_index(control, overrides.ore_branches? as f64),
        MaterialField::OreThickness => numeric_index(control, overrides.ore_thickness? as f64),
        MaterialField::GrassFringeDepth => {
            numeric_index(control, overrides.grass_fringe_depth? as f64)
        }
        MaterialField::ClusterDensity => {
            numeric_index(control, f64::from(overrides.cluster_density?))
        }
        MaterialField::Contrast => numeric_index(control, f64::from(overrides.contrast?)),
        MaterialField::Saturation => numeric_index(control, f64::from(overrides.saturation?)),
        MaterialField::Lightness => numeric_index(control, f64::from(overrides.lightness?)),
        MaterialField::OreCoverage => numeric_index(control, f64::from(overrides.ore_coverage?)),
        MaterialField::OreCenterBias => {
            numeric_index(control, f64::from(overrides.ore_center_bias?))
        }
        MaterialField::LeafHoleDensity => {
            numeric_index(control, f64::from(overrides.leaf_hole_density?))
        }
    })
}

fn write_override(overrides: &mut TypedMaterialOverrides, field: MaterialField, index: u64) {
    let control = field.control_field();
    let integer = || control_value(control, index).round();
    let decimal = || control_value(control, index) as f32;
    match field {
        MaterialField::Pattern => overrides.pattern = Some(choice_at(index)),
        MaterialField::Placement => overrides.placement = Some(choice_at(index)),
        MaterialField::ClusterShape => overrides.cluster_shape = Some(choice_at(index)),
        MaterialField::OrePattern => overrides.ore_pattern = Some(choice_at(index)),
        MaterialField::ClusterSize => overrides.cluster_size = Some(integer() as usize),
        MaterialField::SmoothingPasses => overrides.smoothing_passes = Some(integer() as usize),
        MaterialField::VariantStrength => overrides.variant_strength = Some(integer() as i16),
        MaterialField::OreBranches => overrides.ore_branches = Some(integer() as usize),
        MaterialField::OreThickness => overrides.ore_thickness = Some(integer() as usize),
        MaterialField::GrassFringeDepth => overrides.grass_fringe_depth = Some(integer() as usize),
        MaterialField::ClusterDensity => overrides.cluster_density = Some(decimal()),
        MaterialField::Contrast => overrides.contrast = Some(decimal()),
        MaterialField::Saturation => overrides.saturation = Some(decimal()),
        MaterialField::Lightness => overrides.lightness = Some(decimal()),
        MaterialField::OreCoverage => overrides.ore_coverage = Some(decimal()),
        MaterialField::OreCenterBias => overrides.ore_center_bias = Some(decimal()),
        MaterialField::LeafHoleDensity => overrides.leaf_hole_density = Some(decimal()),
    }
}

impl PackCodeState {
    /// The generation state of a project built purely by randomizing `seed`.
    pub fn randomized(seed: u64) -> Self {
        Self {
            seed,
            parameters: baseline_parameters(seed),
            material: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests;
