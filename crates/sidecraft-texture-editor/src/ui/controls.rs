use bevy_egui::egui;
use sidecraft_textures::{
    BlockKind, ControlChoiceValue, ControlDefinition, ControlField, MaterialField,
    TextureParameters, TextureProject, TypedMaterialOverrides, control_definition, material_fields,
    snap_f32,
};

/// Draws the pack panel, returning a code the user pasted in to restore.
pub(super) fn draw_pack_controls(
    ui: &mut egui::Ui,
    project: &mut TextureProject,
    pasted: &mut String,
) -> Option<String> {
    section(ui, "Pack");
    text_row(ui, "Name", &mut project.pack.name);
    text_row(ui, "Author", &mut project.pack.author);
    ui.horizontal(|ui| {
        ui.label("Seed");
        ui.add(
            egui::DragValue::new(&mut project.seed)
                .speed(1.0)
                .range(0..=u64::MAX),
        );
    });

    // The ID is derived, never typed: it is a reversible code for this exact project, so showing
    // it read-only next to a paste box is the whole round trip.
    let code = project.pack_id();
    ui.horizontal(|ui| {
        ui.label("ID");
        match &code {
            Ok(code) => {
                ui.add(egui::Label::new(egui::RichText::new(code).monospace()).wrap());
                if ui.button("Copy").clicked() {
                    ui.ctx().copy_text(code.clone());
                }
            }
            Err(error) => {
                ui.colored_label(egui::Color32::from_rgb(200, 80, 80), error.to_string());
            }
        }
    });
    ui.small("Paste an ID to rebuild the project it names.");
    let mut restore = None;
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(pasted)
                .hint_text("t1…")
                .desired_width(180.0),
        );
        if ui
            .add_enabled(!pasted.trim().is_empty(), egui::Button::new("Load"))
            .clicked()
        {
            restore = Some(pasted.trim().to_owned());
        }
    });
    restore
}

pub(super) fn draw_global_controls(ui: &mut egui::Ui, values: &mut TextureParameters) {
    section(ui, "Global generation");
    choice(ui, &mut values.palette);
    choice(ui, &mut values.pattern);
    choice(ui, &mut values.placement);
    choice(ui, &mut values.cluster_shape);
    usize_control(ui, ControlField::ClusterSize, &mut values.cluster_size);
    f32_control(
        ui,
        ControlField::ClusterDensity,
        &mut values.cluster_density,
    );
    usize_control(
        ui,
        ControlField::SmoothingPasses,
        &mut values.smoothing_passes,
    );
    f32_control(ui, ControlField::Contrast, &mut values.contrast);
    f32_control(ui, ControlField::Saturation, &mut values.saturation);
    f32_control(ui, ControlField::Lightness, &mut values.lightness);
    i16_control(
        ui,
        ControlField::VariantStrength,
        &mut values.variant_strength,
    );
    choice(ui, &mut values.ore_pattern);
    f32_control(ui, ControlField::OreCoverage, &mut values.ore_coverage);
    usize_control(ui, ControlField::OreBranches, &mut values.ore_branches);
    usize_control(ui, ControlField::OreThickness, &mut values.ore_thickness);
    f32_control(ui, ControlField::OreCenterBias, &mut values.ore_center_bias);
    f32_control(
        ui,
        ControlField::LeafHoleDensity,
        &mut values.leaf_hole_density,
    );
    usize_control(
        ui,
        ControlField::GrassFringeDepth,
        &mut values.grass_fringe_depth,
    );
    choice(ui, &mut values.quality);
}

