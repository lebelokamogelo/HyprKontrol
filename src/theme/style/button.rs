use iced::widget::button;
use iced::{Background, Border, Color, border};

use crate::theme::Theme;
use crate::theme::color::fade;

fn solid(background: Option<Color>, text_color: Color, radius: f32) -> button::Style {
    button::Style {
        background: background.map(Background::Color),
        text_color,
        border: border::rounded(radius),
        ..button::Style::default()
    }
}

fn hot(status: button::Status) -> bool {
    matches!(status, button::Status::Hovered | button::Status::Pressed)
}

pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let (c, r) = (theme.colors(), theme.metrics().control_radius);
    match status {
        button::Status::Disabled => solid(Some(fade(c.accent, 0.35)), fade(c.on_accent, 0.7), r),
        status => solid(Some(fade(c.accent, if hot(status) { 0.88 } else { 1.0 })), c.on_accent, r),
    }
}

pub fn secondary(theme: &Theme, status: button::Status) -> button::Style {
    let (c, r) = (theme.colors(), theme.metrics().control_radius * 0.85);
    match status {
        button::Status::Disabled => solid(Some(fade(c.raised, 0.4)), fade(c.text, 0.4), r),
        status if hot(status) => solid(Some(c.track), c.text, r),
        // Has to stand out on a card and on the bare page alike.
        _ => solid(Some(c.raised), c.text, r),
    }
}

pub fn danger(theme: &Theme, status: button::Status) -> button::Style {
    let base = secondary(theme, status);
    let alpha = if status == button::Status::Disabled { 0.4 } else { 1.0 };
    button::Style { text_color: fade(theme.colors().critical, alpha), ..base }
}

pub fn link(theme: &Theme, status: button::Status) -> button::Style {
    let alpha = match status {
        button::Status::Disabled => 0.4,
        status if hot(status) => 0.8,
        _ => 1.0,
    };
    solid(None, fade(theme.colors().accent, alpha), theme.metrics().control_radius)
}

/// Low-emphasis text button.
pub fn ghost(theme: &Theme, status: button::Status) -> button::Style {
    let c = theme.colors();
    solid(None, if hot(status) { c.accent } else { c.text_dim }, 8.0)
}

pub fn nav(on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let (c, r) = (theme.colors(), theme.metrics().control_radius);
        let background = if on { Some(c.raised) } else { hot(status).then_some(c.card) };
        solid(background, c.text, r)
    }
}

pub fn rail_item(on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let (c, r) = (theme.colors(), theme.metrics().control_radius);
        match on {
            true => solid(Some(c.raised), c.accent, r),
            false => solid(None, if hot(status) { c.text } else { c.text_mute }, r),
        }
    }
}

pub fn tile(theme: &Theme, status: button::Status) -> button::Style {
    let (c, m) = (theme.colors(), theme.metrics());
    button::Style {
        border: Border { color: c.line, width: m.card_border, radius: m.card_radius.into() },
        ..solid(Some(if hot(status) { c.raised } else { c.card }), c.text, 0.0)
    }
}

/// Row that is clickable across its full width.
pub fn row(theme: &Theme, status: button::Status) -> button::Style {
    let c = theme.colors();
    match status {
        button::Status::Disabled => solid(None, fade(c.text, 0.45), 0.0),
        status => solid(hot(status).then(|| fade(c.raised, 0.45)), c.text, 0.0),
    }
}

pub fn segment(on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let (c, r) = (theme.colors(), theme.metrics().control_radius * 0.75);
        match on {
            true => solid(Some(c.raised), c.text, r),
            false => solid(None, if hot(status) { c.text } else { c.text_dim }, r),
        }
    }
}

/// Selectable card inside a grid of options.
pub fn option(on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let (c, r) = (theme.colors(), theme.metrics().control_radius);
        let background = match on {
            true => c.raised,
            false if hot(status) => fade(c.raised, 0.5),
            false => c.background,
        };
        solid(Some(background), c.text, r)
    }
}

/// Thumbnail with a selection ring.
pub fn thumbnail(on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let (c, r) = (theme.colors(), theme.metrics().control_radius);
        let color = match on {
            true => c.accent,
            false if hot(status) => c.text_mute,
            false => Color::TRANSPARENT,
        };
        button::Style {
            border: Border { color, width: 3.0, radius: (r + 2.0).into() },
            ..solid(Some(c.background), c.text, r)
        }
    }
}

/// A button that is a colour choice. Its fill is data and not themed.
pub fn color(fill: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let c = theme.colors();
        let radius = (theme.metrics().control_radius * 0.75).into();
        let ring = if hot(status) { c.text } else { Color::TRANSPARENT };
        button::Style {
            border: Border { color: ring, width: 2.0, radius },
            ..solid(Some(fill), Color::WHITE, 0.0)
        }
    }
}
