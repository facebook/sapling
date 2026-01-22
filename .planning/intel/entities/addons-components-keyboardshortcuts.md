---
path: /Users/jonas/code/sapling/addons/components/KeyboardShortcuts.tsx
type: util
updated: 2026-01-21
status: active
---

# KeyboardShortcuts.tsx

## Purpose

Keyboard shortcut handling infrastructure. Creates type-safe command dispatchers for keyboard-triggered actions with modifier support.

## Exports

- `makeCommandDispatcher()` - Create typed command system with keyboard handling
- `Modifier` - Keyboard modifier enum (Shift, Ctrl, Alt, Cmd)
- `KeyCode` - Key code constants

## Dependencies

- react - Hooks for command registration

## Used By

TBD

## Notes

Provides type-safe keyboard command system. Prevents shortcuts during text input. Supports bitwise modifier combinations.
