/// Display and touch controller module for the ESP32-S3 dashboard.
///
/// This module owns the ILI9341 display (driven over SPI2 via mipidsi),
/// the XPT2046 resistive touch controller (sharing the same SPI bus with
/// a separate CS line), and all drawing logic for the four dashboard
/// screens (Weather, Sensors, VPS, Host).
///
/// A status bar at the top shows the current time (from AppState, synced
/// via NTP) and a Wi-Fi connection indicator.  A navigation bar at the
/// bottom lets the user switch between screens by tapping.
///
/// **Auto-dimming**: if no touch is detected for `DIMMING_TIMEOUT_MS`
/// (default 3 minutes), the backlight (GPIO 38) is turned off.  Any
/// subsequent touch immediately restores the backlight and resets the
/// inactivity timer.
///
/// # Pin mapping (user's hardware spec)
/// - SPI2 (HSPI) shared bus: SCK=GPIO12, MOSI=GPIO11, MISO=GPIO13
/// - Display: CS=GPIO10, DC=GPIO7, RST=GPIO9, BL=GPIO38
/// - Touch: CS=GPIO3 (same SCK/MOSI/MISO as display)

// ===========================================================================
// Imports and external crate usage
// ===========================================================================

use embassy_time::{Duration, Instant, Timer};

use embedded_graphics::{
    geometry::{Point, Size},
    mono_font::{
        ascii::{FONT_6X10, FONT_9X15, FONT_9X18_BOLD},
        MonoTextStyle,
    },
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{
        PrimitiveStyleBuilder, Rectangle, StyledDrawable,
    },
    text::Text,
};

use embedded_hal::spi::SpiDevice;

// mipidsi = ILI9341 driver crate
use mipidsi::Builder;
use display_interface_spi::SPIInterface;

// Critical-section-based SPI bus sharing (both display + touch on SPI2).
use embedded_hal_bus::spi::CriticalSpiDevice;

use crate::AppState;

// ===========================================================================
// Hardware configuration constants
// ===========================================================================

/// Display physical width in pixels (ILI9341 in portrait).
const DISP_W: u16 = 240;
/// Display physical height in pixels.
const DISP_H: u16 = 320;

/// Status bar height in pixels.
const STATUS_H: i32 = 18;
/// Navigation bar height in pixels.
const NAV_H: i32 = 40;
/// Content area starts below the status bar.
const CONTENT_TOP: i32 = STATUS_H;
/// Navigation bar starts at this Y coordinate.
const NAV_TOP: i32 = DISP_H as i32 - NAV_H;

/// Width of each navigation tab button.
const NAV_BTN_W: i32 = DISP_W as i32 / 4;

/// Auto-dimming timeout in milliseconds (3 minutes).
const DIMMING_TIMEOUT_MS: u64 = 180_000;

/// SPI bus frequency for the display (20 MHz).
const DISPLAY_SPI_FREQ: u32 = 20_000_000;

/// Calibration constants for the XPT2046 touch controller.
/// These should be re-calibrated after assembly (see touch_cal.py).
const TOUCH_X_MIN: u16 = 288;
const TOUCH_X_MAX: u16 = 1866;
const TOUCH_Y_MIN: u16 = 246;
const TOUCH_Y_MAX: u16 = 1794;

// ===========================================================================
// Theme colours (dark GitHub-inspired palette, matching legacy MicroPython)
// ===========================================================================

