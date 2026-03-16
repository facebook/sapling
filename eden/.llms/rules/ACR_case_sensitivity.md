---
oncalls: ['source_control']
apply_to_regex: 'eden/(scm|fs)/.*\.rs$'
apply_to_content: 'sort|dedup|DirEntry|inode|tree_entry|manifest|case.insensitive|case_insensitive'
---

# Cross-Platform Path Case Sensitivity

**Severity: HIGH**

## What to Look For

- Sorting or dedup optimizations in directory listing or tree manifest code
- Data structure substitutions (sorted insert → bulk build, BTreeMap → HashMap) in path-handling code
- Code that compares or deduplicates file paths without considering case-insensitive filesystems (macOS, Windows)
- Persistent cache writes (RocksDB, disk) that store the result of path operations without a cache version

## When to Flag

- Removing or replacing a `sort()` or `sort_by()` call in directory entry construction
- Switching from `BTreeMap` (ordered) to `HashMap` (unordered) for path → entry mappings
- Dedup logic that uses byte-exact comparison but feeds a case-insensitive filesystem
- Changes to tree/manifest construction that alter which entry "wins" when two paths differ only in case
- Persistent cache writes of path-derived data without a schema/version field for invalidation

## Do NOT Flag

- Sorting changes in test code or non-path-related data
- Code explicitly annotated with `// case-sensitive only` or gated on platform checks
- In-memory caches that are cleared on restart (self-healing)
- Linux-only code paths (case-sensitive by default)

## Examples

**BAD (optimization changes collision resolution — matches S566000):**
```rust
// "Optimization": build Vec directly instead of sorted insertion
fn build_tree_entries(raw: Vec<(PathComponent, TreeEntry)>) -> Vec<TreeEntry> {
    raw.into_iter().map(|(_, entry)| entry).collect()
    // Without sorting, which entry wins for "README" vs "readme" is
    // now dependent on iteration order, not deterministic.
    // Result gets written to RocksDB → corruption persists after rollback.
}
```

**GOOD (deterministic ordering, case-aware dedup):**
```rust
fn build_tree_entries(mut raw: Vec<(PathComponent, TreeEntry)>) -> Vec<TreeEntry> {
    // Sort deterministically, then dedup case-insensitively for macOS/Windows
    raw.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    raw.dedup_by(|a, b| a.0.eq_ignore_ascii_case(&b.0));
    raw.into_iter().map(|(_, entry)| entry).collect()
}
```

## Recommendation

When optimizing path-handling or tree construction code, always verify behavior on case-insensitive filesystems. Add tests for case collision scenarios ("foo" vs "Foo"). If results are written to a persistent cache (RocksDB, on-disk), include a schema version so corrupted entries can be detected and re-derived on upgrade. Be especially cautious with optimizations that change ordering — the performance win is rarely worth the correctness risk.

## Evidence

- **S566000**: EdenFS optimization changed how directory entry vectors were constructed, altering which entry "won" during case-insensitive collisions. Corrupted entries were persisted to RocksDB, making code rollback insufficient. Caused phantom file modifications on macOS and Windows checkouts.
