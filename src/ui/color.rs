//! A colour editor: four sliders and a hex field.

use iced::widget::{column, row};
use iced::{Center, Color};

use super::{Element, Ui};
use crate::theme::color::{Hex, from_hsv, to_hsv};

/// Colour being edited.
///
/// It is held as hue, saturation and value because RGB forgets the hue of greys:
/// dragging saturation to zero and back would always come back red.
#[derive(Debug, Clone, Default)]
pub struct ColorEdit {
    hue: f32,
    saturation: f32,
    value: f32,
    alpha: f32,
    hex: String,
}

#[derive(Debug, Clone)]
pub enum ColorMessage {
    Hue(f32),
    Saturation(f32),
    Value(f32),
    Alpha(f32),
    Hex(String),
    /// The drag ended.
    Done,
}

impl ColorEdit {
    pub fn new(color: Color) -> Self {
        let (hue, saturation, value) = to_hsv(color);
        Self { hue, saturation, value, alpha: color.a, hex: Hex(color).to_string() }
    }

    pub fn color(&self) -> Color {
        from_hsv(self.hue, self.saturation, self.value, self.alpha)
    }

    /// Returns the new colour, or `None` while the hex field is incomplete.
    pub fn update(&mut self, message: ColorMessage) -> Option<Color> {
        match message {
            ColorMessage::Hue(hue) => self.hue = hue,
            ColorMessage::Saturation(saturation) => self.saturation = saturation / 100.0,
            ColorMessage::Value(value) => self.value = value / 100.0,
            ColorMessage::Alpha(alpha) => self.alpha = alpha / 100.0,
            ColorMessage::Hex(text) => {
                let parsed = text.parse::<Hex>();
                self.hex = text;
                let Hex(color) = parsed.ok()?;
                (self.hue, self.saturation, self.value) = to_hsv(color);
                self.alpha = color.a;
                return Some(color);
            }
            ColorMessage::Done => return None,
        }
        self.hex = Hex(self.color()).to_string();
        Some(self.color())
    }

    pub fn view<'a, M: Clone + 'a>(
        &self,
        ui: Ui<'a>,
        with_alpha: bool,
        wrap: fn(ColorMessage) -> M,
    ) -> Element<'a, M> {
        let line = |label, max, value: f32, message: fn(f32) -> ColorMessage| {
            row![
                ui.small(label).width(72),
                ui.slider(
                    0.0..=max,
                    1.0,
                    value,
                    move |v| wrap(message(v)),
                    wrap(ColorMessage::Done)
                ),
            ]
            .spacing(12)
            .align_y(Center)
        };
        let mut lines = column![
            line("Hue", 360.0, self.hue, ColorMessage::Hue),
            line("Saturation", 100.0, self.saturation * 100.0, ColorMessage::Saturation),
            line("Brightness", 100.0, self.value * 100.0, ColorMessage::Value),
        ]
        .spacing(2);
        if with_alpha {
            lines = lines.push(line("Opacity", 100.0, self.alpha * 100.0, ColorMessage::Alpha));
        }
        let hex = row![
            ui.swatch(self.color(), None),
            ui.field("#rrggbb", &self.hex, move |text| wrap(ColorMessage::Hex(text)))
                .on_submit(wrap(ColorMessage::Done)),
        ]
        .spacing(12)
        .align_y(Center);
        column![lines, hex].spacing(10).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hue_survives_losing_saturation() {
        let mut edit = ColorEdit::new("#01ccff".parse::<Hex>().unwrap().0);
        let hue = edit.hue;
        edit.update(ColorMessage::Saturation(0.0));
        edit.update(ColorMessage::Saturation(100.0));
        assert_eq!(edit.hue, hue);
    }

    #[test]
    fn incomplete_hex_is_kept_but_not_applied() {
        let mut edit = ColorEdit::new(Color::BLACK);
        assert!(edit.update(ColorMessage::Hex("#12".into())).is_none());
        assert_eq!(edit.hex, "#12");
        assert!(edit.update(ColorMessage::Hex("#123456".into())).is_some());
    }
}
