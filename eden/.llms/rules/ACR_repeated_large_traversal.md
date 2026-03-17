---
oncalls: ['source_control']
apply_to_regex: 'eden/(mononoke|scm)/.*\.rs$'
apply_to_content: 'for_each|iter\(\)|collect|manifest|file_changes|changed_files|traverse|walk'
---

# Repeated Large Traversal

**Severity: CRITICAL**

## What to Look For

- Collecting file changes from a commit then iterating over the collection multiple times for different purposes instead of a single pass
- Walking the same commit graph segment or manifest tree twice when one traversal suffices
- Sequential manifest loading across a commit range -- loading manifests one by one in a loop instead of using batch derivation
- Re-fetching data in an inner loop that was already available from an outer scope

## When to Flag

- `let changes: Vec<_> = cs.file_changes().collect()` followed by multiple `for change in &changes` or `.iter().filter(...)` passes that could be merged
- Two separate traversals of the same commit range (e.g., one to collect IDs, another to load data for those IDs)
- `for cs_id in commit_range { derive::<RootManifestId>(ctx, repo, cs_id).await? }` -- sequential manifest derivation instead of batch
- Loading a changeset or manifest inside an inner loop when the same data was fetched (or could have been fetched) in an outer loop or earlier scope
- Calling `.file_changes()` or `.changed_files()` on the same bonsai changeset multiple times

## Do NOT Flag

- Multiple passes over a small, fixed-size collection (e.g., 2-3 elements)
- Separate traversals that genuinely need different starting points or parameters
- Cases where the second traversal depends on results computed during the first (true data dependency)
- Test code

## Examples

**BAD (multiple passes over file changes):**
```rust
let changes: Vec<_> = bonsai.file_changes().collect();

// Pass 1: check for large files
for (path, change) in &changes {
    if let Some(fc) = change.simplify() {
        check_size(fc.size())?;
    }
}

// Pass 2: check for restricted paths
for (path, change) in &changes {
    check_restricted_path(path)?;
}
```

**GOOD (single pass):**
```rust
for (path, change) in bonsai.file_changes() {
    check_restricted_path(path)?;
    if let Some(fc) = change.simplify() {
        check_size(fc.size())?;
    }
}
```

**BAD (sequential manifest derivation -- matches real performance issues):**
```rust
for cs_id in &commit_ids {
    let mf_id = repo
        .derive::<RootManifestId>(ctx, *cs_id)
        .await?;
    process_manifest(mf_id).await?;
}
```

**GOOD (batch derivation):**
```rust
let mf_ids = repo
    .derive_batch::<RootManifestId>(ctx, commit_ids.clone())
    .await?;
for mf_id in mf_ids {
    process_manifest(mf_id).await?;
}
```

Or for linear stacks:
```rust
derive_manifests_for_simple_stack_of_commits(ctx, repo, commit_ids).await?;
```

**BAD (re-fetching in inner loop):**
```rust
for cs_id in &ancestors {
    let bonsai = cs_id.load(ctx, repo.repo_blobstore()).await?;
    for (path, _change) in bonsai.file_changes() {
        // Fetches the same manifest for cs_id again inside the loop
        let mf = repo.derive::<RootManifestId>(ctx, *cs_id).await?;
        let entry = mf.find_entry(ctx, repo.repo_blobstore(), path.clone()).await?;
        process(entry)?;
    }
}
```

**GOOD (fetch once, reuse):**
```rust
for cs_id in &ancestors {
    let bonsai = cs_id.load(ctx, repo.repo_blobstore()).await?;
    let mf = repo.derive::<RootManifestId>(ctx, *cs_id).await?;
    let paths: Vec<_> = bonsai.file_changes().map(|(p, _)| p.clone()).collect();
    let entries = mf.find_entries(ctx, repo.repo_blobstore(), paths).await?;
    for entry in entries {
        process(entry)?;
    }
}
```

## Recommendation

Merge multiple passes over the same collection into a single loop. Use `derive_batch` or `derive_manifests_for_simple_stack_of_commits` instead of deriving manifests one at a time in a loop. Hoist data fetches out of inner loops when they depend only on outer-loop variables. Use `find_entries` for batch path lookup instead of calling `find_entry` in a loop.
