use crate::error::{Result, XpoError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_server() -> String {
    "eu.xpo.sh".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub defaults: DefaultsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthConfig {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>,
    pub user_id: Option<String>,
    pub email: Option<String>,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultsConfig {
    #[serde(default = "default_server")]
    pub server: String,
}

impl Default for DefaultsConfig {
    fn default() -> Self {
        Self {
            server: default_server(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = Self::path();
        if !path.exists() {
            Self::migrate_from_yaml();
            if !path.exists() {
                return Ok(Self::default());
            }
        }
        let contents = std::fs::read_to_string(&path)
            .map_err(|e| XpoError::Config(format!("failed to read {}: {e}", path.display())))?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let dir = Self::dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| XpoError::Config(format!("failed to create {}: {e}", dir.display())))?;
        let contents = toml::to_string_pretty(self)
            .map_err(|e| XpoError::Config(format!("failed to serialize config: {e}")))?;
        let path = Self::path();
        std::fs::write(&path, contents)
            .map_err(|e| XpoError::Config(format!("failed to write config: {e}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    pub fn is_authenticated(&self) -> bool {
        self.auth.access_token.is_some()
    }

    pub fn is_expired(&self) -> bool {
        match self.auth.expires_at {
            Some(exp) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                now >= exp
            }
            None => true,
        }
    }

    pub fn clear_tokens(&mut self) {
        self.auth = AuthConfig::default();
    }

    pub fn path() -> PathBuf {
        Self::dir().join("config.toml")
    }

    pub fn dir() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".xpo")
    }

    fn migrate_from_yaml() {
        let yaml_path = Self::dir().join("config.yaml");
        if yaml_path.exists() {
            let _ = std::fs::remove_file(&yaml_path);
            eprintln!("  ! Config format changed to TOML. Please run: xpo login");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_full_config() {
        let toml_str = r#"
[auth]
access_token = "jwt-abc"
refresh_token = "refresh-xyz"
expires_at = 9999999999
user_id = "uuid-123"
email = "test@xpo.sh"

[defaults]
server = "us.xpo.sh"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.auth.access_token.as_deref(), Some("jwt-abc"));
        assert_eq!(config.auth.refresh_token.as_deref(), Some("refresh-xyz"));
        assert_eq!(config.auth.expires_at, Some(9999999999));
        assert_eq!(config.auth.user_id.as_deref(), Some("uuid-123"));
        assert_eq!(config.auth.email.as_deref(), Some("test@xpo.sh"));
        assert_eq!(config.defaults.server, "us.xpo.sh");
    }

    #[test]
    fn serialize_roundtrip() {
        let config = Config {
            auth: AuthConfig {
                access_token: Some("token".into()),
                refresh_token: Some("refresh".into()),
                expires_at: Some(1234567890),
                user_id: Some("uid".into()),
                email: Some("a@b.com".into()),
                provider: Some("github".into()),
            },
            defaults: DefaultsConfig {
                server: "eu.xpo.sh".into(),
            },
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let restored: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(config.auth.access_token, restored.auth.access_token);
        assert_eq!(config.auth.expires_at, restored.auth.expires_at);
    }

    #[test]
    fn default_server_value() {
        let config = Config::default();
        assert_eq!(config.defaults.server, "eu.xpo.sh");
        let toml_str = "[auth]\naccess_token = \"x\"\n";
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.defaults.server, "eu.xpo.sh");
    }

    #[test]
    fn is_authenticated() {
        let mut config = Config::default();
        assert!(!config.is_authenticated());
        config.auth.access_token = Some("token".into());
        assert!(config.is_authenticated());
    }

    #[test]
    fn is_expired() {
        let mut config = Config::default();
        assert!(config.is_expired());
        config.auth.expires_at = Some(0);
        assert!(config.is_expired());
        config.auth.expires_at = Some(9999999999);
        assert!(!config.is_expired());
    }

    #[test]
    fn clear_tokens() {
        let mut config = Config {
            auth: AuthConfig {
                access_token: Some("t".into()),
                refresh_token: Some("r".into()),
                expires_at: Some(123),
                user_id: Some("u".into()),
                email: Some("e".into()),
                provider: Some("github".into()),
            },
            defaults: DefaultsConfig {
                server: "eu.xpo.sh".into(),
            },
        };
        config.clear_tokens();
        assert!(config.auth.access_token.is_none());
        assert!(config.auth.refresh_token.is_none());
        assert!(config.auth.expires_at.is_none());
        assert!(config.auth.user_id.is_none());
        assert!(config.auth.email.is_none());
        assert_eq!(config.defaults.server, "eu.xpo.sh");
    }
}
