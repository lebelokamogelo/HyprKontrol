//! Interface icons, embedded as strokes on a 24 px grid.
//!
//! They are drawn in white and recoloured by the renderer, so one source serves
//! every theme. A theme can replace any icon by its kebab-case name.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use iced::widget::svg;

macro_rules! icons {
    ($($name:ident => $body:literal),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Icon { $($name),* }

        impl Icon {
            pub const ALL: &[Icon] = &[$(Icon::$name),*];

            fn body(self) -> &'static str {
                match self { $(Icon::$name => $body),* }
            }
        }
    };
}

icons! {
    Home => r#"<path d="M4 11 12 4l8 7v8a1 1 0 0 1-1 1h-4v-6H9v6H5a1 1 0 0 1-1-1z"/>"#,
    Wifi => r#"<path d="M3 9.5a13 13 0 0 1 18 0M6.5 13a8 8 0 0 1 11 0M10 16.5a3 3 0 0 1 4 0M12 19.5v.01"/>"#,
    WifiMid => r#"<path d="M6.5 13a8 8 0 0 1 11 0M10 16.5a3 3 0 0 1 4 0M12 19.5v.01"/><path opacity=".35" d="M3 9.5a13 13 0 0 1 18 0"/>"#,
    WifiLow => r#"<path d="M10 16.5a3 3 0 0 1 4 0M12 19.5v.01"/><path opacity=".35" d="M3 9.5a13 13 0 0 1 18 0M6.5 13a8 8 0 0 1 11 0"/>"#,
    Ethernet => r#"<rect x="4" y="4" width="16" height="11" rx="2"/><path d="M12 15v5M8 20h8M8 8v3M12 8v3M16 8v3"/>"#,
    Bluetooth => r#"<path d="m7 8 10 8-5 4V4l5 4-10 8"/>"#,
    Vpn => r#"<circle cx="8" cy="14" r="4"/><path d="m11 11 8-7M16 7l2.5 2.5"/>"#,
    Hotspot => r#"<circle cx="12" cy="13" r="2"/><path d="M8.2 16.8a5.4 5.4 0 1 1 7.6 0M5.2 19.5a9.5 9.5 0 1 1 13.6 0"/>"#,
    Monitor => r#"<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8.5 20h7M12 16v4"/>"#,
    Windows => r#"<rect x="3.5" y="4" width="8" height="16" rx="2"/><rect x="14.5" y="4" width="6" height="7" rx="2"/><rect x="14.5" y="14" width="6" height="6" rx="2"/>"#,
    Cachy => r#"<path d="M12 3 20 7.5v9L12 21l-8-4.5v-9z"/><path d="m12.5 8-3 4.5h5l-3 4"/>"#,
    Sliders => r#"<path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2"/><circle cx="9" cy="17" r="2"/>"#,
    Battery => r#"<rect x="2.5" y="7.5" width="16" height="9" rx="2.5"/><path d="M21.5 10.5v3M6 10.5v3"/>"#,
    BatteryHealth => r#"<rect x="2.5" y="6.5" width="16.5" height="11" rx="2.8"/><path d="M21.8 10.5v3M5.8 12h2.4l1.5-2.6 2.2 5.2 1.5-2.6h2.4"/>"#,
    ShieldOff => r#"<path d="M12 3.5 19 6v5.5c0 1.6-.4 3-1 4.1M15.6 18.6A14 14 0 0 1 12 20.5c-4-1.5-7-4.5-7-9V7.2M4 4l16 16"/>"#,
    DiskFull => r#"<rect x="3.5" y="5" width="17" height="14" rx="2.5"/><path d="M3.5 13h17M16.5 16h.01M7 9h10"/>"#,
    Sound => r#"<path d="M4 10v4h3.5L12 18V6L7.5 10z"/><path d="M15.5 9a4.5 4.5 0 0 1 0 6M18 6.5a8 8 0 0 1 0 11"/>"#,
    Muted => r#"<path d="M4 10v4h3.5L12 18V6L7.5 10zM16 10l4.5 4.5M20.5 10 16 14.5"/>"#,
    Mic => r#"<rect x="9" y="3.5" width="6" height="11" rx="3"/><path d="M5.5 11.5a6.5 6.5 0 0 0 13 0M12 18v2.5"/>"#,
    Mouse => r#"<rect x="7" y="3.5" width="10" height="17" rx="5"/><path d="M12 7.5v3"/>"#,
    Keyboard => r#"<rect x="2.5" y="6.5" width="19" height="11" rx="2.5"/><path d="M6.5 10.5h.01M10 10.5h.01M13.5 10.5h.01M17 10.5h.01M8.5 14h7"/>"#,
    Disk => r#"<rect x="3.5" y="5" width="17" height="14" rx="2.5"/><path d="M3.5 13h17M16.5 16h.01"/>"#,
    Printer => r#"<path d="M7 9V4h10v5M7 17H4.5v-6a2 2 0 0 1 2-2h11a2 2 0 0 1 2 2v6H17"/><rect x="7" y="14" width="10" height="6"/>"#,
    Contrast => r#"<circle cx="12" cy="12" r="8.5"/><path d="M12 3.5v17M12 7h4.5M12 12h8M12 17h4.5"/>"#,
    Image => r#"<rect x="3.5" y="5" width="17" height="14" rx="2.5"/><path d="m4 16 5-5 4.5 4.5L16 13l4 4"/>"#,
    Wave => r#"<path d="M3 12c3-9 6-9 9 0s6 9 9 0"/>"#,
    Bell => r#"<path d="M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15zM10 20.5h4"/>"#,
    Lock => r#"<rect x="5" y="10.5" width="14" height="9.5" rx="2.5"/><path d="M8.5 10.5V8a3.5 3.5 0 0 1 7 0v2.5"/>"#,
    Apps => r#"<rect x="4" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="4" y="13.5" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.5"/>"#,
    Chip => r#"<rect x="6" y="6" width="12" height="12" rx="2"/><path d="M9.5 3v3M14.5 3v3M9.5 18v3M14.5 18v3M3 9.5h3M3 14.5h3M18 9.5h3M18 14.5h3"/>"#,
    Package => r#"<path d="M12 3.5 20 8v8l-8 4.5L4 16V8zM4 8l8 4.5L20 8M12 12.5v8"/>"#,
    Globe => r#"<circle cx="12" cy="12" r="8.5"/><path d="M3.5 12h17M12 3.5c5 5 5 12 0 17M12 3.5c-5 5-5 12 0 17"/>"#,
    Gpu => r#"<rect x="3" y="7" width="18" height="10" rx="2"/><circle cx="9" cy="12" r="2.2"/><path d="M14.5 10.5h3M14.5 13.5h3M6 17v3"/>"#,
    User => r#"<circle cx="12" cy="8" r="4"/><path d="M4.5 20.5a7.5 7.5 0 0 1 15 0"/>"#,
    Clock => r#"<circle cx="12" cy="12" r="8.5"/><path d="M12 7v5l3.5 2"/>"#,
    Language => r#"<path d="M4 6h9M8.5 4v2M5.5 13c4-2 5.5-4.5 6-7M6 8.5c1.2 2.5 3 4 5.5 5M12 20l4-9.5L20 20M13.3 17h5.4"/>"#,
    Shield => r#"<path d="M12 3.5 19 6v5.5c0 4.5-3 7.5-7 9-4-1.5-7-4.5-7-9V6z"/>"#,
    Services => r#"<rect x="4" y="4.5" width="16" height="6" rx="2"/><rect x="4" y="13.5" width="16" height="6" rx="2"/><path d="M8 7.5h.01M8 16.5h.01"/>"#,
    Info => r#"<circle cx="12" cy="12" r="8.5"/><path d="M12 11v5M12 8v.01"/>"#,
    Search => r#"<circle cx="11" cy="11" r="6.5"/><path d="m16 16 4.5 4.5"/>"#,
    ChevronRight => r#"<path d="m9.5 6 6 6-6 6"/>"#,
    ChevronDown => r#"<path d="m6 9.5 6 6 6-6"/>"#,
    Warning => r#"<circle cx="12" cy="12" r="8.5"/><path d="M12 7.6v5M12 16.2v.01"/>"#,
    Check => r#"<path d="m5 12.5 4.5 4.5L19 7.5"/>"#,
    Plus => r#"<path d="M12 5v14M5 12h14"/>"#,
    Close => r#"<path d="m6 6 12 12M18 6 6 18"/>"#,
    Refresh => r#"<path d="M20 5v5h-5M4 19v-5h5M5.5 9.5a7.5 7.5 0 0 1 13-1.5L20 10M4 14l1.5 2a7.5 7.5 0 0 0 13-1.5"/>"#,
    Sun => r#"<circle cx="12" cy="12" r="4"/><path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6 7 7M17 17l1.4 1.4M5.6 18.4 7 17M17 7l1.4-1.4"/>"#,
    Moon => r#"<path d="M19.5 14.5A8 8 0 0 1 9.5 4.5a8 8 0 1 0 10 10z"/>"#,
    Eye => r#"<path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z"/><circle cx="12" cy="12" r="2.8"/>"#,
    Dnd => r#"<circle cx="12" cy="12" r="8.5"/><path d="M8 12h8"/>"#,
    Gauge => r#"<path d="M4 17a8.5 8.5 0 1 1 16 0M12 14l3.5-4.5"/>"#,
    Leaf => r#"<path d="M5 19c0-9 5-13.5 14-14 0 9-4 14-10.5 14M5 19c2-4.5 5-7 9-9"/>"#,
    Bolt => r#"<path d="M13 3 5 13.5h6L10.5 21 19 10.5h-6z"/>"#,
    Plane => r#"<path d="M12 3c1 0 1.5 1.5 1.5 3v4l7 4.5v2l-7-2.2V18l2 1.5V21L12 20l-3.5 1v-1.5l2-1.5v-3.7l-7 2.2v-2l7-4.5V6c0-1.5.5-3 1.5-3z"/>"#,
    Headphones => r#"<path d="M4.5 15v-2.5a7.5 7.5 0 0 1 15 0V15"/><rect x="3.5" y="14" width="4" height="6" rx="1.8"/><rect x="16.5" y="14" width="4" height="6" rx="1.8"/>"#,
    Folder => r#"<path d="M3.5 18V6.5a1.5 1.5 0 0 1 1.5-1.5h4l2 2.5h8a1.5 1.5 0 0 1 1.5 1.5v9a1.5 1.5 0 0 1-1.5 1.5H5A1.5 1.5 0 0 1 3.5 18z"/>"#,
    Terminal => r#"<rect x="3" y="4.5" width="18" height="15" rx="2.5"/><path d="m7 9.5 3 2.5-3 2.5M12.5 15H17"/>"#,
}

