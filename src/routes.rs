//! HTTP handlers.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use minijinja::{context, Value};
use serde::Deserialize;

use crate::{blueprints, git, lint, render, theme, vault, AppState};

/// GitHub page for the app's own source.
const REPO_URL: &str = "https://github.com/cybercore-tech/cyberdesk";

/// Pull one cookie value out of the `Cookie:` header.
fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            (k.trim() == name).then(|| v.trim().to_string())
        })
}

fn err(msg: impl std::fmt::Display) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Html(format!(
            "<body style='font:15px system-ui;background:#0e0e14;color:#ff6a8a;padding:3rem'>\
             <h2>nope</h2><pre>{}</pre><p><a style='color:#7fd7ff' href='/'>← portal</a></p>",
            html_escape(&msg.to_string())
        )),
    )
        .into_response()
}
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Common context every page needs (sidebar tree + the `＋ new` form data +
/// the search datalist).
fn shell(st: &AppState) -> Value {
    let root = &st.cfg.root;
    let (vault_repo, repo_choices) = repo_info(root);
    context! {
        tree => vault::tree_nested(root),
        templates => vault::templates(root),
        folders => vault::folders(root),
        titles => vault::titles(root).into_iter()
            .map(|(rel, title)| context! { rel => rel, title => title })
            .collect::<Vec<_>>(),
        tags_all => vault::tag_counts(root).into_iter().map(|(t, _)| t).collect::<Vec<_>>(),
        themes => theme::names(),
        theme_default => theme::active_name(),
        repo_url => REPO_URL,
        vault_repo => vault_repo,
        repo_choices => repo_choices,
    }
}

/// The vault's own GitHub URL (from its git remote) + a short pick-list for the
/// `＋ new` dialog's `repo:` field.
fn repo_info(root: &std::path::Path) -> (Option<String>, Vec<String>) {
    let detected = git::head_info(root).1;
    let mut choices = vec![
        "https://github.com/darkstardevx/darknotes".to_string(),
        "https://github.com/cybercore-tech/cyberdesk".to_string(),
    ];
    if let Some(w) = &detected {
        if !choices.contains(w) {
            choices.insert(0, w.clone());
        }
    }
    (detected, choices)
}

fn page(st: &AppState, name: &str, ctx: Value, nav: &str) -> Response {
    let merged = context! { ..ctx, ..shell(st) };
    match st.render.page(name, merged, nav) {
        Ok(h) => h.into_response(),
        Err(e) => err(e),
    }
}

// ── portal ────────────────────────────────────────────────────────────────
pub async fn portal(State(st): State<AppState>) -> Response {
    let root = &st.cfg.root;
    let notes = vault::all_notes(root);
    let total = notes.len();
    let needs_review = notes
        .iter()
        .filter(|n| n.tags.iter().any(|t| t == "needs-review"))
        .count();
    let recent: Vec<_> = vault::recent(root, 12)
        .into_iter()
        .map(|(rel, title)| context! { rel => rel, title => title })
        .collect();
    let templates = vault::templates(root);
    let folders = vault::folders(root);
    let tags = vault::tag_counts(root)
        .into_iter()
        .map(|(t, c)| context! { tag => t, count => c })
        .collect::<Vec<_>>();
    let issue_notes = lint::scan(root).len();
    page(
        &st,
        "portal.html",
        context! {
            total, needs_review, recent,
            n_templates => templates.len(),
            n_folders => folders.len(),
            tags, issue_notes,
        },
        "portal",
    )
}

// ── view ──────────────────────────────────────────────────────────────────
pub async fn view(State(st): State<AppState>, Path(rel): Path<String>) -> Response {
    let root = &st.cfg.root;
    match vault::read(root, &rel) {
        Ok(n) => {
            let all = vault::all_notes(root);
            let idx: HashMap<String, String> = all
                .iter()
                .filter_map(|x| {
                    std::path::Path::new(&x.rel)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| (s.to_string(), x.rel.clone()))
                })
                .collect();
            let body_html = render::wikilinks(&render::html(&n.body), &|stem: &str| {
                idx.get(stem).cloned().or_else(|| {
                    idx.iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case(stem))
                        .map(|(_, v)| v.clone())
                })
            });

            // backlinks: other notes whose body wikilinks to this one
            let my_stem = std::path::Path::new(&n.rel)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_lowercase();
            let backlinks: Vec<_> = all
                .iter()
                .filter(|x| x.rel != n.rel)
                .filter(|x| {
                    render::wikilink_targets(&x.body)
                        .iter()
                        .any(|t| t.to_lowercase() == my_stem)
                })
                .map(|x| context! { rel => x.rel.clone(), title => x.title.clone() })
                .collect();

            page(
                &st,
                "view.html",
                context! {
                    rel => n.rel, title => n.title, tags => n.tags,
                    body_html => body_html,
                    backlinks,
                    toc => render::toc(&n.body).into_iter()
                        .map(|(l, t, a)| context! { level => l, text => t, anchor => a })
                        .collect::<Vec<_>>(),
                },
                "",
            )
        }
        Err(e) => err(e),
    }
}

