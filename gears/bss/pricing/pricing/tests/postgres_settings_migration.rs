//! D-438 and D-543 on Postgres: the same two forward migrations as `settings_migration.rs`,
//! tables in schema `bss`. A database is migrated by the gear's list without the two through the
//! toolkit runner and holds one tenant's settings row. The list then runs with 000014 and applies
//! it alone: the structure is proved by the Postgres schema dump, before and after and against a
//! database built in one run; the row by its columns. The whole list then applies 000024 alone,
//! and the dump loses the table and nothing else; its down then gives the dump back.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod pg_support;
mod schema_dump;

use bss_pricing::infra::storage::migrations::Migrator;
use bss_pricing::module::BssPricingGear;
use pg_support::Pg;
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::{MigratorTrait, SchemaManager};
use serde_json::{Value, json};
use toolkit::contracts::DatabaseCapability;
use toolkit_db::migration_runner::{MigrationError, MigrationResult, run_migrations_for_testing};
use uuid::Uuid;

const MIGRATION: &str = "m20260927_000014_settings_currencies_and_author";
const DROP: &str = "m20261127_000024_drop_pricing_settings";
/// The table's columns in their stored order, which the schema dump does not carry.
const COLUMNS_IN_ORDER: &str = "SELECT column_name::text AS v FROM information_schema.columns \
     WHERE table_schema = 'bss' AND table_name = 'pricing_settings' ORDER BY ordinal_position";
const TENANT: Uuid = Uuid::from_u128(0x7e0a_0000_0000_0000_0000_0000_0014_0002);

/// Run the gear's list without the named migrations.
async fn migrate(pg: &Pg, without: &[&str]) -> Result<MigrationResult, MigrationError> {
    let chain = BssPricingGear::default()
        .migrations()
        .into_iter()
        .filter(|m| !without.contains(&m.name()))
        .collect();
    run_migrations_for_testing(&pg.db().await, chain).await
}
async fn strings(pg: &Pg, sql: &str) -> Vec<String> {
    pg.raw()
        .await
        .query_all_raw(Statement::from_string(DbBackend::Postgres, sql.to_owned()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .iter()
        .map(|row| row.try_get::<String>("", "v").unwrap())
        .collect()
}
async fn dump(pg: &Pg) -> Vec<String> {
    schema_dump::postgres_dump(&pg.raw().await)
        .await
        .lines()
        .map(str::to_owned)
        .collect()
}
async fn row(pg: &Pg) -> Value {
    serde_json::from_str(
        &strings(
            pg,
            "SELECT row_to_json(s)::text AS v FROM bss.pricing_settings s",
        )
        .await
        .pop()
        .unwrap(),
    )
    .unwrap()
}
async fn seeded() -> Pg {
    let pg = Pg::empty().await;
    let before = migrate(&pg, &[MIGRATION, DROP]).await.unwrap();
    assert!(
        !before
            .applied_names
            .iter()
            .any(|n| n == MIGRATION || n == DROP),
        "the chain before this run"
    );
    pg.raw()
        .await
        .execute_raw(Statement::from_string(
            DbBackend::Postgres,
            format!(
                "INSERT INTO bss.pricing_settings (tenant_id, default_timing, default_gl, \
                 default_tax_category, invoice_line_templates, version, created_at, \
                 updated_at) VALUES ('{TENANT}'::uuid, 'arrears', 'GL-1', 'std', \
                 '{{\"usage\":\"{{sku}} usage\"}}'::jsonb, 3, '2026-09-01T09:00:00Z', \
                 '2026-09-02T10:00:00Z')"
            ),
        ))
        .await
        .unwrap();
    pg
}

#[tokio::test]
#[ignore = "needs the Postgres harness"]
async fn postgres_the_forward_migration_adds_currencies_and_updated_by_and_keeps_the_row() {
    let pg = seeded().await;
    let dump_before = dump(&pg).await;
    let row_before = row(&pg).await;

    let result = migrate(&pg, &[DROP]).await.unwrap();
    assert_eq!(result.applied_names, [MIGRATION], "only 000014 was pending");

    let dump_after = dump(&pg).await;
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
        "D-438 Postgres dump diff\nremoved:\n  {}\nadded:\n  {}",
        show(&removed),
        show(&added)
    );
    assert!(removed.is_empty(), "{removed:#?}");
    assert_eq!(
        added,
        [
            "COLUMN bss.pricing_settings currencies jsonb NOT NULL DEFAULT '[]'::jsonb",
            "COLUMN bss.pricing_settings updated_by uuid NULL DEFAULT -",
        ]
    );
    let mut row_after = row(&pg).await;
    let obj = row_after.as_object_mut().unwrap();
    assert_eq!(obj.remove("currencies"), Some(json!([])));
    assert_eq!(obj.remove("updated_by"), Some(json!(null)));
    assert_eq!(row_after, row_before, "every old column as it was");
    let fresh = Pg::empty().await;
    migrate(&fresh, &[DROP]).await.unwrap();
    assert_eq!(
        dump_after,
        dump(&fresh).await,
        "upgraded and built in one run agree"
    );
    assert!(
        migrate(&pg, &[DROP])
            .await
            .unwrap()
            .applied_names
            .is_empty()
    );
}

#[tokio::test]
#[ignore = "needs the Postgres harness"]
async fn postgres_the_drop_removes_the_table_with_its_row_and_nothing_else() {
    let pg = seeded().await;
    migrate(&pg, &[DROP]).await.unwrap();
    let dump_before = dump(&pg).await;
    let columns_before = strings(&pg, COLUMNS_IN_ORDER).await;
    assert_eq!(
        strings(&pg, "SELECT count(*)::text AS v FROM bss.pricing_settings").await,
        ["1"],
        "the table holds the tenant's row"
    );

    let result = migrate(&pg, &[]).await.unwrap();
    assert_eq!(result.applied_names, [DROP], "only 000024 was pending");

    // The table's own lines are gone, its key's index with them; every other line is as it was.
    let dump_after = dump(&pg).await;
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
        !removed.is_empty() && removed.iter().all(|l| l.contains("pricing_settings")),
        "{removed:#?}"
    );
    assert!(
        !dump_after.iter().any(|l| l.contains("pricing_settings")),
        "nothing of the table is left"
    );
    assert_eq!(
        strings(
            &pg,
            "SELECT count(*)::text AS v FROM information_schema.tables \
             WHERE table_schema = 'bss' AND table_name = 'pricing_settings'"
        )
        .await,
        ["0"]
    );
    let fresh = Pg::applied().await;
    assert_eq!(dump_after, dump(&fresh).await, "upgraded and fresh agree");
    assert!(migrate(&pg, &[]).await.unwrap().applied_names.is_empty());

    // Its down creates the table again, empty, in the shape it had before the drop.
    let raw = pg.raw().await;
    let step = Migrator::migrations()
        .into_iter()
        .find(|m| m.name() == DROP)
        .unwrap();
    step.down(&SchemaManager::new(&raw)).await.unwrap();
    step.down(&SchemaManager::new(&raw)).await.unwrap();
    assert_eq!(
        dump(&pg).await,
        dump_before,
        "the same columns, CHECK and key"
    );
    assert_eq!(
        strings(&pg, COLUMNS_IN_ORDER).await,
        columns_before,
        "in the same order"
    );
    assert_eq!(
        strings(&pg, "SELECT count(*)::text AS v FROM bss.pricing_settings").await,
        ["0"],
        "the rows are not restored"
    );
}
