//! Runtime config from the environment.

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    /// vault root — the darknotes working tree
    pub root: PathBuf,
    pub bind: String,
    /// `git push` after every commit (default: off — commit locally only)
    pub auto_push: bool,
    /// Brand shown in the page `<title>` and topbar — lets the same binary
    /// serve a second vault (e.g. cyberdeck's diagnostics output) under
    /// its own identity instead of every deployment saying "cyberdesk".
    pub site_name: String,
}

impl Config {
    pub fn from_env() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let root = std::env::var("CYBERDESK_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(home).join("Vaults/darknotes"));
        Self {
            root,
            bind: std::env::var("CYBERDESK_BIND").unwrap_or_else(|_| "127.0.0.1:8765".into()),
            auto_push: matches!(
                std::env::var("CYBERDESK_PUSH").as_deref(),
                Ok("1") | Ok("true") | Ok("yes")
            ),
            site_name: std::env::var("CYBERDESK_SITE_NAME").unwrap_or_else(|_| "cyberdesk".into()),
        }
    }
}
