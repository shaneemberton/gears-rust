//! The declared tenant pricing settings: their keys, value types and defaults, and the two
//! startup registrations, with doubles of the types registry and of the Settings Service.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{Value, json};
use settings_service_sdk::SettingsContributionClient;
use settings_service_sdk::models::{ContributedClassification, ContributedDeclaration, ScopeClass};
use toolkit::client_hub::ClientHub;

use super::doubles::{Answer, REFUSAL_CODE, RecordingRegistry, RecordingSettings};
use super::{declarations, register_declarations, register_value_types, value_type_schemas};
use crate::module::BssPricingGear;

const DEFAULT_TIMING: &str = "default_timing";
const DEFAULT_GL: &str = "default_gl";
const DEFAULT_TAX_CATEGORY: &str = "default_tax_category";
const OFFERED_CURRENCIES: &str = "offered_currencies";

/// The invoice line template settings, one per SKU kind.
const TEMPLATES: [&str; 4] = [
    "invoice_line_template_recurring",
    "invoice_line_template_usage",
    "invoice_line_template_one_time",
    "invoice_line_template_bundle",
];

/// The settings whose value is a short code.
const SHORT_CODES: [&str; 2] = [DEFAULT_GL, DEFAULT_TAX_CATEGORY];

/// Every declared setting as a stored value names it: its leaf name, its full key and the type
/// its values are of. Written out, never composed: a tenant's stored values are filed under
/// these strings, so a rename here is a new setting that reads its default.
const FROZEN: &[(&str, &str, &str)] = &[
    (
        DEFAULT_TIMING,
        "gts.cf.core.settings.setting_type.v1~cf.bss.pricing.default_timing.v1~",
        "gts.cf.bss.pricing.setting_billing_timing.v1~",
    ),
    (
        DEFAULT_GL,
        "gts.cf.core.settings.setting_type.v1~cf.bss.pricing.default_gl.v1~",
        "gts.cf.bss.pricing.setting_short_code.v1~",
    ),
    (
        DEFAULT_TAX_CATEGORY,
        "gts.cf.core.settings.setting_type.v1~cf.bss.pricing.default_tax_category.v1~",
        "gts.cf.bss.pricing.setting_short_code.v1~",
    ),
    (
        "invoice_line_template_recurring",
        "gts.cf.core.settings.setting_type.v1~cf.bss.pricing.invoice_line_template_recurring.v1~",
        "gts.cf.bss.pricing.setting_line_template.v1~",
    ),
    (
        "invoice_line_template_usage",
        "gts.cf.core.settings.setting_type.v1~cf.bss.pricing.invoice_line_template_usage.v1~",
        "gts.cf.bss.pricing.setting_line_template.v1~",
    ),
    (
        "invoice_line_template_one_time",
        "gts.cf.core.settings.setting_type.v1~cf.bss.pricing.invoice_line_template_one_time.v1~",
        "gts.cf.bss.pricing.setting_line_template.v1~",
    ),
    (
        "invoice_line_template_bundle",
        "gts.cf.core.settings.setting_type.v1~cf.bss.pricing.invoice_line_template_bundle.v1~",
        "gts.cf.bss.pricing.setting_line_template.v1~",
    ),
    (
        OFFERED_CURRENCIES,
        "gts.cf.core.settings.setting_type.v1~cf.bss.pricing.offered_currencies.v1~",
        "gts.cf.bss.pricing.setting_currency_codes.v1~",
    ),
];

/// Every setting the gear declares, by leaf name.
fn setting_names() -> Vec<&'static str> {
    FROZEN.iter().map(|(name, _, _)| *name).collect()
}

/// The full key of the setting named `name`.
fn key_of(name: &str) -> &'static str {
    FROZEN
        .iter()
        .find(|(frozen, _, _)| *frozen == name)
        .map_or_else(|| panic!("`{name}` is not a setting"), |(_, key, _)| *key)
}

const OWN_NAMESPACE: &str = "gts.cf.bss.pricing.";
const DRAFT_07: &str = "http://json-schema.org/draft-07/schema#";

fn declared() -> Vec<ContributedDeclaration> {
    declarations().expect("every key parses")
}

/// The declaration of the setting named `name`.
fn declaration(name: &str) -> ContributedDeclaration {
    let key = key_of(name);
    declared()
        .into_iter()
        .find(|declaration| declaration.key.as_str() == key)
        .unwrap_or_else(|| panic!("`{name}` is not declared"))
}

/// The registered schema of the value type `type_id`.
fn schema_of(type_id: &str) -> Value {
    let id = format!("gts://{type_id}");
    value_type_schemas()
        .into_iter()
        .find(|schema| schema["$id"] == id.as_str())
        .unwrap_or_else(|| panic!("no schema is registered for `{type_id}`"))
}

