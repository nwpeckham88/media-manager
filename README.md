# Media Manager (v1.0 baseline)

Rust backend + SvelteKit frontend service for Jellyfin-oriented media management.

## Features in this baseline

- Toolchain diagnostics for `ffmpeg`, `ffprobe`, `mediainfo`
- Library scan summary (`MM_LIBRARY_ROOTS`)
- Sidecar `.mm.json` workflow with deterministic parity:
  - dry-run
  - apply (requires approved plan hash)
  - rollback (operation snapshot restore)
- Path safety checks (operations only inside configured library roots)
- Preflight diagnostics endpoint
- Runtime branding config endpoint
- Dark/light UI theme toggle with persisted preference
- Optional API bearer token auth (`MM_API_TOKEN`)
- Persistent SQLite audit log for operation history
- Versioned SQLite schema migrations at startup (`schema_migrations` table)

## Local run

1. Configure env (copy from `.env.example`):

```bash
cp .env.example .env
```

2. Build frontend:

```bash
cd frontend
pnpm install
pnpm build
cd ..
```

3. Run backend:

```bash
cargo run
```

4. Open:

- UI: `http://127.0.0.1:8080/`
- Health: `http://127.0.0.1:8080/api/health`

## API highlights

- `GET /api/health`
- `GET /api/config/app`
- `GET /api/config/branding`
- `GET /api/diagnostics/toolchain`
- `GET /api/diagnostics/preflight`
- `GET /api/scan/summary`
- `GET /api/operations/recent?limit=20`
- `GET /api/jobs/recent?limit=20`
- `POST /api/sidecar/dry-run`
- `POST /api/sidecar/apply`
- `POST /api/sidecar/rollback`

Mutating sidecar operations are blocked with `424 Failed Dependency` when preflight is not ready.

## Auth

If `MM_API_TOKEN` is set, all protected API calls require:

```http
Authorization: Bearer <token>
```

Unauthenticated endpoints:

- `GET /api/health`
- `GET /api/config/branding`

## Container run

```bash
docker compose up -d --build
```

Then open `http://127.0.0.1:8080/`.

Update host media mount paths in `docker-compose.yml` as needed.

## Orange Pi 5 Pro (ARM64) compose deployment

Create a target-specific env file locally:

```bash
cp deploy/.env.orange-pi.example .env.orange-pi
```

Set these vars in `.env.orange-pi` for your Orange Pi:

- `HOST_MEDIA_MOVIES_PATH`
- `HOST_MEDIA_TV_PATH`
- `HOST_STATE_DIR`
- `MM_LIBRARY_ROOTS`
- `MM_API_TOKEN` (optional)

Then sync with the helper script (it will copy and rename files on the target):

```bash
./deploy/sync-to-orange-pi.sh dietpi@192.168.2.4:/opt/media-manager
```

The script syncs repo files and then prepares target deploy names:

- `.env.orange-pi` -> `.env`
- `docker-compose.orange-pi.yml` -> `docker-compose.yml`

Run on the target host:

```bash
cd /opt/media-manager
docker compose up -d --build
```

Notes:

- `docker-compose.orange-pi.yml` pins `platform: linux/arm64/v8` for RK3588.
- Update host bind mounts in `docker-compose.orange-pi.yml` for your media paths.
- If you enable auth, set `MM_API_TOKEN` in the compose environment block.
- The runtime image installs pinned `jellyfin-ffmpeg` from `ffmpeg/jellyfin-ffmpeg-rk3588.env`.

## Update pinned jellyfin-ffmpeg

Refresh the pinned artifact metadata (URL + SHA256) when you want to update:

```bash
./tools/update-jellyfin-ffmpeg-manifest.sh
```

Optional distro/arch override:

```bash
./tools/update-jellyfin-ffmpeg-manifest.sh bookworm arm64
```

## Rsync to Orange Pi

Use this direct command:

```bash
rsync -az --info=progress2 --exclude-from deploy/rsync-excludes.txt ./ user@orange-pi:/opt/media-manager
```

If you want the direct command to also stage target deploy names:

```bash
ssh dietpi@192.168.2.4 "cd /opt/media-manager && cp -f .env.orange-pi .env && cp -f docker-compose.orange-pi.yml docker-compose.yml"
```

Or use the helper script:

```bash
./deploy/sync-to-orange-pi.sh user@orange-pi:/opt/media-manager
```
