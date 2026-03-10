use crate::state::{ActiveStream, PendingRequest, SharedState, TunnelMessage};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use xpo_core::error_page::ErrorPage;
use xpo_core::StreamId;

pub async fn handle_http(
    req: Request<hyper::body::Incoming>,
    state: SharedState,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let host = req
        .headers()
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();

    let subdomain = extract_subdomain(&host, &state.config.base_domain);

    if subdomain.is_empty() {
        if req.uri().path() == "/healthz" {
            return Ok(healthz_response(&state));
        }
        return Ok(text_response(StatusCode::NOT_FOUND, "Tunnel not found", ""));
    }

    let (tunnel_tx, tunnel_password) = match state.tunnels.get(&subdomain) {
        Some(t) => (t.tx.clone(), t.password.clone()),
        None => {
            return Ok(text_response(
                StatusCode::NOT_FOUND,
                &format!("{subdomain}.{} is not connected", state.config.base_domain),
                "The tunnel may have been closed",
            ));
        }
    };

    if let Some(ref password) = tunnel_password {
        let authorized = req
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(|v| verify_basic_auth(v, password))
            .unwrap_or(false);
        if !authorized {
            return Ok(Response::builder()
                .status(401)
                .header("WWW-Authenticate", "Basic realm=\"xpo\"")
                .header("content-type", "text/html; charset=utf-8")
                .body(Full::new(Bytes::from(
                    ErrorPage::new(401, "Authentication Required")
                        .hint("This tunnel is password-protected")
                        .render_html(),
                )))
                .unwrap());
        }
    }

    let is_ws = req
        .headers()
        .get("upgrade")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"));

    if is_ws {
        return handle_ws_upgrade(req, state, subdomain, tunnel_tx, host).await;
    }

    let stream_id = StreamId::new();
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

    state
        .pending
        .insert(stream_id, PendingRequest { response_tx });

    let raw_request = serialize_request(req, &host).await;

    match tunnel_tx.try_send(TunnelMessage::HttpRequest {
        stream_id,
        raw_request,
    }) {
        Ok(()) => {}
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
            state.pending.remove(&stream_id);
            return Ok(text_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Tunnel overloaded",
                "Too many concurrent requests",
            ));
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
            state.pending.remove(&stream_id);
            return Ok(text_response(
                StatusCode::BAD_GATEWAY,
                "Tunnel disconnected",
                "The client may have lost connection",
            ));
        }
    }

    match tokio::time::timeout(Duration::from_secs(30), response_rx).await {
        Ok(Ok(raw_response)) => Ok(parse_response(&raw_response)),
        Ok(Err(_)) => {
            state.pending.remove(&stream_id);
            Ok(text_response(
                StatusCode::BAD_GATEWAY,
                "Tunnel dropped request",
                "The client closed before responding",
            ))
        }
        Err(_) => {
            state.pending.remove(&stream_id);
            Ok(text_response(
                StatusCode::GATEWAY_TIMEOUT,
                "Upstream timeout",
                "The local server didn't respond in time",
            ))
        }
    }
}

