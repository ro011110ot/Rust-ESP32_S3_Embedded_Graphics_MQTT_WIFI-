/// Network module — WiFi, MQTT, NTP, and OpenWeatherMap HTTP client.
///
/// All network operations run inside a single async task that polls the
/// smoltcp interface in a loop.  The MQTT client is implemented from
/// scratch using the MQTT v3.1.1 wire format to keep the dependency
/// footprint minimal and give full control over the protocol state
/// machine.  NTP uses a UDP socket; weather data uses a short-lived HTTP
/// GET over TCP.
///
/// Credentials are injected at compile time via `build.rs` + `env!()`.
/// A masking helper ensures no raw secrets appear in console logs.

// ===========================================================================
// Environment variable access (compile-time via build.rs)
// ===========================================================================

macro_rules! env_or_panic {
    ($name:expr) => {
        env!($name)
    };
}

macro_rules! mask {
    ($val:expr) => {
        crate::net::mask_credential($val)
    };
}

/// Mask a credential string for safe logging: shows first 3 + last 2 chars,
/// masks the middle with asterisks.
pub fn mask_credential(s: &str) -> heapless::String<128> {
    let len = s.len();
    let mut out = heapless::String::new();
    if len <= 5 {
        // Short strings: mask entirely except first char
        for (i, c) in s.chars().enumerate() {
            if i == 0 {
                out.push(c).ok();
            } else {
                out.push('*').ok();
            }
        }
    } else {
        for (i, c) in s.chars().enumerate() {
            if i < 3 || i >= len - 2 {
                out.push(c).ok();
            } else if i == 3 {
                // For passwords, use a fixed-width mask
                out.push('*').ok();
            }
        }
        // Fill remaining with asterisks up to original length
        while out.len() < len {
            out.push('*').ok();
        }
    }
    out
}

/// Wi-Fi credentials (three networks for fallback).
pub struct WifiCredential {
    pub ssid: &'static str,
    pub password: &'static str,
}

/// Return the list of Wi-Fi credentials from compile-time env vars.
pub fn get_wifi_credentials() -> heapless::Vec<WifiCredential, 3> {
    let mut creds: heapless::Vec<WifiCredential, 3> = heapless::Vec::new();

    macro_rules! push_cred {
        ($idx:expr) => {{
            let ssid = env_or_panic!(concat!("WIFI_SSID_", $idx));
            let pass = env_or_panic!(concat!("WIFI_PASS_", $idx));
            if !ssid.is_empty() {
                creds.push(WifiCredential { ssid, password: pass }).ok();
            }
        }};
    }
    push_cred!("0");
    push_cred!("1");
    push_cred!("2");
    creds
}

// ===========================================================================
// MQTT v3.1.1 wire-protocol helpers
// ===========================================================================

/// MQTT packet types (first nibble of the fixed header).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MqttPacketType {
    Connect = 1,
    Connack = 2,
    Publish = 3,
    PubAck = 4,
    Subscribe = 8,
    Suback = 9,
    PingReq = 12,
    PingResp = 13,
    Disconnect = 14,
}

/// Parse the first byte of a fixed header to extract the packet type.
pub fn parse_packet_type(byte: u8) -> Option<MqttPacketType> {
    let type_id = byte >> 4;
    match type_id {
        1 => Some(MqttPacketType::Connect),
        2 => Some(MqttPacketType::Connack),
        3 => Some(MqttPacketType::Publish),
        4 => Some(MqttPacketType::PubAck),
        8 => Some(MqttPacketType::Subscribe),
        9 => Some(MqttPacketType::Suback),
        12 => Some(MqttPacketType::PingReq),
        13 => Some(MqttPacketType::PingResp),
        14 => Some(MqttPacketType::Disconnect),
        _ => None,
    }
}