/// A validator over `schema`, built as the Settings Service builds one: formats asserted, the
/// draft taken from the schema's own `$schema`.
fn validator(schema: &Value) -> jsonschema::Validator {
    jsonschema::options()
        .should_validate_formats(true)
        .build(schema)
        .unwrap_or_else(|error| panic!("the schema {schema} does not build: {error}"))
}

/// The validator of the value type the setting named `name` declares.
fn validator_of(name: &str) -> jsonschema::Validator {
    validator(&schema_of(&declaration(name).value_type_id))
}

/// Every `$ref` target anywhere in `schema`.
fn references(schema: &Value, found: &mut Vec<String>) {
    match schema {
        Value::Object(members) => {
            for (keyword, member) in members {
                if keyword == "$ref"
                    && let Some(target) = member.as_str()
                {
                    found.push(target.to_owned());
                }
                references(member, found);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| references(item, found)),
        _ => {}
    }
}

// ── The declarations ────────────────────────────────────────────────────────

#[test]
fn the_keys_and_value_types_are_the_frozen_ones() {
    let declared: BTreeSet<(String, String)> = declared()
        .into_iter()
        .map(|declaration| (declaration.key.to_string(), declaration.value_type_id))
        .collect();
    let frozen: BTreeSet<(String, String)> = FROZEN
        .iter()
        .map(|(_, key, value_type_id)| ((*key).to_owned(), (*value_type_id).to_owned()))
        .collect();
    assert_eq!(declared, frozen);
    assert_eq!(
        declarations().unwrap().len(),
        FROZEN.len(),
        "no key is declared twice"
    );
}

#[test]
fn every_setting_belongs_to_one_tenant_and_is_never_inherited() {
    for name in setting_names() {
        assert_eq!(declaration(name).scope_class, ScopeClass::Local, "{name}");
    }
}

#[test]
fn every_setting_is_public_and_changes_only_after_a_step_up() {
    for name in setting_names() {
        let declaration = declaration(name);
        assert_eq!(
            declaration.data_classification,
            Some(ContributedClassification::Public),
            "{name}"
        );
        assert_eq!(declaration.requires_step_up, Some(true), "{name}");
        assert_ne!(
            declaration.anonymous_exposable,
            Some(true),
            "{name}: a tenant's pricing settings are not served without authentication"
        );
    }
}

#[test]
fn the_defaults_are_what_a_tenant_without_stored_settings_had() {
    assert_eq!(declaration(DEFAULT_TIMING).default_value, json!("advance"));
    for name in SHORT_CODES.into_iter().chain(TEMPLATES) {
        assert_eq!(declaration(name).default_value, json!(""), "{name}");
    }
    assert_eq!(declaration(OFFERED_CURRENCIES).default_value, json!([]));
}

#[test]
fn every_default_is_a_value_of_its_own_type() {
    for name in setting_names() {
        let default = declaration(name).default_value;
        assert!(
            validator_of(name).is_valid(&default),
            "{name}: the default {default} is refused by the setting's value type"
        );
    }
}

#[test]
fn every_setting_carries_a_label_and_a_description() {
    for name in setting_names() {
        let declaration = declaration(name);
        let label = declaration
            .display_name
            .unwrap_or_else(|| panic!("{name} has no display name"));
        assert_eq!(label, label.trim(), "{name}");
        assert!(
            (1..=256).contains(&label.chars().count()),
            "{name}: the display name is {} characters",
            label.chars().count()
        );
        let description = declaration
            .description
            .unwrap_or_else(|| panic!("{name} has no description"));
        assert!(!description.trim().is_empty(), "{name}");
    }
}

#[test]
fn the_labels_tell_the_settings_apart() {
    let labels: BTreeSet<String> = declared()
        .into_iter()
        .filter_map(|declaration| declaration.display_name)
        .collect();
    assert_eq!(labels.len(), setting_names().len());
}

// ── The value types ─────────────────────────────────────────────────────────

#[test]
fn every_value_type_is_the_gears_own_and_has_a_schema() {
    for name in setting_names() {
        let type_id = declaration(name).value_type_id;
        assert!(
            type_id.starts_with(OWN_NAMESPACE) && type_id.ends_with('~'),
            "{name}: `{type_id}` is not a type in the gear's namespace"
        );
        assert!(
            toolkit_gts::GtsId::try_new(&type_id).is_ok(),
            "{name}: `{type_id}` is not a GTS identifier"
        );
        schema_of(&type_id);
    }
}

#[test]
fn no_schema_is_registered_that_no_setting_uses() {
    let used: BTreeSet<String> = declared()
        .into_iter()
        .map(|declaration| format!("gts://{}", declaration.value_type_id))
        .collect();
    let registered: Vec<String> = value_type_schemas()
        .iter()
        .map(|schema| {
            schema["$id"]
                .as_str()
                .expect("a schema names itself")
                .to_owned()
        })
        .collect();
    assert!(!registered.is_empty(), "the gear registers its value types");
    assert_eq!(
        registered.iter().cloned().collect::<BTreeSet<_>>(),
        used,
        "the registered types are the declared ones"
    );
    assert_eq!(registered.len(), used.len(), "no type is registered twice");
}

#[test]
fn every_schema_is_a_concrete_draft_07_schema_that_refers_to_no_other_type() {
    let schemas = value_type_schemas();
    assert!(!schemas.is_empty(), "the gear registers its value types");
    for schema in schemas {
        let id = schema["$id"].as_str().expect("a schema names itself");
        assert_eq!(schema["$schema"], DRAFT_07, "{id}");
        assert!(
            schema
                .get("x-gts-abstract")
                .is_none_or(|flag| flag == false),
            "{id}: an abstract type describes no value"
        );
        let mut targets = Vec::new();
        references(&schema, &mut targets);
        assert_eq!(
            targets,
            Vec::<String>::new(),
            "{id}: the Settings Service resolves no reference outside a type's own parent chain"
        );
        // Building is the check: a schema that does not build validates nothing.
        validator(&schema);
    }
}

#[test]
fn the_timing_type_admits_advance_and_arrears_and_nothing_else() {
    let timing = validator_of(DEFAULT_TIMING);
    for admitted in [json!("advance"), json!("arrears")] {
        assert!(timing.is_valid(&admitted), "{admitted}");
    }
    for refused in [
        json!("monthly"),
        json!("Advance"),
        json!(""),
        json!(null),
        json!(1),
        json!(["advance"]),
    ] {
        assert!(!timing.is_valid(&refused), "{refused}");
    }
}

#[test]
fn a_short_code_is_a_string_of_at_most_64_characters() {
    for name in SHORT_CODES {
        let code = validator_of(name);
        for admitted in [json!(""), json!("4000-REV"), json!("x".repeat(64))] {
            assert!(code.is_valid(&admitted), "{name}: {admitted}");
        }
        for refused in [
            json!("x".repeat(65)),
            json!(null),
            json!(4000),
            json!(["x"]),
        ] {
            assert!(!code.is_valid(&refused), "{name}: {refused}");
        }
    }
}

#[test]
fn a_line_template_is_a_string_of_at_most_2000_characters() {
    for name in TEMPLATES {
        let template = validator_of(name);
        for admitted in [
            json!(""),
            json!("{sku} for {period}"),
            json!("x".repeat(2000)),
        ] {
            assert!(template.is_valid(&admitted), "{name}");
        }
        for refused in [json!("x".repeat(2001)), json!(null), json!(7), json!({})] {
            assert!(!template.is_valid(&refused), "{name}");
        }
    }
}

#[test]
fn a_line_template_is_text_and_the_placeholders_a_line_fills_in() {
    for name in TEMPLATES {
        let template = validator_of(name);
        for admitted in [
            "",
            "Monthly charge",
            "{sku} for {period}",
            "{sku_code}: {plan}, {dimension} per {unit}",
            "first line\nsecond line {sku}",
            "{sku}{period}",
        ] {
            assert!(template.is_valid(&json!(admitted)), "{name}: {admitted:?}");
        }
        for refused in [
            "Charge {foo}",
            "}",
            "{sku",
            "{{sku}}",
            "{}",
            "{SKU}",
            "{ sku }",
            "{sku} for {period}}",
            "{sku.name}",
        ] {
            assert!(!template.is_valid(&json!(refused)), "{name}: {refused:?}");
        }
    }
}

/// The schema and the rule an entry's own template is judged by agree on what a template is. An
/// empty text is the one difference: an entry may carry none, and a setting says "unset" by it.
#[test]
fn a_tenant_template_is_admitted_exactly_when_an_entry_could_carry_it() {
    use crate::domain::price_book_entry::validate_template;

    let template = validator_of(TEMPLATES[0]);
    for text in [
        "Monthly charge",
        "{sku} for {period}",
        "{sku}{sku_code}{unit}{plan}{period}{dimension}",
        "Charge {foo}",
        "}",
        "{",
        "{sku",
        "sku}",
        "{{sku}}",
        "{}",
        "{sku}}",
        "{period} {",
        "a\n{unit}\nb",
    ] {
        assert_eq!(
            template.is_valid(&json!(text)),
            validate_template(text).is_ok(),
            "{text:?}"
        );
    }
}

#[test]
fn the_offered_currencies_are_unique_three_letter_uppercase_codes() {
    let currencies = validator_of(OFFERED_CURRENCIES);
    for admitted in [json!([]), json!(["USD"]), json!(["USD", "EUR", "JPY"])] {
        assert!(currencies.is_valid(&admitted), "{admitted}");
    }
    for refused in [
        json!(["usd"]),
        json!(["USD", "USD"]),
        json!(["USDD"]),
        json!(["US"]),
        json!([" USD"]),
        json!(["USD", 978]),
        json!("USD"),
        json!(null),
    ] {
        assert!(!currencies.is_valid(&refused), "{refused}");
    }
}

// ── Registering the value types ─────────────────────────────────────────────

#[tokio::test]
async fn the_value_types_are_registered_as_the_gear_publishes_them() {
    let registry = RecordingRegistry::answering(Answer::Admits);
    register_value_types(&registry)
        .await
        .expect("the registry admits every schema");
    assert!(!value_type_schemas().is_empty(), "the gear has value types");
    assert_eq!(
        registry.batches(),
        vec![value_type_schemas()],
        "one batch carries every schema"
    );
}

#[tokio::test]
async fn a_value_type_the_registry_refuses_fails_the_registration() {
    let registry = RecordingRegistry::answering(Answer::RefusesFirst);
    let refused = register_value_types(&registry)
        .await
        .expect_err("a refused schema is a failed startup");
    let first = value_type_schemas().remove(0);
    let id = first["$id"].as_str().unwrap().trim_start_matches("gts://");
    assert!(
        format!("{refused:#}").contains(id),
        "the failure names the refused type: {refused:#}"
    );
}

#[tokio::test]
async fn a_registry_that_cannot_be_reached_fails_the_registration() {
    let registry = RecordingRegistry::answering(Answer::Fails);
    let failed = register_value_types(&registry)
        .await
        .expect_err("an unreachable registry is a failed startup");
    assert!(
        format!("{failed:#}").contains("register the settings value types"),
        "the failure says what was being done: {failed:#}"
    );
}

// ── Registering the declarations ────────────────────────────────────────────

/// A hub with a Settings Service double, and the double to inspect.
fn hub_answering(answer: Answer) -> (ClientHub, Arc<RecordingSettings>) {
    let hub = ClientHub::new();
    let settings = Arc::new(RecordingSettings::answering(answer));
    hub.register::<dyn SettingsContributionClient>(settings.clone());
    (hub, settings)
}

#[tokio::test]
async fn the_declarations_are_registered_in_one_call_under_the_gears_name() {
    let (hub, settings) = hub_answering(Answer::Admits);
    register_declarations(&hub)
        .await
        .expect("the Settings Service reconciles every declaration");

    let reconciles = settings.reconciles();
    assert_eq!(reconciles.len(), 1, "one reconcile carries the whole set");
    assert_eq!(reconciles[0].owner_module, BssPricingGear::MODULE_NAME);
    assert!(!reconciles[0].declarations.is_empty(), "nothing was sent");
    assert_eq!(
        serde_json::to_value(&reconciles[0].declarations).unwrap(),
        serde_json::to_value(declared()).unwrap(),
        "what is registered is what the gear declares"
    );
}

#[tokio::test]
async fn a_declaration_the_settings_service_refuses_fails_the_registration() {
    let (hub, settings) = hub_answering(Answer::RefusesFirst);
    let refused = register_declarations(&hub)
        .await
        .expect_err("a refused declaration is a failed startup");

    let sent = settings.reconciles().remove(0).declarations;
    let key = &sent.first().expect("a declaration was sent").key;
    let message = format!("{refused:#}");
    assert!(message.contains(key.as_str()), "names the key: {message}");
    assert!(
        message.contains(REFUSAL_CODE),
        "names the reason: {message}"
    );
}

#[tokio::test]
async fn without_a_settings_client_the_registration_fails() {
    let refused = register_declarations(&ClientHub::new())
        .await
        .expect_err("the gear does not start without the Settings Service");
    assert!(
        format!("{refused:#}").contains("SettingsContributionClient"),
        "names what is missing: {refused:#}"
    );
}

#[tokio::test]
async fn a_settings_service_that_fails_the_call_fails_the_registration() {
    let (hub, _) = hub_answering(Answer::Fails);
    let failed = register_declarations(&hub)
        .await
        .expect_err("a failed reconcile is a failed startup");
    assert!(
        format!("{failed:#}").contains("register the settings declarations"),
        "the failure says what was being done: {failed:#}"
    );
}
