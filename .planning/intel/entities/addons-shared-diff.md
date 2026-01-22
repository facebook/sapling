---
path: /Users/jonas/code/sapling/addons/shared/diff.ts
type: util
updated: 2026-01-21
status: active
---

# diff.ts

## Purpose

Line-based diff algorithm implementation using Myers' algorithm. Calculates differences between two versions of text with context line support.

## Exports

- `diffLines()` - Calculate line differences between texts
- `diffBlocks()` - Get difference blocks with line ranges
- `readableDiffBlocks()` - Human-readable diff prioritizing significant lines
- `collapseContextBlocks()` - Collapse unchanged context regions
- `mergeBlocks()` - Merge two diffs

## Dependencies

- diff-sequences - Myers' diff algorithm implementation

## Used By

TBD

## Notes

Sophisticated diff engine that optimizes for human readability by detecting significant vs insignificant lines (whitespace, braces, etc).
