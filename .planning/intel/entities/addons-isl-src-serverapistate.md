---
path: /Users/jonas/code/sapling/addons/isl/src/serverAPIState.ts
type: module
updated: 2026-01-21
status: active
---

# serverAPIState.ts

## Purpose

Jotai atoms for managing subscriptions and data fetched from ISL server. Maintains smartlog commits, uncommitted changes, bookmarks, and other repository state.

## Exports

- `smartlogCommitsAtom` - Jotai atom for commits in smartlog
- `uncommittedChangesAtom` - Working directory changes
- `bookmarksAtom` - Repository bookmarks
- Various subscription atoms

## Dependencies

- [[addons-isl-src-types]] - Server data types
- jotai - State management

## Used By

TBD
