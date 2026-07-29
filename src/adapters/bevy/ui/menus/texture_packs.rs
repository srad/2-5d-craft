use bevy::prelude::*;

use super::spawn_title;
use crate::{
    AppState,
    adapters::bevy::{
        textures::{TexturePackCatalog, TexturePackChanged, TexturePackPreview},
        ui::{
            UiAction, UiFont, UiStatus, front_end_root_node, menu_panel_node, spawn_compact_button,
            spawn_status, spawn_version,
        },
    },
};

const PAGE_SIZE: usize = 4;

#[derive(Component)]
struct TexturePackRoot;

#[derive(Resource, Default)]
struct TexturePackMenuState {
    selected: String,
    page: usize,
    rebuild: bool,
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<TexturePackMenuState>()
        .add_systems(OnEnter(AppState::TexturePacks), enter_texture_packs)
        .add_systems(OnExit(AppState::TexturePacks), restore_active_pack)
        .add_systems(
            Update,
            (
                handle_texture_pack_actions,
                rebuild_texture_pack_menu.after(handle_texture_pack_actions),
            ),
        );
}

fn enter_texture_packs(
    mut commands: Commands,
    ui_font: Res<UiFont>,
    mut texture_packs: ResMut<TexturePackCatalog>,
    mut preview: ResMut<TexturePackPreview>,
    mut menu: ResMut<TexturePackMenuState>,
    mut status: ResMut<UiStatus>,
) {
    texture_packs.refresh();
    menu.selected = texture_packs.active_id().to_owned();
    menu.page = 0;
    menu.rebuild = false;
    preview.0 = texture_packs.active.clone();
    if !texture_packs.diagnostic.is_empty() {
        status.0.clone_from(&texture_packs.diagnostic);
    }
    spawn_texture_pack_view(&mut commands, &ui_font, &texture_packs, &menu);
}

fn restore_active_pack(
    texture_packs: Res<TexturePackCatalog>,
    mut preview: ResMut<TexturePackPreview>,
) {
    preview.0 = texture_packs.active.clone();
}

fn rebuild_texture_pack_menu(
    mut commands: Commands,
    roots: Query<Entity, With<TexturePackRoot>>,
    ui_font: Res<UiFont>,
    texture_packs: Res<TexturePackCatalog>,
    mut menu: ResMut<TexturePackMenuState>,
) {
    if !menu.rebuild {
        return;
    }
    for entity in &roots {
        commands.entity(entity).despawn();
    }
    menu.page = menu.page.min(page_count(&texture_packs) - 1);
    menu.rebuild = false;
    spawn_texture_pack_view(&mut commands, &ui_font, &texture_packs, &menu);
}

fn spawn_texture_pack_view(
    commands: &mut Commands,
    ui_font: &UiFont,
    texture_packs: &TexturePackCatalog,
    menu: &TexturePackMenuState,
) {
    let font = ui_font.0.clone();
    let pages = page_count(texture_packs);
    let start = menu.page.min(pages - 1) * PAGE_SIZE;
    commands
        .spawn((
            front_end_root_node(),
            TexturePackRoot,
            DespawnOnExit(AppState::TexturePacks),
        ))
        .with_children(|root| {
            root.spawn(menu_panel_node(520.0)).with_children(|panel| {
                spawn_title(panel, &font, "TEXTURE PACKS", 48.0);
                for pack in texture_packs.packs.iter().skip(start).take(PAGE_SIZE) {
                    let selected = pack.id == menu.selected;
                    let active = pack.id == texture_packs.active_id();
                    let state = if pack.validation_error.is_some() {
                        "INVALID"
                    } else if active {
                        "ACTIVE"
                    } else if selected {
                        "PREVIEW"
                    } else {
                        "READY"
                    };
                    spawn_compact_button(
                        panel,
                        &font,
                        &format!("{} — {} [{state}]", pack.name, pack.author),
                        UiAction::SelectTexturePack(pack.id.clone()),
                    );
                }
                panel.spawn((
                    Text::new(format!("PAGE {} / {pages}", menu.page + 1)),
                    TextFont {
                        font: font.clone(),
                        font_size: FontSize::Px(18.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.66, 0.76, 0.69)),
                ));
                if menu.page > 0 {
                    spawn_compact_button(panel, &font, "PREVIOUS", UiAction::TexturePackPage(-1));
                }
                if menu.page + 1 < pages {
                    spawn_compact_button(panel, &font, "NEXT", UiAction::TexturePackPage(1));
                }
                spawn_compact_button(panel, &font, "APPLY", UiAction::ApplyTexturePack);
                spawn_compact_button(panel, &font, "BACK", UiAction::ShowSettings);
                spawn_status(panel, &font);
            });
            spawn_version(root, &font);
        });
}

fn page_count(texture_packs: &TexturePackCatalog) -> usize {
    texture_packs.packs.len().div_ceil(PAGE_SIZE).max(1)
}

fn handle_texture_pack_actions(
    interactions: Query<(&Interaction, &UiAction), Changed<Interaction>>,
    mut pack_changes: MessageWriter<TexturePackChanged>,
    mut texture_packs: ResMut<TexturePackCatalog>,
    mut preview: ResMut<TexturePackPreview>,
    mut menu: ResMut<TexturePackMenuState>,
    mut status: ResMut<UiStatus>,
) {
    for (interaction, action) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match action {
            UiAction::SelectTexturePack(id) => match texture_packs.resolve(id) {
                Ok(pack) => {
                    preview.0 = pack;
                    menu.selected.clone_from(id);
                    menu.rebuild = true;
                    status.0.clear();
                }
                Err(error) => status.0 = format!("Could not preview texture pack: {error}"),
            },
            UiAction::TexturePackPage(offset) => {
                menu.page = if *offset < 0 {
                    menu.page.saturating_sub(offset.unsigned_abs() as usize)
                } else {
                    menu.page.saturating_add(*offset as usize)
                };
                menu.rebuild = true;
            }
            UiAction::ApplyTexturePack => {
                let selected = menu.selected.clone();
                match texture_packs.apply(&selected) {
                    Ok(()) => {
                        preview.0 = texture_packs.active.clone();
                        pack_changes.write(TexturePackChanged);
                        status.0 = format!("Applied {}", texture_packs.active.manifest.name);
                        menu.rebuild = true;
                    }
                    Err(error) => status.0 = format!("Could not apply texture pack: {error}"),
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_always_has_a_page() {
        assert_eq!(0_usize.div_ceil(PAGE_SIZE).max(1), 1);
        assert_eq!(PAGE_SIZE.div_ceil(PAGE_SIZE), 1);
        assert_eq!((PAGE_SIZE + 1).div_ceil(PAGE_SIZE), 2);
    }

    #[test]
    fn texture_pack_actions_stay_out_of_world_sessions() {
        assert!(
            UiAction::SelectTexturePack("default".into())
                .session_command()
                .is_none()
        );
        assert!(UiAction::ApplyTexturePack.session_command().is_none());
    }
}
