//! HTTP handlers.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use minijinja::context;
use serde::Deserialize;

use crate::{git, render, vault, AppState};

fn err(msg: impl std::fmt::Display) -> Response {
    (StatusCode::BAD_REQUEST, Html(format!(
        "<body style='font:15px system-ui;background:#0e0e14;color:#ff6a8a;padding:3rem'>\
         <h2>nope</h2><pre>{}</pre><p><a style='color:#7fd7ff' href='/'>← portal</a></p>",
        html_escape(&msg.to_string())
    ))).into_response()
}
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
fn page(st: &AppState, name: &str, ctx: minijinja::Value, nav: &str) -> Response {
    match st.render.page(name, ctx, nav) {
        Ok(h) => h.into_response(),
        Err(e) => err(e),
    }
}

// ── portal ────────────────────────────────────────────────────────────────
pub async fn portal(State(st): State<AppState>) -> Response {
    let root = &st.cfg.root;
    let notes = vault::all_notes(root);
    let total = notes.len();
    let needs_review: Vec<_> = notes
        .iter()
        .filter(|n| n.tags.iter().any(|t| t == "needs-review"))
        .map(|n| context! { rel => n.rel, title => n.title })
        .collect();
    let recent: Vec<_> = vault::recent(root, 12)
        .into_iter()
        .map(|(rel, title)| context! { rel => rel, title => title })
        .collect();
    let templates = vault::templates(root);
    let folders = vault::folders(root);
    page(
        &st,
        "portal.html",
        context! {
            total, recent, needs_review,
            needs_review_n => needs_review.len(),
            templates, folders,
            tree => vault::tree(root),
        },
        "portal",
    )
}

// ── view (rendered) ───────────────────────────────────────────────────────
pub async fn view(State(st): State<AppState>, Path(rel): Path<String>) -> Response {
    let root = &st.cfg.root;
    match vault::read(root, &rel) {
        Ok(n) => page(
            &st,
            "view.html",
            context! {
                rel => n.rel, title => n.title, tags => n.tags,
                body_html => render::html(&n.body),
                toc => render::toc(&n.body).into_iter()
                    .map(|(l, t, a)| context! { level => l, text => t, anchor => a })
                    .collect::<Vec<_>>(),
                tree => vault::tree(root),
            },
            "",
        ),
        Err(e) => err(e),
    }
}

// ── editor ────────────────────────────────────────────────────────────────
pub async fn edit(State(st): State<AppState>, Path(rel): Path<String>) -> Response {
    let root = &st.cfg.root;
    match vault::read_raw(root, &rel).or_else(|_| vault::safe_rel(&rel).map(|_| String::new())) {
        Ok(raw) => {
            let title = vault::read(root, &rel).map(|n| n.title).unwrap_or_else(|_| rel.clone());
            page(
                &st,
                "edit.html",
                context! {
                    rel => rel, title => title, content => raw,
                    tree => vault::tree(root),
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
    template: String,
    folder: String,
    title: String,
}

pub async fn create(State(st): State<AppState>, Form(f): Form<NewForm>) -> Response {
    let root = &st.cfg.root;
    match vault::create_from_template(root, &f.template, &f.folder, &f.title) {
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
    let root = &st.cfg.root;
    let hits = vault::search(root, sq.q.trim());
    page(
        &st,
        "search.html",
        context! {
            q => sq.q, hits => hits, n => hits.len(),
            tree => vault::tree(root),
        },
        "",
    )
}

pub async fn healthz() -> &'static str {
    "ok"
}
