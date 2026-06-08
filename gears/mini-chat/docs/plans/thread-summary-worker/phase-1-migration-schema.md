# Phase 1: Migration + Entity — Composite Summary Frontier

## Goal

Migrate `thread_summaries` table to use a composite frontier `(summarized_up_to_created_at, summarized_up_to_message_id)` as required by DESIGN.md, and create the SeaORM entity.

## Current State

- `thread_summaries` table exists in `m20260302_000001_initial.rs:186-197` with schema:
  `id, tenant_id, chat_id, summary_text, summarized_up_to (UUID), token_estimate, created_at, updated_at`
- The `summarized_up_to` column is a single UUID — DESIGN.md requires a composite pair
  `(summarized_up_to_created_at TIMESTAMPTZ, summarized_up_to_message_id UUID)` for the
  strict total order `(created_at ASC, id ASC)`.
- No SeaORM entity file exists for `thread_summaries` (no `src/infra/db/entity/thread_summary.rs`).
- Domain model `ThreadSummaryModel` at `src/domain/repos/thread_summary_repo.rs:13-17`
  already has `boundary_message_id: Uuid` and `boundary_created_at: OffsetDateTime` — aligned
  with the composite frontier. Field names differ from DB columns (domain vs persistence naming).

## Tasks

### 1.1 New migration file

File: `src/infra/db/migrations/m20260330_000001_thread_summary_composite_frontier.rs` (new)

**Postgres UP:**
```sql
-- Rename the single-UUID column to the message_id component.
ALTER TABLE thread_summaries
    RENAME COLUMN summarized_up_to TO summarized_up_to_message_id;

-- Add the created_at component of the composite frontier.
ALTER TABLE thread_summaries
    ADD COLUMN summarized_up_to_created_at TIMESTAMPTZ;

-- The table has UNIQUE(chat_id), so at most one row per chat.
-- No backfill needed: the table is empty in P1 (no summary rows exist yet).
-- If any rows existed, we would backfill from messages:
--   UPDATE thread_summaries ts
--   SET summarized_up_to_created_at = (
--       SELECT m.created_at FROM messages m WHERE m.id = ts.summarized_up_to_message_id
--   )
--   WHERE summarized_up_to_created_at IS NULL;

-- Make NOT NULL (safe: table is empty in production).
ALTER TABLE thread_summaries
    ALTER COLUMN summarized_up_to_created_at SET NOT NULL;
```

**SQLite UP:**
```sql
-- SQLite does not support RENAME COLUMN on older versions; use the
-- standard ALTER TABLE ... RENAME COLUMN syntax (supported since 3.25).
ALTER TABLE thread_summaries
    RENAME COLUMN summarized_up_to TO summarized_up_to_message_id;

ALTER TABLE thread_summaries
    ADD COLUMN summarized_up_to_created_at TEXT NOT NULL DEFAULT '1970-01-01T00:00:00Z';
```

**DOWN:**
```sql
ALTER TABLE thread_summaries DROP COLUMN summarized_up_to_created_at;
ALTER TABLE thread_summaries RENAME COLUMN summarized_up_to_message_id TO summarized_up_to;
```

Design notes:
- The table is empty in production (no summary has ever been committed). The migration
  is safe to run without backfill.
- `summarized_up_to_created_at` + `summarized_up_to_message_id` together form the
  inclusive summary frontier per DESIGN.md section "Thread Summary - Stable Range and
  Commit Invariant."
- A missing `thread_summaries` row means no messages have been summarized (empty frontier).

### 1.2 Register migration

File: `src/infra/db/migrations/mod.rs`

Add the new gear declaration and append the migration to `Migrator::migrations()` vec,
after the last existing migration entry.

### 1.3 Create SeaORM entity

File: `src/infra/db/entity/thread_summary.rs` (new)

```rust
use sea_orm::entity::prelude::*;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "thread_summaries")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub chat_id: Uuid,
    pub summary_text: String,
    pub summarized_up_to_created_at: OffsetDateTime,
    pub summarized_up_to_message_id: Uuid,
    pub token_estimate: i32,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::chat::Entity",
        from = "Column::ChatId",
        to = "super::chat::Column::Id"
    )]
    Chat,
}

impl Related<super::chat::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Chat.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
```

### 1.4 Register entity module

File: `src/infra/db/entity/mod.rs`

Add `pub mod thread_summary;` to the entity module declarations.

## Acceptance Criteria

- [ ] Migration runs successfully on both Postgres and SQLite
- [ ] `thread_summaries` table has `summarized_up_to_created_at TIMESTAMPTZ NOT NULL` and
      `summarized_up_to_message_id UUID NOT NULL` columns
- [ ] SeaORM entity `thread_summary::Model` compiles and reflects all DB columns
- [ ] Entity registered in `entity/mod.rs`
- [ ] Existing tests pass (no data in `thread_summaries` table, so migration is backward-compatible)
