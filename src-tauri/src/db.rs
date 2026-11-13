use rusqlite::{params, Connection, Result as SqlResult};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

const KEYRING_SERVICE: &str = "vnsync";

fn keyring_err(e: keyring::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(e))
}

/// Secrets live in the OS keychain (Credential Manager / Keychain / Secret Service), never in SQLite.
fn store_secret(profile_id: &str, secret: &str) -> Result<(), keyring::Error> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, profile_id)?;
    if secret.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e),
        }
    } else {
        entry.set_password(secret)
    }
}

fn load_secret(profile_id: &str) -> String {
    keyring::Entry::new(KEYRING_SERVICE, profile_id)
        .and_then(|e| e.get_password())
        .unwrap_or_default()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: String,
    pub title: String,
    pub original_title: String,
    pub vndb_id: String,
    pub engine: String,
    pub developer: String,
    pub publisher: String,
    pub release_date: String,
    pub genre: String,
    pub rating: String,
    pub description: String,
    pub status: String,
    pub progress: i64,
    pub thumbnail: String,
    pub exe_path: String,
    pub proton_prefix: String,
    pub save_paths: Vec<String>,
    pub playtime_seconds: i64,
    pub last_played_at: Option<String>,
    pub head_snapshot_id: Option<String>,
    pub sync_status: String,
    #[serde(default)]
    pub exe_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotRecord {
    pub id: String,
    pub game_id: String,
    pub parent_id: Option<String>,
    pub device_id: String,
    pub device_name: String,
    pub created_at: String,
    pub tree_hash: String,
    pub file_count: usize,
    pub total_bytes: u64,
    pub local_archive_path: String,
    pub note: String,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageProfile {
    pub id: String,
    pub name: String,
    pub provider_type: String, // "s3", "webdav", "local_fs", "vnsync_server"
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub root_prefix: String,
    pub access_key: String,
    pub secret_key: String,
    pub is_active: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub device_id: String,
    pub device_name: String,
    pub theme: String,
    pub language: String,
    pub auto_sync_on_launch: bool,
    pub auto_sync_on_exit: bool,
    pub backup_before_sync: bool,
    pub max_local_snapshots: usize,
    pub enable_lan_p2p: bool,
    pub lan_port: u16,
    pub game_library_path: String,
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    #[serde(default)]
    pub minimize_to_tray: bool,
    #[serde(default = "default_true")]
    pub ask_on_close: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        let hostname = std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "My-Device".to_string());
        Self {
            device_id: uuid::Uuid::new_v4().to_string(),
            device_name: hostname,
            theme: "system".to_string(),
            language: "en".to_string(),
            auto_sync_on_launch: true,
            auto_sync_on_exit: true,
            backup_before_sync: true,
            max_local_snapshots: 20,
            enable_lan_p2p: true,
            lan_port: 47832,
            game_library_path: String::new(),
            close_to_tray: true,
            minimize_to_tray: false,
            ask_on_close: true,
        }
    }
}

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &Path) -> SqlResult<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.init_schema()?;
        Ok(db)
    }

    pub fn open_in_memory() -> SqlResult<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS games (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                original_title TEXT NOT NULL DEFAULT '',
                vndb_id TEXT NOT NULL DEFAULT '',
                engine TEXT NOT NULL DEFAULT 'Unknown',
                developer TEXT NOT NULL DEFAULT '',
                publisher TEXT NOT NULL DEFAULT '',
                release_date TEXT NOT NULL DEFAULT '',
                genre TEXT NOT NULL DEFAULT '',
                rating TEXT NOT NULL DEFAULT '',
                description TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'Not Started',
                progress INTEGER NOT NULL DEFAULT 0,
                thumbnail TEXT NOT NULL DEFAULT '',
                exe_path TEXT NOT NULL DEFAULT '',
                proton_prefix TEXT NOT NULL DEFAULT '',
                save_paths_json TEXT NOT NULL DEFAULT '[]',
                playtime_seconds INTEGER NOT NULL DEFAULT 0,
                last_played_at TEXT,
                head_snapshot_id TEXT,
                sync_status TEXT NOT NULL DEFAULT 'Local Only'
            );

            CREATE TABLE IF NOT EXISTS snapshots (
                id TEXT PRIMARY KEY,
                game_id TEXT NOT NULL,
                parent_id TEXT,
                device_id TEXT NOT NULL,
                device_name TEXT NOT NULL,
                created_at TEXT NOT NULL,
                tree_hash TEXT NOT NULL,
                file_count INTEGER NOT NULL,
                total_bytes INTEGER NOT NULL,
                local_archive_path TEXT NOT NULL,
                note TEXT NOT NULL DEFAULT '',
                pinned INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS storage_profiles (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                provider_type TEXT NOT NULL,
                endpoint TEXT NOT NULL DEFAULT '',
                bucket TEXT NOT NULL DEFAULT '',
                region TEXT NOT NULL DEFAULT 'auto',
                root_prefix TEXT NOT NULL DEFAULT 'vnsync',
                access_key TEXT NOT NULL DEFAULT '',
                secret_key TEXT NOT NULL DEFAULT '',
                is_active INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value_json TEXT NOT NULL
            );
            ",
        )?;
        Ok(())
    }

    pub fn list_games(&self) -> SqlResult<Vec<Game>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, original_title, vndb_id, engine, developer, publisher,
                    release_date, genre, rating, description, status, progress, thumbnail,
                    exe_path, proton_prefix, save_paths_json, playtime_seconds,
                    last_played_at, head_snapshot_id, sync_status
             FROM games
             ORDER BY COALESCE(last_played_at, '') DESC, title ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let save_paths_json: String = row.get(16)?;
            let save_paths: Vec<String> =
                serde_json::from_str(&save_paths_json).unwrap_or_default();
            let exe_path: String = row.get(14)?;
            let exe_exists = !exe_path.trim().is_empty() && Path::new(&exe_path).is_file();
            Ok(Game {
                id: row.get(0)?,
                title: row.get(1)?,
                original_title: row.get(2)?,
                vndb_id: row.get(3)?,
                engine: row.get(4)?,
                developer: row.get(5)?,
                publisher: row.get(6)?,
                release_date: row.get(7)?,
                genre: row.get(8)?,
                rating: row.get(9)?,
                description: row.get(10)?,
                status: row.get(11)?,
                progress: row.get(12)?,
                thumbnail: row.get(13)?,
                exe_path,
                proton_prefix: row.get(15)?,
                save_paths,
                playtime_seconds: row.get(17)?,
                last_played_at: row.get(18)?,
                head_snapshot_id: row.get(19)?,
                sync_status: row.get(20)?,
                exe_exists,
            })
        })?;
        rows.collect()
    }

    pub fn get_game(&self, id: &str) -> SqlResult<Option<Game>> {
        let games = self.list_games()?;
        Ok(games.into_iter().find(|g| g.id == id))
    }

    pub fn upsert_game(&self, game: &Game) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        let save_paths_json =
            serde_json::to_string(&game.save_paths).unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "INSERT INTO games (
                id, title, original_title, vndb_id, engine, developer, publisher,
                release_date, genre, rating, description, status, progress, thumbnail,
                exe_path, proton_prefix, save_paths_json, playtime_seconds,
                last_played_at, head_snapshot_id, sync_status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)
            ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                original_title = excluded.original_title,
                vndb_id = excluded.vndb_id,
                engine = excluded.engine,
                developer = excluded.developer,
                publisher = excluded.publisher,
                release_date = excluded.release_date,
                genre = excluded.genre,
                rating = excluded.rating,
                description = excluded.description,
                status = excluded.status,
                progress = excluded.progress,
                thumbnail = excluded.thumbnail,
                exe_path = excluded.exe_path,
                proton_prefix = excluded.proton_prefix,
                save_paths_json = excluded.save_paths_json,
                playtime_seconds = excluded.playtime_seconds,
                last_played_at = excluded.last_played_at,
                head_snapshot_id = excluded.head_snapshot_id,
                sync_status = excluded.sync_status",
            params![
                game.id,
                game.title,
                game.original_title,
                game.vndb_id,
                game.engine,
                game.developer,
                game.publisher,
                game.release_date,
                game.genre,
                game.rating,
                game.description,
                game.status,
                game.progress,
                game.thumbnail,
                game.exe_path,
                game.proton_prefix,
                save_paths_json,
                game.playtime_seconds,
                game.last_played_at,
                game.head_snapshot_id,
                game.sync_status,
            ],
        )?;
        Ok(())
    }

    pub fn delete_game(&self, id: &str) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM snapshots WHERE game_id = ?1", params![id])?;
        conn.execute("DELETE FROM games WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn insert_snapshot(&self, snap: &SnapshotRecord) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO snapshots (
                id, game_id, parent_id, device_id, device_name, created_at,
                tree_hash, file_count, total_bytes, local_archive_path, note, pinned
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                snap.id,
                snap.game_id,
                snap.parent_id,
                snap.device_id,
                snap.device_name,
                snap.created_at,
                snap.tree_hash,
                snap.file_count as i64,
                snap.total_bytes as i64,
                snap.local_archive_path,
                snap.note,
                if snap.pinned { 1 } else { 0 },
            ],
        )?;
        conn.execute(
            "UPDATE games SET head_snapshot_id = ?1 WHERE id = ?2",
            params![snap.id, snap.game_id],
        )?;
        Ok(())
    }

    pub fn list_snapshots(&self, game_id: &str) -> SqlResult<Vec<SnapshotRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, game_id, parent_id, device_id, device_name, created_at,
                    tree_hash, file_count, total_bytes, local_archive_path, note, pinned
             FROM snapshots
             WHERE game_id = ?1
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![game_id], |row| {
            let file_count: i64 = row.get(7)?;
            let total_bytes: i64 = row.get(8)?;
            let pinned: i64 = row.get(11)?;
            Ok(SnapshotRecord {
                id: row.get(0)?,
                game_id: row.get(1)?,
                parent_id: row.get(2)?,
                device_id: row.get(3)?,
                device_name: row.get(4)?,
                created_at: row.get(5)?,
                tree_hash: row.get(6)?,
                file_count: file_count as usize,
                total_bytes: total_bytes as u64,
                local_archive_path: row.get(9)?,
                note: row.get(10)?,
                pinned: pinned != 0,
            })
        })?;
        rows.collect()
    }

    pub fn delete_snapshot(&self, snapshot_id: &str) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM snapshots WHERE id = ?1", params![snapshot_id])?;
        Ok(())
    }

    pub fn set_snapshot_pinned(&self, snapshot_id: &str, pinned: bool) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE snapshots SET pinned = ?1 WHERE id = ?2",
            params![if pinned { 1 } else { 0 }, snapshot_id],
        )?;
        Ok(())
    }

    pub fn get_snapshot(&self, snapshot_id: &str) -> SqlResult<Option<SnapshotRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, game_id, parent_id, device_id, device_name, created_at,
                    tree_hash, file_count, total_bytes, local_archive_path, note, pinned
             FROM snapshots WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![snapshot_id], |row| {
            let file_count: i64 = row.get(7)?;
            let total_bytes: i64 = row.get(8)?;
            let pinned: i64 = row.get(11)?;
            Ok(SnapshotRecord {
                id: row.get(0)?,
                game_id: row.get(1)?,
                parent_id: row.get(2)?,
                device_id: row.get(3)?,
                device_name: row.get(4)?,
                created_at: row.get(5)?,
                tree_hash: row.get(6)?,
                file_count: file_count as usize,
                total_bytes: total_bytes as u64,
                local_archive_path: row.get(9)?,
                note: row.get(10)?,
                pinned: pinned != 0,
            })
        })?;
        match rows.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    pub fn list_storage_profiles(&self) -> SqlResult<Vec<StorageProfile>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, provider_type, endpoint, bucket, region, root_prefix, access_key, secret_key, is_active
             FROM storage_profiles ORDER BY name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let active: i64 = row.get(9)?;
            Ok(StorageProfile {
                id: row.get(0)?,
                name: row.get(1)?,
                provider_type: row.get(2)?,
                endpoint: row.get(3)?,
                bucket: row.get(4)?,
                region: row.get(5)?,
                root_prefix: row.get(6)?,
                access_key: row.get(7)?,
                secret_key: row.get(8)?,
                is_active: active != 0,
            })
        })?;
        let mut profiles = rows.collect::<SqlResult<Vec<StorageProfile>>>()?;
        drop(stmt);
        let mut migrated = false;
        for p in &mut profiles {
            if p.secret_key.is_empty() {
                p.secret_key = load_secret(&p.id);
            } else if store_secret(&p.id, &p.secret_key).is_ok() {
                // Migrate a pre-keychain plaintext secret. Kept in the DB if the keychain is unavailable.
                conn.execute(
                    "UPDATE storage_profiles SET secret_key = '' WHERE id = ?1",
                    params![p.id],
                )?;
                migrated = true;
            }
        }
        if migrated {
            // Overwrite freed pages that still hold the old plaintext.
            conn.execute_batch("VACUUM")?;
        }
        Ok(profiles)
    }

    pub fn upsert_storage_profile(&self, profile: &StorageProfile) -> SqlResult<()> {
        store_secret(&profile.id, &profile.secret_key).map_err(keyring_err)?;
        let conn = self.conn.lock().unwrap();
        if profile.is_active {
            conn.execute("UPDATE storage_profiles SET is_active = 0", [])?;
        }
        conn.execute(
            "INSERT INTO storage_profiles (
                id, name, provider_type, endpoint, bucket, region, root_prefix, access_key, secret_key, is_active
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                provider_type = excluded.provider_type,
                endpoint = excluded.endpoint,
                bucket = excluded.bucket,
                region = excluded.region,
                root_prefix = excluded.root_prefix,
                access_key = excluded.access_key,
                secret_key = excluded.secret_key,
                is_active = excluded.is_active",
            params![
                profile.id,
                profile.name,
                profile.provider_type,
                profile.endpoint,
                profile.bucket,
                profile.region,
                profile.root_prefix,
                profile.access_key,
                "",
                if profile.is_active { 1 } else { 0 },
            ],
        )?;
        Ok(())
    }

    pub fn get_active_storage_profile(&self) -> SqlResult<Option<StorageProfile>> {
        Ok(self
            .list_storage_profiles()?
            .into_iter()
            .find(|p| p.is_active))
    }

    pub fn get_settings(&self) -> SqlResult<AppSettings> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT value_json FROM settings WHERE key = 'app'")?;
        let mut rows = stmt.query(params![])?;
        if let Some(row) = rows.next()? {
            let json_str: String = row.get(0)?;
            if let Ok(settings) = serde_json::from_str::<AppSettings>(&json_str) {
                return Ok(settings);
            }
        }
        drop(rows);
        drop(stmt);
        let default = AppSettings::default();
        let json_str = serde_json::to_string(&default).unwrap_or_default();
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value_json) VALUES ('app', ?1)",
            params![json_str],
        )?;
        Ok(default)
    }

    pub fn save_settings(&self, settings: &AppSettings) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        let json_str = serde_json::to_string(settings).unwrap_or_default();
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value_json) VALUES ('app', ?1)",
            params![json_str],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_tray_defaults_and_persistence() {
        let db = Database::open_in_memory().unwrap();
        let mut s = db.get_settings().unwrap();
        assert!(s.close_to_tray);
        assert!(!s.minimize_to_tray);
        assert!(s.ask_on_close);

        s.close_to_tray = false;
        s.minimize_to_tray = true;
        s.ask_on_close = false;
        db.save_settings(&s).unwrap();

        let loaded = db.get_settings().unwrap();
        assert!(!loaded.close_to_tray);
        assert!(loaded.minimize_to_tray);
        assert!(!loaded.ask_on_close);
    }

    /// Touches the real OS keychain: `cargo test -- --ignored secret_lives_in_keychain`
    #[test]
    #[ignore]
    fn secret_lives_in_keychain() {
        let db = Database::open_in_memory().unwrap();
        let id = format!("test-{}", uuid::Uuid::new_v4());
        let mut p = StorageProfile {
            id: id.clone(),
            name: "t".into(),
            provider_type: "vnsync_server".into(),
            endpoint: String::new(),
            bucket: String::new(),
            region: String::new(),
            root_prefix: String::new(),
            access_key: String::new(),
            secret_key: "hunter2".into(),
            is_active: false,
        };
        let raw = |db: &Database| -> String {
            db.conn
                .lock()
                .unwrap()
                .query_row("SELECT secret_key FROM storage_profiles", [], |r| r.get(0))
                .unwrap()
        };

        db.upsert_storage_profile(&p).unwrap();
        assert_eq!(raw(&db), "");
        assert_eq!(db.list_storage_profiles().unwrap()[0].secret_key, "hunter2");

        // Legacy plaintext row migrates into the keychain on read.
        db.conn
            .lock()
            .unwrap()
            .execute("UPDATE storage_profiles SET secret_key = 'legacy'", [])
            .unwrap();
        assert_eq!(db.list_storage_profiles().unwrap()[0].secret_key, "legacy");
        assert_eq!(raw(&db), "");
        assert_eq!(load_secret(&id), "legacy");

        p.secret_key.clear();
        db.upsert_storage_profile(&p).unwrap();
        assert_eq!(load_secret(&id), "");
    }
}
