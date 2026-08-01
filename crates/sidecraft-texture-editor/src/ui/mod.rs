mod controls;

use crate::{
    document::EditorDocument,
    file_ops::{
        Confirmation, EditorCommand, EditorCommands, EditorStatus, ExportCoordinator, PendingAction,
    },
    generation::GenerationCoordinator,
    preview::{PreviewLighting, PreviewViewport},
};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use sidecraft_textures::{BlockKind, random_seed};

#[derive(Resource)]
pub(crate) struct EditorUiState {
    selected_material: BlockKind,
}

impl Default for EditorUiState {
    fn default() -> Self {
        Self {
            selected_material: BlockKind::Stone,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_editor(
    mut contexts: EguiContexts,
    mut document: ResMut<EditorDocument>,
    mut generation: ResMut<GenerationCoordinator>,
    export: Res<ExportCoordinator>,
    status: Res<EditorStatus>,
    mut commands: ResMut<EditorCommands>,
    mut confirmation: ResMut<Confirmation>,
    mut viewport: ResMut<PreviewViewport>,
    mut preview_lighting: ResMut<PreviewLighting>,
    mut ui_state: ResMut<EditorUiState>,
) {
    let Ok(context) = contexts.ctx_mut() else {
        return;
    };
    configure_style(context);
    let locked = export.is_active();
    let mut viewport_ui = egui::Ui::new(
        context.clone(),
        "editor-viewport".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(context.viewport_rect()),
    );

    egui::Panel::top("toolbar")
        .exact_size(48.0)
        .show(&mut viewport_ui, |ui| {
            ui.add_enabled_ui(!locked, |ui| {
                ui.horizontal_centered(|ui| {
                    if ui.button("New").clicked() {
                        request_destructive(
                            PendingAction::New,
                            &document,
                            &mut confirmation,
                            &mut commands,
                        );
                    }
                    if ui.button("Open").clicked() {
                        request_destructive(
                            PendingAction::Open,
                            &document,
                            &mut confirmation,
                            &mut commands,
                        );
                    }
                    if ui.button("Save").clicked() {
                        commands.0.push_back(EditorCommand::Save {
                            choose_path: false,
                            then: None,
                        });
                    }
                    if ui.button("Save As").clicked() {
                        commands.0.push_back(EditorCommand::Save {
                            choose_path: true,
                            then: None,
                        });
                    }
                    ui.separator();
                    if ui.button("Regenerate").clicked() {
                        generation.request_now(document.revision);
                    }
                    if ui.button("New Seed").clicked() {
                        document.project.seed = random_seed();
                        document.mark_changed();
                        generation.request(document.revision);
                    }
                    if ui
                        .add(
                            egui::Button::new("Generate")
                                .fill(egui::Color32::from_rgb(135, 92, 52)),
                        )
                        .clicked()
                    {
                        document.generate_variation(random_seed());
                        generation.request_now(document.revision);
                    }
                    ui.separator();
                    let can_export = generation.can_export(document.revision);
                    if ui
                        .add_enabled(can_export, egui::Button::new("Export Pack"))
                        .clicked()
                    {
                        commands.0.push_back(EditorCommand::Export);
                    }
                });
            });
        });

    egui::Panel::bottom("status")
        .exact_size(32.0)
        .show(&mut viewport_ui, |ui| {
            ui.horizontal(|ui| {
                let path = document.path.as_ref().map_or_else(
                    || "Unsaved project".into(),
                    |path| path.display().to_string(),
                );
                ui.label(if document.is_dirty() {
                    format!("{path}  •  Modified")
                } else {
                    path
                });
                ui.separator();
                if export.is_active() {
                    ui.label("Exporting…");
                } else if generation.is_generating() {
                    ui.label("Generating preview…");
                } else if let Some(error) = &generation.error {
                    ui.colored_label(egui::Color32::from_rgb(224, 112, 96), error);
                } else if let Some(message) = &status.message {
                    ui.label(message);
                } else {
                    ui.label("Ready");
                }
                if let Some(path) = &status.last_export {
                    ui.separator();
                    ui.label(format!("Last export: {}", path.display()));
                }
            });
        });

    egui::Panel::left("parameters")
        .default_size(440.0)
        .size_range(360.0..=560.0)
        .resizable(true)
        .show(&mut viewport_ui, |ui| {
            ui.add_enabled_ui(!locked, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let before = document.project.clone();
                    controls::draw_pack_controls(ui, &mut document.project);
                    controls::draw_global_controls(ui, &mut document.project.parameters);
                    controls::draw_material_controls(
                        ui,
                        &mut document.project,
                        &mut ui_state.selected_material,
                    );
                    if document.project != before {
                        document.mark_changed();
                        generation.request(document.revision);
                    }
                });
            });
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(&mut viewport_ui, |ui| {
            viewport.logical_rect = Some(ui.max_rect());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut *preview_lighting, PreviewLighting::Night, "Night");
                    ui.selectable_value(&mut *preview_lighting, PreviewLighting::Day, "Day");
                });
            });
            ui.allocate_rect(ui.max_rect(), egui::Sense::hover());
        });

    if let Some(action) = confirmation.0 {
        show_confirmation(context, action, &mut confirmation, &mut commands);
    }
}

fn configure_style(context: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(31, 29, 27);
    visuals.window_fill = egui::Color32::from_rgb(38, 35, 32);
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(55, 50, 45);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(77, 67, 55);
    visuals.selection.bg_fill = egui::Color32::from_rgb(135, 92, 52);
    context.set_visuals(visuals);
    let mut style = (*context.style_of(egui::Theme::Dark)).clone();
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(16.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(21.0));
    style.spacing.interact_size = egui::vec2(48.0, 30.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.slider_width = 220.0;
    style.spacing.combo_width = 180.0;
    style.spacing.text_edit_width = 260.0;
    context.set_style_of(egui::Theme::Dark, style);
}

fn request_destructive(
    action: PendingAction,
    document: &EditorDocument,
    confirmation: &mut Confirmation,
    commands: &mut EditorCommands,
) {
    if document.is_dirty() {
        confirmation.0 = Some(action);
    } else {
        commands.0.push_back(EditorCommand::Perform(action));
    }
}

fn show_confirmation(
    context: &egui::Context,
    action: PendingAction,
    confirmation: &mut Confirmation,
    commands: &mut EditorCommands,
) {
    egui::Window::new("Unsaved changes")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(context, |ui| {
            ui.label("Save the current project before continuing?");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    confirmation.0 = None;
                    commands.0.push_back(EditorCommand::Save {
                        choose_path: false,
                        then: Some(action),
                    });
                }
                if ui.button("Discard").clicked() {
                    confirmation.0 = None;
                    commands.0.push_back(EditorCommand::Perform(action));
                }
                if ui.button("Cancel").clicked() {
                    confirmation.0 = None;
                }
            });
        });
}
