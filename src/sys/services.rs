//! systemd units and kernel tunables.

use std::collections::HashMap;

use serde::Deserialize;

use super::{Change, Cmd, Result, query, read};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    System,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub unit: String,
    pub description: String,
    pub active: bool,
    pub failed: bool,
    /// `enabled`, `disabled`, `static` and so on.
    pub state: String,
}

impl Service {
    pub fn name(&self) -> &str {
        self.unit.trim_end_matches(".service")
    }

    /// Static and generated units are started by other units, so enabling them
    /// has no meaning.
    pub fn can_enable(&self) -> bool {
        matches!(self.state.as_str(), "enabled" | "disabled")
    }
}

#[derive(Deserialize)]
struct Unit {
    unit: String,
    active: String,
    description: String,
}

#[derive(Deserialize)]
struct UnitFile {
    unit_file: String,
    state: String,
}

fn systemctl(scope: Scope) -> Cmd {
    match scope {
        Scope::System => Cmd::new("systemctl"),
        Scope::User => Cmd::new("systemctl").arg("--user"),
    }
}

/// Merges loaded units with installed unit files. Neither listing is complete:
/// stopped services are often not loaded, and instances have no file.
pub async fn list(scope: Scope) -> Result<Vec<Service>> {
    let units = systemctl(scope)
        .args(["list-units", "--type=service", "--all", "-o", "json"])
        .output()
        .await?;
    let files =
        systemctl(scope).args(["list-unit-files", "--type=service", "-o", "json"]).output().await?;
    Ok(merge(serde_json::from_str(&units)?, serde_json::from_str(&files)?))
}

fn merge(units: Vec<Unit>, files: Vec<UnitFile>) -> Vec<Service> {
    let mut states: HashMap<String, String> =
        files.into_iter().map(|file| (file.unit_file, file.state)).collect();
    let mut services: Vec<Service> = units
        .into_iter()
        .filter(|unit| unit.unit.ends_with(".service"))
        .map(|unit| Service {
            active: unit.active == "active",
            failed: unit.active == "failed",
            state: states.remove(&unit.unit).unwrap_or_default(),
            description: unit.description,
            unit: unit.unit,
        })
        .collect();
    services.extend(
        states
            .into_iter()
            // Templates such as getty@.service cannot be started without an instance.
            .filter(|(unit, _)| !unit.contains("@."))
            .map(|(unit, state)| Service {
                description: String::new(),
                active: false,
                failed: false,
                unit,
                state,
            }),
    );
    services.sort_by_key(|service| (!service.failed, service.unit.to_lowercase()));
    services
}

pub fn set_active(scope: Scope, unit: &str, on: bool) -> Change {
    let set = |on: bool| systemctl(scope).args([if on { "start" } else { "stop" }, unit]);
    Change::new(set(on)).undo(set(!on))
}

pub fn set_enabled(scope: Scope, unit: &str, on: bool) -> Change {
    let set = |on: bool| systemctl(scope).args([if on { "enable" } else { "disable" }, unit]);
    Change::new(set(on)).undo(set(!on))
}

/// Enables and starts, or disables and stops, in one step.
pub fn set_now(scope: Scope, unit: &str, on: bool) -> Change {
    let set =
        |on: bool| systemctl(scope).args([if on { "enable" } else { "disable" }, "--now", unit]);
    Change::new(set(on)).undo(set(!on))
}

pub fn restart(scope: Scope, unit: &str) -> Cmd {
    systemctl(scope).args(["restart", unit])
}

pub fn logs(scope: Scope, unit: &str) -> Option<Cmd> {
    let valid = unit.chars().all(|c| c.is_ascii_alphanumeric() || "@._:-\\".contains(c));
    let user = if scope == Scope::User { "--user " } else { "" };
    valid.then(|| Cmd::terminal(format!("journalctl {user}-n 200 --no-pager -u '{unit}'")))
}

/// A background service offered as a tweak, if it is installed.
pub struct Tweak {
    pub unit: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub scope: Scope,
}

pub const TWEAKS: &[Tweak] = &[
    Tweak {
        unit: "ananicy-cpp.service",
        label: "Process priorities",
        hint: "Gives games and desktop apps priority over background work (ananicy-cpp)",
        scope: Scope::System,
    },
    Tweak {
        unit: "systemd-oomd.service",
        label: "Out-of-memory protection",
        hint: "Ends runaway apps before the system freezes (systemd-oomd)",
        scope: Scope::System,
    },
    Tweak {
        unit: "bpftune.service",
        label: "Automatic kernel tuning",
        hint: "Adjusts network and memory settings to the workload (bpftune)",
        scope: Scope::System,
    },
    Tweak {
        unit: "fstrim.timer",
        label: "Weekly SSD trim",
        hint: "Keeps write speed up on solid-state drives",
        scope: Scope::System,
    },
    Tweak {
        unit: "paccache.timer",
        label: "Clean the package cache",
        hint: "Keeps the three latest versions of every package",
        scope: Scope::System,
    },
    Tweak {
        unit: "psd.service",
        label: "Browser profile in memory",
        hint: "Fewer disk writes and a faster browser (profile-sync-daemon)",
        scope: Scope::User,
    },
    Tweak {
        unit: "arch-update.timer",
        label: "Update notifications",
        hint: "Checks for new packages in the background",
        scope: Scope::User,
    },
];

