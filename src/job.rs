//! A package operation, shown in place of the panel it was started from.
//!
//! The application runs at most one, since pacman locks its database. It lives
//! outside the panels so that it carries on while the user looks at others.

use iced::futures::{SinkExt, Stream};
use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, scrollable, space};
use iced::{Alignment, Center, Fill, Task, stream, window};

use crate::icon::Icon;
use crate::panels::Id;
use crate::sys::pacman::{self, Answer, Failure, Package, Question, Stage, Transaction};
use crate::sys::session::{self, Event, Keys};
use crate::sys::{Cmd, Session, packages};
use crate::theme::style::{self, Tone};
use crate::ui::{Element, Kind, Ui, Width, chart};

/// Output lines kept on screen. More would be slow to lay out on every change.
const LINES: usize = 200;

/// Packages listed before the rest is summed up.
const ROWS: usize = 60;

pub struct Job {
    pub panel: Id,
    session: Session,
    keys: Keys,
    transaction: Transaction,
    state: State,
    /// Whether the output is shown. `None` leaves it to the layout.
    open: Option<bool>,
    typed: String,
    /// The user has called it off, which is not a failure.
    stopped: bool,
    /// Seconds since the program has printed anything.
    quiet: u32,
    /// The window waits for this job before it closes.
    pub closing: Option<window::Id>,
    /// Modules of the running kernel were missing before this job.
    was_replaced: bool,
    restart: Restart,
}

#[derive(Debug, Clone, PartialEq)]
enum State {
    Running,
    Done,
    Failed(Failure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Restart {
    Unneeded,
    Needed,
    Confirm,
}

#[derive(Debug, Clone)]
pub enum Message {
    Event(Event),
    /// A second has passed.
    Tick,
    Type(String),
    Submit,
    /// Sends an answer. The flag says whether it calls the operation off.
    Answer(String, bool),
    Stop,
    Output(bool),
    KeepOpen,
    Close,
    Retry,
    Mirrors,
    Restart(bool),
    RestartNow,
}

/// What the application has to do after an update.
#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    Nothing,
    Finished,
    /// Called off, with what to tell the user.
    Stopped(String),
    Failed(String),
    Close,
    Retry(Session),
    Open(Id),
    Run(Cmd),
}

impl Job {
    pub fn start(panel: Id, session: Session) -> (Self, Task<Message>) {
        let (keys, receiver) = session::keys();
        let events = events(session.cmd.clone(), receiver);
        let job = Self {
            panel,
            session,
            keys,
            transaction: Transaction::default(),
            state: State::Running,
            open: None,
            typed: String::new(),
            stopped: false,
            quiet: 0,
            closing: None,
            was_replaced: packages::kernel_replaced(),
            restart: Restart::Unneeded,
        };
        (job, Task::run(events, Message::Event))
    }

    pub fn command(&self) -> String {
        self.session.cmd.to_string()
    }

    pub fn is_running(&self) -> bool {
        self.state == State::Running
    }

    /// Whether a question waits for an answer.
    pub fn asks(&self) -> bool {
        self.is_running() && self.transaction.question.is_some()
    }

    /// Calls the operation off, if that leaves the system as it was.
    pub fn stop(&mut self) -> bool {
        let can = self.is_running() && self.transaction.can_stop();
        if can {
            self.stopped = true;
            self.send(session::INTERRUPT);
        }
        can
    }

    fn send(&self, text: &str) {
        // Fails once the program is gone, which is reported by its own event.
        let _ = self.keys.send(text.to_owned());
    }

    fn answer(&mut self, text: &str) {
        self.send(&format!("{text}\n"));
        self.transaction.question = None;
        self.typed.clear();
    }

    pub fn update(&mut self, message: Message) -> Next {
        match message {
            Message::Event(Event::Output(bytes)) => {
                self.quiet = 0;
                self.transaction.feed(&bytes);
            }
            Message::Event(Event::Exited(code)) => return self.finish(code),
            Message::Event(Event::Failed(error)) => {
                let title = error.to_string();
                let hint = "Nothing was changed.".to_owned();
                self.state = State::Failed(Failure { title, hint, mirrors: false });
                self.closing = None;
                return Next::Failed(error.to_string());
            }
            Message::Tick => {
                self.quiet += 1;
                if self.is_running() && self.quiet >= 3 {
                    self.transaction.suspect();
                }
            }
            Message::Type(text) => self.typed = text,
            Message::Submit => {
                let typed = std::mem::take(&mut self.typed);
                self.answer(&typed);
            }
            Message::Answer(text, declines) => {
                self.stopped |= declines;
                self.answer(&text);
            }
            Message::Stop => {
                self.stop();
            }
            Message::Output(open) => self.open = Some(open),
            Message::KeepOpen => self.closing = None,
            Message::Close => return Next::Close,
            Message::Retry => return Next::Retry(self.session.clone()),
            Message::Mirrors => return Next::Open(Id::Mirrors),
            Message::Restart(confirm) => {
                self.restart = if confirm { Restart::Confirm } else { Restart::Needed };
            }
            Message::RestartNow => {
                self.restart = Restart::Needed;
                return Next::Run(Cmd::new("systemctl").arg("reboot"));
            }
        }
        Next::Nothing
    }

