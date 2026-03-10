pub mod auth;
pub mod config;
pub mod error;
pub mod error_page;
pub mod protocol;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub use error::{Result, XpoError};
pub use protocol::{
    ClientControl, Packet, PacketType, ServerControl, StreamId, HEARTBEAT_INTERVAL_SECS,
    HEARTBEAT_TIMEOUT_SECS, PACKET_HEADER_SIZE, RECONNECT_MAX_SECS, RECONNECT_MIN_SECS,
};
