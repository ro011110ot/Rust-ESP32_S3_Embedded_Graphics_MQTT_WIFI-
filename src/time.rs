/// Central European Time (CET, UTC+1) / Central European Summer Time
/// (CEST, UTC+2) conversion with NTP epoch handling.
///
/// The EU DST rule: clock advances on the last Sunday of March at 01:00 UTC
/// and falls back on the last Sunday of October at 01:00 UTC.
///
/// All functions are `const`-compatible or trivial so they can run on
/// any core, including in interrupt context.

/// Seconds between the NTP epoch (1900-01-01) and the Unix epoch
/// (1970-01-01).  NTP timestamps from `pool.ntp.org` use the former.
pub const NTP_TO_UNIX_EPOCH: u64 = 2_208_988_800;

/// Seconds in one hour — the basic DST offset unit.
pub const SECS_PER_HOUR: u64 = 3600;

/// Seconds in one day.
pub const SECS_PER_DAY: u64 = 86_400;

/// Represents a resolved civil time with all fields suitable for display.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    /// 0=Monday … 6=Sunday
    pub weekday: u8,
}

// ---------------------------------------------------------------------------
// Day-of-week helpers
// ---------------------------------------------------------------------------

/// Tomohiko Sakamoto's day-of-week algorithm.
///
/// Returns 0=Sunday, 1=Monday, … 6=Saturday.
const fn day_of_week(year: u32, month: u32, day: u32) -> u32 {
    let t: [u32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if month < 3 { year.wrapping_sub(1) } else { year };
    (y + y / 4 - y / 100 + y / 400 + t[(month - 1) as usize] + day) % 7
}

/// Leap-year check following the Gregorian calendar.
const fn is_leap(year: u32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// Number of days in a given month (1=January … 12=December).
const fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(year) { 29 } else { 28 }
        }
        _ => 0,
    }
}

/// Find the day-of-month of the last Sunday in a given month and year.
const fn last_sunday(year: u32, month: u32) -> u32 {
    let last = days_in_month(year, month);
    let dow = day_of_week(year, month, last);
    // dow is 0=Sun … 6=Sat, so subtract dow to get the last Sunday
    last - dow
}

/// Return the DST offset in seconds (+3600 CET, +7200 CEST) for a Unix
/// timestamp (seconds since 1970-01-01 00:00:00 UTC).
const fn dst_offset(unix_ts: i64) -> i64 {
    if unix_ts < 0 {
        return SECS_PER_HOUR as i64;
    }
    let days = unix_ts / SECS_PER_DAY as i64;
    let _time_secs = unix_ts % SECS_PER_DAY as i64;

    // Walk forward from 1970 to find the correct year/month/day/hour.
    let mut y = 1970u32;
    let mut rem = days;
    loop {
        let diy = if is_leap(y) { 366 } else { 365 };
        if rem < diy {
            break;
        }
        rem -= diy;
        y += 1;
    }

    let mut m = 1u32;
    loop {
        let dim = days_in_month(y, m) as i64;
        if rem < dim {
            break;
        }
        rem -= dim;
        m += 1;
    }

    // DST transition timestamps for this year (seconds since epoch).
    let dst_start_day = last_sunday(y, 3);
    let dst_end_day = last_sunday(y, 10);

    // Compute Unix timestamp for the transition days at 01:00 UTC.
    let dst_start = unix_ts_for_date(y, 3, dst_start_day) + SECS_PER_HOUR as i64;
    let dst_end = unix_ts_for_date(y, 10, dst_end_day) + SECS_PER_HOUR as i64;

    if unix_ts >= dst_start && unix_ts < dst_end {
        2 * SECS_PER_HOUR as i64  // CEST
    } else {
        SECS_PER_HOUR as i64      // CET
    }
}

