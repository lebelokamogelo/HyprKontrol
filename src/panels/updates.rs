use std::time::SystemTime;

use iced::widget::row;
use iced::{Center, Fill, Task};

use super::{Effect, Panel, Request};
use crate::icon::Icon;
use crate::sys::packages::{self, Update};
use crate::sys::{Result, installed};
use crate::ui::{Element, Kind, Ui};

#[derive(Default)]
pub struct State {
    updates: Option<Vec<Update>>,
    checked: Option<SystemTime>,
    checking: bool,
    foreign: usize,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Vec<Update>, usize),
    Check,
    Checked(Result<Vec<Update>>),
    Upgrade,
    UpgradeAur,
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(
            async { tokio::join!(packages::cached_updates(), packages::foreign()) },
            |(updates, foreign)| Message::Loaded(updates, foreign),
        )
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(updates, foreign) => {
                self.checked = packages::last_check();
                self.updates = Some(updates);
                self.foreign = foreign;
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
                        self.checked = Some(SystemTime::now());
                        Effect::none()
                    }
                    Err(error) => Effect::request(Request::Report(error)),
                }
            }
            Message::Upgrade => Effect::session(Some(packages::upgrade())),
            Message::UpgradeAur => Effect::checked(packages::upgrade_aur()),
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
        let checked = self.checked.map_or_else(|| "never".to_owned(), packages::ago);
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
                    let versions = ui.change(update.from.as_str(), update.to.as_str());
                    ui.cell(
                        row![ui.strong(update.name.as_str()).width(Fill), versions]
                            .spacing(12)
                            .align_y(Center),
                    )
                })
                .collect();
            let count = updates.len();
            ui.group(format!("{count} {}", if count == 1 { "package" } else { "packages" }), rows)
        };
        let mut content = vec![body];
        if self.foreign > 0 && packages::upgrade_aur().is_some() {
            let label = match self.foreign {
                1 => "1 package from outside the repositories".to_owned(),
                count => format!("{count} packages from outside the repositories"),
            };
            let update = ui.button("Open terminal", Kind::Secondary, Some(Message::UpgradeAur));
            content.push(ui.group(
                "",
                vec![ui.row(
                    label,
                    "The AUR builds from source, so these update in a terminal",
                    update,
                )],
            ));
        }
        ui.panel("Updates", subtitle, Some(actions), content)
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
