//! The tenant settings every pricing decision reads come from the Settings Service: resolve, the
//! acceptance of a sale, the currency of a new book and the checks of a plan revision. A decision
//! that needs a setting it cannot read is refused as unavailable; the checks, which only show two
//! of them, stay readable. No decision asks the Settings Service from inside a database
//! transaction, where its reader cannot answer.
#![allow(clippy::expect_used, clippy::unwrap_used)]
mod acceptance_support;
mod plan_support;
mod seam_support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use acceptance_support::AcceptanceFixture;
use bss_pricing::api::pricing_read::PricingReadProvider;
use bss_pricing::infra::commercial_terms::errors::commercial_reason;
use bss_pricing::infra::tenant_settings::doubles::{Failure, FakeSettingsReader};
use bss_pricing_sdk::acceptance::{
    AcceptanceQuery, AcceptanceReceipt, NewSaleQuery, PricingAcceptanceV1, SellabilityV1,
};
use bss_pricing_sdk::digest::selected_bindings_digest;
use bss_pricing_sdk::meter_semantics::{MeterSemantics, UsageMeterSemanticsV1};
use bss_pricing_sdk::read::{CatalogRef, PricingReadV1, ResolveQuery};
use bss_pricing_sdk::terms::{BillingTiming, InputSource, MeterRef};
use bss_products_sdk::models::SkuType;
use plan_support::entry_support::policy_support::MeterProvider;
use plan_support::{
    Catalog, Fixture, book, id_of, item, plan, policy_entry, publish, scope, setup,
};
use seam_support::{Row, World, entry_of, put, resolve, world, written};
use serde_json::{Value, json};
use settings_service_sdk::{BulkSelector, SettingKey, SettingsReaderClient};
use toolkit_canonical_errors::CanonicalError;
use toolkit_security::SecurityContext;
use uuid::Uuid;

