mod config;
mod net;
mod offload;

use config::{Config, Role, SyncDirection};
use net::PeerState;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time;
use tracing::{error, info, warn};

fn find_config() -> PathBuf {
    let candidates = [
        dirs::home_dir()
            .unwrap_or_default()
            .join(".config/cin-sync/cin-sync.toml"),
        PathBuf::from("cin-sync.toml"),
    ];
    for c in &candidates {
        if c.exists() {
            return c.clone();
        }
    }
    candidates[0].clone()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cin_sync=info".parse().unwrap()),
        )
        .init();

    let config_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(find_config);

    let config = Config::load(&config_path)?;
    info!(
        "cin-sync starting: {} ({})",
        config.identity.name,
        match config.identity.role {
            Role::Mobile => "mobile",
            Role::Hub => "hub",
        }
    );
    info!("peer: {} @ {}", config.peer.name, config.peer.tailscale_ip);

    let poll = Duration::from_secs(config.peer.poll_interval_secs);
    let mut prev_state = PeerState::Offline;
    let mut first_run = true;

    loop {
        let state = net::check_peer(&config.peer.tailscale_ip).await;

        if state == PeerState::Online && (prev_state == PeerState::Offline || first_run) {
            info!("peer {} came online — starting sync", config.peer.name);
            run_hooks(&config.hooks.on_connect).await;

            if let Err(e) = run_sync(&config).await {
                error!("sync failed: {e}");
            }

            if config.identity.role == Role::Mobile {
                if let Err(e) = run_offload(&config).await {
                    error!("offload failed: {e}");
                }
            }

            run_hooks(&config.hooks.on_sync_done).await;
            first_run = false;
        } else if state == PeerState::Offline && prev_state == PeerState::Online {
            info!("peer {} went offline", config.peer.name);
            run_hooks(&config.hooks.on_disconnect).await;
        }

        prev_state = state;
        time::sleep(poll).await;
    }
}

async fn run_sync(config: &Config) -> anyhow::Result<()> {
    let excludes = &config.offload.exclude;

    for sp in &config.sync.paths {
        let local = sp.expand_local();
        let local_str = local.to_string_lossy().to_string();

        if !local.exists() {
            warn!("sync path does not exist locally, skipping: {local_str}");
            continue;
        }

        match sp.direction {
            SyncDirection::Push | SyncDirection::Bidirectional => {
                info!("pushing {} → {}", local_str, sp.remote);
                if let Err(e) = net::rsync_push(
                    &local_str,
                    &config.peer.ssh_user,
                    &config.peer.tailscale_ip,
                    &sp.remote,
                    excludes,
                )
                .await
                {
                    error!("push failed for {local_str}: {e}");
                }
            }
            _ => {}
        }

        match sp.direction {
            SyncDirection::Pull | SyncDirection::Bidirectional => {
                info!("pulling {} ← {}", local_str, sp.remote);
                if let Err(e) = net::rsync_pull(
                    &config.peer.ssh_user,
                    &config.peer.tailscale_ip,
                    &sp.remote,
                    &local_str,
                    excludes,
                )
                .await
                {
                    error!("pull failed for {local_str}: {e}");
                }
            }
            _ => {}
        }
    }

    Ok(())
}

async fn run_offload(config: &Config) -> anyhow::Result<()> {
    let candidates = offload::scan_candidates(&config.offload);

    if candidates.is_empty() {
        info!("no offload candidates found");
        return Ok(());
    }

    info!("found {} offload candidates", candidates.len());

    for file in &candidates {
        if let Err(e) = offload::offload_file(
            file,
            &config.peer.ssh_user,
            &config.peer.tailscale_ip,
            &config.offload.vault_path,
            config.offload.delete_after_send,
        )
        .await
        {
            error!("offload failed for {}: {e}", file.path.display());
        }
    }

    Ok(())
}

async fn run_hooks(hooks: &[String]) {
    for cmd in hooks {
        info!("running hook: {cmd}");
        match tokio::process::Command::new("sh")
            .args(["-c", cmd])
            .status()
            .await
        {
            Ok(status) if status.success() => info!("hook ok: {cmd}"),
            Ok(status) => warn!("hook exited {}: {cmd}", status),
            Err(e) => error!("hook failed: {cmd}: {e}"),
        }
    }
}
