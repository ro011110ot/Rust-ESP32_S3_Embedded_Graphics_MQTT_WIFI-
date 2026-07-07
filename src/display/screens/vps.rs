use alloc::format;
use embedded_graphics::{
    mono_font::ascii::{FONT_6X10, FONT_9X15},
    pixelcolor::Rgb565,
    prelude::*,
};

use crate::AppState;

use crate::display::theme;
use crate::display::{draw_card, draw_progress_bar, draw_text, fmt_0dp_w, format_uptime};

pub fn draw_vps_screen<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, state: &AppState,
) -> Result<(), D::Error> {
    draw_text(
        display, "VPS Status", 78, 26 + crate::display::CONTENT_Y + crate::display::TITLE_Y_INC,
        &FONT_9X15, theme::theme().primary,
    )?;
    let data = state.read();
    let (cpu, ram, disk, uptime_secs) = if let Some(v) = data.vps {
        (v.cpu_pct, v.ram_pct, v.disk_pct, v.uptime_secs)
    } else {
        (0.0, 0.0, 0.0, 0u64)
    };

    draw_card(display, 5, 46 + crate::display::CONTENT_Y, 230, 52)?;
    draw_text(
        display, "CPU Auslastung", 12, 50 + crate::display::CONTENT_Y,
        &FONT_6X10, theme::theme().text_muted,
    )?;
    draw_text(
        display, &format!("{}%", fmt_0dp_w(cpu)),
        200, 50 + crate::display::CONTENT_Y, &FONT_6X10, theme::theme().warning,
    )?;
    draw_progress_bar(
        display, 16, 68 + crate::display::CONTENT_Y, 200, 12, cpu as u8, theme::theme().warning,
    )?;

    draw_card(display, 5, 106 + crate::display::CONTENT_Y, 230, 52)?;
    draw_text(
        display, "RAM Auslastung", 12, 110 + crate::display::CONTENT_Y,
        &FONT_6X10, theme::theme().text_muted,
    )?;
    draw_text(
        display, &format!("{}%", fmt_0dp_w(ram)),
        200, 110 + crate::display::CONTENT_Y, &FONT_6X10, theme::theme().primary,
    )?;
    draw_progress_bar(
        display, 16, 128 + crate::display::CONTENT_Y, 200, 12, ram as u8, theme::theme().primary,
    )?;

    draw_card(display, 5, 166 + crate::display::CONTENT_Y, 230, 52)?;
    draw_text(
        display, "Speicher", 12, 170 + crate::display::CONTENT_Y,
        &FONT_6X10, theme::theme().text_muted,
    )?;
    draw_text(
        display, &format!("{}%", fmt_0dp_w(disk)),
        200, 170 + crate::display::CONTENT_Y, &FONT_6X10, theme::theme().secondary,
    )?;
    draw_progress_bar(
        display, 16, 188 + crate::display::CONTENT_Y, 200, 12,
        disk as u8, theme::theme().secondary,
    )?;

    draw_card(display, 5, 226 + crate::display::CONTENT_Y, 230, 44)?;
    draw_text(
        display, "System Uptime", 12, 230 + crate::display::CONTENT_Y,
        &FONT_6X10, theme::theme().text_muted,
    )?;
    let uptime_str = format_uptime(uptime_secs);
    draw_text(
        display, &uptime_str, 12, 248 + crate::display::CONTENT_Y,
        &FONT_6X10, theme::theme().text,
    )?;
    Ok(())
}
