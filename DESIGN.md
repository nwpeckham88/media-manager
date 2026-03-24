# Jellyfin Media Manager - DESIGN

## 1. Vision
Build a Rust-based media manager for Jellyfin users that runs in Docker, ingests mapped media libraries, and keeps libraries fast to scan, consistent, and highly organized through safe, deterministic automation.

## 2. Product Principles
- Jellyfin-first conventions by default.
- NFO metadata is authoritative for identity.
- No destructive mutation without dry-run plus explicit apply.
- Full rollback support for applied operations.
- Deterministic, idempotent pipelines.
- Power-user controls without hiding defaults.
- Codec/profile compatibility recommendations via streamer presets, with manual overrides always allowed.
- Product branding (display name and visual identity) must be configurable without code changes.
- Media tooling binaries are runtime-configurable, with first-class support for custom `ffmpeg` builds (especially `jellyfin-ffmpeg`).

## 3. Scope (Current)
- Metadata download and validation.
- NFO generation and maintenance next to media.
- File renaming and moving using Jellyfin conventions.
- Duplicate folder merge handling, including mixed provider-id histories.
- Audio track policy management by language/properties.
- Subtitle management by language and forced/default semantics.
- Added bedtime audio track pipeline for all media items (with explicit eligibility and skip reasons).
- Playback compatibility recommendations via built-in streamer capability profiles.
- First-class RK3588 (Orange Pi 5) deployment support, including ARM64 container assumptions and hardware-accelerated media tooling awareness.
- Dry-run/apply/rollback operational model.
- Frontend with SvelteKit + Tailwind + shadcn-svelte + Bits UI.

## 4. Out of Scope (Current)
- Confidence-explainer UI is deferred.
- Deep UX polish beyond operationally complete workflows is deferred.
- Broad device profile catalog beyond initial streamer set is deferred.

## 5. Architecture
- Single Rust backend service that also serves bundled frontend assets.
- Dockerized runtime with mapped library volumes.
- Internal components:
1. Scanner/Probe subsystem.
2. Metadata subsystem (provider adapters + NFO writer/validator).
3. Planner subsystem (rename/move/merge/subtitle/audio plans).
4. Executor subsystem (apply/checkpoint/rollback).
5. API subsystem (REST + realtime progress stream).
6. Policy subsystem (rules, presets, compatibility profiles).
7. State/Audit subsystem (database + per-item sidecar state).
8. Toolchain subsystem (binary discovery, capability probing, and execution wrappers for `ffmpeg`, `ffprobe`, and `mediainfo`).

### 5.1 Media Toolchain Strategy (RK3588-first)
- Primary target platform: RK3588 (Orange Pi 5) on ARM64.
- Preferred binary set for this platform: `jellyfin-ffmpeg` when available.
- For containerized deployments, bundle media tooling in this application's image rather than depending on binaries from a separate Jellyfin container.
- Runtime binary resolution order:
1. Explicit config paths.
2. Environment variable overrides.
3. Known binary names on `PATH` (`jellyfin-ffmpeg`, `ffmpeg`; matching `ffprobe`).
- On startup, run capability probes (`-version`, codec/encoder/decoder/filter checks, hwaccel listing) and persist a capability snapshot.
- Planner and executor must use capability-aware decisions and gracefully fall back when a requested codec/accelerator is unavailable.

### 5.2 Toolchain Packaging Policy
- Do not fetch floating `latest` ffmpeg assets during Docker build.
- Use pinned `jellyfin-ffmpeg` versions per architecture (for example `linux-arm64`) with checksum verification.
- Keep the pinned artifact metadata in-repo (version, URL, sha256) and make updates explicit via pull requests.
- Optional helper script is allowed, but it should update pinned metadata (manifest/lock file) and not be executed automatically during image build.
- Docker build must be reproducible: same git commit should resolve the same ffmpeg binaries.

## 6. Identity and Metadata Strategy
- Canonical dedupe identity: TMDB primary.
- Secondary identities: IMDb and TVDB.
- Jellyfin source-of-truth identity: NFO provider IDs.
- Folder/file embedded ID tags are treated as migration hints only.
- Metadata reconcile job validates:
1. NFO IDs.
2. Provider lookup consistency.
3. Local media/title/year/runtime consistency.
- Low-confidence items are queued for manual review before apply.

## 7. Naming and Organization
- Enforce Jellyfin-compliant naming for Movies and TV.
- Keep edition/version variants as sibling variants, not merged destructively.
- Merge engine uses two-phase protocol:
1. Equivalence and conflict catalog generation.
2. Controlled apply with explicit per-conflict strategy.
- Support do-not-touch protections by path, item ID, and metadata tag.

