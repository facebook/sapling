---
oncalls: ['source_control']
apply_to_regex: 'eden/mononoke/.*\.rs$'
apply_to_content: 'return Err|bail!|anyhow!|reject|Reject|InvalidRequest|BadRequest'
---

# API Validation Tightening

**Severity: HIGH**

## What to Look For

- New error/rejection paths added to API handlers that previously always succeeded for valid-looking input
- Server-side enforcement of an invariant that was previously only a convention or client-side check
- Stricter input validation on existing endpoints without auditing all callers
- Tightening behavior without a feature flag for gradual per-client/per-repo rollout

## When to Flag

- Adding a `return Err(...)`, `bail!()`, or HTTP 4xx response in an API handler for a condition that was previously accepted
- New validation that rejects inputs that existing clients are known to send (e.g., no-op commits, empty pushes, duplicate file entries)
- Behavior changes rolled out all-at-once without JustKnobs or Configerator gating
- Validation that only applies to the "new" behavior but not explicitly documented for callers

## Do NOT Flag

- Validation on brand-new API endpoints (no existing clients)
- Bug fixes where the old behavior was clearly wrong and documented as a known issue
- Validation behind an existing, already-enabled feature flag
- Input validation that catches truly malformed requests (e.g., null required fields, invalid UTF-8)

## Examples

**BAD (breaking existing clients — matches S578742):**
```rust
pub async fn handle_write(req: WriteRequest) -> Result<Response> {
    // NEW: reject writes where file content didn't actually change
    for file in &req.files {
        if file.old_content == file.new_content {
            return Err(Error::NoOpChange(file.path.clone()));
        }
    }
    // Clients that relied on no-op writes succeeding now get errors.
    // 368 writes failed over 4 hours before this was caught.
    do_write(req).await
}
```

**GOOD (gated rollout):**
```rust
pub async fn handle_write(req: WriteRequest) -> Result<Response> {
    let reject_noops = tunables()
        .get_bool("reject_noop_file_changes")
        .unwrap_or(false);

    if reject_noops {
        for file in &req.files {
            if file.old_content == file.new_content {
                return Err(Error::NoOpChange(file.path.clone()));
            }
        }
    }
    do_write(req).await
}
// Roll out per-repo: enable for a test repo first, then gradually expand.
// If breakage is detected, flip the knob off instantly.
```

## Recommendation

When adding new validation to an existing API endpoint, always gate it behind JustKnobs or a per-client/per-repo Configerator flag. Before enabling, audit callers to see if any rely on the currently-accepted behavior. Start with logging-only mode (log violations without rejecting) to measure impact, then enable rejection gradually. The pattern is: log first → enable for test repos → canary → full rollout.

## Evidence

- **S578742**: Config store backend added validation rejecting no-op file changes. Technically correct, but clients relied on the lenient behavior. 368 writes failed over 4 hours. No feature flag for gradual rollout or quick rollback.
