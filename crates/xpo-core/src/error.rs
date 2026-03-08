use thiserror::Error;

#[derive(Debug, Error)]
pub enum XpoError {
    #[error("authentication failed: {reason}")]
    AuthFailed { reason: String },

    #[error("token expired")]
    TokenExpired,

    #[error("token refresh failed: {0}")]
    TokenRefreshFailed(String),

    #[error("unknown packet type: 0x{0:02x}")]
    UnknownPacketType(u8),

    #[error("packet too short: expected at least {expected} bytes, got {actual}")]
    PacketTooShort { expected: usize, actual: usize },

    #[error("config error: {0}")]
    Config(String),

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("tunnel error: {0}")]
    Tunnel(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("jwt error: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),
}

pub type Result<T> = std::result::Result<T, XpoError>;
