//! Style functions, one per visual role rather than one per widget.
//!
//! Call sites name what an element is ("a nav entry", "a card") and the
//! function decides how the active theme draws it. Anything a theme author can
//! change must be read from [`Theme`] in here; a literal in one of these files
//! is a value nobody can theme.

mod button;
mod control;
mod surface;

use iced::Color;
use iced::widget::{svg, text};

pub use button::*;
pub use control::*;
pub use surface::*;

use super::Theme;

/// A colour role, resolved when drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Text,
    Dim,
    Mute,
    Accent,
    OnAccent,
    Live,
    Warning,
    Critical,
}

impl Tone {
    pub fn color(self, theme: &Theme) -> Color {
        let c = theme.colors();
        match self {
            Tone::Text => c.text,
            Tone::Dim => c.text_dim,
            Tone::Mute => c.text_mute,
            Tone::Accent => c.accent,
            Tone::OnAccent => c.on_accent,
            Tone::Live => c.live,
            Tone::Warning => c.warning,
            Tone::Critical => c.critical,
        }
    }
}

pub fn ink(tone: Tone) -> impl Fn(&Theme) -> text::Style {
    move |theme| text::Style { color: Some(tone.color(theme)) }
}

pub fn icon(tone: Tone) -> impl Fn(&Theme, svg::Status) -> svg::Style {
    move |theme, _| svg::Style { color: Some(tone.color(theme)) }
}
