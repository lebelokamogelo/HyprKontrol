use std::collections::HashMap;
use std::path::PathBuf;

use iced::widget::{button, column, container, image, row, space};
use iced::{Center, ContentFit, Fill, Task};

use super::{Effect, Panel, Request};
use crate::icon::Icon;
use crate::sys::desktop::{self, Thumbnail, Wallpaper};
use crate::sys::{self, Error, op};
use crate::theme::{Tone, style};
use crate::ui::{Element, Kind, Ui};

/// Width of the selection ring around a preview.
const RING: f32 = 3.0;
const MIN_WIDTH: f32 = 188.0;
const GAP: f32 = 14.0;

#[derive(Default)]
pub struct State {
    current: Option<Wallpaper>,
    images: Vec<PathBuf>,
    previews: HashMap<PathBuf, image::Handle>,
    path: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Wallpaper, Vec<PathBuf>),
    Preview(PathBuf, Option<Thumbnail>),
    Use(PathBuf),
    Fit(&'static str),
    Path(String),
    UsePath,
}

const FITS: [(&str, &str); 4] =
    [("cover", "Fill the screen"), ("contain", "Show all of it"), ("tile", "Repeat"), ("fill", "Stretch")];

impl State {
    fn set(&self, edit: impl FnOnce(&mut Wallpaper)) -> Effect<Message> {
        let Some(before) = &self.current else { return Effect::none() };
        let mut after = before.clone();
        edit(&mut after);
        Effect::change(desktop::set_wallpaper(before, &after))
    }

    fn card<'a>(
        &'a self,
        ui: Ui<'a>,
        path: &'a PathBuf,
        width: f32,
        selected: bool,
    ) -> Element<'a, Message> {
        // Previews are cut to 16:9 already, so the box only has to match.
        let height = (width - 2.0 * RING) * 9.0 / 16.0;
        let preview: Element<'a, Message> = match self.previews.get(path) {
            Some(handle) => image(handle.clone())
                .width(Fill)
                .height(height)
                .content_fit(ContentFit::Fill)
                .border_radius(ui.metrics().control_radius)
                .into(),
            None => container(ui.icon(Icon::Image, Tone::Mute))
                .center_x(Fill)
                .center_y(height)
                .into(),
        };
        let name = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let label = row![
            ui.small(name).width(Fill).wrapping(iced::widget::text::Wrapping::None),
            if selected {
                ui.sized_icon(Icon::Check, Tone::Accent, 16.0)
            } else {
                space().into()
            },
        ]
        .align_y(Center)
        .padding([8, 10]);
        button(column![preview, label])
            .padding(RING)
            .width(Fill)
            .style(style::thumbnail(selected))
            .on_press(Message::Use(path.clone()))
            .into()
    }
}

/// Decodes previews one after another, on a thread that may block.
fn previews(paths: Vec<PathBuf>) -> Task<Message> {
    Task::batch(paths.into_iter().map(|path| {
        Task::perform(
            async move {
                let source = path.clone();
                let preview = tokio::task::spawn_blocking(move || desktop::thumbnail(&source));
                (path, preview.await.ok().flatten())
            },
            |(path, preview)| Message::Preview(path, preview),
        )
    }))
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(
            async { (desktop::wallpaper().await, desktop::wallpapers()) },
            |(current, images)| Message::Loaded(current, images),
        )
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(current, images) => {
                self.current = Some(current);
                let missing =
                    images.iter().filter(|p| !self.previews.contains_key(*p)).cloned().collect();
                self.images = images;
                Effect::task(previews(missing))
            }
            Message::Preview(path, Some(preview)) => {
                let handle =
                    image::Handle::from_rgba(preview.width, preview.height, preview.pixels);
                self.previews.insert(path, handle);
                Effect::none()
            }
            Message::Preview(_, None) => Effect::none(),
            Message::Use(path) => self.set(|paper| paper.path = path.display().to_string()),
            Message::Fit(fit) => self.set(|paper| fit.clone_into(&mut paper.fit)),
            Message::Path(path) => {
                self.path = path;
                Effect::none()
            }
            Message::UsePath => {
                let path = match self.path.trim().strip_prefix("~/") {
                    Some(rest) => sys::home().join(rest),
                    None => PathBuf::from(self.path.trim()),
                };
                if path.is_file() && desktop::is_image(&path) {
                    self.path.clear();
                    self.set(|paper| paper.path = path.display().to_string())
                } else {
                    let reason = format!("{} is not an image", path.display());
                    Effect::request(Request::Report(Error::new(reason)))
                }
            }
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let Some(current) = &self.current else {
            return ui.panel("Wallpaper", "", None, vec![ui.loading()]);
        };
        let fit = ui.choose(&FITS, &current.fit, Message::Fit);
        let width = ui.column_width(MIN_WIDTH, GAP, self.images.len());
        let cards = self
            .images
            .iter()
            .map(|path| self.card(ui, path, width, path.as_os_str() == current.path.as_str()))
            .collect();
        let custom = row![
            ui.field("Path to another image", &self.path, Message::Path)
                .on_submit(Message::UsePath),
            ui.button(
                "Use",
                Kind::Secondary,
                (!self.path.trim().is_empty()).then_some(Message::UsePath)
            ),
        ]
        .spacing(8)
        .align_y(Center);
        let mut body = vec![ui.group(
            "",
            vec![ui.row("Placement", "", fit), ui.cell(custom)],
        )];
        if self.images.is_empty() {
            body.push(ui.empty(
                Icon::Image,
                "No images found",
                "Put some in ~/Pictures/Wallpapers and they will show up here.",
                None,
            ));
        } else {
            body.push(ui.grid(MIN_WIDTH, ui.content, GAP, cards));
        }
        let shown = op::tilde(std::path::Path::new(&current.path));
        ui.panel("Wallpaper", shown, None, body)
    }

    fn summary(&self) -> String {
        let Some(current) = &self.current else { return String::new() };
        let stem = std::path::Path::new(&current.path).file_stem().unwrap_or_default();
        stem.to_string_lossy().into_owned()
    }
}
