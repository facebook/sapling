---
path: /Users/jonas/code/sapling/addons/shared/LRU.ts
type: util
updated: 2026-01-21
status: active
---

# LRU.ts

## Purpose

Least-Recently-Used cache implementation with support for immutable objects. Provides decorator-based caching for functions and methods with configurable size and statistics.

## Exports

- `LRU` - LRU cache class
- `cached()` - Function/method caching decorator
- `cachedMethod()` - Instance method caching helper
- Cache statistics tracking

## Dependencies

- immutable - Support for immutable value comparison

## Used By

TBD

## Notes

Sophisticated caching with immutable.is() comparison and collision handling. Supports audit mode for verifying cache correctness.
