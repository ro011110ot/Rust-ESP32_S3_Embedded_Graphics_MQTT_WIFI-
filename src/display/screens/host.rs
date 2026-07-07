use alloc::format;
use embedded_graphics::{
    mono_font::ascii::{FONT_6X10, FONT_9X15},
    pixelcolor::Rgb565,
    prelude::*,
};

use crate::AppState;

use crate::display::theme;
use crate::display::{draw_card, draw_progress_bar, draw_text, fmt_0dp_w, fmt_1dp_w, fmt_2dp_w};

pub fn draw_host_screen<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, state: &AppState,
) -> Result<(), D::Error> {
    draw_text(
        display, "Host Status", 72, 26 + crate::display::CONTENT_Y + crate::display::TITLE_Y_INC,
        &FONT_9X15, theme::theme().primary,
    )?;
    let data = state.read();
    let (cpu, cpu_temp, ram_pct, ssd_temp, net_down) = if let Some(h) = data.host {
        (h.cpu, h.cpu_temp, h.ram_pct, h.ssd_temp, h.net_down)
    } else {
        ([0.0f32; 4], 0.0, 0.0, 0.0, 0.0)
    };

    draw_card(display, 5, 46 + crate::display::CONTENT_Y, 230, 72)?;
    draw_text(
        display, "CPU Auslastung", 12, 50 + crate::display::CONTENT_Y,
        &FONT_6X10, theme::theme().text_muted,
    )?;
    for (i, &core_val) in cpu.iter().enumerate() {
        let by = 62 + crate::display::CONTENT_Y + i as i32 * 10;
        draw_text(
            display, &format!("C{}", i), 12, by,
            &FONT_6X10, theme::theme().text_muted,
        )?;
        draw_progress_bar(
            display, 40, by + 2, 140, 6, core_val as u8, theme::theme().primary,
        )?;
        draw_text(
            display, &format!("{}", fmt_0dp_w(core_val)), 210, by,
            &FONT_6X10, theme::theme().primary,
        )?;
    }

    draw_card(display, 5, 124 + crate::display::CONTENT_Y, 230, 42)?;
    draw_text(
        display, "Temperaturen", 12, 128 + crate::display::CONTENT_Y,
        &FONT_6X10, theme::theme().text_muted,
    )?;
    let t_str = if ssd_temp > 0.0 {
        format!("CPU:{}C SSD:{}C", fmt_0dp_w(cpu_temp), fmt_1dp_w(ssd_temp))
    } else {
        format!("CPU:{}C SSD:--C", fmt_0dp_w(cpu_temp))
    };
    draw_text(
        display, &t_str, 12, 142 + crate::display::CONTENT_Y,
        &FONT_6X10, theme::theme().text,
    )?;
    let t_color = if cpu_temp < 55.0 { theme::theme().secondary }
        else if cpu_temp < 75.0 { theme::theme().warning }
        else { theme::theme().danger };
    draw_progress_bar(
        display, 12, 156 + crate::display::CONTENT_Y, 210, 8, cpu_temp as u8, t_color,
    )?;

    draw_card(display, 5, 172 + crate::display::CONTENT_Y, 230, 42)?;
    draw_text(
        display, "RAM Auslastung", 12, 176 + crate::display::CONTENT_Y,
        &FONT_6X10, theme::theme().text_muted,
    )?;
    let used_gb = ram_pct * 32.0 / 100.0;
    draw_text(
        display, &format!("{} GB / 32 GB", fmt_1dp_w(used_gb)),
        150, 176 + crate::display::CONTENT_Y, &FONT_6X10, theme::theme().text,
    )?;
    draw_progress_bar(
        display, 12, 190 + crate::display::CONTENT_Y, 210, 10, ram_pct as u8, theme::theme().primary,
    )?;

    draw_card(display, 5, 220 + crate::display::CONTENT_Y, 230, 42)?;
    draw_text(
        display, "Netzwerk", 12, 224 + crate::display::CONTENT_Y,
        &FONT_6X10, theme::theme().text_muted,
    )?;
    let net_str = if net_down > 1024.0 {
        format!("DL: {} MB/s", fmt_2dp_w(net_down / 1024.0))
    } else {
        format!("DL: {} KB/s", fmt_1dp_w(net_down))
    };
    draw_text(
        display, &net_str, 12, 240 + crate::display::CONTENT_Y,
        &FONT_6X10, theme::theme().text,
    )?;
    Ok(())
}
