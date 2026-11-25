use iced::overlay::menu;
use iced::widget::{container, pick_list, scrollable, slider, text_input, toggler};
use iced::{Border, Color, Shadow, Vector, border};

use crate::theme::Theme;
use crate::theme::color::fade;

pub fn toggle(theme: &Theme, status: toggler::Status) -> toggler::Style {
    let c = theme.colors();
    let (on, alpha) = match status {
        toggler::Status::Active { is_toggled } | toggler::Status::Hovered { is_toggled } => {
            (is_toggled, 1.0)
        }
        toggler::Status::Disabled { is_toggled } => (is_toggled, 0.4),
    };
    toggler::Style {
        background: fade(if on { c.accent } else { c.track }, alpha).into(),
        background_border_width: 0.0,
        background_border_color: Color::TRANSPARENT,
        foreground: fade(if on { c.on_accent } else { c.knob }, alpha).into(),
        foreground_border_width: 0.0,
        foreground_border_color: Color::TRANSPARENT,
        text_color: None,
        border_radius: (theme.metrics().control_radius == 0.0).then(|| border::radius(0)),
        padding_ratio: 0.14,
    }
}

pub fn slide(theme: &Theme, status: slider::Status) -> slider::Style {
    let c = theme.colors();
    let radius = if status == slider::Status::Dragged { 13.0 } else { 12.0 };
    slider::Style {
        rail: slider::Rail {
            backgrounds: (c.accent.into(), c.track.into()),
            width: 6.0,
            border: border::rounded(3),
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius },
            background: c.accent.into(),
            border_width: 4.0,
            border_color: Color { a: 1.0, ..c.card },
        },
    }
}

pub fn pick(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let c = theme.colors();
    let background = match status {
        pick_list::Status::Active => c.background,
        _ => c.raised,
    };
    pick_list::Style {
        text_color: c.text,
        placeholder_color: c.text_mute,
        handle_color: c.text_mute,
        background: background.into(),
        border: border::rounded(theme.metrics().control_radius * 0.85),
    }
}

pub fn menu(theme: &Theme) -> menu::Style {
    let c = theme.colors();
    let radius = theme.metrics().control_radius.into();
    menu::Style {
        // Menus float over content, so they stay opaque in translucent themes.
        background: Color { a: 1.0, ..c.card }.into(),
        border: Border { color: c.track, width: 1.0, radius },
        text_color: c.text,
        selected_text_color: c.text,
        selected_background: c.raised.into(),
        shadow: Shadow {
            color: Color { a: 0.35, ..Color::BLACK },
            offset: Vector::new(0.0, 6.0),
            blur_radius: 20.0,
        },
    }
}

/// Text field on the page background.
pub fn field(theme: &Theme, status: text_input::Status) -> text_input::Style {
    input(theme, status, theme.colors().card)
}

/// Text field inside a panel.
pub fn inset_field(theme: &Theme, status: text_input::Status) -> text_input::Style {
    input(theme, status, theme.colors().background)
}

fn input(theme: &Theme, status: text_input::Status, background: Color) -> text_input::Style {
    let c = theme.colors();
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: background.into(),
        border: Border {
            color: if focused { c.accent } else { Color::TRANSPARENT },
            width: if focused { 2.0 } else { 0.0 },
            radius: theme.metrics().control_radius.into(),
        },
        icon: c.text_mute,
        placeholder: c.text_mute,
        value: c.text,
        selection: fade(c.accent, 0.35),
    }
}

pub fn scroll(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let c = theme.colors();
    let idle = matches!(status, scrollable::Status::Active { .. });
    let rail = scrollable::Rail {
        background: None,
        border: border::rounded(3),
        scroller: scrollable::Scroller {
            background: fade(c.text_mute, if idle { 0.18 } else { 0.5 }).into(),
            border: border::rounded(3),
        },
    };
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: c.raised.into(),
            border: border::rounded(12),
            shadow: Shadow::default(),
            icon: c.text,
        },
    }
}
