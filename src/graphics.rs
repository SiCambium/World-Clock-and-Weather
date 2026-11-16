//! Vector drawings rendered with `canvas`: weather icons, analogue clocks and
//! small UI glyphs. Drawing them ourselves keeps the look crisp at any size
//! and consistent in light and dark mode, with no emoji or icon fonts.

use std::f32::consts::{PI, TAU};

use iced::mouse;
use iced::widget::canvas::{self, Canvas, Frame, LineCap, Path, Stroke, path::Arc};
use iced::{Color, Element, Point, Rectangle, Renderer, Theme, Vector};

use crate::style::{self, Colors};

fn sized<'a, M: 'a, P: canvas::Program<M> + 'a>(program: P, size: f32) -> Element<'a, M> {
    Canvas::new(program).width(size).height(size).into()
}

/// Scales unit-square coordinates (0..1) to a frame.
struct Unit {
    s: f32,
    ox: f32,
    oy: f32,
}

impl Unit {
    fn new(bounds: Rectangle) -> Self {
        let s = bounds.width.min(bounds.height);
        Self {
            s,
            ox: (bounds.width - s) / 2.0,
            oy: (bounds.height - s) / 2.0,
        }
    }
    fn p(&self, x: f32, y: f32) -> Point {
        Point::new(self.ox + x * self.s, self.oy + y * self.s)
    }
    fn l(&self, v: f32) -> f32 {
        v * self.s
    }
}

fn stroke(color: Color, width: f32) -> Stroke<'static> {
    Stroke::default()
        .with_color(color)
        .with_width(width)
        .with_line_cap(LineCap::Round)
}

// --- Weather icons ---------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Sky {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    Snow,
    Thunder,
    Unknown,
}

fn sky(code: i32) -> Sky {
    match code {
        0 | 1 => Sky::Clear,
        2 => Sky::PartlyCloudy,
        3 => Sky::Cloudy,
        45 | 48 => Sky::Fog,
        51 | 53 | 55 | 56 | 57 => Sky::Drizzle,
        61 | 63 | 65 | 66 | 67 | 80 | 81 | 82 => Sky::Rain,
        71 | 73 | 75 | 77 | 85 | 86 => Sky::Snow,
        95 | 96 | 99 => Sky::Thunder,
        _ => Sky::Unknown,
    }
}

pub struct WeatherIcon {
    pub code: i32,
    pub is_day: bool,
}

/// A weather icon for a WMO code. `code < 0` draws a neutral placeholder.
pub fn weather_icon<'a, M: 'a>(code: i32, is_day: bool, size: f32) -> Element<'a, M> {
    sized(WeatherIcon { code, is_day }, size)
}

fn draw_sun(frame: &mut Frame, u: &Unit, cx: f32, cy: f32, r: f32, c: &Colors) {
    frame.fill(&Path::circle(u.p(cx, cy), u.l(r)), c.sun);
    let st = stroke(c.sun, u.l(0.055));
    for i in 0..8 {
        let a = i as f32 * TAU / 8.0;
        let (s, co) = a.sin_cos();
        let inner = r + 0.08;
        let outer = r + 0.17;
        frame.stroke(
            &Path::line(
                u.p(cx + co * inner, cy + s * inner),
                u.p(cx + co * outer, cy + s * outer),
            ),
            st,
        );
    }
}

