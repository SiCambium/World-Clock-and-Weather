//! Temperature, wind and clock formats. Weather is always fetched and cached
//! in °C and km/h and converted here, so switching units is instant and the
//! offline cache stays valid.

use chrono::Timelike;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum TempUnit {
    Celsius,
    Fahrenheit,
}

impl TempUnit {
    pub const ALL: [TempUnit; 2] = [TempUnit::Celsius, TempUnit::Fahrenheit];

    pub fn label(self) -> &'static str {
        match self {
            TempUnit::Celsius => "°C",
            TempUnit::Fahrenheit => "°F",
        }
    }

    pub fn convert(self, celsius: f64) -> f64 {
        match self {
            TempUnit::Celsius => celsius,
            TempUnit::Fahrenheit => celsius * 9.0 / 5.0 + 32.0,
        }
    }

    /// A rounded temperature with a degree sign, or a dash if unknown.
    pub fn fmt(self, celsius: f64) -> String {
        if celsius.is_finite() {
            // `+ 0.0` turns -0 into 0.
            format!("{}°", self.convert(celsius).round() + 0.0)
        } else {
            "–".into()
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum WindUnit {
    Kmh,
    Mph,
    Ms,
    Knots,
}

impl WindUnit {
    pub const ALL: [WindUnit; 4] = [WindUnit::Kmh, WindUnit::Mph, WindUnit::Ms, WindUnit::Knots];

    pub fn label(self) -> &'static str {
        match self {
            WindUnit::Kmh => "km/h",
            WindUnit::Mph => "mph",
            WindUnit::Ms => "m/s",
            WindUnit::Knots => "kn",
        }
    }

    pub fn fmt(self, kmh: f64) -> String {
        let v = match self {
            WindUnit::Kmh => kmh,
            WindUnit::Mph => kmh / 1.609_344,
            WindUnit::Ms => kmh / 3.6,
            WindUnit::Knots => kmh / 1.852,
        };
        format!("{} {}", v.round() + 0.0, self.label())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum HourFormat {
    H24,
    H12,
}

impl HourFormat {
    pub const ALL: [HourFormat; 2] = [HourFormat::H24, HourFormat::H12];

    pub fn label(self) -> &'static str {
        match self {
            HourFormat::H24 => "24-hour",
            HourFormat::H12 => "12-hour",
        }
    }

    /// "14:05" or "2:05", plus "PM" for 12-hour time.
    pub fn split(self, t: &impl Timelike) -> (String, &'static str) {
        match self {
            HourFormat::H24 => (format!("{:02}:{:02}", t.hour(), t.minute()), ""),
            HourFormat::H12 => {
                let (pm, h) = t.hour12();
                (format!("{h}:{:02}", t.minute()), if pm { "PM" } else { "AM" })
            }
        }
    }

    /// Hours and minutes as one string, for labels.
    pub fn short(self, t: &impl Timelike) -> String {
        match self.split(t) {
            (hm, "") => hm,
            (hm, ampm) => format!("{hm} {ampm}"),
        }
    }
}

/// Defaults that follow the Mac's region, read once at start-up.
pub struct RegionDefaults {
    pub temp: TempUnit,
    pub wind: WindUnit,
    pub hours: HourFormat,
}

pub fn region_defaults() -> RegionDefaults {
    let region = sys_locale::get_locale()
        .and_then(|l| {
            l.split(['-', '_'])
                .skip(1)
                .find(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()))
                .map(str::to_ascii_uppercase)
        })
        .unwrap_or_default();
    defaults_for(&region)
}

fn defaults_for(region: &str) -> RegionDefaults {
    let fahrenheit = matches!(region, "US" | "LR" | "MM" | "BS" | "BZ" | "KY" | "PW" | "FM" | "MH");
    let mph = matches!(region, "US" | "GB" | "LR" | "MM");
    let twelve = matches!(
        region,
        "US" | "CA" | "AU" | "NZ" | "IN" | "PH" | "PK" | "EG" | "SA" | "MY" | "BD" | "CO" | "MX"
    );
    RegionDefaults {
        temp: if fahrenheit { TempUnit::Fahrenheit } else { TempUnit::Celsius },
        wind: if mph { WindUnit::Mph } else { WindUnit::Kmh },
        hours: if twelve { HourFormat::H12 } else { HourFormat::H24 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveTime;

    #[test]
    fn temperatures() {
        assert_eq!(TempUnit::Celsius.fmt(-0.2), "0°");
        assert_eq!(TempUnit::Fahrenheit.fmt(100.0), "212°");
        assert_eq!(TempUnit::Fahrenheit.fmt(f64::NAN), "–");
    }

    #[test]
    fn wind() {
        assert_eq!(WindUnit::Mph.fmt(16.09344), "10 mph");
        assert_eq!(WindUnit::Ms.fmt(36.0), "10 m/s");
    }

    #[test]
    fn hours() {
        let t = NaiveTime::from_hms_opt(14, 5, 0).unwrap();
        assert_eq!(HourFormat::H24.short(&t), "14:05");
        assert_eq!(HourFormat::H12.short(&t), "2:05 PM");
        let midnight = NaiveTime::from_hms_opt(0, 30, 0).unwrap();
        assert_eq!(HourFormat::H12.short(&midnight), "12:30 AM");
    }

    #[test]
    fn regions() {
        assert_eq!(defaults_for("US").temp, TempUnit::Fahrenheit);
        assert_eq!(defaults_for("GB").wind, WindUnit::Mph);
        assert_eq!(defaults_for("GB").hours, HourFormat::H24);
        assert_eq!(defaults_for("DE").wind, WindUnit::Kmh);
    }
}