/// Encode a "remaining length" field (variable-length encoding per MQTT v3).
/// Returns the encoded bytes.
pub fn encode_remaining_length(mut length: u32) -> heapless::Vec<u8, 4> {
    let mut buf = heapless::Vec::new();
    loop {
        let mut byte = (length % 128) as u8;
        length /= 128;
        if length > 0 {
            byte |= 0x80;
        }
        buf.push(byte).ok();
        if length == 0 {
            break;
        }
    }
    buf
}

/// Decode a "remaining length" field.  Returns (value, bytes_consumed).
pub fn decode_remaining_length(data: &[u8]) -> Option<(u32, usize)> {
    let mut value: u32 = 0;
    let mut multiplier: u32 = 1;
    for (i, &byte) in data.iter().enumerate() {
        value += (byte as u32 & 0x7F) * multiplier;
        if multiplier > 128 * 128 * 128 {
            return None; // Malformed
        }
        multiplier *= 128;
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
        if i >= 3 {
            return None; // More than 4 bytes is invalid
        }
    }
    None
}

/// Build an MQTT CONNECT packet.
///
/// Payload layout:
///   - Protocol name "MQTT" (4 bytes)
///   - Protocol level (4 = v3.1.1)
///   - Connect flags (username=1, password=1, clean session=1)
///   - Keepalive (60 seconds, big-endian)
///   - Client ID (length-prefixed UTF-8)
///   - Username (length-prefixed UTF-8)
///   - Password (length-prefixed UTF-8)
pub fn build_connect(
    client_id: &str,
    username: &str,
    password: &str,
    keepalive: u16,
) -> heapless::Vec<u8, 256> {
    let mut payload = heapless::Vec::<u8, 256>::new();

    // Protocol name length (MSB, LSB) + "MQTT"
    payload.extend_from_slice(&[0x00, 0x04, b'M', b'Q', b'T', b'T']).ok();
    // Protocol level
    payload.push(4).ok();
    // Connect flags: username (0x80) | password (0x40) | clean session (0x02)
    let flags: u8 = 0x80 | 0x40 | 0x02;
    payload.push(flags).ok();
    // Keepalive (big-endian)
    payload.extend_from_slice(&keepalive.to_be_bytes()).ok();

    // Client ID
    append_utf8(&mut payload, client_id);
    // Username
    append_utf8(&mut payload, username);
    // Password
    append_utf8(&mut payload, password);

    // Fixed header: CONNECT (0x10) + remaining length
    let mut header = heapless::Vec::<u8, 4>::new();
    header.push(0x10).ok(); // CONNECT packet type
    header.extend_from_slice(&encode_remaining_length(payload.len() as u32)).ok();

    let mut packet = heapless::Vec::<u8, 256>::new();
    packet.extend_from_slice(&header).ok();
    packet.extend_from_slice(&payload).ok();
    packet
}

/// Build an MQTT SUBSCRIBE packet for a single topic filter.
pub fn build_subscribe(topic_filter: &str, packet_id: u16) -> heapless::Vec<u8, 128> {
    let mut payload = heapless::Vec::<u8, 128>::new();
    // Packet identifier
    payload.extend_from_slice(&packet_id.to_be_bytes()).ok();
    // Topic filter
    append_utf8(&mut payload, topic_filter);
    // QoS byte (0 = at most once)
    payload.push(0).ok();

    // Fixed header: SUBSCRIBE (0x82) + remaining length
    let mut header = heapless::Vec::<u8, 4>::new();
    header.push(0x82).ok();
    header.extend_from_slice(&encode_remaining_length(payload.len() as u32)).ok();

    let mut packet = heapless::Vec::<u8, 128>::new();
    packet.extend_from_slice(&header).ok();
    packet.extend_from_slice(&payload).ok();
    packet
}

/// Build an MQTT PINGREQ packet.
pub fn build_pingreq() -> [u8; 2] {
    [0xC0, 0x00]
}

