---
path: /Users/jonas/code/sapling/addons/shared/Comparison.ts
type: model
updated: 2026-01-21
status: active
---

# Comparison.ts

## Purpose

Type definitions and utilities for diff comparisons. Defines comparison modes (uncommitted changes, head changes, committed changes) and helpers for revset generation.

## Exports

- `ComparisonType` - Enum of comparison types
- `Comparison` - Discriminated union type for comparisons
- `revsetArgsForComparison()` - Generate Sapling revset arguments
- `beforeRevsetForComparison()` - Get "before" revision
- `currRevsetForComparison()` - Get "current" revision
- `labelForComparison()` - Human-readable label

## Dependencies

None

## Used By

TBD
