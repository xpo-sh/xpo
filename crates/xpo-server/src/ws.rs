use crate::state::{SharedState, Tunnel, TunnelMessage};
use dashmap::mapref::entry::Entry;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashSet;
use std::sync::LazyLock;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};
use xpo_core::auth::JwtValidator;
use xpo_core::protocol::{ClientControl, Packet, PacketType, ServerControl};
use xpo_core::{HEARTBEAT_INTERVAL_SECS, HEARTBEAT_TIMEOUT_SECS};

const MAX_TUNNELS_PER_USER: usize = 5;
const TUNNEL_CHANNEL_SIZE: usize = 256;
const MAX_WS_MESSAGE_SIZE: usize = 10 * 1024 * 1024;
const MAX_WS_FRAME_SIZE: usize = 2 * 1024 * 1024;

static RESERVED_SUBDOMAINS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    include_str!("reserved_subdomains.txt")
        .lines()
        .filter(|l| !l.is_empty())
        .collect()
});

fn is_valid_subdomain(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !RESERVED_SUBDOMAINS.contains(s)
}

pub async fn handle_websocket<S>(stream: S, state: SharedState)
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let mut ws_config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default();
    ws_config.max_message_size = Some(MAX_WS_MESSAGE_SIZE);
    ws_config.max_frame_size = Some(MAX_WS_FRAME_SIZE);

    let ws_stream = match tokio_tungstenite::accept_async_with_config(stream, Some(ws_config)).await
    {
        Ok(ws) => ws,
        Err(e) => {
            warn!("ws upgrade failed: {e}");
            return;
        }
    };

    let (mut ws_write, mut ws_read) = ws_stream.split();

    let validator = JwtValidator::new(&state.config.jwt_secret);
    let user_id;

    let auth_msg =
        match tokio::time::timeout(std::time::Duration::from_secs(5), ws_read.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => text,
            _ => {
                warn!("auth timeout or invalid message");
                return;
            }
        };

    match ClientControl::from_json(&auth_msg) {
        Ok(ClientControl::Auth { token }) => match validator.validate(&token) {
            Ok(claims) => {
                user_id = claims.sub.clone();
                let resp = ServerControl::AuthOk {
                    user: claims.email.unwrap_or_default(),
                    user_id: claims.sub,
                };
                let _ = ws_write
                    .send(Message::Text(resp.to_json().unwrap().into()))
                    .await;
                info!(user_id = %user_id, "authenticated");
            }
            Err(e) => {
                let resp = ServerControl::AuthFail {
                    reason: e.to_string(),
                };
                let _ = ws_write
                    .send(Message::Text(resp.to_json().unwrap().into()))
                    .await;
                warn!("auth failed: {e}");
                return;
            }
        },
        _ => {
            let resp = ServerControl::AuthFail {
                reason: "expected Auth message".into(),
            };
            let _ = ws_write
                .send(Message::Text(resp.to_json().unwrap().into()))
                .await;
            return;
        }
    }

    let user_tunnel_count = state.get_user_tunnel_count(&user_id);
    if user_tunnel_count >= MAX_TUNNELS_PER_USER {
        let resp = ServerControl::Error {
            message: format!("max {MAX_TUNNELS_PER_USER} tunnels per user"),
        };
        let _ = ws_write
            .send(Message::Text(resp.to_json().unwrap().into()))
            .await;
        warn!(user_id = %user_id, "tunnel limit reached");
        return;
    }

    let hello_msg =
        match tokio::time::timeout(std::time::Duration::from_secs(5), ws_read.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => text,
            _ => {
                warn!("hello timeout");
                return;
            }
        };

    let (subdomain, tunnel_password, tunnel_ttl, tunnel_port) =
        match ClientControl::from_json(&hello_msg) {
            Ok(ClientControl::Hello {
                port,
                subdomain,
                password,
                ttl_secs,
            }) => {
                let sub = subdomain.unwrap_or_else(crate::state::ServerState::generate_subdomain);
                if !is_valid_subdomain(&sub) {
                    let resp = ServerControl::Error {
                        message: "invalid subdomain".into(),
                    };
                    let _ = ws_write
                        .send(Message::Text(resp.to_json().unwrap().into()))
                        .await;
                    warn!(subdomain = %sub, "invalid subdomain");
                    return;
                }
                (sub, password, ttl_secs, port)
            }
            _ => {
                let resp = ServerControl::Error {
                    message: "expected Hello message".into(),
                };
                let _ = ws_write
                    .send(Message::Text(resp.to_json().unwrap().into()))
                    .await;
                return;
            }
        };

    let (tunnel_tx, mut tunnel_rx) = mpsc::channel::<TunnelMessage>(TUNNEL_CHANNEL_SIZE);

    match state.tunnels.entry(subdomain.clone()) {
        Entry::Occupied(_) => {
            let resp = ServerControl::Error {
                message: "subdomain taken".into(),
            };
            let _ = ws_write
                .send(Message::Text(resp.to_json().unwrap().into()))
                .await;
            warn!(subdomain = %subdomain, "subdomain taken");
            return;
        }
        Entry::Vacant(entry) => {
            entry.insert(Tunnel {
                user_id: user_id.clone(),
                subdomain: subdomain.clone(),
                tx: tunnel_tx,
                password: tunnel_password,
                port: tunnel_port,
                created_at: std::time::Instant::now(),
                ttl_secs: tunnel_ttl,
            });
            state.increment_user_tunnels(&user_id);
        }
    }

    let url = state.config.tunnel_url(&subdomain);
    let resp = ServerControl::TunnelReady {
        url: url.clone(),
        subdomain: subdomain.clone(),
    };
    let _ = ws_write
        .send(Message::Text(resp.to_json().unwrap().into()))
        .await;
    info!(subdomain = %subdomain, url = %url, "tunnel ready");

    if let Some(ttl) = tunnel_ttl {
        let state_ttl = state.clone();
        let sub_ttl = subdomain.clone();
        let uid_ttl = user_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(ttl)).await;
            state_ttl.tunnels.remove(&sub_ttl);
            state_ttl.decrement_user_tunnels(&uid_ttl);
            state_ttl.remove_streams_for_subdomain(&sub_ttl);
            info!(subdomain = %sub_ttl, "tunnel TTL expired");
        });
    }

    let state_clone = state.clone();
    let subdomain_clone = subdomain.clone();

    tokio::select! {
        r = async {
            let mut heartbeat_interval = tokio::time::interval(
                std::time::Duration::from_secs(HEARTBEAT_INTERVAL_SECS)
            );
            heartbeat_interval.tick().await;
            loop {
                tokio::select! {
                    _ = heartbeat_interval.tick() => {
                        let hb = Packet::heartbeat().encode();
                        if ws_write.send(Message::Binary(hb.into())).await.is_err() {
                            break;
                        }
                    }
                    msg = tunnel_rx.recv() => {
                        match msg {
                            Some(TunnelMessage::HttpRequest { stream_id, raw_request }) => {
                                let conn = Packet::connection(stream_id).encode();
                                if ws_write.send(Message::Binary(conn.into())).await.is_err() {
                                    break;
                                }
                                let data = Packet::data(stream_id, raw_request).encode();
                                if ws_write.send(Message::Binary(data.into())).await.is_err() {
                                    break;
                                }
                            }
                            Some(TunnelMessage::StreamData { stream_id, data }) => {
                                let pkt = Packet::data(stream_id, data).encode();
                                if ws_write.send(Message::Binary(pkt.into())).await.is_err() {
                                    break;
                                }
                            }
                            Some(TunnelMessage::StreamEnd { stream_id }) => {
                                let pkt = Packet::end(stream_id).encode();
                                let _ = ws_write.send(Message::Binary(pkt.into())).await;
                            }
                            None => break,
                        }
                    }
                }
            }
        } => r,
        _ = async {
            let timeout_dur = std::time::Duration::from_secs(HEARTBEAT_TIMEOUT_SECS);
            let mut last_activity = tokio::time::Instant::now();
            loop {
                let msg = tokio::time::timeout(timeout_dur, ws_read.next()).await;
                match msg {
                    Ok(Some(Ok(Message::Binary(data)))) => {
                        last_activity = tokio::time::Instant::now();
                        if let Ok(packet) = Packet::decode(&data) {
                            match packet.packet_type {
                                PacketType::Pong => {}
                                PacketType::Data => {
                                    if let Some((_, pending)) = state_clone.pending.remove(&packet.stream_id) {
                                        let _ = pending.response_tx.send(packet.payload);
                                    } else if let Some(stream) = state_clone.streams.get(&packet.stream_id) {
                                        let _ = stream.from_client_tx.send(packet.payload);
                                    }
                                }
                                PacketType::End => {
                                    state_clone.pending.remove(&packet.stream_id);
                                    state_clone.remove_stream(&packet.stream_id);
                                }
                                _ => {}
                            }
                        }
                    }
                    Ok(Some(Ok(Message::Close(_)))) | Ok(None) => break,
                    Ok(Some(Err(_))) => break,
                    Err(_) => {
                        if last_activity.elapsed() > timeout_dur {
                            warn!(subdomain = %subdomain_clone, "heartbeat timeout");
                            break;
                        }
                    }
                    _ => {}
                }
            }
        } => {},
    };

    if let Some((_, tunnel)) = state.tunnels.remove(&subdomain) {
        state.decrement_user_tunnels(&tunnel.user_id);
    }
    state.remove_streams_for_subdomain(&subdomain);
    info!(subdomain = %subdomain, "tunnel closed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_subdomains() {
        assert!(is_valid_subdomain("myapp"));
        assert!(is_valid_subdomain("test-app"));
        assert!(is_valid_subdomain("a1b2c3"));
        assert!(is_valid_subdomain("x"));
    }

    #[test]
    fn invalid_subdomains() {
        assert!(!is_valid_subdomain(""));
        assert!(!is_valid_subdomain("-start"));
        assert!(!is_valid_subdomain("end-"));
        assert!(!is_valid_subdomain("UPPER"));
        assert!(!is_valid_subdomain("has space"));
        assert!(!is_valid_subdomain("has.dot"));
        assert!(!is_valid_subdomain(&"a".repeat(64)));
    }

    #[test]
    fn reserved_subdomains_blocked() {
        for name in [
            "admin",
            "auth",
            "api",
            "www",
            "dashboard",
            "login",
            "mail",
            "cdn",
            "static",
            "git",
            "deploy",
            "billing",
            "status",
            "grafana",
            "prometheus",
            "redis",
            "ssh",
            "vpn",
            "ai",
            "control-plane",
            "internal-api",
            "platform",
        ] {
            assert!(!is_valid_subdomain(name), "{name} should be reserved");
        }
    }

    #[test]
    fn reserved_list_loaded() {
        assert!(
            RESERVED_SUBDOMAINS.len() > 150,
            "stoplist should have 150+ entries"
        );
        assert!(RESERVED_SUBDOMAINS.contains("admin"));
        assert!(RESERVED_SUBDOMAINS.contains("ai-models"));
        assert!(!RESERVED_SUBDOMAINS.contains("myapp"));
    }
}
