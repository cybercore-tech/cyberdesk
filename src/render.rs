//! Markdown → HTML (GitHub-flavored) + a table of contents.

use comrak::{markdown_to_html, ComrakOptions};

pub fn html(md: &str) -> String {
    let mut o = ComrakOptions::default();
    o.extension.strikethrough = true;
    o.extension.table = true;
    o.extension.tasklist = true;
    o.extension.autolink = true;
    o.extension.footnotes = true;
    o.extension.header_ids = Some(String::new());
    o.extension.description_lists = true;
    o.render.unsafe_ = false; // no raw HTML passthrough
    markdown_to_html(md, &o)
}

/// `(level, text, anchor)` for every ATX heading, matching comrak's `header_ids`.
pub fn toc(md: &str) -> Vec<(usize, String, String)> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for line in md.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let hashes = t.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && t.chars().nth(hashes) == Some(' ') {
            let text = t[hashes + 1..].trim().to_string();
            out.push((hashes, text.clone(), anchor(&text)));
        }
    }
    out
}

fn anchor(text: &str) -> String {
    let mut s = String::new();
    for ch in text.to_lowercase().chars() {
        if ch.is_alphanumeric() {
            s.push(ch);
        } else if ch == ' ' || ch == '-' || ch == '_' {
            s.push('-');
        }
    }
    s.trim_matches('-').to_string()
}
