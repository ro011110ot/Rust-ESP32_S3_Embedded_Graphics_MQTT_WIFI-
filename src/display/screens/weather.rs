use alloc::format;
use alloc::string::ToString;
use alloc::vec;

use embedded_graphics::{
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::Rectangle,
};
use embedded_text::alignment::{HorizontalAlignment, VerticalAlignment};
use profont::{PROFONT_12_POINT, PROFONT_18_POINT, PROFONT_24_POINT};

use crate::AppState;

use crate::display::theme;
use crate::display::{
    draw_card, draw_textbox, fmt_0dp_w, fmt_1dp_w, CONTENT_Y,
};

// Weather icon sizes
const ICON_SIZE: i32 = 50;

fn draw_weather_icon<D: DrawTarget<Color=Rgb565>>(
    display: &mut D, code: &str, x: i32, y: i32, size: i32,
) -> Result<(), D::Error> {
    let data: &[u8] = match code {
        "01d" => include_bytes!("../../../assets/icons_png/01d.png"),
        "01n" => include_bytes!("../../../assets/icons_png/01n.png"),
        "02d" => include_bytes!("../../../assets/icons_png/02d.png"),
        "02n" => include_bytes!("../../../assets/icons_png/02n.png"),
        "03d" => include_bytes!("../../../assets/icons_png/03d.png"),
        "03n" => include_bytes!("../../../assets/icons_png/03n.png"),
        "04d" => include_bytes!("../../../assets/icons_png/04d.png"),
        "04n" => include_bytes!("../../../assets/icons_png/04n.png"),
        "09d" => include_bytes!("../../../assets/icons_png/09d.png"),
        "09n" => include_bytes!("../../../assets/icons_png/09n.png"),
        "10d" => include_bytes!("../../../assets/icons_png/10d.png"),
        "10n" => include_bytes!("../../../assets/icons_png/10n.png"),
        "11d" => include_bytes!("../../../assets/icons_png/11d.png"),
        "11n" => include_bytes!("../../../assets/icons_png/11n.png"),
        "13d" => include_bytes!("../../../assets/icons_png/13d.png"),
        "13n" => include_bytes!("../../../assets/icons_png/13n.png"),
        "50d" => include_bytes!("../../../assets/icons_png/50d.png"),
        "50n" => include_bytes!("../../../assets/icons_png/50n.png"),
        _ => return Ok(()),
    };
    let header = minipng::decode_png_header(data).unwrap();
    let needed = header.required_bytes_rgba8bpc();
    let mut buf = vec![0u8; needed];
    let mut image = minipng::decode_png(data, &mut buf).unwrap();
    image.convert_to_rgba8bpc().unwrap();
    let pixels = image.pixels();
    let w = image.width() as i32;
    let h = image.height() as i32;

    let scale_x = size as f32 / w as f32;
    let scale_y = size as f32 / h as f32;
    let scale = scale_x.min(scale_y);
    let out_w = (w as f32 * scale) as i32;
    let out_h = (h as f32 * scale) as i32;
    let x_off = (size - out_w) / 2;
    let y_off = (size - out_h) / 2;

    for row in 0..out_h {
        let src_row = (row as f32 / scale) as i32;
        let mut col = 0;
        while col < out_w {
            let src_col = (col as f32 / scale) as i32;
            let idx = ((src_row * w + src_col) * 4) as usize;
            if pixels[idx + 3] >= 128 {
                let start_col = col;
                while col < out_w {
                    let sc = (col as f32 / scale) as i32;
                    let i = ((src_row * w + sc) * 4) as usize;
                    if pixels[i + 3] < 128 { break; }
                    col += 1;
                }
                let len = (col - start_col) as usize;
                let area = Rectangle::new(
                    Point::new(start_col + x + x_off, row + y + y_off),
                    Size::new(len as u32, 1),
                );
                let iter = (0..len).map(|ci| {
                    let sc = ((ci as i32 + start_col) as f32 / scale) as i32;
                    let i = ((src_row * w + sc) * 4) as usize;
                    Rgb565::new(
                        31 - (pixels[i] >> 3),
                        63 - (pixels[i + 1] >> 2),
                        31 - (pixels[i + 2] >> 3),
                    )
                });
                display.fill_contiguous(&area, iter)?;
            } else {
                col += 1;
            }
        }
    }
    Ok(())
}

fn draw_theme_toggle_button<D: DrawTarget<Color=Rgb565>>(
    display: &mut D,
) -> Result<(), D::Error> {
    let icon = if theme::is_dark_mode() { "01n" } else { "01d" };
    draw_weather_icon(display, icon, 185, 40, 36)
}

