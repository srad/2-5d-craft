use super::*;
use crate::{controls::material_fields, pack::validate_metadata};

/// Cheap reproducible stream, independent of the generator under test.
struct TestRng(u64);

impl TestRng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.0 >> 11
    }

    fn below(&mut self, upper: u64) -> u64 {
        self.next() % upper.max(1)
    }
}

fn edited(seed: u64, edits: usize, rng: &mut TestRng) -> PackCodeState {
    let mut state = PackCodeState::randomized(seed);
    for _ in 0..edits {
        let field = ControlField::ALL[rng.below(ControlField::ALL.len() as u64) as usize];
        let index = rng.below(control_cardinality(field));
        write_parameter(&mut state.parameters, field, index);
    }
    state
}

fn with_material(mut state: PackCodeState, blocks: usize, rng: &mut TestRng) -> PackCodeState {
    for block in BlockKind::ALL.into_iter().take(blocks) {
        let applicable = material_fields(block);
        let mut overrides = TypedMaterialOverrides::default();
        for field in applicable {
            if rng.below(2) == 1 {
                let index = rng.below(control_cardinality(field.control_field()));
                write_override(&mut overrides, *field, index);
            }
        }
        if !overrides.is_empty() {
            state.material.insert(block.slug().to_owned(), overrides);
        }
    }
    state
}

fn saturated() -> PackCodeState {
    // Every global driven off its seed baseline and every block overriding every applicable
    // field — the largest state the format can be asked to carry.
    let mut state = PackCodeState::randomized(u64::MAX);
    for field in ControlField::ALL {
        let baseline = read_parameter(&state.parameters, field);
        let last = control_cardinality(field) - 1;
        write_parameter(
            &mut state.parameters,
            field,
            if baseline == last { 0 } else { last },
        );
    }
    for block in BlockKind::ALL {
        let mut overrides = TypedMaterialOverrides::default();
        for field in material_fields(block) {
            write_override(
                &mut overrides,
                *field,
                control_cardinality(field.control_field()) - 1,
            );
        }
        state.material.insert(block.slug().to_owned(), overrides);
    }
    state
}

#[test]
fn a_freshly_randomized_project_encodes_to_the_seed_alone() {
    let mut lengths = Vec::new();
    for seed in [0u64, 1, 42, 4_294_967_295, 1_785_602_118_763_815_816] {
        let state = PackCodeState::randomized(seed);
        let code = encode_pack_code(&state).unwrap();
        assert_eq!(decode_pack_code(&code).unwrap(), state, "seed {seed}");
        lengths.push((seed, code.len()));
    }
    // A 32-bit seed is what `random_seed` now produces; it must beat `generated-<seed>` at 29.
    let (_, typical) = lengths[3];
    assert!(typical <= 12, "32-bit seed produced {typical} characters");
}

#[test]
fn every_mode_round_trips_exactly() {
    let mut rng = TestRng(0x5eed);
    for round in 0..400u64 {
        let seed = rng.next();
        let state = match round % 4 {
            0 => PackCodeState::randomized(seed),
            1 => edited(seed, 1, &mut rng),
            2 => edited(seed, 19, &mut rng),
            _ => with_material(edited(seed, 5, &mut rng), 4, &mut rng),
        };
        let code = encode_pack_code(&state).unwrap();
        let decoded = decode_pack_code(&code).unwrap();
        assert_eq!(decoded, state, "round {round} via {code}");
        // Exact float equality, not approximate: the grid makes this reachable.
        assert_eq!(decoded.parameters, state.parameters);
        assert_eq!(decoded.material, state.material);
    }
}

#[test]
fn the_encoder_picks_the_shortest_representation() {
    let mut rng = TestRng(7);
    let seed = 12_345;
    let untouched = encode_pack_code(&PackCodeState::randomized(seed)).unwrap();
    let one_edit = encode_pack_code(&edited(seed, 1, &mut rng)).unwrap();
    let saturated = encode_pack_code(&saturated()).unwrap();
    assert!(
        untouched.len() < one_edit.len(),
        "seed-only {untouched} should beat a delta {one_edit}"
    );
    assert!(
        one_edit.len() < saturated.len(),
        "a delta {one_edit} should beat a saturated project {saturated}"
    );
}

