//! The tenant pricing settings read from the Settings Service, with a double of its reader in
//! the `ClientHub`.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use settings_service_sdk::SettingKey;
use toolkit::client_hub::ClientHub;
use uuid::Uuid;

use super::SettingsServiceSource;
use super::doubles::{Failure, FakeSettingsReader};
use crate::domain::tenant_settings::{SettingsUnavailable, TenantSettings, TenantSettingsSource};
use crate::infra::reference_ticker::system_actor;
use crate::infra::settings_declarations::{declarations, setting_keys};

const TEMPLATES: [(&str, &str); 4] = [
    ("invoice_line_template_recurring", "recurring"),
    ("invoice_line_template_usage", "usage"),
    ("invoice_line_template_one_time", "one_time"),
    ("invoice_line_template_bundle", "bundle"),
];

const FAILURES: [Failure; 4] = [
    Failure::Unavailable,
    Failure::Internal,
    Failure::Undeclared,
    Failure::Retired,
];

/// A hub holding the reader double, the double, and a source over the hub.
fn wired() -> (Arc<FakeSettingsReader>, SettingsServiceSource) {
    let hub = Arc::new(ClientHub::new());
    let reader = FakeSettingsReader::new().install(&hub);
    (reader, SettingsServiceSource::new(hub))
}

async fn read(source: &SettingsServiceSource, tenant: Uuid) -> TenantSettings {
    source
        .effective(tenant)
        .await
        .expect("the settings are read")
}

fn is_unavailable(got: &Result<TenantSettings, SettingsUnavailable>, case: &str) {
    assert!(
        got.is_err(),
        "{case}: the settings are unavailable, got {got:?}"
    );
}

#[tokio::test]
async fn an_own_value_an_inherited_value_and_a_declared_default_are_each_read() {
    let (reader, source) = wired();
    let tenant = Uuid::new_v4();
    reader.set(tenant, "default_timing", json!("arrears"));
    reader.inherit(tenant, "default_gl", json!("9000"));

    let got = read(&source, tenant).await;

    assert_eq!(got.defaults.default_timing, "arrears", "the tenant's own");
    assert_eq!(
        got.defaults.default_gl.as_deref(),
        Some("9000"),
        "inherited"
    );
    assert_eq!(
        got.defaults.default_tax_category, None,
        "the declared default"
    );
}

#[tokio::test]
async fn nothing_set_reads_the_declared_defaults_from_the_settings_service() {
    let (reader, source) = wired();
    let tenant = Uuid::new_v4();

    let got = read(&source, tenant).await;

    assert_eq!(got.defaults.default_timing, "advance");
    assert_eq!(got.defaults.default_gl, None);
    assert_eq!(got.defaults.default_tax_category, None);
    assert_eq!(got.defaults.invoice_line_templates, BTreeMap::new());
    assert_eq!(got.offered_currencies, Vec::<String>::new());
    assert_eq!(
        reader.bulk_reads().len(),
        1,
        "the defaults are the Settings Service's answer, not a guess"
    );
}

#[tokio::test]
async fn an_empty_code_or_template_is_none() {
    let (reader, source) = wired();
    let tenant = Uuid::new_v4();
    reader.set(tenant, "default_timing", json!("arrears"));
    reader.set(tenant, "default_gl", json!(""));
    reader.set(tenant, "default_tax_category", json!(""));
    reader.set(tenant, "invoice_line_template_usage", json!(""));
    reader.set(tenant, "invoice_line_template_recurring", json!("{sku}"));

    let got = read(&source, tenant).await;

    assert_eq!(got.defaults.default_timing, "arrears");
    assert_eq!(got.defaults.default_gl, None);
    assert_eq!(got.defaults.default_tax_category, None);
    assert_eq!(
        got.defaults.invoice_line_templates,
        BTreeMap::from([("recurring".to_owned(), "{sku}".to_owned())]),
        "an empty template is no template"
    );
}

#[tokio::test]
async fn the_codes_are_read_as_set() {
    let (reader, source) = wired();
    let tenant = Uuid::new_v4();
    reader.set(tenant, "default_gl", json!("4000"));
    reader.set(tenant, "default_tax_category", json!("std"));

    let got = read(&source, tenant).await;

    assert_eq!(got.defaults.default_gl.as_deref(), Some("4000"));
    assert_eq!(got.defaults.default_tax_category.as_deref(), Some("std"));
}