    fn finish(&mut self, code: Option<i32>) -> Next {
        let t = &self.transaction;
        let untouched = t.changes.is_empty();
        if code == Some(0) {
            let replaced = !self.was_replaced && packages::kernel_replaced();
            if t.restart || replaced {
                self.restart = Restart::Needed;
            }
            if self.session == packages::upgrade() {
                packages::record_check();
            }
            self.state = State::Done;
            return Next::Finished;
        }
        // pkexec exits with 126 when the password prompt is dismissed.
        if code == Some(126) && t.stage <= Stage::Preparing && untouched {
            return Next::Stopped("Cancelled at the password prompt".to_owned());
        }
        if self.stopped && untouched {
            let kept = t.packages.iter().any(|package| package.got > 0.0);
            let text = if kept {
                "Stopped. Nothing was changed, finished downloads are kept"
            } else {
                "Cancelled before anything changed"
            };
            return Next::Stopped(text.to_owned());
        }
        let failure = if self.stopped {
            Failure {
                title: "Stopped part way".to_owned(),
                hint: "Some packages had been changed by then. Run it again to finish.".to_owned(),
                mirrors: false,
            }
        } else {
            t.failure(code)
        };
        let reason = failure.title.clone();
        self.state = State::Failed(failure);
        self.closing = None;
        Next::Failed(reason)
    }

    /// Where things stand, in a few words.
    pub fn summary(&self) -> String {
        let t = &self.transaction;
        let steps = |verb: &str| match &t.step {
            Some(step) => format!("{verb} {} of {}", step.index, step.count),
            None => verb.to_owned(),
        };
        match &self.state {
            State::Done if self.restart != Restart::Unneeded => "Restart needed".to_owned(),
            State::Done => "Finished".to_owned(),
            State::Failed(_) => "Failed".to_owned(),
            State::Running if t.question.is_some() => "Waiting for you".to_owned(),
            State::Running => match t.stage {
                Stage::Waiting => "Waiting for password".to_owned(),
                Stage::Preparing if self.session.plain => self.session.doing.clone(),
                Stage::Preparing => "Preparing".to_owned(),
                Stage::Downloading => {
                    format!("Downloading, {:.0}%", (t.progress.fraction * 100.0).floor())
                }
                Stage::Checking => "Checking".to_owned(),
                Stage::Installing if self.removes() => steps("Removing"),
                Stage::Installing => steps("Installing"),
                Stage::Finishing => steps("Finishing"),
            },
        }
    }

    /// Whether packages are only taken away.
    fn removes(&self) -> bool {
        let packages = &self.transaction.packages;
        !packages.is_empty() && packages.iter().all(Package::leaves)
    }

    fn subtitle(&self) -> String {
        let t = &self.transaction;
        match &self.state {
            State::Done => "Finished".to_owned(),
            State::Failed(_) if t.changes.is_empty() => "Nothing was changed".to_owned(),
            State::Failed(_) => "Some packages may have been changed".to_owned(),
            State::Running => match (&t.question, t.stage) {
                (Some(question), _) if question.is_proceed() => {
                    "Nothing changes until you confirm".to_owned()
                }
                (Some(_), _) => "An answer is needed".to_owned(),
                (None, Stage::Waiting) => "Waiting for your password".to_owned(),
                (None, _) if self.stopped => "Stopping".to_owned(),
                (None, Stage::Preparing) => self.session.doing.clone(),
                (None, Stage::Downloading | Stage::Checking) => {
                    "Nothing is changed yet, so stopping is safe".to_owned()
                }
                (None, Stage::Installing) if self.removes() => {
                    "Removing, keep the computer on".to_owned()
                }
                (None, Stage::Installing) => "Installing, keep the computer on".to_owned(),
                (None, Stage::Finishing) => "Almost done, keep the computer on".to_owned(),
            },
        }
    }

