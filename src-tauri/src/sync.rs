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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncOutcome {
    pub action: String, // "up_to_date", "pushed", "pulled", "conflict"
    pub message: String,
    pub local_snapshot: Option<SnapshotRecord>,
    pub remote_head: Option<RemoteGameHead>,
}

pub fn build_operator(profile: &StorageProfile) -> Result<Operator, String> {
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
    let op = build_operator(profile)?;
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
    let profile = db
        .get_active_storage_profile()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No active storage profile configured in Settings".to_string())?;

    let op = build_operator(&profile)?;

    // Check if local save files changed since last local snapshot
    let (current_tree_hash, current_files) = compute_current_save_state(&game)?;
    let mut local_head = match &game.head_snapshot_id {
        Some(id) => db.get_snapshot(id).map_err(|e| e.to_string())?,
        None => None,
    };

    let local_dirty = match &local_head {
        Some(snap) => snap.tree_hash != current_tree_hash,
        None => !current_files.is_empty(),
    };

    if local_dirty {
        let (snap_record, _) = create_snapshot(
            &game,
            vault_dir,
            &settings.device_id,
            &settings.device_name,
            "Auto-checkpoint before sync",
        )?;
        db.insert_snapshot(&snap_record).map_err(|e| e.to_string())?;
        game.head_snapshot_id = Some(snap_record.id.clone());
        local_head = Some(snap_record);
    }

    let head_path = format!("games/{}/head.json", game.id);
    let remote_head: Option<RemoteGameHead> = match op.read(&head_path).await {
        Ok(bytes) => serde_json::from_slice(&bytes.to_vec()).ok(),
        Err(_) => None,
    };

    // Explicit resolution overrides ("push" or "pull")
    if force_direction == Some("push") {
        let snap = local_head.ok_or_else(|| "No local saves found to push".to_string())?;
        let head = push_snapshot_to_remote(&op, &game, &snap).await?;
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
            let head = push_snapshot_to_remote(&op, &game, l_snap).await?;
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

            // Fast-forward push: local snapshot descends directly from remote head
            if l_snap.parent_id.as_deref() == Some(r_head.head_snapshot_id.as_str()) {
                let head = push_snapshot_to_remote(&op, &game, l_snap).await?;
                game.sync_status = "Synced".to_string();
                db.upsert_game(&game).map_err(|e| e.to_string())?;
                return Ok(SyncOutcome {
                    action: "pushed".to_string(),
                    message: "Fast-forward pushed newer local saves".to_string(),
                    local_snapshot: Some(l_snap.clone()),
                    remote_head: Some(head),
                });
            }

            // Fast-forward pull: remote head descends directly from local head
            if r_head.parent_snapshot_id.as_deref() == Some(l_snap.id.as_str()) {
                let snap = pull_snapshot_from_remote(&op, &mut game, r_head, vault_dir, db).await?;
                return Ok(SyncOutcome {
                    action: "pulled".to_string(),
                    message: format!("Fast-forward pulled newer saves from {}", r_head.device_name),
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

async fn push_snapshot_to_remote(
    op: &Operator,
    game: &Game,
    snap: &SnapshotRecord,
) -> Result<RemoteGameHead, String> {
    let archive_bytes =
        fs::read(&snap.local_archive_path).map_err(|e| format!("Failed reading local archive: {}", e))?;
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
    };
    let head_bytes = serde_json::to_vec_pretty(&head).map_err(|e| e.to_string())?;
    let head_path = format!("games/{}/head.json", game.id);
    op.write(&head_path, head_bytes)
        .await
        .map_err(|e| format!("Failed updating remote head.json: {}", e))?;

    Ok(head)
}

async fn pull_snapshot_from_remote(
    op: &Operator,
    game: &mut Game,
    r_head: &RemoteGameHead,
    vault_dir: &Path,
    db: &Database,
) -> Result<SnapshotRecord, String> {
    let remote_archive_path =
        format!("games/{}/snapshots/{}.tar.zst", game.id, r_head.head_snapshot_id);
    let data = op
        .read(&remote_archive_path)
        .await
        .map_err(|e| format!("Failed downloading remote snapshot: {}", e))?;

    let game_vault = vault_dir.join(&game.id);
    fs::create_dir_all(&game_vault).map_err(|e| e.to_string())?;
    let local_archive_path = game_vault.join(format!("{}.tar.zst", r_head.head_snapshot_id));
    fs::write(&local_archive_path, data.to_vec()).map_err(|e| e.to_string())?;

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

    db.insert_snapshot(&snap_record).map_err(|e| e.to_string())?;
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

/// Checks all games in the library for paths belonging to a different OS user account
/// or residing in `settings.game_library_path`, auto-remapping them when the target exists.
pub fn heal_library_paths(db: &Database) -> Result<LibrarySyncOutcome, String> {
    let settings = db.get_settings().map_err(|e| e.to_string())?;
    let games = db.list_games().map_err(|e| e.to_string())?;
    let mut remapped_paths = 0usize;
    let mut missing_exe_count = 0usize;

    for mut g in games.iter().cloned() {
        let mut changed = false;
        let (resolved_exe, exists, exe_remapped) =
            resolve_or_remap_exe_path(&g.exe_path, &settings.game_library_path);
        if exe_remapped && resolved_exe != g.exe_path {
            g.exe_path = resolved_exe;
            remapped_paths += 1;
            changed = true;
        }
        if !exists {
            missing_exe_count += 1;
        }

        let new_saves: Vec<String> = g
            .save_paths
            .iter()
            .map(|p| to_canonical_path(p, &g.exe_path, &g.proton_prefix))
            .collect();
        if new_saves != g.save_paths {
            g.save_paths = new_saves;
            remapped_paths += 1;
            changed = true;
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
    let total_ReMap = remapped_paths + heal_res.remapped_paths;
    Ok(LibrarySyncOutcome {
        added,
        updated,
        remapped_paths: total_ReMap,
        missing_exe_count: heal_res.missing_exe_count,
        total_games: heal_res.total_games,
        message: format!(
            "Library synced: {} added, {} updated, {} user paths remapped ({} need manual folder selection)",
            added, updated, total_ReMap, heal_res.missing_exe_count
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
    let op = build_operator(&profile)?;
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
        let tmp = tempfile::tempdir().unwrap();
        let remote_dir = tmp.path().join("remote_cloud");
        let vault_a = tmp.path().join("vault_a");
        let saves_a = tmp.path().join("saves_a");
        fs::create_dir_all(&saves_a).unwrap();
        fs::write(saves_a.join("save1.dat"), b"state_v1").unwrap();

        let db_a = Database::open_in_memory().unwrap();
        let profile = StorageProfile {
            id: "local-test".to_string(),
            name: "Local Test Cloud".to_string(),
            provider_type: "local_fs".to_string(),
            endpoint: remote_dir.to_string_lossy().to_string(),
            bucket: String::new(),
            region: String::new(),
            root_prefix: String::new(),
            access_key: String::new(),
            secret_key: String::new(),
            is_active: true,
        };
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
    }
}
