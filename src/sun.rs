//! Where the sun is: the point on Earth where it is overhead, its height
//! above the horizon anywhere, and sunrise and sunset times.
//!
//! Uses the low-precision formulas from the Astronomical Almanac (good to
//! about a minute for sunrise and sunset), so it needs no network or data.

use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};

/// Sun altitude used for sunrise and sunset: the upper edge on the horizon,
/// allowing for refraction.
pub const HORIZON: f64 = -0.833;
/// Below this the sky is fully dark (end of civil twilight is -6).
pub const CIVIL: f64 = -6.0;

/// Declination (degrees) and equation of time (minutes) at an instant.
fn ephemeris(t: DateTime<Utc>) -> (f64, f64) {
    let jd = t.timestamp() as f64 / 86_400.0 + 2_440_587.5;
    let n = jd - 2_451_545.0;
    let l = (280.460 + 0.985_647_4 * n).rem_euclid(360.0);
    let g = (357.528 + 0.985_600_3 * n).rem_euclid(360.0).to_radians();
    let lambda = (l + 1.915 * g.sin() + 0.020 * (2.0 * g).sin()).to_radians();
    let eps = (23.439 - 0.000_000_4 * n).to_radians();
    let decl = (eps.sin() * lambda.sin()).asin().to_degrees();
    let ra = (eps.cos() * lambda.sin()).atan2(lambda.cos()).to_degrees();
    let mut diff = l - ra;
    diff -= (diff / 360.0).round() * 360.0;
    (decl, diff * 4.0)
}

/// The point (longitude, latitude) where the sun is directly overhead.
pub fn subsolar(t: DateTime<Utc>) -> (f64, f64) {
    let (decl, eot) = ephemeris(t);
    let hours = t.timestamp().rem_euclid(86_400) as f64 / 3600.0;
    let mut lon = -15.0 * (hours - 12.0 + eot / 60.0);
    lon -= (lon / 360.0).round() * 360.0;
    (lon, decl)
}

/// The sun's height above the horizon (degrees) at a place, given the
/// subsolar point.
pub fn altitude_from(sub: (f64, f64), lon: f64, lat: f64) -> f64 {
    let (phi, decl) = (lat.to_radians(), sub.1.to_radians());
    let h = (lon - sub.0).to_radians();
    (phi.sin() * decl.sin() + phi.cos() * decl.cos() * h.cos())
        .clamp(-1.0, 1.0)
        .asin()
        .to_degrees()
}

pub fn altitude(t: DateTime<Utc>, lon: f64, lat: f64) -> f64 {
    altitude_from(subsolar(t), lon, lat)
}

/// Latitude ranges (degrees, south to north) along one meridian where the
/// sun is below `level`. Used to shade night on the map.
pub fn below_on_meridian(sub: (f64, f64), lon: f64, level: f64, max_lat: f64) -> Vec<(f64, f64)> {
    // sin(alt) = A sin(phi) + B cos(phi) = R sin(phi + a)
    let decl = sub.1.to_radians();
    let h = (lon - sub.0).to_radians();
    let a_ = decl.sin();
    let b_ = decl.cos() * h.cos();
    let r = (a_ * a_ + b_ * b_).sqrt();
    let s = level.to_radians().sin();
    let lo = -max_lat;
    let hi = max_lat;
    let below = |lat: f64| altitude_from(sub, lon, lat) < level;
    if r < 1e-9 || (s / r).abs() >= 1.0 {
        return if below(0.0) { vec![(lo, hi)] } else { Vec::new() };
    }
    let alpha = b_.atan2(a_);
    let base = (s / r).asin();
    let mut roots: Vec<f64> = [base - alpha, std::f64::consts::PI - base - alpha]
        .into_iter()
        .map(|x| {
            let mut d = x.to_degrees();
            d -= (d / 360.0).round() * 360.0;
            d
        })
        .filter(|d| *d > lo && *d < hi)
        .collect();
    roots.sort_by(f64::total_cmp);
    let mut edges = vec![lo];
    edges.extend(roots);
    edges.push(hi);
    let mut out: Vec<(f64, f64)> = Vec::new();
    for w in edges.windows(2) {
        if below((w[0] + w[1]) / 2.0) {
            match out.last_mut() {
                Some(last) if (last.1 - w[0]).abs() < 1e-9 => last.1 = w[1],
                _ => out.push((w[0], w[1])),
            }
        }
    }
    out
}

/// Sunrise and sunset for a place on a calendar date.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Day {
    Normal { rise: DateTime<Utc>, set: DateTime<Utc> },
    /// The sun stays up all day.
    MidnightSun,
    /// The sun doesn't rise.
    PolarNight,
}

impl Day {
    /// Hours of daylight.
    pub fn length_minutes(&self) -> i64 {
        match self {
            Day::Normal { rise, set } => (*set - *rise).num_minutes(),
            Day::MidnightSun => 24 * 60,
            Day::PolarNight => 0,
        }
    }
}

