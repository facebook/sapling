---
path: /Users/jonas/code/sapling/addons/shared/RateLimiter.ts
type: util
updated: 2026-01-21
status: active
---

# RateLimiter.ts

## Purpose

Rate limiter for controlling concurrent async task execution. Queues tasks and runs them with configurable concurrency limit.

## Exports

- `RateLimiter` - Rate limiter class

## Dependencies

- [[addons-shared-typedeventemitter]] - Event emitting for task lifecycle

## Used By

TBD

## Notes

Useful for limiting concurrent API requests or database operations. Tracks running tasks and notifies when slots become available.
