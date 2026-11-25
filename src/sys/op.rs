//! The unit of work a panel hands to the application.

use std::path::PathBuf;

use super::{Cmd, Result, edit, hypr, network, vpn};

/// Something that modifies the system.
#[derive(Debug, Clone)]
pub enum Op {
    /// Commands run in order, stopping at the first failure.
    Run(Vec<Cmd>),
    Hypr(Vec<hypr::Edit>),
    /// Replaces a configuration file, then makes its owner pick it up.
    Write {
        path: PathBuf,
        text: String,
        then: Vec<Cmd>,
    },
    /// Joining a network is the one write that goes over D-Bus, because nmcli
    /// accepts passwords only as arguments.
    Join(network::Join),
    ImportVpn(vpn::Import),
}

impl Op {
    pub async fn apply(self) -> Result<()> {
        match self {
            Op::Run(commands) => run(&commands).await,
            Op::Hypr(edits) => hypr::apply(edits).await,
            Op::Write { path, text, then } => {
                edit::backup_once(&path)?;
                edit::write_atomic(&path, &text)?;
                run(&then).await
            }
            Op::Join(join) => join.activate().await,
            Op::ImportVpn(import) => import.run().await,
        }
    }

    /// One line for the receipt bar.
    pub fn describe(&self) -> String {
        match self {
            Op::Run(commands) => {
                commands.iter().map(ToString::to_string).collect::<Vec<_>>().join(" && ")
            }
            Op::Hypr(edits) => {
                edits.iter().map(hypr::Edit::describe).collect::<Vec<_>>().join(", ")
            }
            Op::Write { path, .. } => format!("edit {}", tilde(path)),
            Op::Join(join) => join.describe(),
            Op::ImportVpn(import) => import.describe(),
        }
    }
}

async fn run(commands: &[Cmd]) -> Result<()> {
    for command in commands {
        command.run().await?;
    }
    Ok(())
}

impl From<Cmd> for Op {
    fn from(command: Cmd) -> Self {
        Op::Run(vec![command])
    }
}

/// Shortens paths below the home directory.
pub fn tilde(path: &std::path::Path) -> String {
    match path.strip_prefix(super::home()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// An operation together with the one that reverts it.
#[derive(Debug, Clone)]
pub struct Change {
    pub apply: Op,
    pub undo: Option<Op>,
}

impl Change {
    pub fn new(apply: impl Into<Op>) -> Self {
        Self { apply: apply.into(), undo: None }
    }

    pub fn undo(mut self, undo: impl Into<Op>) -> Self {
        self.undo = Some(undo.into());
        self
    }

    /// Rewrites a file, with Undo restoring its current contents.
    pub fn write(path: PathBuf, text: String, then: Vec<Cmd>) -> Self {
        let before = std::fs::read_to_string(&path).unwrap_or_default();
        Self::new(Op::Write { path: path.clone(), text, then: then.clone() }).undo(Op::Write {
            path,
            text: before,
            then,
        })
    }
}
