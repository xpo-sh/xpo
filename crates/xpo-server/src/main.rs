mod acme;
mod config;
mod http;
mod state;
mod tls;
mod ws;

use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() {
    let config = config::ServerConfig::from_env();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "xpo_server=info".parse().unwrap()),
        )
        .init();

    info!(
        http_port = config.http_port,
        ws_port = config.ws_port,
        base_domain = %config.base_domain,
        scheme = %config.scheme,
        region = %config.region,
        instance = %config.instance_id,
        tls = config.tls_enabled(),
        "xpo-server starting"
    );

    if config.acme_enabled && !cert_exists_on_disk(&config) {
        info!("provisioning TLS certificate via ACME");
        if let Err(e) = acme::provision_cert(&config).await {
            error!("ACME provisioning failed: {e}");
            std::process::exit(1);
        }
    }

    let (tls_acceptor, cert_resolver) = match tls::build_tls(&config) {
        Some((acceptor, resolver)) => {
            info!("TLS enabled");
            (Some(acceptor), Some(resolver))
        }
        None => {
            if config.acme_enabled {
                let certs = tls::load_certs(&config.cert_path());
                let key = tls::load_key(&config.key_path());
                let (acceptor, resolver) = tls::make_tls(certs, key);
                info!("TLS enabled (ACME cert loaded)");
                (Some(acceptor), Some(resolver))
            } else {
                info!("TLS disabled (plain TCP)");
                (None, None)
            }
        }
    };

    let http_port = config.http_port;
    let ws_port = config.ws_port;
    let config = Arc::new(config);

    if config.acme_enabled {
        if let Some(resolver) = cert_resolver {
            acme::spawn_renewal_task(config.clone(), resolver);
        }
    }

    let state = state::ServerState::new(config);

    let http_state = state.clone();
    let http_tls = tls_acceptor.clone();
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
            let tls = http_tls.clone();
            tokio::spawn(async move {
                serve_http(stream, s, tls).await;
            });
        }
    });

    let ws_state = state.clone();
    let ws_tls = tls_acceptor;
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
            let tls = ws_tls.clone();
            tokio::spawn(async move {
                serve_ws(stream, s, tls).await;
            });
        }
    });

    let _ = tokio::join!(http_handle, ws_handle);
}

fn cert_exists_on_disk(config: &config::ServerConfig) -> bool {
    std::path::Path::new(&config.cert_path()).exists()
        && std::path::Path::new(&config.key_path()).exists()
}

async fn serve_http(
    stream: tokio::net::TcpStream,
    state: state::SharedState,
    tls: Option<TlsAcceptor>,
) {
    if let Some(acceptor) = tls {
        match acceptor.accept(stream).await {
            Ok(tls_stream) => {
                let io = TokioIo::new(tls_stream);
                let svc = service_fn(move |req| {
                    let s = state.clone();
                    async move { http::handle_http(req, s).await }
                });
                let _ = http1::Builder::new()
                    .serve_connection(io, svc)
                    .with_upgrades()
                    .await;
            }
            Err(e) => warn!("tls handshake: {e}"),
        }
    } else {
        let io = TokioIo::new(stream);
        let svc = service_fn(move |req| {
            let s = state.clone();
            async move { http::handle_http(req, s).await }
        });
        let _ = http1::Builder::new()
            .serve_connection(io, svc)
            .with_upgrades()
            .await;
    }
}

async fn serve_ws(
    stream: tokio::net::TcpStream,
    state: state::SharedState,
    tls: Option<TlsAcceptor>,
) {
    if let Some(acceptor) = tls {
        match acceptor.accept(stream).await {
            Ok(tls_stream) => ws::handle_websocket(tls_stream, state).await,
            Err(e) => warn!("tls handshake: {e}"),
        }
    } else {
        ws::handle_websocket(stream, state).await;
    }
}
