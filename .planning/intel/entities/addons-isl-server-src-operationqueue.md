---
path: /Users/jonas/code/sapling/addons/isl-server/src/OperationQueue.ts
type: service
updated: 2026-01-21
status: active
---

# OperationQueue.ts

## Purpose

Queue management for VCS operations ensuring only one operation runs at a time. Handles queueing, progress tracking, and operation cancellation.

## Exports

- `OperationQueue` - Operation queue manager class

## Dependencies

- [[addons-isl-src-types]] - Operation types
- shared utilities - Promise utilities

## Used By

TBD

## Notes

Serializes operations to prevent conflicts and ensure consistent repository state. Tracks operation progress and handles user-initiated aborts.
