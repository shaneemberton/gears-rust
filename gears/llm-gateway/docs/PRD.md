# PRD — LLM Gateway


<!-- toc -->

- [1. Overview](#1-overview)
  - [1.1 Purpose](#11-purpose)
  - [1.2 Background / Problem Statement](#12-background--problem-statement)
  - [1.3 Goals (Business Outcomes)](#13-goals-business-outcomes)
  - [1.4 Glossary](#14-glossary)
- [2. Actors](#2-actors)
  - [2.1 Human Actors](#21-human-actors)
  - [2.2 System Actors](#22-system-actors)
- [3. Operational Concept & Environment](#3-operational-concept--environment)
  - [3.1 Gear-Specific Environment Constraints](#31-gear-specific-environment-constraints)
  - [3.2 Data Ownership & Classification](#32-data-ownership--classification)
- [4. Scope](#4-scope)
  - [4.1 In Scope](#41-in-scope)
  - [4.2 Out of Scope](#42-out-of-scope)
- [5. Functional Requirements](#5-functional-requirements)
  - [P1 — Core](#p1--core)
  - [P2 — Reliability & Governance](#p2--reliability--governance)
  - [P3 — Additional Capabilities](#p3--additional-capabilities)
  - [P4 — Enterprise](#p4--enterprise)
- [6. Non-Functional Requirements](#6-non-functional-requirements)
  - [Scalability](#scalability)
  - [Data Retention](#data-retention)
  - [Compatibility](#compatibility)
  - [NFR Exclusions](#nfr-exclusions)
  - [Recovery](#recovery)
  - [Observability](#observability)
- [7. Public Library Interfaces](#7-public-library-interfaces)
- [8. Use Cases](#8-use-cases)
- [9. Acceptance Criteria](#9-acceptance-criteria)
- [10. Dependencies](#10-dependencies)
- [11. Assumptions](#11-assumptions)
- [12. Risks](#12-risks)
- [13. Open Questions](#13-open-questions)
- [14. Traceability](#14-traceability)

<!-- /toc -->

## 1. Overview

### 1.1 Purpose

LLM Gateway provides unified access to multiple LLM providers. Consumers interact with a single interface regardless of underlying provider. Gateway normalizes requests and responses but does not execute tools or interpret content — this is consumer responsibility.

LLM Gateway is the central integration point between platform consumers and external AI providers. It abstracts provider differences — request formats, authentication, error handling, rate limits — behind a unified API. Consumers send requests in a normalized format; Gateway translates them to provider-specific calls and normalizes responses back.

The Gateway supports diverse modalities: text generation, embeddings, vision, audio, video, and document processing. It handles both synchronous and asynchronous operations, including streaming responses and long-running jobs. All interactions go through the Outbound API Gateway for reliability and credential management.

Gateway is stateless for request processing — it does not store conversation history or execute tools, and consumers provide full context with each request. Gateway persists async and batch job state (internal-to-provider job ID mappings, job results) to support long-running operations that can take up to 24 hours. Gateway guarantees at-least-once delivery of usage records to the Usage Tracker gear.

**Target Users**:
- **Platform Developers** — build AI-powered features using Gateway API
- **External API Consumers** — third-party developers accessing AI capabilities via public API

### 1.2 Background / Problem Statement

Platform consumers need access to AI capabilities from multiple LLM providers — OpenAI, Anthropic, Google, and others. Each provider has its own API format, authentication mechanism, error semantics, and rate limiting behavior. Without a unified abstraction, every consumer must implement provider-specific integration logic, handle failover independently, and manage credentials directly. This fragments the codebase, increases the surface area for security issues, and makes it difficult to enforce consistent governance policies across the platform.

The current state requires each consuming gear to negotiate provider differences at the application layer: translating request formats, normalizing error responses, and implementing retry logic per provider. Usage tracking and budget enforcement are ad-hoc or absent, making it impossible to provide tenant-level cost visibility or enforce spending limits. Content moderation and PII filtering require each consumer to integrate interception logic independently, with no guarantee of consistent policy enforcement.

LLM Gateway addresses these problems by providing a single integration point that abstracts provider differences behind a unified API. It centralizes governance — budget enforcement, rate limiting, usage tracking, and audit logging — at the tenant level. Pre-call and post-response interceptors enable consistent content moderation and PII filtering policies without requiring consumer-side implementation.

### 1.3 Goals (Business Outcomes)

**Success Criteria** (required at GA, baseline: new gear — no prior data; measured over 7-day rolling window after initial 30-day burn-in period):
- Gateway overhead < 50ms P99 at up to 1 000 concurrent requests (excluding provider latency)
- Availability ≥ 99.9% measured monthly (Gateway infrastructure only — excludes provider outages; fallback-active mode counts as available)
- Expected steady-state throughput: up to 500 requests/second; peak: up to 2 000 requests/second
- Maintenance windows follow platform-defined schedule; Gateway supports rolling restarts with zero-downtime deployments

**Post-GA Growth Targets** (6 months after GA):
- Steady-state throughput: up to 1 000 requests/second; peak: up to 5 000 requests/second
- Maintain < 50ms P99 overhead at increased load
- Support up to 50 000 async/batch jobs/day per tenant

**Capacity Planning Inputs**:
- Expected async/batch job volume: up to 10 000 jobs/day per tenant at steady state
- DB row growth: up to 50 000 rows/day across all tenants (job records + usage delivery); bounded by retention cleanup
- Storage per async result: up to 1 MB (text responses); up to 100 MB (media generation results stored in FileStorage, not DB)

**Capabilities**:
- Text generation (chat completion)
- Multimodal input/output (images, audio, video, documents)
- Embeddings generation
- Tool/function calling
- Structured output with schema validation

### 1.4 Glossary

| Term | Definition |
|------|------------|
| OAGW | Outbound API Gateway — handles external API calls, credential injection, circuit breaking |
| TTFT | Time-to-first-token — latency until first response chunk |
| GTS | Generic Type System — JSON Schema-based type definitions |
| FileStorage | Platform gear for storing and retrieving binary files (images, audio, video, documents) |
| Model Registry | Platform gear that resolves model identifiers to provider and endpoint information per tenant |
| Hook Plugin | Gears plugin architecture extension point for per-request processing. Multiple plugins can be enabled per tenant and are invoked in order. Pre-call plugins run before the provider adapter and can modify or block requests. Post-call plugins run after the full response is available and are observe-only — the response has already been delivered or is delivered unconditionally, so there is nothing to modify or block. |

## 2. Actors

### 2.1 Human Actors

No dedicated human actors. All interactions with LLM Gateway are mediated through the Consumer system actor (platform gears, external API clients). Platform administrators who configure models and providers do so through Model Registry, not through LLM Gateway directly.

### 2.2 System Actors

#### Consumer

**ID**: `cpt-cf-llm-gateway-actor-consumer`

- **Role**: Sends requests to the Gateway.
- **Needs**: Provider-agnostic API for all AI modalities; normalized responses; async job management; usage visibility.
- **Direction**: inbound
- **Data exchanged**: Chat completion requests (messages, model, parameters), tool definitions, async job commands; receives normalized responses, streaming chunks, job status/results.
- **Availability**: Consumer availability is not a Gateway concern — Gateway responds to incoming requests as they arrive.

#### Provider

**ID**: `cpt-cf-llm-gateway-actor-provider`

- **Role**: External AI service that processes requests. Accessed via Outbound API Gateway.
- **Direction**: outbound
- **Data exchanged**: Provider-formatted requests (prompts, parameters, tool schemas); receives completions, embeddings, generated media, tool calls, usage metrics.
- **Availability**: Provider unavailability triggers fallback (if configured) or returns a provider-unavailable error to the consumer. OAGW circuit breaking protects against cascading failures.

#### Hook Plugin

**ID**: `cpt-cf-llm-gateway-actor-hook-plugin`

- **Role**: Pre-call and post-response interception (moderation, PII, transformation).
- **Direction**: bidirectional
- **Data exchanged**: Outbound: request/response payloads for inspection; inbound: allow/block/modify decisions with optionally transformed payloads.
- **Availability**: If Hook Plugin is unavailable, Gateway fails the request with a hook-unavailable error. Gateway does not bypass hooks silently — the tenant's security policy must be enforced.

#### Usage Tracker

**ID**: `cpt-cf-llm-gateway-actor-usage-tracker`

- **Role**: Receives AI credit consumption reports.
- **Direction**: outbound
- **Data exchanged**: AI credit consumption records (credit amount, attribution: tenant, user, model). Gateway converts consumed tokens to AI credits using per-model prices obtained from Model Registry. Gateway does not report raw token counts or cost estimates — only AI credit amounts.
- **Availability**: Usage record delivery is guaranteed at-least-once — records are queued and retried until delivered. Usage Tracker unavailability does not block request processing; records are buffered and delivered when the tracker becomes available.

Note: Gateway may report more AI credits than the tenant's allocated quota because token consumption cannot be predicted before the request completes. This is expected behavior — the quota check is a best-effort gate, not a hard limit.

#### Quota Manager

**ID**: `cpt-cf-llm-gateway-actor-quota-manager`

- **Role**: Checks available AI credit quotas before request execution.
- **Direction**: outbound
- **Data exchanged**: Quota check requests (tenant context); receives quota status (available/exceeded).
- **Availability**: If Quota Manager is unavailable, Gateway rejects the request with a quota-check-unavailable error. Gateway does not bypass quota checks silently.

Note: The specific component that provides quota management is an open question. The PRD defines the need for a quota-checking dependency but does not prescribe which gear or service fulfills this role.

#### Audit Gear

**ID**: `cpt-cf-llm-gateway-actor-audit-gear`

- **Role**: Compliance event logging.
- **Direction**: outbound
- **Data exchanged**: Audit events (request started, completed, failed, blocked, fallback triggered) with tenant/user/model attribution.
- **Availability**: Audit Gear unavailability does not block request processing. Audit events are delivered best-effort — Gateway logs a warning if delivery fails but does not reject the consumer request.

#### Model Registry

**ID**: `cpt-cf-llm-gateway-actor-model-registry`

- **Role**: Resolves model identifiers to provider and endpoint information per tenant. Gateway queries Model Registry on every request to determine which provider and endpoint to use. Also provides per-model pricing information used by Gateway to convert consumed tokens into AI credits.
- **Direction**: outbound
- **Data exchanged**: Model resolution requests (model identifier, tenant context); receives provider details (endpoint URL, provider type, capabilities, configuration, per-model pricing for AI credit conversion).
- **Availability**: If Model Registry is unavailable, Gateway returns a model-resolution-unavailable error. Gateway cannot route requests without model resolution.

## 3. Operational Concept & Environment

### 3.1 Gear-Specific Environment Constraints

- Requires persistent storage for async/batch job state and guaranteed usage record delivery
- All external provider calls route through Outbound API Gateway (OAGW) — Gateway never calls providers directly
- Media files (images, audio, video, documents) are stored and retrieved via FileStorage gear

### 3.2 Data Ownership & Classification

Gateway acts as a processor — consumers own request/response data; Gateway owns operational metadata (job state, usage records). Provider response data in persisted job records is held on behalf of the consumer and subject to retention-based cleanup.

| Data Category | Examples | Persistence | Sensitivity | Owner |
|---------------|----------|-------------|-------------|-------|
| Transient request/response | Prompts, completions, embeddings | Not persisted — in-memory only | Consumer-defined (may contain PII) | Consumer |
| Persisted job records | Async job state, batch job ID mappings, job results | Retention-bound (10 min / 48 h) | Potentially sensitive — treated as PII-capable | Consumer (held by Gateway) |
| Usage telemetry | AI credit amounts, tenant/user/model attribution | Delivered to Usage Tracker, then removed | Operational — no PII | Gateway |

## 4. Scope

### 4.1 In Scope

- Unified LLM API
- Provider abstraction via adapters (OpenAI, Anthropic, Google, and others)
- Multimodal support: text, images, audio, video, documents
- Synchronous and streaming request/response
- Async job execution with durably persisted job state and results
- Batch processing with durably persisted job ID mappings
- Tool/function calling pass-through (consumer executes tools)
- Structured output with schema validation
- Pre-call and post-response hook plugin interception
- AI credit usage tracking with guaranteed at-least-once delivery (tokens converted to credits via Model Registry prices)
- Provider fallback and timeout enforcement
- Per-tenant AI credit quota enforcement via Quota Manager

### 4.2 Out of Scope

- Conversation state storage — consumers provide full context per request
- Tool execution — Gateway returns tool calls for consumer to execute
- Prompt engineering or prompt optimization
- Model training, fine-tuning, or model hosting
- Direct provider access bypassing OAGW
- Content caching or response deduplication

## 5. Functional Requirements

### P1 — Core

#### Chat Completion

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-fr-chat-completion-v1`

The system **MUST** accept a chat completion request with messages and model identifier, route it to the resolved provider, and return a normalized response with usage metrics.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-provider`

#### Streaming Chat Completion

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-fr-streaming-v1`

The system **MUST** support streaming mode for chat completions, delivering response chunks in a normalized event format as they arrive from the provider.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-provider`

#### Embeddings Generation

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-fr-embeddings-v1`

The system **MUST** accept one or more text inputs and return vector embeddings in a normalized format with usage metrics.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-provider`

#### Vision (Image Analysis)

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-fr-vision-v1`

The system **MUST** accept messages with image references (FileStorage URLs or external URLs), resolve the media, route to a vision-capable model, and return the analysis with usage metrics.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-provider`

#### Image Generation

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-fr-image-generation-v1`

The system **MUST** accept a text prompt, generate an image via the resolved provider, store the result, and return a retrievable URL with usage metrics.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-provider`

#### Text-to-Speech

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-fr-text-to-speech-v1`

The system **MUST** accept a text input, synthesize audio via the resolved provider, store the result, and return a retrievable URL with usage metrics. The system **MUST** support streaming mode for audio delivery.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-provider`

#### Video Understanding

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-fr-video-understanding-v1`

The system **MUST** accept messages with a video reference (FileStorage URL or external URL), resolve the media, route to a video-capable model, and return the analysis with usage metrics.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-provider`

#### Tool/Function Calling

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-fr-tool-calling-v1`

The system **MUST** accept requests with tool definitions (reference, inline GTS, or unified format), resolve schema references, and forward to the provider. The system **MUST** return tool calls in a unified format for the consumer to execute. The system **MUST NOT** execute tools — this is consumer responsibility.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-provider`

#### Structured Output

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-fr-structured-output-v1`

The system **MUST** accept a JSON schema with the request and validate the provider response against it, returning either the validated response or a validation error with details.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-provider`

#### Document Understanding

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-fr-document-understanding-v1`

The system **MUST** accept messages with a document URL, resolve the document, route to a capable model, and return the analysis with usage metrics.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-provider`

#### Async Jobs

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-fr-async-jobs-v1`

The system **MUST** support async execution for long-running operations, returning a job ID that the consumer polls for results. The system **MUST** durably persist job state and store job results until the retention period expires.

The system **MUST** provide a uniform async experience regardless of whether the provider supports async natively:
- Consumer always receives a job ID and polls for results
- The system **MUST** guarantee result availability for the retention period after completion

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-provider`

#### Usage Tracking

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-fr-usage-tracking-v1`

The system **MUST** report AI credit consumption after each request via Usage Tracker. The system **MUST** obtain per-model prices from Model Registry, convert consumed tokens to AI credits, and report the credit amount with attribution (tenant, user, model). The system **MUST** guarantee at-least-once delivery of usage records.

Cross-cutting concern — applies to all operations, no dedicated UC.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-usage-tracker`, `cpt-cf-llm-gateway-actor-model-registry`

#### Model Capability Check

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-fr-model-capability-check-v1`

The system **MUST** verify that the resolved model supports the capabilities required by the request before dispatching to the provider. If the model does not support a required capability, the system **MUST** return a `capability_not_supported` error to the consumer without calling the provider.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`

#### Request Traceability

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-fr-request-traceability-v1`

The response `id` serves as the gateway request identifier. The system **MUST** propagate this identifier throughout the entire request processing pipeline — provider adapter calls, usage events reported to Usage Tracker, error events, and audit events. All internal processing stages **MUST** be correlatable back to the originating response `id`.

The system **MUST** capture the `provider_request_id` returned by the provider (when available) and include it in usage events reported to Usage Tracker and in error events. This enables end-to-end correlation between gateway operations and provider-side request logs.

Cross-cutting concern — applies to all operations, no dedicated UC.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-provider`, `cpt-cf-llm-gateway-actor-usage-tracker`

### P2 — Reliability & Governance

#### Provider Fallback

- [ ] `p2` - **ID**: `cpt-cf-llm-gateway-fr-provider-fallback-v1`

The system **MUST** automatically switch to a fallback provider with matching capabilities when the primary provider fails.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-provider`

#### Timeout Enforcement

- [ ] `p2` - **ID**: `cpt-cf-llm-gateway-fr-timeout-v1`

The system **MUST** enforce the following timeout types:
- Time-to-first-token (TTFT): max wait for initial response chunk
- Total generation timeout: max duration for complete response

On timeout the system **MUST** trigger fallback (if configured) or return a timeout error.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-provider`

#### Pre-Call Interceptor

- [ ] `p2` - **ID**: `cpt-cf-llm-gateway-fr-pre-call-interceptor-v1`

The system **MUST** invoke Hook Plugin before sending a request to the provider. The plugin can allow, block, or modify the request.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-hook-plugin`

#### Post-Response Interceptor

- [ ] `p2` - **ID**: `cpt-cf-llm-gateway-fr-post-response-interceptor-v1`

The system **MUST** invoke all enabled Hook Plugins in order after the provider adapter returns a fully-assembled, normalized response. For streaming responses, the hook is invoked after all stream chunks have been received and the full response is assembled — the response may already have been delivered to the consumer. For async and batch requests, the hook is invoked after the job completes. Post-call hooks are observe-only: the gateway delivers the response to the consumer unconditionally, and the hook is invoked with a read-only view of the response after delivery (streaming) or as a fire-and-forget step that does not affect response delivery (sync). Batch requests are treated as individual calls — hooks are applied per request as if the requests were not batched.

**Actors**: `cpt-cf-llm-gateway-actor-hook-plugin`, `cpt-cf-llm-gateway-actor-consumer`

#### Per-Tenant Quota Enforcement

- [ ] `p2` - **ID**: `cpt-cf-llm-gateway-fr-budget-enforcement-v1`

The system **MUST** check available AI credit quota before execution via Quota Manager and reject requests when the quota is exhausted. After request completion, the system **MUST** report actual AI credit consumption to Usage Tracker.

Note: Gateway may consume more AI credits than the allocated quota because token consumption cannot be predicted before the request completes. This is expected behavior — the pre-request quota check is a best-effort gate, not a hard ceiling.

Cross-cutting concern — applies to all operations, no dedicated UC.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-quota-manager`, `cpt-cf-llm-gateway-actor-usage-tracker`

#### Rate Limiting

- [ ] `p2` - **ID**: `cpt-cf-llm-gateway-fr-rate-limiting-v1`

The system **MUST** enforce rate limits at tenant and user levels and reject requests exceeding configured limits. Rate limiting is closely related to quota management; both may be provided by the same component.

Note: The specific mechanism and component providing rate limiting is an open question — see Open Questions section.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`

### P3 — Additional Capabilities

#### Speech-to-Text

- [ ] `p3` - **ID**: `cpt-cf-llm-gateway-fr-speech-to-text-v1`

The system **MUST** accept messages with an audio reference (FileStorage URL or external URL), resolve the media, route to an STT-capable model, and return the transcription with usage metrics.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-provider`

#### Video Generation

- [ ] `p3` - **ID**: `cpt-cf-llm-gateway-fr-video-generation-v1`

The system **MUST** accept a text prompt, generate video via the resolved provider, store the result, and return a retrievable URL with usage metrics. Video generation typically requires async mode due to long processing times.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-provider`

#### Realtime Audio

- [ ] `p3` - **ID**: `cpt-cf-llm-gateway-fr-realtime-audio-v1`

The system **MUST** support bidirectional audio streaming via a persistent connection for real-time voice conversations, with usage reported on session close.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-provider`

#### Batch Processing

- [ ] `p3` - **ID**: `cpt-cf-llm-gateway-fr-batch-processing-v1`

The system **MUST** accept a batch of requests for async processing at reduced cost, abstracting provider batch APIs. Batch jobs can take up to 24 hours to complete; the system **MUST** durably persist batch-to-provider job ID mappings and results.

**Actors**: `cpt-cf-llm-gateway-actor-consumer`, `cpt-cf-llm-gateway-actor-provider`

### P4 — Enterprise

#### Audit Events

- [ ] `p4` - **ID**: `cpt-cf-llm-gateway-fr-audit-events-v1`

The system **MUST** emit audit events via Audit Gear for compliance: request started, completed, failed, blocked, fallback triggered.

Cross-cutting concern — applies to all operations, no dedicated UC.

**Actors**: `cpt-cf-llm-gateway-actor-audit-gear`

## 6. Non-Functional Requirements

### Scalability

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-nfr-scalability-v1`

The system **MUST** support horizontal scaling without instance-local state or inter-instance coordination. Any instance **MUST** be able to serve any request. Job state **MUST** be accessible from all instances.

### Data Retention

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-nfr-data-retention-v1`

Async job records (ID mappings, results) are retained for 10 minutes after completion for LLM completion jobs and 48 hours after completion for batch jobs. Expired records are cleaned up automatically. Usage delivery records are removed after successful delivery.

### Compatibility

- [ ] `p2` - **ID**: `cpt-cf-llm-gateway-nfr-compatibility-v1`

Gateway API **MUST** maintain backward compatibility within the same major version. Breaking changes require a new API version prefix. Co-existence with other platform gears is guaranteed — Gateway communicates exclusively through SDK traits and ClientHub, with no shared mutable state. The specific API protocol is a design choice documented in DESIGN.md.

### NFR Exclusions

The following quality domains are handled at the platform level and do not require gear-specific NFRs:

- **Authentication / Authorization**: Handled by platform AuthN/AuthZ gears via SecurityContext. All Gateway endpoints require valid authentication; authorization is enforced per tenant through platform middleware.
- **Security implementation**: Credential management handled by CredStore; TLS and network security handled by infrastructure. Content security (PII filtering, moderation) is handled by Hook Plugins, not Gateway itself. Data classification and ownership are documented in § 3.2.
- **Safety**: Not applicable — LLM Gateway is a software-only API gear with no physical interaction or safety-critical operations.
- **Usability / Accessibility**: Not applicable — API-only gear with no user interface.
- **Compliance / Regulatory**: LLM Gateway does not store conversation content. Synchronous request/response data is transient. Async and batch job results are durably persisted for the retention period (up to 48 hours) and may contain provider response data that includes PII depending on consumer requests; Gateway treats all persisted job results as potentially sensitive and relies on retention-based cleanup as the primary data protection control. Compliance requirements for data processed by LLM providers are the responsibility of consumers and the providers themselves.
- **Operations (Deployment / Monitoring)**: Deployment, infrastructure monitoring, distributed tracing, and health checks are handled by platform infrastructure. Gear-level operational metrics (request counters, latency histograms, error breakdowns) are in scope — see Observability NFR below.
- **Maintainability / Documentation**: Follows platform-wide documentation standards. API documentation generated from OpenAPI specs in DESIGN.

### Recovery

- [ ] `p2` - **ID**: `cpt-cf-llm-gateway-nfr-recovery-v1`

**Native async jobs** (provider supports async natively — Gateway stores the provider job ID and resumes polling after restart): job state and results must survive Gateway instance restarts with zero data loss (RPO: 0). Recovery time is bounded by infrastructure restart time (RTO: platform-defined). Persistent storage availability is an infrastructure concern.

**Simulated async jobs** (sync provider + async request — Gateway executes the provider call synchronously on behalf of the consumer): NOT guaranteed to survive Gateway outages. If the Gateway instance is interrupted mid-execution, the in-flight provider call is lost and the job is marked failed. Interrupted simulated jobs are not retried to prevent unauthorized spending of provider tokens — the consumer receives a failed job status and must resubmit if desired.

Batch job metadata (ID mappings, status) survives restarts. Individual batch request results follow the same native/simulated distinction as async jobs.

### Observability

- [ ] `p2` - **ID**: `cpt-cf-llm-gateway-nfr-observability-v1`

The system **MUST** emit OpenTelemetry-compatible operational metrics covering the request lifecycle. Metrics **MUST** include:

- Request counters with model, provider, and status dimensions
- Streaming: streams started and streams aborted (by reason: client disconnect, timeout, error)
- Provider fallback events with source/target provider and reason
- Time-to-first-token latency histogram with model and provider dimensions
- Hook plugin blocks by hook type (pre-call, post-response)
- Budget operations: reservation attempts and settlement completions
- Schema validation failures for structured output with model and provider dimensions
- Async job cache misses (job not found on poll)

All metrics **MUST** be emittable via the platform's OpenTelemetry metrics infrastructure. Metric names **MUST** use the `llm_gateway_` prefix. Label cardinality **MUST** remain bounded — labels are limited to enumerable dimensions (model, provider, status, reason, hook type); unbounded values (tenant ID, request ID) **MUST NOT** be used as metric labels.

## 7. Public Library Interfaces

Not applicable — LLM Gateway exposes only a REST API (documented in DESIGN.md), not a public Rust crate interface. If inter-gear programmatic access is needed in the future, an SDK crate with ClientHub traits will be introduced as a separate artifact.

## 8. Use Cases

#### UC-001: Chat Completion

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-usecase-chat-completion-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: Model available for tenant.

**Flow**:
1. Consumer sends chat_completion(model, messages)
2. Gateway resolves provider via Model Registry
3. Gateway sends request via Outbound API Gateway
4. Provider returns response
5. Gateway returns normalized response with usage

**Postconditions**: Response returned, usage reported.

**Alternative Flows**:
- **Provider error**: Gateway normalizes provider error to Gateway error format and returns to consumer.
- **Timeout**: Gateway enforces TTFT and total timeouts; on expiry triggers fallback (if configured) or returns timeout error.
- **Invalid model**: Gateway returns model-not-found error if model is unavailable for tenant.
- **Model not capable**: Gateway returns capability_not_supported error if model does not support a capability required by the request (e.g., tool calling, vision, structured output).

**Acceptance criteria**:
- Response in normalized format regardless of provider
- Usage metrics included (AI credit amount, model attribution)
- Provider errors normalized to Gateway error format

#### UC-002: Streaming Chat Completion

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-usecase-streaming-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: Model available for tenant, model supports streaming.

**Flow**:
1. Consumer sends chat_completion(stream=true)
2. Gateway resolves provider via Model Registry
3. Gateway establishes streaming connection to provider
4. Gateway normalizes each chunk
5. Gateway streams chunks to Consumer
6. Gateway sends final usage summary

**Postconditions**: Stream completed, usage reported.

**Alternative Flows**:
- **Mid-stream provider failure**: Gateway closes stream with error event and reports partial usage.
- **Consumer disconnects**: Gateway cancels upstream provider request and reports usage for consumed tokens.
- **Model not capable**: Gateway returns capability_not_supported error if model does not support streaming.

**Acceptance criteria**:
- Chunks normalized from provider format
- Final message includes usage metrics
- Connection errors propagated to consumer

#### UC-003: Embeddings Generation

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-usecase-embeddings-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: Embedding model available for tenant.

**Flow**:
1. Consumer sends embed(model, input[])
2. Gateway resolves provider via Model Registry
3. Gateway sends request via Outbound API Gateway
4. Provider returns vectors
5. Gateway returns vectors with usage

**Postconditions**: Vectors returned, usage reported.

**Alternative Flows**:
- **Provider error**: Gateway normalizes error and returns to consumer.
- **Invalid input**: Gateway returns validation error if input is empty or exceeds provider limits.

**Acceptance criteria**:
- Vectors returned in normalized format
- Usage metrics included (AI credit amount)

#### UC-004: Vision (Image Analysis)

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-usecase-vision-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: Model available for tenant, model supports required content type.

**Flow**:
1. Consumer sends chat_completion with image URLs
2. Gateway resolves provider via Model Registry
3. Gateway fetches images from FileStorage
4. Gateway sends request via Outbound API Gateway
5. Provider returns analysis
6. Gateway returns response with usage

**Postconditions**: Response returned, usage reported.

**Alternative Flows**:
- **Media resolution failure**: Gateway returns media-not-found error if FileStorage URL is invalid or inaccessible.
- **Provider error**: Gateway normalizes error and returns to consumer.

**Acceptance criteria**:
- Multiple images supported per request
- Response in normalized format
- Usage metrics included

#### UC-005: Image Generation

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-usecase-image-generation-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: Image generation model available for tenant.

**Flow**:
1. Consumer sends generation request with prompt
2. Gateway resolves provider via Model Registry
3. Gateway sends request via Outbound API Gateway
4. Provider returns generated image
5. Gateway stores image in FileStorage
6. Gateway returns URL with usage

**Postconditions**: Image stored, URL returned, usage reported.

**Alternative Flows**:
- **Provider error**: Gateway normalizes error and returns to consumer.
- **FileStorage upload failure**: Gateway returns storage error; generated image is lost.

**Acceptance criteria**:
- Generated image accessible via returned URL
- Response in normalized format
- Usage metrics included

#### UC-006: Speech-to-Text

- [ ] `p3` - **ID**: `cpt-cf-llm-gateway-usecase-speech-to-text-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: STT model available for tenant.

**Flow**:
1. Consumer sends message with audio URL
2. Gateway resolves provider via Model Registry
3. Gateway fetches audio from FileStorage
4. Gateway sends request via Outbound API Gateway
5. Provider returns transcription
6. Gateway returns text response with usage

**Postconditions**: Transcription returned, usage reported.

**Alternative Flows**:
- **Media resolution failure**: Gateway returns media-not-found error if audio URL is invalid or inaccessible.
- **Provider error**: Gateway normalizes error and returns to consumer.

**Acceptance criteria**:
- Transcription in normalized format
- Usage metrics included

#### UC-007: Text-to-Speech

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-usecase-text-to-speech-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: TTS model available for tenant.

**Flow**:
1. Consumer sends TTS request with text
2. Gateway resolves provider via Model Registry
3. Gateway sends request via Outbound API Gateway
4. Provider returns audio
5. Gateway stores audio in FileStorage
6. Gateway returns URL with usage

**Postconditions**: Audio stored, URL returned, usage reported.

**Alternative Flows**:
- **Provider error**: Gateway normalizes error and returns to consumer.
- **FileStorage upload failure**: Gateway returns storage error; generated audio is lost.

**Acceptance criteria**:
- Generated audio accessible via returned URL
- Streaming mode supported (audio chunks returned directly)
- Usage metrics included

#### UC-008: Video Understanding

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-usecase-video-understanding-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: Model available for tenant, model supports required content type.

**Flow**:
1. Consumer sends message with video URL
2. Gateway resolves provider via Model Registry
3. Gateway fetches video from FileStorage
4. Gateway sends request via Outbound API Gateway
5. Provider returns analysis
6. Gateway returns response with usage

**Postconditions**: Response returned, usage reported.

**Alternative Flows**:
- **Media resolution failure**: Gateway returns media-not-found error if video URL is invalid or inaccessible.
- **Provider error**: Gateway normalizes error and returns to consumer.

**Acceptance criteria**:
- Response in normalized format
- Usage metrics included

#### UC-009: Video Generation

- [ ] `p3` - **ID**: `cpt-cf-llm-gateway-usecase-video-generation-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: Video generation model available for tenant.

**Flow**:
1. Consumer sends generation request with prompt
2. Gateway resolves provider via Model Registry
3. Gateway sends request via Outbound API Gateway
4. Provider returns generated video
5. Gateway stores video in FileStorage
6. Gateway returns URL with usage

**Postconditions**: Video stored, URL returned, usage reported.

**Alternative Flows**:
- **Provider error**: Gateway normalizes error and returns to consumer.
- **FileStorage upload failure**: Gateway returns storage error; generated video is lost.

**Acceptance criteria**:
- Generated video accessible via returned URL
- Async mode supported (typically required due to long processing)
- Usage metrics included

#### UC-010: Tool/Function Calling

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-usecase-tool-calling-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: Model available for tenant, model supports function calling.

**Flow**:
1. Consumer sends chat_completion with tool definitions
2. Gateway resolves provider via Model Registry
3. Gateway resolves schemas via Type Registry (for reference and inline GTS formats)
4. Gateway converts tools to provider format
5. Gateway sends request via Outbound API Gateway
6. Provider returns tool_calls
7. Gateway returns tool_calls in unified format
8. Consumer executes tools, sends results
9. Gateway forwards tool results to provider
10. Provider returns final response
11. Gateway returns response with usage

**Postconditions**: Response returned, usage reported.

**Alternative Flows**:
- **Schema resolution failure**: Gateway returns schema-not-found error if GTS reference is unresolvable.
- **Provider error**: Gateway normalizes error and returns to consumer.
- **Partial tool execution**: Consumer sends partial tool results; Gateway forwards to provider for continued generation.

**Acceptance criteria**:
- Tool definitions supported: reference, inline GTS, unified format (OpenAI-like)
- Tool calls returned in unified format
- Response in normalized format

#### UC-011: Structured Output

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-usecase-structured-output-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: Model available for tenant.

**Flow**:
1. Consumer sends chat_completion with response_schema
2. Gateway resolves provider via Model Registry
3. Gateway sends request via Outbound API Gateway
4. Provider returns JSON response
5. Gateway validates response against schema
6. Gateway returns validated response with usage (or validation_error if invalid)

**Postconditions**: Valid JSON returned, usage reported.

**Alternative Flows**:
- **Schema validation failure**: Gateway returns validation_error with details if response does not match schema.
- **Provider error**: Gateway normalizes error and returns to consumer.

**Acceptance criteria**:
- Response validated against provided schema
- Returns validation_error with details if schema validation fails
- Response in normalized format

#### UC-012: Document Understanding

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-usecase-document-understanding-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: Model available for tenant, model supports required content type.

**Flow**:
1. Consumer sends message with document URL
2. Gateway resolves provider via Model Registry
3. Gateway fetches document from FileStorage
4. Gateway sends request via Outbound API Gateway
5. Provider returns analysis
6. Gateway returns response with usage

**Postconditions**: Response returned, usage reported.

**Alternative Flows**:
- **Media resolution failure**: Gateway returns media-not-found error if document URL is invalid or inaccessible.
- **Provider error**: Gateway normalizes error and returns to consumer.

**Acceptance criteria**:
- Response in normalized format
- Usage metrics included

#### UC-013: Async Jobs

- [ ] `p1` - **ID**: `cpt-cf-llm-gateway-usecase-async-jobs-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: Model available for tenant.

**Flow**:
1. Consumer sends request with async=true
2. Gateway resolves provider via Model Registry
3. Gateway initiates async job
4. Gateway returns job_id
5. Consumer polls get_job(job_id)
6. Gateway returns status/result
7. (Optional) Consumer cancels job via cancel_job(job_id)

**Postconditions**: Job completed, cancelled, or result returned.

**Alternative Flows**:
- **Provider error during execution**: Gateway marks job as failed, stores error details in job result.
- **Job not found**: Gateway returns job-not-found error if job ID is invalid or expired.
- **Cancellation of completed job**: Gateway returns job-already-completed status; no-op.

**Acceptance criteria**:
- Sync provider + async request: Gateway simulates job
- Async provider + sync request: Gateway polls internally
- Job status: pending, running, completed, failed, cancelled
- Job cancellation supported

#### UC-014: Realtime Audio

- [ ] `p3` - **ID**: `cpt-cf-llm-gateway-usecase-realtime-audio-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: Model available for tenant, model supports realtime audio.

**Flow**:
1. Consumer establishes persistent bidirectional connection
2. Gateway resolves provider via Model Registry
3. Gateway connects to provider via bidirectional channel
4. Bidirectional audio/text streaming
5. Consumer closes connection
6. Gateway returns usage summary

**Postconditions**: Session closed, usage reported.

**Alternative Flows**:
- **Connection failure**: Gateway returns connection error if realtime channel cannot be established.
- **Mid-session provider failure**: Gateway closes session with error event and reports partial usage.

**Acceptance criteria**:
- Bidirectional streaming supported
- Usage summary on close

#### UC-015: Provider Fallback

- [ ] `p2` - **ID**: `cpt-cf-llm-gateway-usecase-provider-fallback-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: Model available for tenant.

**Flow**:
1. Consumer sends request with fallback configuration
2. Gateway resolves provider via Model Registry
3. Gateway sends request to primary provider
4. Primary provider fails
5. Gateway selects fallback from request configuration
6. Gateway sends request to fallback provider
7. Gateway returns response (fallback indicated)

**Postconditions**: Response returned via fallback.

**Acceptance criteria**:
- Fallback configuration provided in request
- Fallback selection based on capability match
- Response includes fallback indicator

#### UC-016: Timeout Enforcement

- [ ] `p2` - **ID**: `cpt-cf-llm-gateway-usecase-timeout-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: Model available for tenant.

**Flow**:
1. Consumer sends request
2. Gateway starts timeout tracking (TTFT, total)
3. Gateway sends request to provider
4. If TTFT timeout: Gateway triggers fallback or error
5. If total timeout: Gateway triggers fallback or error
6. Gateway returns response or error

**Postconditions**: Response returned or timeout error.

**Acceptance criteria**:
- TTFT (time-to-first-token) timeout enforced
- Total generation timeout enforced
- On timeout: fallback (if configured) or error

#### UC-017: Pre-Call Interceptor

- [ ] `p2` - **ID**: `cpt-cf-llm-gateway-usecase-pre-call-interceptor-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: Hook Plugin configured for tenant.

**Flow**:
1. Consumer sends request
2. Gateway invokes all enabled Hook Plugins in order (pre_call)
3. Each plugin can allow, block, or modify the request; modified request is passed to the next plugin
4. If any plugin blocks: Gateway returns request_blocked error; remaining plugins are not invoked
5. If all plugins allow/modify: Gateway proceeds with the (possibly modified) request

**Postconditions**: Request processed with (possibly modified) request, or blocked.

**Acceptance criteria**:
- All enabled plugins are invoked in order
- Each plugin can allow, block, or modify the request
- First blocking plugin stops execution and returns request_blocked error
- Modified request is passed to subsequent plugins and ultimately to the provider adapter

#### UC-018: Post-Response Interceptor

- [ ] `p2` - **ID**: `cpt-cf-llm-gateway-usecase-post-response-interceptor-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: Hook Plugin configured for tenant.

**Flow**:
1. Provider adapter returns normalized response (for streaming: after all chunks received and full response assembled; for async/batch: after job completes)
2. Gateway invokes all enabled Hook Plugins in order (post_response)
3. Each plugin observes and processes the response (e.g., logging, metrics); plugin outcome has no effect on response delivery
4. Response is delivered to the consumer regardless of post-call plugin outcome

**Postconditions**: Response delivered; plugins have processed the response.

**Acceptance criteria**:
- All enabled plugins are invoked in order after the full response is available
- Post-call plugins are observe-only; plugin outcome has no effect on response delivery
- Response is always delivered to the consumer regardless of post-call plugin outcome
- For streaming: hook invoked after stream completes and full response is assembled; response already delivered to consumer
- For batch: hooks are applied per individual request as if requests were not batched

#### UC-019: Rate Limiting

- [ ] `p2` - **ID**: `cpt-cf-llm-gateway-usecase-rate-limiting-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: Rate limits configured for tenant.

**Flow**:
1. Consumer sends request
2. Gateway checks rate limits
3. If limit exceeded: Gateway returns rate_limited error
4. If within limits: Gateway proceeds with request

**Postconditions**: Request processed or rejected.

**Acceptance criteria**:
- Rate limits enforced at tenant level
- Rate limits enforced at user level
- Exceeded requests return rate_limited error

#### UC-020: Batch Processing

- [ ] `p3` - **ID**: `cpt-cf-llm-gateway-usecase-batch-processing-v1`
**Actor**: `cpt-cf-llm-gateway-actor-consumer`

**Preconditions**: Model available for tenant. If the resolved provider does not support a native batch API, Gateway rejects the batch request with a batch-not-supported error.

**Flow**:
1. Consumer submits batch of requests
2. Gateway resolves provider via Model Registry
3. Gateway submits to provider batch API
4. Gateway returns batch_id
5. Consumer polls for results
6. Gateway returns status and results
7. (Optional) Consumer cancels batch

**Postconditions**: Batch completed, results returned.

**Acceptance criteria**:
- Abstracts OpenAI Batch API, Anthropic Message Batches
- Partial results available as completed
- Batch cancellation supported

## 9. Acceptance Criteria

- [ ] All supported modalities (text, image, audio, video, document) produce normalized responses regardless of provider
- [ ] Gateway overhead < 50ms P99 excluding provider latency
- [ ] Availability ≥ 99.9%
- [ ] Async and batch job state survives Gateway instance restarts
- [ ] Usage records are delivered to Usage Tracker at least once for every completed request
- [ ] Expired job records are cleaned up within the defined retention windows

## 10. Dependencies

| Dependency | Description | Criticality |
|------------|-------------|-------------|
| Outbound API Gateway (OAGW) | Routes all external provider calls, injects credentials, provides circuit breaking | `p1` |
| Model Registry | Resolves model identifiers to provider and endpoint information | `p1` |
| FileStorage | Stores and retrieves media files (images, audio, video, documents) | `p1` |
| CredStore | Provides provider API credentials to OAGW | `p1` |
| Usage Tracker | Receives AI credit consumption reports | `p1` |
| Quota Manager | Checks available AI credit quotas before request execution (specific component TBD — see Open Questions) | `p2` |
| Type Registry | Resolves GTS schema references for tool definitions | `p2` |
| Audit Gear | Receives compliance audit events | `p4` |

## 11. Assumptions

- LLM providers are accessible via OAGW with valid credentials in CredStore
- Model Registry maintains an up-to-date catalog of available models and their capabilities per tenant
- FileStorage is available for media operations (upload, download)
- Database is available and shared across all Gateway instances

## 12. Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| Provider API breaking changes | Adapter incompatibility, request failures | Version-pinned adapters, integration test suite per provider |
| Provider rate limit exhaustion | Requests rejected by provider | Gateway-level rate limiting, provider fallback chains |
| Database growth from batch jobs | Storage pressure, degraded query performance | Automated retention cleanup, monitoring on table sizes |
| Provider outage during batch processing | Batch jobs stuck in pending state | Timeout-based job expiration, consumer-visible status updates |
| Usage record delivery lag | Budget enforcement decisions based on stale data | Guaranteed delivery mechanism with short polling interval |

## 13. Open Questions

- **Quota enforcement ownership and component** (Owner: Platform Architecture, Resolve by: before P2 implementation begins): Two related decisions must be made together. (1) *Ownership boundary*: does Gateway own quota *enforcement* (atomic preflight reserve + terminal settle + bounded debit on abort) or only *metering* (report usage, external component enforces)? The current `check_quota()` → proceed → `report_usage()` sequence is non-atomic — under concurrent load, multiple requests pass `check_quota()` before any `report_usage()` completes, allowing a tenant to exceed their limit by N×budget (N = concurrent in-flight requests). If Gateway owns enforcement, the reserve/settle pattern is required, not optional — a best-effort gate is effectively decorative under load. If Gateway does metering only, the external component handles enforcement asynchronously. (2) *Component identity*: the specific component that provides quota management is not yet defined — it may be a dedicated Quota Manager gear, an extension of Usage Tracker, or an external service. Both decisions must be resolved before implementing `cpt-cf-llm-gateway-fr-budget-enforcement-v1`.
- **Budget enforcement edge cases** (Owner: Platform Architecture, Resolve by: with quota enforcement ownership decision above): The following scenarios must be addressed when the ownership boundary is decided: (a) *Provider stream without usage* — if a provider stream closes before delivering usage data (network error, provider error mid-stream), policy must specify whether to debit input tokens only, report zero, or surface an error; (b) *Fallback billing* — when a primary provider fails after consuming input tokens, policy must define whether partial cost is reported before initiating fallback or only on final completion, with one usage event per committed debit and no double-reporting across fallback attempts; (c) *Background job budgeting* — quota is checked at submission time but the job executes minutes later under a possibly changed quota state, requiring the reserve/settle pattern to span the submission-to-execution gap if Gateway owns enforcement.
- **Rate limiting mechanism** (Owner: Platform Architecture, Resolve by: before P2 implementation begins): Rate limiting (`cpt-cf-llm-gateway-fr-rate-limiting-v1`) is closely related to quota management. Whether rate limiting and quota enforcement are provided by the same component or separate components is an open question.
- **Request monitoring and observability** (Owner: Platform Architecture, Resolve by: before P1 implementation begins): The LLM Gateway needs to track how many requests are being processed, including metrics such as request counts, latency, error rates, and token usage per provider/model. However, the platform does not yet have a standardized approach to monitoring and observability across gears. A platform-wide monitoring strategy must be agreed upon before implementing gear-level metrics, to ensure consistency and avoid fragmented solutions.
- **Cross-gear request traceability** (Owner: Platform Architecture, Resolve by: before P1 implementation begins): The platform needs a mechanism that assigns a request ID at the incoming requests gateway (api-gateway gear) and propagates it through all inter-gear calls via ClientHub/SDK traits. This would enable end-to-end request tracing across gear boundaries — for example, a consumer request arriving at the API gateway, flowing through LLM Gateway, then to Model Registry and Usage Tracker, all correlated by a single platform-level request ID. Currently, each gear operates with its own request context; there is no standardized correlation ID propagated across inter-gear boundaries. This is a platform-level concern that affects all gears, not just LLM Gateway. The LLM Gateway's response `id` (`cpt-cf-llm-gateway-fr-request-traceability-v1`) provides gear-level traceability, but a platform-wide mechanism is needed for cross-gear correlation.

## 14. Traceability

- **Design**: [DESIGN.md](./DESIGN.md)
- **ADRs**: [ADR/](./ADR/)
- **Decomposition**: Not yet created — planned for feature-level breakdown