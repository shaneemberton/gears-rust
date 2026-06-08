# Thread Summary Worker Implementation Plan

Implements `cpt-cf-mini-chat-seq-thread-summary` (P1) from DESIGN.md.

## Context

When a long conversation accumulates enough context to approach the model's token budget,
the system must asynchronously summarize older messages into a compressed "thread summary."
This is a Level 1 compression strategy: the LLM generates a summary of older messages,
which replaces them in the context window for subsequent turns. This keeps token costs
bounded while preserving key facts, decisions, and document references.

The infrastructure stubs already exist:

1. **`thread_summaries` table** — created in the initial migration
   (`m20260302_000001_initial.rs:186-197`), but the current schema has `summarized_up_to UUID`
   which stores only the message ID. DESIGN.md requires a composite frontier
   `(summarized_up_to_created_at, summarized_up_to_message_id)` for the strict total order
   `(created_at ASC, id ASC)`. A migration is needed to add the `created_at` component.

2. **`ThreadSummaryHandler`** — P1 stub at `src/infra/workers/thread_summary_worker.rs`
   that returns `Retry` for all messages. Registered in `gear.rs` on the
   `mini-chat.thread_summary` queue with decoupled strategy.

3. **`ThreadSummaryRepository`** — trait at `src/domain/repos/thread_summary_repo.rs` with
   `get_latest()` method; infra impl at `src/infra/db/repo/thread_summary_repo.rs` always
   returns `Ok(None)`.

4. **`enqueue_thread_summary_task`** — already implemented on `InfraOutboxEnqueuer`
   (`src/infra/outbox.rs:88-120`), partitioned by `chat_id`, marked `#[allow(dead_code)]`.

5. **`is_compressed` column** — exists on `messages` table (always `false`), entity has
   the field at `src/infra/db/entity/message.rs:33`.

6. **`LlmProvider::complete()`** — non-streaming Responses API method already available on
   the `LlmProvider` trait (`src/infra/llm/mod.rs:420-427`).

7. **Context assembly** — `gather_context()` in `stream_service/mod.rs` already fetches
   `thread_summary_repo.get_latest()` and passes it to `assemble_context()`.

## Architecture Decisions

- **Outbox-driven, no gear-local polling** — per DESIGN.md MUST. Uses the shared
  transactional outbox. No leader election, no second worker state machine.

- **System task isolation** — thread summary is a `requester_type=system` task. It MUST NOT
  create `chat_turns` rows, MUST NOT debit per-user quota. Usage events carry
  `requester_type=system` and `system_task_type="thread_summary_update"` for tenant-level
  billing attribution.

- **Trigger in finalization path** — the trigger evaluates the assembled request's
  `reserve_tokens` (already computed during preflight) against the compression threshold
  (`80% of token_budget`). If exceeded, a durable outbox message is enqueued in the same
  transaction as the turn's CAS finalization. This ensures atomicity per DESIGN.md.

- **Frozen range identity** — the outbox payload carries `base_frontier` and
  `frozen_target_frontier` as `(created_at, message_id)` pairs. The handler loads exactly
  the messages in that range and does not recompute the target at execution time.

- **CAS-protected commit** — the handler's atomic commit advances the frontier only if
  it still equals `base_frontier`. At most one handler wins per frozen range.

- **Non-streaming LLM call** — uses `LlmProvider::complete()` (Responses API non-streaming)
  per DESIGN.md section 3.5: "Thread summary generation, doc summary" uses
  `POST /outbound/llm/responses`.

- **`OutboxEnqueuer` trait extension** — add `enqueue_thread_summary` to the domain trait
  (currently the method is only on `InfraOutboxEnqueuer` directly, not on the trait).

## Rust Guidelines Applied (M-* from Microsoft Pragmatic Rust Guidelines)

- **M-ERRORS-CANONICAL-STRUCTS**: summary errors use existing `DomainError` variants.
- **M-LOG-STRUCTURED**: structured tracing with named fields (`chat_id`, `tenant_id`,
  `base_frontier`, `frozen_target`, `system_request_id`).
- **M-PANIC-ON-BUG**: payload deserialization failure -> `Reject` (dead-letter).
  CAS losers log and return `Success` (idempotent skip).
- **M-CONCISE-NAMES**: `ThreadSummaryHandler`, `ThreadSummaryTaskPayload`.
- **M-SERVICES-CLONE**: handler holds `Arc<T>` deps, is `Send + Sync`.
- **M-STATIC-VERIFICATION**: clippy clean, no `#[allow]` without `reason`.
- **M-STRONG-TYPES**: `SummaryFrontier` struct instead of loose `(OffsetDateTime, Uuid)` tuples.

## Phases

| Phase | File | Summary |
|-------|------|---------|
| 1 | [phase-1-migration-schema.md](phase-1-migration-schema.md) | Migrate `thread_summaries` schema to composite frontier; add SeaORM entity |
| 2 | [phase-2-domain-types.md](phase-2-domain-types.md) | Domain types: `ThreadSummaryTaskPayload`, `SummaryFrontier`, outbox enqueuer trait extension |
| 3 | [phase-3-repo-layer.md](phase-3-repo-layer.md) | Repository methods: `get_latest` (real impl), `upsert_with_cas`, `mark_compressed` |
| 4 | [phase-4-trigger.md](phase-4-trigger.md) | Trigger evaluation in finalization path + outbox enqueue in CAS transaction |
| 5 | [phase-5-handler.md](phase-5-handler.md) | Replace stub handler: load range, call LLM, CAS commit, emit usage event |
| 6 | [phase-6-observability.md](phase-6-observability.md) | Prometheus metrics per DESIGN.md |
| 7 | [phase-7-tests.md](phase-7-tests.md) | Unit + integration tests |

## Dependency Graph

```
Phase 1 ──┬──► Phase 3 (repo uses new entity columns)
           │
           └──► Phase 2 ──► Phase 4 (trigger uses domain types + enqueuer)
                   │
                   └──────► Phase 5 (handler uses domain types + repo + LLM)
                               │
Phase 6 ◄────────────────────┘ (metrics wired into trigger + handler)
Phase 7 depends on all above
```

Phases 2 and 3 can be developed in parallel after Phase 1.
Phases 4 and 5 can be developed in parallel after Phases 2 and 3.
