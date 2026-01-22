---
path: /Users/jonas/code/sapling/addons/isl/src/Drawers.tsx
type: component
updated: 2026-01-22
status: active
---

# Drawers.tsx

## Purpose

Implements a resizable drawer layout system for ISL with support for four-sided drawers (left, right, top, bottom). Provides drag-to-resize functionality, collapsible panels, and responsive auto-collapse behavior based on viewport size.

## Exports

- `Drawers` - Main container component that renders up to four side drawers around central content
- `Drawer` - Individual drawer panel component with resize handle and collapse/expand functionality
- `AllDrawersState` - Type representing state for all four drawer sides
- `DrawerState` - Type for single drawer state (size and collapsed boolean)
- `ErrorBoundaryComponent` - Type for error boundary component class used to wrap drawer content

## Dependencies

- `jotai` - State management (useAtom)
- `react` - Core React hooks and createElement
- `shared/debounce` - Debounce utility for resize handling
- [[drawerState]] - Drawer state atoms (islDrawerState, autoCollapsedState, responsiveState)
- `./Drawers.css` - Drawer styling

## Used By

TBD

## Notes

- Uses CSS custom properties for dynamic sizing (`--drawer-right-size`, etc.)
- Implements sticky collapse behavior when drawer is resized below `stickyCollapseSizePx` (60px)
- Minimum drawer size is enforced at `minDrawerSizePx` (100px)
- Auto-collapse feature uses `useAutoCollapseDrawers` hook to respond to viewport breakpoints
- Drawer resize is handled via pointer events with debounced state updates for performance