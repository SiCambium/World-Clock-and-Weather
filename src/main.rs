//! TimeAtlas: world clocks, weather and a map in one native window.

mod api;
mod graphics;
mod map;
mod model;
mod store;
mod style;
mod secrets;
mod sun;
mod tiles;
mod tz;
mod units;

use std::cell::Cell;
use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, Timelike, Utc};
use iced::keyboard;
use iced::theme::Mode;
use iced::widget::{
    Space, button, canvas, center, column, container, mouse_area, opaque, pick_list, responsive, row, rule, span, toggler,
    scrollable, sensor, stack, text, text_input,
};
use iced::{
    Alignment, Element, Fill, Length, Point, Size, Subscription, Task, Theme, alignment, window,
};

use graphics::Glyph;
use model::{DetailedForecast, GeocodeResult, MAX_PLACES, MIN_PLACES, Place, Weather};
use store::ClockStyle;
use style::{INTER, INTER_LIGHT, INTER_MEDIUM, INTER_SEMIBOLD};
use units::{HourFormat, TempUnit, WindUnit};

const WEATHER_REFRESH: Duration = Duration::from_secs(10 * 60);
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(350);
/// The map never takes more than this share of the space below the header.
const MAP_MAX_SHARE: f32 = 1.0 / 3.0;
/// Below this height the map is hidden so every clock stays visible.
const MAP_MIN_HEIGHT: f32 = 130.0;
const SECTION_GAP: f32 = 16.0;
const CARD_MIN_WIDTH: f32 = 300.0;
const CARD_GAP: f32 = 14.0;
const CARD_PADDING: f32 = 16.0;
/// Height of a card's name and region lines.
const CARD_HEADER: f32 = 42.0;
/// Space a card uses around its clock and weather: padding, header and gap.
const CARD_CHROME: f32 = CARD_PADDING * 2.0 + CARD_HEADER + 12.0;
/// Window size at which everything is drawn at 100%. Larger windows scale
/// text and layout up proportionally; smaller ones scale down a little
/// before switching to the stacked layout.
// The layout is designed at this size; larger or smaller windows scale it.
// Tall enough for two rows of cards in any clock style plus the map.
const BASE_SIZE: Size = Size::new(1240.0, 940.0);
const MIN_SCALE: f32 = 0.85;
const MAX_SCALE: f32 = 1.8;

pub fn main() -> iced::Result {
    let mut app = iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .scale_factor(|app: &App| app.scale)
        .default_font(INTER)
        .antialiasing(true)
        .window(window::Settings {
            size: BASE_SIZE,
            min_size: Some(Size::new(560.0, 520.0)),
            resizable: true,
            platform_specific: platform_window(),
            ..window::Settings::default()
        });
    for font in style::FONTS {
        app = app.font(*font);
    }
    app.run()
}

#[cfg(target_os = "macos")]
fn platform_window() -> window::settings::PlatformSpecific {
    window::settings::PlatformSpecific {
        title_hidden: true,
        titlebar_transparent: true,
        fullsize_content_view: true,
    }
}

#[cfg(not(target_os = "macos"))]
fn platform_window() -> window::settings::PlatformSpecific {
    window::settings::PlatformSpecific::default()
}

/// Space at the top of the window for the macOS traffic-light buttons,
/// since the content runs under a transparent title bar.
#[cfg(target_os = "macos")]
const TITLEBAR_INSET: f32 = 22.0;
#[cfg(not(target_os = "macos"))]
const TITLEBAR_INSET: f32 = 0.0;

// --- State -----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum WeatherStatus {
    Idle,
    Loading,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tab {
    Today,
    Week,
}

#[derive(Debug, Clone, PartialEq)]
enum Note {
    Info(String),
    Error(String),
}

#[derive(Default)]
struct Settings {
    query: String,
    seq: u64,
    searching: bool,
    results: Vec<GeocodeResult>,
    note: Option<Note>,
    /// The place being renamed and the name typed so far.
    renaming: Option<(String, String)>,
    /// A new API key being typed for the current map style.
    api_key: String,
    /// Result of the last save, shown under the key field.
    key_note: Option<Note>,
}

struct Detail {
    place_id: String,
    tab: Tab,
    forecast: Option<Result<DetailedForecast, String>>,
}

enum Overlay {
    None,
    Settings,
    Detail(Detail),
}

struct App {
    places: Vec<Place>,
    weather: HashMap<String, Weather>,
    weather_at: Option<DateTime<Utc>>,
    weather_status: WeatherStatus,
    weather_failures: u32,
    now: DateTime<Utc>,
    dark: bool,
    map: map::View,
    map_sized: bool,
    tiles: tiles::Tiles,
    settings: Settings,
    overlay: Overlay,
    notice: Option<String>,
    prefs: store::Prefs,
    /// What is on screen now; starts from the saved defaults.
    show_map: bool,
    show_weather: bool,
    /// True once the user pans or zooms, so resizing stops re-framing.
    map_moved: bool,
    /// Interface scale, following the window size.
    scale: f32,
    /// Card picked with the arrow keys (index in display order).
    selected: Option<usize>,
    /// Columns in the card grid as last laid out, for arrow-key moves.
    grid_cols: Cell<usize>,
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    WindowResized(Size),
    ThemeChanged(Mode),
    Escape,
    Key(Shortcut),
    DismissNotice,

    RefreshWeather,
    WeatherLoaded(Result<Vec<(String, Weather)>, String>),

    MapResized(Size),
    Map(map::Event),
    MapZoom(f64),
    MapFit,
    MapRefresh,
    TileRetryTick,
    TileFetched(tiles::Fetched),
    MapProvider(tiles::Provider),
    ApiKeyChanged(String),
    SaveApiKey,
    RemoveApiKey,
    ShowMap(bool),
    ShowWeather(bool),
    DefaultShowMap(bool),
    ClockStyle(store::ClockStyle),
    DefaultShowWeather(bool),
    TempUnit(TempUnit),
    WindUnit(WindUnit),
    HourFormat(HourFormat),
    Order(store::Order),
    ShowRelative(bool),
    NightOnMap(bool),

    OpenSettings,
    CloseOverlay,
    SearchChanged(String),
    SearchSubmit,
    SearchDebounced(u64),
    SearchResults(u64, Result<Vec<GeocodeResult>, String>),
    AddPlace(usize),
    RemovePlace(String),
    MovePlace(String, i32),
    StartRename(String),
    RenameChanged(String),
    RenameSubmit,
    RenameCancel,

    OpenDetail(String),
    DetailTab(Tab),
    DetailLoaded(String, Result<DetailedForecast, String>),
}

/// Keyboard shortcuts. Only keys no widget used (such as a text field)
/// arrive here.
#[derive(Debug, Clone, Copy)]
enum Shortcut {
    Refresh,
    Settings,
    AddPlace,
    Style(ClockStyle),
    ToggleMap,
    ToggleWeather,
    Move(i32, i32),
    Open,
}

fn shortcut(key: &keyboard::Key, modifiers: keyboard::Modifiers) -> Option<Message> {
    use keyboard::key::Named;
    let cmd = modifiers.command();
    let s = match key.as_ref() {
        keyboard::Key::Named(Named::Escape) => return Some(Message::Escape),
        keyboard::Key::Character(c) if cmd => match c {
            "r" => Shortcut::Refresh,
            "," => Shortcut::Settings,
            "n" | "f" => Shortcut::AddPlace,
            "1" => Shortcut::Style(ClockStyle::Digital),
            "2" => Shortcut::Style(ClockStyle::Analog),
            "3" => Shortcut::Style(ClockStyle::Both),
            "m" if modifiers.shift() => Shortcut::ToggleMap,
            "w" if modifiers.shift() => Shortcut::ToggleWeather,
            _ => return None,
        },
        _ if cmd || modifiers.control() || modifiers.alt() => return None,
        keyboard::Key::Named(Named::ArrowLeft) => Shortcut::Move(-1, 0),
        keyboard::Key::Named(Named::ArrowRight) => Shortcut::Move(1, 0),
        keyboard::Key::Named(Named::ArrowUp) => Shortcut::Move(0, -1),
        keyboard::Key::Named(Named::ArrowDown) => Shortcut::Move(0, 1),
        keyboard::Key::Named(Named::Enter | Named::Space) => Shortcut::Open,
        _ => return None,
    };
    Some(Message::Key(s))
}

impl From<map::Event> for Message {
    fn from(e: map::Event) -> Self {
        Message::Map(e)
    }
}

