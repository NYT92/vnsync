pub mod db;
pub mod metadata;
pub mod resolver;
pub mod snapshot;
pub mod sync;

use db::{AppSettings, Database, Game, SnapshotRecord, StorageProfile};
use metadata::{search_vndb, VndbResult};
use resolver::{
    detect_game_from_exe_with_db, get_active_save_locations, locate_game_executable,
    parse_save_locations_json, resolve_or_remap_exe_path, resolve_save_path, to_canonical_path,
    DetectionResult, SaveLocationDbStatus, DEFAULT_SAVE_LOCATIONS_JSON,
};
use snapshot::{create_snapshot, restore_snapshot_archive};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use sync::{
    build_library_catalog, heal_library_paths, merge_library_catalog, parse_library_catalog_json,
    sync_game, sync_library_catalog, test_storage_connection, LibrarySyncOutcome, SyncOutcome,
};
use tauri::{AppHandle, Emitter, Manager, State};

pub struct AppState {
    pub db: Arc<Database>,
    pub app_data_dir: PathBuf,
    pub vault_dir: PathBuf,
    pub running_games: Arc<Mutex<HashSet<String>>>,
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
    state.db.insert_snapshot(&record).map_err(|e| e.to_string())?;
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
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;

    if settings.backup_before_sync {
        let _ = create_snapshot(
            &game,
            &state.vault_dir,
            &settings.device_id,
            &settings.device_name,
            "Pre-restore safety backup",
        )
        .and_then(|(rec, _)| state.db.insert_snapshot(&rec).map_err(|e| e.to_string()));
    }

    let snap = state
        .db
        .get_snapshot(&snapshot_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Snapshot not found".to_string())?;

    let count = restore_snapshot_archive(&game, Path::new(&snap.local_archive_path))?;
    game.head_snapshot_id = Some(snap.id);
    state.db.upsert_game(&game).map_err(|e| e.to_string())?;
    Ok(count)
}

#[tauri::command]
async fn sync_game_now(
    state: State<'_, AppState>,
    game_id: String,
    force_direction: Option<String>,
) -> Result<SyncOutcome, String> {
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
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    state
        .db
        .save_settings(&settings)
        .map_err(|e| e.to_string())?;
    Ok(settings)
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
        return Err("No executable path configured. Click 'Locate Game' to select the game folder or .exe.".to_string());
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

            app.manage(AppState {
                db: Arc::new(db),
                app_data_dir,
                vault_dir,
                running_games: Arc::new(Mutex::new(HashSet::new())),
            });

            #[cfg(desktop)]
            {
                use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
                use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

                let show_item = MenuItem::with_id(app, "show", "Show VNSync", true, None::<&str>)?;
                let hide_item = MenuItem::with_id(app, "hide", "Hide to Tray", true, None::<&str>)?;
                let separator = PredefinedMenuItem::separator(app)?;
                let quit_item = MenuItem::with_id(app, "quit", "Exit VNSync", true, None::<&str>)?;

                let menu = Menu::with_items(app, &[&show_item, &hide_item, &separator, &quit_item])?;

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
        ])
        .run(tauri::generate_context!())
        .expect("error while running VNSync tauri application");
}