/// State of each tweak, or `None` where the unit is not installed.
pub async fn tweaks() -> Vec<Option<bool>> {
    let mut states = Vec::with_capacity(TWEAKS.len());
    for tweak in TWEAKS {
        let state = systemctl(tweak.scope)
            .args(["show", "-p", "UnitFileState", "-p", "LoadState", tweak.unit])
            .output()
            .await
            .unwrap_or_default();
        let get = |key: &str| super::lookup(&state, key).unwrap_or_default();
        states.push((get("LoadState") == "loaded").then(|| get("UnitFileState") == "enabled"));
    }
    states
}

/// A kernel parameter, its current value and the offered range.
#[derive(Debug, Clone, PartialEq)]
pub struct Sysctl {
    pub key: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub max: f32,
    pub value: f32,
}

const SYSCTLS: [(&str, &str, &str, f32); 3] = [
    (
        "vm.swappiness",
        "Swappiness",
        "How readily memory moves to swap. High values suit compressed swap (zram).",
        200.0,
    ),
    (
        "vm.vfs_cache_pressure",
        "Cache pressure",
        "How readily file metadata is dropped from memory. Lower keeps more of it.",
        200.0,
    ),
    (
        "kernel.nmi_watchdog",
        "Lockup watchdog",
        "Detects frozen cores at the cost of some power. 0 is off.",
        1.0,
    ),
];

pub fn sysctls() -> Vec<Sysctl> {
    SYSCTLS
        .into_iter()
        .filter_map(|(key, label, hint, max)| {
            let value = super::read_number(format!("/proc/sys/{}", key.replace('.', "/")))?;
            Some(Sysctl { key, label, hint, max, value: value as f32 })
        })
        .collect()
}

const SYSCTL_FILE: &str = "/etc/sysctl.d/99-hyprkontrol.conf";

/// Persists a parameter and loads it in one authorisation. Values written
/// earlier by this program are kept.
pub fn set_sysctl(key: &str, before: f32, after: f32) -> Change {
    let saved = read(SYSCTL_FILE).unwrap_or_default();
    let command = |value: f32| {
        let mut lines: Vec<String> = saved
            .lines()
            .filter(|line| line.split('=').next().map(str::trim) != Some(key))
            .map(str::to_owned)
            .collect();
        lines.push(format!("{key} = {value:.0}"));
        let script = format!("cat > {SYSCTL_FILE} && sysctl -q -p {SYSCTL_FILE}");
        Cmd::new("sh").args(["-c", &script]).stdin(lines.join("\n") + "\n").root()
    };
    Change::new(command(after)).undo(command(before))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zram {
    pub size: u64,
    pub algorithm: String,
}

pub fn zram() -> Option<Zram> {
    let size = super::read_number("/sys/block/zram0/disksize")? as u64;
    let algorithms = read("/sys/block/zram0/comp_algorithm").unwrap_or_default();
    // The active one is in brackets: "lzo [zstd] lz4".
    let algorithm = algorithms.split(['[', ']']).nth(1).unwrap_or_default().to_owned();
    (size > 0).then_some(Zram { size, algorithm })
}

pub async fn is_active(scope: Scope, unit: &str) -> bool {
    match scope {
        Scope::System => query("systemctl", ["is-active", unit]).await.is_some(),
        Scope::User => query("systemctl", ["--user", "is-active", unit]).await.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_loaded_units_with_unit_files() {
        let unit = |unit: &str, active: &str| Unit {
            unit: unit.into(),
            active: active.into(),
            description: "d".into(),
        };
        let file = |unit_file: &str, state: &str| UnitFile {
            unit_file: unit_file.into(),
            state: state.into(),
        };
        let services = merge(
            vec![unit("a.service", "active"), unit("broken.service", "failed")],
            vec![
                file("a.service", "enabled"),
                file("idle.service", "disabled"),
                file("getty@.service", "enabled"),
            ],
        );
        let names: Vec<_> = services.iter().map(Service::name).collect();
        assert_eq!(names, ["broken", "a", "idle"]);
        assert!(services[1].active && services[1].can_enable());
        assert!(!services[0].can_enable());
    }
}
