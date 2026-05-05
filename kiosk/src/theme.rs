use bevy::color::Oklcha;
use bevy::prelude::Color;

use kiosk::KioskTheme;

pub struct ThemeColors {
    pub bg: Color,
    pub card_bg: Color,
    pub divider: Color,
    pub content: Color,
    pub dim: Color,
}

pub fn resolve(t: &KioskTheme) -> ThemeColors {
    // bg: (lightness, chroma, hue) — directly from neiam-saver
    let (bg_l, bg_c, hue) = match t {
        KioskTheme::Her       => (0.35, 0.104, 24.0),
        KioskTheme::AfterDark => (0.29, 0.122, 277.0),
        KioskTheme::Forest    => (0.27, 0.063, 153.0),
        KioskTheme::Sky       => (0.29, 0.063, 243.0),
        KioskTheme::Clays     => (0.28, 0.074, 46.0),
        KioskTheme::Stones    => (0.27, 0.006, 34.0),
    };

    // card_bg and divider are derived by stepping the lightness up from bg.
    // card_bg: +0.07 L, slightly less chroma
    // divider: +0.12 L, less chroma (subtle boundary line)
    let dim = match t {
        KioskTheme::Her       => Color::from(Oklcha::new(0.50, 0.076, 22.0,  1.0)),
        KioskTheme::AfterDark => Color::from(Oklcha::new(0.48, 0.090, 285.0, 1.0)),
        KioskTheme::Forest    => Color::from(Oklcha::new(0.52, 0.063, 153.0, 1.0)),
        KioskTheme::Sky       => Color::from(Oklcha::new(0.50, 0.063, 243.0, 1.0)),
        KioskTheme::Clays     => Color::from(Oklcha::new(0.50, 0.074, 46.0,  1.0)),
        KioskTheme::Stones    => Color::from(Oklcha::new(0.44, 0.006, 34.0,  1.0)),
    };

    ThemeColors {
        bg:      Color::from(Oklcha::new(bg_l,        bg_c,        hue, 1.0)),
        card_bg: Color::from(Oklcha::new(bg_l + 0.07, bg_c * 0.80, hue, 1.0)),
        divider: Color::from(Oklcha::new(bg_l + 0.12, bg_c * 0.60, hue, 1.0)),
        content: Color::from(Oklcha::new(0.98,        0.050,       hue, 1.0)),
        dim,
    }
}