    pub fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let t = &self.transaction;
        let action = match &self.state {
            State::Done => Some(ui.button("Done", Kind::Secondary, Some(Message::Close))),
            State::Failed(_) => Some(ui.actions(vec![
                ui.button("Close", Kind::Secondary, Some(Message::Close)),
                ui.button("Try again", Kind::Primary, Some(Message::Retry)),
            ])),
            // The card asks, and No is one of its answers.
            State::Running if t.question.as_ref().is_some_and(Question::is_proceed) => None,
            State::Running if t.can_stop() => {
                let label = if t.stage == Stage::Waiting { "Cancel" } else { "Stop" };
                Some(ui.button(label, Kind::Secondary, (!self.stopped).then_some(Message::Stop)))
            }
            State::Running => None,
        };

        let mut body = Vec::new();
        if self.closing.is_some() {
            body.push(self.waiting_to_close(ui));
        }
        match &self.state {
            State::Done => body.extend(self.finished(ui)),
            State::Failed(failure) => body.push(failed(ui, failure)),
            State::Running => body.push(match (&t.question, t.stage) {
                (Some(question), _) if question.is_proceed() => self.review(ui, question),
                (Some(question), _) => self.question(ui, question),
                (None, Stage::Waiting) => ui.group(
                    "",
                    vec![ui.icon_row(
                        Icon::Lock,
                        Tone::Accent,
                        "Type your password in the system prompt",
                        "Your desktop asks for it once. HyprKontrol never sees it.",
                        space(),
                    )],
                ),
                (None, Stage::Preparing) => self.preparing(ui),
                (None, _) => self.progress(ui),
            }),
        }

        let output = (t.stage != Stage::Waiting).then(|| self.output(ui));
        let packages = (!t.packages.is_empty()).then(|| self.packages(ui));
        match (packages, output) {
            (Some(packages), Some(output)) if ui.width == Width::Wide => {
                body.push(ui.columns(packages, output));
            }
            (packages, output) => body.extend(packages.into_iter().chain(output)),
        }
        ui.panel(self.panel.title(), self.subtitle(), action, body)
    }