fn draw_moon(frame: &mut Frame, u: &Unit, cx: f32, cy: f32, r: f32, c: &Colors) {
    // Crescent: the big circle minus an offset circle.
    let (c1, r1) = ((cx, cy), r);
    let (c2, r2) = ((cx + r * 0.55, cy - r * 0.45), r * 0.85);
    let (dx, dy) = (c2.0 - c1.0, c2.1 - c1.1);
    let d = (dx * dx + dy * dy).sqrt();
    let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h = (r1 * r1 - a * a).max(0.0).sqrt();
    let (mx, my) = (c1.0 + a * dx / d, c1.1 + a * dy / d);
    let p1 = (mx + h * dy / d, my - h * dx / d);
    let p2 = (mx - h * dy / d, my + h * dx / d);
    let ang = |c: (f32, f32), p: (f32, f32)| (p.1 - c.1).atan2(p.0 - c.0);

    let a1s = ang(c1, p1);
    let mut a1e = ang(c1, p2);
    // Go the long way round the big circle (away from the cut).
    let mid_dir = dy.atan2(dx);
    let norm = |x: f32| (x % TAU + TAU) % TAU;
    // choose direction that does not pass mid_dir
    let ccw_contains = |s: f32, e: f32, t: f32| {
        let (s, e, t) = (norm(s), norm(e), norm(t));
        if s <= e { t >= s && t <= e } else { t >= s || t <= e }
    };
    if ccw_contains(a1s, a1e, mid_dir) {
        // sweep the other way
        while a1e > a1s {
            a1e -= TAU;
        }
    } else {
        while a1e < a1s {
            a1e += TAU;
        }
    }
    let a2s = ang(c2, p2);
    let mut a2e = ang(c2, p1);
    // Short way round the cut circle, through the inside of the big circle.
    let back_dir = (-dy).atan2(-dx);
    if ccw_contains(a2s, a2e, back_dir) {
        while a2e < a2s {
            a2e += TAU;
        }
    } else {
        while a2e > a2s {
            a2e -= TAU;
        }
    }

    let moon = Path::new(|b| {
        b.move_to(u.p(p1.0, p1.1));
        b.arc(Arc {
            center: u.p(c1.0, c1.1),
            radius: u.l(r1),
            start_angle: a1s.into(),
            end_angle: a1e.into(),
        });
        b.arc(Arc {
            center: u.p(c2.0, c2.1),
            radius: u.l(r2),
            start_angle: a2s.into(),
            end_angle: a2e.into(),
        });
        b.close();
    });
    let moon_color = if c.dark { Color::from_rgb8(0xf2, 0xe6, 0xb8) } else { Color::from_rgb8(0xc9, 0xa8, 0x4a) };
    frame.fill(&moon, moon_color);
}

fn draw_cloud(frame: &mut Frame, u: &Unit, dx: f32, dy: f32, scale: f32, color: Color) {
    let t = |x: f32, y: f32| u.p(0.5 + (x - 0.5) * scale + dx, 0.5 + (y - 0.5) * scale + dy);
    let r = |v: f32| u.l(v * scale);
    let cloud = Path::new(|b| {
        b.circle(t(0.33, 0.60), r(0.15));
        b.circle(t(0.52, 0.50), r(0.21));
        b.circle(t(0.71, 0.61), r(0.14));
    });
    frame.fill(&cloud, color);
    let base = Path::rounded_rectangle(
        t(0.33, 0.55),
        iced::Size::new(r(0.38), r(0.20)),
        r(0.10).into(),
    );
    frame.fill(&base, color);
    let left = Path::rectangle(t(0.33, 0.55), iced::Size::new(r(0.2), r(0.2)));
    frame.fill(&left, color);
}

impl<M> canvas::Program<M> for WeatherIcon {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let u = Unit::new(bounds);
        let c = style::colors(theme);
        let celestial = |frame: &mut Frame, cx, cy, r| {
            if self.is_day {
                draw_sun(frame, &u, cx, cy, r, &c);
            } else {
                draw_moon(frame, &u, cx, cy, r + 0.04, &c);
            }
        };

