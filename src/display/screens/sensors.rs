use embedded_graphics::{
    geometry::{Point, Size},
    mono_font::ascii::FONT_6X10,
    pixelcolor::Rgb565,
    prelude::*,
    primitives::Rectangle,
};
use embedded_text::alignment::{HorizontalAlignment, VerticalAlignment};

use crate::AppState;

use crate::display::theme;
use crate::display::{draw_card, draw_textbox, sanitize_text, CONTENT_Y};

pub fn draw_sensors_screen<D: DrawTarget<Color=Rgb565>>(
    display: &mut D, state: &AppState,
) -> Result<(), D::Error> {
    // --- Header card ---
    let hdr_y = CONTENT_Y + 32;
    draw_card(display, 5, hdr_y, 230, 24)?;
    draw_textbox(
        display, "Sensor-Daten",
        Rectangle::new(Point::new(11, hdr_y), Size::new(218, 24)),
        &FONT_6X10, theme::theme().primary,
        HorizontalAlignment::Center, VerticalAlignment::Middle,
    )?;

    let data = state.read();
    let sensors = &data.sensors;
    let (card_w, card_h, gap) = (108i32, 66i32, 4i32);
    let start_x = 5;
    let start_y = hdr_y + 24 + 4;

    for i in 0..6 {
        let (col, row) = (i % 2, i / 2);
        let x = start_x + col * (card_w + gap);
        let y = start_y + row * (card_h + gap);
        draw_card(display, x, y, card_w, card_h)?;
        if let Some(sensor) = sensors.get(i as usize) {
            let label = sanitize_text(sensor.label.as_str());
            draw_textbox(
                display, &label,
                Rectangle::new(Point::new(x + 6, y + 2), Size::new(card_w as u32 - 12, 16)),
                &FONT_6X10, theme::theme().text_muted,
                HorizontalAlignment::Left, VerticalAlignment::Middle,
            )?;
            let value = sanitize_text(sensor.value.as_str());
            draw_textbox(
                display, &value,
                Rectangle::new(Point::new(x + 6, y + 28), Size::new(card_w as u32 - 12, 30)),
                &FONT_6X10, theme::theme().text,
                HorizontalAlignment::Left, VerticalAlignment::Middle,
            )?;
        } else {
            draw_textbox(
                display, "--",
                Rectangle::new(Point::new(x + 6, y + 28), Size::new(card_w as u32 - 12, 30)),
                &FONT_6X10, theme::theme().text_muted,
                HorizontalAlignment::Left, VerticalAlignment::Middle,
            )?;
        }
    }
    Ok(())
}