// ── editor ────────────────────────────────────────────────────────────────
pub async fn edit(State(st): State<AppState>, Path(rel): Path<String>) -> Response {
    let root = &st.cfg.root;
    let raw = vault::read_raw(root, &rel).unwrap_or_default();
    match vault::safe_rel(&rel) {
        Ok(_) => {
            let title = vault::read(root, &rel)
                .map(|n| n.title)
                .unwrap_or_else(|_| rel.clone());
            page(
                &st,
                "edit.html",
                context! {
                    rel => rel, title => title, content => raw,
                    is_new => !vault::exists(root, &rel),
                },
                "",
            )
        }
        Err(e) => err(e),
    }
}

#[derive(Deserialize)]
pub struct SaveForm {
    content: String,
}

pub async fn save(
    State(st): State<AppState>,
    Path(rel): Path<String>,
    Form(f): Form<SaveForm>,
) -> Response {
    let root = &st.cfg.root;
    match vault::write_raw(root, &rel, &f.content) {
        Ok(()) => {
            git::commit(root, &format!("note: {rel}"));
            if st.cfg.auto_push {
                let r = root.clone();
                tokio::task::spawn_blocking(move || git::push(&r));
            }
            Redirect::to(&format!("/n/{rel}")).into_response()
        }
        Err(e) => err(e),
    }
}

// ── new from template ─────────────────────────────────────────────────────
#[derive(Deserialize)]
pub struct NewForm {
    #[serde(default)]
    template: String,
    #[serde(default = "inbox")]
    folder: String,
    /// used when `folder == "__new__"`
    #[serde(default)]
    new_folder: String,
    title: String,
    /// explicit file stem — blank => slug of the title
    #[serde(default)]
    filename: String,
    /// comma / whitespace separated
    #[serde(default)]
    tags: String,
    /// checkbox: "on" adds the `needs-review` tag
    #[serde(default)]
    needs_review: String,
    /// checkbox: "on" runs the mechanical linter on the new file
    #[serde(default)]
    tidy: String,
    /// checkbox: "on" prefixes the file name with today's date
    #[serde(default)]
    date_prefix: String,
    /// "popup" opens the new note in the pop-up editor, else the full page
    #[serde(default)]
    open_in: String,
    /// JSON object of extra `{{key}}` → value template substitutions
    #[serde(default)]
    fields_json: String,
    /// appended after the template body (e.g. pasted text)
    #[serde(default)]
    body: String,
}
fn inbox() -> String {
    "Inbox".into()
}
fn on(s: &str) -> bool {
    matches!(s.trim(), "on" | "true" | "1" | "yes")
}

pub async fn create(State(st): State<AppState>, Form(f): Form<NewForm>) -> Response {
    let root = &st.cfg.root;
    let template = if f.template.trim().is_empty() {
        "note"
    } else {
        f.template.trim()
    };
    let folder = if f.folder == "__new__" {
        f.new_folder.trim()
    } else {
        f.folder.trim()
    };

    let mut tags: Vec<String> = f
        .tags
        .split([',', ' ', '\t', '\n'])
        .map(|t| t.trim().trim_start_matches('#').to_string())
        .filter(|t| !t.is_empty())
        .collect();
    if on(&f.needs_review) && !tags.iter().any(|t| t == "needs-review") {
        tags.push("needs-review".into());
    }
    tags.dedup();

    let fields: HashMap<String, String> = if f.fields_json.trim().is_empty() {
        HashMap::new()
    } else {
        serde_json::from_str(&f.fields_json).unwrap_or_default()
    };

    let stem = if !f.filename.trim().is_empty() {
        f.filename.trim().to_string()
    } else {
        vault::slugify(f.title.trim())
    };
    let stem = if on(&f.date_prefix) && !stem.is_empty() {
        format!("{}-{}", chrono::Local::now().format("%Y-%m-%d"), stem)
    } else {
        stem
    };
    let filename = (!stem.is_empty()).then_some(stem);

    let opts = vault::NewNote {
        template,
        folder,
        title: f.title.trim(),
        filename: filename.as_deref(),
        tags: &tags,
        fields: &fields,
        body: &f.body,
    };
    match vault::create_note(root, &opts) {
        Ok(rel) => {
            if on(&f.tidy) {
                if let Some(clean) = lint::apply_mechanical(root, &rel) {
                    let _ = vault::write_raw(root, &rel, &clean);
                }
            }
            git::commit(root, &format!("note: new — {rel}"));
            if st.cfg.auto_push {
                let r = root.clone();
                tokio::task::spawn_blocking(move || git::push(&r));
            }
            let dest = if f.open_in.trim() == "popup" {
                format!("/#fed:{rel}")
            } else {
                format!("/e/{rel}")
            };
            Redirect::to(&dest).into_response()
        }
        Err(e) => err(e),
    }
}

// ── delete ────────────────────────────────────────────────────────────────
pub async fn remove(State(st): State<AppState>, Path(rel): Path<String>) -> Response {
    let root = &st.cfg.root;
    match vault::delete(root, &rel) {
        Ok(()) => {
            git::commit(root, &format!("note: remove — {rel}"));
            Redirect::to("/").into_response()
        }
        Err(e) => err(e),
    }
}

