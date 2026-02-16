# Migration Notes

This directory tracks OpenAPI-to-AsyncAPI migration guidance.

Baseline tooling in this repository:
- `retasync-convert openapi --in <oas> --out <asyncapi>`
- Optional profile: `retasync-convert openapi --in <oas> --out <asyncapi> --profile emergency-management`
- `<out>.mapping.json` for deterministic operation mapping
- `<out>.warnings.json` for unsupported constructs
