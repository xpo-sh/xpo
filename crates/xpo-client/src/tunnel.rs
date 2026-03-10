use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use xpo_core::protocol::{ClientControl, Packet, PacketType, ServerControl};
use xpo_core::{HEARTBEAT_TIMEOUT_SECS, RECONNECT_MAX_SECS, RECONNECT_MIN_SECS};
use xpo_tui::app::{BannerInfo, TuiApp};
use xpo_tui::event::AppEvent;
use xpo_tui::model::{ConnStatus, RequestLog};

pub async fn run(
    port: u16,
    subdomain: Option<String>,
    server: &str,
    max_logs: usize,
    visible_rows: usize,
    cors: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let use_tui = TuiApp::check_terminal_size();
    let quit_flag = Arc::new(AtomicBool::new(false));
    let quit_notify = Arc::new(tokio::sync::Notify::new());

    let (app_tx, events) = TuiApp::create_channel();

    let tui_state = Arc::new(std::sync::Mutex::new(TuiThreadState {
        events: Some(events),
        handle: None,
    }));

    let qf_check = quit_flag.clone();
    let quit_notify2 = quit_notify.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if qf_check.load(Ordering::Relaxed) {
                quit_notify2.notify_one();
                break;
            }
        }
    });

    let mut backoff = RECONNECT_MIN_SECS;
    let mut first_connect = true;

    loop {
        match connect_and_run(
            port,
            subdomain.clone(),
            server,
            cors,
            &app_tx,
            &quit_flag,
            &quit_notify,
            use_tui,
            first_connect,
            &tui_state,
            max_logs,
            visible_rows,
        )
        .await
        {
            Ok(()) => break,
            Err(e) => {
                let msg = e.to_string();
                let short = if msg.contains("Connection refused") {
                    "server unreachable"
                } else if msg.contains("Connection reset") {
                    "connection lost"
                } else if msg.contains("heartbeat timeout") {
                    "heartbeat timeout"
                } else {
                    &msg
                };

                if use_tui {
                    let _ = app_tx.send(AppEvent::Connection(ConnStatus::Reconnecting {
                        attempt: (backoff / RECONNECT_MIN_SECS) as u32,
                        next_retry_secs: backoff,
                    }));
                } else {
                    eprintln!(
                        "  {} {}  (retry in {backoff}s)",
                        console::style("!").yellow().bold(),
                        short,
                    );
                }

                tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(RECONNECT_MAX_SECS);
                first_connect = false;

                if quit_flag.load(Ordering::Relaxed) {
                    break;
                }
            }
        }
    }

    drop(app_tx);

    let handle = tui_state.lock().unwrap().handle.take();
    if let Some(h) = handle {
        let _ = h.join();
    }

    Ok(())
}

struct TuiThreadState {
    events: Option<xpo_tui::event::EventHandler>,
    handle: Option<std::thread::JoinHandle<()>>,
}

