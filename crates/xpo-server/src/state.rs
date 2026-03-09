use crate::config::ServerConfig;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use xpo_core::StreamId;

pub type SharedState = Arc<ServerState>;

pub struct ServerState {
    pub tunnels: DashMap<String, Tunnel>,
    pub pending: DashMap<StreamId, PendingRequest>,
    pub streams: DashMap<StreamId, ActiveStream>,
    pub config: Arc<ServerConfig>,
}

#[allow(dead_code)]
pub struct Tunnel {
    pub user_id: String,
    pub subdomain: String,
    pub tx: mpsc::Sender<TunnelMessage>,
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
            config,
        })
    }

    pub fn generate_subdomain() -> String {
        use rand::Rng;
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
