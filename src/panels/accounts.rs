use iced::widget::{column, row};
use iced::{Center, Task};

use super::{Effect, Panel};
use crate::icon::Icon;
use crate::sys::region::{self, Account, Accounts};
use crate::theme::Tone;
use crate::ui::{Element, Kind, Ui};

#[derive(Default)]
pub struct State {
    accounts: Option<Accounts>,
    name: String,
    login: String,
    admin: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Accounts),
    Name(String),
    SaveName,
    Shell(String),
    Password,
    Admin(String, bool),
    Login(String),
    NewAdmin(bool),
    Add,
}

impl State {
    fn me(&self) -> Option<&Account> {
        let accounts = self.accounts.as_ref()?;
        accounts.list.iter().find(|account| account.login == accounts.current)
    }

    fn mine<'a>(&'a self, ui: Ui<'a>, me: &'a Account, shells: &[String]) -> Element<'a, Message> {
        let name = self.name.trim();
        // A colon separates fields in /etc/passwd and a comma fields in GECOS.
        let renamed = name != me.name && !name.is_empty() && !name.contains([':', ',']);
        let rename = row![
            ui.field("Full name", &self.name, Message::Name)
                .on_submit_maybe(renamed.then_some(Message::SaveName)),
            ui.button("Save", Kind::Secondary, renamed.then_some(Message::SaveName)),
        ]
        .spacing(8)
        .align_y(Center);
        ui.group(
            "You",
            vec![
                ui.cell(column![ui.small("Full name"), rename].spacing(6)),
                ui.value("Login", me.login.as_str()),
                ui.row(
                    "Shell",
                    "",
                    ui.pick(shells.to_vec(), Some(me.shell.clone()), Message::Shell),
                ),
                ui.row(
                    "Password",
                    "",
                    ui.button("Change", Kind::Secondary, Some(Message::Password)),
                ),
                ui.value("Groups", me.groups.join(", ")),
            ],
        )
    }
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(region::accounts(), Message::Loaded)
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(accounts) => {
                self.accounts = Some(accounts);
                self.name = self.me().map(|me| me.name.clone()).unwrap_or_default();
                Effect::none()
            }
            Message::Name(name) => {
                self.name = name;
                Effect::none()
            }
            Message::SaveName => match self.me() {
                Some(me) => {
                    Effect::change(region::set_full_name(&me.login, &me.name, self.name.trim()))
                }
                None => Effect::none(),
            },
            Message::Shell(shell) => match self.me() {
                Some(me) => Effect::change(region::set_shell(&me.login, &me.shell, &shell)),
                None => Effect::none(),
            },
            Message::Password => Effect::run(region::change_password()),
            Message::Admin(login, on) => Effect::change(region::set_admin(&login, on)),
            Message::Login(login) => {
                self.login = login;
                Effect::none()
            }
            Message::NewAdmin(on) => {
                self.admin = on;
                Effect::none()
            }
            Message::Add => {
                let login = std::mem::take(&mut self.login);
                Effect::checked(region::add_account(&login, self.admin))
            }
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let Some(accounts) = &self.accounts else {
            return ui.panel("Accounts", "", None, vec![ui.loading()]);
        };
        let admins = accounts.list.iter().filter(|account| account.admin).count();
        let mut others: Vec<_> = accounts
            .list
            .iter()
            .map(|account| {
                let login = account.login.clone();
                // Without any administrator nobody could undo this.
                let locked = account.admin && admins < 2;
                let toggle =
                    (!locked).then_some(move |on| Message::Admin(login.clone(), on));
                ui.icon_row(
                    Icon::User,
                    Tone::Mute,
                    account.name.as_str(),
                    if account.admin { "Administrator" } else { "Standard" },
                    ui.switch(account.admin, toggle),
                )
            })
            .collect();
        let valid = region::valid_login(&self.login);
        let form = column![
            row![
                ui.field("Login for a new account", &self.login, Message::Login)
                    .on_submit_maybe(valid.then_some(Message::Add)),
                ui.button("Add", Kind::Primary, valid.then_some(Message::Add)),
            ]
            .spacing(8)
            .align_y(Center),
            row![ui.switch(self.admin, Some(Message::NewAdmin)), ui.text("Administrator")]
                .spacing(10)
                .align_y(Center),
        ]
        .spacing(10);
        others.push(ui.cell(form));
        let everyone =
            ui.group("Everyone. The switch makes someone an administrator.", others);
        let body = match self.me() {
            Some(me) => ui.columns(self.mine(ui, me, &accounts.shells), everyone),
            None => everyone,
        };
        ui.panel("Accounts", "", None, vec![body])
    }

    fn summary(&self) -> String {
        self.me().map(|me| me.name.clone()).unwrap_or_default()
    }
}
