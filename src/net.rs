fn fmt_1dp_str(val: f32) -> heapless::String<16> {
    let i = val as i32;
    let frac = (val.abs() - (val as i32).abs() as f32) * 10.0 + 0.5;
    let f = frac as u32 % 10;
    let mut s = heapless::String::new();
    let _ = core::fmt::write(&mut s, format_args!("{}.{}C", i, f));
    s
}
fn fmt_0dp_str(val: f32) -> heapless::String<16> {
    let i = (if val >= 0.0 { val + 0.5 } else { val - 0.5 }) as i32;
    let mut s = heapless::String::new();
    let _ = core::fmt::write(&mut s, format_args!("{}%", i));
    s
}

macro_rules! env_or_panic {
    ($name:expr) => { env!($name) };
}

macro_rules! mask {
    ($val:expr) => { crate::net::mask_credential($val) };
}

pub fn mask_credential(s: &str) -> heapless::String<128> {
    let len = s.len();
    let mut out = heapless::String::new();
    if len <= 5 {
        for (i, c) in s.chars().enumerate() {
            if i == 0 { out.push(c).ok(); } else { out.push('*').ok(); }
        }
    } else {
        for (i, c) in s.chars().enumerate() {
            if i < 3 || i >= len - 2 { out.push(c).ok(); }
            else if i == 3 { out.push('*').ok(); }
        }
        while out.len() < len { out.push('*').ok(); }
    }
    out
}

pub struct WifiCredential {
    pub ssid: &'static str,
    pub password: &'static str,
}

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