    fn waiting_to_close<'a>(&self, ui: Ui<'a>) -> Element<'a, Message> {
        ui.group(
            "",
            vec![ui.icon_row(
                Icon::Clock,
                Tone::Warning,
                "The window closes when this is done",
                "Closing it now would leave packages half installed",
                ui.button("Keep open", Kind::Secondary, Some(Message::KeepOpen)),
            )],
        )
    }

    fn preparing<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let detail = match self.transaction.detail.as_str() {
            "" => self.session.doing.as_str(),
            detail => detail,
        };
        ui.group("", vec![ui.icon_row(Icon::Refresh, Tone::Accent, detail, "", space().width(0))])
    }

    /// pacman's question whether to go ahead, with what it would mean.
    fn review<'a>(&'a self, ui: Ui<'a>, question: &'a Question) -> Element<'a, Message> {
        let t = &self.transaction;
        let verb = if question.is_removal() { "Remove" } else { "Install" };
        let heading = match t.packages.as_slice() {
            [] => question.text().to_owned(),
            [package] => format!("{verb} {}?", package.name),
            packages => format!("{verb} these {} packages?", packages.len()),
        };
        let figure = |value: String, label: &'a str| -> Element<'a, Message> {
            column![ui.bold(value), ui.small(label)].spacing(1).width(Fill).into()
        };
        let mut figures = Vec::new();
        if let Some(size) = &t.download_size {
            figures.push(figure(round(size), "to download"));
        }
        if let Some(size) = &t.installed_size {
            figures.push(figure(round(size), "once installed"));
        }
        if let Some(size) = &t.removed_size {
            figures.push(figure(round(size), "will be freed"));
        }
        // Without older versions to replace, this repeats the installed size.
        if let Some(size) = &t.net_size {
            figures.push(match Disk::of(size) {
                Disk::Same => figure("No change".to_owned(), "in disk use"),
                Disk::Less(size) => figure(size, "less disk used"),
                Disk::More(size) => figure(format!("+{size}"), "more disk used"),
            });
        }
        let mut content = column![ui.heading(heading)].spacing(16);
        if !figures.is_empty() {
            content = content.push(row(figures).spacing(12));
        }
        let (no, yes) = match question.is_removal() {
            true => ("No", "Yes, remove"),
            false => ("No", "Yes, install"),
        };
        let buttons = ui.actions(vec![
            ui.button(no, Kind::Secondary, Some(Message::Answer("n".into(), true))),
            ui.button(yes, Kind::Primary, Some(Message::Answer("y".into(), false))),
        ]);
        ui.card(content.push(ui.divider()).push(asked(ui, question, buttons)))
    }

    /// Any other question.
    fn question<'a>(&'a self, ui: Ui<'a>, question: &'a Question) -> Element<'a, Message> {
        let mut content = column![ui.heading(question.text())].spacing(16);
        let context: Vec<_> = question.context.iter().map(String::as_str).collect();
        match &question.answer {
            Answer::YesNo(suggestion) => {
                if !context.is_empty() {
                    content =
                        content.push(ui.mono(context.join("\n")).style(style::ink(Tone::Dim)));
                }
                let kind = |yes: bool| {
                    if yes == *suggestion { Kind::Primary } else { Kind::Secondary }
                };
                let answer = |text: &str| Some(Message::Answer(text.to_owned(), false));
                let buttons = ui.actions(vec![
                    ui.button("No", kind(false), answer("n")),
                    ui.button("Yes", kind(true), answer("y")),
                ]);
                content = content.push(ui.divider()).push(asked(ui, question, buttons));
                ui.card(content)
            }
            Answer::Number(choices, default) if !choices.is_empty() => {
                let title = context.first().copied().unwrap_or("Choose one");
                let rows = choices
                    .iter()
                    .map(|choice| {
                        // One of them is taken if the question is just confirmed.
                        let suggested = choice.number == *default;
                        let (tone, kind) = match suggested {
                            true => (Tone::Accent, Kind::Primary),
                            false => (Tone::Mute, Kind::Secondary),
                        };
                        let answer = Message::Answer(choice.number.clone(), false);
                        ui.icon_row(
                            Icon::Package,
                            tone,
                            choice.name.as_str(),
                            format!("From {}", choice.origin),
                            ui.button("Choose", kind, Some(answer)),
                        )
                    })
                    .collect();
                ui.group(title.trim_start_matches(":: "), rows)
            }
            Answer::Number(..) | Answer::Text => {
                if !context.is_empty() {
                    content =
                        content.push(ui.mono(context.join("\n")).style(style::ink(Tone::Dim)));
                }
                let field = ui
                    .field("Type an answer", &self.typed, Message::Type)
                    .on_submit(Message::Submit);
                let send = ui.button("Send", Kind::Primary, Some(Message::Submit));
                ui.card(content.push(row![field, send].spacing(8).align_y(Center)))
            }
        }
    }

    fn progress<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let t = &self.transaction;
        let weights: Vec<f32> = match t.packages.iter().map(|package| package.size).collect() {
            Some::<Vec<f32>>(sizes) if sizes.iter().all(|size| *size > 0.0) => sizes,
            _ => vec![1.0; t.packages.len()],
        };
        let parts = |done: fn(&Package) -> f32| -> Vec<(f32, f32)> {
            weights.iter().copied().zip(t.packages.iter().map(done)).collect()
        };
        let counted = |index: usize, count: usize| -> Element<'a, Message> {
            let raise = iced::Padding::ZERO.bottom(ui.metrics().text_size * 0.5);
            row![
                ui.figure(index.to_string()),
                container(ui.heading(format!("of {count}"))).padding(raise)
            ]
            .spacing(8)
            .align_y(iced::Bottom)
            .into()
        };

        let step = t.step.as_ref();
        let fraction = step.and_then(|step| step.fraction).unwrap_or(0.0);
        let (figure, headline, detail, parts) = match t.stage {
            Stage::Downloading => {
                let total = t.download_size.as_deref().and_then(pacman::size);
                let headline = match total {
                    Some(total) => format!("{:.1} of {total:.1} MiB", t.progress.got),
                    None => format!("{:.1} MiB", t.progress.got),
                };
                let left = t.progress.left.map(|left| format!(", {}", time_left(left)));
                let speed = t.progress.speed.as_str();
                let detail = format!("{speed}{}", left.unwrap_or_default());
                let percent = (t.progress.fraction * 100.0).floor();
                (ui.percentage(percent), headline, detail, parts(|package| package.fetched))
            }
            Stage::Installing => {
                let (index, count) = step.map_or((0, t.packages.len()), |s| (s.index, s.count));
                let name = step.and_then(|step| step.label.split_once(' ')).map_or("", |s| s.1);
                let package = t.packages.iter().find(|package| package.name == name);
                let detail = match package.map(|p| (p.from.as_deref(), p.to.as_deref())) {
                    Some((Some(from), Some(to))) => format!("{from} to {to}"),
                    Some((None, Some(to))) => to.to_owned(),
                    Some((Some(from), None)) => from.to_owned(),
                    _ => String::new(),
                };
                let figure = counted(index, count);
                (figure, name.to_owned(), detail, parts(|package| package.applied))
            }
            Stage::Finishing => {
                let (index, count) = step.map_or((0, 0), |step| (step.index, step.count));
                let label = step.map_or("", |step| step.label.trim_end_matches('.'));
                let parts = (1..=count).map(|n| (1.0, if n < index { 1.0 } else { 0.0 }));
                let mut parts: Vec<_> = parts.collect();
                // Hooks do not report how far they are. Half is the honest guess.
                if let Some(current) = index.checked_sub(1).and_then(|index| parts.get_mut(index)) {
                    current.1 = 0.5;
                }
                (counted(index, count), label.to_owned(), t.detail.clone(), parts)
            }
            _ => {
                let label = step.map_or("Checking packages", |step| step.label.as_str());
                let headline = format!("{}{}", label[..1].to_uppercase(), &label[1..]);
                let detail = "Before anything is changed".to_owned();
                (ui.percentage((fraction * 100.0).floor()), headline, detail, vec![(1.0, fraction)])
            }
        };

        let clipped = |text: iced::widget::Text<'a, crate::theme::Theme>| {
            container(text.wrapping(Wrapping::None)).clip(true)
        };
        let labels = column![clipped(ui.bold(headline)), clipped(ui.small(detail))]
            .spacing(1)
            .align_x(Alignment::End);
        let labels = container(labels).width(Fill).align_x(Alignment::End).clip(true);
        let top = row![figure, labels].spacing(16).align_y(iced::Bottom);
        let content = column![top, chart::Parts { parts }.view(), self.phases(ui)];
        ui.card(content.spacing(14))
    }

    /// The way ahead, as the labels under the gauge.
    fn phases<'a>(&self, ui: Ui<'a>) -> Element<'a, Message> {
        let t = &self.transaction;
        let mut phases = Vec::new();
        if t.download_size.is_some() {
            phases.push(("Download", Stage::Downloading));
        }
        phases.push((if self.removes() { "Remove" } else { "Install" }, Stage::Installing));
        phases.push(("Finish", Stage::Finishing));
        let now = match t.stage {
            Stage::Checking => Stage::Installing,
            stage => stage,
        };
        let last = phases.len() - 1;
        let labels = phases.into_iter().enumerate().map(|(index, (label, stage))| {
            let (tone, state) = match stage.cmp(&now) {
                std::cmp::Ordering::Equal => (Tone::Accent, "now"),
                std::cmp::Ordering::Less => (Tone::Text, "done"),
                std::cmp::Ordering::Greater => (Tone::Mute, "next"),
            };
            let align = match index {
                0 => Alignment::Start,
                index if index == last => Alignment::End,
                _ => Alignment::Center,
            };
            let label = ui.bold(label).style(style::ink(tone));
            let label = column![label, ui.small(state)].spacing(1).align_x(align);
            container(label).width(Fill).align_x(align).into()
        });
        row(labels).spacing(12).into()
    }

    fn finished<'a>(&'a self, ui: Ui<'a>) -> Vec<Element<'a, Message>> {
        let t = &self.transaction;
        let title = match t.summary() {
            Some(summary) => summary,
            None if t.has_nothing_to_do() => "There was nothing to do".to_owned(),
            None => self.session.done.clone(),
        };
        let mut facts = Vec::new();
        if let Some(size) = &t.download_size {
            facts.push(format!("{} downloaded", round(size)));
        }
        match t.net_size.as_deref().map(Disk::of) {
            Some(Disk::Same) => {}
            Some(Disk::Less(size)) => facts.push(format!("{size} of disk space freed")),
            Some(Disk::More(size)) => facts.push(format!("{size} more disk used")),
            None => {
                let freed = t.removed_size.as_deref().map(round);
                facts.extend(freed.map(|size| format!("{size} of disk space freed")));
            }
        }
        let mut head = column![
            ui.sized_icon(Icon::Check, Tone::Live, 36.0),
            ui.heading(title).align_x(Center),
        ]
        .spacing(8)
        .align_x(Center)
        .max_width(420);
        if !facts.is_empty() {
            head = head.push(ui.small(facts.join(", ")).align_x(Center));
        }
        let mut rows: Vec<Element<'a, Message>> =
            vec![container(head).padding([28, 20]).center_x(Fill).into()];
        let restart = |label: &'a str, hint: &'a str, control| {
            ui.icon_row(Icon::Refresh, Tone::Mute, label, hint, control)
        };
        match self.restart {
            Restart::Unneeded => {}
            Restart::Needed => rows.push(restart(
                "Restart to use what was updated",
                "The old versions keep running until then",
                ui.button("Restart", Kind::Secondary, Some(Message::Restart(true))),
            )),
            Restart::Confirm => rows.push(restart(
                "Restart now?",
                "Open programs are closed",
                ui.actions(vec![
                    ui.button("Cancel", Kind::Secondary, Some(Message::Restart(false))),
                    ui.button("Restart now", Kind::Danger, Some(Message::RestartNow)),
                ]),
            )),
        }
        let mut body = vec![ui.group("", rows)];
        let notes: Vec<_> = t
            .errors()
            .into_iter()
            .map(|text| (Tone::Warning, text))
            .chain(t.warnings().into_iter().map(|text| (Tone::Mute, text)))
            .map(|(tone, text)| ui.icon_row(Icon::Warning, tone, text, "", space().width(0)))
            .collect();
        if !notes.is_empty() {
            body.push(ui.group("pacman noted", notes));
        }
        body
    }

    fn packages<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let t = &self.transaction;
        let count = t.packages.len();
        let mut packages: Vec<_> = t.packages.iter().collect();
        // A long list scrolls, so what moves goes to the top.
        if count > 12 && self.is_running() && t.question.is_none() {
            packages.sort_by_key(|package| match t.stage {
                Stage::Downloading => activity(package.fetched, package.got > 0.0),
                Stage::Installing => activity(package.applied, package.applied > 0.0),
                _ => 0,
            });
        }
        let mut rows: Vec<_> =
            packages.into_iter().take(ROWS).map(|package| self.package(ui, package)).collect();
        if count > ROWS {
            rows.push(ui.cell(ui.small(format!("And {} more", count - ROWS))));
        }
        ui.group(format!("{count} {}", if count == 1 { "package" } else { "packages" }), rows)
    }

    fn package<'a>(&self, ui: Ui<'a>, package: &'a Package) -> Element<'a, Message> {
        let t = &self.transaction;
        let narrow = ui.width == Width::Narrow;
        let name = container(ui.strong(package.name.as_str()).wrapping(Wrapping::None))
            .width(Fill)
            .clip(true);
        let mut line = row![name].spacing(12).align_y(Center);
        let note = |text: String, tone: Tone| {
            ui.small(text).style(style::ink(tone)).wrapping(Wrapping::None)
        };
        let check = || ui.sized_icon(Icon::Check, Tone::Live, 18.0);
        let percent = |fraction: f32| {
            let text = format!("{:.0}%", (fraction * 100.0).floor());
            ui.strong(text).width(40).align_x(Alignment::End).wrapping(Wrapping::None)
        };
        let mut bar = None;
        let running = self.is_running() && t.question.is_none();
        match (&self.state, t.stage) {
            (State::Running, Stage::Downloading) if running && t.download_size.is_some() => {
                bar = Some(package.fetched);
                line = match package.size {
                    _ if package.failed => line.push(note("Failed".to_owned(), Tone::Critical)),
                    _ if package.fetched >= 1.0 => {
                        line.push(note("Downloaded".to_owned(), Tone::Mute)).push(check())
                    }
                    Some(size) if !narrow => {
                        let text = format!("{:.1} of {size:.1} MiB", package.got);
                        line.push(note(text, Tone::Mute)).push(percent(package.fetched))
                    }
                    _ => line.push(percent(package.fetched)),
                };
            }
            (State::Running, Stage::Installing) if running => {
                let (done, doing) = if package.leaves() {
                    ("Removed", "Removing")
                } else {
                    ("Installed", "Installing")
                };
                bar = Some(package.applied);
                line = if package.applied >= 1.0 {
                    line.push(note(done.to_owned(), Tone::Mute)).push(check())
                } else if package.applied > 0.0 {
                    let doing = note(doing.to_owned(), Tone::Mute);
                    line.push(doing).push(percent(package.applied))
                } else {
                    line.push(note("Waiting".to_owned(), Tone::Mute))
                };
            }
            (State::Running, Stage::Finishing) if running => {
                let done = if package.leaves() { "Removed" } else { "Installed" };
                line = line.push(note(done.to_owned(), Tone::Mute)).push(check());
            }
            (State::Failed(_), _) if t.changes.is_empty() => {
                line = if package.failed {
                    line.push(note("Failed".to_owned(), Tone::Critical))
                } else if package.fetched >= 1.0 {
                    line.push(note("Downloaded".to_owned(), Tone::Mute)).push(check())
                } else {
                    line.push(versions(ui, package))
                };
            }
            _ => {
                line = line.push(versions(ui, package));
                if self.state == State::Done {
                    line = line.push(check());
                }
            }
        }
        let bar = bar.map(|fraction| chart::line(fraction, Tone::Accent));
        ui.cell(column![line].push(bar).spacing(8))
    }

    fn output<'a>(&self, ui: Ui<'a>) -> Element<'a, Message> {
        let open = self.open.unwrap_or(ui.width == Width::Wide || self.session.plain);
        let chevron = if open { Icon::ChevronDown } else { Icon::ChevronRight };
        let labels = column![ui.text("Output"), ui.small("What the command prints")].spacing(1);
        let heading = row![
            ui.icon(Icon::Terminal, Tone::Mute),
            labels.width(Fill),
            ui.icon(chevron, Tone::Mute)
        ]
        .spacing(14)
        .align_y(Center);
        let heading = iced::widget::button(heading)
            .padding([ui.d(12.0), 18.0])
            .width(Fill)
            .style(style::row)
            .on_press(Message::Output(!open));
        if !open {
            return ui.group("", vec![heading.into()]);
        }
        let bar = || scrollable::Scrollbar::new().width(6).scroller_width(6).margin(2);
        let text = ui.mono(self.transaction.output(LINES)).style(style::ink(Tone::Dim));
        let text = container(text.wrapping(Wrapping::None)).padding([12, 14]);
        let text = scrollable(text).width(Fill).height(ui.d(236.0)).direction(
            scrollable::Direction::Both {
                vertical: bar().anchor(scrollable::Anchor::End),
                horizontal: bar(),
            },
        );
        let well = container(text).style(style::well).width(Fill).clip(true);
        let well = container(well).padding(iced::Padding::new(18.0).top(0.0));
        let content = column![heading, well].width(Fill);
        container(content).style(style::card).width(Fill).clip(true).into()
    }
}

