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
use crate::display::{draw_card, draw_progress_bar, draw_textbox, fmt_0dp_w, fmt_1dp_w, fmt_2dp_w, CONTENT_Y};

// --- Layout constants (relative to CONTENT_Y) ---
const TITLE_H: i32 = 24;
const CARD_H_CPU: i32 = 66;
const CARD_H_SMALL: i32 = 40;
const CARD_GAP: i32 = 4;
const CARD_W: i32 = 230;
const CARD_X: i32 = 5;

pub fn draw_host_screen<D: DrawTarget<Color=Rgb565>>(
    display: &mut D, state: &AppState,
) -> Result<(), D::Error> {
    // --- Header card ---
    let hdr_y = CONTENT_Y + 32;
    draw_card(display, CARD_X, hdr_y, CARD_W, TITLE_H)?;
    draw_textbox(
        display, "Host Status",
        Rectangle::new(Point::new(CARD_X + 6, hdr_y), Size::new(CARD_W as u32 - 12, TITLE_H as u32)),
        &PROFONT_12_POINT, theme::theme().primary,
        HorizontalAlignment::Center, VerticalAlignment::Middle,
    )?;

    let data = state.read();
    let (cpu, cpu_temp, ram_pct, ssd_temp, net_down) = if let Some(h) = data.host {
        (h.cpu, h.cpu_temp, h.ram_pct, h.ssd_temp, h.net_down)
    } else {
        ([0.0f32; 4], 0.0, 0.0, 0.0, 0.0)
    };

    // Offsets for each card block
    let cy = |i: i32| {
        hdr_y + TITLE_H + CARD_GAP + i * (CARD_H_SMALL + CARD_GAP)
            + if i > 0 { CARD_H_CPU - CARD_H_SMALL } else { 0 }
    };

    // --- Card 0: CPU ---
    draw_card(display, CARD_X, cy(0), CARD_W, CARD_H_CPU)?;
    draw_textbox(
        display, "CPU Auslastung",
        Rectangle::new(Point::new(16, cy(0) + 2), Size::new(130, 16)),
        &PROFONT_12_POINT, theme::theme().text_muted,
        HorizontalAlignment::Left, VerticalAlignment::Middle,
    )?;
    for (i, &core_val) in cpu.iter().enumerate() {
        let by = cy(0) + 18 + i as i32 * 13;
        draw_textbox(
            display, &format!("C{}", i),
            Rectangle::new(Point::new(16, by), Size::new(20, 12)),
            &PROFONT_12_POINT, theme::theme().text_muted,
            HorizontalAlignment::Left, VerticalAlignment::Middle,
        )?;
        draw_progress_bar(display, 40, by + 1, 140, 7, core_val as u8, theme::theme().primary)?;
        draw_textbox(
            display, &fmt_0dp_w(core_val),
            Rectangle::new(Point::new(180, by), Size::new(48, 12)),
            &PROFONT_12_POINT, theme::theme().primary,
            HorizontalAlignment::Right, VerticalAlignment::Middle,
        )?;
    }

    // --- Card 1: Temperaturen ---
    let y1 = cy(1);
    draw_card(display, CARD_X, y1, CARD_W, CARD_H_SMALL)?;
    draw_textbox(
        display, "Temperaturen",
        Rectangle::new(Point::new(16, y1 + 2), Size::new(130, 16)),
        &PROFONT_12_POINT, theme::theme().text_muted,
        HorizontalAlignment::Left, VerticalAlignment::Middle,
    )?;
    let t_str = if ssd_temp > 0.0 {
        format!("CPU:{}C  SSD:{}C", fmt_0dp_w(cpu_temp), fmt_1dp_w(ssd_temp))
    } else {
        format!("CPU:{}C  SSD:--C", fmt_0dp_w(cpu_temp))
    };
    draw_textbox(
        display, &t_str,
        Rectangle::new(Point::new(16, y1 + 18), Size::new(200, 14)),
        &PROFONT_12_POINT, theme::theme().text,
        HorizontalAlignment::Left, VerticalAlignment::Middle,
    )?;
    let t_color = if cpu_temp < 55.0 { theme::theme().secondary } else if cpu_temp < 75.0 { theme::theme().warning } else { theme::theme().danger };
    draw_progress_bar(display, 16, y1 + 32, 200, 6, cpu_temp as u8, t_color)?;

    // --- Card 2: RAM ---
    let y2 = cy(2);
    draw_card(display, CARD_X, y2, CARD_W, CARD_H_SMALL)?;
    draw_textbox(
        display, "RAM Auslastung",
        Rectangle::new(Point::new(16, y2 + 2), Size::new(130, 16)),
        &PROFONT_12_POINT, theme::theme().text_muted,
        HorizontalAlignment::Left, VerticalAlignment::Middle,
    )?;
    let used_gb = ram_pct * 32.0 / 100.0;
    draw_textbox(
        display, &format!("{} GB / 32 GB", fmt_1dp_w(used_gb)),
        Rectangle::new(Point::new(16, y2 + 18), Size::new(200, 14)),
        &PROFONT_12_POINT, theme::theme().text,
        HorizontalAlignment::Left, VerticalAlignment::Middle,
    )?;
    draw_progress_bar(display, 16, y2 + 32, 200, 6, ram_pct as u8, theme::theme().primary)?;

    // --- Card 3: Network ---
    let y3 = cy(3);
    draw_card(display, CARD_X, y3, CARD_W, CARD_H_SMALL)?;
    draw_textbox(
        display, "Netzwerk",
        Rectangle::new(Point::new(16, y3 + 2), Size::new(130, 16)),
        &PROFONT_12_POINT, theme::theme().text_muted,
        HorizontalAlignment::Left, VerticalAlignment::Middle,
    )?;
    let net_str = if net_down > 1024.0 {
        format!("DL: {} MB/s", fmt_2dp_w(net_down / 1024.0))
    } else {
        format!("DL: {} KB/s", fmt_1dp_w(net_down))
    };
    draw_textbox(
        display, &net_str,
        Rectangle::new(Point::new(16, y3 + 18), Size::new(200, 14)),
        &PROFONT_12_POINT, theme::theme().text,
        HorizontalAlignment::Left, VerticalAlignment::Middle,
    )?;
    Ok(())
}
