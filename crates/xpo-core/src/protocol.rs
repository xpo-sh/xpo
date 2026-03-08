use crate::error::{Result, XpoError};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

pub const PACKET_HEADER_SIZE: usize = 17;
pub const HEARTBEAT_INTERVAL_SECS: u64 = 30;
pub const HEARTBEAT_TIMEOUT_SECS: u64 = 40;
pub const RECONNECT_MIN_SECS: u64 = 1;
pub const RECONNECT_MAX_SECS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StreamId(pub Uuid);

impl StreamId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(Uuid::from_bytes(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        self.0.as_bytes()
    }
}

impl Default for StreamId {
    fn default() -> Self {
        Self(Uuid::nil())
    }
}

impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ClientControl {
    Auth {
        token: String,
    },
    Hello {
        port: u16,
        subdomain: Option<String>,
    },
}

impl ClientControl {
    pub fn to_json(&self) -> std::result::Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(s: &str) -> std::result::Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ServerControl {
    AuthOk { user: String, user_id: String },
    AuthFail { reason: String },
    TunnelReady { url: String, subdomain: String },
    Error { message: String },
}

impl ServerControl {
    pub fn to_json(&self) -> std::result::Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(s: &str) -> std::result::Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketType {
    Connection = 0x01,
    Data = 0x02,
    End = 0x03,
    Heartbeat = 0x04,
    Pong = 0x05,
}

impl TryFrom<u8> for PacketType {
    type Error = XpoError;

