# Phase 6: Tests

## Goal

Comprehensive test coverage for the cleanup worker across unit and integration layers.

## Test Categories

### 6.1 Unit Tests — Repository Methods (Phase 1)

File: `src/infra/db/repo/attachment_repo.rs` (test gear)

| Test | Asserts |
|------|---------|
| `find_pending_cleanup_by_chat_returns_only_pending` | Filters out `done`, `failed`, NULL cleanup_status |
| `find_pending_cleanup_by_chat_scoped_to_tenant` | Different tenant's attachments not returned |
| `mark_cleanup_done_transitions_pending` | `cleanup_status` = `done`, `cleanup_updated_at` set |
| `mark_cleanup_done_idempotent_on_terminal` | Returns 0 rows affected if already `done` or `failed` |
| `record_cleanup_attempt_increments_counter` | `cleanup_attempts` incremented, error recorded |
| `record_cleanup_attempt_transitions_to_failed` | At max_attempts threshold, status becomes `failed` |
| `mark_attachments_pending_for_chat_bulk` | All matching attachments get `pending`, others untouched |
| `mark_attachments_pending_skips_already_set` | Attachments with existing cleanup_status unchanged |

File: `src/infra/db/repo/vector_store_repo.rs` (test gear)

| Test | Asserts |
|------|---------|
| `find_by_chat_system_no_scope` | Returns row without AccessScope |
| `delete_system_hard_deletes` | Row removed from table |

### 6.2 Unit Tests — AttachmentCleanupHandler (Phase 3)

File: `src/infra/workers/cleanup_worker.rs` (test gear)

| Test | Asserts |
|------|---------|
| `invalid_payload_rejects` | Malformed JSON → `Reject` |
| `success_marks_done` | Provider delete OK → `mark_cleanup_done` called → `Success` |
| `provider_error_retries` | Provider 5xx → `record_cleanup_attempt` → `Retry` |
| `max_attempts_rejects` | `record_cleanup_attempt` returns `failed` → `Reject` |
| `no_provider_file_id_succeeds` | `provider_file_id = None` → `Success` (nothing to delete) |
| `chat_soft_deleted_skips` | Parent chat deleted → appropriate handling |
| `cancel_returns_retry` | CancellationToken fired → `Retry` |

### 6.3 Unit Tests — ChatCleanupHandler (Phase 4)

File: `src/infra/workers/chat_cleanup_worker.rs` (or same file, test gear)

| Test | Asserts |
|------|---------|
| `invalid_payload_rejects` | Malformed JSON → `Reject` |
| `active_chat_rejects` | `chats.deleted_at IS NULL` → `Reject` |
| `no_attachments_no_vs_succeeds` | Empty chat → `Success` |
| `all_attachments_done_no_vs_succeeds` | All files deleted, no VS → `Success` |
| `all_attachments_done_vs_deleted` | Files done, VS deleted, row removed → `Success` |
| `partial_failure_retries` | Some files fail → `Retry` |
| `all_failed_vs_deleted_with_metric` | All files failed, VS still deleted, metric emitted |
| `vs_delete_failure_retries` | VS provider error → `Retry`, VS row preserved |
| `idempotent_rerun` | Second invocation with all terminal → `Success` |
| `cancel_mid_loop_retries` | Token fired during attachment loop → `Retry` |
| `vs_not_deleted_while_pending` | Pending attachments → VS not touched |

### 6.4 Unit Tests — Chat Deletion Service (Phase 2)

File: `src/domain/service/chat_service_test.rs`

| Test | Asserts |
|------|---------|
| `delete_chat_enqueues_cleanup_event` | `ChatCleanupEvent` recorded in `RecordingOutboxEnqueuer` |
| `delete_chat_marks_attachments_pending` | Attachments get `cleanup_status = 'pending'` |
| `delete_chat_event_has_stable_request_id` | `system_request_id` is a valid UUID |
| `delete_chat_no_attachments_still_enqueues` | Event enqueued even for empty chat |

### 6.5 Integration Tests

File: `tests/cleanup_integration.rs` (or appropriate integration test location)

| Test | Asserts |
|------|---------|
| `end_to_end_attachment_delete_cleanup` | Delete attachment → outbox delivers → handler processes → status `done` |
| `end_to_end_chat_delete_cleanup` | Delete chat → attachments marked pending → handler processes all → VS deleted → row removed |
| `crash_recovery_resumes` | Handler processes partial batch, restart, resumes remaining |

Integration tests may need:
- In-memory SQLite or test Postgres
- Mock OAGW (or `RagHttpClient` with test double)
- Outbox running in test mode

### 6.6 Test Infrastructure

- **Mock `RagHttpClient`**: return configurable success/failure per call. Can use the existing `FileStorageError` variants.
- **Mock repos**: extend existing `RecordingOutboxEnqueuer` pattern. Or use the real repos with in-memory DB.
- **Outbox test harness**: if `toolkit_db::outbox` supports synchronous/test-mode delivery, use it. Otherwise, test handlers directly by calling `handle()` with constructed `OutboxMessage`.

## Acceptance Criteria

- [ ] All unit tests pass
- [ ] Integration tests cover the happy path and crash recovery
- [ ] No `#[ignore]` tests without documented reason
- [ ] Test coverage for every `HandlerResult` variant in both handlers
- [ ] Test coverage for every cleanup_status transition
