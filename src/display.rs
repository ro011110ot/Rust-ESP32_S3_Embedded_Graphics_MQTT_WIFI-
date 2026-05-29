use alloc::boxed::Box;
use alloc::format;
use alloc::string::ToString;
use alloc::vec;
use core::fmt::Write;

use embedded_hal_bus::spi::AtomicDevice;
use embedded_hal_bus::util::AtomicCell;

use embassy_time::{Duration, Instant, Timer};

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    mono_font::ascii::{FONT_6X10, FONT_9X15, FONT_9X18_BOLD},
    mono_font::MonoTextStyle,
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyleBuilder, Rectangle, StyledDrawable},
    text::Text,
    Pixel,
};
use embedded_hal::delay::DelayNs;

use esp_hal::{
    delay::Delay,
    dma::{DmaRxBuf, DmaTxBuf},
    gpio::{Level, Output, OutputConfig},
    peripherals::Peripherals,
    spi::master::{Config as SpiConfig, Spi},
    time::Rate,
};

use mipidsi::dcs::InterfaceExt;
use mipidsi::options::{ColorInversion, ColorOrder, Orientation};
use mipidsi::{models::ILI9341Rgb565, Builder};
use mipidsi::interface::SpiInterface;

use crate::AppState;

const DISP_W: u16 = 240;
const DISP_H: u16 = 320;
pub const PIXEL_COUNT: usize = DISP_W as usize * DISP_H as usize;
pub const FB_SIZE: usize = PIXEL_COUNT * 2;
const BYTE_SWAP: bool = false;

const STATUS_H: i32 = 18;
const CONTENT_Y: i32 = 12;
const TITLE_Y_INC: i32 = 10;
const NAV_H: i32 = 40;
const NAV_TOP: i32 = DISP_H as i32 - NAV_H;

const NAV_BTN_W: i32 = DISP_W as i32 / 4;

const DIMMING_TIMEOUT_MS: u64 = 180_000;

const TOUCH_X_MIN: u16 = 288;
const TOUCH_X_MAX: u16 = 1866;
const TOUCH_Y_MIN: u16 = 230;
const TOUCH_Y_MAX: u16 = 1850;

const TOUCH_X_OFFSET: i32 = 20;
const TOUCH_Y_OFFSET: i32 = 0;

const THEME_BG: Rgb565 = Rgb565::new(3, 6, 3);
const THEME_CARD: Rgb565 = Rgb565::new(5, 10, 5);
const THEME_BORDER: Rgb565 = Rgb565::new(8, 16, 9);
const THEME_PRIMARY: Rgb565 = Rgb565::new(6, 46, 31);
const THEME_SECONDARY: Rgb565 = Rgb565::new(2, 46, 16);
const THEME_WARNING: Rgb565 = Rgb565::new(30, 39, 1);
const THEME_DANGER: Rgb565 = Rgb565::new(29, 17, 8);
const THEME_TEXT: Rgb565 = Rgb565::new(30, 61, 30);
const THEME_TEXT_MUTED: Rgb565 = Rgb565::new(18, 36, 18);
const THEME_NAV: Rgb565 = Rgb565::new(3, 6, 3);
const THEME_NAV_ACTIVE: Rgb565 = Rgb565::new(8, 16, 9);
const THEME_NAV_INACTIVE: Rgb565 = Rgb565::new(5, 10, 5);

static mut FB: Option<Box<[u8]>> = None;

pub fn init_fb(buf: Box<[u8]>) {
    unsafe { FB = Some(buf); }
}

struct DisplayBuffer;

impl OriginDimensions for DisplayBuffer {
    fn size(&self) -> Size {
        Size::new(DISP_W as u32, DISP_H as u32)
    }
}

