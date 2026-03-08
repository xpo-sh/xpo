use console::style;
use futures_util::{SinkExt, StreamExt};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use xpo_core::auth::create_test_token;
use xpo_core::protocol::{ClientControl, Packet, PacketType, ServerControl};
use xpo_core::{HEARTBEAT_TIMEOUT_SECS, RECONNECT_MAX_SECS, RECONNECT_MIN_SECS};

struct LogState {
    entries: VecDeque<String>,
    displayed: usize,
    max: usize,
}

impl LogState {
    fn new(max: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            displayed: 0,
            max,
        }
    }
}

pub async fn run(
    port: u16,
    subdomain: Option<String>,
    server: &str,
    max_logs: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut backoff = RECONNECT_MIN_SECS;

    loop {
        match connect_and_run(port, subdomain.clone(), server, max_logs).await {
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
                eprintln!(
                    "  {} {}  {}",
                    style("!").yellow().bold(),
                    short,
                    style(format!("(retry in {backoff}s)")).dim()
                );
                tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(RECONNECT_MAX_SECS);
            }
        }
    }

    Ok(())
}

async fn connect_and_run(
    port: u16,
    subdomain: Option<String>,
    server: &str,
    max_logs: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let ws_url = format!("ws://{server}");
    let (ws_stream, _) = connect_async(&ws_url).await?;
    let (mut ws_write, mut ws_read) = ws_stream.split();

    let token = get_token();
    let auth = ClientControl::Auth { token };
    ws_write.send(Message::Text(auth.to_json()?)).await?;

    let auth_resp = ws_read.next().await.ok_or("no auth response")??;

    let (user, tunnel_url) = match auth_resp {
        Message::Text(text) => match ServerControl::from_json(&text)? {
            ServerControl::AuthOk { user, .. } => {
                let hello = ClientControl::Hello { port, subdomain };
                ws_write.send(Message::Text(hello.to_json()?)).await?;

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

    print_banner(&tunnel_url, port, &user);

    let upstream_port = port;
    let (resp_tx, mut resp_rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let ws_relays: Arc<
        dashmap::DashMap<xpo_core::StreamId, tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
    > = Arc::new(dashmap::DashMap::new());
    let log_state = Arc::new(std::sync::Mutex::new(LogState::new(max_logs)));

    loop {
        tokio::select! {
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
                                        let ls = log_state.clone();
                                        tokio::spawn(async move {
                                            let result = proxy_to_upstream(upstream_port, &payload, stream_id, &ls).await;
                                            let resp_pkt = Packet::data(stream_id, result.response);
                                            let _ = tx.send(Message::Binary(resp_pkt.encode()));
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
                                                                    if tx3.send(Message::Binary(pkt.encode())).is_err() {
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
                                                    let _ = tx2.send(Message::Binary(end_pkt.encode()));
                                                    relays.remove(&sid);
                                                });
                                            } else {
                                                let end_pkt = Packet::end(stream_id);
                                                let _ = tx.send(Message::Binary(end_pkt.encode()));
                                            }
                                        });
                                    }
                                }
                                PacketType::Heartbeat => {
                                    let pong = Packet::pong();
                                    let _ = ws_write.send(Message::Binary(pong.encode())).await;
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

struct ProxyResult {
    response: Vec<u8>,
    ws_relay: Option<WsRelay>,
}

struct WsRelay {
    stream_id: xpo_core::StreamId,
    upstream: TcpStream,
}

async fn proxy_to_upstream(
    port: u16,
    raw_request: &[u8],
    stream_id: xpo_core::StreamId,
    log_state: &Arc<std::sync::Mutex<LogState>>,
) -> ProxyResult {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let is_ws_upgrade = {
        let s = String::from_utf8_lossy(raw_request).to_ascii_lowercase();
        s.contains("upgrade: websocket")
    };

    let mut upstream = match TcpStream::connect(("localhost", port)).await {
        Ok(s) => s,
        Err(_) => {
            return ProxyResult {
                response:
                    b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 18\r\n\r\nupstream is down\r\n"
                        .to_vec(),
                ws_relay: None,
            };
        }
    };

    let request_bytes = if is_ws_upgrade {
        raw_request.to_vec()
    } else {
        inject_connection_close(raw_request)
    };

    if upstream.write_all(&request_bytes).await.is_err() {
        return ProxyResult {
            response: b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 18\r\n\r\nupstream is down\r\n"
                .to_vec(),
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
                    log_request(log_state, raw_request, &response);
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

    log_request(log_state, raw_request, &response);
    ProxyResult {
        response,
        ws_relay: None,
    }
}

fn inject_connection_close(raw: &[u8]) -> Vec<u8> {
    let s = String::from_utf8_lossy(raw);
    if let Some(pos) = s.find("\r\n") {
        let mut patched = Vec::with_capacity(raw.len() + 20);
        patched.extend_from_slice(&raw[..pos + 2]);
        patched.extend_from_slice(b"Connection: close\r\n");
        let rest = &raw[pos + 2..];
        let rest_str = String::from_utf8_lossy(rest);
        let filtered: String = rest_str
            .lines()
            .filter(|l| !l.to_ascii_lowercase().starts_with("connection:"))
            .collect::<Vec<_>>()
            .join("\r\n");
        patched.extend_from_slice(filtered.as_bytes());
        if !filtered.ends_with("\r\n\r\n") {
            patched.extend_from_slice(b"\r\n");
        }
        patched
    } else {
        raw.to_vec()
    }
}

fn log_request(state: &Arc<std::sync::Mutex<LogState>>, raw_request: &[u8], raw_response: &[u8]) {
    let req_str = String::from_utf8_lossy(raw_request);
    let parts: Vec<&str> = req_str.split_whitespace().collect();
    let method = parts.first().copied().unwrap_or("???");
    let path = parts.get(1).copied().unwrap_or("/");

    let resp_str = String::from_utf8_lossy(raw_response);
    let status = resp_str.split_whitespace().nth(1).unwrap_or("---");

    let status_code: u16 = status.parse().unwrap_or(0);
    let styled_status = if status_code >= 500 {
        style(status).red().bold()
    } else if status_code >= 400 {
        style(status).yellow()
    } else if status_code >= 300 {
        style(status).cyan()
    } else if status_code >= 200 {
        style(status).green()
    } else {
        style(status).dim()
    };

    let line = format!("  {:<6} {} {}", style(method).bold(), path, styled_status);

    let mut state = state.lock().unwrap();
    state.entries.push_back(line);
    if state.max > 0 && state.entries.len() > state.max {
        state.entries.pop_front();
    }

    if state.displayed > 0 {
        print!("\x1b[{}A\x1b[J", state.displayed);
    }
    for entry in &state.entries {
        println!("{entry}");
    }
    use std::io::Write;
    std::io::stdout().flush().ok();
    state.displayed = state.entries.len();
}

fn print_banner(url: &str, port: u16, user: &str) {
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

fn get_token() -> String {
    let config = xpo_core::config::Config::load().unwrap_or_default();
    if let Some(token) = config.access_token {
        return token;
    }
    let claims = xpo_core::auth::Claims {
        sub: "dev-user".into(),
        aud: "authenticated".into(),
        exp: (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600),
        iat: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        email: Some("dev@localhost".into()),
        role: Some("authenticated".into()),
    };
    create_test_token("xpo-dev-secret-for-local-testing", &claims)
}
