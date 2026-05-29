#![no_std]
#![no_main]

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

use core::cell::RefCell;

use critical_section::Mutex as CsMutex;
use embassy_executor::Spawner;
use embassy_time::{Duration, Instant, Timer};
use esp_hal::{
    interrupt::software::SoftwareInterruptControl,
    timer::timg::TimerGroup,
};
use esp_hal_smartled::{
    buffer_size, color_order, Rgb8RmtSmartLeds, Timing,
};
use heapless::Vec;
use smart_leds_trait::{RGB8, SmartLedsWrite};

use core::alloc::GlobalAlloc;
use core::alloc::Layout;
use core::mem::MaybeUninit;
use esp_alloc::{HeapRegion, MemoryCapability};

struct EspAlloc;

unsafe impl GlobalAlloc for EspAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        esp_alloc::HEAP.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        esp_alloc::HEAP.dealloc(ptr, layout)
    }
}

#[global_allocator]
static A: EspAlloc = EspAlloc;



mod display;
mod led;
mod time;
mod net;

use crate::led::LedCommand;

/// Halved WS2812 timing to cancel the `* 2` workaround in
/// `esp-hal-smartled2` v0.28.2 (`zero_pulse`/`one_pulse`).
/// The driver's FIXME (line 357) multiplies all durations by 2,
/// which lengthens T0H from 350 ns to 700 ns — too long for a 0 bit.
/// By halving the base values, `(* 2) / 2 = 1` → correct timing.
struct Ws2812FixedTiming;
impl Timing for Ws2812FixedTiming {
    const TIME_0_HIGH: u16 = 175;
    const TIME_0_LOW: u16 = 350;
    const TIME_1_HIGH: u16 = 400;
    const TIME_1_LOW: u16 = 300;
}

#[derive(Debug, Clone, Default)]
pub struct SensorReading {
    pub label: heapless::String<32>,
    pub value: heapless::String<16>,
}

#[derive(Debug, Clone, Default)]
pub struct WeatherData {
    pub temp: f32,
    pub humidity: f32,
    pub wind: f32,
    pub pressure: f32,
    pub desc: heapless::String<64>,
    pub icon: heapless::String<8>,
}

#[derive(Debug, Clone, Default)]
pub struct VpsData {
    pub cpu_pct: f32,
    pub ram_pct: f32,
    pub disk_pct: f32,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Default)]
pub struct HostData {
    pub cpu: [f32; 4],
    pub cpu_temp: f32,
    pub ram_pct: f32,
    pub ssd_temp: f32,
    pub net_down: f32,
}

struct AppStateInner {
    pub sensors: Vec<SensorReading, 8>,
    pub vps: Option<VpsData>,
    pub host: Option<HostData>,
    pub weather: Option<WeatherData>,
    pub wifi_connected: bool,
    pub active_screen: display::Screen,
    pub local_time: Option<time::LocalTime>,
}

pub struct AppState {
    inner: CsMutex<RefCell<AppStateInner>>,
}

impl AppState {
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

    pub fn read(&self) -> AppStateSnapshot {
        critical_section::with(|cs| {
            let inner = self.inner.borrow_ref(cs);
            AppStateSnapshot {
                sensors: inner.sensors.clone(),
                vps: inner.vps.clone(),
                host: inner.host.clone(),
                weather: inner.weather.clone(),
                wifi_connected: inner.wifi_connected,
                active_screen: inner.active_screen,
                local_time: inner.local_time,
            }
        })
    }

    pub fn set_wifi_connected(&self, v: bool) {
        critical_section::with(|cs| {
            self.inner.borrow_ref_mut(cs).wifi_connected = v;
        });
    }

    pub fn set_active_screen(&self, s: display::Screen) {
        critical_section::with(|cs| {
            self.inner.borrow_ref_mut(cs).active_screen = s;
        });
    }

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

    pub fn set_host(
        &self, cpu: [f32; 4], cpu_temp: f32, ram: f32, ssd_temp: f32, net_down: f32,
    ) {
        critical_section::with(|cs| {
            let inner = &mut *self.inner.borrow_ref_mut(cs);
            inner.host = Some(HostData {
                cpu, cpu_temp, ram_pct: ram, ssd_temp, net_down,
            });
        });
    }

    pub fn add_sensor(&self, label: &str, value: &str) {
        critical_section::with(|cs| {
            let inner = &mut *self.inner.borrow_ref_mut(cs);
            for s in inner.sensors.iter_mut() {
                if s.label.as_str() == label {
                    s.value = heapless::String::try_from(value).unwrap_or_default();
                    return;
                }
            }
            if inner.sensors.len() < inner.sensors.capacity() {
                let _ = inner.sensors.push(SensorReading {
                    label: heapless::String::try_from(label).unwrap_or_default(),
                    value: heapless::String::try_from(value).unwrap_or_default(),
                });
            }
        });
    }

    pub fn set_local_time(&self, t: time::LocalTime) {
        critical_section::with(|cs| {
            self.inner.borrow_ref_mut(cs).local_time = Some(t);
        });
    }

    pub fn tick_local_time(&self) {
        critical_section::with(|cs| {
            if let Some(ref mut t) = self.inner.borrow_ref_mut(cs).local_time {
                t.second += 1;
                if t.second >= 60 {
                    t.second = 0;
                    t.minute += 1;
                    if t.minute >= 60 {
                        t.minute = 0;
                        t.hour += 1;
                        if t.hour >= 24 {
                            t.hour = 0;
                        }
                    }
                }
            }
        });
    }

