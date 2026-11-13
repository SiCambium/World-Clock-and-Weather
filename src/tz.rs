//! Time zone helpers built on chrono-tz.

use chrono::{DateTime, Offset, TimeZone, Utc};
use chrono_tz::Tz;

/// Local wall-clock time in a zone. Falls back to UTC for unknown zones.
pub fn local(zone: &str, now: DateTime<Utc>) -> DateTime<chrono::FixedOffset> {
    match zone.parse::<Tz>() {
        Ok(tz) => tz.from_utc_datetime(&now.naive_utc()).fixed_offset(),
        Err(_) => now.fixed_offset(),
    }
}

/// UTC offset of a zone in minutes at the given instant.
pub fn offset_minutes(zone: &str, now: DateTime<Utc>) -> i32 {
    match zone.parse::<Tz>() {
        Ok(tz) => tz.offset_from_utc_datetime(&now.naive_utc()).fix().local_minus_utc() / 60,
        Err(_) => 0,
    }
}

/// Formats an offset like "UTC+5:30" or "UTC-4".
pub fn offset_label(prefix: &str, minutes: i32) -> String {
    let sign = if minutes < 0 { '-' } else { '+' };
    let abs = minutes.abs();
    let (h, m) = (abs / 60, abs % 60);
    if m == 0 {
        format!("{prefix}{sign}{h}")
    } else {
        format!("{prefix}{sign}{h}:{m:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(offset_label("UTC", 330), "UTC+5:30");
        assert_eq!(offset_label("UTC", -240), "UTC-4");
        assert_eq!(offset_label("GMT", 0), "GMT+0");
    }

    #[test]
    fn known_zone() {
        let t = Utc.with_ymd_and_hms(2026, 1, 15, 12, 0, 0).unwrap();
        assert_eq!(offset_minutes("Asia/Kolkata", t), 330);
        assert_eq!(offset_minutes("Europe/London", t), 0);
        assert_eq!(offset_minutes("Not/AZone", t), 0);
    }
}
