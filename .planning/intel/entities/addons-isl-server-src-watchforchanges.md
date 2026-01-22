---
path: /Users/jonas/code/sapling/addons/isl-server/src/WatchForChanges.ts
type: service
updated: 2026-01-21
status: active
---

# WatchForChanges.ts

## Purpose

File system watcher service using Watchman and EdenFS to detect repository changes and trigger smartlog/status updates. Implements intelligent polling with adaptive intervals based on page focus.

## Exports

- `WatchForChanges` - File system watching service

## Dependencies

- [[addons-isl-server-src-utils]] - Utility functions
- watchman - File system monitoring
- edenfs - Virtual filesystem change notifications

## Used By

TBD

## Notes

Balances resource usage by adjusting polling frequency based on UI visibility and focus state. Supports both Watchman and EdenFS as fallback mechanisms.
