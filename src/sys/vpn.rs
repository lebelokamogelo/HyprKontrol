//! Importing VPN profiles, and logging in to the ones that need a password.

use std::path::{Path, PathBuf};

use super::op::tilde;
use super::{Cmd, Result, query};

/// Directives whose value is a file, relative to the profile.
const FILES: [&str; 9] = [
    "ca",
    "cert",
    "key",
    "pkcs12",
    "tls-auth",
    "tls-crypt",
    "extra-certs",
    "crl-verify",
    "auth-user-pass",
];

/// An OpenVPN profile in a form NetworkManager can import.
#[derive(Debug, PartialEq, Eq)]
struct Prepared {
    text: String,
    /// Directives that were left out.
    dropped: Vec<String>,
}

/// Leaves out what NetworkManager's importer rejects.
///
/// It refuses a whole profile over one directive it does not know. Firewall
/// vendors routinely add `route remote_host … net_gateway`, which keeps traffic
/// to the VPN server itself out of the tunnel. NetworkManager creates that route
/// on its own for every VPN, so nothing is lost by dropping the line.
///
/// The result is imported from another folder, hence the absolute file paths.
fn prepare(text: &str, dir: &Path) -> Prepared {
    let mut lines = Vec::new();
    let mut dropped = Vec::new();
    let mut inline = false;
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        if line.trim_start().starts_with('<') {
            inline = !line.trim_start().starts_with("</");
        }
        match words.as_slice() {
            ["route", "remote_host", ..] if !inline => dropped.push(line.trim().to_owned()),
            [directive, file, rest @ ..] if !inline && FILES.contains(directive) => {
                let path = Path::new(file.trim_matches('"'));
                let absolute = dir.join(path);
                if path.is_relative() && absolute.is_file() {
                    let mut words = vec![(*directive).to_owned()];
                    words.push(format!("\"{}\"", absolute.display()));
                    words.extend(rest.iter().map(|word| (*word).to_owned()));
                    lines.push(words.join(" "));
                } else {
                    lines.push(line.to_owned());
                }
            }
            _ => lines.push(line.to_owned()),
        }
    }
    Prepared { text: lines.join("\n") + "\n", dropped }
}

/// Adds a profile from a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub path: PathBuf,
    pub wireguard: bool,
}

impl Import {
    /// Tells the formats apart by content, as both are often named `.conf`.
    pub fn new(path: PathBuf) -> Result<Self> {
        let text = std::fs::read_to_string(&path)
            .map_err(|e| super::Error::new(format!("Could not read {}: {e}", tilde(&path))))?;
        let wireguard = text.lines().any(|line| line.trim() == "[Interface]");
        Ok(Self { path, wireguard })
    }

    pub fn describe(&self) -> String {
        self.command(&self.path).to_string()
    }

    fn command(&self, path: &Path) -> Cmd {
        let kind = if self.wireguard { "wireguard" } else { "openvpn" };
        let path = path.display().to_string();
        Cmd::new("nmcli").args(["connection", "import", "type", kind, "file", &path])
    }

    pub async fn run(&self) -> Result<()> {
        if self.wireguard {
            return self.command(&self.path).run().await;
        }
        let text = std::fs::read_to_string(&self.path)?;
        let prepared = prepare(&text, self.path.parent().unwrap_or(Path::new("/")));
        if prepared.dropped.is_empty() {
            return self.command(&self.path).run().await;
        }
        // The profile is named after the file, so the copy keeps its name.
        let copy = PrivateCopy::new(&self.path, &prepared.text)?;
        self.command(&copy.path).run().await
    }
}

/// A short-lived file that only the user can read. Profiles hold private keys.
struct PrivateCopy {
    dir: PathBuf,
    path: PathBuf,
}

impl PrivateCopy {
    fn new(original: &Path, text: &str) -> Result<Self> {
        use std::io::Write;
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

        // The runtime directory is in memory, so the key never reaches the disk.
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .ok_or_else(|| super::Error::new("XDG_RUNTIME_DIR is not set"))?;
        let dir = Path::new(&runtime).join(format!("hyprkontrol-import-{}", std::process::id()));
        std::fs::DirBuilder::new().mode(0o700).recursive(true).create(&dir)?;
        let copy =
            Self { path: dir.join(original.file_name().unwrap_or("profile.ovpn".as_ref())), dir };
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&copy.path)?;
        file.write_all(text.as_bytes())?;
        Ok(copy)
    }
}

impl Drop for PrivateCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.dir);
    }
}

/// What a profile needs before it can connect.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Login {
    pub username: String,
    pub needs_password: bool,
}

pub async fn login(uuid: &str) -> Login {
    let args = ["-t", "-e", "no", "-f", "vpn.data", "connection", "show", "uuid", uuid];
    parse_login(&query("nmcli", args).await.unwrap_or_default())
}

