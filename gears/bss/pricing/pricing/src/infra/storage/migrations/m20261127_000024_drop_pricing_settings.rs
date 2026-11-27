//! D-543: the `pricing_settings` table is dropped.
//!
//! A forward migration. The tenant settings are declared in the Settings Service, read from it and
//! written there (D-540, D-541, D-542); nothing in the gear reads or writes the table, and its
//! rows are not carried anywhere (D-541). No other table references it.
//!
//! `DROP TABLE IF EXISTS` on both dialects, so a replay changes nothing; it runs inside the
//! toolkit runner's transaction. The rows go with the table.
//!
//! `down()` creates the table again, empty, in the shape `m20260926_000001` and
//! `m20260927_000014` left it: the same columns in the same order, the same CHECK and key. The
//! rows are not restored. The `SQLite` text is the text that database held before the drop, so a
//! schema dump taken after `down()` equals the one taken before `up()`.
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const PG_UP: &[&str] = &["DROP TABLE IF EXISTS bss.pricing_settings"];
const SQLITE_UP: &[&str] = &["DROP TABLE IF EXISTS pricing_settings"];

const PG_DOWN: &[&str] = &[r"CREATE TABLE IF NOT EXISTS bss.pricing_settings (
  tenant_id uuid PRIMARY KEY, default_timing text NOT NULL CHECK (default_timing IN ('advance','arrears')),
  default_gl text, default_tax_category text,
  invoice_line_templates jsonb NOT NULL, version bigint NOT NULL DEFAULT 1,
  created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL,
  currencies jsonb NOT NULL DEFAULT '[]'::jsonb, updated_by uuid
)"];
/// The text of `m20260926_000001` with the two columns `m20260927_000014` appended, placed as
/// `SQLite`'s `ADD COLUMN` writes them into the stored text.
const SQLITE_DOWN: &[&str] = &[r"CREATE TABLE IF NOT EXISTS pricing_settings (
  tenant_id text PRIMARY KEY, default_timing text NOT NULL CHECK (default_timing IN ('advance','arrears')),
  default_gl text, default_tax_category text,
  invoice_line_templates text NOT NULL, version integer NOT NULL DEFAULT 1,
  created_at text NOT NULL, updated_at text NOT NULL
, currencies text NOT NULL DEFAULT '[]', updated_by text)"];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        super::exec_backend(self.name(), manager, PG_UP, SQLITE_UP).await
    }

    /// The table again, empty, in its last shape.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        super::exec_backend(self.name(), manager, PG_DOWN, SQLITE_DOWN).await
    }
}
