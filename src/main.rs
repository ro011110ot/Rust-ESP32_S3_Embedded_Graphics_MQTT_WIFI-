/// ESP32-S3 async dashboard — main entry point and shared state.
///
/// This file ties together the display, networking, LED, and time modules.
/// It defines the global `AppState` that all async tasks read/write via
/// a `critical_section::Mutex` for safe interior mutability.
///
/// The `#[main]` macro (from `esp_hal` with embassy feature) sets up the
/// async executor, initialises HW peripherals, and spawns three tasks:
///   1. `network_task`  — WiFi, MQTT, NTP, weather fetch
///   2. `display_task`  — ILI9341, XPT2046 touch, auto-dimming
///   3. `led_task`      — RGB NeoPixel status LED
///
/// # Security
/// All credentials come from compile-time env vars injected by `build.rs`.
/// The `mask!()` macro hides secrets in log output.

#![no_std]
#![no_main]

// ===========================================================================
// External crate imports
// ===========================================================================

use core::cell::RefCell;

use critical_section::Mutex as CsMutex;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use heapless::Vec;

// esp-hal entry point macro and peripheral initialisation.
use esp_hal::{
    clock::ClockControl,
    embassy,
    gpio::{IO, Output, PushPull},
    peripherals::Peripherals,
    prelude::*,
    system::SystemControl,
    timer::timg::TimerGroup,
};

// ===========================================================================
// Module declarations
// ===========================================================================

mod display;
mod led;
mod net;
mod time;

// ===========================================================================
// Shared application state
// ===========================================================================

/// Sensor reading: label + value string (e.g. "Temp" + "23.5 C").
#[derive(Debug, Clone, Default)]
pub struct SensorReading {
    pub label: heapless::String<32>,
    pub value: heapless::String<16>,
}

/// Weather data from OpenWeatherMap API.
#[derive(Debug, Clone, Default)]
pub struct WeatherData {
    pub temp: f32,
    pub humidity: f32,
    pub wind: f32,
    pub pressure: f32,
    pub desc: heapless::String<64>,
    pub icon: heapless::String<8>,
}

/// VPS monitoring data (remote server).
#[derive(Debug, Clone, Default)]
pub struct VpsData {
    pub cpu_pct: f32,
    pub ram_pct: f32,
    pub disk_pct: f32,
    pub uptime_secs: u64,
}

/// Host monitoring data (local machine).
#[derive(Debug, Clone, Default)]
pub struct HostData {
    pub cpu: [f32; 4],
    pub cpu_temp: f32,
    pub ram_pct: f32,
    pub ssd_temp: f32,
    pub net_down: f32,
}

/// The actual data stored behind the Mutex.
struct AppStateInner {
    pub sensors: Vec<SensorReading, 8>,
    pub vps: Option<VpsData>,
    pub host: Option<HostData>,
    pub weather: Option<WeatherData>,
    pub wifi_connected: bool,
    pub active_screen: display::Screen,
    pub local_time: Option<time::LocalTime>,
}

/// Thread-safe wrapper around the entire application state.
///
/// All methods use `critical_section` to guarantee exclusive access.
/// This is lightweight enough for an MCU — at 240 MHz the critical
/// section is typically <1 µs.
pub struct AppState {
    inner: CsMutex<RefCell<AppStateInner>>,
}

impl AppState {
    /// Create a new AppState with default values.
    pub const fn new() -> Self {
        Self {
            inner: CsMutex::new(RefCell::new(AppStateInner {
                sensors: Vec::new(),
                vps: None,
                host: None,
                weather: None,
                wifi_connected: false,
                active_screen: display::Screen::Weather,
                local_time: None,
            })),
        }
    }

    /// Read a snapshot of the current state.
    pub fn read(&self) -> AppStateSnapshot {
        critical_section::with(|cs| {
            let inner = self.inner.borrow_ref(cs);
            AppStateSnapshot {
                sensors: inner.sensors.clone(),
                vps: inner.vps,
                host: inner.host,
                weather: inner.weather.clone(),
                wifi_connected: inner.wifi_connected,
                active_screen: inner.active_screen,
                local_time: inner.local_time,
            }
        })
    }

