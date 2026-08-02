use bevy::prelude::*;
use sidecraft_textures::TexturePackSummary;

use super::spawn_title;
use crate::{
    AppState,
    adapters::bevy::{
        textures::{TexturePackCatalog, TexturePackChanged, TexturePackPreview},
        ui::{
            UiAction, UiFont, UiStatus, button_row_node, front_end_root_node,
            list::{ListRow, RowSelected, ellipsize, spawn_select_list},
            menu_panel_node, spawn_compact_button, spawn_status, spawn_version, theme,
        },
    },
};

/// How many rows are visible before the list scrolls, and how much of a name fits
/// on a row before it is cut.
const VISIBLE_ROWS: usize = 5;
const TITLE_LIMIT: usize = 30;

#[derive(Component)]
struct TexturePackRoot;

#[derive(Resource, Default)]
struct TexturePackMenuState {
    selected: String,
    rebuild: bool,
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<TexturePackMenuState>()
        .add_systems(OnEnter(AppState::TexturePacks), enter_texture_packs)
        .add_systems(OnExit(AppState::TexturePacks), restore_active_pack)
        .add_systems(
            Update,
            (
                handle_row_selection.run_if(in_state(AppState::TexturePacks)),
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
    let rows = pack_rows(texture_packs, &menu.selected);
    commands
        .spawn((
            front_end_root_node(),
            TexturePackRoot,
            DespawnOnExit(AppState::TexturePacks),
        ))
        .with_children(|root| {
            root.spawn(menu_panel_node(440.0)).with_children(|panel| {
                spawn_title(panel, &font, "TEXTURE PACKS", 32.0);
                spawn_select_list(panel, &font, &rows, VISIBLE_ROWS);
                if needs_empty_hint(&texture_packs.packs) {
                    let root = texture_packs.custom_root();
                    let shown = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
                    panel.spawn((
                        Text::new(format!(
                            "No user packs in {}\nExport one there from the texture editor.",
                            shown.display()
                        )),
                        theme::label(&font, theme::TEXT_SM, theme::TEXT_DIM),
                    ));
                }
                panel.spawn(button_row_node()).with_children(|actions| {
                    spawn_compact_button(actions, &font, "APPLY", UiAction::ApplyTexturePack);
                    spawn_compact_button(actions, &font, "BACK", UiAction::ShowSettings);
                });
                spawn_status(panel, &font);
            });
            spawn_version(root, &font);
        });
}

/// Whether the catalogue holds nothing but the built-in pack.
///
/// A one-entry list reads as though the default were locked, when in truth there
/// is simply nowhere for a second pack to have come from. Discovery skips a
/// missing or empty user folder without a word, so the screen has to say it.
fn needs_empty_hint(packs: &[TexturePackSummary]) -> bool {
    packs.len() <= 1
}

/// Builds the list rows. The name goes on its own line and the author and state
/// on a dimmer second line: cramming all three into one label is what used to
/// overflow the row for names like `Generated 1785602118763815816`.
fn pack_rows(texture_packs: &TexturePackCatalog, selected: &str) -> Vec<ListRow> {
    pack_rows_from(&texture_packs.packs, texture_packs.active_id(), selected)
}

/// Split from `pack_rows` so the row shaping can be tested without standing up a
/// whole catalogue, which owns a fully resolved pack.
fn pack_rows_from(packs: &[TexturePackSummary], active: &str, selected: &str) -> Vec<ListRow> {
    packs
        .iter()
        .map(|pack| ListRow {
            key: pack.id.clone(),
            title: ellipsize(&pack.name, TITLE_LIMIT),
            detail: format!(
                "{} · {}",
                ellipsize(&pack.author, TITLE_LIMIT),
                pack_state(pack, active)
            ),
            selected: pack.id == selected,
        })
        .collect()
}

/// The row's standing. There is deliberately no `PREVIEW` state: selecting a row
/// no longer rebuilds the list — that would throw away the scroll position — so a
/// tag saying which row is selected would go stale the moment it mattered. The
/// selection highlight carries that already.
fn pack_state(pack: &TexturePackSummary, active: &str) -> &'static str {
    if pack.validation_error.is_some() {
        "INVALID"
    } else if pack.id == active {
        "ACTIVE"
    } else {
        "READY"
    }
}

/// Previews whichever pack the list reports as chosen.
///
/// Gated on the screen's state: the list widget is shared, so WORLD SELECT raises
/// the same message with a world id in it.
fn handle_row_selection(
    mut selections: MessageReader<RowSelected>,
    texture_packs: Res<TexturePackCatalog>,
    mut preview: ResMut<TexturePackPreview>,
    mut menu: ResMut<TexturePackMenuState>,
    mut status: ResMut<UiStatus>,
) {
    for selection in selections.read() {
        match texture_packs.resolve(&selection.key) {
            Ok(pack) => {
                preview.0 = pack;
                menu.selected.clone_from(&selection.key);
                status.0.clear();
            }
            Err(error) => status.0 = format!("Could not preview texture pack: {error}"),
        }
    }
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
        // Selecting a pack now arrives as a `RowSelected` message from the list,
        // so applying is the only button this screen still owns.
        if !matches!(action, UiAction::ApplyTexturePack) {
            continue;
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(id: &str) -> TexturePackSummary {
        TexturePackSummary {
            id: id.to_owned(),
            name: id.to_owned(),
            author: "Test".into(),
            path: std::path::PathBuf::from(id),
            validation_error: None,
        }
    }

    #[test]
    fn the_hint_appears_only_while_the_default_stands_alone() {
        assert!(needs_empty_hint(&[summary("default")]));
        assert!(!needs_empty_hint(&[summary("default"), summary("mine")]));
    }

    #[test]
    fn texture_pack_actions_stay_out_of_world_sessions() {
        assert!(UiAction::ApplyTexturePack.session_command().is_none());
    }

    /// The overflow came from packing name, author and state into one label. The
    /// name now stands alone on the row's first line, which is what gives it the
    /// width to fit — `Generated 1785602118763815816` is 29 characters and needs
    /// no cutting at all once it is not sharing the line.
    #[test]
    fn rows_lead_with_the_name_and_keep_the_author_and_state_apart() {
        let mut packs = vec![summary("default"), summary("mine")];
        packs[1].name = "Generated 1785602118763815816".into();
        packs[1].author = "Sidecraft texture generator".into();
        let rows = pack_rows_from(&packs, "default", "default");

        assert_eq!(rows[0].key, "default");
        assert!(rows[0].selected, "the active pack starts selected");
        assert!(rows[0].detail.contains("ACTIVE"), "{}", rows[0].detail);

        assert_eq!(rows[1].title, "Generated 1785602118763815816");
        assert!(
            !rows[1].title.contains("Sidecraft"),
            "the author belongs on the second line, not crammed into the name"
        );
        assert!(rows[1].detail.contains("READY"), "{}", rows[1].detail);
        assert!(!rows[1].selected);
    }

    #[test]
    fn a_name_beyond_the_row_width_is_cut() {
        let mut packs = vec![summary("wordy")];
        packs[0].name = "A Pack Name Far Longer Than Any Row Could Show".into();
        let rows = pack_rows_from(&packs, "none", "none");

        assert_eq!(rows[0].title.chars().count(), TITLE_LIMIT);
        assert!(rows[0].title.ends_with('…'), "{}", rows[0].title);
    }
}
