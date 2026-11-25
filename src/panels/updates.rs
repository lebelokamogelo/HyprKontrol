use iced::widget::row;
use iced::{Center, Fill, Task};

use super::{Effect, Panel, Request};
use crate::icon::Icon;
use crate::sys::packages::{self, Update};
use crate::sys::{Result, installed};
use crate::theme::{Tone, style};
use crate::ui::{Element, Kind, Ui};

#[derive(Default)]
pub struct State {
    updates: Option<Vec<Update>>,
    checked: Option<String>,
    checking: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Vec<Update>),
    Check,
    Checked(Result<Vec<Update>>),
    Upgrade,
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(packages::cached_updates(), Message::Loaded)
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(updates) => {
                self.checked = packages::last_sync().map(packages::ago);
                self.updates = Some(updates);
                Effect::none()
            }
            Message::Check => {
                self.checking = true;
                Effect::task(Task::perform(packages::fresh_updates(), Message::Checked))
            }
            Message::Checked(result) => {
                self.checking = false;
                match result {
                    Ok(updates) => {
                        self.updates = Some(updates);
                        self.checked = Some("just now".to_owned());
                        Effect::none()
                    }
                    Err(error) => Effect::request(Request::Report(error)),
                }
            }
            Message::Upgrade => Effect::run(packages::upgrade()),
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let Some(updates) = &self.updates else {
            return ui.panel("Updates", "", None, vec![ui.loading()]);
        };
        let check = ui.button(
            if self.checking { "Checking" } else { "Check now" },
            Kind::Secondary,
            (!self.checking && installed("checkupdates")).then_some(Message::Check),
        );
        let upgrade = ui.button(
            "Update everything",
            Kind::Primary,
            (!updates.is_empty()).then_some(Message::Upgrade),
        );
        let actions = ui.actions(vec![check, upgrade]);
        let checked = self.checked.as_deref().unwrap_or("never");
        let subtitle = format!("Last checked {checked}");
        let body = if updates.is_empty() {
            ui.empty(
                Icon::Check,
                "Everything is up to date",
                "CachyOS is a rolling release, so check every few days.",
                None,
            )
        } else {
            let rows = updates
                .iter()
                .map(|update| {
                    let versions = row![
                        ui.small(update.from.as_str()),
                        ui.sized_icon(Icon::ChevronRight, Tone::Mute, 14.0),
                        ui.small(update.to.as_str()).style(style::ink(Tone::Live)),
                    ]
                    .spacing(6)
                    .align_y(Center);
                    ui.cell(
                        row![ui.strong(update.name.as_str()).width(Fill), versions]
                            .spacing(12)
                            .align_y(Center),
                    )
                })
                .collect();
            ui.group(format!("{} packages", updates.len()), rows)
        };
        ui.panel("Updates", subtitle, Some(actions), vec![body])
    }

    fn summary(&self) -> String {
        match self.updates.as_deref() {
            None => String::new(),
            Some([]) => "Up to date".to_owned(),
            Some([_]) => "1 update".to_owned(),
            Some(updates) => format!("{} updates", updates.len()),
        }
    }
}
