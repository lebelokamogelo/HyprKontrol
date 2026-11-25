//! Battery, power profiles, screen brightness and logind actions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use zbus::zvariant::OwnedValue;

use super::{Change, Cmd, Op, lookup, query, read, read_number};

const SUPPLY: &str = "/sys/class/power_supply";
const LOGIND_DROP_IN: &str = "/etc/systemd/logind.conf.d/50-hyprkontrol.conf";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Battery {
    pub percent: f32,
    pub status: String,
    /// Energy in watt-hours: now, when full today, and when the cell was new.
    pub now: f32,
    pub full: f32,
    pub design: f32,
    pub watts: f32,
    pub cycles: Option<u32>,
    /// Path of the charge limit, when the firmware offers one.
    pub limit_path: Option<PathBuf>,
    pub limit: Option<u8>,
}

impl Battery {
    /// Share of the original capacity that is left, from 0 to 1.
    pub fn health(&self) -> f32 {
        if self.design > 0.0 { (self.full / self.design).min(1.0) } else { 1.0 }
    }

    pub fn charging(&self) -> bool {
        self.status == "Charging"
    }

    /// Hours until full or empty. Unknown while the rate is close to zero.
    pub fn hours(&self) -> Option<f32> {
        if self.watts < 0.5 {
            return None;
        }
        match self.status.as_str() {
            "Charging" => Some((self.full - self.now).max(0.0) / self.watts),
            "Discharging" => Some(self.now / self.watts),
            _ => None,
        }
    }

    pub fn summary(&self) -> String {
        let time = self.hours().map(duration);
        match (self.status.as_str(), time) {
            ("Charging", Some(time)) => {
                format!("Charging at {:.0} W, full in {time}", self.watts)
            }
            ("Discharging", Some(time)) => format!("Using {:.1} W, {time} left", self.watts),
            ("Full", _) => "Fully charged".to_owned(),
            ("Not charging", _) => "Plugged in, not charging".to_owned(),
            (status, _) => status.to_owned(),
        }
    }
}

