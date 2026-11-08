import { invoke } from '@tauri-apps/api/core'

export interface Game {
  id: string
  title: string
  originalTitle: string
  vndbId: string
  engine: string
  developer: string
  publisher: string
  releaseDate: string
  genre: string
  rating: string
  description: string
  status: string
  progress: number
  thumbnail: string
  exePath: string
  protonPrefix: string
  savePaths: string[]
  playtimeSeconds: number
  lastPlayedAt: string | null
  headSnapshotId: string | null
  syncStatus: string
  exeExists?: boolean
}

export interface SnapshotRecord {
  id: string
  gameId: string
  parentId: string | null
  deviceId: string
  deviceName: string
  createdAt: string
  treeHash: string
  fileCount: number
  totalBytes: number
  localArchivePath: string
  note: string
  pinned: boolean
}

export interface StorageProfile {
  id: string
  name: string
  providerType: 's3' | 'webdav' | 'local_fs'
  endpoint: string
  bucket: string
  region: string
  rootPrefix: string
  accessKey: string
  secretKey: string
  isActive: boolean
}

export interface AppSettings {
  deviceId: string
  deviceName: string
  theme: string
  language: string
  autoSyncOnLaunch: boolean
  autoSyncOnExit: boolean
  backupBeforeSync: boolean
  maxLocalSnapshots: number
  enableLanP2p: boolean
  lanPort: number
  gameLibraryPath: string
  closeToTray: boolean
  minimizeToTray: boolean
  askOnClose: boolean
}

export interface DetectionResult {
  engine: string
  suggestedTitle: string
  canonicalSavePaths: string[]
  resolvedSavePaths: string[]
  existingSaveFiles: number
}

export interface SaveLocationEntry {
  id: string
  title: string
  developer: string
  engine: string
  vndbId: string
  exeNames: string[]
  savePaths: string[]
  notes: string
}

export interface SaveLocationDbStatus {
  source: 'default' | 'custom'
  entries: SaveLocationEntry[]
}

export interface VndbResult {
  id: string
  title: string
  alttitle: string
  released: string
  description: string
  imageUrl: string
  developer: string
  rating: number
}

export interface RemoteGameHead {
  gameId: string
  title: string
  headSnapshotId: string
  parentSnapshotId: string | null
  treeHash: string
  deviceId: string
  deviceName: string
  updatedAt: string
  fileCount: number
  totalBytes: number
}

export interface SyncOutcome {
  action: 'up_to_date' | 'pushed' | 'pulled' | 'conflict'
  message: string
  localSnapshot: SnapshotRecord | null
  remoteHead: RemoteGameHead | null
}

export interface LibrarySyncOutcome {
  added: number
  updated: number
  remappedPaths: number
  missingExeCount: number
  totalGames: number
  message: string
}

function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

