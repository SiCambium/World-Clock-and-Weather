//! Map tile loading: a small download queue with an on-disk cache, retries
//! with backoff, and a manual refresh.
//!
//! Follows the OpenStreetMap tile usage policy: a single official host, an
//! identifying User-Agent, a local cache kept for at least seven days, a low
//! number of parallel downloads, and no bulk prefetching.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use iced::widget::image;
use serde::{Deserialize, Serialize};

/// Map styles the user can pick from. All are free to use without an
/// account or API key, within each provider's fair-use terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Provider {
    #[default]
    OpenStreetMap,
    CartoLight,
    CartoDark,
    CartoVoyager,
    OpenTopoMap,
}

impl Provider {
    pub const ALL: [Provider; 5] = [
        Provider::OpenStreetMap,
        Provider::CartoVoyager,
        Provider::CartoLight,
        Provider::CartoDark,
        Provider::OpenTopoMap,
    ];

    /// Folder name for the disk cache.
    fn cache_name(self) -> &'static str {
        match self {
            Provider::OpenStreetMap => "osm",
            Provider::CartoLight => "carto-light",
            Provider::CartoDark => "carto-dark",
            Provider::CartoVoyager => "carto-voyager",
            Provider::OpenTopoMap => "opentopomap",
        }
    }

    fn url(self) -> &'static str {
        // CARTO serves double-resolution tiles, which look sharp on Retina.
        match self {
            Provider::OpenStreetMap => "https://tile.openstreetmap.org/{z}/{x}/{y}.png",
            Provider::CartoLight => "https://basemaps.cartocdn.com/light_all/{z}/{x}/{y}@2x.png",
            Provider::CartoDark => "https://basemaps.cartocdn.com/dark_all/{z}/{x}/{y}@2x.png",
            Provider::CartoVoyager => "https://basemaps.cartocdn.com/rastertiles/voyager/{z}/{x}/{y}@2x.png",
            Provider::OpenTopoMap => "https://tile.opentopomap.org/{z}/{x}/{y}.png",
        }
    }

    pub fn attribution(self) -> &'static str {
        match self {
            Provider::OpenStreetMap => "© OpenStreetMap contributors",
            Provider::CartoLight | Provider::CartoDark | Provider::CartoVoyager => {
                "© OpenStreetMap contributors © CARTO"
            }
            Provider::OpenTopoMap => "© OpenStreetMap contributors, SRTM · © OpenTopoMap (CC-BY-SA)",
        }
    }

    pub fn max_zoom(self) -> u8 {
        match self {
            Provider::OpenStreetMap => 19,
            Provider::OpenTopoMap => 17,
            _ => 20,
        }
    }

    /// Whether the tiles are already dark, so dark mode needn't dim them.
    pub fn is_dark(self) -> bool {
        self == Provider::CartoDark
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Provider::OpenStreetMap => "OpenStreetMap (standard)",
            Provider::CartoVoyager => "CARTO Voyager (clean, colourful)",
            Provider::CartoLight => "CARTO Positron (light)",
            Provider::CartoDark => "CARTO Dark Matter (dark)",
            Provider::OpenTopoMap => "OpenTopoMap (terrain)",
        })
    }
}

const MAX_IN_FLIGHT: usize = 4;
const MAX_ATTEMPTS: u8 = 4;
const CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MEMORY_LIMIT: usize = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

impl TileKey {
    pub fn parent(self) -> Option<TileKey> {
        (self.z > 0).then(|| TileKey {
            z: self.z - 1,
            x: self.x / 2,
            y: self.y / 2,
        })
    }
}

#[derive(Debug, Clone)]
enum TileState {
    Queued,
    Loading,
    Ready(image::Handle),
    Failed { attempts: u8, retry_at: Instant },
}

/// Result of a download, delivered back to the app as a message.
#[derive(Debug, Clone)]
pub struct Fetched {
    pub provider: Provider,
    pub key: TileKey,
    pub generation: u64,
    pub result: Result<Vec<u8>, String>,
}