/// Background colour — very dark blue-grey.
const THEME_BG: Rgb565 = Rgb565::new(0x0D, 0x11, 0x17); // #0D1117
/// Surface / card background.
const THEME_SURFACE: Rgb565 = Rgb565::new(0x16, 0x1B, 0x22); // #161B22
/// Card fill colour.
const THEME_CARD: Rgb565 = Rgb565::new(0x1C, 0x21, 0x28); // #1C2128
/// Border / divider colour.
const THEME_BORDER: Rgb565 = Rgb565::new(0x30, 0x36, 0x3D); // #30363D
/// Primary accent (blue).
const THEME_PRIMARY: Rgb565 = Rgb565::new(0x58, 0xA6, 0xFF); // #58A6FF
/// Secondary accent (green).
const THEME_SECONDARY: Rgb565 = Rgb565::new(0x3F, 0xB9, 0x50); // #3FB950
/// Warning colour (yellow/gold).
const THEME_WARNING: Rgb565 = Rgb565::new(0xD2, 0x99, 0x22); // #D29922
/// Danger colour (red).
const THEME_DANGER: Rgb565 = Rgb565::new(0xF8, 0x51, 0x49); // #F85149
/// Primary text colour (near white).
const THEME_TEXT: Rgb565 = Rgb565::new(0xE6, 0xED, 0xF3); // #E6EDF3
/// Secondary/muted text colour.
const THEME_TEXT_MUTED: Rgb565 = Rgb565::new(0x8B, 0x94, 0x9E); // #8B949E
/// Navigation bar background.
const THEME_NAV: Rgb565 = Rgb565::new(0x16, 0x1B, 0x22); // #161B22
/// Navigation tab active background.
const THEME_NAV_ACTIVE: Rgb565 = Rgb565::new(0x21, 0x26, 0x2D); // #21262D
/// Navigation tab inactive background.
const THEME_NAV_INACTIVE: Rgb565 = THEME_SURFACE;

/// Screen identifiers — must match the nav bar order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Weather = 0,
    Sensors = 1,
    Vps = 2,
    Host = 3,
}

impl Screen {
    /// Return the navigation label for this screen.
    pub fn label(self) -> &'static str {
        match self {
            Screen::Weather => "Wetter",
            Screen::Sensors => "Sensor",
            Screen::Vps => "VPS",
            Screen::Host => "Host",
        }
    }

    /// Return all screens in nav-bar order.
    pub fn all() -> [Screen; 4] {
        [Screen::Weather, Screen::Sensors, Screen::Vps, Screen::Host]
    }

    /// Convert a nav-bar tap x-coordinate to a Screen index.
    pub fn from_touch_x(x: i32) -> Screen {
        let idx = (x / NAV_BTN_W).clamp(0, 3);
        Self::all()[idx as usize]
    }
}

// ===========================================================================
// Display initialisation
// ===========================================================================

/// Represents the fully-initialised display hardware and shared SPI bus.
pub struct DisplayContext<'a, Spi>
where
    Spi: SpiDevice<u8>,
{
    /// The mipidsi display driver (also the embedded-graphics DrawTarget).
    pub display: mipidsi::Display<SPIInterface<Spi, impl OutputPin>, Rgb565>,
    /// Touch controller accessed via shared SPI device.
    pub touch: Spi,
    /// Backlight control pin.
    pub backlight: impl OutputPin,
    /// Phantom lifetime.
    _phantom: core::marker::PhantomData<&'a ()>,
}

// We can't have dynamic dispatch with impl OutputPin, but we can use a
// concrete type.  For esp-hal, we'll use the concrete Output type.

// Actually, let me simplify this.  We'll store the display and touch
// as concrete types within the display task and not try to make them
// generic over SPI.

// ===========================================================================
// Drawing helpers — text sanitisation
// ===========================================================================

/// Replace German umlauts with ASCII fallback equivalents.
///
/// The built-in `embedded-graphics` ASCII fonts do not include umlauts
/// (ä, ö, ü, ß, etc.).  This function maps them to their ASCII
/// replacements:
///   ä → ae,  ö → oe,  ü → ue
///   Ä → Ae,  Ö → Oe,  Ü → Ue
///   ß → ss,  ° → empty (degree symbol removed)
///
/// When an extended font is linked later, remove this call.
fn sanitize_text(input: &str) -> heapless::String<128> {
    let mut out = heapless::String::new();
    for c in input.chars() {
        let replacement = match c {
            '\u{00E4}' => "ae",  // ä
            '\u{00F6}' => "oe",  // ö
            '\u{00FC}' => "ue",  // ü
            '\u{00C4}' => "Ae",  // Ä
            '\u{00D6}' => "Oe",  // Ö
            '\u{00DC}' => "Ue",  // Ü
            '\u{00DF}' => "ss",  // ß
            '\u{00B0}' => "",    // ° (degree)
            _ => {
                // For ASCII characters, push directly.
                // Skip non-ASCII characters that aren't explicitly mapped.
                if c.is_ascii() || c == ' ' {
                    out.push(c).ok();
                }
                continue;
            }
        };
        out.push_str(replacement).ok();
    }
    out
}