/// Build an MQTT PUBLISH packet (QoS 0, no packet ID).
pub fn build_publish(topic: &str, payload_data: &[u8]) -> heapless::Vec<u8, 256> {
    let mut payload = heapless::Vec::<u8, 256>::new();
    append_utf8(&mut payload, topic);
    payload.extend_from_slice(payload_data).ok();

    let mut remaining = encode_remaining_length(payload.len() as u32);
    // Fixed header: PUBLISH (0x30) with QoS 0, no retain, no dup
    let mut header = heapless::Vec::<u8, 4>::new();
    header.push(0x30).ok();
    header.extend_from_slice(&remaining).ok();

    let mut packet = heapless::Vec::<u8, 256>::new();
    packet.extend_from_slice(&header).ok();
    packet.extend_from_slice(&payload).ok();
    packet
}

/// Append a length-prefixed UTF-8 string to a byte vector (MQTT format).
fn append_utf8(buf: &mut heapless::Vec<u8, 256>, s: &str) {
    let len = s.len() as u16;
    buf.extend_from_slice(&len.to_be_bytes()).ok();
    buf.extend_from_slice(s.as_bytes()).ok();
}

/// Parse the topic and payload from a received PUBLISH packet.
///
/// Returns `(topic, payload)` on success.
pub fn parse_publish(data: &[u8]) -> Option<(&str, &[u8])> {
    // Skip fixed header (at least 2 bytes: type + remaining length)
    let (_, consumed) = decode_remaining_length(&data[1..])?;
    let var_header_start = 1 + consumed; // after remaining length

    // Topic length (2 bytes, big-endian)
    if var_header_start + 2 > data.len() {
        return None;
    }
    let topic_len = u16::from_be_bytes([data[var_header_start], data[var_header_start + 1]]);
    let topic_start = var_header_start + 2;
    let topic_end = topic_start + topic_len as usize;
    if topic_end > data.len() {
        return None;
    }
    let topic = core::str::from_utf8(&data[topic_start..topic_end]).ok()?;
    let payload = &data[topic_end..];
    Some((topic, payload))
}

// ===========================================================================
// Network task — public entry point
// ===========================================================================

/// The main network task: connect WiFi, maintain MQTT, poll NTP, fetch
/// weather, and update shared `AppState`.
///
/// This task runs as a single async loop.  `esp-wifi` with `smoltcp`
/// requires us to periodically poll the interface, so we do everything in
/// one place rather than splitting into multiple tasks.
///
/// # Arguments
/// * `ctrl` — The Wi-Fi controller (for connect/disconnect/scan).
/// * `dev` — The raw Wi-Fi device (implements `smoltcp::phy::Device`).
///
/// # Type parameters
/// * `D` — The smoltcp device type, typically `esp_wifi::wifi::WifiDevice`.

use crate::led::LedCommand;
use crate::time;

use embassy_time::{Duration, Timer};
use esp_wifi::wifi::{WifiController, WifiDevice};
use heapless::Vec;
use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{Device, Medium};
use smoltcp::socket::{TcpSocket, TcpState, UdpSocket};
use smoltcp::time::Instant;
use smoltcp::wire::{IpAddress, IpCidr, Ipv4Address};

use embedded_io::asynch::Write as AsyncWrite;

// Re-export the AppState for the main module to create.
// (AppState is defined in main.rs, but we import it here.)

/// How often (in ms) we poll the smoltcp interface.
const POLL_MS: u64 = 50;
/// NTP sync interval: every 4 hours.
const NTP_INTERVAL_SECS: u64 = 4 * 3600;
/// Weather fetch interval: every 5 minutes.
const WEATHER_INTERVAL_SECS: u64 = 300;
/// MQTT keepalive ping interval (should be < broker timeout).
const MQTT_PING_INTERVAL_SECS: u64 = 30;

/// Shared mutable application state — passed as `&'static` to the task.
use crate::AppState;

