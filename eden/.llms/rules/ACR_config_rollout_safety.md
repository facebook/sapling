---
oncalls: ['source_control']
apply_to_regex: 'eden/(mononoke|scm)/.*(\.rs|\.thrift|\.cconf|\.cinc)$'
apply_to_content: 'config|ConfigValue|tunables|justknobs|JustKnobs|rate.limit|RateLimit'
---

# Configuration and Rate Limit Rollout Safety

**Severity: MEDIUM**

## What to Look For

- New config values without a sensible default when config fetch fails
- All-or-nothing config changes that can't be canaried
- Hardcoded values that should be in Configerator or JustKnobs
- Rate limiting logic that exempts/skips checks for "expired" or "default" or "unknown" states
- New server-side validation or rejection paths in previously-infallible APIs without a feature flag

## When to Flag

- Code that calls `config.get("key")` without a fallback/default value
- New boolean configs defaulting to `true` for a destructive or restricting behavior
- Magic numbers in server code that should be tunable (timeouts, batch sizes, limits)
- Rate limit code that returns "allow" when override is expired/missing — should return "deny" or "use default limit"
- Adding a new `return Err(...)` in an API handler that previously always succeeded, without gating behind a JustKnobs flag (see S578742)

## Do NOT Flag

- Config reads in test code or test fixtures
- CLI tool flags (not server configs)
- Environment variables for local development settings
- Configs with explicit defaults documented in the config schema
- New validation on brand-new API methods (no existing clients to break)
- **JustKnobs reads using `?` without a fallback** — for `justknobs::eval()` / `justknobs::get()` / `justknobs::get_as()`, propagating the error via `?` is the correct pattern. Do NOT suggest `.unwrap_or(default)` — that hides misconfiguration and is expensive for non-existent knobs. Defaults belong in `just_knobs.json`. See D92827579.

## Examples

**BAD (rate limit bypass — matches S498806):**
```rust
fn check_rate_limit(config: &RateLimitConfig) -> bool {
    if config.override_entry.is_expired() {
        return true; // "expired = no limit" lets traffic through unbounded
    }
    config.current_rate() < config.limit()
}
```

**GOOD (deny on unknown state):**
```rust
fn check_rate_limit(config: &RateLimitConfig) -> bool {
    let limit = match &config.override_entry {
        Some(entry) if !entry.is_expired() => entry.limit(),
        _ => config.default_limit(), // Fall back to default, never bypass
    };
    config.current_rate() < limit
}
```

**BAD (new validation without flag — matches S578742):**
```rust
pub async fn handle_commit(req: CommitRequest) -> Result<Response> {
    if req.files.iter().all(|f| f.old_content == f.new_content) {
        return Err(Error::NoOpCommit); // NEW: rejects no-op commits
        // But 368 existing clients rely on no-op commits succeeding!
    }
    do_commit(req).await
}
```

**GOOD (gated validation):**
```rust
pub async fn handle_commit(req: CommitRequest) -> Result<Response> {
    if tunables().get_bool("reject_noop_commits").unwrap_or(false)
        && req.files.iter().all(|f| f.old_content == f.new_content)
    {
        return Err(Error::NoOpCommit);
    }
    do_commit(req).await
}
```

## Recommendation

Always provide safe defaults for config values. New configs should default to current behavior (off for new features, current values for tuning knobs). Rate limiting must "fail closed" — if the override/config state is unknown, expired, or missing, apply the default limit, never bypass it. When adding new validation/rejection to an existing API, gate it behind JustKnobs so you can roll it out per-client or per-repo and roll back instantly.

## Evidence

- **S498806**: Expired Configerator rate limit overrides bypassed throttling entirely, allowing unbounded commit rate and hours-long derivation backlog.
- **S578742**: Backend added stricter validation (rejecting no-op file changes) that was technically correct but broke clients relying on the previous lenient behavior. 368 writes failed over 4 hours. No feature flag for gradual rollout.
