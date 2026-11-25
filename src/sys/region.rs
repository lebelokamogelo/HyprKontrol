//! Time, language, keyboard layouts and accounts.

use super::{Change, Cmd, lookup, query, read};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Clock {
    pub zone: String,
    pub ntp: bool,
    pub can_ntp: bool,
    pub synchronized: bool,
    pub local_rtc: bool,
    pub now: String,
}

pub async fn clock() -> Clock {
    let (show, now) =
        tokio::join!(query("timedatectl", ["show"]), query("date", ["+%A %-d %B %Y, %H:%M"]),);
    let show = show.unwrap_or_default();
    let flag = |key: &str| lookup(&show, key) == Some("yes");
    Clock {
        zone: lookup(&show, "Timezone").unwrap_or_default().to_owned(),
        ntp: flag("NTP"),
        can_ntp: flag("CanNTP"),
        synchronized: flag("NTPSynchronized"),
        local_rtc: flag("LocalRTC"),
        now: now.unwrap_or_default().trim().to_owned(),
    }
}

async fn lines<const N: usize>(program: &str, args: [&str; N]) -> Vec<String> {
    query(program, args)
        .await
        .map(|text| text.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

pub async fn zones() -> Vec<String> {
    lines("timedatectl", ["list-timezones"]).await
}

pub fn set_zone(before: &str, after: &str) -> Change {
    let set = |zone: &str| Cmd::new("timedatectl").args(["set-timezone", zone]);
    Change::new(set(after)).undo(set(before))
}

pub fn set_ntp(on: bool) -> Change {
    let set =
        |on: bool| Cmd::new("timedatectl").args(["set-ntp", if on { "true" } else { "false" }]);
    Change::new(set(on)).undo(set(!on))
}

pub fn set_local_rtc(on: bool) -> Change {
    let set =
        |on: bool| Cmd::new("timedatectl").args(["set-local-rtc", if on { "1" } else { "0" }]);
    Change::new(set(on)).undo(set(!on))
}

/// `when` is `YYYY-MM-DD HH:MM`, which timedatectl validates.
pub fn set_time(when: &str) -> Change {
    Change::new(Cmd::new("timedatectl").args(["set-time", when]))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Locale {
    pub language: String,
    pub formats: String,
    pub available: Vec<String>,
}

/// Categories that make up "formats": dates, numbers, currency and paper size.
const FORMATS: [&str; 5] = ["LC_TIME", "LC_NUMERIC", "LC_MONETARY", "LC_MEASUREMENT", "LC_PAPER"];

pub async fn locale() -> Locale {
    let text = read("/etc/locale.conf").unwrap_or_default();
    let language = lookup(&text, "LANG").unwrap_or("C.UTF-8").to_owned();
    Locale {
        formats: lookup(&text, "LC_TIME").unwrap_or(&language).to_owned(),
        available: lines("localectl", ["list-locales"]).await,
        language,
    }
}

/// `localectl set-locale` replaces the whole file, so both values are always
/// passed together.
pub fn set_locale(before: (&str, &str), after: (&str, &str)) -> Change {
    let set = |(language, formats): (&str, &str)| {
        Cmd::new("localectl")
            .arg("set-locale")
            .arg(format!("LANG={language}"))
            .args(FORMATS.iter().map(|key| format!("{key}={formats}")))
    };
    Change::new(set(after)).undo(set(before))
}

/// Generates a locale. Editing `locale.gen` and running `locale-gen` belong
/// together, so they share one authorisation.
pub fn generate_locale(name: &str) -> Option<Cmd> {
    let valid = name.ends_with(".UTF-8")
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "_.@-".contains(c));
    valid.then(|| {
        let script = format!(
            "sed -i 's/^#\\s*{0} UTF-8/{0} UTF-8/' /etc/locale.gen && locale-gen",
            name.replace('.', "\\.")
        );
        Cmd::new("sh").args(["-c", &script]).root()
    })
}

/// Locales that can be generated, from the comments in `locale.gen`.
pub fn generatable() -> Vec<String> {
    read("/etc/locale.gen").map(|text| parse_locale_gen(&text)).unwrap_or_default()
}

fn parse_locale_gen(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.strip_prefix('#'))
        .filter_map(|line| line.trim().strip_suffix(" UTF-8"))
        .filter(|name| name.ends_with(".UTF-8") && !name.contains(' '))
        .map(str::to_owned)
        .collect()
}

/// A keyboard layout or variant with its description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub code: String,
    pub name: String,
}

impl std::fmt::Display for Layout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layouts {
    pub layouts: Vec<Layout>,
    /// Pairs of layout code and variant.
    pub variants: Vec<(String, Layout)>,
    pub options: Vec<Layout>,
}

pub fn layouts() -> Layouts {
    read("/usr/share/X11/xkb/rules/base.lst").map(|text| parse_layouts(&text)).unwrap_or_default()
}

