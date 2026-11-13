//! A native slippy map drawn on a canvas from raster tiles, with pan, zoom
//! and live-time place markers.

use std::f64::consts::PI;

use iced::alignment;
use iced::keyboard;
use iced::mouse::{self, ScrollDelta};
use iced::widget::Action;
use iced::widget::canvas::{self, Frame, Path, Stroke, Text};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, Vector};

use crate::style;
use crate::tiles::{TileKey, Tiles};


const TILE: f64 = 256.0;
const MAX_FIT_ZOOM: f64 = 5.0;

/// Where the map is looking. `center` is in normalised Web Mercator
/// coordinates (0..1 on both axes).
#[derive(Debug, Clone, Copy)]
pub struct View {
    pub center: (f64, f64),
    pub zoom: f64,
    pub size: Size,
    /// Deepest zoom the current tile provider offers.
    pub max_zoom: u8,
    /// Middle of the group of places (normalised x). When the world repeats
    /// across the view, pins are drawn on the copy nearest this, so the
    /// group always stays together and in west-to-east order.
    pub anchor: f64,
}

impl Default for View {
    fn default() -> Self {
        Self {
            center: (0.5, project(0.0, 20.0).1),
            zoom: 2.0,
            size: Size::new(800.0, 400.0),
            max_zoom: 19,
            anchor: 0.5,
        }
    }
}

pub fn project(lon: f64, lat: f64) -> (f64, f64) {
    let lat = lat.clamp(-85.0511, 85.0511).to_radians();
    let x = (lon + 180.0) / 360.0;
    let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / PI) / 2.0;
    (x, y)
}

impl View {
    fn world_px(&self) -> f64 {
        TILE * 2f64.powf(self.zoom)
    }

    fn min_zoom(&self) -> f64 {
        // Never show empty space above or below the world. A wide, short map
        // may show the world more than once across; pins are drawn once, on
        // the copy nearest the centre, which `fit` places on the cities.
        (self.size.height as f64 / TILE).log2().max(0.0)
    }

    pub fn max_zoom(&self) -> f64 {
        self.max_zoom as f64
    }

    pub fn clamp(&mut self) {
        self.zoom = self.zoom.clamp(self.min_zoom(), self.max_zoom());
        let half = self.size.height as f64 / 2.0 / self.world_px();
        let (lo, hi) = (half, 1.0 - half);
        self.center.1 = if lo <= hi { self.center.1.clamp(lo, hi) } else { 0.5 };
        self.center.0 = self.center.0.rem_euclid(1.0);
    }

    /// Screen position (relative to the canvas) of a normalised point,
    /// choosing the horizontal world copy closest to the centre.
    fn to_screen(&self, p: (f64, f64)) -> Point {
        let w = self.world_px();
        // Choose the copy of the point nearest the group's anchor...
        let mut x = p.0;
        x -= (x - self.anchor).round();
        // ...then the copy of that group nearest the view centre.
        let mut dx = x - self.center.0;
        dx -= (self.anchor - self.center.0).round();
        Point::new(
            (self.size.width as f64 / 2.0 + dx * w) as f32,
            (self.size.height as f64 / 2.0 + (p.1 - self.center.1) * w) as f32,
        )
    }

    pub fn pan(&mut self, delta: Vector) {
        let w = self.world_px();
        self.center.0 -= delta.x as f64 / w;
        self.center.1 -= delta.y as f64 / w;
        self.clamp();
    }

    /// Zooms by `amount` levels, keeping the point under `anchor` fixed.
    pub fn zoom_at(&mut self, amount: f64, anchor: Point) {
        let before = self.world_px();
        let ax = anchor.x as f64 - self.size.width as f64 / 2.0;
        let ay = anchor.y as f64 - self.size.height as f64 / 2.0;
        let target = (self.center.0 + ax / before, self.center.1 + ay / before);
        self.zoom = (self.zoom + amount).clamp(self.min_zoom(), self.max_zoom());
        let after = self.world_px();
        self.center = (target.0 - ax / after, target.1 - ay / after);
        self.clamp();
    }