pub(super) fn draw_material_controls(
    ui: &mut egui::Ui,
    project: &mut TextureProject,
    selected: &mut BlockKind,
) {
    section(ui, "Material overrides");
    egui::ComboBox::from_label("Material")
        .selected_text(title(selected.slug()))
        .show_ui(ui, |ui| {
            for block in BlockKind::ALL {
                ui.selectable_value(selected, block, title(block.slug()));
            }
        });
    ui.horizontal(|ui| {
        if ui.button("Reset material").clicked() {
            project.material.remove(selected.slug());
        }
        if ui.button("Reset all").clicked() {
            project.material.clear();
        }
    });
    ui.small("Enable only the values this material should override.");

    let defaults = &project.parameters;
    let overrides = project.material.entry(selected.slug().into()).or_default();
    for field in material_fields(*selected) {
        draw_optional(ui, *field, overrides, defaults);
    }
    if overrides.is_empty() {
        project.material.remove(selected.slug());
    }
}

fn draw_optional(
    ui: &mut egui::Ui,
    field: MaterialField,
    values: &mut TypedMaterialOverrides,
    defaults: &TextureParameters,
) {
    match field {
        MaterialField::Pattern => optional_choice(ui, field, &mut values.pattern, defaults.pattern),
        MaterialField::Placement => {
            optional_choice(ui, field, &mut values.placement, defaults.placement)
        }
        MaterialField::ClusterShape => {
            optional_choice(ui, field, &mut values.cluster_shape, defaults.cluster_shape)
        }
        MaterialField::ClusterSize => {
            optional_usize(ui, field, &mut values.cluster_size, defaults.cluster_size)
        }
        MaterialField::ClusterDensity => optional_f32(
            ui,
            field,
            &mut values.cluster_density,
            defaults.cluster_density,
        ),
        MaterialField::SmoothingPasses => optional_usize(
            ui,
            field,
            &mut values.smoothing_passes,
            defaults.smoothing_passes,
        ),
        MaterialField::Contrast => optional_f32(ui, field, &mut values.contrast, defaults.contrast),
        MaterialField::Saturation => {
            optional_f32(ui, field, &mut values.saturation, defaults.saturation)
        }
        MaterialField::Lightness => {
            optional_f32(ui, field, &mut values.lightness, defaults.lightness)
        }
        MaterialField::VariantStrength => optional_i16(
            ui,
            field,
            &mut values.variant_strength,
            defaults.variant_strength,
        ),
        MaterialField::OrePattern => {
            optional_choice(ui, field, &mut values.ore_pattern, defaults.ore_pattern)
        }
        MaterialField::OreCoverage => {
            optional_f32(ui, field, &mut values.ore_coverage, defaults.ore_coverage)
        }
        MaterialField::OreBranches => {
            optional_usize(ui, field, &mut values.ore_branches, defaults.ore_branches)
        }
        MaterialField::OreThickness => {
            optional_usize(ui, field, &mut values.ore_thickness, defaults.ore_thickness)
        }
        MaterialField::OreCenterBias => optional_f32(
            ui,
            field,
            &mut values.ore_center_bias,
            defaults.ore_center_bias,
        ),
        MaterialField::LeafHoleDensity => optional_f32(
            ui,
            field,
            &mut values.leaf_hole_density,
            defaults.leaf_hole_density,
        ),
        MaterialField::GrassFringeDepth => optional_usize(
            ui,
            field,
            &mut values.grass_fringe_depth,
            defaults.grass_fringe_depth,
        ),
    }
}

fn text_row(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(value);
    });
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);
    ui.heading(title);
    ui.separator();
}

fn choice<T: ControlChoiceValue>(ui: &mut egui::Ui, value: &mut T) {
    let definition = control_definition(T::CONTROL_FIELD);
    egui::ComboBox::from_label(definition.label)
        .selected_text(choice_label(definition, value.value()))
        .show_ui(ui, |ui| {
            for variant in T::values().iter().copied() {
                ui.selectable_value(value, variant, choice_label(definition, variant.value()));
            }
        });
}

fn usize_control(ui: &mut egui::Ui, field: ControlField, value: &mut usize) {
    let definition = control_definition(field);
    ui.add(
        egui::Slider::new(
            value,
            definition.minimum.unwrap() as usize..=definition.maximum.unwrap() as usize,
        )
        .text(definition.label)
        .step_by(definition.step.unwrap()),
    );
}

