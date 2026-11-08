use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_SAVE_LOCATIONS_JSON: &str = include_str!("../data/save_locations.json");

fn default_db_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveLocationEntry {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub developer: String,
    #[serde(default)]
    pub engine: String,
    #[serde(default)]
    pub vndb_id: String,
    #[serde(default)]
    pub exe_names: Vec<String>,
    pub save_paths: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveLocationDatabase {
    #[serde(default = "default_db_version")]
    pub version: u32,
    #[serde(default)]
    pub description: String,
    pub entries: Vec<SaveLocationEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveLocationDbStatus {
    pub source: String,
    pub entries: Vec<SaveLocationEntry>,
}

pub fn parse_save_locations_json(raw: &str) -> Result<Vec<SaveLocationEntry>, String> {
    if let Ok(db) = serde_json::from_str::<SaveLocationDatabase>(raw) {
        return Ok(db.entries);
    }
    if let Ok(entries) = serde_json::from_str::<Vec<SaveLocationEntry>>(raw) {
        return Ok(entries);
    }
    Err(
        "Invalid save_locations JSON format: expected { entries: [...] } or an array of entries"
            .to_string(),
    )
}

pub fn get_active_save_locations(app_data_dir: &Path) -> SaveLocationDbStatus {
    let custom_file = app_data_dir.join("custom_save_locations.json");
    if custom_file.is_file() {
        if let Ok(raw) = fs::read_to_string(&custom_file) {
            if let Ok(entries) = parse_save_locations_json(&raw) {
                return SaveLocationDbStatus {
                    source: "custom".to_string(),
                    entries,
                };
            }
        }
    }
    let entries = parse_save_locations_json(DEFAULT_SAVE_LOCATIONS_JSON).unwrap_or_default();
    SaveLocationDbStatus {
        source: "default".to_string(),
        entries,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectionResult {
    pub engine: String,
    pub suggested_title: String,
    pub canonical_save_paths: Vec<String>,
    pub resolved_save_paths: Vec<String>,
    pub existing_save_files: usize,
}

fn infer_game_title(exe_path: &str) -> String {
    let exe = Path::new(exe_path);
    let game_dir = exe
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    exe.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| {
            if s.eq_ignore_ascii_case("game")
                || s.eq_ignore_ascii_case("start")
                || s.eq_ignore_ascii_case("launcher")
            {
                game_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(s)
                    .to_string()
            } else {
                s.to_string()
            }
        })
        .unwrap_or_else(|| "Unknown Game".to_string())
}

/// Expands portable path tokens (`<game_dir>`, `<win_appdata>`, `<win_locallow>`, `<win_documents>`, `<game_title>`)
/// into concrete host paths. When `proton_prefix` is set on Linux/Steam Deck, Windows tokens map inside
/// `<proton_prefix>/drive_c/users/steamuser/...`.
pub fn resolve_save_path(canonical_pattern: &str, exe_path: &str, proton_prefix: &str) -> PathBuf {
    let game_dir = Path::new(exe_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let title = infer_game_title(exe_path);
    let normalized = canonical_pattern
        .replace('\\', "/")
        .replace("<game_title>", &title);

    if let Some(rest) = normalized.strip_prefix("<game_dir>/") {
        return game_dir.join(rest);
    }
    if normalized == "<game_dir>" {
        return game_dir;
    }

    let use_proton = !proton_prefix.trim().is_empty();
    let proton_user_root = if use_proton {
        let pfx = Path::new(proton_prefix);
        if pfx.join("pfx").exists() {
            pfx.join("pfx/drive_c/users/steamuser")
        } else {
            pfx.join("drive_c/users/steamuser")
        }
    } else {
        PathBuf::new()
    };

    if let Some(rest) = normalized.strip_prefix("<win_appdata>/") {
        if use_proton {
            return proton_user_root.join("AppData/Roaming").join(rest);
        }
        if cfg!(target_os = "windows") {
            if let Some(roaming) = dirs::data_dir() {
                return roaming.join(rest);
            }
        } else if cfg!(target_os = "macos") {
            if let Some(home) = dirs::home_dir() {
                if let Some(renpy_sub) = rest.strip_prefix("RenPy/") {
                    return home.join("Library/RenPy").join(renpy_sub);
                }
                return home.join("Library/Application Support").join(rest);
            }
        } else if let Some(home) = dirs::home_dir() {
            if let Some(renpy_sub) = rest.strip_prefix("RenPy/") {
                return home.join(".renpy").join(renpy_sub);
            }
            if let Some(data) = dirs::data_dir() {
                return data.join(rest);
            }
        }
    }

    if let Some(rest) = normalized.strip_prefix("<win_locallow>/") {
        if use_proton {
            return proton_user_root.join("AppData/LocalLow").join(rest);
        }
        if let Some(home) = dirs::home_dir() {
            if cfg!(target_os = "windows") {
                return home.join("AppData/LocalLow").join(rest);
            } else if cfg!(target_os = "macos") {
                return home.join("Library/Application Support").join(rest);
            } else {
                return home.join(".config/unity3d").join(rest);
            }
        }
    }

    if let Some(rest) = normalized.strip_prefix("<win_documents>/") {
        if use_proton {
            return proton_user_root.join("Documents").join(rest);
        }
        if let Some(docs) = dirs::document_dir() {
            return docs.join(rest);
        }
    }

    let direct = PathBuf::from(canonical_pattern);
    if !direct.exists() {
        if let Some(remapped) = remap_os_user_path(canonical_pattern) {
            return remapped;
        }
    }
    direct
}

/// Extracts the path relative to a user's home directory from Windows (`C:/Users/<user>/...`),
/// Wine/Proton (`.../drive_c/users/<user>/...`), Linux (`/home/<user>/...`), or macOS (`/Users/<user>/...`).
pub fn extract_user_home_relative(raw_path: &str) -> Option<String> {
    let norm = raw_path.replace('\\', "/");
    let lower = norm.to_lowercase();

    if let Some(idx) = lower.find("/drive_c/users/") {
        let after_users = &norm[idx + "/drive_c/users/".len()..];
        if let Some(slash) = after_users.find('/') {
            let rel = &after_users[slash + 1..];
            if !rel.is_empty() {
                return Some(rel.to_string());
            }
        }
    }

    // Windows drive letter: e.g. "c:/users/<user>/..."
    let bytes = lower.as_bytes();
    if bytes.len() > 9 && bytes[0].is_ascii_alphabetic() && &lower[1..9] == ":/users/" {
        let after_users = &norm[9..];
        if let Some(slash) = after_users.find('/') {
            let rel = &after_users[slash + 1..];
            if !rel.is_empty() {
                return Some(rel.to_string());
            }
        }
    }

    for prefix in ["/home/", "/users/"] {
        if lower.starts_with(prefix) {
            let after_users = &norm[prefix.len()..];
            if let Some(slash) = after_users.find('/') {
                let rel = &after_users[slash + 1..];
                if !rel.is_empty() {
                    return Some(rel.to_string());
                }
            }
        }
    }

    None
}

/// Remaps a path containing another OS user's home folder (e.g. `C:\Users\OtherUser\...`)
/// to `target_home` (or the current OS user's home directory).
pub fn remap_os_user_path_with_home(raw_path: &str, target_home: &Path) -> Option<PathBuf> {
    let rel = extract_user_home_relative(raw_path)?;
    let candidate = target_home.join(rel);
    if candidate == Path::new(raw_path) {
        None
    } else {
        Some(candidate)
    }
}

pub fn remap_os_user_path(raw_path: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    remap_os_user_path_with_home(raw_path, &home)
}

/// Resolves or auto-remaps a game's executable path if it points to a different OS username
/// or resides inside `game_library_path`. Returns `(resolved_path, exists_on_disk, was_updated)`.
pub fn resolve_or_remap_exe_path(exe_path: &str, game_library_path: &str) -> (String, bool, bool) {
    let trimmed = exe_path.trim();
    if trimmed.is_empty() {
        return (String::new(), false, false);
    }
    let orig = Path::new(trimmed);
    if orig.is_file() {
        return (trimmed.to_string(), true, false);
    }

    // 1. Try remapping OS username (e.g. C:\Users\OldUser\... -> C:\Users\CurrentUser\...)
    if let Some(remapped) = remap_os_user_path(trimmed) {
        if remapped.is_file() {
            return (remapped.to_string_lossy().to_string(), true, true);
        }
    }

    // 2. Try locating same folder/exe inside configured default game_library_path
    let lib_trimmed = game_library_path.trim();
    if !lib_trimmed.is_empty() {
        let lib_root = Path::new(lib_trimmed);
        if let Some(exe_name) = orig.file_name() {
            if let Some(parent_folder) = orig.parent().and_then(|p| p.file_name()) {
                let candidate = lib_root.join(parent_folder).join(exe_name);
                if candidate.is_file() {
                    return (candidate.to_string_lossy().to_string(), true, true);
                }
            }
            let direct_candidate = lib_root.join(exe_name);
            if direct_candidate.is_file() {
                return (direct_candidate.to_string_lossy().to_string(), true, true);
            }
        }
    }

    (trimmed.to_string(), false, false)
}

/// Locates a game executable when the user selects either a `.exe` file or a game directory.
pub fn locate_game_executable(
    selected_path: &str,
    previous_exe_path: &str,
) -> Result<String, String> {
    let path = Path::new(selected_path.trim());
    if !path.exists() {
        return Err(format!("Selected path does not exist: {}", selected_path));
    }
    if path.is_file() {
        return Ok(path.to_string_lossy().to_string());
    }

    let prev_exe_name = previous_exe_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim();

    if !prev_exe_name.is_empty() {
        let exact = path.join(prev_exe_name);
        if exact.is_file() {
            return Ok(exact.to_string_lossy().to_string());
        }
    }

    let ignored = [
        "unins000.exe",
        "uninst.exe",
        "uninstall.exe",
        "unitycrashhandler64.exe",
        "unitycrashhandler32.exe",
        "dxsetup.exe",
        "notification_helper.exe",
        "python.exe",
        "pythonw.exe",
        "zsync.exe",
        "zsyncmake.exe",
    ];

    let mut candidates: Vec<PathBuf> = Vec::new();
    for entry in walkdir::WalkDir::new(path)
        .max_depth(2)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let p = entry.path();
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_lowercase();
        if !prev_exe_name.is_empty() && name.eq_ignore_ascii_case(prev_exe_name) {
            return Ok(p.to_string_lossy().to_string());
        }
        if name.ends_with(".exe")
            && !ignored.contains(&name.as_str())
            && !name.starts_with("vcredist")
            && !name.starts_with("unins")
        {
            candidates.push(p.to_path_buf());
        }
    }

    candidates.sort_by_key(|c| c.components().count());
    if let Some(best) = candidates.into_iter().next() {
        Ok(best.to_string_lossy().to_string())
    } else {
        Err("No game executable (.exe) found in the selected folder".to_string())
    }
}

/// Converts a concrete path selected by the user into a portable canonical path when possible.
pub fn to_canonical_path(raw_path: &str, exe_path: &str, proton_prefix: &str) -> String {
    let raw = Path::new(raw_path);
    if !exe_path.is_empty() {
        if let Some(game_dir) = Path::new(exe_path).parent() {
            if let Ok(rel) = raw.strip_prefix(game_dir) {
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                return if rel_str.is_empty() {
                    "<game_dir>".to_string()
                } else {
                    format!("<game_dir>/{}", rel_str)
                };
            }
        }
    }

    if !proton_prefix.is_empty() {
        let pfx = Path::new(proton_prefix);
        let user_root = if pfx.join("pfx").exists() {
            pfx.join("pfx/drive_c/users/steamuser")
        } else {
            pfx.join("drive_c/users/steamuser")
        };
        let roaming = user_root.join("AppData/Roaming");
        if let Ok(rel) = raw.strip_prefix(&roaming) {
            return format!("<win_appdata>/{}", rel.to_string_lossy().replace('\\', "/"));
        }
        let locallow = user_root.join("AppData/LocalLow");
        if let Ok(rel) = raw.strip_prefix(&locallow) {
            return format!(
                "<win_locallow>/{}",
                rel.to_string_lossy().replace('\\', "/")
            );
        }
    }

    if let Some(roaming) = dirs::data_dir() {
        if let Ok(rel) = raw.strip_prefix(&roaming) {
            return format!("<win_appdata>/{}", rel.to_string_lossy().replace('\\', "/"));
        }
    }

    if let Some(docs) = dirs::document_dir() {
        if let Ok(rel) = raw.strip_prefix(&docs) {
            return format!(
                "<win_documents>/{}",
                rel.to_string_lossy().replace('\\', "/")
            );
        }
    }

    // Handle paths from any OS user account (e.g. C:\Users\OtherUser\AppData\Roaming\...)
    if let Some(user_rel) = extract_user_home_relative(raw_path) {
        let lower_rel = user_rel.to_lowercase();
        for (prefix, token) in [
            ("appdata/roaming/", "<win_appdata>"),
            ("appdata/locallow/", "<win_locallow>"),
            ("documents/", "<win_documents>"),
            ("my documents/", "<win_documents>"),
        ] {
            if lower_rel.starts_with(prefix) {
                let rest = &user_rel[prefix.len()..];
                if !rest.is_empty() {
                    return format!("{}/{}", token, rest);
                }
            }
        }
    }

    raw_path.replace('\\', "/")
}

/// Inspects a game directory / executable to detect the engine and suggest canonical save paths,
/// consulting the active save location database first.
pub fn detect_game_from_exe_with_db(
    exe_path: &str,
    proton_prefix: &str,
    presets: &[SaveLocationEntry],
) -> DetectionResult {
    let exe = Path::new(exe_path);
    let game_dir = exe
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let suggested_title = infer_game_title(exe_path);
    let exe_file_name = exe
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_lowercase();
    let dir_name = game_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_lowercase();

    let mut engine = "Custom / Generic".to_string();
    let mut canonical_paths: Vec<String> = Vec::new();

    // 0. Check save_locations.json preset database by executable name or title
    if !exe_file_name.is_empty() || !dir_name.is_empty() {
        for preset in presets {
            let exe_matched = !exe_file_name.is_empty()
                && !exe_file_name.eq_ignore_ascii_case("game.exe")
                && !exe_file_name.eq_ignore_ascii_case("launcher.exe")
                && preset
                    .exe_names
                    .iter()
                    .any(|n| n.eq_ignore_ascii_case(&exe_file_name));
            let title_matched = !dir_name.is_empty()
                && !preset.vndb_id.is_empty()
                && preset.title.eq_ignore_ascii_case(&dir_name);

            if exe_matched || title_matched {
                if !preset.engine.is_empty() {
                    engine = preset.engine.clone();
                }
                let expanded: Vec<String> = preset
                    .save_paths
                    .iter()
                    .map(|p| p.replace("<game_title>", &suggested_title))
                    .collect();
                let existing: Vec<String> = expanded
                    .iter()
                    .filter(|p| resolve_save_path(p, exe_path, proton_prefix).exists())
                    .cloned()
                    .collect();
                canonical_paths = if existing.is_empty() {
                    expanded
                } else {
                    existing
                };
                break;
            }
        }
    }

    // 1. Ren'Py check (`renpy/` directory or `game/saves`)
    if canonical_paths.is_empty()
        && (game_dir.join("renpy").is_dir() || game_dir.join("game").is_dir())
    {
        let mut is_renpy = game_dir.join("renpy").is_dir();
        if !is_renpy {
            if let Ok(entries) = fs::read_dir(game_dir.join("game")) {
                is_renpy = entries.flatten().any(|e| {
                    let name = e.file_name().to_string_lossy().to_lowercase();
                    name.ends_with(".rpa") || name.ends_with(".rpyb") || name.ends_with(".rpy")
                });
            }
        }
        if is_renpy {
            engine = "Ren'Py".to_string();
            canonical_paths.push("<game_dir>/game/saves".to_string());
            if let Some(save_dir_name) = detect_renpy_save_dir(&game_dir) {
                canonical_paths.push(format!("<win_appdata>/RenPy/{}", save_dir_name));
            }
        }
    }

    // 2. RPG Maker MV / MZ check (`www/data/System.json` or `data/System.json`)
    if engine == "Custom / Generic" {
        if game_dir.join("www/data/System.json").is_file() {
            engine = "RPG Maker MV".to_string();
            canonical_paths.push("<game_dir>/www/save".to_string());
        } else if game_dir.join("data/System.json").is_file() {
            engine = "RPG Maker MZ".to_string();
            canonical_paths.push("<game_dir>/save".to_string());
        }
    }

    // 3. RPG Maker XP / VX / VX Ace check (`Game.rgss*` or `Game.ini`)
    if engine == "Custom / Generic" {
        if let Ok(entries) = fs::read_dir(&game_dir) {
            let has_rgss = entries.flatten().any(|e| {
                let name = e.file_name().to_string_lossy().to_lowercase();
                name.ends_with(".rgssad") || name.ends_with(".rgss2a") || name.ends_with(".rgss3a")
            });
            if has_rgss {
                engine = "RPG Maker VX / XP".to_string();
                if game_dir.join("Save").is_dir() {
                    canonical_paths.push("<game_dir>/Save".to_string());
                } else {
                    canonical_paths.push("<game_dir>/Save*.rvdata2".to_string());
                    canonical_paths.push("<game_dir>/Save*.rvdata".to_string());
                    canonical_paths.push("<game_dir>/Save*.rxdata".to_string());
                }
            }
        }
    }

    // 3b. TyranoBuilder / TyranoScript (`tyrano/`, `resources/app/tyrano`, `*_tyrano_data.sav`, or `*_sf.sav`)
    if engine == "Custom / Generic" {
        let has_tyrano_dir = game_dir.join("tyrano").is_dir()
            || game_dir.join("www/tyrano").is_dir()
            || game_dir.join("resources/app/tyrano").is_dir();
        let has_tyrano_sav = fs::read_dir(&game_dir)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .any(|e| {
                let name = e.file_name().to_string_lossy().to_lowercase();
                name.ends_with("_tyrano_data.sav")
                    || name.ends_with("_sf.sav")
                    || name.ends_with("_tyrano_auto_save.sav")
            });
        if has_tyrano_dir || has_tyrano_sav {
            engine = "TyranoBuilder".to_string();
            canonical_paths.push("<game_dir>/*.sav".to_string());
            if game_dir.join("save").is_dir() {
                canonical_paths.push("<game_dir>/save".to_string());
            }
        }
    }

    // 4. Kirikiri / KAG (`data.xp3` or `*.xp3`)
    if engine == "Custom / Generic" {
        if let Ok(entries) = fs::read_dir(&game_dir) {
            let has_xp3 = entries.flatten().any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .ends_with(".xp3")
            });
            if has_xp3 {
                engine = "Kirikiri / KAG".to_string();
                for candidate in ["savedata", "save", "data/save"] {
                    if game_dir.join(candidate).is_dir() {
                        canonical_paths.push(format!("<game_dir>/{}", candidate));
                    }
                }
                if canonical_paths.is_empty() {
                    canonical_paths.push("<game_dir>/savedata".to_string());
                }
            }
        }
    }

    // 4b. Nitroplus / Nitro+CHiRAL (`*.npa` or `*.npk` archives)
    if engine == "Custom / Generic" {
        let has_nitro_archive = [game_dir.clone(), game_dir.join("data")]
            .iter()
            .filter_map(|d| fs::read_dir(d).ok())
            .flatten()
            .flatten()
            .any(|e| {
                let name = e.file_name().to_string_lossy().to_lowercase();
                name.ends_with(".npa") || name.ends_with(".npk")
            });
        if has_nitro_archive {
            engine = "Nitroplus / NPK".to_string();
            let candidates = [
                format!("<win_appdata>/Nitro+CHiRAL/{}", suggested_title),
                format!("<win_appdata>/Nitroplus/{}", suggested_title),
                format!("<win_appdata>/NITRO ORIGIN/{}", suggested_title),
                format!("<win_documents>/Nitro+CHiRAL/{}", suggested_title),
                format!("<win_documents>/Nitroplus/{}", suggested_title),
                "<game_dir>/savedata".to_string(),
            ];
            for cand in &candidates {
                if resolve_save_path(cand, exe_path, proton_prefix).exists() {
                    canonical_paths.push(cand.clone());
                }
            }
            if canonical_paths.is_empty() {
                canonical_paths.push(format!("<win_appdata>/Nitro+CHiRAL/{}", suggested_title));
                canonical_paths.push(format!("<win_appdata>/Nitroplus/{}", suggested_title));
                canonical_paths.push("<game_dir>/savedata".to_string());
            }
        }
    }

    // 5. Unity (`UnityPlayer.dll` or `*_Data/app.info`)
    if engine == "Custom / Generic" {
        if let Some((company, product)) = detect_unity_app_info(&game_dir) {
            engine = "Unity".to_string();
            canonical_paths.push(format!("<win_locallow>/{}/{}", company, product));
        }
    }

    // 6. Generic fallback save folders or root save files inside game directory
    if canonical_paths.is_empty() {
        for folder in ["save", "saves", "savedata", "SaveData", "Save", "userdata"] {
            if game_dir.join(folder).is_dir() {
                canonical_paths.push(format!("<game_dir>/{}", folder));
            }
        }
    }
    if canonical_paths.is_empty() {
        let has_root_sav = fs::read_dir(&game_dir)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .any(|e| {
                e.file_type().map(|t| t.is_file()).unwrap_or(false)
                    && e.file_name()
                        .to_string_lossy()
                        .to_lowercase()
                        .ends_with(".sav")
            });
        if has_root_sav {
            canonical_paths.push("<game_dir>/*.sav".to_string());
        } else {
            canonical_paths.push("<game_dir>/save".to_string());
        }
    }

    let resolved_save_paths: Vec<String> = canonical_paths
        .iter()
        .map(|c| {
            resolve_save_path(c, exe_path, proton_prefix)
                .to_string_lossy()
                .to_string()
        })
        .collect();

    let existing_save_files = resolved_save_paths
        .iter()
        .map(|p| count_matching_save_files(Path::new(p), &game_dir))
        .sum();

    DetectionResult {
        engine,
        suggested_title,
        canonical_save_paths: canonical_paths,
        resolved_save_paths,
        existing_save_files,
    }
}

pub fn detect_game_from_exe(exe_path: &str, proton_prefix: &str) -> DetectionResult {
    let presets = parse_save_locations_json(DEFAULT_SAVE_LOCATIONS_JSON).unwrap_or_default();
    detect_game_from_exe_with_db(exe_path, proton_prefix, &presets)
}

fn detect_renpy_save_dir(game_dir: &Path) -> Option<String> {
    let options_rpy = game_dir.join("game/options.rpy");
    if let Ok(content) = fs::read_to_string(options_rpy) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("define config.save_directory")
                || trimmed.starts_with("config.save_directory")
            {
                if let Some(eq_idx) = trimmed.find('=') {
                    let val = trimmed[eq_idx + 1..]
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'');
                    if !val.is_empty() {
                        return Some(val.to_string());
                    }
                }
            }
        }
    }
    None
}

