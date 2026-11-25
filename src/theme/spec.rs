//! The theme file format, documented for authors in `docs/THEMES.md`.
//!
//! A value exists in three places: the file, where it is optional; the resolved
//! theme, where it is not; and the editor, which needs a label and a range. The
//! macros below declare each value once and derive all three. Adding a colour
//! means adding one line here and the same key to `themes/cachyos-dark.toml`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use iced::Color;
use serde::{Deserialize, Serialize};

use super::color::{Hex, contrast};

macro_rules! colors {
    ($($key:ident, $label:literal, $hint:literal;)*) => {
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct Colors { $(pub $key: Color),* }

        #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct ColorFile {
            $(#[serde(skip_serializing_if = "Option::is_none")] pub $key: Option<Hex>),*
        }

        impl ColorFile {
            fn apply(&self, colors: &mut Colors) {
                $(if let Some(Hex(value)) = self.$key { colors.$key = value; })*
            }

            fn diff(base: &Colors, colors: &Colors) -> Self {
                Self { $($key: (base.$key != colors.$key).then_some(Hex(colors.$key))),* }
            }
        }

        /// Every role in editor order.
        pub const ROLES: &[Role] = &[
            $(Role {
                label: $label,
                hint: $hint,
                get: |colors| colors.$key,
                set: |colors, value| colors.$key = value,
            }),*
        ];

        impl Default for Colors {
            fn default() -> Self {
                Self { $($key: Color::BLACK),* }
            }
        }
    };
}

macro_rules! metrics {
    ($($key:ident, $label:literal, $min:literal ..= $max:literal, $step:literal;)*) => {
        #[derive(Debug, Clone, Copy, Default, PartialEq)]
        pub struct Metrics { $(pub $key: f32),* }

        #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct MetricFile {
            $(#[serde(skip_serializing_if = "Option::is_none")] pub $key: Option<f32>),*
        }

        impl MetricFile {
            fn apply(&self, metrics: &mut Metrics) {
                $(if let Some(value) = self.$key { metrics.$key = value.clamp($min, $max); })*
            }

            fn diff(base: &Metrics, metrics: &Metrics) -> Self {
                Self { $($key: (base.$key != metrics.$key).then_some(metrics.$key)),* }
            }
        }

        pub const KNOBS: &[Knob] = &[
            $(Knob {
                label: $label,
                min: $min,
                max: $max,
                step: $step,
                get: |metrics| metrics.$key,
                set: |metrics, value| metrics.$key = value,
            }),*
        ];
    };
}

/// Editor metadata for one colour.
pub struct Role {
    pub label: &'static str,
    pub hint: &'static str,
    pub get: fn(&Colors) -> Color,
    pub set: fn(&mut Colors, Color),
}

/// Editor metadata for one number.
pub struct Knob {
    pub label: &'static str,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub get: fn(&Metrics) -> f32,
    pub set: fn(&mut Metrics, f32),
}

colors! {
    background, "Background", "Behind every panel";
    rail, "Rail", "Icon strip on the left";
    sidebar, "Sidebar", "List of panels in a section";
    card, "Card", "Groups of rows and tiles";
    raised, "Raised", "Selected and hovered items";
    track, "Track", "Empty part of sliders and switches";
    knob, "Knob", "Switch handle when off";
    line, "Line", "Dividers and borders";
    text, "Text", "Main text";
    text_dim, "Dim text", "Values and descriptions";
    text_mute, "Muted text", "Hints and captions";
    accent, "Accent", "Active controls";
    on_accent, "On accent", "Text and icons over the accent";
    live, "Live", "Readings, charts and commands";
    warning, "Warning", "Needs attention";
    critical, "Critical", "Errors and destructive actions";
}

metrics! {
    card_radius, "Card corners", 0.0 ..= 32.0, 1.0;
    control_radius, "Control corners", 0.0 ..= 24.0, 1.0;
    card_border, "Card border", 0.0 ..= 3.0, 1.0;
    density, "Density", 0.75 ..= 1.4, 0.05;
    gap, "Gap between cards", 6.0 ..= 40.0, 1.0;
    content_padding, "Content padding", 12.0 ..= 64.0, 1.0;
    rail_width, "Rail width", 60.0 ..= 120.0, 1.0;
    sidebar_width, "Sidebar width", 200.0 ..= 380.0, 2.0;
    text_size, "Text size", 11.0 ..= 18.0, 1.0;
    icon_size, "Icon size", 14.0 ..= 28.0, 1.0;
    icon_stroke, "Icon stroke", 1.0 ..= 2.5, 0.25;
}

