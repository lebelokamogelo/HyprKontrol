//! NetworkManager.
//!
//! Reads and simple writes use `nmcli`, whose terse output is stable and whose
//! commands are worth showing to the user. Joining a network is done over D-Bus
//! so that the password never appears in a process listing.

use std::collections::HashMap;

use zbus::zvariant::{OwnedObjectPath, Value};

use super::rfkill::{self, Radio};
use super::{Cmd, Result, query};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    Open,
    /// WPA or WPA2 with a passphrase.
    Psk,
    /// WPA3 only.
    Sae,
    /// Company and campus networks with a user name.
    Enterprise,
    /// WEP. Broken for two decades, so it is listed but cannot be joined here.
    Legacy,
}

impl Security {
    fn parse(text: &str) -> Self {
        if text.is_empty() || text == "--" || text.contains("OWE") {
            Security::Open
        } else if text.contains("802.1X") {
            Security::Enterprise
        } else if text.contains("WPA1") || text.contains("WPA2") {
            Security::Psk
        } else if text.contains("WPA3") {
            Security::Sae
        } else {
            Security::Legacy
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Security::Open => "Open",
            Security::Psk => "WPA2",
            Security::Sae => "WPA3",
            Security::Enterprise => "Enterprise",
            Security::Legacy => "WEP",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessPoint {
    pub ssid: String,
    pub signal: u8,
    pub security: Security,
    pub active: bool,
    pub five_ghz: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub uuid: String,
    pub name: String,
    pub kind: String,
    pub autoconnect: bool,
    pub active: bool,
}

impl Connection {
    pub fn is_vpn(&self) -> bool {
        matches!(self.kind.as_str(), "vpn" | "wireguard")
    }

    pub fn is_wifi(&self) -> bool {
        self.kind == "802-11-wireless"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub kind: String,
    pub state: String,
    pub connection: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Address {
    pub ip: String,
    pub gateway: String,
    pub dns: Vec<String>,
    pub hardware: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Network {
    pub wifi_present: bool,
    pub wifi_enabled: bool,
    pub enabled: bool,
    pub access_points: Vec<AccessPoint>,
    pub connections: Vec<Connection>,
    pub devices: Vec<Device>,
    pub address: Option<Address>,
    pub radios: Vec<Radio>,
}

impl Network {
    pub fn wifi_device(&self) -> Option<&Device> {
        self.devices.iter().find(|device| device.kind == "wifi")
    }

    pub fn wired_device(&self) -> Option<&Device> {
        self.devices.iter().find(|device| device.kind == "ethernet" && device.state != "unmanaged")
    }

    pub fn active_wifi(&self) -> Option<&AccessPoint> {
        self.access_points.iter().find(|ap| ap.active)
    }

    /// The saved profile for `ssid`. NetworkManager names profiles after the
    /// network unless the user renamed them.
    pub fn known(&self, ssid: &str) -> Option<&Connection> {
        self.connections.iter().find(|c| c.is_wifi() && c.name == ssid)
    }

    pub fn hotspot(&self) -> Option<&Connection> {
        self.connections.iter().find(|c| c.name == HOTSPOT && c.active)
    }
}

pub const HOTSPOT: &str = "Hotspot";

/// Splits a terse line. nmcli escapes `:` and `\` inside values.
fn split_terse(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        let current = fields.last_mut().expect("there is always a current field");
        match c {
            '\\' => current.extend(chars.next()),
            ':' => fields.push(String::new()),
            c => current.push(c),
        }
    }
    fields
}

async fn table<const N: usize>(columns: &str, args: [&str; N]) -> Vec<Vec<String>> {
    Cmd::new("nmcli")
        .args(["-t", "-f", columns])
        .args(args)
        .output()
        .await
        .map(|text| text.lines().map(split_terse).collect())
        .unwrap_or_default()
}

pub async fn load() -> Result<Network> {
    // Asking for the version first turns "not installed" and "not running" into
    // one clear error instead of a panel full of empty lists.
    Cmd::new("nmcli").args(["-t", "-f", "RUNNING", "general"]).run().await?;

    let (general, devices, connections, access_points) = tokio::join!(
        table("WIFI-HW,WIFI,NETWORKING", ["general"]),
        table("DEVICE,TYPE,STATE,CONNECTION", ["device"]),
        table("UUID,NAME,TYPE,AUTOCONNECT,ACTIVE", ["connection", "show"]),
        table("IN-USE,SSID,SIGNAL,SECURITY,FREQ", ["device", "wifi", "list", "--rescan", "no"]),
    );
    let general = general.into_iter().next().unwrap_or_default();
    let at = |fields: &[String], index: usize| fields.get(index).cloned().unwrap_or_default();

    let devices: Vec<_> = devices
        .iter()
        .map(|f| Device { name: at(f, 0), kind: at(f, 1), state: at(f, 2), connection: at(f, 3) })
        .collect();
    let connected = devices.iter().find(|device| {
        matches!(device.kind.as_str(), "wifi" | "ethernet") && device.state == "connected"
    });
    let address = match connected {
        Some(device) => address(&device.name).await,
        None => None,
    };
    Ok(Network {
        wifi_present: devices.iter().any(|device| device.kind == "wifi"),
        wifi_enabled: at(&general, 1) == "enabled",
        enabled: at(&general, 2) == "enabled",
        access_points: merge(access_points),
        connections: connections
            .iter()
            .map(|f| Connection {
                uuid: at(f, 0),
                name: at(f, 1),
                kind: at(f, 2),
                autoconnect: at(f, 3) == "yes",
                active: at(f, 4) == "yes",
            })
            .collect(),
        devices,
        address,
        radios: rfkill::radios(),
    })
}

/// Collapses the radios of one network into a single entry and drops hidden ones.
fn merge(rows: Vec<Vec<String>>) -> Vec<AccessPoint> {
    let mut merged: Vec<AccessPoint> = Vec::new();
    for row in rows {
        let [in_use, ssid, signal, security, freq] = row.as_slice() else { continue };
        if ssid.is_empty() {
            continue;
        }
        let found = AccessPoint {
            ssid: ssid.clone(),
            signal: signal.parse().unwrap_or(0),
            security: Security::parse(security),
            active: in_use == "*",
            five_ghz: freq.split_whitespace().next().and_then(|f| f.parse().ok()).unwrap_or(0)
                > 4000,
        };
        match merged.iter_mut().find(|known| known.ssid == found.ssid) {
            Some(known) => {
                known.active |= found.active;
                known.signal = known.signal.max(found.signal);
                known.five_ghz |= found.five_ghz;
            }
            None => merged.push(found),
        }
    }
    merged.sort_by_key(|ap| (!ap.active, std::cmp::Reverse(ap.signal)));
    merged
}

async fn address(device: &str) -> Option<Address> {
    let fields = "IP4.ADDRESS,IP4.GATEWAY,IP4.DNS,GENERAL.HWADDR";
    let text = query("nmcli", ["-t", "-f", fields, "device", "show", device]).await?;
    let mut address = Address::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.replace('\\', "");
        match key.split('[').next().unwrap_or(key) {
            "IP4.ADDRESS" if address.ip.is_empty() => address.ip = value,
            "IP4.GATEWAY" => address.gateway = value,
            "IP4.DNS" => address.dns.push(value),
            "GENERAL.HWADDR" => address.hardware = value,
            _ => {}
        }
    }
    Some(address)
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

pub fn set_wifi(on: bool) -> Cmd {
    Cmd::new("nmcli").args(["radio", "wifi", on_off(on)])
}

pub fn rescan() -> Cmd {
    Cmd::new("nmcli").args(["device", "wifi", "rescan"])
}

pub fn up(uuid: &str) -> Cmd {
    Cmd::new("nmcli").args(["connection", "up", "uuid", uuid])
}

pub fn down(uuid: &str) -> Cmd {
    Cmd::new("nmcli").args(["connection", "down", "uuid", uuid])
}

pub fn forget(uuid: &str) -> Cmd {
    Cmd::new("nmcli").args(["connection", "delete", "uuid", uuid])
}

pub fn set_autoconnect(uuid: &str, on: bool) -> Cmd {
    let value = if on { "yes" } else { "no" };
    Cmd::new("nmcli").args(["connection", "modify", "uuid", uuid, "connection.autoconnect", value])
}

/// How a new wireless profile authenticates.
#[derive(Clone, PartialEq, Eq)]
pub enum Auth {
    Open,
    Psk(String),
    Sae(String),
    Enterprise { identity: String, password: String, check_certificate: bool },
}

/// Creates a wireless profile and activates it.
#[derive(Clone, PartialEq, Eq)]
pub struct Join {
    pub device: String,
    pub ssid: String,
    pub auth: Auth,
    /// Share this machine's connection instead of joining one.
    pub hotspot: bool,
}

/// Written by hand so that a password cannot end up in a log.
impl std::fmt::Debug for Join {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Join").field("ssid", &self.ssid).finish_non_exhaustive()
    }
}

type Settings<'a> = HashMap<&'a str, HashMap<&'a str, Value<'a>>>;

impl Join {
    pub fn describe(&self) -> String {
        if self.hotspot {
            format!("start hotspot \"{}\"", self.ssid)
        } else {
            format!("connect to \"{}\"", self.ssid)
        }
    }

    fn settings(&self) -> Settings<'_> {
        let id = if self.hotspot { HOTSPOT } else { &self.ssid };
        let mut connection =
            HashMap::from([("id", Value::from(id)), ("type", Value::from("802-11-wireless"))]);
        let mut wireless = HashMap::from([("ssid", Value::from(self.ssid.as_bytes().to_vec()))]);
        let mut settings = Settings::new();
        if self.hotspot {
            connection.insert("autoconnect", Value::from(false));
            wireless.insert("mode", Value::from("ap"));
            let shared = || HashMap::from([("method", Value::from("shared"))]);
            settings.insert("ipv4", shared());
            settings.insert("ipv6", HashMap::from([("method", Value::from("ignore"))]));
        }
        let security = match &self.auth {
            Auth::Open => None,
            Auth::Psk(password) => Some(("wpa-psk", Some(password))),
            Auth::Sae(password) => Some(("sae", Some(password))),
            Auth::Enterprise { .. } => Some(("wpa-eap", None)),
        };
        if let Some((key_management, password)) = security {
            let mut section = HashMap::from([("key-mgmt", Value::from(key_management))]);
            if let Some(password) = password {
                section.insert("psk", Value::from(password.as_str()));
            }
            settings.insert("802-11-wireless-security", section);
        }
        if let Auth::Enterprise { identity, password, check_certificate } = &self.auth {
            settings.insert(
                "802-1x",
                HashMap::from([
                    ("eap", Value::from(vec!["peap"])),
                    ("phase2-auth", Value::from("mschapv2")),
                    ("identity", Value::from(identity.as_str())),
                    ("password", Value::from(password.as_str())),
                    ("system-ca-certs", Value::from(*check_certificate)),
                ]),
            );
        }
        settings.insert("connection", connection);
        settings.insert("802-11-wireless", wireless);
        settings
    }

    pub async fn activate(&self) -> Result<()> {
        const SERVICE: &str = "org.freedesktop.NetworkManager";
        let bus = zbus::Connection::system().await?;
        let manager =
            zbus::Proxy::new(&bus, SERVICE, "/org/freedesktop/NetworkManager", SERVICE).await?;
        let device: OwnedObjectPath =
            manager.call("GetDeviceByIpIface", &(self.device.as_str(),)).await?;
        let any = OwnedObjectPath::try_from("/").expect("the root path is valid");
        let _: (OwnedObjectPath, OwnedObjectPath) =
            manager.call("AddAndActivateConnection", &(self.settings(), device, any)).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_escaped_fields() {
        assert_eq!(split_terse(r"*:Caf\: Wi\\Fi:75"), ["*", r"Caf: Wi\Fi", "75"]);
    }

    #[test]
    fn merges_radios_of_one_network() {
        let row = |fields: [&str; 5]| fields.map(str::to_owned).to_vec();
        let merged = merge(vec![
            row(["", "Home", "40", "WPA2", "2437 MHz"]),
            row(["*", "Home", "70", "WPA2", "5180 MHz"]),
            row(["", "", "99", "WPA2", "2412 MHz"]),
            row(["", "Cafe", "90", "", "2412 MHz"]),
        ]);
        assert_eq!(merged.len(), 2);
        assert_eq!((merged[0].ssid.as_str(), merged[0].signal), ("Home", 70));
        assert!(merged[0].active && merged[0].five_ghz);
        assert_eq!(merged[1].security, Security::Open);
    }

    #[test]
    fn recognises_security() {
        assert_eq!(Security::parse("WPA2 802.1X"), Security::Enterprise);
        assert_eq!(Security::parse("WPA2 WPA3"), Security::Psk);
        assert_eq!(Security::parse("WPA3"), Security::Sae);
        assert_eq!(Security::parse("WEP"), Security::Legacy);
    }

    #[test]
    fn secrets_stay_out_of_descriptions() {
        let join = Join {
            device: "wlan0".into(),
            ssid: "Home".into(),
            auth: Auth::Psk("hunter22".into()),
            hotspot: false,
        };
        assert!(!format!("{join:?} {}", join.describe()).contains("hunter22"));
        assert!(join.settings().contains_key("802-11-wireless-security"));
    }

    /// NetworkManager rejects a profile outright when a value has another type.
    #[test]
    fn settings_have_the_types_networkmanager_expects() {
        use zbus::zvariant::DynamicType;

        let join = Join {
            device: "wlan0".into(),
            ssid: "Campus".into(),
            auth: Auth::Enterprise {
                identity: "ada".into(),
                password: "secret".into(),
                check_certificate: true,
            },
            hotspot: false,
        };
        let settings = join.settings();
        let signature =
            |section: &str, key: &str| settings[section][key].value_signature().to_string();
        assert_eq!(settings.signature().to_string(), "a{sa{sv}}");
        assert_eq!(signature("802-11-wireless", "ssid"), "ay");
        assert_eq!(signature("802-1x", "eap"), "as");
        assert_eq!(signature("802-1x", "system-ca-certs"), "b");
        assert_eq!(settings["802-11-wireless-security"]["key-mgmt"], Value::from("wpa-eap"));
    }
}