impl App {
    fn boot() -> (Self, Task<Message>) {
        let (places, notice) = store::load();
        let cache = store::load_weather();
        let prefs = store::load_prefs();
        let app = Self {
            places,
            weather: cache.weather,
            weather_at: cache.fetched_at,
            weather_status: WeatherStatus::Idle,
            weather_failures: 0,
            now: Utc::now(),
            dark: true,
            map: map::View {
                max_zoom: prefs.map_provider.max_zoom(),
                ..map::View::default()
            },
            map_sized: false,
            tiles: {
                let mut t = tiles::Tiles::new(prefs.map_provider);
                let key = prefs.map_provider.key_service().and_then(secrets::get);
                let _ = t.set_provider(prefs.map_provider, key);
                t
            },
            settings: Settings::default(),
            overlay: Overlay::None,
            notice,
            show_map: prefs.show_map,
            show_weather: prefs.show_weather,
            prefs,
            map_moved: false,
            scale: 1.0,
            selected: None,
            grid_cols: Cell::new(1),
        };
        let tasks = Task::batch([
            iced::system::theme().map(Message::ThemeChanged),
            Task::done(Message::RefreshWeather),
        ]);
        (app, tasks)
    }

    fn title(&self) -> String {
        "TimeAtlas".into()
    }

    fn theme(&self) -> Theme {
        style::theme(self.dark)
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![
            iced::time::every(Duration::from_millis(250)).map(|_| Message::Tick),
            iced::time::every(Duration::from_secs(2)).map(|_| Message::TileRetryTick),
            iced::system::theme_changes().map(Message::ThemeChanged),
            window::resize_events().map(|(_, size)| Message::WindowResized(size)),
            keyboard::listen().filter_map(|event| match event {
                keyboard::Event::KeyPressed { key, modifiers, .. } => shortcut(&key, modifiers),
                _ => None,
            }),
        ];
        // Regular refresh, or a quicker retry with backoff after a failure.
        let every = if self.weather_failures == 0 {
            WEATHER_REFRESH
        } else {
            Duration::from_secs((15u64 << self.weather_failures.min(5)).min(600))
        };
        subs.push(iced::time::every(every).map(|_| Message::RefreshWeather));
        Subscription::batch(subs)
    }

    // --- Update ------------------------------------------------------------

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => {
                self.now = Utc::now();
                Task::none()
            }
            Message::WindowResized(size) => {
                // Reported sizes are in scaled units, so undo the current
                // scale first to avoid feedback.
                let w = size.width * self.scale;
                let h = size.height * self.scale;
                let fit = (w / BASE_SIZE.width).min(h / BASE_SIZE.height);
                let scale = ((fit.clamp(MIN_SCALE, MAX_SCALE)) * 20.0).round() / 20.0;
                self.scale = scale;
                Task::none()
            }
            Message::ThemeChanged(mode) => {
                self.dark = !matches!(mode, Mode::Light);
                Task::none()
            }
            Message::Escape => {
                if matches!(self.overlay, Overlay::None) {
                    self.selected = None;
                }
                self.overlay = Overlay::None;
                Task::none()
            }
            Message::CloseOverlay => {
                self.overlay = Overlay::None;
                Task::none()
            }
            Message::Key(key) => self.shortcut(key),
            Message::DismissNotice => {
                self.notice = None;
                Task::none()
            }

            Message::RefreshWeather => {
                if !self.show_weather || self.weather_status == WeatherStatus::Loading {
                    return Task::none();
                }
                self.weather_status = WeatherStatus::Loading;
                Task::perform(
                    api::current_weather(self.places.clone()),
                    Message::WeatherLoaded,
                )
            }
            Message::WeatherLoaded(Ok(list)) => {
                for (id, w) in list {
                    self.weather.insert(id, w);
                }
                self.weather.retain(|id, _| self.places.iter().any(|p| &p.id == id));
                self.weather_at = Some(Utc::now());
                self.weather_status = WeatherStatus::Idle;
                self.weather_failures = 0;
                store::save_weather(&store::WeatherCache {
                    fetched_at: self.weather_at,
                    weather: self.weather.clone(),
                });
                Task::none()
            }
            Message::WeatherLoaded(Err(e)) => {
                self.weather_status = WeatherStatus::Failed(e);
                self.weather_failures += 1;
                Task::none()
            }

            Message::MapResized(size) => {
                self.map.size = size;
                if size.width > 10.0 && size.height > 10.0 && (!self.map_sized || !self.map_moved) {
                    self.map_sized = true;
                    self.fit_map();
                }
                self.map.clamp();
                self.load_tiles()
            }
            Message::Map(map::Event::Pan(delta)) => {
                self.map_moved = true;
                self.map.pan(delta);
                self.load_tiles()
            }
            Message::Map(map::Event::Zoom { amount, anchor }) => {
                self.map_moved = true;
                self.map.zoom_at(amount, anchor);
                self.load_tiles()
            }
            Message::MapZoom(amount) => {
                self.map_moved = true;
                self.map.zoom_center(amount);
                self.load_tiles()
            }
            Message::MapFit => {
                self.map_moved = false;
                self.fit_map();
                self.load_tiles()
            }
            Message::MapRefresh => {
                let start = self.tiles.refresh();
                fetch_tiles(start)
            }
            Message::TileRetryTick => fetch_tiles(self.tiles.retry_due()),
            Message::TileFetched(fetched) => fetch_tiles(self.tiles.finish(fetched)),
            Message::ShowMap(on) => {
                self.show_map = on;
                if !on {
                    self.map_sized = false;
                }
                Task::none()
            }
            Message::ShowWeather(on) => {
                self.show_weather = on;
                if on {
                    Task::done(Message::RefreshWeather)
                } else {
                    Task::none()
                }
            }
            Message::ClockStyle(style) => {
                self.prefs.clock_style = style;
                store::save_prefs(&self.prefs);
                Task::none()
            }
            Message::DefaultShowMap(on) => {
                self.prefs.show_map = on;
                store::save_prefs(&self.prefs);
                Task::none()
            }
            Message::DefaultShowWeather(on) => {
                self.prefs.show_weather = on;
                store::save_prefs(&self.prefs);
                Task::none()
            }
            Message::TempUnit(u) => self.set_pref(|p| p.temp_unit = u),
            Message::WindUnit(u) => self.set_pref(|p| p.wind_unit = u),
            Message::HourFormat(f) => self.set_pref(|p| p.hour_format = f),
            Message::Order(o) => self.set_pref(|p| p.order = o),
            Message::ShowRelative(on) => self.set_pref(|p| p.show_relative = on),
            Message::NightOnMap(on) => self.set_pref(|p| p.night_on_map = on),
            Message::MapProvider(provider) => {
                if provider == self.tiles.provider() {
                    return Task::none();
                }
                self.prefs.map_provider = provider;
                store::save_prefs(&self.prefs);
                self.map.max_zoom = provider.max_zoom();
                self.map.clamp();
                // Load the new style straight away, with its saved key.
                let key = provider.key_service().and_then(secrets::get);
                let start = self.tiles.set_provider(provider, key);
                self.settings.api_key.clear();
                self.settings.key_note = None;
                Task::batch([fetch_tiles(start), self.load_tiles()])
            }
            Message::ApiKeyChanged(key) => {
                self.settings.api_key = key;
                self.settings.key_note = None;
                Task::none()
            }
            Message::SaveApiKey => {
                let Some(service) = self.tiles.provider().key_service() else {
                    return Task::none();
                };
                let key = self.settings.api_key.trim().to_string();
                if key.is_empty() {
                    return Task::none();
                }
                match secrets::set(service, &key) {
                    Ok(()) => {
                        self.settings.api_key.clear();
                        self.settings.key_note = Some(Note::Info("Key saved. Loading the map…".into()));
                        let start = self.tiles.set_api_key(Some(key));
                        Task::batch([fetch_tiles(start), self.load_tiles()])
                    }
                    Err(e) => {
                        self.settings.key_note = Some(Note::Error(format!("Couldn't save the key: {e}")));
                        Task::none()
                    }
                }
            }
            Message::RemoveApiKey => {
                let Some(service) = self.tiles.provider().key_service() else {
                    return Task::none();
                };
                secrets::remove(service);
                self.settings.key_note = Some(Note::Info("Key removed.".into()));
                let start = self.tiles.set_api_key(None);
                fetch_tiles(start)
            }