impl Colors {
    /// Contrast of the combinations that carry text.
    pub fn readability(&self) -> impl Iterator<Item = (&'static str, f32)> + use<> {
        [
            ("Text on cards", self.text, self.card),
            ("Dim text on cards", self.text_dim, self.card),
            ("Muted text on cards", self.text_mute, self.card),
            ("Text on selected items", self.text, self.raised),
            ("Text on the accent", self.on_accent, self.accent),
        ]
        .into_iter()
        .map(|(label, text, surface)| (label, contrast(text, surface)))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fonts {
    /// Interface family. Unset follows the desktop font.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui: Option<String>,
    /// Family for commands and paths.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mono: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rail_labels: Option<bool>,
}

/// A theme as written on disk.
///
/// Anything left out is taken from `base`. Unknown keys are rejected so that a
/// typo is reported to the author instead of silently having no effect.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(default)]
    pub colors: ColorFile,
    #[serde(default)]
    pub fonts: Fonts,
    #[serde(default)]
    pub metrics: MetricFile,
    #[serde(default)]
    pub layout: LayoutFile,
    /// Icon name to SVG path, relative to the theme file.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub icons: BTreeMap<String, String>,
}

/// A fully resolved theme.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Spec {
    pub id: String,
    pub name: String,
    pub author: String,
    pub base: Option<String>,
    /// Source file, or `None` for a built-in theme.
    pub path: Option<PathBuf>,
    pub colors: Colors,
    pub fonts: Fonts,
    pub metrics: Metrics,
    pub rail_labels: bool,
    pub icons: BTreeMap<String, PathBuf>,
}

impl Spec {
    /// Layers `file` over `self`, taking over its identity.
    pub fn extend(mut self, id: &str, file: &File, path: Option<PathBuf>) -> Self {
        file.colors.apply(&mut self.colors);
        file.metrics.apply(&mut self.metrics);
        if file.fonts.ui.is_some() {
            self.fonts.ui.clone_from(&file.fonts.ui);
        }
        if file.fonts.mono.is_some() {
            self.fonts.mono.clone_from(&file.fonts.mono);
        }
        if let Some(value) = file.layout.rail_labels {
            self.rail_labels = value;
        }
        if let Some(dir) = path.as_deref().and_then(std::path::Path::parent) {
            for (name, svg) in &file.icons {
                self.icons.insert(name.clone(), dir.join(svg));
            }
        }
        Self {
            id: id.to_owned(),
            name: file.name.clone(),
            author: file.author.clone().unwrap_or_default(),
            base: file.base.clone(),
            path,
            ..self
        }
    }

    /// Reduces the theme to what differs from `base`.
    ///
    /// Saved themes stay short this way, and keep following their base when a
    /// later release adjusts it.
    pub fn to_file(&self, base: &Spec) -> File {
        let changed = |mine: &Option<String>, theirs: &Option<String>| {
            if mine == theirs { None } else { mine.clone() }
        };
        File {
            name: self.name.clone(),
            author: (!self.author.is_empty()).then(|| self.author.clone()),
            base: Some(base.id.clone()),
            colors: ColorFile::diff(&base.colors, &self.colors),
            fonts: Fonts {
                ui: changed(&self.fonts.ui, &base.fonts.ui),
                mono: changed(&self.fonts.mono, &base.fonts.mono),
            },
            metrics: MetricFile::diff(&base.metrics, &self.metrics),
            layout: LayoutFile {
                rail_labels: (self.rail_labels != base.rail_labels).then_some(self.rail_labels),
            },
            icons: self
                .icons
                .iter()
                .filter(|(name, path)| base.icons.get(*name) != Some(path))
                .map(|(name, path)| (name.clone(), path.display().to_string()))
                .collect(),
        }
    }
}