impl DrawTarget for DisplayBuffer {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Rgb565>>,
    {
        let fb = unsafe {
            let raw: *mut Option<Box<[u8]>> = core::ptr::addr_of_mut!(FB);
            (*raw).as_mut().unwrap().as_mut()
        };
        for Pixel(coord, color) in pixels {
            if coord.x >= 0 && coord.x < DISP_W as i32
                && coord.y >= 0 && coord.y < DISP_H as i32
            {
                let idx = (coord.y as usize * DISP_W as usize + coord.x as usize) * 2;
                let raw: u16 = color.into_storage();
                let stored = if BYTE_SWAP { raw.swap_bytes() } else { raw };
                fb[idx..idx + 2].copy_from_slice(&stored.to_ne_bytes());
            }
        }
        Ok(())
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I)
        -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Rgb565>,
    {
        let fb = unsafe {
            let raw: *mut Option<Box<[u8]>> = core::ptr::addr_of_mut!(FB);
            (*raw).as_mut().unwrap().as_mut()
        };
        let x0 = area.top_left.x.max(0).min(DISP_W as i32 - 1) as usize;
        let y0 = area.top_left.y.max(0).min(DISP_H as i32 - 1) as usize;
        let x1 = (area.top_left.x + area.size.width as i32 - 1)
            .max(0).min(DISP_W as i32 - 1) as usize;
        let y1 = (area.top_left.y + area.size.height as i32 - 1)
            .max(0).min(DISP_H as i32 - 1) as usize;
        if x0 > x1 || y0 > y1 {
            return Ok(());
        }
        let row_w = x1 - x0 + 1;
        let total = row_w.checked_mul(y1 - y0 + 1).unwrap_or(0);
        for (i, color) in colors.into_iter().enumerate() {
            if i >= total { break; }
            let px = x0 + i % row_w;
            let py = y0 + i / row_w;
            let idx = py * DISP_W as usize + px;
            if idx >= PIXEL_COUNT { break; }
            let idx = idx * 2;
            let raw: u16 = color.into_storage();
            let stored = if BYTE_SWAP { raw.swap_bytes() } else { raw };
            fb[idx..idx + 2].copy_from_slice(&stored.to_ne_bytes());
        }
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Rgb565)
        -> Result<(), Self::Error>
    {
        self.fill_contiguous(area, core::iter::repeat(color))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Weather = 0,
    Sensors = 1,
    Vps = 2,
    Host = 3,
}

impl Screen {
    pub fn label(self) -> &'static str {
        match self {
            Screen::Weather => "Wetter",
            Screen::Sensors => "Sensor",
            Screen::Vps => "VPS",
            Screen::Host => "Host",
        }
    }

    pub fn all() -> [Screen; 4] {
        [Screen::Weather, Screen::Sensors, Screen::Vps, Screen::Host]
    }

    pub fn from_touch_x(x: i32) -> Screen {
        let idx = (x / NAV_BTN_W).clamp(0, 3);
        Self::all()[idx as usize]
    }
}

fn sanitize_text(input: &str) -> heapless::String<128> {
    let mut out = heapless::String::new();
    for c in input.chars() {
        let _ = match c {
            '\u{00E4}' => out.push_str("ae"),
            '\u{00F6}' => out.push_str("oe"),
            '\u{00FC}' => out.push_str("ue"),
            '\u{00C4}' => out.push_str("Ae"),
            '\u{00D6}' => out.push_str("Oe"),
            '\u{00DC}' => out.push_str("Ue"),
            '\u{00DF}' => out.push_str("ss"),
            '\u{00B0}' => Ok(()),
            _ if c.is_ascii() => out.push(c),
            _ => Ok(()),
        };
    }
    out
}

fn draw_card<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i32, y: i32, w: i32, h: i32,
) -> Result<(), D::Error> {
    Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32))
        .draw_styled(
            &PrimitiveStyleBuilder::new().fill_color(THEME_CARD).build(),
            display,
        )?;
    Rectangle::new(Point::new(x, y), Size::new(w as u32, 1))
        .draw_styled(
            &PrimitiveStyleBuilder::new()
                .fill_color(THEME_BORDER)
                .stroke_color(THEME_BORDER)
                .stroke_width(1)
                .build(),
            display,
        )?;
    Ok(())
}

fn draw_text<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    text: &str, x: i32, y: i32,
    font: &embedded_graphics::mono_font::MonoFont,
    color: Rgb565,
) -> Result<(), D::Error> {
    let style = MonoTextStyle::new(font, color);
    Text::new(text, Point::new(x, y), style).draw(display).map(|_| ())
}

