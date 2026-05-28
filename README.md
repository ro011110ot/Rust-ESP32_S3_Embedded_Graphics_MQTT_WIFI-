# ESP32-S3 Async Rust Dashboard

A production-ready, async Rust firmware for the **ESP32-S3** featuring a
dark-themed 240x320 dashboard on an **ILI9341** display with **XPT2046**
resistive touch, driving real-time data from MQTT sensors, a remote VPS,
a local host monitor, and OpenWeatherMap.

## Features

- **4-Screen Dashboard** — Weather, Sensors, VPS, Host (ported from legacy
  LVGL MicroPython layout)
- **Async Networking** — WiFi, MQTT v3.1.1, NTP time sync, and HTTP weather
  fetch all run in a single smoltcp-based async task
- **NTP + DST** — Auto-synchronises time via `pool.ntp.org` and computes
  CET/CEST (German daylight saving) using the EU DST rule
- **Auto-Dimming** — Turns off the display backlight after 3 minutes of
  inactivity; any touch wakes it instantly
- **RGB Status LED** — The onboard NeoPixel (GPIO 48) shows WiFi/MQTT
  connection state via colour
- **Secure Credentials** — All secrets injected at compile time via
  `build.rs` + `.env`; never embedded in source code
- **Touch Navigation** — Bottom tab bar for screen switching

## Hardware Requirements

| Component | Details |
|-----------|---------|
| MCU       | ESP32-S3 (QFN56, rev v0.2), 16 MB Flash, 8 MB PSRAM |
| Display   | ILI9341 SPI, 240×320 RGB565 |
| Touch     | XPT2046 Resistive Touch (shared SPI bus with display) |
| LED       | WS2812B NeoPixel RGB (GPIO 48) |

### Pin Configuration

All SPI devices share **HSPI / SPI2**:

| Signal    | GPIO | Connected To       |
|-----------|------|--------------------|
| SPI2 SCK  | 12   | Display SCK + T_CLK|
| SPI2 MOSI | 11   | Display SDI + T_DIN|
| SPI2 MISO | 13   | Display SDO + T_DO |
| Display CS| 10   | ILI9341 CS         |
| Display DC| 7    | ILI9341 DC         |
| Display RST| 9   | ILI9341 RESET      |
| Display BL| 38   | ILI9341 Backlight  |
| Touch CS  | 3    | XPT2046 T_CS       |
| RGB LED   | 48   | WS2812 Data In     |

## Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) (nightly toolchain)
- `espflash` for flashing: `cargo install espflash`
- LLVM tools for Xtensa target: `cargo install ldproxy` or use
  `espup` to install the full toolchain

### 1. Install the Xtensa target

```bash
rustup target add xtensa-esp32s3-none-elf
```

Or use [espup](https://github.com/esp-rs/espup) for a complete
toolchain:

```bash
cargo install espup
espup install
```

### 2. Configure credentials

Copy the template and fill in your values:

```bash
cp .env_TEMPLATE .env
```

Edit `.env` with your Wi-Fi SSID/password, MQTT broker, and
OpenWeatherMap API key:

```ini
WIFI_SSID_0=MyHomeNetwork
WIFI_PASS_0=MySecurePassword
OWM_API_KEY=your_openweathermap_api_key
OWM_CITY=Berlin
OWM_COUNTRY=DE
MQTT_BROKER=192.168.1.100
MQTT_PORT=1883
MQTT_USER=mqtt_user
MQTT_PASS=mqtt_password
MQTT_CLIENT_ID=ESP32-S3-Rust
```

> ⚠️ **Security**: `.env` is listed in `.gitignore` and must never be
> committed. The `.env_TEMPLATE` shows required keys with blank values.

### 3. Build and flash

```bash
cargo build --release
espflash flash target/xtensa-esp32s3-none-elf/release/esp32-s3-embedded-graphics-mqtt-wifi --monitor
```

Or use the cargo runner shorthand:

```bash
cargo run --release
```

### 4. First boot

On first boot the device will:
1. Connect to the first available Wi-Fi network
2. Sync time via NTP (pool.ntp.org)
3. Connect to the MQTT broker and subscribe to `Sensors/#`,
   `sensors/#`, `vps/monitor`, `host/monitor`
4. Display the Weather screen
5. The RGB LED turns solid green when fully connected

## MQTT Topics

| Topic             | Direction | Payload Example |
|:------------------|:----------|:----------------|
| `vps/monitor`     | Receive   | `{"cpu":12.5,"ram":45.3,"disk":67.1,"uptime":123456}` |
| `host/monitor`    | Receive   | `{"cpu":[34.3,45.2,12.1,78.9],"cpu_temp":91,"ram":34.6,"ssd_temp":30.85,"net_down":4.59}` |
| `sensors/#`       | Receive   | `{"temperature":23.5,"humidity":45}` or DS18B20 format |
| `Sensors/#`       | Receive   | Same as above (legacy topic) |

### Publishing your own data

Any MQTT client can publish to these topics. Example using `mosquitto_pub`:

```bash
mosquitto_pub -h broker.local -t "vps/monitor" \
  -m '{"cpu":23.4,"ram":56.7,"disk":78.9,"uptime":12345}'
```

## Architecture

```
src/
├── main.rs       # Entry point, AppState, executor, task spawning
├── display.rs    # ILI9341 init, XPT2046 touch, all 4 screens,
│                 #   auto-dimming, drawing primitives
├── net.rs        # WiFi, MQTT client, NTP, weather HTTP fetch,
│                 #   credential masking
├── led.rs        # WS2812 RGB LED driver, command queue
└── time.rs       # CET/CEST DST calculation, NTP→local conversion
```

### Data Flow

```
WiFi + MQTT (net.rs)
    │
    ▼
AppState (critical_section::Mutex<RefCell<...>>)
    │
    ├──▶ Display task (display.rs) — read & redraw every 50 ms
    │       │
    │       └──▶ Touch → nav switching, backlight wake
    │
    └──▶ LED task (led.rs) — read command queue → NeoPixel colour
```

## Screen Layouts

### Weather Screen
- Date/time card (top)
- Weather status card with temperature, description, icon
- 2×2 detail grid: temperature, humidity, wind, pressure

### Sensors Screen
- 2×3 grid of sensor cards (DHT11, DS18B20, etc.)
- Slot-mapped from MQTT sensor data

### VPS Screen
- CPU, RAM, disk usage progress bars
- System uptime display

### Host Screen
- 4 per-core CPU mini-bars
- CPU / SSD temperature with colour coding
- RAM usage bar with GB label
- Network download speed

## Auto-Dimming

The display backlight turns off after **3 minutes** of inactivity (no touch).
Any touch on the XPT2046 immediately restores the backlight and resets the
timer.  This is configurable via `DIMMING_TIMEOUT_MS` in `src/display.rs`.

## License

MIT — see [LICENSE](LICENSE).

## Acknowledgements

- Legacy MicroPython LVGL application that served as the UI reference
- [esp-rs](https://github.com/esp-rs) team for the amazing Rust HAL
- OpenWeatherMap for the weather data API
