use chrono::{Datelike, TimeZone, Timelike};
use chrono_tz::Europe::Berlin;

pub const NTP_TO_UNIX_EPOCH: u64 = 2_208_988_800;

#[derive(Debug, Clone, Copy, Default, defmt::Format)]
pub struct LocalTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub weekday: u8,
}

impl LocalTime {
    /// Converts a Unix timestamp to LocalTime based on Berlin timezone
    pub fn from_unix(unix_ts: i64) -> Option<Self> {
        let dt = Berlin.timestamp_opt(unix_ts, 0).earliest()?;
        Some(Self {
            year: dt.year() as u16,
            month: dt.month() as u8,
            day: dt.day() as u8,
            hour: dt.hour() as u8,
            minute: dt.minute() as u8,
            second: dt.second() as u8,
            weekday: dt.weekday().num_days_from_monday() as u8,
        })
    }
}

pub fn ntp_to_unix(ntp_secs: u64) -> i64 {
    ntp_secs.wrapping_sub(NTP_TO_UNIX_EPOCH) as i64
}

pub fn ntp_to_local(ntp_secs: u64) -> Option<LocalTime> {
    LocalTime::from_unix(ntp_to_unix(ntp_secs))
}