            Message::OpenSettings => {
                self.settings = Settings::default();
                self.overlay = Overlay::Settings;
                iced::widget::operation::focus("search")
            }
            Message::SearchChanged(query) => {
                self.settings.query = query;
                self.settings.seq += 1;
                let q = self.settings.query.trim();
                if q.chars().count() < 2 {
                    self.settings.results.clear();
                    self.settings.searching = false;
                    self.settings.note = None;
                    return Task::none();
                }
                self.settings.searching = true;
                let seq = self.settings.seq;
                Task::perform(tokio::time::sleep(SEARCH_DEBOUNCE), move |_| {
                    Message::SearchDebounced(seq)
                })
            }
            Message::SearchSubmit => {
                if self.settings.query.trim().chars().count() < 1 {
                    return Task::none();
                }
                self.settings.seq += 1;
                self.settings.searching = true;
                self.search(self.settings.seq)
            }
            Message::SearchDebounced(seq) => {
                if seq != self.settings.seq {
                    return Task::none();
                }
                self.search(seq)
            }
            Message::SearchResults(seq, result) => {
                if seq != self.settings.seq {
                    return Task::none(); // superseded by a newer keystroke
                }
                self.settings.searching = false;
                match result {
                    Ok(results) => {
                        self.settings.results = results;
                        self.settings.note = None;
                    }
                    Err(e) => {
                        self.settings.results.clear();
                        self.settings.note = Some(Note::Error(e));
                    }
                }
                Task::none()
            }
            Message::AddPlace(index) => {
                let Some(result) = self.settings.results.get(index).cloned() else {
                    return Task::none();
                };
                match store::add(&self.places, &result) {
                    Ok(places) => {
                        self.places = places;
                        self.settings.note = Some(Note::Info(format!("Added {}.", result.name)));
                        self.fit_map();
                        self.weather_status = WeatherStatus::Idle;
                        Task::batch([self.load_tiles(), Task::done(Message::RefreshWeather)])
                    }
                    Err(e) => {
                        self.settings.note = Some(Note::Error(e));
                        Task::none()
                    }
                }
            }
            Message::MovePlace(id, step) => {
                // Moving a place switches to "My order", starting from the
                // order currently on screen.
                let mut places = self.places.clone();
                if self.prefs.order == store::Order::TimeZone {
                    places = self.sorted_places().into_iter().cloned().collect();
                    self.prefs.order = store::Order::Manual;
                    store::save_prefs(&self.prefs);
                }
                match store::move_place(&places, &id, step) {
                    Ok(p) => self.places = p,
                    Err(e) => self.settings.note = Some(Note::Error(e)),
                }
                Task::none()
            }
            Message::StartRename(id) => {
                let current = self
                    .places
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.display_name().to_string())
                    .unwrap_or_default();
                self.settings.renaming = Some((id, current));
                iced::widget::operation::focus("rename")
            }
            Message::RenameChanged(text) => {
                if let Some((_, t)) = &mut self.settings.renaming {
                    *t = text;
                }
                Task::none()
            }
            Message::RenameSubmit => {
                if let Some((id, label)) = self.settings.renaming.take() {
                    match store::rename(&self.places, &id, &label) {
                        Ok(p) => self.places = p,
                        Err(e) => self.settings.note = Some(Note::Error(e)),
                    }
                }
                Task::none()
            }
            Message::RenameCancel => {
                self.settings.renaming = None;
                Task::none()
            }
            Message::RemovePlace(id) => match store::remove(&self.places, &id) {
                Ok(places) => {
                    self.places = places;
                    self.weather.remove(&id);
                    self.fit_map();
                    self.load_tiles()
                }
                Err(e) => {
                    self.settings.note = Some(Note::Error(e));
                    Task::none()
                }
            },

            Message::OpenDetail(id) => {
                let Some(place) = self.places.iter().find(|p| p.id == id) else {
                    return Task::none();
                };
                let (lat, lon) = (place.latitude, place.longitude);
                self.overlay = Overlay::Detail(Detail {
                    place_id: id.clone(),
                    tab: Tab::Today,
                    forecast: None,
                });
                Task::perform(api::detailed_forecast(lat, lon), move |r| {
                    Message::DetailLoaded(id.clone(), r)
                })
            }
            Message::DetailTab(tab) => {
                if let Overlay::Detail(d) = &mut self.overlay {
                    d.tab = tab;
                }
                Task::none()
            }
            Message::DetailLoaded(id, result) => {
                if let Overlay::Detail(d) = &mut self.overlay {
                    if d.place_id == id {
                        d.forecast = Some(result);
                    }
                }
                Task::none()
            }
        }
    }

    fn set_pref(&mut self, change: impl FnOnce(&mut store::Prefs)) -> Task<Message> {
        change(&mut self.prefs);
        store::save_prefs(&self.prefs);
        Task::none()
    }

    fn shortcut(&mut self, key: Shortcut) -> Task<Message> {
        let no_overlay = matches!(self.overlay, Overlay::None);
        match key {
            Shortcut::Refresh => {
                Task::batch([
                    Task::done(Message::RefreshWeather),
                    Task::done(Message::MapRefresh),
                ])
            }
            Shortcut::Settings | Shortcut::AddPlace => Task::done(Message::OpenSettings),
            Shortcut::Style(style) => Task::done(Message::ClockStyle(style)),
            Shortcut::ToggleMap => Task::done(Message::ShowMap(!self.show_map)),
            Shortcut::ToggleWeather => Task::done(Message::ShowWeather(!self.show_weather)),
            Shortcut::Move(dx, dy) if no_overlay => {
                let n = self.places.len() as i32;
                let cols = self.grid_cols.get().max(1) as i32;
                let next = match self.selected {
                    None => 0,
                    Some(i) => (i as i32 + dx + dy * cols).clamp(0, n - 1),
                };
                self.selected = Some(next as usize);
                Task::none()
            }
            Shortcut::Open if no_overlay => {
                let place = self
                    .selected
                    .and_then(|i| self.sorted_places().get(i).map(|p| p.id.clone()));
                match place {
                    Some(id) if self.show_weather => Task::done(Message::OpenDetail(id)),
                    _ => Task::none(),
                }
            }
            _ => Task::none(),
        }
    }

    fn search(&self, seq: u64) -> Task<Message> {
        Task::perform(api::search_city(self.settings.query.clone()), move |r| {
            Message::SearchResults(seq, r)
        })
    }

    fn fit_map(&mut self) {
        let pts: Vec<(f64, f64)> = self.places.iter().map(|p| (p.longitude, p.latitude)).collect();
        self.map.fit(&pts);
    }

    fn load_tiles(&mut self) -> Task<Message> {
        if !self.map_sized {
            return Task::none();
        }
        let start = self.tiles.want(self.map.visible_tiles());
        fetch_tiles(start)
    }

    /// Places in display order: west to east by current UTC offset, or the
    /// user's own order.
    fn sorted_places(&self) -> Vec<&Place> {
        let mut v: Vec<&Place> = self.places.iter().collect();
        if self.prefs.order == store::Order::TimeZone {
            v.sort_by_key(|p| tz::offset_minutes(&p.timezone, self.now));
        }
        v
    }

    // --- View --------------------------------------------------------------

    fn view(&self) -> Element<'_, Message> {
        let base = container(column![self.header(), self.body()].spacing(16))
            .padding(iced::Padding {
                top: 16.0 + TITLEBAR_INSET,
                right: 20.0,
                bottom: 20.0,
                left: 20.0,
            })
            .width(Fill)
            .height(Fill)
            .style(style::app_background);

        let panel = match &self.overlay {
            Overlay::None => return base.into(),
            Overlay::Settings => self.settings_panel(),
            Overlay::Detail(d) => self.detail_panel(d),
        };

        let scrim = mouse_area(container(Space::new()).width(Fill).height(Fill).style(style::scrim))
            .on_press(Message::CloseOverlay);

        stack![base, opaque(scrim), center(opaque(panel)).padding(24)].into()
    }

    fn header(&self) -> Element<'_, Message> {
        let brand = row![
            container(graphics::glyph(Glyph::Globe, 22.0))
                .padding(8)
                .style(style::badge),
            column![
                text("TimeAtlas").size(20).font(INTER_SEMIBOLD),
                text("Time and weather around the world").size(12.5).style(muted),
            ]
            .spacing(1),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        let status: Element<'_, Message> = if !self.show_weather {
            Space::new().into()
        } else {
            match &self.weather_status {
            WeatherStatus::Loading => text("Updating weather…").size(12.5).style(muted).into(),
            WeatherStatus::Failed(_) => row![
                text("Weather offline").size(12.5).style(danger),
                button(text("Retry").size(12.5))
                    .padding([4, 10])
                    .style(style::secondary_button)
                    .on_press(Message::RefreshWeather),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            WeatherStatus::Idle => match self.weather_at {
                Some(at) => text(updated_label(at, self.now)).size(12.5).style(muted).into(),
                None => Space::new().into(),
            },
            }
        };
        let _ = &self.weather_status; // keep borrowck simple

        let quick = |label: &'static str, on: bool, msg: fn(bool) -> Message| {
            toggler(on).label(label).on_toggle(msg).size(18).text_size(13)
        };
        let toggles = container(
            row![
                quick("Map", self.show_map, Message::ShowMap),
                quick("Weather", self.show_weather, Message::ShowWeather),
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        )
        .padding([7, 12])
        .style(style::inset);

        let settings = button(graphics::glyph(Glyph::Settings, 18.0))
            .padding(9)
            .style(style::icon_button)
            .on_press(Message::OpenSettings);

        let mut col = column![
            row![brand, Space::new().width(Fill), status, toggles, settings]
                .spacing(14)
                .align_y(Alignment::Center)
        ]
        .spacing(10);

        if let Some(notice) = &self.notice {
            col = col.push(
                container(
                    row![
                        text(notice).size(13).width(Fill),
                        button(graphics::glyph(Glyph::Close, 12.0))
                            .padding(6)
                            .style(style::icon_button)
                            .on_press(Message::DismissNotice),
                    ]
                    .align_y(Alignment::Center),
                )
                .padding([8, 12])
                .style(style::inset),
            );
        }
        col.into()
    }

    fn body(&self) -> Element<'_, Message> {
        // Clocks come first, above the map. The cards always stretch to fill
        // the space they are given, so there is never a blank gap: with the
        // map off they fill the whole window; with it on, the map takes up
        // to a third and the cards fill the rest. If the window is too short
        // for both, the map is hidden rather than squeeze a clock off screen.
        responsive(move |size| {
            let n = self.places.len();
            let min_h = card_min_height(self.prefs.clock_style, self.show_weather);
            if !self.show_map {
                return self.cards(size);
            }
            // Height of the shortest grid of smallest cards: as many
            // columns as fit, so as few rows as possible.
            let need = (1..=n.max(1))
                .filter_map(|cols| {
                    let cw = (size.width - CARD_GAP * (cols as f32 - 1.0)) / cols as f32;
                    (cw >= CARD_MIN_WIDTH || cols == 1).then(|| {
                        let rows = n.div_ceil(cols) as f32;
                        rows * (min_h + CARD_GAP) - CARD_GAP
                    })
                })
                .fold(f32::INFINITY, f32::min);
            let left_over = size.height - need - SECTION_GAP;
            let map_height = left_over.min(size.height * MAP_MAX_SHARE);
            if map_height < MAP_MIN_HEIGHT {
                // Say why the map is missing rather than leave a blank gap.
                if left_over < 24.0 {
                    return self.cards(size);
                }
                let hint = 22.0;
                return column![
                    self.cards(Size::new(size.width, size.height - hint)),
                    container(text("Make the window taller to see the map").size(12).style(muted))
                        .center_x(Fill)
                        .height(hint - 4.0),
                ]
                .spacing(4)
                .into();
            }
            let cards_h = size.height - map_height - SECTION_GAP;
            column![
                self.cards(Size::new(size.width, cards_h)),
                container(self.map_panel()).height(map_height),
            ]
            .spacing(SECTION_GAP)
            .into()
        })
        .into()
    }

    /// The clock cards, laid out in the grid that fills `area` best. Each
    /// card stretches to its share and sizes its clock to fit, so no space
    /// is wasted at any window size. Scrolls only when even the smallest
    /// cards can't all fit.
    fn cards(&self, area: Size) -> Element<'_, Message> {
        let style = self.prefs.clock_style;
        let places = self.sorted_places();
        let n = places.len().max(1);
        let min_h = card_min_height(style, self.show_weather);
        let fit = grid_layout(n, area.width, area.height, min_h);
        let (layout, scroll) = match fit {
            Some(g) => (g, false),
            None => {
                // Too many places for the window: keep cards at their
                // smallest height and scroll.
                let cols = (((area.width + CARD_GAP) / (CARD_MIN_WIDTH + CARD_GAP)).floor() as usize).clamp(1, n);
                let width = (area.width - 10.0 - CARD_GAP * (cols as f32 - 1.0)) / cols as f32;
                (Grid { cols, rows: n.div_ceil(cols), width, height: min_h }, true)
            }
        };
        self.grid_cols.set(layout.cols);

        let mut grid = column![].spacing(CARD_GAP);
        let mut index = 0;
        for chunk in places.chunks(layout.cols) {
            // A short last row stretches its cards across the full width.
            let card_w = (area.width - if scroll { 10.0 } else { 0.0 } - CARD_GAP * (chunk.len() as f32 - 1.0))
                / chunk.len() as f32;
            let mut r = row![].spacing(CARD_GAP).height(if scroll {
                Length::Fixed(layout.height)
            } else {
                Length::FillPortion(1)
            });
            for place in chunk {
                let selected = self.selected == Some(index);
                r = r.push(
                    container(self.card(place, Size::new(card_w, layout.height), selected))
                        .width(Length::FillPortion(1))
                        .height(Fill),
                );
                index += 1;
            }
            grid = grid.push(r);
        }

        if scroll {
            scrollable(container(grid).padding(iced::Padding { right: 10.0, ..iced::Padding::ZERO }))
                .style(style::scroll)
                .height(Fill)
                .into()
        } else {
            grid.height(Fill).into()
        }
    }

    fn card<'a>(&'a self, place: &'a Place, size: Size, selected: bool) -> Element<'a, Message> {
        let local = tz::local(&place.timezone, self.now);
        let offset = tz::offset_minutes(&place.timezone, self.now);
        let today = Local::now().date_naive();
        let day_note = match local.date_naive().signed_duration_since(today).num_days() {
            1 => " · Tomorrow",
            -1 => " · Yesterday",
            _ => "",
        };
        let style = self.prefs.clock_style;
        let hours = self.prefs.hour_format;
        let sun_alt = sun::altitude(self.now, place.longitude, place.latitude);
        let phase = Phase::from_altitude(sun_alt);
        let c = style::colors(&self.theme());

        let mut badges = column![
            container(text(tz::offset_label("GMT", offset)).size(11.5).font(INTER_MEDIUM))
                .padding([3, 9])
                .style(style::badge),
        ]
        .spacing(4)
        .align_x(alignment::Horizontal::Right);
        if self.prefs.show_relative {
            let mine = Local::now().offset().local_minus_utc() / 60;
            badges = badges.push(text(relative_label(offset - mine)).size(11.5).style(muted));
        }
        let mut name_row = row![text(place.display_name()).size(17).font(INTER_SEMIBOLD)]
            .spacing(6)
            .align_y(Alignment::Center);
        if phase == Phase::Night {
            name_row = name_row.push(graphics::glyph_colored(Glyph::Moon, 16.0, c.muted));
        }
        let header = row![
            column![name_row, text(place.region()).size(12).style(muted)]
                .spacing(2)
                .width(Fill),
            badges,
        ]
        .align_y(Alignment::Start)
        .height(CARD_HEADER);

        // Sizes that fill the card's inner box.
        let inner = Size::new(size.width - CARD_PADDING * 2.0, size.height - CARD_CHROME);
        let m = card_metrics(style, self.show_weather, inner, hours);

        let date = text(format!(
            "{} {} {}{}",
            weekday(local.weekday()),
            local.day(),
            month(local.month()),
            day_note
        ))
        .size(12.5 * m.text_scale)
        .style(muted);
        let sun_line = self.sun_line(place, local.date_naive(), offset, m.text_scale);

        let mut info = column![].spacing(2);
        if style.digital() {
            let (hm, ampm) = hours.split(&local);
            let secs = m.digits * 0.42;
            // One line of rich text so the small seconds share the big
            // digits' baseline. The fixed width stops the clock shifting as
            // the seconds change.
            let mut spans = vec![
                span::<(), _>(hm).size(m.digits).font(INTER_LIGHT).color(c.text),
                span::<(), _>(format!(":{:02}", local.second()))
                    .size(secs)
                    .font(INTER_LIGHT)
                    .color(c.muted),
            ];
            if !ampm.is_empty() {
                spans.push(span::<(), _>(format!(" {ampm}")).size(secs).font(INTER_MEDIUM).color(c.muted));
            }
            info = info.push(
                container(
                    iced::widget::text::Rich::with_spans(spans)
                        .wrapping(text::Wrapping::None),
                )
                .width(digital_width(m.digits, hours)),
            );
        }
        let face = |size: f32| graphics::clock(local.hour(), local.minute(), local.second(), size);
        let time: Element<'_, Message> = match style {
            ClockStyle::Both => row![info.push(date).push(sun_line), face(m.face)]
                .spacing(12)
                .align_y(Alignment::Center)
                .into(),
            ClockStyle::Digital => info.push(date).push(sun_line).into(),
            ClockStyle::Analog => column![face(m.face), date, sun_line]
                .spacing(4)
                .align_x(Alignment::Center)
                .into(),
        };

        let main: Element<'_, Message> = if !self.show_weather {
            container(time).center(Fill).into()
        } else if style != ClockStyle::Both {
            // One clock face drawn large, with the weather beside it.
            row![
                container(time).center(Fill),
                container(self.side_weather(place, m.weather_scale))
                    .padding([10.0 * m.weather_scale, 12.0 * m.weather_scale])
                    .style(style::inset),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into()
        } else {
            column![
                container(time).center(Fill),
                container(self.weather_block(place, m.weather_scale))
                    .padding([10.0 * m.weather_scale, 12.0 * m.weather_scale])
                    .style(style::inset)
                    .width(Fill),
            ]
            .spacing(12)
            .into()
        };

        button(column![header, main].spacing(12).height(Fill))
            .padding(CARD_PADDING)
            .width(Fill)
            .height(Fill)
            .style(card_button(phase, selected))
            .on_press_maybe(self.show_weather.then(|| Message::OpenDetail(place.id.clone())))
            .into()
    }

    /// "↑ 07:24  ↓ 16:07" for the place's local date.
    fn sun_line<'a>(&self, place: &Place, date: chrono::NaiveDate, offset: i32, ts: f32) -> Element<'a, Message> {
        let hours = self.prefs.hour_format;
        let at = |t: DateTime<Utc>| hours.short(&(t + chrono::Duration::minutes(offset as i64)));
        let content: Element<'a, Message> = match sun::day(date, offset, place.longitude, place.latitude) {
            sun::Day::Normal { rise, set } => row![
                graphics::glyph(Glyph::Sunrise, 17.0 * ts),
                text(at(rise)).size(12.0 * ts).style(muted),
                Space::new().width(8.0 * ts),
                graphics::glyph(Glyph::Sunset, 17.0 * ts),
                text(at(set)).size(12.0 * ts).style(muted),
            ]
            .spacing(4)
            .align_y(Alignment::Center)
            .into(),
            sun::Day::MidnightSun => text("Sun up all day").size(12.0 * ts).style(muted).into(),
            sun::Day::PolarNight => text("No sunrise today").size(12.0 * ts).style(muted).into(),
        };
        content
    }

    /// The full weather panel shown under the clocks.
    fn weather_block<'a>(&'a self, place: &'a Place, k: f32) -> Element<'a, Message> {
        let t = self.prefs.temp_unit;
        match self.weather.get(&place.id) {
            Some(w) => {
                let main = row![
                    graphics::weather_icon(w.code, w.is_day, 42.0 * k),
                    column![
                        text(t.fmt(w.temperature)).size(24.0 * k).font(INTER_SEMIBOLD),
                        text(model::condition_label(w.code)).size(12.5 * k).style(muted),
                    ]
                    .spacing(0)
                    .width(Fill),
                    column![
                        text(format!("H {}  L {}", t.fmt(w.high), t.fmt(w.low)))
                            .size(12.5 * k)
                            .font(INTER_MEDIUM),
                        text(format!("Feels {}", t.fmt(w.apparent))).size(12.0 * k).style(muted),
                    ]
                    .spacing(2)
                    .align_x(alignment::Horizontal::Right),
                ]
                .spacing(10)
                .align_y(Alignment::Center);
                let details = row![
                    text(format!("Humidity {}%", w.humidity.round())).size(11.5 * k).style(muted),
                    text("·").size(11.5 * k).style(muted),
                    text(format!("Wind {}", self.prefs.wind_unit.fmt(w.wind_speed)))
                        .size(11.5 * k)
                        .style(muted),
                    Space::new().width(Fill),
                    text("Forecast ›").size(11.5 * k).style(accent).font(INTER_MEDIUM),
                ]
                .spacing(6);
                column![main, details].spacing(8).into()
            }
            None => {
                let msg = match &self.weather_status {
                    WeatherStatus::Failed(_) => "Weather unavailable right now",
                    _ => "Loading weather…",
                };
                row![
                    graphics::weather_icon(-1, true, 42.0 * k),
                    text(msg).size(13.0 * k).style(muted),
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .into()
            }
        }
    }

    /// A narrow weather panel that sits beside a single large clock.
    fn side_weather<'a>(&'a self, place: &'a Place, k: f32) -> Element<'a, Message> {
        let t = self.prefs.temp_unit;
        let width = SIDE_WEATHER_WIDTH * k;
        match self.weather.get(&place.id) {
            Some(w) => column![
                row![
                    graphics::weather_icon(w.code, w.is_day, 34.0 * k),
                    text(t.fmt(w.temperature)).size(24.0 * k).font(INTER_SEMIBOLD),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
                text(model::condition_label(w.code)).size(12.0 * k).style(muted),
                text(format!("H {}  L {}", t.fmt(w.high), t.fmt(w.low)))
                    .size(12.0 * k)
                    .font(INTER_MEDIUM),
                text(format!("Feels {}", t.fmt(w.apparent))).size(11.5 * k).style(muted),
                text("Forecast ›").size(11.5 * k).style(accent).font(INTER_MEDIUM),
            ]
            .spacing(3.0 * k)
            .width(width)
            .into(),
            None => {
                let msg = match &self.weather_status {
                    WeatherStatus::Failed(_) => "Weather unavailable",
                    _ => "Loading weather…",
                };
                column![
                    graphics::weather_icon(-1, true, 34.0 * k),
                    text(msg).size(12.0 * k).style(muted),
                ]
                .spacing(6)
                .width(width)
                .into()
            }
        }
    }

    fn map_panel(&self) -> Element<'_, Message> {
        let markers = self
            .places
            .iter()
            .map(|p| {
                let local = tz::local(&p.timezone, self.now);
                let mut subtitle = self.prefs.hour_format.short(&local);
                if let Some(w) = self.weather.get(&p.id).filter(|_| self.show_weather) {
                    subtitle.push_str(&format!(" · {}", self.prefs.temp_unit.fmt(w.temperature)));
                }
                map::Marker {
                    lon: p.longitude,
                    lat: p.latitude,
                    title: p.display_name().to_string(),
                    subtitle,
                }
            })
            .collect();

        let tile_layer = canvas(map::TileLayer {
            view: &self.map,
            tiles: &self.tiles,
        })
        .width(Fill)
        .height(Fill);

        let marker_layer = sensor(
            canvas(map::MapCanvas {
                view: &self.map,
                markers,
                dim: !self.tiles.provider().is_dark(),
                sun: self.prefs.night_on_map.then(|| sun::subsolar(self.now)),
            })
            .width(Fill)
            .height(Fill),
        )
        .on_show(Message::MapResized)
        .on_resize(Message::MapResized);

        let control = |g: Glyph, msg: Message, tip: &'static str| {
            iced::widget::tooltip(
                button(graphics::glyph(g, 15.0))
                    .padding(8)
                    .style(style::icon_button)
                    .on_press(msg),
                container(text(tip).size(12)).padding([4, 8]).style(style::inset),
                iced::widget::tooltip::Position::Bottom,
            )
        };

        let controls = container(
            // A single row, so the controls fit even when the map is a
            // short strip under the clocks.
            row![
                control(Glyph::Plus, Message::MapZoom(1.0), "Zoom in"),
                control(Glyph::Minus, Message::MapZoom(-1.0), "Zoom out"),
                rule::vertical(1),
                control(Glyph::Fit, Message::MapFit, "Show all places"),
                control(Glyph::Refresh, Message::MapRefresh, "Reload map"),
            ]
            .spacing(2)
            .height(31),
        )
        .padding(3)
        .style(style::map_control_group);

        let failed = self.tiles.failed_count();
        let key_problem = match self.tiles.provider().key_service() {
            Some(service) if self.tiles.needs_key() => {
                Some(format!("{} needs an API key", service.name()))
            }
            Some(service) if self.tiles.key_rejected() => {
                Some(format!("{} didn't accept the API key", service.name()))
            }
            _ => None,
        };
        let status: Element<'_, Message> = if let Some(problem) = key_problem {
            container(
                row![
                    text(problem).size(12),
                    button(text("Add key").size(12))
                        .padding([3, 10])
                        .style(style::primary_button)
                        .on_press(Message::OpenSettings),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([6, 10])
            .style(style::map_control_group)
            .into()
        } else if failed > 0 {
            container(
                row![
                    text("Some map tiles didn't load").size(12),
                    button(text("Retry").size(12))
                        .padding([3, 10])
                        .style(style::primary_button)
                        .on_press(Message::MapRefresh),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([6, 10])
            .style(style::map_control_group)
            .into()
        } else if self.tiles.loading_count() > 0 {
            container(text("Loading map…").size(12))
                .padding([5, 10])
                .style(style::map_control_group)
                .into()
        } else {
            Space::new().into()
        };

        let attribution = container(text(self.tiles.provider().attribution()).size(10.5))
            .padding([2, 6])
            .style(style::attribution);

        let overlay = column![
            row![Space::new().width(Fill), controls],
            Space::new().height(Fill),
            row![status, Space::new().width(Fill), attribution].align_y(Alignment::End),
        ]
        .padding(10);

        container(stack![
            container(Space::new()).width(Fill).height(Fill),
            tile_layer,
            marker_layer,
            overlay
        ])
            .width(Fill)
            .height(Fill)
            .clip(true)
            .style(style::map_frame)
            .into()
    }

    fn panel_header<'a>(&self, title: String, subtitle: String) -> Element<'a, Message> {
        row![
            column![text(title).size(20).font(INTER_SEMIBOLD)]
                .push((!subtitle.is_empty()).then(|| text(subtitle).size(12.5).style(muted)))
                .spacing(2)
                .width(Fill),
            button(graphics::glyph(Glyph::Close, 14.0))
                .padding(8)
                .style(style::icon_button)
                .on_press(Message::CloseOverlay),
        ]
        .align_y(Alignment::Start)
        .into()
    }

    fn settings_panel(&self) -> Element<'_, Message> {
        let s = &self.settings;
        let can_remove = self.places.len() > MIN_PLACES;
        let at_max = self.places.len() >= MAX_PLACES;

        let shown = self.sorted_places();
        let last = shown.len().saturating_sub(1);
        let current = column(shown.into_iter().enumerate().map(|(i, p)| {
            if let Some((id, label)) = &s.renaming {
                if *id == p.id {
                    return container(
                        row![
                            text_input(&p.name, label)
                                .id("rename")
                                .on_input(Message::RenameChanged)
                                .on_submit(Message::RenameSubmit)
                                .padding([7, 10])
                                .size(13.5)
                                .style(style::search_input),
                            button(text("Save").size(12.5).font(INTER_MEDIUM))
                                .padding([5, 12])
                                .style(style::primary_button)
                                .on_press(Message::RenameSubmit),
                            button(text("Cancel").size(12.5))
                                .padding([5, 10])
                                .style(style::secondary_button)
                                .on_press(Message::RenameCancel),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center),
                    )
                    .padding([8, 12])
                    .style(style::inset)
                    .into();
                }
            }
            let icon = |g: Glyph, msg: Option<Message>, tip: &'static str| {
                iced::widget::tooltip(
                    button(graphics::glyph(g, 14.0))
                        .padding(6)
                        .style(style::icon_button)
                        .on_press_maybe(msg),
                    container(text(tip).size(12)).padding([4, 8]).style(style::inset),
                    iced::widget::tooltip::Position::Top,
                )
            };
            let mut remove = button(text("Remove").size(12.5).font(INTER_MEDIUM))
                .padding([5, 10])
                .style(style::danger_button);
            if can_remove {
                remove = remove.on_press(Message::RemovePlace(p.id.clone()));
            }
            let actions = row![
                icon(Glyph::Up, (i > 0).then(|| Message::MovePlace(p.id.clone(), -1)), "Move up"),
                icon(Glyph::Down, (i < last).then(|| Message::MovePlace(p.id.clone(), 1)), "Move down"),
                icon(Glyph::Edit, Some(Message::StartRename(p.id.clone())), "Rename"),
                remove,
            ]
            .spacing(2)
            .align_y(Alignment::Center);
            place_row(p.display_name(), p.region(), actions.into())
        }))
        .spacing(6);

        let input = text_input("Search for a city…", &s.query)
            .id("search")
            .on_input(Message::SearchChanged)
            .on_submit(Message::SearchSubmit)
            .padding([10, 12])
            .size(14)
            .style(style::search_input);

        let note: Element<'_, Message> = if s.searching {
            text("Searching…").size(12.5).style(muted).into()
        } else {
            match &s.note {
                Some(Note::Info(m)) => text(m).size(12.5).style(accent).into(),
                Some(Note::Error(m)) => text(m).size(12.5).style(danger).into(),
                None => Space::new().into(),
            }
        };

        let results = column(s.results.iter().enumerate().map(|(i, r)| {
            let already = self
                .places
                .iter()
                .any(|p| p.id == model::place_id(r.latitude, r.longitude));
            let label = if already { "Added" } else { "Add" };
            let mut add = button(text(label).size(12.5).font(INTER_MEDIUM))
                .padding([5, 12])
                .style(style::primary_button);
            if !at_max && !already {
                add = add.on_press(Message::AddPlace(i));
            }
            place_row(&r.name, r.region(), add.into())
        }))
        .spacing(6);

        let switch = |label: &'static str, on: bool, msg: fn(bool) -> Message| {
            toggler(on)
                .label(label)
                .on_toggle(msg)
                .size(20)
                .text_size(13.5)
                .width(Fill)
        };
        let display = column![
            column![
                text("When TimeAtlas opens").size(14).font(INTER_SEMIBOLD),
                text("You can also switch these any time from the top bar.")
                    .size(12.5)
                    .style(muted),
            ]
            .spacing(2),
            switch("Show the map", self.prefs.show_map, Message::DefaultShowMap),
            switch("Show weather", self.prefs.show_weather, Message::DefaultShowWeather),
        ]
        .spacing(10);

        let p = &self.prefs;
        let clock_style = column![
            text("Clock style").size(14).font(INTER_SEMIBOLD),
            segmented(&ClockStyle::ALL, p.clock_style, ClockStyle::label, Message::ClockStyle),
        ]
        .spacing(8);

        let labelled = |label: &'static str, control: Element<'static, Message>| {
            row![text(label).size(13.5).width(110), control]
                .spacing(10)
                .align_y(Alignment::Center)
        };
        let formats = column![
            text("Units and time").size(14).font(INTER_SEMIBOLD),
            labelled("Temperature", segmented(&TempUnit::ALL, p.temp_unit, TempUnit::label, Message::TempUnit)),
            labelled("Wind", segmented(&WindUnit::ALL, p.wind_unit, WindUnit::label, Message::WindUnit)),
            labelled("Time", segmented(&HourFormat::ALL, p.hour_format, HourFormat::label, Message::HourFormat)),
            labelled("Order", segmented(&store::Order::ALL, p.order, store::Order::label, Message::Order)),
            switch("Show the time difference from this Mac", p.show_relative, Message::ShowRelative),
            switch("Shade night and twilight on the map", p.night_on_map, Message::NightOnMap),
        ]
        .spacing(10);

        let map_style = column![
            text("Map style").size(14).font(INTER_SEMIBOLD),
            pick_list(
                tiles::Provider::ALL,
                Some(self.tiles.provider()),
                Message::MapProvider,
            )
            .width(Fill)
            .padding([9, 12])
            .text_size(13.5),
        ]
        .spacing(8);
        let map_style = match self.tiles.provider().key_service() {
            None => map_style,
            Some(service) => {
                let saved = !self.tiles.needs_key();
                let status = if self.tiles.key_rejected() {
                    text(format!("{} turned this key down. Check it and save it again.", service.name()))
                        .size(12.5)
                        .style(danger)
                } else if saved {
                    text("A key is saved in your Keychain.").size(12.5).style(muted)
                } else {
                    text(format!(
                        "This style needs a free API key from {}. Get one at {}",
                        service.name(),
                        service.signup_url()
                    ))
                    .size(12.5)
                    .style(muted)
                };
                let field = text_input(
                    if saved { "Paste a new key to replace it" } else { "Paste your API key" },
                    &self.settings.api_key,
                )
                .id("api-key")
                .secure(true)
                .on_input(Message::ApiKeyChanged)
                .on_submit(Message::SaveApiKey)
                .padding([9, 12])
                .size(13.5)
                .style(style::search_input);
                let mut actions = row![
                    field,
                    button(text("Save").size(13))
                        .padding([8, 14])
                        .style(style::primary_button)
                        .on_press_maybe((!self.settings.api_key.trim().is_empty()).then_some(Message::SaveApiKey)),
                ]
                .spacing(8)
                .align_y(Alignment::Center);
                if saved {
                    actions = actions.push(
                        button(text("Remove").size(13))
                            .padding([8, 14])
                            .style(style::secondary_button)
                            .on_press(Message::RemoveApiKey),
                    );
                }
                let note: Element<'_, Message> = match &self.settings.key_note {
                    Some(Note::Info(m)) => text(m).size(12.5).style(accent).into(),
                    Some(Note::Error(m)) => text(m).size(12.5).style(danger).into(),
                    None => Space::new().into(),
                };
                map_style
                    .push(text(format!("{} API key", service.name())).size(13).font(INTER_MEDIUM))
                    .push(actions)
                    .push(status)
                    .push(note)
            }
        };

        let content = column![
            self.panel_header("Settings".into(), String::new()),
            clock_style,
            rule::horizontal(1),
            formats,
            rule::horizontal(1),
            display,
            rule::horizontal(1),
            map_style,
            rule::horizontal(1),
            column![
                text("Your places").size(14).font(INTER_SEMIBOLD),
                text(format!("Up to {MAX_PLACES} places. Use the arrows to set your own order."))
                    .size(12.5)
                    .style(muted),
            ]
            .spacing(2),
            current,
            rule::horizontal(1),
            text("Add a place").size(14).font(INTER_SEMIBOLD),
            row![
                container(graphics::glyph(Glyph::Search, 16.0)).padding([0, 2]),
                input
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            note,
            results,
            rule::horizontal(1),
            self.about(),
        ]
        .spacing(14);

        container(scrollable(content.padding(iced::Padding { right: 10.0, ..iced::Padding::ZERO })).style(style::scroll))
            .padding(22)
            .max_width(520)
            .max_height(760)
            .style(style::panel)
            .into()
    }

    /// Version, data sources and keyboard shortcuts.
    fn about(&self) -> Element<'_, Message> {
        let shortcuts = [
            ("⌘R", "Refresh weather and map"),
            ("⌘,  or  ⌘N", "Settings and add a place"),
            ("⌘1  ⌘2  ⌘3", "Digital, analogue or both"),
            ("⇧⌘M  ⇧⌘W", "Show or hide the map or weather"),
            ("Arrow keys", "Pick a place; Return opens its forecast"),
            ("Esc", "Close a panel"),
        ];
        column![
            text("About TimeAtlas").size(14).font(INTER_SEMIBOLD),
            text(format!("Version {}", env!("CARGO_PKG_VERSION"))).size(12.5).style(muted),
            text(format!(
                "Weather and place search by Open-Meteo.com (CC BY 4.0). Map tiles {}. \
                 Sunrise, sunset and the day and night shading are worked out on your Mac.",
                self.tiles.provider().attribution()
            ))
            .size(12)
            .style(muted),
            text("To report a mistake in the map, visit openstreetmap.org/fixthemap.")
                .size(12)
                .style(muted),
            text("Keyboard shortcuts").size(13).font(INTER_MEDIUM),
            column(shortcuts.into_iter().map(|(keys, what)| {
                row![
                    text(keys).size(12).font(INTER_MEDIUM).width(110),
                    text(what).size(12).style(muted),
                ]
                .spacing(10)
                .into()
            }))
            .spacing(4),
        ]
        .spacing(8)
        .into()
    }

    /// "Sunrise 07:24 · Sunset 16:07 · 8 h 43 m of daylight, 2 min shorter
    /// than yesterday" for the place's local today.
    fn daylight_summary(&self, place: &Place) -> String {
        let offset = tz::offset_minutes(&place.timezone, self.now);
        let today = tz::local(&place.timezone, self.now).date_naive();
        let day = |d| sun::day(d, offset, place.longitude, place.latitude);
        let (now, before) = (day(today), today.pred_opt().map(day));
        let hours = self.prefs.hour_format;
        let at = |t: DateTime<Utc>| hours.short(&(t + chrono::Duration::minutes(offset as i64)));
        match now {
            sun::Day::Normal { rise, set } => {
                let len = now.length_minutes();
                let mut s = format!(
                    "Sunrise {} · Sunset {} · {} h {} m of daylight",
                    at(rise),
                    at(set),
                    len / 60,
                    len % 60
                );
                if let Some(b) = before {
                    let diff = len - b.length_minutes();
                    match diff {
                        0 => s.push_str(", about the same as yesterday"),
                        d if d > 0 => s.push_str(&format!(", {d} min longer than yesterday")),
                        d => s.push_str(&format!(", {} min shorter than yesterday", -d)),
                    }
                }
                s
            }
            sun::Day::MidnightSun => "The sun stays up all day".into(),
            sun::Day::PolarNight => "The sun doesn't rise today".into(),
        }
    }

    fn detail_panel<'a>(&'a self, d: &'a Detail) -> Element<'a, Message> {
        let place = self.places.iter().find(|p| p.id == d.place_id);
        let (title, subtitle) = place
            .map(|p| (p.display_name().to_string(), p.region()))
            .unwrap_or_default();

        let tab = |label: &'static str, t: Tab| {
            button(text(label).size(13).font(INTER_MEDIUM))
                .padding([6, 16])
                .style(style::tab_button(d.tab == t))
                .on_press(Message::DetailTab(t))
        };
        let tabs = container(row![tab("Next 24 hours", Tab::Today), tab("7 days", Tab::Week)].spacing(4))
            .padding(4)
            .style(style::inset);

        let body: Element<'_, Message> = match &d.forecast {
            None => center(text("Loading forecast…").style(muted)).height(160).into(),
            Some(Err(e)) => center(text(e).style(danger)).height(160).into(),
            Some(Ok(f)) => match d.tab {
                Tab::Today => hourly_view(f, self.prefs.temp_unit, self.prefs.hour_format),
                Tab::Week => daily_view(f, self.prefs.temp_unit),
            },
        };
        let daylight = place.map(|p| {
            row![
                graphics::glyph(Glyph::Sunrise, 16.0),
                text(self.daylight_summary(p)).size(12.5).style(muted),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
        });

        container(column![self.panel_header(title, subtitle)].push(daylight).push(tabs).push(body).spacing(18))
            .padding(22)
            .max_width(620)
            .style(style::panel)
            .into()
    }
}

