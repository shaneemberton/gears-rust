# Phase 2: Chat Cleanup Event & Enqueue

## Goal

Define the `ChatCleanupEvent`, add it to the outbox enqueuer, register a new outbox queue, and update `delete_chat` to atomically soft-delete + mark attachments pending + enqueue the cleanup event.

## Current State

- `delete_chat` in `chat_service.rs:247-267` only calls `chat_repo.soft_delete()`. No outbox event.
- `OutboxEnqueuer` trait has no `enqueue_chat_cleanup` method.
- `OutboxConfig` has no `chat_cleanup_queue_name`.
- No `ChatCleanupEvent` type exists.

## Tasks

### 2.1 Define `ChatCleanupEvent` and `CleanupReason` enum

File: `src/domain/repos/outbox_enqueuer.rs`

Per DESIGN.md (line 1758), the payload must contain `tenant_id`, `chat_id`, `system_request_id`, `reason`, `chat_deleted_at`.

```rust
/// Why provider cleanup was triggered.
/// Enum ensures exhaustive match in the handler and prevents typo-driven bugs
/// (M-STRONG-TYPES).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CleanupReason {
    /// Chat was explicitly soft-deleted by the user.
    ChatSoftDelete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCleanupEvent {
    pub reason: CleanupReason,
    pub chat_id: Uuid,
    pub system_request_id: Uuid,         // Stable UUID v4, persisted at enqueue, reused across retries
    pub chat_deleted_at: OffsetDateTime,
}
```

**`tenant_id` is included** per DESIGN.md line 1758 (MUST). Workers don't use it for DB queries (chat_id is globally unique), but it's required for structured logging and traceability.

**Why no `event_type: String`?** The existing `AttachmentCleanupEvent` uses a `String` `event_type` field. For the new event, `CleanupReason` enum replaces this — stronger typing catches mismatches at compile time (M-STRONG-TYPES). Consider also migrating `AttachmentCleanupEvent.event_type` to an enum in a follow-up.

### 2.2 Add `enqueue_chat_cleanup` to `OutboxEnqueuer` trait

File: `src/domain/repos/outbox_enqueuer.rs`

```rust
async fn enqueue_chat_cleanup(
    &self,
    runner: &(dyn DBRunner + Sync),
    event: ChatCleanupEvent,
) -> Result<(), DomainError>;
```

### 2.3 Add queue name to config

File: `src/config.rs` — `OutboxConfig` struct

```rust
/// Queue name for chat-deletion cleanup events.
#[serde(default = "default_chat_cleanup_queue_name")]
pub chat_cleanup_queue_name: String,
```

Default: `"mini-chat.chat_cleanup"`.

Update `validate()` to check non-empty. Update `Default` impl.

### 2.4 Implement `enqueue_chat_cleanup` in infra

File: `src/infra/outbox.rs` — `InfraOutboxEnqueuer`

- Store `chat_cleanup_queue_name` field.
- Partition by `chat_id` — per design: "queue SHOULD partition by chat_id so that all cleanup messages for the same chat are assigned to the same partition and processed sequentially". Different chats clean up in parallel across partitions.
- Reuse the same hash-modulo approach as `partition_for()` but keyed on `chat_id`.

```rust
async fn enqueue_chat_cleanup(
    &self,
    runner: &(dyn DBRunner + Sync),
    event: ChatCleanupEvent,
) -> Result<(), DomainError> {
    let partition = self.partition_for(event.chat_id); // hash chat_id → partition
    let payload = serde_json::to_vec(&event)...;
    self.outbox.enqueue(runner, &self.chat_cleanup_queue_name, partition, payload, "application/json").await...;
    Ok(())
}
```

### 2.5 Update `delete_chat` service method

File: `src/domain/service/chat_service.rs`

The current flow:
1. Resolve access scope
2. `chat_repo.soft_delete()`

New flow (single transaction):
1. Resolve access scope
2. Start transaction
3. `chat_repo.soft_delete()` — sets `chats.deleted_at`
4. `attachment_repo.mark_attachments_pending_for_chat()` — sets `cleanup_status = 'pending'` on all chat attachments (from Phase 1)
5. `outbox_enqueuer.enqueue_chat_cleanup()` — enqueues `ChatCleanupEvent`
6. Commit transaction
7. `outbox_enqueuer.flush()` — notify outbox sequencer

The `system_request_id` is `Uuid::new_v4()` generated once at enqueue time.

**Important**: The service currently uses `self.db.conn()` (not a transaction). This must change to `self.db.transaction()` to ensure atomicity of all three operations.

### 2.6 Register chat cleanup queue in gear.rs

File: `src/gear.rs` (around line 205)

Add a new queue registration in the outbox builder:

```rust
.queue(&chat_cleanup_queue_name, partitions)
.decoupled(ChatCleanupHandler::new(/* deps */))
```

For now (before Phase 4), register with a stub handler that returns `Retry`, similar to current `AttachmentCleanupHandler`.

### 2.7 Update test helpers

File: `src/domain/service/test_helpers.rs`

- Add `chat_cleanup_events: Mutex<Vec<ChatCleanupEvent>>` to `RecordingOutboxEnqueuer`
- Implement `enqueue_chat_cleanup` to record events

## Acceptance Criteria

- [ ] `ChatCleanupEvent` serializes/deserializes correctly
- [ ] `delete_chat` atomically: soft-deletes chat, marks attachments pending, enqueues event
- [ ] Chat cleanup queue is registered in gear startup
- [ ] Existing `delete_chat` tests updated to verify event enqueue
- [ ] New test: `delete_chat` on chat with attachments → attachments get `cleanup_status = 'pending'`
- [ ] New test: `delete_chat` on chat with no attachments → event still enqueued (handler handles empty attachment list gracefully)
