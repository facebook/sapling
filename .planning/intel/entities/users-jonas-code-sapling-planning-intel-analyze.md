---
path: /Users/jonas/code/sapling/.planning/intel/analyze.js
type: util
updated: 2026-01-21
status: active
---

# analyze.js

## Purpose

CLI script that analyzes JavaScript/TypeScript codebases to extract exports, imports, and detect code conventions. Used for generating codebase intelligence by scanning files and identifying patterns like export declarations, import statements, and directory purposes.

## Exports

- `EXTENSIONS` - Array of supported file extensions (.js, .ts, .jsx, .tsx, .mjs, .cjs)
- `EXCLUDE_DIRS` - Array of directories to skip during analysis
- `EXPORT_PATTERNS` - Regex patterns for detecting various export syntaxes
- `IMPORT_PATTERNS` - Regex patterns for detecting import/require statements
- `DIR_PURPOSES` - Mapping of directory names to their semantic purposes
- `SUFFIX_PURPOSES` - Mapping of file suffixes to their semantic purposes

## Dependencies

- fs (Node.js built-in)
- path (Node.js built-in)

## Used By

TBD

## Notes

- Supports both ES6 modules and CommonJS export/import patterns
- Contains ISL-specific directory mappings (dag, stackEdit, ComparisonView, etc.)
- Designed to run as a standalone CLI script (has shebang)