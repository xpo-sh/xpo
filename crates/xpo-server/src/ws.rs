use crate::state::{SharedState, Tunnel, TunnelMessage};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};
use xpo_core::auth::JwtValidator;
use xpo_core::protocol::{ClientControl, Packet, PacketType, ServerControl};
use xpo_core::{HEARTBEAT_INTERVAL_SECS, HEARTBEAT_TIMEOUT_SECS};

pub async fn handle_websocket(stream: TcpStream, state: SharedState) {
    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            warn!("ws upgrade failed: {e}");
            return;
        }
    };

    let (mut ws_write, mut ws_read) = ws_stream.split();

    let validator = JwtValidator::new(&state.jwt_secret);
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
                let _ = ws_write.send(Message::Text(resp.to_json().unwrap())).await;
                info!(user_id = %user_id, "authenticated");
            }
            Err(e) => {
                let resp = ServerControl::AuthFail {
                    reason: e.to_string(),
                };
                let _ = ws_write.send(Message::Text(resp.to_json().unwrap())).await;
                warn!("auth failed: {e}");
                return;
            }
        },
        _ => {
            let resp = ServerControl::AuthFail {
                reason: "expected Auth message".into(),
            };
            let _ = ws_write.send(Message::Text(resp.to_json().unwrap())).await;
            return;
        }
    }

    let hello_msg =
        match tokio::time::timeout(std::time::Duration::from_secs(5), ws_read.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => text,
            _ => {
                warn!("hello timeout");
                return;
            }
        };

    let subdomain = match ClientControl::from_json(&hello_msg) {
        Ok(ClientControl::Hello { subdomain, .. }) => {
            let sub = subdomain.unwrap_or_else(crate::state::ServerState::generate_subdomain);
            if state.tunnels.contains_key(&sub) {
                let resp = ServerControl::Error {
                    message: "subdomain taken".into(),
                };
                let _ = ws_write.send(Message::Text(resp.to_json().unwrap())).await;
                warn!(subdomain = %sub, "subdomain taken");
                return;
            }
            sub
        }
        _ => {
            let resp = ServerControl::Error {
                message: "expected Hello message".into(),
            };
            let _ = ws_write.send(Message::Text(resp.to_json().unwrap())).await;
            return;
        }
    };

    let (tunnel_tx, mut tunnel_rx) = mpsc::unbounded_channel::<TunnelMessage>();

    state.tunnels.insert(
        subdomain.clone(),
        Tunnel {
            user_id: user_id.clone(),
            subdomain: subdomain.clone(),
            tx: tunnel_tx,
        },
    );

    let url = format!("http://{subdomain}.localhost:8080");
    let resp = ServerControl::TunnelReady {
        url: url.clone(),
        subdomain: subdomain.clone(),
    };
    let _ = ws_write.send(Message::Text(resp.to_json().unwrap())).await;
    info!(subdomain = %subdomain, url = %url, "tunnel ready");

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
                        if ws_write.send(Message::Binary(hb)).await.is_err() {
                            break;
                        }
                    }
                    msg = tunnel_rx.recv() => {
                        match msg {
                            Some(TunnelMessage::HttpRequest { stream_id, raw_request }) => {
                                let conn = Packet::connection(stream_id).encode();
                                if ws_write.send(Message::Binary(conn)).await.is_err() {
                                    break;
                                }
                                let data = Packet::data(stream_id, raw_request).encode();
                                if ws_write.send(Message::Binary(data)).await.is_err() {
                                    break;
                                }
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
                                PacketType::Data | PacketType::End => {
                                    if let Some((_, pending)) = state_clone.pending.remove(&packet.stream_id) {
                                        let _ = pending.response_tx.send(packet.payload);
                                    }
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

    state.tunnels.remove(&subdomain);
    info!(subdomain = %subdomain, "tunnel closed");
}