#[test]
fn the_largest_possible_code_stays_a_usable_folder_name() {
    let state = saturated();
    let code = encode_pack_code(&state).unwrap();
    assert_eq!(decode_pack_code(&code).unwrap(), state);
    assert!(
        code.len() <= 150,
        "saturated project needs {} characters: {code}",
        code.len()
    );
    validate_metadata(&code, "Saturated", "Tests").unwrap();
}

#[test]
fn every_code_is_a_valid_pack_id() {
    let mut rng = TestRng(99);
    for round in 0..60u64 {
        let seed = rng.next();
        let state = with_material(edited(seed, round as usize % 20, &mut rng), 9, &mut rng);
        let code = encode_pack_code(&state).unwrap();
        validate_metadata(&code, "Name", "Author").unwrap();
    }
}

#[test]
fn distinct_projects_never_share_a_code() {
    let mut rng = TestRng(2024);
    let mut seen = std::collections::BTreeMap::new();
    for _ in 0..400 {
        let seed = rng.below(64);
        let state = with_material(edited(seed, 3, &mut rng), 3, &mut rng);
        let code = encode_pack_code(&state).unwrap();
        if let Some(previous) = seen.insert(code.clone(), state.clone()) {
            assert_eq!(previous, state, "{code} names two different projects");
        }
    }
}

#[test]
fn mistyped_and_malformed_codes_are_rejected() {
    let code = encode_pack_code(&PackCodeState::randomized(1_785_602_118)).unwrap();
    assert!(decode_pack_code(&code).is_ok());
    assert!(decode_pack_code(code.trim_start_matches('t')).is_err());
    assert!(decode_pack_code(&format!("{code}0")).is_err());
    assert!(decode_pack_code("t!!!").is_err());
    assert!(decode_pack_code("t").is_err());

    // Flipping any single character must be caught by the checksum or the structure.
    let mut caught = 0;
    let mut total = 0;
    for position in 1..code.len() {
        for replacement in digits::ALPHABET.iter().map(|byte| *byte as char) {
            let mut mistyped = code.clone().into_bytes();
            if mistyped[position] == replacement as u8 {
                continue;
            }
            mistyped[position] = replacement as u8;
            let mistyped = String::from_utf8(mistyped).unwrap();
            total += 1;
            if decode_pack_code(&mistyped).is_err() {
                caught += 1;
            }
        }
    }
    // A 5-bit checksum lets roughly 1 in 32 slip through; anything near that is healthy.
    assert!(
        caught * 100 / total >= 90,
        "only caught {caught} of {total} single-character typos"
    );
}

#[test]
fn material_overrides_on_inapplicable_fields_are_refused() {
    let mut state = PackCodeState::randomized(5);
    // Ore controls do not affect dirt.
    let overrides = TypedMaterialOverrides {
        ore_coverage: Some(0.25),
        ..Default::default()
    };
    state.material.insert("dirt".into(), overrides);
    assert!(encode_pack_code(&state).is_err());

    let mut unknown = PackCodeState::randomized(5);
    unknown
        .material
        .insert("not-a-block".into(), TypedMaterialOverrides::default());
    assert!(encode_pack_code(&unknown).is_err());
}

#[test]
fn a_code_survives_the_full_project_round_trip() {
    // The path the editor actually takes: project -> options -> pack, all reproducible.
    let mut rng = TestRng(31337);
    for _ in 0..20 {
        let state = with_material(edited(rng.next(), 4, &mut rng), 3, &mut rng);
        let code = encode_pack_code(&state).unwrap();
        let decoded = decode_pack_code(&code).unwrap();
        let project = crate::TextureProject {
            project_schema_version: crate::PROJECT_SCHEMA_VERSION,
            generator_version: crate::GENERATOR_VERSION,
            seed: decoded.seed,
            pack: crate::PackSettings {
                name: "Round Trip".into(),
                author: "Tests".into(),
            },
            parameters: decoded.parameters.clone(),
            material: decoded.material.clone(),
        };
        project.validate().unwrap();
        let options = project.to_generate_options().unwrap();
        assert_eq!(
            encode_pack_code(&PackCodeState {
                seed: options.seed,
                parameters: crate::TextureParameters::from_options(&options),
                material: decoded.material,
            })
            .unwrap(),
            code
        );
    }
}