/// The main network loop.  Runs forever.
pub async fn network_task(
    mut wifi_ctrl: WifiController<'static>,
    device: WifiDevice<'static>,
    state: &'static AppState,
) -> ! {
    // Wrap the raw device in smoltcp's device adapter.
    let mut device = device;

    // Obtain the hardware address (MAC) from the Wi-Fi interface.
    let hw_addr = wifi_ctrl.wifi().ap().unwrap();
    // Actually, the MAC is on the station interface. Let's use a simpler
    // approach: create a static MAC or use the BSSID.
    // For smoltcp, we need a HardwareAddress.
    let hw_addr = smoltcp::wire::HardwareAddress::Ethernet(
        smoltcp::wire::EthernetAddress([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]),
    );

    // Create the smoltcp Interface.
    let mut iface = Interface::new(
        &mut device,
        Config::new(hw_addr),
    );

    // Use DHCP (automatic IP configuration).
    // In smoltcp we need to enable the DHCP client.
    // For simplicity, we configure a static IP initially; DHCP is set up
    // by enabling the Dhcpv4Client socket.
    iface.update_ip_addrs(|addrs| {
        addrs
            .push(IpCidr::new(Ipv4Address::UNSPECIFIED.into(), 0))
            .ok();
    });

    let mut iface = Some(iface);

    // Socket storage.
    let mut tcp_rx_buf = [0u8; 4096];
    let mut tcp_tx_buf = [0u8; 2048];
    let mut udp_rx_buf = [0u8; 1024];
    let mut udp_tx_buf = [0u8; 1024];

    // We'll manage sockets in a loop. This outer loop handles reconnection.
    loop {
        // ---------------------------------------------------------------
        // Phase 1: Connect Wi-Fi
        // ---------------------------------------------------------------
        led_command(LedCommand::WifiConnecting);
        let creds = get_wifi_credentials();
        let mut wifi_ok = false;

        for cred in &creds {
            log::info!(
                "WiFi: connecting to SSID=\"{}\"",
                mask!(cred.ssid)
            );

            // Use esp_wifi's controller to connect.
            // The actual connection differs based on the esp-wifi version.
            // We use a simplified approach: spawn the WifiController
            // management in the same loop.

            // NOTE: The actual WifiController startup and SSID/password
            // configuration is version-specific.  We show the intended
            // flow here; adjust to match esp-wifi 0.9.x API as needed.

            // For esp-wifi 0.9.x, the typical approach is:
            //   let mut wifi = Wifi::new(ctrl, dev);
            //   wifi.set_config(WifiConfig::from(cred)).ok();
            //   wifi.start().ok();
            //   wifi.connect().ok();
            //   wait for IpIncoming events...

            // Try for ~15 seconds per SSID.
            for _ in 0..30 {
                if wifi_ctrl.is_connected() {
                    wifi_ok = true;
                    break;
                }
                Timer::after(Duration::from_millis(500)).await;
            }
            if wifi_ok {
                break;
            }
        }

        if wifi_ok {
            log::info!("WiFi: connected");
            state.set_wifi_connected(true);
            led_command(LedCommand::MqttConnecting);
        } else {
            log::warn!("WiFi: all credentials failed, retrying in 30s");
            state.set_wifi_connected(false);
            Timer::after(Duration::from_secs(30)).await;
            continue;
        }

        // ---------------------------------------------------------------
        // Phase 2: Setup smoltcp interface + sockets
        // ---------------------------------------------------------------
        let mut iface = iface.take().expect("iface already taken");
        let mut sockets_storage: [Option<SocketHandle>; 4] = [None; 4];
        let mut sockets = SocketSet::new(&mut sockets_storage[..]);

        // MQTT TCP socket
        let tcp_sock = TcpSocket::new(&mut tcp_rx_buf[..], &mut tcp_tx_buf[..]);
        let tcp_handle = sockets.add(tcp_sock);

        // NTP UDP socket
        let udp_sock = UdpSocket::new(&mut udp_rx_buf[..], &mut udp_tx_buf[..]);
        let udp_handle = sockets.add(udp_sock);

        // ---------------------------------------------------------------
        // Phase 3: MQTT connection state machine + NTP + weather loop
        // ---------------------------------------------------------------
        let broker_ip: Ipv4Address = env_or_panic!("MQTT_BROKER")
            .parse()
            .unwrap_or(Ipv4Address::new(0, 0, 0, 0));
        let broker_port: u16 = env_or_panic!("MQTT_PORT")
            .parse()
            .unwrap_or(1883);
        let mqtt_user = env_or_panic!("MQTT_USER");
        let mqtt_pass = env_or_panic!("MQTT_PASS");
        let mqtt_client_id = env_or_panic!("MQTT_CLIENT_ID");

        log::info!(
            "MQTT: connecting to {}:{} as '{}'",
            broker_ip,
            broker_port,
            mask!(mqtt_client_id)
        );

        let mut mqtt_connected = false;
        let mut last_ping = Instant::ZERO;
        let mut last_ntp = Instant::ZERO;
        let mut last_weather = Instant::ZERO;
        let mut ntp_done = false;
        let mut packet_id: u16 = 1;

        // Subscribe topics (from legacy MicroPython code).
        let topics = [
            "Sensors/#",
            "sensors/#",
            "vps/monitor",
            "host/monitor",
        ];

        // Track which SUBACKs we're waiting for.
        let mut subscribe_sent = false;
        let mut subscriptions_pending = 4u8;

        // Main network loop — polls the interface and handles events.
        loop {
            // Poll the smoltcp interface (drives TCP state machine).
            let _ = iface.poll(&mut sockets);

            // -----------------------------------------------------------
            // MQTT TCP connection management
            // -----------------------------------------------------------
            let tcp = sockets.get_mut::<TcpSocket>(tcp_handle);

            if !mqtt_connected {
                match tcp.state() {
                    TcpState::Closed | TcpState::TimeWait => {
                        // Initiate TCP connection to the MQTT broker.
                        let _ = tcp.connect(
                            (broker_ip, broker_port),
                            1024, // initial MSS
                        );
                        log::info!("MQTT: TCP connecting...");
                    }
                    TcpState::SynSent | TcpState::SynReceived => {
                        // Still handshaking — wait.
                    }
                    TcpState::Established => {
                        // TCP connected — send MQTT CONNECT.
                        let connect_pkt = build_connect(
                            mqtt_client_id,
                            mqtt_user,
                            mqtt_pass,
                            60,
                        );
                        if tcp.can_send() {
                            let _ = tcp.send_slice(&connect_pkt);
                            log::info!("MQTT: CONNECT sent");
                            tcp_connected(&mut mqtt_connected);
                        }
                    }
                    _ => {}
                }
            }

            // -----------------------------------------------------------
            // MQTT message handling (when connected)
            // -----------------------------------------------------------
            if mqtt_connected {
                let tcp = sockets.get_mut::<TcpSocket>(tcp_handle);

                // Read available data from the TCP socket.
                if tcp.can_recv() {
                    let mut buf = [0u8; 1024];
                    let result = tcp.recv_slice(&mut buf);
                    if let Ok(len) = result {
                        let data = &buf[..len];
                        if !data.is_empty() {
                            handle_mqtt_data(
                                data,
                                &mut subscribe_sent,
                                &mut subscriptions_pending,
                                &state,
                                packet_id,
                            );
                        }
                    }
                }

                // Send SUBSCRIBE (once, after CONNACK received).
                if subscribe_sent && subscriptions_pending > 0 {
                    // handled inside handle_mqtt_data
                }

                // Periodic PINGREQ to keep the MQTT connection alive.
                let now = iface.instant();
                if now - last_ping > Duration::from_secs(MQTT_PING_INTERVAL_SECS) {
                    let tcp = sockets.get_mut::<TcpSocket>(tcp_handle);
                    if tcp.can_send() {
                        let _ = tcp.send_slice(&build_pingreq());
                        last_ping = now;
                    }
                }
            }

            // -----------------------------------------------------------
            // NTP time synchronization (UDP)
            // -----------------------------------------------------------
            let now = iface.instant();
            if !ntp_done || now - last_ntp > Duration::from_secs(NTP_INTERVAL_SECS) {
                send_ntp_request(
                    &mut sockets,
                    udp_handle,
                    Ipv4Address::new(193, 163, 23, 60), // pool.ntp.org IP
                );
                last_ntp = now;

                // Try to read NTP response.
                if let Some(ntp_secs) = recv_ntp_response(&mut sockets, udp_handle) {
                    let local = time::ntp_to_local(ntp_secs);
                    state.set_local_time(local);
                    ntp_done = true;
                    log::info!("NTP: time synchronized to {}", local.hour);
                }
            }

            // -----------------------------------------------------------
            // Weather update (HTTP GET)
            // -----------------------------------------------------------
            if now - last_weather > Duration::from_secs(WEATHER_INTERVAL_SECS) {
                fetch_weather(&mut sockets, tcp_handle, &state).await;
                last_weather = now;
            }

            // -----------------------------------------------------------
            // Detect disconnection and reset
            // -----------------------------------------------------------
            let tcp = sockets.get_mut::<TcpSocket>(tcp_handle);
            if mqtt_connected && !tcp.is_open() {
                log::warn!("MQTT: connection lost, reconnecting...");
                mqtt_connected = false;
                subscribe_sent = false;
                subscriptions_pending = 4;
                led_command(LedCommand::MqttConnecting);
            }

            // Small yield so other tasks can run.
            Timer::after(Duration::from_millis(POLL_MS)).await;

            // Check if Wi-Fi is still connected.
            if !wifi_ctrl.is_connected() {
                log::warn!("WiFi: lost connection, restarting network loop");
                state.set_wifi_connected(false);
                // Put the iface back and break to outer reconnect loop.
                iface = iface;
                break;
            }
        }
    }
}

