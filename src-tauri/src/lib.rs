pub mod db;
pub mod lock;
pub mod metadata;
pub mod resolver;
pub mod snapshot;
pub mod sync;

use db::{AppSettings, Database, Game, SnapshotRecord, StorageProfile};
use lock::LockState;
use metadata::{search_vndb, VndbResult};
use resolver::{
    detect_game_from_exe_with_db, get_active_save_locations, locate_game_executable,
    parse_save_locations_json, resolve_or_remap_exe_path, resolve_save_path, to_canonical_path,
    DetectionResult, SaveLocationDbStatus, DEFAULT_SAVE_LOCATIONS_JSON,
};
use serde::Serialize;
use snapshot::create_snapshot;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sync::{
    build_library_catalog, delete_cloud_game_data, heal_library_paths, list_cloud_snapshots,
    merge_library_catalog, parse_library_catalog_json, restore_as_new_snapshot,
    restore_cloud_snapshot, server_list_devices, server_revoke_device, server_sign_in, sync_game,
    sync_library_catalog, test_storage_connection, LibrarySyncOutcome, RemoteGameHead,
    ServerDevice, SyncOutcome,
};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;

/// Passed by the OS autostart entry, so the app can start hidden in the tray.
const AUTOSTART_ARG: &str = "--autostart";

pub struct AppState {
    pub db: Arc<Database>,
    pub app_data_dir: PathBuf,
    pub vault_dir: PathBuf,
    pub running_games: Arc<Mutex<HashSet<String>>>,
    pub lock: Mutex<LockState>,
    /// Serializes manual, pre-launch and background syncs.
    // ponytail: one global lock; a long background pass delays a manual sync. Per-game locks if that hurts.
    pub sync_lock: tokio::sync::Mutex<()>,
}

