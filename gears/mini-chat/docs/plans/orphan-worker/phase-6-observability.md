# Phase 6: Observability — Metrics

## Goal

Wire the three DESIGN.md-specified orphan watchdog metrics into the `MiniChatMetricsPort` trait and the OpenTelemetry implementation.

## Current State

- `MiniChatMetricsPort` trait at `src/domain/ports/metrics.rs` has no orphan-specific methods.
- `MiniChatMetricsMeter` at `src/infra/metrics.rs` has two deferred instruments:
  - `cancel_orphan: Counter<u64>` (line 108) — tracks "orphaned cancel events" (a turn remaining running after cancellation). This is a different concept — keep as-is.
  - `orphan_turn: Counter<u64>` (line 189) — placeholder for "orphan turns detected by watchdog". Rename to `orphan_detected`.
- Metric label `trigger::ORPHAN_TIMEOUT = "orphan_timeout"` exists at `src/domain/ports/metric_labels.rs:108`.
- No `reason` label constants exist yet (needed for `reason="stale_progress"`).

## Required Metrics (from DESIGN.md)

| Metric Name | Type | Labels | Description |
|-------------|------|--------|-------------|
| `mini_chat_orphan_detected_total` | counter | `reason="stale_progress"` | Orphan candidate identified by stale-progress scan |
| `mini_chat_orphan_finalized_total` | counter | `reason="stale_progress"` | CAS finalization succeeded (turn transitioned to terminal) |
| `mini_chat_orphan_scan_duration_seconds` | histogram | — | Watchdog scan execution duration |

Note: OTel counter names omit `_total` — the Prometheus exporter appends it automatically.

## Tasks

### 6.1 Add `reason` label constant

File: `src/domain/ports/metric_labels.rs`

```rust
pub mod reason {
    pub const STALE_PROGRESS: &str = "stale_progress";
}
```

### 6.2 Add methods to `MiniChatMetricsPort` trait

File: `src/domain/ports/metrics.rs`

```rust
// ── P1: Orphan Watchdog (3 metrics) ─────────────────────────────────

/// `{prefix}_orphan_detected` — counter
/// Increments when watchdog identifies orphan candidate by stale-progress rule.
/// `reason`: `stale_progress`
fn record_orphan_detected(&self, reason: &str);

/// `{prefix}_orphan_finalized` — counter
/// Increments ONLY when watchdog wins CAS finalization.
/// `reason`: `stale_progress`
fn record_orphan_finalized(&self, reason: &str);

/// `{prefix}_orphan_scan_duration_seconds` — histogram
/// Watchdog scan execution duration (including candidate processing).
fn record_orphan_scan_duration_seconds(&self, seconds: f64);
```

### 6.3 Add no-op implementations

File: `src/domain/ports/metrics.rs` (in `NoopMetrics` impl)

```rust
fn record_orphan_detected(&self, _reason: &str) {}
fn record_orphan_finalized(&self, _reason: &str) {}
fn record_orphan_scan_duration_seconds(&self, _seconds: f64) {}
```

### 6.4 Update infra metrics struct

File: `src/infra/metrics.rs`

Replace the deferred `orphan_turn` field with properly named instruments:

```rust
// ── P1: Orphan Watchdog ──────────────────────────────────────────────
orphan_detected: Counter<u64>,
orphan_finalized: Counter<u64>,
orphan_scan_duration: Histogram<f64>,
```

Remove the `#[allow(dead_code)]` and `// deferred:` comments since these are now active.

### 6.5 Register instruments with OTel meter

File: `src/infra/metrics.rs` (in constructor)

```rust
orphan_detected: meter
    .u64_counter(format!("{prefix}_orphan_detected"))
    .with_description("Orphan turns detected by watchdog")
    .build(),
orphan_finalized: meter
    .u64_counter(format!("{prefix}_orphan_finalized"))
    .with_description("Orphan turns finalized by watchdog (CAS won)")
    .build(),
orphan_scan_duration: meter
    .f64_histogram(format!("{prefix}_orphan_scan_duration_seconds"))
    .with_description("Watchdog scan execution duration")
    .build(),
```

### 6.6 Implement `MiniChatMetricsPort` methods

File: `src/infra/metrics.rs` (in `impl MiniChatMetricsPort for MiniChatMetricsMeter`)

```rust
fn record_orphan_detected(&self, reason: &str) {
    self.orphan_detected.add(1, &[KeyValue::new(key::REASON, reason.to_owned())]);
}

fn record_orphan_finalized(&self, reason: &str) {
    self.orphan_finalized.add(1, &[KeyValue::new(key::REASON, reason.to_owned())]);
}

fn record_orphan_scan_duration_seconds(&self, seconds: f64) {
    self.orphan_scan_duration.record(seconds, &[]);
}
```

Add `REASON` constant to `key` gear in `metric_labels.rs` if not already present:
```rust
pub const REASON: &str = "reason";
```

### 6.7 Keep `cancel_orphan` as-is

The existing `cancel_orphan` counter tracks a different concept — "a turn remained running longer than configured timeout after cancellation." This is NOT the same as the orphan watchdog detection. Keep it unchanged as a deferred metric.

## Acceptance Criteria

- [ ] All 3 metrics registered with OTel meter and exposed at `/metrics`
- [ ] Metric names match DESIGN.md: `orphan_detected`, `orphan_finalized`, `orphan_scan_duration_seconds`
- [ ] `reason` label populated with `"stale_progress"` on counter metrics
- [ ] `NoopMetrics` updated with no-op implementations
- [ ] Existing metrics (including `cancel_orphan`) unbroken
- [ ] Deferred `orphan_turn` field removed/replaced — no dead code
