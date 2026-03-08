use crate::error::{Result, XpoError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_server() -> String {
    "eu.xpo.sh".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>,
    pub user_id: Option<String>,
    pub email: Option<String>,
    #[serde(default = "default_server")]
    pub default_server: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = Self::path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(&path)
            .map_err(|e| XpoError::Config(format!("failed to read {}: {e}", path.display())))?;
        let config: Config = serde_yaml::from_str(&contents)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let dir = Self::dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| XpoError::Config(format!("failed to create {}: {e}", dir.display())))?;
        let contents = serde_yaml::to_string(self)?;
        std::fs::write(Self::path(), contents)
            .map_err(|e| XpoError::Config(format!("failed to write config: {e}")))?;
        Ok(())
    }

    pub fn is_authenticated(&self) -> bool {
        self.access_token.is_some()
    }

    pub fn is_expired(&self) -> bool {
        match self.expires_at {
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
        self.access_token = None;
        self.refresh_token = None;
        self.expires_at = None;
        self.user_id = None;
        self.email = None;
    }

    pub fn path() -> PathBuf {
        Self::dir().join("config.yaml")
    }

    pub fn dir() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".xpo")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_full_config() {
        let yaml = r#"
access_token: jwt-abc
refresh_token: refresh-xyz
expires_at: 9999999999
user_id: uuid-123
email: test@xpo.sh
default_server: us.xpo.sh
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.access_token.as_deref(), Some("jwt-abc"));
        assert_eq!(config.refresh_token.as_deref(), Some("refresh-xyz"));
        assert_eq!(config.expires_at, Some(9999999999));
        assert_eq!(config.user_id.as_deref(), Some("uuid-123"));
        assert_eq!(config.email.as_deref(), Some("test@xpo.sh"));
        assert_eq!(config.default_server, "us.xpo.sh");
    }

    #[test]
    fn serialize_roundtrip() {
        let config = Config {
            access_token: Some("token".into()),
            refresh_token: Some("refresh".into()),
            expires_at: Some(1234567890),
            user_id: Some("uid".into()),
            email: Some("a@b.com".into()),
            default_server: "eu.xpo.sh".into(),
        };
        let yaml = serde_yaml::to_string(&config).unwrap();
        let restored: Config = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(config.access_token, restored.access_token);
        assert_eq!(config.expires_at, restored.expires_at);
    }

    #[test]
    fn default_server_value() {
        let config = Config::default();
        assert_eq!(config.default_server, "");
        let yaml = "access_token: x\n";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.default_server, "eu.xpo.sh");
    }

    #[test]
    fn is_authenticated() {
        let mut config = Config::default();
        assert!(!config.is_authenticated());
        config.access_token = Some("token".into());
        assert!(config.is_authenticated());
    }

    #[test]
    fn is_expired() {
        let mut config = Config::default();
        assert!(config.is_expired());
        config.expires_at = Some(0);
        assert!(config.is_expired());
        config.expires_at = Some(9999999999);
        assert!(!config.is_expired());
    }

    #[test]
    fn clear_tokens() {
        let mut config = Config {
            access_token: Some("t".into()),
            refresh_token: Some("r".into()),
            expires_at: Some(123),
            user_id: Some("u".into()),
            email: Some("e".into()),
            default_server: "eu.xpo.sh".into(),
        };
        config.clear_tokens();
        assert!(config.access_token.is_none());
        assert!(config.refresh_token.is_none());
        assert!(config.expires_at.is_none());
        assert!(config.user_id.is_none());
        assert!(config.email.is_none());
        assert_eq!(config.default_server, "eu.xpo.sh");
    }
}