fn failed<'a>(ui: Ui<'a>, failure: &'a Failure) -> Element<'a, Message> {
    let mut rows = vec![ui.icon_row(
        Icon::Warning,
        Tone::Warning,
        failure.title.as_str(),
        failure.hint.as_str(),
        space().width(0),
    )];
    if failure.mirrors {
        rows.push(ui.link(
            Icon::Globe,
            Tone::Mute,
            "Rank the mirrors",
            "Puts the fastest servers first",
            Message::Mirrors,
        ));
    }
    ui.group("", rows)
}

/// The question as it was printed, with the answers next to it.
fn asked<'a>(
    ui: Ui<'a>,
    question: &'a Question,
    buttons: Element<'a, Message>,
) -> Element<'a, Message> {
    let prompt = ui.mono(question.prompt.as_str()).style(style::ink(Tone::Mute)).width(Fill);
    if ui.content < 520.0 || question.prompt.len() > 48 {
        column![prompt, buttons].spacing(12).into()
    } else {
        row![prompt, buttons].spacing(12).align_y(Center).into()
    }
}

fn versions<'a>(ui: Ui<'a>, package: &'a Package) -> Element<'a, Message> {
    let to = |version: &'a str| ui.small(version).style(style::ink(Tone::Live));
    match (package.from.as_deref(), package.to.as_deref()) {
        (Some(from), Some(version)) => ui.change(from, version),
        (None, Some(version)) => to(version).wrapping(Wrapping::None).into(),
        (Some(from), None) => ui.small(from).wrapping(Wrapping::None).into(),
        (None, None) => space().into(),
    }
}

