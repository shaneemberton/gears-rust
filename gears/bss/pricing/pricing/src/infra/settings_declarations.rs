//! The tenant pricing settings this gear declares in the Settings Service.
//!
//! The gear owns the setting keys, the value types their values validate against, and the two
//! startup steps that publish them: the value types go to the types registry during `init`, the
//! declarations to the Settings Service during `post_init`, once the registry has finalized what
//! `init` registered.

use std::num::NonZeroU32;
use std::sync::LazyLock;

use anyhow::Context as _;
use serde_json::{Value, json};
use settings_service_sdk::models::{ContributedClassification, ContributedDeclaration, ScopeClass};
use settings_service_sdk::{SettingKey, SettingKeyError, SettingsContributionClient};
use toolkit::client_hub::ClientHub;
use toolkit_gts::gts_id;
use types_registry_sdk::{RegisterResult, TypesRegistryClient};
use uuid::Uuid;

use crate::infra::reference_ticker::system_actor;
use crate::module::BssPricingGear;

const VENDOR: &str = "cf";
const PACKAGE: &str = "bss";
const CATEGORY: &str = "pricing";

/// When a charge is billed: before the period it covers, or after it.
const BILLING_TIMING: &str = gts_id!("cf.bss.pricing.setting_billing_timing.v1~");
/// A ledger or tax code. The empty string means none is set.
const SHORT_CODE: &str = gts_id!("cf.bss.pricing.setting_short_code.v1~");
/// An invoice line template: text and the placeholders an invoice line fills in. The empty string
/// means none is set.
const LINE_TEMPLATE: &str = gts_id!("cf.bss.pricing.setting_line_template.v1~");
/// Three-letter currency codes, each once. The empty list offers every currency.
const CURRENCY_CODES: &str = gts_id!("cf.bss.pricing.setting_currency_codes.v1~");

/// A template is text outside braces and, in braces, a placeholder a line can fill in. It follows
/// `domain::price_book_entry::validate_template`, which judges an entry's own template: a tenant
/// default that an entry could not carry would reach an invoice line unfilled. A NUL character
/// is no text: the stores a resolved value is written to refuse it.
const LINE_TEMPLATE_PATTERN: &str =
    r"^(?:[^{}\x00]|\{(?:sku|sku_code|unit|plan|period|dimension)\})*$";
/// A code is any text without a NUL character.
const SHORT_CODE_PATTERN: &str = r"^[^\x00]*$";

/// One declared setting: its leaf name, value type, default and what an administrator reads.
struct Setting {
    name: &'static str,
    value_type_id: &'static str,
    default: Value,
    display_name: &'static str,
    description: String,
}

fn settings() -> Vec<Setting> {
    // `domain::resolve::invoice_inputs` takes a line's template from the entry, then from the
    // SKU, and only then from this setting.
    let template = |name, display_name, sku_kind: &str| Setting {
        name,
        value_type_id: LINE_TEMPLATE,
        default: json!(""),
        display_name,
        description: format!(
            "The invoice line text of {sku_kind} SKU when neither its price book entry nor the \
             SKU carries a template: an entry's own template wins, then the SKU's. The text may \
             use the placeholders {{sku}}, {{sku_code}}, {{unit}}, {{plan}}, {{period}} and \
             {{dimension}}. Empty means no default."
        ),
    };
    vec![
        Setting {
            name: "default_timing",
            value_type_id: BILLING_TIMING,
            default: json!("advance"),
            display_name: "Default billing timing",
            description: "Whether a charge is billed in advance or in arrears when its SKU does \
                          not say."
                .to_owned(),
        },
        Setting {
            name: "default_gl",
            value_type_id: SHORT_CODE,
            default: json!(""),
            display_name: "Default GL code",
            description: "The general ledger code of an invoice line whose SKU names none. Empty \
                          means no default."
                .to_owned(),
        },
        Setting {
            name: "default_tax_category",
            value_type_id: SHORT_CODE,
            default: json!(""),
            display_name: "Default tax category",
            description: "The tax category of an invoice line whose SKU names none. Empty means \
                          no default."
                .to_owned(),
        },
        template(
            "invoice_line_template_recurring",
            "Invoice line template: recurring",
            "a recurring",
        ),
        template(
            "invoice_line_template_usage",
            "Invoice line template: usage",
            "a usage",
        ),
        template(
            "invoice_line_template_one_time",
            "Invoice line template: one-time",
            "a one-time",
        ),
        template(
            "invoice_line_template_bundle",
            "Invoice line template: bundle",
            "a bundle",
        ),
        Setting {
            name: "offered_currencies",
            value_type_id: CURRENCY_CODES,
            default: json!([]),
            display_name: "Offered currencies",
            description: "The currencies a new price book may be created in, as three-letter \
                          currency codes. Empty means any currency."
                .to_owned(),
        },
    ]
}

/// A concrete schema of the value type `type_id`. The Settings Service validates a value under
/// the draft the schema's own `$schema` names, and resolves no reference outside a type's own
/// parent chain, so each schema states its draft and is self-contained.
fn value_type_schema(type_id: &str, title: &str, constraints: Value) -> Value {
    let mut schema = json!({
        "$id": format!("gts://{type_id}"),
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": title,
    });
    if let (Value::Object(schema), Value::Object(constraints)) = (&mut schema, constraints) {
        schema.extend(constraints);
    }
    schema
}

