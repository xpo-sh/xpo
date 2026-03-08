use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub token: Option<String>,
    pub default_server: Option<String>,
}

impl Config {
    pub fn path() -> PathBuf {
        dirs_next().join("config.yaml")
    }

    pub fn dir() -> PathBuf {
        dirs_next()
    }
}

fn dirs_next() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".xpo")
}