// ── search ────────────────────────────────────────────────────────────────
#[derive(Deserialize)]
pub struct SearchQ {
    #[serde(default)]
    q: String,
}

pub async fn search(State(st): State<AppState>, Query(sq): Query<SearchQ>) -> Response {
    let hits = vault::search(&st.cfg.root, sq.q.trim());
    let n = hits.len();
    page(&st, "search.html", context! { q => sq.q, hits, n }, "")
}

// ── all notes / by folder / by tag / templates ───────────────────────────
pub async fn all(State(st): State<AppState>) -> Response {
    let notes: Vec<_> = vault::all_notes(&st.cfg.root)
        .into_iter()
        .map(|n| context! { rel => n.rel, title => n.title, tags => n.tags })
        .collect();
    let n = notes.len();
    page(
        &st,
        "list.html",
        context! { heading => "all notes", notes, n },
        "",
    )
}

pub async fn by_tag(State(st): State<AppState>, Path(tag): Path<String>) -> Response {
    let notes: Vec<_> = vault::by_tag(&st.cfg.root, &tag)
        .into_iter()
        .map(|n| context! { rel => n.rel, title => n.title, tags => n.tags })
        .collect();
    let n = notes.len();
    page(
        &st,
        "list.html",
        context! { heading => format!("#{tag}"), notes, n },
        "",
    )
}

pub async fn templates_page(State(st): State<AppState>) -> Response {
    let root = &st.cfg.root;
    let list: Vec<_> = vault::templates(root)
        .into_iter()
        .map(|t| {
            let rel = format!("_templates/{t}.md");
            let body = std::fs::read_to_string(root.join(&rel)).unwrap_or_default();
            let lines = body.lines().count();
            context! { name => t, rel => rel, preview => body, lines => lines }
        })
        .collect();
    page(&st, "templates.html", context! { list }, "templates")
}

#[derive(Deserialize)]
pub struct NewTemplate {
    name: String,
    #[serde(default)]
    content: String,
}

/// Create `_templates/<slug>.md` from the templates page. Commits, redirects.
pub async fn template_new(State(st): State<AppState>, Form(f): Form<NewTemplate>) -> Response {
    let root = &st.cfg.root;
    let slug = vault::slugify(f.name.trim().trim_end_matches(".md"));
    if slug.is_empty() {
        return err("template needs a name");
    }
    let rel = format!("_templates/{slug}.md");
    if vault::exists(root, &rel) {
        return err(format!("a template named {slug} already exists"));
    }
    let body = if f.content.trim().is_empty() {
        "---\ntitle: \"{{title}}\"\nuser: darkstardevx@gmail.com\nrepo: {{repo}}\nlicense: MIT\ntags: []\ncreated: {{date}}\n---\n\n# {{title}}\n\n".to_string()
    } else {
        f.content.clone()
    };
    match vault::write_raw(root, &rel, &body) {
        Ok(()) => {
            git::commit(root, &format!("template: new — {slug}"));
            Redirect::to("/templates").into_response()
        }
        Err(e) => err(e),
    }
}

// ── API reference page ───────────────────────────────────────────────────
pub async fn api_docs(State(st): State<AppState>) -> Response {
    page(&st, "api.html", context! {}, "api")
}

pub async fn folders_page(State(st): State<AppState>) -> Response {
    let root = &st.cfg.root;
    let n_folders = vault::folders(root).len();
    let n_notes = vault::all_notes(root).len();
    // `tree` is already supplied by shell(); the page renders its dir nodes
    page(
        &st,
        "folders.html",
        context! { n_folders, n_notes },
        "folders",
    )
}

// ── lint ──────────────────────────────────────────────────────────────────
#[derive(Deserialize)]
pub struct LintQ {
    /// filter to a single issue kind (empty = show everything)
    #[serde(default)]
    kind: String,
}

pub async fn lint_page(State(st): State<AppState>, Query(q): Query<LintQ>) -> Response {
    let all = lint::scan(&st.cfg.root);
    let sel = q.kind.trim().to_string();

    // kind → count across the whole vault (stable regardless of the filter)
    let mut counts: std::collections::BTreeMap<&'static str, usize> = Default::default();
    for r in &all {
        for i in &r.issues {
            *counts.entry(i.kind).or_default() += 1;
        }
    }
    let kinds: Vec<_> = counts
        .iter()
        .map(|(k, c)| context! { kind => k, count => c, on => (*k == sel.as_str()) })
        .collect();

    // reports, filtered to the selected kind
    let reports: Vec<lint::NoteReport> = all
        .iter()
        .filter_map(|r| {
            let issues: Vec<lint::Issue> = r
                .issues
                .iter()
                .filter(|i| sel.is_empty() || i.kind == sel)
                .cloned()
                .collect();
            (!issues.is_empty()).then(|| lint::NoteReport {
                rel: r.rel.clone(),
                title: r.title.clone(),
                issues,
            })
        })
        .collect();

    let total_issues: usize = reports.iter().map(|r| r.issues.len()).sum();
    let fixable: usize = reports
        .iter()
        .flat_map(|r| &r.issues)
        .filter(|i| i.fixable)
        .count();
    page(
        &st,
        "lint.html",
        context! { reports, n => reports.len(), total_issues, fixable, kinds, sel },
        "lint",
    )
}