impl Icon {
    /// Parses the kebab-case name used in theme files, such as `chevron-right`.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|icon| icon.name() == name)
    }

    /// Returns the kebab-case name of the icon.
    pub fn name(self) -> String {
        let mut name = String::new();
        for (index, c) in format!("{self:?}").chars().enumerate() {
            if c.is_ascii_uppercase() && index > 0 {
                name.push('-');
            }
            name.push(c.to_ascii_lowercase());
        }
        name
    }

    /// Returns the SVG handle for the given stroke width.
    ///
    /// Handles are cached because the renderer keys its rasterised copies on
    /// them; building a fresh one per frame would defeat that cache.
    pub fn handle(self, stroke: f32) -> svg::Handle {
        type Cache = Mutex<HashMap<(Icon, u32), svg::Handle>>;
        static CACHE: OnceLock<Cache> = OnceLock::new();
        let mut cache = CACHE.get_or_init(Default::default).lock().expect("icon cache poisoned");
        cache
            .entry((self, stroke.to_bits()))
            .or_insert_with(|| {
                let source = format!(
                    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="{stroke}" stroke-linecap="round" stroke-linejoin="round">{}</svg>"##,
                    self.body()
                );
                svg::Handle::from_memory(source.into_bytes())
            })
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        assert_eq!(Icon::ChevronRight.name(), "chevron-right");
        for icon in Icon::ALL {
            assert_eq!(Icon::from_name(&icon.name()), Some(*icon));
        }
    }
}
