# Phase 5: Handler — Replace Stub with Real Summary Logic

## Goal

Replace the `ThreadSummaryHandler` stub with real logic: deserialize payload, load messages
in the frozen range, call the LLM via Responses API (non-streaming), and CAS-commit the
new summary with frontier advance and `is_compressed` marking.

## Current State

- Stub at `src/infra/workers/thread_summary_worker.rs` returns `HandlerResult::Retry`.
- `ThreadSummaryTaskPayload` (Phase 2) defines the deserialized outbox payload.
- Repository methods (Phase 3): `get_latest`, `upsert_with_cas`, `mark_messages_compressed`,
  `fetch_messages_in_range`.
- `LlmProvider::complete()` at `src/infra/llm/mod.rs:420-427` sends non-streaming requests.
- `SecurityContext` can be constructed for system tasks (pattern from `ChatCleanupHandler`
  which uses `DEFAULT_SUBJECT_ID`).
- Config: `ThreadSummaryWorkerConfig` has `enabled`, `reconcile_interval_secs`, etc.

## Design Constraints

From DESIGN.md:
- Handler MUST bind to the frozen target frontier from the outbox payload.
- Handler MUST load non-deleted, non-compressed messages in `(base_frontier, frozen_target]`.
- LLM call uses `POST /outbound/llm/responses` (non-streaming Responses API).
- If LLM succeeds: atomic CAS commit (save summary, advance frontier, mark compressed).
- CAS MUST succeed only if stored frontier still equals `base_frontier`.
- If CAS fails (frontier already advanced): finish without another commit (`Success`).
- If LLM fails: keep previous summary unchanged, don't advance frontier, return `Retry`.
- System task: `requester_type=system`, no `chat_turns` row, no user quota debit.
- Usage event MUST carry `requester_type=system`, `system_task_type="thread_summary_update"`.
- `system_request_id` from payload reused across retries (not regenerated).

## Tasks

### 5.1 Define `ThreadSummaryDeps` struct

File: `src/infra/workers/thread_summary_worker.rs`

```rust
/// Dependencies for the thread summary outbox handler.
pub struct ThreadSummaryDeps {
    pub db: Arc<DbProvider>,
    pub thread_summary_repo: Arc<dyn ThreadSummaryRepository>,
    pub message_repo: Arc<dyn MessageRepository>,
    pub llm_provider: Arc<dyn LlmProvider>,
    pub outbox_enqueuer: Arc<dyn OutboxEnqueuer>,
    pub metrics: Arc<dyn MiniChatMetricsPort>,
    pub upstream_alias: String,
    pub summary_model: String,  // model to use for summarization (from config)
    pub summary_system_prompt: String,  // system instructions for summary generation
}
```

### 5.2 Replace `ThreadSummaryHandler` struct

File: `src/infra/workers/thread_summary_worker.rs`

```rust
pub struct ThreadSummaryHandler {
    deps: Arc<ThreadSummaryDeps>,
}

impl ThreadSummaryHandler {
    pub fn new(deps: Arc<ThreadSummaryDeps>) -> Self {
        Self { deps }
    }
}
```

### 5.3 Implement `MessageHandler` for `ThreadSummaryHandler`

File: `src/infra/workers/thread_summary_worker.rs`

