//! The uncomplicated firewall.
//!
//! `ufw status` needs root. The files behind it are world-readable, so the panel
//! can show state without asking for a password just to look.

use super::{Change, Cmd, installed, lookup, read};

const RULES: &str = "/etc/ufw/user.rules";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub action: String,
    pub protocol: String,
    pub port: String,
    pub source: String,
    pub incoming: bool,
}

impl Rule {
    pub fn title(&self) -> String {
        let port =
            if self.port == "any" { "All ports".to_owned() } else { format!("Port {}", self.port) };
        match self.protocol.as_str() {
            "any" => port,
            protocol => format!("{port} ({})", protocol.to_uppercase()),
        }
    }

    fn args(&self) -> Vec<String> {
        let direction = if self.incoming { "in" } else { "out" };
        let mut args = vec![self.action.clone(), direction.to_owned()];
        if self.protocol != "any" {
            args.extend(["proto".to_owned(), self.protocol.clone()]);
        }
        args.extend(["from".to_owned(), self.source.clone(), "to".to_owned(), "any".to_owned()]);
        if self.port != "any" {
            args.extend(["port".to_owned(), self.port.clone()]);
        }
        args
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Firewall {
    pub installed: bool,
    pub enabled: bool,
    pub incoming: String,
    pub outgoing: String,
    pub rules: Vec<Rule>,
}

pub fn load() -> Firewall {
    let defaults = read("/etc/default/ufw").unwrap_or_default();
    let policy = |key: &str| match lookup(&defaults, key) {
        Some("ACCEPT") => "allow",
        Some("REJECT") => "reject",
        _ => "deny",
    };
    Firewall {
        installed: installed("ufw"),
        enabled: read("/etc/ufw/ufw.conf")
            .is_some_and(|text| lookup(&text, "ENABLED") == Some("yes")),
        incoming: policy("DEFAULT_INPUT_POLICY").to_owned(),
        outgoing: policy("DEFAULT_OUTPUT_POLICY").to_owned(),
        rules: read(RULES).map(|text| parse_rules(&text)).unwrap_or_default(),
    }
}

/// ufw stores each rule as a comment it can read back:
/// `### tuple ### allow tcp 22 0.0.0.0/0 any 0.0.0.0/0 in`.
fn parse_rules(text: &str) -> Vec<Rule> {
    text.lines()
        .filter_map(|line| line.strip_prefix("### tuple ### "))
        .filter_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            let [action, protocol, port, _to, _source_port, source, direction, ..] =
                fields.as_slice()
            else {
                return None;
            };
            Some(Rule {
                action: (*action).to_owned(),
                protocol: (*protocol).to_owned(),
                port: (*port).to_owned(),
                source: if *source == "0.0.0.0/0" { "any".into() } else { (*source).to_owned() },
                // An interface may follow the direction, as in "in_eth0".
                incoming: direction.starts_with("in"),
            })
        })
        .collect()
}

pub fn set_enabled(on: bool) -> Change {
    let set = |on: bool| match on {
        // Without --force, ufw asks about SSH on its standard input.
        true => Cmd::new("ufw").args(["--force", "enable"]).root(),
        false => Cmd::new("ufw").arg("disable").root(),
    };
    Change::new(set(on)).undo(set(!on))
}

pub fn set_policy(direction: &str, before: &str, after: &str) -> Change {
    let set = |policy: &str| Cmd::new("ufw").args(["default", policy, direction]).root();
    Change::new(set(after)).undo(set(before))
}

pub fn add(rule: &Rule) -> Change {
    Change::new(Cmd::new("ufw").args(rule.args()).root())
        .undo(Cmd::new("ufw").arg("delete").args(rule.args()).root())
}

pub fn delete(rule: &Rule) -> Change {
    Change::new(Cmd::new("ufw").arg("delete").args(rule.args()).root())
        .undo(Cmd::new("ufw").args(rule.args()).root())
}

/// Accepts a port, a range or a comma-separated list, as ufw does.
pub fn valid_port(text: &str) -> bool {
    let port = |p: &str| p.parse::<u16>().is_ok_and(|n| n > 0);
    !text.is_empty()
        && text.split(',').all(|part| match part.split_once(':') {
            Some((low, high)) => port(low) && port(high),
            None => port(part),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_round_trip_to_commands() {
        let rules = parse_rules(
            "*filter\n### tuple ### allow tcp 22 0.0.0.0/0 any 0.0.0.0/0 in\n\
             ### tuple ### deny any any 0.0.0.0/0 any 10.0.0.0/8 out\n",
        );
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].title(), "Port 22 (TCP)");
        assert_eq!(
            add(&rules[0]).apply.describe(),
            "pkexec ufw allow in proto tcp from any to any port 22"
        );
        assert_eq!(
            delete(&rules[1]).apply.describe(),
            "pkexec ufw delete deny out from 10.0.0.0/8 to any"
        );
    }

    #[test]
    fn validates_ports() {
        assert!(valid_port("22") && valid_port("8000:8010") && valid_port("80,443"));
        assert!(!valid_port("") && !valid_port("0") && !valid_port("70000") && !valid_port("ssh"));
    }
}