/// Every way the Settings Service fails to tell a tenant's settings: the whole read, or one
/// setting of an otherwise answered read.
#[derive(Debug, Clone, Copy)]
enum Outage {
    Read(Failure),
    Key(&'static str, Failure),
}

const OUTAGES: [Outage; 6] = [
    Outage::Read(Failure::Unavailable),
    Outage::Read(Failure::Internal),
    Outage::Key("default_gl", Failure::Undeclared),
    Outage::Key("offered_currencies", Failure::Undeclared),
    Outage::Key("default_timing", Failure::Retired),
    Outage::Key("offered_currencies", Failure::Retired),
];

/// What a caller reads when a decision needs settings that cannot be read: the code and nothing
/// about the cause.
const SETTINGS_UNAVAILABLE: &str =
    "SETTINGS_UNAVAILABLE: the tenant pricing settings are unavailable";

fn start(settings: &FakeSettingsReader, outage: Outage) {
    match outage {
        Outage::Read(failure) => settings.fail_reads(Some(failure)),
        Outage::Key(name, failure) => settings.fail_key(name, Some(failure)),
    }
}

fn end(settings: &FakeSettingsReader, outage: Outage) {
    match outage {
        Outage::Read(_) => settings.fail_reads(None),
        Outage::Key(name, _) => settings.fail_key(name, None),
    }
}

/// The tenant's values of the invoice inputs.
fn set_invoice_defaults(f: &Fixture, kind: &str, template: &str) {
    f.set_setting("default_timing", json!("arrears"));
    f.set_setting("default_gl", json!("9000"));
    f.set_setting("default_tax_category", json!("std"));
    f.set_setting(&format!("invoice_line_template_{kind}"), json!(template));
}

async fn resolved_item(w: &World) -> Value {
    let query = format!("plan_revision_id={}&date=2026-10-05", w.revision);
    let (status, body) = resolve(&w.f, &query).await;
    assert_eq!(status, 200, "{body}");
    body["items"][0].clone()
}

// ------------------------------------------------------------------ resolve

#[tokio::test]
async fn resolve_takes_what_the_entry_and_the_sku_leave_unset_from_the_tenant_settings() {
    let w = world().await;
    set_invoice_defaults(&w.f, "recurring", "{sku} per {period}");

    let item = resolved_item(&w).await;

    assert_eq!(
        item["invoice_line_template"],
        json!({"value":"{sku} per {period}","source":"tenant"})
    );
    assert_eq!(item["gl_code"], json!({"value":"9000","source":"tenant"}));
    assert_eq!(
        item["tax_category"],
        json!({"value":"std","source":"tenant"})
    );
    assert_eq!(
        item["billing_timing"],
        json!({"value":"arrears","source":"tenant"})
    );
}

#[tokio::test]
async fn an_entry_template_and_a_sku_value_win_over_the_tenant_settings() {
    let (f, catalog) = setup().await;
    let eur = book(&f, "eur").await;
    let sku = catalog.sku(SkuType::Recurring);
    let mut content = catalog.content(sku);
    content.gl_code = Some("4000".into());
    catalog.version(sku, 1, "2026-09-01", content);
    let entry = entry_of(
        &f,
        eur,
        sku,
        "recurring",
        (Some("month"), None, Some("{plan} seat")),
    )
    .await;
    put(&f, entry, Row::default()).await;
    let (created, revision) = plan(&f, "pro", eur).await;
    item(&f, revision, sku, Some(entry), "paid").await;
    publish(&f, id_of(&created["id"]), revision).await;
    set_invoice_defaults(&f, "recurring", "{sku} per {period}");

    let (status, body) = resolve(&f, &format!("plan_revision_id={revision}&date=2026-10-05")).await;

    assert_eq!(status, 200, "{body}");
    let item = &body["items"][0];
    assert_eq!(
        item["invoice_line_template"],
        json!({"value":"{plan} seat","source":"entry"})
    );
    assert_eq!(item["gl_code"], json!({"value":"4000","source":"sku"}));
    assert_eq!(
        item["tax_category"],
        json!({"value":"std","source":"tenant"}),
        "what the SKU leaves unset is still the tenant's"
    );
}

#[tokio::test]
async fn resolve_with_nothing_set_reads_the_declared_defaults_from_the_settings_service() {
    let w = world().await;
    let tenant = w.f.ctx.subject_tenant_id();
    let settings = w.f.settings();
    let before = settings.bulk_reads().len();

    let item = resolved_item(&w).await;

    let absent = json!({"value": null, "source": null});
    assert_eq!(item["invoice_line_template"], absent);
    assert_eq!(item["gl_code"], absent);
    assert_eq!(item["tax_category"], absent);
    assert_eq!(
        item["billing_timing"],
        json!({"value":"advance","source":"tenant"})
    );
    let reads = settings.bulk_reads();
    assert_eq!(
        reads.len() - before,
        1,
        "one resolve is one read of the settings: {reads:?}"
    );
    assert_eq!(reads[before].scope, format!("/tenants/{tenant}"));
    assert_eq!(settings.single_reads(), 0);
}

#[tokio::test]
async fn the_settings_door_no_longer_feeds_resolve() {
    let w = world().await;
    let (_, _, tag) = w.f.call("GET", "/settings", json!({}), None, None).await;
    let stored = json!({"default_timing":"arrears","default_gl":"1111",
        "default_tax_category":"door","invoice_line_templates":{"recurring":"from the door"},
        "currencies":[]});
    let (status, body, _) = w.f.call("PUT", "/settings", stored, Some(&tag), None).await;
    assert_eq!(status, 200, "{body}");
    w.f.set_setting("default_gl", json!("9000"));

    let item = resolved_item(&w).await;

    assert_eq!(item["gl_code"], json!({"value":"9000","source":"tenant"}));
    assert_eq!(
        item["tax_category"],
        json!({"value": null, "source": null}),
        "a value only the door stores is not read"
    );
    assert_eq!(
        item["billing_timing"],
        json!({"value":"advance","source":"tenant"})
    );
}

#[tokio::test]
async fn resolve_is_unavailable_while_the_settings_are() {
    let w = world().await;
    let settings = w.f.settings();
    set_invoice_defaults(&w.f, "recurring", "{sku} per {period}");
    let query = format!("plan_revision_id={}&date=2026-10-05", w.revision);
    for outage in OUTAGES {
        start(&settings, outage);
        let (status, body) = resolve(&w.f, &query).await;
        assert_eq!(status, 503, "{outage:?}: {body}");
        assert_eq!(body["detail"], SETTINGS_UNAVAILABLE, "{outage:?}: {body}");
        end(&settings, outage);
    }
    let (status, body) = resolve(&w.f, &query).await;
    assert_eq!(status, 200, "the settings answer again: {body}");
}

/// The in-process resolve of a published usage item whose SKU version names no invoice input.
struct SdkWorld {
    f: Fixture,
    provider: PricingReadProvider,
    query: ResolveQuery,
}

async fn sdk_world() -> SdkWorld {
    let (f, catalog) = setup().await;
    let eur = book(&f, "eur").await;
    let sku = catalog.sku(SkuType::Usage);
    catalog.version(sku, 1, "2026-09-01", catalog.content(sku));
    let entry = entry_of(&f, eur, sku, "usage", (None, None, None)).await;
    put(
        &f,
        entry,
        Row {
            price: json!({"rate":"0.047"}),
            ..Row::default()
        },
    )
    .await;
    let (created, revision_id) = plan(&f, "seam", eur).await;
    item(&f, revision_id, sku, Some(entry), "paid").await;
    publish(&f, id_of(&created["id"]), revision_id).await;
    let tenant = f.ctx.subject_tenant_id();
    let provider = PricingReadProvider::new(
        f.state.clone(),
        Arc::new(plan_support::entry_support::enforcer_for(tenant)),
    );
    let query = ResolveQuery {
        catalog: CatalogRef { tenant_id: tenant },
        revision_id,
        date: seam_support::date("2026-09-15"),
        item_id: None,
        pins: vec![],
    };
    SdkWorld { f, provider, query }
}

#[tokio::test]
async fn the_in_process_resolve_binds_the_tenant_settings_as_seller_settings() {
    let w = sdk_world().await;
    set_invoice_defaults(&w.f, "usage", "{sku} usage");

    let resolved = w.provider.resolve(&w.f.ctx, w.query.clone()).await.unwrap();

    let invoice = &resolved.cells[0].binding.as_ref().unwrap().invoice;
    assert_eq!(invoice.template, "{sku} usage");
    assert_eq!(invoice.template_source, InputSource::SellerSettings);
    assert_eq!(invoice.gl_code, "9000");
    assert_eq!(invoice.tax_category, "std");
    assert_eq!(invoice.timing, BillingTiming::Arrears);
}

#[tokio::test]
async fn the_in_process_resolve_is_unavailable_while_the_settings_are() {
    let w = sdk_world().await;
    let settings = w.f.settings();
    set_invoice_defaults(&w.f, "usage", "{sku} usage");
    for outage in OUTAGES {
        start(&settings, outage);
        let refused = w
            .provider
            .resolve(&w.f.ctx, w.query.clone())
            .await
            .unwrap_err();
        assert_eq!(refused.status_code(), 503, "{outage:?}: {refused:?}");
        assert_eq!(refused.detail(), SETTINGS_UNAVAILABLE, "{outage:?}");
        end(&settings, outage);
    }
    w.provider.resolve(&w.f.ctx, w.query.clone()).await.unwrap();
}

// ------------------------------------------------------------------ acceptance

/// A sale whose invoice inputs all come from the tenant settings: the sold SKU's version names
/// none, and the tenant sets each. The query carries the digest of what resolve answers now.
async fn sale_on_tenant_settings() -> (AcceptanceFixture, NewSaleQuery) {
    let f = AcceptanceFixture::new().await;
    let sku = f.resolved().await.cells[0].binding.as_ref().unwrap().sku_id;
    f.catalog
        .version(sku, 4, "2026-09-15", f.catalog.content(sku));
    set_invoice_defaults(&f.fixture, "usage", "{sku} usage");
    let query = offered(&f).await;
    (f, query)
}

/// The fixture's sale, offered as resolve answers it now.
async fn offered(f: &AcceptanceFixture) -> NewSaleQuery {
    let resolved = f.resolved().await;
    let mut query = f.query.clone();
    query.resolved_bindings_digest =
        selected_bindings_digest(&resolved, &query.selections).unwrap();
    query
}

async fn check(
    f: &AcceptanceFixture,
    query: &NewSaleQuery,
) -> Result<AcceptanceReceipt, CanonicalError> {
    f.sellability
        .check(&f.ctx, query.clone(), f.meta.clone())
        .await
}

fn frozen_gl(receipt: &AcceptanceReceipt) -> &str {
    &receipt.bindings[0].invoice.gl_code
}

#[tokio::test]
async fn the_receipt_freezes_the_settings_read_once_at_capture() {
    let (f, query) = sale_on_tenant_settings().await;
    let settings = f.fixture.settings();
    let before = settings.bulk_reads().len();

    let receipt = check(&f, &query).await.unwrap();

    let invoice = &receipt.bindings[0].invoice;
    assert_eq!(invoice.template, "{sku} usage");
    assert_eq!(invoice.template_source, InputSource::SellerSettings);
    assert_eq!(invoice.gl_code, "9000");
    assert_eq!(invoice.tax_category, "std");
    assert_eq!(invoice.timing, BillingTiming::Arrears);
    assert_eq!(
        settings.bulk_reads().len() - before,
        1,
        "the settings are read once for a sale, and not again to commit it"
    );

    // What the tenant sets afterwards reaches neither a replay nor the stored receipt.
    f.fixture.set_setting("default_gl", json!("9999"));
    f.fixture.set_setting("default_timing", json!("advance"));
    assert_eq!(check(&f, &query).await.unwrap(), receipt);
    let stored = f
        .acceptance
        .acceptance(
            &f.ctx,
            AcceptanceQuery {
                catalog: CatalogRef {
                    tenant_id: f.ctx.subject_tenant_id(),
                },
                acceptance_id: receipt.acceptance_id,
            },
        )
        .await
        .unwrap();
    assert_eq!(stored, receipt);
    assert_eq!(f.counts().await, (1, 1, 1));
}

/// A meter that lets the tenant change a setting while it answers: after the sale's settings
/// were captured and before the sale is committed.
struct SettingChangedMidSale {
    settings: Arc<FakeSettingsReader>,
    tenant: Uuid,
    done: AtomicBool,
    meter: MeterProvider,
}

#[async_trait::async_trait]
impl UsageMeterSemanticsV1 for SettingChangedMidSale {
    async fn resolve(
        &self,
        ctx: &SecurityContext,
        meter: MeterRef,
    ) -> Result<MeterSemantics, CanonicalError> {
        if !self.done.swap(true, Ordering::SeqCst) {
            self.settings.set(self.tenant, "default_gl", json!("9999"));
        }
        self.meter.resolve(ctx, meter).await
    }
}

#[tokio::test]
async fn a_setting_changed_between_capture_and_commit_does_not_move_the_selection() {
    let (f, query) = sale_on_tenant_settings().await;
    let settings = f.fixture.settings();
    let hook = Arc::new(SettingChangedMidSale {
        settings: settings.clone(),
        tenant: f.ctx.subject_tenant_id(),
        done: AtomicBool::new(false),
        meter: MeterProvider::default(),
    });
    f.fixture
        .state
        .hub
        .register::<dyn UsageMeterSemanticsV1>(hook.clone());
    let before = settings.bulk_reads().len();

    let receipt = check(&f, &query)
        .await
        .expect("the sale is committed on the settings it captured");

    assert_eq!(frozen_gl(&receipt), "9000", "as captured, not as changed");
    assert_eq!(
        hook.meter.calls.load(Ordering::SeqCst),
        1,
        "the sale was captured once: nothing made it start over"
    );
    assert_eq!(settings.bulk_reads().len() - before, 1);
    assert_eq!(f.counts().await, (1, 1, 1));
}

#[tokio::test]
async fn a_setting_changed_since_the_offer_is_a_changed_resolution_at_once() {
    let (f, query) = sale_on_tenant_settings().await;
    let settings = f.fixture.settings();
    f.fixture.set_setting("default_gl", json!("9999"));
    let before = settings.bulk_reads().len();

    let refused = check(&f, &query).await.unwrap_err();

    assert_eq!(
        commercial_reason(&refused).as_deref(),
        Some("ResolutionChanged"),
        "{refused:?}"
    );
    assert_eq!(
        settings.bulk_reads().len() - before,
        1,
        "refused on the first read: the settings are not read again"
    );
    assert_eq!(f.counts().await, (0, 0, 0));

    // Offered again on the settings as they are, the sale is accepted with them.
    let again = offered(&f).await;
    assert_eq!(frozen_gl(&check(&f, &again).await.unwrap()), "9999");
}

#[tokio::test]
async fn acceptance_is_unavailable_while_the_settings_are_and_records_nothing() {
    let (f, query) = sale_on_tenant_settings().await;
    let settings = f.fixture.settings();
    for outage in OUTAGES {
        start(&settings, outage);
        let refused = check(&f, &query).await.unwrap_err();
        assert_eq!(refused.status_code(), 503, "{outage:?}: {refused:?}");
        assert_eq!(f.counts().await, (0, 0, 0), "{outage:?}");
        end(&settings, outage);
    }
    let receipt = check(&f, &query)
        .await
        .expect("the refused command was not kept: the same key is accepted");
    assert_eq!(f.counts().await, (1, 1, 1));

    // An accepted command replays its receipt without the settings.
    settings.fail_reads(Some(Failure::Unavailable));
    assert_eq!(check(&f, &query).await.unwrap(), receipt);
}

// ------------------------------------------------------------------ a new book's currency

async fn create_book(f: &Fixture, code: &str, currency: &str) -> (u16, Value) {
    let (status, body, _) = f
        .call(
            "POST",
            "/price-books",
            json!({"code":code,"name":code,"currency":currency}),
            None,
            Some(&format!("book-{code}")),
        )
        .await;
    (status, body)
}

async fn book_count(f: &Fixture) -> usize {
    let (status, books, _) = f.call("GET", "/price-books", json!({}), None, None).await;
    assert_eq!(status, 200, "{books}");
    books["items"].as_array().unwrap().len()
}

#[tokio::test]
async fn a_new_book_takes_a_currency_the_settings_service_says_is_offered() {
    let (f, _) = setup().await;
    let settings = f.settings();
    let before = settings.bulk_reads().len();
    // Nothing set: the declared default, an empty list, offers every currency.
    assert_eq!(create_book(&f, "jpy", "JPY").await.0, 201);
    assert_eq!(
        settings.bulk_reads().len() - before,
        1,
        "the list is the Settings Service's answer, not a guess"
    );

    f.set_setting("offered_currencies", json!(["EUR", "USD"]));
    assert_eq!(create_book(&f, "eur", "EUR").await.0, 201);
    assert_eq!(create_book(&f, "usd", "USD").await.0, 201);
    let (status, body) = create_book(&f, "gbp", "GBP").await;
    assert_eq!(status, 409, "{body}");
    assert!(body.to_string().contains("CURRENCY_NOT_OFFERED"), "{body}");
    assert_eq!(book_count(&f).await, 3, "the refused book is not there");

    f.set_setting("offered_currencies", json!([]));
    assert_eq!(create_book(&f, "gbp", "GBP").await.0, 201);
}

#[tokio::test]
async fn the_settings_door_no_longer_narrows_a_new_books_currency() {
    let (f, _) = setup().await;
    let (_, _, tag) = f.call("GET", "/settings", json!({}), None, None).await;
    let stored = json!({"default_timing":"advance","default_gl":null,
        "default_tax_category":null,"invoice_line_templates":{},"currencies":["EUR"]});
    let (status, body, _) = f.call("PUT", "/settings", stored, Some(&tag), None).await;
    assert_eq!(status, 200, "{body}");

    let (status, body) = create_book(&f, "gbp", "GBP").await;

    assert_eq!(status, 201, "only the Settings Service narrows it: {body}");
}

#[tokio::test]
async fn a_book_is_not_created_while_the_settings_are_unavailable_and_nothing_is_claimed() {
    let (f, _) = setup().await;
    let settings = f.settings();
    f.set_setting("offered_currencies", json!(["EUR"]));
    let untouched = written(&f).await;
    for outage in OUTAGES {
        start(&settings, outage);
        let (status, body) = create_book(&f, "eur", "EUR").await;
        assert_eq!(status, 503, "{outage:?}: {body}");
        assert_eq!(body["detail"], SETTINGS_UNAVAILABLE, "{outage:?}: {body}");
        assert_eq!(
            written(&f).await,
            untouched,
            "{outage:?}: no audit, claim, event or reference"
        );
        // A body the door refuses by itself is refused as it always is: the list is not needed
        // to say so.
        let (status, body) = create_book(&f, "bad", "euro").await;
        assert_eq!(status, 400, "{outage:?}: {body}");
        assert!(
            body.to_string().contains("BOOK_CURRENCY_INVALID"),
            "{outage:?}: {body}"
        );
        end(&settings, outage);
    }
    assert_eq!(book_count(&f).await, 0);

    // The refused key was not claimed: the same request creates the book once the settings
    // answer.
    let (status, created) = create_book(&f, "eur", "EUR").await;
    assert_eq!(status, 201, "{created}");

    // A created book replays its answer without the settings, and a key reused for another body
    // is still told so.
    for outage in OUTAGES {
        start(&settings, outage);
        let (status, replayed) = create_book(&f, "eur", "EUR").await;
        assert_eq!(status, 201, "{outage:?}: {replayed}");
        assert_eq!(replayed, created, "{outage:?}");
        let (status, body) = create_book(&f, "eur", "USD").await;
        assert_eq!(status, 409, "{outage:?}: {body}");
        assert!(
            body.to_string().contains("IDEMPOTENCY_CONFLICT"),
            "{outage:?}: {body}"
        );
        end(&settings, outage);
    }
    assert_eq!(book_count(&f).await, 1);
}

// ------------------------------------------------------------------ the checks of a revision

/// A draft revision that is ready to submit: one paid usage item on a priced entry of its book.
async fn green_draft(f: &Fixture, catalog: &Catalog, code: &str) -> Uuid {
    let eur = book(f, code).await;
    let (_, revision) = plan(f, code, eur).await;
    let sku = catalog.sku(SkuType::Usage);
    let entry = policy_entry(f, eur, sku, "usage", None).await;
    let tenant = f.ctx.subject_tenant_id();
    let conn = f.db.conn().unwrap();
    let stored = bss_pricing::infra::storage::repo::price_book_entry_repo::find(
        &conn,
        &scope(f),
        tenant,
        entry,
    )
    .await
    .unwrap()
    .unwrap();
    let mut price = plan_support::entry_support::price(&stored);
    price.state = "approved".into();
    price.effective_from = seam_support::date("2020-01-01");
    bss_pricing::infra::storage::repo::price_repo::insert(&conn, &scope(f), price)
        .await
        .unwrap();
    item(f, revision, sku, Some(entry), "paid").await;
    revision
}

async fn checks(f: &Fixture, revision: Uuid) -> Value {
    let path = format!("/plan-revisions/{revision}/checks");
    let (status, body, _) = f.call("GET", &path, json!({}), None, None).await;
    assert_eq!(status, 200, "{body}");
    body
}

fn descriptors(checks: &Value) -> &Value {
    checks["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["code"] == "DESCRIPTORS")
        .unwrap_or_else(|| panic!("no DESCRIPTORS row in {checks}"))
}

async fn submit(f: &Fixture, revision: Uuid) -> (u16, Value) {
    let path = format!("/plan-revisions/{revision}/submit");
    let (status, body, _) = f.call("POST", &path, json!({}), None, Some("submit")).await;
    (status, body)
}

#[tokio::test]
async fn the_checks_show_the_tenant_gl_and_tax_and_stay_readable_without_them() {
    let (f, catalog) = setup().await;
    let settings = f.settings();
    let revision = green_draft(&f, &catalog, "pro").await;
    f.set_setting("default_gl", json!("9000"));
    f.set_setting("default_tax_category", json!("std"));

    let told = checks(&f, revision).await;

    let row = descriptors(&told);
    let detail = row["detail"].as_str().unwrap();
    assert!(detail.contains("9000"), "the tenant's GL code: {detail}");
    assert!(
        detail.contains("std"),
        "the tenant's tax category: {detail}"
    );
    assert_eq!((&row["info"], &row["ok"]), (&json!(true), &json!(true)));
    assert_eq!(told["ready"], true, "{told}");

    for outage in [
        Outage::Read(Failure::Unavailable),
        Outage::Read(Failure::Internal),
        Outage::Key("default_gl", Failure::Undeclared),
        Outage::Key("default_tax_category", Failure::Retired),
    ] {
        start(&settings, outage);
        let untold = checks(&f, revision).await;
        let row = descriptors(&untold);
        let detail = row["detail"].as_str().unwrap();
        assert!(
            !detail.contains("9000") && !detail.contains("std"),
            "{outage:?}: the row names no value it could not read: {detail}"
        );
        assert_eq!((&row["info"], &row["ok"]), (&json!(true), &json!(true)));
        assert_eq!(
            untold["ready"], true,
            "{outage:?}: the row is for information: {untold}"
        );
        let batch = format!("/plan-revisions/checks?revision_ids={revision}");
        let (status, body, _) = f.call("GET", &batch, json!({}), None, None).await;
        assert_eq!(status, 200, "{outage:?}: {body}");
        assert_eq!(body["items"][0]["checks"], untold, "{outage:?}");
        end(&settings, outage);
    }

    // The submit door judges the same checks: it does not need the settings either.
    settings.fail_reads(Some(Failure::Unavailable));
    let (status, body) = submit(&f, revision).await;
    assert_eq!(status, 201, "{body}");
}

#[tokio::test]
async fn the_batch_of_checks_reads_the_settings_once_for_the_whole_set() {
    let (f, catalog) = setup().await;
    let settings = f.settings();
    let one = green_draft(&f, &catalog, "one").await;
    let two = green_draft(&f, &catalog, "two").await;
    f.set_setting("default_gl", json!("9000"));
    let before = settings.bulk_reads().len();

    let batch = format!("/plan-revisions/checks?revision_ids={one},{two}");
    let (status, body, _) = f.call("GET", &batch, json!({}), None, None).await;

    assert_eq!(status, 200, "{body}");
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "{body}");
    for item in items {
        let detail = descriptors(&item["checks"])["detail"].as_str().unwrap();
        assert!(detail.contains("9000"), "{detail}");
    }
    assert_eq!(
        settings.bulk_reads().len() - before,
        1,
        "one read of the settings, whatever the number of revisions"
    );
}

// ------------------------------------------------------------------ never inside a transaction

/// The reader double refuses, and counts, a read made inside a transaction, as the Settings
/// Service's reader does: the guard the next test stands on.
#[tokio::test]
async fn the_reader_double_refuses_a_read_inside_a_transaction() {
    let (f, _) = setup().await;
    let settings = f.settings();
    let tenant = f.ctx.subject_tenant_id();
    let key: SettingKey = settings.key_of("default_gl").parse().unwrap();
    let ask = move |reader: Arc<FakeSettingsReader>, key: SettingKey| async move {
        reader
            .get_effective_bulk(
                &plan_support::entry_support::user_of(tenant),
                BulkSelector::Keys(vec![key]),
                format!("/tenants/{tenant}"),
            )
            .await
    };

    ask(settings.clone(), key.clone())
        .await
        .expect("outside a transaction the read is answered");
    assert_eq!(settings.reads_in_transaction(), 0);

    let inside = settings.clone();
    let refused =
        f.db.db()
            .transaction_ref(move |_tx| Box::pin(async move { Ok(ask(inside, key).await) }))
            .await
            .unwrap();
    assert!(refused.is_err(), "inside a transaction the read is refused");
    assert_eq!(settings.reads_in_transaction(), 1);
}

#[tokio::test]
async fn no_decision_asks_the_settings_service_inside_a_transaction() {
    // Resolve, through the door and in process, and the acceptance of a sale.
    let (f, query) = sale_on_tenant_settings().await;
    let settings = f.fixture.settings();
    let tenant = f.ctx.subject_tenant_id();
    let mut asked = settings.bulk_reads().len();
    let mut asked_since = |site: &str| {
        let now = settings.bulk_reads().len();
        assert!(now > asked, "{site} reads the tenant settings");
        asked = now;
    };

    let path = format!(
        "plan_revision_id={}&date={}",
        query.plan_revision_id,
        query.start_at.date()
    );
    let (status, body) = resolve(&f.fixture, &path).await;
    assert_eq!(status, 200, "{body}");
    asked_since("the resolve door");

    f.resolved().await;
    asked_since("the in-process resolve");

    check(&f, &query).await.unwrap();
    asked_since("the acceptance of a sale");

    // A new book, and the checks of a revision with the submit that judges them.
    f.fixture
        .set_setting("offered_currencies", json!(["EUR", "USD"]));
    assert_eq!(create_book(&f.fixture, "usd", "USD").await.0, 201);
    asked_since("the creation of a book");
    let (status, body) = create_book(&f.fixture, "gbp", "GBP").await;
    assert_eq!(status, 409, "{body}");
    asked_since("the refusal of a book");

    let revision = green_draft(&f.fixture, &f.catalog, "guard").await;
    checks(&f.fixture, revision).await;
    asked_since("the checks of a revision");
    let (status, body) = submit(&f.fixture, revision).await;
    assert_eq!(status, 201, "{body}");

    assert_eq!(
        settings.reads_in_transaction(),
        0,
        "the Settings Service's reader cannot answer inside a transaction"
    );
    assert_eq!(settings.single_reads(), 0);
    assert!(
        settings
            .bulk_reads()
            .iter()
            .all(|read| read.scope == format!("/tenants/{tenant}")),
        "every read is of the tenant's own scope"
    );
}
