//! Vault linter — scans every note for common problems and proposes fixes.
//! The mechanical fixes (whitespace, CRLF, blank runs, missing title) can be
//! applied from the `/lint` page; the rest are advisory.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use crate::vault;

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    /// stable id, e.g. "trailing_ws" — also the fix selector
    pub kind: &'static str,
    pub detail: String,
    /// mechanically fixable by `apply_fix`
    pub fixable: bool,
    /// for a diff-style preview (fixable issues only)
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NoteReport {
    pub rel: String,
    pub title: String,
    pub issues: Vec<Issue>,
}

fn is_pathy(s: &str) -> bool {
    let s = s.trim();
    s.contains(" | ")
        || s.contains("/usr/")
        || s.contains("/etc/")
        || s.contains("/sys/")
        || s.contains("${")
        || s.starts_with("http")
        || s.len() > 80
        || s.split_whitespace().next().map_or(false, |w| {
            matches!(
                w.to_lowercase().as_str(),
                "find" | "cat" | "sudo" | "curl" | "wget" | "run" | "hx" | "awk" | "sed" | "grep"
            )
        })
}

fn headings(body: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut fence = false;
    for line in body.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        if fence {
            continue;
        }
        let h = t.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&h) && t.chars().nth(h) == Some(' ') {
            out.push((h, t[h + 1..].trim().to_string()));
        }
    }
    out
}

/// The whole-vault scan.
pub fn scan(root: &Path) -> Vec<NoteReport> {
    let notes = vault::all_notes(root);

    // redundancy: group by normalized body hash
    let mut bodies: HashMap<String, Vec<String>> = HashMap::new();
    for n in &notes {
        let norm: String = n.body.split_whitespace().collect::<Vec<_>>().join(" ");
        if norm.len() > 40 {
            bodies.entry(norm).or_default().push(n.rel.clone());
        }
    }
    let dup_of: HashMap<String, Vec<String>> = bodies
        .into_values()
        .filter(|v| v.len() > 1)
        .flat_map(|v| {
            v.iter()
                .map(|r| (r.clone(), v.iter().filter(|x| *x != r).cloned().collect()))
                .collect::<Vec<_>>()
        })
        .collect();

    let existing: std::collections::HashSet<&str> = notes.iter().map(|n| n.rel.as_str()).collect();

    let mut reports = Vec::new();
    for n in &notes {
        let mut issues = Vec::new();
        let raw = vault::read_raw(root, &n.rel).unwrap_or_default();
        let hs = headings(&n.body);
        let stem = Path::new(&n.rel).file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let humanized = stem.replace(['-', '_'], " ");

        // ── title ──
        let fm_title = raw
            .strip_prefix("---\n")
            .and_then(|r| r.split("\n---").next())
            .and_then(|fm| {
                fm.lines()
                    .find_map(|l| l.trim().strip_prefix("title:").map(|v| v.trim().trim_matches('"').to_string()))
            });
        let title_str = fm_title.clone().unwrap_or_default();
        if title_str.is_empty() {
            issues.push(Issue {
                kind: "empty_title",
                detail: "no frontmatter title".into(),
                fixable: true,
                before: "title: \"\"".into(),
                after: format!("title: \"{humanized}\""),
            });
        } else if is_pathy(&title_str) {
            issues.push(Issue {
                kind: "pathy_title",
                detail: format!("title looks like a command/path: {title_str}"),
                fixable: true,
                before: format!("title: \"{title_str}\""),
                after: format!("title: \"{humanized}\""),
            });
        } else if let Some((1, h1)) = hs.first().cloned() {
            if title_str.to_lowercase() != h1.to_lowercase() {
                issues.push(Issue {
                    kind: "title_mismatch",
                    detail: format!("frontmatter “{title_str}” ≠ H1 “{h1}”"),
                    fixable: false,
                    before: String::new(),
                    after: String::new(),
                });
            }
        }

        // ── headings ──
        let h1s = hs.iter().filter(|(l, _)| *l == 1).count();
        if h1s == 0 {
            issues.push(Issue { kind: "no_h1", detail: "no H1 heading".into(), fixable: false, before: String::new(), after: String::new() });
        } else if h1s > 1 {
            issues.push(Issue { kind: "multi_h1", detail: format!("{h1s} H1 headings"), fixable: false, before: String::new(), after: String::new() });
        }
        let mut prev = 0usize;
        for (l, txt) in &hs {
            if prev != 0 && *l > prev + 1 {
                issues.push(Issue {
                    kind: "skipped_heading",
                    detail: format!("H{prev} → H{l} at “{txt}”"),
                    fixable: false,
                    before: String::new(),
                    after: String::new(),
                });
                break;
            }
            prev = *l;
        }

        // ── formatting ──
        if raw.contains('\r') {
            issues.push(Issue { kind: "crlf", detail: "CRLF line endings".into(), fixable: true, before: "\\r\\n".into(), after: "\\n".into() });
        }
        let tw = raw.lines().filter(|l| l.ends_with(' ') || l.ends_with('\t')).count();
        if tw > 0 {
            issues.push(Issue { kind: "trailing_ws", detail: format!("{tw} line(s) with trailing whitespace"), fixable: true, before: format!("{tw} lines"), after: "trimmed".into() });
        }
        if raw.contains("\n\n\n") {
            issues.push(Issue { kind: "blank_runs", detail: "3+ consecutive blank lines".into(), fixable: true, before: "\\n\\n\\n".into(), after: "\\n\\n".into() });
        }

        // ── links ──
        for cap in regex_lite_wikilinks(&n.body) {
            let target = if cap.ends_with(".md") { cap.clone() } else { format!("{cap}.md") };
            let hit = existing.iter().any(|r| r.ends_with(&target) || **r == cap);
            if !hit {
                issues.push(Issue {
                    kind: "broken_link",
                    detail: format!("[[{cap}]] → no matching note"),
                    fixable: false,
                    before: String::new(),
                    after: String::new(),
                });
            }
        }

        // ── hardware diagnostics (cyberdeck reports specifically) ──
        issues.extend(hardware_issues(&n.body));

        // ── redundancy ──
        if let Some(others) = dup_of.get(&n.rel) {
            issues.push(Issue {
                kind: "redundant",
                detail: format!("identical body to: {}", others.join(", ")),
                fixable: false,
                before: String::new(),
                after: String::new(),
            });
        }

        // ── filename ──
        if n.rel.contains(' ') || n.rel.contains(".md.md") {
            issues.push(Issue {
                kind: "bad_filename",
                detail: "spaces or double .md in the path".into(),
                fixable: false,
                before: String::new(),
                after: String::new(),
            });
        }

        if !issues.is_empty() {
            reports.push(NoteReport { rel: n.rel.clone(), title: n.title.clone(), issues });
        }
    }
    reports.sort_by(|a, b| b.issues.len().cmp(&a.issues.len()));
    reports
}

