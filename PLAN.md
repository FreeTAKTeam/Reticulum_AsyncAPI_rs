# Reticulum_AsyncAPI_rs V1 Specification (Decision Complete)

`Reticulum_AsyncAPI_rs` will be a Rust-first, AsyncAPI-authoritative framework that delivers the same conceptual capability class as `Reticulum_OpenAPI`, but as a clean break in contract and runtime behavior. V1 will use a daemon-bridge model (external Reticulum/LXMF daemon via RPC), canonical MessagePack on the mesh data-plane, async-job semantics on local HTTP control-plane, SQLite-backed local state, identity allowlist ACLs, SSE updates, and a full Emergency CRUD reference implementation including file/resource transfer.

## Scope
- In:
  - AsyncAPI-first mesh contract for commands/events over Reticulum/LXMF.
  - Daemon-bridge runtime (no embedded runtime requirement in framework binary).
  - Minimal RW local HTTP control-plane with async jobs, logs, config, contract retrieval, cache query, ACL management.
  - Canonical MessagePack wire format on mesh.
  - Identity allowlist/denylist enforcement.
  - Compile-time Rust code generation from AsyncAPI.
  - OpenAPI-to-AsyncAPI converter tool for migration.
  - Emergency CRUD reference app (commands, events, transfer, SSE, local control-plane).
  - Cross-desktop support (Linux/macOS/Windows) in CI.
  - EPL license.
- Out:
  - Runtime dual-contract support (OpenAPI + AsyncAPI simultaneously).
  - At-least-once or exactly-once guarantees.
  - Non-Rust SDK generation in v1.
  - Full admin plane for global mesh orchestration.

## Public APIs, Interfaces, and Types

### Rust crates and exported interfaces
- `retasync_contract`:
  - `MeshCommandEnvelope<T>`
  - `MeshResultEnvelope<T>`
  - `MeshEventEnvelope<T>`
  - `MeshTransferEnvelope<T>`
  - Shared scalar types: `MessageId`, `CorrelationId`, `IdentityHash`, `OperationName`, `EventName`.
- `retasync_codegen`:
  - Generates `generated/contracts.rs` from AsyncAPI.
  - Generates typed command/event payload structs.
  - Generates `CommandDispatch` trait (one async method per command operation).
  - Generates `MeshClient` typed call API for Rust consumers.
- `retasync_mesh_bridge`:
  - `RpcMeshBridge` trait implementation for daemon RPC transport.
  - `send_command`, `publish_event`, `start_transfer`, `query_receipt`, `poll_events`.
- `retasync_control_plane`:
  - HTTP handlers and SSE stream.
  - Async job orchestration and local cache query API.
- `retasync_storage`:
  - SQLite repositories for jobs, cached events/messages, transfer records, ACL, config snapshots.
- `retasync_convert`:
  - `retasync-convert openapi --in <oas> --out <asyncapi>`.

### Local HTTP control-plane (v1)
- `GET /health/live`
- `GET /health/ready`
- `GET /v1/node/status`
- `GET /v1/node/config`
- `PUT /v1/node/config`
- `GET /v1/contracts/asyncapi`
- `GET /v1/jobs/{job_id}`
- `GET /v1/jobs/{job_id}/result`
- `POST /v1/jobs/commands/{operation}`
- `POST /v1/jobs/transfers/upload`
- `GET /v1/transfers/{transfer_id}`
- `GET /v1/cache/events`
- `GET /v1/cache/messages`
- `GET /v1/logs`
- `GET /v1/logs/stream` (SSE)
- `GET /v1/security/allowlist`
- `POST /v1/security/allowlist`
- `DELETE /v1/security/allowlist/{identity_hash}`

### HTTP behavior contract
- Command/transfer initiation endpoints are async-job only.
- `POST /v1/jobs/...` returns `202 Accepted` with `job_id`, `status_url`, `submitted_at`.
- No endpoint promises immediate global mesh completion.
- Blocking RPC mode is not exposed in v1.

