use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct Config {
    pub identity: Identity,
    pub peer: Peer,
    pub sync: SyncConfig,
    pub offload: OffloadConfig,
    #[serde(default)]
    pub hooks: Hooks,
}

#[derive(Debug, Deserialize)]
pub struct Identity {
    pub name: String,
    pub role: Role,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Mobile,
    Hub,
}

#[derive(Debug, Deserialize)]
pub struct Peer {
    pub name: String,
    pub tailscale_ip: String,
    pub ssh_user: String,
    #[serde(default = "default_poll")]
    pub poll_interval_secs: u64,
}

fn default_poll() -> u64 {
    15
}

#[derive(Debug, Deserialize)]
pub struct SyncConfig {
    pub paths: Vec<SyncPath>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct SyncPath {
    pub local: String,
    pub remote: String,
    pub direction: SyncDirection,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum SyncDirection {
    Bidirectional,
    Push,
    Pull,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OffloadConfig {
    pub vault_path: String,
    #[serde(default = "default_true")]
    pub delete_after_send: bool,
    #[serde(default = "default_min_size")]
    pub min_size_mb: u64,
    #[serde(default)]
    pub always_offload: Vec<String>,
    #[serde(default)]
    pub watch_dirs: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn default_min_size() -> u64 {
    500
}

#[derive(Debug, Default, Deserialize)]
pub struct Hooks {
    #[serde(default)]
    pub on_connect: Vec<String>,
    #[serde(default)]
    pub on_disconnect: Vec<String>,
    #[serde(default)]
    pub on_sync_done: Vec<String>,
}

impl SyncPath {
    pub fn expand_local(&self) -> PathBuf {
        expand_tilde(&self.local)
    }
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        dirs::home_dir().expect("cannot determine home directory")
    } else if let Some(stripped) = path.strip_prefix("~/") {
        dirs::home_dir()
            .expect("cannot determine home directory")
            .join(stripped)
    } else {
        PathBuf::from(path)
    }
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&text)?;
        Ok(config)
    }
}
