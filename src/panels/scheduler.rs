use std::time::Duration;

use iced::widget::{column, container, row};
use iced::{Fill, Length, Task};

use super::{Effect, Panel, Watch};
use crate::icon::Icon;
use crate::sys::packages::{self, MODES, Scheduler};
use crate::sys::probe::{self, Sample, Topology};
use crate::sys::services::{self, Scope};
use crate::ui::{Element, Kind, Ui, chart};

/// The scheduler state is read once per this many samples.
const REFRESH: u32 = 5;

#[derive(Default)]
pub struct State {
    scheduler: Option<Scheduler>,
    topology: Topology,
    sample: Sample,
    loads: Vec<f32>,
    ticks: u32,
    /// What the user has picked but not started yet. An empty name stands for
    /// the kernel default.
    picked: Option<(String, &'static str)>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Scheduler, Topology),
    Sampled(Box<Sample>),
    Pick(String),
    Mode(&'static str),
    Start,
    AtBoot(bool),
}

impl State {
    fn running(&self) -> Option<&(String, String)> {
        self.scheduler.as_ref()?.running.as_ref()
    }

    fn selection(&self) -> (String, &'static str) {
        if let Some(picked) = &self.picked {
            return picked.clone();
        }
        match self.running() {
            Some((name, mode)) => {
                let known = MODES.iter().find(|(key, _)| key == mode);
                (name.clone(), known.map_or("auto", |(key, _)| key))
            }
            None => (String::new(), "auto"),
        }
    }

    fn is_current(&self) -> bool {
        let (name, mode) = self.selection();
        match self.running() {
            Some((running, running_mode)) => *running == name && running_mode == mode,
            None => name.is_empty(),
        }
    }

    fn threads<'a>(&self, ui: Ui<'a>) -> Element<'a, Message> {
        let celsius =
            self.sample.celsius.map_or(String::new(), |c| format!("{c:.0} °C package"));
        let header = row![ui.caption("Load per thread").width(Fill), ui.small(celsius)];
        let threads = chart::Threads {
            loads: self.loads.clone(),
            groups: self.topology.groups.iter().map(|(_, count)| *count).collect(),
        };
        let kinds = self.topology.groups.len();
        let labels = self.topology.groups.iter().enumerate().map(|(index, (ghz, count))| {
            let name = match (kinds, index) {
                (1, _) => "All cores",
                (_, 0) => "Performance cores",
                (_, 1) => "Efficient cores",
                _ => "Low-power cores",
            };
            container(ui.small(format!("{name}, {ghz:.1} GHz")))
                .width(Length::FillPortion(*count as u16))
                .into()
        });
        let labels = row(labels).spacing(chart::GROUP_GAP);
        ui.card(column![header, threads.view(ui.d(72.0)), labels].spacing(12))
    }

    fn choices<'a>(&self, ui: Ui<'a>, scheduler: &'a Scheduler) -> Element<'a, Message> {
        let (selected, _) = self.selection();
        let names = std::iter::once("").chain(scheduler.available.iter().map(String::as_str));
        let pills = names
            .map(|name| {
                let label = if name.is_empty() { "Kernel default" } else { name };
                ui.pill(label, selected == name, Message::Pick(name.to_owned()))
            })
            .collect();
        // The grid sits in roughly half of the content on wide windows.
        let width = if ui.content < 760.0 { ui.content } else { ui.content * 0.57 };
        let grid = ui.grid(150.0, width - 28.0, 8.0, pills);
        let padding = iced::Padding { top: 0.0, right: 14.0, bottom: 14.0, left: 14.0 };
        ui.group("Choose a scheduler", vec![container(grid).padding(padding).into()])
    }

    fn details<'a>(&self, ui: Ui<'a>, scheduler: &Scheduler) -> Element<'a, Message> {
        let (name, mode) = self.selection();
        let current = self.is_current();
        let mut rows = Vec::new();
        if name.is_empty() {
            rows.push(ui.cell(ui.dim("The scheduler built into the kernel you are running.")));
        } else {
            let blurb = packages::scheduler_blurb(&name);
            let blurb = if blurb.is_empty() { "Experimental." } else { blurb };
            rows.push(ui.cell(ui.dim(blurb)));
            rows.push(ui.cell(ui.row_of_modes(mode)));
            rows.push(ui.toggle(
                "Start with the system",
                "Enables the scx_loader service",
                scheduler.at_boot,
                Some(Message::AtBoot),
            ));
            rows.push(ui.value("If it crashes", "The kernel default takes over"));
        }
        let label = match (current, name.is_empty()) {
            (true, _) => "Running".to_owned(),
            (false, true) => "Return to default".to_owned(),
            (false, false) => format!("Start {name}"),
        };
        let start = ui.button(label, Kind::Primary, (!current).then_some(Message::Start));
        rows.push(ui.row("", "", start));
        ui.group(if name.is_empty() { "Kernel default".to_owned() } else { name }, rows)
    }
}