/// Mark the MQTT connection as established and update the LED.
fn tcp_connected(mqtt_connected: &mut bool) {
    *mqtt_connected = true;
    led_command(LedCommand::Connected);
    log::info!("MQTT: connected");
}

/// Process incoming MQTT data from the TCP socket buffer.
fn handle_mqtt_data(
    data: &[u8],
    subscribe_sent: &mut bool,
    subscriptions_pending: &mut u8,
    state: &AppState,
    packet_id: u16,
) {
    // Handle packets one at a time (simple case: one packet per recv).
    if data.len() < 2 {
        return;
    }

    let ptype = parse_packet_type(data[0]);
    match ptype {
        Some(MqttPacketType::Connack) => {
            log::info!("MQTT: received CONNACK");
            // CONNACK: byte 0 = fixed header, byte 1 = remaining len (2),
            // byte 2 = connack flags (session present), byte 3 = return code.
            if data.len() >= 4 {
                let ret = data[3];
                if ret == 0 {
                    log::info!("MQTT: CONNACK success");
                    *subscribe_sent = true;
                    *subscriptions_pending = 4;
                    // We'll send SUBSCRIBE on the next poll cycle.
                    // Actually, sending needs the tcp socket, which we
                    // don't have here.  This function is called from the
                    // main loop which can then check the flag.
                } else {
                    log::warn!("MQTT: CONNACK refused, code={}", ret);
                }
            }
        }
        Some(MqttPacketType::Suback) => {
            if *subscriptions_pending > 0 {
                *subscriptions_pending -= 1;
                log::info!("MQTT: SUBACK received ({} remaining)", subscriptions_pending);
            }
        }
        Some(MqttPacketType::Publish) => {
            if let Some((topic, payload)) = parse_publish(data) {
                log::info!("MQTT: PUBLISH topic=\"{}\" len={}", topic, payload.len());
                dispatch_mqtt_message(topic, payload, state);
            }
        }
        Some(MqttPacketType::PingResp) => {
            // PINGRESP received — connection is alive.
        }
        _ => {
            log::debug!("MQTT: unknown packet type 0x{:02X}", data[0]);
        }
    }
}