## Mesh/Data-plane Contract (AsyncAPI authoritative)

### AsyncAPI baseline
- AsyncAPI 3.x document at `contracts/retasyncapi-v1.asyncapi.yaml`.
- Naming convention: namespaced snake case.
  - Command operations: `emergency_action_message.create`, `event.retrieve`.
  - Event operations: `emergency_action_message.created`, `transfer.progress`.
- Hybrid channel model:
  - `commands/{operation}`
  - `results/{operation}`
  - `events/{event}`

### Envelope schema (shared)
- Required fields:
  - `message_id` (UUIDv7 string)
  - `operation` or `event` (namespaced snake)
  - `sent_at` (RFC3339 UTC)
  - `source_identity` (hex)
  - `destination_identity` (hex)
  - `content_type` (`application/msgpack`)
  - `payload` (operation-specific schema)
- Optional fields:
  - `correlation_id` (required for result messages)
  - `ttl_ms`
  - `transport_hint` (`link` or `lxmf`)

### Transport selection
- Dual mode:
  - Commands: direct link preferred.
  - Fallback commands/events: LXMF propagation path.
  - Bridge annotates transport used in local job/result metadata.

### Delivery semantics
- At-most-once in v1:
  - No automatic retry loop in mesh bridge.
  - Failed sends become terminal job failure.
  - Re-submission is explicit client behavior.
  - Correlation IDs still used for tracing and result association.

### Wire encoding
- Canonical MessagePack only on mesh.
- Canonicalization rules enforced in shared codec module.
- JSON is only for local HTTP boundary and logs.

## Control-plane Design

### Security posture
- Default bind: loopback.
- Non-loopback bind requires configured bearer token.
- Mesh ACL:
  - Optional allowlist/denylist by identity hash.
  - Enforced before command dispatch.
  - Managed via local control-plane endpoints.

### Observability
- Logs endpoint supports filter params (`since`, `level`, `contains`, `limit`).
- SSE stream emits:
  - `job.status.changed`
  - `mesh.event.received`
  - `transfer.progress`
  - `node.link.state`
- Metrics endpoint is intentionally out of v1 scope.

## Persistence and Local State (SQLite)

### Required tables
- `jobs`
- `job_attempts`
- `job_results`
- `cached_events`
- `cached_messages`
- `transfers`
- `acl_allowlist`
- `acl_denylist`
- `node_config_revisions`

### Retention defaults
- Jobs/results retained 24h.
- Cached events/messages retained 24h.
- Transfer records retained 7d.
- Retention values configurable via node config.

## Runtime and Daemon Bridge

### Runtime model
- Framework process (`retasyncd`) bridges to external daemon(s) over RPC.
- Uses framed RPC codec compatible with `reticulum-rs`/`lxmf-rs` daemon surface.
- Readiness fails when daemon unavailable; liveness remains healthy unless process is degraded.

### Configuration
- `config/node.toml`:
  - `rpc.endpoint`
  - `http.bind`
  - `http.auth_token`
  - `storage.sqlite_path`
  - `acl.mode`
  - `retention.*`
  - `transport.prefer_link=true`
- Dynamic mutable subset exposed via `PUT /v1/node/config`.

## Code Generation and Migration Tooling

### Compile-time codegen
- Command: `cargo xtask codegen`.
- CI guard: `cargo xtask codegen --check`.
- Generated artifacts are committed and validated for drift.
- Build fails if AsyncAPI and generated Rust types diverge.

### OpenAPI -> AsyncAPI converter
- CLI: `retasync-convert openapi --in <path> --out <path>`.
- Output:
  - Converted AsyncAPI document.
  - Mapping report (`operationId -> command operation`).
  - Warning report for unsupported source constructs.
- Converter includes a profile for the existing EmergencyManagement OAS file in this repo.

## Repository Layout (target)

