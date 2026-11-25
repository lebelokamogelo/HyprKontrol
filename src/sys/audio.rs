//! Sound devices and streams, through PulseAudio's protocol.
//!
//! `pactl` is used over `wpctl` because it speaks JSON and works the same on
//! PipeWire and PulseAudio.

use serde::Deserialize;
use std::collections::BTreeMap;

use super::{Cmd, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Output,
    Input,
}

impl Direction {
    fn noun(self) -> &'static str {
        match self {
            Direction::Output => "sink",
            Direction::Input => "source",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub name: String,
    pub label: String,
    pub volume: f32,
    pub muted: bool,
    pub default: bool,
    pub headphones: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    pub index: u32,
    pub app: String,
    pub volume: f32,
    pub muted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub name: String,
    pub label: String,
    pub profile: String,
    /// Profile id and description, limited to what the hardware offers now.
    pub profiles: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Audio {
    pub outputs: Vec<Device>,
    pub inputs: Vec<Device>,
    pub streams: Vec<Stream>,
    pub cards: Vec<Card>,
}

impl Audio {
    pub fn output(&self) -> Option<&Device> {
        self.outputs.iter().find(|device| device.default)
    }

    pub fn input(&self) -> Option<&Device> {
        self.inputs.iter().find(|device| device.default)
    }
}

#[derive(Deserialize)]
struct Info {
    default_sink_name: String,
    default_source_name: String,
}

#[derive(Deserialize)]
struct Channel {
    value_percent: String,
}

#[derive(Deserialize)]
struct Port {
    name: String,
    description: String,
    #[serde(default)]
    availability: String,
    #[serde(default, rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct RawDevice {
    name: String,
    description: String,
    mute: bool,
    volume: BTreeMap<String, Channel>,
    #[serde(default)]
    ports: Vec<Port>,
    active_port: Option<String>,
}

#[derive(Deserialize)]
struct RawStream {
    index: u32,
    mute: bool,
    #[serde(default)]
    volume: BTreeMap<String, Channel>,
    #[serde(default)]
    properties: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct RawProfile {
    description: String,
    available: bool,
}

#[derive(Deserialize)]
struct RawCard {
    name: String,
    active_profile: String,
    profiles: BTreeMap<String, RawProfile>,
    #[serde(default)]
    properties: BTreeMap<String, String>,
}

/// Channels can differ when the balance is off centre. The loudest one is the
/// volume, which is also how pavucontrol and the GNOME slider treat it.
fn level(volume: &BTreeMap<String, Channel>) -> f32 {
    volume
        .values()
        .filter_map(|channel| channel.value_percent.trim_end_matches('%').trim().parse().ok())
        .fold(0.0, f32::max)
}

fn device(raw: RawDevice, default: &str) -> Option<Device> {
    let port = raw.ports.iter().find(|port| Some(&port.name) == raw.active_port.as_ref());
    let is_default = raw.name == default;
    // Laptops expose an HDMI sink per connector. Hide the ones with no cable,
    // unless that sink is selected and the user needs to see why it is silent.
    if !is_default && port.is_some_and(|port| port.availability == "not available") {
        return None;
    }
    Some(Device {
        label: port.map_or(raw.description, |port| port.description.clone()),
        headphones: port.is_some_and(|port| port.kind.contains("Head")),
        volume: level(&raw.volume),
        muted: raw.mute,
        default: is_default,
        name: raw.name,
    })
}

async fn list<T: serde::de::DeserializeOwned>(kind: &str) -> Result<Vec<T>> {
    let reply = Cmd::new("pactl").args(["-f", "json", "list", kind]).output().await?;
    Ok(serde_json::from_str(&reply)?)
}

pub async fn load() -> Result<Audio> {
    let info = Cmd::new("pactl").args(["-f", "json", "info"]).output().await?;
    let info: Info = serde_json::from_str(&info)?;
    let (sinks, sources, streams, cards) = tokio::join!(
        list::<RawDevice>("sinks"),
        list::<RawDevice>("sources"),
        list::<RawStream>("sink-inputs"),
        list::<RawCard>("cards"),
    );
    Ok(Audio {
        outputs: sinks?
            .into_iter()
            .filter_map(|raw| device(raw, &info.default_sink_name))
            .collect(),
        inputs: sources?
            .into_iter()
            // Every sink has a monitor source, which is not a microphone.
            .filter(|raw| !raw.name.ends_with(".monitor"))
            .filter_map(|raw| device(raw, &info.default_source_name))
            .collect(),
        streams: streams?
            .into_iter()
            .map(|raw| Stream {
                index: raw.index,
                app: raw
                    .properties
                    .get("application.name")
                    .or_else(|| raw.properties.get("media.name"))
                    .cloned()
                    .unwrap_or_else(|| "Unknown application".to_owned()),
                volume: level(&raw.volume),
                muted: raw.mute,
            })
            .collect(),
        cards: cards?
            .into_iter()
            .map(|raw| Card {
                label: raw
                    .properties
                    .get("device.description")
                    .cloned()
                    .unwrap_or_else(|| raw.name.clone()),
                profiles: raw
                    .profiles
                    .into_iter()
                    .filter(|(name, profile)| profile.available || *name == raw.active_profile)
                    .map(|(name, profile)| (name, profile.description))
                    .collect(),
                profile: raw.active_profile,
                name: raw.name,
            })
            .collect(),
    })
}

pub fn set_volume(direction: Direction, name: &str, percent: f32) -> Cmd {
    Cmd::new("pactl")
        .arg(format!("set-{}-volume", direction.noun()))
        .arg(name)
        .arg(format!("{percent:.0}%"))
}

pub fn set_mute(direction: Direction, name: &str, muted: bool) -> Cmd {
    Cmd::new("pactl").arg(format!("set-{}-mute", direction.noun())).arg(name).arg(if muted {
        "1"
    } else {
        "0"
    })
}

pub fn set_default(direction: Direction, name: &str) -> Cmd {
    Cmd::new("pactl").arg(format!("set-default-{}", direction.noun())).arg(name)
}

pub fn set_stream_volume(index: u32, percent: f32) -> Cmd {
    Cmd::new("pactl").args(["set-sink-input-volume", &index.to_string(), &format!("{percent:.0}%")])
}

pub fn set_stream_mute(index: u32, muted: bool) -> Cmd {
    Cmd::new("pactl").args([
        "set-sink-input-mute",
        &index.to_string(),
        if muted { "1" } else { "0" },
    ])
}

pub fn set_profile(card: &str, profile: &str) -> Cmd {
    Cmd::new("pactl").args(["set-card-profile", card, profile])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(name: &str, availability: &str) -> RawDevice {
        serde_json::from_value(serde_json::json!({
            "name": name, "description": "Long card name Speaker", "mute": false,
            "volume": {"front-left": {"value_percent": "65%"}, "front-right": {"value_percent": "40%"}},
            "ports": [{"name": "[Out] Speaker", "description": "Speaker",
                       "availability": availability, "type": "Speaker"}],
            "active_port": "[Out] Speaker",
        }))
        .unwrap()
    }

    #[test]
    fn unplugged_devices_are_hidden_unless_selected() {
        assert!(device(raw("hdmi", "not available"), "speaker").is_none());
        assert!(device(raw("hdmi", "not available"), "hdmi").is_some());
        let speaker = device(raw("speaker", "available"), "speaker").unwrap();
        assert_eq!((speaker.label.as_str(), speaker.volume), ("Speaker", 65.0));
    }
}
