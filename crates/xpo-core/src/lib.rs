pub mod auth;
pub mod config;
pub mod error;
pub mod error_page;
pub mod http;
pub mod protocol;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub use error::{Result, XpoError};
pub use http::{content_type_to_extension, extract_body_preview, parse_http_headers};
pub use protocol::{
    ClientControl, Packet, PacketType, ServerControl, StreamId, HEARTBEAT_INTERVAL_SECS,
    HEARTBEAT_TIMEOUT_SECS, PACKET_HEADER_SIZE, RECONNECT_MAX_SECS, RECONNECT_MIN_SECS,
};
