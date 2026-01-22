---
path: /Users/jonas/code/sapling/addons/isl/src/types.ts
type: model
updated: 2026-01-21
status: active
---

# types.ts

## Purpose

Defines all TypeScript type definitions and interfaces used throughout ISL. Covers data models for commits, operations, diffs, repository state, and client-server messaging contracts.

## Exports

- `CommitInfo` - Information about a single commit in the smartlog
- `SmartlogCommits` - Array of commits with repository history
- `UncommittedChanges` - Uncommitted file changes in working directory
- `Operation` - VCS operation definition with arguments and tracking
- `Disposable` - Resource cleanup interface
- `ClientToServerMessage` - Client message types sent to ISL server
- `ServerToClientMessage` - Server message types sent to client
- And 50+ other types for repository state, diffs, bookmarks, etc.

## Dependencies

None

## Used By

TBD

## Notes

Central type definition file that serves as the contract between client and server. All major data structures pass through these types.
