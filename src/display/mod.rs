use alloc::boxed::Box;
use alloc::format;
use core::fmt::Write;

use embassy_time::{Duration, Instant, Timer};
use embedded_hal_bus::spi::AtomicDevice;
use embedded_hal_bus::util::AtomicCell;

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    mono_font::{ascii::FONT_6X10, MonoFont, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyleBuilder, Rectangle, StyledDrawable},
    text::Text,
    Pixel,
};

use embedded_hal::delay::DelayNs;
use embedded_text::{
    alignment::{HorizontalAlignment, VerticalAlignment},
    style::TextBoxStyleBuilder,
    TextBox,
};

use esp_hal::{
    delay::Delay,
    dma::{DmaRxBuf, DmaTxBuf},
    gpio::{Level, Output, OutputConfig},
    peripherals::Peripherals,
    spi::master::{Config as SpiConfig, Spi},
    time::Rate,
};

use mipidsi::dcs::InterfaceExt;
use mipidsi::interface::SpiInterface;
use mipidsi::options::{ColorInversion, ColorOrder, Orientation};
use mipidsi::{models::ILI9341Rgb565, Builder};

use crate::AppState;

mod theme;
pub(crate) mod touch;
pub(crate) mod screens;


// --- Display Constants ---
const DISP_W: u16 = 240;
const DISP_H: u16 = 320;
pub(crate) const PIXEL_COUNT: usize = DISP_W as usize * DISP_H as usize;
pub(crate) const FB_SIZE: usize = PIXEL_COUNT * 2;
const BYTE_SWAP: bool = false;

// --- UI Layout Constants ---
pub(crate) const STATUS_H: i32 = 18;
pub(crate) const CONTENT_Y: i32 = 12;
const NAV_H: i32 = 40;
pub(crate) const NAV_TOP: i32 = DISP_H as i32 - NAV_H;
pub(crate) const NAV_BTN_W: i32 = DISP_W as i32 / 4;

// --- Theme Toggle Button ---
pub(crate) const THEME_BTN_X: i32 = 185;
pub(crate) const THEME_BTN_Y: i32 = CONTENT_Y + 22;
pub(crate) const THEME_BTN_W: i32 = 50;
pub(crate) const THEME_BTN_H: i32 = 50;

// --- Power Management ---
const DIMMING_TIMEOUT_MS: u64 = 180_000;

// ============================================================================
// Screens Navigation
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
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

// ============================================================================
// Framebuffer Management
// ============================================================================

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
        I: IntoIterator<Item=Pixel<Rgb565>>,
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
                let stored = raw;
                fb[idx..idx + 2].copy_from_slice(&stored.to_ne_bytes());
            }
        }
        Ok(())
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I)
                          -> Result<(), Self::Error>
    where
        I: IntoIterator<Item=Rgb565>,
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
            let stored = raw;
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

// ============================================================================
// UI Drawing Primitives
// ============================================================================

pub(crate) fn sanitize_text(input: &str) -> heapless::String<128> {
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

pub(crate) fn draw_card<D: DrawTarget<Color=Rgb565>>(
    display: &mut D, x: i32, y: i32, w: i32, h: i32,
) -> Result<(), D::Error> {
    Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32))
        .draw_styled(
            &PrimitiveStyleBuilder::new().fill_color(theme::theme().card).build(),
            display,
        )?;
    Rectangle::new(Point::new(x, y), Size::new(w as u32, 1))
        .draw_styled(
            &PrimitiveStyleBuilder::new()
                .fill_color(theme::theme().border)
                .stroke_color(theme::theme().border)
                .stroke_width(1)
                .build(),
            display,
        )?;
    Ok(())
}

pub(crate) fn draw_text<D: DrawTarget<Color=Rgb565>>(
    display: &mut D, text: &str, x: i32, y: i32,
    font: &MonoFont, color: Rgb565,
) -> Result<(), D::Error> {
    let style = MonoTextStyle::new(font, color);
    Text::new(text, Point::new(x, y), style).draw(display).map(|_| ())
}

pub(crate) fn draw_textbox<D: DrawTarget<Color=Rgb565>>(
    display: &mut D, text: &str, bounds: Rectangle,
    font: &MonoFont, color: Rgb565,
    h_align: HorizontalAlignment, v_align: VerticalAlignment,
) -> Result<(), D::Error> {
    let char_style = MonoTextStyle::new(font, color);
    let tbox_style = TextBoxStyleBuilder::new()
        .alignment(h_align)
        .vertical_alignment(v_align)
        .build();
    TextBox::with_textbox_style(text, bounds, char_style, tbox_style).draw(display)?;
    Ok(())
}