#[tokio::test]
async fn each_sku_kind_takes_its_own_template() {
    let (reader, source) = wired();
    let tenant = Uuid::new_v4();
    for (name, kind) in TEMPLATES {
        reader.set(tenant, name, json!(format!("{{sku}} as {kind}")));
    }

    let got = read(&source, tenant).await;

    let expected: BTreeMap<String, String> = TEMPLATES
        .iter()
        .map(|(_, kind)| ((*kind).to_owned(), format!("{{sku}} as {kind}")))
        .collect();
    assert_eq!(got.defaults.invoice_line_templates, expected);
}

#[tokio::test]
async fn the_offered_currencies_are_read_in_their_order() {
    let (reader, source) = wired();
    let tenant = Uuid::new_v4();
    reader.set(tenant, "offered_currencies", json!(["USD", "EUR"]));

    let got = read(&source, tenant).await;

    assert_eq!(got.offered_currencies, ["USD", "EUR"]);
}

#[tokio::test]
async fn a_tenant_reads_its_own_settings_only() {
    let (reader, source) = wired();
    let (tenant, other) = (Uuid::new_v4(), Uuid::new_v4());
    reader.set(other, "default_gl", json!("9000"));
    reader.set(tenant, "default_gl", json!("4000"));

    assert_eq!(
        read(&source, tenant).await.defaults.default_gl.as_deref(),
        Some("4000")
    );
    assert_eq!(
        read(&source, other).await.defaults.default_gl.as_deref(),
        Some("9000")
    );
}

#[tokio::test]
async fn every_declared_setting_is_asked_in_one_bulk_read_of_the_tenant_scope() {
    let (reader, source) = wired();
    let tenant = Uuid::new_v4();

    read(&source, tenant).await;

    let reads = reader.bulk_reads();
    assert_eq!(reads.len(), 1, "one bulk read: {reads:?}");
    assert_eq!(reader.single_reads(), 0, "no setting is read on its own");
    let asked: BTreeSet<String> = reads[0].keys.iter().cloned().collect();
    let declared: BTreeSet<String> = declarations()
        .unwrap()
        .into_iter()
        .map(|d| d.key.to_string())
        .collect();
    assert_eq!(asked, declared);
    assert_eq!(reads[0].keys.len(), declared.len(), "each key once");
    assert_eq!(reads[0].scope, format!("/tenants/{tenant}"));
}

#[tokio::test]
async fn the_settings_are_read_as_pricings_system_actor_of_the_tenant() {
    let (reader, source) = wired();
    let tenant = Uuid::new_v4();

    read(&source, tenant).await;

    let reads = reader.bulk_reads();
    assert_eq!(reads.len(), 1, "{reads:?}");
    let actor = system_actor(tenant).unwrap();
    assert_eq!(reads[0].subject_id, actor.subject_id());
    assert_eq!(reads[0].subject_tenant_id, tenant);
}

#[tokio::test]
async fn a_failed_read_is_unavailable() {
    for failure in FAILURES {
        let (reader, source) = wired();
        let tenant = Uuid::new_v4();
        reader.set(tenant, "default_gl", json!("9000"));
        reader.fail_reads(Some(failure));

        let got = source.effective(tenant).await;

        is_unavailable(&got, &format!("{failure:?}"));
    }
}

#[tokio::test]
async fn one_setting_that_fails_makes_all_of_them_unavailable() {
    let names: Vec<String> = declarations()
        .unwrap()
        .into_iter()
        .map(|d| d.key.leaf_slug().to_owned())
        .collect();
    for failure in FAILURES {
        for name in &names {
            let (reader, source) = wired();
            let tenant = Uuid::new_v4();
            reader.fail_key(name, Some(failure));

            let got = source.effective(tenant).await;

            is_unavailable(&got, &format!("{failure:?} on {name}"));
        }
    }
}

#[tokio::test]
async fn a_reader_that_never_answers_is_unavailable_once_the_read_timeout_passes() {
    let hub = Arc::new(ClientHub::new());
    let reader = FakeSettingsReader::new().install(&hub);
    reader.never_answer(true);
    let source = SettingsServiceSource::new(hub).with_read_timeout(Duration::from_millis(20));

    let got = tokio::time::timeout(Duration::from_secs(5), source.effective(Uuid::new_v4()))
        .await
        .expect("the read gives up on its own");

    is_unavailable(&got, "no answer");
    assert_eq!(reader.bulk_reads().len(), 1, "the read was sent");
}