fn draw_progress_bar<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i32, y: i32, w: i32, h: i32,
    pct: u8, color: Rgb565,
) -> Result<(), D::Error> {
    Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32))
        .draw_styled(
            &PrimitiveStyleBuilder::new()
                .fill_color(THEME_BORDER)
                .build(),
            display,
        )?;
    if pct == 0 { return Ok(()); }
    let fill_w = (w as u32 * pct as u32 / 100).max(3);
    let inner_h = (h as u32).saturating_sub(2).max(1);
    Rectangle::new(
        Point::new(x + 1, y + 1),
        Size::new(fill_w.saturating_sub(2), inner_h),
    )
    .draw_styled(
        &PrimitiveStyleBuilder::new().fill_color(color).build(),
        display,
    )?;
    Ok(())
}

fn draw_status_bar<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, time_str: &str, wifi_connected: bool,
) -> Result<(), D::Error> {
    Rectangle::new(Point::new(0, CONTENT_Y), Size::new(DISP_W as u32, STATUS_H as u32))
        .draw_styled(
            &PrimitiveStyleBuilder::new().fill_color(THEME_BG).build(),
            display,
        )?;
    draw_text(display, time_str, 4, CONTENT_Y + 4, &FONT_6X10, THEME_TEXT_MUTED)?;
    let dot_color = if wifi_connected { THEME_SECONDARY } else { THEME_DANGER };
    Rectangle::new(Point::new(DISP_W as i32 - 14, CONTENT_Y + 6), Size::new(8, 8))
        .draw_styled(
            &PrimitiveStyleBuilder::new().fill_color(dot_color).build(),
            display,
        )?;
    Ok(())
}

fn draw_nav_bar<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, active: Screen,
) -> Result<(), D::Error> {
    Rectangle::new(Point::new(0, NAV_TOP), Size::new(DISP_W as u32, NAV_H as u32))
        .draw_styled(
            &PrimitiveStyleBuilder::new().fill_color(THEME_NAV).build(),
            display,
        )?;
    for (i, screen) in Screen::all().iter().enumerate() {
        let is_active = *screen == active;
        let x = i as i32 * NAV_BTN_W;
        Rectangle::new(
            Point::new(x + 2, NAV_TOP + 2),
            Size::new(NAV_BTN_W as u32 - 4, NAV_H as u32 - 4),
        )
        .draw_styled(
            &PrimitiveStyleBuilder::new()
                .fill_color(
                    if is_active { THEME_NAV_ACTIVE } else { THEME_NAV_INACTIVE },
                )
                .build(),
            display,
        )?;
        let label = sanitize_text(screen.label());
        draw_text(
            display, &label,
            x + (NAV_BTN_W - (label.len() as i32 * 9)) / 2,
            NAV_TOP + 14,
            &FONT_6X10,
            if is_active { THEME_TEXT } else { THEME_TEXT_MUTED },
        )?;
    }
    Ok(())
}