#[derive(Deserialize)]
pub struct LintFix {
    /// a specific rel path, or "*" for every fixable note
    rel: String,
}

pub async fn lint_fix(State(st): State<AppState>, Form(f): Form<LintFix>) -> Response {
    let root = &st.cfg.root;
    let targets: Vec<String> = if f.rel == "*" {
        lint::scan(root)
            .into_iter()
            .filter(|r| r.issues.iter().any(|i| i.fixable))
            .map(|r| r.rel)
            .collect()
    } else {
        vec![f.rel.clone()]
    };
    let mut fixed = 0;
    for rel in &targets {
        if let Some(new) = lint::apply_mechanical(root, rel) {
            if vault::write_raw(root, rel, &new).is_ok() {
                fixed += 1;
            }
        }
    }
    if fixed > 0 {
        git::commit(root, &format!("lint: mechanical fixes ({fixed} note(s))"));
    }
    Redirect::to("/lint").into_response()
}

/// JSON form of the same scan `/lint` renders — for anything that wants to
/// show "possible problems and fixes" without parsing HTML (e.g. Mission
/// Control's Diagnostics tab reading the cyberdeck-diagnostics-deck
/// instance of this same binary).
pub async fn api_lint(State(st): State<AppState>) -> impl IntoResponse {
    let reports = lint::scan(&st.cfg.root);
    let total_issues: usize = reports.iter().map(|r| r.issues.len()).sum();
    let fixable: usize = reports
        .iter()
        .flat_map(|r| &r.issues)
        .filter(|i| i.fixable)
        .count();
    axum::Json(serde_json::json!({
        "reports": reports,
        "total_issues": total_issues,
        "fixable": fixable,
    }))
}

// ── theme (cybercore switcher) ────────────────────────────────────────────
/// `:root{}` custom properties for the browser's chosen theme. Linked from
/// every page, so switching is just a cookie + reload — no server restart.
pub async fn theme_css(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let css = cookie(&headers, "cyberdesk_theme")
        .and_then(|slug| theme::css_for_slug(&slug))
        .unwrap_or_else(|| st.render.css.clone());
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        css,
    )
        .into_response()
}

/// Set (or, for an unknown slug, clear) the theme cookie, then bounce back.
pub async fn theme_set(Path(slug): Path<String>, headers: HeaderMap) -> Response {
    let known = theme::select(&slug);
    let set_cookie = if known {
        format!("cyberdesk_theme={slug}; Path=/; Max-Age=31536000; SameSite=Lax")
    } else {
        "cyberdesk_theme=; Path=/; Max-Age=0; SameSite=Lax".to_string()
    };
    let back = headers
        .get(header::REFERER)
        .and_then(|v| v.to_str().ok())
        .filter(|r| r.contains("://"))
        .unwrap_or("/")
        .to_string();
    let mut res = Redirect::to(&back).into_response();
    if let Ok(v) = HeaderValue::from_str(&set_cookie) {
        res.headers_mut().insert(header::SET_COOKIE, v);
    }
    res
}

/// Raw `:root{}` CSS for one theme slug, for the JS theme-picker dropdown
/// to fetch on selection (no page reload) — mirrors `theme_set`'s cookie
/// write (so a plain reload without JS still lands on the same theme via
/// `/theme.css`) but returns CSS instead of redirecting.
pub async fn api_theme_css(Path(slug): Path<String>) -> Response {
    let Some(css) = theme::css_for_slug(&slug) else {
        return (StatusCode::NOT_FOUND, format!("unknown theme: {slug}")).into_response();
    };
    theme::select(&slug);
    let set_cookie = format!("cyberdesk_theme={slug}; Path=/; Max-Age=31536000; SameSite=Lax");
    let mut res = (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        css,
    )
        .into_response();
    if let Ok(v) = HeaderValue::from_str(&set_cookie) {
        res.headers_mut().insert(header::SET_COOKIE, v);
    }
    res
}

// ── repo activity ─────────────────────────────────────────────────────────
pub async fn repo_page(State(st): State<AppState>) -> Response {
    let app_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let (app_branch, app_web) = git::head_info(app_dir);
    let (vault_branch, vault_web) = git::head_info(&st.cfg.root);
    page(
        &st,
        "repo.html",
        context! {
            app_commits => git::log(app_dir, 25),
            app_branch, app_web => app_web.clone().unwrap_or_else(|| REPO_URL.into()),
            app_path => app_dir.display().to_string(),
            vault_commits => git::log(&st.cfg.root, 25),
            vault_branch, vault_web,
            vault_path => st.cfg.root.display().to_string(),
        },
        "repo",
    )
}

// ── api ───────────────────────────────────────────────────────────────────
pub async fn api_titles(State(st): State<AppState>) -> impl IntoResponse {
    axum::Json(
        vault::titles(&st.cfg.root)
            .into_iter()
            .map(|(rel, title)| serde_json::json!({ "rel": rel, "title": title }))
            .collect::<Vec<_>>(),
    )
}