#[tokio::test]
async fn without_a_settings_client_the_settings_are_unavailable() {
    let source = SettingsServiceSource::new(Arc::new(ClientHub::new()));

    let got = source.effective(Uuid::new_v4()).await;

    is_unavailable(&got, "no client");
}

#[tokio::test]
async fn the_settings_client_is_looked_up_at_each_read() {
    let hub = Arc::new(ClientHub::new());
    let source = SettingsServiceSource::new(hub.clone());
    let tenant = Uuid::new_v4();
    is_unavailable(
        &source.effective(tenant).await,
        "before the client is bound",
    );

    let reader = FakeSettingsReader::new().install(&hub);
    reader.set(tenant, "default_gl", json!("9000"));

    assert_eq!(
        read(&source, tenant).await.defaults.default_gl.as_deref(),
        Some("9000"),
        "a client bound after the source was built is found"
    );
}

#[tokio::test]
async fn a_value_of_another_shape_is_unavailable() {
    for (name, value) in [
        ("default_timing", json!("weekly")),
        ("default_timing", json!(null)),
        ("default_gl", json!(7)),
        ("default_tax_category", json!(["std"])),
        ("invoice_line_template_usage", json!({"text": "{sku}"})),
        ("offered_currencies", json!("EUR")),
        ("offered_currencies", json!(["EUR", 7])),
    ] {
        let (reader, source) = wired();
        let tenant = Uuid::new_v4();
        reader.set(tenant, name, value.clone());

        let got = source.effective(tenant).await;

        is_unavailable(&got, &format!("{name} = {value}"));
    }
}

#[tokio::test]
async fn a_setting_left_out_of_the_answer_makes_all_of_them_unavailable() {
    let names: Vec<String> = declarations()
        .unwrap()
        .into_iter()
        .map(|d| d.key.leaf_slug().to_owned())
        .collect();
    for name in &names {
        let (reader, source) = wired();
        reader.leave_out(name, true);

        let refused = source
            .effective(Uuid::new_v4())
            .await
            .expect_err("a setting nobody answered is not its default");

        assert_eq!(refused.detail, format!("{name} was not answered"));
    }
}

#[test]
fn a_declared_setting_the_read_does_not_know_is_unavailable() {
    let unknown =
        SettingKey::contributed("cf", "bss", "pricing", "default_dunning", NonZeroU32::MIN)
            .unwrap();
    let values = HashMap::from([(unknown.clone(), json!("weekly"))]);

    let refused = super::settings_from(&[unknown], values)
        .expect_err("a setting pricing declares and does not read is a defect, not a default");

    assert_eq!(refused.detail, "default_dunning is declared and not read");
}

#[tokio::test]
async fn an_internal_failure_keeps_its_status_and_diagnostic_for_the_log() {
    // The whole read, then one setting of an answered read.
    for whole in [true, false] {
        let (reader, source) = wired();
        if whole {
            reader.fail_reads(Some(Failure::Internal));
        } else {
            reader.fail_key("default_gl", Some(Failure::Internal));
        }

        let refused = source.effective(Uuid::new_v4()).await.unwrap_err();

        assert_eq!(refused.status, Some(500), "{refused:?}");
        assert_eq!(
            refused.diagnostic.as_deref(),
            Some("the settings store failed"),
            "the cause an internal error keeps out of its text: {refused:?}"
        );
    }
}

#[tokio::test]
async fn a_failure_that_is_not_internal_has_a_status_and_no_diagnostic() {
    let (reader, source) = wired();
    reader.fail_reads(Some(Failure::Unavailable));

    let refused = source.effective(Uuid::new_v4()).await.unwrap_err();

    assert_eq!(refused.status, Some(503), "{refused:?}");
    assert_eq!(refused.diagnostic, None);
}

#[test]
fn the_keys_asked_for_are_the_declared_ones() {
    let declared: Vec<SettingKey> = declarations().unwrap().into_iter().map(|d| d.key).collect();

    assert_eq!(setting_keys().unwrap(), declared.as_slice());
}
