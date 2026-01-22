---
path: /Users/jonas/code/sapling/addons/shared/debounce.ts
type: util
updated: 2026-01-21
status: active
---

# debounce.ts

## Purpose

Rate-limiting utility for delaying function execution until a repeated action completes. Common pattern for event handlers like typing, resizing, and scrolling.

## Exports

- `debounce()` - Create debounced function with reset/isPending methods
- `DebouncedFunction` - Type for debounced function

## Dependencies

None

## Used By

TBD

## Notes

Returns function with additional methods for controlling pending invocations and cleanup. Supports leading/trailing edge execution modes.
