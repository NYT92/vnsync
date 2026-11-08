use crate::db::{Game, SnapshotRecord};
use crate::resolver::{
    is_game_binary_or_archive, is_root_save_file_name, resolve_save_path, wildcard_match,
};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDigest {
    pub root_index: usize,
    pub rel_path: String,
    pub size: u64,
    pub blake3: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotManifest {
    pub snapshot_id: String,
    pub parent_id: Option<String>,
    pub game_id: String,
    pub device_id: String,
    pub device_name: String,
    pub created_at: String,
    pub tree_hash: String,
    pub files: Vec<FileDigest>,
}

/// Scans resolved save paths for a game and computes deterministic BLAKE3 tree hash without writing an archive.
/// Supports directory paths, single file paths, and filename wildcard rules (e.g. `<game_dir>/*.sav`).
/// If a rule points to `<game_dir>` itself, only root-level save files are collected (never game binaries/subfolders).
pub fn compute_current_save_state(
    game: &Game,
) -> Result<(String, Vec<(usize, PathBuf, FileDigest)>), String> {
    let mut collected: Vec<(usize, PathBuf, FileDigest)> = Vec::new();
    let game_dir = Path::new(&game.exe_path)
        .parent()
        .map(|p| p.to_path_buf());

    for (root_idx, pattern) in game.save_paths.iter().enumerate() {
        let root_path = resolve_save_path(pattern, &game.exe_path, &game.proton_prefix);

        // 1. Check if last path segment is a wildcard pattern (e.g. `<game_dir>/*.sav` or `Save*.rvdata2`)
        if let Some(glob_name) = root_path.file_name().and_then(|s| s.to_str()) {
            if glob_name.contains('*') || glob_name.contains('?') {
                if let Some(parent_dir) = root_path.parent() {
                    if let Ok(entries) = fs::read_dir(parent_dir) {
                        for entry in entries.flatten() {
                            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                                continue;
                            }
                            let fname = entry.file_name().to_string_lossy().to_string();
                            if !is_game_binary_or_archive(&fname)
                                && wildcard_match(glob_name, &fname)
                            {
                                let full_path = entry.path();
                                let digest = hash_file(&full_path, root_idx, &fname)?;
                                collected.push((root_idx, full_path, digest));
                            }
                        }
                    }
                }
                continue;
            }
        }

        if !root_path.exists() {
            continue;
        }

        // 2. Single file rule (e.g. `<game_dir>/data_sf.sav`)
        if root_path.is_file() {
            let fname = root_path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if !is_game_binary_or_archive(&fname) {
                let digest = hash_file(&root_path, root_idx, &fname)?;
                collected.push((root_idx, root_path, digest));
            }
            continue;
        }

        // 3. Safety guard: if rule points directly to `<game_dir>`, only collect top-level save files
        let is_game_root = pattern.trim().replace('\\', "/").trim_end_matches('/') == "<game_dir>"
            || game_dir.as_deref() == Some(root_path.as_path());
        if is_game_root {
            if let Ok(entries) = fs::read_dir(&root_path) {
                for entry in entries.flatten() {
                    if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                        continue;
                    }
                    let fname = entry.file_name().to_string_lossy().to_string();
                    if is_root_save_file_name(&fname) {
                        let full_path = entry.path();
                        let digest = hash_file(&full_path, root_idx, &fname)?;
                        collected.push((root_idx, full_path, digest));
                    }
                }
            }
            continue;
        }

        // 4. Dedicated save directory: walk recursively, excluding game binaries/archives
        for entry in walkdir::WalkDir::new(&root_path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
        {
            let fname = entry.file_name().to_string_lossy();
            if is_game_binary_or_archive(&fname) {
                continue;
            }
            let full_path = entry.path().to_path_buf();
            let rel = full_path
                .strip_prefix(&root_path)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let digest = hash_file(&full_path, root_idx, &rel)?;
            collected.push((root_idx, full_path, digest));
        }
    }

    collected.sort_by(|a, b| {
        a.2.root_index
            .cmp(&b.2.root_index)
            .then_with(|| a.2.rel_path.cmp(&b.2.rel_path))
    });

    let mut tree_hasher = blake3::Hasher::new();
    for (_, _, item) in &collected {
        tree_hasher.update(format!("{}:{}:{}\n", item.root_index, item.rel_path, item.blake3).as_bytes());
    }
    let tree_hash = format!("blake3:{}", tree_hasher.finalize().to_hex());

    Ok((tree_hash, collected))
}

fn hash_file(path: &Path, root_index: usize, rel_path: &str) -> Result<FileDigest, String> {
    let mut file = File::open(path).map_err(|e| format!("Failed to open {:?}: {}", path, e))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 65536];
    let mut size = 0u64;
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok(FileDigest {
        root_index,
        rel_path: rel_path.to_string(),
        size,
        blake3: hasher.finalize().to_hex().to_string(),
    })
}

