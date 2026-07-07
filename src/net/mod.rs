// Macros must be defined before module declarations so they
// are visible to all child submodules.
macro_rules! env_or_panic {
    ($name:expr) => { env!($name) };
}

macro_rules! mask {
    ($val:expr) => { $crate::net::wifi::mask_credential($val) };
}

pub mod wifi;
pub mod mqtt;
pub mod services;


use embassy_net::dns::DnsQueryType;
use embassy_net::tcp::TcpSocket;
use embassy_net::{IpAddress, Stack};
use embassy_time::{Duration, Timer};

use crate::led::LedCommand;
use crate::AppState;

const NTP_INTERVAL_SECS: u64 = 4 * 3600;
const NTP_RETRY_SECS: u64 = 30;
const WEATHER_INTERVAL_SECS: u64 = 300;
const MQTT_PING_INTERVAL_SECS: u64 = 30;
const MAX_RECONNECT_FAILS: u32 = 5;

pub async fn network_task(
    stack: Stack<'static>,
    mut wifi_ctrl: esp_radio::wifi::WifiController<'static>,
    state: &'static AppState,
) -> ! {
    let mut reconnect_fails: u32 = 0;
    loop {
        led_command(LedCommand::WifiConnecting);
        let creds = wifi::get_wifi_credentials();
        let mut wifi_ok = false;

        for cred in &creds {
            defmt::info!("WiFi: connecting to SSID=\"{}\"", mask!(cred.ssid));

            let config = esp_radio::wifi::Config::Station(
                esp_radio::wifi::sta::StationConfig::default()
                    .with_ssid(cred.ssid)
                    .with_password(cred.password.into()),
            );
            let _ = wifi_ctrl.set_config(&config);

            Timer::after(Duration::from_millis(10)).await;

            match embassy_time::with_timeout(
                Duration::from_secs(15), wifi_ctrl.connect_async(),
            )
                .await
            {
                Ok(Ok(info)) => {
                    defmt::info!(
                        "WiFi: connected to {:?}",
                        defmt::Debug2Format(&info.ssid),
                    );
                    wifi_ok = true;
                    break;
                }
                Ok(Err(e)) => {
                    defmt::warn!(
                        "WiFi: connection failed: {:?}",
                        defmt::Debug2Format(&e),
                    );
                }
                Err(_) => {
                    defmt::warn!("WiFi: connection timed out");
                }
            }
        }

        if wifi_ok {
            reconnect_fails = 0;
            defmt::info!("WiFi: connected");
            state.set_wifi_connected(true);
            led_command(LedCommand::MqttConnecting);
        } else {
            reconnect_fails += 1;
            defmt::warn!(
                "WiFi: reconnect attempt {}/{} failed, retrying in 30s",
                reconnect_fails,
                MAX_RECONNECT_FAILS,
            );
            if reconnect_fails >= MAX_RECONNECT_FAILS {
                defmt::error!(
                    "WiFi: {} consecutive failures, performing hard reset",
                    reconnect_fails,
                );
                led_command(LedCommand::Error);
                Timer::after(Duration::from_millis(500)).await;
                esp_hal::system::software_reset();
            }
            state.set_wifi_connected(false);
            Timer::after(Duration::from_secs(30)).await;
            continue;
        }

        stack.wait_config_up().await;
        defmt::info!("Network: DHCP config received");

        let broker_host = env_or_panic!("MQTT_BROKER");
        let broker_ip: embassy_net::Ipv4Address = match broker_host.parse() {
            Ok(ip) => ip,
            Err(_) => {
                defmt::info!("MQTT: resolving {} via DNS...", broker_host);
                loop {
                    match stack.dns_query(broker_host, DnsQueryType::A).await {
                        Ok(addrs) => match addrs.first() {
                            Some(IpAddress::Ipv4(addr)) => {
                                defmt::info!("MQTT: resolved {} to {}", broker_host, addr);
                                break *addr;
                            }
                            _ => defmt::warn!("MQTT: DNS returned no IPv4 address"),
                        },
                        Err(e) => defmt::warn!(
                            "MQTT: DNS lookup failed: {:?}",
                            defmt::Debug2Format(&e),
                        ),
                    }
                    led_command(LedCommand::Error);
                    Timer::after(Duration::from_secs(30)).await;
                }
            }
        };
        let broker_port: u16 = env_or_panic!("MQTT_PORT").parse().unwrap_or(1883);
        let mqtt_user = env_or_panic!("MQTT_USER");
        let mqtt_pass = env_or_panic!("MQTT_PASS");
        let mqtt_client_id = env_or_panic!("MQTT_CLIENT_ID");

        defmt::info!(
            "MQTT: connecting to {}:{} as '{}'",
            broker_ip, broker_port, mask!(mqtt_client_id),
        );

        let mut ntp_due = true;
        let mut weather_due = true;
        let mut last_ntp = embassy_time::Instant::now();
        let mut last_ntp_attempt = embassy_time::Instant::now();
        let mut last_weather = embassy_time::Instant::now();
        let mut last_ping = embassy_time::Instant::now();

        let topics = ["Sensors/#", "sensors/#", "vps/monitor", "host/monitor"];

        let mut rx_buf = [0u8; 4096];
        let mut tx_buf = [0u8; 2048];
        let mut mqtt = TcpSocket::new(stack, &mut rx_buf[..], &mut tx_buf[..]);

        match mqtt.connect((broker_ip, broker_port)).await {
            Ok(()) => defmt::info!("MQTT: TCP connected"),
            Err(e) => {
                defmt::warn!(
                    "MQTT: TCP connect failed: {:?}",
                    defmt::Debug2Format(&e),
                );
                led_command(LedCommand::Error);
                Timer::after(Duration::from_secs(10)).await;
                continue;
            }
        }

        let connect_pkt = mqtt::build_connect(mqtt_client_id, mqtt_user, mqtt_pass, 60);
        let _ = mqtt.write(&connect_pkt).await;
        let _ = mqtt.flush().await;
        defmt::info!("MQTT: CONNECT sent");

        let mut mqtt_established = false;
        let mut subscribe_sent = false;
        let mut subscriptions_pending = 4u8;
        let mut packet_id: u16 = 1;

        loop {
            if !stack.is_link_up() {
                defmt::warn!("WiFi: link is down, reconnecting...");
                led_command(LedCommand::MqttConnecting);
                break;
            }

            let mut mqtt_read_buf = [0u8; 1024];

            let read_fut = mqtt.read(&mut mqtt_read_buf[..]);
            match embassy_time::with_timeout(
                Duration::from_millis(500), read_fut,
            )
                .await
            {
                Ok(Ok(0)) => {
                    defmt::warn!("MQTT: connection closed");
                    break;
                }
                Ok(Ok(len)) => {
                    handle_mqtt_data(
                        &mqtt_read_buf[..len],
                        &mut mqtt_established,
                        &mut subscribe_sent,
                        &mut subscriptions_pending,
                        state,
                        packet_id,
                    );
                }
                Ok(Err(_)) => {
                    defmt::warn!("MQTT: read error");
                    break;
                }
                Err(_) => {}
            }

            if mqtt_established && subscribe_sent && subscriptions_pending > 0 {
                for &topic in &topics {
                    let sub = mqtt::build_subscribe(topic, packet_id);
                    packet_id = packet_id.wrapping_add(1);
                    if mqtt.write(&sub).await.is_ok() {
                        defmt::info!("MQTT: SUBSCRIBE sent for {}", topic);
                    }
                }
                subscribe_sent = false;
            }

            let now = embassy_time::Instant::now();

            let ping_due = now - last_ping
                > Duration::from_secs(MQTT_PING_INTERVAL_SECS);
            if mqtt_established && ping_due {
                let _ = mqtt.write(&mqtt::build_pingreq()).await;
                last_ping = now;
            }

            if ntp_due
                || (now - last_ntp
                > Duration::from_secs(NTP_INTERVAL_SECS)
                && now - last_ntp_attempt
                > Duration::from_secs(NTP_RETRY_SECS))
            {
                ntp_due = false;
                last_ntp_attempt = now;
                if let Some(local) = services::ntp_sync(&stack).await {
                    state.set_local_time(local);
                    defmt::info!("NTP: time synchronized");
                    last_ntp = now;
                }
            }

            if weather_due
                || now - last_weather
                > Duration::from_secs(WEATHER_INTERVAL_SECS)
            {
                weather_due = false;
                services::fetch_weather(&stack, state).await;
                last_weather = now;
            }

            if !mqtt.may_send() && mqtt_established {
                defmt::warn!("MQTT: connection lost, reconnecting...");
                led_command(LedCommand::MqttConnecting);
                break;
            }
        }
    }
}

