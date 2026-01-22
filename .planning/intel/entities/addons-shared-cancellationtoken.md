---
path: /Users/jonas/code/sapling/addons/shared/CancellationToken.ts
type: util
updated: 2026-01-21
status: active
---

# CancellationToken.ts

## Purpose

Cancellation token for signaling async operation cancellation. Similar to AbortSignal in browser APIs. Allows caller to cancel work and implementation to respond.

## Exports

- `CancellationToken` - Cancellation token class

## Dependencies

None

## Used By

TBD

## Notes

Useful for cancelling long-running operations. Can be polled or used with callbacks. Only supports single cancellation.