#[allow(clippy::too_many_arguments)]
async fn connect_and_run(
    port: u16,
    subdomain: Option<String>,
    server: &str,
    cors: bool,
    app_tx: &std::sync::mpsc::Sender<AppEvent>,
    quit_flag: &Arc<AtomicBool>,
    quit_notify: &Arc<tokio::sync::Notify>,
    use_tui: bool,
    first_connect: bool,
    tui_state: &Arc<std::sync::Mutex<TuiThreadState>>,
    max_logs: usize,
    visible_rows: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let ws_url = if server.starts_with("localhost") || server.starts_with("127.0.0.1") {
        format!("ws://{server}")
    } else {
        format!("wss://{server}")
    };
    let (ws_stream, _) = connect_async(&ws_url).await?;
    let (mut ws_write, mut ws_read) = ws_stream.split();

    let token = get_token().await;
    let auth = ClientControl::Auth { token };
    ws_write.send(Message::Text(auth.to_json()?.into())).await?;

    let auth_resp = ws_read.next().await.ok_or("no auth response")??;

    let (user, tunnel_url) = match auth_resp {
        Message::Text(text) => match ServerControl::from_json(&text)? {
            ServerControl::AuthOk { user, .. } => {
                let hello = ClientControl::Hello { port, subdomain };
                ws_write
                    .send(Message::Text(hello.to_json()?.into()))
                    .await?;

                let hello_resp = ws_read.next().await.ok_or("no hello response")??;

                match hello_resp {
                    Message::Text(t) => match ServerControl::from_json(&t)? {
                        ServerControl::TunnelReady { url, .. } => (user, url),
                        ServerControl::Error { message } => {
                            return Err(format!("server error: {message}").into());
                        }
                        _ => return Err("unexpected server response".into()),
                    },
                    _ => return Err("unexpected message type".into()),
                }
            }
            ServerControl::AuthFail { reason } => {
                return Err(format!("auth failed: {reason}").into());
            }
            _ => return Err("unexpected auth response".into()),
        },
        _ => return Err("unexpected message type".into()),
    };

    let _ = app_tx.send(AppEvent::Connection(ConnStatus::Connected));

    if use_tui && first_connect {
        let mut ts = tui_state.lock().unwrap();
        if ts.handle.is_none() {
            let banner = BannerInfo {
                title: "xpo share".to_string(),
                url: tunnel_url.clone(),
                target: format!("localhost:{port}"),
                extra_lines: if user.is_empty() {
                    vec![]
                } else {
                    vec![user.clone()]
                },
                has_qr: true,
                qr_url: Some(tunnel_url.clone()),
            };

            let events = ts.events.take().unwrap();
            let qf = quit_flag.clone();

            ts.handle = Some(std::thread::spawn(move || {
                run_tui_loop(banner, max_logs, visible_rows, events, &qf);
            }));
        }
    } else if !use_tui && first_connect {
        legacy_print_banner(&tunnel_url, port, &user);
    }

    let (resp_tx, mut resp_rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let ws_relays: Arc<
        dashmap::DashMap<xpo_core::StreamId, tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
    > = Arc::new(dashmap::DashMap::new());

    loop {
        if quit_flag.load(Ordering::Relaxed) {
            return Ok(());
        }

        tokio::select! {
            _ = quit_notify.notified() => {
                return Ok(());
            }
            msg = tokio::time::timeout(
                std::time::Duration::from_secs(HEARTBEAT_TIMEOUT_SECS),
                ws_read.next(),
            ) => {
                match msg {
                    Ok(Some(Ok(Message::Binary(data)))) => {
                        if let Ok(packet) = Packet::decode(&data) {
                            match packet.packet_type {
                                PacketType::Connection => {}
                                PacketType::Data => {
                                    if let Some(relay_tx) = ws_relays.get(&packet.stream_id) {
                                        let _ = relay_tx.send(packet.payload);
                                    } else {
                                        let stream_id = packet.stream_id;
                                        let payload = packet.payload;
                                        let tx = resp_tx.clone();
                                        let relays = ws_relays.clone();
                                        let event_tx = app_tx.clone();
                                        tokio::spawn(async move {
                                            let result = proxy_to_upstream(port, &payload, stream_id, &event_tx, cors).await;
                                            let resp_pkt = Packet::data(stream_id, result.response);
                                            let _ = tx.send(Message::Binary(resp_pkt.encode().into()));
                                            if let Some(relay) = result.ws_relay {
                                                let (relay_data_tx, mut relay_data_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
                                                relays.insert(stream_id, relay_data_tx);
                                                let tx2 = tx.clone();
                                                tokio::spawn(async move {
                                                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                                                    let (mut ur, mut uw) = relay.upstream.into_split();
                                                    let tx3 = tx2.clone();
                                                    let sid = relay.stream_id;
                                                    let read_task = tokio::spawn(async move {
                                                        let mut buf = [0u8; 8192];
                                                        loop {
                                                            match ur.read(&mut buf).await {
                                                                Ok(0) | Err(_) => break,
                                                                Ok(n) => {
                                                                    let pkt = Packet::data(sid, buf[..n].to_vec());
                                                                    if tx3.send(Message::Binary(pkt.encode().into())).is_err() {
                                                                        break;
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    });
                                                    let write_task = tokio::spawn(async move {
                                                        while let Some(data) = relay_data_rx.recv().await {
                                                            if uw.write_all(&data).await.is_err() {
                                                                break;
                                                            }
                                                        }
                                                    });
                                                    let _ = tokio::join!(read_task, write_task);
                                                    let end_pkt = Packet::end(sid);
                                                    let _ = tx2.send(Message::Binary(end_pkt.encode().into()));
                                                    relays.remove(&sid);
                                                });
                                            } else {
                                                let end_pkt = Packet::end(stream_id);
                                                let _ = tx.send(Message::Binary(end_pkt.encode().into()));
                                            }
                                        });
                                    }
                                }
                                PacketType::Heartbeat => {
                                    let pong = Packet::pong();
                                    let _ = ws_write.send(Message::Binary(pong.encode().into())).await;
                                }
                                PacketType::End => {}
                                PacketType::Pong => {}
                            }
                        }
                    }
                    Ok(Some(Ok(Message::Close(_)))) | Ok(None) => {
                        return Err("server closed connection".into());
                    }
                    Ok(Some(Err(e))) => {
                        return Err(format!("ws error: {e}").into());
                    }
                    Err(_) => {
                        return Err("heartbeat timeout".into());
                    }
                    _ => {}
                }
            }
            Some(ws_msg) = resp_rx.recv() => {
                let _ = ws_write.send(ws_msg).await;
            }
        }
    }
}

fn run_tui_loop(
    banner: BannerInfo,
    max_logs: usize,
    visible_rows: usize,
    events: xpo_tui::event::EventHandler,
    quit_flag: &Arc<AtomicBool>,
) {
    let mut terminal = match TuiApp::init_terminal() {
        Ok(t) => t,
        Err(_) => return,
    };
    let mut app = TuiApp::new(banner, max_logs, visible_rows);

    loop {
        let _ = terminal.draw(|frame| xpo_tui::render::draw(frame, &app));

        match events.next() {
            Ok(event) => {
                app.handle_event(event);
                if app.should_quit {
                    quit_flag.store(true, Ordering::Relaxed);
                    break;
                }
            }
            Err(_) => break,
        }
    }

    let summary = app.summary_line();
    drop(terminal);
    TuiApp::restore_terminal();
    print!("\r\x1b[2K");
    println!("{summary}");
}

fn legacy_print_banner(url: &str, port: u16, user: &str) {
    let d = "\x1b[2m";
    let b = "\x1b[1m";
    let c = "\x1b[36;1m";
    let r = "\x1b[0m";

    let line1 = "xpo share";
    let line2 = format!("{url} -> localhost:{port}");
    let line3 = if user.is_empty() {
        "Ctrl+C to stop".to_string()
    } else {
        format!("{user} - Ctrl+C to stop")
    };

    let inner = line1.len().max(line2.len()).max(line3.len()) + 4;
    let border = "\u{2500}".repeat(inner);
    let empty = " ".repeat(inner);

    let pad1 = inner - line1.len() - 2;
    let pad2 = inner - line2.len() - 2;
    let pad3 = inner - line3.len() - 2;

    println!();
    println!("  {d}\u{256d}{border}\u{256e}{r}");
    println!("  {d}\u{2502}{r}{empty}{d}\u{2502}{r}");
    println!(
        "  {d}\u{2502}{r}  {b}{line1}{r}{}{d}\u{2502}{r}",
        " ".repeat(pad1)
    );
    println!("  {d}\u{2502}{r}{empty}{d}\u{2502}{r}");
    println!(
        "  {d}\u{2502}{r}  {c}{url}{r} -> localhost:{port}{}{d}\u{2502}{r}",
        " ".repeat(pad2)
    );
    println!("  {d}\u{2502}{r}{empty}{d}\u{2502}{r}");
    println!(
        "  {d}\u{2502}{r}  {d}{line3}{r}{}{d}\u{2502}{r}",
        " ".repeat(pad3)
    );
    println!("  {d}\u{2502}{r}{empty}{d}\u{2502}{r}");
    println!("  {d}\u{2570}{border}\u{256f}{r}");
    println!();
}

struct ProxyResult {
    response: Vec<u8>,
    ws_relay: Option<WsRelay>,
}

struct WsRelay {
    stream_id: xpo_core::StreamId,
    upstream: TcpStream,
}

static REQUEST_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

async fn proxy_to_upstream(
    port: u16,
    raw_request: &[u8],
    stream_id: xpo_core::StreamId,
    event_tx: &std::sync::mpsc::Sender<AppEvent>,
    cors: bool,
) -> ProxyResult {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let start = std::time::Instant::now();

    let req_str = String::from_utf8_lossy(raw_request);
    let parts: Vec<&str> = req_str.split_whitespace().collect();
    let method = parts.first().copied().unwrap_or("???").to_string();
    let path = parts.get(1).copied().unwrap_or("/").to_string();

    if cors && is_cors_preflight(raw_request) {
        let response = build_cors_preflight_response();
        send_request_log(event_tx, &method, &path, 204, start.elapsed());
        return ProxyResult {
            response,
            ws_relay: None,
        };
    }

    let is_ws_upgrade = {
        let s = String::from_utf8_lossy(raw_request).to_ascii_lowercase();
        s.contains("upgrade: websocket")
    };

    let mut upstream = match TcpStream::connect(("localhost", port)).await {
        Ok(s) => s,
        Err(_) => {
            send_request_log(event_tx, &method, &path, 502, start.elapsed());
            return ProxyResult {
                response: crate::error_page::error_response(502, "upstream is down", ".sh"),
                ws_relay: None,
            };
        }
    };

    let request_bytes = if is_ws_upgrade {
        rewrite_host_header(raw_request, port)
    } else {
        let rewritten = rewrite_host_header(raw_request, port);
        inject_connection_close(&rewritten)
    };

    if upstream.write_all(&request_bytes).await.is_err() {
        send_request_log(event_tx, &method, &path, 502, start.elapsed());
        return ProxyResult {
            response: crate::error_page::error_response(502, "upstream is down", ".sh"),
            ws_relay: None,
        };
    }

    let mut response = Vec::with_capacity(4096);
    let mut buf = [0u8; 8192];

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match tokio::time::timeout_at(deadline, upstream.read(&mut buf)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                response.extend_from_slice(&buf[..n]);
                if is_ws_upgrade
                    && response.starts_with(b"HTTP/1.1 101")
                    && response.windows(4).any(|w| w == b"\r\n\r\n")
                {
                    if cors {
                        response = inject_cors_headers(&response);
                    }
                    let status = parse_response_status(&response);
                    send_request_log(event_tx, &method, &path, status, start.elapsed());
                    return ProxyResult {
                        response,
                        ws_relay: Some(WsRelay {
                            stream_id,
                            upstream,
                        }),
                    };
                }
            }
            _ => break,
        }
        if response.len() > 10 * 1024 * 1024 {
            break;
        }
    }

    if cors {
        response = inject_cors_headers(&response);
    }
    let status = parse_response_status(&response);
    send_request_log(event_tx, &method, &path, status, start.elapsed());
    ProxyResult {
        response,
        ws_relay: None,
    }
}

fn parse_response_status(raw_response: &[u8]) -> u16 {
    let resp_str = String::from_utf8_lossy(raw_response);
    resp_str
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn send_request_log(
    tx: &std::sync::mpsc::Sender<AppEvent>,
    method: &str,
    path: &str,
    status: u16,
    duration: std::time::Duration,
) {
    let log = RequestLog {
        id: REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed),
        timestamp: time::OffsetDateTime::now_utc(),
        method: method.to_string(),
        path: path.to_string(),
        status,
        duration_ms: duration.as_millis() as u64,
        request_headers: vec![],
        response_headers: vec![],
        body_preview: None,
        body_size: 0,
    };
    let _ = tx.send(AppEvent::Request(log));
}

const CORS_HEADERS: &str = "\
Access-Control-Allow-Origin: *\r\n\
Access-Control-Allow-Methods: GET, POST, PUT, PATCH, DELETE, OPTIONS, HEAD\r\n\
Access-Control-Allow-Headers: Accept, Authorization, Content-Type, X-Requested-With\r\n\
Access-Control-Allow-Credentials: true\r\n\
Access-Control-Max-Age: 86400\r\n";

const CORS_HEADER_PREFIXES: &[&str] = &[
    "access-control-allow-origin:",
    "access-control-allow-methods:",
    "access-control-allow-headers:",
    "access-control-allow-credentials:",
    "access-control-max-age:",
    "access-control-expose-headers:",
];

fn is_cors_preflight(raw_request: &[u8]) -> bool {
    let s = String::from_utf8_lossy(raw_request);
    let method = s.split_whitespace().next().unwrap_or("");
    if !method.eq_ignore_ascii_case("OPTIONS") {
        return false;
    }
    let lower = s.to_ascii_lowercase();
    lower.contains("origin:")
}

fn build_cors_preflight_response() -> Vec<u8> {
    format!(
        "HTTP/1.1 204 No Content\r\n\
         {CORS_HEADERS}\
         Content-Length: 0\r\n\
         \r\n"
    )
    .into_bytes()
}

fn inject_cors_headers(raw_response: &[u8]) -> Vec<u8> {
    let header_end = match raw_response.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(pos) => pos,
        None => return raw_response.to_vec(),
    };

    let header_bytes = &raw_response[..header_end];
    let body_bytes = &raw_response[header_end + 4..];
    let header_str = String::from_utf8_lossy(header_bytes);

    let first_crlf = match header_str.find("\r\n") {
        Some(pos) => pos,
        None => return raw_response.to_vec(),
    };

    let mut patched = Vec::with_capacity(raw_response.len() + 512);
    patched.extend_from_slice(&header_bytes[..first_crlf + 2]);

    for line in header_str[first_crlf + 2..].split("\r\n") {
        if line.is_empty() {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if CORS_HEADER_PREFIXES.iter().any(|p| lower.starts_with(p)) {
            continue;
        }
        patched.extend_from_slice(line.as_bytes());
        patched.extend_from_slice(b"\r\n");
    }

    patched.extend_from_slice(CORS_HEADERS.as_bytes());
    patched.extend_from_slice(b"\r\n");
    patched.extend_from_slice(body_bytes);
    patched
}

fn rewrite_host_header(raw: &[u8], port: u16) -> Vec<u8> {
    let header_end = match raw.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(pos) => pos,
        None => return raw.to_vec(),
    };

    let header_bytes = &raw[..header_end];
    let body_bytes = &raw[header_end + 4..];
    let header_str = String::from_utf8_lossy(header_bytes);

    let first_crlf = match header_str.find("\r\n") {
        Some(pos) => pos,
        None => return raw.to_vec(),
    };

    let mut original_host = None;
    let mut patched = Vec::with_capacity(raw.len() + 64);
    patched.extend_from_slice(&header_bytes[..first_crlf + 2]);

    for line in header_str[first_crlf + 2..].split("\r\n") {
        if line.is_empty() {
            continue;
        }
        if line.to_ascii_lowercase().starts_with("host:") {
            if let Some((_, val)) = line.split_once(':') {
                original_host = Some(val.trim().to_string());
            }
            patched.extend_from_slice(format!("Host: localhost:{port}\r\n").as_bytes());
        } else {
            patched.extend_from_slice(line.as_bytes());
            patched.extend_from_slice(b"\r\n");
        }
    }

    if let Some(orig) = original_host {
        patched.extend_from_slice(format!("X-Forwarded-Host: {orig}\r\n").as_bytes());
    }

    patched.extend_from_slice(b"\r\n");
    patched.extend_from_slice(body_bytes);
    patched
}

fn inject_connection_close(raw: &[u8]) -> Vec<u8> {
    let header_end = raw.windows(4).position(|w| w == b"\r\n\r\n");
    let (header_bytes, body_bytes) = match header_end {
        Some(pos) => (&raw[..pos], &raw[pos + 4..]),
        None => return raw.to_vec(),
    };

    let header_str = String::from_utf8_lossy(header_bytes);
    let first_crlf = match header_str.find("\r\n") {
        Some(pos) => pos,
        None => return raw.to_vec(),
    };

    let mut patched = Vec::with_capacity(raw.len() + 20);
    patched.extend_from_slice(&header_bytes[..first_crlf + 2]);
    patched.extend_from_slice(b"Connection: close\r\n");

    for line in header_str[first_crlf + 2..].split("\r\n") {
        if !line.is_empty() && !line.to_ascii_lowercase().starts_with("connection:") {
            patched.extend_from_slice(line.as_bytes());
            patched.extend_from_slice(b"\r\n");
        }
    }
    patched.extend_from_slice(b"\r\n");
    patched.extend_from_slice(body_bytes);
    patched
}

#[allow(dead_code, clippy::type_complexity)]
fn parse_raw_request(raw: &[u8]) -> Option<(String, String, Vec<(String, String)>, Vec<u8>)> {
    let header_end = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let header_bytes = &raw[..header_end];
    let body = raw[header_end + 4..].to_vec();

    let header_str = String::from_utf8_lossy(header_bytes);
    let mut lines = header_str.split("\r\n");

    let first_line = lines.next()?;
    let parts: Vec<&str> = first_line.splitn(3, ' ').collect();
    if parts.len() < 2 {
        return None;
    }
    let method = parts[0].to_string();
    let path = parts[1].to_string();

    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }

    Some((method, path, headers, body))
}

async fn get_token() -> String {
    match crate::auth::get_token().await {
        Ok(token) => token,
        Err(e) => {
            eprintln!(
                "  {} Not logged in: {e}\n  Run: xpo login",
                console::style("!").red().bold()
            );
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_connection_close_preserves_binary_body() {
        let body: &[u8] = &[0x00, 0x01, 0xFF, 0xFE, 0x80, 0x7F, 0xDE, 0xAD];
        let mut raw =
            b"POST /api HTTP/1.1\r\nHost: example.com\r\nContent-Length: 8\r\n\r\n".to_vec();
        raw.extend_from_slice(body);

        let patched = inject_connection_close(&raw);

        let header_end = patched.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let actual_body = &patched[header_end + 4..];
        assert_eq!(actual_body, body, "binary body must be preserved exactly");
    }

    #[test]
    fn inject_connection_close_adds_header() {
        let raw = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let patched = inject_connection_close(raw);
        let header_end = patched.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let header_str = String::from_utf8_lossy(&patched[..header_end]);
        assert!(
            header_str.contains("Connection: close"),
            "Connection: close header must be present"
        );
    }

    #[test]
    fn inject_connection_close_removes_existing_connection_header() {
        let raw = b"GET / HTTP/1.1\r\nHost: example.com\r\nConnection: keep-alive\r\n\r\n";
        let patched = inject_connection_close(raw);
        let header_end = patched.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let header_str = String::from_utf8_lossy(&patched[..header_end]).to_ascii_lowercase();

        let count = header_str.matches("connection:").count();
        assert_eq!(count, 1, "exactly one Connection header must remain");
        assert!(
            header_str.contains("connection: close"),
            "the Connection header must be 'close'"
        );
    }

    #[test]
    fn inject_connection_close_preserves_json_body() {
        let body = br#"{"key":"value"}"#;
        let mut raw = format!(
            "POST /data HTTP/1.1\r\nHost: api.test\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            body.len()
        ).into_bytes();
        raw.extend_from_slice(body);

        let patched = inject_connection_close(&raw);

        let header_end = patched.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let actual_body = &patched[header_end + 4..];
        assert_eq!(actual_body, body);

        let header_str = String::from_utf8_lossy(&patched[..header_end]);
        assert!(header_str.contains("Connection: close"));
        assert!(header_str.contains("Content-Type: application/json"));
        assert!(header_str.contains(&format!("Content-Length: {}", body.len())));
    }

    #[test]
    fn inject_connection_close_no_crlfcrlf_returns_unchanged() {
        let raw = b"GET / HTTP/1.1\r\nHost: example.com\r\n";
        let patched = inject_connection_close(raw);
        assert_eq!(
            patched, raw,
            "input without header terminator must be returned as-is"
        );
    }

    #[test]
    fn inject_connection_close_empty_body() {
        let raw = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let patched = inject_connection_close(raw);
        let header_end = patched.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let actual_body = &patched[header_end + 4..];
        assert!(actual_body.is_empty(), "empty body must stay empty");
    }

    #[test]
    fn rewrite_host_header_replaces_host() {
        let raw = b"GET / HTTP/1.1\r\nHost: myapp.xpo.sh\r\nAccept: */*\r\n\r\n";
        let patched = rewrite_host_header(raw, 5173);
        let s = String::from_utf8_lossy(&patched);
        assert!(s.contains("Host: localhost:5173"), "Host must be rewritten");
        assert!(
            s.contains("X-Forwarded-Host: myapp.xpo.sh"),
            "original host must be in X-Forwarded-Host"
        );
        assert!(s.contains("Accept: */*"), "other headers preserved");
    }

    #[test]
    fn rewrite_host_header_preserves_body() {
        let body = b"hello world";
        let mut raw =
            b"POST /api HTTP/1.1\r\nHost: test.xpo.sh\r\nContent-Length: 11\r\n\r\n".to_vec();
        raw.extend_from_slice(body);
        let patched = rewrite_host_header(&raw, 3000);
        let header_end = patched.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let actual_body = &patched[header_end + 4..];
        assert_eq!(actual_body, body);
    }

    #[test]
    fn rewrite_host_header_no_host() {
        let raw = b"GET / HTTP/1.1\r\nAccept: */*\r\n\r\n";
        let patched = rewrite_host_header(raw, 3000);
        let s = String::from_utf8_lossy(&patched);
        assert!(!s.contains("X-Forwarded-Host"));
        assert!(s.contains("Accept: */*"));
    }

    #[test]
    fn qr_code_generates_for_valid_url() {
        use fast_qr::qr::QRBuilder;
        let qr = QRBuilder::new("https://myapp.xpo.sh").build();
        assert!(qr.is_ok());
        let qr_str = qr.unwrap().to_str();
        assert!(!qr_str.is_empty());
        assert!(qr_str.contains('\u{2588}'));
    }

    #[test]
    fn is_cors_preflight_detects_options_with_origin() {
        let req = b"OPTIONS /api HTTP/1.1\r\nHost: example.com\r\nOrigin: https://foo.com\r\n\r\n";
        assert!(is_cors_preflight(req));
    }

    #[test]
    fn is_cors_preflight_rejects_options_without_origin() {
        let req = b"OPTIONS /api HTTP/1.1\r\nHost: example.com\r\n\r\n";
        assert!(!is_cors_preflight(req));
    }

    #[test]
    fn is_cors_preflight_rejects_get_with_origin() {
        let req = b"GET /api HTTP/1.1\r\nHost: example.com\r\nOrigin: https://foo.com\r\n\r\n";
        assert!(!is_cors_preflight(req));
    }

    #[test]
    fn build_cors_preflight_response_returns_204() {
        let resp = build_cors_preflight_response();
        let s = String::from_utf8(resp).unwrap();
        assert!(s.starts_with("HTTP/1.1 204 No Content\r\n"));
        assert!(s.contains("Access-Control-Allow-Origin: *"));
        assert!(s.contains("Access-Control-Allow-Methods:"));
        assert!(s.contains("Access-Control-Allow-Headers:"));
        assert!(s.contains("Access-Control-Allow-Credentials: true"));
        assert!(s.contains("Access-Control-Max-Age: 86400"));
        assert!(s.contains("Content-Length: 0"));
        assert!(s.ends_with("\r\n\r\n"));
    }

    #[test]
    fn inject_cors_headers_adds_all_headers() {
        let resp = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}";
        let patched = inject_cors_headers(resp);
        let s = String::from_utf8_lossy(&patched);
        assert!(s.contains("Access-Control-Allow-Origin: *"));
        assert!(s.contains("Access-Control-Allow-Methods:"));
        assert!(s.contains("Access-Control-Allow-Headers:"));
        assert!(s.contains("Access-Control-Allow-Credentials: true"));
        assert!(s.contains("Access-Control-Max-Age: 86400"));
        assert!(s.contains("Content-Type: application/json"));
        assert!(s.contains("{\"ok\":true}"));
    }

    #[test]
    fn inject_cors_headers_strips_existing_cors() {
        let resp = b"HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: https://old.com\r\nAccess-Control-Allow-Methods: GET\r\nContent-Type: text/html\r\n\r\nhi";
        let patched = inject_cors_headers(resp);
        let s = String::from_utf8_lossy(&patched);
        assert!(!s.contains("https://old.com"));
        let origin_count = s.matches("Access-Control-Allow-Origin:").count();
        assert_eq!(origin_count, 1);
        assert!(s.contains("Access-Control-Allow-Origin: *"));
        assert!(s.contains("Content-Type: text/html"));
        let header_end = patched.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let body = &patched[header_end + 4..];
        assert_eq!(body, b"hi");
    }

    #[test]
    fn inject_cors_headers_preserves_binary_body() {
        let body: &[u8] = &[0x00, 0x01, 0xFF, 0xFE];
        let mut resp =
            b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\r\n".to_vec();
        resp.extend_from_slice(body);
        let patched = inject_cors_headers(&resp);
        let header_end = patched.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let actual_body = &patched[header_end + 4..];
        assert_eq!(actual_body, body);
    }

    #[test]
    fn inject_cors_headers_no_header_terminator() {
        let resp = b"HTTP/1.1 200 OK\r\n";
        let patched = inject_cors_headers(resp);
        assert_eq!(patched, resp.to_vec());
    }

    #[test]
    fn parse_raw_request_basic_get() {
        let raw = b"GET /hello HTTP/1.1\r\nHost: myapp.xpo.sh\r\nAccept: */*\r\n\r\n";
        let (method, path, headers, body) = parse_raw_request(raw).unwrap();
        assert_eq!(method, "GET");
        assert_eq!(path, "/hello");
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0], ("Host".to_string(), "myapp.xpo.sh".to_string()));
        assert!(body.is_empty());
    }

    #[test]
    fn parse_raw_request_post_with_body() {
        let raw = b"POST /api HTTP/1.1\r\nHost: test.xpo.sh\r\nContent-Length: 13\r\n\r\n{\"key\":\"val\"}";
        let (method, path, headers, body) = parse_raw_request(raw).unwrap();
        assert_eq!(method, "POST");
        assert_eq!(path, "/api");
        assert_eq!(body, b"{\"key\":\"val\"}");
    }

    #[test]
    fn parse_raw_request_with_query() {
        let raw = b"GET /search?q=test&page=1 HTTP/1.1\r\nHost: x.xpo.sh\r\n\r\n";
        let (_, path, _, _) = parse_raw_request(raw).unwrap();
        assert_eq!(path, "/search?q=test&page=1");
    }

    #[test]
    fn parse_raw_request_no_header_end() {
        let raw = b"GET / HTTP/1.1\r\nHost: x";
        assert!(parse_raw_request(raw).is_none());
    }

    #[test]
    fn parse_raw_request_binary_body() {
        let mut raw = b"POST /upload HTTP/1.1\r\nHost: x.xpo.sh\r\nContent-Type: application/octet-stream\r\n\r\n".to_vec();
        let binary: &[u8] = &[0x00, 0x01, 0xFF, 0xFE, 0x80];
        raw.extend_from_slice(binary);
        let (_, _, _, body) = parse_raw_request(&raw).unwrap();
        assert_eq!(body, binary);
    }
}