fn led_command(cmd: LedCommand) {
    crate::led::send_led_command(cmd);
}

fn handle_mqtt_data(
    data: &[u8],
    mqtt_established: &mut bool,
    subscribe_sent: &mut bool,
    subscriptions_pending: &mut u8,
    state: &AppState,
    _packet_id: u16,
) {
    if data.len() < 2 { return; }
    match mqtt::parse_packet_type(data[0]) {
        Some(mqtt::MqttPacketType::Connack) => {
            if data.len() >= 4 && data[3] == 0 {
                defmt::info!("MQTT: CONNACK success");
                *mqtt_established = true;
                *subscribe_sent = true;
                *subscriptions_pending = 4;
                led_command(LedCommand::Connected);
            }
        }
        Some(mqtt::MqttPacketType::Suback) => {
            if *subscriptions_pending > 0 { *subscriptions_pending -= 1; }
            defmt::info!(
                "MQTT: SUBACK received ({} remaining)",
                *subscriptions_pending,
            );
        }
        Some(mqtt::MqttPacketType::Publish) => {
            if let Some((topic, payload)) = mqtt::parse_publish(data) {
                dispatch_mqtt_message(topic, payload, state);
            }
        }
        Some(mqtt::MqttPacketType::PingResp) => {}
        _ => defmt::info!("MQTT: unknown packet type 0x{:02X}", data[0]),
    }
}

