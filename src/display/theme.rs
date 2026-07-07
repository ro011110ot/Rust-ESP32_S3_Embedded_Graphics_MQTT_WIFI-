use core::sync::atomic::{AtomicBool, Ordering};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::RgbColor;

pub(crate) struct Theme {
    pub(crate) bg: Rgb565,
    pub(crate) card: Rgb565,
    pub(crate) border: Rgb565,
    pub(crate) primary: Rgb565,
    pub(crate) secondary: Rgb565,
    pub(crate) warning: Rgb565,
    pub(crate) danger: Rgb565,
    pub(crate) text: Rgb565,
    pub(crate) text_muted: Rgb565,
    pub(crate) nav: Rgb565,
    pub(crate) nav_active: Rgb565,
    pub(crate) nav_inactive: Rgb565,
}

const THEME_DEFAULT: Theme = Theme {
    bg: Rgb565::WHITE,
    card: Rgb565::new(29, 60, 29),
    border: Rgb565::new(24, 48, 24),
    primary: Rgb565::BLUE,
    secondary: Rgb565::new(0, 32, 31),
    warning: Rgb565::new(31, 32, 0),
    danger: Rgb565::RED,
    text: Rgb565::BLACK,
    text_muted: Rgb565::new(10, 20, 10),
    nav: Rgb565::new(28, 58, 28),
    nav_active: Rgb565::new(20, 40, 20),
    nav_inactive: Rgb565::new(28, 58, 28),
};

const THEME_DARK: Theme = Theme {
    bg: Rgb565::BLACK,
    card: Rgb565::new(5, 10, 5),
    border: Rgb565::new(8, 16, 8),
    primary: Rgb565::new(10, 30, 31),
    secondary: Rgb565::new(0, 40, 31),
    warning: Rgb565::YELLOW,
    danger: Rgb565::RED,
    text: Rgb565::WHITE,
    text_muted: Rgb565::new(20, 40, 20),
    nav: Rgb565::new(3, 6, 3),
    nav_active: Rgb565::new(8, 16, 8),
    nav_inactive: Rgb565::new(3, 6, 3),
};

static DARK_MODE: AtomicBool = AtomicBool::new(true);

pub(crate) fn theme() -> &'static Theme {
    if DARK_MODE.load(Ordering::Relaxed) {
        &THEME_DARK
    } else {
        &THEME_DEFAULT
    }
}

pub fn toggle_theme() {
    DARK_MODE.fetch_xor(true, Ordering::Relaxed);
}

pub(crate) fn is_dark_mode() -> bool {
    DARK_MODE.load(Ordering::Relaxed)
}
