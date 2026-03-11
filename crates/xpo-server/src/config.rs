use std::fs;
use std::time::Instant;

const DEFAULT_HTTP_PORT: u16 = 8080;
const DEFAULT_WS_PORT: u16 = 8081;
const DEFAULT_JWT_SECRET: &str = "xpo-dev-secret-for-local-testing";

pub struct ServerConfig {
    pub http_bind: String,
    pub http_port: u16,
    pub ws_bind: String,
    pub ws_port: u16,
    pub base_domain: String,
    pub scheme: String,
    pub region: String,
    pub instance_id: String,
    pub jwt_key_material: String,
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

        let base_domain = env_str("BASE_DOMAIN", "localhost");
        let jwt_key_material = resolve_jwt_key_material(
            &base_domain,
            std::env::var("JWT_SECRET").ok(),
            std::env::var("JWT_PUBLIC_KEY").ok(),
            std::env::var("JWT_PUBLIC_KEY_PATH").ok(),
        )
        .unwrap_or_else(|e| panic!("FATAL: {e}"));
        let _ = xpo_core::auth::JwtValidator::new(&jwt_key_material);

        Self {
            http_bind: env_str("HTTP_BIND", "0.0.0.0"),
            http_port: env_parse("HTTP_PORT", DEFAULT_HTTP_PORT),
            ws_bind: env_str("WS_BIND", "0.0.0.0"),
            ws_port: env_parse("WS_PORT", DEFAULT_WS_PORT),
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
            started_at: Instant::now(),

            base_domain,
            jwt_key_material,
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

fn resolve_jwt_key_material(
    base_domain: &str,
    jwt_secret: Option<String>,
    jwt_public_key: Option<String>,
    jwt_public_key_path: Option<String>,
) -> std::result::Result<String, String> {
    let is_local = is_local_base_domain(base_domain);
    let jwt_secret = non_empty_value(jwt_secret);
    let jwt_public_key = non_empty_value(jwt_public_key);
    let jwt_public_key_path = non_empty_value(jwt_public_key_path);

    if let Some(public_key) = jwt_public_key {
        return ensure_public_key_pem(normalize_multiline_env(public_key));
    }

    if let Some(public_key_path) = jwt_public_key_path {
        return read_public_key_from_path(&public_key_path).and_then(ensure_public_key_pem);
    }

    if let Some(jwt_secret) = jwt_secret {
        return Ok(jwt_secret);
    }

    if is_local {
        return Ok(DEFAULT_JWT_SECRET.to_string());
    }

    Err(format!(
        "JWT_PUBLIC_KEY, JWT_PUBLIC_KEY_PATH, or JWT_SECRET must be set when BASE_DOMAIN={base_domain}."
    ))
}

fn read_public_key_from_path(path: &str) -> std::result::Result<String, String> {
    let public_key = fs::read_to_string(path)
        .map_err(|e| format!("failed to read JWT_PUBLIC_KEY_PATH={path}: {e}"))?;
    let public_key = normalize_multiline_env(public_key);

    if public_key.trim().is_empty() {
        return Err(format!("JWT public key file is empty: {path}"));
    }

    Ok(public_key)
}

fn normalize_multiline_env(value: String) -> String {
    value.replace("\\n", "\n")
}

fn ensure_public_key_pem(public_key: String) -> std::result::Result<String, String> {
    if !public_key.trim_start().starts_with("-----BEGIN ") {
        return Err("JWT public key material must be PEM encoded.".into());
    }

    Ok(public_key)
}

fn non_empty_value(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn is_local_base_domain(base_domain: &str) -> bool {
    matches!(base_domain, "localhost" | "127.0.0.1" | "::1" | "[::1]")
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const TEST_PUBLIC_KEY: &str =
        "-----BEGIN PUBLIC KEY-----\nline-one\nline-two\n-----END PUBLIC KEY-----\n";

    #[test]
    fn localhost_defaults_to_dev_secret() {
        let jwt_key = resolve_jwt_key_material("localhost", None, None, None).unwrap();
        assert_eq!(jwt_key, DEFAULT_JWT_SECRET);
    }

    #[test]
    fn public_domain_rejects_shared_secret() {
        let jwt_key =
            resolve_jwt_key_material("xpo.sh", Some("super-secret".into()), None, None).unwrap();
        assert_eq!(jwt_key, "super-secret");
    }

    #[test]
    fn public_domain_requires_public_key() {
        let err = resolve_jwt_key_material("xpo.sh", None, None, None).unwrap_err();
        assert!(err.contains("JWT_PUBLIC_KEY"));
        assert!(err.contains("JWT_SECRET"));
    }

    #[test]
    fn inline_public_key_unescapes_newlines() {
        let public_key = resolve_jwt_key_material(
            "xpo.sh",
            None,
            Some("-----BEGIN PUBLIC KEY-----\\nline-one\\n-----END PUBLIC KEY-----".into()),
            None,
        )
        .unwrap();

        assert!(public_key.contains('\n'));
        assert!(public_key.starts_with("-----BEGIN PUBLIC KEY-----"));
    }

    #[test]
    fn public_key_path_loads_file() {
        let mut path = std::env::temp_dir();
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("xpo-jwt-public-{suffix}.pem"));

        fs::write(&path, TEST_PUBLIC_KEY).unwrap();

        let public_key =
            resolve_jwt_key_material("xpo.sh", None, None, Some(path.display().to_string()))
                .unwrap();
        assert_eq!(public_key, TEST_PUBLIC_KEY);

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn public_key_path_requires_pem() {
        let mut path = std::env::temp_dir();
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("xpo-jwt-public-invalid-{suffix}.pem"));

        fs::write(&path, "not-a-pem").unwrap();

        let err = resolve_jwt_key_material("xpo.sh", None, None, Some(path.display().to_string()))
            .unwrap_err();
        assert!(err.contains("PEM"));

        fs::remove_file(path).unwrap();
    }
}
