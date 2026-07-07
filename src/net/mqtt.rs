use alloc::format;
use heapless::{String, Vec};

use crate::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
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

pub fn encode_remaining_length(mut length: u32) -> Vec<u8, 4> {
    let mut buf = Vec::new();
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
) -> Vec<u8, 256> {
    let mut payload = Vec::<u8, 256>::new();
    payload.extend_from_slice(&[0x00, 0x04, b'M', b'Q', b'T', b'T']).ok();
    payload.push(4).ok();
    payload.push(0x80 | 0x40 | 0x02).ok();
    payload.extend_from_slice(&keepalive.to_be_bytes()).ok();
    append_utf8(&mut payload, client_id);
    append_utf8(&mut payload, username);
    append_utf8(&mut payload, password);
    let mut header = Vec::<u8, 4>::new();
    header.push(0x10).ok();
    header.extend_from_slice(&encode_remaining_length(payload.len() as u32)).ok();
    let mut packet = Vec::<u8, 256>::new();
    packet.extend_from_slice(&header).ok();
    packet.extend_from_slice(&payload).ok();
    packet
}

pub fn build_subscribe(topic_filter: &str, packet_id: u16) -> Vec<u8, 128> {
    let mut payload = Vec::<u8, 256>::new();
    payload.extend_from_slice(&packet_id.to_be_bytes()).ok();
    append_utf8(&mut payload, topic_filter);
    payload.push(0).ok();
    let mut header = Vec::<u8, 4>::new();
    header.push(0x82).ok();
    header.extend_from_slice(&encode_remaining_length(payload.len() as u32)).ok();
    let mut packet = Vec::<u8, 128>::new();
    packet.extend_from_slice(&header).ok();
    packet.extend_from_slice(&payload).ok();
    packet
}

pub fn build_pingreq() -> [u8; 2] { [0xC0, 0x00] }

#[allow(dead_code)]
pub fn build_publish(topic: &str, payload_data: &[u8]) -> Vec<u8, 256> {
    let mut payload = Vec::<u8, 256>::new();
    append_utf8(&mut payload, topic);
    payload.extend_from_slice(payload_data).ok();
    let remaining = encode_remaining_length(payload.len() as u32);
    let mut header = Vec::<u8, 4>::new();
    header.push(0x30).ok();
    header.extend_from_slice(&remaining).ok();
    let mut packet = Vec::<u8, 256>::new();
    packet.extend_from_slice(&header).ok();
    packet.extend_from_slice(&payload).ok();
    packet
}

fn append_utf8(buf: &mut Vec<u8, 256>, s: &str) {
    let len = s.len() as u16;
    buf.extend_from_slice(&len.to_be_bytes()).ok();
    buf.extend_from_slice(s.as_bytes()).ok();
}

pub fn parse_publish(data: &[u8]) -> Option<(&str, &[u8])> {
    let (_, consumed) = decode_remaining_length(&data[1..])?;
    let var_header_start = 1 + consumed;
    if var_header_start + 2 > data.len() { return None; }
    let topic_len = u16::from_be_bytes([data[var_header_start], data[var_header_start + 1]]);
    let topic_start = var_header_start + 2;
    let topic_end = topic_start + topic_len as usize;
    if topic_end > data.len() { return None; }
    let topic = core::str::from_utf8(&data[topic_start..topic_end]).ok()?;
    let payload = &data[topic_end..];
    Some((topic, payload))
}

// ============================================================================
// Payload Parsing Helpers
// ============================================================================

fn fmt_1dp_str(val: f32) -> String<16> {
    let i = val as i32;
    let frac = (val.abs() - (val as i32).abs() as f32) * 10.0 + 0.5;
    let f = frac as u32 % 10;
    let mut s = String::new();
    let _ = core::fmt::write(&mut s, format_args!("{}.{}C", i, f));
    s
}

fn fmt_0dp_str(val: f32) -> String<16> {
    let i = (if val >= 0.0 { val + 0.5 } else { val - 0.5 }) as i32;
    let mut s = String::new();
    let _ = core::fmt::write(&mut s, format_args!("{}%", i));
    s
}

pub(crate) fn json_extract<'a>(msg: &'a str, key: &str) -> Option<&'a str> {
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

pub(crate) fn handle_vps_data(msg: &str, state: &AppState) {
    let cpu = json_extract(msg, "cpu")
        .and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let ram = json_extract(msg, "ram")
        .and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let disk = json_extract(msg, "disk")
        .and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let uptime = json_extract(msg, "uptime_seconds")
        .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    state.set_vps(cpu, ram, disk, uptime);
}

pub(crate) fn handle_host_data(msg: &str, state: &AppState) {
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

pub(crate) fn handle_sensor_data(msg: &str, state: &AppState) {
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