impl<'a> Ui<'a> {
    /// Mode selector. Five modes fit in a row only when there is space for it.
    fn row_of_modes(self, mode: &'static str) -> Element<'a, Message> {
        if self.content < 400.0 {
            return self.choose(&MODES, mode, Message::Mode);
        }
        self.segments(MODES, &mode, Message::Mode, false)
    }
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::batch([
            Task::perform(packages::scheduler(), |scheduler| {
                Message::Loaded(scheduler, probe::topology())
            }),
            sample(),
        ])
    }

    fn refresh(&self) -> Task<Message> {
        if self.ticks.is_multiple_of(REFRESH) { self.load() } else { sample() }
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(scheduler, topology) => {
                self.scheduler = Some(scheduler);
                self.topology = topology;
                if self.is_current() {
                    self.picked = None;
                }
                Effect::none()
            }
            Message::Sampled(sample) => {
                self.ticks = self.ticks.wrapping_add(1);
                self.loads = sample
                    .threads
                    .iter()
                    .zip(&self.sample.threads)
                    .map(|(now, before)| now.load_since(*before))
                    .collect();
                // There is nothing to compare the first sample with.
                if self.loads.is_empty() {
                    self.loads = vec![0.0; sample.threads.len()];
                }
                self.sample = *sample;
                Effect::none()
            }
            Message::Pick(name) => {
                let (_, mode) = self.selection();
                self.picked = Some((name, mode));
                Effect::none()
            }
            Message::Mode(mode) => {
                let (name, _) = self.selection();
                self.picked = Some((name, mode));
                Effect::none()
            }
            Message::Start => {
                let (name, mode) = self.selection();
                let target = (!name.is_empty()).then_some((name.as_str(), mode));
                Effect::change(packages::set_scheduler(self.running(), target))
            }
            Message::AtBoot(on) => {
                Effect::change(services::set_enabled(Scope::System, "scx_loader.service", on))
            }
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let Some(scheduler) = &self.scheduler else {
            return ui.panel("Scheduler", "", None, vec![ui.loading()]);
        };
        let subtitle = match self.running() {
            Some((name, _)) => format!("{name} is running. Swap it live, no reboot needed."),
            None => "Kernel default is running. Swap it live, no reboot needed.".to_owned(),
        };
        let mut body = vec![self.threads(ui)];
        if !scheduler.supported {
            body.push(ui.empty(
                Icon::Chip,
                "This kernel has no sched-ext support",
                "Boot a CachyOS kernel to switch schedulers.",
                None,
            ));
        } else if scheduler.available.is_empty() {
            body.push(ui.empty(
                Icon::Chip,
                "No schedulers are installed",
                "Install scx-scheds and scx-tools to choose one.",
                None,
            ));
        } else {
            body.push(ui.split(4, self.choices(ui, scheduler), 3, self.details(ui, scheduler)));
        }
        ui.panel("Scheduler", subtitle, None, body)
    }

    fn summary(&self) -> String {
        match (&self.scheduler, self.running()) {
            (None, _) => String::new(),
            (Some(_), Some((name, _))) => name.clone(),
            (Some(_), None) => "Kernel default".to_owned(),
        }
    }

    fn watch(&self) -> Watch {
        Watch::Every(Duration::from_secs(1))
    }
}

fn sample() -> Task<Message> {
    Task::future(async { Message::Sampled(Box::new(probe::sample())) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picking_what_already_runs_is_not_a_change() {
        let mut state = State {
            scheduler: Some(Scheduler {
                running: Some(("lavd".into(), "gaming".into())),
                ..Scheduler::default()
            }),
            ..State::default()
        };
        assert!(state.is_current());
        state.picked = Some(("lavd".into(), "auto"));
        assert!(!state.is_current());
        state.picked = Some((String::new(), "auto"));
        assert!(!state.is_current());
    }
}
