use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    Auth {
        token: String,
    },
    Share {
        port: u16,
        subdomain: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    Authenticated {
        user: String,
    },
    TunnelReady {
        url: String,
        subdomain: String,
    },
    ProxyRequest {
        request_id: String,
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProxyResponse {
    pub request_id: String,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}
