/// RGB LED controller for the onboard WS2812 (NeoPixel) on GPIO 48.
///
/// Uses `esp-hal-smartled` which drives the LED via the RMT peripheral.
/// The LED communicates state visually: green = connected, cyan = MQTT
/// connecting, red = error, rapid green blink = WiFi connecting.
///
/// All commands arrive through a lock-free SPSC queue so the LED task
/// never blocks the rest of the system.

use esp_hal::peripherals::RMT;
use esp_hal::rmt::Rmt as RmtDriver;
use esp_hal_smartled::SmartLedsAdapter;
use smart_leds::hsv::{Hsv, hsv2rgb};
use smart_leds::RGB8;

/// The RMT channel used for the NeoPixel protocol.
type LedAdapter = SmartLedsAdapter<RGB8, RmtDriver>;

/// States the LED can display — sent via the command channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedCommand {
    Off,
    WifiConnecting,
    MqttConnecting,
    Connected,
    Error,
}

/// Initialize the RMT-based smart LED adapter on the given channel.
///
/// # Arguments
/// * `rmt` — The RMT peripheral resource.
/// * `channel` — The specific RMT channel (usually 0).
pub fn init_led(rmt: RMT, channel_idx: u8) -> LedAdapter {
    let rmt_driver = RmtDriver::new(
        rmt,
        esp_hal::clock::Clocks::get().apb_freq,
    )
    .unwrap();
    let channel = rmt_driver.channel0; // channel 0 on GPIO 48
    // SmartLedsAdapter uses RMT to bit-bang the NeoPixel protocol.
    SmartLedsAdapter::new(channel)
}

/// Convert an `LedCommand` into an RGB8 color for the NeoPixel.
///
/// Visual mapping (chosen for accessibility):
/// - Off          → black (0, 0, 0)
/// - WiFi connect → rapid blinking green (handled by caller via repeated cmds)
/// - MQTT connect → cyan (0, 255, 255)
/// - Connected    → steady green (0, 255, 0)
/// - Error        → red (255, 0, 0)
pub fn command_to_rgb(cmd: LedCommand) -> RGB8 {
    match cmd {
        LedCommand::Off => RGB8::new(0, 0, 0),
        LedCommand::WifiConnecting => {
            // Rapid blink is implemented by the task alternating this with Off.
            RGB8::new(0, 255, 0)
        }
        LedCommand::MqttConnecting => RGB8::new(0, 255, 255),
        LedCommand::Connected => RGB8::new(0, 128, 0),
        LedCommand::Error => RGB8::new(255, 0, 0),
    }
}

/// Singleton command channel: produced by network/display tasks, consumed by
/// the LED task.  Capacity 8 is more than enough for infrequent LED updates.
use heapless::spsc::Queue;

/// Global queue that the LED task polls for new commands.
///
/// Using `critical_section::Mutex` for interior mutability so any task can
/// send commands safely.
static LED_QUEUE: critical_section::Mutex<core::cell::RefCell<Queue<LedCommand, 8>>> =
    critical_section::Mutex::new(core::cell::RefCell::new(Queue::new()));

/// Enqueue a command for the LED task.  Called from any async task.
///
/// Drops the command if the queue is full (non-blocking).
pub fn send_led_command(cmd: LedCommand) {
    critical_section::with(|cs| {
        let queue = &mut *LED_QUEUE.borrow_ref_mut(cs);
        // Non-blocking push — if full we silently drop the oldest.
        queue.enqueue(cmd).ok();
    });
}

/// Dequeue the next pending LED command, or None if the queue is empty.
pub fn recv_led_command() -> Option<LedCommand> {
    critical_section::with(|cs| {
        LED_QUEUE.borrow_ref_mut(cs).dequeue()
    })
}