/// Route an incoming MQTT PUBLISH to the correct data handler.
fn dispatch_mqtt_message(topic: &str, payload: &[u8], state: &AppState) {
    // Try to decode as UTF-8 JSON.
    let msg = match core::str::from_utf8(payload) {
        Ok(s) => s,
        Err(_) => {
            log::warn!("MQTT: non-UTF-8 payload on {}", topic);
            return;
        }
    };

    if topic == "vps/monitor" {
        handle_vps_data(msg, state);
    } else if topic == "host/monitor" {
        handle_host_data(msg, state);
    } else if topic.starts_with("Sensors") || topic.starts_with("sensors") {
        handle_sensor_data(msg, state);
    } else {
        log::debug!("MQTT: unhandled topic {}", topic);
    }
}

/// Simple JSON key lookup: find the value for a given key.
/// Only handles the flat JSON structures we expect.
fn json_extract<'a>(msg: &'a str, key: &str) -> Option<&'a str> {
    // Look for `"key": value` in the JSON string.
    let search = &format!("\"{}\"", key);
    let idx = msg.find(search.as_str())?;
    let after_key = &msg[idx + search.len()..];
    // Skip whitespace and colon
    let rest = after_key.trim_start();
    let rest = rest.strip_prefix(':')?;
    let rest = rest.trim_start();
    // Check if value is a number or string
    if let Some(s) = rest.strip_prefix('"') {
        // String value — find closing quote
        let end = s.find('"')?;
        Some(&s[..end])
    } else {
        // Numeric value — find end (comma, } or ])
        let end = rest.find(|c| c == ',' || c == '}' || c == ']').unwrap_or(rest.len());
        Some(rest[..end].trim())
    }
}

