//! The vault: a directory of markdown notes. Every note is a `.md` file with an
//! optional YAML-ish frontmatter block (`title`, `tags`). No database — the
//! filesystem is the model, git is the history.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use walkdir::WalkDir;

/// A parsed note.
#[derive(Debug, Clone)]
pub struct Note {
    pub rel: String,
    pub title: String,
    pub tags: Vec<String>,
    /// markdown body (frontmatter stripped)
    pub body: String,
}

/// One row in the sidebar tree.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TreeItem {
    pub rel: String,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
}

/// A search hit.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Hit {
    pub rel: String,
    pub title: String,
    pub snippet: String,
}

/// Reject anything that could escape the vault or isn't a markdown note.
pub fn safe_rel(rel: &str) -> Result<String> {
    let rel = rel.trim_start_matches('/');
    let bad = |c: char| c.is_control() || matches!(c, '"' | '<' | '>' | '\\' | '#' | '?' | '\0');
    if rel.is_empty()
        || rel.contains("..")
        || rel.starts_with('.')
        || rel.contains("/.")
        || rel.chars().any(bad)
        || Path::new(rel).is_absolute()
    {
        bail!("bad path: {rel}");
    }
    if !rel.ends_with(".md") {
        bail!("not a .md note: {rel}");
    }
    Ok(rel.to_string())
}

fn abs(root: &Path, rel: &str) -> Result<PathBuf> {
    Ok(root.join(safe_rel(rel)?))
}

// ── frontmatter ────────────────────────────────────────────────────────────
fn parse(raw: &str) -> (Option<String>, Vec<String>, String) {
    if let Some(rest) = raw.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---\n").or_else(|| rest.find("\n---")) {
            let fm = &rest[..end];
            let body = rest[end..].trim_start_matches('\n').trim_start_matches("---").trim_start_matches('\n');
            let mut title = None;
            let mut tags = Vec::new();
            for line in fm.lines() {
                let line = line.trim();
                if let Some(v) = line.strip_prefix("title:") {
                    title = Some(v.trim().trim_matches('"').trim_matches('\'').to_string());
                } else if let Some(v) = line.strip_prefix("tags:") {
                    tags = v
                        .trim()
                        .trim_start_matches('[')
                        .trim_end_matches(']')
                        .split(',')
                        .map(|s| s.trim().trim_matches('"').to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
            }
            return (title, tags, body.to_string());
        }
    }
    (None, Vec::new(), raw.to_string())
}