pub(crate) fn draw_progress_bar<D: DrawTarget<Color=Rgb565>>(
    display: &mut D, x: i32, y: i32, w: i32, h: i32, pct: u8, color: Rgb565,
) -> Result<(), D::Error> {
    Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32))
        .draw_styled(
            &PrimitiveStyleBuilder::new().fill_color(theme::theme().border).build(),
            display,
        )?;
    if pct == 0 { return Ok(()); }
    let fill_w = (w as u32 * pct as u32 / 100).max(3);
    let inner_h = (h as u32).saturating_sub(2).max(1);
    Rectangle::new(Point::new(x + 1, y + 1), Size::new(fill_w.saturating_sub(2), inner_h))
        .draw_styled(
            &PrimitiveStyleBuilder::new().fill_color(color).build(),
            display,
        )?;
    Ok(())
}

fn draw_status_bar<D: DrawTarget<Color=Rgb565>>(
    display: &mut D, time_str: &str, wifi_connected: bool,
) -> Result<(), D::Error> {
    Rectangle::new(
        Point::new(0, CONTENT_Y),
        Size::new(DISP_W as u32, STATUS_H as u32),
    )
        .draw_styled(
            &PrimitiveStyleBuilder::new().fill_color(theme::theme().bg).build(),
            display,
        )?;
    draw_text(
        display, time_str, 4, CONTENT_Y + 4, &FONT_6X10, theme::theme().text_muted,
    )?;
    let dot_color = if wifi_connected { theme::theme().secondary } else { theme::theme().danger };
    Rectangle::new(Point::new(DISP_W as i32 - 14, CONTENT_Y + 6), Size::new(8, 8))
        .draw_styled(
            &PrimitiveStyleBuilder::new().fill_color(dot_color).build(),
            display,
        )?;
    Ok(())
}

fn draw_nav_bar<D: DrawTarget<Color=Rgb565>>(
    display: &mut D, active: Screen,
) -> Result<(), D::Error> {
    Rectangle::new(Point::new(0, NAV_TOP), Size::new(DISP_W as u32, NAV_H as u32))
        .draw_styled(
            &PrimitiveStyleBuilder::new().fill_color(theme::theme().nav).build(),
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
                    .fill_color(if is_active { theme::theme().nav_active } else { theme::theme().nav_inactive })
                    .build(),
                display,
            )?;
        let label = sanitize_text(screen.label());
        draw_text(
            display, &label,
            x + (NAV_BTN_W - (label.len() as i32 * 9)) / 2, NAV_TOP + 14,
            &FONT_6X10,
            if is_active { theme::theme().text } else { theme::theme().text_muted },
        )?;
    }
    Ok(())
}

// ============================================================================
// Utilities & Formatting
// ============================================================================

pub(crate) fn format_uptime(total_secs: u64) -> heapless::String<32> {
    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let mins = (total_secs % 3600) / 60;
    let mut s = heapless::String::new();
    if days > 0 {
        write!(&mut s, "{}d {:02}h {:02}m", days, hours, mins).ok();
    } else {
        write!(&mut s, "{:02}h {:02}m", hours, mins).ok();
    }
    s
}

pub(crate) fn fmt_1dp_w(val: f32) -> alloc::string::String {
    let i = val as i32;
    let frac = (val.abs() - (val as i32).abs() as f32) * 10.0 + 0.5;
    format!("{}.{}", i, frac as u32 % 10)
}

pub(crate) fn fmt_0dp_w(val: f32) -> alloc::string::String {
    format!("{}", (if val >= 0.0 { val + 0.5 } else { val - 0.5 }) as i32)
}

pub(crate) fn fmt_2dp_w(val: f32) -> alloc::string::String {
    let i = val as i32;
    let frac = (val.abs() - (val as i32).abs() as f32) * 100.0 + 0.5;
    format!("{}.{:02}", i, frac as u32 % 100)
}

fn format_time(state: &AppState) -> heapless::String<32> {
    let data = state.read();
    if let Some(t) = data.local_time {
        let mut s = heapless::String::new();
        write!(&mut s, "{:02}:{:02}:{:02}", t.hour, t.minute, t.second).ok();
        s
    } else {
        heapless::String::try_from("--:--:--").unwrap()
    }
}