fn fetch_tiles(start: Vec<tiles::Job>) -> Task<Message> {
    Task::batch(start.into_iter().map(|(provider, key, generation, api_key)| {
        Task::perform(tiles::fetch(provider, key, generation, api_key), Message::TileFetched)
    }))
}

fn place_row<'a>(name: &str, region: String, action: Element<'a, Message>) -> Element<'a, Message> {
    container(
        row![
            column![
                text(name.to_string()).size(14).font(INTER_MEDIUM),
                text(region).size(12).style(muted),
            ]
            .spacing(1)
            .width(Fill),
            action,
        ]
        .align_y(Alignment::Center),
    )
    .padding([8, 12])
    .style(style::inset)
    .into()
}

fn hourly_view(f: &DetailedForecast, t: TempUnit, hours: HourFormat) -> Element<'_, Message> {
    if f.hourly.is_empty() {
        return text("No hourly data available.").style(muted).into();
    }
    let cards = row(f.hourly.iter().map(|h| {
        container(
            column![
                text(hour_label(&h.label, hours)).size(12).style(muted).font(INTER_MEDIUM),
                graphics::weather_icon(h.code, is_daytime(&h.label), 30.0),
                text(t.fmt(h.temperature)).size(15).font(INTER_SEMIBOLD),
                text(format!("{}%", h.precip_prob.round())).size(11.5).style(rain),
            ]
            .spacing(6)
            .align_x(alignment::Horizontal::Center),
        )
        .padding([10, 6])
        .width(62)
        .style(style::inset)
        .into()
    }))
    .spacing(8);

    scrollable(container(cards).padding(iced::Padding { bottom: 12.0, ..iced::Padding::ZERO }))
        .direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::default()))
        .style(style::scroll)
        .into()
}

