use crate::config::{expand_tilde, OffloadConfig};
use crate::net;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use tracing::{info, warn, error};
use walkdir::WalkDir;

pub struct OffloadCandidate {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub reason: &'static str,
}

pub fn scan_candidates(config: &OffloadConfig) -> Vec<OffloadCandidate> {
    let mut candidates = Vec::new();
    let min_bytes = config.min_size_mb * 1024 * 1024;

    for dir in &config.watch_dirs {
        let expanded = expand_tilde(dir);
        if !expanded.exists() {
            continue;
        }

        let walker = WalkDir::new(&expanded)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_excluded(e.path(), &config.exclude));

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    warn!("walkdir error: {e}");
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path().to_path_buf();
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };

            if matches_patterns(&path, &config.always_offload) {
                candidates.push(OffloadCandidate {
                    path,
                    size_bytes: meta.len(),
                    reason: "pattern match",
                });
            } else if meta.len() >= min_bytes {
                candidates.push(OffloadCandidate {
                    path,
                    size_bytes: meta.len(),
                    reason: "size threshold",
                });
            }
        }
    }

    candidates
}

pub async fn offload_file(
    file: &OffloadCandidate,
    remote_user: &str,
    remote_ip: &str,
    vault_path: &str,
    delete_after: bool,
) -> anyhow::Result<()> {
    let local_hash = tokio::task::spawn_blocking({
        let path = file.path.clone();
        move || hash_file(&path)
    })
    .await??;
    let vault = expand_tilde(vault_path);
    let remote_vault = vault.to_string_lossy();

    let home = dirs::home_dir().expect("cannot determine home directory");
    let relative = file
        .path
        .strip_prefix(&home)
        .unwrap_or(&file.path);
    let remote_dir = format!(
        "{remote_vault}/{}",
        relative.parent().map(|p| p.to_string_lossy()).unwrap_or_default()
    );
    let filename = file
        .path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("no filename: {}", file.path.display()))?
        .to_string_lossy();
    let remote_file = format!("{remote_dir}/{filename}");

    info!(
        "offloading {} ({:.1} MB, {})",
        file.path.display(),
        file.size_bytes as f64 / (1024.0 * 1024.0),
        file.reason
    );

    net::scp_file(
        &file.path.to_string_lossy(),
        remote_user,
        remote_ip,
        &remote_dir,
    )
    .await?;

    let verified = net::verify_remote_file(remote_user, remote_ip, &remote_file, &local_hash).await?;

    if verified {
        info!("verified remote copy: {filename}");
        if delete_after {
            std::fs::remove_file(&file.path)
                .map_err(|e| anyhow::anyhow!("delete {}: {e}", file.path.display()))?;
            info!("deleted local: {}", file.path.display());
        }
    } else {
        error!(
            "hash mismatch after transfer — keeping local copy: {}",
            file.path.display()
        );
    }

    Ok(())
}

fn hash_file(path: &Path) -> anyhow::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let hash = hasher.finalize();
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    Ok(hex)
}

fn matches_patterns(path: &Path, patterns: &[String]) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    patterns.iter().any(|pat| {
        if let Some(ext) = pat.strip_prefix("*.") {
            name.ends_with(&format!(".{ext}"))
        } else {
            name == pat
        }
    })
}

fn is_excluded(path: &Path, excludes: &[String]) -> bool {
    path.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        excludes.iter().any(|exc| s == *exc)
    })
}
