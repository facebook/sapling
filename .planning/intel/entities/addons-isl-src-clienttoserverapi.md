---
path: /Users/jonas/code/sapling/addons/isl/src/ClientToServerAPI.ts
type: service
updated: 2026-01-21
status: active
---

# ClientToServerAPI.ts

## Purpose

API client for sending messages to ISL server and receiving responses. Handles serialization, subscription management, and request-response patterns.

## Exports

- `ClientToServerAPI` - Main API client class
- Message type definitions

## Dependencies

- [[addons-isl-src-types]] - Message types
- shared utilities - Serialization and utility functions

## Used By

TBD

## Notes

Bidirectional communication channel between client and server. Manages WebSocket connection lifecycle and message routing.
