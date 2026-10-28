---
name: rederivation-safety
metadata:
  oncalls: ['scm_server_infra']
  strict: true
  apply_to_path: 'eden/mononoke/.*\.rs$'
  apply_to_content: 'rederiv|backfill|Rederivation|blobstore.*put.*behaviour|OverwriteAndLog|cache.mode|derive_bulk|AlwaysRederive|manifold.weak.consistency'
---

# Rederivation and Backfilling Safety

**Severity: HIGH**

## What to Look For

- Code that rederives already-derived data without special blobstore/cache handling
- Backfill logic that assumes blobstore `put` will overwrite existing data
- Rederivation paths that don't disable caching
- Selective rederivation from a specific commit without considering interruption scenarios


## When to Flag

- A rederivation path that calls `blobstore.put()` without `OverwriteAndLog` put behaviour -- the default blobstore `put` is a no-op if the key already exists
- Rederivation code that doesn't disable caches (`cache-mode=disabled`) -- stale cached results will mask the new derivations
- Partial rederivation logic (e.g., "rederive from commit X to heads") without documenting what happens if the process is interrupted midway -- you may end up with an inconsistent mix of old and new derivations with no clear boundary
- Adding rederivation support to a service (e.g., async requests worker) without plumbing through blobstore put behaviour, cache mode, and manifold weak consistency flags

## Do NOT Flag

- Normal first-time derivation or backfilling of underived commits
- Code that changes the blobstore key prefix to force fresh derivation (this is the safe approach)
- Test code using in-memory blobstores (put always overwrites there)

## Examples

**BAD (rederivation without overwrite):**
```rust
// Rederive all descendants of bad_commit
manager.derive_bulk_locally(ctx, &cs_ids, Some(Arc::new(AlwaysRederive)), &[dt], None).await?;
// BUG: blobstore.put() is a no-op for existing keys in production!
```

**GOOD (change prefix instead of rederiving in place):**
```rust
// Increment the mapping key prefix version to force fresh derivation
// Old: "derived_root_fsnode.v1.{changeset_id}"
// New: "derived_root_fsnode.v2.{changeset_id}"
```

**GOOD (if rederivation is truly needed, use correct flags):**
```rust
// Production rederivation requires all of:
// --blobstore-put-behaviour=OverwriteAndLog
// --cache-mode=disabled
// --manifold-weak-consistency-ms=0
```
