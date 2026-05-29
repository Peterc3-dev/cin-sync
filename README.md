# cin-sync

A small Rust daemon that keeps a pair of machines on the same Tailscale network in
sync, and offloads large files from one machine to the other when they're both
reachable.

It was written for a personal two-node setup (a laptop and an always-on hub), so
it is opinionated and single-peer by design rather than a general-purpose sync
tool.

## What it does

The daemon runs a loop:

1. Polls the configured peer with `ping` (over its Tailscale IP) on an interval.
2. When the peer transitions from offline to online, it:
   - runs the configured `on_connect` hooks,
   - performs an rsync sync of each configured path,
   - if this node's role is `mobile`, scans the watch directories for large or
     pattern-matched files and offloads them to the peer,
   - runs the `on_sync_done` hooks.
3. When the peer goes offline, it runs the `on_disconnect` hooks.

### Sync

Each sync path has a direction:

- `bidirectional` — pull from the peer first (so remote-only files arrive), then
  push local changes. Push does **not** use `--delete` in this mode, to avoid
  destroying remote-only files.
- `push` — push to the peer with `--delete`.
- `pull` — pull from the peer.

Sync is implemented by shelling out to `rsync` over SSH.

### Offload

When the node's role is `mobile`, after sync it walks the configured
`watch_dirs` and selects files that either match an `always_offload` glob
(e.g. `*.gguf`, `*.safetensors`) or exceed `min_size_mb`. Each candidate is:

1. hashed locally with SHA-256,
2. transferred to the peer's vault path via rsync (`--partial --progress`),
   recreating its path relative to `$HOME` under the vault,
3. verified by running `sha256sum` on the remote copy and comparing hashes,
4. deleted locally only if `delete_after_send` is set **and** the hashes match.

A hash mismatch leaves the local copy in place.

### Hooks

`on_connect`, `on_disconnect`, and `on_sync_done` are lists of shell commands run
via `sh -c`. They are optional and default to empty.

## Status

Working for its intended single-peer use case. Scope is intentionally narrow:

- One peer only (configured by name + Tailscale IP).
- Peer reachability is detected with `ping`, not a Tailscale status check.
- Sync and transfer rely on external `rsync`, `ssh`, and `ssh-based sha256sum`
  being available on both ends, with key-based SSH already set up.
- There is no conflict resolution beyond rsync's own timestamp/size logic; the
  bidirectional mode is last-writer-wins per file via two rsync passes.
- No automated tests.

## Requirements

- Rust (2021 edition) to build.
- `rsync`, `ssh`, and (on the peer) `sha256sum` on `PATH`.
- SSH access to the peer with key-based auth.
- Tailscale (or any network) providing the peer's IP.

## Build

```sh
cargo build --release
```

## Configure

The daemon looks for its config at, in order:

1. `~/.config/cin-sync/cin-sync.toml`
2. `cin-sync.toml` in the current directory

or a path passed as the first CLI argument.

See [`cin-sync.toml`](cin-sync.toml) in this repo for a complete, commented
example. The main sections are:

- `[identity]` — this node's `name` and `role` (`mobile` or `hub`).
- `[peer]` — the peer's `name`, `tailscale_ip`, `ssh_user`, and
  `poll_interval_secs`.
- `[sync]` — list of `paths` (each with `local`, `remote`, `direction`) and
  global `exclude` patterns. Paths support `~` expansion.
- `[offload]` — `vault_path`, `delete_after_send`, `min_size_mb`,
  `always_offload` globs, `watch_dirs`, and `exclude`.
- `[hooks]` — `on_connect`, `on_disconnect`, `on_sync_done`.

## Run

```sh
# uses ~/.config/cin-sync/cin-sync.toml or ./cin-sync.toml
cin-sync

# or point it at a specific config
cin-sync /path/to/cin-sync.toml
```

Log level is controlled by `RUST_LOG` (defaults to `cin_sync=info`).

## License

MIT — see [LICENSE](LICENSE).
