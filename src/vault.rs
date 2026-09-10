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

/// Options for [`create_note`].
pub struct NewNote<'a> {
    pub template: &'a str,
    pub folder: &'a str,
    pub title: &'a str,
    /// explicit file stem — slugified for safety; blank => slug of the title
    pub filename: Option<&'a str>,
    pub tags: &'a [String],
    /// extra `{{key}}` substitutions declared by a template
    pub fields: &'a std::collections::HashMap<String, String>,
    /// appended after the filled template body (e.g. pasted text)
    pub body: &'a str,
}

/// Create a note from `_templates/<template>.md`, substituting `{{title}}`,
/// `{{date}}`, `{{slug}}` and any `opts.fields`. Returns the new note's rel path.
pub fn create_note(root: &Path, opts: &NewNote) -> Result<String> {
    let stem_src = opts.filename.map(str::trim).filter(|s| !s.is_empty()).unwrap_or(opts.title);
    let slug = slugify(stem_src.trim_end_matches(".md"));
    if slug.is_empty() {
        bail!("need a title or file name");
    }
    let folder = opts.folder.trim_matches('/');
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

    let tpl_path = root.join("_templates").join(format!("{}.md", slugify(opts.template)));
    let tpl = std::fs::read_to_string(&tpl_path)
        .unwrap_or_else(|_| "---\ntitle: \"{{title}}\"\ntags: []\ncreated: {{date}}\n---\n\n# {{title}}\n\n".into());
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut filled = tpl
        .replace("{{title}}", opts.title)
        .replace("{{date}}", &today)
        .replace("{{slug}}", &slug);
    for (k, v) in opts.fields {
        filled = filled.replace(&format!("{{{{{k}}}}}"), v);
    }
    if !opts.tags.is_empty() {
        let list = opts.tags.join(", ");
        filled = filled
            .replacen("tags: []", &format!("tags: [{list}]"), 1)
            .replacen("tags:  []", &format!("tags: [{list}]"), 1);
    }
    let body = opts.body.trim();
    if !body.is_empty() {
        if !filled.ends_with('\n') {
            filled.push('\n');
        }
        if !filled.ends_with("\n\n") {
            filled.push('\n');
        }
        filled.push_str(body);
        filled.push('\n');
    }
    write_raw(root, &rel, &filled)?;
    Ok(rel)
}

/// Move/rename a note within the vault. The final path segment is slugified
/// (folders keep their casing); returns the new rel path.
pub fn rename(root: &Path, from: &str, to: &str) -> Result<String> {
    let from = safe_rel(from)?;
    let raw = to.trim().trim_start_matches('/').trim_end_matches(".md");
    let (dir, stem) = match raw.rsplit_once('/') {
        Some((d, s)) => (format!("{}/", d.trim_matches('/')), s),
        None => (String::new(), raw),
    };
    let stem = slugify(stem);
    if stem.is_empty() {
        bail!("destination needs a file name");
    }
    let to = safe_rel(&format!("{dir}{stem}.md"))?;
    if from == to {
        bail!("source and destination are the same");
    }
    let src = root.join(&from);
    let dst = root.join(&to);
    if !src.is_file() {
        bail!("no such note: {from}");
    }
    if dst.exists() {
        bail!("{to} already exists");
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&src, &dst)?;
    Ok(to)
}

/// Lower-cased, ascii-alphanumeric, dash-separated slug.
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in s.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.extend(ch.to_lowercase());
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

/// Nested tree for the collapsible sidebar.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Node {
    pub name: String,
    pub rel: String,
    pub is_dir: bool,
    pub children: Vec<Node>,
    pub count: usize,
}