/// Raw file contents for the pop-up editor.
pub async fn api_raw(State(st): State<AppState>, Path(rel): Path<String>) -> Response {
    match vault::safe_rel(&rel) {
        Ok(r) => {
            let exists = vault::exists(&st.cfg.root, &r);
            let content = vault::read_raw(&st.cfg.root, &r).unwrap_or_default();
            axum::Json(serde_json::json!({ "rel": r, "content": content, "exists": exists }))
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

fn stem_of(rel: &str) -> &str {
    std::path::Path::new(rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("note")
}

#[derive(Deserialize)]
pub struct ApiSave {
    rel: String,
    content: String,
    /// run the mechanical linter on `content` before writing
    #[serde(default)]
    tidy: bool,
}

/// Save (or save-as) from the pop-up editor. Creates the file + parent dirs if
/// missing, commits, and optionally pushes. Returns JSON incl. the final
/// `content` (which may differ from the input when `tidy` is set).
pub async fn api_save(State(st): State<AppState>, axum::Json(f): axum::Json<ApiSave>) -> Response {
    let root = &st.cfg.root;
    let mut rel = f.rel.trim().trim_start_matches('/').to_string();
    if !rel.ends_with(".md") {
        rel.push_str(".md");
    }
    let existed = vault::exists(root, &rel);
    let content = if f.tidy {
        lint::tidy_str(&f.content, stem_of(&rel))
    } else {
        f.content.clone()
    };
    match vault::write_raw(root, &rel, &content) {
        Ok(()) => {
            let verb = if existed { "note" } else { "note: new —" };
            git::commit(root, &format!("{verb} {rel}"));
            if st.cfg.auto_push {
                let r = root.clone();
                tokio::task::spawn_blocking(move || git::push(&r));
            }
            axum::Json(serde_json::json!({
                "ok": true, "rel": rel, "created": !existed, "content": content
            }))
            .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct ApiTidy {
    #[serde(default)]
    rel: String,
    content: String,
}

/// Mechanically lint `content` without touching the filesystem — the editor's
/// "tidy" button. Returns `{ content, changed }`.
pub async fn api_tidy(axum::Json(f): axum::Json<ApiTidy>) -> Response {
    let stem = if f.rel.trim().is_empty() {
        "note"
    } else {
        stem_of(f.rel.trim())
    };
    let out = lint::tidy_str(&f.content, stem);
    let changed = out != f.content;
    axum::Json(serde_json::json!({ "content": out, "changed": changed })).into_response()
}

#[derive(Deserialize)]
pub struct ApiDelete {
    rel: String,
}

/// Delete a file from the pop-up editor (with a git commit). JSON in/out.
pub async fn api_delete(
    State(st): State<AppState>,
    axum::Json(f): axum::Json<ApiDelete>,
) -> Response {
    let root = &st.cfg.root;
    let rel = f.rel.trim();
    match vault::delete(root, rel) {
        Ok(()) => {
            git::commit(root, &format!("note: remove — {rel}"));
            if st.cfg.auto_push {
                let r = root.clone();
                tokio::task::spawn_blocking(move || git::push(&r));
            }
            axum::Json(serde_json::json!({ "ok": true, "rel": rel })).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct ApiMove {
    from: String,
    to: String,
}

/// Rename / move a note. JSON in/out.
pub async fn api_move(State(st): State<AppState>, axum::Json(f): axum::Json<ApiMove>) -> Response {
    let root = &st.cfg.root;
    match vault::rename(root, f.from.trim(), f.to.trim()) {
        Ok(to) => {
            git::commit(root, &format!("note: move — {} → {}", f.from.trim(), to));
            if st.cfg.auto_push {
                let r = root.clone();
                tokio::task::spawn_blocking(move || git::push(&r));
            }
            axum::Json(serde_json::json!({ "ok": true, "from": f.from.trim(), "to": to }))
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ── vendored CodeMirror (offline; no CDN) ─────────────────────────────────
pub async fn cm_js() -> impl IntoResponse {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        include_str!("../assets/cm.bundle.js"),
    )
}
pub async fn cm_css() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        include_str!("../assets/cm.bundle.css"),
    )
}

/// Shared cybercore design tokens (fonts / sizing / spacing / motion).
pub async fn tokens_css() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        cybercore::tokens::CSS,
    )
}

/// Shared cybercore component styles (cards / badges / tables / theme
/// picker / popup viewers) — the same visual language cyberdeck uses.
pub async fn components_css() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            // Unlike tokens.css (rarely touched), this file is under active
            // iteration — a long max-age here caused real, repeated
            // "the fix isn't showing up" confusion across multiple browser
            // tabs/origins after each redeploy. `no-cache` (not `no-store`)
            // still permits caching but forces revalidation with the server
            // first — with no ETag/Last-Modified support to validate
            // against, that means every load gets the current build, no
            // hard-refresh required. Revisit once this stops changing every
            // few minutes.
            (header::CACHE_CONTROL, "no-cache"),
        ],
        cybercore::components::CSS,
    )
}

/// Self-hosted webfont `@font-face` rules.
pub async fn fonts_css() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=604800"),
        ],
        include_str!("../assets/fonts.css"),
    )
}

fn woff2(bytes: &'static [u8]) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "font/woff2"),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        bytes,
    )
}
pub async fn font_mono() -> impl IntoResponse {
    woff2(include_bytes!("../assets/fonts/JetBrainsMono.woff2"))
}
pub async fn font_ui() -> impl IntoResponse {
    woff2(include_bytes!("../assets/fonts/Inter.woff2"))
}

fn svg(body: String) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "image/svg+xml; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        body,
    )
}
pub async fn logo_svg() -> impl IntoResponse {
    svg(include_str!("../assets/logo/cyberdesk-logo.svg").to_string())
}
pub async fn mark_svg() -> impl IntoResponse {
    svg(include_str!("../assets/logo/cyberdesk-mark.svg").to_string())
}
pub async fn favicon_svg(State(st): State<AppState>) -> impl IntoResponse {
    if let Some(path) = &st.cfg.favicon_path {
        if let Ok(custom) = std::fs::read_to_string(path) {
            return svg(custom);
        }
    }
    svg(include_str!("../assets/logo/favicon.svg").to_string())
}

