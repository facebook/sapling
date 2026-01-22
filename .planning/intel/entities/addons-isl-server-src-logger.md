---
path: /Users/jonas/code/sapling/addons/isl-server/src/logger.ts
type: util
updated: 2026-01-21
status: active
---

# logger.ts

## Purpose

Logging infrastructure for ISL server. Provides standardized logging with timestamps, levels, and ANSI coloring for different log levels.

## Exports

- `Logger` - Abstract logger interface
- `StdoutLogger` - Console-based logger with ANSI colors
- Log level types

## Dependencies

None

## Used By

TBD

## Notes

Used throughout server for consistent logging output. Supports multiple implementations for different deployment environments.