    pub fn zoom_center(&mut self, amount: f64) {
        let c = Point::new(self.size.width / 2.0, self.size.height / 2.0);
        self.zoom_at(amount, c);
    }

    /// Frames all the given points with some padding.
    pub fn fit(&mut self, points: &[(f64, f64)]) {
        if points.is_empty() {
            return;
        }
        let projected: Vec<(f64, f64)> = points.iter().map(|(lon, lat)| project(*lon, *lat)).collect();
        let (mut y0, mut y1) = (1.0f64, 0.0f64);
        for (_, y) in &projected {
            y0 = y0.min(*y);
            y1 = y1.max(*y);
        }
        // Horizontally, treat the world as a ring: put the widest empty gap
        // between places at the edges so the places form one group.
        let mut xs: Vec<f64> = projected.iter().map(|(x, _)| *x).collect();
        xs.sort_by(f64::total_cmp);
        let n = xs.len();
        let (mut gap, mut after) = (xs[0] + 1.0 - xs[n - 1], 0usize);
        for i in 0..n - 1 {
            if xs[i + 1] - xs[i] > gap {
                gap = xs[i + 1] - xs[i];
                after = i + 1;
            }
        }
        let x0 = xs[after];
        let x1 = if after == 0 { xs[n - 1] } else { xs[after - 1] + 1.0 };
        self.anchor = ((x0 + x1) / 2.0).rem_euclid(1.0);
        // Extra room on the right and top for the name labels.
        let w = (self.size.width as f64 - 60.0 - 170.0).max(50.0);
        let h = (self.size.height as f64 - 90.0 - 50.0).max(50.0);
        let span_x = (x1 - x0).max(1e-6);
        let span_y = (y1 - y0).max(1e-6);
        let zoom = (w / (span_x * TILE)).min(h / (span_y * TILE)).log2();
        self.zoom = zoom.min(MAX_FIT_ZOOM).clamp(self.min_zoom(), self.max_zoom());
        let world_px = TILE * 2f64.powf(self.zoom);
        // Shift so the labels' extra space is balanced.
        self.center = (
            ((x0 + x1) / 2.0 + 55.0 / world_px).rem_euclid(1.0),
            (y0 + y1) / 2.0 - 20.0 / world_px,
        );
        self.clamp();
    }

    /// The integer tile zoom to fetch and the scale to draw it at.
    fn tile_level(&self) -> (u8, f64) {
        let z = self.zoom.floor().clamp(0.0, self.max_zoom()) as u8;
        (z, 2f64.powf(self.zoom - z as f64))
    }

    /// Every tile slot on screen as (key, unwrapped column), including
    /// repeated copies of the world when zoomed far out.
    fn tile_slots(&self) -> Vec<(TileKey, i64)> {
        let (z, scale) = self.tile_level();
        let n = 1u32 << z;
        let size = TILE * scale;
        let cx = self.center.0 * n as f64;
        let cy = self.center.1 * n as f64;
        let half_w = self.size.width as f64 / 2.0 / size;
        let half_h = self.size.height as f64 / 2.0 / size;
        let (x0, x1) = ((cx - half_w).floor() as i64, (cx + half_w).floor() as i64);
        let (y0, y1) = (
            ((cy - half_h).floor() as i64).max(0),
            ((cy + half_h).floor() as i64).min(n as i64 - 1),
        );
        let mut slots = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                let wx = x.rem_euclid(n as i64) as u32;
                slots.push((TileKey { z, x: wx, y: y as u32 }, x));
            }
        }
        slots
    }

    /// Tiles covering the view, centre-out so the middle loads first.
    pub fn visible_tiles(&self) -> Vec<TileKey> {
        let (z, _) = self.tile_level();
        let n = 1u32 << z;
        let cx = self.center.0 * n as f64;
        let cy = self.center.1 * n as f64;
        let mut keys = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (key, _) in self.tile_slots() {
            if seen.insert(key) {
                keys.push(key);
            }
        }
        keys.sort_by(|a, b| {
            let d = |k: &TileKey| {
                let mut dx = k.x as f64 + 0.5 - cx;
                dx -= (dx / n as f64).round() * n as f64;
                let dy = k.y as f64 + 0.5 - cy;
                dx * dx + dy * dy
            };
            d(a).total_cmp(&d(b))
        });
        keys
    }
}