pub async fn healthz() -> &'static str {
    "ok"
}

// ── Blueprints ───────────────────────────────────────────────────────

fn blueprints_page(st: &AppState, name: &str, ctx: Value) -> Response {
    match st.render.page(name, ctx, "blueprints") {
        Ok(html) => html.into_response(),
        Err(e) => err(e),
    }
}

pub async fn blueprints_list(State(st): State<AppState>) -> Response {
    let templates = match blueprints::list_templates(&st.cfg.blueprints_dir) {
        Ok(t) => t,
        Err(e) => return err(e),
    };
    let projects = match blueprints::list_tracked_projects(&st.cfg.blueprints_dir) {
        Ok(p) => p,
        Err(e) => return err(e),
    };
    blueprints_page(&st, "blueprint_list.html", context! { templates, projects })
}

pub async fn blueprint_template_new_form(State(st): State<AppState>) -> Response {
    let seeded = blueprints::Template::seed_defaults();
    blueprints_page(
        &st,
        "blueprint_template_edit.html",
        context! { editing => false, template => seeded },
    )
}

#[derive(Deserialize, Default)]
pub struct TemplateForm {
    #[serde(default)]
    slug: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    agents_md: String,
    #[serde(default)]
    error_policy: String,
    #[serde(default)]
    dependency_policy: String,
    #[serde(default)]
    testing_strategy: String,
    #[serde(default)]
    full_validation_gate: String,
    #[serde(default)]
    feature_isolation_gate: String,
    #[serde(default)]
    package_gate: String,
    #[serde(default)]
    ci_info: String,
    #[serde(default)]
    git_commit_standard: String,
    #[serde(default)]
    changelog_policy: String,
    #[serde(default)]
    release_checklist: String,
    #[serde(default)]
    definition_of_done: String,
}

impl TemplateForm {
    fn into_template(self) -> blueprints::Template {
        blueprints::Template {
            slug: blueprints::slugify(&self.slug),
            name: self.name,
            agents_md: self.agents_md,
            error_policy: self.error_policy,
            dependency_policy: self.dependency_policy,
            testing_strategy: self.testing_strategy,
            full_validation_gate: self.full_validation_gate,
            feature_isolation_gate: self.feature_isolation_gate,
            package_gate: self.package_gate,
            ci_info: self.ci_info,
            git_commit_standard: self.git_commit_standard,
            changelog_policy: self.changelog_policy,
            release_checklist: self.release_checklist,
            definition_of_done: self.definition_of_done,
        }
    }
}

pub async fn blueprint_template_new(
    State(st): State<AppState>,
    Form(f): Form<TemplateForm>,
) -> Response {
    let template = f.into_template();
    if template.slug.is_empty() {
        return err("template needs a slug");
    }
    match blueprints::save_template(&st.cfg.blueprints_dir, &template) {
        Ok(()) => Redirect::to("/blueprints").into_response(),
        Err(e) => err(e),
    }
}

pub async fn blueprint_template_edit_form(
    State(st): State<AppState>,
    Path(slug): Path<String>,
) -> Response {
    match blueprints::load_template(&st.cfg.blueprints_dir, &slug) {
        Ok(Some(t)) => blueprints_page(
            &st,
            "blueprint_template_edit.html",
            context! { editing => true, template => t },
        ),
        Ok(None) => err(format!("no such template: {slug}")),
        Err(e) => err(e),
    }
}

pub async fn blueprint_template_edit(
    State(st): State<AppState>,
    Path(_slug): Path<String>,
    Form(f): Form<TemplateForm>,
) -> Response {
    let template = f.into_template();
    match blueprints::save_template(&st.cfg.blueprints_dir, &template) {
        Ok(()) => Redirect::to("/blueprints").into_response(),
        Err(e) => err(e),
    }
}

pub async fn blueprint_instance_new_form(State(st): State<AppState>) -> Response {
    let templates = match blueprints::list_templates(&st.cfg.blueprints_dir) {
        Ok(t) => t,
        Err(e) => return err(e),
    };
    if templates.is_empty() {
        return err("no blueprint templates yet — create one first at /blueprints/templates/new");
    }
    blueprints_page(&st, "blueprint_new.html", context! { templates })
}

