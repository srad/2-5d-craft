//! The look of the front end, in one place.
//!
//! The menus used to carry their colours as literals at each spawn site, which
//! is how they drifted into looking generic: flat fills, one uniform border, and
//! font sizes landing on fractional pixels. The retro treatment here is drawn in
//! code rather than in art — a two-tone bevel, a hard unblurred shadow, and a
//! tight palette — so it needs no assets. Every panel styled through these
//! helpers can later swap to authored 9-slice art without touching layout.

use bevy::prelude::*;

// The palette is the game's own, from `crates/sidecraft-textures/src/palette.rs`,
// rather than invented for the UI. The first draft was a cold blue-grey that
// appears nowhere in the art, which is precisely why the menus looked bolted on:
// every block in the world is warm.

/// Deep background behind a panel — the Earthy palette's near-black brown.
pub(super) const INK: Color = Color::srgb_u8(27, 22, 17);
/// Panel fill — dirt, dark.
pub(super) const PANEL: Color = Color::srgb_u8(59, 36, 23);
/// Raised face of a button or row — dirt, mid.
pub(super) const RAISED: Color = Color::srgb_u8(91, 55, 30);
/// Lit edge of a bevel, top and left — wood, light.
pub(super) const LIGHT_EDGE: Color = Color::srgb_u8(126, 75, 34);
/// Shaded edge of a bevel: bottom and right.
pub(super) const DARK_EDGE: Color = Color::srgb_u8(27, 22, 17);
/// Selection and focus accent — the player's own terracotta.
pub(super) const ACCENT: Color = Color::srgb_u8(174, 111, 62);
/// Face of a hovered control.
pub(super) const HOVER: Color = Color::srgb_u8(110, 69, 38);
/// Face of a pressed control, and of the selected row.
pub(super) const PRESSED: Color = Color::srgb_u8(138, 90, 46);
/// Primary text.
pub(super) const TEXT: Color = Color::srgb_u8(232, 220, 200);
/// Secondary text: authors, states, hints.
pub(super) const TEXT_DIM: Color = Color::srgb_u8(160, 138, 106);
/// Status line.
pub(super) const TEXT_WARN: Color = Color::srgb_u8(232, 163, 60);
/// Dims the world behind the pause menu. Warm like `INK` rather than the cold
/// wash it replaced, so the paused world does not shift colour as it recedes.
pub(super) const SCRIM: Color = Color::srgba(0.106, 0.086, 0.067, 0.82);

/// Font sizes, kept whole so VT323's glyphs land on pixel boundaries.
pub(super) const TEXT_SM: f32 = 12.0;
pub(super) const TEXT_MD: f32 = 14.0;
pub(super) const TEXT_LG: f32 = 18.0;
pub(super) const TEXT_XL: f32 = 24.0;

/// Border thickness for every bevelled surface. Thicker than the controls it
/// frames are wide would suggest, on purpose: a heavy edge on a small control is
/// what reads as a pixel grid rather than a thin modern outline.
pub(super) const BEVEL: f32 = 3.0;

/// Control sizes, gathered here so a further tuning pass is a value edit.
pub(super) const BUTTON_W: f32 = 216.0;
pub(super) const BUTTON_H: f32 = 42.0;
pub(super) const COMPACT_W: f32 = 120.0;
pub(super) const COMPACT_H: f32 = 36.0;
pub(super) const PANEL_PAD: f32 = 18.0;
pub(super) const PANEL_GAP: f32 = 6.0;

/// A raised bevel: lit along the top and left, shaded along the bottom and right.
pub(super) fn bevel_raised() -> BorderColor {
    BorderColor {
        top: LIGHT_EDGE,
        left: LIGHT_EDGE,
        bottom: DARK_EDGE,
        right: DARK_EDGE,
    }
}

/// The same bevel inverted, so a surface reads as pushed into the panel. Used for
/// the list viewport and for pressed controls.
pub(super) fn bevel_sunken() -> BorderColor {
    BorderColor {
        top: DARK_EDGE,
        left: DARK_EDGE,
        bottom: LIGHT_EDGE,
        right: LIGHT_EDGE,
    }
}

/// A hard offset shadow. Zero blur is the whole point: a soft shadow reads as
/// modern chrome, a crisp one reads as pixel art.
pub(super) fn pixel_shadow() -> BoxShadow {
    BoxShadow::new(
        Color::srgba(0.0, 0.0, 0.0, 0.55),
        px(4),
        px(4),
        px(0),
        px(0),
    )
}

/// Text styling shorthand, so spawn sites stop repeating the same three fields.
///
/// Every label in the front end goes through here, which is what makes
/// [`FontSmoothing::None`] worth setting in one place: antialiased glyph edges
/// were most of why the menus read as generic rather than pixel-art, whatever the
/// palette. Bevy's own guidance pairs this with `UiAntiAlias::Off` on the camera.
pub(super) fn label(font: &FontSource, size: f32, color: Color) -> (TextFont, TextColor) {
    (
        TextFont {
            font: font.clone(),
            font_size: FontSize::Px(size),
            font_smoothing: FontSmoothing::None,
            ..default()
        },
        TextColor(color),
    )
}