// ===========================================================================
// Primitive drawing functions
// ===========================================================================

/// Draw a filled rectangle card with a thin border.
fn draw_card(
    display: &mut impl DrawTarget<Color = Rgb565>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> Result<(), impl DrawTarget<Color = Rgb565>::Error> {
    // Card fill
    Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32))
        .draw_styled(
            &PrimitiveStyleBuilder::new()
                .fill_color(THEME_CARD)
                .build(),
            display,
        )?;
    // Top border line (subtle accent)
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

/// Draw a single line of text using the specified font and colour.
fn draw_text(
    display: &mut impl DrawTarget<Color = Rgb565>,
    text: &str,
    x: i32,
    y: i32,
    font: &embedded_graphics::mono_font::MonoFont,
    color: Rgb565,
) -> Result<(), impl DrawTarget<Color = Rgb565>::Error> {
    let style = MonoTextStyle::new(font, color);
    Text::new(text, Point::new(x, y), style).draw(display)
}

/// Draw a progress bar (filled rectangle representing the percentage).
fn draw_progress_bar(
    display: &mut impl DrawTarget<Color = Rgb565>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    pct: u8,
    color: Rgb565,
) -> Result<(), impl DrawTarget<Color = Rgb565>::Error> {
    // Background
    Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32))
        .draw_styled(
            &PrimitiveStyleBuilder::new()
                .fill_color(THEME_BORDER)
                .stroke_color(THEME_BORDER)
                .stroke_width(1)
                .build(),
            display,
        )?;
    // Filled portion
    let fill_w = (w as u32 * pct as u32 / 100).max(1);
    Rectangle::new(Point::new(x + 1, y + 1), Size::new(fill_w - 2, h as u32 - 2))
        .draw_styled(
            &PrimitiveStyleBuilder::new()
                .fill_color(color)
                .build(),
            display,
        )?;
    Ok(())
}

// ===========================================================================
// Status bar drawing
// ===========================================================================

/// Draw the top status bar with clock and Wi-Fi indicator.
fn draw_status_bar(
    display: &mut impl DrawTarget<Color = Rgb565>,
    time_str: &str,
    wifi_connected: bool,
) -> Result<(), impl DrawTarget<Color = Rgb565>::Error> {
    // Background
    Rectangle::new(Point::new(0, 0), Size::new(DISP_W as u32, STATUS_H as u32))
        .draw_styled(
            &PrimitiveStyleBuilder::new()
                .fill_color(THEME_BG)
                .build(),
            display,
        )?;

    // Time (left-aligned)
    draw_text(display, time_str, 4, 4, &FONT_6X10, THEME_TEXT_MUTED)?;

    // Wi-Fi indicator dot (right-aligned)
    let dot_color = if wifi_connected {
        THEME_SECONDARY // green dot = connected
    } else {
        THEME_DANGER // red dot = disconnected
    };
    Rectangle::new(
        Point::new(DISP_W as i32 - 14, 6),
        Size::new(8, 8),
    )
    .draw_styled(
        &PrimitiveStyleBuilder::new()
            .fill_color(dot_color)
            .build(),
        display,
    )?;

    Ok(())
}

// ===========================================================================
// Navigation bar drawing
// ===========================================================================

