---
oncalls: ['source_control']
apply_to_regex: 'eden/mononoke/servers/slapi/.*\.rs$'
apply_to_content: 'HandlerError|HttpError|e4xx|e5xx|MononokeError|status_code|\?;'
---

# HTTP Status Code Semantics

**Severity: HIGH**

## What to Look For

Bare `?` in handler code converts errors to `HandlerError` via `From<anyhow::Error>`, which defaults to HTTP 500. This silently turns client errors and expected conditions into internal server errors.

## When to Flag

- New error paths using bare `?` without `.context()` or explicit `HttpError` conversion in handler code
- Rate limiting responses using 429 (Too Many Requests) for server-wide limits -- should be 503 (Service Unavailable) since it's not per-user
- Access-denied conditions (redaction, permissions) returning 500 instead of 403
- New `MononokeError` variants without a corresponding mapping in `MononokeErrorExt`
- Repo-not-found returning 404 when repo exists tier-wide but isn't loaded on this shard -- should be 503 (retriable)

## Do NOT Flag

- Bare `?` on operations that genuinely indicate internal errors (blobstore failures, DB errors)
- Error conversions already using explicit `HttpError::e4xx()` or `HttpError::e5xx()`

## Examples

**BAD (bare ? turns client error into 500):**
```rust
async fn handler(ctx: SaplingRemoteApiContext<..>, req: Request) -> HandlerResult<..> {
    let bookmark = parse_bookmark(&req.name)?;  // Invalid bookmark name -> 500
    // ...
}
```

**GOOD (explicit 400 for client errors):**
```rust
let bookmark = parse_bookmark(&req.name)
    .map_err(|e| HttpError::e400(e.context("invalid bookmark name")))?;
```

**BAD (429 for server-wide rate limit -- matches D95521399):**
```rust
// Server-wide (untargeted) rate limit -- not specific to this user
return Err(HttpError::e429("rate limited".into()));
```

**GOOD (503 for server-wide limit):**
```rust
return Err(HttpError::e503("server overloaded".into()));
```

## Evidence

- **D95521399**: Changed untargeted rate limiting from 429 to 503. 429 was misleading because the limit wasn't per-user.
- **D88865547**: Redacted blob errors surfaced as 500 instead of 403. Added explicit `RedactionError -> e403` mapping.
- **D111918397**: `get_repo` returned 404 for repos not loaded on the shard. Now returns 503 (`RepoNotLoaded`)
  when repo exists tier-wide; 404 only for truly unknown repos.