/// Cyberdeck-diagnostics-specific checks — a no-op against a regular
/// darknotes note (none of these literal patterns show up in prose), but
/// this is what turns "Cyberdeck Diagnostic Output" from a plain markdown
/// viewer into something that actually flags problems. Deliberately plain
/// string scanning, no regex dependency, matching the rest of this file —
/// extend here as cyberdeck's own report format grows new sections.
/// Thresholds are calibrated against this machine's own real reports
/// (e.g. a 74%-of-design battery genuinely should read as "aging").
fn hardware_issues(body: &str) -> Vec<Issue> {
    let mut out = Vec::new();
    let no_fix = String::new();

    // S.M.A.R.T. — cyberdeck can't read drive health without root, and a
    // genuine failure reads "FAILED" in smartctl's own output either way.
    if body.contains("smartctl") && body.contains("Permission denied") {
        out.push(Issue {
            kind: "smart_permission",
            detail: "S.M.A.R.T. check needs root — smartctl couldn't read drive health. https://wiki.archlinux.org/title/S.M.A.R.T.".into(),
            fixable: false,
            before: no_fix.clone(),
            after: no_fix.clone(),
        });
    }
    if body.contains("FAILED") {
        out.push(Issue {
            kind: "smart_failed",
            detail: "a FAILED result appears in this report — check the raw output above.".into(),
            fixable: false,
            before: no_fix.clone(),
            after: no_fix.clone(),
        });
    }

    // Thermal — "Zone N: NN.NN°C" / "Peak temperature: NN.NN°C" lines.
    for line in body.lines() {
        let Some(idx) = line.find("°C") else { continue };
        let head = &line[..idx];
        let Some(cut) = head.rfind(|c: char| !c.is_ascii_digit() && c != '.') else { continue };
        let Ok(temp) = head[cut + 1..].trim().parse::<f64>() else { continue };
        if temp >= 90.0 {
            out.push(Issue {
                kind: "thermal_critical",
                detail: format!("{temp:.1}°C is critically hot — check cooling/airflow. https://wiki.archlinux.org/title/Improving_performance#Overheating"),
                fixable: false,
                before: no_fix.clone(),
                after: no_fix.clone(),
            });
        } else if temp >= 80.0 {
            out.push(Issue {
                kind: "thermal_warning",
                detail: format!("{temp:.1}°C is running hot. https://wiki.archlinux.org/title/Improving_performance#Overheating"),
                fixable: false,
                before: no_fix.clone(),
                after: no_fix.clone(),
            });
        }
    }

    // Battery health — the energy-source block's own `capacity:` field is
    // energy-full ÷ energy-full-design, i.e. wear, not charge level.
    for line in body.lines() {
        let Some(rest) = line.trim().strip_prefix("capacity:") else { continue };
        let Ok(pct) = rest.trim().trim_end_matches('%').parse::<f64>() else { continue };
        if pct < 60.0 {
            out.push(Issue {
                kind: "battery_degraded",
                detail: format!("battery health at {pct:.0}% of design capacity — meaningfully degraded. https://wiki.archlinux.org/title/Laptop#Battery"),
                fixable: false,
                before: no_fix.clone(),
                after: no_fix.clone(),
            });
        } else if pct < 80.0 {
            out.push(Issue {
                kind: "battery_aging",
                detail: format!("battery health at {pct:.0}% of design capacity — normal aging, worth watching. https://wiki.archlinux.org/title/Laptop#Battery"),
                fixable: false,
                before: no_fix.clone(),
                after: no_fix.clone(),
            });
        }
    }

    // Cooling — cyberdeck reports this exact literal when no fan-control
    // daemon is running.
    if body.contains("Fancontrol Daemon") && body.contains("Not Detected") {
        out.push(Issue {
            kind: "no_fan_control",
            detail: "no fan-control daemon detected — fans run on firmware defaults. https://wiki.archlinux.org/title/Fan_speed_control".into(),
            fixable: false,
            before: no_fix,
            after: String::new(),
        });
    }

    out
}