fn draw_weather_icon<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, code: &str, x: i32, y: i32,
) -> Result<(), D::Error> {
    let data: &[u8] = match code {
        "01d" => include_bytes!("../Example/icons_png/01d.png"),
        "01n" => include_bytes!("../Example/icons_png/01n.png"),
        "02d" => include_bytes!("../Example/icons_png/02d.png"),
        "02n" => include_bytes!("../Example/icons_png/02n.png"),
        "03d" => include_bytes!("../Example/icons_png/03d.png"),
        "03n" => include_bytes!("../Example/icons_png/03n.png"),
        "04d" => include_bytes!("../Example/icons_png/04d.png"),
        "04n" => include_bytes!("../Example/icons_png/04n.png"),
        "09d" => include_bytes!("../Example/icons_png/09d.png"),
        "09n" => include_bytes!("../Example/icons_png/09n.png"),
        "10d" => include_bytes!("../Example/icons_png/10d.png"),
        "10n" => include_bytes!("../Example/icons_png/10n.png"),
        "11d" => include_bytes!("../Example/icons_png/11d.png"),
        "11n" => include_bytes!("../Example/icons_png/11n.png"),
        "13d" => include_bytes!("../Example/icons_png/13d.png"),
        "13n" => include_bytes!("../Example/icons_png/13n.png"),
        "50d" => include_bytes!("../Example/icons_png/50d.png"),
        "50n" => include_bytes!("../Example/icons_png/50n.png"),
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
                    if pixels[i + 3] < 128 {
                        break;
                    }
                    col += 1;
                }
                let len = (col - start_col) as usize;
                let area = Rectangle::new(
                    Point::new(start_col + x, row + y),
                    Size::new(len as u32, 1),
                );
                let iter = (0..len).map(|ci| {
                    let i = (ci + start_col as usize + (row * w) as usize) * 4;
                    Rgb565::new(
                        pixels[i] >> 3,
                        pixels[i + 1] >> 2,
                        pixels[i + 2] >> 3,
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

fn draw_weather_screen<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, state: &AppState,
) -> Result<(), D::Error> {
    draw_card(display, 5, 22 + CONTENT_Y, 230, 58)?;
    let data = state.read();
    let time_str = if let Some(t) = data.local_time {
        format!("{:02}.{:02}.{:04}", t.day, t.month, t.year)
    } else {
        "--.--.----".to_string()
    };
    draw_text(display, &time_str, 10, 28 + CONTENT_Y + TITLE_Y_INC, &FONT_6X10, THEME_TEXT_MUTED)?;
    let time_str2 = if let Some(t) = data.local_time {
        format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second)
    } else {
        "--:--:--".to_string()
    };
    draw_text(display, &time_str2, 10, 52 + CONTENT_Y + TITLE_Y_INC, &FONT_9X15, THEME_PRIMARY)?;
    draw_card(display, 5, 85 + CONTENT_Y, 230, 72)?;
    let temp_str = if let Some(ref w) = data.weather {
        format!("{} C", fmt_1dp_w(w.temp))
    } else {
        "--.- C".to_string()
    };
    draw_text(display, &temp_str, 10, 95 + CONTENT_Y, &FONT_9X18_BOLD, THEME_TEXT)?;
    let desc_str = if let Some(w) = &data.weather {
        sanitize_text(&w.desc)
    } else {
        heapless::String::try_from("--").unwrap()
    };
    draw_text(display, &desc_str, 10, 120 + CONTENT_Y, &FONT_6X10, THEME_TEXT_MUTED)?;
    let icon_code = data.weather.as_ref().map(|w| w.icon.as_str()).unwrap_or("--");
    draw_weather_icon(display, icon_code, 170, 102)?;
    let (t, h, w, p) = if let Some(ref wx) = data.weather {
        (wx.temp, wx.humidity, wx.wind, wx.pressure)
    } else {
        (0.0, 0.0, 0.0, 0.0)
    };
    draw_card(display, 5, 163 + CONTENT_Y, 108, 64)?;
    draw_text(display, "Temp", 8, 171 + CONTENT_Y, &FONT_6X10, THEME_WARNING)?;
    draw_text(display, &format!("{}C", fmt_1dp_w(t)), 8, 189 + CONTENT_Y, &FONT_6X10, THEME_TEXT)?;
    draw_card(display, 119, 163 + CONTENT_Y, 108, 64)?;
    draw_text(display, "Feuchte", 122, 171 + CONTENT_Y, &FONT_6X10, THEME_PRIMARY)?;
    draw_text(
        display, &format!("{}%", fmt_0dp_w(h)), 122, 189 + CONTENT_Y, &FONT_6X10, THEME_TEXT,
    )?;
    draw_card(display, 5, 233 + CONTENT_Y, 108, 64)?;
    draw_text(display, "Wind", 8, 241 + CONTENT_Y, &FONT_6X10, THEME_SECONDARY)?;
    draw_text(
        display, &format!("{}km/h", fmt_1dp_w(w)), 8, 259 + CONTENT_Y, &FONT_6X10, THEME_TEXT,
    )?;
    draw_card(display, 119, 233 + CONTENT_Y, 108, 64)?;
    draw_text(display, "Druck", 122, 241 + CONTENT_Y, &FONT_6X10, THEME_WARNING)?;
    draw_text(
        display, &format!("{}hPa", fmt_0dp_w(p)), 122, 259 + CONTENT_Y, &FONT_6X10, THEME_TEXT,
    )?;
    Ok(())
}

fn draw_sensors_screen<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, state: &AppState,
) -> Result<(), D::Error> {
    draw_text(display, "Sensor-Daten", 60, 26 + CONTENT_Y + TITLE_Y_INC, &FONT_9X15, THEME_PRIMARY)?;
    let data = state.read();
    let sensors = &data.sensors;
    let card_w = 108i32;
    let card_h = 72i32;
    let gap = 6i32;
    let start_x = 5i32;
    let start_y = 48i32 + CONTENT_Y;
    for i in 0..6 {
        let col = i % 2;
        let row = i / 2;
        let x = start_x + col * (card_w + gap);
        let y = start_y + row * (card_h + gap);
        draw_card(display, x, y, card_w, card_h)?;
        if let Some(sensor) = sensors.get(i as usize) {
            let label = sanitize_text(sensor.label.as_str());
            draw_text(display, &label, x + 4, y + 4, &FONT_6X10, THEME_TEXT_MUTED)?;
            let value = sanitize_text(sensor.value.as_str());
            draw_text(display, &value, x + 4, y + 32, &FONT_6X10, THEME_TEXT)?;
        } else {
            draw_text(display, "--", x + 4, y + 32, &FONT_6X10, THEME_TEXT_MUTED)?;
        }
    }
    Ok(())
}

fn draw_vps_screen<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, state: &AppState,
) -> Result<(), D::Error> {
    draw_text(display, "VPS Status", 78, 50 + CONTENT_Y, &FONT_9X15, THEME_PRIMARY)?;
    let data = state.read();
    let (cpu, ram, disk, uptime_secs) = if let Some(v) = data.vps {
        (v.cpu_pct, v.ram_pct, v.disk_pct, v.uptime_secs)
    } else {
        (0.0, 0.0, 0.0, 0u64)
    };
    draw_card(display, 5, 46 + CONTENT_Y, 230, 52)?;
    draw_text(display, "CPU Auslastung", 12, 50 + CONTENT_Y, &FONT_6X10, THEME_TEXT_MUTED)?;
    draw_text(
        display, &format!("{}%", fmt_0dp_w(cpu)), 200, 50 + CONTENT_Y,
        &FONT_6X10, THEME_WARNING,
    )?;
    draw_progress_bar(display, 16, 68 + CONTENT_Y, 200, 12, cpu as u8, THEME_WARNING)?;
    draw_card(display, 5, 106 + CONTENT_Y, 230, 52)?;
    draw_text(display, "RAM Auslastung", 12, 110 + CONTENT_Y, &FONT_6X10, THEME_TEXT_MUTED)?;
    draw_text(
        display, &format!("{}%", fmt_0dp_w(ram)), 200, 110 + CONTENT_Y,
        &FONT_6X10, THEME_PRIMARY,
    )?;
    draw_progress_bar(display, 16, 128 + CONTENT_Y, 200, 12, ram as u8, THEME_PRIMARY)?;
    draw_card(display, 5, 166 + CONTENT_Y, 230, 52)?;
    draw_text(display, "Speicher", 12, 170 + CONTENT_Y, &FONT_6X10, THEME_TEXT_MUTED)?;
    draw_text(
        display, &format!("{}%", fmt_0dp_w(disk)), 200, 170 + CONTENT_Y,
        &FONT_6X10, THEME_SECONDARY,
    )?;
    draw_progress_bar(display, 16, 188 + CONTENT_Y, 200, 12, disk as u8, THEME_SECONDARY)?;
    draw_card(display, 5, 226 + CONTENT_Y, 230, 44)?;
    draw_text(display, "System Uptime", 12, 230 + CONTENT_Y, &FONT_6X10, THEME_TEXT_MUTED)?;
    let uptime_str = format_uptime(uptime_secs);
    draw_text(display, &uptime_str, 12, 248 + CONTENT_Y, &FONT_6X10, THEME_TEXT)?;
    Ok(())
}