        match sky(self.code) {
            Sky::Clear => celestial(&mut frame, 0.5, 0.5, 0.19),
            Sky::PartlyCloudy => {
                celestial(&mut frame, 0.36, 0.36, 0.13);
                draw_cloud(&mut frame, &u, 0.06, 0.06, 0.95, c.cloud);
            }
            Sky::Cloudy => {
                draw_cloud(&mut frame, &u, -0.08, -0.06, 0.75, style::darken(c.cloud, 0.12));
                draw_cloud(&mut frame, &u, 0.04, 0.04, 1.0, c.cloud);
            }
            Sky::Fog => {
                draw_cloud(&mut frame, &u, 0.0, -0.1, 0.9, c.cloud);
                let st = stroke(c.muted, u.l(0.05));
                for (i, y) in [0.70, 0.80, 0.90].iter().enumerate() {
                    let off = if i % 2 == 0 { 0.0 } else { 0.06 };
                    frame.stroke(&Path::line(u.p(0.22 + off, *y), u.p(0.78 - off, *y)), st);
                }
            }
            Sky::Drizzle | Sky::Rain => {
                draw_cloud(&mut frame, &u, 0.0, -0.12, 0.95, c.cloud);
                let heavy = sky(self.code) == Sky::Rain;
                let st = stroke(c.rain, u.l(if heavy { 0.055 } else { 0.045 }));
                let len = if heavy { 0.14 } else { 0.07 };
                for x in [0.36, 0.52, 0.68] {
                    frame.stroke(&Path::line(u.p(x, 0.68), u.p(x - 0.05, 0.68 + len)), st);
                }
            }
            Sky::Snow => {
                draw_cloud(&mut frame, &u, 0.0, -0.12, 0.95, c.cloud);
                for (x, y) in [(0.36, 0.76), (0.52, 0.84), (0.68, 0.76)] {
                    frame.fill(&Path::circle(u.p(x, y), u.l(0.045)), c.snow);
                    frame.stroke(&Path::circle(u.p(x, y), u.l(0.045)), stroke(c.rain, u.l(0.015)));
                }
            }
            Sky::Thunder => {
                draw_cloud(&mut frame, &u, 0.0, -0.12, 0.95, style::darken(c.cloud, 0.1));
                let bolt = Path::new(|b| {
                    b.move_to(u.p(0.54, 0.58));
                    b.line_to(u.p(0.42, 0.78));
                    b.line_to(u.p(0.51, 0.78));
                    b.line_to(u.p(0.45, 0.95));
                    b.line_to(u.p(0.63, 0.71));
                    b.line_to(u.p(0.54, 0.71));
                    b.line_to(u.p(0.60, 0.58));
                    b.close();
                });
                frame.fill(&bolt, c.sun);
            }
            Sky::Unknown => {
                frame.stroke(
                    &Path::circle(u.p(0.5, 0.5), u.l(0.22)),
                    stroke(Color { a: 0.5, ..c.muted }, u.l(0.04)),
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

// --- Analogue clock --------------------------------------------------------

pub struct Clock {
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub is_day: bool,
}

pub fn clock<'a, M: 'a>(hour: u32, minute: u32, second: u32, size: f32) -> Element<'a, M> {
    sized(
        Clock {
            hour,
            minute,
            second,
            is_day: (6..18).contains(&hour),
        },
        size,
    )
}

impl<M> canvas::Program<M> for Clock {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let u = Unit::new(bounds);
        let c = style::colors(theme);
        let center = u.p(0.5, 0.5);

        let face = if self.is_day {
            c.surface_raised
        } else if c.dark {
            Color::from_rgb8(0x10, 0x14, 0x22)
        } else {
            Color::from_rgb8(0x24, 0x2b, 0x3d)
        };
        let ink = if self.is_day { c.text } else { Color::from_rgb8(0xe9, 0xed, 0xf4) };

        frame.fill(&Path::circle(center, u.l(0.47)), face);
        frame.stroke(&Path::circle(center, u.l(0.47)), stroke(c.border, u.l(0.02)));

        for i in 0..12 {
            let a = i as f32 * TAU / 12.0;
            let major = i % 3 == 0;
            let (s, co) = a.sin_cos();
            let (r0, r1) = (if major { 0.34 } else { 0.37 }, 0.41);
            frame.stroke(
                &Path::line(
                    u.p(0.5 + co * r0, 0.5 + s * r0),
                    u.p(0.5 + co * r1, 0.5 + s * r1),
                ),
                stroke(
                    Color { a: if major { 0.8 } else { 0.35 }, ..ink },
                    u.l(if major { 0.035 } else { 0.025 }),
                ),
            );
        }

        let hand = |frame: &mut Frame, angle: f32, len: f32, width: f32, color: Color| {
            let a = angle - PI / 2.0;
            let (s, co) = a.sin_cos();
            frame.stroke(
                &Path::line(
                    u.p(0.5 - co * 0.06, 0.5 - s * 0.06),
                    u.p(0.5 + co * len, 0.5 + s * len),
                ),
                stroke(color, u.l(width)),
            );
        };
        let (h, m, s) = (self.hour as f32, self.minute as f32, self.second as f32);
        hand(&mut frame, ((h % 12.0) + m / 60.0) / 12.0 * TAU, 0.22, 0.06, ink);
        hand(&mut frame, (m + s / 60.0) / 60.0 * TAU, 0.33, 0.04, ink);
        hand(&mut frame, s / 60.0 * TAU, 0.37, 0.018, c.accent);
        frame.fill(&Path::circle(center, u.l(0.04)), c.accent);

        vec![frame.into_geometry()]
    }
}

// --- UI glyphs -------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Glyph {
    Settings,
    Close,
    Refresh,
    Plus,
    Minus,
    Fit,
    Search,
    Globe,
    Moon,
    Sunrise,
    Sunset,
    Up,
    Down,
    Edit,
}

struct GlyphProgram {
    glyph: Glyph,
    color: Option<Color>,
}

/// A small line icon that inherits the accent or text colour.
pub fn glyph<'a, M: 'a>(glyph: Glyph, size: f32) -> Element<'a, M> {
    sized(GlyphProgram { glyph, color: None }, size)
}