#[tauri::command]
fn get_games(state: State<'_, AppState>) -> Result<Vec<Game>, String> {
    let _ = heal_library_paths(&state.db);
    state.db.list_games().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_game(state: State<'_, AppState>, id: String) -> Result<Option<Game>, String> {
    let _ = heal_library_paths(&state.db);
    state.db.get_game(&id).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_game(state: State<'_, AppState>, mut game: Game) -> Result<Game, String> {
    if game.id.trim().is_empty() {
        game.id = uuid::Uuid::now_v7().to_string();
    }
    let lib_root = state
        .db
        .get_settings()
        .map(|s| s.game_library_path)
        .unwrap_or_default();
    let (resolved_exe, exists, _) = resolve_or_remap_exe_path(&game.exe_path, &lib_root);
    game.exe_path = resolved_exe;
    game.exe_exists = exists;
    game.save_paths = game
        .save_paths
        .iter()
        .map(|p| to_canonical_path(p, &game.exe_path, &game.proton_prefix))
        .collect();
    state.db.upsert_game(&game).map_err(|e| e.to_string())?;
    Ok(game)
}

#[tauri::command]
fn relocate_game_path(
    state: State<'_, AppState>,
    game_id: String,
    selected_path: String,
) -> Result<Game, String> {
    let mut game = state
        .db
        .get_game(&game_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Game not found".to_string())?;
    let exe_path = locate_game_executable(&selected_path, &game.exe_path)?;
    game.exe_path = exe_path;
    game.exe_exists = true;

    if game.save_paths.is_empty() {
        let db = get_active_save_locations(&state.app_data_dir);
        let det = detect_game_from_exe_with_db(&game.exe_path, &game.proton_prefix, &db.entries);
        game.save_paths = det.canonical_save_paths;
        if game.engine.is_empty() || game.engine == "Custom / Generic" {
            game.engine = det.engine;
        }
    } else {
        game.save_paths = game
            .save_paths
            .iter()
            .map(|p| to_canonical_path(p, &game.exe_path, &game.proton_prefix))
            .collect();
    }

    state.db.upsert_game(&game).map_err(|e| e.to_string())?;
    Ok(game)
}

#[tauri::command]
fn heal_game_paths(state: State<'_, AppState>) -> Result<LibrarySyncOutcome, String> {
    heal_library_paths(&state.db)
}

#[tauri::command]
async fn sync_library_now(
    state: State<'_, AppState>,
    direction: Option<String>,
) -> Result<LibrarySyncOutcome, String> {
    let _guard = state.sync_lock.lock().await;
    sync_library_catalog(&state.db, direction.as_deref()).await
}

#[tauri::command]
fn export_library_backup(state: State<'_, AppState>, file_path: String) -> Result<String, String> {
    let catalog = build_library_catalog(&state.db)?;
    let count = catalog.games.len();
    let json = serde_json::to_string_pretty(&catalog).map_err(|e| e.to_string())?;
    std::fs::write(&file_path, json).map_err(|e| format!("Failed writing backup file: {}", e))?;
    Ok(format!("Exported {} games to {}", count, file_path))
}

#[tauri::command]
fn import_library_backup(
    state: State<'_, AppState>,
    file_path: String,
) -> Result<LibrarySyncOutcome, String> {
    let raw = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed reading backup file: {}", e))?;
    let incoming = parse_library_catalog_json(&raw)?;
    merge_library_catalog(&state.db, &incoming)
}

#[tauri::command]
fn remove_game(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.db.delete_game(&id).map_err(|e| e.to_string())
}

#[tauri::command]
fn detect_game_engine(
    state: State<'_, AppState>,
    exe_path: String,
    proton_prefix: String,
) -> DetectionResult {
    let db = get_active_save_locations(&state.app_data_dir);
    detect_game_from_exe_with_db(&exe_path, &proton_prefix, &db.entries)
}

#[tauri::command]
fn get_save_location_db(state: State<'_, AppState>) -> SaveLocationDbStatus {
    get_active_save_locations(&state.app_data_dir)
}

#[tauri::command]
fn load_custom_save_locations(
    state: State<'_, AppState>,
    file_path: String,
) -> Result<SaveLocationDbStatus, String> {
    let raw = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed reading JSON file: {}", e))?;
    let entries = parse_save_locations_json(&raw)?;
    let custom_dest = state.app_data_dir.join("custom_save_locations.json");
    std::fs::write(&custom_dest, &raw)
        .map_err(|e| format!("Failed saving custom database: {}", e))?;
    Ok(SaveLocationDbStatus {
        source: "custom".to_string(),
        entries,
    })
}

#[tauri::command]
fn reset_save_locations(state: State<'_, AppState>) -> Result<SaveLocationDbStatus, String> {
    let custom_dest = state.app_data_dir.join("custom_save_locations.json");
    if custom_dest.exists() {
        let _ = std::fs::remove_file(&custom_dest);
    }
    Ok(get_active_save_locations(&state.app_data_dir))
}

#[tauri::command]
fn preview_resolved_paths(
    canonical_paths: Vec<String>,
    exe_path: String,
    proton_prefix: String,
) -> Vec<String> {
    canonical_paths
        .iter()
        .map(|p| {
            resolve_save_path(p, &exe_path, &proton_prefix)
                .to_string_lossy()
                .to_string()
        })
        .collect()
}

#[tauri::command]
async fn search_vndb_metadata(query: String) -> Result<Vec<VndbResult>, String> {
    search_vndb(&query).await
}

#[tauri::command]
fn list_game_snapshots(
    state: State<'_, AppState>,
    game_id: String,
) -> Result<Vec<SnapshotRecord>, String> {
    state.db.list_snapshots(&game_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_manual_snapshot(
    state: State<'_, AppState>,
    game_id: String,
    note: String,
) -> Result<SnapshotRecord, String> {
    let mut game = state
        .db
        .get_game(&game_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Game not found".to_string())?;
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let label = if note.trim().is_empty() {
        "Manual checkpoint".to_string()
    } else {
        note
    };
    let (record, _) = create_snapshot(
        &game,
        &state.vault_dir,
        &settings.device_id,
        &settings.device_name,
        &label,
    )?;
    state
        .db
        .insert_snapshot(&record)
        .map_err(|e| e.to_string())?;
    game.head_snapshot_id = Some(record.id.clone());
    state.db.upsert_game(&game).map_err(|e| e.to_string())?;
    Ok(record)
}

#[tauri::command]
fn restore_game_snapshot(
    state: State<'_, AppState>,
    game_id: String,
    snapshot_id: String,
) -> Result<usize, String> {
    let mut game = state
        .db
        .get_game(&game_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Game not found".to_string())?;
    let snap = state
        .db
        .get_snapshot(&snapshot_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Snapshot not found".to_string())?;

    let note = format!("Restored: {}", snap.note);
    let (count, _) = restore_as_new_snapshot(
        &state.db,
        &state.vault_dir,
        &mut game,
        Path::new(&snap.local_archive_path),
        None,
        &note,
    )?;
    Ok(count)
}

/// Removes a snapshot's DB row, archive and manifest. The current head is kept.
fn delete_local_snapshot(db: &Database, snap: &SnapshotRecord) -> Result<u64, String> {
    let game = db.get_game(&snap.game_id).map_err(|e| e.to_string())?;
    if game.and_then(|g| g.head_snapshot_id).as_deref() == Some(snap.id.as_str()) {
        return Err("The current HEAD snapshot cannot be deleted".to_string());
    }
    let archive = Path::new(&snap.local_archive_path);
    let freed = std::fs::metadata(archive).map(|m| m.len()).unwrap_or(0);
    let _ = std::fs::remove_file(archive);
    let _ = std::fs::remove_file(archive.with_extension("").with_extension("json"));
    db.delete_snapshot(&snap.id).map_err(|e| e.to_string())?;
    Ok(freed)
}

#[tauri::command]
fn delete_game_snapshot(state: State<'_, AppState>, snapshot_id: String) -> Result<(), String> {
    let snap = state
        .db
        .get_snapshot(&snapshot_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Snapshot not found".to_string())?;
    delete_local_snapshot(&state.db, &snap).map(|_| ())
}

#[tauri::command]
fn set_snapshot_pinned(
    state: State<'_, AppState>,
    snapshot_id: String,
    pinned: bool,
) -> Result<(), String> {
    state
        .db
        .set_snapshot_pinned(&snapshot_id, pinned)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn export_game_snapshot(
    state: State<'_, AppState>,
    snapshot_id: String,
    dest_path: String,
) -> Result<(), String> {
    let snap = state
        .db
        .get_snapshot(&snapshot_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Snapshot not found".to_string())?;
    std::fs::copy(&snap.local_archive_path, &dest_path)
        .map(|_| ())
        .map_err(|e| format!("Export failed: {}", e))
}

#[tauri::command]
async fn list_game_cloud_snapshots(
    state: State<'_, AppState>,
    game_id: String,
) -> Result<Vec<RemoteGameHead>, String> {
    list_cloud_snapshots(&state.db, &game_id).await
}

#[tauri::command]
async fn restore_game_cloud_snapshot(
    state: State<'_, AppState>,
    game_id: String,
    snapshot_id: String,
) -> Result<usize, String> {
    restore_cloud_snapshot(&state.db, &state.vault_dir, &game_id, &snapshot_id)
        .await
        .map(|(count, _)| count)
}

#[tauri::command]
async fn delete_game_cloud_data(state: State<'_, AppState>, game_id: String) -> Result<(), String> {
    delete_cloud_game_data(&state.db, &game_id).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GameStorageUsage {
    game_id: String,
    title: String,
    snapshot_count: usize,
    pinned_count: usize,
    local_bytes: u64,
}

#[tauri::command]
fn get_storage_usage(state: State<'_, AppState>) -> Result<Vec<GameStorageUsage>, String> {
    let games = state.db.list_games().map_err(|e| e.to_string())?;
    games
        .into_iter()
        .map(|g| {
            let snaps = state.db.list_snapshots(&g.id).map_err(|e| e.to_string())?;
            Ok(GameStorageUsage {
                snapshot_count: snaps.len(),
                pinned_count: snaps.iter().filter(|s| s.pinned).count(),
                local_bytes: snaps
                    .iter()
                    .filter_map(|s| std::fs::metadata(&s.local_archive_path).ok())
                    .map(|m| m.len())
                    .sum(),
                game_id: g.id,
                title: g.title,
            })
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PruneOutcome {
    deleted: usize,
    freed_bytes: u64,
}

/// Keeps the newest `keep` snapshots per game plus pinned ones and the HEAD; deletes the rest.
/// `game_id` None prunes every game.
#[tauri::command]
fn prune_local_snapshots(
    state: State<'_, AppState>,
    game_id: Option<String>,
    keep: usize,
) -> Result<PruneOutcome, String> {
    let ids = match game_id {
        Some(id) => vec![id],
        None => state
            .db
            .list_games()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|g| g.id)
            .collect(),
    };
    let mut out = PruneOutcome {
        deleted: 0,
        freed_bytes: 0,
    };
    for id in ids {
        let head = state
            .db
            .get_game(&id)
            .map_err(|e| e.to_string())?
            .and_then(|g| g.head_snapshot_id);
        // list_snapshots is newest first
        let snaps = state.db.list_snapshots(&id).map_err(|e| e.to_string())?;
        for snap in snaps.iter().skip(keep) {
            if snap.pinned || head.as_deref() == Some(snap.id.as_str()) {
                continue;
            }
            out.freed_bytes += delete_local_snapshot(&state.db, snap)?;
            out.deleted += 1;
        }
    }
    Ok(out)
}

#[tauri::command]
async fn sync_game_now(
    state: State<'_, AppState>,
    game_id: String,
    force_direction: Option<String>,
) -> Result<SyncOutcome, String> {
    let _guard = state.sync_lock.lock().await;
    sync_game(
        &state.db,
        &state.vault_dir,
        &game_id,
        force_direction.as_deref(),
    )
    .await
}

#[tauri::command]
fn get_storage_profiles(state: State<'_, AppState>) -> Result<Vec<StorageProfile>, String> {
    state.db.list_storage_profiles().map_err(|e| e.to_string())
}

#[tauri::command]
fn save_storage_profile(
    state: State<'_, AppState>,
    mut profile: StorageProfile,
) -> Result<StorageProfile, String> {
    if profile.id.trim().is_empty() {
        profile.id = uuid::Uuid::new_v4().to_string();
    }
    state
        .db
        .upsert_storage_profile(&profile)
        .map_err(|e| e.to_string())?;
    Ok(profile)
}

#[tauri::command]
async fn test_storage_profile(profile: StorageProfile) -> Result<String, String> {
    test_storage_connection(&profile).await
}

#[tauri::command]
fn get_app_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    state.db.get_settings().map_err(|e| e.to_string())
}

#[tauri::command]
fn save_app_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    state
        .db
        .save_settings(&settings)
        .map_err(|e| e.to_string())?;
    let autostart = app.autolaunch();
    if autostart.is_enabled().unwrap_or(false) != settings.launch_on_startup {
        if settings.launch_on_startup {
            autostart.enable()
        } else {
            autostart.disable()
        }
        .map_err(|e| format!("Settings saved, but launch on startup failed: {}", e))?;
    }
    Ok(settings)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LockStatus {
    enabled: bool,
    locked: bool,
    idle_minutes: u32,
    biometric: bool,
}

#[tauri::command]
fn get_lock_status(state: State<'_, AppState>) -> Result<LockStatus, String> {
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    Ok(LockStatus {
        enabled: !state.db.get_lock_pin_hash().is_empty(),
        locked: state.lock.lock().map_err(|e| e.to_string())?.locked,
        idle_minutes: settings.lock_idle_minutes,
        biometric: settings.lock_biometric,
    })
}

#[tauri::command]
async fn is_biometric_available() -> bool {
    tokio::task::spawn_blocking(lock::biometric_available)
        .await
        .unwrap_or(false)
}

fn set_locked(app: &AppHandle, state: &AppState, locked: bool) -> Result<(), String> {
    state.lock.lock().map_err(|e| e.to_string())?.locked = locked;
    let _ = app.emit("lock:changed", locked);
    Ok(())
}

#[tauri::command]
fn lock_app(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state.db.get_lock_pin_hash().is_empty() {
        return Err("Set a PIN first".to_string());
    }
    set_locked(&app, &state, true)
}

#[tauri::command]
fn unlock_app(app: AppHandle, state: State<'_, AppState>, pin: String) -> Result<(), String> {
    let stored = state.db.get_lock_pin_hash();
    state
        .lock
        .lock()
        .map_err(|e| e.to_string())?
        .try_unlock(&stored, &pin)?;
    let _ = app.emit("lock:changed", false);
    Ok(())
}

#[tauri::command]
async fn unlock_app_biometric(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    if !settings.lock_biometric {
        return Err("Biometric unlock is turned off".to_string());
    }
    tokio::task::spawn_blocking(|| lock::biometric_verify("Unlock VNSync"))
        .await
        .map_err(|e| e.to_string())??;
    set_locked(&app, &state, false)
}

/// Sets or changes the PIN. Changing an existing PIN needs the current one.
#[tauri::command]
fn set_lock_pin(
    state: State<'_, AppState>,
    current_pin: String,
    new_pin: String,
) -> Result<(), String> {
    let stored = state.db.get_lock_pin_hash();
    if !stored.is_empty() && !lock::verify_pin(&stored, &current_pin) {
        return Err("Current PIN is wrong".to_string());
    }
    lock::validate_pin(&new_pin)?;
    state
        .db
        .set_lock_pin_hash(&lock::hash_pin(&new_pin))
        .map_err(|e| e.to_string())
}

/// Turns the app lock off.
#[tauri::command]
fn clear_lock_pin(state: State<'_, AppState>, current_pin: String) -> Result<(), String> {
    let stored = state.db.get_lock_pin_hash();
    if !stored.is_empty() && !lock::verify_pin(&stored, &current_pin) {
        return Err("Current PIN is wrong".to_string());
    }
    state.db.set_lock_pin_hash("").map_err(|e| e.to_string())
}

fn active_server_profile(db: &Database) -> Result<StorageProfile, String> {
    db.get_active_storage_profile()
        .map_err(|e| e.to_string())?
        .filter(|p| p.provider_type == "vnsync_server")
        .ok_or_else(|| "Active storage profile is not a VNSync server".to_string())
}

/// Signs this PC in to a VNSync server and saves the device token as the active profile.
#[tauri::command]
async fn server_sign_in_device(
    state: State<'_, AppState>,
    mut profile: StorageProfile,
    password: String,
) -> Result<StorageProfile, String> {
    if profile.provider_type != "vnsync_server" {
        return Err("Profile is not a VNSync server".to_string());
    }
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    profile.secret_key = server_sign_in(
        &profile,
        &password,
        &settings.device_id,
        &settings.device_name,
    )
    .await?;
    if profile.id.trim().is_empty() {
        profile.id = uuid::Uuid::new_v4().to_string();
    }
    profile.is_active = true;
    state
        .db
        .upsert_storage_profile(&profile)
        .map_err(|e| e.to_string())?;
    Ok(profile)
}

/// Signs this PC out: revokes its token on the server (best effort) and forgets it locally.
#[tauri::command]
async fn server_sign_out_device(state: State<'_, AppState>) -> Result<StorageProfile, String> {
    let mut profile = active_server_profile(&state.db)?;
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let _ = server_revoke_device(&profile, &settings.device_id).await;
    profile.secret_key.clear();
    state
        .db
        .upsert_storage_profile(&profile)
        .map_err(|e| e.to_string())?;
    Ok(profile)
}

#[tauri::command]
async fn server_get_devices(state: State<'_, AppState>) -> Result<Vec<ServerDevice>, String> {
    server_list_devices(&active_server_profile(&state.db)?).await
}

#[tauri::command]
async fn server_remove_device(state: State<'_, AppState>, device_id: String) -> Result<(), String> {
    server_revoke_device(&active_server_profile(&state.db)?, &device_id).await
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BackgroundSyncReport {
    changed: usize,
    conflicts: Vec<String>,
    errors: Vec<String>,
}

/// Syncs the library catalog and every game that is not running.
async fn background_sync_pass(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !matches!(state.db.get_active_storage_profile(), Ok(Some(_))) {
        return;
    }
    let _guard = state.sync_lock.lock().await;
    let mut report = BackgroundSyncReport {
        changed: 0,
        conflicts: Vec::new(),
        errors: Vec::new(),
    };
    if let Err(e) = sync_library_catalog(&state.db, None).await {
        report.errors.push(format!("Library: {}", e));
    }
    let running = state
        .running_games
        .lock()
        .map(|r| r.clone())
        .unwrap_or_default();
    for game in state.db.list_games().unwrap_or_default() {
        // A pull rewrites save files, so never touch a game that is open.
        if running.contains(&game.id) || game.save_paths.is_empty() {
            continue;
        }
        match sync_game(&state.db, &state.vault_dir, &game.id, None).await {
            // Report a conflict once, not on every pass.
            Ok(o) if o.action == "conflict" => {
                if game.sync_status != "Conflict" {
                    report.conflicts.push(game.title);
                }
            }
            Ok(o) if o.action != "up_to_date" => report.changed += 1,
            Ok(_) => {}
            Err(e) => report.errors.push(format!("{}: {}", game.title, e)),
        }
    }
    if !report.conflicts.is_empty() {
        use tauri_plugin_notification::NotificationExt;
        let _ = app
            .notification()
            .builder()
            .title("VNSync: save conflict")
            .body(format!(
                "Open VNSync to choose which saves to keep: {}",
                report.conflicts.join(", ")
            ))
            .show();
    }
    let _ = app.emit("sync:background", report);
}

/// Runs a sync when the app opens (if enabled), then every `background_sync_minutes`.
/// Settings are re-read every tick, so changes apply without a restart.
fn spawn_background_sync(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let sync_on_open = app
            .state::<AppState>()
            .db
            .get_settings()
            .map(|s| s.sync_on_app_open)
            .unwrap_or(false);
        if sync_on_open {
            background_sync_pass(&app).await;
        }
        let mut last_run = Instant::now();
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let minutes = app
                .state::<AppState>()
                .db
                .get_settings()
                .map(|s| s.background_sync_minutes)
                .unwrap_or(0);
            if minutes > 0 && last_run.elapsed() >= Duration::from_secs(u64::from(minutes) * 60) {
                background_sync_pass(&app).await;
                last_run = Instant::now();
            }
        }
    });
}

#[tauri::command]
fn get_running_games(state: State<'_, AppState>) -> Vec<String> {
    state
        .running_games
        .lock()
        .map(|set| set.iter().cloned().collect())
        .unwrap_or_default()
}

#[tauri::command]
async fn launch_game(
    app: AppHandle,
    state: State<'_, AppState>,
    game_id: String,
) -> Result<String, String> {
    let _ = heal_library_paths(&state.db);
    let game = state
        .db
        .get_game(&game_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Game not found".to_string())?;

    if game.exe_path.trim().is_empty() {
        return Err(
            "No executable path configured. Click 'Locate Game' to select the game folder or .exe."
                .to_string(),
        );
    }
    let exe = PathBuf::from(&game.exe_path);
    if !exe.exists() {
        return Err(format!(
            "Executable not found at '{}'. Click 'Locate Game' to select the game folder or .exe.",
            game.exe_path
        ));
    }

    // Atomically check and mark the game as running to prevent double-launches
    {
        let mut running = state
            .running_games
            .lock()
            .map_err(|_| "Failed to lock running games state".to_string())?;
        if running.contains(&game_id) {
            return Err(format!("{} is already running!", game.title));
        }
        running.insert(game_id.clone());
    }

    let _ = app.emit("game:started", &game_id);

    let settings = match state.db.get_settings() {
        Ok(s) => s,
        Err(e) => {
            if let Ok(mut running) = state.running_games.lock() {
                running.remove(&game_id);
            }
            let _ = app.emit("game:exited", &game_id);
            return Err(e.to_string());
        }
    };

    // Optional pre-launch sync
    if settings.auto_sync_on_launch {
        if let Ok(Some(_)) = state.db.get_active_storage_profile() {
            let _guard = state.sync_lock.lock().await;
            let _ = sync_game(&state.db, &state.vault_dir, &game_id, None).await;
        }
    }

    let db = Arc::clone(&state.db);
    let running_games = Arc::clone(&state.running_games);
    let vault_dir = state.vault_dir.clone();
    let game_id_clone = game.id.clone();
    let work_dir = exe
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    tokio::task::spawn_blocking(move || {
        let start = Instant::now();

        let status = Command::new(&exe).current_dir(&work_dir).status();

        let elapsed_secs = start.elapsed().as_secs() as i64;
        if let Ok(Some(mut g)) = db.get_game(&game_id_clone) {
            g.playtime_seconds += elapsed_secs.max(1);
            g.last_played_at = Some(chrono::Utc::now().to_rfc3339());
            if g.status == "Not Started" {
                g.status = "Playing".to_string();
            }
            let _ = db.upsert_game(&g);

            if settings.auto_sync_on_exit {
                if let Ok((snap, _)) = create_snapshot(
                    &g,
                    &vault_dir,
                    &settings.device_id,
                    &settings.device_name,
                    "Auto-save on game exit",
                ) {
                    let _ = db.insert_snapshot(&snap);
                }
            }
        }

        if let Ok(mut running) = running_games.lock() {
            running.remove(&game_id_clone);
        }
        let _ = app.emit("game:exited", &game_id_clone);
        status
    });

    Ok(format!("Launched {}", game.title))
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[tauri::command]
fn hide_to_tray(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_ARG]),
        ))
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from(".vnsync_data"));
            std::fs::create_dir_all(&app_data_dir)?;
            let db_path = app_data_dir.join("vnsync.sqlite");
            let vault_dir = app_data_dir.join("vault");
            std::fs::create_dir_all(&vault_dir)?;

            let db = Database::open(&db_path).expect("Failed to initialize SQLite database");

            // Seed default sample storage profile if none exists
            if let Ok(profiles) = db.list_storage_profiles() {
                if profiles.is_empty() {
                    let default_cloud = StorageProfile {
                        id: "default-local-cloud".to_string(),
                        name: "Local Sync Folder".to_string(),
                        provider_type: "local_fs".to_string(),
                        endpoint: app_data_dir
                            .join("cloud_mirror")
                            .to_string_lossy()
                            .to_string(),
                        bucket: String::new(),
                        region: "auto".to_string(),
                        root_prefix: String::new(),
                        access_key: String::new(),
                        secret_key: String::new(),
                        is_active: true,
                    };
                    let _ = db.upsert_storage_profile(&default_cloud);
                }
            }

            let default_json_copy = app_data_dir.join("default_save_locations.json");
            let _ = std::fs::write(&default_json_copy, DEFAULT_SAVE_LOCATIONS_JSON);

            let locked_at_start = !db.get_lock_pin_hash().is_empty();
            let start_hidden = std::env::args().any(|a| a == AUTOSTART_ARG)
                && db
                    .get_settings()
                    .map(|s| s.start_minimized)
                    .unwrap_or(false);

            app.manage(AppState {
                db: Arc::new(db),
                app_data_dir,
                vault_dir,
                running_games: Arc::new(Mutex::new(HashSet::new())),
                lock: Mutex::new(LockState::new(locked_at_start)),
                sync_lock: tokio::sync::Mutex::new(()),
            });

            // The window starts hidden (tauri.conf.json) so autostart can stay in the tray.
            if !start_hidden {
                show_main_window(app.handle());
            }
            spawn_background_sync(app.handle().clone());

            #[cfg(desktop)]
            {
                use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
                use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

                let show_item = MenuItem::with_id(app, "show", "Show VNSync", true, None::<&str>)?;
                let hide_item = MenuItem::with_id(app, "hide", "Hide to Tray", true, None::<&str>)?;
                let separator = PredefinedMenuItem::separator(app)?;
                let quit_item = MenuItem::with_id(app, "quit", "Exit VNSync", true, None::<&str>)?;

                let menu =
                    Menu::with_items(app, &[&show_item, &hide_item, &separator, &quit_item])?;

                let mut tray_builder = TrayIconBuilder::new()
                    .tooltip("VNSync")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "show" => show_main_window(app),
                        "hide" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.hide();
                            }
                        }
                        "quit" => app.exit(0),
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            show_main_window(tray.app_handle());
                        }
                    });

                if let Some(icon) = app.default_window_icon().cloned() {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.set_icon(icon.clone());
                    }
                    tray_builder = tray_builder.icon(icon);
                }

                let _ = tray_builder.build(app)?;
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(settings) = state.db.get_settings() {
                        if settings.close_to_tray {
                            api.prevent_close();
                            let _ = window.hide();
                        }
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_games,
            get_game,
            save_game,
            relocate_game_path,
            heal_game_paths,
            sync_library_now,
            export_library_backup,
            import_library_backup,
            remove_game,
            detect_game_engine,
            get_save_location_db,
            load_custom_save_locations,
            reset_save_locations,
            preview_resolved_paths,
            search_vndb_metadata,
            list_game_snapshots,
            create_manual_snapshot,
            restore_game_snapshot,
            delete_game_snapshot,
            set_snapshot_pinned,
            export_game_snapshot,
            list_game_cloud_snapshots,
            restore_game_cloud_snapshot,
            delete_game_cloud_data,
            get_storage_usage,
            prune_local_snapshots,
            sync_game_now,
            get_storage_profiles,
            save_storage_profile,
            test_storage_profile,
            get_app_settings,
            save_app_settings,
            get_running_games,
            launch_game,
            hide_to_tray,
            quit_app,
            get_lock_status,
            is_biometric_available,
            lock_app,
            unlock_app,
            unlock_app_biometric,
            set_lock_pin,
            clear_lock_pin,
            server_sign_in_device,
            server_sign_out_device,
            server_get_devices,
            server_remove_device,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VNSync tauri application");
}