fn parse_layouts(text: &str) -> Layouts {
    let mut result = Layouts::default();
    let mut section = "";
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("! ") {
            section = name.trim();
            continue;
        }
        let Some((code, name)) = line.trim().split_once(char::is_whitespace) else { continue };
        let name = name.trim();
        match section {
            "layout" => {
                result.layouts.push(Layout { code: code.to_owned(), name: name.to_owned() });
            }
            // "dvorak          us: English (Dvorak)"
            "variant" => {
                if let Some((layout, name)) = name.split_once(": ") {
                    let variant = Layout { code: code.to_owned(), name: name.to_owned() };
                    result.variants.push((layout.to_owned(), variant));
                }
            }
            "option" if code.contains(':') => {
                result.options.push(Layout { code: code.to_owned(), name: name.to_owned() });
            }
            _ => {}
        }
    }
    result.layouts.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Account {
    pub login: String,
    pub name: String,
    pub uid: u32,
    pub shell: String,
    pub home: String,
    pub admin: bool,
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Accounts {
    pub current: String,
    pub list: Vec<Account>,
    pub shells: Vec<String>,
}

pub async fn accounts() -> Accounts {
    let (passwd, whoami) = tokio::join!(query("getent", ["passwd"]), query("id", ["-un"]));
    let groups = read("/etc/group").unwrap_or_default();
    let mut shells: Vec<String> = read("/etc/shells")
        .unwrap_or_default()
        .lines()
        .filter(|line| line.starts_with("/usr/bin/"))
        .map(str::to_owned)
        .collect();
    shells.sort();
    shells.dedup();
    Accounts {
        current: whoami.unwrap_or_default().trim().to_owned(),
        list: parse_accounts(&passwd.unwrap_or_default(), &groups),
        shells,
    }
}

/// Keeps people. System users have ids below 1000 and `nobody` is 65534.
fn parse_accounts(passwd: &str, groups: &str) -> Vec<Account> {
    passwd
        .lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split(':').collect();
            let [login, _, uid, _, gecos, home, shell] = fields.as_slice() else { return None };
            let uid: u32 = uid.parse().ok()?;
            if !(1000..60000).contains(&uid) {
                return None;
            }
            let member_of: Vec<String> = groups
                .lines()
                .filter_map(|line| {
                    let (group, members) = (line.split(':').next()?, line.rsplit(':').next()?);
                    members.split(',').any(|m| m == *login).then(|| group.to_owned())
                })
                .collect();
            let name = gecos.split(',').next().unwrap_or_default();
            Some(Account {
                name: if name.is_empty() { (*login).to_owned() } else { name.to_owned() },
                login: (*login).to_owned(),
                uid,
                shell: (*shell).to_owned(),
                home: (*home).to_owned(),
                admin: member_of.iter().any(|group| group == "wheel"),
                groups: member_of,
            })
        })
        .collect()
}

pub fn set_full_name(login: &str, before: &str, after: &str) -> Change {
    let set = |name: &str| Cmd::new("usermod").args(["--comment", name, login]).root();
    Change::new(set(after)).undo(set(before))
}

pub fn set_shell(login: &str, before: &str, after: &str) -> Change {
    let set = |shell: &str| Cmd::new("usermod").args(["--shell", shell, login]).root();
    Change::new(set(after)).undo(set(before))
}

pub fn set_admin(login: &str, admin: bool) -> Change {
    let set = |admin: bool| {
        let flag = if admin { "--add" } else { "--delete" };
        Cmd::new("gpasswd").args([flag, login, "wheel"]).root()
    };
    Change::new(set(admin)).undo(set(!admin))
}

/// Passwords are entered in a terminal, where `passwd` applies the PAM rules.
pub fn change_password() -> Cmd {
    Cmd::terminal("passwd")
}

/// Login names follow the rules of `useradd`.
pub fn valid_login(login: &str) -> bool {
    let mut chars = login.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && login.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-".contains(c))
}

/// Creates an account and sets its password in the same terminal session.
pub fn add_account(login: &str, admin: bool) -> Option<Cmd> {
    valid_login(login).then(|| {
        let groups = if admin { " -G wheel" } else { "" };
        Cmd::terminal(format!("sudo useradd -m{groups} {login} && sudo passwd {login}"))
    })
}

pub fn set_hostname(before: &str, after: &str) -> Change {
    let set = |name: &str| Cmd::new("hostnamectl").args(["set-hostname", name]);
    Change::new(set(after)).undo(set(before))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_people_and_their_groups() {
        let passwd = "root:x:0:0::/root:/bin/bash\n\
                      ada:x:1000:1000:Ada Lovelace,,,:/home/ada:/usr/bin/fish\n\
                      nobody:x:65534:65534::/:/usr/bin/nologin\n";
        let accounts = parse_accounts(passwd, "wheel:x:998:ada,bob\nvideo:x:985:ada\n");
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].name, "Ada Lovelace");
        assert!(accounts[0].admin);
        assert_eq!(accounts[0].groups, ["wheel", "video"]);
    }

    #[test]
    fn parses_xkb_rules() {
        let rules = "! model\n  pc105  Generic\n\n! layout\n  us  English (US)\n  za  English (South Africa)\n\n\
                     ! variant\n  dvorak  us: English (Dvorak)\n\n! option\n  ctrl  Ctrl position\n  ctrl:nocaps  Caps Lock as Ctrl\n";
        let layouts = parse_layouts(rules);
        assert_eq!(layouts.layouts.len(), 2);
        assert_eq!(layouts.variants[0].0, "us");
        assert_eq!(layouts.options.len(), 1);
    }

    #[test]
    fn rejects_unsafe_names() {
        assert!(valid_login("ada-2") && !valid_login("2ada") && !valid_login("a;b"));
        assert!(generate_locale("en_ZA.UTF-8").is_some());
        assert!(generate_locale("x/e id; .UTF-8").is_none());
    }

    #[test]
    fn lists_commented_locales() {
        let text = "# Comment\n#af_ZA.UTF-8 UTF-8\nen_ZA.UTF-8 UTF-8\n#de_DE ISO-8859-1\n";
        assert_eq!(parse_locale_gen(text), ["af_ZA.UTF-8"]);
    }
}
