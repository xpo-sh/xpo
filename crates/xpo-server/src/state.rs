use crate::config::ServerConfig;
use dashmap::DashMap;
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use xpo_core::StreamId;

pub type SharedState = Arc<ServerState>;

pub struct ServerState {
    pub tunnels: DashMap<String, Tunnel>,
    pub pending: DashMap<StreamId, PendingRequest>,
    pub streams: DashMap<StreamId, ActiveStream>,
    pub user_tunnel_count: DashMap<String, AtomicUsize>,
    pub subdomain_streams: DashMap<String, HashSet<StreamId>>,
    pub config: Arc<ServerConfig>,
}

#[allow(dead_code)]
pub struct Tunnel {
    pub user_id: String,
    pub subdomain: String,
    pub tx: mpsc::Sender<TunnelMessage>,
    pub password: Option<String>,
    pub port: u16,
    pub created_at: std::time::Instant,
    pub ttl_secs: Option<u64>,
}

pub enum TunnelMessage {
    HttpRequest {
        stream_id: StreamId,
        raw_request: Vec<u8>,
    },
    StreamData {
        stream_id: StreamId,
        data: Vec<u8>,
    },
    StreamEnd {
        stream_id: StreamId,
    },
}

pub struct PendingRequest {
    pub response_tx: tokio::sync::oneshot::Sender<Vec<u8>>,
}

#[allow(dead_code)]
pub struct ActiveStream {
    pub from_client_tx: mpsc::UnboundedSender<Vec<u8>>,
    pub tunnel_subdomain: String,
}

impl ServerState {
    pub fn new(config: Arc<ServerConfig>) -> SharedState {
        Arc::new(Self {
            tunnels: DashMap::new(),
            pending: DashMap::new(),
            streams: DashMap::new(),
            user_tunnel_count: DashMap::new(),
            subdomain_streams: DashMap::new(),
            config,
        })
    }

    pub fn increment_user_tunnels(&self, user_id: &str) {
        self.user_tunnel_count
            .entry(user_id.to_string())
            .or_insert_with(|| AtomicUsize::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn decrement_user_tunnels(&self, user_id: &str) {
        if let Some(counter) = self.user_tunnel_count.get(user_id) {
            let prev = counter.fetch_sub(1, Ordering::Relaxed);
            drop(counter);
            if prev <= 1 {
                self.user_tunnel_count.remove(user_id);
            }
        }
    }

    pub fn get_user_tunnel_count(&self, user_id: &str) -> usize {
        self.user_tunnel_count
            .get(user_id)
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    pub fn add_stream(&self, stream_id: StreamId, subdomain: &str, stream: ActiveStream) {
        self.streams.insert(stream_id, stream);
        self.subdomain_streams
            .entry(subdomain.to_string())
            .or_default()
            .insert(stream_id);
    }

    pub fn remove_stream(&self, stream_id: &StreamId) {
        if let Some((_, stream)) = self.streams.remove(stream_id) {
            if let Some(mut set) = self.subdomain_streams.get_mut(&stream.tunnel_subdomain) {
                set.remove(stream_id);
                if set.is_empty() {
                    let subdomain = stream.tunnel_subdomain.clone();
                    drop(set);
                    self.subdomain_streams.remove(&subdomain);
                }
            }
        }
    }

    pub fn remove_streams_for_subdomain(&self, subdomain: &str) {
        if let Some((_, stream_ids)) = self.subdomain_streams.remove(subdomain) {
            for id in stream_ids {
                self.streams.remove(&id);
            }
        }
    }

    pub fn generate_subdomain() -> String {
        use rand::RngExt;
        let mut rng = rand::rng();
        let chars: Vec<char> = (0..6)
            .map(|_| {
                let idx = rng.random_range(0..36u8);
                if idx < 10 {
                    (b'0' + idx) as char
                } else {
                    (b'a' + idx - 10) as char
                }
            })
            .collect();
        chars.into_iter().collect()
    }
}
