# Reticulum_AsyncAPI_rs

AsyncAPI-first Reticulum/LXMF framework in Rust.

## Purpose

`Reticulum_AsyncAPI_rs` is the Rust successor framework inspired by
`Reticulum_OpenAPI`, with AsyncAPI as the authoritative mesh contract for all
commands/events crossing Reticulum/LXMF.

- Mesh/Data-plane: canonical MessagePack envelopes driven by AsyncAPI.
- Local Control-plane: minimal HTTP API with async jobs, status, config,
  contract retrieval, logs/SSE, cache, and ACL management.

## Workspace

- `contracts/retasyncapi-v1.asyncapi.yaml`: v1 mesh contract.
- `crates/retasync_contract`: shared envelopes, scalar types, MessagePack codec,
  generated contract types.
- `crates/retasync_codegen`: AsyncAPI -> Rust codegen library.
- `crates/retasync_mesh_bridge`: daemon bridge trait and baseline in-memory implementation.
- `crates/retasync_control_plane`: local HTTP control-plane and SSE.
- `crates/retasync_storage`: SQLite repository and schema.
- `crates/retasync_transfer`: transfer domain types.
- `crates/retasync_cli`: `retasyncd` daemon binary.
- `tools/retasync-convert`: OpenAPI -> AsyncAPI migration tool.
- `xtask`: `cargo xtask codegen` and `cargo xtask codegen --check`.
- `examples/emergency_crud`: emergency CRUD command envelope example.

## Local Commands

```bash
cargo xtask codegen
cargo xtask codegen --check
cargo run -p retasync-convert -- openapi --in path/to/openapi.yaml --out contracts/converted.asyncapi.yaml
cargo run -p retasync-convert -- openapi --in path/to/openapi.yaml --out contracts/converted.asyncapi.yaml --profile emergency-management
cargo run -p retasync_cli -- serve --config config/node.toml
```

## Control-Plane Endpoints (v1)

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
- `GET /v1/logs/stream`
- `GET /v1/security/allowlist`
- `POST /v1/security/allowlist`
- `DELETE /v1/security/allowlist/{identity_hash}`

## License

EPL-2.0
