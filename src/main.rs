//! TimeAtlas: world clocks, weather and a map in one native window.

mod api;
mod graphics;
mod map;
mod model;
mod store;
mod style;
mod tiles;
mod tz;

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, Timelike, Utc};
use iced::keyboard;
use iced::theme::Mode;
use iced::widget::{
    Space, button, canvas, center, column, container, mouse_area, opaque, pick_list, responsive, row, rule, toggler,
    scrollable, sensor, stack, text, text_input,
};
use iced::{
    Alignment, Element, Fill, Length, Point, Size, Subscription, Task, Theme, alignment, window,
};

use graphics::Glyph;
use model::{DetailedForecast, GeocodeResult, MAX_PLACES, MIN_PLACES, Place, Weather};
use style::{INTER, INTER_LIGHT, INTER_MEDIUM, INTER_SEMIBOLD};

const WEATHER_REFRESH: Duration = Duration::from_secs(10 * 60);
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(350);
const WIDE_LAYOUT: f32 = 1060.0;
const CARD_MIN_WIDTH: f32 = 300.0;
/// Window size at which everything is drawn at 100%. Larger windows scale
/// text and layout up proportionally; smaller ones scale down a little
/// before switching to the stacked layout.
const BASE_SIZE: Size = Size::new(1240.0, 820.0);
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
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    WindowResized(Size),
    ThemeChanged(Mode),
    Escape,
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
    ShowMap(bool),
    ShowWeather(bool),
    DefaultShowMap(bool),
    DefaultShowWeather(bool),

    OpenSettings,
    CloseOverlay,
    SearchChanged(String),
    SearchSubmit,
    SearchDebounced(u64),
    SearchResults(u64, Result<Vec<GeocodeResult>, String>),
    AddPlace(usize),
    RemovePlace(String),

    OpenDetail(String),
    DetailTab(Tab),
    DetailLoaded(String, Result<DetailedForecast, String>),
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
            tiles: tiles::Tiles::new(prefs.map_provider),
            settings: Settings::default(),
            overlay: Overlay::None,
            notice,
            show_map: prefs.show_map,
            show_weather: prefs.show_weather,
            prefs,
            map_moved: false,
            scale: 1.0,
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
            iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick),
            iced::time::every(Duration::from_secs(2)).map(|_| Message::TileRetryTick),
            iced::system::theme_changes().map(Message::ThemeChanged),
            window::resize_events().map(|(_, size)| Message::WindowResized(size)),
            keyboard::listen().filter_map(|event| match event {
                keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                } => Some(Message::Escape),
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
            Message::Escape | Message::CloseOverlay => {
                self.overlay = Overlay::None;
                Task::none()
            }
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
            Message::MapProvider(provider) => {
                if provider == self.tiles.provider() {
                    return Task::none();
                }
                self.prefs.map_provider = provider;
                store::save_prefs(&self.prefs);
                self.map.max_zoom = provider.max_zoom();
                self.map.clamp();
                let _ = self.tiles.set_provider(provider);
                self.load_tiles()
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

    /// Places ordered west to east by current UTC offset.
    fn sorted_places(&self) -> Vec<&Place> {
        let mut v: Vec<&Place> = self.places.iter().collect();
        v.sort_by_key(|p| tz::offset_minutes(&p.timezone, self.now));
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
        responsive(move |size| {
            if !self.show_map {
                self.cards(size.width - 8.0)
            } else if size.width >= WIDE_LAYOUT {
                let cards_w = (size.width * 0.42).clamp(340.0, 680.0);
                row![
                    container(self.cards(cards_w - 8.0)).width(cards_w).height(Fill),
                    self.map_panel(),
                ]
                .spacing(18)
                .into()
            } else {
                column![
                    container(self.cards(size.width - 8.0))
                        .height(Length::FillPortion(5)),
                    container(self.map_panel()).height(Length::FillPortion(4)),
                ]
                .spacing(16)
                .into()
            }
        })
        .into()
    }

    fn cards(&self, width: f32) -> Element<'_, Message> {
        let gap = 14.0;
        let cols = (((width + gap) / (CARD_MIN_WIDTH + gap)).floor() as usize).max(1);
        let places = self.sorted_places();

        let mut grid = column![].spacing(gap);
        for chunk in places.chunks(cols) {
            let mut r = row![].spacing(gap);
            for place in chunk {
                r = r.push(container(self.card(place)).width(Length::FillPortion(1)));
            }
            for _ in chunk.len()..cols {
                r = r.push(Space::new().width(Length::FillPortion(1)));
            }
            grid = grid.push(r);
        }

        scrollable(container(grid).padding(iced::Padding {
            right: 8.0,
            bottom: 4.0,
            ..iced::Padding::ZERO
        }))
        .style(style::scroll)
        .height(Fill)
        .into()
    }

    fn card<'a>(&'a self, place: &'a Place) -> Element<'a, Message> {
        let local = tz::local(&place.timezone, self.now);
        let offset = tz::offset_minutes(&place.timezone, self.now);
        let today = Local::now().date_naive();
        let day_note = match local.date_naive().signed_duration_since(today).num_days() {
            1 => " · Tomorrow",
            -1 => " · Yesterday",
            _ => "",
        };

        let header = row![
            column![
                text(&place.name).size(17).font(INTER_SEMIBOLD),
                text(place.region()).size(12).style(muted),
            ]
            .spacing(2)
            .width(Fill),
            container(text(tz::offset_label("GMT", offset)).size(11.5).font(INTER_MEDIUM))
                .padding([3, 9])
                .style(style::badge),
        ]
        .align_y(Alignment::Start);

        let time = row![
            column![
                row![
                    text(format!("{:02}:{:02}", local.hour(), local.minute()))
                        .size(40)
                        .font(INTER_LIGHT),
                    text(format!(":{:02}", local.second()))
                        .size(16)
                        .style(muted)
                        .font(INTER_MEDIUM),
                ]
                .align_y(Alignment::End)
                .spacing(2),
                text(format!(
                    "{} {} {}{}",
                    weekday(local.weekday()),
                    local.day(),
                    month(local.month()),
                    day_note
                ))
                .size(12.5)
                .style(muted),
            ]
            .spacing(2)
            .width(Fill),
            graphics::clock(local.hour(), local.minute(), local.second(), 60.0),
        ]
        .align_y(Alignment::Center);

        let weather: Element<'_, Message> = match self.weather.get(&place.id) {
            Some(w) => {
                let main = row![
                    graphics::weather_icon(w.code, w.is_day, 42.0),
                    column![
                        text(format!("{}°", w.temperature.round())).size(24).font(INTER_SEMIBOLD),
                        text(model::condition_label(w.code)).size(12.5).style(muted),
                    ]
                    .spacing(0)
                    .width(Fill),
                    column![
                        text(format!("H {}°  L {}°", fmt_temp(w.high), fmt_temp(w.low)))
                            .size(12.5)
                            .font(INTER_MEDIUM),
                        text(format!("Feels {}°", w.apparent.round())).size(12).style(muted),
                    ]
                    .spacing(2)
                    .align_x(alignment::Horizontal::Right),
                ]
                .spacing(10)
                .align_y(Alignment::Center);
                let details = row![
                    text(format!("Humidity {}%", w.humidity.round())).size(11.5).style(muted),
                    text("·").size(11.5).style(muted),
                    text(format!("Wind {} km/h", w.wind_speed.round())).size(11.5).style(muted),
                    Space::new().width(Fill),
                    text("Forecast ›").size(11.5).style(accent).font(INTER_MEDIUM),
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
                    graphics::weather_icon(-1, true, 42.0),
                    text(msg).size(13).style(muted),
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .into()
            }
        };

        let mut body = column![header, time].spacing(12);
        if self.show_weather {
            body = body.push(container(weather).padding([10, 12]).style(style::inset).width(Fill));
        }

        button(body)
            .padding(16)
            .width(Fill)
            .style(card_button)
            .on_press_maybe(
                self.show_weather
                    .then(|| Message::OpenDetail(place.id.clone())),
            )
            .into()
    }

    fn map_panel(&self) -> Element<'_, Message> {
        let markers = self
            .places
            .iter()
            .map(|p| {
                let local = tz::local(&p.timezone, self.now);
                let mut subtitle = format!("{:02}:{:02}", local.hour(), local.minute());
                if let Some(w) = self.weather.get(&p.id).filter(|_| self.show_weather) {
                    subtitle.push_str(&format!(" · {}°", w.temperature.round()));
                }
                map::Marker {
                    lon: p.longitude,
                    lat: p.latitude,
                    title: p.name.clone(),
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
                iced::widget::tooltip::Position::Left,
            )
        };

        let controls = container(
            column![
                control(Glyph::Plus, Message::MapZoom(1.0), "Zoom in"),
                control(Glyph::Minus, Message::MapZoom(-1.0), "Zoom out"),
                rule::horizontal(1),
                control(Glyph::Fit, Message::MapFit, "Show all places"),
                control(Glyph::Refresh, Message::MapRefresh, "Reload map"),
            ]
            .spacing(2)
            .width(31),
        )
        .padding(3)
        .style(style::map_control_group);

        let failed = self.tiles.failed_count();
        let status: Element<'_, Message> = if failed > 0 {
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

        let current = column(self.sorted_places().into_iter().map(|p| {
            let mut remove = button(text("Remove").size(12.5).font(INTER_MEDIUM))
                .padding([5, 10])
                .style(style::danger_button);
            if can_remove {
                remove = remove.on_press(Message::RemovePlace(p.id.clone()));
            }
            place_row(&p.name, p.region(), remove.into())
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

        let content = column![
            self.panel_header("Settings".into(), String::new()),
            display,
            rule::horizontal(1),
            map_style,
            rule::horizontal(1),
            column![
                text("Your places").size(14).font(INTER_SEMIBOLD),
                text(format!("Keep between {MIN_PLACES} and {MAX_PLACES} places."))
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
            text(format!(
                "Weather data by Open-Meteo.com (CC BY 4.0). Map tiles {}.",
                self.tiles.provider().attribution()
            ))
            .size(11.5)
            .style(muted),
        ]
        .spacing(14);

        container(scrollable(content.padding(iced::Padding { right: 10.0, ..iced::Padding::ZERO })).style(style::scroll))
            .padding(22)
            .max_width(480)
            .max_height(680)
            .style(style::panel)
            .into()
    }

    fn detail_panel<'a>(&'a self, d: &'a Detail) -> Element<'a, Message> {
        let place = self.places.iter().find(|p| p.id == d.place_id);
        let (title, subtitle) = place
            .map(|p| (p.name.clone(), p.region()))
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
                Tab::Today => hourly_view(f),
                Tab::Week => daily_view(f),
            },
        };

        container(column![self.panel_header(title, subtitle), tabs, body].spacing(18))
            .padding(22)
            .max_width(620)
            .style(style::panel)
            .into()
    }
}

fn fetch_tiles(start: Vec<(tiles::Provider, tiles::TileKey, u64)>) -> Task<Message> {
    Task::batch(start.into_iter().map(|(provider, key, generation)| {
        Task::perform(tiles::fetch(provider, key, generation), Message::TileFetched)
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

fn hourly_view(f: &DetailedForecast) -> Element<'_, Message> {
    if f.hourly.is_empty() {
        return text("No hourly data available.").style(muted).into();
    }
    let cards = row(f.hourly.iter().map(|h| {
        container(
            column![
                text(&h.label).size(12).style(muted).font(INTER_MEDIUM),
                graphics::weather_icon(h.code, is_daytime(&h.label), 30.0),
                text(format!("{}°", h.temperature.round())).size(15).font(INTER_SEMIBOLD),
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

fn daily_view(f: &DetailedForecast) -> Element<'_, Message> {
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
                text(format!("{}°", d.low.round())).size(13).style(muted).width(32).align_x(alignment::Horizontal::Right),
                range_bar(d.low, d.high, lo, hi),
                text(format!("{}°", d.high.round())).size(13).font(INTER_SEMIBOLD).width(32),
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

// --- Small helpers ---------------------------------------------------------

fn card_button(theme: &Theme, status: button::Status) -> button::Style {
    let c = style::colors(theme);
    let base = style::card(theme);
    let border_color = match status {
        button::Status::Hovered | button::Status::Pressed => iced::Color { a: 0.6, ..c.accent },
        _ => c.border,
    };
    button::Style {
        background: base.background,
        text_color: c.text,
        border: iced::Border {
            color: border_color,
            ..base.border
        },
        shadow: base.shadow,
        snap: true,
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

fn fmt_temp(v: f64) -> String {
    if v.is_finite() { format!("{}", v.round()) } else { "–".into() }
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
