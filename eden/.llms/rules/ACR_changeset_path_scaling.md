---
oncalls: ['source_control']
apply_to_regex: 'eden/mononoke/.*\.rs$'
apply_to_content: 'changed_files|file_changes|path|paths|manifest|diff|list_all'
---

# Changeset Path Scaling

**Severity: CRITICAL**

## What to Look For

Operations that scale with O(changeset_paths) — iterating, collecting, or processing all paths in a changeset. Large directories and bulk commits (e.g., codemod commits touching hundreds of thousands of files) make this a production risk.

## When to Flag

- Collecting all changed paths into a Vec/HashMap before processing
- Iterating all paths in a changeset to filter, check, or transform them
- Loading full manifests or directory listings to compare changesets
- Passing all paths through a per-path RPC, DB lookup, or hook check without batching or pagination
- Any loop over changeset paths without a size limit, pagination, or streaming

## Do NOT Flag

- Streaming/paginated APIs that process paths in bounded chunks
- Operations that are already bounded (e.g., filtering to a known small set of paths first)
- Code that operates on a single path or a small fixed set of paths
- Test code

## Examples

**BAD (collecting all paths into memory):**
```rust
let all_paths: Vec<_> = changeset.file_changes().collect();
for path in &all_paths {
    check_hook(path).await?;
}
```

**BAD (per-path DB lookup without batching):**
```rust
for path in changeset.file_changes() {
    let metadata = db.get_file_metadata(path).await?;
    // ...
}
```

**GOOD (streaming with bounded concurrency):**
```rust
changeset
    .file_changes()
    .try_for_each_concurrent(100, |path| async move {
        check_hook(path).await
    })
    .await?;
```

**GOOD (batched DB lookups):**
```rust
for chunk in changeset.file_changes().chunks(1000) {
    let metadata = db.get_file_metadata_batch(&chunk).await?;
    // ...
}
```

## Why This Matters

Some repositories contain commits that touch hundreds of thousands of paths (codemod commits, large directory moves). Code that is O(changeset_paths) without streaming, batching, or pagination will OOM, timeout, or saturate downstream services when it hits these commits. Large directories (e.g., fbcode/third-party) compound the problem since a single directory listing can return millions of entries.

## Recommendation

Always assume a changeset can touch an unbounded number of paths. Use streaming or pagination over collecting. Batch downstream calls. Apply bounded concurrency. If you must collect paths, add a size check and fail early with a clear error rather than OOMing.
