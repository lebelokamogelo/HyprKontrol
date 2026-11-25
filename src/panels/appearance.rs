use iced::widget::{button, row};
use iced::{Color, Task};

use super::{Effect, Panel};
use crate::icon::Icon;
use crate::sys::desktop::{self, Look};
use crate::theme::{Tone, style};
use crate::ui::{Element, Ui};

#[derive(Debug, Clone, Default)]
pub struct Data {
    look: Look,
    themes: Vec<String>,
    icons: Vec<String>,
    cursors: Vec<String>,
    fonts: Vec<String>,
    mono: Vec<String>,
}

#[derive(Default)]
pub struct State {
    data: Option<Data>,
    text_scale: Option<f32>,
    cursor_size: Option<f32>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Box<Data>),
    Dark(bool),
    Accent(&'static str),
    Theme(String),
    Icons(String),
    Cursor(String),
    CursorSize(f32),
    SaveCursorSize,
    Font(String),
    MonoFont(String),
    FontSize(u32),
    TextScale(f32),
    SaveTextScale,
}

async fn load() -> Data {
    let (look, (fonts, mono)) = tokio::join!(desktop::look(), desktop::fonts());
    Data {
        look,
        themes: desktop::themes(),
        icons: desktop::icon_themes(),
        cursors: desktop::cursor_themes(),
        fonts,
        mono,
    }
}

/// Size of a Pango font description, defaulting to the GTK default.
fn font_size(description: &str) -> u32 {
    description.rsplit_once(' ').and_then(|(_, size)| size.parse().ok()).unwrap_or(11)
}

impl State {
    fn set(&self, key: &str, get: fn(&Look) -> &str, value: &str) -> Effect<Message> {
        match &self.data {
            Some(data) => Effect::change(desktop::set(key, get(&data.look), value)),
            None => Effect::none(),
        }
    }

    fn accents<'a>(ui: Ui<'a>, look: &Look) -> Element<'a, Message> {
        let colors = desktop::ACCENTS.into_iter().map(|(name, hex)| {
            let [_, r, g, b] = hex.to_be_bytes();
            let mark = if look.accent == name {
                ui.sized_icon(Icon::Check, Tone::OnAccent, 16.0)
            } else {
                iced::widget::space().width(16).height(16).into()
            };
            button(mark)
                .padding(8)
                .style(style::color(Color::from_rgb8(r, g, b)))
                .on_press(Message::Accent(name))
                .into()
        });
        row(colors).spacing(8).wrap().vertical_spacing(8).into()
    }
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(load(), |data| Message::Loaded(Box::new(data)))
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        if let Message::Loaded(data) = message {
            self.data = Some(*data);
            self.text_scale = None;
            self.cursor_size = None;
            return Effect::none();
        }
        let Some(data) = &self.data else { return Effect::none() };
        let look = &data.look;
        match message {
            Message::Loaded(_) => Effect::none(),
            Message::Dark(dark) => Effect::change(desktop::set_dark(look, dark)),
            Message::Accent(name) => self.set("accent-color", |l| &l.accent, name),
            Message::Theme(name) => self.set("gtk-theme", |l| &l.gtk_theme, &name),
            Message::Icons(name) => self.set("icon-theme", |l| &l.icon_theme, &name),
            Message::Cursor(name) => Effect::change(desktop::set_cursor(
                (&look.cursor_theme, look.cursor_size),
                (&name, look.cursor_size),
            )),
            Message::CursorSize(size) => {
                self.cursor_size = Some(size);
                Effect::none()
            }
            Message::SaveCursorSize => match self.cursor_size {
                Some(size) => Effect::change(desktop::set_cursor(
                    (&look.cursor_theme, look.cursor_size),
                    (&look.cursor_theme, size as u32),
                )),
                None => Effect::none(),
            },
            Message::Font(family) => {
                let value = format!("{family} {}", font_size(&look.font));
                self.set("font-name", |l| &l.font, &value)
            }
            Message::MonoFont(family) => {
                let value = format!("{family} {}", font_size(&look.mono_font));
                self.set("monospace-font-name", |l| &l.mono_font, &value)
            }
            Message::FontSize(size) => {
                let value = format!("{} {size}", desktop::font_family(&look.font));
                self.set("font-name", |l| &l.font, &value)
            }
            Message::TextScale(scale) => {
                self.text_scale = Some(scale);
                Effect::none()
            }
            Message::SaveTextScale => match self.text_scale {
                Some(scale) => Effect::change(desktop::set(
                    "text-scaling-factor",
                    &look.text_scale.to_string(),
                    &scale.to_string(),
                )),
                None => Effect::none(),
            },
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let Some(data) = &self.data else {
            return ui.panel("Appearance", "", None, vec![ui.loading()]);
        };
        let look = &data.look;
        let mode = ui.segments(
            [(false, "Light"), (true, "Dark")],
            &look.dark,
            Message::Dark,
            true,
        );
        let select = |options: &Vec<String>, selected: &str, message: fn(String) -> Message| {
            ui.pick(options.clone(), Some(selected.to_owned()), message)
        };
        let size = self.cursor_size.unwrap_or(look.cursor_size as f32);
        let scale = self.text_scale.unwrap_or(look.text_scale);
        let sizes: Vec<u32> = (8..=20).collect();

        let mut style_rows = vec![
            ui.row("Accent", "", Self::accents(ui, look)),
            ui.row("GTK theme", "", select(&data.themes, &look.gtk_theme, Message::Theme)),
            ui.row("Icons", "", select(&data.icons, &look.icon_theme, Message::Icons)),
        ];
        if ui.content < 520.0 {
            // Nine swatches do not fit beside a label on a narrow tile.
            style_rows[0] = ui.cell(
                iced::widget::column![ui.text("Accent"), Self::accents(ui, look)].spacing(10),
            );
        }
        let cursor_rows = vec![
            ui.row("Theme", "", select(&data.cursors, &look.cursor_theme, Message::Cursor)),
            ui.slide(
                "Size",
                16.0..=64.0,
                4.0,
                size,
                format!("{size:.0} px"),
                Message::CursorSize,
                Message::SaveCursorSize,
            ),
        ];
        let font_rows = vec![
            ui.row(
                "Interface",
                "",
                select(&data.fonts, desktop::font_family(&look.font), Message::Font),
            ),
            ui.row(
                "Interface size",
                "",
                ui.pick(sizes, Some(font_size(&look.font)), Message::FontSize),
            ),
            ui.row(
                "Monospace",
                "",
                select(&data.mono, desktop::font_family(&look.mono_font), Message::MonoFont),
            ),
            ui.slide(
                "Text size",
                0.75..=2.0,
                0.05,
                scale,
                format!("{:.0}%", scale * 100.0),
                Message::TextScale,
                Message::SaveTextScale,
            ),
        ];
        ui.panel(
            "Appearance",
            "Applies to GTK. Other toolkits follow dark mode through the portal.",
            Some(mode),
            vec![ui.columns(
                ui.stack(vec![
                    ui.group("Style", style_rows),
                    ui.group("Cursor", cursor_rows),
                ]),
                ui.group("Fonts", font_rows),
            )],
        )
    }

    fn summary(&self) -> String {
        match &self.data {
            Some(data) => {
                let mode = if data.look.dark { "Dark" } else { "Light" };
                format!("{mode}, {}", data.look.gtk_theme)
            }
            None => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_size_falls_back() {
        assert_eq!(font_size("Adwaita Sans 13"), 13);
        assert_eq!(font_size("Inter"), 11);
    }
}