pub fn parse_packet_type(byte: u8) -> Option<MqttPacketType> {
    match byte >> 4 {
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

pub fn encode_remaining_length(mut length: u32) -> heapless::Vec<u8, 4> {
    let mut buf = heapless::Vec::new();
    loop {
        let mut byte = (length % 128) as u8;
        length /= 128;
        if length > 0 { byte |= 0x80; }
        buf.push(byte).ok();
        if length == 0 { break; }
    }
    buf
}

pub fn decode_remaining_length(data: &[u8]) -> Option<(u32, usize)> {
    let mut value = 0u32;
    let mut multiplier = 1u32;
    for (i, &byte) in data.iter().enumerate() {
        value += (byte as u32 & 0x7F) * multiplier;
        if multiplier > 128 * 128 * 128 { return None; }
        multiplier *= 128;
        if byte & 0x80 == 0 { return Some((value, i + 1)); }
        if i >= 3 { return None; }
    }
    None
}

pub fn build_connect(
    client_id: &str, username: &str, password: &str, keepalive: u16,
) -> heapless::Vec<u8, 256> {
    let mut payload = heapless::Vec::<u8, 256>::new();
    payload.extend_from_slice(&[0x00, 0x04, b'M', b'Q', b'T', b'T']).ok();
    payload.push(4).ok();
    payload.push(0x80 | 0x40 | 0x02).ok();
    payload.extend_from_slice(&keepalive.to_be_bytes()).ok();
    append_utf8(&mut payload, client_id);
    append_utf8(&mut payload, username);
    append_utf8(&mut payload, password);
    let mut header = heapless::Vec::<u8, 4>::new();
    header.push(0x10).ok();
    header.extend_from_slice(&encode_remaining_length(payload.len() as u32)).ok();
    let mut packet = heapless::Vec::<u8, 256>::new();
    packet.extend_from_slice(&header).ok();
    packet.extend_from_slice(&payload).ok();
    packet
}

pub fn build_subscribe(topic_filter: &str, packet_id: u16) -> heapless::Vec<u8, 128> {
    let mut payload = heapless::Vec::<u8, 256>::new();
    payload.extend_from_slice(&packet_id.to_be_bytes()).ok();
    append_utf8(&mut payload, topic_filter);
    payload.push(0).ok();
    let mut header = heapless::Vec::<u8, 4>::new();
    header.push(0x82).ok();
    header.extend_from_slice(&encode_remaining_length(payload.len() as u32)).ok();
    let mut packet = heapless::Vec::<u8, 128>::new();
    packet.extend_from_slice(&header).ok();
    packet.extend_from_slice(&payload).ok();
    packet
}

pub fn build_pingreq() -> [u8; 2] { [0xC0, 0x00] }

#[allow(dead_code)]
pub fn build_publish(topic: &str, payload_data: &[u8]) -> heapless::Vec<u8, 256> {
    let mut payload = heapless::Vec::<u8, 256>::new();
    append_utf8(&mut payload, topic);
    payload.extend_from_slice(payload_data).ok();
    let remaining = encode_remaining_length(payload.len() as u32);
    let mut header = heapless::Vec::<u8, 4>::new();
    header.push(0x30).ok();
    header.extend_from_slice(&remaining).ok();
    let mut packet = heapless::Vec::<u8, 256>::new();
    packet.extend_from_slice(&header).ok();
    packet.extend_from_slice(&payload).ok();
    packet
}

fn append_utf8(buf: &mut heapless::Vec<u8, 256>, s: &str) {
    let len = s.len() as u16;
    buf.extend_from_slice(&len.to_be_bytes()).ok();
    buf.extend_from_slice(s.as_bytes()).ok();
}

pub fn parse_publish(data: &[u8]) -> Option<(&str, &[u8])> {
    let (_, consumed) = decode_remaining_length(&data[1..])?;
    let var_header_start = 1 + consumed;
    if var_header_start + 2 > data.len() { return None; }
    let topic_len =
        u16::from_be_bytes([data[var_header_start], data[var_header_start + 1]]);
    let topic_start = var_header_start + 2;
    let topic_end = topic_start + topic_len as usize;
    if topic_end > data.len() { return None; }
    let topic = core::str::from_utf8(&data[topic_start..topic_end]).ok()?;
    let payload = &data[topic_end..];
    Some((topic, payload))
}

use alloc::format;
use crate::led::LedCommand;
use crate::time;
use crate::AppState;

use embassy_net::dns::DnsQueryType;
use embassy_net::tcp::TcpSocket;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpAddress, IpListenEndpoint, Stack};
use embassy_time::{Duration, Timer};

const NTP_INTERVAL_SECS: u64 = 4 * 3600;
const NTP_RETRY_SECS: u64 = 30;
const WEATHER_INTERVAL_SECS: u64 = 300;
const MQTT_PING_INTERVAL_SECS: u64 = 30;

pub async fn network_task(
    stack: Stack<'static>,
    mut wifi_ctrl: esp_radio::wifi::WifiController<'static>,
    state: &'static AppState,
) -> ! {
    loop {
        led_command(LedCommand::WifiConnecting);
        let creds = get_wifi_credentials();
        let mut wifi_ok = false;

        for cred in &creds {
            log::info!("WiFi: connecting to SSID=\"{}\"", mask!(cred.ssid));

            let config = esp_radio::wifi::Config::Station(
                esp_radio::wifi::sta::StationConfig::default()
                    .with_ssid(cred.ssid)
                    .with_password(cred.password.into()),
            );
            let _ = wifi_ctrl.set_config(&config);

            // Give the net_runner_task a chance to process the set_config
            // before we await the connect future.
            Timer::after(Duration::from_millis(10)).await;

            match embassy_time::with_timeout(
                Duration::from_secs(15), wifi_ctrl.connect_async(),
            ).await {
                Ok(Ok(info)) => {
                    log::info!("WiFi: connected to {:?}", info.ssid);
                    wifi_ok = true;
                    break;
                }
                Ok(Err(e)) => {
                    log::warn!("WiFi: connection failed: {:?}", e);
                }
                Err(_) => {
                    log::warn!("WiFi: connection timed out");
                }
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

        stack.wait_config_up().await;
        log::info!("Network: DHCP config received");

        let broker_ip: embassy_net::Ipv4Address =
            match env_or_panic!("MQTT_BROKER").parse() {
            Ok(ip) => ip,
            Err(_) => {
                log::warn!("MQTT: invalid broker IP, retrying");
                led_command(LedCommand::Error);
                Timer::after(Duration::from_secs(30)).await;
                continue;
            }
        };
        let broker_port: u16 = env_or_panic!("MQTT_PORT").parse().unwrap_or(1883);
        let mqtt_user = env_or_panic!("MQTT_USER");
        let mqtt_pass = env_or_panic!("MQTT_PASS");
        let mqtt_client_id = env_or_panic!("MQTT_CLIENT_ID");

        log::info!(
            "MQTT: connecting to {}:{} as '{}'",
            broker_ip, broker_port, mask!(mqtt_client_id)
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
            Ok(()) => log::info!("MQTT: TCP connected"),
            Err(e) => {
                log::warn!("MQTT: TCP connect failed: {:?}", e);
                led_command(LedCommand::Error);
                Timer::after(Duration::from_secs(10)).await;
                continue;
            }
        }

        let connect_pkt = build_connect(mqtt_client_id, mqtt_user, mqtt_pass, 60);
        let _ = mqtt.write(&connect_pkt).await;
        let _ = mqtt.flush().await;
        log::info!("MQTT: CONNECT sent");

        let mut mqtt_established = false;
        let mut subscribe_sent = false;
        let mut subscriptions_pending = 4u8;
        let mut packet_id: u16 = 1;

        loop {
            let mut mqtt_read_buf = [0u8; 1024];

            let read_fut =
                mqtt.read(&mut mqtt_read_buf[..]);
            match embassy_time::with_timeout(
                Duration::from_millis(500), read_fut,
            ).await {
                Ok(Ok(0)) => {
                    log::warn!("MQTT: connection closed");
                    break;
                }
                Ok(Ok(len)) => {
                    handle_mqtt_data(
                        &mqtt_read_buf[..len],
                        &mut mqtt_established,
                        &mut subscribe_sent,
                        &mut subscriptions_pending,
                        &state,
                        packet_id,
                    );
                }
                Ok(Err(_)) => {
                    log::warn!("MQTT: read error");
                    break;
                }
                Err(_) => {}
            }

            if mqtt_established && subscribe_sent && subscriptions_pending > 0 {
                for &topic in &topics {
                    let sub = build_subscribe(topic, packet_id);
                    packet_id = packet_id.wrapping_add(1);
                    if mqtt.write(&sub).await.is_ok() {
                        log::info!("MQTT: SUBSCRIBE sent for {}", topic);
                    }
                }
                subscribe_sent = false;
            }

            let now = embassy_time::Instant::now();

            let ping_due = now - last_ping
                > embassy_time::Duration::from_secs(MQTT_PING_INTERVAL_SECS);
            if mqtt_established && ping_due {
                let _ = mqtt.write(&build_pingreq()).await;
                last_ping = now;
            }

            if ntp_due
                || (now - last_ntp
                    > embassy_time::Duration::from_secs(NTP_INTERVAL_SECS)
                    && now - last_ntp_attempt
                        > embassy_time::Duration::from_secs(NTP_RETRY_SECS))
            {
                ntp_due = false;
                last_ntp_attempt = now;
                if let Some(local) = ntp_sync(&stack).await {
                    state.set_local_time(local);
                    log::info!("NTP: time synchronized");
                    last_ntp = now;
                }
            }

            if weather_due
                || now - last_weather
                    > embassy_time::Duration::from_secs(WEATHER_INTERVAL_SECS)
            {
                weather_due = false;
                fetch_weather(&stack, &state).await;
                last_weather = now;
            }

            if !mqtt.may_send() && mqtt_established {
                log::warn!("MQTT: connection lost, reconnecting...");
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
    match parse_packet_type(data[0]) {
        Some(MqttPacketType::Connack) => {
            if data.len() >= 4 && data[3] == 0 {
                log::info!("MQTT: CONNACK success");
                *mqtt_established = true;
                *subscribe_sent = true;
                *subscriptions_pending = 4;
                led_command(LedCommand::Connected);
            }
        }
        Some(MqttPacketType::Suback) => {
            if *subscriptions_pending > 0 { *subscriptions_pending -= 1; }
            log::info!("MQTT: SUBACK received ({} remaining)", *subscriptions_pending);
        }
        Some(MqttPacketType::Publish) => {
            if let Some((topic, payload)) = parse_publish(data) {
                dispatch_mqtt_message(topic, payload, state);
            }
        }
        Some(MqttPacketType::PingResp) => {}
        _ => log::debug!("MQTT: unknown packet type 0x{:02X}", data[0]),
    }
}

/// Strip any non-UTF-8 bytes from both ends of a byte slice so
/// the remaining portion decodes as valid UTF-8. Uses
/// `from_utf8` error info to skip past invalid bytes at the
/// leading edge, then walks backward from the trailing edge.
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
            log::warn!("MQTT: non-UTF-8 payload on {}", topic);
            return;
        }
    };
    if topic == "vps/monitor" { handle_vps_data(msg, state); }
    else if topic == "host/monitor" { handle_host_data(msg, state); }
    else if topic.starts_with("Sensors") || topic.starts_with("sensors") {
        handle_sensor_data(msg, state);
    }
    else { log::debug!("MQTT: unhandled topic {}", topic); }
}

fn json_extract<'a>(msg: &'a str, key: &str) -> Option<&'a str> {
    let search = &format!("\"{}\"", key);
    let idx = msg.find(search.as_str())?;
    let after_key = &msg[idx + search.len()..];
    let rest = after_key.trim_start();
    let rest = rest.strip_prefix(':')?;
    let rest = rest.trim_start();
    if let Some(s) = rest.strip_prefix('"') {
        let end = s.find('"')?;
        Some(&s[..end])
    } else {
        let end = rest.find(|c| c == ',' || c == '}' || c == ']').unwrap_or(rest.len());
        Some(rest[..end].trim())
    }
}

fn handle_vps_data(msg: &str, state: &AppState) {
    let cpu = json_extract(msg, "cpu")
        .and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let ram = json_extract(msg, "ram")
        .and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let disk = json_extract(msg, "disk")
        .and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let uptime = json_extract(msg, "uptime")
        .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    state.set_vps(cpu, ram, disk, uptime);
}

fn handle_host_data(msg: &str, state: &AppState) {
    let cpu_temp = json_extract(msg, "cpu_temp")
        .and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let ram = json_extract(msg, "ram")
        .and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let ssd_temp = json_extract(msg, "ssd_temp")
        .and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let net_down = json_extract(msg, "net_down")
        .and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let cpu = parse_cpu_array(msg);
    state.set_host(cpu, cpu_temp, ram, ssd_temp, net_down);
}

fn parse_cpu_array(msg: &str) -> [f32; 4] {
    let mut result = [0.0f32; 4];
    if let Some(start) = msg.find("\"cpu\"") {
        let after = &msg[start..];
        if let Some(bracket) = after.find('[') {
            let array_str = &after[bracket + 1..];
            let end = array_str.find(']').unwrap_or(array_str.len());
            let mut idx = 0;
            for num_str in array_str[..end].split(',') {
                if idx >= 4 { break; }
                if let Ok(v) = num_str.trim().parse::<f32>() {
                    result[idx] = v; idx += 1;
                }
            }
        }
    }
    result
}

fn handle_sensor_data(msg: &str, state: &AppState) {
    if let Some(temp) = json_extract(msg, "temperature") {
        if let Some(hum) = json_extract(msg, "humidity") {
            if let (Ok(t), Ok(h)) = (temp.parse::<f32>(), hum.parse::<f32>()) {
                state.add_sensor("Temperature", &fmt_1dp_str(t));
                state.add_sensor("Humidity", &fmt_0dp_str(h));
                return;
            }
        }
    }
    if let Some(temp_val) = json_extract(msg, "Temp") {
        if let Some(sensor_id) = json_extract(msg, "id") {
            if let Ok(t) = temp_val.parse::<f32>() {
                let label = sensor_id.split('_').next().unwrap_or("DS18B20");
                state.add_sensor(label, &fmt_1dp_str(t));
            }
        }
    }
}

async fn ntp_sync(stack: &Stack<'static>) -> Option<time::LocalTime> {
    let mut rx_meta = [PacketMetadata::EMPTY; 1];
    let mut rx_buf = [0u8; 48];
    let mut tx_meta = [PacketMetadata::EMPTY; 1];
    let mut tx_buf = [0u8; 48];
    let mut udp = UdpSocket::new(
        *stack,
        &mut rx_meta[..], &mut rx_buf[..],
        &mut tx_meta[..], &mut tx_buf[..],
    );
    let _ = udp.bind(IpListenEndpoint { addr: None, port: 12345 });

    let mut request = [0u8; 48];
    request[0] = 0x1B;
    let ntp_host = env_or_panic!("NTP_SERVER");
    let ntp_server = match ntp_host.parse::<embassy_net::Ipv4Address>() {
        Ok(ip) => ip,
        Err(_) => {
            match stack.dns_query(ntp_host, DnsQueryType::A).await {
                Ok(addrs) => match addrs.get(0) {
                    Some(IpAddress::Ipv4(addr)) => {
                        log::info!("NTP: resolved {} to {}", ntp_host, addr);
                        *addr
                    }
                    _ => {
                        log::warn!("NTP: DNS returned no IPv4 address");
                        return None;
                    }
                },
                Err(e) => {
                    log::warn!("NTP: DNS lookup failed: {:?}", e);
                    return None;
                }
            }
        }
    };
    let _ = udp.send_to(&request, (ntp_server, 123)).await;

    let mut buf = [0u8; 48];
    match embassy_time::with_timeout(
        Duration::from_secs(3), udp.recv_from(&mut buf),
    ).await {
        Ok(Ok((len, _))) if len >= 44 => {
            let secs = u32::from_be_bytes([buf[40], buf[41], buf[42], buf[43]]) as u64;
            if secs == 0 {
                log::warn!("NTP: response has zero transmit timestamp");
                return None;
            }
            time::ntp_to_local(secs).or_else(|| {
                log::warn!("NTP: invalid timestamp {}", secs);
                None
            })
        }
        _ => None,
    }
}

async fn fetch_weather(stack: &Stack<'static>, state: &'static AppState) {
    led_command(LedCommand::WeatherFetching);
    let api_key = env_or_panic!("OWM_API_KEY");
    let city = env_or_panic!("OWM_CITY");
    let country = env_or_panic!("OWM_COUNTRY");

    let host = match stack.dns_query(
        "api.openweathermap.org", DnsQueryType::A,
    ).await {
        Ok(addrs) => match addrs.get(0) {
            Some(IpAddress::Ipv4(addr)) => *addr,
            _ => {
                log::warn!("Weather: DNS returned no IPv4 address");
                return;
            }
        },
        Err(e) => {
            log::warn!("Weather: DNS lookup failed: {:?}", e);
            return;
        }
    };
    log::info!("Weather: resolved api.openweathermap.org to {}", host);

    let request_str = format!(
        "GET /data/2.5/weather?q={},{}&appid={}&units=metric&lang=de HTTP/1.1\r\n\
         Host: api.openweathermap.org\r\nConnection: close\r\n\r\n",
        city, country, api_key
    );

    let mut rx_buf = [0u8; 2048];
    let mut tx_buf = [0u8; 512];
    let mut tcp = TcpSocket::new(*stack, &mut rx_buf[..], &mut tx_buf[..]);

    if embassy_time::with_timeout(
        Duration::from_secs(10), tcp.connect((host, 80)),
    ).await.is_err() {
        log::warn!("Weather: TCP connect to {} timed out", host);
        return;
    }

    let (mut reader, mut writer) = tcp.split();
    if writer.write(request_str.as_bytes()).await.is_err() {
        log::warn!("Weather: HTTP write failed");
        return;
    }
    if writer.flush().await.is_err() {
        log::warn!("Weather: HTTP flush failed");
        return;
    }

    let mut resp_buf = [0u8; 4096];
    let mut total = 0usize;
    loop {
        match embassy_time::with_timeout(
            Duration::from_secs(5), reader.read(&mut resp_buf[total..]),
        ).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => total += n,
            _ => break,
        }
        if total >= resp_buf.len() { break; }
    }
    if total == 0 {
        log::warn!("Weather: empty response");
        return;
    }

    let resp = core::str::from_utf8(&resp_buf[..total]).unwrap_or("");
    log::info!("Weather: HTTP response ({} bytes)", total);
    let preview_len = resp.len().min(500);
    log::info!("Weather: response preview: {}", &resp[..preview_len]);
    let body = resp.split("\r\n\r\n").nth(1).unwrap_or("");
    if body.is_empty() {
        log::warn!("Weather: no HTTP body found");
        return;
    }

    let temp = json_extract(body, "temp")
        .and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let humidity = json_extract(body, "humidity")
        .and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let pressure = json_extract(body, "pressure")
        .and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let wind = json_extract(body, "speed")
        .and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let desc = json_extract(body, "description").unwrap_or("--");
    let icon = json_extract(body, "icon").unwrap_or("--");

    state.set_weather(temp, humidity, wind, pressure, desc, icon);
    led_command(LedCommand::Connected);
    log::info!("Weather: {}C {} {} humidity={}% wind={}km/h",
        fmt_1dp_str(temp), desc, icon, fmt_0dp_str(humidity),
        fmt_1dp_str(wind));
}
