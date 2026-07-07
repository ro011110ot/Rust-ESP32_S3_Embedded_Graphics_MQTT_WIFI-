use alloc::format;
use embedded_graphics::{
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::Rectangle,
};
use embedded_text::alignment::{HorizontalAlignment, VerticalAlignment};
use profont::PROFONT_12_POINT;

use crate::AppState;

use crate::display::theme;
use crate::display::{
    draw_card, draw_progress_bar, draw_textbox, fmt_0dp_w, format_uptime, CONTENT_Y,
};

// --- Layout constants (relative to CONTENT_Y) ---
const TITLE_H: i32 = 26;
const CARD_H: i32 = 46;
const CARD_GAP: i32 = 4;
const CARD_W: i32 = 230;
const CARD_X: i32 = 5;
const UPTIME_CARD_H: i32 = 44;

pub fn draw_vps_screen<D: DrawTarget<Color=Rgb565>>(
    display: &mut D, state: &AppState,
) -> Result<(), D::Error> {
    // --- Header card ---
    let hdr_y = CONTENT_Y + 32;
    draw_card(display, CARD_X, hdr_y, CARD_W, TITLE_H)?;
    draw_textbox(
        display, "VPS Status",
        Rectangle::new(Point::new(CARD_X + 6, hdr_y), Size::new(CARD_W as u32 - 12, TITLE_H as u32)),
        &PROFONT_12_POINT, theme::theme().primary,
        HorizontalAlignment::Center, VerticalAlignment::Middle,
    )?;

    let data = state.read();
    let (cpu, ram, disk, uptime_secs) = if let Some(v) = data.vps {
        (v.cpu_pct, v.ram_pct, v.disk_pct, v.uptime_secs)
    } else {
        (0.0, 0.0, 0.0, 0u64)
    };

    // We use `base_y + i * (card_h + gap)` for uniform card spacing.
    let cy = |i: i32| hdr_y + TITLE_H + CARD_GAP + i * (CARD_H + CARD_GAP);

    // --- Card 0: CPU ---
    draw_card(display, CARD_X, cy(0), CARD_W, CARD_H)?;
    draw_textbox(
        display, "CPU Auslastung",
        Rectangle::new(Point::new(16, cy(0) + 2), Size::new(130, 18)),
        &PROFONT_12_POINT, theme::theme().text_muted,
        HorizontalAlignment::Left, VerticalAlignment::Middle,
    )?;
    draw_textbox(
        display, &format!("{}%", fmt_0dp_w(cpu)),
        Rectangle::new(Point::new(160, cy(0) + 2), Size::new(68, 18)),
        &PROFONT_12_POINT, theme::theme().warning,
        HorizontalAlignment::Right, VerticalAlignment::Middle,
    )?;
    draw_progress_bar(display, 16, cy(0) + 24, 200, 12, cpu as u8, theme::theme().warning)?;

    // --- Card 1: RAM ---
    draw_card(display, CARD_X, cy(1), CARD_W, CARD_H)?;
    draw_textbox(
        display, "RAM Auslastung",
        Rectangle::new(Point::new(16, cy(1) + 2), Size::new(130, 18)),
        &PROFONT_12_POINT, theme::theme().text_muted,
        HorizontalAlignment::Left, VerticalAlignment::Middle,
    )?;
    draw_textbox(
        display, &format!("{}%", fmt_0dp_w(ram)),
        Rectangle::new(Point::new(160, cy(1) + 2), Size::new(68, 18)),
        &PROFONT_12_POINT, theme::theme().primary,
        HorizontalAlignment::Right, VerticalAlignment::Middle,
    )?;
    draw_progress_bar(display, 16, cy(1) + 24, 200, 12, ram as u8, theme::theme().primary)?;

    // --- Card 2: Disk ---
    draw_card(display, CARD_X, cy(2), CARD_W, CARD_H)?;
    draw_textbox(
        display, "Speicher",
        Rectangle::new(Point::new(16, cy(2) + 2), Size::new(130, 18)),
        &PROFONT_12_POINT, theme::theme().text_muted,
        HorizontalAlignment::Left, VerticalAlignment::Middle,
    )?;
    draw_textbox(
        display, &format!("{}%", fmt_0dp_w(disk)),
        Rectangle::new(Point::new(160, cy(2) + 2), Size::new(68, 18)),
        &PROFONT_12_POINT, theme::theme().secondary,
        HorizontalAlignment::Right, VerticalAlignment::Middle,
    )?;
    draw_progress_bar(display, 16, cy(2) + 24, 200, 12, disk as u8, theme::theme().secondary)?;

    // --- Card 3: Uptime ---
    let yu = cy(3);
    draw_card(display, CARD_X, yu, CARD_W, UPTIME_CARD_H)?;
    draw_textbox(
        display, "System Uptime",
        Rectangle::new(Point::new(16, yu + 2), Size::new(130, 16)),
        &PROFONT_12_POINT, theme::theme().text_muted,
        HorizontalAlignment::Left, VerticalAlignment::Middle,
    )?;
    draw_textbox(
        display, &format_uptime(uptime_secs),
        Rectangle::new(Point::new(16, yu + 20), Size::new(200, 20)),
        &PROFONT_12_POINT, theme::theme().text,
        HorizontalAlignment::Left, VerticalAlignment::Middle,
    )?;
    Ok(())
}