    /// Update the Wi-Fi connected flag.
    pub fn set_wifi_connected(&self, v: bool) {
        critical_section::with(|cs| {
            self.inner.borrow_ref_mut(cs).wifi_connected = v;
        });
    }

    /// Set the active screen.
    pub fn set_active_screen(&self, s: display::Screen) {
        critical_section::with(|cs| {
            self.inner.borrow_ref_mut(cs).active_screen = s;
        });
    }

    /// Store VPS data.
    pub fn set_vps(&self, cpu: f32, ram: f32, disk: f32, uptime: u64) {
        critical_section::with(|cs| {
            let inner = &mut *self.inner.borrow_ref_mut(cs);
            inner.vps = Some(VpsData {
                cpu_pct: cpu,
                ram_pct: ram,
                disk_pct: disk,
                uptime_secs: uptime,
            });
        });
    }

    /// Store Host data.
    pub fn set_host(
        &self,
        cpu: [f32; 4],
        cpu_temp: f32,
        ram: f32,
        ssd_temp: f32,
        net_down: f32,
    ) {
        critical_section::with(|cs| {
            let inner = &mut *self.inner.borrow_ref_mut(cs);
            inner.host = Some(HostData {
                cpu,
                cpu_temp,
                ram_pct: ram,
                ssd_temp,
                net_down,
            });
        });
    }

    /// Add or update a sensor reading.
    pub fn add_sensor(&self, label: &str, value: &str) {
        critical_section::with(|cs| {
            let inner = &mut *self.inner.borrow_ref_mut(cs);
            // Try to update existing sensor with same label
            for s in inner.sensors.iter_mut() {
                if s.label.as_str() == label {
                    s.value = heapless::String::from(value);
                    return;
                }
            }
            // Otherwise append (up to capacity)
            if inner.sensors.len() < inner.sensors.capacity() {
                let _ = inner.sensors.push(SensorReading {
                    label: heapless::String::try_from(label).unwrap_or_default(),
                    value: heapless::String::try_from(value).unwrap_or_default(),
                });
            }
        });
    }

    /// Store the local time (from NTP + DST).
    pub fn set_local_time(&self, t: time::LocalTime) {
        critical_section::with(|cs| {
            self.inner.borrow_ref_mut(cs).local_time = Some(t);
        });
    }

    /// Store weather data.
    pub fn set_weather(
        &self,
        temp: f32,
        humidity: f32,
        wind: f32,
        pressure: f32,
        desc: &str,
        icon: &str,
    ) {
        critical_section::with(|cs| {
            let inner = &mut *self.inner.borrow_ref_mut(cs);
            inner.weather = Some(WeatherData {
                temp,
                humidity,
                wind,
                pressure,
                desc: heapless::String::try_from(desc).unwrap_or_default(),
                icon: heapless::String::try_from(icon).unwrap_or_default(),
            });
        });
    }
}

/// A snapshot of the AppState at a given moment (read atomically).
#[derive(Debug, Clone)]
pub struct AppStateSnapshot {
    pub sensors: Vec<SensorReading, 8>,
    pub vps: Option<VpsData>,
    pub host: Option<HostData>,
    pub weather: Option<WeatherData>,
    pub wifi_connected: bool,
    pub active_screen: display::Screen,
    pub local_time: Option<time::LocalTime>,
}

// ===========================================================================
// LED task
// ===========================================================================