```rust
#[async_trait]
impl MessageHandler for ThreadSummaryHandler {
    async fn handle(&self, msg: &OutboxMessage, cancel: CancellationToken) -> HandlerResult {
        // 1. Deserialize payload
        let payload: ThreadSummaryTaskPayload = match serde_json::from_slice(&msg.payload) {
            Ok(p) => p,
            Err(e) => {
                error!(
                    partition_id = msg.partition_id,
                    seq = msg.seq,
                    error = %e,
                    "thread summary: invalid payload, dead-lettering"
                );
                return HandlerResult::Reject {
                    reason: format!("payload deserialization failed: {e}"),
                };
            }
        };

        let span_fields = /* structured: chat_id, tenant_id, system_request_id */;

        // 2. Pre-check: verify stored frontier still matches base_frontier
        //    (early CAS guard before expensive LLM call)
        let conn = match self.deps.db.conn() {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "thread summary: DB connection failed");
                return HandlerResult::Retry {
                    reason: format!("db connection: {e}"),
                };
            }
        };

        let base_frontier = match (&payload.base_frontier_created_at, &payload.base_frontier_message_id) {
            (Some(ca), Some(mid)) => Some(SummaryFrontier { created_at: *ca, message_id: *mid }),
            _ => None,
        };

        let current = self.deps.thread_summary_repo
            .get_latest(&conn, &AccessScope::for_tenant(payload.tenant_id), payload.chat_id)
            .await;

        match &current {
            Ok(Some(existing)) => {
                if base_frontier.as_ref() != Some(&existing.frontier) {
                    // Frontier already advanced — another handler won. Idempotent skip.
                    info!("thread summary: frontier already advanced, skipping");
                    self.deps.metrics.record_thread_summary_cas_conflict();
                    return HandlerResult::Success;
                }
            }
            Ok(None) => {
                if base_frontier.is_some() {
                    // Expected existing frontier but none exists — should not happen.
                    // Could be a race with chat deletion (CASCADE). Skip gracefully.
                    warn!("thread summary: expected frontier but none found, skipping");
                    return HandlerResult::Success;
                }
            }
            Err(e) => {
                warn!(error = %e, "thread summary: pre-check query failed");
                return HandlerResult::Retry {
                    reason: format!("pre-check query: {e}"),
                };
            }
        }

        let target_frontier = SummaryFrontier {
            created_at: payload.frozen_target_created_at,
            message_id: payload.frozen_target_message_id,
        };

        // 3. Load messages in range
        let messages = match self.deps.message_repo
            .fetch_messages_in_range(
                &conn,
                payload.chat_id,
                base_frontier.as_ref(),
                &target_frontier,
            )
            .await
        {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "thread summary: message fetch failed");
                return HandlerResult::Retry {
                    reason: format!("message fetch: {e}"),
                };
            }
        };

        if messages.is_empty() {
            // No messages to summarize (all deleted or already compressed).
            info!("thread summary: no messages in range, skipping");
            return HandlerResult::Success;
        }

        // 4. Build summarization prompt
        let existing_summary = current
            .as_ref()
            .ok()
            .and_then(|c| c.as_ref())
            .map(|s| s.content.as_str());

        let user_content = build_summary_prompt(existing_summary, &messages);

        // 5. Call LLM (non-streaming Responses API)
        let security_ctx = build_system_security_context(payload.tenant_id);

        let request = llm_request(&self.deps.summary_model)
            .system(&self.deps.summary_system_prompt)
            .user(&user_content)
            .build_non_streaming();

        let llm_result = self.deps.llm_provider
            .complete(security_ctx, request, &self.deps.upstream_alias)
            .await;

        let response = match llm_result {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "thread summary: LLM call failed");
                self.deps.metrics.record_thread_summary_execution("provider_error");
                self.deps.metrics.record_summary_fallback();
                return HandlerResult::Retry {
                    reason: format!("LLM provider error: {e}"),
                };
            }
        };

        if cancel.is_cancelled() {
            return HandlerResult::Retry {
                reason: "cancelled".to_owned(),
            };
        }

        let summary_text = response.output_text();
        let token_estimate = estimate_summary_tokens(&summary_text);
        let usage = response.usage();

        // 6. CAS-protected atomic commit
        let cas_result = self.deps.db.transaction(|tx| {
            let deps = Arc::clone(&self.deps);
            let payload = payload.clone();
            let base_frontier = base_frontier.clone();
            let target_frontier = target_frontier.clone();
            let summary_text = summary_text.clone();
            Box::pin(async move {
                // 6a. Upsert summary with CAS
                let rows = deps.thread_summary_repo
                    .upsert_with_cas(
                        tx,
                        payload.chat_id,
                        payload.tenant_id,
                        base_frontier.as_ref(),
                        &target_frontier,
                        &summary_text,
                        token_estimate,
                    )
                    .await?;

                if rows == 0 {
                    // CAS conflict — another handler won.
                    return Ok(false);
                }

                // 6b. Mark messages as compressed
                deps.message_repo
                    .mark_messages_compressed(
                        tx,
                        payload.chat_id,
                        base_frontier.as_ref(),
                        &target_frontier,
                    )
                    .await?;

                // 6c. Enqueue system usage event
                let usage_event = build_system_usage_event(
                    &payload,
                    usage,
                );
                deps.outbox_enqueuer
                    .enqueue_usage_event(tx, usage_event)
                    .await?;

                Ok(true)
            })
        }).await;

        match cas_result {
            Ok(true) => {
                self.deps.outbox_enqueuer.flush();
                self.deps.metrics.record_thread_summary_execution("success");
                info!(
                    chat_id = %payload.chat_id,
                    messages_compressed = messages.len(),
                    "thread summary: committed successfully"
                );
                HandlerResult::Success
            }
            Ok(false) => {
                self.deps.metrics.record_thread_summary_cas_conflict();
                info!("thread summary: CAS conflict on commit, skipping");
                HandlerResult::Success
            }
            Err(e) => {
                warn!(error = %e, "thread summary: commit failed");
                self.deps.metrics.record_thread_summary_execution("retry");
                HandlerResult::Retry {
                    reason: format!("commit failed: {e}"),
                }
            }
        }
    }
}
```

