use std::time::Instant;

const DEFAULT_HTTP_PORT: u16 = 8080;
const DEFAULT_WS_PORT: u16 = 8081;
const DEFAULT_JWT_SECRET: &str = "xpo-dev-secret-for-local-testing";

pub struct ServerConfig {
    pub http_port: u16,
    pub ws_port: u16,
    pub base_domain: String,
    pub scheme: String,
    pub region: String,
    pub instance_id: String,
    pub jwt_secret: String,
    pub started_at: Instant,

    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
    pub tls_self_signed: bool,

    pub acme_enabled: bool,
    pub acme_email: Option<String>,
    pub acme_staging: bool,
    pub cert_dir: String,
    pub acme_account_path: String,
    pub cf_api_token: Option<String>,
    pub cf_zone_id: Option<String>,
}

impl ServerConfig {
    pub fn from_env() -> Self {
        let tls_cert = std::env::var("TLS_CERT").ok();
        let tls_key = std::env::var("TLS_KEY").ok();
        let tls_self_signed = env_str("TLS_SELF_SIGNED", "false") == "true";
        let acme_enabled = env_str("ACME_ENABLED", "false") == "true";
        let tls_enabled = tls_cert.is_some() || tls_self_signed || acme_enabled;

        Self {
            http_port: env_parse("HTTP_PORT", DEFAULT_HTTP_PORT),
            ws_port: env_parse("WS_PORT", DEFAULT_WS_PORT),
            base_domain: env_str("BASE_DOMAIN", "localhost"),
            scheme: if tls_enabled {
                env_str("SCHEME", "https")
            } else {
                env_str("SCHEME", "http")
            },
            region: env_str("REGION", "local"),
            instance_id: env_str(
                "INSTANCE_ID",
                &std::env::var("HOSTNAME")
                    .or_else(|_| std::env::var("HOST"))
                    .unwrap_or_else(|_| "unknown".to_string()),
            ),
            jwt_secret: env_str("JWT_SECRET", DEFAULT_JWT_SECRET),
            started_at: Instant::now(),

            tls_cert,
            tls_key,
            tls_self_signed,

            acme_enabled,
            acme_email: std::env::var("ACME_EMAIL").ok(),
            acme_staging: env_str("ACME_STAGING", "true") == "true",
            cert_dir: env_str("CERT_DIR", "/etc/xpo/certs"),
            acme_account_path: env_str("ACME_ACCOUNT_PATH", "/etc/xpo/acme/account.json"),
            cf_api_token: std::env::var("CF_API_TOKEN").ok(),
            cf_zone_id: std::env::var("CF_ZONE_ID").ok(),
        }
    }

    pub fn tls_enabled(&self) -> bool {
        self.tls_cert.is_some() || self.tls_self_signed || self.acme_enabled
    }

    pub fn tunnel_url(&self, subdomain: &str) -> String {
        if self.scheme == "https" {
            format!("https://{subdomain}.{}", self.base_domain)
        } else {
            format!("http://{subdomain}.{}:{}", self.base_domain, self.http_port)
        }
    }

    pub fn cert_path(&self) -> String {
        format!("{}/wildcard.pem", self.cert_dir)
    }

    pub fn key_path(&self) -> String {
        format!("{}/wildcard.key", self.cert_dir)
    }
}

fn env_str(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