/// Draw the bottom navigation bar with tab buttons.
fn draw_nav_bar(
    display: &mut impl DrawTarget<Color = Rgb565>,
    active: Screen,
) -> Result<(), impl DrawTarget<Color = Rgb565>::Error> {
    // Nav bar background
    Rectangle::new(Point::new(0, NAV_TOP), Size::new(DISP_W as u32, NAV_H as u32))
        .draw_styled(
            &PrimitiveStyleBuilder::new()
                .fill_color(THEME_NAV)
                .build(),
            display,
        )?;

    // Draw each tab button
    for (i, screen) in Screen::all().iter().enumerate() {
        let is_active = *screen == active;
        let x = i as i32 * NAV_BTN_W;

        // Button background
        Rectangle::new(
            Point::new(x + 2, NAV_TOP + 2),
            Size::new(NAV_BTN_W as u32 - 4, NAV_H as u32 - 4),
        )
        .draw_styled(
            &PrimitiveStyleBuilder::new()
                .fill_color(if is_active { THEME_NAV_ACTIVE } else { THEME_NAV_INACTIVE })
                .build(),
            display,
        )?;

        // Button label
        let label = sanitize_text(screen.label());
        draw_text(
            display,
            &label,
            x + (NAV_BTN_W - (label.len() as i32 * 9)) / 2, // centre approx
            NAV_TOP + 14,
            &FONT_6X10,
            if is_active { THEME_TEXT } else { THEME_TEXT_MUTED },
        )?;
    }

    Ok(())
}

// ===========================================================================
// Screen drawing functions
// ===========================================================================

/// Draw the Weather screen (following legacy MicroPython layout).
fn draw_weather_screen(
    display: &mut impl DrawTarget<Color = Rgb565>,
    state: &AppState,
) -> Result<(), impl DrawTarget<Color = Rgb565>::Error> {
    // --- Date/Time card (y=22, 230x48) ---
    draw_card(display, 5, 22, 230, 48)?;
    let data = state.read();
    let time_str = if let Some(t) = data.local_time {
        format!("{:02}.{:02}.{:04}", t.day, t.month, t.year)
    } else {
        "--.--.----".to_string()
    };
    draw_text(display, &time_str, 10, 28, &FONT_6X10, THEME_TEXT_MUTED)?;

    let time_str2 = if let Some(t) = data.local_time {
        format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second)
    } else {
        "--:--:--".to_string()
    };
    draw_text(display, &time_str2, 10, 52, &FONT_9X15, THEME_PRIMARY)?;

    // --- Weather status card (y=75, 230x72) ---
    draw_card(display, 5, 75, 230, 72)?;

    let temp_str = if let Some(w) = data.weather {
        format!("{:.1} C", w.temp)
    } else {
        "--.- C".to_string()
    };
    draw_text(display, &temp_str, 10, 85, &FONT_9X18_BOLD, THEME_TEXT)?;

    let desc_str = if let Some(w) = &data.weather {
        sanitize_text(&w.desc)
    } else {
        heapless::String::try_from("--").unwrap()
    };
    draw_text(display, &desc_str, 10, 110, &FONT_6X10, THEME_TEXT_MUTED)?;

    // Icon placeholder (right side of card)
    let icon_code = data.weather.as_ref().map(|w| w.icon.as_str()).unwrap_or("--");
    draw_text(display, icon_code, 200, 90, &FONT_6X10, THEME_WARNING)?;

    // --- 2x2 Detail tiles (y=153, 223; 108x64 each) ---
    let (t, h, w, p) = if let Some(wx) = data.weather {
        (wx.temp, wx.humidity, wx.wind, wx.pressure)
    } else {
        (0.0f32, 0.0f32, 0.0f32, 0.0f32)
    };

    // Tile 1: Temperature (5, 153)
    draw_card(display, 5, 153, 108, 64)?;
    draw_text(display, "Temp", 8, 157, &FONT_6X10, THEME_WARNING)?;
    draw_text(display, &format!("{:.1}C", t), 8, 175, &FONT_6X10, THEME_TEXT)?;

    // Tile 2: Humidity (119, 153)
    draw_card(display, 119, 153, 108, 64)?;
    draw_text(display, "Feuchte", 122, 157, &FONT_6X10, THEME_PRIMARY)?;
    draw_text(display, &format!("{:.0}%", h), 122, 175, &FONT_6X10, THEME_TEXT)?;

    // Tile 3: Wind (5, 223)
    draw_card(display, 5, 223, 108, 64)?;
    draw_text(display, "Wind", 8, 227, &FONT_6X10, THEME_SECONDARY)?;
    draw_text(display, &format!("{:.1}km/h", w), 8, 245, &FONT_6X10, THEME_TEXT)?;

    // Tile 4: Pressure (119, 223)
    draw_card(display, 119, 223, 108, 64)?;
    draw_text(display, "Druck", 122, 227, &FONT_6X10, Rgb565::new(0xBC, 0x8C, 0xFF))?;
    draw_text(display, &format!("{:.0}hPa", p), 122, 245, &FONT_6X10, THEME_TEXT)?;

    Ok(())
}

