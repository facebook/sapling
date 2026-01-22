---
path: /Users/jonas/code/sapling/addons/isl-server/src/Repository.ts
type: service
updated: 2026-01-21
status: active
---

# Repository.ts

## Purpose

Core repository abstraction for executing Sapling commands and fetching repository state. Acts as bridge between ISL and the underlying Sapling CLI.

## Exports

- `Repository` - Main repository service class with methods for VCS operations
- Repository information interfaces

## Dependencies

- [[addons-isl-server-src-commands]] - Command execution utilities
- [[addons-isl-server-src-watchforchanges]] - File system change detection
- shared utilities - Message passing and serialization

## Used By

TBD

## Notes

Critical service that handles all interaction with Sapling CLI. Manages operation queue, caching, and file watching.