/// A small line icon in a given colour.
pub fn glyph_colored<'a, M: 'a>(glyph: Glyph, size: f32, color: Color) -> Element<'a, M> {
    sized(GlyphProgram { glyph, color: Some(color) }, size)
}

impl<M> canvas::Program<M> for GlyphProgram {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let u = Unit::new(bounds);
        let c = style::colors(theme);
        let color = self.color.unwrap_or(c.text);
        let st = stroke(color, u.l(0.09).max(1.4));

        match self.glyph {
            Glyph::Settings => {
                // Three slider lines with knobs: a clean "manage" icon.
                for (y, knob) in [(0.25, 0.65), (0.5, 0.35), (0.75, 0.58)] {
                    frame.stroke(&Path::line(u.p(0.12, y), u.p(0.88, y)), st);
                    frame.fill(&Path::circle(u.p(knob, y), u.l(0.11)), color);
                }
            }
            Glyph::Close => {
                frame.stroke(&Path::line(u.p(0.25, 0.25), u.p(0.75, 0.75)), st);
                frame.stroke(&Path::line(u.p(0.75, 0.25), u.p(0.25, 0.75)), st);
            }
            Glyph::Refresh => {
                let (start, end) = (PI / 6.0, PI * 11.0 / 6.0);
                let r = 0.3;
                let arc = Path::new(|b| {
                    b.arc(Arc {
                        center: u.p(0.5, 0.5),
                        radius: u.l(r),
                        start_angle: start.into(),
                        end_angle: (end - 0.25).into(),
                    });
                });
                frame.stroke(&arc, st);
                // Filled arrow head at the end of the arc, pointing along it.
                let (s, co) = end.sin_cos();
                let at = (0.5 + co * r, 0.5 + s * r);
                let tangent = (-s, co);
                let normal = (co, s);
                let head = Path::new(|b| {
                    b.move_to(u.p(at.0 + tangent.0 * 0.14, at.1 + tangent.1 * 0.14));
                    b.line_to(u.p(at.0 + normal.0 * 0.15 - tangent.0 * 0.08, at.1 + normal.1 * 0.15 - tangent.1 * 0.08));
                    b.line_to(u.p(at.0 - normal.0 * 0.15 - tangent.0 * 0.08, at.1 - normal.1 * 0.15 - tangent.1 * 0.08));
                    b.close();
                });
                frame.fill(&head, color);
            }
            Glyph::Plus => {
                frame.stroke(&Path::line(u.p(0.5, 0.2), u.p(0.5, 0.8)), st);
                frame.stroke(&Path::line(u.p(0.2, 0.5), u.p(0.8, 0.5)), st);
            }
            Glyph::Minus => {
                frame.stroke(&Path::line(u.p(0.2, 0.5), u.p(0.8, 0.5)), st);
            }
            Glyph::Fit => {
                for (x, y, sx, sy) in [
                    (0.18, 0.18, 1.0, 1.0),
                    (0.82, 0.18, -1.0, 1.0),
                    (0.18, 0.82, 1.0, -1.0),
                    (0.82, 0.82, -1.0, -1.0),
                ] {
                    let corner = Path::new(|b| {
                        b.move_to(u.p(x, y + sy * 0.2));
                        b.line_to(u.p(x, y));
                        b.line_to(u.p(x + sx * 0.2, y));
                    });
                    frame.stroke(&corner, st);
                }
                frame.fill(&Path::circle(u.p(0.5, 0.5), u.l(0.08)), color);
            }
            Glyph::Search => {
                frame.stroke(&Path::circle(u.p(0.43, 0.43), u.l(0.24)), st);
                frame.stroke(&Path::line(u.p(0.61, 0.61), u.p(0.84, 0.84)), st);
            }
            Glyph::Moon => {
                // A crescent: the part of disc A outside disc B, traced as
                // one outline (separate arcs would each start a new shape).
                let (ca, ra) = ((0.5f32, 0.5f32), 0.38f32);
                let (cb, rb) = ((0.7f32, 0.36f32), 0.32f32);
                let (dx, dy) = (cb.0 - ca.0, cb.1 - ca.1);
                let d = (dx * dx + dy * dy).sqrt();
                let a = (ra * ra - rb * rb + d * d) / (2.0 * d);
                let h = (ra * ra - a * a).max(0.0).sqrt();
                let (px, py) = (ca.0 + a * dx / d, ca.1 + a * dy / d);
                let i1 = (px - h * dy / d, py + h * dx / d);
                let i2 = (px + h * dy / d, py - h * dx / d);
                let ang = |c: (f32, f32), p: (f32, f32)| (p.1 - c.1).atan2(p.0 - c.0);
                // Of the two ways round a circle between the crossing
                // points, pick the one whose middle passes `keep`.
                let sweep = |c: (f32, f32), r: f32, from: f32, to: f32, keep: &dyn Fn(f32, f32) -> bool| {
                    let mut s = (to - from).rem_euclid(TAU);
                    let mid = from + s / 2.0;
                    if !keep(c.0 + r * mid.cos(), c.1 + r * mid.sin()) {
                        s -= TAU;
                    }
                    s
                };
                let dist = |p: (f32, f32), x: f32, y: f32| ((x - p.0).powi(2) + (y - p.1).powi(2)).sqrt();
                // Outer edge: round A from i1 to i2, outside B.
                let outer_from = ang(ca, i1);
                let outer_sweep = sweep(ca, ra, outer_from, ang(ca, i2), &|x, y| dist(cb, x, y) > rb);
                // Inner edge: back round B from i2 to i1, inside A.
                let inner_from = ang(cb, i2);
                let inner_sweep = sweep(cb, rb, inner_from, ang(cb, i1), &|x, y| dist(ca, x, y) < ra);
                let crescent = Path::new(|b| {
                    let steps = 24;
                    b.move_to(u.p(i1.0, i1.1));
                    for k in 1..=steps {
                        let t = outer_from + outer_sweep * k as f32 / steps as f32;
                        b.line_to(u.p(ca.0 + ra * t.cos(), ca.1 + ra * t.sin()));
                    }
                    for k in 1..=steps {
                        let t = inner_from + inner_sweep * k as f32 / steps as f32;
                        b.line_to(u.p(cb.0 + rb * t.cos(), cb.1 + rb * t.sin()));
                    }
                    b.close();
                });
                frame.fill(&crescent, color);
            }
            Glyph::Sunrise | Glyph::Sunset => {
                let sun = c.sun;
                let thin = stroke(sun, u.l(0.08).max(1.2));
                // Half sun on the horizon with three rays.
                let half = Path::new(|b| {
                    b.arc(Arc {
                        center: u.p(0.5, 0.68),
                        radius: u.l(0.2),
                        start_angle: PI.into(),
                        end_angle: TAU.into(),
                    });
                    b.close();
                });
                frame.fill(&half, sun);
                for a in [PI * 1.15, PI * 1.5, PI * 1.85] {
                    let (s, co) = a.sin_cos();
                    frame.stroke(
                        &Path::line(u.p(0.5 + co * 0.29, 0.68 + s * 0.29), u.p(0.5 + co * 0.4, 0.68 + s * 0.4)),
                        thin,
                    );
                }
                frame.stroke(&Path::line(u.p(0.1, 0.7), u.p(0.9, 0.7)), stroke(color, u.l(0.07).max(1.1)));
                // Arrow: up for sunrise, down for sunset.
                let up = self.glyph == Glyph::Sunrise;
                let (tip, base) = if up { (0.82, 0.96) } else { (0.96, 0.82) };
                let arrow = Path::new(|b| {
                    b.move_to(u.p(0.5, tip));
                    b.line_to(u.p(0.38, base));
                    b.line_to(u.p(0.62, base));
                    b.close();
                });
                frame.fill(&arrow, color);
            }
            Glyph::Up | Glyph::Down => {
                let (a, b2) = if self.glyph == Glyph::Up { (0.32, 0.66) } else { (0.68, 0.34) };
                let chevron = Path::new(|b| {
                    b.move_to(u.p(0.25, b2));
                    b.line_to(u.p(0.5, a));
                    b.line_to(u.p(0.75, b2));
                });
                frame.stroke(&chevron, st);
            }
            Glyph::Edit => {
                // A pencil at 45 degrees.
                frame.stroke(&Path::line(u.p(0.28, 0.72), u.p(0.72, 0.28)), stroke(color, u.l(0.16).max(2.0)));
                let tip = Path::new(|b| {
                    b.move_to(u.p(0.18, 0.82));
                    b.line_to(u.p(0.22, 0.66));
                    b.line_to(u.p(0.34, 0.78));
                    b.close();
                });
                frame.fill(&tip, color);
            }
            Glyph::Globe => {
                let thin = stroke(color, u.l(0.06).max(1.2));
                frame.stroke(&Path::circle(u.p(0.5, 0.5), u.l(0.4)), thin);
                frame.stroke(&Path::line(u.p(0.1, 0.5), u.p(0.9, 0.5)), thin);
                let meridian = Path::new(|b| {
                    b.ellipse(canvas::path::arc::Elliptical {
                        center: u.p(0.5, 0.5),
                        radii: Vector::new(u.l(0.17), u.l(0.4)),
                        rotation: 0.0.into(),
                        start_angle: 0.0.into(),
                        end_angle: TAU.into(),
                    });
                });
                frame.stroke(&meridian, thin);
            }
        }
        vec![frame.into_geometry()]
    }
}