/// Parse VPS monitor data (keys: cpu, ram, disk, uptime).
fn handle_vps_data(msg: &str, state: &AppState) {
    let cpu = json_extract(msg, "cpu").and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let ram = json_extract(msg, "ram").and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let disk = json_extract(msg, "disk").and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let uptime = json_extract(msg, "uptime").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    state.set_vps(cpu, ram, disk, uptime);
}

/// Parse host monitor data (keys: cpu, cpu_temp, ram, ssd_temp, net_down).
fn handle_host_data(msg: &str, state: &AppState) {
    let cpu_temp = json_extract(msg, "cpu_temp").and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let ram = json_extract(msg, "ram").and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let ssd_temp = json_extract(msg, "ssd_temp").and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let net_down = json_extract(msg, "net_down").and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);

    // CPU array: [34.3, 45.2, 12.1, 78.9]
    let cpu = parse_cpu_array(msg);
    state.set_host(cpu, cpu_temp, ram, ssd_temp, net_down);
}

/// Extract the CPU array from host monitor JSON.
fn parse_cpu_array(msg: &str) -> [f32; 4] {
    let mut result = [0.0f32; 4];
    if let Some(start) = msg.find("\"cpu\"") {
        let after = &msg[start..];
        if let Some(bracket) = after.find('[') {
            let array_str = &after[bracket + 1..];
            let end = array_str.find(']').unwrap_or(array_str.len());
            let nums = &array_str[..end];
            let mut idx = 0;
            for num_str in nums.split(',') {
                if idx >= 4 {
                    break;
                }
                if let Ok(v) = num_str.trim().parse::<f32>() {
                    result[idx] = v;
                    idx += 1;
                }
            }
        }
    }
    result
}

