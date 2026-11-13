//! Persists the saved places and the last good weather.
//!
//! Files live in the app's support folder
//! (`~/Library/Application Support/TimeAtlas/`; inside the App Sandbox macOS
//! redirects this to the app's container automatically). Writes are atomic
//! (temp file then rename) so a crash can never leave a half-written file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::{GeocodeResult, MAX_PLACES, MIN_PLACES, Place, Weather};

#[derive(Serialize, Deserialize)]
struct ConfigFile {
    places: Vec<Place>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct WeatherCache {
    pub fetched_at: Option<DateTime<Utc>>,
    pub weather: HashMap<String, Weather>,
}

fn support_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TimeAtlas")
}

fn config_path() -> PathBuf {
    support_dir().join("config.json")
}

fn weather_path() -> PathBuf {
    support_dir().join("weather-cache.json")
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

pub fn default_places() -> Vec<Place> {
    vec![
        Place {
            id: "51.5074_-0.1278".into(),
            name: "London".into(),
            admin1: "England".into(),
            country: "United Kingdom".into(),
            timezone: "Europe/London".into(),
            latitude: 51.5074,
            longitude: -0.1278,
        },
        Place {
            id: "40.7128_-74.0060".into(),
            name: "New York".into(),
            admin1: "New York".into(),
            country: "United States".into(),
            timezone: "America/New_York".into(),
            latitude: 40.7128,
            longitude: -74.0060,
        },
    ]
}

/// Loads saved places. If the file exists but cannot be read, it is kept as
/// `config.json.bak` and a warning is returned alongside the defaults, so the
/// user's list is never silently overwritten.
pub fn load() -> (Vec<Place>, Option<String>) {
    let path = config_path();
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => {
            let places = default_places();
            let _ = save(&places);
            return (places, None);
        }
    };
    match serde_json::from_slice::<ConfigFile>(&bytes) {
        Ok(c) if !c.places.is_empty() => (c.places, None),
        _ => {
            let backup = path.with_extension("json.bak");
            let _ = std::fs::copy(&path, &backup);
            (
                default_places(),
                Some(format!(
                    "Your saved places could not be read, so the defaults are shown. The old file was kept at {}.",
                    backup.display()
                )),
            )
        }
    }
}

pub fn save(places: &[Place]) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(&ConfigFile {
        places: places.to_vec(),
    })
    .map_err(|e| e.to_string())?;
    write_atomic(&config_path(), &json).map_err(|e| format!("Could not save places: {e}"))
}

/// Adds a place, enforcing the limits. Returns the new list.
pub fn add(places: &[Place], result: &GeocodeResult) -> Result<Vec<Place>, String> {
    if places.len() >= MAX_PLACES {
        return Err(format!("You can track at most {MAX_PLACES} places."));
    }
    let new = Place::from_result(result);
    if places.iter().any(|p| p.id == new.id) {
        return Err(format!("{} is already in your list.", result.name));
    }
    let mut updated = places.to_vec();
    updated.push(new);
    save(&updated)?;
    Ok(updated)
}

/// Removes a place by id, enforcing the minimum. Returns the new list.
pub fn remove(places: &[Place], id: &str) -> Result<Vec<Place>, String> {
    if places.len() <= MIN_PLACES {
        return Err(format!("You must keep at least {MIN_PLACES} places."));
    }
    let updated: Vec<Place> = places.iter().filter(|p| p.id != id).cloned().collect();
    if updated.len() == places.len() {
        return Err("Place not found.".into());
    }
    save(&updated)?;
    Ok(updated)
}

pub fn load_weather() -> WeatherCache {
    std::fs::read(weather_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_weather(cache: &WeatherCache) {
    if let Ok(json) = serde_json::to_vec(cache) {
        let _ = write_atomic(&weather_path(), &json);
    }
}

/// App preferences other than the places list.
#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct Prefs {
    #[serde(default)]
    pub map_provider: crate::tiles::Provider,
    #[serde(default = "yes")]
    pub show_map: bool,
    #[serde(default = "yes")]
    pub show_weather: bool,
    #[serde(default)]
    pub clock_style: ClockStyle,
}

/// How each place's time is shown.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClockStyle {
    Digital,
    Analog,
    #[default]
    Both,
}

impl ClockStyle {
    pub const ALL: [ClockStyle; 3] = [ClockStyle::Digital, ClockStyle::Analog, ClockStyle::Both];

    pub fn label(self) -> &'static str {
        match self {
            ClockStyle::Digital => "Digital",
            ClockStyle::Analog => "Analogue",
            ClockStyle::Both => "Both",
        }
    }

    pub fn digital(self) -> bool {
        self != ClockStyle::Analog
    }

    pub fn analog(self) -> bool {
        self != ClockStyle::Digital
    }
}

fn yes() -> bool {
    true
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            map_provider: Default::default(),
            show_map: true,
            show_weather: true,
            clock_style: ClockStyle::Both,
        }
    }
}

fn prefs_path() -> PathBuf {
    support_dir().join("preferences.json")
}

pub fn load_prefs() -> Prefs {
    std::fs::read(prefs_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_prefs(prefs: &Prefs) {
    if let Ok(json) = serde_json::to_vec_pretty(prefs) {
        let _ = write_atomic(&prefs_path(), &json);
    }
}