fn format_uptime(total_secs: u64) -> heapless::String<32> {
    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let mins = (total_secs % 3600) / 60;
    let mut s = heapless::String::new();
    if days > 0 {
        core::write!(&mut s, "{}d {:02}h {:02}m", days, hours, mins).ok();
    } else {
        core::write!(&mut s, "{:02}h {:02}m", hours, mins).ok();
    }
    s
}

fn draw_host_screen<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, state: &AppState,
) -> Result<(), D::Error> {
    draw_text(display, "Host Monitor", 72, 40 + CONTENT_Y,
              &FONT_9X15, THEME_PRIMARY)?;
    let data = state.read();
    let (cpu, cpu_temp, ram_pct, ssd_temp, net_down) = if let Some(h) = data.host {
        (h.cpu, h.cpu_temp, h.ram_pct, h.ssd_temp, h.net_down)
    } else {
        ([0.0f32; 4], 0.0, 0.0, 0.0, 0.0)
    };
    draw_card(display, 5, 36 + CONTENT_Y, 230, 72)?;
    draw_text(display, "CPU Auslastung", 12, 40 + CONTENT_Y,
              &FONT_6X10, THEME_TEXT_MUTED)?;
    for (i, &core_val) in cpu.iter().enumerate() {
        let by = 52 + CONTENT_Y + i as i32 * 10;
        draw_text(display, &format!("C{}", i), 12, by,
                  &FONT_6X10, THEME_TEXT_MUTED)?;
        draw_progress_bar(display, 40, by + 2, 140, 6,
                          core_val as u8, THEME_PRIMARY)?;
        draw_text(display, &format!("{}", fmt_0dp_w(core_val)),
                  210, by, &FONT_6X10, THEME_PRIMARY)?;
    }
    draw_card(display, 5, 114 + CONTENT_Y, 230, 42)?;
    draw_text(display, "Temperaturen", 12, 118 + CONTENT_Y,
              &FONT_6X10, THEME_TEXT_MUTED)?;
    let t_str = if ssd_temp > 0.0 {
        format!("CPU:{}C SSD:{}C", fmt_0dp_w(cpu_temp),
                fmt_1dp_w(ssd_temp))
    } else {
        format!("CPU:{}C SSD:--C", fmt_0dp_w(cpu_temp))
    };
    draw_text(display, &t_str, 12, 132 + CONTENT_Y,
              &FONT_6X10, THEME_TEXT)?;
    let t_color = if cpu_temp < 55.0 {
        THEME_SECONDARY
    } else if cpu_temp < 75.0 {
        THEME_WARNING
    } else {
        THEME_DANGER
    };
    draw_progress_bar(display, 12, 146 + CONTENT_Y, 210, 8,
                      cpu_temp as u8, t_color)?;
    draw_card(display, 5, 162 + CONTENT_Y, 230, 42)?;
    draw_text(display, "RAM Auslastung", 12, 166 + CONTENT_Y,
              &FONT_6X10, THEME_TEXT_MUTED)?;
    let used_gb = ram_pct * 32.0 / 100.0;
    draw_text(display, &format!("{} GB / 32 GB", fmt_1dp_w(used_gb)),
              150, 166 + CONTENT_Y, &FONT_6X10, THEME_TEXT)?;
    draw_progress_bar(display, 12, 180 + CONTENT_Y, 210, 10,
                      ram_pct as u8, THEME_PRIMARY)?;
    draw_card(display, 5, 210 + CONTENT_Y, 230, 42)?;
    draw_text(display, "Netzwerk", 12, 214 + CONTENT_Y,
              &FONT_6X10, THEME_TEXT_MUTED)?;
    let net_str = if net_down > 1024.0 {
        format!("DL: {} MB/s", fmt_2dp_w(net_down / 1024.0))
    } else {
        format!("DL: {} KB/s", fmt_1dp_w(net_down))
    };
    draw_text(display, &net_str, 12, 230 + CONTENT_Y,
              &FONT_6X10, THEME_TEXT)?;
    Ok(())
}

