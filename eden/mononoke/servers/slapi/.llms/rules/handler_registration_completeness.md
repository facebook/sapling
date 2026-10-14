---
name: handler-registration-completeness
metadata:
  oncalls: ['source_control']
  strict: true
  apply_to_path: 'eden/mononoke/servers/slapi/.*\.rs$'
  apply_to_content: 'SaplingRemoteApiHandler|SaplingRemoteApiMethod|build_router|log_stats'
---

# SLAPI Handler Registration Completeness

**Severity: HIGH**

## What to Look For

Adding a new SLAPI endpoint requires updating four locations in lockstep. Missing the ODS or route registration steps causes **silent failures** (no compile error).

## When to Flag

- A new `impl SaplingRemoteApiHandler` without a corresponding `SaplingRemoteApiMethod` enum variant
- A new `SaplingRemoteApiMethod` variant without a matching arm in `log_stats` (`middleware/ods.rs`)
- A new handler struct not registered via `Handlers::setup::<Handler>(route)` in `build_router`
- A handler whose response type embeds errors (e.g., `Result<..., ServerError>` fields) but doesn't implement `extract_in_band_error`
- A new per-handler ODS failure counter incremented only on the failure path -- also `add_value(0)` for it on the
  success path, otherwise a monitor on it reads no data rather than zero
- A ratio monitor built from two counters with different units (per-item vs per-batch) -- add a batch counter
  beside the per-item one so the ratio compares like with like
- A per-repo `dynamic_histogram` of a duration as the only record of it -- also record it on the request row with
  `PerfCounters::add_duration_us`, since the per-repo histogram is sparse per task

## Do NOT Flag

- Changes to existing handlers that don't add new method variants
- Test code

## Examples

**BAD (handler exists but never registered -- matches D90503346):**
```rust
pub struct StreamingCloneHandler;

#[async_trait]
impl SaplingRemoteApiHandler for StreamingCloneHandler {
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::StreamingClone;
    const ENDPOINT: &'static str = "/streaming_clone";
    // ...
}

// build_router: no Handlers::setup::<StreamingCloneHandler>(route) call!
// Handler is dead code, endpoint is unreachable.
```

**GOOD (all four locations updated):**
1. Enum variant added to `SaplingRemoteApiMethod` + `Display` impl
2. ODS `quantile_stat` declaration in `define_stats!` + arm in `log_stats`
   (do NOT use the deprecated `histogram` macro -- wrong tail percentiles)
3. `Handlers::setup::<NewHandler>(route)` in `build_router`
4. `extract_in_band_error` implemented if response embeds errors

## Evidence

- **D90503346**: StreamingCloneHandler was never registered in `build_router`. Masked by `#[allow(dead_code)]` on the struct.
- **D90988392**: ListBookmarkPatterns correctly added all four pieces.
- **D107104769**: Migrated `*_duration_ms` to `quantile_stat`; old `histogram` p99 was 2-3× too high.
- **D124630048**: `upload_trees` augmented-manifest counters -- `built`/`attempted` counted trees while the failure
  counters counted batches, the failure counters were never exported at zero, and the per-repo `build_ms` histogram
  read far below the per-request figure.