/// Parses `vpn.data:connection-type = password-tls, password-flags = 1, …`.
fn parse_login(text: &str) -> Login {
    let data = text.trim().strip_prefix("vpn.data:").unwrap_or_default();
    let get = |key: &str| {
        data.split(',').find_map(|entry| {
            let (name, value) = entry.split_once('=')?;
            (name.trim() == key).then(|| value.trim())
        })
    };
    let uses_password = get("connection-type").is_some_and(|kind| kind.starts_with("password"));
    // Flag 0 means NetworkManager has the password stored, 4 that none is needed.
    let stored = matches!(get("password-flags"), Some("0" | "4"));
    Login {
        username: get("username").unwrap_or_default().to_owned(),
        needs_password: uses_password && !stored,
    }
}

/// Saves the user name, then connects. The password is handed over through
/// standard input and is not kept.
pub fn connect(uuid: &str, before: &Login, username: &str, password: &str) -> Vec<Cmd> {
    let mut commands = Vec::new();
    if username != before.username {
        let entry = format!("username={username}");
        commands.push(Cmd::new("nmcli").args([
            "connection",
            "modify",
            "uuid",
            uuid,
            "+vpn.data",
            &entry,
        ]));
    }
    let up = ["connection", "up", "uuid", uuid, "passwd-file", "/dev/stdin"];
    commands.push(Cmd::new("nmcli").args(up).stdin(format!("vpn.secrets.password:{password}\n")));
    commands
}

/// User names are stored in a comma-separated list, without a way to quote.
pub fn valid_username(name: &str) -> bool {
    !name.is_empty() && !name.contains([',', '\n'])
}

#[cfg(test)]
mod tests {
    use super::*;

    const VENDOR: &str = "client\ndev tun\nremote vpn.example.com 8443\n\
        route remote_host 255.255.255.255 net_gateway\nauth-user-pass\n\
        <ca>\nroute remote_host is not a directive in here\n</ca>\nverb 3\n";

    #[test]
    fn drops_the_vendor_route_and_nothing_else() {
        let prepared = prepare(VENDOR, Path::new("/nonexistent"));
        assert_eq!(prepared.dropped, ["route remote_host 255.255.255.255 net_gateway"]);
        let expected = VENDOR.replace("route remote_host 255.255.255.255 net_gateway\n", "");
        assert_eq!(prepared.text, expected);
        assert!(prepare("route 10.0.0.0 255.0.0.0\n", Path::new("/")).dropped.is_empty());
    }

    #[test]
    fn file_references_survive_the_move() {
        let dir = std::env::temp_dir().join(format!("hyprkontrol-vpn-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ca.crt"), "x").unwrap();
        let prepared = prepare("ca ca.crt\nkey missing.key\nauth-user-pass\n", &dir);
        let absolute = format!("ca \"{}\"", dir.join("ca.crt").display());
        assert_eq!(prepared.text, format!("{absolute}\nkey missing.key\nauth-user-pass\n"));
    }

    #[test]
    fn recognises_profiles_that_ask_for_a_password() {
        let login = parse_login(
            "vpn.data:connection-type = password-tls, password-flags = 1, username = ada\n",
        );
        assert_eq!(login, Login { username: "ada".into(), needs_password: true });
        assert!(!parse_login("vpn.data:connection-type = tls").needs_password);
        assert!(
            !parse_login("vpn.data:connection-type = password, password-flags = 0").needs_password
        );
    }

    #[test]
    fn passwords_never_show_up_in_commands() {
        let commands = connect("id", &Login::default(), "ada", "hunter22");
        let shown: Vec<_> = commands.iter().map(ToString::to_string).collect();
        assert!(!shown.concat().contains("hunter22"));
        assert!(!format!("{commands:?}").contains("hunter22"));
        assert_eq!(commands.len(), 2);
        let known = Login { username: "ada".into(), needs_password: true };
        assert_eq!(connect("id", &known, "ada", "x").len(), 1);
    }

    /// Imports a throwaway profile and removes it again:
    ///
    ///     VPN_PROFILE=/path/to/test.ovpn cargo test imports_a_vendor_profile -- --ignored
    #[tokio::test]
    #[ignore = "adds and removes a NetworkManager profile"]
    async fn imports_a_vendor_profile() {
        let path = PathBuf::from(std::env::var("VPN_PROFILE").unwrap());
        let name = path.file_stem().unwrap().to_str().unwrap().to_owned();
        Import::new(path).unwrap().run().await.unwrap();
        let list = query("nmcli", ["-t", "-f", "UUID,NAME", "connection", "show"]).await.unwrap();
        let uuid = list
            .lines()
            .find_map(|line| line.strip_suffix(&format!(":{name}")))
            .expect("the profile should be listed")
            .to_owned();
        let login = login(&uuid).await;
        query("nmcli", ["connection", "delete", "uuid", &uuid]).await.unwrap();
        assert!(login.needs_password);
    }
}
