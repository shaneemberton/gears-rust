//! The `cache_invalidate` events of a request: the broker-native envelope, the
//! size accounting against the broker's event limit, and the split of a key set
//! that would not fit.
//!
//! The domain gathers what a request touched ([`TenantKeys`]) and decides what
//! becomes of each publish; what an event looks like on the wire is the broker
//! client's, so it is built here.

use event_broker_sdk::validate::is_trace_parent;
use event_broker_sdk::{EventBrokerError, EventV1, GtsTypeId};
use settings_service_sdk::gts::{ACTIVATION_SUBJECT_TYPE, CACHE_INVALIDATE_EVENT_TYPE};
use tracing::{debug, warn};

use crate::domain::coherence::{BroadcastContext, TenantKeys};
use crate::infra::event_schemas::CacheInvalidation;

/// The envelope `source` of every broadcast: the gear's name.
pub const EVENT_SOURCE: &str = "settings-service";

/// The broker's event size limit, measured as the broker measures it: the
/// event's JSON form, every member included ([`EventV1::encoded_len`]). The
/// broker refuses a larger event, so a broadcast stays strictly under it, with
/// [`HEADROOM`] to spare.
pub const BROKER_EVENT_LIMIT_BYTES: usize = EventV1::MAX_ENCODED_BYTES;

/// What a part keeps free below the limit. The base of a part is measured on
/// the envelope of its first event, so the room left covers everything that can
/// differ between the events of one request (the timestamp's fractional digits,
/// at most nine) by a margin far wider than any of it; the maximal-width
/// timestamp is not measured.
pub const HEADROOM: usize = 1024;

/// The size of `event` as the broker's limit counts it. An event that cannot
/// be measured, which no JSON `data` is, counts as over any limit, so the split
/// keeps each key of it apart.
#[must_use]
pub fn serialised_len(event: &EventV1) -> usize {
    event.encoded_len().unwrap_or(usize::MAX)
}

/// The request's trace context as the envelope may carry it: the broker refuses
/// an event whose `trace_parent` is not a W3C traceparent, and a broadcast is
/// worth more than the trace link, so a malformed one is left out.
fn envelope_trace_parent(ctx: &BroadcastContext) -> Option<&str> {
    let trace_parent = ctx.trace_parent.as_deref()?;
    if is_trace_parent(trace_parent) {
        Some(trace_parent)
    } else {
        debug!(
            change_set_id = %ctx.change_set_id,
            "malformed trace context left out of the cache_invalidate envelope"
        );
        None
    }
}

fn envelope(
    ctx: &BroadcastContext,
    trace_parent: Option<&str>,
    tenant: &str,
    keys: Vec<String>,
) -> Result<EventV1, EventBrokerError> {
    // @cpt-begin:cpt-cf-settings-service-flow-replica-coherence-publish:p1:inst-rc-publish-3
    let body = CacheInvalidation {
        change_set_id: ctx.change_set_id,
        tenant: tenant.to_owned(),
        changed_keys: keys,
        subject_type: None,
        subject_id: None,
    };
    // @cpt-end:cpt-cf-settings-service-flow-replica-coherence-publish:p1:inst-rc-publish-3
    // @cpt-begin:cpt-cf-settings-service-flow-replica-coherence-publish:p1:inst-rc-publish-7
    // The builder fixes the event's id (a fresh UUID v7) and `occurred_at`
    // (now) when it builds.
    let event_type = GtsTypeId::new(CACHE_INVALIDATE_EVENT_TYPE);
    let builder = match serde_json::to_value(&body) {
        Ok(data) => EventV1::builder_for(event_type, data),
        Err(_) => EventV1::builder_without_data(event_type),
    }
    .tenant_id(ctx.root_tenant)
    .source(EVENT_SOURCE)
    .subject(tenant)
    .subject_type_id(GtsTypeId::new(ACTIVATION_SUBJECT_TYPE));
    match trace_parent {
        Some(trace_parent) => builder.trace_parent(trace_parent),
        None => builder,
    }
    .build()
    // @cpt-end:cpt-cf-settings-service-flow-replica-coherence-publish:p1:inst-rc-publish-7
}

/// Keep a built part. Every member of the envelope is the gear's own and the
/// tenant's envelope already built once, so a refusal here is a broken
/// invariant; it is logged rather than published broken.
fn keep(events: &mut Vec<EventV1>, tenant: &str, built: Result<EventV1, EventBrokerError>) {
    match built {
        Ok(event) => events.push(event),
        Err(error) => warn!(
            %tenant,
            %error,
            "cache_invalidate not built; peers converge within cache_ttl_seconds"
        ),
    }
}

/// The broadcasts of a request: per tenant, in first-touch order, one event or
/// — when its key set would not fit the broker's limit — several, keys packed
/// in order until the next would reach the limit less [`HEADROOM`], each part
/// under the limit, with its own `id`.
#[must_use]
pub fn build_broadcasts(touched: &[TenantKeys], ctx: &BroadcastContext) -> Vec<EventV1> {
    let budget = BROKER_EVENT_LIMIT_BYTES - HEADROOM;
    let trace_parent = envelope_trace_parent(ctx);
    let mut events = Vec::new();
    for tenant in touched {
        // @cpt-begin:cpt-cf-settings-service-flow-replica-coherence-publish:p1:inst-rc-publish-4
        let part_of = |keys| envelope(ctx, trace_parent, &tenant.tenant, keys);
        let base = match part_of(Vec::new()) {
            Ok(empty) => serialised_len(&empty),
            Err(error) => {
                keep(&mut events, &tenant.tenant, Err(error));
                continue;
            }
        };
        let mut part: Vec<String> = Vec::new();
        let mut len = base;
        for key in &tenant.keys {
            let quoted = serde_json::to_string(key).map_or(key.len() + 2, |s| s.len());
            // The comma between two keys of one part.
            let cost = quoted + usize::from(!part.is_empty());
            if !part.is_empty() && len.saturating_add(cost) >= budget {
                keep(
                    &mut events,
                    &tenant.tenant,
                    part_of(std::mem::take(&mut part)),
                );
                len = base;
            }
            len = len.saturating_add(quoted + usize::from(!part.is_empty()));
            part.push(key.clone());
        }
        if !part.is_empty() {
            keep(&mut events, &tenant.tenant, part_of(part));
        }
        // @cpt-end:cpt-cf-settings-service-flow-replica-coherence-publish:p1:inst-rc-publish-4
    }
    events
}

#[cfg(test)]
#[path = "broadcast_tests.rs"]
mod broadcast_tests;