fn daily_view(f: &DetailedForecast, t: TempUnit) -> Element<'_, Message> {
    if f.daily.is_empty() {
        return text("No forecast data available.").style(muted).into();
    }
    let lo = f.daily.iter().map(|d| d.low).fold(f64::INFINITY, f64::min);
    let hi = f.daily.iter().map(|d| d.high).fold(f64::NEG_INFINITY, f64::max);

    column(f.daily.iter().enumerate().map(|(i, d)| {
        let name = if i == 0 {
            "Today".to_string()
        } else {
            chrono::NaiveDate::parse_from_str(&d.date, "%Y-%m-%d")
                .map(|date| weekday(date.weekday()).to_string())
                .unwrap_or_else(|_| d.date.clone())
        };
        container(
            row![
                text(name).size(13.5).font(INTER_MEDIUM).width(56),
                graphics::weather_icon(d.code, true, 28.0),
                text(model::condition_label(d.code)).size(12.5).style(muted).width(Fill),
                text(format!("{}%", d.precip_prob.round())).size(12).style(rain).width(40),
                text(t.fmt(d.low)).size(13).style(muted).width(36).align_x(alignment::Horizontal::Right),
                range_bar(d.low, d.high, lo, hi),
                text(t.fmt(d.high)).size(13).font(INTER_SEMIBOLD).width(36),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([8, 12])
        .style(style::inset)
        .into()
    }))
    .spacing(6)
    .into()
}

/// A small bar showing a day's low to high against the week's range.
fn range_bar<'a>(low: f64, high: f64, min: f64, max: f64) -> Element<'a, Message> {
    struct Bar {
        start: f32,
        end: f32,
    }
    impl canvas::Program<Message> for Bar {
        type State = ();
        fn draw(
            &self,
            _: &(),
            renderer: &iced::Renderer,
            theme: &Theme,
            bounds: iced::Rectangle,
            _: iced::mouse::Cursor,
        ) -> Vec<canvas::Geometry> {
            let c = style::colors(theme);
            let mut frame = canvas::Frame::new(renderer, bounds.size());
            let h = 5.0;
            let y = (bounds.height - h) / 2.0;
            frame.fill(
                &canvas::Path::rounded_rectangle(Point::new(0.0, y), Size::new(bounds.width, h), (h / 2.0).into()),
                c.border,
            );
            let x0 = self.start * bounds.width;
            let w = ((self.end - self.start) * bounds.width).max(h);
            frame.fill(
                &canvas::Path::rounded_rectangle(Point::new(x0, y), Size::new(w, h), (h / 2.0).into()),
                canvas::Gradient::Linear(
                    canvas::gradient::Linear::new(Point::new(x0, 0.0), Point::new(x0 + w, 0.0))
                        .add_stop(0.0, c.rain)
                        .add_stop(1.0, c.sun),
                ),
            );
            vec![frame.into_geometry()]
        }
    }
    let span = (max - min).max(1.0);
    canvas(Bar {
        start: ((low - min) / span) as f32,
        end: ((high - min) / span) as f32,
    })
    .width(110)
    .height(14)
    .into()
}

// --- Card layout -----------------------------------------------------------

/// Width of the weather panel beside a single clock, before scaling.
const SIDE_WEATHER_WIDTH: f32 = 118.0;
/// Space the side weather panel takes with its padding and gap.
const SIDE_WEATHER_ROOM: f32 = SIDE_WEATHER_WIDTH + 24.0 + 12.0;
const SIDE_WEATHER_HEIGHT: f32 = 136.0;
/// Height of the weather panel under the clocks, with padding and gap.
const BELOW_WEATHER_HEIGHT: f32 = 84.0 + 12.0;
/// Date and sunrise lines under a digital clock.
const TIME_LINES: f32 = 40.0;
const MIN_DIGITS: f32 = 28.0;
const MIN_FACE: f32 = 64.0;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Grid {
    cols: usize,
    rows: usize,
    width: f32,
    height: f32,
}

/// Picks the number of columns whose cards best fill `width` × `height`
/// for `n` places, or `None` if even the smallest cards don't fit. Cards
/// that are closest to a pleasant shape (and largest) win.
fn grid_layout(n: usize, width: f32, height: f32, min_h: f32) -> Option<Grid> {
    let n = n.max(1);
    let mut best: Option<(f32, Grid)> = None;
    for cols in 1..=n {
        let rows = n.div_ceil(cols);
        let w = (width - CARD_GAP * (cols as f32 - 1.0)) / cols as f32;
        let h = (height - CARD_GAP * (rows as f32 - 1.0)) / rows as f32;
        if (w < CARD_MIN_WIDTH && cols > 1) || h < min_h {
            continue;
        }
        // Score: the size of the largest 1.6:1 box that fits in the card,
        // with a small penalty for a short last row.
        let empty = (rows * cols - n) as f32;
        let score = (w / 1.6).min(h) * (1.0 - 0.04 * empty);
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, Grid { cols, rows, width: w, height: h }));
        }
    }
    best.map(|(_, g)| g)
}

