//! Colour parsing and the arithmetic behind the theme editor.

use std::fmt;
use std::str::FromStr;

use iced::Color;
use serde::{Deserialize, Serialize};

/// A colour written as `#rgb`, `#rrggbb` or `#rrggbbaa`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Hex(pub Color);

impl FromStr for Hex {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || format!("\"{text}\" is not a colour, use #rrggbb or #rrggbbaa");
        let digits = text.trim().trim_start_matches('#');
        if !digits.is_ascii() || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        let expanded: String = match digits.len() {
            3 => digits.chars().flat_map(|c| [c, c]).collect(),
            6 | 8 => digits.to_owned(),
            _ => return Err(invalid()),
        };
        let channel = |at: usize| {
            u8::from_str_radix(&expanded[at..at + 2], 16).map(f32::from).map_err(|_| invalid())
        };
        Ok(Self(Color {
            r: channel(0)? / 255.0,
            g: channel(2)? / 255.0,
            b: channel(4)? / 255.0,
            a: if expanded.len() == 8 { channel(6)? / 255.0 } else { 1.0 },
        }))
    }
}

impl fmt::Display for Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b, a] = self.0.into_rgba8();
        write!(f, "#{r:02x}{g:02x}{b:02x}")?;
        if a < 255 {
            write!(f, "{a:02x}")?;
        }
        Ok(())
    }
}

impl TryFrom<String> for Hex {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl From<Hex> for String {
    fn from(color: Hex) -> Self {
        color.to_string()
    }
}

/// WCAG relative luminance, ignoring alpha.
pub fn luminance(color: Color) -> f32 {
    let linear = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
}

/// Lowest contrast at which body text counts as readable (WCAG level AA).
pub const READABLE: f32 = 4.5;

/// WCAG contrast ratio, from 1 (identical) to 21 (black on white).
pub fn contrast(a: Color, b: Color) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Returns `color` with its alpha scaled by `factor`.
pub fn fade(color: Color, factor: f32) -> Color {
    Color { a: color.a * factor, ..color }
}

/// Converts to hue (degrees), saturation and value.
pub fn to_hsv(color: Color) -> (f32, f32, f32) {
    let (max, min) = (color.r.max(color.g).max(color.b), color.r.min(color.g).min(color.b));
    let delta = max - min;
    let hue = if delta == 0.0 {
        0.0
    } else if max == color.r {
        60.0 * ((color.g - color.b) / delta).rem_euclid(6.0)
    } else if max == color.g {
        60.0 * ((color.b - color.r) / delta + 2.0)
    } else {
        60.0 * ((color.r - color.g) / delta + 4.0)
    };
    (hue, if max == 0.0 { 0.0 } else { delta / max }, max)
}

/// Converts from hue (degrees), saturation and value.
pub fn from_hsv(hue: f32, saturation: f32, value: f32, a: f32) -> Color {
    let channel = |n: f32| {
        let k = (n + hue / 60.0).rem_euclid(6.0);
        value - value * saturation * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    Color { r: channel(5.0), g: channel(3.0), b: channel(1.0), a }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_length() {
        assert_eq!("#fff".parse::<Hex>().unwrap().to_string(), "#ffffff");
        assert_eq!("01ccff".parse::<Hex>().unwrap().to_string(), "#01ccff");
        assert_eq!("#01ccff80".parse::<Hex>().unwrap().to_string(), "#01ccff80");
        for bad in ["", "#12", "#gggggg", "#12345", "rød"] {
            assert!(bad.parse::<Hex>().is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn contrast_matches_wcag() {
        assert!((contrast(Color::WHITE, Color::BLACK) - 21.0).abs() < 0.01);
        assert!((contrast(Color::BLACK, Color::BLACK) - 1.0).abs() < 0.01);
    }

    #[test]
    fn hsv_round_trips() {
        for hex in ["#01ccff", "#82dccc", "#f0c674", "#000000", "#ffffff", "#7a3b91"] {
            let Hex(color) = hex.parse().unwrap();
            let (h, s, v) = to_hsv(color);
            assert_eq!(Hex(from_hsv(h, s, v, 1.0)).to_string(), hex);
        }
    }
}
