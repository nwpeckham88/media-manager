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