#[derive(Deserialize)]
pub struct InstanceNewForm {
    template_slug: String,
    project_name: String,
    target_dir: String,
    edition: String,
    msrv: String,
    workspace_members: String,
    vision: String,
    long_term_capabilities: String,
    core_requirements: String,
    non_goals: String,
    architectural_principles: String,
    feature_matrix: String,
    public_api_surfaces: String,
    persistence_notes: String,
    security_invariants: String,
    cli_contract: String,
    phases: String,
    protected_areas: String,
    raw_idea: String,
}

pub async fn blueprint_instance_new(
    State(st): State<AppState>,
    Form(f): Form<InstanceNewForm>,
) -> Response {
    let template = match blueprints::load_template(&st.cfg.blueprints_dir, &f.template_slug) {
        Ok(Some(t)) => t,
        Ok(None) => return err(format!("no such template: {}", f.template_slug)),
        Err(e) => return err(e),
    };
    let slug = blueprints::slugify(&f.project_name);
    if slug.is_empty() {
        return err("project needs a name");
    }
    let target_dir = std::path::PathBuf::from(f.target_dir.trim());
    if !target_dir.is_absolute() {
        return err("target directory must be an absolute path");
    }
    let phases = blueprints::parse_lines(&f.phases);
    let current_phase = phases.first().cloned().unwrap_or_default();
    let instance = blueprints::Instance {
        slug: slug.clone(),
        template_slug: f.template_slug,
        project_name: f.project_name,
        target_dir,
        edition: f.edition,
        msrv: f.msrv,
        workspace_members: blueprints::parse_lines(&f.workspace_members),
        vision: f.vision,
        long_term_capabilities: blueprints::parse_lines(&f.long_term_capabilities),
        core_requirements: blueprints::parse_lines(&f.core_requirements),
        non_goals: blueprints::parse_lines(&f.non_goals),
        architectural_principles: blueprints::parse_lines(&f.architectural_principles),
        feature_matrix: blueprints::parse_feature_matrix(&f.feature_matrix),
        public_api_surfaces: blueprints::parse_lines(&f.public_api_surfaces),
        persistence_notes: blueprints::parse_lines(&f.persistence_notes),
        security_invariants: blueprints::parse_lines(&f.security_invariants),
        cli_contract: f.cli_contract,
        phases,
        protected_areas: blueprints::parse_lines(&f.protected_areas),
        raw_idea: f.raw_idea,
        current_phase,
        current_milestone: String::new(),
        feature_state: Vec::new(),
        validation_status: Vec::new(),
        current_failure: String::new(),
        next_intended_work: String::new(),
        notes_for_next_agent: String::new(),
        handoff_log: Vec::new(),
    };
    if let Err(e) = blueprints::save_instance(&st.cfg.blueprints_dir, &instance) {
        return err(e);
    }
    if let Err(e) = blueprints::export(&template, &instance) {
        return err(format!("saved, but export failed: {e}"));
    }
    Redirect::to(&format!("/blueprints/{slug}")).into_response()
}

pub async fn blueprint_dashboard(State(st): State<AppState>, Path(slug): Path<String>) -> Response {
    let instance = match blueprints::load_instance_by_slug(&st.cfg.blueprints_dir, &slug) {
        Ok(Some(i)) => i,
        Ok(None) => return err(format!("no such blueprint instance: {slug}")),
        Err(e) => return err(e),
    };
    let workspace_members_text = blueprints::format_lines(&instance.workspace_members);
    let long_term_capabilities_text = blueprints::format_lines(&instance.long_term_capabilities);
    let core_requirements_text = blueprints::format_lines(&instance.core_requirements);
    let non_goals_text = blueprints::format_lines(&instance.non_goals);
    let architectural_principles_text =
        blueprints::format_lines(&instance.architectural_principles);
    let public_api_surfaces_text = blueprints::format_lines(&instance.public_api_surfaces);
    let persistence_notes_text = blueprints::format_lines(&instance.persistence_notes);
    let security_invariants_text = blueprints::format_lines(&instance.security_invariants);
    let protected_areas_text = blueprints::format_lines(&instance.protected_areas);
    let phases_text = blueprints::format_lines(&instance.phases);
    let feature_matrix_text = blueprints::format_feature_matrix(&instance.feature_matrix);
    let feature_state_text = blueprints::format_feature_state(&instance.feature_state);
    let validation_status_text = blueprints::format_validation_status(&instance.validation_status);
    blueprints_page(
        &st,
        "blueprint_dashboard.html",
        context! {
            instance, workspace_members_text, long_term_capabilities_text,
            core_requirements_text, non_goals_text, architectural_principles_text,
            public_api_surfaces_text, persistence_notes_text, security_invariants_text,
            protected_areas_text, phases_text, feature_matrix_text, feature_state_text,
            validation_status_text,
        },
    )
}

#[derive(Deserialize)]
pub struct InstanceSaveForm {
    project_name: String,
    edition: String,
    msrv: String,
    workspace_members: String,
    vision: String,
    long_term_capabilities: String,
    core_requirements: String,
    non_goals: String,
    architectural_principles: String,
    feature_matrix: String,
    public_api_surfaces: String,
    persistence_notes: String,
    security_invariants: String,
    cli_contract: String,
    phases: String,
    protected_areas: String,
    raw_idea: String,
    current_phase: String,
    current_milestone: String,
    feature_state: String,
    validation_status: String,
    current_failure: String,
    next_intended_work: String,
    notes_for_next_agent: String,
}