    pub fn set_weather(
        &self, temp: f32, humidity: f32, wind: f32, pressure: f32,
        desc: &str, icon: &str,
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

#[embassy_executor::task]
async fn led_task() {
    let p = unsafe { esp_hal::peripherals::Peripherals::steal() };

    // Drive the LED data line LOW for > 280 us to reset the WS2812
    // before RMT takes over, avoiding an undefined power-on state.
    unsafe {
        use esp_hal::gpio::{Level, Output, OutputConfig};
        let periphs = esp_hal::peripherals::Peripherals::steal();
        let _reset = Output::new(
            periphs.GPIO48, Level::Low, OutputConfig::default(),
        );
        Timer::after(Duration::from_millis(1)).await;
    }

    let rmt = esp_hal::rmt::Rmt::new(p.RMT, esp_hal::time::Rate::from_mhz(80)).unwrap();
    let mut led =
        Rgb8RmtSmartLeds::<{ buffer_size::<RGB8>(1) }, _, color_order::Grb, Ws2812FixedTiming>::new(
            rmt.channel0, p.GPIO48,
        ).unwrap();
    log::info!("LED: task started");

    let mut current_cmd = LedCommand::WifiConnecting;
    let mut blink_on = true;
    let mut last_toggle = Instant::now();
    loop {
        if let Some(cmd) = led::recv_led_command() {
            current_cmd = cmd;
            blink_on = true;
            last_toggle = Instant::now();
        }
        let interval = led::blink_interval_ms(current_cmd);
        let rgb = if let Some(ms) = interval {
            let now = Instant::now();
            if now.duration_since(last_toggle) > Duration::from_millis(ms) {
                blink_on = !blink_on;
                last_toggle = now;
            }
            if blink_on { led::command_to_rgb(current_cmd) } else { RGB8::new(0, 0, 0) }
        } else {
            led::command_to_rgb(current_cmd)
        };
        if led.write(core::iter::once(rgb)).is_err() {
            log::warn!("LED: write error");
        }
        Timer::after(Duration::from_millis(50)).await;
    }
}

#[embassy_executor::task]
async fn net_runner_task(
    mut runner: embassy_net::Runner<'static, esp_radio::wifi::Interface<'static>>,
) {
    runner.run().await
}

#[embassy_executor::task]
async fn app_net_task(
    stack: embassy_net::Stack<'static>,
    wifi_ctrl: esp_radio::wifi::WifiController<'static>,
    state: &'static AppState,
) {
    crate::net::network_task(stack, wifi_ctrl, state).await
}

#[embassy_executor::task]
async fn display_task(state: &'static AppState) {
    crate::display::display_task(state).await
}

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    esp_println::logger::init_logger(log::LevelFilter::Info);

    unsafe {
        const HEAP_SIZE: usize = 160 * 1024;
        static mut HEAP_BUF: MaybeUninit<[u8; HEAP_SIZE]> = MaybeUninit::uninit();
        esp_alloc::HEAP.add_region(HeapRegion::new(
            core::ptr::addr_of_mut!(HEAP_BUF).cast::<u8>(),
            HEAP_SIZE,
            MemoryCapability::Internal.into(),
        ));
        // dram2_seg from memory.x: ORIGIN=0x3FCDB700, LEN=0x3FCED710-0x3FCDB700
        esp_alloc::HEAP.add_region(HeapRegion::new(
            0x3FCDB700usize as *mut u8,
            0x3FCED710usize - 0x3FCDB700usize,
            MemoryCapability::Internal.into(),
        ));
        // Allocate framebuffer before anything else uses the heap
        let fb = alloc::vec![0u8; display::FB_SIZE].into_boxed_slice();
        display::init_fb(fb);
    }

    let peripherals = esp_hal::init(
        esp_hal::Config::default().with_cpu_clock(esp_hal::clock::CpuClock::max()),
    );

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    log::info!("System: ESP32-S3 initialised");

    static STATE: AppState = AppState::new();
    let state: &'static AppState = &STATE;

    let rng_seed = esp_hal::rng::Rng::new().random() as u64;
    let controller_config = esp_radio::wifi::ControllerConfig::default();
    let (wifi_ctrl, interfaces) = esp_radio::wifi::new(
        peripherals.WIFI, controller_config,
    )
        .expect("esp-radio wifi init failed");

    // SAFETY: single-threaded; resources are only used by the network runner task
    static mut RESOURCES: embassy_net::StackResources<4> =
        embassy_net::StackResources::<4>::new();
    let (stack, runner) = embassy_net::new(
        interfaces.station,
        embassy_net::Config::dhcpv4(Default::default()),
        unsafe { &mut *core::ptr::addr_of_mut!(RESOURCES) },
        rng_seed,
    );

    spawner.spawn(net_runner_task(runner).expect("spawn net_runner"));
    spawner.spawn(app_net_task(stack, wifi_ctrl, state).expect("spawn app_net"));
    spawner.spawn(display_task(state).expect("spawn display"));
    spawner.spawn(led_task().expect("spawn led"));

    log::info!("System: all tasks spawned");

    loop {
        Timer::after(Duration::from_secs(10)).await;
    }
}

use esp_backtrace as _;
use esp_println as _;

#[export_name = "log_impl"]
fn log_impl(record: &log::Record) {
    esp_println::println!(
        "[{}][{}] {}", record.level(), record.target(), record.args()
    );
}