### 5.4 Build summarization prompt

File: `src/infra/workers/thread_summary_worker.rs`

```rust
/// Build the user message content for the summarization LLM call.
///
/// If an existing summary exists, it is included as context so the LLM can
/// produce an incremental update rather than starting from scratch.
fn build_summary_prompt(
    existing_summary: Option<&str>,
    messages: &[MessageModel],
) -> String {
    let mut prompt = String::new();

    if let Some(summary) = existing_summary {
        prompt.push_str("## Existing Summary\n\n");
        prompt.push_str(summary);
        prompt.push_str("\n\n## New Messages to Incorporate\n\n");
    } else {
        prompt.push_str("## Messages to Summarize\n\n");
    }

    for msg in messages {
        let role = match msg.role {
            MessageRole::User => "User",
            MessageRole::Assistant => "Assistant",
            MessageRole::System => "System",
        };
        prompt.push_str(&format!("**{role}**: {}\n\n", msg.content));
    }

    prompt.push_str(
        "Produce a concise summary that preserves key facts, decisions, \
         document references, and action items. If an existing summary was \
         provided, update it to incorporate the new messages."
    );

    prompt
}
```

### 5.5 Build system usage event

File: `src/infra/workers/thread_summary_worker.rs`

Per DESIGN.md section "System Task Attribution Rules":

```rust
fn build_system_usage_event(
    payload: &ThreadSummaryTaskPayload,
    usage: Option<Usage>,
) -> UsageEvent {
    let u = usage.unwrap_or_default();
    let dedupe_key = format!(
        "{}/{}/{}",
        payload.tenant_id.simple(),
        payload.system_task_type,
        payload.system_request_id.simple(),
    );

    UsageEvent {
        event_type: "usage_snapshot".to_owned(),
        dedupe_key,
        tenant_id: payload.tenant_id,
        user_id: None,  // system task — no user attribution
        requester_type: "system".to_owned(),
        chat_id: Some(payload.chat_id),
        request_id: payload.system_request_id,
        usage: UsageTokens {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            cache_read_input_tokens: u.cache_read_input_tokens,
            cache_write_input_tokens: u.cache_write_input_tokens,
            reasoning_tokens: u.reasoning_tokens,
        },
        outcome: "completed".to_owned(),
        settlement_method: "actual".to_owned(),
        // System tasks do not carry model/credit fields from preflight.
        // MiniChatManager attributes cost via tenant operational bucket.
        effective_model: None,
        actual_credits_micro: None,
    }
}
```