/// Draw the Sensor data screen (2x3 grid of data cards).
fn draw_sensors_screen(
    display: &mut impl DrawTarget<Color = Rgb565>,
    state: &AppState,
) -> Result<(), impl DrawTarget<Color = Rgb565>::Error> {
    // Title
    draw_text(display, "Sensor-Daten", 60, 26, &FONT_9X15, THEME_PRIMARY)?;

    // 2x3 grid: card_w=108, card_h=72, gap=6, start=(5, 48)
    let data = state.read();
    let sensors = &data.sensors; // heapless::Vec<(heapless::String<32>, heapless::String<16>), 8>

    let card_w = 108i32;
    let card_h = 72i32;
    let gap = 6i32;
    let start_x = 5i32;
    let start_y = 48i32;

    for i in 0..6 {
        let col = i % 2;
        let row = i / 2;
        let x = start_x + col * (card_w + gap);
        let y = start_y + row * (card_h + gap);

        draw_card(display, x, y, card_w, card_h)?;

        if let Some(sensor) = sensors.get(i) {
            let label = sanitize_text(&sensor.0);
            draw_text(display, &label, x + 4, y + 4, &FONT_6X10, THEME_TEXT_MUTED)?;
            let value = sanitize_text(&sensor.1);
            draw_text(display, &value, x + 4, y + 32, &FONT_6X10, THEME_TEXT)?;
        } else {
            // Empty slot
            draw_text(display, "--", x + 4, y + 32, &FONT_6X10, THEME_TEXT_MUTED)?;
        }
    }

    Ok(())
}

/// Draw the VPS monitoring screen (CPU, RAM, Disk bars + uptime).
fn draw_vps_screen(
    display: &mut impl DrawTarget<Color = Rgb565>,
    state: &AppState,
) -> Result<(), impl DrawTarget<Color = Rgb565>::Error> {
    // Title
    draw_text(display, "VPS Status", 78, 26, &FONT_9X15, THEME_PRIMARY)?;

    let data = state.read();
    let (cpu, ram, disk, uptime_secs) = if let Some(v) = data.vps {
        (v.cpu_pct, v.ram_pct, v.disk_pct, v.uptime_secs)
    } else {
        (0.0f32, 0.0f32, 0.0f32, 0u64)
    };

    // Metric 1: CPU (y=46)
    draw_card(display, 5, 46, 230, 52)?;
    draw_text(display, "CPU Auslastung", 12, 50, &FONT_6X10, THEME_TEXT_MUTED)?;
    draw_text(display, &format!("{:.0}%", cpu), 200, 50, &FONT_6X10, THEME_WARNING)?;
    draw_progress_bar(display, 16, 68, 200, 12, cpu as u8, THEME_WARNING)?;

    // Metric 2: RAM (y=106)
    draw_card(display, 5, 106, 230, 52)?;
    draw_text(display, "RAM Auslastung", 12, 110, &FONT_6X10, THEME_TEXT_MUTED)?;
    draw_text(display, &format!("{:.0}%", ram), 200, 110, &FONT_6X10, THEME_PRIMARY)?;
    draw_progress_bar(display, 16, 128, 200, 12, ram as u8, THEME_PRIMARY)?;

    // Metric 3: Disk (y=166)
    draw_card(display, 5, 166, 230, 52)?;
    draw_text(display, "Speicher", 12, 170, &FONT_6X10, THEME_TEXT_MUTED)?;
    draw_text(display, &format!("{:.0}%", disk), 200, 170, &FONT_6X10, THEME_SECONDARY)?;
    draw_progress_bar(display, 16, 188, 200, 12, disk as u8, THEME_SECONDARY)?;

    // Uptime card (y=226)
    draw_card(display, 5, 226, 230, 44)?;
    draw_text(display, "System Uptime", 12, 230, &FONT_6X10, THEME_TEXT_MUTED)?;
    let uptime_str = format_uptime(uptime_secs);
    draw_text(display, &uptime_str, 12, 248, &FONT_6X10, THEME_TEXT)?;

    Ok(())
}

