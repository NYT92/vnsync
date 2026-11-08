# VNSync

this app is totally vibe-coded by me cuz i really need it right now...

Cross-platform Visual Novel & RPG save synchronization manager built with **Tauri v2 (Rust)** and **Vue 3 + Nuxt UI v4**.

## Features

- **Cross-OS & Steam Deck Save Path Resolver**: Stores portable save path rules (`<game_dir>`, `<win_appdata>`, `<win_locallow>`, `<win_documents>`) and translates them automatically across Windows, macOS, and Linux Wine/Proton `compatdata` prefixes.
- **Engine Fingerprinting**: Automatically detects save directories for Ren'Py, RPG Maker (MV, MZ, VX Ace, VX, XP), Kirikiri / KAG (`.xp3`), and Unity games.
- **Content-Addressable Save Vault**: Packs save directories into deterministic `.tar.zst` archives hashed with **BLAKE3**. Keeps local safety snapshots before any restore or sync overwrite.
- **Lineage Conflict Detection & Resolution UI**: Tracks parent snapshot IDs and BLAKE3 tree hashes to perform safe fast-forward push/pull or surface side-by-side conflict resolution when two devices diverge.
- **Unified Storage Backends (Apache OpenDAL)**: Sync via S3-compatible object storage (Cloudflare R2, AWS S3, MinIO, Backblaze B2), WebDAV (Nextcloud, ownCloud, Synology), or local/shared network folders.
- **VNDB API v2 Integration**: Search and auto-populate Visual Novel titles, Japanese original titles, cover art, developers, release dates, and descriptions.
- **Game Launcher & Playtime Tracker**: Launches games with pre-flight save pull, tracks playtime duration, and snapshots saves upon exit.

## Development

### Prerequisites
- **Rust** (stable toolchain via `rustup`)
- **Node.js** (v20.19+ or v22.12+)

### Setup & Run
```bash
# Install frontend dependencies
cd frontend
npm install

# Run Rust backend unit tests
cd ../src-tauri
cargo test

# Launch Tauri desktop app in development mode
cargo tauri dev
```

## License

GPL-3.0
