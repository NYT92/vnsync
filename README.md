# VNSync

this app is totally vibe-coded by me cuz i really need it right now...

Cross-platform Visual Novel & RPG save synchronization manager built with **Tauri v2 (Rust)** and **Vue 3 + Nuxt UI v4**.

<img width="1349" height="854" alt="vnsync_YppzdbcNQ0" src="https://github.com/user-attachments/assets/d30f50b4-9616-441d-8048-4a5b0b9ba5ff" />

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

## Self-hosted server

`server/` is a small sync server you can run on a home server or VPS instead of S3/WebDAV. It rejects a push when another device already moved the save forward, keeps the last `KEEP_SNAPSHOTS` snapshots per game, and lists them at `GET /api/games/<game id>/history`.

```bash
cd server
npm install
cp .env.example .env   # set SECRET to a long random string
npm start              # needs Node.js v24.2+
```

Or deploy it to Railway in one click and use the `https://` URL it gives you:

[![Deploy on Railway](https://railway.com/button.svg)](https://railway.com/deploy/6Ym7Dg?referralCode=nyt92railway&utm_medium=integration&utm_source=template&utm_campaign=generic)

In the app: **Settings → Cloud & Storage Backend → VNSync Server (Self-Hosted)**, then enter the server URL (for example `http://192.168.1.10:3000`) and the `SECRET` as token.

The token travels in a request header. Put the server behind an HTTPS reverse proxy before you expose it outside your home network.

## Caution

This app is still in development and 90% vibe-coded with the user guided manual. With this proof and working concept, we might build the actual app with many more feature based on my own plan. Use it or not it up to you because the app still have some bug in code to be fixed.

## License

GPL-3.0
