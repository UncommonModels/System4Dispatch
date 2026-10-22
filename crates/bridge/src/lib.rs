//! `s4d-bridge`: an OpenLCB hub (`rolcb::hub`) plus a web server that serves
//! the System4Dispatch app, a shared layout file, and a `/ws` endpoint that
//! attaches each browser to the hub.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use rolcb::gridconnect::{self, Decoder};
use rolcb::hub::Hub;
use rust_embed::RustEmbed;
use serde::Serialize;

/// The web app, as built by `dx bundle --platform web --release` (see the
/// README). When it hasn't been built, a placeholder page is served instead.
#[derive(RustEmbed)]
#[folder = "../../target/dx/system4dispatch/release/web/public"]
#[allow_missing = true]
struct WebApp;

#[derive(Clone)]
pub struct AppState {
    pub hub: Arc<Hub>,
    /// Shared layout file; `None` disables `/layout.json`.
    pub layout_path: Option<PathBuf>,
    /// Human-readable list of uplinks for the info endpoint.
    pub uplinks: Vec<String>,
    /// Serve files from this directory instead of the embedded app.
    pub static_dir: Option<PathBuf>,
}

#[derive(Serialize)]
struct Info {
    version: &'static str,
    uplinks: Vec<String>,
    shared_layout: bool,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/info", get(info))
        .route("/layout.json", get(get_layout).put(put_layout))
        .route("/ws", get(ws_upgrade))
        .fallback(static_file)
        .with_state(state)
}

async fn info(State(s): State<AppState>) -> Json<Info> {
    Json(Info {
        version: env!("CARGO_PKG_VERSION"),
        uplinks: s.uplinks.clone(),
        shared_layout: s.layout_path.is_some(),
    })
}

async fn get_layout(State(s): State<AppState>) -> Response {
    let Some(path) = s.layout_path else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match tokio::fs::read_to_string(&path).await {
        Ok(text) => (
            [
                (header::CONTENT_TYPE, "application/json"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            text,
        )
            .into_response(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn put_layout(State(s): State<AppState>, body: String) -> Response {
    let Some(path) = s.layout_path else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if let Err(e) = s4d_core::Layout::from_json(&body) {
        return (StatusCode::BAD_REQUEST, format!("invalid layout: {e}")).into_response();
    }
    // Write to a temp file and rename, so a crash never leaves half a file.
    let tmp = path.with_extension("json.tmp");
    let result = async {
        tokio::fs::write(&tmp, body.as_bytes()).await?;
        tokio::fs::rename(&tmp, &path).await
    }
    .await;
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn ws_upgrade(State(s): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| relay(socket, s.hub))
}

/// Attach one browser to the hub until either side goes away.
async fn relay(socket: WebSocket, hub: Arc<Hub>) {
    let mut port = hub.port();
    let (mut sink, mut stream) = socket.split();
    let mut decoder = Decoder::new();
    loop {
        tokio::select! {
            msg = stream.next() => match msg {
                Some(Ok(WsMessage::Text(t))) => {
                    for f in decoder.push(t.as_bytes()).into_iter().flatten() {
                        port.send(f);
                    }
                }
                Some(Ok(WsMessage::Binary(b))) => {
                    for f in decoder.push(&b).into_iter().flatten() {
                        port.send(f);
                    }
                }
                Some(Ok(WsMessage::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            },
            frame = port.recv() => {
                let Some(first) = frame else { break };
                let mut text = gridconnect::format(&first);
                text.push('\n');
                while let Some(f) = port.try_recv() {
                    text.push_str(&gridconnect::format(&f));
                    text.push('\n');
                    if text.len() > 16 * 1024 {
                        break;
                    }
                }
                if sink.send(WsMessage::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
    }
}

const PLACEHOLDER: &str = r#"<!doctype html><meta charset="utf-8"><title>s4d-bridge</title>
<body style="font-family:system-ui;max-width:40em;margin:3em auto;padding:0 1em">
<h1>s4d-bridge is running</h1>
<p>This binary was built without the web app embedded. Build it with
<code>dx bundle --platform web --release -p system4dispatch</code>, then rebuild the bridge,
or start it with <code>--static-dir</code> pointing at a built app.</p>
<p>The OpenLCB WebSocket endpoint is available at <code>/ws</code>.</p>"#;

async fn static_file(State(s): State<AppState>, uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    if path.split('/').any(|seg| seg == "..") {
        return StatusCode::BAD_REQUEST.into_response();
    }

    if let Some(dir) = &s.static_dir {
        let full = dir.join(path);
        let full = if full.is_file() {
            full
        } else {
            dir.join("index.html")
        };
        return match tokio::fs::read(&full).await {
            Ok(bytes) => file_response(&full.to_string_lossy(), bytes),
            Err(_) => StatusCode::NOT_FOUND.into_response(),
        };
    }

    match WebApp::get(path).or_else(|| {
        if path.contains('.') {
            None
        } else {
            WebApp::get("index.html")
        }
    }) {
        Some(file) => {
            let mime = file.metadata.mimetype().to_string();
            (
                [(header::CONTENT_TYPE, mime)],
                Body::from(file.data.into_owned()),
            )
                .into_response()
        }
        None if path == "index.html" => (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            PLACEHOLDER,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn file_response(name: &str, bytes: Vec<u8>) -> Response {
    let ty = match name.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript",
        "wasm" => "application/wasm",
        "css" => "text/css",
        "json" | "webmanifest" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    };
    ([(header::CONTENT_TYPE, ty)], bytes).into_response()
}