### 5.6 Build system `SecurityContext`

File: `src/infra/workers/thread_summary_worker.rs`

Same pattern as `ChatCleanupHandler` (cleanup_worker.rs line ~46):

```rust
fn build_system_security_context(tenant_id: Uuid) -> SecurityContext {
    SecurityContext::new(
        DEFAULT_SUBJECT_ID,
        tenant_id,
        /* additional claims as needed */
    )
}
```

### 5.7 Update handler registration in `gear.rs`

File: `src/gear.rs`

Replace the stub handler registration (line ~515-516) with the real handler:

```rust
let summary_deps = Arc::new(ThreadSummaryDeps {
    db: Arc::clone(&db),
    thread_summary_repo: Arc::clone(&thread_summary_repo),
    message_repo: Arc::clone(&message_repo),
    llm_provider: Arc::clone(&llm_provider),
    outbox_enqueuer: Arc::clone(&outbox_enqueuer),
    metrics: Arc::clone(&metrics),
    upstream_alias: config.upstream_alias.clone(),
    summary_model: config.thread_summary_worker.model.clone(),
    summary_system_prompt: config.thread_summary_worker.system_prompt.clone(),
});

// ...in outbox pipeline builder:
.queue(&od.outbox_config.thread_summary_queue_name, partitions)
.decoupled(ThreadSummaryHandler::new(summary_deps))
```

### 5.8 Add config fields for summary model and prompt

File: `src/config/background.rs`

Add to `ThreadSummaryWorkerConfig`:

```rust
/// Model to use for summary generation (e.g., "gpt-4o-mini").
/// If empty, uses the chat's configured model.
pub model: String,

/// System prompt for the summarization LLM call.
pub system_prompt: String,
```

Defaults: implementation-defined (e.g., `model: "gpt-4o-mini"`, `system_prompt: "You are a conversation summarizer..."`)

## Open Questions

- **Summary model**: Should the summary use the same model as the chat, or a cheaper model
  (e.g., gpt-4o-mini)? DESIGN.md doesn't specify. Recommend: configurable, default to a
  cost-effective model since summaries don't need the full capabilities of the chat model.

- **Summary system prompt**: The exact wording needs product input. The handler should use
  a configurable string. A reasonable default focuses on preserving facts, decisions,
  references, and action items.

- **`ResponseResult` shape**: Need to verify `LlmProvider::complete()` return type structure
  to extract `output_text()` and `usage()`. Adapt based on the actual `ResponseResult` type.

- **Token estimation for summary**: `estimate_summary_tokens(&summary_text)` could use
  `estimate_item_tokens(text_bytes, budgets)` from context_assembly, or a simpler
  `len() / 4` heuristic since this is stored for informational purposes.

## Acceptance Criteria

- [ ] Payload deserialization failure -> `Reject` (dead-letter)
- [ ] DB connection failure -> `Retry`
- [ ] Pre-check: frontier already advanced -> `Success` (idempotent skip)
- [ ] Empty message range -> `Success` (nothing to summarize)
- [ ] LLM call failure -> `Retry`, previous summary unchanged, metrics emitted
- [ ] LLM success + CAS win: summary saved, frontier advanced, messages compressed, usage enqueued
- [ ] LLM success + CAS loss: `Success` (idempotent skip), no summary written
- [ ] Commit failure -> `Retry`
- [ ] `outbox_enqueuer.flush()` called post-commit on CAS win
- [ ] Usage event carries `requester_type=system`, `dedupe_key` format per DESIGN.md
- [ ] `system_request_id` from payload — not regenerated
- [ ] Cancellation token checked after LLM call
- [ ] Structured logging with `chat_id`, `tenant_id`, `system_request_id` on all paths
- [ ] Stub handler and stub test removed
