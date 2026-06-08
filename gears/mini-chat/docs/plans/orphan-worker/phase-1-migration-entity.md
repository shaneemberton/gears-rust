# Phase 1: Migration + Entity — Add `last_progress_at` Column

## Goal

Add the `last_progress_at` column to `chat_turns` so orphan detection can use durable stale-progress timestamps instead of raw age from `started_at`.

## Current State

- `chat_turns` table has no `last_progress_at` column (see initial migration at `src/infra/db/migrations/m20260302_000001_initial.rs`).
- Entity at `src/infra/db/entity/chat_turn.rs` has no `last_progress_at` field. Current columns end with `started_at`, `completed_at`, `updated_at`.
- `CreateTurnParams` at `src/domain/repos/turn_repo.rs` has no `last_progress_at` — the timestamp is always `now()` so it's not a caller-provided param.
- `create_turn` implementation at `src/infra/db/repo/turn_repo.rs` lines 22–55 sets `started_at: Set(now)` but has no `last_progress_at`.
- `OrphanWatchdogConfig` at `src/config/background.rs` line 14 already references the column in a doc comment.

## Tasks

### 1.1 New migration file

File: `src/infra/db/migrations/m20260329_000001_add_last_progress_at.rs` (new)

**Postgres UP:**
```sql
ALTER TABLE chat_turns ADD COLUMN last_progress_at TIMESTAMPTZ;

-- Backfill any existing running turns so orphan watchdog can evaluate them.
UPDATE chat_turns
   SET last_progress_at = started_at
 WHERE last_progress_at IS NULL
   AND state = 'running';

-- Partial index for efficient watchdog scans.
-- Only running, non-deleted turns with a progress timestamp participate.
CREATE INDEX IF NOT EXISTS idx_chat_turns_orphan_scan
    ON chat_turns (last_progress_at)
    WHERE state = 'running' AND deleted_at IS NULL;
```

**SQLite UP:**
```sql
ALTER TABLE chat_turns ADD COLUMN last_progress_at TEXT;

UPDATE chat_turns
   SET last_progress_at = started_at
 WHERE last_progress_at IS NULL
   AND state = 'running';

-- SQLite does not support partial indexes with WHERE on expressions involving
-- multiple columns reliably across all versions. Use a regular index instead;
-- the application-level query still filters correctly.
CREATE INDEX IF NOT EXISTS idx_chat_turns_orphan_scan
    ON chat_turns (last_progress_at);
```

**DOWN:**
```sql
DROP INDEX IF EXISTS idx_chat_turns_orphan_scan;
ALTER TABLE chat_turns DROP COLUMN last_progress_at;
```

Design notes:
- Column is nullable — terminal turns (`completed`, `failed`, `cancelled`) do not require `last_progress_at`. Only `running` turns must have a non-NULL value.
- No `CHECK` constraint at the DB level. SQLite does not support `ADD CONSTRAINT` via `ALTER TABLE`. The invariant (`running` → `last_progress_at IS NOT NULL`) is enforced by the application: `create_turn` always sets the value, and `cas_finalize_orphan` (Phase 3) does not clear it.
- The partial index `idx_chat_turns_orphan_scan` on Postgres dramatically narrows the scan to only rows the watchdog cares about.

### 1.2 Register migration

File: `src/infra/db/migrations/mod.rs`

Add the new gear declaration and append the migration to `Migrator::migrations()` vec, after the last existing migration entry.

### 1.3 Update entity

File: `src/infra/db/entity/chat_turn.rs`

Add to `Model` struct (after `started_at`):
```rust
pub last_progress_at: Option<OffsetDateTime>,
```

The field is `Option<OffsetDateTime>` because terminal turns may have NULL.

### 1.4 Initialize in `create_turn`

File: `src/infra/db/repo/turn_repo.rs`

In the `create_turn` method (lines 29–53), add to the `ActiveModel`:
```rust
last_progress_at: Set(Some(now)),
```

This satisfies the DESIGN.md requirement: "MUST be initialized when the turn enters `running`."

No change to `CreateTurnParams` is needed — `last_progress_at` is always `now()` at creation time, not a caller-provided parameter.

## Acceptance Criteria

- [ ] Migration runs successfully on both Postgres and SQLite
- [ ] Entity compiles with the new `last_progress_at` field
- [ ] `create_turn` sets `last_progress_at = now()`
- [ ] Partial index `idx_chat_turns_orphan_scan` exists (Postgres) for efficient watchdog queries
- [ ] Existing tests pass — new field is `Option`, backward compatible
