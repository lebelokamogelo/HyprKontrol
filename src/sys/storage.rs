//! Disks, file systems and reclaimable space.

use std::path::Path;

use serde::Deserialize;

use super::{Cmd, Result, cache_dir, query};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    pub mount: String,
    pub filesystem: String,
    pub used: u64,
    pub size: u64,
}

impl Volume {
    pub fn free(&self) -> u64 {
        self.size.saturating_sub(self.used)
    }

    pub fn fraction(&self) -> f32 {
        if self.size == 0 { 0.0 } else { self.used as f32 / self.size as f32 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disk {
    pub name: String,
    pub model: String,
    pub size: u64,
    pub rotational: bool,
    pub volumes: Vec<Volume>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Storage {
    pub disks: Vec<Disk>,
    pub package_cache: u64,
    pub journal: u64,
    pub user_cache: u64,
}

#[derive(Deserialize)]
struct Listing {
    blockdevices: Vec<Block>,
}

#[derive(Deserialize)]
struct Block {
    name: String,
    size: u64,
    #[serde(rename = "type")]
    kind: String,
    fstype: Option<String>,
    #[serde(default)]
    mountpoints: Vec<Option<String>>,
    model: Option<String>,
    #[serde(default)]
    fsused: Option<Number>,
    #[serde(default)]
    fssize: Option<Number>,
    rota: bool,
    #[serde(default)]
    children: Vec<Block>,
}

/// lsblk switched these fields from strings to numbers between releases.
#[derive(Deserialize, Clone)]
#[serde(untagged)]
enum Number {
    Int(u64),
    Text(String),
}

impl Number {
    fn get(&self) -> u64 {
        match self {
            Number::Int(value) => *value,
            Number::Text(text) => text.parse().unwrap_or(0),
        }
    }
}

/// Collects mounted file systems below a disk, through LUKS and LVM layers.
fn volumes(block: &Block, found: &mut Vec<Volume>) {
    // A Btrfs partition is mounted once per subvolume. The root stands for all.
    let mounts: Vec<_> = block.mountpoints.iter().flatten().collect();
    let mount = mounts.iter().find(|m| **m == "/").or_else(|| mounts.first());
    if let (Some(mount), Some(size)) = (mount, &block.fssize) {
        found.push(Volume {
            mount: (**mount).clone(),
            filesystem: block.fstype.clone().unwrap_or_default(),
            used: block.fsused.as_ref().map_or(0, Number::get),
            size: size.get(),
        });
    }
    for child in &block.children {
        volumes(child, found);
    }
}

fn parse(text: &str) -> Result<Vec<Disk>> {
    let listing: Listing = serde_json::from_str(text)?;
    Ok(listing
        .blockdevices
        .iter()
        .filter(|block| block.kind == "disk" && !block.name.starts_with("zram"))
        .map(|block| {
            let mut found = Vec::new();
            volumes(block, &mut found);
            Disk {
                name: block.name.clone(),
                model: block.model.clone().unwrap_or_default().trim().to_owned(),
                size: block.size,
                rotational: block.rota,
                volumes: found,
            }
        })
        .collect())
}

/// Disks and their file systems. Cheap, unlike [`load`].
pub async fn disks() -> Result<Vec<Disk>> {
    let columns = "NAME,SIZE,TYPE,FSTYPE,MOUNTPOINTS,MODEL,FSUSED,FSSIZE,ROTA";
    parse(&Cmd::new("lsblk").args(["-J", "-b", "-o", columns]).output().await?)
}

/// The file system mounted at `/`.
pub fn root(disks: &[Disk]) -> Option<&Volume> {
    disks.iter().flat_map(|disk| &disk.volumes).find(|volume| volume.mount == "/")
}

/// Disks together with the size of the caches, which takes a walk over every
/// file in them.
pub async fn load() -> Result<Storage> {
    let disks = disks().await?;
    let cache = cache_dir();
    let (package_cache, user_cache, journal) = tokio::join!(
        folder_size(Path::new("/var/cache/pacman/pkg")),
        folder_size(&cache),
        folder_size(Path::new("/var/log/journal")),
    );
    Ok(Storage { disks, package_cache, journal, user_cache })
}

async fn folder_size(path: &Path) -> u64 {
    // Unreadable subfolders make du exit with an error after printing a total,
    // so its output is used regardless of the status.
    let output = tokio::process::Command::new("du")
        .args(["-sb", "--"])
        .arg(path)
        .stderr(std::process::Stdio::null())
        .output()
        .await;
    output
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().next()?.parse().ok())
        .unwrap_or(0)
}

pub fn clean_packages() -> Cmd {
    Cmd::new("paccache").args(["-rk2"]).root()
}

pub fn clean_journal() -> Cmd {
    Cmd::new("journalctl").arg("--vacuum-size=200M").root()
}

pub async fn trim_enabled() -> bool {
    query("systemctl", ["is-enabled", "fstrim.timer"]).await.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_volumes_through_encryption() {
        let text = r#"{"blockdevices":[
            {"name":"zram0","size":1,"type":"disk","fstype":"swap","mountpoints":["[SWAP]"],"model":null,"rota":false},
            {"name":"nvme0n1","size":500,"type":"disk","fstype":null,"mountpoints":[],"model":"SSD ","rota":false,
             "children":[{"name":"p2","size":400,"type":"part","fstype":"crypto_LUKS","mountpoints":[null],"model":null,"rota":false,
              "children":[{"name":"root","size":400,"type":"crypt","fstype":"btrfs","mountpoints":["/home","/"],"model":null,
                           "fsused":100,"fssize":"400","rota":false}]}]}]}"#;
        let disks = parse(text).unwrap();
        assert_eq!(disks.len(), 1);
        assert_eq!(disks[0].model, "SSD");
        let volume = &disks[0].volumes[0];
        assert_eq!((volume.mount.as_str(), volume.free()), ("/", 300));
        assert!((volume.fraction() - 0.25).abs() < 1e-6);
    }
}