fn title_from(body: &str, rel: &str) -> String {
    for line in body.lines() {
        if let Some(h) = line.strip_prefix("# ") {
            return h.trim().to_string();
        }
    }
    Path::new(rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(rel)
        .replace(['-', '_'], " ")
}

// ── read / write / create / delete ────────────────────────────────────────
pub fn read(root: &Path, rel: &str) -> Result<Note> {
    let p = abs(root, rel)?;
    let raw = std::fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
    let (title, tags, body) = parse(&raw);
    let title = title.filter(|t| !t.is_empty()).unwrap_or_else(|| title_from(&body, rel));
    Ok(Note { rel: rel.to_string(), title, tags, body })
}

/// Raw file contents (frontmatter included) — what the editor shows.
pub fn read_raw(root: &Path, rel: &str) -> Result<String> {
    Ok(std::fs::read_to_string(abs(root, rel)?)?)
}

/// Overwrite a note with raw contents from the editor (frontmatter + body).
pub fn write_raw(root: &Path, rel: &str, raw: &str) -> Result<()> {
    let p = abs(root, rel)?;
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = raw.replace("\r\n", "\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    std::fs::write(&p, out)?;
    Ok(())
}

pub fn delete(root: &Path, rel: &str) -> Result<()> {
    std::fs::remove_file(abs(root, rel)?)?;
    Ok(())
}

pub fn exists(root: &Path, rel: &str) -> bool {
    safe_rel(rel).ok().map(|r| root.join(r).is_file()).unwrap_or(false)
}

/// Create a note from `_templates/<template>.md`, substituting `{{title}}`,
/// `{{date}}`, `{{slug}}`. Returns the new note's rel path.
pub fn create_from_template(
    root: &Path,
    template: &str,
    folder: &str,
    title: &str,
) -> Result<String> {
    let slug = slugify(title);
    if slug.is_empty() {
        bail!("empty title");
    }
    let folder = folder.trim_matches('/');
    if folder.contains("..") {
        bail!("bad folder");
    }
    let rel = if folder.is_empty() {
        format!("{slug}.md")
    } else {
        format!("{folder}/{slug}.md")
    };
    safe_rel(&rel)?;
    if root.join(&rel).exists() {
        bail!("a note named {rel} already exists");
    }

    let tpl_path = root.join("_templates").join(format!("{}.md", slugify(template)));
    let tpl = std::fs::read_to_string(&tpl_path)
        .unwrap_or_else(|_| "---\ntitle: \"{{title}}\"\ntags: []\ncreated: {{date}}\n---\n\n# {{title}}\n\n".into());
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let filled = tpl
        .replace("{{title}}", title)
        .replace("{{date}}", &today)
        .replace("{{slug}}", &slug);
    write_raw(root, &rel, &filled)?;
    Ok(rel)
}

pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in s.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

// ── tree / listing / search ──────────────────────────────────────────────
fn hidden(name: &str) -> bool {
    name.starts_with('.') || name == "scripts" || name == "book"
}

pub fn tree(root: &Path) -> Vec<TreeItem> {
    let mut items = Vec::new();
    for entry in WalkDir::new(root)
        .min_depth(1)
        .sort_by(|a, b| {
            (b.file_type().is_dir(), a.file_name()).cmp(&(a.file_type().is_dir(), b.file_name()))
        })
        .into_iter()
        .filter_entry(|e| !hidden(&e.file_name().to_string_lossy()))
    {
        let Ok(e) = entry else { continue };
        let rel = e.path().strip_prefix(root).unwrap().to_string_lossy().to_string();
        let is_dir = e.file_type().is_dir();
        if !is_dir && !rel.ends_with(".md") {
            continue;
        }
        if rel == "MANIFEST.md" || rel == "README.md" {
            continue;
        }
        items.push(TreeItem {
            name: e.file_name().to_string_lossy().trim_end_matches(".md").to_string(),
            depth: rel.matches('/').count(),
            is_dir,
            rel,
        });
    }
    items
}

/// Every note's rel path + title (for search index / "recent" / counts).
pub fn all_notes(root: &Path) -> Vec<Note> {
    let mut v: Vec<Note> = WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| !hidden(&e.file_name().to_string_lossy()))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.path().extension().map_or(false, |x| x == "md"))
        .filter_map(|e| {
            let rel = e.path().strip_prefix(root).ok()?.to_string_lossy().to_string();
            if rel == "MANIFEST.md" || rel == "README.md" || rel.starts_with("_templates/") {
                return None;
            }
            read(root, &rel).ok()
        })
        .collect();
    v.sort_by(|a, b| a.rel.to_lowercase().cmp(&b.rel.to_lowercase()));
    v
}

pub fn recent(root: &Path, n: usize) -> Vec<(String, String)> {
    let mut files: Vec<(std::time::SystemTime, String)> = WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| !hidden(&e.file_name().to_string_lossy()))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.path().extension().map_or(false, |x| x == "md"))
        .filter_map(|e| {
            let rel = e.path().strip_prefix(root).ok()?.to_string_lossy().to_string();
            if rel == "MANIFEST.md" || rel == "README.md" || rel.starts_with("_templates/") {
                return None;
            }
            let mt = e.metadata().ok()?.modified().ok()?;
            Some((mt, rel))
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files
        .into_iter()
        .take(n)
        .map(|(_, rel)| {
            let title = read(root, &rel).map(|nt| nt.title).unwrap_or_else(|_| rel.clone());
            (rel, title)
        })
        .collect()
}

pub fn templates(root: &Path) -> Vec<String> {
    let dir = root.join("_templates");
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.strip_suffix(".md").map(|s| s.to_string())
        })
        .collect();
    v.sort();
    v
}

pub fn folders(root: &Path) -> Vec<String> {
    let mut v: Vec<String> = WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| !hidden(&e.file_name().to_string_lossy()))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir())
        .filter_map(|e| {
            let rel = e.path().strip_prefix(root).ok()?.to_string_lossy().to_string();
            (rel != "_templates").then_some(rel)
        })
        .collect();
    v.sort();
    v
}

pub fn search(root: &Path, q: &str) -> Vec<Hit> {
    let ql = q.to_lowercase();
    if ql.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for n in all_notes(root) {
        let bl = n.body.to_lowercase();
        let tl = n.title.to_lowercase();
        if tl.contains(&ql) || bl.contains(&ql) || n.tags.iter().any(|t| t.to_lowercase().contains(&ql)) {
            let snippet = bl
                .find(&ql)
                .map(|i| {
                    let s = i.saturating_sub(40);
                    let e = (i + ql.len() + 80).min(n.body.len());
                    format!("…{}…", n.body[s..e].replace('\n', " "))
                })
                .unwrap_or_default();
            hits.push(Hit { rel: n.rel, title: n.title, snippet });
        }
        if hits.len() >= 100 {
            break;
        }
    }
    hits
}
