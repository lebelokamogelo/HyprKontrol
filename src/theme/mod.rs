//! The theme handed to iced.
//!
//! iced ships a theme of its own, but its palette stops at six colours. Ours is
//! a full set of roles plus metrics, fonts and icons loaded at runtime, so
//! [`Theme`] implements the widget catalogs itself. Styles read from it on every
//! draw, which is why edits show up without rebuilding any state.

pub mod color;
mod font;
pub mod library;
pub mod spec;
pub mod style;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use iced::font::{Family, Weight};
use iced::overlay::menu;
use iced::theme::{Base, Mode, Palette};
use iced::widget::{
    button, container, pick_list, scrollable, slider, svg, text, text_input, toggler,
};
use iced::{Color, Font};

use crate::icon::Icon;
pub use library::Library;
pub use spec::{Colors, Metrics, Spec};
pub use style::Tone;

const DEFAULT_MONO: &str = "Adwaita Mono";

#[derive(Debug, Clone)]
pub struct Theme(Arc<Inner>);

#[derive(Debug)]
struct Inner {
    /// Some widgets cache layout and compare themes by name. The name carries a
    /// revision so that editing a theme in place still counts as a change.
    name: String,
    spec: Spec,
    dark: bool,
    icons: HashMap<Icon, svg::Handle>,
}

impl Theme {
    pub fn new(spec: Spec) -> Self {
        static REVISION: AtomicU32 = AtomicU32::new(0);
        font::register_weights(spec.fonts.ui.as_deref().unwrap_or(system_family()));
        font::register_weights(spec.fonts.mono.as_deref().unwrap_or(DEFAULT_MONO));
        let revision = REVISION.fetch_add(1, Ordering::Relaxed);
        let icons = spec
            .icons
            .iter()
            .filter_map(|(name, path)| Some((Icon::from_name(name)?, svg::Handle::from_path(path))))
            .collect();
        Self(Arc::new(Inner {
            name: format!("{}#{revision}", spec.id),
            dark: color::luminance(spec.colors.background) < 0.2,
            icons,
            spec,
        }))
    }

    pub fn spec(&self) -> &Spec {
        &self.0.spec
    }

    pub fn colors(&self) -> &Colors {
        &self.0.spec.colors
    }

    pub fn metrics(&self) -> &Metrics {
        &self.0.spec.metrics
    }

    pub fn icon(&self, icon: Icon) -> svg::Handle {
        self.0
            .icons
            .get(&icon)
            .cloned()
            .unwrap_or_else(|| icon.handle(self.0.spec.metrics.icon_stroke))
    }

    pub fn font(&self, weight: Weight) -> Font {
        let family = match &self.0.spec.fonts.ui {
            Some(name) => intern(name),
            None => system_family(),
        };
        Font { family: Family::Name(family), weight, ..Font::DEFAULT }
    }

    pub fn mono(&self) -> Font {
        let family = self.0.spec.fonts.mono.as_deref().unwrap_or(DEFAULT_MONO);
        Font::with_name(intern(family))
    }
}

static SYSTEM_FAMILY: OnceLock<&'static str> = OnceLock::new();

/// Records the desktop font. Only the first call has an effect.
pub fn set_system_family(name: &str) {
    let _ = SYSTEM_FAMILY.set(intern(name));
}

fn system_family() -> &'static str {
    SYSTEM_FAMILY.get().copied().unwrap_or("Adwaita Sans")
}

/// iced requires font family names to be `'static`.
///
/// Names come from theme files, so they are leaked, once per distinct value.
/// The set is bounded by the fonts a user tries in one session.
fn intern(name: &str) -> &'static str {
    static NAMES: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut names = NAMES.get_or_init(Default::default).lock().expect("font names poisoned");
    if let Some(known) = names.get(name) {
        return known;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.insert(leaked);
    leaked
}

impl Base for Theme {
    fn default(preference: Mode) -> Self {
        let library = Library::load(std::path::Path::new(""));
        let id = if preference == Mode::Light { "cachyos-light" } else { library::DEFAULT };
        Theme::new(library.get_or_default(id).clone())
    }

    fn mode(&self) -> Mode {
        if self.0.dark { Mode::Dark } else { Mode::Light }
    }

    fn base(&self) -> iced::theme::Style {
        iced::theme::Style {
            // Every surface paints its own background. Filling the window here
            // as well would double the alpha of translucent themes.
            background_color: Color::TRANSPARENT,
            text_color: self.colors().text,
        }
    }

    fn palette(&self) -> Option<Palette> {
        let c = self.colors();
        Some(Palette {
            background: c.background,
            text: c.text,
            primary: c.accent,
            success: c.live,
            warning: c.warning,
            danger: c.critical,
        })
    }

    fn name(&self) -> &str {
        &self.0.name
    }
}

/// All catalogs share one shape: the class is a closure, and styling calls it.
macro_rules! catalog {
    ($widget:ident, $default:expr $(, $status:ident)?) => {
        impl $widget::Catalog for Theme {
            type Class<'a> = $widget::StyleFn<'a, Self>;

            fn default<'a>() -> Self::Class<'a> {
                Box::new($default)
            }

            fn style(
                &self,
                class: &Self::Class<'_>
                $(, $status: $widget::Status)?
            ) -> $widget::Style {
                class(self $(, $status)?)
            }
        }
    };
}

catalog!(button, style::secondary, status);
catalog!(container, |_| container::Style::default());
catalog!(scrollable, style::scroll, status);
catalog!(slider, style::slide, status);
catalog!(svg, |_, _| svg::Style::default(), status);
catalog!(text, |_| text::Style::default());
catalog!(text_input, style::field, status);
catalog!(toggler, style::toggle, status);

impl menu::Catalog for Theme {
    type Class<'a> = menu::StyleFn<'a, Self>;

    fn default<'a>() -> <Self as menu::Catalog>::Class<'a> {
        Box::new(style::menu)
    }

    fn style(&self, class: &<Self as menu::Catalog>::Class<'_>) -> menu::Style {
        class(self)
    }
}

impl pick_list::Catalog for Theme {
    type Class<'a> = pick_list::StyleFn<'a, Self>;

    fn default<'a>() -> <Self as pick_list::Catalog>::Class<'a> {
        Box::new(style::pick)
    }

    fn style(
        &self,
        class: &<Self as pick_list::Catalog>::Class<'_>,
        status: pick_list::Status,
    ) -> pick_list::Style {
        class(self, status)
    }
}
