use smart_leds_trait::RGB8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedCommand {
    WifiConnecting,
    MqttConnecting,
    WeatherFetching,
    Connected,
    Error,
}

pub fn command_to_rgb(cmd: LedCommand) -> RGB8 {
    match cmd {
        LedCommand::WifiConnecting => RGB8::new(0, 0, 255),
        LedCommand::MqttConnecting => RGB8::new(255, 128, 0),
        LedCommand::WeatherFetching => RGB8::new(255, 32, 128),
        LedCommand::Connected => RGB8::new(0, 0, 0),
        LedCommand::Error => RGB8::new(255, 0, 0),
    }
}

pub fn blink_interval_ms(cmd: LedCommand) -> Option<u64> {
    match cmd {
        LedCommand::WifiConnecting => Some(250),
        LedCommand::MqttConnecting => Some(500),
        LedCommand::WeatherFetching => None,
        LedCommand::Connected => None,
        LedCommand::Error => Some(500),
    }
}

use heapless::spsc::Queue;

static LED_QUEUE: critical_section::Mutex<core::cell::RefCell<Queue<LedCommand, 8>>> =
    critical_section::Mutex::new(core::cell::RefCell::new(Queue::new()));

pub fn send_led_command(cmd: LedCommand) {
    critical_section::with(|cs| {
        LED_QUEUE.borrow_ref_mut(cs).enqueue(cmd).ok();
    });
}

pub fn recv_led_command() -> Option<LedCommand> {
    critical_section::with(|cs| LED_QUEUE.borrow_ref_mut(cs).dequeue())
}