pub async fn blueprint_save(
    State(st): State<AppState>,
    Path(slug): Path<String>,
    Form(f): Form<InstanceSaveForm>,
) -> Response {
    let mut instance = match blueprints::load_instance_by_slug(&st.cfg.blueprints_dir, &slug) {
        Ok(Some(i)) => i,
        Ok(None) => return err(format!("no such blueprint instance: {slug}")),
        Err(e) => return err(e),
    };
    let template = match blueprints::load_template(&st.cfg.blueprints_dir, &instance.template_slug)
    {
        Ok(Some(t)) => t,
        Ok(None) => {
            return err(format!(
                "template {} no longer exists",
                instance.template_slug
            ))
        }
        Err(e) => return err(e),
    };
    instance.project_name = f.project_name;
    instance.edition = f.edition;
    instance.msrv = f.msrv;
    instance.workspace_members = blueprints::parse_lines(&f.workspace_members);
    instance.vision = f.vision;
    instance.long_term_capabilities = blueprints::parse_lines(&f.long_term_capabilities);
    instance.core_requirements = blueprints::parse_lines(&f.core_requirements);
    instance.non_goals = blueprints::parse_lines(&f.non_goals);
    instance.architectural_principles = blueprints::parse_lines(&f.architectural_principles);
    instance.feature_matrix = blueprints::parse_feature_matrix(&f.feature_matrix);
    instance.public_api_surfaces = blueprints::parse_lines(&f.public_api_surfaces);
    instance.persistence_notes = blueprints::parse_lines(&f.persistence_notes);
    instance.security_invariants = blueprints::parse_lines(&f.security_invariants);
    instance.cli_contract = f.cli_contract;
    instance.phases = blueprints::parse_lines(&f.phases);
    instance.protected_areas = blueprints::parse_lines(&f.protected_areas);
    instance.raw_idea = f.raw_idea;
    instance.current_phase = f.current_phase;
    instance.current_milestone = f.current_milestone;
    instance.feature_state = blueprints::parse_feature_state(&f.feature_state);
    instance.validation_status = blueprints::parse_validation_status(&f.validation_status);
    instance.current_failure = f.current_failure;
    instance.next_intended_work = f.next_intended_work;
    instance.notes_for_next_agent = f.notes_for_next_agent;

    if let Err(e) = blueprints::save_instance(&st.cfg.blueprints_dir, &instance) {
        return err(e);
    }
    if let Err(e) = blueprints::export(&template, &instance) {
        return err(format!("saved, but export failed: {e}"));
    }
    Redirect::to(&format!("/blueprints/{slug}")).into_response()
}

#[derive(Deserialize)]
pub struct HandoffForm {
    summary: String,
}

pub async fn blueprint_handoff(
    State(st): State<AppState>,
    Path(slug): Path<String>,
    Form(f): Form<HandoffForm>,
) -> Response {
    let instance = match blueprints::append_handoff(&st.cfg.blueprints_dir, &slug, f.summary) {
        Ok(i) => i,
        Err(e) => return err(e),
    };
    let template = match blueprints::load_template(&st.cfg.blueprints_dir, &instance.template_slug)
    {
        Ok(Some(t)) => t,
        Ok(None) => {
            return err(format!(
                "template {} no longer exists",
                instance.template_slug
            ))
        }
        Err(e) => return err(e),
    };
    if let Err(e) = blueprints::export(&template, &instance) {
        return err(format!("saved, but export failed: {e}"));
    }
    Redirect::to(&format!("/blueprints/{slug}")).into_response()
}

/// Live preview of the generated files, rendered as HTML for the popup
/// viewer (same `.viewer-body.markdown-body` pattern the "View Output"
/// modal already uses).
pub async fn api_blueprint_preview(
    State(st): State<AppState>,
    Path(slug): Path<String>,
) -> Response {
    let instance = match blueprints::load_instance_by_slug(&st.cfg.blueprints_dir, &slug) {
        Ok(Some(i)) => i,
        Ok(None) => return err(format!("no such blueprint instance: {slug}")),
        Err(e) => return err(e),
    };
    let template = match blueprints::load_template(&st.cfg.blueprints_dir, &instance.template_slug)
    {
        Ok(Some(t)) => t,
        Ok(None) => {
            return err(format!(
                "template {} no longer exists",
                instance.template_slug
            ))
        }
        Err(e) => return err(e),
    };
    let mut combined = String::new();
    match blueprints::render_agents_md(&template, &instance) {
        Ok(s) => combined.push_str(&s),
        Err(e) => return err(e),
    }
    combined.push_str("\n\n---\n\n");
    match blueprints::render_project_spec(&template, &instance) {
        Ok(s) => combined.push_str(&s),
        Err(e) => return err(e),
    }
    combined.push_str("\n\n---\n\n");
    combined.push_str(&blueprints::render_project_state(&instance));
    Html(render::html(&combined)).into_response()
}