/// Sorts what is under way before what waits, and that before what is done.
fn activity(fraction: f32, started: bool) -> u8 {
    match (fraction >= 1.0, started) {
        (true, _) => 2,
        (false, true) => 0,
        (false, false) => 1,
    }
}

/// What a transaction does to the space used on disk.
#[derive(Debug, PartialEq)]
enum Disk {
    Same,
    Less(String),
    More(String),
}

impl Disk {
    /// Reads pacman's net size, which is negative when space is freed.
    fn of(size: &str) -> Self {
        let (freed, size) = match size.trim().strip_prefix('-') {
            Some(size) => (true, round(size)),
            None => (false, round(size)),
        };
        match freed {
            _ if size.starts_with("0.0 ") => Disk::Same,
            true => Disk::Less(size),
            false => Disk::More(size),
        }
    }
}

/// `442.39 MiB` as `442.4 MiB`.
fn round(size: &str) -> String {
    match size.trim().split_once(' ').map(|(value, unit)| (value.parse::<f32>(), unit)) {
        Some((Ok(value), unit)) => format!("{value:.1} {unit}"),
        _ => size.to_owned(),
    }
}

fn time_left(seconds: u32) -> String {
    match seconds {
        0..=5 => "a few seconds left".to_owned(),
        6..=90 => format!("{} s left", seconds.div_ceil(5) * 5),
        91..=5400 => format!("{} min left", (seconds + 30) / 60),
        _ => format!("{} h left", (seconds + 1800) / 3600),
    }
}

