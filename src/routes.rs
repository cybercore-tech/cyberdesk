//! HTTP handlers.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use minijinja::{context, Value};
use serde::Deserialize;

use crate::{git, lint, render, vault, AppState};

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
            use std::collections::HashMap;
            let idx: HashMap<String, String> = vault::all_notes(root)
                .into_iter()
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
            page(
                &st,
                "view.html",
                context! {
                    rel => n.rel, title => n.title, tags => n.tags,
                    body_html => body_html,
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
    title: String,
}
fn inbox() -> String {
    "Inbox".into()
}

pub async fn create(State(st): State<AppState>, Form(f): Form<NewForm>) -> Response {
    let root = &st.cfg.root;
    let template = if f.template.trim().is_empty() { "note" } else { f.template.trim() };
    match vault::create_from_template(root, template, &f.folder, &f.title) {
        Ok(rel) => {
            git::commit(root, &format!("note: new — {rel}"));
            Redirect::to(&format!("/e/{rel}")).into_response()
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
pub async fn lint_page(State(st): State<AppState>) -> Response {
    let reports = lint::scan(&st.cfg.root);
    let total_issues: usize = reports.iter().map(|r| r.issues.len()).sum();
    let fixable: usize = reports
        .iter()
        .flat_map(|r| &r.issues)
        .filter(|i| i.fixable)
        .count();
    page(
        &st,
        "lint.html",
        context! { reports, n => reports.len(), total_issues, fixable },
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

// ── api ───────────────────────────────────────────────────────────────────
pub async fn api_titles(State(st): State<AppState>) -> impl IntoResponse {
    axum::Json(
        vault::titles(&st.cfg.root)
            .into_iter()
            .map(|(rel, title)| serde_json::json!({ "rel": rel, "title": title }))
            .collect::<Vec<_>>(),
    )
}

pub async fn healthz() -> &'static str {
    "ok"
}
