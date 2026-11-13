//! Open-Meteo geocoding and forecast clients (no API key required).

use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;

use crate::model::{DayPoint, DetailedForecast, GeocodeResult, HourPoint, Place, Weather, place_id};

pub const USER_AGENT: &str = concat!("TimeAtlas/", env!("CARGO_PKG_VERSION"), " (macOS desktop app)");

/// Shared HTTP client.
pub fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(8))
            .build()
            .expect("HTTP client")
    })
}

async fn get_json<T: for<'de> Deserialize<'de>>(
    url: &str,
    query: &[(&str, String)],
    service: &str,
) -> Result<T, String> {
    let resp = client()
        .get(url)
        .query(query)
        .send()
        .await
        .map_err(|e| format!("Could not reach the {service} service ({e})."))?;
    if !resp.status().is_success() {
        return Err(format!("The {service} service returned {}.", resp.status()));
    }
    resp.json::<T>()
        .await
        .map_err(|e| format!("Could not read the {service} response ({e})."))
}

// --- Geocoding -------------------------------------------------------------

#[derive(Deserialize)]
struct GeocodeResponse {
    #[serde(default)]
    results: Vec<GeocodeItem>,
}

#[derive(Deserialize)]
struct GeocodeItem {
    name: String,
    #[serde(default)]
    admin1: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    timezone: Option<String>,
    latitude: f64,
    longitude: f64,
}

/// Former or historical English names mapped to the name the geocoder
/// (GeoNames) indexes them under.
const CITY_ALIASES: &[(&str, &str)] = &[
    ("bangalore", "Bengaluru"),
    ("bombay", "Mumbai"),
    ("calcutta", "Kolkata"),
    ("madras", "Chennai"),
    ("cochin", "Kochi"),
    ("trivandrum", "Thiruvananthapuram"),
    ("poona", "Pune"),
    ("mysore", "Mysuru"),
    ("baroda", "Vadodara"),
    ("peking", "Beijing"),
    ("canton", "Guangzhou"),
    ("saigon", "Ho Chi Minh City"),
    ("rangoon", "Yangon"),
    ("constantinople", "Istanbul"),
    ("christiania", "Oslo"),
    ("danzig", "Gdansk"),
];

/// Returns the current name for a former city name, only when the whole
/// query is that former name (so "Mad" still finds Madrid first).
fn alias_for(query: &str) -> Option<&'static str> {
    let q = query.trim().to_lowercase();
    CITY_ALIASES
        .iter()
        .find(|(alias, _)| *alias == q)
        .map(|(_, canonical)| *canonical)
}

async fn geocode(name: &str) -> Result<Vec<GeocodeResult>, String> {
    let parsed: GeocodeResponse = get_json(
        "https://geocoding-api.open-meteo.com/v1/search",
        &[
            ("name", name.to_string()),
            ("count", "8".into()),
            ("language", "en".into()),
            ("format", "json".into()),
        ],
        "geocoding",
    )
    .await?;

    Ok(parsed
        .results
        .into_iter()
        .filter_map(|r| {
            let timezone = r.timezone.filter(|t| !t.is_empty())?;
            Some(GeocodeResult {
                name: r.name,
                admin1: r.admin1.unwrap_or_default(),
                country: r.country.unwrap_or_default(),
                timezone,
                latitude: r.latitude,
                longitude: r.longitude,
            })
        })
        .collect())
}

fn merge(first: Vec<GeocodeResult>, second: Vec<GeocodeResult>) -> Vec<GeocodeResult> {
    let mut seen = std::collections::HashSet::new();
    first
        .into_iter()
        .chain(second)
        .filter(|r| seen.insert(place_id(r.latitude, r.longitude)))
        .collect()
}

/// Looks up candidate cities by free-text name.
pub async fn search_city(query: String) -> Result<Vec<GeocodeResult>, String> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Err("Please enter a city name.".into());
    }
    let mut results = geocode(&query).await?;
    if let Some(canonical) = alias_for(&query) {
        if let Ok(alias_results) = geocode(canonical).await {
            // Normal matches stay first; the renamed city follows.
            results = merge(results, alias_results.into_iter().take(1).collect());
        }
    }
    if results.is_empty() {
        return Err(format!("No places found for \"{query}\"."));
    }
    Ok(results)
}

// --- Current weather -------------------------------------------------------

#[derive(Deserialize)]
struct CurrentResponse {
    current: CurrentBlock,
    daily: CurrentDaily,
}

#[derive(Deserialize)]
struct CurrentBlock {
    temperature_2m: f64,
    relative_humidity_2m: f64,
    apparent_temperature: f64,
    is_day: i32,
    weather_code: i32,
    wind_speed_10m: f64,
}

