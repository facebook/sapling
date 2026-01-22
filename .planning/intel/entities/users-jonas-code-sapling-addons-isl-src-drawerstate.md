---
path: /Users/jonas/code/sapling/addons/isl/src/drawerState.ts
type: config
updated: 2026-01-22
status: active
---

# drawerState.ts

## Purpose

Defines the persistent state atom for ISL's drawer UI panels (right, left, top, bottom). Stores drawer sizes and collapsed states in localStorage for persistence across sessions.

## Exports

- `islDrawerState` - Jotai atom backed by localStorage containing size and collapsed state for all four drawer positions (right, left, top, bottom)

## Dependencies

- [[jotaiUtils]] - provides `localStorageBackedAtom` for persistent state
- `AllDrawersState` type from [[Drawers]]

## Used By

TBD

## Notes

- Default right drawer width is 500px; other drawers default to 200px
- Left, top, and bottom drawers start collapsed by default; right drawer starts expanded
- Uses localStorage key `'isl.drawer-state'` for persistence