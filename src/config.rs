//! Preferences of this program, stored in `config.toml`.
//!
//! Every field has a default and unknown keys are ignored, so a file written by
//! a newer or older release still loads.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::sys::{Error, Result, config_dir, edit};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub theme: String,
    /// Switch between the two themes below along with the desktop.
    pub follow_desktop: bool,
    pub light_theme: String,
    pub dark_theme: String,
    /// Panel shown at launch.
    pub start: String,
    /// Sections in display order. Sections left out are appended.
    pub sections: Vec<String>,
    /// Panels that are not listed. They stay reachable through search.
    pub hidden: Vec<String>,
    /// Quick controls on Home, in order.
    pub tiles: Vec<String>,
    /// Show the bar with the last command and Undo.
    pub receipt: bool,
}

impl Default for Config {
    fn default() -> Self {
        let owned = |values: &[&str]| values.iter().map(|v| (*v).to_owned()).collect();
        Self {
            theme: crate::theme::library::DEFAULT.to_owned(),
            follow_desktop: false,
            light_theme: "cachyos-light".to_owned(),
            dark_theme: "cachyos-dark".to_owned(),
            start: "home".to_owned(),
            sections: owned(&["home", "connect", "devices", "desktop", "cachyos", "system"]),
            hidden: Vec::new(),
            tiles: owned(&[
                "wifi",
                "bluetooth",
                "power",
                "firewall",
                "dnd",
                "night-light",
                "keep-awake",
                "scheduler",
            ]),
            receipt: true,
        }
    }
}

fn path() -> PathBuf {
    config_dir().join("hyprkontrol/config.toml")
}

impl Config {
    /// Loads the file, falling back to defaults when it is missing or broken.
    pub fn load() -> Self {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let text = toml::to_string(self)
            .map_err(|e| Error::new(format!("Could not encode preferences: {e}")))?;
        edit::write_atomic(&path(), &text)
    }

    /// The theme to use while the desktop is dark or light.
    pub fn active_theme(&self, desktop_dark: bool) -> &str {
        match (self.follow_desktop, desktop_dark) {
            (false, _) => &self.theme,
            (true, true) => &self.dark_theme,
            (true, false) => &self.light_theme,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_and_future_files_load() {
        let config: Config = toml::from_str("theme = \"glass\"\nfrom_the_future = 1\n").unwrap();
        assert_eq!(config.theme, "glass");
        assert_eq!(config.tiles, Config::default().tiles);
    }

    #[test]
    fn follows_the_desktop_when_asked() {
        let mut config = Config { theme: "glass".into(), ..Config::default() };
        assert_eq!(config.active_theme(false), "glass");
        config.follow_desktop = true;
        assert_eq!(config.active_theme(false), "cachyos-light");
        assert_eq!(config.active_theme(true), "cachyos-dark");
    }
}
