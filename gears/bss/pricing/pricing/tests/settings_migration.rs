//! The two forward migrations of `pricing_settings` on `SQLite`, inside the toolkit runner's
//! transaction, on a database that holds a tenant's row.
//!
//! D-438: `m20260927_000014_settings_currencies_and_author` adds `pricing_settings.currencies` (a
//! JSON array, default `[]`) and `pricing_settings.updated_by` (nullable). D-543:
//! `m20261127_000024_drop_pricing_settings` drops the table with its rows.
//!
//! A file database is migrated by the gear's list without the two and holds one tenant's settings
//! row, written with the encodings the application wrote (a `Uuid` is a 16-byte blob; the
//! timestamps are copied from a row the repository wrote). The list then runs with 000014 and
//! applies it alone; the structure is proved by the schema dump and the table text, the row by its
//! columns. The whole list then applies 000024 alone, and the dump loses the table and nothing
//! else. The Postgres twin is `postgres_settings_migration.rs`.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod schema_dump;

use bss_pricing::infra::storage::entity::price_book;
use bss_pricing::infra::storage::repo::book_repo;
use bss_pricing::module::BssPricingGear;
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use serde_json::json;
use toolkit::contracts::DatabaseCapability;
use toolkit_db::migration_runner::{MigrationError, MigrationResult, run_migrations_for_testing};
use toolkit_db::secure::AccessScope;
use toolkit_db::{ConnectOpts, DBProvider, connect_db};
use uuid::Uuid;

const MIGRATION: &str = "m20260927_000014_settings_currencies_and_author";
const DROP: &str = "m20261127_000024_drop_pricing_settings";
const TENANT: Uuid = Uuid::from_u128(0x7e0a_0000_0000_0000_0000_0000_0014_0001);

