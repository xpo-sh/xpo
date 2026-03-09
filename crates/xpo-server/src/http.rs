use crate::state::{ActiveStream, PendingRequest, SharedState, TunnelMessage};
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
        return Ok(text_response(StatusCode::NOT_FOUND, "Tunnel not found"));
    }

    let tunnel_tx = match state.tunnels.get(&subdomain) {
        Some(t) => t.tx.clone(),
        None => {
            return Ok(text_response(
                StatusCode::NOT_FOUND,
                &format!("<b>{subdomain}</b>.xpo.sh is not connected"),
            ));
        }
    };

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

    let raw_request = serialize_request(&req, &host).await;

    if tunnel_tx
        .send(TunnelMessage::HttpRequest {
            stream_id,
            raw_request,
        })
        .is_err()
    {
        state.pending.remove(&stream_id);
        return Ok(text_response(
            StatusCode::BAD_GATEWAY,
            "tunnel disconnected",
        ));
    }

    match tokio::time::timeout(Duration::from_secs(30), response_rx).await {
        Ok(Ok(raw_response)) => Ok(parse_response(&raw_response)),
        Ok(Err(_)) => {
            state.pending.remove(&stream_id);
            Ok(text_response(
                StatusCode::BAD_GATEWAY,
                "tunnel dropped request",
            ))
        }
        Err(_) => {
            state.pending.remove(&stream_id);
            Ok(text_response(
                StatusCode::GATEWAY_TIMEOUT,
                "upstream timeout",
            ))
        }
    }
}

async fn handle_ws_upgrade(
    req: Request<hyper::body::Incoming>,
    state: SharedState,
    subdomain: String,
    tunnel_tx: tokio::sync::mpsc::UnboundedSender<TunnelMessage>,
    host: String,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let stream_id = StreamId::new();
    let raw_request = serialize_request(&req, &host).await;

    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    state
        .pending
        .insert(stream_id, PendingRequest { response_tx });

    if tunnel_tx
        .send(TunnelMessage::HttpRequest {
            stream_id,
            raw_request,
        })
        .is_err()
    {
        state.pending.remove(&stream_id);
        return Ok(text_response(
            StatusCode::BAD_GATEWAY,
            "tunnel disconnected",
        ));
    }

    let raw_response = match tokio::time::timeout(Duration::from_secs(10), response_rx).await {
        Ok(Ok(data)) => data,
        _ => {
            state.pending.remove(&stream_id);
            return Ok(text_response(StatusCode::BAD_GATEWAY, "ws upgrade failed"));
        }
    };

    let resp_str = String::from_utf8_lossy(&raw_response);
    if !resp_str.starts_with("HTTP/1.1 101") {
        return Ok(parse_response(&raw_response));
    }

    tracing::debug!(subdomain = %subdomain, "ws upgrade");

    let (from_client_tx, mut from_client_rx) = tokio::sync::mpsc::unbounded_channel();
    state.streams.insert(
        stream_id,
        ActiveStream {
            from_client_tx,
            tunnel_subdomain: subdomain,
        },
    );

    let tunnel_tx_clone = tunnel_tx;
    let state_clone = state.clone();

    tokio::spawn(async move {
        let upgraded = match hyper::upgrade::on(req).await {
            Ok(u) => u,
            Err(_) => {
                state_clone.streams.remove(&stream_id);
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
                            let _ = tunnel_tx_clone.send(TunnelMessage::StreamData {
                                stream_id,
                                data: buf[..n].to_vec(),
                            });
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

        let _ = tunnel_tx_clone.send(TunnelMessage::StreamEnd { stream_id });
        state_clone.streams.remove(&stream_id);
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

fn extract_subdomain(host: &str, base_domain: &str) -> String {
    let host = host.split(':').next().unwrap_or(host);
    let suffix = format!(".{base_domain}");
    if host.ends_with(&suffix) {
        return host.strip_suffix(&suffix).unwrap_or("").to_string();
    }
    if host.ends_with(".localhost") {
        return host.strip_suffix(".localhost").unwrap_or("").to_string();
    }
    String::new()
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

async fn serialize_request(req: &Request<hyper::body::Incoming>, host: &str) -> Vec<u8> {
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
            raw.push_str(&format!("{}: {v}\r\n", name));
        }
    }
    raw.push_str("\r\n");
    raw.into_bytes()
}

fn parse_response(raw: &[u8]) -> Response<Full<Bytes>> {
    if raw.is_empty() {
        return text_response(StatusCode::BAD_GATEWAY, "empty response from upstream");
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
            builder = builder.header(name, value);
        }
    }

    let body_bytes = Bytes::copy_from_slice(body);
    builder = builder.header("content-length", body_bytes.len());
    builder
        .body(Full::new(body_bytes))
        .unwrap_or_else(|_| text_response(status, "response build error"))
}

fn text_response(status: StatusCode, body: &str) -> Response<Full<Bytes>> {
    let code = status.as_u16();
    let html = format!(
        "<!DOCTYPE html>\
        <html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
        <title>{code} - xpo.sh</title>\
        <style>\
        *{{margin:0;padding:0;box-sizing:border-box}}\
        body{{font-family:'SF Mono','JetBrains Mono','Fira Code',Menlo,Consolas,monospace;\
        display:flex;align-items:center;justify-content:center;height:100vh;\
        background:#0a0a0f;color:#e2e2e8}}\
        .c{{text-align:center}}\
        .code{{font-size:96px;font-weight:800;line-height:1;color:#1e1e2e}}\
        .msg{{margin:16px 0 0;font-size:15px}}\
        .msg b{{color:#22d3ee}}\
        .hint{{color:#555570;font-size:13px;margin:8px 0 0}}\
        a{{color:#22d3ee;text-decoration:none}}\
        a:hover{{text-decoration:underline}}\
        .brand{{position:fixed;bottom:24px;color:#555570;font-size:12px}}\
        .brand span{{color:#22d3ee;font-weight:600}}\
        @media(prefers-color-scheme:light){{\
        body{{background:#f5f6f8;color:#111827}}\
        .code{{color:#e2e4e9}}\
        .msg b{{color:#0891b2}}\
        .hint{{color:#6b7280}}\
        a{{color:#0891b2}}\
        .brand{{color:#6b7280}}\
        .brand span{{color:#0891b2}}\
        }}\
        </style></head>\
        <body><div class=\"c\">\
        <p class=\"code\">{code}</p>\
        <p class=\"msg\">{body}</p>\
        <p class=\"hint\"><a href=\"https://xpo.sh\">xpo.sh</a></p>\
        </div>\
        <div class=\"brand\"><span>xpo</span>.sh</div>\
        </body></html>"
    );
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
}