fn i16_control(ui: &mut egui::Ui, field: ControlField, value: &mut i16) {
    let definition = control_definition(field);
    ui.add(
        egui::Slider::new(
            value,
            definition.minimum.unwrap() as i16..=definition.maximum.unwrap() as i16,
        )
        .text(definition.label)
        .step_by(definition.step.unwrap()),
    );
}

fn f32_control(ui: &mut egui::Ui, field: ControlField, value: &mut f32) {
    let definition = control_definition(field);
    ui.add(
        egui::Slider::new(
            value,
            definition.minimum.unwrap() as f32..=definition.maximum.unwrap() as f32,
        )
        .text(definition.label)
        .step_by(definition.step.unwrap()),
    );
}

fn optional_choice<T: ControlChoiceValue>(
    ui: &mut egui::Ui,
    field: MaterialField,
    value: &mut Option<T>,
    default: T,
) {
    let definition = control_definition(field.control_field());
    debug_assert_eq!(definition.field, T::CONTROL_FIELD);
    let mut enabled = value.is_some();
    ui.horizontal(|ui| {
        if ui.checkbox(&mut enabled, definition.label).changed() {
            *value = enabled.then_some(default);
        }
        if let Some(current) = value {
            egui::ComboBox::from_id_salt(("material", definition.key))
                .selected_text(choice_label(definition, current.value()))
                .show_ui(ui, |ui| {
                    for variant in T::values().iter().copied() {
                        ui.selectable_value(
                            current,
                            variant,
                            choice_label(definition, variant.value()),
                        );
                    }
                });
        }
    });
}

fn optional_usize(
    ui: &mut egui::Ui,
    field: MaterialField,
    value: &mut Option<usize>,
    default: usize,
) {
    let definition = control_definition(field.control_field());
    optional_numeric(ui, definition, value, default, |ui, current| {
        ui.add(
            egui::DragValue::new(current)
                .range(definition.minimum.unwrap() as usize..=definition.maximum.unwrap() as usize)
                .speed(definition.step.unwrap()),
        );
    });
}

fn optional_i16(ui: &mut egui::Ui, field: MaterialField, value: &mut Option<i16>, default: i16) {
    let definition = control_definition(field.control_field());
    optional_numeric(ui, definition, value, default, |ui, current| {
        ui.add(
            egui::DragValue::new(current)
                .range(definition.minimum.unwrap() as i16..=definition.maximum.unwrap() as i16)
                .speed(definition.step.unwrap()),
        );
    });
}

fn optional_f32(ui: &mut egui::Ui, field: MaterialField, value: &mut Option<f32>, default: f32) {
    let definition = control_definition(field.control_field());
    optional_numeric(ui, definition, value, default, |ui, current| {
        ui.add(
            egui::DragValue::new(current)
                .range(definition.minimum.unwrap() as f32..=definition.maximum.unwrap() as f32)
                .speed(definition.step.unwrap()),
        );
        // `speed` is drag sensitivity, not a step: unlike the global sliders this widget will
        // happily produce 1.0637. Snap it, or the value cannot be addressed by a pack code.
        *current = snap_f32(definition.field, *current);
    });
}

fn optional_numeric<T>(
    ui: &mut egui::Ui,
    definition: &ControlDefinition,
    value: &mut Option<T>,
    default: T,
    add_value: impl FnOnce(&mut egui::Ui, &mut T),
) {
    let mut enabled = value.is_some();
    ui.horizontal(|ui| {
        if ui.checkbox(&mut enabled, definition.label).changed() {
            *value = enabled.then_some(default);
        }
        if let Some(current) = value {
            add_value(ui, current);
        }
    });
}

fn choice_label(definition: &ControlDefinition, value: &str) -> &'static str {
    definition
        .choices
        .iter()
        .find(|choice| choice.value == value)
        .map_or("Unknown", |choice| choice.label)
}

fn title(slug: &str) -> String {
    slug.split('_')
        .map(|word| {
            let mut characters = word.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(characters).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}
