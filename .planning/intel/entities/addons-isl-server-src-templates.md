---
path: /Users/jonas/code/sapling/addons/isl-server/src/templates.ts
type: module
updated: 2026-01-21
status: active
---

# templates.ts

## Purpose

Template generation for Sapling template language used in smartlog and diff commands. Defines field extraction patterns and output parsing logic.

## Exports

- `mainFetchTemplateFields()` - Template fields for commit fetching
- `parseCommitInfoOutput()` - Parse raw template output into CommitInfo
- `getMainFetchTemplate()` - Get full template string
- Template parsing utilities

## Dependencies

- [[addons-isl-src-types]] - CommitInfo and SmartlogCommits types

## Used By

TBD

## Notes

Handles template definition and parsing for extracting structured data from Sapling. Critical for smartlog rendering and commit metadata fetching.
