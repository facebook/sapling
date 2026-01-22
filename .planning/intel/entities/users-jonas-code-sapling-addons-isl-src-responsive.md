---
path: /Users/jonas/code/sapling/addons/isl/src/responsive.tsx
type: util
updated: 2026-01-22
status: active
---

# responsive.tsx

## Purpose

Provides responsive layout utilities and state management for ISL's UI. Handles main content width tracking, compact rendering mode, UI zoom functionality, and narrow commit tree breakpoint detection.

## Exports

- `mainContentWidthState` - Jotai atom storing the current main content width in pixels (default 500)
- `renderCompactAtom` - Config-backed atom for compact rendering mode toggle
- `zoomUISettingAtom` - LocalStorage-backed atom for UI zoom level, syncs to CSS `--zoom` variable
- `useZoomShortcut` - Hook registering ZoomIn/ZoomOut keyboard commands
- `useMainContentWidth` - Hook returning a ref that tracks element width via ResizeObserver
- `NARROW_COMMIT_TREE_WIDTH` - Breakpoint constant (800px) for narrow commit tree
- `NARROW_COMMIT_TREE_WIDTH_WHEN_COMPACT` - Breakpoint constant (300px) when compact mode enabled
- `isNarrowCommitTree` - Derived atom computing whether commit tree should render narrow

## Dependencies

- jotai (external)
- react (external)
- [[ISLShortcuts]] - `useCommand` hook for keyboard shortcuts
- [[jotaiUtils]] - `atomWithOnChange`, `configBackedAtom`, `localStorageBackedAtom`, `readAtom`, `writeAtom`

## Used By

TBD

## Notes

- Zoom changes are applied by setting `--zoom` CSS custom property on document.body
- Width tracking uses ResizeObserver for efficient responsive updates
- Narrow breakpoint threshold changes based on compact mode state