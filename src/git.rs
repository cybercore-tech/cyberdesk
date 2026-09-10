//! Thin wrapper over the `git` CLI in the vault working tree.

use std::path::Path;
use std::process::Command;

fn run(root: &Path, args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new("git").arg("-C").arg(root).args(args).output()
}

/// `git add -A` + commit. No-ops cleanly if there's nothing staged.
pub fn commit(root: &Path, message: &str) {
    if run(root, &["rev-parse", "--is-inside-work-tree"]).map(|o| o.status.success()).unwrap_or(false) {
        let _ = run(root, &["add", "-A"]);
        let out = run(root, &["commit", "-m", message]);
        match out {
            Ok(o) if o.status.success() => {
                tracing::info!("git: {}", String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or(""));
            }
            Ok(o) => {
                let s = String::from_utf8_lossy(&o.stdout);
                if !s.contains("nothing to commit") {
                    tracing::warn!("git commit: {}", s.trim());
                }
            }
            Err(e) => tracing::warn!("git commit failed: {e}"),
        }
    }
}

pub fn push(root: &Path) {
    match run(root, &["push"]) {
        Ok(o) if o.status.success() => tracing::info!("git: pushed"),
        Ok(o) => tracing::warn!("git push: {}", String::from_utf8_lossy(&o.stderr).trim()),
        Err(e) => tracing::warn!("git push failed: {e}"),
    }
}