async fn handle_ws_upgrade(
    req: Request<hyper::body::Incoming>,
    state: SharedState,
    subdomain: String,
    tunnel_tx: tokio::sync::mpsc::Sender<TunnelMessage>,
    host: String,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let stream_id = StreamId::new();
    let raw_request = serialize_request_headers(&req, &host);

    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    state
        .pending
        .insert(stream_id, PendingRequest { response_tx });

    match tunnel_tx.try_send(TunnelMessage::HttpRequest {
        stream_id,
        raw_request,
    }) {
        Ok(()) => {}
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
            state.pending.remove(&stream_id);
            return Ok(text_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Tunnel overloaded",
                "Too many concurrent requests",
            ));
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
            state.pending.remove(&stream_id);
            return Ok(text_response(
                StatusCode::BAD_GATEWAY,
                "Tunnel disconnected",
                "The client may have lost connection",
            ));
        }
    }

    let raw_response = match tokio::time::timeout(Duration::from_secs(10), response_rx).await {
        Ok(Ok(data)) => data,
        _ => {
            state.pending.remove(&stream_id);
            return Ok(text_response(
                StatusCode::BAD_GATEWAY,
                "WebSocket upgrade failed",
                "The local server rejected the upgrade",
            ));
        }
    };

    let resp_str = String::from_utf8_lossy(&raw_response);
    if !resp_str.starts_with("HTTP/1.1 101") {
        return Ok(parse_response(&raw_response));
    }

    tracing::debug!(subdomain = %subdomain, "ws upgrade");

    let (from_client_tx, mut from_client_rx) = tokio::sync::mpsc::unbounded_channel();
    state.add_stream(
        stream_id,
        &subdomain,
        ActiveStream {
            from_client_tx,
            tunnel_subdomain: subdomain.clone(),
        },
    );

    let state_clone = state.clone();

    tokio::spawn(async move {
        let upgraded = match hyper::upgrade::on(req).await {
            Ok(u) => u,
            Err(_) => {
                state_clone.remove_stream(&stream_id);
                return;
            }
        };

        let mut io = TokioIo::new(upgraded);
        let mut buf = [0u8; 8192];

        loop {
            tokio::select! {
                result = io.read(&mut buf) => {
                    match result {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if tunnel_tx.try_send(TunnelMessage::StreamData {
                                stream_id,
                                data: buf[..n].to_vec(),
                            }).is_err() {
                                break;
                            }
                        }
                    }
                }
                data = from_client_rx.recv() => {
                    match data {
                        Some(bytes) => {
                            if io.write_all(&bytes).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
            }
        }

        let _ = tunnel_tx.try_send(TunnelMessage::StreamEnd { stream_id });
        state_clone.remove_stream(&stream_id);
    });

    let mut resp = Response::new(Full::new(Bytes::new()));
    *resp.status_mut() = StatusCode::SWITCHING_PROTOCOLS;

    let header_str = String::from_utf8_lossy(&raw_response);
    for line in header_str.lines().skip(1) {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim();
            let value = value.trim();
            if let (Ok(hn), Ok(hv)) = (name.parse::<hyper::header::HeaderName>(), value.parse()) {
                resp.headers_mut().insert(hn, hv);
            }
        }
    }

    Ok(resp)
}

fn verify_basic_auth(header_value: &str, expected_password: &str) -> bool {
    use base64::Engine;
    let encoded = header_value.strip_prefix("Basic ").unwrap_or("");
    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded) {
        if let Ok(credentials) = String::from_utf8(decoded) {
            if let Some((_user, pass)) = credentials.split_once(':') {
                return pass == expected_password;
            }
        }
    }
    false
}

fn extract_subdomain(host: &str, base_domain: &str) -> String {
    let host = host.split(':').next().unwrap_or(host);
    let suffix = format!(".{base_domain}");
    let sub = if host.ends_with(&suffix) {
        host.strip_suffix(&suffix).unwrap_or("").to_string()
    } else if host.ends_with(".localhost") {
        host.strip_suffix(".localhost").unwrap_or("").to_string()
    } else {
        return String::new();
    };
    if sub.contains('.') {
        return String::new();
    }
    sub
}

fn healthz_response(state: &SharedState) -> Response<Full<Bytes>> {
    let uptime = state.config.started_at.elapsed().as_secs();
    let active_tunnels = state.tunnels.len();
    let active_streams = state.streams.len();

    let body = format!(
        r#"{{"status":"ok","version":"{}","region":"{}","instance":"{}","uptime_secs":{},"active_tunnels":{},"active_streams":{}}}"#,
        env!("CARGO_PKG_VERSION"),
        state.config.region,
        state.config.instance_id,
        uptime,
        active_tunnels,
        active_streams,
    );

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .header("content-length", body.len())
        .body(Full::new(Bytes::from(body)))
        .unwrap()
}

async fn serialize_request(req: Request<hyper::body::Incoming>, host: &str) -> Vec<u8> {
    let mut bytes = serialize_request_headers(&req, host);
    let body = req
        .into_body()
        .collect()
        .await
        .map(|c| c.to_bytes())
        .unwrap_or_default();
    if !body.is_empty() {
        bytes.extend_from_slice(&body);
    }
    bytes
}

fn serialize_request_headers(req: &Request<hyper::body::Incoming>, host: &str) -> Vec<u8> {
    let method = req.method().as_str();
    let path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/");
    let mut raw = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\n");
    for (name, value) in req.headers() {
        if name == "host" {
            continue;
        }
        if let Ok(v) = value.to_str() {
            raw.push_str(name.as_str());
            raw.push_str(": ");
            raw.push_str(v);
            raw.push_str("\r\n");
        }
    }
    raw.push_str("\r\n");
    raw.into_bytes()
}

