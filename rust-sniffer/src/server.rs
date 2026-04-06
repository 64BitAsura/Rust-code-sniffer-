//! HTTP serve command — exposes indexed symbols over a REST API and
//! serves an embedded single-page web UI.
//!
//! ## Endpoints
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | `GET` | `/` | Embedded Symbol Explorer web UI |
//! | `GET` | `/api/status` | Index metadata (file count, symbol count, indexed_at) |
//! | `GET` | `/api/symbols` | Full symbol list from the cache (`symbols.json`) |

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::Html, routing::get, Json, Router};
use serde::Serialize;
use tower_http::cors::{Any, CorsLayer};

use crate::incremental::load_cached_symbols;
use crate::meta::IndexMeta;
use crate::symbols::FileSymbols;

/// Embedded web-UI HTML, compiled directly into the binary.
static UI_HTML: &str = include_str!("embedded/ui.html");

// ─── Shared application state ──────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    index_dir: Arc<PathBuf>,
}

// ─── Response types ────────────────────────────────────────────────────────

/// Returned by `GET /api/status`.
#[derive(Serialize)]
struct StatusResponse {
    indexed_at: Option<String>,
    root: Option<String>,
    file_count: Option<usize>,
    symbol_count: Option<usize>,
}

// ─── Handlers ─────────────────────────────────────────────────────────────

async fn serve_ui() -> Html<&'static str> {
    Html(UI_HTML)
}

async fn api_status(State(state): State<AppState>) -> Json<StatusResponse> {
    let meta = IndexMeta::load(&state.index_dir);
    Json(StatusResponse {
        indexed_at: meta.as_ref().map(|m| m.indexed_at.clone()),
        root: meta.as_ref().map(|m| m.root.clone()),
        file_count: meta.as_ref().map(|m| m.file_count),
        symbol_count: meta.as_ref().map(|m| m.symbol_count),
    })
}

async fn api_symbols(
    State(state): State<AppState>,
) -> Result<Json<Vec<FileSymbols>>, StatusCode> {
    let symbols = load_cached_symbols(&state.index_dir).unwrap_or_default();
    Ok(Json(symbols))
}

// ─── Public entry point ────────────────────────────────────────────────────

/// Start the HTTP server, block until interrupted.
///
/// The server binds on `host:port` (defaults: `localhost:3741`).
pub async fn run_server(
    index_dir: &Path,
    host: &str,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let state = AppState {
        index_dir: Arc::new(index_dir.to_owned()),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(serve_ui))
        .route("/api/status", get(api_status))
        .route("/api/symbols", get(api_symbols))
        .layer(cors)
        .with_state(state);

    let addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    println!("rust-sniffer serve  listening on  http://{addr}");
    println!("  GET /            — Symbol Explorer web UI");
    println!("  GET /api/status  — index metadata (JSON)");
    println!("  GET /api/symbols — symbol list (JSON)");
    println!();
    println!("Press Ctrl+C to stop.");

    axum::serve(listener, app).await?;
    Ok(())
}
