//! HTTP handlers.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use minijinja::{context, Value};
use serde::Deserialize;

use crate::{git, lint, render, theme, vault, AppState};

/// GitHub page for the app's own source.
const REPO_URL: &str = "https://github.com/darkstardevx/cyberdesk";

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
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Common context every page needs (sidebar tree + the `＋ new` form data +
/// the search datalist).
fn shell(st: &AppState) -> Value {
    let root = &st.cfg.root;
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
    }
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
    let needs_review = notes.iter().filter(|n| n.tags.iter().any(|t| t == "needs-review")).count();
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
                    idx.iter().find(|(k, _)| k.eq_ignore_ascii_case(stem)).map(|(_, v)| v.clone())
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
            let title = vault::read(root, &rel).map(|n| n.title).unwrap_or_else(|_| rel.clone());
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
    let template = if f.template.trim().is_empty() { "note" } else { f.template.trim() };
    let folder = if f.folder == "__new__" { f.new_folder.trim() } else { f.folder.trim() };

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
    page(&st, "list.html", context! { heading => "all notes", notes, n }, "")
}

pub async fn by_tag(State(st): State<AppState>, Path(tag): Path<String>) -> Response {
    let notes: Vec<_> = vault::by_tag(&st.cfg.root, &tag)
        .into_iter()
        .map(|n| context! { rel => n.rel, title => n.title, tags => n.tags })
        .collect();
    let n = notes.len();
    page(&st, "list.html", context! { heading => format!("#{tag}"), notes, n }, "")
}

pub async fn templates_page(State(st): State<AppState>) -> Response {
    let root = &st.cfg.root;
    let list: Vec<_> = vault::templates(root)
        .into_iter()
        .map(|t| {
            let rel = format!("_templates/{t}.md");
            let body = std::fs::read_to_string(root.join(&rel)).unwrap_or_default();
            context! { name => t, rel => rel, preview => body }
        })
        .collect();
    page(&st, "templates.html", context! { list }, "")
}

pub async fn folders_page(State(st): State<AppState>) -> Response {
    let root = &st.cfg.root;
    let list: Vec<_> = vault::folders(root)
        .into_iter()
        .map(|f| {
            let n = vault::all_notes(root).iter().filter(|x| x.rel.starts_with(&format!("{f}/"))).count();
            context! { rel => f, count => n }
        })
        .collect();
    page(&st, "folders.html", context! { list }, "")
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
    let fixable: usize = reports.iter().flat_map(|r| &r.issues).filter(|i| i.fixable).count();
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
    let known = theme::names().contains(&slug.as_str());
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
    let stem = if f.rel.trim().is_empty() { "note" } else { stem_of(f.rel.trim()) };
    let out = lint::tidy_str(&f.content, stem);
    let changed = out != f.content;
    axum::Json(serde_json::json!({ "content": out, "changed": changed })).into_response()
}

#[derive(Deserialize)]
pub struct ApiDelete {
    rel: String,
}

/// Delete a file from the pop-up editor (with a git commit). JSON in/out.
pub async fn api_delete(State(st): State<AppState>, axum::Json(f): axum::Json<ApiDelete>) -> Response {
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
            axum::Json(serde_json::json!({ "ok": true, "from": f.from.trim(), "to": to })).into_response()
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
            (header::CONTENT_TYPE, "application/javascript; charset=utf-8"),
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

pub async fn healthz() -> &'static str {
    "ok"
}
