use iced::widget::container;
use iced::{Border, Color, border};

use super::Tone;
use crate::theme::Theme;

fn surface(color: Color, radius: f32) -> container::Style {
    container::Style {
        background: Some(color.into()),
        border: border::rounded(radius),
        ..container::Style::default()
    }
}

pub fn content(theme: &Theme) -> container::Style {
    surface(theme.colors().background, 0.0)
}

pub fn rail(theme: &Theme) -> container::Style {
    surface(theme.colors().rail, 0.0)
}

pub fn sidebar(theme: &Theme) -> container::Style {
    surface(theme.colors().sidebar, 0.0)
}

pub fn card(theme: &Theme) -> container::Style {
    let (c, m) = (theme.colors(), theme.metrics());
    container::Style {
        border: Border { color: c.line, width: m.card_border, radius: m.card_radius.into() },
        ..surface(c.card, m.card_radius)
    }
}

/// Inset surface inside a panel.
pub fn well(theme: &Theme) -> container::Style {
    surface(theme.colors().background, theme.metrics().control_radius)
}

/// Inset surface directly on the page.
pub fn tray(theme: &Theme) -> container::Style {
    surface(theme.colors().card, theme.metrics().control_radius)
}

pub fn divider(theme: &Theme) -> container::Style {
    surface(theme.colors().line, 0.0)
}

pub fn disc(on: bool, size: f32) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let c = theme.colors();
        // Square themes get square discs.
        let radius = if theme.metrics().control_radius == 0.0 { 0.0 } else { size / 2.0 };
        surface(if on { c.accent } else { c.track }, radius)
    }
}

pub fn swatch(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let radius = (theme.metrics().control_radius * 0.75).into();
        container::Style {
            border: Border { color: theme.colors().line, width: 1.0, radius },
            ..surface(color, 0.0)
        }
    }
}

pub fn badge(theme: &Theme) -> container::Style {
    let c = theme.colors();
    container::Style {
        text_color: Some(c.text_dim),
        ..surface(c.raised, theme.metrics().control_radius * 0.6)
    }
}

pub fn meter(tone: Option<Tone>) -> impl Fn(&Theme) -> container::Style {
    move |theme| surface(tone.map_or(theme.colors().track, |tone| tone.color(theme)), 4.0)
}

/// A radio indicator: a ring that thickens into a filled disc when selected.
pub fn radio(on: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let c = theme.colors();
        let square = theme.metrics().control_radius == 0.0;
        container::Style {
            border: Border {
                color: if on { c.accent } else { c.text_mute },
                width: if on { 6.0 } else { 2.0 },
                radius: if square { 0.0 } else { 10.0 }.into(),
            },
            ..container::Style::default()
        }
    }
}
