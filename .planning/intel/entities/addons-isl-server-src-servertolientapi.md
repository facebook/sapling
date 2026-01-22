---
path: /Users/jonas/code/sapling/addons/isl-server/src/ServerToClientAPI.ts
type: service
updated: 2026-01-21
status: active
---

# ServerToClientAPI.ts

## Purpose

Server-side API handler that processes client messages and sends responses. Manages subscriptions, operations, and repository state synchronization to multiple clients.

## Exports

- `ServerToClientAPI` - Main API handler class

## Dependencies

- [[addons-isl-server-src-repository]] - Repository operations
- [[addons-isl-src-types]] - Shared message types
- shared utilities - Serialization and utilities

## Used By

TBD

## Notes

Central message router for server-side logic. Processes all incoming client requests and pushes updates to subscribed clients. Handles multi-client scenarios with repository caching.
