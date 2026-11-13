//! Plain data types shared across the app.

use serde::{Deserialize, Serialize};

pub const MIN_PLACES: usize = 2;
pub const MAX_PLACES: usize = 10;

/// A saved location the user is tracking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub admin1: String,
    #[serde(default)]
    pub country: String,
    pub timezone: String,
    pub latitude: f64,
    pub longitude: f64,
}

impl Place {
    pub fn from_result(r: &GeocodeResult) -> Self {
        Self {
            id: place_id(r.latitude, r.longitude),
            name: r.name.clone(),
            admin1: r.admin1.clone(),
            country: r.country.clone(),
            timezone: r.timezone.clone(),
            latitude: r.latitude,
            longitude: r.longitude,
        }
    }

    pub fn region(&self) -> String {
        region_label(&self.admin1, &self.country)
    }
}

/// A candidate returned from a city search.
#[derive(Debug, Clone, PartialEq)]
pub struct GeocodeResult {
    pub name: String,
    pub admin1: String,
    pub country: String,
    pub timezone: String,
    pub latitude: f64,
    pub longitude: f64,
}

impl GeocodeResult {
    pub fn region(&self) -> String {
        region_label(&self.admin1, &self.country)
    }
}

/// Current conditions plus today's high and low for one place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weather {
    pub temperature: f64,
    pub apparent: f64,
    pub humidity: f64,
    pub wind_speed: f64,
    pub code: i32,
    pub is_day: bool,
    pub high: f64,
    pub low: f64,
}

/// One hour of today's forecast.
#[derive(Debug, Clone, PartialEq)]
pub struct HourPoint {
    /// Local time as "HH:MM".
    pub label: String,
    pub temperature: f64,
    pub code: i32,
    pub precip_prob: f64,
}

/// One day of the multi-day outlook.
#[derive(Debug, Clone, PartialEq)]
pub struct DayPoint {
    /// ISO date, "YYYY-MM-DD".
    pub date: String,
    pub code: i32,
    pub high: f64,
    pub low: f64,
    pub precip_prob: f64,
}

/// Hourly view of today plus a seven day outlook for one place.
#[derive(Debug, Clone, PartialEq)]
pub struct DetailedForecast {
    pub hourly: Vec<HourPoint>,
    pub daily: Vec<DayPoint>,
}

/// Stable identifier for a location, matching the original app's format so
/// saved places carry over.
pub fn place_id(lat: f64, lon: f64) -> String {
    format!("{lat:.4}_{lon:.4}")
}

fn region_label(admin1: &str, country: &str) -> String {
    [admin1, country]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(", ")
}

/// WMO weather code to a human label.
pub fn condition_label(code: i32) -> &'static str {
    match code {
        0 => "Clear sky",
        1 => "Mostly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 => "Fog",
        48 => "Rime fog",
        51 => "Light drizzle",
        53 => "Drizzle",
        55 => "Dense drizzle",
        56 | 57 => "Freezing drizzle",
        61 => "Light rain",
        63 => "Rain",
        65 => "Heavy rain",
        66 | 67 => "Freezing rain",
        71 => "Light snow",
        73 => "Snow",
        75 => "Heavy snow",
        77 => "Snow grains",
        80 => "Light showers",
        81 => "Showers",
        82 => "Violent showers",
        85 => "Snow showers",
        86 => "Heavy snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm with hail",
        _ => "Unknown",
    }
}
