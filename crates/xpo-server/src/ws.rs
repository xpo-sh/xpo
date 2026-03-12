use crate::state::{SharedState, Tunnel, TunnelMessage};
use dashmap::mapref::entry::Entry;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashSet;
use std::sync::LazyLock;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc::{self, error::TrySendError};
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, info, warn};
use xpo_core::protocol::{ClientControl, Packet, PacketType, ServerControl};
use xpo_core::{HEARTBEAT_INTERVAL_SECS, HEARTBEAT_TIMEOUT_SECS};

const DEFAULT_FREE_MAX_TUNNELS: usize = 6;
const ABSOLUTE_MAX_TUNNELS_PER_USER: usize = 32;
const DEFAULT_FREE_MAX_TTL_SECS: u64 = 3600;
const TUNNEL_CHANNEL_SIZE: usize = 256;
const MAX_WS_MESSAGE_SIZE: usize = 12 * 1024 * 1024;
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

struct PlanLimits {
    max_tunnels: usize,
    max_ttl_secs: Option<u64>,
    allow_custom_subdomain: bool,
}

impl PlanLimits {
    fn free_defaults() -> Self {
        Self {
            max_tunnels: DEFAULT_FREE_MAX_TUNNELS,
            max_ttl_secs: Some(DEFAULT_FREE_MAX_TTL_SECS),
            allow_custom_subdomain: false,
        }
    }

    fn from_profile(profile: &crate::supabase::UserProfile) -> Self {
        let max_tunnels = (profile.max_tunnels as usize).min(ABSOLUTE_MAX_TUNNELS_PER_USER);
        let max_ttl_secs = profile.max_ttl_secs.map(|v| v as u64);
        let allow_custom_subdomain = matches!(profile.plan.as_str(), "pro" | "team");

        Self {
            max_tunnels,
            max_ttl_secs,
            allow_custom_subdomain,
        }
    }
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

    let user_id;
    let plan_limits;

    let auth_msg =
        match tokio::time::timeout(std::time::Duration::from_secs(5), ws_read.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => text,
            _ => {
                warn!("auth timeout or invalid message");
                return;
            }
        };

