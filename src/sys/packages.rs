//! pacman, CachyOS kernels, schedulers, mirrors and drivers.
//!
//! Anything that installs or removes packages opens in a terminal. pacman asks
//! about conflicts, replacements and keys, and those questions are the user's to
//! answer; `--noconfirm` from a settings window would answer them blindly.

use std::path::Path;

use super::{Change, Cmd, query};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub name: String,
    pub from: String,
    pub to: String,
}

fn parse_updates(text: &str) -> Vec<Update> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let (name, from) = (words.next()?, words.next()?);
            (words.next()? == "->").then_some(())?;
            Some(Update { name: name.into(), from: from.into(), to: words.next()?.into() })
        })
        .collect()
}

/// Updates known from the last database sync. Instant, but possibly stale.
pub async fn cached_updates() -> Vec<Update> {
    parse_updates(&query("pacman", ["-Qu"]).await.unwrap_or_default())
}

/// Updates on the mirrors right now. `checkupdates` syncs into a temporary
/// database, which avoids leaving the system in a partially synced state.
pub async fn fresh_updates() -> super::Result<Vec<Update>> {
    match Cmd::new("checkupdates").output().await {
        Ok(text) => Ok(parse_updates(&text)),
        // It exits with 2 and prints nothing when everything is current.
        Err(error) if error.to_string().contains("failed (") => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

/// Date of the last `pacman -Sy`.
pub fn last_sync() -> Option<std::time::SystemTime> {
    std::fs::metadata("/var/lib/pacman/sync/cachyos.db")
        .or_else(|_| std::fs::metadata("/var/lib/pacman/sync/core.db"))
        .ok()?
        .modified()
        .ok()
}

/// Describes how long ago `time` was, in words.
pub fn ago(time: std::time::SystemTime) -> String {
    let minutes = time.elapsed().map(|elapsed| elapsed.as_secs() / 60).unwrap_or(0);
    let plural = |n: u64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match minutes {
        0..=1 => "just now".to_owned(),
        2..=59 => plural(minutes, "minute"),
        60..=1439 => plural(minutes / 60, "hour"),
        _ => plural(minutes / 1440, "day"),
    }
}

pub fn upgrade() -> Cmd {
    let helper = ["paru", "yay"].into_iter().find(|helper| super::installed(helper));
    match helper {
        Some(helper) => Cmd::terminal(format!("{helper} -Syu")),
        None => Cmd::terminal("sudo pacman -Syu"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kernel {
    pub package: String,
    pub available: String,
    pub installed: Option<String>,
    pub running: bool,
}

impl Kernel {
    /// Short name, such as `bore` for `linux-cachyos-bore`.
    pub fn label(&self) -> String {
        match self.package.strip_prefix("linux-cachyos") {
            Some("") => "Default".to_owned(),
            Some(flavour) => {
                let words: Vec<_> = flavour
                    .trim_start_matches('-')
                    .split('-')
                    .map(|word| match word {
                        "lts" | "eevdf" | "bmq" | "rc" | "rt" | "eac" => word.to_uppercase(),
                        word => {
                            let mut chars = word.chars();
                            chars.next().map_or_else(String::new, |first| {
                                first.to_uppercase().collect::<String>() + chars.as_str()
                            })
                        }
                    })
                    .collect();
                words.join(" ")
            }
            None => self.package.clone(),
        }
    }

    pub fn blurb(&self) -> &'static str {
        match self.package.as_str() {
            "linux-cachyos" => "Recommended for most machines",
            "linux-cachyos-lts" => "Long-term support, a good fallback",
            "linux-cachyos-bore" => "BORE scheduler, tuned for responsiveness",
            "linux-cachyos-eevdf" => "The scheduler from mainline Linux",
            "linux-cachyos-bmq" => "BMQ scheduler",
            "linux-cachyos-hardened" => "Extra security hardening",
            "linux-cachyos-rt-bore" => "Real-time, for audio production",
            "linux-cachyos-server" => "Throughput over latency",
            "linux-cachyos-deckify" => "For handheld consoles",
            "linux-cachyos-rc" => "Release candidate of the next version",
            _ => "",
        }
    }
}

fn parse_kernels(listing: &str, running: &str) -> Vec<Kernel> {
    const VARIANTS: [&str; 6] = ["headers", "nvidia", "zfs", "r8125", "dbg", "lto"];
    let mut kernels: Vec<Kernel> = Vec::new();
    for line in listing.lines() {
        let mut words = line.split_whitespace();
        let (Some(_repo), Some(package), Some(version)) =
            (words.next(), words.next(), words.next())
        else {
            continue;
        };
        let is_kernel = package == "linux-cachyos" || package.starts_with("linux-cachyos-");
        if !is_kernel || VARIANTS.iter().any(|variant| package.contains(variant)) {
            continue;
        }
        // pacman prints "[installed]", or "[installed: 1.2]" when they differ.
        let rest = words.collect::<Vec<_>>().join(" ");
        let installed = rest
            .strip_prefix("[installed")
            .map(|tail| tail.trim_start_matches(':').trim_end_matches(']').trim().to_owned());
        let installed = installed.map(|v| if v.is_empty() { version.to_owned() } else { v });
        if kernels.iter().any(|kernel| kernel.package == package) {
            continue;
        }
        let flavour = package.trim_start_matches("linux-");
        // uname reports "7.3.5-2-cachyos"; the flavour follows the version.
        let running_flavour = running.splitn(3, '-').nth(2).unwrap_or_default();
        kernels.push(Kernel {
            running: installed.is_some() && running_flavour == flavour,
            package: package.to_owned(),
            available: version.to_owned(),
            installed,
        });
    }
    kernels.sort_by_key(|k| (!k.running, k.installed.is_none(), k.package.clone()));
    kernels
}

pub async fn kernels() -> Vec<Kernel> {
    let running = super::read("/proc/sys/kernel/osrelease").unwrap_or_default();
    parse_kernels(&query("pacman", ["-Sl"]).await.unwrap_or_default(), &running)
}

/// Package names end up in a shell command, so they are checked even though they
/// come from pacman's own listing.
fn valid_package(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "@._+-".contains(c))
}

pub fn install(packages: &[&str]) -> Option<Cmd> {
    packages
        .iter()
        .all(|package| valid_package(package))
        .then(|| Cmd::terminal(format!("sudo pacman -S {}", packages.join(" "))))
}

pub fn remove(packages: &[&str]) -> Option<Cmd> {
    packages
        .iter()
        .all(|package| valid_package(package))
        .then(|| Cmd::terminal(format!("sudo pacman -Rns {}", packages.join(" "))))
}

/// Kernels are installed with their headers, which DKMS modules need to build.
pub fn install_kernel(package: &str) -> Option<Cmd> {
    install(&[package, &format!("{package}-headers")])
}

pub fn remove_kernel(package: &str) -> Option<Cmd> {
    let headers = format!("{package}-headers");
    let has_headers = Path::new("/usr/src").join(package).exists();
    if has_headers { remove(&[package, &headers]) } else { remove(&[package]) }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scheduler {
    /// Name and mode of the running scheduler, if any.
    pub running: Option<(String, String)>,
    pub available: Vec<String>,
    pub at_boot: bool,
    pub supported: bool,
}

pub const MODES: [(&str, &str); 5] = [
    ("auto", "Auto"),
    ("gaming", "Gaming"),
    ("lowlatency", "Latency"),
    ("powersave", "Saver"),
    ("server", "Server"),
];

/// What each scheduler is for. Entries without a description are experimental
/// and are presented without a claim about their purpose.
pub fn scheduler_blurb(name: &str) -> &'static str {
    match name {
        "bpfland" => {
            "Favours interactive work. Tasks that wait on input get the CPU first, so the \
             desktop stays responsive under load."
        }
        "lavd" => {
            "Built for games. It keeps the tasks behind each frame on time, and saves power \
             on handhelds and laptops."
        }
        "rusty" => {
            "Splits the processor by cache and balances between the parts. Suits large \
             machines with mixed work."
        }
        "rustland" => {
            "Makes its decisions in user space and favours interactive tasks. More overhead \
             than bpfland."
        }
        "flash" => {
            "Earliest deadline first. Aims for predictable timing, for example in audio work."
        }
        "cosmos" => "A light general-purpose scheduler that prefers to keep tasks where they ran.",
        "p2dq" => "Compares two queues and takes work from the busier one. General purpose.",
        "tickless" => {
            "Cuts timer interrupts on busy cores. Meant for servers and virtual machines."
        }
        "beerland" => "Prefers to keep tasks on the same core, to reuse warm caches.",
        _ => "",
    }
}

pub async fn scheduler() -> Scheduler {
    let (get, list, boot) = tokio::join!(
        query("scxctl", ["get"]),
        query("scxctl", ["list"]),
        query("systemctl", ["is-enabled", "scx_loader.service"]),
    );
    Scheduler {
        running: get.as_deref().and_then(parse_running),
        available: list.as_deref().map(parse_schedulers).unwrap_or_default(),
        at_boot: boot.is_some_and(|state| state.trim() == "enabled"),
        supported: Path::new("/sys/kernel/sched_ext").is_dir(),
    }
}

/// Parses `running Bpfland in Auto mode`.
fn parse_running(text: &str) -> Option<(String, String)> {
    let words: Vec<_> = text.split_whitespace().collect();
    match words.as_slice() {
        ["running", name, "in", mode, ..] => Some((name.to_lowercase(), mode.to_lowercase())),
        ["running", name, ..] => Some((name.to_lowercase(), "auto".to_owned())),
        _ => None,
    }
}

/// Parses `supported schedulers: ["bpfland", "lavd"]`.
fn parse_schedulers(text: &str) -> Vec<String> {
    let mut names: Vec<String> = text.split('"').skip(1).step_by(2).map(str::to_owned).collect();
    names.sort();
    names
}

/// Starts a scheduler, switches to it, or returns to the kernel default.
pub fn set_scheduler(current: Option<&(String, String)>, target: Option<(&str, &str)>) -> Change {
    let command = |from_running: bool, to: Option<(&str, &str)>| match to {
        Some((name, mode)) => {
            let verb = if from_running { "switch" } else { "start" };
            Cmd::new("scxctl").args([verb, "--sched", name, "--mode", mode])
        }
        None => Cmd::new("scxctl").arg("stop"),
    };
    let before = current.map(|(name, mode)| (name.as_str(), mode.as_str()));
    Change::new(command(current.is_some(), target)).undo(command(target.is_some(), before))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorList {
    pub label: &'static str,
    pub servers: Vec<String>,
    pub ranked: Option<std::time::SystemTime>,
}

pub fn mirrors() -> Vec<MirrorList> {
    [("CachyOS", "/etc/pacman.d/cachyos-mirrorlist"), ("Arch Linux", "/etc/pacman.d/mirrorlist")]
        .into_iter()
        .filter_map(|(label, path)| {
            let text = super::read(path)?;
            Some(MirrorList {
                label,
                servers: parse_mirrors(&text),
                ranked: std::fs::metadata(path).ok().and_then(|m| m.modified().ok()),
            })
        })
        .collect()
}

fn parse_mirrors(text: &str) -> Vec<String> {
    let mut hosts: Vec<String> = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("Server"))
        .filter_map(|line| line.split("://").nth(1)?.split('/').next().map(str::to_owned))
        .collect();
    hosts.dedup();
    hosts
}

pub fn rank_mirrors() -> Cmd {
    Cmd::terminal("sudo cachyos-rate-mirrors")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Driver {
    pub device: String,
    pub profile: String,
    pub installed: bool,
}

/// Parses the tables `chwd` prints, pairing every device with its profiles.
fn parse_drivers(available: &str, installed: &str) -> Vec<Driver> {
    let installed: Vec<_> = installed.lines().filter_map(table_cell).collect();
    let mut device = String::new();
    let mut drivers = Vec::new();
    for line in available.lines() {
        if let Some(heading) = line.strip_prefix("> ") {
            // "0000:00:02.0 (0300:8086:4628) VGA compatible controller Intel:"
            device = heading
                .split_once(") ")
                .map_or(heading, |(_, name)| name)
                .trim_end_matches(':')
                .to_owned();
        } else if let Some(profile) = table_cell(line) {
            drivers.push(Driver {
                installed: installed.contains(&profile),
                device: device.clone(),
                profile,
            });
        }
    }
    drivers
}

fn table_cell(line: &str) -> Option<String> {
    let cell = line.trim().strip_prefix('│')?.split('┆').next()?.trim();
    (!cell.is_empty() && cell != "Name").then(|| cell.to_owned())
}

pub async fn drivers() -> Vec<Driver> {
    let (available, installed) =
        tokio::join!(query("chwd", ["--list"]), query("chwd", ["--list-installed"]));
    parse_drivers(&available.unwrap_or_default(), &installed.unwrap_or_default())
}

pub fn install_driver(profile: &str) -> Option<Cmd> {
    valid_package(profile).then(|| Cmd::terminal(format!("sudo chwd --install {profile}")))
}

pub fn remove_driver(profile: &str) -> Option<Cmd> {
    valid_package(profile).then(|| Cmd::terminal(format!("sudo chwd --remove {profile}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_updates_and_skips_noise() {
        let updates = parse_updates("linux-cachyos 7.3.5-2 -> 7.3.6-1\nwarning: something\n");
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].to, "7.3.6-1");
    }

    #[test]
    fn lists_kernels_with_the_running_one_first() {
        let listing = "cachyos linux-cachyos 7.3.5-2 [installed]\n\
                       cachyos linux-cachyos-bore 7.3.5-2\n\
                       cachyos linux-cachyos-headers 7.3.5-2 [installed]\n\
                       cachyos linux-cachyos-lts 6.18.64-1 [installed: 6.18.60-3]\n\
                       cachyos linux-cachyos-nvidia 7.3.5-2\n\
                       cachyos-v3 linux-cachyos 7.3.5-2 [installed]\n";
        let kernels = parse_kernels(listing, "7.3.5-2-cachyos");
        let names: Vec<_> = kernels.iter().map(|k| k.package.as_str()).collect();
        assert_eq!(names, ["linux-cachyos", "linux-cachyos-lts", "linux-cachyos-bore"]);
        assert!(kernels[0].running && !kernels[1].running);
        assert_eq!(kernels[1].installed.as_deref(), Some("6.18.60-3"));
        assert_eq!(kernels[1].label(), "LTS");
        assert_eq!(kernels[2].label(), "Bore");
    }

    #[test]
    fn package_names_cannot_inject_shell() {
        assert!(install(&["linux-cachyos"]).is_some());
        assert!(install(&["x; rm -rf ~"]).is_none());
        assert!(install_driver("$(id)").is_none());
    }

    #[test]
    fn parses_scheduler_state() {
        assert_eq!(parse_running("no scx scheduler running"), None);
        assert_eq!(
            parse_running("running Bpfland in Auto mode"),
            Some(("bpfland".into(), "auto".into()))
        );
        assert_eq!(
            parse_schedulers("supported schedulers: [\"lavd\", \"cake\"]"),
            ["cake", "lavd"]
        );
    }

    #[test]
    fn scheduler_changes_undo_to_the_previous_state() {
        let running = ("lavd".to_owned(), "gaming".to_owned());
        let change = set_scheduler(Some(&running), None);
        assert_eq!(change.apply.describe(), "scxctl stop");
        assert_eq!(change.undo.unwrap().describe(), "scxctl start --sched lavd --mode gaming");
        let change = set_scheduler(None, Some(("bpfland", "auto")));
        assert_eq!(change.undo.unwrap().describe(), "scxctl stop");
    }

    #[test]
    fn pairs_drivers_with_devices() {
        let available = "> 0000:00:02.0 (0300:8086:4628) VGA compatible controller Intel Corporation:\n\n\
                         ╭──────────┬──────────╮\n│ Name     ┆ Priority │\n╞══════════╪══════════╡\n\
                         │ intel    ┆ 4        │\n│ fallback ┆ 3        │\n╰──────────┴──────────╯\n";
        let installed = "│ Name  ┆ Priority │\n│ intel ┆ 4        │\n";
        let drivers = parse_drivers(available, installed);
        assert_eq!(drivers.len(), 2);
        assert_eq!(drivers[0].device, "VGA compatible controller Intel Corporation");
        assert!(drivers[0].installed && !drivers[1].installed);
    }

    #[test]
    fn reads_mirror_hosts() {
        let text = "# Server = https://off.example/\nServer = https://cdn77.cachyos.org/repo/$arch/$repo\n";
        assert_eq!(parse_mirrors(text), ["cdn77.cachyos.org"]);
    }
}
