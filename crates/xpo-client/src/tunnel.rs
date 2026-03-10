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

#[allow(clippy::too_many_arguments)]
pub async fn run(
    port: u16,
    subdomain: Option<String>,
    server: &str,
    max_logs: usize,
    visible_rows: usize,
    cors: bool,
    password: Option<String>,
    ttl_secs: Option<u64>,
) -> Result<(), Box<dyn std::error::Error>> {
    let http_client = build_http_client();
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
            &http_client,
            &app_tx,
            &quit_flag,
            &quit_notify,
            use_tui,
            first_connect,
            &tui_state,
            max_logs,
            visible_rows,
            password.clone(),
            ttl_secs,
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
    client: &reqwest::Client,
    app_tx: &std::sync::mpsc::Sender<AppEvent>,
    quit_flag: &Arc<AtomicBool>,
    quit_notify: &Arc<tokio::sync::Notify>,
    use_tui: bool,
    first_connect: bool,
    tui_state: &Arc<std::sync::Mutex<TuiThreadState>>,
    max_logs: usize,
    visible_rows: usize,
    password: Option<String>,
    ttl_secs: Option<u64>,
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
                let hello = ClientControl::Hello {
                    port,
                    subdomain,
                    password: password.clone(),
                    ttl_secs,
                };
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

    if let Some(ttl) = ttl_secs {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(ttl);
        let _ = app_tx.send(AppEvent::TtlDeadline(deadline));
    }

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
                                        let client_clone = client.clone();
                                        tokio::spawn(async move {
                                            let result = proxy_to_upstream(&client_clone, port, &payload, stream_id, &event_tx, cors).await;
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
    client: &reqwest::Client,
    port: u16,
    raw_request: &[u8],
    stream_id: xpo_core::StreamId,
    event_tx: &std::sync::mpsc::Sender<AppEvent>,
    cors: bool,
) -> ProxyResult {
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

    if is_ws_upgrade {
        return proxy_ws_upgrade(port, raw_request, stream_id, event_tx, cors, start).await;
    }

    let response = proxy_http_reqwest(client, port, raw_request, cors).await;
    let status = parse_response_status(&response);
    send_request_log(event_tx, &method, &path, status, start.elapsed());

    ProxyResult {
        response,
        ws_relay: None,
    }
}

async fn proxy_ws_upgrade(
    port: u16,
    raw_request: &[u8],
    stream_id: xpo_core::StreamId,
    event_tx: &std::sync::mpsc::Sender<AppEvent>,
    cors: bool,
    start: std::time::Instant,
) -> ProxyResult {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let req_str = String::from_utf8_lossy(raw_request);
    let parts: Vec<&str> = req_str.split_whitespace().collect();
    let method = parts.first().copied().unwrap_or("???").to_string();
    let path = parts.get(1).copied().unwrap_or("/").to_string();

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

    let request_bytes = rewrite_host_header(raw_request, port);

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
                if response.starts_with(b"HTTP/1.1 101")
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

#[allow(clippy::type_complexity)]
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

fn serialize_response(
    status: u16,
    reason: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Vec<u8> {
    let mut raw = format!("HTTP/1.1 {status} {reason}\r\n");
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("transfer-encoding") {
            continue;
        }
        if name.eq_ignore_ascii_case("content-length") {
            continue;
        }
        raw.push_str(name);
        raw.push_str(": ");
        raw.push_str(value);
        raw.push_str("\r\n");
    }
    raw.push_str(&format!("Content-Length: {}\r\n", body.len()));
    raw.push_str("\r\n");

    let mut bytes = raw.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

const MAX_BODY_SIZE: usize = 10 * 1024 * 1024;

fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(10))
        .pool_idle_timeout(std::time::Duration::from_secs(300))
        .pool_max_idle_per_host(32)
        .http1_only()
        .no_proxy()
        .build()
        .expect("failed to build HTTP client")
}

async fn proxy_http_reqwest(
    client: &reqwest::Client,
    port: u16,
    raw_request: &[u8],
    cors: bool,
) -> Vec<u8> {
    let (method, path, headers, body) = match parse_raw_request(raw_request) {
        Some(parsed) => parsed,
        None => return crate::error_page::error_response(502, "malformed request", ".sh"),
    };

    let url = format!("http://localhost:{port}{path}");

    let reqwest_method = match reqwest::Method::from_bytes(method.as_bytes()) {
        Ok(m) => m,
        Err(_) => return crate::error_page::error_response(400, "invalid method", ".sh"),
    };

    let mut req_builder = client.request(reqwest_method, &url);

    let mut original_host = None;
    for (name, value) in &headers {
        if name.eq_ignore_ascii_case("host") {
            original_host = Some(value.clone());
            continue;
        }
        if name.eq_ignore_ascii_case("connection") {
            continue;
        }
        if let Ok(header_name) = reqwest::header::HeaderName::from_bytes(name.as_bytes()) {
            if let Ok(header_value) = reqwest::header::HeaderValue::from_str(value) {
                req_builder = req_builder.header(header_name, header_value);
            }
        }
    }

    if let Some(ref orig) = original_host {
        req_builder = req_builder.header("X-Forwarded-Host", orig.as_str());
    }

    if !body.is_empty() {
        req_builder = req_builder.body(body);
    }

    let response = match req_builder.send().await {
        Ok(resp) => resp,
        Err(e) => {
            if e.is_timeout() {
                return crate::error_page::error_response(504, "upstream timeout", ".sh");
            }
            return crate::error_page::error_response(502, "upstream is down", ".sh");
        }
    };

    let status = response.status().as_u16();
    let reason = response
        .status()
        .canonical_reason()
        .unwrap_or("OK")
        .to_string();

    if let Some(cl) = response.content_length() {
        if cl > MAX_BODY_SIZE as u64 {
            return crate::error_page::error_response(502, "response too large", ".sh");
        }
    }

    let resp_headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|val| (k.as_str().to_string(), val.to_string()))
        })
        .collect();

    let body_bytes = match response.bytes().await {
        Ok(b) => {
            if b.len() > MAX_BODY_SIZE {
                return crate::error_page::error_response(502, "response too large", ".sh");
            }
            b
        }
        Err(_) => {
            return crate::error_page::error_response(502, "failed to read response", ".sh");
        }
    };

    let mut result = serialize_response(status, &reason, &resp_headers, &body_bytes);

    if cors {
        result = inject_cors_headers(&result);
    }

    result
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
        let (method, path, _headers, body) = parse_raw_request(raw).unwrap();
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

    #[test]
    fn serialize_response_basic() {
        let headers = vec![("Content-Type".to_string(), "text/plain".to_string())];
        let body = b"hello";
        let raw = serialize_response(200, "OK", &headers, body);
        let s = String::from_utf8_lossy(&raw);
        assert!(s.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(s.contains("Content-Type: text/plain\r\n"));
        assert!(s.contains("Content-Length: 5\r\n"));
        assert!(s.contains("\r\n\r\nhello"));
    }

    #[test]
    fn serialize_response_strips_transfer_encoding() {
        let headers = vec![
            ("Transfer-Encoding".to_string(), "chunked".to_string()),
            ("Content-Encoding".to_string(), "gzip".to_string()),
        ];
        let raw = serialize_response(200, "OK", &headers, b"data");
        let s = String::from_utf8_lossy(&raw);
        assert!(!s.contains("Transfer-Encoding"));
        assert!(s.contains("Content-Encoding: gzip"));
        assert!(s.contains("Content-Length: 4"));
    }

    #[test]
    fn serialize_response_strips_content_length() {
        let headers = vec![("Content-Length".to_string(), "999".to_string())];
        let raw = serialize_response(200, "OK", &headers, b"hi");
        let s = String::from_utf8_lossy(&raw);
        assert!(s.contains("Content-Length: 2\r\n"));
        let count = s.matches("Content-Length").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn serialize_response_empty_body() {
        let raw = serialize_response(204, "No Content", &[], b"");
        let s = String::from_utf8_lossy(&raw);
        assert!(s.starts_with("HTTP/1.1 204 No Content\r\n"));
        assert!(s.contains("Content-Length: 0\r\n"));
    }

    #[test]
    fn serialize_response_preserves_binary_body() {
        let body: &[u8] = &[0x00, 0x1F, 0x8B, 0x08, 0xFF];
        let raw = serialize_response(200, "OK", &[], body);
        let header_end = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let actual_body = &raw[header_end + 4..];
        assert_eq!(actual_body, body);
    }

    #[tokio::test]
    async fn proxy_http_reqwest_basic_200() {
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = upstream.local_addr().unwrap().port();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut stream, _) = upstream.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = stream.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);
            assert!(
                req.contains("x-forwarded-host: myapp.xpo.sh")
                    || req.contains("X-Forwarded-Host: myapp.xpo.sh")
            );
            let resp = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
            stream.write_all(resp.as_bytes()).await.unwrap();
        });
        let client = build_http_client();
        let raw_request = b"GET /hello HTTP/1.1\r\nHost: myapp.xpo.sh\r\nAccept: */*\r\n\r\n";
        let response = proxy_http_reqwest(&client, port, raw_request, false).await;
        let s = String::from_utf8_lossy(&response);
        assert!(s.contains("200 OK") || s.contains("200 ok"), "status: {s}");
        assert!(s.contains("ok"));
    }

    #[tokio::test]
    async fn proxy_http_reqwest_chunked_response() {
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = upstream.local_addr().unwrap().port();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut stream, _) = upstream.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let resp = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
            stream.write_all(resp.as_bytes()).await.unwrap();
        });
        let client = build_http_client();
        let raw_request = b"GET / HTTP/1.1\r\nHost: test.xpo.sh\r\n\r\n";
        let response = proxy_http_reqwest(&client, port, raw_request, false).await;
        let s = String::from_utf8_lossy(&response);
        assert!(s.contains("200"));
        assert!(!s.to_ascii_lowercase().contains("transfer-encoding"));
        let header_end = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let body = &response[header_end + 4..];
        assert_eq!(body, b"hello world", "chunked body must be decoded");
    }

    #[tokio::test]
    async fn proxy_http_reqwest_gzip_preserved() {
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = upstream.local_addr().unwrap().port();
        let gzip_body: Vec<u8> = {
            use std::io::Write;
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            encoder.write_all(b"hello gzip world").unwrap();
            encoder.finish().unwrap()
        };
        let gzip_clone = gzip_body.clone();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut stream, _) = upstream.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                gzip_clone.len()
            );
            stream.write_all(headers.as_bytes()).await.unwrap();
            stream.write_all(&gzip_clone).await.unwrap();
        });
        let client = build_http_client();
        let raw_request = b"GET / HTTP/1.1\r\nHost: test.xpo.sh\r\nAccept-Encoding: gzip\r\n\r\n";
        let response = proxy_http_reqwest(&client, port, raw_request, false).await;
        let s = String::from_utf8_lossy(&response);
        assert!(
            s.to_ascii_lowercase().contains("content-encoding: gzip")
                || s.to_ascii_lowercase().contains("content-encoding:gzip"),
            "gzip header preserved"
        );
        let header_end = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let body = &response[header_end + 4..];
        assert_eq!(body, &gzip_body, "gzip body must pass through unmodified");
    }

    #[tokio::test]
    async fn proxy_http_reqwest_connection_refused() {
        let client = build_http_client();
        let raw_request = b"GET / HTTP/1.1\r\nHost: test.xpo.sh\r\n\r\n";
        let response = proxy_http_reqwest(&client, 19999, raw_request, false).await;
        let s = String::from_utf8_lossy(&response);
        assert!(s.contains("502"));
    }

    #[tokio::test]
    async fn proxy_http_reqwest_post_body_forwarded() {
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = upstream.local_addr().unwrap().port();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut stream, _) = upstream.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = stream.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);
            assert!(
                req.contains("{\"name\":\"test\"}"),
                "POST body must be forwarded"
            );
            let resp = "HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            stream.write_all(resp.as_bytes()).await.unwrap();
        });
        let client = build_http_client();
        let raw_request = b"POST /api HTTP/1.1\r\nHost: test.xpo.sh\r\nContent-Type: application/json\r\nContent-Length: 15\r\n\r\n{\"name\":\"test\"}";
        let response = proxy_http_reqwest(&client, port, raw_request, false).await;
        let s = String::from_utf8_lossy(&response);
        assert!(s.contains("201"));
    }

    #[tokio::test]
    async fn proxy_http_reqwest_redirect_not_followed() {
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = upstream.local_addr().unwrap().port();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut stream, _) = upstream.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let resp = "HTTP/1.1 302 Found\r\nLocation: /other\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            stream.write_all(resp.as_bytes()).await.unwrap();
        });
        let client = build_http_client();
        let raw_request = b"GET / HTTP/1.1\r\nHost: test.xpo.sh\r\n\r\n";
        let response = proxy_http_reqwest(&client, port, raw_request, false).await;
        let s = String::from_utf8_lossy(&response);
        assert!(s.contains("302"), "redirect must NOT be followed");
    }

    #[tokio::test]
    async fn proxy_http_reqwest_binary_response() {
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = upstream.local_addr().unwrap().port();
        let binary_body: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0xFF];
        let bin_clone = binary_body.clone();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut stream, _) = upstream.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bin_clone.len()
            );
            stream.write_all(headers.as_bytes()).await.unwrap();
            stream.write_all(&bin_clone).await.unwrap();
        });
        let client = build_http_client();
        let raw_request = b"GET /img.png HTTP/1.1\r\nHost: test.xpo.sh\r\n\r\n";
        let response = proxy_http_reqwest(&client, port, raw_request, false).await;
        let header_end = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let body = &response[header_end + 4..];
        assert_eq!(
            body, &binary_body,
            "binary body must be byte-for-byte identical"
        );
    }

    #[tokio::test]
    async fn proxy_http_reqwest_strict_host_check() {
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = upstream.local_addr().unwrap().port();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut stream, _) = upstream.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = stream.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);
            let host_line = req
                .lines()
                .find(|l| l.to_ascii_lowercase().starts_with("host:"))
                .unwrap_or("");
            let host_val = host_line
                .split_once(':')
                .map(|(_, v)| v.trim())
                .unwrap_or("");
            let resp = if host_val.starts_with("localhost") {
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
            } else {
                "HTTP/1.1 403 Forbidden\r\nContent-Length: 20\r\nConnection: close\r\n\r\nInvalid Host header"
            };
            stream.write_all(resp.as_bytes()).await.unwrap();
        });
        let client = build_http_client();
        let raw_request = b"GET / HTTP/1.1\r\nHost: fi5f4h.xpo.sh\r\nAccept: */*\r\n\r\n";
        let response = proxy_http_reqwest(&client, port, raw_request, false).await;
        let s = String::from_utf8_lossy(&response);
        assert!(
            s.contains("200"),
            "strict host check must pass because Host is rewritten to localhost: {s}"
        );
    }

    #[tokio::test]
    async fn proxy_http_reqwest_chunked_plus_gzip() {
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = upstream.local_addr().unwrap().port();
        let gzip_body: Vec<u8> = {
            use std::io::Write;
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            encoder.write_all(b"chunked gzip test").unwrap();
            encoder.finish().unwrap()
        };
        let gz_clone = gzip_body.clone();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut stream, _) = upstream.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let chunk_hex = format!("{:x}", gz_clone.len());
            let mut resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nTransfer-Encoding: chunked\r\n\r\n{chunk_hex}\r\n"
            )
            .into_bytes();
            resp.extend_from_slice(&gz_clone);
            resp.extend_from_slice(b"\r\n0\r\n\r\n");
            stream.write_all(&resp).await.unwrap();
        });
        let client = build_http_client();
        let raw_request = b"GET / HTTP/1.1\r\nHost: test.xpo.sh\r\nAccept-Encoding: gzip\r\n\r\n";
        let response = proxy_http_reqwest(&client, port, raw_request, false).await;
        let header_end = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let body = &response[header_end + 4..];
        assert_eq!(
            body, &gzip_body,
            "chunked+gzip: body must be decoded chunks but still gzip"
        );
        let s = String::from_utf8_lossy(&response);
        assert!(!s.to_ascii_lowercase().contains("transfer-encoding"));
    }

    #[tokio::test]
    async fn proxy_http_reqwest_empty_body_204() {
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = upstream.local_addr().unwrap().port();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut stream, _) = upstream.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let resp = "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n";
            stream.write_all(resp.as_bytes()).await.unwrap();
        });
        let client = build_http_client();
        let raw_request = b"DELETE /item/1 HTTP/1.1\r\nHost: test.xpo.sh\r\n\r\n";
        let response = proxy_http_reqwest(&client, port, raw_request, false).await;
        let s = String::from_utf8_lossy(&response);
        assert!(s.contains("204"));
    }

    #[tokio::test]
    async fn proxy_http_reqwest_4xx_passthrough() {
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = upstream.local_addr().unwrap().port();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut stream, _) = upstream.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let resp = "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found";
            stream.write_all(resp.as_bytes()).await.unwrap();
        });
        let client = build_http_client();
        let raw_request = b"GET /missing HTTP/1.1\r\nHost: test.xpo.sh\r\n\r\n";
        let response = proxy_http_reqwest(&client, port, raw_request, false).await;
        let s = String::from_utf8_lossy(&response);
        assert!(s.contains("404"));
        assert!(s.contains("not found"));
    }

    #[tokio::test]
    async fn proxy_http_reqwest_custom_headers_preserved() {
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = upstream.local_addr().unwrap().port();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut stream, _) = upstream.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = stream.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);
            assert!(req
                .to_ascii_lowercase()
                .contains("authorization: bearer token123"));
            let resp = "HTTP/1.1 200 OK\r\nSet-Cookie: sid=abc; Path=/\r\nX-Response-Id: r42\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
            stream.write_all(resp.as_bytes()).await.unwrap();
        });
        let client = build_http_client();
        let raw_request = b"GET /api HTTP/1.1\r\nHost: test.xpo.sh\r\nAuthorization: Bearer token123\r\nX-Custom: myvalue\r\n\r\n";
        let response = proxy_http_reqwest(&client, port, raw_request, false).await;
        let s = String::from_utf8_lossy(&response).to_ascii_lowercase();
        assert!(s.contains("set-cookie"), "Set-Cookie must be preserved");
        assert!(s.contains("sid=abc"), "cookie value must be preserved");
    }

    #[tokio::test]
    async fn proxy_http_reqwest_host_with_port() {
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = upstream.local_addr().unwrap().port();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut stream, _) = upstream.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = stream.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);
            let host_line = req
                .lines()
                .find(|l| l.to_ascii_lowercase().starts_with("host:"))
                .unwrap_or("");
            assert!(
                host_line.contains("localhost"),
                "Host must be rewritten: {host_line}"
            );
            assert!(req
                .to_ascii_lowercase()
                .contains("x-forwarded-host: myapp.xpo.sh:443"));
            let resp = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
            stream.write_all(resp.as_bytes()).await.unwrap();
        });
        let client = build_http_client();
        let raw_request = b"GET / HTTP/1.1\r\nHost: myapp.xpo.sh:443\r\n\r\n";
        let response = proxy_http_reqwest(&client, port, raw_request, false).await;
        let s = String::from_utf8_lossy(&response);
        assert!(s.contains("200"));
    }
}