/// The smallest card height that still shows everything.
fn card_min_height(style: ClockStyle, weather: bool) -> f32 {
    let digital = 1.25 * MIN_DIGITS + TIME_LINES;
    let inner = match style {
        ClockStyle::Digital => digital,
        ClockStyle::Analog => MIN_FACE + TIME_LINES + 8.0,
        ClockStyle::Both => digital.max(MIN_FACE),
    };
    let inner = match (weather, style) {
        (false, _) => inner,
        (true, ClockStyle::Both) => inner + BELOW_WEATHER_HEIGHT,
        (true, _) => inner.max(SIDE_WEATHER_HEIGHT),
    };
    inner + CARD_CHROME
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Metrics {
    /// Font size of the digital clock's hours and minutes.
    digits: f32,
    /// Diameter of the analogue clock.
    face: f32,
    /// How much the weather panel is enlarged in a big card.
    weather_scale: f32,
    /// How much the date and sunrise lines are enlarged beside a big clock.
    text_scale: f32,
}

/// Estimated width of the digital clock text, so it can sit in a box that
/// doesn't change size every second.
fn digital_width(digits: f32, hours: HourFormat) -> f32 {
    let secs = digits * 0.42;
    let main = 2.55 * digits + 0.62 * secs * 3.0;
    let extra = if hours == HourFormat::H12 { 0.62 * secs * 3.0 } else { 0.0 };
    (main + extra) * 1.06 + 4.0
}

/// Clock and weather sizes that fill a card's inner box.
fn card_metrics(style: ClockStyle, weather: bool, inner: Size, hours: HourFormat) -> Metrics {
    let (iw, ih) = (inner.width.max(1.0), inner.height.max(1.0));
    let k = if weather {
        match style {
            // The weather never takes more than about half the card.
            ClockStyle::Both => (iw / 420.0).min(0.45 * ih / BELOW_WEATHER_HEIGHT).clamp(1.0, 1.45),
            _ => (iw / 420.0).min(0.8 * ih / SIDE_WEATHER_HEIGHT).clamp(1.0, 2.0),
        }
    } else {
        1.0
    };
    let per_digit = digital_width(100.0, hours) / 100.0;
    let room_w = if weather && style != ClockStyle::Both { iw - SIDE_WEATHER_ROOM * k } else { iw };
    // Digits for a given height, after the date and sunrise lines, which
    // grow a little with the clock.
    let digits_for = |h: f32, w: f32, max: f32| {
        let first = ((h - TIME_LINES) / 1.25).min(w / per_digit);
        let ts = (first / 56.0).clamp(1.0, 1.5);
        let d = ((h - TIME_LINES * ts) / 1.25).min(w / per_digit).clamp(MIN_DIGITS * 0.8, max);
        (d, (d / 56.0).clamp(1.0, 1.5))
    };
    match style {
        ClockStyle::Digital => {
            let (digits, text_scale) = digits_for(ih, room_w, 190.0);
            Metrics { digits, face: 0.0, weather_scale: k, text_scale }
        }
        ClockStyle::Analog => {
            let first = (ih - TIME_LINES - 8.0).min(room_w);
            let ts = (first / 170.0).clamp(1.0, 1.4);
            let face = (ih - TIME_LINES * ts - 8.0).min(room_w).clamp(MIN_FACE * 0.8, 420.0);
            Metrics { digits: 0.0, face, weather_scale: k, text_scale: ts }
        }
        ClockStyle::Both => {
            let h = if weather { ih - BELOW_WEATHER_HEIGHT * k } else { ih };
            let face = h.min(iw * 0.42).clamp(MIN_FACE * 0.8, 340.0);
            let (digits, text_scale) = digits_for(h, iw - face - 12.0, 150.0);
            Metrics { digits, face, weather_scale: k, text_scale }
        }
    }
}

/// Time of day at a place, from the sun's height, used to tint its card.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Day,
    Twilight,
    Night,
}

