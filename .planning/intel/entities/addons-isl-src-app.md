---
path: /Users/jonas/code/sapling/addons/isl/src/App.tsx
type: component
updated: 2026-01-21
status: active
---

# App.tsx

## Purpose

Root React component for ISL that orchestrates the entire application UI. Manages connection to server, repository state, and coordinates rendering of smartlog, commits, uncommitted changes, and operation UI layers.

## Exports

- `App()` - Main application component

## Dependencies

- [[addons-isl-src-types]] - TypeScript type definitions for ISL data
- [[addons-isl-src-operationsstate]] - Operations state management
- [[addons-isl-src-serverapistate]] - Server API state and subscription management
- [[addons-isl-src-renderdag]] - DAG visualization component
- [[addons-isl-src-commit]] - Commit rendering component
- [[addons-isl-src-uncommittedchanges]] - Uncommitted changes component
- react - UI component library
- jotai - Lightweight state management

## Used By

TBD

## Notes

Central hub for ISL application that wires together server communication, state management, and UI components. Uses Jotai atoms for reactive state management across the application.
