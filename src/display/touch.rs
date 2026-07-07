use crate::AppState;

use super::theme::{is_dark_mode, toggle_theme};
use super::NAV_TOP;
use super::Screen;

const TOUCH_X_MIN: u16 = 288;
const TOUCH_X_MAX: u16 = 1866;
const TOUCH_Y_MIN: u16 = 230;
const TOUCH_Y_MAX: u16 = 1850;
const TOUCH_X_OFFSET: i32 = 20;
const TOUCH_Y_OFFSET: i32 = 0;

fn read_touch_x(spi: &mut impl embedded_hal::spi::SpiDevice<u8>) -> Option<u16> {
    let mut buf = [0x90u8, 0x00, 0x00, 0x00];
    spi.transfer_in_place(&mut buf).ok()?;
    let raw = ((buf[1] as u16) << 8) | buf[2] as u16;
    Some(raw >> 4)
}

fn read_touch_y(spi: &mut impl embedded_hal::spi::SpiDevice<u8>) -> Option<u16> {
    let mut buf = [0xD0u8, 0x00, 0x00, 0x00];
    spi.transfer_in_place(&mut buf).ok()?;
    let raw = ((buf[1] as u16) << 8) | buf[2] as u16;
    Some(raw >> 4)
}

pub fn read_touch(spi: &mut impl embedded_hal::spi::SpiDevice<u8>) -> Option<(i32, i32)> {
    let raw_x = read_touch_x(spi)?;
    let raw_y = read_touch_y(spi)?;
    if raw_x == 0x0FFF || raw_x == 0 || raw_x == 0x7FF { return None; }
    if raw_y == 0x0FFF || raw_y == 0 || raw_y == 0x7FF { return None; }
    if raw_x <= TOUCH_X_MIN || raw_x >= TOUCH_X_MAX { return None; }
    if raw_y <= TOUCH_Y_MIN || raw_y >= TOUCH_Y_MAX { return None; }

    let px = (TOUCH_Y_MAX - raw_y) as i32 * super::DISP_W as i32
        / (TOUCH_Y_MAX - TOUCH_Y_MIN) as i32 + TOUCH_X_OFFSET;
    let py = (raw_x - TOUCH_X_MIN) as i32 * super::DISP_H as i32
        / (TOUCH_X_MAX - TOUCH_X_MIN) as i32 + TOUCH_Y_OFFSET;
    let px = px.clamp(0, super::DISP_W as i32 - 1);
    let py = py.clamp(0, super::DISP_H as i32 - 1);

    defmt::info!("Touch: raw({},{}) screen({},{})", raw_x, raw_y, px, py);
    Some((px, py))
}

pub(crate) fn handle_theme_toggle(tx: i32, ty: i32, state: &AppState) -> bool {
    if state.read().active_screen != Screen::Weather { return false; }
    if tx >= super::THEME_BTN_X
        && tx < super::THEME_BTN_X + super::THEME_BTN_W
        && ty >= super::THEME_BTN_Y
        && ty < super::THEME_BTN_Y + super::THEME_BTN_H
    {
        toggle_theme();
        defmt::info!("Display: theme toggled (dark={})", is_dark_mode());
        return true;
    }
    false
}

pub fn handle_nav_touch(tx: i32, ty: i32, state: &AppState) -> bool {
    if ty < NAV_TOP { return false; }
    let new_screen = Screen::from_touch_x(tx);
    let current = state.read().active_screen;
    if new_screen != current {
        state.set_active_screen(new_screen);
        defmt::info!("Display: switched to {:?}", new_screen);
        return true;
    }
    false
}