fn parse_response(raw: &[u8]) -> Response<Full<Bytes>> {
    if raw.is_empty() {
        return text_response(
            StatusCode::BAD_GATEWAY,
            "Empty response from upstream",
            "The local server sent no data",
        );
    }

    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(raw.len());

    let header_part = &raw[..header_end];
    let body_start = std::cmp::min(header_end + 4, raw.len());
    let body = &raw[body_start..];

    let header_str = String::from_utf8_lossy(header_part);
    let mut lines = header_str.lines();

    let status_line = lines.next().unwrap_or("HTTP/1.1 502 Bad Gateway");
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(502);

    let status = StatusCode::from_u16(status_code).unwrap_or(StatusCode::BAD_GATEWAY);

    let mut builder = Response::builder().status(status);
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim();
            let value = value.trim();
            if name.eq_ignore_ascii_case("transfer-encoding")
                || name.eq_ignore_ascii_case("content-length")
            {
                continue;
            }
            if let (Ok(hn), Ok(hv)) = (
                name.parse::<hyper::header::HeaderName>(),
                value.parse::<hyper::header::HeaderValue>(),
            ) {
                builder = builder.header(hn, hv);
            }
        }
    }

    let body_bytes = Bytes::copy_from_slice(body);
    builder = builder.header("content-length", body_bytes.len());
    builder
        .body(Full::new(body_bytes))
        .unwrap_or_else(|_| text_response(status, "Response build error", ""))
}

fn text_response(status: StatusCode, message: &str, hint: &str) -> Response<Full<Bytes>> {
    let html = ErrorPage::new(status.as_u16(), message)
        .hint(hint)
        .render_html();
    Response::builder()
        .status(status)
        .header("content-type", "text/html; charset=utf-8")
        .header("content-length", html.len())
        .body(Full::new(Bytes::from(html)))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_subdomain_localhost() {
        assert_eq!(extract_subdomain("myapp.localhost:8080", "xpo.sh"), "myapp");
        assert_eq!(extract_subdomain("myapp.localhost", "xpo.sh"), "myapp");
        assert_eq!(
            extract_subdomain("test-app.localhost:8080", "xpo.sh"),
            "test-app"
        );
    }

    #[test]
    fn extract_subdomain_xpo_sh() {
        assert_eq!(extract_subdomain("myapp.xpo.sh", "xpo.sh"), "myapp");
        assert_eq!(extract_subdomain("myapp.xpo.sh:443", "xpo.sh"), "myapp");
    }

    #[test]
    fn extract_subdomain_custom_domain() {
        assert_eq!(extract_subdomain("myapp.tunnel.dev", "tunnel.dev"), "myapp");
        assert_eq!(
            extract_subdomain("myapp.tunnel.dev:8080", "tunnel.dev"),
            "myapp"
        );
    }

    #[test]
    fn extract_subdomain_none() {
        assert_eq!(extract_subdomain("localhost:8080", "xpo.sh"), "");
        assert_eq!(extract_subdomain("xpo.sh", "xpo.sh"), "");
        assert_eq!(extract_subdomain("example.com", "xpo.sh"), "");
        assert_eq!(extract_subdomain("", "xpo.sh"), "");
    }

    #[test]
    fn extract_subdomain_rejects_multi_level() {
        assert_eq!(extract_subdomain("a.b.xpo.sh", "xpo.sh"), "");
        assert_eq!(extract_subdomain("deep.sub.localhost:8080", "xpo.sh"), "");
    }

    #[test]
    fn text_response_escapes_xss_in_body() {
        let html =
            ErrorPage::new(404, "<script>alert(1)</script>.xpo.sh is not connected").render_html();
        assert!(
            !html.contains("<script>"),
            "rendered HTML must not contain raw <script>"
        );
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains(".xpo.sh is not connected"));
    }

    #[test]
    fn parse_response_basic() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<h1>hello</h1>";
        let resp = parse_response(raw);
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn parse_response_empty() {
        let resp = parse_response(b"");
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn verify_basic_auth_valid() {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode("user:secret123");
        let header = format!("Basic {encoded}");
        assert!(verify_basic_auth(&header, "secret123"));
    }

    #[test]
    fn verify_basic_auth_invalid() {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode("user:wrongpass");
        let header = format!("Basic {encoded}");
        assert!(!verify_basic_auth(&header, "secret123"));
    }

    #[test]
    fn verify_basic_auth_any_username() {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode("anyuser:mypass");
        let header = format!("Basic {encoded}");
        assert!(verify_basic_auth(&header, "mypass"));

        let encoded2 = base64::engine::general_purpose::STANDARD.encode(":mypass");
        let header2 = format!("Basic {encoded2}");
        assert!(verify_basic_auth(&header2, "mypass"));
    }
}
