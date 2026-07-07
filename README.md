# ESP32-S3 Async Rust Dashboard

A production-ready, async Rust firmware for the **ESP32-S3** featuring a clean, responsive 240×320 dashboard on an **ILI9341** display with **XPT2046** resistive touch. It visualizes real-time data from MQTT sensors, a remote VPS, a local host monitor, and OpenWeatherMap APIs.

## Key Features

* **Four-Screen Dashboard Interface:** Cycle between Weather, Local Sensors, VPS Monitoring, and Host System metrics.
* **Fully Asynchronous Stack:** Utilizing `embassy-net` and `esp-hal`, it natively juggles WiFi, MQTT v3.1.1, NTP time sync, and HTTP REST requests concurrently.
* **Intelligent Auto-Dimming:** Preserves panel lifespan by switching off the backlight after 3 minutes of inactivity. Any screen touch wakes the device instantly.
* **Secure Credential Injection:** No hardcoded secrets. All WiFi, MQTT, and API credentials are injected via environment variables (`.env`) during compilation using `build.rs`.
* **RGB LED Status Feedback:** Utilizes the onboard WS2812B NeoPixel to indicate networking states (e.g., connecting, fetching data, stable connection, error).

---

## Hardware Configuration & Pinout

This project expects the display and touch controller to share **HSPI / SPI2** on the ESP32-S3.

| ESP32-S3 GPIO | ILI9341 Display | XPT2046 Touch | Cable Color |
| :--- | :--- | :--- | :--- |
| **GPIO 12** (SCK) | SCK | T_CLK | Yellow |
| **GPIO 11** (MOSI) | SDI | T_DIN | Green |
| **GPIO 13** (MISO) | SDO | T_DO | Orange |
| **GPIO 10** | CS | - | Blue |
| **GPIO 7** | DC | - | Violet |
| **GPIO 9** | RESET | - | Grey |
| **GPIO 38** | BL (Backlight) | - | White |
| **GPIO 3** | - | T_CS | Brown |
| **GPIO 48** | Data In (WS2812B LED) | - | - |

---

## Getting Started

### 1. Prerequisites & Toolchain Setup

You need the `xtensa` Rust toolchain. If you are operating on an Arch-based Linux distribution (like CachyOS or Manjaro), you can generally utilize your native package manager or the AUR, but the official `espup` tool is highly recommended:

```bash
cargo install espup
espup install
source ~/export-esp.sh
cargo install espflash
```

### 2. Environment Configuration

Copy the credential template and populate your variables:

```bash
cp .env_TEMPLATE .env
```

Provide the necessary credentials:
- **WiFi:** Supports up to 3 fallback networks.
- **MQTT Broker:** IP address, credentials, and topics.
- **OpenWeatherMap:** API key and location.

> The `.env` file is explicitly ignored in `.gitignore`. Never commit this file.

### 3. Build & Flash

Connect your ESP32-S3 and run:

```bash
cargo run --release
```

`cargo run` is mapped to `espflash flash --monitor` via `.cargo/config.toml`. The monitor
automatically decodes defmt log data from the USB-JTAG-Serial port, formatting it
alongside the ELF debug symbols for readable output.

### Defmt Logging

This project uses **defmt** (v1.1) as its logging framework. All log output is
transmitted via the USB-JTAG-Serial port using rzCOBS encoding and decoded by
`espflash --monitor`. External crate output (via the `log` crate) is still
supported through the `log-04` feature on `esp-println`.

## Project Architecture

```text
.
├── assets/
│   └── icons_png/          # Raw weather icon assets (PNG)
├── src/
│   ├── main.rs             # Entry point, allocator, AppState, task spawner
│   ├── display/
│   │   ├── mod.rs          # Framebuffer, Screen enum, UI primitives, display_task
│   │   ├── theme.rs        # Theme struct, light/dark themes, toggle
│   │   ├── touch.rs        # XPT2046 driver, touch reading, nav/theme handlers
│   │   └── screens/
│   │       ├── mod.rs
│   │       ├── weather.rs  # Weather screen with icon rendering
│   │       ├── sensors.rs  # Local sensor grid screen
│   │       ├── vps.rs      # VPS monitoring screen
│   │       └── host.rs     # Host system monitoring screen
│   ├── net/
│   │   ├── mod.rs          # network_task, MQTT data handling, dispatch
│   │   ├── wifi.rs         # WiFi credentials, mask_credential
│   │   ├── mqtt.rs         # MQTT packet encode/decode, payload parsers
│   │   └── services.rs     # NTP sync, OpenWeatherMap HTTP fetch
│   ├── led.rs              # WS2812B NeoPixel LED feedback
│   └── time.rs             # NTP-to-local time conversion (Europe/Berlin)
```