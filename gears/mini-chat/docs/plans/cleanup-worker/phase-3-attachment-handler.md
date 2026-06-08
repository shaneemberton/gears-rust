# Phase 3: Per-Attachment Cleanup Handler

## Goal

Implement `AttachmentCleanupHandler` to actually delete a single provider file when an attachment is deleted via the attachment-deletion API path.

## Current State

- Stub returns `HandlerResult::Retry` for every message.
- `AttachmentCleanupEvent` already exists with `provider_file_id`, `storage_backend`, `attachment_kind`.
- `RagHttpClient.delete()` is best-effort (404 = success, no status check).
- Handler is registered in `gear.rs` as `.decoupled(AttachmentCleanupHandler)`.

## Design Constraints (from DESIGN.md)

- 404 from provider = success (idempotency).
- On success: `cleanup_status = 'done'`, `cleanup_updated_at = now()`.
- On retryable failure: increment `cleanup_attempts`, record error, keep `pending`, return `Retry`.
- On `cleanup_attempts >= max_attempts`: `cleanup_status = 'failed'`, return `Reject`.
- Attachment-level cleanup MUST NOT process attachments whose parent chat is soft-deleted (ownership transfers to chat-deletion path).

## Tasks

### 3.1 Add dependencies to handler struct

File: `src/infra/workers/cleanup_worker.rs`

```rust
pub struct AttachmentCleanupHandler {
    rag_client: Arc<RagHttpClient>,
    attachment_repo: Arc<dyn AttachmentRepository>,
    db: Arc<dyn DatabaseProvider>,       // for DB connection
    max_attempts: u32,                    // from CleanupWorkerConfig
    oagw_route_prefix: String,           // e.g., "/outbound/llm" — to build DELETE URI
}
```

### 3.2 Implement handle()

```rust
async fn handle(&self, msg: &OutboxMessage, cancel: CancellationToken) -> HandlerResult {
    // 1. Deserialize payload
    let event: AttachmentCleanupEvent = match serde_json::from_slice(&msg.payload) {
        Ok(e) => e,
        Err(e) => return HandlerResult::Reject {
            reason: format!("invalid payload: {e}"),
        },
    };

    // 2. Guard: skip if parent chat is soft-deleted (ownership transferred to chat-deletion path)
    //    Check chats.deleted_at via a lightweight query.
    //    If chat is deleted → Ok (ack as no-op; chat-deletion handler owns cleanup).

    // 3. Build DELETE URI from event fields
    //    URI pattern depends on storage_backend + attachment_kind:
    //    - provider_file_id → DELETE {oagw_route_prefix}/files/{provider_file_id}

    // 4. Call RagHttpClient.delete() with system SecurityContext
    //    RagHttpClient.delete() is already idempotent (404 = success).

    // 5. On success → mark_cleanup_done(attachment_id)
    //    Return HandlerResult::Success

    // 6. On error → record_cleanup_attempt(attachment_id, error, max_attempts)
    //    If returned status = "failed" → HandlerResult::Reject
    //    If returned status = "pending" → HandlerResult::Retry
}
```

### 3.3 System SecurityContext

The handler runs in a background worker — no user session. Need a way to construct a system-level `SecurityContext` for OAGW calls.

Options:
- Check if `SecurityContext` has a `system()` or `service()` constructor.
- Or construct from `tenant_id` in the event payload with a service-account identity.

Investigate how `UsageEventHandler` or other background code obtains credentials for outbound calls. The OAGW `proxy_request` may need specific auth headers.

### 3.4 Wire dependencies in gear.rs

File: `src/gear.rs`

Change from:
```rust
.decoupled(crate::infra::workers::cleanup_worker::AttachmentCleanupHandler)
```

To:
```rust
.decoupled(AttachmentCleanupHandler::new(
    rag_client.clone(),
    attachment_repo.clone(),
    db.clone(),
    config.cleanup_worker.max_attempts,
    oagw_route_prefix.clone(),
))
```

### 3.5 Handle cancellation

Check `cancel.is_cancelled()` before the provider call. If cancelled, return `Retry` to allow graceful shutdown without losing work.

## Open Questions

1. **System SecurityContext construction** — how does a background worker authenticate to OAGW? This may require investigation of existing patterns (e.g., service tokens, tenant-scoped system contexts).
2. **OAGW route prefix** — what's the exact DELETE URI pattern? Check existing file upload/delete code paths for the route structure.
3. **Chat soft-delete guard** — **Settled**: handler checks `chats.deleted_at` via `is_deleted_system()`. If parent chat is soft-deleted, returns `MessageResult::Ok` (no-op ack) — ownership transferred to chat-deletion handler. Implemented in `cleanup_worker.rs:124-131`.

## Acceptance Criteria

- [ ] Handler deserializes `AttachmentCleanupEvent` and calls provider DELETE
- [ ] Success → `cleanup_status = 'done'`, returns `Success`
- [ ] Retryable error → increments attempts, returns `Retry`
- [ ] Max attempts exceeded → `cleanup_status = 'failed'`, returns `Reject`
- [ ] Invalid payload → `Reject` (dead-letter)
- [ ] Parent chat soft-deleted → ack as no-op success (ownership transferred to chat-deletion handler)
- [ ] Cancellation token respected
- [ ] Structured tracing for each outcome
