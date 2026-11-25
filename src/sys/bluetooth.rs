//! Bluetooth through `bluetoothctl`, which ships with BlueZ itself.

use super::{Change, Cmd, Op, Result, query, rfkill};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Device {
    pub address: String,
    pub name: String,
    pub icon: String,
    pub paired: bool,
    pub connected: bool,
    pub trusted: bool,
    pub battery: Option<u8>,
}

/// What keeps the radio from being powered, besides its own setting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Block {
    #[default]
    None,
    Software,
    /// A switch or the firmware. Nothing here can change it.
    Hardware,
}

impl Block {
    pub fn current() -> Self {
        let radios = rfkill::of_kind("bluetooth");
        if radios.iter().any(|radio| radio.hard) {
            Block::Hardware
        } else if radios.iter().any(|radio| radio.soft) {
            Block::Software
        } else {
            Block::None
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bluetooth {
    /// `None` when the machine has no adapter or the service is stopped.
    pub powered: Option<bool>,
    pub block: Block,
    pub discoverable: bool,
    pub name: String,
    pub devices: Vec<Device>,
}

fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| line.trim().strip_prefix(key)?.strip_prefix(": "))
}

fn flag(text: &str, key: &str) -> bool {
    field(text, key) == Some("yes")
}

pub async fn load() -> Result<Bluetooth> {
    let Some(adapter) = query("bluetoothctl", ["show"]).await else {
        return Ok(Bluetooth::default());
    };
    let listing = query("bluetoothctl", ["devices"]).await.unwrap_or_default();
    let mut devices = Vec::new();
    for address in listing.lines().filter_map(|line| line.split_whitespace().nth(1)) {
        if let Some(info) = query("bluetoothctl", ["info", address]).await {
            devices.push(parse_device(address, &info));
        }
    }
    devices.sort_by_key(|d| (!d.connected, !d.paired, d.name.to_lowercase()));
    Ok(Bluetooth {
        powered: field(&adapter, "Powered").map(|value| value == "yes"),
        block: Block::current(),
        discoverable: flag(&adapter, "Discoverable"),
        name: field(&adapter, "Alias").unwrap_or_default().to_owned(),
        devices,
    })
}

fn parse_device(address: &str, info: &str) -> Device {
    Device {
        address: address.to_owned(),
        name: field(info, "Alias").or_else(|| field(info, "Name")).unwrap_or(address).to_owned(),
        icon: field(info, "Icon").unwrap_or_default().to_owned(),
        paired: flag(info, "Paired"),
        connected: flag(info, "Connected"),
        trusted: flag(info, "Trusted"),
        // Reported as "0x64 (100)".
        battery: field(info, "Battery Percentage")
            .and_then(|value| value.split(['(', ')']).nth(1))
            .and_then(|value| value.parse().ok()),
    }
}

fn power(on: bool) -> Cmd {
    Cmd::new("bluetoothctl").args(["power", if on { "on" } else { "off" }])
}

/// Turns the radio on or off, lifting a software block if there is one.
///
/// The adapter takes a moment to come back after the block is gone, and refuses
/// to power on until it has. Undo restores the block along with the power state.
pub fn set_power(on: bool, block: Block) -> Change {
    let mut start = vec![power(true).retry(8)];
    let mut stop = vec![power(false)];
    if block == Block::Software {
        start.insert(0, rfkill::unblock("bluetooth"));
        if on {
            stop.push(rfkill::block("bluetooth"));
        }
    }
    let (apply, undo) = if on { (start, stop) } else { (stop, start) };
    Change::new(Op::Run(apply)).undo(Op::Run(undo))
}

/// Phrases the errors of BlueZ, which are D-Bus names meant for programs.
pub fn explain(reason: &str) -> String {
    const KNOWN: [(&str, &str); 9] = [
        ("page-timeout", "The device did not answer. Check that it is on and nearby"),
        ("ConnectionAttemptFailed", "The device did not answer. Check that it is on and nearby"),
        ("profile-unavailable", "The device offers nothing this computer can use"),
        ("AuthenticationFailed", "Pairing was refused. Put the device in pairing mode again"),
        ("AuthenticationRejected", "Pairing was refused. Put the device in pairing mode again"),
        ("AuthenticationTimeout", "Pairing took too long. Put the device in pairing mode again"),
        ("AuthenticationCanceled", "Pairing was cancelled on the device"),
        ("AlreadyExists", "The device is already paired"),
        ("NotReady", "Bluetooth is off"),
    ];
    if reason.starts_with("Failed to set power on") {
        return "The Bluetooth adapter would not turn on".to_owned();
    }
    let known = KNOWN.iter().find(|(code, _)| reason.contains(code));
    known.map_or_else(|| reason.to_owned(), |(_, text)| (*text).to_owned())
}

pub fn discoverable(on: bool) -> Cmd {
    Cmd::new("bluetoothctl").args(["discoverable", if on { "on" } else { "off" }])
}

/// Searches for `seconds`, during which new devices appear in the listing.
pub fn scan(seconds: u32) -> Cmd {
    Cmd::new("bluetoothctl").args(["--timeout", &seconds.to_string(), "scan", "on"])
}

pub fn connect(address: &str) -> Cmd {
    Cmd::new("bluetoothctl").args(["connect", address])
}

pub fn disconnect(address: &str) -> Cmd {
    Cmd::new("bluetoothctl").args(["disconnect", address])
}

pub fn trust(address: &str, on: bool) -> Cmd {
    Cmd::new("bluetoothctl").args([if on { "trust" } else { "untrust" }, address])
}

pub fn remove(address: &str) -> Cmd {
    Cmd::new("bluetoothctl").args(["remove", address])
}

/// Pairs, trusts and connects.
///
/// Trust is what lets a device reconnect by itself later. The sequence covers
/// devices that need no confirmation; one that displays a code has to be paired
/// in a terminal, where the code can be compared.
pub fn pair(address: &str) -> Vec<Cmd> {
    vec![Cmd::new("bluetoothctl").args(["pair", address]), trust(address, true), connect(address)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powering_on_lifts_a_software_block_and_undo_restores_it() {
        let change = set_power(true, Block::Software);
        assert_eq!(change.apply.describe(), "rfkill unblock bluetooth && bluetoothctl power on");
        assert_eq!(
            change.undo.unwrap().describe(),
            "bluetoothctl power off && rfkill block bluetooth"
        );
        let change = set_power(true, Block::None);
        assert_eq!(change.apply.describe(), "bluetoothctl power on");
    }

    /// Turns the radio on and puts it back the way it was:
    ///
    ///     cargo test powers_on_and_back -- --ignored
    #[tokio::test]
    #[ignore = "needs a Bluetooth adapter that is off"]
    async fn powers_on_and_back() {
        let before = load().await.unwrap();
        assert_eq!(before.powered, Some(false), "the adapter has to be off to start with");
        let change = set_power(true, before.block);
        let applied = change.apply.apply().await;
        let during = load().await.unwrap();
        change.undo.unwrap().apply().await.unwrap();
        let after = load().await.unwrap();
        applied.unwrap();
        assert_eq!(during.powered, Some(true));
        assert_eq!((after.powered, after.block), (before.powered, before.block));
    }

    #[test]
    fn errors_are_phrased_for_people() {
        assert_eq!(
            explain("Failed to connect: org.bluez.Error.Failed br-connection-page-timeout"),
            "The device did not answer. Check that it is on and nearby"
        );
        assert_eq!(
            explain("Failed to set power on: org.bluez.Error.Failed"),
            "The Bluetooth adapter would not turn on"
        );
        assert_eq!(explain("something new"), "something new");
    }

    #[test]
    fn parses_device_info() {
        let info = "Device AA:BB (public)\n\tName: Buds\n\tAlias: My Buds\n\tIcon: audio-headset\n\t\
                    Paired: yes\n\tTrusted: no\n\tConnected: yes\n\tBattery Percentage: 0x50 (80)\n";
        let device = parse_device("AA:BB", info);
        assert_eq!(device.name, "My Buds");
        assert_eq!(device.battery, Some(80));
        assert!(device.paired && device.connected && !device.trusted);
    }
}