/// Strip non-UTF-8 bytes from both ends of a byte slice so
/// the remaining portion decodes as valid UTF-8.
fn trim_to_utf8(data: &[u8]) -> &[u8] {
    let len = data.len();
    let mut start = 0;
    while start < len {
        match core::str::from_utf8(&data[start..]) {
            Ok(_) => break,
            Err(e) => {
                let valid_up_to = e.valid_up_to();
                if let Some(bad_len) = e.error_len() {
                    start += valid_up_to + bad_len;
                } else {
                    return &data[start..start + valid_up_to];
                }
            }
        }
    }
    if start >= len {
        return &[];
    }
    let mut end = len;
    while end > start {
        if core::str::from_utf8(&data[start..end]).is_ok() {
            break;
        }
        end -= 1;
    }
    &data[start..end]
}

fn dispatch_mqtt_message(topic: &str, payload: &[u8], state: &AppState) {
    let trimmed = trim_to_utf8(payload);
    let msg = match core::str::from_utf8(trimmed) {
        Ok(s) => s,
        Err(_) => {
            defmt::warn!("MQTT: non-UTF-8 payload on {}", topic);
            return;
        }
    };
    if topic == "vps/monitor" {
        mqtt::handle_vps_data(msg, state);
    } else if topic == "host/monitor" {
        mqtt::handle_host_data(msg, state);
    } else if topic.starts_with("Sensors") || topic.starts_with("sensors") {
        mqtt::handle_sensor_data(msg, state);
    } else {
        defmt::info!("MQTT: unhandled topic {}", topic);
    }
}
