---
oncalls: ['source_control']
apply_to_regex: 'eden/(mononoke|scm)/.*\.rs$'
apply_to_content: 'join_all|spawn|FuturesUnordered|for_each_concurrent|buffer_unordered|tokio::spawn'
---

# Unbounded Concurrency and Backlog Stampede

**Severity: HIGH**

## What to Look For

- `futures::future::join_all()` on an unbounded collection
- `FuturesUnordered` without a concurrency limit
- `tokio::spawn` in a loop without a semaphore
- `StreamExt::for_each_concurrent(None, ...)` — unlimited parallelism
- Backlog drain after recovery: queued work released all-at-once without throttling
- Rate limit code with exemptions for "expired" or "default" states that effectively bypass the limit

## When to Flag

- `join_all(items.iter().map(|i| fetch(i)))` where `items` is user-controlled or repository-scale
- `for_each_concurrent(None, ...)` on streams of unbounded size
- Spawning tasks in a loop without `Semaphore::acquire()`
- Queue consumers that drain backlogs without a max-batch-size or draining rate
- Rate limiting logic that skips checks when config/override is "expired" (should deny, not exempt)
- Per-request data fetches without a size/memory budget (see S552025: single 24GB commit OOM'd servers)

## Do NOT Flag

- `join_all()` on a small, fixed set of futures (e.g., 2-3 known operations)
- `buffer_unordered(N)` or `buffered(N)` with a concrete limit
- Bounded channels (`mpsc::channel(N)`) used for backpressure
- Test code that exercises the fan-out path

## Examples

**BAD (unbounded fan-out):**
```rust
let results = futures::future::join_all(
    changeset_ids.iter().map(|cs| fetch_changeset(ctx, repo, *cs))
).await;
// If changeset_ids has 100K entries, this spawns 100K concurrent fetches
```

**GOOD (bounded):**
```rust
let results: Vec<_> = stream::iter(changeset_ids)
    .map(|cs| fetch_changeset(ctx, repo, cs))
    .buffer_unordered(100)
    .try_collect()
    .await?;
```

**BAD (backlog stampede — matches S493741 pattern):**
```rust
// After upstream SEV is mitigated, all queued jobs resume at once
while let Some(job) = backlog_queue.pop() {
    tokio::spawn(process_job(job));
}
```

**GOOD (graduated drain):**
```rust
let drain_semaphore = Semaphore::new(50); // max 50 concurrent during drain
while let Some(job) = backlog_queue.pop() {
    let permit = drain_semaphore.acquire().await?;
    tokio::spawn(async move {
        let _permit = permit;
        process_job(job).await
    });
}
```

**BAD (rate limit bypass — matches S498806 pattern):**
```rust
fn should_rate_limit(override_config: &Override) -> bool {
    if override_config.is_expired() {
        return false; // Expired override = no limit. WRONG: lets unlimited traffic through.
    }
    check_rate(override_config.limit())
}
```

## Recommendation

Use `StreamExt::buffer_unordered(N)` or `Semaphore` to cap concurrency. Choose N based on downstream capacity (50-200 for blobstore, 10-50 for SQL). After an outage or backlog buildup, drain queues gradually — never release the entire backlog at once. Add per-request memory budgets for data fetches: if a single request would fetch more than X MB, reject it early rather than OOM.

## Evidence

- **S493741**: When upstream SEV S493707 was mitigated, the released backlog of deferred `commit_location_to_hash` calls overloaded Mononoke's MySQL backend.
- **S498806**: Expired Configerator rate limit overrides bypassed throttling, allowing unbounded commit rate that created a derivation backlog lasting hours.
- **S617619**: Merge driver executed `sl diff` between rebase source and destination without scoping to relevant files, causing 5x LFS traffic spike and OOM-based load shedding.
- **S552025**: Single 24GB commit fetched by multiple systems caused OOMs — no per-request memory budget.
