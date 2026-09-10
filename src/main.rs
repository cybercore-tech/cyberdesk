//! cyberdesk — a local, git-backed markdown notes portal + editor for the
//! darknotes vault. One binary, server-rendered, no npm.

mod config;
mod git;
mod lint;
mod render;
mod routes;
mod theme;
mod vault;
mod web;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::trace::TraceLayer;

use crate::config::Config;
use crate::web::Renderer;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub render: Renderer,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cyberdesk=info,tower_http=warn".into()),
        )
        .init();

    let cfg = Config::from_env();
    if !cfg.root.is_dir() {
        anyhow::bail!(
            "vault not found: {} — set CYBERDESK_ROOT",
            cfg.root.display()
        );
    }
    tracing::info!("vault: {}  theme: {}", cfg.root.display(), theme::active_name());

    let state = AppState { cfg: Arc::new(cfg.clone()), render: Renderer::new() };

    let app = Router::new()
        .route("/", get(routes::portal))
        .route("/healthz", get(routes::healthz))
        .route("/search", get(routes::search))
        .route("/all", get(routes::all))
        .route("/folders", get(routes::folders_page))
        .route("/templates", get(routes::templates_page))
        .route("/tag/*tag", get(routes::by_tag))
        .route("/lint", get(routes::lint_page))
        .route("/lint/fix", post(routes::lint_fix))
        .route("/repo", get(routes::repo_page))
        .route("/theme.css", get(routes::theme_css))
        .route("/theme/set/:slug", get(routes::theme_set))
        .route("/api/titles", get(routes::api_titles))
        .route("/api/raw/*path", get(routes::api_raw))
        .route("/api/save", post(routes::api_save))
        .route("/api/tidy", post(routes::api_tidy))
        .route("/api/delete", post(routes::api_delete))
        .route("/api/move", post(routes::api_move))
        .route("/vendor/cm.js", get(routes::cm_js))
        .route("/vendor/cm.css", get(routes::cm_css))
        .route("/logo.svg", get(routes::logo_svg))
        .route("/mark.svg", get(routes::mark_svg))
        .route("/favicon.svg", get(routes::favicon_svg))
        .route("/new", post(routes::create))
        .route("/n/*path", get(routes::view))
        .route("/e/*path", get(routes::edit).post(routes::save))
        .route("/rm/*path", post(routes::remove))
        .layer(CatchPanicLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr: SocketAddr = cfg.bind.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("cyberdesk on http://{addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
