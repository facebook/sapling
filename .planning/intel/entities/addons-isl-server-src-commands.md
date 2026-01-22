---
path: /Users/jonas/code/sapling/addons/isl-server/src/commands.ts
type: module
updated: 2026-01-21
status: active
---

# commands.ts

## Purpose

Utility functions for executing Sapling CLI commands with proper configuration, environment setup, and error handling. Handles command serialization and result parsing.

## Exports

- `runCommand()` - Execute arbitrary Sapling command
- `findRoot()` - Find repository root directory
- `findDotDir()` - Find Sapling metadata directory
- `getConfigs()` - Read repository configuration
- `setConfig()` - Write repository configuration

## Dependencies

- [[addons-isl-server-src-utils]] - CLI utilities
- shared utilities - JSON parsing

## Used By

TBD

## Notes

Abstraction layer over direct CLI execution. Manages command environment, timeout handling, and consistent error reporting.
