# Phase 5: Watchdog Wiring — Replace Stub with Real Logic

## Goal

Replace the stub tick log in `orphan_watchdog.rs` with a real scan-finalize loop and inject the required dependencies through `background_workers.rs` and `gear.rs`.

## Current State

- Stub at `src/infra/workers/orphan_watchdog.rs` (98 lines): leader-elected periodic loop, logs `"orphan_watchdog: tick (stub -- no scan yet)"` on each tick. Accepts `(elector, config, cancel)`.
- `spawn_workers` at `src/background_workers.rs` passes only `(elector, config, cancel)` — no repo, DB, or service dependencies.
- `WorkerConfigs` struct holds only `orphan_watchdog: OrphanWatchdogConfig`.
- Two existing unit tests: `disabled_returns_immediately`, `shutdown_on_cancel`.

## Tasks

### 5.1 Define `OrphanWatchdogDeps` struct

File: `src/infra/workers/orphan_watchdog.rs`

Bundle all dependencies to keep the `run` signature clean:

```rust
/// Dependencies for the orphan watchdog scan-finalize loop.
pub struct OrphanWatchdogDeps<TR: TurnRepository + 'static, MR: MessageRepository + 'static> {
    pub finalization_svc: Arc<FinalizationService<TR, MR>>,
    pub turn_repo: Arc<TR>,
    pub db: Arc<DbProvider>,
    pub metrics: Arc<dyn MiniChatMetricsPort>,
}
```

### 5.2 Update `run` function signature

```rust
pub async fn run<TR: TurnRepository + 'static, MR: MessageRepository + 'static>(
    elector: Arc<dyn LeaderElector>,
    config: OrphanWatchdogConfig,
    deps: OrphanWatchdogDeps<TR, MR>,
    cancel: CancellationToken,
) -> anyhow::Result<()>
```

### 5.3 Implement scan-finalize loop

Replace the stub tick log with:

```rust
const BATCH_LIMIT: u32 = 100;

loop {
    tokio::select! {
        _ = ticker.tick() => {
            let scan_start = std::time::Instant::now();
            let conn = deps.db.conn();

            // 1. Find candidates
            let candidates = match deps.turn_repo
                .find_orphan_candidates(&conn, config.timeout_secs, BATCH_LIMIT)
                .await
            {
                Ok(c) => c,
                Err(e) => {
                    error!(error = %e, "orphan_watchdog: scan query failed");
                    continue; // retry on next tick
                }
            };

            info!(
                count = candidates.len(),
                "orphan_watchdog: scan completed"
            );

            // 2. Process each candidate
            for turn in &candidates {
                if cancel.is_cancelled() {
                    info!("orphan_watchdog: shutting down mid-scan");
                    return Ok(());
                }

                deps.metrics.record_orphan_detected("stale_progress");

                let input = OrphanFinalizationInput::from_turn(turn);
                match deps.finalization_svc
                    .finalize_orphan_turn(input, config.timeout_secs)
                    .await
                {
                    Ok(true) => {
                        deps.metrics.record_orphan_finalized("stale_progress");
                        info!(
                            turn_id = %turn.id,
                            tenant_id = %turn.tenant_id,
                            chat_id = %turn.chat_id,
                            "orphan_watchdog: finalized orphan turn"
                        );
                    }
                    Ok(false) => {
                        debug!(
                            turn_id = %turn.id,
                            "orphan_watchdog: CAS lost (already finalized or progress renewed)"
                        );
                    }
                    Err(e) => {
                        error!(
                            turn_id = %turn.id,
                            error = %e,
                            "orphan_watchdog: finalization error"
                        );
                        // Continue to next candidate — don't abort the scan.
                    }
                }
            }

            // 3. Record scan duration
            let scan_secs = scan_start.elapsed().as_secs_f64();
            deps.metrics.record_orphan_scan_duration_seconds(scan_secs);
        }
        () = cancel.cancelled() => {
            info!("orphan_watchdog: shutting down");
            return Ok(());
        }
    }
}
```

Design notes:
- **`BATCH_LIMIT = 100`**: bounds the scan result set. If more orphans exist, they're picked up on the next tick (60s default). In practice, orphan counts should be low.
- **`cancel.is_cancelled()` between candidates**: ensures prompt shutdown during a large batch.
- **Scan query failure**: logs error and retries on the next tick. Does not abort the watchdog.
- **Individual finalization error**: logs and continues. One bad turn doesn't block others.
- **Structured logging**: every outcome is logged with `turn_id`, `tenant_id`, `chat_id`.

### 5.4 Update `background_workers.rs`

File: `src/background_workers.rs`

Expand `spawn_workers` to accept the orphan watchdog dependencies:

```rust
pub fn spawn_workers<TR, MR>(
    configs: &WorkerConfigs,
    parent_cancel: &CancellationToken,
    leader_elector: Option<&Arc<dyn LeaderElector>>,
    orphan_deps: Option<OrphanWatchdogDeps<TR, MR>>,  // None if watchdog disabled
) -> anyhow::Result<(WorkerHandles, CancellationToken)>
where
    TR: TurnRepository + 'static,
    MR: MessageRepository + 'static,
```

Alternative: add deps to `WorkerConfigs` as an `Option<OrphanWatchdogDeps>`, or create a separate `WorkerDeps` struct. Choose whichever approach is cleanest given the existing signature.

### 5.5 Update `gear.rs` wiring

File: `src/gear.rs`

In the `start()` method, construct `OrphanWatchdogDeps` from the already-available services:

```rust
let orphan_deps = if self.worker_configs.orphan_watchdog.enabled {
    Some(OrphanWatchdogDeps {
        finalization_svc: Arc::clone(&self.finalization_svc),
        turn_repo: Arc::clone(&self.turn_repo),
        db: Arc::clone(&self.db),
        metrics: Arc::clone(&self.metrics),
    })
} else {
    None
};
```

Pass `orphan_deps` to `spawn_workers`.

### 5.6 Update existing tests

File: `src/infra/workers/orphan_watchdog.rs` (test module)

The existing `disabled_returns_immediately` and `shutdown_on_cancel` tests need to be updated to pass the new `OrphanWatchdogDeps` parameter. For these tests, use mock/noop implementations:
- `MockTurnRepository` that returns empty candidates
- `MockFinalizationService` (or construct a real one with mock repos)
- Noop metrics

## Acceptance Criteria

- [ ] Stub tick log replaced with real scan-finalize loop
- [ ] Dependencies injected through `background_workers.rs` → `gear.rs`
- [ ] Graceful shutdown between candidates (`cancel.is_cancelled()` check)
- [ ] Leader election still works correctly
- [ ] Scan query failures don't crash the watchdog (retry on next tick)
- [ ] Individual finalization errors don't block other candidates
- [ ] Structured logging for each outcome (finalized, CAS lost, error)
- [ ] Existing tests updated and passing