pub fn draw_weather_screen<D: DrawTarget<Color=Rgb565>>(
    display: &mut D, state: &AppState,
) -> Result<(), D::Error> {
    let data = state.read();

    // --- Card 0: Date / Time ---
    let c0_y = 22 + CONTENT_Y;
    draw_card(display, 5, c0_y, 230, 52)?;
    let time_str = if let Some(t) = data.local_time {
        format!("{:02}.{:02}.{:04}", t.day, t.month, t.year)
    } else {
        "--.--.----".to_string()
    };
    draw_textbox(
        display, &time_str,
        Rectangle::new(Point::new(8, c0_y + 4), Size::new(160, 18)),
        &PROFONT_12_POINT, theme::theme().text_muted,
        HorizontalAlignment::Left, VerticalAlignment::Middle,
    )?;
    let time_str2 = if let Some(t) = data.local_time {
        format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second)
    } else {
        "--:--:--".to_string()
    };
    draw_textbox(
        display, &time_str2,
        Rectangle::new(Point::new(8, c0_y + 24), Size::new(160, 24)),
        &PROFONT_18_POINT, theme::theme().primary,
        HorizontalAlignment::Left, VerticalAlignment::Middle,
    )?;
    draw_theme_toggle_button(display)?;

    // --- Card 1: Weather condition ---
    let c1_y = 80 + CONTENT_Y;
    draw_card(display, 5, c1_y, 230, 62)?;
    let temp_str = if let Some(ref w) = data.weather {
        format!("{} C", fmt_1dp_w(w.temp))
    } else {
        "--.- C".to_string()
    };
    draw_textbox(
        display, &temp_str,
        Rectangle::new(Point::new(8, c1_y + 4), Size::new(120, 36)),
        &PROFONT_24_POINT, theme::theme().text,
        HorizontalAlignment::Left, VerticalAlignment::Middle,
    )?;
    let desc_str = if let Some(w) = &data.weather {
        crate::display::sanitize_text(&w.desc)
    } else {
        heapless::String::try_from("--").unwrap()
    };
    draw_textbox(
        display, &desc_str,
        Rectangle::new(Point::new(8, c1_y + 42), Size::new(120, 16)),
        &PROFONT_12_POINT, theme::theme().text_muted,
        HorizontalAlignment::Left, VerticalAlignment::Middle,
    )?;

    let icon_code = data.weather.as_ref().map(|w| w.icon.as_str()).unwrap_or("--");
    draw_weather_icon(display, icon_code, 170, c1_y + 4, ICON_SIZE)?;

    // --- Cards 2-5: 2x2 grid (Temp, Feuchte, Wind, Druck) ---
    let (t, h, w_spd, p) = if let Some(ref wx) = data.weather {
        (wx.temp, wx.humidity, wx.wind, wx.pressure)
    } else {
        (0.0, 0.0, 0.0, 0.0)
    };

    let grid_x = [5, 120];
    let grid_y = [148 + CONTENT_Y, 204 + CONTENT_Y];
    let card_w: i32 = 109;
    let card_h: i32 = 50;

    let entries = [
        ("Temp", &format!("{}C", fmt_1dp_w(t)), theme::theme().warning),
        ("Feuchte", &format!("{}%", fmt_0dp_w(h)), theme::theme().primary),
        ("Wind", &format!("{}km/h", fmt_1dp_w(w_spd)), theme::theme().secondary),
        ("Druck", &format!("{}hPa", fmt_0dp_w(p)), theme::theme().warning),
    ];

    for (i, (label, value, color)) in entries.iter().enumerate() {
        let col = i % 2;
        let row = i / 2;
        let x = grid_x[col];
        let y = grid_y[row];
        let inner_label = Rectangle::new(Point::new(x + 6, y + 4), Size::new(card_w as u32 - 12, 16));
        let inner_value = Rectangle::new(Point::new(x + 6, y + 20), Size::new(card_w as u32 - 12, 24));

        draw_card(display, x, y, card_w, card_h)?;
        draw_textbox(display, label, inner_label, &PROFONT_12_POINT, *color,
                     HorizontalAlignment::Left, VerticalAlignment::Middle)?;
        draw_textbox(display, value, inner_value, &PROFONT_12_POINT, theme::theme().text,
                     HorizontalAlignment::Left, VerticalAlignment::Middle)?;
    }

    Ok(())
}
