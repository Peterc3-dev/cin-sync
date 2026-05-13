use std::process::Stdio;
use tokio::process::Command;
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PeerState {
    Online,
    Offline,
}

pub async fn check_peer(ip: &str) -> PeerState {
    let Ok(output) = Command::new("ping")
        .args(["-c", "1", "-W", "2", ip])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
    else {
        return PeerState::Offline;
    };

    if output.success() {
        PeerState::Online
    } else {
        PeerState::Offline
    }
}

pub async fn rsync_push(
    local_path: &str,
    remote_user: &str,
    remote_ip: &str,
    remote_path: &str,
    excludes: &[String],
) -> anyhow::Result<()> {
    let dest = format!("{remote_user}@{remote_ip}:{remote_path}");
    let mut args = vec![
        "-avz".to_string(),
        "--delete".to_string(),
        "-e".to_string(),
        "ssh -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10".to_string(),
    ];
    for exc in excludes {
        args.push("--exclude".to_string());
        args.push(exc.clone());
    }
    // Ensure trailing slash so rsync syncs contents, not the dir itself
    let src = if local_path.ends_with('/') {
        local_path.to_string()
    } else {
        format!("{local_path}/")
    };
    args.push(src);
    args.push(dest);

    info!("rsync push: {}", args.join(" "));
    let output = Command::new("rsync")
        .args(&args)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("rsync push failed: {stderr}");
    }
    Ok(())
}

pub async fn rsync_pull(
    remote_user: &str,
    remote_ip: &str,
    remote_path: &str,
    local_path: &str,
    excludes: &[String],
) -> anyhow::Result<()> {
    let src = format!("{remote_user}@{remote_ip}:{remote_path}/");
    let mut args = vec![
        "-avz".to_string(),
        "-e".to_string(),
        "ssh -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10".to_string(),
    ];
    for exc in excludes {
        args.push("--exclude".to_string());
        args.push(exc.clone());
    }
    args.push(src);
    args.push(format!("{local_path}/"));

    info!("rsync pull: {}", args.join(" "));
    let output = Command::new("rsync")
        .args(&args)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("rsync pull failed: {stderr}");
    }
    Ok(())
}

pub async fn scp_file(
    local_file: &str,
    remote_user: &str,
    remote_ip: &str,
    remote_dir: &str,
) -> anyhow::Result<()> {
    let dest = format!("{remote_user}@{remote_ip}:{remote_dir}/");

    let escaped_dir = shell_escape(remote_dir);
    let mkdir_status = Command::new("ssh")
        .args([
            "-o", "StrictHostKeyChecking=accept-new",
            "-o", "ConnectTimeout=10",
            &format!("{remote_user}@{remote_ip}"),
            &format!("mkdir -p {escaped_dir}"),
        ])
        .status()
        .await?;
    if !mkdir_status.success() {
        anyhow::bail!("failed to create remote dir: {remote_dir}");
    }

    let output = Command::new("rsync")
        .args([
            "-avz", "--progress",
            "-e", "ssh -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10",
            local_file,
            &dest,
        ])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("file transfer failed: {stderr}");
    }
    Ok(())
}

pub async fn verify_remote_file(
    remote_user: &str,
    remote_ip: &str,
    remote_path: &str,
    expected_sha256: &str,
) -> anyhow::Result<bool> {
    let output = Command::new("ssh")
        .args([
            "-o", "StrictHostKeyChecking=accept-new",
            "-o", "ConnectTimeout=10",
            &format!("{remote_user}@{remote_ip}"),
            &format!("sha256sum {}", shell_escape(remote_path)),
        ])
        .output()
        .await?;

    if !output.status.success() {
        return Ok(false);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let remote_hash = stdout.split_whitespace().next().unwrap_or("");
    Ok(remote_hash == expected_sha256)
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