## 8. Audio and Subtitle Design
- Audio decision engine selects/marks tracks by language, channels, codec traits, and preference policy.
- Subtitle policy normalizes language/default/forced state and handles ambiguous forced cases via conflict state.
- Image-based subtitle workflows are policy-driven and explicitly tracked.
- Bedtime profile:
1. Adds an additional derived track for each media item when eligible.
2. Produces stereo, normalized, bounded-dynamics output for dialog clarity.
3. Uses configurable codec/profile targets (for example Opus).
4. Is idempotent via profile-version and source-stream fingerprints.
5. Stores skip reasons for non-eligible items.
- ffmpeg invocation must support custom binary paths and per-profile execution flags (for example hardware acceleration args when validated for RK3588).

## 9. Compatibility Profile System
- Built-in streamer capability array starts with:
1. Onn 4K Pro.
2. Nvidia Shield Pro.
- Streamer profile selection influences recommendations.
- Manual preferred format override always wins.
- Policy modes:
1. Recommended mode suggests best-fit format.
2. Enforced mode executes transcode/selection decisions.

## 10. Sidecar State File
Use `.mm.json` as the sidecar filename.

Purpose:
- Store expected/preferred state snapshots.
- Track metadata schema versions.
- Track derived artifact ownership and lineage.
- Track pipeline fingerprints for idempotency.
- Track reconcile status and drift markers.

Core fields:
- `schema_version`
- `item_uid`
- `source_fingerprints`
- `provider_ids`
- `nfo_state`
- `preferred_policy_state`
- `applied_state`
- `derived_artifacts`
- `last_reconcile_result`
- `last_operation_id`
- `protected_flags`

Rules:
- Sidecar is machine-managed.
- Sidecar changes are atomic with apply checkpoints.
- Sidecar is rollback-aware.
- Sidecar is not the only source of truth, but the operational truth for this tool.

## 11. Safety Model
- Hard preflight checks before apply:
1. Mount writability.
2. Rename/move behavior.
3. Temp-file creation.
4. fsync/atomic guarantees where possible.
5. Toolchain checks for configured `ffmpeg`/`ffprobe`/`mediainfo` binaries and required codec/filter availability.
- Dry-run/apply parity guarantee: apply executes exactly what dry-run approved.
- Rollback snapshot required for any mutating operation.
- Interrupted-job recovery required with deterministic resume policy.
- Apply blocked if invariants cannot be guaranteed.

## 12. Operational Modes
- Scope-limited rollout:
1. Small subset.
2. Single folder.
3. Single series/movie collection.
4. Full library.
- Preset classes:
1. Safe presets: simpler operations, easier execution path.
2. Aggressive presets: stricter confirmation and conflict handling path.

## 13. Library Invariants (Post-Apply Validation)
- NFO exists and is parseable for managed items.
- No orphaned generated assets.
- Default/forced audio/subtitle flags are policy-consistent.
- Naming/path conventions remain Jellyfin-valid.
- Edition/version variants preserved correctly.
- Sidecar and filesystem state are consistent.

## 14. API and Frontend
Backend:
- REST for scan/inspect/plan/dry-run/apply/rollback/presets/rules.
- Realtime progress stream for long-running jobs.
- Contract-first API definitions and typed clients.

Frontend:
- SvelteKit app with Tailwind + shadcn-svelte + Bits UI.
- Branding is runtime-configurable (for example app name, logo path, browser title, sidebar title, and accent palette tokens).
- Core workflows:
1. Library browse and inspect.
2. Plan preview and diff.
3. Conflict resolution.
4. Apply monitoring.
5. Rollback and history.
6. Rules/presets management.
7. Compatibility profile selection with manual override control.

## 15. Storage and Audit
- Persistent DB for jobs, events, operation plans, conflicts, snapshots.
- Artifact ownership tracking for generated tracks/subtitles/NFO updates.
- Versioned golden fixture corpus to prevent behavior drift.
- Release gates based on deterministic expected outcomes.

## 16. Verification Strategy
- Unit tests for identity normalization, naming determinism, and policy logic.
- Integration tests for provider fallback and NFO roundtrip correctness.
- Executor tests for dry-run/apply parity and rollback integrity.
- Audio/subtitle validation tests via probe assertions.
- Bedtime generation coverage tests and loudness/peak conformance checks.
- E2E tests for scan -> dry-run -> resolve -> apply -> rollback.
- Long-running interruption/recovery tests.
- Performance benchmarks on large synthetic libraries.