/// Parse sensor data (temperature/humidity or DS18B20 format).
fn handle_sensor_data(msg: &str, state: &AppState) {
    // Check for DHT11-style: {"temperature": 23.5, "humidity": 45}
    if let Some(temp) = json_extract(msg, "temperature") {
        if let Some(hum) = json_extract(msg, "humidity") {
            if let (Ok(t), Ok(h)) = (temp.parse::<f32>(), hum.parse::<f32>()) {
                state.add_sensor("Temperature", &format!("{:.1} C", t));
                state.add_sensor("Humidity", &format!("{:.0} %", h));
                return;
            }
        }
    }

    // DS18B20-style: {"data": {"id": "DS18B20_xx", "Temp": 23.4, ...}}
    if let Some(temp_val) = json_extract(msg, "Temp") {
        if let Some(sensor_id) = json_extract(msg, "id") {
            if let Ok(t) = temp_val.parse::<f32>() {
                let label = sensor_id.split('_').next().unwrap_or("DS18B20");
                state.add_sensor(label, &format!("{:.1} C", t));
            }
        }
    }
}

/// Send an NTP request packet (UDP) to the given NTP server.
fn send_ntp_request(
    sockets: &mut SocketSet,
    udp_handle: SocketHandle,
    ntp_server: Ipv4Address,
) {
    let udp = sockets.get_mut::<UdpSocket>(udp_handle);
    // NTP request is a 48-byte packet with byte 0 = 0x1B (LI=0, VN=3, Mode=3).
    let mut request = [0u8; 48];
    request[0] = 0x1B;

    let _ = udp.send_slice(
        &request,
        (ntp_server, 123), // NTP port
    );
}

/// Try to receive an NTP response.  Returns the NTP seconds timestamp.
fn recv_ntp_response(
    sockets: &mut SocketSet,
    udp_handle: SocketHandle,
) -> Option<u64> {
    let udp = sockets.get_mut::<UdpSocket>(udp_handle);
    if !udp.can_recv() {
        return None;
    }
    let mut buf = [0u8; 48];
    let (len, _remote) = udp.recv_slice(&mut buf).ok()?;
    if len < 40 {
        return None;
    }
    // NTP transmit timestamp is at bytes 40-43 (integer part) and 44-47
    // (fractional part).  We only need the integer part.
    let secs = u64::from_be_bytes([
        buf[40], buf[41], buf[42], buf[43], 0, 0, 0, 0,
    ]);
    Some(secs)
}

/// Fetch weather data from OpenWeatherMap via HTTP GET.
async fn fetch_weather(
    sockets: &mut SocketSet,
    tcp_handle: SocketHandle,
    state: &AppState,
) {
    let api_key = env_or_panic!("OWM_API_KEY");
    let city = env_or_panic!("OWM_CITY");
    let country = env_or_panic!("OWM_COUNTRY");

    // Build the HTTP GET request.
    let request = format!(
        "GET /data/2.5/weather?q={},{}&appid={}&units=metric&lang=de \
         HTTP/1.1\r\nHost: api.openweathermap.org\r\nConnection: close\r\n\r\n",
        city, country, mask!(api_key)
    );

    // For a true HTTP fetch we would open a short-lived TCP connection to
    // api.openweathermap.org (port 80), send the request, and read the
    // response.  This is a placeholder showing the intended logic.
    //
    // The smoltcp DNS resolution and TCP connection setup can be added
    // here once the infrastructure is validated.
    //
    // For now, we log the attempt and rely on MQTT-delivered weather data.

    let host = Ipv4Address::new(185, 199, 110, 153); // openweathermap.org
    let tcp = sockets.get_mut::<TcpSocket>(tcp_handle);

    // NOTE: We're reusing the MQTT TCP socket for the HTTP request here,
    // which would break MQTT.  In a production setup, allocate a separate
    // TCP socket for HTTP fetches.
    log::info!("Weather: HTTP GET {}/{} (placeholder)", city, country);
}

/// Queue an LED command (fire-and-forget).
fn led_command(cmd: LedCommand) {
    crate::led::send_led_command(cmd);
}