export const VnSyncApi = {
  async getGames(): Promise<Game[]> {
    if (!isTauri()) return []
    return invoke<Game[]>('get_games')
  },

  async getGame(id: string): Promise<Game | null> {
    if (!isTauri()) return null
    return invoke<Game | null>('get_game', { id })
  },

  async saveGame(game: Game): Promise<Game> {
    return invoke<Game>('save_game', { game })
  },

  async relocateGamePath(gameId: string, selectedPath: string): Promise<Game> {
    return invoke<Game>('relocate_game_path', { gameId, selectedPath })
  },

  async healGamePaths(): Promise<LibrarySyncOutcome> {
    return invoke<LibrarySyncOutcome>('heal_game_paths')
  },

  async syncLibraryNow(direction?: 'push' | 'pull'): Promise<LibrarySyncOutcome> {
    return invoke<LibrarySyncOutcome>('sync_library_now', {
      direction: direction ?? null
    })
  },

  async exportLibraryBackup(filePath: string): Promise<string> {
    return invoke<string>('export_library_backup', { filePath })
  },

  async importLibraryBackup(filePath: string): Promise<LibrarySyncOutcome> {
    return invoke<LibrarySyncOutcome>('import_library_backup', { filePath })
  },

  async removeGame(id: string): Promise<void> {
    return invoke<void>('remove_game', { id })
  },

  async detectGameEngine(exePath: string, protonPrefix = ''): Promise<DetectionResult> {
    return invoke<DetectionResult>('detect_game_engine', { exePath, protonPrefix })
  },

  async getSaveLocationDb(): Promise<SaveLocationDbStatus> {
    if (!isTauri()) return { source: 'default', entries: [] }
    return invoke<SaveLocationDbStatus>('get_save_location_db')
  },

  async loadCustomSaveLocations(filePath: string): Promise<SaveLocationDbStatus> {
    return invoke<SaveLocationDbStatus>('load_custom_save_locations', { filePath })
  },

  async resetSaveLocations(): Promise<SaveLocationDbStatus> {
    return invoke<SaveLocationDbStatus>('reset_save_locations')
  },

  async previewResolvedPaths(
    canonicalPaths: string[],
    exePath: string,
    protonPrefix = ''
  ): Promise<string[]> {
    if (!isTauri()) return canonicalPaths
    return invoke<string[]>('preview_resolved_paths', {
      canonicalPaths,
      exePath,
      protonPrefix
    })
  },

  async searchVndb(query: string): Promise<VndbResult[]> {
    if (!isTauri()) return []
    return invoke<VndbResult[]>('search_vndb_metadata', { query })
  },

  async listSnapshots(gameId: string): Promise<SnapshotRecord[]> {
    if (!isTauri()) return []
    return invoke<SnapshotRecord[]>('list_game_snapshots', { gameId })
  },

  async createManualSnapshot(gameId: string, note: string): Promise<SnapshotRecord> {
    return invoke<SnapshotRecord>('create_manual_snapshot', { gameId, note })
  },

  async restoreSnapshot(gameId: string, snapshotId: string): Promise<number> {
    return invoke<number>('restore_game_snapshot', { gameId, snapshotId })
  },

  async syncGameNow(
    gameId: string,
    forceDirection?: 'push' | 'pull'
  ): Promise<SyncOutcome> {
    return invoke<SyncOutcome>('sync_game_now', {
      gameId,
      forceDirection: forceDirection ?? null
    })
  },

  async getStorageProfiles(): Promise<StorageProfile[]> {
    if (!isTauri()) return []
    return invoke<StorageProfile[]>('get_storage_profiles')
  },

  async saveStorageProfile(profile: StorageProfile): Promise<StorageProfile> {
    return invoke<StorageProfile>('save_storage_profile', { profile })
  },

  async testStorageProfile(profile: StorageProfile): Promise<string> {
    return invoke<string>('test_storage_profile', { profile })
  },

  async getAppSettings(): Promise<AppSettings> {
    return invoke<AppSettings>('get_app_settings')
  },

  async saveAppSettings(settings: AppSettings): Promise<AppSettings> {
    return invoke<AppSettings>('save_app_settings', { settings })
  },

  async getRunningGames(): Promise<string[]> {
    if (!isTauri()) return []
    return invoke<string[]>('get_running_games')
  },

  async launchGame(gameId: string): Promise<string> {
    return invoke<string>('launch_game', { gameId })
  },

  async hideToTray(): Promise<void> {
    if (!isTauri()) return
    return invoke<void>('hide_to_tray')
  },

  async quitApp(): Promise<void> {
    if (!isTauri()) return
    return invoke<void>('quit_app')
  }
}

export function formatPlaytime(seconds: number): string {
  if (!seconds || seconds <= 0) return '0m'
  const hours = Math.floor(seconds / 3600)
  const mins = Math.floor((seconds % 3600) / 60)
  if (hours === 0) return `${ Math.max(1, mins) }m`
  return `${ hours }h ${ mins }m`
}

export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  const k = 1024
  const sizes = ['B', 'KB', 'MB', 'GB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return `${ parseFloat((bytes / Math.pow(k, i)).toFixed(1)) } ${ sizes[i] }`
}