## 17. Implementation Phases
1. Bootstrap repo, core architecture, and schema.
2. Scanner/probe and metadata/NFO foundation.
3. Planner for rename/move/merge plus subtitle/audio policy.
4. Executor with checkpoints and rollback.
5. API and realtime progress.
6. Frontend workflows.
7. Docker/ops hardening.
8. Validation, performance, release hardening.

## 18. Current Decisions Log
- Movies + TV are in MVP.
- NFO IDs are authoritative for Jellyfin identity.
- Bedtime track is an added derived track for all eligible media.
- Streamer profile library starts with Onn 4K Pro and Nvidia Shield Pro.
- Manual format override is always allowed even when streamer profile is selected.
- `.jfmm-state.json` sidecar is adopted.
- Confidence-explainer UI is excluded for now.
- Program name is not finalized; display branding must be easy to change via configuration.
- RK3588 (Orange Pi 5) is a primary deployment target, not a secondary compatibility target.
- Custom ffmpeg paths are first-class; default preference is `jellyfin-ffmpeg` where available.
- Media-manager image should include its own pinned ffmpeg toolchain; it must not rely on cross-container access to Jellyfin's binaries.

## 18.1 Branding Configuration Model
- Keep a stable internal service id (for example `jfmm`) and separate it from user-facing branding.
- Add a config block such as:
- `branding.app_name`
- `branding.short_name`
- `branding.logo_url`
- `branding.browser_title_template`
- `branding.theme_tokens`
- Serve branding config through API (for example `/api/config/branding`) so frontend reads it at runtime.
- Avoid embedding product name literals in frontend routes/components beyond a single branding provider.
- Persist default branding in config file or env-backed settings, not in compiled constants.

## 19. Gaps and Decisions Still Needed
These are the main unresolved items before implementation should start in earnest.

1. Metadata provider auth and quotas
- Decide which provider APIs need keys in MVP and how rate limits are handled.
- Decide caching TTL and offline behavior when providers fail.

2. TVDB integration depth
- Decide whether TVDB is a hard dependency for TV in MVP or optional fallback only.
- Decide behavior when TVDB plugin/data is unavailable.

3. Bedtime target profile defaults
- Lock default loudness/true-peak targets, codec, bitrate, and channel policy.
- Define when an existing track is already "good enough" to skip derivation.

4. Sidecar lifecycle and coexistence
- Decide whether sidecars are always next to media or optionally in central DB-only mode.
- Decide how sidecars behave on external file moves done outside this tool.

5. Conflict policy defaults
- Define default action classes for collisions: skip, rename-suffix, replace, manual only.
- Define which conflict classes are never auto-resolvable.

6. API auth model for container deployments
- Decide auth strategy for local-only usage vs remote access (none/token/session).
- Decide if single-user mode is enough for MVP.

7. Resource scheduling
- Decide default ffmpeg concurrency, CPU limits, and job priorities.
- Decide policy for throttling during active Jellyfin playback windows.

8. Subtitle OCR policy
- Decide if OCR for image-based subtitles is MVP or post-MVP.
- If MVP, decide engine/tooling and quality threshold.

9. Data model for editions/versions
- Finalize schema for theatrical/director/extended variants and UI representation.
- Define merge behavior when one variant has richer metadata than another.

10. Upgrade/migration strategy
- Define schema migration guarantees for DB and sidecar version changes.
- Define rollback policy when migrations partially fail.

11. RK3588 toolchain packaging and defaults
- Finalize pinned `jellyfin-ffmpeg` version/channel strategy for ARM64 updates.
- Finalize default hardware acceleration policy on RK3588 (enabled by default vs opt-in with validation).
- Decide minimum required ffmpeg capabilities for bedtime and general transcode pipelines.

## 20. Recommended Immediate Work
Since gaps still exist, implementation should start in parallel with decision closure:

1. Start Phase 1 now
- Scaffold Rust workspace, crate boundaries, and DB migrations.
- Implement domain entities and operation/event audit tables.

2. Close high-impact decisions first
- Bedtime defaults, metadata auth/rate-limit behavior, and conflict defaults.

3. Build a thin vertical slice
- Scan one folder -> resolve metadata -> write NFO -> dry-run rename plan -> apply/rollback.
- This validates core architecture before broad feature expansion.

4. Build RK3588-first toolchain slice
- Implement binary discovery + capability probe for `ffmpeg`/`ffprobe` with `jellyfin-ffmpeg` preference.
- Add startup diagnostics endpoint/report to show resolved binaries and available capabilities.
- Validate one bedtime-track transcode path on RK3588-targeted container image.