/// Asynchronous task that drives the RGB NeoPixel (GPIO 48).
///
/// Listens on the global `LED_QUEUE` for commands and updates the
/// physical LED colour accordingly.
#[embassy_executor::task]
async fn led_task() {
    // A static LED flag so we know the current state.
    // The actual RMT + SmartLedsAdapter init would happen here using
    // peripherals passed from main():
    //
    //   let rmt = ...;
    //   let mut led = crate::led::init_led(rmt, 0);
    //
    // For now, we show the polling loop.

    log::info!("LED: task started");

    loop {
        // Check for new commands.
        if let Some(cmd) = crate::led::recv_led_command() {
            let rgb = crate::led::command_to_rgb(cmd);
            // led.write(core::iter::once(rgb)).ok();
            log::debug!("LED: cmd={:?} -> RGB({},{},{})", cmd, rgb.r, rgb.g, rgb.b);
        }
        Timer::after(Duration::from_millis(100)).await;
    }
}

// ===========================================================================
// Main entry point
// ===========================================================================

/// The async entry point, run by the embassy executor.
///
/// Initialises peripherals, starts the smoltcp WiFi stack, and spawns
/// the three main tasks.  The main function itself becomes the idle task
/// once spawning is done.
#[esp_hal::main]
async fn main(spawner: Spawner) -> ! {
    // Initialise the HAL (clock, PSRAM, etc.).
    let peripherals = esp_hal::init(esp_hal::Config::default());

    // Set up the embassy time driver using TIMG0.
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    embassy::init(timg0.timer0);

    log::info!("System: ESP32-S3 initialised");

    // Allocate a second timer group for WiFi (esp-wifi needs a timer).
    let timg1 = TimerGroup::new(peripherals.TIMG1);

    // ------------------------------------------------------------------
    // Initialise Wi-Fi + the smoltcp network stack.
    // ------------------------------------------------------------------
    let (wifi_ctrl, wifi_dev) = esp_wifi::initialize(
        esp_wifi::EspWifiInitFor::Wifi,
        timg1.timer0,
        esp_hal::rng::Rng::new(peripherals.RNG),
        peripherals.RADIO_CLK,
    )
    .expect("esp-wifi initialisation failed");

    let wifi_ctrl = esp_wifi::wifi::WifiController::new(wifi_ctrl);
    let wifi_dev = esp_wifi::wifi::WifiDevice::new(wifi_dev);

    log::info!("WiFi: initialised");

    // ------------------------------------------------------------------
    // Create the global shared state.
    // ------------------------------------------------------------------
    static STATE: AppState = AppState::new();
    let state: &'static AppState = &STATE;

    // ------------------------------------------------------------------
    // Spawn the three main async tasks.
    // ------------------------------------------------------------------
    spawner
        .spawn(network_task(wifi_ctrl, wifi_dev, state))
        .expect("failed to spawn network task");

    spawner
        .spawn(display_task(state))
        .expect("failed to spawn display task");

    spawner
        .spawn(led_task())
        .expect("failed to spawn LED task");

    log::info!("System: all tasks spawned");

    // The main function runs as an idle task — we just loop with a
    // periodic yield.  Embassy manages all spawned tasks.
    loop {
        Timer::after(Duration::from_secs(10)).await;
    }
}

// ===========================================================================
// Task declarations (re-exports for clarity)
// ===========================================================================

/// Network task: WiFi, MQTT, NTP, weather HTTP.
#[embassy_executor::task]
async fn network_task(
    wifi_ctrl: esp_wifi::wifi::WifiController<'static>,
    wifi_dev: esp_wifi::wifi::WifiDevice<'static>,
    state: &'static AppState,
) {
    crate::net::network_task(wifi_ctrl, wifi_dev, state).await
}

/// Display task: ILI9341, XPT2046 touch, auto-dimming.
#[embassy_executor::task]
async fn display_task(state: &'static AppState) {
    crate::display::display_task(state).await
}

// ===========================================================================
// Panic handler and logging
// ===========================================================================

// esp-backtrace provides a panic handler that prints the panic message
// and a backtrace via the USB serial JTAG port.
use esp_backtrace as _;
// esp-println provides the log!() / println!() macros for USB serial.
use esp_println as _;

// Ensure log messages are printed via esp-println.
#[export_name = "log_impl"]
fn log_impl(record: &log::Record) {
    esp_println::println!(
        "[{}][{}] {}",
        record.level(),
        record.target(),
        record.args()
    );
}
