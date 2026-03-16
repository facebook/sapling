---
oncalls: ['source_control']
apply_to_regex: 'eden/mononoke/.*\.rs$'
apply_to_content: 'memcache|cachelib|CacheKey|cache_key|blobstore|shard|routing|RocksDB'
---

# Cache and Routing State Invalidation

**Severity: HIGH**

## What to Look For

- Changes to cache key format or components without a version bump
- Mutations that don't invalidate the corresponding cache entry
- Blobstore reads that panic on `None` instead of treating as a cache miss
- Sorting or ordering optimizations that change tie-breaking in cached data structures
- Persistent caches (RocksDB, on-disk) storing derived state without a cache invalidation/migration path
- Shard/routing caches using polling instead of event-based invalidation

## When to Flag

- Cache key construction changes without bumping the key version/prefix
- A write path that doesn't invalidate or update the cache
- `blobstore.get().await?.unwrap()` without handling `None`
- **Optimizations that change ordering in cached data** — e.g., switching from sorted-insert to bulk-build changes which entry "wins" on collision (see S566000)
- Persistent cache writes (RocksDB, disk) of derived/transformed data without a version field — rollback won't fix corrupted entries
- Shard assignment caches without subscription-based invalidation or TTL bounds

## Do NOT Flag

- Cache key changes that include a version bump or namespace change
- Read-through caches where the miss path is explicitly handled
- Test-only cache implementations or mock blobstores
- Small, bounded metadata reads (under 1 KB)
- In-memory caches that are cleared on restart (corruption is self-healing)

## Examples

**BAD (key format change without version):**
```rust
// Before: format!("cs:{}", cs_id)
// After:  format!("cs:{}:{}", repo_id, cs_id)
fn cache_key(repo_id: &RepoId, cs_id: &ChangesetId) -> String {
    format!("cs:{}:{}", repo_id, cs_id)
}
```

**GOOD (versioned key):**
```rust
fn cache_key(repo_id: &RepoId, cs_id: &ChangesetId) -> String {
    format!("cs:v2:{}:{}", repo_id, cs_id)
}
```

**BAD (ordering change corrupts persistent cache — matches S566000):**
```rust
// "Optimization": build Vec directly instead of inserting into sorted structure.
// Changes which entry wins during case-insensitive collisions ("foo" vs "Foo").
// Corrupted results get written to RocksDB, persisting across code rollback.
fn build_dir_entries(entries: Vec<DirEntry>) -> Vec<DirEntry> {
    entries  // No re-sort! Collision winner changes vs old sorted-insert path.
}
```

**GOOD (preserve deterministic ordering):**
```rust
fn build_dir_entries(mut entries: Vec<DirEntry>) -> Vec<DirEntry> {
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    entries
}
```

**BAD (shard routing with polling — matches S556044):**
```rust
// Client-side cache of shard assignments, refreshed by polling every 60s.
// During rolling deploys, shard moves happen faster than the poll interval.
let shard_map = shard_manager_client.get_shard_map().await?;
// Stale for up to 60s — requests land on wrong servers, get "repo not found" (403).
```

## Recommendation

Always include a version component in cache keys. When changing key format, bump the version. For persistent caches (RocksDB, disk), include a schema version and validate on read — if the version doesn't match, treat as a miss and re-derive. Never optimize away sort/dedup steps in code that feeds a persistent cache. For shard/routing caches, use event-based invalidation (subscriptions) instead of polling, and return 503 (retriable) not 403 when a shard isn't loaded.

## Evidence

- **S566000**: EdenFS optimization removed O(n*log(n)) re-insertion in directory entry construction. Changed which entry "won" during case-insensitive collisions. Corrupted data persisted in RocksDB, so code rollback alone didn't fix it.
- **S556044**: ShardManager client-side cache published stale shard maps during rolling deploys. Requests routed to wrong servers, returned 403 instead of retriable 5xx. Recurred 4+ times across services (S525475, S609287).