/// Sunrise and sunset on `date` (local calendar date at the place, whose
/// UTC offset in minutes is `offset`).
pub fn day(date: NaiveDate, offset: i32, lon: f64, lat: f64) -> Day {
    // Start from local noon, then refine twice with the sun's position at
    // each event.
    let noon_local = date.and_hms_opt(12, 0, 0).unwrap();
    let noon_utc = Utc.from_utc_datetime(&noon_local) - Duration::minutes(offset as i64);
    let event = |guess: DateTime<Utc>, rising: bool| -> Option<DateTime<Utc>> {
        let mut t = guess;
        for _ in 0..3 {
            let (decl, eot) = ephemeris(t);
            let (phi, d) = (lat.to_radians(), decl.to_radians());
            let cos_h = (HORIZON.to_radians().sin() - phi.sin() * d.sin()) / (phi.cos() * d.cos());
            if !(-1.0..=1.0).contains(&cos_h) {
                return None;
            }
            let h0 = cos_h.acos().to_degrees() / 15.0; // hours
            // Solar noon in UTC hours on the UTC date of `t`'s local day.
            let solar_noon = 12.0 - lon / 15.0 - eot / 60.0;
            let hours = if rising { solar_noon - h0 } else { solar_noon + h0 };
            let day_start = Utc.from_utc_datetime(&noon_utc.date_naive().and_hms_opt(0, 0, 0).unwrap());
            let mut cand = day_start + Duration::milliseconds((hours * 3_600_000.0) as i64);
            // Keep the event within 14 hours of local noon.
            while cand - noon_utc > Duration::hours(14) {
                cand -= Duration::days(1);
            }
            while noon_utc - cand > Duration::hours(14) {
                cand += Duration::days(1);
            }
            t = cand;
        }
        Some(t)
    };
    match (event(noon_utc, true), event(noon_utc, false)) {
        (Some(rise), Some(set)) => Day::Normal { rise, set },
        _ => {
            if altitude(noon_utc, lon, lat) > HORIZON {
                Day::MidnightSun
            } else {
                Day::PolarNight
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn subsolar_near_equinox_noon() {
        let t = Utc.with_ymd_and_hms(2026, 3, 20, 12, 0, 0).unwrap();
        let (lon, lat) = subsolar(t);
        assert!(lat.abs() < 1.0, "{lat}");
        assert!(lon.abs() < 3.0, "{lon}");
    }

    #[test]
    fn solstice_declination() {
        let (_, lat) = subsolar(Utc.with_ymd_and_hms(2026, 6, 21, 12, 0, 0).unwrap());
        assert!((lat - 23.44).abs() < 0.1, "{lat}");
    }

    #[test]
    fn london_sunrise_in_november() {
        // 16 Nov 2026 in London: sunrise about 07:20, sunset about 16:07 (GMT).
        let d = day(NaiveDate::from_ymd_opt(2026, 11, 16).unwrap(), 0, -0.1278, 51.5074);
        let Day::Normal { rise, set } = d else { panic!("{d:?}") };
        let rise_m = rise.hour() * 60 + rise.minute();
        let set_m = set.hour() * 60 + set.minute();
        assert!((rise_m as i32 - (7 * 60 + 20)).abs() <= 3, "{rise}");
        assert!((set_m as i32 - (16 * 60 + 7)).abs() <= 3, "{set}");
    }

    #[test]
    fn sydney_with_offset() {
        // Sydney, 16 Nov 2026 (AEDT, +11): sunrise about 05:46, sunset about 19:39.
        let d = day(NaiveDate::from_ymd_opt(2026, 11, 16).unwrap(), 660, 151.2093, -33.8688);
        let Day::Normal { rise, set } = d else { panic!("{d:?}") };
        let local = |t: DateTime<Utc>| {
            let l = t + Duration::minutes(660);
            l.hour() as i32 * 60 + l.minute() as i32
        };
        assert!((local(rise) - (5 * 60 + 46)).abs() <= 4, "{rise}");
        assert!((local(set) - (19 * 60 + 39)).abs() <= 4, "{set}");
    }

    #[test]
    fn polar() {
        let date = NaiveDate::from_ymd_opt(2026, 12, 21).unwrap();
        assert_eq!(day(date, 0, 15.0, 78.0), Day::PolarNight);
        assert_eq!(day(date, 0, 166.0, -78.0), Day::MidnightSun);
    }

    #[test]
    fn meridian_night_matches_altitude() {
        let t = Utc.with_ymd_and_hms(2026, 11, 16, 15, 0, 0).unwrap();
        let sub = subsolar(t);
        for lon in [-170.0, -90.0, -30.0, 0.0, 45.0, 120.0, 179.0] {
            let ranges = below_on_meridian(sub, lon, 0.0, 85.0);
            for lat in (-84..=84).step_by(4) {
                let lat = lat as f64;
                let inside = ranges.iter().any(|(a, b)| lat > *a && lat < *b);
                let alt = altitude_from(sub, lon, lat);
                if alt.abs() > 0.5 {
                    assert_eq!(inside, alt < 0.0, "lon {lon} lat {lat} alt {alt} {ranges:?}");
                }
            }
        }
    }
}