fn regex_lite_wikilinks(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let b = body.as_bytes();
    let mut i = 0;
    while i + 3 < b.len() {
        if &b[i..i + 2] == b"[[" {
            if let Some(end) = body[i + 2..].find("]]") {
                let inner = &body[i + 2..i + 2 + end];
                let name = inner.split('|').next().unwrap_or(inner).trim();
                if !name.is_empty() && !name.starts_with('#') {
                    out.push(name.to_string());
                }
                i += end + 4;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Apply the mechanical fixes for one note. Returns the new raw text, or None if
/// nothing changed.
/// Mechanical cleanup on raw text: CRLF→LF, strip trailing whitespace, collapse
/// 3+ blank lines, fill an empty/pathy frontmatter title from `stem`, ensure a
/// single trailing newline. Pure — no filesystem.
pub fn tidy_str(raw: &str, stem: &str) -> String {
    let mut t = raw.replace("\r\n", "\n").replace('\r', "\n");
    t = t.split('\n').map(|l| l.trim_end()).collect::<Vec<_>>().join("\n");
    while t.contains("\n\n\n") {
        t = t.replace("\n\n\n", "\n\n");
    }
    let human = stem.replace(['-', '_'], " ");
    if let Some(rest) = t.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let fm = &rest[..end];
            let body = &rest[end..];
            let new_fm: String = fm
                .lines()
                .map(|l| {
                    if let Some(v) = l.trim().strip_prefix("title:") {
                        let cur = v.trim().trim_matches('"');
                        if cur.is_empty() || is_pathy(cur) {
                            return format!("title: \"{human}\"");
                        }
                    }
                    l.to_string()
                })
                .collect::<Vec<_>>()
                .join("\n");
            t = format!("---\n{new_fm}{body}");
        }
    }
    if !t.ends_with('\n') {
        t.push('\n');
    }
    t
}

pub fn apply_mechanical(root: &Path, rel: &str) -> Option<String> {
    let raw = vault::read_raw(root, rel).ok()?;
    let stem = Path::new(rel).file_stem().and_then(|s| s.to_str()).unwrap_or("note");
    let t = tidy_str(&raw, stem);
    (t != raw).then_some(t)
}
