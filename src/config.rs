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
    /// When set, the sidebar shows this text instead of the cyberdesk
    /// wordmark image — `site_name` alone only ever changed `<title>`/
    /// `alt`, not the actual logo graphic, which stayed the compiled-in
    /// cyberdesk SVG regardless. `None` (the default, and always for the
    /// primary vault) keeps the original image exactly as before.
    pub logo_text: Option<String>,
    /// When set, an absolute path to a custom favicon SVG read from disk
    /// at request time (not baked in) — so a second deployment can have
    /// its own tab icon without a rebuild. Falls back to the compiled-in
    /// cyberdesk favicon when unset or unreadable.
    pub favicon_path: Option<PathBuf>,
    /// Where blueprint *templates* (schema definitions, reusable across
    /// projects) live. Deliberately outside `root` (the vault) — nothing
    /// under here is a darknotes note, and `vault::all_notes`/
    /// `tree_nested` would wrongly sweep it in if it sat inside the vault.
    /// Per-project *instances*' own data lives inside each project's own
    /// target directory instead (`<target>/.blueprint/instance.json`),
    /// not here — this only holds the shared template definitions plus a
    /// small index of which target directories are tracked.
    pub blueprints_dir: PathBuf,
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
            logo_text: std::env::var("CYBERDESK_LOGO_TEXT").ok(),
            favicon_path: std::env::var("CYBERDESK_FAVICON_PATH")
                .ok()
                .map(PathBuf::from),
            blueprints_dir: std::env::var("CYBERDESK_BLUEPRINTS_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| cybercore::paths::sysops_root().join("cyberdesk/_blueprints")),
        }
    }
}