/// Cheap title read for the sidebar: frontmatter `title:` or the first H1,
/// else the filename stem with dashes/underscores turned into spaces.
fn quick_title(path: &Path, stem: &str) -> String {
    use std::io::{BufRead, BufReader};
    let humanized = || stem.replace(['-', '_'], " ");
    let Ok(f) = std::fs::File::open(path) else { return humanized() };
    let mut r = BufReader::new(f);
    let mut line = String::new();
    let (mut in_fm, mut first) = (false, true);
    for _ in 0..60 {
        line.clear();
        if r.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let t = line.trim();
        if first {
            first = false;
            if t == "---" {
                in_fm = true;
                continue;
            }
        }
        if in_fm {
            if t == "---" {
                in_fm = false;
            } else if let Some(v) = t.strip_prefix("title:") {
                let v = v.trim().trim_matches('"').trim_matches('\'').trim();
                if !v.is_empty() && !v.contains("{{") {
                    return v.to_string();
                }
            }
        } else if let Some(h) = t.strip_prefix("# ") {
            let h = h.trim();
            if !h.contains("{{") {
                return h.to_string();
            }
        }
    }
    humanized()
}

pub fn tree_nested(root: &Path) -> Vec<Node> {
    fn build(dir: &Path, root: &Path) -> Vec<Node> {
        let mut dirs: Vec<Node> = Vec::new();
        let mut files: Vec<Node> = Vec::new();
        let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if hidden(&name) {
                continue;
            }
            let rel = e.path().strip_prefix(root).unwrap().to_string_lossy().to_string();
            if e.path().is_dir() {
                let children = build(&e.path(), root);
                let count = children.iter().map(|c| if c.is_dir { c.count } else { 1 }).sum();
                dirs.push(Node { name, rel, is_dir: true, children, count });
            } else if name.ends_with(".md") && name != "MANIFEST.md" && name != "README.md" {
                let stem = name.trim_end_matches(".md");
                files.push(Node {
                    name: quick_title(&e.path(), stem),
                    rel,
                    is_dir: false,
                    children: Vec::new(),
                    count: 0,
                });
            }
        }
        dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        dirs.into_iter().chain(files).collect()
    }
    build(root, root)
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

/// Byte-clamp `b` down/up to the nearest char boundary of `s`.
fn floor_boundary(s: &str, mut b: usize) -> usize {
    b = b.min(s.len());
    while b > 0 && !s.is_char_boundary(b) {
        b -= 1;
    }
    b
}
fn ceil_boundary(s: &str, mut b: usize) -> usize {
    b = b.min(s.len());
    while b < s.len() && !s.is_char_boundary(b) {
        b += 1;
    }
    b
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
        if tl.contains(&ql)
            || bl.contains(&ql)
            || n.tags.iter().any(|t| t.to_lowercase().contains(&ql))
        {
            let snippet = bl
                .find(&ql)
                .map(|i| {
                    let s = floor_boundary(&bl, i.saturating_sub(40));
                    let e = ceil_boundary(&bl, i + ql.len() + 80);
                    format!("…{}…", bl[s..e].replace('\n', " ").trim())
                })
                .unwrap_or_default();
            hits.push(Hit { rel: n.rel, title: n.title, snippet });
        }
        if hits.len() >= 200 {
            break;
        }
    }
    hits
}

/// Every distinct tag, sorted, with a count.
pub fn tag_counts(root: &Path) -> Vec<(String, usize)> {
    use std::collections::BTreeMap;
    let mut m: BTreeMap<String, usize> = BTreeMap::new();
    for n in all_notes(root) {
        for t in n.tags {
            *m.entry(t).or_default() += 1;
        }
    }
    m.into_iter().collect()
}

/// Notes carrying a given tag.
pub fn by_tag(root: &Path, tag: &str) -> Vec<Note> {
    all_notes(root)
        .into_iter()
        .filter(|n| n.tags.iter().any(|t| t == tag))
        .collect()
}

/// All note titles (for the search datalist / link completion).
pub fn titles(root: &Path) -> Vec<(String, String)> {
    all_notes(root).into_iter().map(|n| (n.rel, n.title)).collect()
}
