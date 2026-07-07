use embedded_graphics::{
    mono_font::ascii::FONT_6X10,
    pixelcolor::Rgb565,
    prelude::*,
};

use crate::AppState;

use crate::display::theme;
use crate::display::{draw_card, draw_text, sanitize_text};

pub fn draw_sensors_screen<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, state: &AppState,
) -> Result<(), D::Error> {
    draw_text(
        display, "Sensor-Daten", 60, 26 + crate::display::CONTENT_Y + crate::display::TITLE_Y_INC,
        &embedded_graphics::mono_font::ascii::FONT_9X15, theme::theme().primary,
    )?;
    let data = state.read();
    let sensors = &data.sensors;
    let (card_w, card_h, gap, start_x, start_y) = (108i32, 72i32, 6i32, 5i32, 48i32 + crate::display::CONTENT_Y);

    for i in 0..6 {
        let (col, row) = (i % 2, i / 2);
        let x = start_x + col * (card_w + gap);
        let y = start_y + row * (card_h + gap);
        draw_card(display, x, y, card_w, card_h)?;
        if let Some(sensor) = sensors.get(i as usize) {
            let label = sanitize_text(sensor.label.as_str());
            draw_text(display, &label, x + 4, y + 4, &FONT_6X10, theme::theme().text_muted)?;
            let value = sanitize_text(sensor.value.as_str());
            draw_text(display, &value, x + 4, y + 32, &FONT_6X10, theme::theme().text)?;
        } else {
            draw_text(display, "--", x + 4, y + 32, &FONT_6X10, theme::theme().text_muted)?;
        }
    }
    Ok(())
}
