use alloc::format;
use alloc::string::ToString;
use alloc::vec;

use embedded_graphics::{
    geometry::Point,
    mono_font::ascii::{FONT_6X10, FONT_9X15, FONT_9X18_BOLD},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::Rectangle,
};

use crate::AppState;

use crate::display::theme;
use crate::display::{
    draw_card, draw_text, fmt_0dp_w, fmt_1dp_w, THEME_BTN_X, THEME_BTN_Y,
};

fn draw_weather_icon<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, code: &str, x: i32, y: i32,
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

    for row in 0..h {
        let mut col = 0;
        while col < w {
            let idx = ((row * w + col) * 4) as usize;
            if pixels[idx + 3] >= 128 {
                let start_col = col;
                while col < w {
                    let i = ((row * w + col) * 4) as usize;
                    if pixels[i + 3] < 128 { break; }
                    col += 1;
                }
                let len = (col - start_col) as usize;
                let area = Rectangle::new(
                    Point::new(start_col + x, row + y),
                    Size::new(len as u32, 1),
                );
                let iter = (0..len).map(|ci| {
                    let i = (ci + start_col as usize + (row * w) as usize) * 4;
                    Rgb565::new(pixels[i] >> 3, pixels[i + 1] >> 2, pixels[i + 2] >> 3)
                });
                display.fill_contiguous(&area, iter)?;
            } else {
                col += 1;
            }
        }
    }
    Ok(())
}

fn draw_theme_toggle_button<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
) -> Result<(), D::Error> {
    let icon = if theme::is_dark_mode() { "01n" } else { "01d" };
    draw_weather_icon(display, icon, THEME_BTN_X, THEME_BTN_Y)
}

pub fn draw_weather_screen<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, state: &AppState,
) -> Result<(), D::Error> {
    draw_card(display, 5, 22 + crate::display::CONTENT_Y, 230, 58)?;
    let data = state.read();
    let time_str = if let Some(t) = data.local_time {
        format!("{:02}.{:02}.{:04}", t.day, t.month, t.year)
    } else {
        "--.--.----".to_string()
    };
    draw_text(
        display, &time_str, 10, 28 + crate::display::CONTENT_Y + crate::display::TITLE_Y_INC,
        &FONT_6X10, theme::theme().text_muted,
    )?;
    let time_str2 = if let Some(t) = data.local_time {
        format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second)
    } else {
        "--:--:--".to_string()
    };
    draw_text(
        display, &time_str2, 10, 52 + crate::display::CONTENT_Y + crate::display::TITLE_Y_INC,
        &FONT_9X15, theme::theme().primary,
    )?;
    draw_theme_toggle_button(display)?;

    draw_card(display, 5, 85 + crate::display::CONTENT_Y, 230, 72)?;
    let temp_str = if let Some(ref w) = data.weather {
        format!("{} C", fmt_1dp_w(w.temp))
    } else {
        "--.- C".to_string()
    };
    draw_text(
        display, &temp_str, 10, 95 + crate::display::CONTENT_Y,
        &FONT_9X18_BOLD, theme::theme().text,
    )?;
    let desc_str = if let Some(w) = &data.weather {
        crate::display::sanitize_text(&w.desc)
    } else {
        heapless::String::try_from("--").unwrap()
    };
    draw_text(
        display, &desc_str, 10, 120 + crate::display::CONTENT_Y,
        &FONT_6X10, theme::theme().text_muted,
    )?;

    let icon_code = data.weather.as_ref().map(|w| w.icon.as_str()).unwrap_or("--");
    draw_weather_icon(display, icon_code, 170, 102)?;

    let (t, h, w_spd, p) = if let Some(ref wx) = data.weather {
        (wx.temp, wx.humidity, wx.wind, wx.pressure)
    } else {
        (0.0, 0.0, 0.0, 0.0)
    };

    draw_card(display, 5, 163 + crate::display::CONTENT_Y, 108, 64)?;
    draw_text(
        display, "Temp", 8, 171 + crate::display::CONTENT_Y,
        &FONT_6X10, theme::theme().warning,
    )?;
    draw_text(
        display, &format!("{}C", fmt_1dp_w(t)),
        8, 189 + crate::display::CONTENT_Y, &FONT_6X10, theme::theme().text,
    )?;

    draw_card(display, 119, 163 + crate::display::CONTENT_Y, 108, 64)?;
    draw_text(
        display, "Feuchte", 122, 171 + crate::display::CONTENT_Y,
        &FONT_6X10, theme::theme().primary,
    )?;
    draw_text(
        display, &format!("{}%", fmt_0dp_w(h)),
        122, 189 + crate::display::CONTENT_Y, &FONT_6X10, theme::theme().text,
    )?;

    draw_card(display, 5, 233 + crate::display::CONTENT_Y, 108, 64)?;
    draw_text(
        display, "Wind", 8, 241 + crate::display::CONTENT_Y,
        &FONT_6X10, theme::theme().secondary,
    )?;
    draw_text(
        display, &format!("{}km/h", fmt_1dp_w(w_spd)),
        8, 259 + crate::display::CONTENT_Y, &FONT_6X10, theme::theme().text,
    )?;

    draw_card(display, 119, 233 + crate::display::CONTENT_Y, 108, 64)?;
    draw_text(
        display, "Druck", 122, 241 + crate::display::CONTENT_Y,
        &FONT_6X10, theme::theme().warning,
    )?;
    draw_text(
        display, &format!("{}hPa", fmt_0dp_w(p)),
        122, 259 + crate::display::CONTENT_Y, &FONT_6X10, theme::theme().text,
    )?;

    Ok(())
}
