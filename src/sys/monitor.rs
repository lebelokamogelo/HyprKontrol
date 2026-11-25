//! Display outputs.

use serde::Deserialize;

use super::Result;
use super::hypr::{self, Rule};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    pub name: String,
    pub make: String,
    pub model: String,
    pub width: u32,
    pub height: u32,
    pub refresh_rate: f64,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub transform: u8,
    pub disabled: bool,
    #[serde(default)]
    pub mirror_of: String,
    #[serde(default)]
    pub available_modes: Vec<String>,
}

/// A resolution and refresh rate, as Hyprland lists them.
#[derive(Debug, Clone, PartialEq)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    pub hz: f64,
}

impl Mode {
    /// Parses `1920x1080@60.04Hz`.
    pub fn parse(text: &str) -> Option<Self> {
        let (size, hz) = text.split_once('@')?;
        let (width, height) = size.split_once('x')?;
        Some(Self {
            width: width.parse().ok()?,
            height: height.parse().ok()?,
            hz: hz.trim_end_matches("Hz").parse().ok()?,
        })
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} × {} at {:.0} Hz", self.width, self.height, self.hz)
    }
}

impl Monitor {
    pub fn title(&self) -> String {
        if self.is_internal() {
            "Built-in display".to_owned()
        } else {
            // Panels without a readable name report the raw product code.
            let model = if self.model.starts_with("0x") { "" } else { &self.model };
            format!("{} {model}", self.make).trim().to_owned()
        }
    }

    pub fn is_internal(&self) -> bool {
        ["eDP", "LVDS", "DSI"].iter().any(|prefix| self.name.starts_with(prefix))
    }

    pub fn modes(&self) -> Vec<Mode> {
        let mut modes: Vec<_> =
            self.available_modes.iter().filter_map(|m| Mode::parse(m)).collect();
        modes.dedup_by(|a, b| a.to_string() == b.to_string());
        modes
    }

    /// The listed mode closest to what is active. The reported rate is measured
    /// and rarely equals the listed one exactly.
    pub fn mode(&self) -> Option<Mode> {
        self.modes()
            .into_iter()
            .filter(|mode| mode.width == self.width && mode.height == self.height)
            .min_by(|a, b| {
                (a.hz - self.refresh_rate).abs().total_cmp(&(b.hz - self.refresh_rate).abs())
            })
    }

    /// Size on the desktop, after scaling and rotation.
    pub fn logical(&self) -> (f64, f64) {
        let (w, h) = (f64::from(self.width) / self.scale, f64::from(self.height) / self.scale);
        if self.transform % 2 == 1 { (h, w) } else { (w, h) }
    }

    pub fn rule(&self) -> Rule {
        let mode = match self.mode() {
            Some(mode) => format!("{}x{}@{:.2}", mode.width, mode.height, mode.hz),
            None => "preferred".to_owned(),
        };
        Rule {
            output: self.name.clone(),
            mode,
            position: format!("{}x{}", self.x, self.y),
            scale: self.scale,
            transform: self.transform,
            mirror: if self.mirror_of == "none" { String::new() } else { self.mirror_of.clone() },
            disabled: self.disabled,
        }
    }
}

pub async fn load() -> Result<Vec<Monitor>> {
    Ok(serde_json::from_value(hypr::json("monitors all").await?)?)
}

/// Scales that divide a mode into whole logical pixels.
///
/// Hyprland refuses any other scale, and the refusal would leave the user with
/// an error for a choice the panel itself offered.
pub fn valid_scales(width: u32, height: u32) -> Vec<f64> {
    (60..=360)
        // Wayland expresses fractional scales in steps of 1/120.
        .map(|step| f64::from(step) / 120.0)
        .filter(|scale| {
            let whole = |pixels: u32| {
                let logical = f64::from(pixels) / scale;
                (logical - logical.round()).abs() < 1e-6
            };
            whole(width) && whole(height)
        })
        .filter(|scale| (f64::from(width) / scale) >= 640.0)
        .collect()
}

pub const TRANSFORMS: [&str; 8] =
    ["Normal", "90°", "180°", "270°", "Flipped", "Flipped 90°", "Flipped 180°", "Flipped 270°"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modes() {
        let mode = Mode::parse("1920x1080@60.04Hz").unwrap();
        assert_eq!(mode.to_string(), "1920 × 1080 at 60 Hz");
        assert_eq!(Mode::parse("nonsense"), None);
    }

    #[test]
    fn offers_only_scales_hyprland_accepts() {
        let scales = valid_scales(1920, 1080);
        assert!(scales.contains(&1.0) && scales.contains(&1.25) && scales.contains(&1.5));
        assert!(!scales.iter().any(|s| (s - 1.3).abs() < 1e-9));
    }

    #[test]
    fn rotation_swaps_the_logical_size() {
        let monitor: Monitor = serde_json::from_value(serde_json::json!({
            "name": "eDP-1", "make": "AU", "model": "0x3DA3", "width": 1920, "height": 1080,
            "refreshRate": 60.038, "x": 0, "y": 0, "scale": 1.25, "transform": 1, "disabled": false,
            "mirrorOf": "none", "availableModes": ["1920x1080@60.04Hz", "1920x1080@40.03Hz"]
        }))
        .unwrap();
        assert_eq!(monitor.logical(), (864.0, 1536.0));
        assert_eq!(monitor.rule().mode, "1920x1080@60.04");
        assert_eq!(monitor.title(), "Built-in display");
    }
}
