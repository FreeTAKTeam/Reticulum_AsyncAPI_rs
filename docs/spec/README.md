# Reticulum_AsyncAPI_rs Architecture

This directory will hold deeper architecture narratives and operational runbooks.

Current baseline:
- AsyncAPI contract is authoritative for mesh commands/events.
- Local control-plane is async-job oriented and intentionally non-global.
- Runtime uses daemon RPC bridge semantics (link preferred, LXMF fallback).