/// The schemas of the gear's own value types, as the types registry receives them.
#[must_use]
pub fn value_type_schemas() -> Vec<Value> {
    vec![
        value_type_schema(
            BILLING_TIMING,
            "BSS pricing billing timing",
            json!({ "type": "string", "enum": ["advance", "arrears"] }),
        ),
        value_type_schema(
            SHORT_CODE,
            "BSS pricing short code",
            json!({ "type": "string", "maxLength": 64, "pattern": SHORT_CODE_PATTERN }),
        ),
        value_type_schema(
            LINE_TEMPLATE,
            "BSS pricing invoice line template",
            json!({ "type": "string", "maxLength": 2000, "pattern": LINE_TEMPLATE_PATTERN }),
        ),
        value_type_schema(
            CURRENCY_CODES,
            "BSS pricing currency codes",
            json!({
                "type": "array",
                "items": { "type": "string", "pattern": "^[A-Z]{3}$" },
                "uniqueItems": true,
            }),
        ),
    ]
}

/// The declarations of the tenant pricing settings.
///
/// Each belongs to one tenant and is not inherited: a tenant's pricing follows its own settings
/// only.
///
/// # Errors
///
/// When a setting key does not parse, which is a defect in this gear.
pub fn declarations() -> Result<Vec<ContributedDeclaration>, SettingKeyError> {
    settings()
        .into_iter()
        .map(|setting| {
            let key =
                SettingKey::contributed(VENDOR, PACKAGE, CATEGORY, setting.name, NonZeroU32::MIN)?;
            let mut declaration = ContributedDeclaration::new(
                key,
                setting.value_type_id,
                setting.default,
                ScopeClass::Local,
            );
            declaration.display_name = Some(setting.display_name.to_owned());
            declaration.description = Some(setting.description);
            declaration.data_classification = Some(ContributedClassification::Public);
            declaration.requires_step_up = Some(true);
            Ok(declaration)
        })
        .collect()
}

/// The keys of the declared settings, built once: every read of a tenant's settings asks for them.
///
/// # Errors
///
/// When a setting key does not parse, which is a defect in this gear.
pub fn setting_keys() -> Result<&'static [SettingKey], &'static str> {
    static KEYS: LazyLock<Result<Vec<SettingKey>, String>> = LazyLock::new(|| {
        declarations()
            .map(|declared| declared.into_iter().map(|d| d.key).collect())
            .map_err(|error| error.to_string())
    });
    match &*KEYS {
        Ok(keys) => Ok(keys),
        Err(error) => Err(error),
    }
}

/// Register the gear's own value types in the types registry. Called from the gear's `init`.
///
/// # Errors
///
/// When the registry call fails or the registry refuses a schema.
pub async fn register_value_types(registry: &dyn TypesRegistryClient) -> anyhow::Result<()> {
    let results = registry
        .register(value_type_schemas())
        .await
        .context("bss-pricing: register the settings value types")?;
    for result in results {
        if let RegisterResult::Err { gts_id, error } = result {
            anyhow::bail!(
                "bss-pricing: failed to register the settings value type {}: {error}",
                gts_id.as_deref().unwrap_or("?")
            );
        }
    }
    Ok(())
}

/// Register the declarations with the Settings Service found in `hub`. Called from the gear's
/// `post_init`.
///
/// # Errors
///
/// When no settings client is registered, the call fails, or the Settings Service refuses a
/// declaration.
pub async fn register_declarations(hub: &ClientHub) -> anyhow::Result<()> {
    let settings = hub.try_get::<dyn SettingsContributionClient>().context(
        "bss-pricing: SettingsContributionClient absent from ClientHub; \
             settings-service module must be registered",
    )?;
    let declarations =
        declarations().context("bss-pricing: a declared setting key does not parse")?;
    // The declarations are the gear's, not a tenant's: the actor carries no tenant.
    let actor =
        system_actor(Uuid::nil()).context("bss-pricing: the system actor could not be built")?;
    let declared = declarations.len();
    let reconciled = settings
        .register_declarations(&actor, BssPricingGear::MODULE_NAME.to_owned(), declarations)
        .await
        .context("bss-pricing: register the settings declarations")?;
    // A refused declaration does not fail the call: it comes back beside the reconciled ones.
    if !reconciled.errors.is_empty() {
        let refused: Vec<String> = reconciled
            .errors
            .iter()
            .map(|error| format!("{} ({}: {})", error.key, error.code, error.message))
            .collect();
        anyhow::bail!(
            "bss-pricing: the Settings Service refused settings declarations: {}",
            refused.join("; ")
        );
    }
    tracing::info!(
        declared,
        registered = reconciled.registered,
        updated = reconciled.updated,
        reactivated = reconciled.reactivated,
        retired = reconciled.retired,
        "bss-pricing: the tenant settings declarations are reconciled with the Settings Service"
    );
    Ok(())
}

/// Doubles of the types registry and of the Settings Service for the gear's startup tests.
#[cfg(any(test, feature = "test-support"))]
#[path = "settings_declarations_doubles.rs"]
pub mod doubles;

#[cfg(test)]
#[path = "settings_declarations_tests.rs"]
mod settings_declarations_tests;