- `contracts/retasyncapi-v1.asyncapi.yaml`
- `crates/retasync_contract`
- `crates/retasync_codegen`
- `crates/retasync_mesh_bridge`
- `crates/retasync_control_plane`
- `crates/retasync_storage`
- `crates/retasync_transfer`
- `crates/retasync_cli`
- `tools/retasync-convert`
- `examples/emergency_crud`
- `docs/spec/`
- `docs/migration/`

## Action Items
[ ] Create Rust workspace and EPL licensing scaffold with crate boundaries listed above.  
[ ] Implement canonical MessagePack codec and shared envelope/domain scalar types in `retasync_contract`.  
[ ] Define and commit `contracts/retasyncapi-v1.asyncapi.yaml` with hybrid command/result/event channels and emergency + transfer operations.  
[ ] Implement compile-time codegen pipeline (`xtask`) that emits typed Rust models, dispatch traits, and typed client stubs from AsyncAPI.  
[ ] Implement daemon RPC bridge adapter and transport selection logic (link preferred, LXMF fallback) with at-most-once semantics.  
[ ] Implement SQLite storage/repository layer with migrations and retention cleanup jobs.  
[ ] Implement local HTTP control-plane and SSE endpoints with async-job lifecycle and cache/log access.  
[ ] Implement security controls: loopback-default binding, non-loopback token enforcement, ACL allowlist APIs, mesh ACL enforcement hook.  
[ ] Implement resource/file transfer pipeline and AsyncAPI operations/events for transfer status and completion.  
[ ] Build Emergency CRUD reference app against generated contracts and verify complete command/event/transfer flow.  
[ ] Build OpenAPI->AsyncAPI converter and mapping report output; validate against current EmergencyManagement OAS.  
[ ] Add docs for architecture, operations, migration, and contract authoring workflow.

## Test Cases and Acceptance Scenarios

### Contract and codegen
- AsyncAPI lint/validation passes.
- `cargo xtask codegen --check` passes with no drift.
- Generated types match required schemas and operation names.

### Mesh codec and envelope
- Canonical MessagePack vectors are stable across OS targets.
- Invalid payload shapes fail fast with typed errors.
- Correlation IDs round-trip from command to result.

### Daemon bridge
- RPC timeout/failure yields terminal job failure with diagnostic reason.
- Link-first then LXMF fallback path selected correctly when link unavailable.
- At-most-once behavior verified: no auto retry after send failure.

### HTTP control-plane
- `POST /v1/jobs/commands/{operation}` returns `202` with valid `job_id`.
- `GET /v1/jobs/{id}` transitions through valid states.
- SSE stream emits job and mesh events.
- `GET /v1/contracts/asyncapi` returns current contract document.
- Logs query endpoint filters and paginates correctly.

### Security
- Loopback bind without token works locally.
- Non-loopback bind rejects requests without valid bearer token.
- ACL allowlist blocks non-authorized identity hashes.

### Transfer
- Upload job lifecycle moves to success with completion metadata.
- Transfer failure path records reason and emits SSE status event.

### Migration tooling
- Converter produces valid AsyncAPI for existing EmergencyManagement OAS.
- Converter emits deterministic mapping report.
- Unsupported OpenAPI constructs are surfaced as warnings.

### Cross-desktop CI
- Linux/macOS/Windows matrix builds and tests pass.
- Integration suite with mocked daemon RPC passes on all three OSes.

## Assumptions and Defaults Chosen
- Runtime communication with Reticulum/LXMF is via external daemon RPC in v1.
- Rust stack defaults: `tokio`, `axum`, `serde`, `sqlx` (SQLite), `tracing`.
- Mesh payload encoding is canonical MessagePack only.
- V1 delivery is at-most-once by design.
- Control-plane is minimal RW and local-operations-focused.
- SDK generation is Rust-only in v1.
- Emergency CRUD reference app is mandatory for v1 acceptance.
- Repository license is EPL.
