//! Colours, fonts and widget styles for a modern light and dark look.

use iced::font::{Family, Weight};
use iced::theme::Palette;
use iced::widget::{button, container, scrollable, text_input};
use iced::{Background, Border, Color, Font, Shadow, Theme, Vector, color};

pub const INTER: Font = Font {
    family: Family::Name("Inter"),
    ..Font::DEFAULT
};
pub const INTER_LIGHT: Font = Font {
    weight: Weight::Light,
    ..INTER
};
pub const INTER_MEDIUM: Font = Font {
    weight: Weight::Medium,
    ..INTER
};
pub const INTER_SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..INTER
};

pub const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Light.ttf"),
    include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
];

pub const RADIUS: f32 = 16.0;

/// Semantic colours for the current appearance.
#[derive(Debug, Clone, Copy)]
pub struct Colors {
    pub dark: bool,
    pub background: Color,
    pub surface: Color,
    pub surface_raised: Color,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub danger: Color,
    pub sun: Color,
    pub cloud: Color,
    pub rain: Color,
    pub snow: Color,
}

const DARK: Colors = Colors {
    dark: true,
    background: color!(0x0e1117),
    surface: color!(0x171b23),
    surface_raised: color!(0x1e232d),
    border: color!(0x272d39),
    text: color!(0xe9edf4),
    muted: color!(0x8a93a6),
    accent: color!(0x6b93ff),
    accent_soft: color!(0x6b93ff, 0.16),
    danger: color!(0xff6b6b),
    sun: color!(0xffc53d),
    cloud: color!(0xc5cede),
    rain: color!(0x5fb0ff),
    snow: color!(0xe6f1ff),
};

const LIGHT: Colors = Colors {
    dark: false,
    background: color!(0xf2f4f8),
    surface: color!(0xffffff),
    surface_raised: color!(0xf7f8fb),
    border: color!(0xe1e5ec),
    text: color!(0x141821),
    muted: color!(0x667085),
    accent: color!(0x3d6bf5),
    accent_soft: color!(0x3d6bf5, 0.10),
    danger: color!(0xe5484d),
    sun: color!(0xf5a524),
    cloud: color!(0x9aa5b8),
    rain: color!(0x2f8cf0),
    snow: color!(0x8fb4e3),
};

pub fn colors(theme: &Theme) -> Colors {
    if theme.extended_palette().is_dark { DARK } else { LIGHT }
}

pub fn theme(dark: bool) -> Theme {
    let c = if dark { DARK } else { LIGHT };
    Theme::custom(
        if dark { "TimeAtlas Dark" } else { "TimeAtlas Light" },
        Palette {
            background: c.background,
            text: c.text,
            primary: c.accent,
            success: color!(0x30a46c),
            warning: c.sun,
            danger: c.danger,
        },
    )
}

// --- Containers ------------------------------------------------------------

pub fn app_background(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.background.into()),
        text_color: Some(c.text),
        ..container::Style::default()
    }
}

pub fn card(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.surface.into()),
        text_color: Some(c.text),
        border: Border {
            color: c.border,
            width: 1.0,
            radius: RADIUS.into(),
        },
        ..container::Style::default()
    }
}

pub fn panel(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, if c.dark { 0.55 } else { 0.18 }),
            offset: Vector::new(0.0, 16.0),
            blur_radius: 28.0,
        },
        border: Border {
            color: c.border,
            width: 1.0,
            radius: 20.0.into(),
        },
        ..card(theme)
    }
}

pub fn inset(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.surface_raised.into()),
        border: Border {
            color: c.border,
            width: 1.0,
            radius: 12.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn badge(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.accent_soft.into()),
        text_color: Some(c.accent),
        border: Border {
            radius: 999.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

pub fn scrim(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(
            Color::from_rgba(0.0, 0.0, 0.0, if c.dark { 0.55 } else { 0.28 }).into(),
        ),
        ..container::Style::default()
    }
}

pub fn map_frame(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.surface_raised.into()),
        border: Border {
            color: c.border,
            width: 1.0,
            radius: RADIUS.into(),
        },
        ..container::Style::default()
    }
}

pub fn map_control_group(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(Color { a: 0.92, ..c.surface }.into()),
        border: Border {
            color: c.border,
            width: 1.0,
            radius: 10.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.18),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 8.0,
        },
        ..container::Style::default()
    }
}