#[derive(Deserialize)]
struct CurrentDaily {
    #[serde(default)]
    temperature_2m_max: Vec<Option<f64>>,
    #[serde(default)]
    temperature_2m_min: Vec<Option<f64>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany<T> {
    Many(Vec<T>),
    One(T),
}

/// Fetches current weather for every place in one request. The result is in
/// the same order as `places`, keyed by place id.
pub async fn current_weather(places: Vec<Place>) -> Result<Vec<(String, Weather)>, String> {
    if places.is_empty() {
        return Ok(Vec::new());
    }
    let join = |f: fn(&Place) -> f64| {
        places
            .iter()
            .map(|p| format!("{:.4}", f(p)))
            .collect::<Vec<_>>()
            .join(",")
    };

    let parsed: OneOrMany<CurrentResponse> = get_json(
        "https://api.open-meteo.com/v1/forecast",
        &[
            ("latitude", join(|p| p.latitude)),
            ("longitude", join(|p| p.longitude)),
            (
                "current",
                "temperature_2m,relative_humidity_2m,apparent_temperature,is_day,weather_code,wind_speed_10m".into(),
            ),
            ("daily", "temperature_2m_max,temperature_2m_min".into()),
            ("forecast_days", "1".into()),
            ("timezone", "auto".into()),
            ("temperature_unit", "celsius".into()),
            ("wind_speed_unit", "kmh".into()),
        ],
        "weather",
    )
    .await?;

    let list = match parsed {
        OneOrMany::Many(v) => v,
        OneOrMany::One(one) => vec![one],
    };

    Ok(places
        .iter()
        .zip(list)
        .map(|(p, f)| {
            let first = |v: &Vec<Option<f64>>| v.first().copied().flatten().unwrap_or(f64::NAN);
            (
                p.id.clone(),
                Weather {
                    temperature: f.current.temperature_2m,
                    apparent: f.current.apparent_temperature,
                    humidity: f.current.relative_humidity_2m,
                    wind_speed: f.current.wind_speed_10m,
                    code: f.current.weather_code,
                    is_day: f.current.is_day == 1,
                    high: first(&f.daily.temperature_2m_max),
                    low: first(&f.daily.temperature_2m_min),
                },
            )
        })
        .collect())
}

// --- Detailed forecast -----------------------------------------------------

#[derive(Deserialize)]
struct DetailedResponse {
    hourly: DetailedHourly,
    daily: DetailedDaily,
}

#[derive(Deserialize)]
struct DetailedHourly {
    time: Vec<String>,
    #[serde(default)]
    temperature_2m: Vec<Option<f64>>,
    #[serde(default)]
    weather_code: Vec<Option<i32>>,
    #[serde(default)]
    precipitation_probability: Vec<Option<f64>>,
}

#[derive(Deserialize)]
struct DetailedDaily {
    time: Vec<String>,
    #[serde(default)]
    weather_code: Vec<Option<i32>>,
    #[serde(default)]
    temperature_2m_max: Vec<Option<f64>>,
    #[serde(default)]
    temperature_2m_min: Vec<Option<f64>>,
    #[serde(default)]
    precipitation_probability_max: Vec<Option<f64>>,
}

fn at<T: Copy + Default>(v: &[Option<T>], i: usize) -> T {
    v.get(i).copied().flatten().unwrap_or_default()
}

/// Hourly breakdown of today plus a seven day outlook for one location.
pub async fn detailed_forecast(lat: f64, lon: f64) -> Result<DetailedForecast, String> {
    let f: DetailedResponse = get_json(
        "https://api.open-meteo.com/v1/forecast",
        &[
            ("latitude", format!("{lat:.4}")),
            ("longitude", format!("{lon:.4}")),
            (
                "hourly",
                "temperature_2m,weather_code,precipitation_probability".into(),
            ),
            (
                "daily",
                "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max".into(),
            ),
            ("forecast_days", "7".into()),
            // Hourly data starting from the current hour, not midnight.
            ("forecast_hours", "24".into()),
            ("timezone", "auto".into()),
            ("temperature_unit", "celsius".into()),
            ("wind_speed_unit", "kmh".into()),
        ],
        "weather",
    )
    .await?;

    let hourly = f
        .hourly
        .time
        .iter()
        .take(24)
        .enumerate()
        .map(|(i, t)| HourPoint {
            label: if i == 0 {
                "Now".to_string()
            } else {
                t.split('T').nth(1).unwrap_or(t).chars().take(5).collect()
            },
            temperature: at(&f.hourly.temperature_2m, i),
            code: at(&f.hourly.weather_code, i),
            precip_prob: at(&f.hourly.precipitation_probability, i),
        })
        .collect();

    let daily = f
        .daily
        .time
        .iter()
        .enumerate()
        .map(|(i, d)| DayPoint {
            date: d.clone(),
            code: at(&f.daily.weather_code, i),
            high: at(&f.daily.temperature_2m_max, i),
            low: at(&f.daily.temperature_2m_min, i),
            precip_prob: at(&f.daily.precipitation_probability_max, i),
        })
        .collect();

    Ok(DetailedForecast { hourly, daily })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases() {
        assert_eq!(alias_for("Bombay"), Some("Mumbai"));
        assert_eq!(alias_for("bom"), None);
        assert_eq!(alias_for("Mad"), None);
        assert_eq!(alias_for("Salisbury"), None);
        assert_eq!(alias_for("London"), None);
    }

    #[test]
    fn parses_single_and_multi() {
        let one = r#"{"current":{"temperature_2m":1.0,"relative_humidity_2m":2.0,"apparent_temperature":3.0,"is_day":1,"weather_code":3,"wind_speed_10m":4.0},"daily":{"temperature_2m_max":[5.0],"temperature_2m_min":[null]}}"#;
        let many = format!("[{one},{one}]");
        assert!(matches!(serde_json::from_str::<OneOrMany<CurrentResponse>>(one).unwrap(), OneOrMany::One(_)));
        match serde_json::from_str::<OneOrMany<CurrentResponse>>(&many).unwrap() {
            OneOrMany::Many(v) => assert_eq!(v.len(), 2),
            _ => panic!("expected list"),
        }
    }
}