/// Compute the Unix timestamp for midnight (00:00 UTC) of a given date.
const fn unix_ts_for_date(year: u32, month: u32, day: u32) -> i64 {
    let mut total = 0i64;
    let mut y = 1970u32;
    while y < year {
        total += if is_leap(y) { 366 } else { 365 };
        y += 1;
    }
    let mut m = 1u32;
    while m < month {
        total += days_in_month(year, m) as i64;
        m += 1;
    }
    total += (day as i64 - 1);
    total * SECS_PER_DAY as i64
}

/// Convert a Unix timestamp to a `LocalTime` by applying the CET/CEST
/// DST offset appropriate for the timestamp.
pub fn unix_to_local(unix_ts: i64) -> LocalTime {
    let offset = dst_offset(unix_ts);
    let local_ts = unix_ts + offset;
    unix_to_utc(local_ts)
}

/// Decompose a Unix timestamp (already in "local" zone) into a `LocalTime`.
fn unix_to_utc(ts: i64) -> LocalTime {
    let mut days = ts / SECS_PER_DAY as i64;
    if ts < 0 {
        return LocalTime::default();
    }
    let rem = ts % SECS_PER_DAY as i64;
    let h = (rem / SECS_PER_HOUR as i64) as u8;
    let m = ((rem % SECS_PER_HOUR as i64) / 60) as u8;
    let s = (rem % 60) as u8;

    let mut y = 1970u32;
    loop {
        let diy = if is_leap(y) { 366 } else { 365 };
        if days < diy {
            break;
        }
        days -= diy;
        y += 1;
    }

    let mut mo = 1u32;
    loop {
        let dim = days_in_month(y, mo) as i64;
        if days < dim {
            break;
        }
        days -= dim;
        mo += 1;
    }

    let d = (days + 1) as u8;
    let wd = day_of_week(y, mo, d as u32) as u8;
    // Convert from 0=Sun..6=Sat to 0=Mon..6=Sun
    let wd = (wd + 6) % 7;

    LocalTime {
        year: y as u16,
        month: mo as u8,
        day: d,
        hour: h,
        minute: m,
        second: s,
        weekday: wd,
    }
}

/// Convert an NTP 64-bit timestamp (seconds since 1900-01-01) to a Unix
/// timestamp (seconds since 1970-01-01).
pub fn ntp_to_unix(ntp_secs: u64) -> i64 {
    (ntp_secs.wrapping_sub(NTP_TO_UNIX_EPOCH)) as i64
}

/// Convenience: convert an NTP timestamp directly to a local `LocalTime`.
pub fn ntp_to_local(ntp_secs: u64) -> LocalTime {
    let unix = ntp_to_unix(ntp_secs);
    unix_to_local(unix)
}

// ---------------------------------------------------------------------------
// Tests (only compile on host)
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_dates() {
        // 2024-03-31 01:00 UTC should be the switch to CEST.
        // The last Sunday of March 2024 is March 31.
        let ts = unix_ts_for_date(2024, 3, 31) + SECS_PER_HOUR as i64;
        assert_eq!(dst_offset(ts - 1), SECS_PER_HOUR as i64);      // CET
        assert_eq!(dst_offset(ts), 2 * SECS_PER_HOUR as i64);       // CEST

        // 2024-10-27 01:00 UTC should be the switch back to CET.
        let ts2 = unix_ts_for_date(2024, 10, 27) + SECS_PER_HOUR as i64;
        assert_eq!(dst_offset(ts2 - 1), 2 * SECS_PER_HOUR as i64);  // CEST
        assert_eq!(dst_offset(ts2), SECS_PER_HOUR as i64);           // CET
    }

    #[test]
    fn test_ntp_conversion() {
        // NTP time 0 = 1900-01-01, Unix time should be negative (before 1970)
        assert!(ntp_to_unix(0) < 0);

        // NTP time for 2024-01-01 00:00:00 UTC
        let ntp = NTP_TO_UNIX_EPOCH + 1704067200;
        let local = ntp_to_local(ntp);
        // Should be CET (UTC+1), so hour should be 1 on Jan 1.
        assert_eq!(local.month, 1);
        assert_eq!(local.day, 1);
        assert_eq!(local.hour, 1);
    }
}
