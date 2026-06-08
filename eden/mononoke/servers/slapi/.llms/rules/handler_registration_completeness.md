---
oncalls: ['source_control']
apply_to_regex: 'eden/mononoke/servers/slapi/.*\.rs$'
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
