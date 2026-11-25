//! Live readings and hardware facts, straight from `/proc` and `/sys`.

use std::path::PathBuf;

use super::{lookup, query, read, read_number};

/// Cumulative jiffies of one CPU. Load is the change between two samples.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Jiffies {
    total: u64,
    idle: u64,
}

impl Jiffies {
    /// Share of time spent busy since `earlier`, from 0 to 1.
    pub fn load_since(self, earlier: Self) -> f32 {
        let total = self.total.saturating_sub(earlier.total);
        let idle = self.idle.saturating_sub(earlier.idle);
        if total == 0 { 0.0 } else { 1.0 - idle as f32 / total as f32 }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sample {
    pub cpu: Jiffies,
    pub threads: Vec<Jiffies>,
    pub ghz: f32,
    pub celsius: Option<f32>,
    pub memory_used: u64,
    pub memory_total: u64,
}

pub fn sample() -> Sample {
    let stat = read("/proc/stat").unwrap_or_default();
    let mut lines = stat.lines().filter(|line| line.starts_with("cpu")).map(jiffies);
    let memory = read("/proc/meminfo").unwrap_or_default();
    let total = memory_field(&memory, "MemTotal");
    Sample {
        cpu: lines.next().unwrap_or_default(),
        threads: lines.collect(),
        ghz: average_clock(),
        celsius: temperature(),
        memory_used: total.saturating_sub(memory_field(&memory, "MemAvailable")),
        memory_total: total,
    }
}

/// Fields 4 and 5 are idle and iowait. Only the first eight count towards the
/// total, since guest time is already included in user time.
fn jiffies(line: &str) -> Jiffies {
    let values: Vec<u64> =
        line.split_whitespace().skip(1).take(8).filter_map(|v| v.parse().ok()).collect();
    Jiffies {
        total: values.iter().sum(),
        idle: values.get(3).unwrap_or(&0) + values.get(4).unwrap_or(&0),
    }
}

fn memory_field(text: &str, key: &str) -> u64 {
    text.lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(':')?.split_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
        * 1024
}

fn cpu_dirs() -> Vec<PathBuf> {
    (0..1024)
        .map(|n| PathBuf::from(format!("/sys/devices/system/cpu/cpu{n}")))
        .take_while(|path| path.exists())
        .collect()
}

fn average_clock() -> f32 {
    let clocks: Vec<f32> = cpu_dirs()
        .iter()
        .filter_map(|dir| read_number(dir.join("cpufreq/scaling_cur_freq")))
        .map(|khz| khz as f32 / 1e6)
        .collect();
    if clocks.is_empty() { 0.0 } else { clocks.iter().sum::<f32>() / clocks.len() as f32 }
}

/// Package temperature. Zone names differ between vendors, so the known CPU
/// zones are tried in order and hwmon covers AMD.
fn temperature() -> Option<f32> {
    const ZONES: [&str; 3] = ["x86_pkg_temp", "TCPU", "cpu-thermal"];
    let zones: Vec<_> =
        std::fs::read_dir("/sys/class/thermal").ok()?.flatten().map(|e| e.path()).collect();
    let zone = ZONES.iter().find_map(|name| {
        let path = zones.iter().find(|path| read(path.join("type")).as_deref() == Some(*name))?;
        read_number(path.join("temp"))
    });
    let hwmon = || {
        std::fs::read_dir("/sys/class/hwmon").ok()?.flatten().map(|e| e.path()).find_map(|path| {
            let name = read(path.join("name"))?;
            matches!(name.as_str(), "k10temp" | "zenpower" | "coretemp")
                .then(|| read_number(path.join("temp1_input")))
                .flatten()
        })
    };
    zone.or_else(hwmon).map(|milli| milli as f32 / 1000.0)
}

/// Threads grouped by their maximum clock, fastest group first.
///
/// This separates performance from efficiency cores without a table of CPU
/// models. A uniform processor yields a single group.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Topology {
    pub groups: Vec<(f32, usize)>,
    pub max_ghz: f32,
}

pub fn topology() -> Topology {
    let mut groups: Vec<(f32, usize)> = Vec::new();
    for dir in cpu_dirs() {
        let ghz = read_number(dir.join("cpufreq/cpuinfo_max_freq")).unwrap_or(0.0) as f32 / 1e6;
        // Favoured cores boost slightly higher than their siblings, which does
        // not make them a different kind of core.
        match groups.last_mut() {
            Some((known, count)) if (*known - ghz).abs() < 0.4 => {
                *count += 1;
                *known = known.max(ghz);
            }
            _ => groups.push((ghz, 1)),
        }
    }
    let max_ghz = groups.iter().map(|(ghz, _)| *ghz).fold(0.0, f32::max);
    Topology { groups, max_ghz }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Machine {
    pub os: String,
    pub kernel: String,
    pub hostname: String,
    pub model: String,
    pub firmware: String,
    pub cpu: String,
    pub threads: usize,
    pub gpus: Vec<String>,
    pub memory: u64,
    pub desktop: String,
    pub uptime: String,
}

pub async fn machine() -> Machine {
    let (lspci, hyprland) = tokio::join!(query("lspci", []), query("hyprctl", ["version"]));
    Machine {
        gpus: lspci.as_deref().map(parse_gpus).unwrap_or_default(),
        desktop: hyprland
            .and_then(|text| text.split_whitespace().nth(1).map(|v| format!("Hyprland {v}")))
            .unwrap_or_default(),
        ..identity()
    }
}

/// The part that only takes reading a few files.
pub fn identity() -> Machine {
    let cpuinfo = read("/proc/cpuinfo").unwrap_or_default();
    let dmi = |name: &str| read(format!("/sys/class/dmi/id/{name}")).unwrap_or_default();
    Machine {
        os: read("/etc/os-release")
            .and_then(|text| lookup(&text, "PRETTY_NAME").map(str::to_owned))
            .unwrap_or_else(|| "Linux".to_owned()),
        kernel: read("/proc/sys/kernel/osrelease").unwrap_or_default(),
        hostname: read("/proc/sys/kernel/hostname").unwrap_or_default(),
        model: model_name(&dmi("sys_vendor"), &dmi("product_name")),
        firmware: dmi("bios_version"),
        cpu: cpuinfo
            .lines()
            .find_map(|line| line.strip_prefix("model name"))
            .map(|value| short_cpu(value.trim_start_matches([' ', '\t', ':'])))
            .unwrap_or_default(),
        threads: cpu_dirs().len(),
        memory: memory_field(&read("/proc/meminfo").unwrap_or_default(), "MemTotal"),
        uptime: uptime(),
        ..Machine::default()
    }
}

/// Joins vendor and product. Some vendors repeat their name in the product.
fn model_name(vendor: &str, product: &str) -> String {
    if product.starts_with(vendor) {
        product.to_owned()
    } else {
        format!("{vendor} {product}").trim().to_owned()
    }
}

/// Drops trademark noise and the generation prefix: the model number says both.
fn short_cpu(name: &str) -> String {
    let name = name.replace("(R)", "").replace("(TM)", "").replace(" CPU", "");
    let name = name.split(" @").next().unwrap_or(&name);
    match name.find("Core") {
        Some(start) => name[start..].trim().to_owned(),
        None => name.trim().to_owned(),
    }
}

fn parse_gpus(lspci: &str) -> Vec<String> {
    lspci
        .lines()
        .filter(|line| {
            ["VGA compatible", "3D controller", "Display controller"]
                .iter()
                .any(|k| line.contains(k))
        })
        .filter_map(|line| line.split_once(": ").map(|(_, name)| name))
        .map(|name| {
            let name = name.split(" (rev").next().unwrap_or(name);
            // "Corporation GA104 [GeForce RTX 3070]": the marketing name is in brackets.
            let vendor = name.split_whitespace().next().unwrap_or_default();
            match name.rsplit_once('[').and_then(|(_, tail)| tail.split(']').next()) {
                Some(model) => format!("{vendor} {model}"),
                None => name.to_owned(),
            }
        })
        .collect()
}

pub fn uptime() -> String {
    let seconds = read("/proc/uptime")
        .and_then(|text| text.split('.').next().and_then(|s| s.parse::<u64>().ok()))
        .unwrap_or(0);
    match (seconds / 86400, seconds / 3600 % 24, seconds / 60 % 60) {
        (0, 0, m) => format!("{m} min"),
        (0, h, m) => format!("{h} h {m} min"),
        (d, h, _) => format!("{d} d {h} h"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_ignores_guest_time() {
        let before = jiffies("cpu0 100 0 0 900 0 0 0 0 500 0");
        let after = jiffies("cpu0 150 0 0 950 0 0 0 0 900 0");
        assert!((after.load_since(before) - 0.5).abs() < 1e-6);
        assert_eq!(after.load_since(after), 0.0);
    }

    #[test]
    fn shortens_hardware_names() {
        assert_eq!(model_name("HP", "HP ProBook 450"), "HP ProBook 450");
        assert_eq!(model_name("LENOVO", "20XW"), "LENOVO 20XW");
        assert_eq!(short_cpu("12th Gen Intel(R) Core(TM) i7-1255U"), "Core i7-1255U");
        assert_eq!(
            short_cpu("AMD Ryzen 7 7840U w/ Radeon 780M Graphics"),
            "AMD Ryzen 7 7840U w/ Radeon 780M Graphics"
        );
        let gpus = parse_gpus(
            "00:02.0 VGA compatible controller: Intel Corporation Alder Lake-UP3 GT2 [UHD Graphics] (rev 0c)\n00:1f.3 Audio: x",
        );
        assert_eq!(gpus, ["Intel UHD Graphics"]);
    }
}