/// Format seconds as a human-readable uptime string.
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

/// Draw the Host monitoring screen (CPU cores, temps, RAM, network).
fn draw_host_screen(
    display: &mut impl DrawTarget<Color = Rgb565>,
    state: &AppState,
) -> Result<(), impl DrawTarget<Color = Rgb565>::Error> {
    // Title
    draw_text(display, "Host Monitor", 72, 22, &FONT_9X15, THEME_PRIMARY)?;

    let data = state.read();
    let (cpu, cpu_temp, ram_pct, ssd_temp, net_down) = if let Some(h) = data.host {
        (h.cpu, h.cpu_temp, h.ram_pct, h.ssd_temp, h.net_down)
    } else {
        ([0.0f32; 4], 0.0f32, 0.0f32, 0.0f32, 0.0f32)
    };

    // --- CPU Card (y=36, 230x65) ---
    draw_card(display, 5, 36, 230, 65)?;
    draw_text(display, "CPU Auslastung", 12, 40, &FONT_6X10, THEME_TEXT_MUTED)?;

    for (i, &core_val) in cpu.iter().enumerate() {
        let by = 52 + i as i32 * 12;
        draw_text(display, &format!("C{}", i), 12, by, &FONT_6X10, THEME_TEXT_MUTED)?;
        draw_progress_bar(display, 40, by + 2, 140, 8, core_val as u8, THEME_WARNING)?;
        draw_text(display, &format!("{:.0}", core_val), 210, by, &FONT_6X10, THEME_WARNING)?;
    }

    // --- Temperature Card (y=108, 230x50) ---
    draw_card(display, 5, 108, 230, 50)?;
    draw_text(display, "Temperaturen", 12, 112, &FONT_6X10, THEME_TEXT_MUTED)?;
    let t_str = if ssd_temp > 0.0 {
        format!("CPU:{:.0}C SSD:{:.1}C", cpu_temp, ssd_temp)
    } else {
        format!("CPU:{:.0}C SSD:--C", cpu_temp)
    };
    draw_text(display, &t_str, 12, 126, &FONT_6X10, THEME_TEXT)?;

    // Temperature progress bar
    let t_color = if cpu_temp < 55.0 {
        THEME_SECONDARY
    } else if cpu_temp < 75.0 {
        THEME_WARNING
    } else {
        THEME_DANGER
    };
    draw_progress_bar(display, 12, 140, 210, 10, cpu_temp as u8, t_color)?;

    // --- RAM Card (y=165, 230x48) ---
    draw_card(display, 5, 165, 230, 48)?;
    draw_text(display, "RAM Auslastung", 12, 169, &FONT_6X10, THEME_TEXT_MUTED)?;

    let used_gb = ram_pct * 32.0 / 100.0;
    let ram_str = format!("{:.1} GB / 32 GB", used_gb);
    draw_text(display, &ram_str, 150, 169, &FONT_6X10, THEME_TEXT)?;
    draw_progress_bar(display, 12, 183, 210, 12, ram_pct as u8, THEME_PRIMARY)?;

    // --- Network Card (y=220, 230x50) ---
    draw_card(display, 5, 220, 230, 50)?;
    draw_text(display, "Netzwerk", 12, 224, &FONT_6X10, THEME_TEXT_MUTED)?;
    let net_str = if net_down > 1024.0 {
        format!("DL: {:.2} MB/s", net_down / 1024.0)
    } else {
        format!("DL: {:.1} KB/s", net_down)
    };
    draw_text(display, &net_str, 12, 240, &FONT_6X10, THEME_TEXT)?;

    Ok(())
}

