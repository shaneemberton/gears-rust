# Phase 4: Chat-Level Cleanup Handler

## Goal

Implement `ChatCleanupHandler` that processes a chat-scoped cleanup event: iterates all pending attachments, deletes provider files, then deletes the vector store.

## Design Requirements (from DESIGN.md)

### Attachment cleanup loop
- Load all attachments with `cleanup_status = 'pending'` for the chat.
- For each: call provider DELETE via OAGW → update `cleanup_status`.
- 404 from provider = success.
- On retryable failure: increment attempts, record error, keep `pending`.
- On `attempts >= max_attempts`: set `failed` (terminal).

### Vector store cleanup ordering
- Vector store is delete-eligible only when ALL attachment rows are terminal (`done` or `failed`).
- MUST NOT delete vector store while any attachment is `pending`.
- If delete-eligible with any `failed` attachments: emit `mini_chat_cleanup_vector_store_with_failed_attachments_total` metric, proceed with deletion.
- On successful delete or 404: hard-delete `chat_vector_stores` row (durable completion marker).
- On retryable failure: leave row, return `Retry`.

### Idempotency
- Handler may be invoked multiple times for the same event (outbox redelivery).
- Must be idempotent: skip already-terminal attachments, skip already-deleted vector stores.

### Guard
- Active chats (`chats.deleted_at IS NULL`) MUST NOT be processed.

## Tasks

### 4.1 Create `ChatCleanupHandler` struct

File: `src/infra/workers/cleanup_worker.rs` (or a new `chat_cleanup_worker.rs`)

```rust
pub struct ChatCleanupHandler {
    rag_client: Arc<RagHttpClient>,
    attachment_repo: Arc<dyn AttachmentRepository>,
    vector_store_repo: Arc<dyn VectorStoreRepository>,
    chat_repo: Arc<dyn ChatRepository>,         // to verify chat is deleted
    db: Arc<dyn DatabaseProvider>,
    max_attempts: u32,
    oagw_route_prefix: String,
}
```

### 4.2 Implement handle()

```rust
async fn handle(&self, msg: &OutboxMessage, cancel: CancellationToken) -> HandlerResult {
    // 1. Deserialize ChatCleanupEvent (no tenant_id — chat_id is globally unique)
    let event: ChatCleanupEvent = match serde_json::from_slice(&msg.payload) {
        Ok(e) => e,
        Err(e) => return HandlerResult::Reject { reason: ... },
    };

    // 2. Guard: verify chat is actually soft-deleted (query by chat_id, no scope)
    //    If chats.deleted_at IS NULL → Reject (programming error or race)

    // 3. Load pending attachments
    let pending = attachment_repo.find_pending_cleanup_by_chat(
        &conn, event.chat_id
    ).await?;

    // 4. Process each attachment
    let mut any_retry = false;
    for att in &pending {
        if cancel.is_cancelled() {
            return HandlerResult::Retry { reason: "shutdown".into() };
        }

        match self.delete_provider_file(att).await {
            Ok(()) => {
                attachment_repo.mark_cleanup_done(&conn, att.id).await?;
            }
            Err(e) => {
                let status = attachment_repo.record_cleanup_attempt(
                    &conn, att.id, &e.to_string(), self.max_attempts
                ).await?;
                if status == "pending" {
                    any_retry = true;
                }
                // if "failed" → terminal, continue to next attachment
            }
        }
    }

    // 5. If any attachment is still pending → Retry (come back later)
    if any_retry {
        return HandlerResult::Retry {
            reason: "some attachments still pending".into(),
        };
    }

    // 6. Vector store cleanup
    //    Load chat_vector_stores row. If no row → all done.
    let vs = vector_store_repo.find_by_chat_system(&conn, event.chat_id).await?;
    if let Some(vs_row) = vs {
        // Re-check: all attachments must be terminal (no pending left)
        let still_pending = attachment_repo.find_pending_cleanup_by_chat(
            &conn, event.chat_id
        ).await?;
        if !still_pending.is_empty() {
            return HandlerResult::Retry { reason: "attachments still pending" };
        }

        // Check for failed attachments → emit metric if any
        // (query count of cleanup_status = 'failed' for this chat)

        // Delete vector store via OAGW
        if let Some(vs_id) = &vs_row.vector_store_id {
            match self.delete_vector_store(&event, vs_id).await {
                Ok(()) => {}
                Err(e) => {
                    return HandlerResult::Retry {
                        reason: format!("vector store delete failed: {e}"),
                    };
                }
            }
        }

        // Hard-delete chat_vector_stores row (durable completion marker)
        vector_store_repo.delete_system(&conn, vs_row.id).await?;
    }

    // 7. All done
    HandlerResult::Success
}
```

### 4.3 Provider delete helpers

```rust
impl ChatCleanupHandler {
    async fn delete_provider_file(
        &self,
        att: &AttachmentModel,
    ) -> Result<(), FileStorageError> {
        if let Some(ref file_id) = att.provider_file_id {
            let uri = format!("{}/files/{}", self.oagw_route_prefix, file_id);
            // System-level SecurityContext from client credentials (no tenant/user session).
            let ctx = self.system_security_context();
            self.rag_client.delete(ctx, &uri).await
        } else {
            // No provider file to delete — treat as success
            Ok(())
        }
    }

    async fn delete_vector_store(
        &self,
        vector_store_id: &str,
    ) -> Result<(), FileStorageError> {
        let uri = format!("{}/vector_stores/{}", self.oagw_route_prefix, vector_store_id);
        let ctx = self.system_security_context();
        self.rag_client.delete(ctx, &uri).await
    }
}
```

### 4.4 Register in gear.rs

```rust
.queue(&chat_cleanup_queue_name, partitions)
.decoupled(ChatCleanupHandler::new(
    rag_client.clone(),
    attachment_repo.clone(),
    vector_store_repo.clone(),
    chat_repo.clone(),
    db.clone(),
    config.cleanup_worker.max_attempts,
    oagw_route_prefix.clone(),
))
```

### 4.5 Error handling strategy

| Scenario | Action |
|----------|--------|
| Payload deserialization fails | `Reject` (dead-letter) |
| Chat not soft-deleted | `Reject` (guard violation) |
| Provider 5xx / timeout | `record_cleanup_attempt`, `Retry` |
| Provider 4xx (not 404) | `record_cleanup_attempt`, may become `failed` |
| DB error | `Retry` (transient infra) |
| All attachments terminal, VS deleted | `Success` |
| Some attachments still `pending` after batch | `Retry` |
| Cancellation token fired | `Retry` (graceful shutdown) |

## Acceptance Criteria

- [ ] Chat guard: rejects event if chat is not soft-deleted
- [ ] Processes all pending attachments, marks each done/failed
- [ ] Respects max_attempts per attachment
- [ ] Vector store deleted only after all attachments are terminal
- [ ] Metric emitted when vector store deleted with failed attachments
- [ ] chat_vector_stores row hard-deleted on VS cleanup success
- [ ] Idempotent: safe to re-run on same event
- [ ] Cancellation-aware within the attachment loop
- [ ] Structured tracing for every state transition
