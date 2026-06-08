# Cleanup Worker Implementation Plan

Implements `cpt-cf-mini-chat-fr-chat-deletion-cleanup` (P1) from DESIGN.md.

## Context

Two distinct cleanup paths exist per design:

1. **Per-attachment cleanup** — triggered by `DELETE /v1/chats/{chat_id}/attachments/{id}`.
   Already enqueues `AttachmentCleanupEvent` into `mini-chat.attachment_cleanup` queue.
   Handler (`AttachmentCleanupHandler`) is a P1 stub returning `Retry`.

2. **Chat-deletion cleanup** — triggered by `DELETE /v1/chats/{id}`.
   Currently only soft-deletes the chat row. Does NOT mark attachments as `pending`,
   does NOT enqueue any cleanup event, and no `ChatCleanupEvent` type exists yet.

Both paths converge on the same attachment cleanup state machine (`pending` → `done` | `failed`)
and the vector-store ordering invariant (all attachments terminal before vector-store delete).

## Architecture Decisions

- **Outbox-driven, no gear-local polling** — per design MUST.
- **Per-attachment handler** deletes a single provider file (simple, one event = one file).
- **Chat-level handler** receives a chat-scoped event, iterates pending attachments, then deletes vector store.
- **Two separate outbox queues** — attachment cleanup partitioned by `tenant_id`, chat cleanup partitioned by `chat_id`.
- **RagHttpClient.delete()** is already best-effort (no status check, 404 = success). Fits the idempotency requirement.
- **SecurityContext for OAGW calls** — handler needs a way to construct/obtain a system-level security context for provider calls (no user session in background workers).

## Rust Guidelines Applied (M-* from Microsoft Pragmatic Rust Guidelines)

- **M-ERRORS-CANONICAL-STRUCTS**: cleanup errors use `FileStorageError` variants, mapped to `HandlerResult`.
- **M-LOG-STRUCTURED**: structured tracing with named fields (tenant_id, chat_id, attachment_id, cleanup_status).
- **M-PANIC-ON-BUG**: payload deserialization failure → `Reject` (dead-letter), not panic.
- **M-CONCISE-NAMES**: `ChatCleanupHandler`, not `ChatCleanupManagerService`.
- **M-SERVICES-CLONE**: handler structs hold `Arc<T>` deps, are `Send + Sync`.
- **M-STATIC-VERIFICATION**: clippy clean, no `#[allow]` without `reason`.

## Phases

| Phase | File | Summary |
|-------|------|---------|
| 1 | [phase-1-repo-layer.md](phase-1-repo-layer.md) | Attachment & vector-store repo methods for cleanup queries/updates |
| 2 | [phase-2-chat-cleanup-event.md](phase-2-chat-cleanup-event.md) | `ChatCleanupEvent`, outbox enqueuer, `delete_chat` transaction |
| 3 | [phase-3-attachment-handler.md](phase-3-attachment-handler.md) | Implement `AttachmentCleanupHandler` (per-attachment file delete) |
| 4 | [phase-4-chat-handler.md](phase-4-chat-handler.md) | Implement `ChatCleanupHandler` (chat-scoped batch + vector store) |
| 5 | [phase-5-observability.md](phase-5-observability.md) | Prometheus metrics per design |
| 6 | [phase-6-tests.md](phase-6-tests.md) | Unit + integration tests |

## Dependency Graph

```
Phase 1 ──┬──► Phase 3 (uses repo methods)
           │
           └──► Phase 2 ──► Phase 4 (uses event + repo methods)
                               │
Phase 5 ◄─────────────────────┘ (metrics wired into handlers)
Phase 6 depends on all above
```

Phases 2 and 3 can be developed in parallel after Phase 1.
