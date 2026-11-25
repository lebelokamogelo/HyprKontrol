//! Kernel-level radio switches.
//!
//! A radio can be blocked below the daemon that drives it. BlueZ then refuses to
//! power on with nothing but "Failed", and it stays that way across reboots,
//! because systemd saves the switches at shutdown and restores them at boot.
//!
//! Soft blocks are software and can be lifted; the active user is allowed to do
//! that without a password. Hard blocks come from a physical switch or the
//! firmware and can only be reported.

use std::path::Path;

use super::{Change, Cmd, read};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Radio {
    pub id: u32,
    pub kind: String,
    pub soft: bool,
    pub hard: bool,
}

impl Radio {
    pub fn blocked(&self) -> bool {
        self.soft || self.hard
    }
}

pub fn radios() -> Vec<Radio> {
    load(Path::new("/sys/class/rfkill"))
}

fn load(dir: &Path) -> Vec<Radio> {
    let mut radios: Vec<Radio> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let flag = |name: &str| read(path.join(name)).as_deref() == Some("1");
            Some(Radio {
                id: entry.file_name().to_str()?.strip_prefix("rfkill")?.parse().ok()?,
                kind: read(path.join("type"))?,
                soft: flag("soft"),
                hard: flag("hard"),
            })
        })
        .collect();
    radios.sort_by_key(|radio| radio.id);
    radios
}

/// The switches of one kind, such as `bluetooth`.
pub fn of_kind(kind: &str) -> Vec<Radio> {
    radios().into_iter().filter(|radio| radio.kind == kind).collect()
}

/// Airplane mode is on when there are radios and none of them may transmit.
pub fn airplane(radios: &[Radio]) -> bool {
    !radios.is_empty() && radios.iter().all(Radio::blocked)
}

fn switch(block: bool, targets: &[String]) -> Cmd {
    Cmd::new("rfkill").arg(if block { "block" } else { "unblock" }).args(targets.iter().cloned())
}

pub fn block(kind: &str) -> Cmd {
    switch(true, &[kind.to_owned()])
}

pub fn unblock(kind: &str) -> Cmd {
    switch(false, &[kind.to_owned()])
}

/// Blocks or unblocks everything.
///
/// Undo touches only the radios that were in the other state before. Leaving
/// airplane mode by Undo must not switch on a radio that had been off all along.
pub fn set_airplane(radios: &[Radio], on: bool) -> Change {
    let moved: Vec<String> =
        radios.iter().filter(|radio| radio.soft != on).map(|radio| radio.id.to_string()).collect();
    let change = Change::new(switch(on, &["all".to_owned()]));
    if moved.is_empty() { change } else { change.undo(switch(!on, &moved)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn radio(id: u32, kind: &str, soft: bool) -> Radio {
        Radio { id, kind: kind.to_owned(), soft, hard: false }
    }

    #[test]
    fn airplane_needs_every_radio_off() {
        assert!(!airplane(&[]));
        assert!(!airplane(&[radio(0, "bluetooth", true), radio(1, "wlan", false)]));
        assert!(airplane(&[radio(0, "bluetooth", true), radio(1, "wlan", true)]));
    }

    #[test]
    fn undo_leaves_radios_alone_that_were_off_before() {
        let radios = [radio(0, "bluetooth", true), radio(1, "wlan", false)];
        let change = set_airplane(&radios, true);
        assert_eq!(change.apply.describe(), "rfkill block all");
        assert_eq!(change.undo.unwrap().describe(), "rfkill unblock 1");
    }

    #[test]
    fn reads_the_kernel_layout() {
        let dir = std::env::temp_dir().join(format!("hyprkontrol-rfkill-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (name, kind, soft) in [("rfkill1", "wlan", "0"), ("rfkill0", "bluetooth", "1")] {
            let path = dir.join(name);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("type"), format!("{kind}\n")).unwrap();
            std::fs::write(path.join("soft"), format!("{soft}\n")).unwrap();
            std::fs::write(path.join("hard"), "0\n").unwrap();
        }
        assert_eq!(load(&dir), [radio(0, "bluetooth", true), radio(1, "wlan", false)]);
    }
}