#[derive(Default)]
pub struct Tiles {
    provider: Provider,
    states: HashMap<TileKey, TileState>,
    queue: VecDeque<TileKey>,
    in_flight: usize,
    /// Bumped by `refresh`, so stale downloads from before it are ignored.
    generation: u64,
    /// Tiles currently on screen, for eviction and for status.
    wanted: HashSet<TileKey>,
}

impl Tiles {
    pub fn new(provider: Provider) -> Self {
        Self {
            provider,
            ..Self::default()
        }
    }

    pub fn provider(&self) -> Provider {
        self.provider
    }

    /// Switches map style, dropping every tile of the old one.
    pub fn set_provider(&mut self, provider: Provider) -> Vec<(Provider, TileKey, u64)> {
        self.provider = provider;
        self.states.clear();
        self.refresh()
    }

    pub fn handle(&self, key: TileKey) -> Option<&image::Handle> {
        match self.states.get(&key) {
            Some(TileState::Ready(h)) => Some(h),
            _ => None,
        }
    }

    /// How many on-screen tiles have given up or are waiting to retry.
    pub fn failed_count(&self) -> usize {
        self.wanted
            .iter()
            .filter(|k| matches!(self.states.get(k), Some(TileState::Failed { .. })))
            .count()
    }

    pub fn loading_count(&self) -> usize {
        self.wanted
            .iter()
            .filter(|k| {
                matches!(
                    self.states.get(k),
                    Some(TileState::Queued | TileState::Loading) | None
                )
            })
            .count()
    }

    /// Declares which tiles are visible. Returns the downloads to start.
    pub fn want(&mut self, keys: Vec<TileKey>) -> Vec<(Provider, TileKey, u64)> {
        self.wanted = keys.iter().copied().collect();

        // Drop queued tiles that scrolled out of view.
        let wanted = &self.wanted;
        self.queue.retain(|k| wanted.contains(k));
        self.states
            .retain(|k, s| !matches!(s, TileState::Queued) || wanted.contains(k));

        for key in keys {
            if !self.states.contains_key(&key) {
                self.states.insert(key, TileState::Queued);
                self.queue.push_back(key);
            }
        }
        self.evict();
        self.pump()
    }

    /// Records a finished download. Returns further downloads to start.
    pub fn finish(&mut self, fetched: Fetched) -> Vec<(Provider, TileKey, u64)> {
        if fetched.generation != self.generation || fetched.provider != self.provider {
            return self.pump();
        }
        self.in_flight = self.in_flight.saturating_sub(1);
        let attempts = match self.states.get(&fetched.key) {
            Some(TileState::Failed { attempts, .. }) => *attempts,
            _ => 0,
        };
        let state = match fetched.result {
            Ok(bytes) => TileState::Ready(image::Handle::from_bytes(bytes)),
            Err(_) => {
                let attempts = attempts + 1;
                // 2s, 4s, 8s... then wait for the user to press refresh.
                let delay = if attempts >= MAX_ATTEMPTS {
                    Duration::from_secs(60 * 60 * 24)
                } else {
                    Duration::from_secs(1 << attempts)
                };
                TileState::Failed {
                    attempts,
                    retry_at: Instant::now() + delay,
                }
            }
        };
        self.states.insert(fetched.key, state);
        self.pump()
    }

    /// Called periodically: re-queues failed visible tiles whose backoff has
    /// passed.
    pub fn retry_due(&mut self) -> Vec<(Provider, TileKey, u64)> {
        let now = Instant::now();
        let due: Vec<TileKey> = self
            .wanted
            .iter()
            .copied()
            .filter(|k| matches!(self.states.get(k), Some(TileState::Failed { retry_at, .. }) if *retry_at <= now))
            .collect();
        for key in due {
            self.queue.push_back(key);
        }
        self.pump()
    }

