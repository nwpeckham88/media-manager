# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

### Backend (Rust)

```bash
cargo build                    # Build debug
cargo build --release          # Build release
cargo run                      # Run backend (serves bundled frontend at :8080)
cargo test                     # Run all tests
cargo test sidecar_workflow    # Run tests matching a pattern
cargo test -- --nocapture      # Show println! output
cargo fmt                      # Format code
cargo clippy -- -D warnings    # Lint (treat warnings as errors)
cargo check                    # Fast compile check without producing binary
```

### Frontend (SvelteKit + pnpm)

```bash
cd frontend
pnpm install                   # Install dependencies
pnpm build                     # Build static assets to frontend/build/
pnpm dev                       # Dev server with hot reload (proxies API)
pnpm check                     # Type-check Svelte/TypeScript
```

### Full local dev cycle

```bash
# 1. Build frontend once (or run pnpm dev in a separate terminal)
cd frontend && pnpm build && cd ..

# 2. Start backend (also serves frontend/build/)
cargo run
# UI: http://127.0.0.1:8080/  Health: http://127.0.0.1:8080/api/health
```

### Environment

Copy `.env.example` to `.env` before first run. Key variables:

| Variable | Purpose |
|---|---|
| `MM_LIBRARY_ROOTS` | Colon-separated paths to media directories |
| `MM_API_TOKEN` | Optional bearer token auth (omit to disable) |
| `MM_STATE_DIR` | Directory for SQLite DB and sidecar snapshots (default: `.mm-state`) |
| `MM_PORT` | HTTP port (default: `8080`) |
| `MM_FFMPEG_PATH` / `MM_FFPROBE_PATH` | Override binary paths (prefer `jellyfin-ffmpeg`) |

## Architecture

### Overview

Single-process Rust binary (Axum) that also serves the pre-built SvelteKit frontend as static files from `frontend/build/`. There is no separate frontend server in production.

### Backend modules (`src/`)

| Module | Role |
|---|---|
| `main.rs` | Startup: config, toolchain probe, DB migrations, Axum router assembly |
| `config.rs` | `AppConfig` loaded entirely from environment variables |
| `api/routes.rs` | All REST route handlers and `AppState` (shared via `Arc`) |
| `auth.rs` | Bearer token middleware (applied to all `/api/*` except health/branding) |
| `scanner.rs` | Filesystem walk, library scan summary, library browse |
| `toolchain.rs` | ffmpeg/ffprobe/mediainfo discovery and capability probing at startup |
| `preflight.rs` | Mount writability + toolchain checks; mutations blocked with 424 when not ready |
| `sidecar_workflow.rs` | Dry-run → apply → rollback lifecycle for `.mm.json` sidecar files |
| `sidecar_store.rs` | Read/write `.mm.json` sidecar files from disk |
| `domain/sidecar.rs` | `SidecarState` and `DesiredMediaState` domain types |
| `operations.rs` | In-memory `OperationLog` for recent operation events |
| `audit_store.rs` | Persistent SQLite operation audit log |
| `jobs_store.rs` | Persistent SQLite job records (status, progress) |
| `db_migrations.rs` | Versioned schema migrations run at startup |
| `path_policy.rs` | Enforces that all file operations stay within configured `library_roots` |

### Key architectural patterns

**Dry-run / apply / rollback parity**: Every mutating operation follows this three-step model. `dry-run` generates a plan + `plan_hash`. `apply` requires the same `plan_hash` to guarantee it executes exactly what was previewed. `rollback` restores from a snapshot written atomically with the apply.

**Preflight gate**: `POST /api/sidecar/apply` and bulk apply endpoints return `424 Failed Dependency` if `run_preflight()` fails (mount not writable, toolchain missing). Always check `/api/diagnostics/preflight` before expecting mutations to work.

**Path safety**: `path_policy` validates every target path is under a configured `MM_LIBRARY_ROOTS` entry before any rename/write operation.

**AppState**: The single shared state struct, wrapped in `Arc`, injected into all handlers. Holds `AuditStore`, `JobsStore`, `OperationLog`, `ToolchainSnapshot`, and config fields. SQLite connections are opened per-request from the stored `audit_db_path`.

**Sidecar files**: `.mm.json` files live next to each media file and track the operational state of that item (applied state, rollback snapshots, provider IDs). They are the per-item source of truth for this tool.

### Frontend (`frontend/src/`)

SvelteKit app compiled to static output (`adapter-static`), served by the Rust backend.

**Routes** map to workflow stages:
- `/` — Dashboard, workflow hub with stage progression
- `/consolidation` — Exact/semantic duplicate detection and quarantine
- `/metadata` — Provider ID reconciliation (metadata stage)
- `/formatting` — Bulk rename to `Movie Name (Year)` convention
- `/queue` — Verify stage, pending job queue
- `/operations` — Centralized rollback and operation history
- `/library` — Advanced manual library browse
- `/onboarding` — First-run wizard

**Workflow state** (`src/lib/workflow/progress.ts`) is persisted in browser localStorage (`mm-workflow-progress-v1`) and synced from API heuristics on dashboard load.

**Shared components** (`src/lib/components/`): `StageSidebar`, `WorkflowProgress`, `ConfirmDialog`, `OperationResultBanner`.

### Database

SQLite at `MM_STATE_DIR/audit.sqlite3`. Schema is versioned via `db_migrations.rs` (`schema_migrations` table). Tables: `audit_log`, `jobs`, `media_index`.

### Deployment

- **Local**: `cargo run` with `frontend/build/` present
- **Docker**: `docker compose up -d --build` — single service, reads `.env` for bind mounts
- **Orange Pi 5 (ARM64)**: Use `deploy/sync-to-orange-pi.sh`; image bundles pinned `jellyfin-ffmpeg` from `ffmpeg/jellyfin-ffmpeg-rk3588.env`