impl Phase {
    fn from_altitude(alt: f64) -> Self {
        if alt > sun::HORIZON {
            Phase::Day
        } else if alt > sun::CIVIL * 2.0 {
            Phase::Twilight
        } else {
            Phase::Night
        }
    }
}

/// "+5 h", "−8 h", "+5 h 30" or "Same time as you".
fn relative_label(diff: i32) -> String {
    if diff == 0 {
        return "Same time as you".into();
    }
    let sign = if diff < 0 { '−' } else { '+' };
    let (h, m) = (diff.abs() / 60, diff.abs() % 60);
    if m == 0 { format!("{sign}{h} h from you") } else { format!("{sign}{h} h {m:02} from you") }
}

// --- Small helpers ---------------------------------------------------------

fn card_button(phase: Phase, selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let c = style::colors(theme);
        let base = style::card(theme);
        // Night cards take a deep blue tint and twilight a softer one, so a
        // row of cities reads as a sweep from day to night.
        let night = if c.dark { iced::Color::from_rgb8(0x0b, 0x10, 0x24) } else { iced::Color::from_rgb8(0xe4, 0xe8, 0xf6) };
        let background = match phase {
            Phase::Day => c.surface,
            Phase::Twilight => style::mix(c.surface, night, 0.45),
            Phase::Night => style::mix(c.surface, night, 0.9),
        };
        let border_color = match status {
            _ if selected => c.accent,
            button::Status::Hovered | button::Status::Pressed => iced::Color { a: 0.6, ..c.accent },
            _ => c.border,
        };
        button::Style {
            background: Some(background.into()),
            text_color: c.text,
            border: iced::Border {
                color: border_color,
                width: if selected { 2.0 } else { 1.0 },
                ..base.border
            },
            shadow: base.shadow,
            snap: true,
        }
    }
}