// ============================================================================
// Main Display Task
// ============================================================================

pub async fn display_task(state: &'static AppState) {
    defmt::info!("Display: task spawned");
    let p = unsafe { Peripherals::steal() };

    let mut backlight = Output::new(p.GPIO38, Level::High, OutputConfig::default());
    defmt::info!("Display: backlight on");

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

    let spi = Spi::new(spi_dev, SpiConfig::default().with_frequency(Rate::from_mhz(10)))
        .unwrap()
        .with_sck(sclk)
        .with_mosi(mosi)
        .with_miso(miso)
        .with_dma(p.DMA_CH0)
        .with_buffers(dma_rx, dma_tx);

    let spi_bus = AtomicCell::new(spi);

    let mut touch_dev = AtomicDevice::new_no_delay(
        &spi_bus, Output::new(cs_touch, Level::High, OutputConfig::default()),
    ).unwrap();
    let display_dev = AtomicDevice::new_no_delay(
        &spi_bus, Output::new(cs_disp, Level::High, OutputConfig::default()),
    ).unwrap();

    let mut buf = [0u8; 4096];
    let di = SpiInterface::new(
        display_dev, Output::new(dc, Level::High, OutputConfig::default()), &mut buf,
    );
    let mut delay = Delay::new();

    let mut reset_pin = Output::new(rst, Level::High, OutputConfig::default());
    _ = reset_pin.set_low();
    delay.delay_ms(50);
    _ = reset_pin.set_high();
    delay.delay_ms(50);

    let mut display = match Builder::new(ILI9341Rgb565, di)
        .reset_pin(reset_pin)
        .display_size(DISP_W, DISP_H)
        .orientation(Orientation::new())
        .color_order(ColorOrder::Bgr)
        .invert_colors(ColorInversion::Inverted)
        .init(&mut delay)
    {
        Ok(d) => d,
        Err(e) => {
            defmt::warn!("Display: ILI9341 init failed: {:?}", defmt::Debug2Format(&e));
            return;
        }
    };

    defmt::info!("Display: ILI9341 init complete");
    unsafe { display.dcs().write_raw(0x11, &[]).ok(); }
    delay.delay_ms(150);
    unsafe { display.dcs().write_raw(0x29, &[]).ok(); }
    delay.delay_ms(50);

    display
        .set_pixels(
            0, 0, DISP_W - 1, DISP_H - 1,
            core::iter::repeat(Rgb565::BLACK).take(PIXEL_COUNT),
        )
        .ok();

    Timer::after(Duration::from_secs(5)).await;

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
        }

        if let Some((tx, ty)) = touch::read_touch(&mut touch_dev) {
            last_touch = Instant::now();
            if !backlight_on {
                backlight_on = true;
                _ = backlight.set_high();
            } else if !touch::handle_nav_touch(tx, ty, state) {
                touch::handle_theme_toggle(tx, ty, state);
            }
        }

        if backlight_on {
            let mut fb_display = DisplayBuffer;

            fb_display
                .fill_solid(
                    &Rectangle::new(
                        Point::new(0, 0),
                        Size::new(DISP_W as u32, DISP_H as u32),
                    ),
                    theme::theme().bg,
                )
                .ok();
            let time_str = format_time(state);
            draw_status_bar(&mut fb_display, &time_str, state.read().wifi_connected).ok();

            match state.read().active_screen {
                Screen::Weather => screens::weather::draw_weather_screen(&mut fb_display, state),
                Screen::Sensors => screens::sensors::draw_sensors_screen(&mut fb_display, state),
                Screen::Vps => screens::vps::draw_vps_screen(&mut fb_display, state),
                Screen::Host => screens::host::draw_host_screen(&mut fb_display, state),
            }
                .ok();

            draw_nav_bar(&mut fb_display, state.read().active_screen).ok();

            let fb = unsafe { (*core::ptr::addr_of_mut!(FB)).as_ref().unwrap().as_ref() };
            for cy in 0..16 {
                let y0 = cy * 20;
                let y1 = (y0 + 19).min(DISP_H - 1);
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
                display
                    .set_pixels(0, y0, DISP_W - 1, y1, iter)
                    .ok();
                if cy < 15 {
                    Timer::after(Duration::from_millis(10)).await;
                }
            }
        }
    }
}
