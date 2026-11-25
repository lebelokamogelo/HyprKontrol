use iced::Task;
use iced::widget::column;

use super::{Effect, Panel};
use crate::sys::services::{self, Scope};
use crate::sys::storage::{self, Disk, Storage};
use crate::sys::{self, Result};
use crate::ui::{Element, Kind, Ui};

#[derive(Default)]
pub struct State {
    storage: Option<Result<Storage>>,
    trim: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Result<Storage>, bool),
    CleanPackages,
    CleanJournal,
    OpenCache,
    Trim(bool),
}

fn disk<'a>(ui: Ui<'a>, disk: &'a Disk) -> Element<'a, Message> {
    let kind = if disk.rotational { "Hard disk" } else { "Solid state" };
    let name = if disk.model.is_empty() { disk.name.as_str() } else { disk.model.as_str() };
    let mut rows =
        vec![ui.value(name, format!("{kind}, {}", sys::bytes(disk.size)))];
    rows.extend(disk.volumes.iter().map(|volume| {
        let shown = format!(
            "{} free of {} ({})",
            sys::bytes(volume.free()),
            sys::bytes(volume.size),
            volume.filesystem
        );
        ui.cell(ui.meter(volume.mount.as_str(), shown, volume.fraction()))
    }));
    ui.group("", rows)
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(
            async { tokio::join!(storage::load(), storage::trim_enabled()) },
            |(storage, trim)| Message::Loaded(storage, trim),
        )
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(storage, trim) => {
                self.storage = Some(storage);
                self.trim = trim;
                Effect::none()
            }
            Message::CleanPackages => Effect::run(storage::clean_packages()),
            Message::CleanJournal => Effect::run(storage::clean_journal()),
            Message::OpenCache => Effect::run(
                sys::Cmd::new("xdg-open").arg(sys::cache_dir().display().to_string()).detached(),
            ),
            Message::Trim(on) => {
                Effect::change(services::set_now(Scope::System, "fstrim.timer", on))
            }
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let storage = match &self.storage {
            None => return ui.panel("Storage", "", None, vec![ui.loading()]),
            Some(Err(error)) => {
                return ui.panel("Storage", "", None, vec![ui.failure(error.to_string())]);
            }
            Some(Ok(storage)) => storage,
        };
        let disks = column(storage.disks.iter().map(|d| disk(ui, d)))
            .spacing(ui.metrics().gap);
        let reclaim = ui.group(
            "Space you can get back",
            vec![
                ui.row(
                    "Package cache",
                    format!("{}. Keeps two versions for downgrades", sys::bytes(storage.package_cache)),
                    ui.button("Clean", Kind::Secondary, Some(Message::CleanPackages)),
                ),
                ui.row(
                    "System log",
                    format!("{}. Keeps the latest 200 MB", sys::bytes(storage.journal)),
                    ui.button("Trim", Kind::Secondary, Some(Message::CleanJournal)),
                ),
                ui.row(
                    "Application caches",
                    format!("{}. Review before deleting", sys::bytes(storage.user_cache)),
                    ui.button("Open", Kind::Secondary, Some(Message::OpenCache)),
                ),
            ],
        );
        let care = ui.group(
            "",
            vec![ui.toggle(
                "Weekly SSD trim",
                "Keeps write speed up over time",
                self.trim,
                Some(Message::Trim),
            )],
        );
        ui.panel(
            "Storage",
            self.summary(),
            None,
            vec![ui.columns(disks, ui.stack(vec![reclaim, care]))],
        )
    }

    fn summary(&self) -> String {
        match &self.storage {
            Some(Ok(storage)) => storage::root(&storage.disks)
                .map(|volume| format!("{} free", sys::bytes(volume.free())))
                .unwrap_or_default(),
            _ => String::new(),
        }
    }
}