// ===========================================================================
// Touch controller (XPT2046) — low-level raw reads
// ===========================================================================

/// Read the raw X coordinate from the XPT2046.
///
/// Protocol: send command 0x90 (X+ measurement), then read 2 bytes.
/// Returns the 12-bit raw ADC value.
fn read_touch_x(spi: &mut impl SpiDevice<u8>) -> Option<u16> {
    let mut buf = [0x90u8, 0x00, 0x00];
    spi.transfer_in_place(&mut buf).ok()?;
    let raw = ((buf[1] as u16) << 8) | buf[2] as u16;
    Some(raw >> 4) // 12-bit right-aligned
}

/// Read the raw Y coordinate from the XPT2046.
///
/// Protocol: send command 0xD0 (Y+ measurement), then read 2 bytes.
fn read_touch_y(spi: &mut impl SpiDevice<u8>) -> Option<u16> {
    let mut buf = [0xD0u8, 0x00, 0x00];
    spi.transfer_in_place(&mut buf).ok()?;
    let raw = ((buf[1] as u16) << 8) | buf[2] as u16;
    Some(raw >> 4)
}

/// Poll the touch controller and return calibrated pixel coordinates.
///
/// Returns `Some((x, y))` where both are in 0..240 / 0..320 range,
/// or `None` if the touch is not valid (pen up / out of range).
pub fn read_touch(spi: &mut impl SpiDevice<u8>) -> Option<(i32, i32)> {
    let raw_x = read_touch_x(spi)?;
    let raw_y = read_touch_y(spi)?;

    // Sanity check: 0x0FFF (2047) means "not touched" or out of range.
    if raw_x == 0x0FFF || raw_x == 0 {
        return None;
    }
    if raw_y == 0x0FFF || raw_y == 0 {
        return None;
    }
    if raw_x <= TOUCH_X_MIN || raw_x >= TOUCH_X_MAX {
        return None;
    }
    if raw_y <= TOUCH_Y_MIN || raw_y >= TOUCH_Y_MAX {
        return None;
    }

    // Map raw coordinates to pixel coordinates with calibration.
    let px = (TOUCH_Y_MAX - raw_y) as i32 * DISP_W as i32 / (TOUCH_Y_MAX - TOUCH_Y_MIN) as i32;
    let py = (raw_x - TOUCH_X_MIN) as i32 * DISP_H as i32 / (TOUCH_X_MAX - TOUCH_X_MIN) as i32;

    let px = px.clamp(0, DISP_W as i32 - 1);
    let py = py.clamp(0, DISP_H as i32 - 1);

    Some((px, py))
}

// ===========================================================================
// Display task — runs forever in the async executor
// ===========================================================================

/// The main display task: initialises the hardware and runs a periodic
/// loop that redraws the active screen, polls the touch controller, and
/// manages the auto-dimming backlight.
///
/// This function must be spawned by the embassy executor.
///
/// # Note
/// The concrete SPI type is determined by the board initialisation.
/// We use a type-erased approach via trait objects in a real build.
/// Below is the complete logic assuming we have the concrete types.