fn duration(hours: f32) -> String {
    let minutes = (hours * 60.0).round() as u32;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// Reads the first system battery.
pub fn battery() -> Option<Battery> {
    std::fs::read_dir(SUPPLY).ok()?.flatten().map(|entry| entry.path()).find_map(|path| {
        let is_battery = read(path.join("type")).as_deref() == Some("Battery");
        // Wireless mice and headsets report as batteries too, with device scope.
        let is_peripheral = read(path.join("scope")).as_deref() == Some("Device");
        (is_battery && !is_peripheral).then(|| read_battery(&path)).flatten()
    })
}

fn read_battery(path: &Path) -> Option<Battery> {
    let number = |name: &str| read_number(path.join(name)).map(|value| value as f32 / 1e6);
    // Firmware reports either energy in Wh or charge in Ah. The latter is
    // converted with the design voltage, as upower does.
    let (now, full, design, watts) = match number("energy_full") {
        Some(full) => (
            number("energy_now")?,
            full,
            number("energy_full_design").unwrap_or(full),
            number("power_now").unwrap_or(0.0),
        ),
        None => {
            let volts = number("voltage_min_design").or_else(|| number("voltage_now"))?;
            let full = number("charge_full")? * volts;
            (
                number("charge_now")? * volts,
                full,
                number("charge_full_design").map_or(full, |charge| charge * volts),
                number("current_now").unwrap_or(0.0) * volts,
            )
        }
    };
    let limit_path = path.join("charge_control_end_threshold");
    let limit = read_number(&limit_path).map(|value| value as u8);
    Some(Battery {
        percent: read_number(path.join("capacity")).unwrap_or(0.0) as f32,
        status: read(path.join("status")).unwrap_or_default(),
        now,
        full,
        design,
        watts: watts.abs(),
        cycles: read_number(path.join("cycle_count")).map(|n| n as u32).filter(|n| *n > 0),
        limit_path: limit.is_some().then_some(limit_path),
        limit,
    })
}

pub fn set_limit(path: &Path, before: u8, after: u8) -> Change {
    let write =
        |value: u8| Cmd::new("tee").arg(path.display().to_string()).stdin(value.to_string()).root();
    Change::new(write(after)).undo(write(before))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profiles {
    pub active: String,
    pub available: Vec<String>,
    pub battery_aware: Option<bool>,
}

/// Reads the profile daemon over D-Bus. `powerprofilesctl` would do, but it is a
/// Python program, and starting an interpreter every few seconds shows up on the
/// battery this panel is about.
pub async fn profiles() -> Profiles {
    read_profiles().await.unwrap_or_default()
}

async fn read_profiles() -> zbus::Result<Profiles> {
    const NAME: &str = "org.freedesktop.UPower.PowerProfiles";
    const PATH: &str = "/org/freedesktop/UPower/PowerProfiles";

    let bus = zbus::Connection::system().await?;
    let daemon = zbus::Proxy::new(&bus, NAME, PATH, NAME).await?;
    let listed: Vec<HashMap<String, OwnedValue>> = daemon.get_property("Profiles").await?;
    let mut available: Vec<String> = listed
        .into_iter()
        .filter_map(|mut profile| String::try_from(profile.remove("Profile")?).ok())
        .collect();
    available.sort_by_key(|name| PROFILE_ORDER.iter().position(|known| known == name));
    Ok(Profiles {
        active: daemon.get_property("ActiveProfile").await?,
        available,
        battery_aware: daemon.get_property("BatteryAware").await.ok(),
    })
}

const PROFILE_ORDER: [&str; 3] = ["power-saver", "balanced", "performance"];

pub fn set_profile(before: &str, after: &str) -> Change {
    let set = |name: &str| Cmd::new("powerprofilesctl").args(["set", name]);
    Change::new(set(after)).undo(set(before))
}

pub fn set_battery_aware(on: bool) -> Change {
    let set = |on: bool| {
        let flag = if on { "--enable" } else { "--disable" };
        Cmd::new("powerprofilesctl").args(["configure-battery-aware", flag])
    };
    Change::new(set(on)).undo(set(!on))
}

/// Returns the screen brightness in percent, or `None` on desktops.
pub async fn brightness() -> Option<f32> {
    let text = query("brightnessctl", ["-m", "-c", "backlight"]).await?;
    text.lines().next()?.split(',').nth(3)?.trim_end_matches('%').parse().ok()
}

/// The floor keeps the panel readable. At zero some backlights switch off.
pub fn set_brightness(percent: f32) -> Cmd {
    Cmd::new("brightnessctl").args([
        "-q",
        "-c",
        "backlight",
        "set",
        &format!("{:.0}%", percent.max(1.0)),
    ])
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Logind {
    pub lid: String,
    pub power_key: String,
}

const LID: &str = "HandleLidSwitch";
const POWER_KEY: &str = "HandlePowerKey";

/// Actions offered for the lid and the power button, with their logind names.
pub const ACTIONS: [(&str, &str); 6] = [
    ("ignore", "Do nothing"),
    ("lock", "Lock"),
    ("suspend", "Suspend"),
    ("hibernate", "Hibernate"),
    ("poweroff", "Power off"),
    ("reboot", "Restart"),
];

/// Reads the effective values. Later drop-ins override earlier ones.
pub fn logind() -> Logind {
    let mut files = vec![
        PathBuf::from("/usr/lib/systemd/logind.conf"),
        PathBuf::from("/etc/systemd/logind.conf"),
    ];
    for dir in ["/usr/lib/systemd/logind.conf.d", "/etc/systemd/logind.conf.d"] {
        let mut drop_ins: Vec<_> =
            std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
        drop_ins.sort();
        files.extend(drop_ins);
    }
    let mut state = Logind { lid: "suspend".into(), power_key: "poweroff".into() };
    for text in files.iter().filter_map(read) {
        if let Some(value) = lookup(&text, LID) {
            state.lid = value.to_owned();
        }
        if let Some(value) = lookup(&text, POWER_KEY) {
            state.power_key = value.to_owned();
        }
    }
    state
}

/// Writes both actions to a drop-in and reloads logind in one authorisation.
pub fn set_logind(state: &Logind) -> Op {
    let text = format!(
        "# Written by HyprKontrol\n[Login]\n{LID}={}\n{POWER_KEY}={}\n",
        state.lid, state.power_key
    );
    let script =
        format!("install -Dm644 /dev/stdin {LOGIND_DROP_IN} && systemctl reload systemd-logind");
    Cmd::new("sh").args(["-c", &script]).stdin(text).root().into()
}

/// Whether the machine has a lid, judged by logind's own inhibitor source.
pub fn has_lid() -> bool {
    Path::new("/proc/acpi/button/lid").is_dir() || battery().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_remaining_time() {
        let battery = Battery {
            status: "Charging".into(),
            now: 8.2,
            full: 36.1,
            design: 51.3,
            watts: 37.3,
            ..Battery::default()
        };
        assert_eq!(battery.summary(), "Charging at 37 W, full in 45 min");
        assert!((battery.health() - 0.7).abs() < 0.01);
        assert_eq!(duration(1.5), "1 h 30 min");
    }
}
