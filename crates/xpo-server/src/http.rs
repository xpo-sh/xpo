use crate::state::{PendingRequest, SharedState, TunnelMessage};
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::{Request, Response, StatusCode};
use std::time::Duration;
use xpo_core::StreamId;

pub async fn handle_http(
    req: Request<hyper::body::Incoming>,
    state: SharedState,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let host = req
        .headers()
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    let subdomain = extract_subdomain(host);

    if subdomain.is_empty() {
        return Ok(text_response(StatusCode::NOT_FOUND, "no tunnel found"));
    }

    let tunnel_tx = match state.tunnels.get(&subdomain) {
        Some(t) => t.tx.clone(),
        None => {
            return Ok(text_response(
                StatusCode::NOT_FOUND,
                &format!("tunnel '{subdomain}' not found"),
            ));
        }
    };

    let stream_id = StreamId::new();
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

    state
        .pending
        .insert(stream_id, PendingRequest { response_tx });

    let raw_request = serialize_request(&req, host).await;

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

fn extract_subdomain(host: &str) -> String {
    let host = host.split(':').next().unwrap_or(host);
    if host.ends_with(".localhost") {
        return host.trim_end_matches(".localhost").to_string();
    }
    if host.ends_with(".xpo.sh") {
        return host.trim_end_matches(".xpo.sh").to_string();
    }
    String::new()
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
            if !name.eq_ignore_ascii_case("transfer-encoding") {
                builder = builder.header(name, value);
            }
        }
    }

    builder
        .body(Full::new(Bytes::copy_from_slice(body)))
        .unwrap_or_else(|_| text_response(status, "response build error"))
}

fn text_response(status: StatusCode, body: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain")
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_subdomain_localhost() {
        assert_eq!(extract_subdomain("myapp.localhost:8080"), "myapp");
        assert_eq!(extract_subdomain("myapp.localhost"), "myapp");
        assert_eq!(extract_subdomain("test-app.localhost:8080"), "test-app");
    }

    #[test]
    fn extract_subdomain_xpo_sh() {
        assert_eq!(extract_subdomain("myapp.xpo.sh"), "myapp");
        assert_eq!(extract_subdomain("myapp.xpo.sh:443"), "myapp");
    }

    #[test]
    fn extract_subdomain_none() {
        assert_eq!(extract_subdomain("localhost:8080"), "");
        assert_eq!(extract_subdomain("example.com"), "");
        assert_eq!(extract_subdomain(""), "");
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
