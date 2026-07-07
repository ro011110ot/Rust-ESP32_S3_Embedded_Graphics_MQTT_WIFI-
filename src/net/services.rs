use embassy_net::dns::DnsQueryType;
use embassy_net::tcp::TcpSocket;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpAddress, IpListenEndpoint, Stack};
use embassy_time::Duration;

use crate::led::LedCommand;
use crate::time;
use crate::AppState;

use super::led_command;
use super::mqtt::json_extract;

pub async fn ntp_sync(stack: &Stack<'static>) -> Option<time::LocalTime> {
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
                Ok(addrs) => match addrs.first() {
                    Some(IpAddress::Ipv4(addr)) => {
                        defmt::info!("NTP: resolved {} to {}", ntp_host, addr);
                        *addr
                    }
                    _ => {
                        defmt::warn!("NTP: DNS returned no IPv4 address");
                        return None;
                    }
                },
                Err(e) => {
                    defmt::warn!(
                        "NTP: DNS lookup failed: {:?}",
                        defmt::Debug2Format(&e),
                    );
                    return None;
                }
            }
        }
    };
    let _ = udp.send_to(&request, (ntp_server, 123)).await;

    let mut buf = [0u8; 48];
    match embassy_time::with_timeout(
        Duration::from_secs(3), udp.recv_from(&mut buf),
    )
        .await
    {
        Ok(Ok((len, _))) if len >= 44 => {
            let secs = u32::from_be_bytes([buf[40], buf[41], buf[42], buf[43]]) as u64;
            if secs == 0 {
                defmt::warn!("NTP: response has zero transmit timestamp");
                return None;
            }
            time::ntp_to_local(secs).or_else(|| {
                defmt::warn!("NTP: invalid timestamp {}", secs);
                None
            })
        }
        _ => None,
    }
}

pub async fn fetch_weather(stack: &Stack<'static>, state: &AppState) {
    led_command(LedCommand::WeatherFetching);
    let api_key = env_or_panic!("OWM_API_KEY");
    let city = env_or_panic!("OWM_CITY");
    let country = env_or_panic!("OWM_COUNTRY");

    let host = match stack.dns_query(
        "api.openweathermap.org", DnsQueryType::A,
    )
        .await
    {
        Ok(addrs) => match addrs.first() {
            Some(IpAddress::Ipv4(addr)) => *addr,
            _ => {
                defmt::warn!("Weather: DNS returned no IPv4 address");
                return;
            }
        },
        Err(e) => {
            defmt::warn!(
                "Weather: DNS lookup failed: {:?}",
                defmt::Debug2Format(&e),
            );
            return;
        }
    };
    defmt::info!("Weather: resolved api.openweathermap.org to {}", host);

    let request_str = alloc::format!(
        "GET /data/2.5/weather?q={},{}&appid={}&units=metric&lang=de HTTP/1.1\r\n\
         Host: api.openweathermap.org\r\nConnection: close\r\n\r\n",
        city, country, api_key,
    );

    let mut rx_buf = [0u8; 2048];
    let mut tx_buf = [0u8; 512];
    let mut tcp = TcpSocket::new(*stack, &mut rx_buf[..], &mut tx_buf[..]);

    if embassy_time::with_timeout(
        Duration::from_secs(10), tcp.connect((host, 80)),
    )
        .await
        .is_err()
    {
        defmt::warn!("Weather: TCP connect to {} timed out", host);
        return;
    }

    let (mut reader, mut writer) = tcp.split();
    if writer.write(request_str.as_bytes()).await.is_err() {
        defmt::warn!("Weather: HTTP write failed");
        return;
    }
    if writer.flush().await.is_err() {
        defmt::warn!("Weather: HTTP flush failed");
        return;
    }

    let mut resp_buf = [0u8; 4096];
    let mut total = 0usize;
    loop {
        match embassy_time::with_timeout(
            Duration::from_secs(5), reader.read(&mut resp_buf[total..]),
        )
            .await
        {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => total += n,
            _ => break,
        }
        if total >= resp_buf.len() { break; }
    }
    if total == 0 {
        defmt::warn!("Weather: empty response");
        return;
    }

    let resp = core::str::from_utf8(&resp_buf[..total]).unwrap_or("");
    defmt::info!("Weather: HTTP response ({} bytes)", total);
    let preview_len = resp.len().min(500);
    defmt::info!("Weather: response preview: {}", &resp[..preview_len]);
    let body = resp.split("\r\n\r\n").nth(1).unwrap_or("");
    if body.is_empty() {
        defmt::warn!("Weather: no HTTP body found");
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
    defmt::info!("Weather: fetch complete");

    led_command(LedCommand::Connected);
}
