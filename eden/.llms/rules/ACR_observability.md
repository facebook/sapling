---
oncalls: ['source_control']
apply_to_regex: 'eden/(mononoke|scm)/.*\.rs$'
apply_to_content: 'pub async fn handle|pub fn handle|OdsCounter|perf_counters|scuba|PerfCounterType|kill.switch|feature.flag'
---

# Observability and Operational Readiness

**Severity: MEDIUM**

## What to Look For

- New request handlers or code paths without ODS counters or Scuba logging
- Error cases that log generic messages without request context (request ID, repo name, path)
- New features without a kill switch or feature flag for quick rollback
- SLO-critical paths without latency tracking (perf counters or Scuba samples)

## When to Flag

- A new `pub async fn` handler that doesn't increment any ODS counter
- Error branches that use `error!("failed")` without structured fields (repo, request_id, etc.)
- New functionality behind no feature flag that could break production
- Missing graceful degradation on dependency failures (e.g., cache miss escalates to full error)

## Do NOT Flag

- Internal helper functions called by an already-instrumented handler
- Test utilities or CLI tools
- Pure data transformations with no I/O
- Code that adds to an existing instrumented pipeline (parent already counts)

## Examples

**BAD (no observability):**
```rust
pub async fn handle_lookup(ctx: &CoreContext, params: LookupParams) -> Result<Response> {
    let result = do_lookup(ctx, &params).await?;
    Ok(Response::from(result))
}
```

**GOOD (instrumented):**
```rust
pub async fn handle_lookup(ctx: &CoreContext, params: LookupParams) -> Result<Response> {
    ctx.perf_counters().increment_counter(PerfCounterType::LookupCalls);
    let result = do_lookup(ctx, &params).await.with_context(|| {
        format!("lookup failed for repo={} path={}", params.repo, params.path)
    })?;
    ctx.perf_counters().increment_counter(PerfCounterType::LookupSuccess);
    Ok(Response::from(result))
}
```

## Recommendation

Every new request handler should increment at least one ODS counter (calls + success/failure). Error logs should include structured context fields. New features should be gated behind a JustKnobs flag or config toggle so they can be disabled in production without a code push.
