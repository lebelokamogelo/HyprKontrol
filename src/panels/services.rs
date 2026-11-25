use iced::Task;
use iced::widget::{column, row, space};
use iced::{Center, Fill};

use super::{Effect, Panel};
use crate::icon::Icon;
use crate::sys::services::{self, Scope, Service};
use crate::sys::Result;
use crate::theme::Tone;
use crate::ui::{Element, Kind, Ui};

/// Rows beyond this are reached by searching. A system has hundreds of units.
const LIMIT: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    Running,
    Enabled,
    Failed,
    All,
}

#[derive(Default)]
pub struct State {
    services: Option<Result<Vec<Service>>>,
    scope: Scope,
    filter: Filter,
    query: String,
    selected: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Scope, Result<Vec<Service>>),
    Scope(Scope),
    Filter(Filter),
    Query(String),
    Select(String),
    Active(String, bool),
    Enabled(String, bool),
    Restart(String),
    Logs(String),
}

impl State {
    fn matches(&self, service: &Service) -> bool {
        let query = self.query.trim().to_lowercase();
        // A search covers everything, since the user asked for it by name.
        if !query.is_empty() {
            return service.unit.to_lowercase().contains(&query)
                || service.description.to_lowercase().contains(&query);
        }
        match self.filter {
            Filter::Running => service.active,
            Filter::Enabled => service.state == "enabled",
            Filter::Failed => service.failed,
            Filter::All => true,
        }
    }

    fn entry<'a>(&'a self, ui: Ui<'a>, service: &'a Service) -> Element<'a, Message> {
        let (tone, state) = match (service.failed, service.active) {
            (true, _) => (Tone::Critical, "Failed"),
            (false, true) => (Tone::Live, "Running"),
            (false, false) => (Tone::Mute, "Stopped"),
        };
        let hint = if service.description.is_empty() {
            state.to_owned()
        } else {
            format!("{state}. {}", service.description)
        };
        let unit = || service.unit.clone();
        let line =
            ui.link(Icon::Services, tone, service.name(), hint, Message::Select(unit()));
        if self.selected.as_deref() != Some(service.unit.as_str()) {
            return line;
        }
        let start = if service.active {
            ui.button("Stop", Kind::Danger, Some(Message::Active(unit(), false)))
        } else {
            ui.button("Start", Kind::Primary, Some(Message::Active(unit(), true)))
        };
        let boot = match service.can_enable() {
            true => {
                let on = service.state == "enabled";
                let toggled = unit();
                row![
                    ui.switch(on, Some(move |on| Message::Enabled(toggled.clone(), on))),
                    ui.text("Start automatically"),
                ]
                .spacing(10)
                .align_y(Center)
                .into()
            }
            false => ui.badge(service.state.as_str()),
        };
        let controls = ui.actions(vec![
            start,
            ui.button(
                "Restart",
                Kind::Secondary,
                service.active.then(|| Message::Restart(unit())),
            ),
            ui.button("Show log", Kind::Secondary, Some(Message::Logs(unit()))),
            boot,
        ]);
        column![line, ui.cell(controls)].into()
    }
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        let scope = self.scope;
        Task::perform(services::list(scope), move |list| Message::Loaded(scope, list))
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(scope, services) => {
                // A slow answer for the other scope must not replace this one.
                if scope == self.scope {
                    self.services = Some(services);
                }
                Effect::none()
            }
            Message::Scope(scope) => {
                self.scope = scope;
                self.services = None;
                self.selected = None;
                Effect::task(self.load())
            }
            Message::Filter(filter) => {
                self.filter = filter;
                Effect::none()
            }
            Message::Query(query) => {
                self.query = query;
                Effect::none()
            }
            Message::Select(unit) => {
                self.selected = (self.selected.as_ref() != Some(&unit)).then_some(unit);
                Effect::none()
            }
            Message::Active(unit, on) => {
                Effect::change(services::set_active(self.scope, &unit, on))
            }
            Message::Enabled(unit, on) => {
                Effect::change(services::set_enabled(self.scope, &unit, on))
            }
            Message::Restart(unit) => Effect::run(services::restart(self.scope, &unit)),
            Message::Logs(unit) => Effect::checked(services::logs(self.scope, &unit)),
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let scope = ui.segments(
            [(Scope::System, "System"), (Scope::User, "Yours")],
            &self.scope,
            Message::Scope,
            true,
        );
        let filter = ui.segments(
            [
                (Filter::Running, "Running"),
                (Filter::Enabled, "Enabled"),
                (Filter::Failed, "Failed"),
                (Filter::All, "All"),
            ],
            &self.filter,
            Message::Filter,
            true,
        );
        let search =
            ui.search("Search services", "", |field| field, &self.query, Message::Query);
        let controls: Element<'a, Message> = if ui.content < 640.0 {
            column![search, filter].spacing(10).into()
        } else {
            row![search, space().width(Fill), filter].spacing(12).align_y(Center).into()
        };
        let mut body = vec![controls];
        match &self.services {
            None => body.push(ui.loading()),
            Some(Err(error)) => body.push(ui.failure(error.to_string())),
            Some(Ok(services)) => {
                let found: Vec<_> = services.iter().filter(|s| self.matches(s)).collect();
                let count = found.len();
                let mut rows: Vec<_> =
                    found.into_iter().take(LIMIT).map(|s| self.entry(ui, s)).collect();
                if count > LIMIT {
                    let more = count - LIMIT;
                    rows.push(ui.cell(ui.small(format!("And {more} more. Search to find them."))));
                }
                body.push(if rows.is_empty() {
                    ui.empty(Icon::Check, "Nothing here", "No service matches.", None)
                } else {
                    ui.group("", rows)
                });
            }
        }
        ui.panel("Services", self.summary(), Some(scope), body)
    }

    fn summary(&self) -> String {
        let Some(Ok(services)) = &self.services else { return String::new() };
        let failed = services.iter().filter(|service| service.failed).count();
        let running = services.iter().filter(|service| service.active).count();
        match failed {
            0 => format!("{running} running"),
            count => format!("{running} running, {count} failed"),
        }
    }
}
