---
path: /Users/jonas/code/sapling/addons/components/theme/tokens.stylex.ts
type: config
updated: 2026-01-21
status: active
---

# tokens.stylex.ts

## Purpose

Defines StyleX theme variables for the ISL UI, providing design tokens for colors, spacing, typography, and border radii. Supports both dark (default) and light themes with CSS custom property integration for VS Code compatibility.

## Exports

- `colors` - StyleX vars for dark theme colors (backgrounds, foregrounds, status colors, semantic colors)
- `light` - StyleX theme override for light mode, remapping color values
- `spacing` - StyleX vars for consistent spacing scale (none through xxxlarge)
- `radius` - StyleX vars for border radius values (small, round, extraround, full)
- `font` - StyleX vars for relative font sizes (smaller through bigger)
- `graphiteColors` - StyleX vars for Graphite-inspired deep navy UI palette
- `layoutSpacing` - StyleX vars for layout-specific spacing (truncated in source)

## Dependencies

- @stylexjs/stylex (external)

## Used By

TBD

## Notes

- Default theme is dark; light theme applies overrides via `stylex.createTheme`
- Colors reference CSS custom properties (e.g., `var(--background)`) for VS Code theming integration
- Includes semantic color tokens for git status (modified, added, removed, missing) and signals (good, medium, bad)