    fn try_from(byte: u8) -> Result<Self> {
        match byte {
            0x01 => Ok(Self::Connection),
            0x02 => Ok(Self::Data),
            0x03 => Ok(Self::End),
            0x04 => Ok(Self::Heartbeat),
            0x05 => Ok(Self::Pong),
            _ => Err(XpoError::UnknownPacketType(byte)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Packet {
    pub packet_type: PacketType,
    pub stream_id: StreamId,
    pub payload: Vec<u8>,
}

impl Packet {
    pub fn new(packet_type: PacketType, stream_id: StreamId, payload: Vec<u8>) -> Self {
        Self {
            packet_type,
            stream_id,
            payload,
        }
    }

    pub fn connection(stream_id: StreamId) -> Self {
        Self::new(PacketType::Connection, stream_id, Vec::new())
    }

    pub fn data(stream_id: StreamId, payload: Vec<u8>) -> Self {
        Self::new(PacketType::Data, stream_id, payload)
    }

    pub fn end(stream_id: StreamId) -> Self {
        Self::new(PacketType::End, stream_id, Vec::new())
    }

    pub fn heartbeat() -> Self {
        Self::new(PacketType::Heartbeat, StreamId::default(), Vec::new())
    }

    pub fn pong() -> Self {
        Self::new(PacketType::Pong, StreamId::default(), Vec::new())
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(PACKET_HEADER_SIZE + self.payload.len());
        buf.push(self.packet_type as u8);
        buf.extend_from_slice(self.stream_id.as_bytes());
        buf.extend_from_slice(&self.payload);
        buf
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < PACKET_HEADER_SIZE {
            return Err(XpoError::PacketTooShort {
                expected: PACKET_HEADER_SIZE,
                actual: data.len(),
            });
        }
        let packet_type = PacketType::try_from(data[0])?;
        let mut stream_bytes = [0u8; 16];
        stream_bytes.copy_from_slice(&data[1..17]);
        let stream_id = StreamId::from_bytes(stream_bytes);
        let payload = data[17..].to_vec();
        Ok(Self {
            packet_type,
            stream_id,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_id_unique() {
        let a = StreamId::new();
        let b = StreamId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn stream_id_bytes_roundtrip() {
        let id = StreamId::new();
        let bytes = *id.as_bytes();
        assert_eq!(id, StreamId::from_bytes(bytes));
    }

    #[test]
    fn client_control_auth_json() {
        let msg = ClientControl::Auth {
            token: "abc123".into(),
        };
        let json = msg.to_json().unwrap();
        assert!(json.contains("\"type\":\"Auth\""));
        assert!(json.contains("\"token\":\"abc123\""));
        assert_eq!(msg, ClientControl::from_json(&json).unwrap());
    }

    #[test]
    fn client_control_hello_json() {
        let msg = ClientControl::Hello {
            port: 3000,
            subdomain: Some("myapp".into()),
        };
        let json = msg.to_json().unwrap();
        assert!(json.contains("\"type\":\"Hello\""));
        assert_eq!(msg, ClientControl::from_json(&json).unwrap());
    }

    #[test]
    fn server_control_all_variants_json() {
        let cases: Vec<ServerControl> = vec![
            ServerControl::AuthOk {
                user: "a@b.com".into(),
                user_id: "uuid-1".into(),
            },
            ServerControl::AuthFail {
                reason: "bad token".into(),
            },
            ServerControl::TunnelReady {
                url: "https://myapp.xpo.sh".into(),
                subdomain: "myapp".into(),
            },
            ServerControl::Error {
                message: "subdomain taken".into(),
            },
        ];
        for msg in cases {
            let json = msg.to_json().unwrap();
            assert_eq!(msg, ServerControl::from_json(&json).unwrap());
        }
    }

    #[test]
    fn packet_type_valid() {
        assert_eq!(PacketType::try_from(0x01).unwrap(), PacketType::Connection);
        assert_eq!(PacketType::try_from(0x02).unwrap(), PacketType::Data);
        assert_eq!(PacketType::try_from(0x03).unwrap(), PacketType::End);
        assert_eq!(PacketType::try_from(0x04).unwrap(), PacketType::Heartbeat);
        assert_eq!(PacketType::try_from(0x05).unwrap(), PacketType::Pong);
    }

    #[test]
    fn packet_type_invalid() {
        assert!(PacketType::try_from(0x00).is_err());
        assert!(PacketType::try_from(0x06).is_err());
        assert!(PacketType::try_from(0xFF).is_err());
    }

    #[test]
    fn packet_encode_decode_roundtrip() {
        let id = StreamId::new();
        let packets = vec![
            Packet::connection(id),
            Packet::data(id, b"hello world".to_vec()),
            Packet::end(id),
            Packet::heartbeat(),
            Packet::pong(),
        ];
        for pkt in packets {
            let encoded = pkt.encode();
            assert_eq!(pkt, Packet::decode(&encoded).unwrap());
        }
    }

    #[test]
    fn packet_binary_format() {
        let id = StreamId::from_bytes([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        let pkt = Packet::data(id, vec![0xAA, 0xBB]);
        let bytes = pkt.encode();
        assert_eq!(bytes[0], 0x02);
        assert_eq!(
            &bytes[1..17],
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
        );
        assert_eq!(&bytes[17..], &[0xAA, 0xBB]);
    }

    #[test]
    fn packet_decode_too_short() {
        let err = Packet::decode(&[0x01, 0x02]).unwrap_err();
        assert!(err.to_string().contains("packet too short"));
    }

    #[test]
    fn packet_decode_unknown_type() {
        let mut data = vec![0xFF];
        data.extend_from_slice(&[0u8; 16]);
        let err = Packet::decode(&data).unwrap_err();
        assert!(err.to_string().contains("unknown packet type"));
    }

    #[test]
    fn packet_large_payload() {
        let id = StreamId::new();
        let payload = vec![0x42; 64 * 1024];
        let pkt = Packet::data(id, payload.clone());
        let encoded = pkt.encode();
        assert_eq!(encoded.len(), PACKET_HEADER_SIZE + 64 * 1024);
        assert_eq!(Packet::decode(&encoded).unwrap().payload, payload);
    }

    #[test]
    fn heartbeat_pong_empty_payload() {
        let hb = Packet::heartbeat();
        assert!(hb.payload.is_empty());
        assert_eq!(hb.packet_type, PacketType::Heartbeat);

        let pong = Packet::pong();
        assert!(pong.payload.is_empty());
        assert_eq!(pong.packet_type, PacketType::Pong);
    }
}