fn events(
    cmd: Cmd,
    keys: tokio::sync::mpsc::UnboundedReceiver<String>,
) -> impl Stream<Item = Event> {
    stream::channel(16, async move |mut output| {
        let mut running = match session::Running::start(&cmd, keys) {
            Ok(running) => running,
            Err(error) => {
                let _ = output.send(Event::Failed(error)).await;
                return;
            }
        };
        loop {
            let event = running.next().await;
            let last = matches!(event, Event::Exited(_));
            if output.send(event).await.is_err() || last {
                return;
            }
        }
    })
}

#[cfg(test)]
impl Job {
    /// A job that has printed `output`, without a program behind it.
    pub fn recorded(panel: Id, session: Session, output: &[u8]) -> Self {
        let (mut job, _) = Self::start(panel, session);
        let _ = job.update(Message::Event(Event::Output(output.to_vec())));
        job
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(output: &str) -> Job {
        Job::recorded(Id::Updates, packages::upgrade(), output.as_bytes())
    }

    fn exit(job: &mut Job, code: i32) -> Next {
        job.update(Message::Event(Event::Exited(Some(code))))
    }

    #[test]
    fn dismissing_the_password_prompt_is_not_a_failure() {
        let mut job = job("Error executing command as another user: Request dismissed\r\n");
        assert!(matches!(exit(&mut job, 126), Next::Stopped(text) if text.contains("password")));
    }

    #[test]
    fn declining_is_not_a_failure() {
        let mut job = job("Packages (1) tree-2.2-1\r\n\r\n:: Proceed with installation? [Y/n] ");
        assert!(job.asks());
        assert_eq!(job.summary(), "Waiting for you");
        assert_eq!(job.update(Message::Answer("n".into(), true)), Next::Nothing);
        assert!(!job.asks());
        assert_eq!(exit(&mut job, 1), Next::Stopped("Cancelled before anything changed".into()));
    }

    #[test]
    fn stops_only_while_that_is_safe() {
        let mut early = job(":: Retrieving packages...\r\n");
        assert!(early.stop());
        assert!(matches!(exit(&mut early, 130), Next::Stopped(_)));
        let mut late = job(":: Processing package changes...\r\n(1/2) upgrading bat [##] 100%\r\n");
        assert!(!late.stop());
        assert_eq!(late.summary(), "Installing 1 of 2");
        assert!(matches!(exit(&mut late, 1), Next::Failed(_)));
        assert_eq!(late.subtitle(), "Some packages may have been changed");
    }

    #[test]
    fn restarting_takes_two_steps() {
        let mut job = job("==> INFO: Reboot is recommended due to the upgrade\r\n");
        assert_eq!(exit(&mut job, 0), Next::Finished);
        assert_eq!(job.restart, Restart::Needed);
        assert_eq!(job.update(Message::Restart(true)), Next::Nothing);
        assert!(matches!(job.update(Message::RestartNow), Next::Run(_)));
    }

    #[test]
    fn a_failed_job_does_not_close_the_window() {
        let mut job = job("error: something new\r\n");
        job.closing = Some(window::Id::unique());
        assert_eq!(exit(&mut job, 1), Next::Failed("Something new".into()));
        assert_eq!(job.closing, None);
    }

    #[test]
    fn says_how_long_it_takes() {
        assert_eq!(time_left(3), "a few seconds left");
        assert_eq!(time_left(42), "45 s left");
        assert_eq!(time_left(420), "7 min left");
        assert_eq!(time_left(7200), "2 h left");
        assert_eq!(round("442.39 MiB"), "442.4 MiB");
        assert_eq!(round("unknown"), "unknown");
        assert_eq!(Disk::of("0.00 MiB"), Disk::Same);
        assert_eq!(Disk::of("-0.04 MiB"), Disk::Same);
        assert_eq!(Disk::of("-4.98 MiB"), Disk::Less("5.0 MiB".into()));
        assert_eq!(Disk::of("1.16 MiB"), Disk::More("1.2 MiB".into()));
    }
}
