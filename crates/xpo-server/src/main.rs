mod http;
mod state;
mod ws;

use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tracing::{error, info};

const DEFAULT_HTTP_PORT: u16 = 8080;
const DEFAULT_WS_PORT: u16 = 8081;
const DEFAULT_JWT_SECRET: &str = "xpo-dev-secret-for-local-testing";

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "xpo_server=info".parse().unwrap()),
        )
        .init();

    let http_port = std::env::var("HTTP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_HTTP_PORT);

    let ws_port = std::env::var("WS_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_WS_PORT);

    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| DEFAULT_JWT_SECRET.to_string());

    let state = state::ServerState::new(jwt_secret);

    let http_state = state.clone();
    let http_handle = tokio::spawn(async move {
        let listener = TcpListener::bind(format!("0.0.0.0:{http_port}"))
            .await
            .unwrap();
        info!(port = http_port, "http listening");
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(v) => v,
                Err(e) => {
                    error!("http accept: {e}");
                    continue;
                }
            };
            let s = http_state.clone();
            tokio::spawn(async move {
                let io = TokioIo::new(stream);
                let svc = service_fn(move |req| {
                    let s = s.clone();
                    async move { http::handle_http(req, s).await }
                });
                let _ = http1::Builder::new()
                    .serve_connection(io, svc)
                    .with_upgrades()
                    .await;
            });
        }
    });

    let ws_state = state.clone();
    let ws_handle = tokio::spawn(async move {
        let listener = TcpListener::bind(format!("0.0.0.0:{ws_port}"))
            .await
            .unwrap();
        info!(port = ws_port, "ws listening");
        loop {
            let (stream, addr) = match listener.accept().await {
                Ok(v) => v,
                Err(e) => {
                    error!("ws accept: {e}");
                    continue;
                }
            };
            info!(addr = %addr, "tunnel client connecting");
            let s = ws_state.clone();
            tokio::spawn(ws::handle_websocket(stream, s));
        }
    });

    let _ = tokio::join!(http_handle, ws_handle);
}