/// A pin on the map.
pub struct Marker {
    pub lon: f64,
    pub lat: f64,
    pub title: String,
    pub subtitle: String,
}

#[derive(Debug, Clone)]
pub enum Event {
    Pan(Vector),
    Zoom { amount: f64, anchor: Point },
}

/// The interactive top layer: handles pan and zoom, and draws the markers.
pub struct MapCanvas<'a> {
    pub view: &'a View,
    pub markers: Vec<Marker>,
    /// Dim the tiles slightly (bright tiles in dark mode).
    pub dim: bool,
}

/// The bottom layer: raster tiles only. Kept separate because images are
/// composited above vector shapes within one canvas layer.
pub struct TileLayer<'a> {
    pub view: &'a View,
    pub tiles: &'a Tiles,
}

impl<'a, M> canvas::Program<M> for TileLayer<'a> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let c = style::colors(theme);
        let mut frame = Frame::new(renderer, bounds.size());
        let view = View {
            size: bounds.size(),
            ..*self.view
        };
        let clip = Rectangle::new(Point::ORIGIN, bounds.size());
        frame.with_clip(clip, |frame| {
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), c.surface_raised);

            // Tiles, with a scaled-up ancestor as a placeholder while loading.
            let (z, scale) = view.tile_level();
            let n = 1u32 << z;
            let tile_px = (TILE * scale) as f32;
            let w = view.world_px();
            for (key, column) in view.tile_slots() {
                let dx = column as f64 / n as f64 - view.center.0;
                let x = (view.size.width as f64 / 2.0 + dx * w) as f32;
                let y = (view.size.height as f64 / 2.0 + (key.y as f64 / n as f64 - view.center.1) * w) as f32;
                let rect = Rectangle::new(Point::new(x.floor(), y.floor()), Size::new(tile_px.ceil() + 1.0, tile_px.ceil() + 1.0));

                if let Some(handle) = self.tiles.handle(key) {
                    frame.draw_image(rect, handle);
                    continue;
                }
                // Ancestor placeholder.
                let mut anc = key;
                let mut depth = 0u32;
                while let Some(parent) = anc.parent() {
                    anc = parent;
                    depth += 1;
                    if depth > 4 {
                        break;
                    }
                    if let Some(handle) = self.tiles.handle(anc) {
                        let factor = (1u32 << depth) as f32;
                        let sub_x = (key.x - anc.x * (1 << depth)) as f32;
                        let sub_y = (key.y - anc.y * (1 << depth)) as f32;
                        let big = Rectangle::new(
                            Point::new(rect.x - sub_x * tile_px, rect.y - sub_y * tile_px),
                            Size::new(tile_px * factor, tile_px * factor),
                        );
                        frame.with_clip(rect, |f| {
                            f.draw_image(
                                Rectangle::new(
                                    Point::new(big.x - rect.x, big.y - rect.y),
                                    big.size(),
                                ),
                                handle,
                            );
                        });
                        break;
                    }
                }
            }

        });
        vec![frame.into_geometry()]
    }
}

#[derive(Default)]
pub struct State {
    drag_from: Option<Point>,
    modifiers: keyboard::Modifiers,
    last_click: Option<(std::time::Instant, Point)>,
}