fn muted(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(style::colors(theme).muted),
    }
}

fn accent(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(style::colors(theme).accent),
    }
}

fn danger(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(style::colors(theme).danger),
    }
}

fn rain(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(style::colors(theme).rain),
    }
}

/// Reformats an "HH:MM" forecast label for 12-hour time ("Now" stays).
fn hour_label(label: &str, hours: HourFormat) -> String {
    match chrono::NaiveTime::parse_from_str(label, "%H:%M") {
        Ok(t) if hours == HourFormat::H12 => {
            let (pm, h) = t.hour12();
            format!("{h} {}", if pm { "PM" } else { "AM" })
        }
        _ => label.to_string(),
    }
}

/// A row of buttons choosing one of a few options.
fn segmented<T: Copy + PartialEq + 'static>(
    options: &[T],
    current: T,
    label: fn(T) -> &'static str,
    msg: fn(T) -> Message,
) -> Element<'static, Message> {
    container(
        row(options.iter().map(|&o| {
            button(
                text(label(o))
                    .size(13)
                    .font(INTER_MEDIUM)
                    .width(Fill)
                    .align_x(alignment::Horizontal::Center),
            )
            .padding([6, 8])
            .width(Fill)
            .style(style::tab_button(current == o))
            .on_press(msg(o))
            .into()
        }))
        .spacing(4),
    )
    .padding(4)
    .width(Fill)
    .style(style::inset)
    .into()
}

fn is_daytime(label: &str) -> bool {
    label
        .get(0..2)
        .and_then(|h| h.parse::<u32>().ok())
        .map(|h| (6..19).contains(&h))
        .unwrap_or(true)
}

fn updated_label(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let mins = (now - at).num_minutes();
    match mins {
        i64::MIN..=0 => "Updated just now".into(),
        1 => "Updated 1 min ago".into(),
        2..=59 => format!("Updated {mins} min ago"),
        _ => format!("Updated {} h ago", mins / 60),
    }
}

fn weekday(d: chrono::Weekday) -> &'static str {
    match d {
        chrono::Weekday::Mon => "Mon",
        chrono::Weekday::Tue => "Tue",
        chrono::Weekday::Wed => "Wed",
        chrono::Weekday::Thu => "Thu",
        chrono::Weekday::Fri => "Fri",
        chrono::Weekday::Sat => "Sat",
        chrono::Weekday::Sun => "Sun",
    }
}

fn month(m: u32) -> &'static str {
    [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .get(m.saturating_sub(1) as usize)
    .copied()
    .unwrap_or("")
}
