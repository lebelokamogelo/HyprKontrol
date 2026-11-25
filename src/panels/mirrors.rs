use iced::Task;

use super::{Effect, Panel};
use crate::sys::packages::{self, MirrorList};
use crate::ui::{Element, Kind, Ui};

/// pacman only falls back past the first few servers when they fail.
const SHOWN: usize = 8;

#[derive(Default)]
pub struct State {
    lists: Option<Vec<MirrorList>>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Vec<MirrorList>),
    Rank,
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::future(async { Message::Loaded(packages::mirrors()) })
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(lists) => {
                self.lists = Some(lists);
                Effect::none()
            }
            Message::Rank => Effect::session(Some(packages::rank_mirrors())),
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let Some(lists) = &self.lists else {
            return ui.panel("Mirrors", "", None, vec![ui.loading()]);
        };
        let rank = ui.button("Rank by speed", Kind::Primary, Some(Message::Rank));
        let mut cards: Vec<_> = lists
            .iter()
            .map(|list| {
                let ranked = list.ranked.map_or("never".to_owned(), packages::ago);
                let mut rows = vec![ui.value("Last ranked", ranked)];
                rows.extend(list.servers.iter().take(SHOWN).enumerate().map(|(index, server)| {
                    ui.row(
                        server.as_str(),
                        "",
                        if index == 0 { ui.badge("In use") } else { ui.small(format!("{}", index + 1)).into() },
                    )
                }));
                if list.servers.len() > SHOWN {
                    let more = list.servers.len() - SHOWN;
                    rows.push(ui.cell(ui.small(format!("And {more} more"))));
                }
                ui.group(list.label, rows)
            })
            .collect();
        let right = cards.split_off(cards.len().div_ceil(2));
        ui.panel(
            "Mirrors",
            "Servers that packages are downloaded from, fastest first",
            Some(rank),
            vec![ui.columns(ui.stack(cards), ui.stack(right))],
        )
    }

    fn summary(&self) -> String {
        self.lists
            .as_ref()
            .and_then(|lists| lists.first()?.ranked)
            .map(|time| format!("Ranked {}", packages::ago(time)))
            .unwrap_or_default()
    }
}