impl<'a, M> canvas::Program<M> for MapCanvas<'a>
where
    M: From<Event>,
{
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<M>> {
        match event {
            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                state.modifiers = *m;
                None
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor.position_in(bounds)?;
                let now = std::time::Instant::now();
                if let Some((t, p)) = state.last_click {
                    if now.duration_since(t).as_millis() < 350 && p.distance(pos) < 6.0 {
                        state.last_click = None;
                        state.drag_from = None;
                        return Some(
                            Action::publish(Event::Zoom { amount: 1.0, anchor: pos }.into()).and_capture(),
                        );
                    }
                }
                state.last_click = Some((now, pos));
                state.drag_from = Some(pos);
                Some(Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.drag_from.take().map(|_| Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let from = state.drag_from?;
                let pos = cursor.position_from(bounds.position())?;
                let delta = pos - from;
                if delta.x == 0.0 && delta.y == 0.0 {
                    return None;
                }
                state.drag_from = Some(pos);
                Some(Action::publish(Event::Pan(delta).into()).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let pos = cursor.position_in(bounds)?;
                let zoom_mod = state.modifiers.command() || state.modifiers.control();
                let ev = match *delta {
                    // Mouse wheel: zoom.
                    ScrollDelta::Lines { y, .. } => Event::Zoom {
                        amount: (y as f64 * 0.5).clamp(-1.0, 1.0),
                        anchor: pos,
                    },
                    // Trackpad with ⌘ or ⌃: zoom smoothly.
                    ScrollDelta::Pixels { y, .. } if zoom_mod => Event::Zoom {
                        amount: (y as f64 / 120.0).clamp(-1.0, 1.0),
                        anchor: pos,
                    },
                    // Trackpad two-finger scroll: pan, like Apple Maps.
                    ScrollDelta::Pixels { x, y } => Event::Pan(Vector::new(x, y)),
                };
                Some(Action::publish(ev.into()).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let c = style::colors(theme);
        let mut frame = Frame::new(renderer, bounds.size());
        let view = View {
            size: bounds.size(),
            ..*self.view
        };

        // Soften bright map tiles in dark mode.
        if c.dark && self.dim {
            frame.fill_rectangle(
                Point::ORIGIN,
                bounds.size(),
                Color::from_rgba(0.04, 0.05, 0.08, 0.28),
            );
        }

        // Pins first, then labels placed to avoid each other and the pins.
        let shown: Vec<(&Marker, Point)> = self
            .markers
            .iter()
            .map(|m| (m, view.to_screen(project(m.lon, m.lat))))
            .collect();
        let points: Vec<Point> = shown.iter().map(|(_, p)| *p).collect();
        for p in &points {
            draw_pin(&mut frame, *p, &c);
        }
        let pins: Vec<Rectangle> = points
            .iter()
            .map(|p| Rectangle::new(Point::new(p.x - 10.0, p.y - 32.0), Size::new(20.0, 34.0)))
            .collect();
        let mut placed: Vec<Rectangle> = Vec::new();
        for (marker, p) in shown.iter().map(|(m, p)| (*m, p)) {
            let size = label_size(marker);
            let candidates = [
                Point::new(p.x + 12.0, p.y - 44.0),               // right, above
                Point::new(p.x + 12.0, p.y - 6.0),                // right, below
                Point::new(p.x - 12.0 - size.width, p.y - 44.0),  // left, above
                Point::new(p.x - 12.0 - size.width, p.y - 6.0),   // left, below
            ];
            let overlap = |r: &Rectangle| -> f32 {
                placed
                    .iter()
                    .chain(pins.iter())
                    .filter_map(|o| o.intersection(r))
                    .map(|i| i.width * i.height)
                    .sum()
            };
            let best = candidates
                .iter()
                .map(|o| Rectangle::new(*o, size))
                .min_by(|a, b| overlap(a).total_cmp(&overlap(b)))
                .unwrap();
            draw_label(&mut frame, best.position(), marker, &c);
            placed.push(best);
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag_from.is_some() {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(bounds) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

const TITLE_SIZE: f32 = 12.5;
const SUB_SIZE: f32 = 11.5;

fn label_size(marker: &Marker) -> Size {
    let approx = |s: &str, size: f32| s.chars().count() as f32 * size * 0.56;
    Size::new(
        approx(&marker.title, TITLE_SIZE).max(approx(&marker.subtitle, SUB_SIZE)) + 20.0,
        38.0,
    )
}

fn draw_pin(frame: &mut Frame, p: Point, c: &style::Colors) {
    // A teardrop with a white core.
    let pin = Path::new(|b| {
        let r = 9.0;
        let cy = p.y - 22.0;
        b.move_to(p);
        b.bezier_curve_to(Point::new(p.x - 3.0, cy + 10.0), Point::new(p.x - r, cy + 6.0), Point::new(p.x - r, cy));
        b.arc(canvas::path::Arc {
            center: Point::new(p.x, cy),
            radius: r,
            start_angle: std::f32::consts::PI.into(),
            end_angle: (2.0 * std::f32::consts::PI).into(),
        });
        b.bezier_curve_to(Point::new(p.x + r, cy + 6.0), Point::new(p.x + 3.0, cy + 10.0), p);
        b.close();
    });
    frame.fill(&Path::circle(Point::new(p.x, p.y + 1.0), 3.0), Color::from_rgba(0.0, 0.0, 0.0, 0.25));
    frame.fill(&pin, c.accent);
    frame.stroke(&pin, Stroke::default().with_color(Color::WHITE).with_width(1.5));
    frame.fill(&Path::circle(Point::new(p.x, p.y - 22.0), 3.5), Color::WHITE);
}

/// A pill with the place name and its live local time.
fn draw_label(frame: &mut Frame, origin: Point, marker: &Marker, c: &style::Colors) {
    let size = label_size(marker);
    let pill = Path::rounded_rectangle(origin, size, 9.0.into());
    frame.fill(
        &Path::rounded_rectangle(origin + Vector::new(0.0, 2.0), size, 9.0.into()),
        Color::from_rgba(0.0, 0.0, 0.0, 0.18),
    );
    frame.fill(&pill, Color { a: 0.95, ..c.surface });
    frame.stroke(&pill, Stroke::default().with_color(c.border).with_width(1.0));
    frame.fill_text(Text {
        content: marker.title.clone(),
        position: origin + Vector::new(10.0, 6.0),
        color: c.text,
        size: TITLE_SIZE.into(),
        font: style::INTER_SEMIBOLD,
        align_x: alignment::Horizontal::Left.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });
    frame.fill_text(Text {
        content: marker.subtitle.clone(),
        position: origin + Vector::new(10.0, 21.0),
        color: c.muted,
        size: SUB_SIZE.into(),
        font: style::INTER,
        align_x: alignment::Horizontal::Left.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_bounds() {
        let (x, y) = project(0.0, 0.0);
        assert!((x - 0.5).abs() < 1e-9 && (y - 0.5).abs() < 1e-9);
        let (x, _) = project(-180.0, 0.0);
        assert!(x.abs() < 1e-9);
    }

    #[test]
    fn fit_keeps_points_visible() {
        let mut v = View {
            size: Size::new(900.0, 500.0),
            ..View::default()
        };
        let pts = [(-0.1278, 51.5074), (-74.006, 40.7128)];
        v.fit(&pts);
        for (lon, lat) in pts {
            let p = v.to_screen(project(lon, lat));
            assert!(p.x > 0.0 && p.x < 900.0 && p.y > 0.0 && p.y < 500.0, "{p:?}");
        }
    }

    #[test]
    fn tiles_wrap_horizontally() {
        let v = View {
            center: (0.0, 0.5),
            zoom: 2.0,
            size: Size::new(600.0, 400.0),
            max_zoom: 19,
            anchor: 0.5,
        };
        let keys = v.visible_tiles();
        assert!(keys.iter().all(|k| k.x < 4 && k.y < 4));
        assert!(keys.iter().any(|k| k.x == 3) && keys.iter().any(|k| k.x == 0));
    }
}

#[cfg(test)]
mod strip_tests {
    use super::*;

    #[test]
    fn wide_strip_keeps_west_to_east_order() {
        let mut v = View {
            size: Size::new(1200.0, 156.0),
            ..View::default()
        };
        let pts = [(-74.006, 40.7128), (-0.1278, 51.5074), (72.88, 19.07), (139.69, 35.69), (151.2, -33.87)];
        v.fit(&pts);
        let xs: Vec<f32> = pts.iter().map(|(lon, lat)| v.to_screen(project(*lon, *lat)).x).collect();
        assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
    }
}
