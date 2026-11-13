use crate::db::{Database, Game, SnapshotRecord, StorageProfile};
use crate::resolver::{resolve_or_remap_exe_path, to_canonical_path};
use crate::snapshot::{compute_current_save_state, create_snapshot, restore_snapshot_archive};
use opendal::{services, Operator};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryCatalog {
    pub version: u32,
    pub updated_at: String,
    pub device_id: String,
    pub device_name: String,
    pub games: Vec<Game>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySyncOutcome {
    pub added: usize,
    pub updated: usize,
    pub remapped_paths: usize,
    pub missing_exe_count: usize,
    pub total_games: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteGameHead {
    pub game_id: String,
    pub title: String,
    pub head_snapshot_id: String,
    pub parent_snapshot_id: Option<String>,
    pub tree_hash: String,
    pub device_id: String,
    pub device_name: String,
    pub updated_at: String,
    pub file_count: usize,
    pub total_bytes: u64,
    /// Older snapshot ids in this head's lineage, newest first, capped at `MAX_LINEAGE`.
    /// Lets a device several snapshots behind fast-forward instead of seeing a conflict.
    #[serde(default)]
    pub ancestors: Vec<String>,
}

const MAX_LINEAGE: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncOutcome {
    pub action: String, // "up_to_date", "pushed", "pulled", "conflict"
    pub message: String,
    pub local_snapshot: Option<SnapshotRecord>,
    pub remote_head: Option<RemoteGameHead>,
}

/// Precondition for replacing a game's remote `head.json`.
pub enum HeadExpect {
    /// No remote head may exist yet.
    Absent,
    /// The remote head must still be this snapshot id.
    Is(String),
    /// Overwrite unconditionally (explicit conflict resolution).
    Any,
}

pub enum Remote {
    Dal(Operator),
    /// Self-hosted vnsync-server (`server/`), which enforces `HeadExpect` atomically.
    Server {
        base_url: String,
        token: String,
        client: reqwest::Client,
    },
}

impl Remote {
    fn server_request(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder, String> {
        match self {
            Remote::Server {
                base_url,
                token,
                client,
            } => Ok(client
                .request(method, format!("{}/api/{}", base_url, path))
                .bearer_auth(token)),
            Remote::Dal(_) => Err("Not a VNSync server profile".to_string()),
        }
    }

    async fn send(req: reqwest::RequestBuilder) -> Result<reqwest::Response, String> {
        let resp = req.send().await.map_err(|e| e.to_string())?;
        match resp.status().as_u16() {
            200..=299 => Ok(resp),
            401 => Err("Server rejected the token".to_string()),
            412 => Err("Remote saves changed during sync. Sync again.".to_string()),
            code => Err(format!("Server returned HTTP {}", code)),
        }
    }

    pub async fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        match self {
            Remote::Dal(op) => op
                .read(path)
                .await
                .map(|b| b.to_vec())
                .map_err(|e| e.to_string()),
            Remote::Server { .. } => {
                let req = self.server_request(reqwest::Method::GET, &format!("blob/{}", path))?;
                let resp = Self::send(req).await?;
                resp.bytes()
                    .await
                    .map(|b| b.to_vec())
                    .map_err(|e| e.to_string())
            }
        }
    }

    pub async fn write(&self, path: &str, bytes: Vec<u8>) -> Result<(), String> {
        self.write_head(path, bytes, HeadExpect::Any).await
    }

    /// Writes `path`; a vnsync-server rejects the write when `expected` no longer holds.
    /// OpenDAL backends cannot check it and always overwrite.
    pub async fn write_head(
        &self,
        path: &str,
        bytes: Vec<u8>,
        expected: HeadExpect,
    ) -> Result<(), String> {
        match self {
            Remote::Dal(op) => op
                .write(path, bytes)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string()),
            Remote::Server { .. } => {
                let req = self
                    .server_request(reqwest::Method::PUT, &format!("blob/{}", path))?
                    .body(bytes);
                let req = match expected {
                    HeadExpect::Absent => req.header("If-None-Match", "*"),
                    HeadExpect::Is(id) => req.header("If-Match", id),
                    HeadExpect::Any => req,
                };
                Self::send(req).await.map(|_| ())
            }
        }
    }
}

impl Remote {
    async fn read_head(&self, game_id: &str) -> Option<RemoteGameHead> {
        let bytes = self
            .read(&format!("games/{}/head.json", game_id))
            .await
            .ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Cloud snapshots of a game, newest first.
    pub async fn history(&self, game_id: &str) -> Result<Vec<RemoteGameHead>, String> {
        let mut list: Vec<RemoteGameHead> = match self {
            Remote::Server { .. } => {
                let req = self
                    .server_request(reqwest::Method::GET, &format!("games/{}/history", game_id))?;
                let bytes = Self::send(req)
                    .await?
                    .bytes()
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?
            }
            Remote::Dal(_) => match self.read(&dal_history_path(game_id)).await {
                Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
                Err(_) => Vec::new(),
            },
        };
        // Heads pushed before history was recorded still show up.
        if let Some(head) = self.read_head(game_id).await {
            if !list
                .iter()
                .any(|h| h.head_snapshot_id == head.head_snapshot_id)
            {
                list.insert(0, head);
            }
        }
        Ok(list)
    }

    /// Deletes every cloud file of a game: head, history and snapshot archives.
    pub async fn delete_game_data(&self, game_id: &str) -> Result<(), String> {
        match self {
            Remote::Dal(op) => op
                .remove_all(&format!("games/{}/", game_id))
                .await
                .map_err(|e| e.to_string()),
            Remote::Server { .. } => {
                let req =
                    self.server_request(reqwest::Method::DELETE, &format!("games/{}", game_id))?;
                Self::send(req).await.map(|_| ())
            }
        }
    }
}

/// OpenDAL backends keep their own history file; vnsync-server records history in `writeHead`.
fn dal_history_path(game_id: &str) -> String {
    format!("games/{}/history.json", game_id)
}

fn active_remote(db: &Database) -> Result<Remote, String> {
    let profile = db
        .get_active_storage_profile()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No active storage profile configured in Settings".to_string())?;
    build_remote(&profile)
}

pub async fn list_cloud_snapshots(
    db: &Database,
    game_id: &str,
) -> Result<Vec<RemoteGameHead>, String> {
    active_remote(db)?.history(game_id).await
}

pub async fn delete_cloud_game_data(db: &Database, game_id: &str) -> Result<(), String> {
    active_remote(db)?.delete_game_data(game_id).await
}

/// Restores an archive over the game's current saves, then records the result as a new
/// snapshot on top of `parent` (default: the current head). The next sync then pushes the
/// restored state instead of pulling a newer head back over it.
pub fn restore_as_new_snapshot(
    db: &Database,
    vault_dir: &Path,
    game: &mut Game,
    archive: &Path,
    parent: Option<String>,
    note: &str,
) -> Result<(usize, SnapshotRecord), String> {
    let settings = db.get_settings().map_err(|e| e.to_string())?;
    if settings.backup_before_sync {
        let (backup, _) = create_snapshot(
            game,
            vault_dir,
            &settings.device_id,
            &settings.device_name,
            "Pre-restore safety backup",
        )?;
        db.insert_snapshot(&backup).map_err(|e| e.to_string())?;
        game.head_snapshot_id = Some(backup.id);
    }

    let count = restore_snapshot_archive(game, archive)?;
    if parent.is_some() {
        game.head_snapshot_id = parent;
    }
    let (record, _) = create_snapshot(
        game,
        vault_dir,
        &settings.device_id,
        &settings.device_name,
        note,
    )?;
    db.insert_snapshot(&record).map_err(|e| e.to_string())?;
    game.head_snapshot_id = Some(record.id.clone());
    db.upsert_game(game).map_err(|e| e.to_string())?;
    Ok((count, record))
}

/// Downloads one cloud snapshot (if not already in the local vault) and restores it.
pub async fn restore_cloud_snapshot(
    db: &Database,
    vault_dir: &Path,
    game_id: &str,
    snapshot_id: &str,
) -> Result<(usize, SnapshotRecord), String> {
    let mut game = db
        .get_game(game_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Game not found".to_string())?;
    let settings = db.get_settings().map_err(|e| e.to_string())?;
    if heal_game_paths(&mut game, &settings.game_library_path).0 {
        db.upsert_game(&game).map_err(|e| e.to_string())?;
    }

    let op = active_remote(db)?;
    let history = op.history(game_id).await?;
    let entry = history
        .iter()
        .find(|h| h.head_snapshot_id == snapshot_id)
        .ok_or_else(|| "Snapshot not found in cloud storage".to_string())?;

    let archive = vault_dir
        .join(game_id)
        .join(format!("{}.tar.zst", snapshot_id));
    if !archive.is_file() {
        let data = op
            .read(&format!(
                "games/{}/snapshots/{}.tar.zst",
                game_id, snapshot_id
            ))
            .await
            .map_err(|e| format!("Failed downloading cloud snapshot: {}", e))?;
        fs::create_dir_all(vault_dir.join(game_id)).map_err(|e| e.to_string())?;
        fs::write(&archive, &data).map_err(|e| e.to_string())?;
    }

    // Build on the current cloud head, so the next sync fast-forward pushes this restore.
    let parent = op
        .read_head(game_id)
        .await
        .map(|h| h.head_snapshot_id)
        .or_else(|| game.head_snapshot_id.clone());
    let when = chrono::DateTime::parse_from_rfc3339(&entry.updated_at)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| entry.updated_at.clone());
    let note = format!("Restored cloud save from {} ({})", entry.device_name, when);
    restore_as_new_snapshot(db, vault_dir, &mut game, &archive, parent, &note)
}

pub fn build_remote(profile: &StorageProfile) -> Result<Remote, String> {
    if profile.provider_type == "vnsync_server" {
        let base_url = profile.endpoint.trim().trim_end_matches('/').to_string();
        if base_url.is_empty() {
            return Err("VNSync server URL is empty".to_string());
        }
        return Ok(Remote::Server {
            base_url,
            token: profile.secret_key.clone(),
            client: reqwest::Client::new(),
        });
    }
    build_operator(profile).map(Remote::Dal)
}

fn build_operator(profile: &StorageProfile) -> Result<Operator, String> {
    match profile.provider_type.as_str() {
        "s3" => {
            let mut builder = services::S3::default()
                .bucket(&profile.bucket)
                .region(if profile.region.is_empty() {
                    "auto"
                } else {
                    &profile.region
                })
                .access_key_id(&profile.access_key)
                .secret_access_key(&profile.secret_key);
            if !profile.endpoint.is_empty() {
                builder = builder.endpoint(&profile.endpoint);
            }
            if !profile.root_prefix.is_empty() {
                builder = builder.root(&profile.root_prefix);
            }
            Operator::new(builder)
                .map_err(|e| format!("S3 configuration error: {}", e))
                .map(|op| op.finish())
        }
        "webdav" => {
            let mut builder = services::Webdav::default()
                .endpoint(&profile.endpoint)
                .username(&profile.access_key)
                .password(&profile.secret_key);
            if !profile.root_prefix.is_empty() {
                builder = builder.root(&profile.root_prefix);
            }
            Operator::new(builder)
                .map_err(|e| format!("WebDAV configuration error: {}", e))
                .map(|op| op.finish())
        }
        "local_fs" => {
            let root = if profile.endpoint.is_empty() {
                std::env::temp_dir()
                    .join("vnsync_remote")
                    .to_string_lossy()
                    .to_string()
            } else {
                profile.endpoint.clone()
            };
            let _ = fs::create_dir_all(&root);
            let builder = services::Fs::default().root(&root);
            Operator::new(builder)
                .map_err(|e| format!("Local storage error: {}", e))
                .map(|op| op.finish())
        }
        other => Err(format!("Unsupported storage provider: {}", other)),
    }
}

pub async fn test_storage_connection(profile: &StorageProfile) -> Result<String, String> {
    let remote = build_remote(profile)?;
    let op = match &remote {
        Remote::Dal(op) => op,
        Remote::Server { .. } => {
            let req = remote.server_request(reqwest::Method::GET, "ping")?;
            Remote::send(req).await?;
            return Ok(format!(
                "Connected to {} ({})",
                profile.name, profile.provider_type
            ));
        }
    };
    let probe_path = "vnsync_healthcheck.txt";
    op.write(probe_path, "ok")
        .await
        .map_err(|e| format!("Write check failed: {}", e))?;
    let data = op
        .read(probe_path)
        .await
        .map_err(|e| format!("Read check failed: {}", e))?;
    let _ = op.delete(probe_path).await;
    if data.to_vec() == b"ok" {
        Ok(format!(
            "Connected to {} ({})",
            profile.name, profile.provider_type
        ))
    } else {
        Err("Verification payload mismatch".to_string())
    }
}

pub async fn sync_game(
    db: &Database,
    vault_dir: &Path,
    game_id: &str,
    force_direction: Option<&str>, // None, Some("push"), Some("pull")
) -> Result<SyncOutcome, String> {
    let mut game = db
        .get_game(game_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Game not found".to_string())?;

    let settings = db.get_settings().map_err(|e| e.to_string())?;
    // Paths may still point at another PC (e.g. after a catalog import); fix them before
    // reading or restoring saves, so a single-game sync works without a library sync first.
    if heal_game_paths(&mut game, &settings.game_library_path).0 {
        db.upsert_game(&game).map_err(|e| e.to_string())?;
    }
    let profile = db
        .get_active_storage_profile()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No active storage profile configured in Settings".to_string())?;

    let op = build_remote(&profile)?;

    // Check if local save files changed since last local snapshot
    let (current_tree_hash, current_files) = compute_current_save_state(&game)?;
    let mut local_head = match &game.head_snapshot_id {
        Some(id) => db.get_snapshot(id).map_err(|e| e.to_string())?,
        None => None,
    };

    // No save files found locally is never a new local change: checkpointing that empty
    // state would push it over real saves from another device.
    let local_dirty = !current_files.is_empty()
        && local_head
            .as_ref()
            .is_none_or(|snap| snap.tree_hash != current_tree_hash);

    if local_dirty {
        let (snap_record, _) = create_snapshot(
            &game,
            vault_dir,
            &settings.device_id,
            &settings.device_name,
            "Auto-checkpoint before sync",
        )?;
        db.insert_snapshot(&snap_record)
            .map_err(|e| e.to_string())?;
        game.head_snapshot_id = Some(snap_record.id.clone());
        local_head = Some(snap_record);
    }

    let head_path = format!("games/{}/head.json", game.id);
    let remote_head: Option<RemoteGameHead> = match op.read(&head_path).await {
        Ok(bytes) => serde_json::from_slice(&bytes).ok(),
        Err(_) => None,
    };

    // Explicit resolution overrides ("push" or "pull")
    if force_direction == Some("push") {
        let snap = local_head.ok_or_else(|| "No local saves found to push".to_string())?;
        let ancestors = local_lineage(db, snap.parent_id.clone(), None);
        let head = push_snapshot_to_remote(&op, &game, &snap, ancestors, HeadExpect::Any).await?;
        game.sync_status = "Synced".to_string();
        db.upsert_game(&game).map_err(|e| e.to_string())?;
        return Ok(SyncOutcome {
            action: "pushed".to_string(),
            message: "Pushed local save snapshot to remote storage".to_string(),
            local_snapshot: Some(snap),
            remote_head: Some(head),
        });
    }

    if force_direction == Some("pull") {
        let r_head = remote_head.ok_or_else(|| "No remote snapshot exists to pull".to_string())?;
        let snap = pull_snapshot_from_remote(&op, &mut game, &r_head, vault_dir, db).await?;
        return Ok(SyncOutcome {
            action: "pulled".to_string(),
            message: format!("Restored remote snapshot from {}", r_head.device_name),
            local_snapshot: Some(snap),
            remote_head: Some(r_head),
        });
    }

    // Automatic Lineage & Hash Comparison
    match (&local_head, &remote_head) {
        (None, None) => Ok(SyncOutcome {
            action: "up_to_date".to_string(),
            message: "No local or remote save files found yet".to_string(),
            local_snapshot: None,
            remote_head: None,
        }),
        (Some(l_snap), None) => {
            let ancestors = local_lineage(db, l_snap.parent_id.clone(), None);
            let head =
                push_snapshot_to_remote(&op, &game, l_snap, ancestors, HeadExpect::Absent).await?;
            game.sync_status = "Synced".to_string();
            db.upsert_game(&game).map_err(|e| e.to_string())?;
            Ok(SyncOutcome {
                action: "pushed".to_string(),
                message: "Uploaded initial save snapshot".to_string(),
                local_snapshot: Some(l_snap.clone()),
                remote_head: Some(head),
            })
        }
        (None, Some(r_head)) => {
            let snap = pull_snapshot_from_remote(&op, &mut game, r_head, vault_dir, db).await?;
            Ok(SyncOutcome {
                action: "pulled".to_string(),
                message: format!("Downloaded save snapshot from {}", r_head.device_name),
                local_snapshot: Some(snap),
                remote_head: Some(r_head.clone()),
            })
        }
        (Some(l_snap), Some(r_head)) => {
            if l_snap.tree_hash == r_head.tree_hash || l_snap.id == r_head.head_snapshot_id {
                game.sync_status = "Synced".to_string();
                db.upsert_game(&game).map_err(|e| e.to_string())?;
                return Ok(SyncOutcome {
                    action: "up_to_date".to_string(),
                    message: "Saves are already in sync".to_string(),
                    local_snapshot: Some(l_snap.clone()),
                    remote_head: Some(r_head.clone()),
                });
            }

            // Fast-forward push: remote head is somewhere in the local snapshot's lineage
            let ancestors = local_lineage(db, l_snap.parent_id.clone(), Some(r_head));
            if ancestors.contains(&r_head.head_snapshot_id) {
                let expected = HeadExpect::Is(r_head.head_snapshot_id.clone());
                let head = push_snapshot_to_remote(&op, &game, l_snap, ancestors, expected).await?;
                game.sync_status = "Synced".to_string();
                db.upsert_game(&game).map_err(|e| e.to_string())?;
                return Ok(SyncOutcome {
                    action: "pushed".to_string(),
                    message: "Fast-forward pushed newer local saves".to_string(),
                    local_snapshot: Some(l_snap.clone()),
                    remote_head: Some(head),
                });
            }

            // Fast-forward pull: local head is somewhere in the remote head's lineage
            if r_head.parent_snapshot_id.as_deref() == Some(l_snap.id.as_str())
                || r_head.ancestors.contains(&l_snap.id)
            {
                let snap = pull_snapshot_from_remote(&op, &mut game, r_head, vault_dir, db).await?;
                return Ok(SyncOutcome {
                    action: "pulled".to_string(),
                    message: format!(
                        "Fast-forward pulled newer saves from {}",
                        r_head.device_name
                    ),
                    local_snapshot: Some(snap),
                    remote_head: Some(r_head.clone()),
                });
            }

            // Diverged lineage -> Conflict!
            game.sync_status = "Conflict".to_string();
            db.upsert_game(&game).map_err(|e| e.to_string())?;
            Ok(SyncOutcome {
                action: "conflict".to_string(),
                message: format!(
                    "Save conflict detected between {} and {}",
                    l_snap.device_name, r_head.device_name
                ),
                local_snapshot: Some(l_snap.clone()),
                remote_head: Some(r_head.clone()),
            })
        }
    }
}

/// Walks local snapshot parents starting at `start`, newest first. On reaching `remote`'s head
/// it appends that head's recorded ancestors, since older remote snapshots may not exist locally.
fn local_lineage(
    db: &Database,
    start: Option<String>,
    remote: Option<&RemoteGameHead>,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = start;
    while let Some(id) = cur {
        if out.len() >= MAX_LINEAGE || out.contains(&id) {
            break;
        }
        if let Some(r) = remote.filter(|r| r.head_snapshot_id == id) {
            out.push(id);
            out.extend(r.ancestors.iter().cloned());
            break;
        }
        cur = db
            .get_snapshot(&id)
            .ok()
            .flatten()
            .and_then(|s| s.parent_id);
        out.push(id);
    }
    out.truncate(MAX_LINEAGE);
    out
}

async fn push_snapshot_to_remote(
    op: &Remote,
    game: &Game,
    snap: &SnapshotRecord,
    ancestors: Vec<String>,
    expected: HeadExpect,
) -> Result<RemoteGameHead, String> {
    let archive_bytes = fs::read(&snap.local_archive_path)
        .map_err(|e| format!("Failed reading local archive: {}", e))?;
    let remote_archive_path = format!("games/{}/snapshots/{}.tar.zst", game.id, snap.id);
    op.write(&remote_archive_path, archive_bytes)
        .await
        .map_err(|e| format!("Failed uploading snapshot archive: {}", e))?;

    let head = RemoteGameHead {
        game_id: game.id.clone(),
        title: game.title.clone(),
        head_snapshot_id: snap.id.clone(),
        parent_snapshot_id: snap.parent_id.clone(),
        tree_hash: snap.tree_hash.clone(),
        device_id: snap.device_id.clone(),
        device_name: snap.device_name.clone(),
        updated_at: snap.created_at.clone(),
        file_count: snap.file_count,
        total_bytes: snap.total_bytes,
        ancestors,
    };
    let head_bytes = serde_json::to_vec_pretty(&head).map_err(|e| e.to_string())?;
    let head_path = format!("games/{}/head.json", game.id);
    op.write_head(&head_path, head_bytes, expected)
        .await
        .map_err(|e| format!("Failed updating remote head.json: {}", e))?;

    if let Remote::Dal(_) = op {
        // Best effort: the push already succeeded; history only feeds the cloud snapshot list.
        // ponytail: read-modify-write, two devices pushing at once can drop an entry.
        let path = dal_history_path(&game.id);
        let mut history: Vec<RemoteGameHead> = match op.read(&path).await {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        history.retain(|h| h.head_snapshot_id != head.head_snapshot_id);
        history.insert(0, head.clone());
        if let Ok(bytes) = serde_json::to_vec(&history) {
            let _ = op.write(&path, bytes).await;
        }
    }

    Ok(head)
}

async fn pull_snapshot_from_remote(
    op: &Remote,
    game: &mut Game,
    r_head: &RemoteGameHead,
    vault_dir: &Path,
    db: &Database,
) -> Result<SnapshotRecord, String> {
    let remote_archive_path = format!(
        "games/{}/snapshots/{}.tar.zst",
        game.id, r_head.head_snapshot_id
    );
    let data = op
        .read(&remote_archive_path)
        .await
        .map_err(|e| format!("Failed downloading remote snapshot: {}", e))?;

    let game_vault = vault_dir.join(&game.id);
    fs::create_dir_all(&game_vault).map_err(|e| e.to_string())?;
    let local_archive_path = game_vault.join(format!("{}.tar.zst", r_head.head_snapshot_id));
    fs::write(&local_archive_path, &data).map_err(|e| e.to_string())?;

    restore_snapshot_archive(game, &local_archive_path)?;

    let snap_record = SnapshotRecord {
        id: r_head.head_snapshot_id.clone(),
        game_id: game.id.clone(),
        parent_id: r_head.parent_snapshot_id.clone(),
        device_id: r_head.device_id.clone(),
        device_name: r_head.device_name.clone(),
        created_at: r_head.updated_at.clone(),
        tree_hash: r_head.tree_hash.clone(),
        file_count: r_head.file_count,
        total_bytes: r_head.total_bytes,
        local_archive_path: local_archive_path.to_string_lossy().to_string(),
        note: format!("Synced from {}", r_head.device_name),
        pinned: false,
    };

    db.insert_snapshot(&snap_record)
        .map_err(|e| e.to_string())?;
    game.head_snapshot_id = Some(snap_record.id.clone());
    game.sync_status = "Synced".to_string();
    db.upsert_game(game).map_err(|e| e.to_string())?;

    Ok(snap_record)
}

pub fn build_library_catalog(db: &Database) -> Result<LibraryCatalog, String> {
    let settings = db.get_settings().map_err(|e| e.to_string())?;
    let games = db.list_games().map_err(|e| e.to_string())?;
    Ok(LibraryCatalog {
        version: 1,
        updated_at: chrono::Utc::now().to_rfc3339(),
        device_id: settings.device_id,
        device_name: settings.device_name,
        games,
    })
}

pub fn parse_library_catalog_json(raw: &str) -> Result<Vec<Game>, String> {
    if let Ok(cat) = serde_json::from_str::<LibraryCatalog>(raw) {
        return Ok(cat.games);
    }
    if let Ok(games) = serde_json::from_str::<Vec<Game>>(raw) {
        return Ok(games);
    }
    Err("Invalid library backup JSON format".to_string())
}

/// Remaps one game's exe and save paths to this PC. Returns `(changed, exe_exists, remapped_count)`.
fn heal_game_paths(g: &mut Game, game_library_path: &str) -> (bool, bool, usize) {
    let mut remapped = 0usize;
    let (resolved_exe, exists, exe_remapped) =
        resolve_or_remap_exe_path(&g.exe_path, game_library_path);
    if exe_remapped && resolved_exe != g.exe_path {
        g.exe_path = resolved_exe;
        remapped += 1;
    }
    let new_saves: Vec<String> = g
        .save_paths
        .iter()
        .map(|p| to_canonical_path(p, &g.exe_path, &g.proton_prefix))
        .collect();
    if new_saves != g.save_paths {
        g.save_paths = new_saves;
        remapped += 1;
    }
    (remapped > 0, exists, remapped)
}

/// Checks all games in the library for paths belonging to a different OS user account
/// or residing in `settings.game_library_path`, auto-remapping them when the target exists.
pub fn heal_library_paths(db: &Database) -> Result<LibrarySyncOutcome, String> {
    let settings = db.get_settings().map_err(|e| e.to_string())?;
    let games = db.list_games().map_err(|e| e.to_string())?;
    let mut remapped_paths = 0usize;
    let mut missing_exe_count = 0usize;

    for mut g in games.iter().cloned() {
        let (changed, exists, remapped) = heal_game_paths(&mut g, &settings.game_library_path);
        remapped_paths += remapped;
        if !exists {
            missing_exe_count += 1;
        }
        if changed {
            db.upsert_game(&g).map_err(|e| e.to_string())?;
        }
    }

    let total_games = games.len();
    Ok(LibrarySyncOutcome {
        added: 0,
        updated: 0,
        remapped_paths,
        missing_exe_count,
        total_games,
        message: format!(
            "Checked {} games ({} user paths remapped, {} need executable location)",
            total_games, remapped_paths, missing_exe_count
        ),
    })
}

/// Merges an incoming list of games (from cloud library catalog or backup file) into the local database,
/// auto-remapping OS user paths when folders exist under the current user.
pub fn merge_library_catalog(
    db: &Database,
    incoming: &[Game],
) -> Result<LibrarySyncOutcome, String> {
    let settings = db.get_settings().map_err(|e| e.to_string())?;
    let mut local_games = db.list_games().map_err(|e| e.to_string())?;
    let mut added = 0usize;
    let mut updated = 0usize;
    let mut remapped_paths = 0usize;

    for inc in incoming {
        let (inc_exe, inc_exe_exists, exe_was_remapped) =
            resolve_or_remap_exe_path(&inc.exe_path, &settings.game_library_path);
        if exe_was_remapped {
            remapped_paths += 1;
        }
        let canonical_saves: Vec<String> = inc
            .save_paths
            .iter()
            .map(|p| to_canonical_path(p, &inc_exe, &inc.proton_prefix))
            .collect();

        let existing_idx = local_games.iter().position(|g| {
            g.id == inc.id
                || (!inc.vndb_id.is_empty() && g.vndb_id == inc.vndb_id)
                || g.title.eq_ignore_ascii_case(&inc.title)
        });

        if let Some(idx) = existing_idx {
            let mut local = local_games[idx].clone();
            let local_exe_exists =
                !local.exe_path.trim().is_empty() && Path::new(&local.exe_path).is_file();

            if !local_exe_exists && (inc_exe_exists || local.exe_path.trim().is_empty()) {
                local.exe_path = inc_exe;
            }
            if local.original_title.is_empty() && !inc.original_title.is_empty() {
                local.original_title = inc.original_title.clone();
            }
            if local.vndb_id.is_empty() && !inc.vndb_id.is_empty() {
                local.vndb_id = inc.vndb_id.clone();
            }
            if (local.engine.is_empty() || local.engine == "Custom / Generic")
                && !inc.engine.is_empty()
            {
                local.engine = inc.engine.clone();
            }
            if local.developer.is_empty() && !inc.developer.is_empty() {
                local.developer = inc.developer.clone();
            }
            if local.release_date.is_empty() && !inc.release_date.is_empty() {
                local.release_date = inc.release_date.clone();
            }
            if local.description.is_empty() && !inc.description.is_empty() {
                local.description = inc.description.clone();
            }
            if local.thumbnail.is_empty() && !inc.thumbnail.is_empty() {
                local.thumbnail = inc.thumbnail.clone();
            }
            if local.save_paths.is_empty() && !canonical_saves.is_empty() {
                local.save_paths = canonical_saves;
            }
            local.playtime_seconds = local.playtime_seconds.max(inc.playtime_seconds);
            local.progress = local.progress.max(inc.progress);
            if local.status == "Not Started" && inc.status != "Not Started" {
                local.status = inc.status.clone();
            }

            db.upsert_game(&local).map_err(|e| e.to_string())?;
            local_games[idx] = local;
            updated += 1;
        } else {
            let new_game = Game {
                id: if inc.id.trim().is_empty() {
                    uuid::Uuid::now_v7().to_string()
                } else {
                    inc.id.clone()
                },
                title: inc.title.clone(),
                original_title: inc.original_title.clone(),
                vndb_id: inc.vndb_id.clone(),
                engine: inc.engine.clone(),
                developer: inc.developer.clone(),
                publisher: inc.publisher.clone(),
                release_date: inc.release_date.clone(),
                genre: inc.genre.clone(),
                rating: inc.rating.clone(),
                description: inc.description.clone(),
                status: inc.status.clone(),
                progress: inc.progress,
                thumbnail: inc.thumbnail.clone(),
                exe_path: inc_exe,
                proton_prefix: inc.proton_prefix.clone(),
                save_paths: canonical_saves,
                playtime_seconds: inc.playtime_seconds,
                last_played_at: inc.last_played_at.clone(),
                head_snapshot_id: None,
                sync_status: "Local Only".to_string(),
                exe_exists: inc_exe_exists,
            };
            db.upsert_game(&new_game).map_err(|e| e.to_string())?;
            local_games.push(new_game);
            added += 1;
        }
    }

    let heal_res = heal_library_paths(db)?;
    let total_remapped = remapped_paths + heal_res.remapped_paths;
    Ok(LibrarySyncOutcome {
        added,
        updated,
        remapped_paths: total_remapped,
        missing_exe_count: heal_res.missing_exe_count,
        total_games: heal_res.total_games,
        message: format!(
            "Library synced: {} added, {} updated, {} user paths remapped ({} need manual folder selection)",
            added, updated, total_remapped, heal_res.missing_exe_count
        ),
    })
}

/// Syncs the game library catalog (`library/catalog.json`) with active storage profile.
pub async fn sync_library_catalog(
    db: &Database,
    direction: Option<&str>, // None (bidirectional merge), Some("push"), Some("pull")
) -> Result<LibrarySyncOutcome, String> {
    let profile = db
        .get_active_storage_profile()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No active storage profile configured in Settings".to_string())?;
    let op = build_remote(&profile)?;
    let catalog_path = "library/catalog.json";

    let mut outcome = if direction != Some("push") {
        match op.read(catalog_path).await {
            Ok(bytes) => {
                let raw = String::from_utf8_lossy(&bytes.to_vec()).to_string();
                let incoming = parse_library_catalog_json(&raw)?;
                merge_library_catalog(db, &incoming)?
            }
            Err(_) => {
                if direction == Some("pull") {
                    return Err("No remote library catalog backup found in storage".to_string());
                }
                heal_library_paths(db)?
            }
        }
    } else {
        heal_library_paths(db)?
    };

    if direction != Some("pull") {
        let catalog = build_library_catalog(db)?;
        let payload = serde_json::to_vec_pretty(&catalog).map_err(|e| e.to_string())?;
        op.write(catalog_path, payload)
            .await
            .map_err(|e| format!("Failed uploading library catalog: {}", e))?;
        if direction == Some("push") {
            outcome.message = format!(
                "Backed up {} games in library catalog to {}",
                catalog.games.len(),
                profile.name
            );
        }
    }

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sync_push_pull_and_conflict() {
        let remote_dir = tempfile::tempdir().unwrap();
        sync_roundtrip(StorageProfile {
            id: "local-test".to_string(),
            name: "Local Test Cloud".to_string(),
            provider_type: "local_fs".to_string(),
            endpoint: remote_dir.path().to_string_lossy().to_string(),
            bucket: String::new(),
            region: String::new(),
            root_prefix: String::new(),
            access_key: String::new(),
            secret_key: String::new(),
            is_active: true,
        })
        .await;
    }

    /// Needs a running vnsync-server with an empty DATA_DIR:
    /// `VNSYNC_TEST_SERVER=http://localhost:3000 VNSYNC_TEST_TOKEN=... cargo test -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn test_sync_against_vnsync_server() {
        let profile = StorageProfile {
            id: "server-test".to_string(),
            name: "Test Server".to_string(),
            provider_type: "vnsync_server".to_string(),
            endpoint: std::env::var("VNSYNC_TEST_SERVER").unwrap(),
            bucket: String::new(),
            region: String::new(),
            root_prefix: String::new(),
            access_key: String::new(),
            secret_key: std::env::var("VNSYNC_TEST_TOKEN").unwrap(),
            is_active: true,
        };
        test_storage_connection(&profile).await.unwrap();
        sync_roundtrip(profile.clone()).await;

        // The server must refuse a head write whose expected parent is stale.
        let remote = build_remote(&profile).unwrap();
        let stale = remote
            .write_head(
                "games/vn-1/head.json",
                br#"{"headSnapshotId":"other"}"#.to_vec(),
                HeadExpect::Absent,
            )
            .await;
        assert_eq!(
            stale.unwrap_err(),
            "Remote saves changed during sync. Sync again."
        );
    }

    async fn sync_roundtrip(profile: StorageProfile) {
        let tmp = tempfile::tempdir().unwrap();
        let vault_a = tmp.path().join("vault_a");
        let saves_a = tmp.path().join("saves_a");
        fs::create_dir_all(&saves_a).unwrap();
        fs::write(saves_a.join("save1.dat"), b"state_v1").unwrap();

        let db_a = Database::open_in_memory().unwrap();
        db_a.upsert_storage_profile(&profile).unwrap();

        let game_a = Game {
            id: "vn-1".to_string(),
            title: "Fate/stay night".to_string(),
            original_title: String::new(),
            vndb_id: "v11".to_string(),
            engine: "Kirikiri / KAG".to_string(),
            developer: "TYPE-MOON".to_string(),
            publisher: String::new(),
            release_date: "2004-01-30".to_string(),
            genre: "Fantasy".to_string(),
            rating: "M".to_string(),
            description: String::new(),
            status: "Playing".to_string(),
            progress: 25,
            thumbnail: String::new(),
            exe_path: saves_a.join("Game.exe").to_string_lossy().to_string(),
            proton_prefix: String::new(),
            save_paths: vec!["<game_dir>".to_string()],
            playtime_seconds: 0,
            last_played_at: None,
            head_snapshot_id: None,
            sync_status: "Local Only".to_string(),
            exe_exists: false,
        };
        db_a.upsert_game(&game_a).unwrap();

        let res1 = sync_game(&db_a, &vault_a, "vn-1", None).await.unwrap();
        assert_eq!(res1.action, "pushed");

        // Second sync without changes -> up_to_date
        let res2 = sync_game(&db_a, &vault_a, "vn-1", None).await.unwrap();
        assert_eq!(res2.action, "up_to_date");

        // Test library catalog backup & restore to second DB
        sync_library_catalog(&db_a, Some("push")).await.unwrap();

        let db_b = Database::open_in_memory().unwrap();
        db_b.upsert_storage_profile(&profile).unwrap();
        let lib_res = sync_library_catalog(&db_b, Some("pull")).await.unwrap();
        assert_eq!(lib_res.added, 1);
        assert_eq!(db_b.list_games().unwrap().len(), 1);

        // Device B: same game id, its own save dir, no saves yet -> pulls A's saves.
        let vault_b = tmp.path().join("vault_b");
        let saves_b = tmp.path().join("saves_b");
        fs::create_dir_all(&saves_b).unwrap();
        let mut game_b = game_a.clone();
        game_b.exe_path = saves_b.join("Game.exe").to_string_lossy().to_string();
        db_b.upsert_game(&game_b).unwrap();
        let sync_b = || sync_game(&db_b, &vault_b, "vn-1", None);
        let sync_a = || sync_game(&db_a, &vault_a, "vn-1", None);
        assert_eq!(sync_b().await.unwrap().action, "pulled");
        assert_eq!(fs::read(saves_b.join("save1.dat")).unwrap(), b"state_v1");

        // A pushes several snapshots while B is idle; one B sync must fast-forward, not conflict.
        for v in ["state_v2", "state_v3"] {
            fs::write(saves_a.join("save1.dat"), v).unwrap();
            assert_eq!(sync_a().await.unwrap().action, "pushed");
        }
        assert_eq!(sync_b().await.unwrap().action, "pulled");
        assert_eq!(fs::read(saves_b.join("save1.dat")).unwrap(), b"state_v3");

        // B plays and pushes; A fast-forwards.
        fs::write(saves_b.join("save1.dat"), b"state_v4").unwrap();
        assert_eq!(sync_b().await.unwrap().action, "pushed");
        assert_eq!(sync_a().await.unwrap().action, "pulled");
        assert_eq!(fs::read(saves_a.join("save1.dat")).unwrap(), b"state_v4");

        // A restores an older cloud save; the next sync pushes it instead of pulling v4 back.
        let history = list_cloud_snapshots(&db_a, "vn-1").await.unwrap();
        assert_eq!(history.len(), 4);
        let v2 = history[2].head_snapshot_id.clone();
        restore_cloud_snapshot(&db_a, &vault_a, "vn-1", &v2)
            .await
            .unwrap();
        assert_eq!(fs::read(saves_a.join("save1.dat")).unwrap(), b"state_v2");
        assert_eq!(sync_a().await.unwrap().action, "pushed");
        assert_eq!(sync_b().await.unwrap().action, "pulled");
        assert_eq!(fs::read(saves_b.join("save1.dat")).unwrap(), b"state_v2");

        delete_cloud_game_data(&db_a, "vn-1").await.unwrap();
        assert!(list_cloud_snapshots(&db_a, "vn-1")
            .await
            .unwrap()
            .is_empty());
    }
}