/// Packs a game's current save files into `<vault_dir>/<game_id>/<snapshot_id>.tar.zst`.
pub fn create_snapshot(
    game: &Game,
    vault_dir: &Path,
    device_id: &str,
    device_name: &str,
    note: &str,
) -> Result<(SnapshotRecord, SnapshotManifest), String> {
    let (tree_hash, files) = compute_current_save_state(game)?;

    let snapshot_id = uuid::Uuid::now_v7().to_string();
    let created_at = chrono::Utc::now().to_rfc3339();
    let game_vault = vault_dir.join(&game.id);
    fs::create_dir_all(&game_vault).map_err(|e| e.to_string())?;

    let archive_path = game_vault.join(format!("{}.tar.zst", snapshot_id));
    let manifest_path = game_vault.join(format!("{}.json", snapshot_id));

    let digests: Vec<FileDigest> = files.iter().map(|(_, _, d)| d.clone()).collect();
    let total_bytes: u64 = digests.iter().map(|d| d.size).sum();

    let manifest = SnapshotManifest {
        snapshot_id: snapshot_id.clone(),
        parent_id: game.head_snapshot_id.clone(),
        game_id: game.id.clone(),
        device_id: device_id.to_string(),
        device_name: device_name.to_string(),
        created_at: created_at.clone(),
        tree_hash: tree_hash.clone(),
        files: digests.clone(),
    };

    let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    fs::write(&manifest_path, &manifest_bytes).map_err(|e| e.to_string())?;

    let tar_file = File::create(&archive_path).map_err(|e| e.to_string())?;
    let encoder = zstd::Encoder::new(tar_file, 3).map_err(|e| e.to_string())?;
    let mut tar_builder = tar::Builder::new(encoder);

    // Embed manifest.json at archive root
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest_bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar_builder
        .append_data(&mut header, "manifest.json", manifest_bytes.as_slice())
        .map_err(|e| e.to_string())?;

    for (root_idx, full_path, digest) in &files {
        let entry_name = format!("root_{}/{}", root_idx, digest.rel_path);
        tar_builder
            .append_path_with_name(full_path, &entry_name)
            .map_err(|e| format!("Failed packing {}: {}", entry_name, e))?;
    }

    let encoder = tar_builder.into_inner().map_err(|e| e.to_string())?;
    encoder.finish().map_err(|e| e.to_string())?;

    let record = SnapshotRecord {
        id: snapshot_id,
        game_id: game.id.clone(),
        parent_id: game.head_snapshot_id.clone(),
        device_id: device_id.to_string(),
        device_name: device_name.to_string(),
        created_at,
        tree_hash,
        file_count: digests.len(),
        total_bytes,
        local_archive_path: archive_path.to_string_lossy().to_string(),
        note: note.to_string(),
        pinned: false,
    };

    Ok((record, manifest))
}

fn target_restore_dir_for_rule(resolved: &Path) -> PathBuf {
    let is_glob = resolved
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.contains('*') || s.contains('?'))
        .unwrap_or(false);
    let is_single_file = resolved.is_file()
        || (!resolved.is_dir() && resolved.extension().is_some());

    if is_glob || is_single_file {
        resolved
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        resolved.to_path_buf()
    }
}

