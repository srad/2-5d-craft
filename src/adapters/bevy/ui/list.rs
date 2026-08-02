//! A scrollable single-selection list, shared by the screens that present a set
//! of things to choose from.
//!
//! Both such screens used to spawn one fixed-size button per entry. That reads as
//! a wall of identical slabs, it cannot scroll, and any label longer than the
//! button wraps and spills out of it — which is exactly what generated texture
//! pack names did. Bevy ships the headless machinery for a proper list box, so
//! the behaviour here is `bevy_ui_widgets`' and only the look is ours.

use bevy::{
    input_focus::AutoFocus,
    prelude::*,
    ui::{Selectable, Selected},
    ui_widgets::{
        ControlOrientation, ListBox, ListItem, ScrollArea, Scrollbar, ScrollbarThumb, ValueChange,
        listbox_update_selection,
    },
};

use super::theme;

/// One entry to display. `key` is how the owning screen recognises the row again
/// when it is chosen — a texture pack id, a world id.
pub(super) struct ListRow {
    pub key: String,
    pub title: String,
    pub detail: String,
    pub selected: bool,
}

/// The row's identity, carried on the entity so the selection observer can report
/// which one was chosen without the screen tracking entity ids itself.
#[derive(Component)]
struct RowKey(String);

/// Raised when a row is chosen, by click or by keyboard.
#[derive(Message, Debug, Clone)]
pub(super) struct RowSelected {
    pub key: String,
}

/// Width of the scrollbar gutter, and the thumb inside it.
const SCROLLBAR_WIDTH: f32 = 12.0;
/// Row height is budgeted from *line* height, not font size: two stacked lines
/// occupy roughly 1.2× their sizes, so a 18px title over a 12px detail needs
/// about 36px before padding. Undershoot this and the second line vanishes
/// without a trace, because the row clips its overflow.
const ROW_HEIGHT: f32 = 40.0;

pub(super) fn spawn_select_list(
    parent: &mut ChildSpawnerCommands,
    font: &FontSource,
    rows: &[ListRow],
    visible_rows: usize,
) {
    let height = ROW_HEIGHT * visible_rows as f32;
    parent
        .spawn(Node {
            width: percent(100),
            height: px(height),
            flex_direction: FlexDirection::Row,
            ..default()
        })
        .with_children(|frame| {
            let viewport = frame
                .spawn((
                    Node {
                        flex_grow: 1.0,
                        height: percent(100),
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::scroll_y(),
                        border: UiRect::all(px(theme::BEVEL)),
                        ..default()
                    },
                    BackgroundColor(theme::INK),
                    theme::bevel_sunken(),
                    // `ScrollArea` handles the wheel; `ListBox` handles selection
                    // and arrow keys. `AutoFocus` is what makes the arrow keys
                    // reach it at all — the list only reacts to focused input, it
                    // never claims focus itself.
                    ScrollArea,
                    ListBox,
                    AutoFocus,
                ))
                .with_children(|list| {
                    for row in rows {
                        spawn_row(list, font, row);
                    }
                })
                .observe(listbox_update_selection)
                .observe(report_selection)
                .id();

            frame
                .spawn((
                    Node {
                        width: px(SCROLLBAR_WIDTH),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(theme::INK),
                    Scrollbar {
                        target: viewport,
                        orientation: ControlOrientation::Vertical,
                        min_thumb_length: 24.0,
                    },
                ))
                .with_child((
                    // Deliberately no `Node`: the thumb's size and position are
                    // written after layout by the scrollbar's own system, which
                    // only reads the border styling from here.
                    ScrollbarThumb {
                        border: UiRect::all(px(theme::BEVEL)),
                        ..default()
                    },
                    BackgroundColor(theme::RAISED),
                    theme::bevel_raised(),
                ));
        });
}

fn spawn_row(parent: &mut ChildSpawnerCommands, font: &FontSource, row: &ListRow) {
    let (font_title, color_title) = theme::label(font, theme::TEXT_LG, theme::TEXT);
    let (font_detail, color_detail) = theme::label(font, theme::TEXT_SM, theme::TEXT_DIM);
    let mut entity = parent.spawn((
        // No `Button` here on purpose: `style_buttons` repaints anything with one
        // whenever its `Interaction` changes, which would wipe the selected
        // highlight as soon as the pointer left the row.
        ListItem,
        Selectable,
        RowKey(row.key.clone()),
        Node {
            width: percent(100),
            height: px(ROW_HEIGHT),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            padding: UiRect::axes(px(8), px(2)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(row_color(row.selected)),
    ));
    entity.with_children(|row_node| {
        row_node.spawn((Text::new(row.title.clone()), font_title, color_title));
        if !row.detail.is_empty() {
            row_node.spawn((Text::new(row.detail.clone()), font_detail, color_detail));
        }
    });
    if row.selected {
        entity.insert(Selected);
    }
    entity
        .observe(row_pointer_over)
        .observe(row_pointer_out)
        .observe(row_pointer_click);
}

fn row_color(selected: bool) -> Color {
    if selected {
        theme::PRESSED
    } else {
        theme::PANEL
    }
}

/// Translates the list box's entity-level notification into something the screens
/// can act on without knowing which entity is which row.
fn report_selection(
    change: On<ValueChange<Entity>>,
    rows: Query<&RowKey>,
    mut selected: MessageWriter<RowSelected>,
) {
    if let Ok(key) = rows.get(change.event().value) {
        selected.write(RowSelected { key: key.0.clone() });
    }
}

fn row_pointer_over(
    over: On<Pointer<Over>>,
    mut rows: Query<&mut BackgroundColor, With<ListItem>>,
) {
    if let Ok(mut color) = rows.get_mut(over.entity) {
        color.0 = theme::HOVER;
    }
}

fn row_pointer_out(
    out: On<Pointer<Out>>,
    mut rows: Query<(&mut BackgroundColor, Has<Selected>), With<ListItem>>,
) {
    if let Ok((mut color, selected)) = rows.get_mut(out.entity) {
        color.0 = row_color(selected);
    }
}

/// The hover tint outlives the click that selects a row, so repaint every row
/// once the selection has settled.
fn row_pointer_click(
    _click: On<Pointer<Click>>,
    mut rows: Query<(&mut BackgroundColor, Has<Selected>), With<ListItem>>,
) {
    for (mut color, selected) in &mut rows {
        color.0 = row_color(selected);
    }
}

/// Shortens a label to fit, so a long name is cut with an ellipsis rather than
/// wrapping out of its row.
pub(super) fn ellipsize(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let kept: String = text.chars().take(limit.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_labels_are_left_alone() {
        assert_eq!(ellipsize("Dust", 10), "Dust");
        assert_eq!(ellipsize("exactly-10", 10), "exactly-10");
    }

    #[test]
    fn long_labels_are_cut_to_the_limit() {
        // The name that started this: it used to wrap out of a 38-pixel button.
        let cut = ellipsize("Generated 1785602118763815816", 16);
        assert_eq!(cut.chars().count(), 16);
        assert!(cut.ends_with('…'), "{cut}");
    }
}