pub fn attribution(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(Color { a: 0.85, ..c.surface }.into()),
        text_color: Some(c.muted),
        border: Border {
            radius: 6.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

// --- Buttons ---------------------------------------------------------------

fn base_button(bg: Option<Color>, fg: Color, border: Color, radius: f32) -> button::Style {
    button::Style {
        background: bg.map(Background::Color),
        text_color: fg,
        border: Border {
            color: border,
            width: if border.a > 0.0 { 1.0 } else { 0.0 },
            radius: radius.into(),
        },
        ..button::Style::default()
    }
}

pub fn primary_button(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let bg = match status {
        button::Status::Hovered => lighten(c.accent, 0.08),
        button::Status::Pressed => darken(c.accent, 0.08),
        button::Status::Disabled => Color { a: 0.4, ..c.accent },
        button::Status::Active => c.accent,
    };
    base_button(Some(bg), Color::WHITE, Color::TRANSPARENT, 10.0)
}

pub fn secondary_button(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let (bg, fg) = match status {
        button::Status::Hovered => (c.accent_soft, c.accent),
        button::Status::Pressed => (Color { a: 0.25, ..c.accent }, c.accent),
        button::Status::Disabled => (c.surface_raised, Color { a: 0.4, ..c.muted }),
        button::Status::Active => (c.surface_raised, c.text),
    };
    base_button(Some(bg), fg, c.border, 10.0)
}

pub fn danger_button(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let (bg, fg) = match status {
        button::Status::Hovered => (Color { a: 0.14, ..c.danger }, c.danger),
        button::Status::Pressed => (Color { a: 0.24, ..c.danger }, c.danger),
        button::Status::Disabled => (Color::TRANSPARENT, Color { a: 0.35, ..c.muted }),
        button::Status::Active => (Color::TRANSPARENT, c.danger),
    };
    base_button(Some(bg), fg, Color::TRANSPARENT, 8.0)
}

/// Borderless icon button (header, map controls, close buttons).
pub fn icon_button(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let (bg, fg) = match status {
        button::Status::Hovered => (Some(c.accent_soft), c.accent),
        button::Status::Pressed => (Some(Color { a: 0.25, ..c.accent }), c.accent),
        button::Status::Disabled => (None, Color { a: 0.4, ..c.muted }),
        button::Status::Active => (None, c.text),
    };
    base_button(bg, fg, Color::TRANSPARENT, 8.0)
}

pub fn tab_button(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let c = colors(theme);
        let (bg, fg) = if active {
            (Some(c.surface), c.text)
        } else if matches!(status, button::Status::Hovered) {
            (None, c.text)
        } else {
            (None, c.muted)
        };
        let mut s = base_button(bg, fg, Color::TRANSPARENT, 8.0);
        if active {
            s.shadow = Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.12),
                offset: Vector::new(0.0, 1.0),
                blur_radius: 4.0,
            };
        }
        s
    }
}

// --- Inputs ----------------------------------------------------------------

pub fn search_input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let c = colors(theme);
    let border = match status {
        text_input::Status::Focused { .. } => c.accent,
        text_input::Status::Hovered => lighten(c.border, 0.1),
        _ => c.border,
    };
    text_input::Style {
        background: c.surface_raised.into(),
        border: Border {
            color: border,
            width: if matches!(status, text_input::Status::Focused { .. }) { 2.0 } else { 1.0 },
            radius: 10.0.into(),
        },
        icon: c.muted,
        placeholder: c.muted,
        value: c.text,
        selection: Color { a: 0.35, ..c.accent },
    }
}

pub fn scroll(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let c = colors(theme);
    let mut s = scrollable::default(theme, status);
    for rail in [&mut s.vertical_rail, &mut s.horizontal_rail] {
        rail.background = None;
        rail.border = Border::default();
        rail.scroller.background = Color { a: 0.35, ..c.muted }.into();
        rail.scroller.border = Border {
            radius: 999.0.into(),
            ..Border::default()
        };
    }
    s
}

// --- Helpers ---------------------------------------------------------------

pub fn lighten(c: Color, amount: f32) -> Color {
    Color {
        r: (c.r + amount).min(1.0),
        g: (c.g + amount).min(1.0),
        b: (c.b + amount).min(1.0),
        a: c.a,
    }
}

pub fn darken(c: Color, amount: f32) -> Color {
    Color {
        r: (c.r - amount).max(0.0),
        g: (c.g - amount).max(0.0),
        b: (c.b - amount).max(0.0),
        a: c.a,
    }
}
