use crate::{
    mcp::Gateway,
    store::{err, AppResult, Store},
};
use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Router,
};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use std::sync::Arc;
use subtle::ConstantTimeEq;
use tokio_util::sync::CancellationToken;

async fn authenticate(State(store): State<Arc<Store>>, req: Request<Body>, next: Next) -> Response {
    if req.headers().contains_key("origin") {
        return (
            StatusCode::FORBIDDEN,
            "Browser origins are not enabled for this machine-to-machine endpoint",
        )
            .into_response();
    }
    let expected = store.token.lock().unwrap().clone();
    let token = req
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or("");
    if !bool::from(token.as_bytes().ct_eq(expected.as_bytes())) {
        return (StatusCode::UNAUTHORIZED, "Invalid bearer token").into_response();
    }
    next.run(req).await
}
pub async fn start(store: Arc<Store>) -> AppResult<()> {
    if store.live.lock().unwrap().server.is_some() {
        return Ok(());
    }
    let port = store.config().port;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|e| format!("无法监听本机端口 {port}：{e}"))?;
    let cancel = CancellationToken::new();
    let generation = uuid::Uuid::new_v4().to_string();
    let mut config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true);
    config.cancellation_token = cancel.clone();
    config.max_request_body_bytes = 2 * 1024 * 1024;
    // cloudflared sets the origin Host explicitly to loopback; public Host is never trusted.
    let state = store.clone();
    let service = StreamableHttpService::new(
        move || Ok(Gateway::new(state.clone())),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let router = Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(store.clone(), authenticate));
    {
        let mut live = store.live.lock().unwrap();
        live.server = Some(cancel.clone());
        live.port = Some(port);
        live.server_id = Some(generation.clone());
    }
    store.audit("server.start", &format!("127.0.0.1:{port}"), true);
    tauri::async_runtime::spawn(async move {
        if let Err(e) = axum::serve(listener, router)
            .with_graceful_shutdown(cancel.cancelled_owned())
            .await
        {
            store.audit("server.error", &err(e), false);
        }
        let mut live = store.live.lock().unwrap();
        if live.server_id.as_ref() == Some(&generation) {
            live.server = None;
            live.port = None;
            live.server_id = None;
        }
    });
    Ok(())
}
pub fn stop(store: &Store) {
    store.cancel_commands();
    crate::proxy::disconnect_all(store);
    let mut live = store.live.lock().unwrap();
    if let Some(c) = live.tunnel.take() {
        c.cancel();
    }
    live.tunnel_url = None;
    live.tunnel_id = None;
    if let Some(c) = live.server.take() {
        c.cancel();
    }
    live.port = None;
    live.server_id = None;
    drop(live);
    store.audit("server.stop", "", true);
}