    match ClientControl::from_json(&auth_msg) {
        Ok(ClientControl::Auth { token }) => match state.jwt_validator.validate(&token) {
            Ok(claims) => {
                user_id = claims.sub.clone();
                plan_limits = if let Some(ref supa) = state.supabase {
                    match supa.get_user_profile(&user_id).await {
                        Ok(Some(profile)) => PlanLimits::from_profile(&profile),
                        Ok(None) => {
                            warn!(user_id = %user_id, "no user profile found, using free defaults");
                            PlanLimits::free_defaults()
                        }
                        Err(e) => {
                            error!(user_id = %user_id, error = %e, "PostgREST query failed");
                            let resp = ServerControl::Error {
                                message: "service temporarily unavailable".into(),
                            };
                            let _ = ws_write
                                .send(Message::Text(resp.to_json().unwrap().into()))
                                .await;
                            return;
                        }
                    }
                } else {
                    PlanLimits::free_defaults()
                };
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
    if user_tunnel_count >= plan_limits.max_tunnels {
        let resp = ServerControl::Error {
            message: format!("max {} tunnels per user", plan_limits.max_tunnels),
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
                if subdomain.is_some() && !plan_limits.allow_custom_subdomain {
                    let resp = ServerControl::Error {
                        message: "custom subdomains require Pro".into(),
                    };
                    let _ = ws_write
                        .send(Message::Text(resp.to_json().unwrap().into()))
                        .await;
                    return;
                }
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
                let effective_ttl = plan_limits
                    .max_ttl_secs
                    .map(|max_ttl| ttl_secs.unwrap_or(max_ttl).min(max_ttl));
                (sub, password, effective_ttl, port)
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
    let relay_control_tx = tunnel_tx.clone();

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

    let ttl_notify = std::sync::Arc::new(tokio::sync::Notify::new());
    if let Some(ttl) = tunnel_ttl {
        let state_ttl = state.clone();
        let sub_ttl = subdomain.clone();
        let uid_ttl = user_id.clone();
        let notify = ttl_notify.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(ttl)).await;
            state_ttl.tunnels.remove(&sub_ttl);
            state_ttl.decrement_user_tunnels(&uid_ttl);
            state_ttl.remove_streams_for_subdomain(&sub_ttl);
            info!(subdomain = %sub_ttl, "tunnel TTL expired");
            notify.notify_one();
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
                                    } else {
                                        let stream_id = packet.stream_id;
                                        let payload = packet.payload;
                                        let relay_tx = state_clone
                                            .streams
                                            .get(&stream_id)
                                            .map(|stream| stream.from_client_tx.clone());

                                        if let Some(tx) = relay_tx {
                                            match tx.try_send(payload) {
                                                Ok(()) => {}
                                                Err(TrySendError::Full(_)) => {
                                                    warn!(
                                                        subdomain = %subdomain_clone,
                                                        stream_id = %stream_id,
                                                        "closing overloaded relay stream"
                                                    );
                                                    state_clone.remove_stream(&stream_id);
                                                    if relay_control_tx
                                                        .try_send(TunnelMessage::StreamEnd { stream_id })
                                                        .is_err()
                                                    {
                                                        break;
                                                    }
                                                }
                                                Err(TrySendError::Closed(_)) => {
                                                    state_clone.remove_stream(&stream_id);
                                                }
                                            }
                                        }
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
        _ = ttl_notify.notified() => {
            info!(subdomain = %subdomain, "closing WS connection due to TTL expiry");
            let close = Message::Close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
                code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Normal,
                reason: "TTL expired".into(),
            }));
            let _ = ws_write.send(close).await;
        },
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

    #[test]
    fn plan_limits_free_defaults() {
        let limits = PlanLimits::free_defaults();
        assert_eq!(limits.max_tunnels, DEFAULT_FREE_MAX_TUNNELS);
        assert_eq!(limits.max_ttl_secs, Some(DEFAULT_FREE_MAX_TTL_SECS));
        assert!(!limits.allow_custom_subdomain);
    }

    #[test]
    fn plan_limits_from_pro_profile() {
        let profile = crate::supabase::UserProfile {
            id: "user-123".into(),
            plan: "pro".into(),
            max_tunnels: 32,
            max_ttl_secs: None,
            max_reserved_subdomains: 5,
        };

        let limits = PlanLimits::from_profile(&profile);
        assert_eq!(limits.max_tunnels, ABSOLUTE_MAX_TUNNELS_PER_USER);
        assert_eq!(limits.max_ttl_secs, None);
        assert!(limits.allow_custom_subdomain);
    }

    #[test]
    fn plan_limits_from_team_profile() {
        let profile = crate::supabase::UserProfile {
            id: "user-456".into(),
            plan: "team".into(),
            max_tunnels: 20,
            max_ttl_secs: Some(7200),
            max_reserved_subdomains: 10,
        };

        let limits = PlanLimits::from_profile(&profile);
        assert_eq!(limits.max_tunnels, 20);
        assert_eq!(limits.max_ttl_secs, Some(7200));
        assert!(limits.allow_custom_subdomain);
    }

    #[test]
    fn plan_limits_from_free_profile() {
        let profile = crate::supabase::UserProfile {
            id: "user-789".into(),
            plan: "free".into(),
            max_tunnels: 6,
            max_ttl_secs: Some(3600),
            max_reserved_subdomains: 0,
        };

        let limits = PlanLimits::from_profile(&profile);
        assert_eq!(limits.max_tunnels, 6);
        assert_eq!(limits.max_ttl_secs, Some(3600));
        assert!(!limits.allow_custom_subdomain);
    }

    #[test]
    fn plan_limits_caps_at_absolute_max() {
        let profile = crate::supabase::UserProfile {
            id: "user-999".into(),
            plan: "pro".into(),
            max_tunnels: 100,
            max_ttl_secs: None,
            max_reserved_subdomains: 50,
        };

        let limits = PlanLimits::from_profile(&profile);
        assert_eq!(limits.max_tunnels, ABSOLUTE_MAX_TUNNELS_PER_USER);
    }
}