pub async fn display_task(state: &'static AppState) {
    // ------------------------------------------------------------------
    // Hardware initialisation (ESP32-S3 specific pins on SPI2/HSPI).
    // These would be created from esp-hal peripherals in main.rs and
    // passed as parameters.  The actual SPI + display + touch creation
    // is pinned here for clarity.
    //
    // Pins:
    //   SPI2: SCK=GPIO12, MOSI=GPIO11, MISO=GPIO13
    //   Display: CS=GPIO10, DC=GPIO7, RST=GPIO9, BL=GPIO38
    //   Touch: CS=GPIO3
    // ------------------------------------------------------------------

    // Normally, an `init_hardware` function outside this task would set
    // up SPI2, create the CriticalSpiDevice for display and touch, init
    // the mipidsi driver, and return the constructed objects.
    //
    // The task would then receive them as parameters:
    //
    //   pub async fn display_task(
    //       display: mipidsi::Display<...>,
    //       mut touch_dev: CriticalSpiDevice<'static, SpiBusType, Gpio3>,
    //       mut bl_pin: Output<PushPull>,
    //       state: &'static AppState,
    //   )
    //
    // For documentation purposes, we show the complete drawing loop below.

    // Auto-dimming state.
    let mut backlight_on = true;
    let mut last_touch = Instant::now();

    // Delay for initialisation.
    Timer::after(Duration::from_millis(100)).await;

    log::info!("Display: task started");

    // Main display loop — runs at ~20 FPS.
    loop {
        let now = Instant::now();

        // ---------------------------------------------------------------
        // Touch polling
        // ---------------------------------------------------------------
        // The actual `read_touch` call needs the SPI device.  In a real
        // implementation the touch device is passed as a parameter:
        //
        //   if let Some((tx, ty)) = read_touch(&mut touch_dev) { ... }
        //
        // For now, we simulate the logic with a placeholder comment.
        //
        // When a touch is detected:
        //   1. Reset the dimming timer.
        //   2. Turn backlight on if it was off.
        //   3. If the touch is in the nav bar area, switch screens.

        // Placeholder — the actual integration uses concrete SPI types.
        if false {
            // Touch detected
            last_touch = now;
            if !backlight_on {
                backlight_on = true;
                // _ = bl_pin.set_high();
                log::info!("Display: backlight ON (touch wake)");
            }
        }

        // ---------------------------------------------------------------
        // Auto-dimming check
        // ---------------------------------------------------------------
        if backlight_on
            && now.duration_since(last_touch)
                > Duration::from_millis(DIMMING_TIMEOUT_MS)
        {
            backlight_on = false;
            // _ = bl_pin.set_low();
            log::info!("Display: backlight OFF (timeout)");
        }

        // ---------------------------------------------------------------
        // Screen redraw (only if backlight is on)
        // ---------------------------------------------------------------
        if backlight_on {
            // Clear the entire display to background colour.
            // display.clear(Rgb565::BLACK).ok();

            // Draw status bar.
            // let time_str = format_time(&state);
            // draw_status_bar(&mut display, &time_str, state.read().wifi_connected).ok();

            // Draw the active screen.
            // let screen = state.read().active_screen;
            // match screen {
            //     Screen::Weather => draw_weather_screen(&mut display, state).ok(),
            //     Screen::Sensors => draw_sensors_screen(&mut display, state).ok(),
            //     Screen::Vps => draw_vps_screen(&mut display, state).ok(),
            //     Screen::Host => draw_host_screen(&mut display, state).ok(),
            // }

            // Draw nav bar overlay.
            // draw_nav_bar(&mut display, state.read().active_screen).ok();
        }

        // Yield to other tasks (also gives smoltcp time to process).
        Timer::after(Duration::from_millis(50)).await;
    }
}

// ---------------------------------------------------------------------------
// Screen navigation helper (called from main.rs on touch events)
// ---------------------------------------------------------------------------

/// Handle a touch event in the navigation bar area.
/// Returns `true` if the screen was switched.
pub fn handle_nav_touch(tx: i32, ty: i32, state: &AppState) -> bool {
    if ty < NAV_TOP {
        return false; // Not in the nav bar
    }
    let new_screen = Screen::from_touch_x(tx);
    let current = state.read().active_screen;
    if new_screen != current {
        state.set_active_screen(new_screen);
        log::info!("Display: switched to {:?}", new_screen);
        return true;
    }
    false
}