fn detect_unity_app_info(game_dir: &Path) -> Option<(String, String)> {
    let entries = fs::read_dir(game_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && path.file_name()?.to_string_lossy().ends_with("_Data") {
            let app_info = path.join("app.info");
            if let Ok(text) = fs::read_to_string(app_info) {
                let mut lines = text.lines();
                let company = lines.next()?.trim().to_string();
                let product = lines.next()?.trim().to_string();
                if !company.is_empty() && !product.is_empty() {
                    return Some((company, product));
                }
            }
        }
    }
    None
}

/// Case-insensitive wildcard matching supporting `*` and `?`.
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_idx, mut match_idx) = (None::<usize>, 0usize);

    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star_idx = Some(pi);
            match_idx = ti;
            pi += 1;
        } else if let Some(s_idx) = star_idx {
            pi = s_idx + 1;
            match_idx += 1;
            ti = match_idx;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Returns true if a filename in the root of `<game_dir>` is a known save file
/// (used when `<game_dir>` itself is configured, to prevent uploading the entire game).
pub fn is_root_save_file_name(file_name: &str) -> bool {
    let lower = file_name.to_lowercase();
    if is_game_binary_or_archive(&lower) {
        return false;
    }
    lower.ends_with(".sav")
        || lower.ends_with(".rpgsave")
        || lower.ends_with(".rvdata")
        || lower.ends_with(".rvdata2")
        || lower.ends_with(".rxdata")
        || lower.ends_with(".lsd")
        || lower.ends_with(".sol")
        || (lower.contains("save")
            && (lower.ends_with(".dat") || lower.ends_with(".json") || lower.ends_with(".bin")))
}

/// Returns true if a file is a game executable, library, or resource archive that must never be packed as a save.
pub fn is_game_binary_or_archive(file_name: &str) -> bool {
    let lower = file_name.to_lowercase();
    [
        ".exe", ".dll", ".so", ".dylib", ".pak", ".asar", ".xp3", ".rpa", ".npa", ".npk",
        ".rgssad", ".rgss2a", ".rgss3a", ".pck", ".obb", ".wasm", ".pdb",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

pub fn count_matching_save_files(target: &Path, game_dir: &Path) -> usize {
    if let Some(name) = target.file_name().and_then(|s| s.to_str()) {
        if name.contains('*') || name.contains('?') {
            if let Some(parent) = target.parent() {
                if let Ok(entries) = fs::read_dir(parent) {
                    return entries
                        .flatten()
                        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                        .filter(|e| {
                            let fname = e.file_name().to_string_lossy().to_string();
                            !is_game_binary_or_archive(&fname) && wildcard_match(name, &fname)
                        })
                        .count();
                }
            }
            return 0;
        }
    }

    if !target.exists() {
        return 0;
    }
    if target.is_file() {
        return 1;
    }
    if target == game_dir {
        if let Ok(entries) = fs::read_dir(target) {
            return entries
                .flatten()
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .filter(|e| is_root_save_file_name(&e.file_name().to_string_lossy()))
                .count();
        }
        return 0;
    }

    walkdir::WalkDir::new(target)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| !is_game_binary_or_archive(&e.file_name().to_string_lossy()))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proton_prefix_translation() {
        let resolved = resolve_save_path(
            "<win_appdata>/RenPy/SteinsGate",
            "/games/SteinsGate/Game.exe",
            "/home/deck/.local/share/Steam/steamapps/compatdata/12345",
        );
        let norm = resolved.to_string_lossy().replace('\\', "/");
        assert!(norm.ends_with(
            "compatdata/12345/drive_c/users/steamuser/AppData/Roaming/RenPy/SteinsGate"
        ));
    }

    #[test]
    fn test_game_dir_expansion_and_detection() {
        let tmp = tempfile::tempdir().unwrap();
        let game_dir = tmp.path();
        fs::create_dir_all(game_dir.join("www/data")).unwrap();
        fs::create_dir_all(game_dir.join("www/save")).unwrap();
        fs::write(game_dir.join("www/data/System.json"), "{}").unwrap();
        fs::write(game_dir.join("www/save/file1.rpgsave"), "save").unwrap();
        let exe_path = game_dir.join("Game.exe");
        fs::write(&exe_path, "").unwrap();

        let detected = detect_game_from_exe(&exe_path.to_string_lossy(), "");
        assert_eq!(detected.engine, "RPG Maker MV");
        assert_eq!(detected.canonical_save_paths, vec!["<game_dir>/www/save"]);
        assert_eq!(detected.existing_save_files, 1);
    }

    #[test]
    fn test_default_save_locations_json_and_nitro_detection() {
        let entries = parse_save_locations_json(DEFAULT_SAVE_LOCATIONS_JSON).unwrap();
        assert!(entries.len() >= 10);
        assert!(entries.iter().any(|e| e.developer == "Nitro+CHiRAL"));

        let tmp = tempfile::tempdir().unwrap();
        let game_dir = tmp.path();
        fs::write(game_dir.join("nss.npa"), "archive").unwrap();
        let exe_path = game_dir.join("SlowDamage.exe");
        fs::write(&exe_path, "").unwrap();

        let detected = detect_game_from_exe_with_db(&exe_path.to_string_lossy(), "", &entries);
        assert_eq!(detected.engine, "Nitroplus / NPK");
        assert!(detected
            .canonical_save_paths
            .contains(&"<win_appdata>/Nitro+CHiRAL/Slow Damage".to_string()));
    }

    #[test]
    fn test_cross_user_path_remap_and_locate() {
        assert_eq!(
            to_canonical_path("C:\\Users\\Alice\\AppData\\Roaming\\RenPy\\MyVN", "", ""),
            "<win_appdata>/RenPy/MyVN"
        );
        assert_eq!(
            to_canonical_path(
                "C:\\Users\\Bob\\AppData\\LocalLow\\Company\\Product",
                "",
                ""
            ),
            "<win_locallow>/Company/Product"
        );

        let tmp = tempfile::tempdir().unwrap();
        let remapped = remap_os_user_path_with_home(
            "C:\\Users\\WrongUser\\Games\\SteinsGate\\Game.exe",
            tmp.path(),
        )
        .unwrap();
        assert_eq!(remapped, tmp.path().join("Games/SteinsGate/Game.exe"));

        let game_folder = tmp.path().join("Games/SteinsGate");
        fs::create_dir_all(&game_folder).unwrap();
        fs::write(game_folder.join("unins000.exe"), "").unwrap();
        fs::write(game_folder.join("SteinsGate.exe"), "").unwrap();

        let located = locate_game_executable(
            &game_folder.to_string_lossy(),
            "C:\\Users\\WrongUser\\Games\\SteinsGate\\SteinsGate.exe",
        )
        .unwrap();
        assert!(located.ends_with("SteinsGate.exe"));
    }
}