    /// Forgets every tile that is not loaded and starts again; tiles already
    /// on screen stay until their replacements arrive.
    pub fn refresh(&mut self) -> Vec<(Provider, TileKey, u64)> {
        self.generation += 1;
        self.in_flight = 0;
        self.queue.clear();
        self.states.retain(|_, s| matches!(s, TileState::Ready(_)));
        let missing: Vec<TileKey> = self
            .wanted
            .iter()
            .copied()
            .filter(|k| !self.states.contains_key(k))
            .collect();
        for key in missing {
            self.states.insert(key, TileState::Queued);
            self.queue.push_back(key);
        }
        self.pump()
    }

    fn pump(&mut self) -> Vec<(Provider, TileKey, u64)> {
        let mut start = Vec::new();
        while self.in_flight < MAX_IN_FLIGHT {
            let Some(key) = self.queue.pop_front() else { break };
            match self.states.get(&key) {
                Some(TileState::Ready(_) | TileState::Loading) => continue,
                Some(TileState::Failed { attempts, .. }) => {
                    // Keep the attempt count; mark loading via a sentinel.
                    let attempts = *attempts;
                    self.states.insert(
                        key,
                        TileState::Failed {
                            attempts,
                            retry_at: Instant::now() + Duration::from_secs(3600),
                        },
                    );
                }
                _ => {
                    self.states.insert(key, TileState::Loading);
                }
            }
            self.in_flight += 1;
            start.push((self.provider, key, self.generation));
        }
        start
    }

    fn evict(&mut self) {
        if self.states.len() <= MEMORY_LIMIT {
            return;
        }
        let wanted = &self.wanted;
        // Keep visible tiles and their ancestors (used as blurry placeholders).
        let mut keep: HashSet<TileKey> = HashSet::new();
        for k in wanted {
            let mut cur = Some(*k);
            while let Some(c) = cur {
                keep.insert(c);
                cur = c.parent();
            }
        }
        self.states
            .retain(|k, s| keep.contains(k) || matches!(s, TileState::Loading));
    }
}

// --- Downloading -----------------------------------------------------------

fn cache_path(provider: Provider, key: TileKey) -> Option<PathBuf> {
    let base = dirs::cache_dir()?;
    Some(
        base.join("TimeAtlas")
            .join("tiles")
            .join(provider.cache_name())
            .join(key.z.to_string())
            .join(key.x.to_string())
            .join(format!("{}.png", key.y)),
    )
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G'])
}

/// Loads one tile: fresh disk cache first, then the network, then a stale
/// cached copy if the network fails.
pub async fn fetch(provider: Provider, key: TileKey, generation: u64) -> Fetched {
    let path = cache_path(provider, key);
    let mut stale: Option<Vec<u8>> = None;

    if let Some(path) = &path {
        if let Ok(bytes) = tokio::fs::read(path).await {
            let fresh = tokio::fs::metadata(path)
                .await
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .is_some_and(|age| age < CACHE_TTL);
            if is_png(&bytes) {
                if fresh {
                    return Fetched {
                        provider,
                        key,
                        generation,
                        result: Ok(bytes),
                    };
                }
                stale = Some(bytes);
            }
        }
    }

    let url = provider
        .url()
        .replace("{z}", &key.z.to_string())
        .replace("{x}", &key.x.to_string())
        .replace("{y}", &key.y.to_string());

    let network = async {
        let resp = crate::api::client()
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        let bytes = resp.bytes().await.map_err(|e| e.to_string())?.to_vec();
        if !is_png(&bytes) {
            return Err("not an image".to_string());
        }
        Ok(bytes)
    }
    .await;

    let result = match network {
        Ok(bytes) => {
            if let Some(path) = &path {
                if let Some(dir) = path.parent() {
                    let _ = tokio::fs::create_dir_all(dir).await;
                }
                let tmp = path.with_extension("tmp");
                if tokio::fs::write(&tmp, &bytes).await.is_ok() {
                    let _ = tokio::fs::rename(&tmp, path).await;
                }
            }
            Ok(bytes)
        }
        Err(e) => stale.ok_or(e),
    };

    Fetched {
        provider,
        key,
        generation,
        result,
    }
}