/// A file database, so the runner's pool, the repositories and a raw connection see one schema.
struct Lite {
    /// The database's own temporary directory, removed with the `Lite` (the file, its `-wal` and
    /// its `-shm`).
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
}
impl Lite {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("pricing-settings-")
            .tempdir()
            .unwrap();
        let path = dir.path().join("db.sqlite3");
        Self { _dir: dir, path }
    }
    fn dsn(&self) -> String {
        format!("sqlite://{}?mode=rwc", self.path.display())
    }
    async fn pool(&self) -> toolkit_db::Db {
        connect_db(
            &self.dsn(),
            ConnectOpts {
                max_conns: Some(1),
                min_conns: Some(1),
                ..ConnectOpts::default()
            },
        )
        .await
        .unwrap()
    }
    /// Run the gear's list without the named migrations.
    async fn migrate(&self, without: &[&str]) -> Result<MigrationResult, MigrationError> {
        let chain = BssPricingGear::default()
            .migrations()
            .into_iter()
            .filter(|m| !without.contains(&m.name()))
            .collect();
        run_migrations_for_testing(&self.pool().await, chain).await
    }
    async fn raw(&self) -> DatabaseConnection {
        Database::connect(self.dsn()).await.unwrap()
    }
    async fn exec(&self, sql: &str) {
        let raw = self.raw().await;
        raw.execute_raw(Statement::from_string(DbBackend::Sqlite, sql.to_owned()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
        raw.close().await.unwrap();
    }
    async fn strings(&self, sql: &str) -> Vec<String> {
        let raw = self.raw().await;
        let rows = raw
            .query_all_raw(Statement::from_string(DbBackend::Sqlite, sql.to_owned()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"))
            .iter()
            .map(|row| row.try_get::<String>("", "v").unwrap())
            .collect();
        raw.close().await.unwrap();
        rows
    }
    async fn dump(&self) -> Vec<String> {
        let raw = self.raw().await;
        let dump = schema_dump::sqlite_dump(&raw).await;
        raw.close().await.unwrap();
        stanza_lines(&dump)
    }
    async fn ddl(&self) -> String {
        self.strings(
            "SELECT sql AS v FROM sqlite_master WHERE type = 'table' AND name = 'pricing_settings'",
        )
        .await
        .pop()
        .unwrap()
    }
    /// The settings row: every column, blobs as hex, as JSON.
    async fn row(&self) -> String {
        let columns = self
            .strings("SELECT name AS v FROM pragma_table_info('pricing_settings') ORDER BY cid")
            .await;
        let pairs = columns
            .iter()
            .map(|c| {
                format!(
                    "'{c}', CASE typeof(\"{c}\") WHEN 'blob' THEN 'x:' || hex(\"{c}\") ELSE \"{c}\" END"
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        self.strings(&format!(
            "SELECT json_object({pairs}) AS v FROM pricing_settings"
        ))
        .await
        .pop()
        .unwrap()
    }
}
/// Each dump line prefixed with the stanza it belongs to; the runner's ledger left out.
fn stanza_lines(dump: &str) -> Vec<String> {
    let mut stanza = String::new();
    let mut lines = Vec::new();
    for line in dump.lines() {
        if !line.starts_with(' ') {
            line.clone_into(&mut stanza);
        }
        if stanza.contains("toolkit_migrations") {
            continue;
        }
        if line.starts_with(' ') {
            lines.push(format!("{stanza} ::{line}"));
        } else {
            lines.push(line.to_owned());
        }
    }
    lines
}

/// A database before the two migrations: the chain without them and one settings row.
async fn seeded() -> Lite {
    let db = Lite::new();
    let before = db.migrate(&[MIGRATION, DROP]).await.unwrap();
    assert!(
        !before
            .applied_names
            .iter()
            .any(|n| n == MIGRATION || n == DROP),
        "the chain before this run"
    );
    // A book written by the repository gives the timestamps the application's encoding.
    let now = time::OffsetDateTime::now_utc();
    let provider = DBProvider::<toolkit_db::DbError>::new(db.pool().await);
    book_repo::insert(
        &provider.conn().unwrap(),
        &AccessScope::for_tenant(TENANT),
        price_book::Model {
            id: Uuid::from_u128(0xb0),
            tenant_id: TENANT,
            code: "eur".into(),
            name: "eur".into(),
            currency: "EUR".into(),
            valid_from: None,
            valid_until: None,
            description: None,
            version: 1,
            created_at: now,
            updated_at: now,
            archived_at: None,
            archived_by: None,
        },
    )
    .await
    .unwrap();
    db.exec(&format!(
        "INSERT INTO pricing_settings (tenant_id, default_timing, default_gl, \
         default_tax_category, invoice_line_templates, version, created_at, updated_at) \
         VALUES (X'{}', 'arrears', 'GL-1', 'std', '{{\"usage\":\"{{sku}} usage\"}}', 3, \
         (SELECT created_at FROM pricing_price_book), (SELECT updated_at FROM pricing_price_book))",
        TENANT.simple()
    ))
    .await;
    db
}

#[tokio::test]
async fn the_forward_migration_adds_currencies_and_updated_by_and_keeps_the_row() {
    let db = seeded().await;
    let dump_before = db.dump().await;
    let ddl_before = db.ddl().await;
    let row_before: serde_json::Value = serde_json::from_str(&db.row().await).unwrap();

    let result = db.migrate(&[DROP]).await.unwrap();
    assert_eq!(result.applied_names, [MIGRATION], "only 000014 was pending");

    // The structure: exactly two columns added to pricing_settings.
    let dump_after = db.dump().await;
    let removed: Vec<&String> = dump_before
        .iter()
        .filter(|l| !dump_after.contains(l))
        .collect();
    let added: Vec<&String> = dump_after
        .iter()
        .filter(|l| !dump_before.contains(l))
        .collect();
    let show = |lines: &[&String]| {
        lines
            .iter()
            .map(|l| l.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    };
    eprintln!(
        "D-438 SQLite dump diff\nremoved:\n  {}\nadded:\n  {}",
        show(&removed),
        show(&added)
    );
    assert!(removed.is_empty(), "{removed:#?}");
    assert_eq!(
        added,
        [
            "TABLE pricing_settings ::  COLUMN currencies TEXT NOT NULL DEFAULT '[]' PK 0",
            "TABLE pricing_settings ::  COLUMN updated_by TEXT NULL DEFAULT - PK 0",
        ]
    );
    let ddl_after = db.ddl().await;
    eprintln!("D-438 SQLite table text after:\n{ddl_after}");
    assert_eq!(
        ddl_after.replace(
            ", currencies text NOT NULL DEFAULT '[]', updated_by text",
            ""
        ),
        ddl_before,
        "ADD COLUMN appends the two columns and nothing else"
    );
    // The row survives: every old column as it was, `currencies` empty, `updated_by` NULL.
    let mut row_after: serde_json::Value = serde_json::from_str(&db.row().await).unwrap();
    let obj = row_after.as_object_mut().unwrap();
    assert_eq!(obj.remove("currencies"), Some(json!("[]")));
    assert_eq!(obj.remove("updated_by"), Some(json!(null)));
    assert_eq!(row_after, row_before);
    // An upgraded database and one built in a single run hold the same schema.
    let fresh = Lite::new();
    fresh.migrate(&[DROP]).await.unwrap();
    assert_eq!(dump_after, fresh.dump().await);
    // A second run applies nothing.
    assert!(db.migrate(&[DROP]).await.unwrap().applied_names.is_empty());
}

#[tokio::test]
async fn the_drop_removes_the_table_with_its_row_and_nothing_else() {
    let db = seeded().await;
    db.migrate(&[DROP]).await.unwrap();
    let dump_before = db.dump().await;
    assert_eq!(
        db.strings("SELECT count(*) || '' AS v FROM pricing_settings")
            .await,
        ["1"],
        "the table holds the tenant's row"
    );

    let result = db.migrate(&[]).await.unwrap();
    assert_eq!(result.applied_names, [DROP], "only 000024 was pending");

    // The structure: the table's own lines are gone, every other line is as it was.
    let dump_after = db.dump().await;
    let removed: Vec<&String> = dump_before
        .iter()
        .filter(|l| !dump_after.contains(l))
        .collect();
    let added: Vec<&String> = dump_after
        .iter()
        .filter(|l| !dump_before.contains(l))
        .collect();
    assert!(added.is_empty(), "{added:#?}");
    assert!(
        !removed.is_empty()
            && removed
                .iter()
                .all(|l| l.starts_with("TABLE pricing_settings")),
        "{removed:#?}"
    );
    assert!(
        !dump_after.iter().any(|l| l.contains("pricing_settings")),
        "nothing of the table is left"
    );
    assert!(
        db.strings("SELECT name AS v FROM sqlite_master WHERE name = 'pricing_settings'")
            .await
            .is_empty()
    );
    // An upgraded database and a fresh one hold the same schema.
    let fresh = Database::connect("sqlite::memory:").await.unwrap();
    assert_eq!(
        dump_after,
        stanza_lines(&schema_dump::migrate_and_dump_sqlite(&fresh).await)
    );
    // A second run applies nothing.
    assert!(db.migrate(&[]).await.unwrap().applied_names.is_empty());
}