fn fmt_1dp_w(val: f32) -> alloc::string::String {
    let i = val as i32;
    let frac = (val.abs() - (val as i32).abs() as f32) * 10.0 + 0.5;
    let f = frac as u32 % 10;
    format!("{}.{}", i, f)
}
fn fmt_0dp_w(val: f32) -> alloc::string::String {
    let i = (if val >= 0.0 { val + 0.5 } else { val - 0.5 }) as i32;
    format!("{}", i)
}
fn fmt_2dp_w(val: f32) -> alloc::string::String {
    let i = val as i32;
    let frac = (val.abs() - (val as i32).abs() as f32) * 100.0 + 0.5;
    let f = frac as u32 % 100;
    format!("{}.{:02}", i, f)
}

fn read_touch_x(spi: &mut impl embedded_hal::spi::SpiDevice<u8>) -> Option<u16> {
    // 4 bytes = 32 SCLK at 10 MHz (100 ns/bit):
    //   byte 0: command (0x90 = X position, 12-bit, differential)
    //   bytes 1-3: dummy clocks for ADC conversion (~3 us)
    // Result is clocked out by SCLK during bytes 1-2.
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

pub fn read_touch(
    spi: &mut impl embedded_hal::spi::SpiDevice<u8>,
) -> Option<(i32, i32)> {
    let raw_x = read_touch_x(spi)?;
    let raw_y = read_touch_y(spi)?;
    if raw_x == 0x0FFF || raw_x == 0 || raw_x == 0x7FF { return None; }
    if raw_y == 0x0FFF || raw_y == 0 || raw_y == 0x7FF { return None; }
    if raw_x <= TOUCH_X_MIN || raw_x >= TOUCH_X_MAX { return None; }
    if raw_y <= TOUCH_Y_MIN || raw_y >= TOUCH_Y_MAX { return None; }
    let px = (TOUCH_Y_MAX - raw_y) as i32 * DISP_W as i32
        / (TOUCH_Y_MAX - TOUCH_Y_MIN) as i32 + TOUCH_X_OFFSET;
    let py = (raw_x - TOUCH_X_MIN) as i32 * DISP_H as i32
        / (TOUCH_X_MAX - TOUCH_X_MIN) as i32 + TOUCH_Y_OFFSET;
    let px = px.clamp(0, DISP_W as i32 - 1);
    let py = py.clamp(0, DISP_H as i32 - 1);
    log::info!("Touch: raw({},{}) screen({},{})", raw_x, raw_y, px, py);
    Some((px, py))
}

pub async fn display_task(state: &'static AppState) {
    log::info!("Display: task spawned");
    let p = unsafe { Peripherals::steal() };

    // Turn backlight on immediately so the display is lit even if a
    // later step blocks or panics.
    let mut backlight = Output::new(p.GPIO38, Level::High, OutputConfig::default());
    log::info!("Display: backlight on");

    let sclk = p.GPIO12;
    let mosi = p.GPIO11;
    let miso = p.GPIO13;
    let cs_disp = p.GPIO10;
    let dc = p.GPIO7;
    let rst = p.GPIO9;
    let cs_touch = p.GPIO3;
    let spi_dev = p.SPI2;

    let (rx_buf, rx_desc, tx_buf, tx_desc) = esp_hal::dma_buffers!(64, 4096);
    let dma_rx = DmaRxBuf::new(rx_desc, rx_buf).unwrap();
    let dma_tx = DmaTxBuf::new(tx_desc, tx_buf).unwrap();

    let spi = Spi::new(
        spi_dev,
        SpiConfig::default().with_frequency(Rate::from_mhz(10)),
    )
    .unwrap()
    .with_sck(sclk)
    .with_mosi(mosi)
    .with_miso(miso)
    .with_dma(p.DMA_CH0)
    .with_buffers(dma_rx, dma_tx);

    let spi_bus = AtomicCell::new(spi);

    let mut touch_dev = AtomicDevice::new_no_delay(
        &spi_bus,
        Output::new(cs_touch, Level::High, OutputConfig::default()),
    ).unwrap();

    let display_dev = AtomicDevice::new_no_delay(
        &spi_bus,
        Output::new(cs_disp, Level::High, OutputConfig::default()),
    ).unwrap();

    let mut buf = [0u8; 4096];
    let di = SpiInterface::new(
        display_dev,
        Output::new(dc, Level::High, OutputConfig::default()),
        &mut buf,
    );

    let mut delay = Delay::new();

    // Explicit hardware reset: 50ms low, 50ms high
    let mut reset_pin = Output::new(rst, Level::High, OutputConfig::default());
    _ = reset_pin.set_low();
    delay.delay_ms(50);
    _ = reset_pin.set_high();
    delay.delay_ms(50);

    let mut display = match Builder::new(ILI9341Rgb565, di)
        .reset_pin(reset_pin)
        .display_size(DISP_W, DISP_H)
        .orientation(Orientation::new())
        .color_order(ColorOrder::Rgb)
        .invert_colors(ColorInversion::Inverted)
        .init(&mut delay)
    {
        Ok(d) => d,
        Err(e) => {
            log::warn!("Display: ILI9341 init failed: {:?}", e);
            return;
        }
    };

    log::info!("Display: ILI9341 init complete");

    // Explicitly re-send SLPOUT (0x11) + DISPON (0x29).
    // Builder::init() should already do this, but some modules need it
    // repeated after the hardware reset cycle.
    unsafe { display.dcs().write_raw(0x11, &[]).ok(); }
    delay.delay_ms(150);
    unsafe { display.dcs().write_raw(0x29, &[]).ok(); }
    delay.delay_ms(50);
    log::info!("Display: explicit SLPOUT + DISPON sent");

    // Clear screen to black to flush any power-on noise from VRAM.
    display
        .set_pixels(
            0, 0, DISP_W - 1, DISP_H - 1,
            core::iter::repeat(Rgb565::new(0, 0, 0)).take(PIXEL_COUNT),
        )
        .ok();
    log::info!("Display: VRAM cleared");

    log::info!("Display: entering startup delay");
    // Give WiFi a head start before we consume SPI bandwidth.
    Timer::after(Duration::from_secs(5)).await;
    log::info!("Display: startup complete, entering loop");

    let mut backlight_on = true;
    let mut last_touch = Instant::now();
    let mut last_second = Instant::now();

    loop {
        Timer::after(Duration::from_millis(50)).await;

        let now = Instant::now();

        if now - last_second >= Duration::from_secs(1) {
            last_second = now;
            state.tick_local_time();
        }

        if backlight_on
            && now.duration_since(last_touch)
                > Duration::from_millis(DIMMING_TIMEOUT_MS)
        {
            backlight_on = false;
            _ = backlight.set_low();
            log::info!("Display: backlight OFF (timeout)");
        }

        if let Some((tx, ty)) = read_touch(&mut touch_dev) {
            last_touch = Instant::now();
            if !backlight_on {
                backlight_on = true;
                _ = backlight.set_high();
                log::info!("Display: backlight ON (touch wake)");
            } else {
                handle_nav_touch(tx, ty, state);
            }
        }

        if backlight_on {
            let mut fb_display = DisplayBuffer;

            // 1) Background – fill_solid bypasses Rectangle::draw_styled
            fb_display.fill_solid(
                &Rectangle::new(
                    Point::new(0, 0),
                    Size::new(DISP_W as u32, DISP_H as u32),
                ),
                THEME_BG,
            ).ok();

            // 2) Status bar
            let time_str = format_time(state);
            draw_status_bar(
                &mut fb_display, &time_str, state.read().wifi_connected,
            ).ok();

            // 3) Screen content
            let screen = state.read().active_screen;
            let _ = match screen {
                Screen::Weather => draw_weather_screen(&mut fb_display, state),
                Screen::Sensors => draw_sensors_screen(&mut fb_display, state),
                Screen::Vps => draw_vps_screen(&mut fb_display, state),
                Screen::Host => draw_host_screen(&mut fb_display, state),
            };

            // 4) Nav bar
            draw_nav_bar(&mut fb_display, state.read().active_screen).ok();

            // 5) Flush FB → display in 16 chunks
            // SAFETY: FB is initialized before display_task runs, never modified after
            let fb = unsafe { (*core::ptr::addr_of_mut!(FB)).as_ref().unwrap().as_ref() };
            for cy in 0..16 {
                let y0 = cy * 20;
                let y1 = (y0 + 19).min(DISP_H as u16 - 1);
                let iter = (y0..=y1).flat_map(move |y| {
                    let base = y as usize * DISP_W as usize;
                    (0..DISP_W as usize).map(move |x| {
                        let idx = (base + x) * 2;
                        let raw = (fb[idx + 1] as u16) << 8 | fb[idx] as u16;
                        Rgb565::new(
                            ((raw >> 11) & 0x1F) as u8,
                            ((raw >> 5) & 0x3F) as u8,
                            (raw & 0x1F) as u8,
                        )
                    })
                });
                display.set_pixels(0, y0, DISP_W - 1, y1, iter).ok();
                if cy < 15 {
                    Timer::after(Duration::from_millis(10)).await;
                }
            }
        }
    }
}

fn format_time(state: &AppState) -> heapless::String<32> {
    let data = state.read();
    if let Some(t) = data.local_time {
        let mut s = heapless::String::new();
        core::write!(&mut s, "{:02}:{:02}:{:02}", t.hour, t.minute, t.second).ok();
        s
    } else {
        heapless::String::try_from("--:--:--").unwrap()
    }
}

pub fn handle_nav_touch(tx: i32, ty: i32, state: &AppState) -> bool {
    if ty < NAV_TOP { return false; }
    let new_screen = Screen::from_touch_x(tx);
    let current = state.read().active_screen;
    if new_screen != current {
        state.set_active_screen(new_screen);
        log::info!("Display: switched to {:?}", new_screen);
        return true;
    }
    false
}
