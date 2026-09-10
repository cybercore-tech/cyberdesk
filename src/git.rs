//! Thin wrapper over the `git` CLI in the vault working tree.

use std::path::Path;
use std::process::Command;

fn run(root: &Path, args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new("git").arg("-C").arg(root).args(args).output()
}

fn out(root: &Path, args: &[&str]) -> Option<String> {
    let o = run(root, args).ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// One commit, ready for a template.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Commit {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub ago: String,
    pub subject: String,
}

/// The last `n` commits reachable from HEAD (empty if `dir` isn't a repo).
pub fn log(dir: &Path, n: usize) -> Vec<Commit> {
    // %x1f = unit separator, %x1e = record separator — safe against odd subjects
    let fmt = "--pretty=format:%h%x1f%an%x1f%ad%x1f%ar%x1f%s";
    let Some(raw) = out(dir, &["log", &format!("-{n}"), "--date=short", fmt]) else {
        return Vec::new();
    };
    raw.lines()
        .filter_map(|line| {
            let mut f = line.split('\u{1f}');
            Some(Commit {
                hash: f.next()?.to_string(),
                author: f.next()?.to_string(),
                date: f.next()?.to_string(),
                ago: f.next()?.to_string(),
                subject: f.next().unwrap_or_default().to_string(),
            })
        })
        .collect()
}

/// `(branch, web_url)` for `dir` — `web_url` is the `origin` remote rewritten
/// to an `https://…` browsable form when it looks like GitHub.
pub fn head_info(dir: &Path) -> (Option<String>, Option<String>) {
    let branch = out(dir, &["rev-parse", "--abbrev-ref", "HEAD"]).filter(|b| b != "HEAD");
    let web = out(dir, &["remote", "get-url", "origin"]).map(|u| web_url(&u));
    (branch, web)
}

/// `git@github.com:owner/repo.git` / `https://github.com/owner/repo.git`
/// → `https://github.com/owner/repo`
pub fn web_url(remote: &str) -> String {
    let s = remote.trim();
    let s = s
        .strip_prefix("git@github.com:")
        .map(|rest| format!("https://github.com/{rest}"))
        .unwrap_or_else(|| s.to_string());
    s.strip_suffix(".git").unwrap_or(s.as_str()).to_string()
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
