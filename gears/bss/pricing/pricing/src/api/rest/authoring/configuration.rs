//! Atomic dimension registry operations, and the offered-currency rule of a new book.
//!
//! @cpt-dod:cpt-cf-bss-pricing-dod-dimension-registry:p1
use super::{
    dto::{
        PricingDimensionKey, PricingDimensionKeyPatch, PricingDimensionRegistry,
        PricingDimensionValue, PricingDimensionValueUsage, PricingDimensions,
    },
    support::{DoorError, audit, conflict, conflict_because, invalid, response, value},
};
use crate::{
    domain::dimension,
    infra::storage::{
        RepoError,
        entity::dimension_key,
        repo::{dimension_repo, price_book_entry_repo, price_repo},
    },
};
use axum::{http::StatusCode, response::Response};
use std::collections::{BTreeMap, BTreeSet};
use toolkit_canonical_errors::CanonicalError;
use toolkit_db::secure::{AccessScope, DBRunner};
use toolkit_security::SecurityContext;
use uuid::Uuid;

/// D-438: a NEW book takes a currency the tenant offers; an empty list offers every currency.
/// `offered` is the tenant's list as the Settings Service told it before the transaction
/// (D-541): the book author needs no grant on the settings.
/// # Errors
/// 409 `CURRENCY_NOT_OFFERED`.
pub fn offer_currency(offered: &[String], currency: &str) -> Result<(), DoorError> {
    if offered.is_empty() || offered.iter().any(|c| c == currency) {
        return Ok(());
    }
    Err(conflict_because(
        "CURRENCY_NOT_OFFERED",
        format!(
            "the tenant settings offer {}, not {currency}",
            offered.join(", ")
        ),
    )
    .into())
}
/// The stored registry, by key, and its content tag: the first 64 SHA-256 bits of the complete
/// sorted collection, each row's version included, which keeps the door's strong decimal-tag
/// grammar. What uses a value is not part of it.
async fn stored(
    tx: &impl DBRunner,
    scope: &AccessScope,
    tenant: Uuid,
) -> Result<(Vec<(String, Vec<String>, i64)>, u64), DoorError> {
    let rows = dimension_repo::list(tx, scope, tenant).await?;
    let content: Vec<_> = rows
        .iter()
        .map(|r| (&r.key, &r.values, r.version))
        .collect();
    let hash =
        crate::api::rest::preconditions::request_digest(&content).map_err(CanonicalError::from)?;
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&hash[..8]);
    let tag = u64::from_be_bytes(bytes);
    let mut keys = Vec::new();
    for r in rows {
        let values = serde_json::from_value(r.values)
            .map_err(|e| RepoError::CorruptRow(format!("dimension {} values: {e}", r.key)))?;
        keys.push((r.key, values, r.version));
    }
    Ok((keys, tag))
}
/// D-436: the prices of every state that use each `(key, value)`, from ONE grouped count.
async fn value_use(
    tx: &impl DBRunner,
    tenant: Uuid,
) -> Result<BTreeMap<(String, String), u64>, DoorError> {
    let mut used = BTreeMap::new();
    for row in price_repo::count_by_key_and_value(tx, tenant).await? {
        let count = u64::try_from(row.count)
            .map_err(|_| RepoError::CorruptRow(format!("a negative price count {}", row.count)))?;
        used.insert((row.dimension_key, row.dim_value), count);
    }
    Ok(used)
}
/// The registry as its doors answer it: each key with its values and their use. A tenant that
/// stores no registry reads the seed key, declared and not yet valued (spec decision 4).
fn render(
    keys: Vec<(String, Vec<String>, i64)>,
    used: &BTreeMap<(String, String), u64>,
) -> PricingDimensionRegistry {
    let mut items: Vec<PricingDimensionKey> = keys
        .into_iter()
        .map(|(key, values, _)| {
            let values = values
                .into_iter()
                .map(|value| {
                    let prices = used
                        .get(&(key.clone(), value.clone()))
                        .copied()
                        .unwrap_or_default();
                    PricingDimensionValue {
                        value,
                        usage: PricingDimensionValueUsage { prices },
                    }
                })
                .collect();
            PricingDimensionKey { key, values }
        })
        .collect();
    if items.is_empty() {
        items.push(PricingDimensionKey {
            key: dimension::SEED_KEY.into(),
            values: Vec::new(),
        });
    }
    PricingDimensionRegistry { items }
}
/// The first value of `removed` a price uses, as the 409 that names it (D-436).
fn value_in_use(
    used: &BTreeMap<(String, String), u64>,
    key: &str,
    removed: impl Fn(&str) -> bool,
) -> Option<CanonicalError> {
    used.iter()
        .find(|((k, v), n)| k == key && **n > 0 && removed(v))
        .map(|((k, v), n)| {
            conflict_because(
                "DIM_VALUE_IN_USE",
                format!(
                    "{k}={v} is used by {n} price{}",
                    if *n == 1 { "" } else { "s" }
                ),
            )
        })
}
pub async fn dimensions(
    tx: &impl DBRunner,
    scope: &AccessScope,
    tenant: Uuid,
) -> Result<(PricingDimensionRegistry, u64), DoorError> {
    let (keys, tag) = stored(tx, scope, tenant).await?;
    let used = value_use(tx, tenant).await?;
    Ok((render(keys, &used), tag))
}
pub async fn put_dimensions(
    tx: &impl DBRunner,
    scope: &AccessScope,
    ctx: &SecurityContext,
    correlation: Uuid,
    version: u64,
    mut body: PricingDimensions,
) -> Result<Response, DoorError> {
    let tenant = ctx.subject_tenant_id();
    let (old, tag) = stored(tx, scope, tenant).await?;
    if tag != version {
        return Err(conflict("STALE_REVISION").into());
    }
    let mut keys = BTreeSet::new();
    for item in &mut body.items {
        item.key = item.key.trim().into();
        item.values = item
            .values
            .iter()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .collect();
        if !keys.insert(item.key.clone()) {
            return Err(invalid("items", "DIM_KEY_DUPLICATE").into());
        }
        // D-457: only the key or values the stored registry does not hold are capped, so a row
        // stored before the caps never locks the registry.
        let held = old
            .iter()
            .find(|(key, _, _)| *key == item.key)
            .map(|(_, values, _)| values.as_slice());
        super::caps::new_dimension_text(&item.key, &item.values, held)?;
        if let Some(e) = dimension::validate(&item.key, &item.values).first() {
            return Err(invalid("items", e.code).into());
        }
    }
    // D-436: every state counts (a rejected or pending price still carries its value), read in
    // one grouped count and one read of the keys entries name, whatever the number of entries.
    let used = value_use(tx, tenant).await?;
    for (key, _) in used.keys() {
        let next = body.items.iter().find(|i| &i.key == key);
        if let Some(refusal) = value_in_use(&used, key, |v| {
            next.is_none_or(|i| !i.values.iter().any(|kept| kept == v))
        }) {
            return Err(refusal.into());
        }
    }
    // An entry names the key whatever its prices hold (the entry's key is a foreign key to the
    // registry): removing it is the same refusal PATCH /price-book-entries answers.
    for named in price_book_entry_repo::named_keys(tx, tenant).await? {
        if !keys.contains(&named) {
            return Err(conflict_because(
                "DIMENSION_KEY_IN_USE",
                format!("a price book entry names the key {named}"),
            )
            .into());
        }
    }
    for (key, _, version) in &old {
        if !keys.contains(key) {
            dimension_repo::delete(tx, scope, tenant, key, *version).await?;
        }
    }
    for item in body.items {
        let prior = old.iter().find(|(key, _, _)| *key == item.key);
        let values = value(&item.values)?;
        let model = dimension_key::Model {
            tenant_id: tenant,
            key: item.key,
            values,
            version: prior.map_or(1, |(_, _, v)| *v),
        };
        if prior.is_some() {
            dimension_repo::update(tx, scope, model).await?;
        } else {
            dimension_repo::insert(tx, scope, model).await?;
        }
    }
    audit(tx, ctx, correlation, "dimension_keys.update", tenant, 0).await?;
    // The write moved no price, so the use read above still holds.
    let (keys, tag) = stored(tx, scope, tenant).await?;
    Ok(response(StatusCode::OK, &render(keys, &used), Some(tag))?)
}
/// `PATCH /dimension-keys` (D-436): add and remove values of ONE declared key — stored, or the
/// seed key while the tenant stores no registry — at the registry the caller read (If-Match).
/// The result keeps the key's values in their order without the removed ones, then the added
/// ones in the order sent; it is judged as the PUT judges a key. An empty patch writes nothing.
/// # Errors
/// 400 `DIM_NOT_DECLARED`, `DIM_VALUE_DUPLICATE`, `DIM_VALUE_UNKNOWN`, or the key's own rule
/// (`DIM_VALUE_INVALID`, `DIM_VALUES_FEW`); 409 `DIM_VALUE_IN_USE` naming the value, or
/// `STALE_REVISION`.
pub async fn patch_dimensions(
    tx: &impl DBRunner,
    scope: &AccessScope,
    ctx: &SecurityContext,
    correlation: Uuid,
    version: u64,
    body: PricingDimensionKeyPatch,
) -> Result<Response, DoorError> {
    let tenant = ctx.subject_tenant_id();
    let (old, tag) = stored(tx, scope, tenant).await?;
    if tag != version {
        return Err(conflict("STALE_REVISION").into());
    }
    let key = body.key.trim().to_owned();
    let prior = old.iter().find(|(k, _, _)| *k == key);
    if prior.is_none() && !(key == dimension::SEED_KEY && old.is_empty()) {
        return Err(invalid("key", "DIM_NOT_DECLARED").into());
    }
    let current: Vec<String> = prior
        .map(|(_, values, _)| values.clone())
        .unwrap_or_default();
    let clean = |values: &[String]| -> Vec<String> {
        values
            .iter()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .collect()
    };
    let (add, remove) = (clean(&body.add), clean(&body.remove));
    let mut named = BTreeSet::new();
    for v in add.iter().chain(&remove) {
        if !named.insert(v.as_str()) || (add.contains(v) && current.contains(v)) {
            return Err(invalid("add", "DIM_VALUE_DUPLICATE").into());
        }
    }
    if remove.iter().any(|v| !current.contains(v)) {
        return Err(invalid("remove", "DIM_VALUE_UNKNOWN").into());
    }
    let used = value_use(tx, tenant).await?;
    if add.is_empty() && remove.is_empty() {
        return Ok(response(StatusCode::OK, &render(old, &used), Some(tag))?);
    }
    let next: Vec<String> = current
        .into_iter()
        .filter(|v| !remove.contains(v))
        .chain(add)
        .collect();
    if let Some(e) = dimension::validate(&key, &next).first() {
        return Err(invalid("add", e.code).into());
    }
    if let Some(refusal) = value_in_use(&used, &key, |v| remove.iter().any(|r| r == v)) {
        return Err(refusal.into());
    }
    let model = dimension_key::Model {
        tenant_id: tenant,
        key,
        values: value(&next)?,
        version: prior.map_or(1, |(_, _, v)| *v),
    };
    if prior.is_some() {
        dimension_repo::update(tx, scope, model).await?;
    } else {
        dimension_repo::insert(tx, scope, model).await?;
    }
    audit(tx, ctx, correlation, "dimension_keys.patch", tenant, 0).await?;
    let (keys, tag) = stored(tx, scope, tenant).await?;
    Ok(response(StatusCode::OK, &render(keys, &used), Some(tag))?)
}