/// Extracts a `.tar.zst` snapshot archive back into the game's resolved save directories.
pub fn restore_snapshot_archive(game: &Game, archive_path: &Path) -> Result<usize, String> {
    let file = File::open(archive_path)
        .map_err(|e| format!("Failed to open snapshot archive {:?}: {}", archive_path, e))?;
    let decoder = zstd::Decoder::new(file).map_err(|e| e.to_string())?;
    let mut archive = tar::Archive::new(decoder);

    let resolved_roots: Vec<PathBuf> = game
        .save_paths
        .iter()
        .map(|p| {
            let r = resolve_save_path(p, &game.exe_path, &game.proton_prefix);
            target_restore_dir_for_rule(&r)
        })
        .collect();

    for root in &resolved_roots {
        fs::create_dir_all(root).map_err(|e| e.to_string())?;
    }

    let mut restored_files = 0usize;
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path_str = entry
            .path()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");

        if path_str == "manifest.json" {
            continue;
        }

        if let Some(rest) = path_str.strip_prefix("root_") {
            if let Some(slash_idx) = rest.find('/') {
                let idx_str = &rest[..slash_idx];
                let rel_path = &rest[slash_idx + 1..];
                if let Ok(root_idx) = idx_str.parse::<usize>() {
                    if let Some(target_root) = resolved_roots.get(root_idx) {
                        // Guard against path traversal
                        if rel_path.contains("..") {
                            continue;
                        }
                        let dest = target_root.join(rel_path);
                        if let Some(parent) = dest.parent() {
                            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                        }
                        entry.unpack(&dest).map_err(|e| e.to_string())?;
                        restored_files += 1;
                    }
                }
            }
        }
    }

    Ok(restored_files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_pack_and_restore_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let save_dir = tmp.path().join("saves");
        let vault_dir = tmp.path().join("vault");
        fs::create_dir_all(&save_dir).unwrap();
        fs::write(save_dir.join("slot1.sav"), b"chapter_3_progress").unwrap();

        let game = Game {
            id: "test-vn".to_string(),
            title: "Test VN".to_string(),
            original_title: String::new(),
            vndb_id: String::new(),
            engine: "Ren'Py".to_string(),
            developer: String::new(),
            publisher: String::new(),
            release_date: String::new(),
            genre: String::new(),
            rating: String::new(),
            description: String::new(),
            status: "Playing".to_string(),
            progress: 50,
            thumbnail: String::new(),
            exe_path: tmp.path().join("Game.exe").to_string_lossy().to_string(),
            proton_prefix: String::new(),
            save_paths: vec!["<game_dir>/saves".to_string()],
            playtime_seconds: 120,
            last_played_at: None,
            head_snapshot_id: None,
            sync_status: "Local Only".to_string(),
            exe_exists: false,
        };

        let (record, _manifest) =
            create_snapshot(&game, &vault_dir, "dev-1", "PC", "Test checkpoint").unwrap();
        assert_eq!(record.file_count, 1);

        // Corrupt the save file, then restore from snapshot
        fs::write(save_dir.join("slot1.sav"), b"corrupted").unwrap();
        let count =
            restore_snapshot_archive(&game, Path::new(&record.local_archive_path)).unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            fs::read_to_string(save_dir.join("slot1.sav")).unwrap(),
            "chapter_3_progress"
        );
    }

    #[test]
    fn test_tyrano_root_glob_and_game_dir_guard_skip_game_binaries() {
        let tmp = tempfile::tempdir().unwrap();
        let game_dir = tmp.path().join("MyTyranoVN");
        let vault_dir = tmp.path().join("vault");
        fs::create_dir_all(game_dir.join("resources/app/tyrano")).unwrap();

        // Game binaries & assets that MUST NOT be uploaded
        let exe_path = game_dir.join("MyTyranoVN.exe");
        fs::write(&exe_path, b"huge_executable_binary").unwrap();
        fs::write(game_dir.join("nw.dll"), b"huge_dll").unwrap();
        fs::write(game_dir.join("resources/app.asar"), b"huge_asar_archive").unwrap();
        fs::write(game_dir.join("resources/app/tyrano/tyrano.js"), b"engine_js").unwrap();

        // Root save files that SHOULD be packed
        fs::write(game_dir.join("mygame_tyrano_data.sav"), b"save_slot_1").unwrap();
        fs::write(game_dir.join("mygame_sf.sav"), b"system_flags").unwrap();

        let game = Game {
            id: "tyrano-vn".to_string(),
            title: "My Tyrano VN".to_string(),
            original_title: String::new(),
            vndb_id: String::new(),
            engine: "TyranoBuilder".to_string(),
            developer: String::new(),
            publisher: String::new(),
            release_date: String::new(),
            genre: String::new(),
            rating: String::new(),
            description: String::new(),
            status: "Playing".to_string(),
            progress: 10,
            thumbnail: String::new(),
            exe_path: exe_path.to_string_lossy().to_string(),
            proton_prefix: String::new(),
            save_paths: vec!["<game_dir>/*.sav".to_string(), "<game_dir>".to_string()],
            playtime_seconds: 0,
            last_played_at: None,
            head_snapshot_id: None,
            sync_status: "Local Only".to_string(),
            exe_exists: true,
        };

        let (record, manifest) =
            create_snapshot(&game, &vault_dir, "dev-1", "PC", "Tyrano test").unwrap();

        // Each rule (`<game_dir>/*.sav` and `<game_dir>`) should only see the 2 .sav files, never .exe/.dll/.asar/subfolders
        assert!(manifest.files.iter().all(|f| f.rel_path.ends_with(".sav")));
        assert!(!manifest.files.iter().any(|f| f.rel_path.contains("MyTyranoVN.exe")));
        assert!(!manifest.files.iter().any(|f| f.rel_path.contains("app.asar")));

        // Corrupt a root save file and restore
        fs::write(game_dir.join("mygame_tyrano_data.sav"), b"wiped").unwrap();
        restore_snapshot_archive(&game, Path::new(&record.local_archive_path)).unwrap();
        assert_eq!(
            fs::read_to_string(game_dir.join("mygame_tyrano_data.sav")).unwrap(),
            "save_slot_1"
        );
    }
}
