---
oncalls: ['source_control']
apply_to_regex: 'eden/(mononoke|scm)/.*\.rs$'
apply_to_content: '\.unwrap\(|\.expect\('
---

# Rust Unwrap/Expect Safety

**Severity: HIGH**

## What to Look For

- Calls to `.unwrap()` or `.expect()` on `Option` or `Result` types
- Bare `?` without `.context()` or `.with_context()` that loses error context
- Fatal parse/read of files during startup without graceful degradation

## When to Flag

- `.unwrap()` or `.expect()` in any non-test, non-example code path
- Error propagation with `?` on an opaque error type (no `.context()`)
- `panic!()` in server code paths (not CLI tools or setup code)
- Startup code that `.unwrap()`s on parsing files that may be temporarily empty, malformed, or absent (see Evidence)

## Do NOT Flag

- `.unwrap()` inside `#[cfg(test)]` modules or files ending in `_test.rs`
- `.expect()` in `main()` for truly required one-time initialization (e.g., CLI arg parsing)
- `.unwrap()` on values proven safe by a preceding `if let` / `match` / `.is_some()` check on the same binding
- `.unwrap()` in doc examples or benches
- `regex::Regex::new("literal").unwrap()` for compile-time-known patterns
- **JustKnobs `?` without `.context()`** — for `justknobs::eval()` / `justknobs::get()` / `justknobs::get_as()`, bare `?` is the **correct** pattern. Using `.unwrap_or(default)` is an anti-pattern because (1) fetching a non-existent knob is unexpectedly expensive and (2) a default silently hides misconfiguration. Defaults belong in `just_knobs.json`, not at the call site. See D92827579.

## Examples

**BAD (startup crashloop — matches S527246 pattern):**
```rust
fn load_tls_seeds() -> TlsSeeds {
    let contents = std::fs::read_to_string("/var/facebook/x509_identities/server.pem.seeds")
        .expect("seeds file must exist");
    serde_json::from_str(&contents).expect("seeds must be valid JSON")
    // If the seeds agent produces a momentarily empty file, every
    // restarting Mononoke task crashloops simultaneously.
}
```

**GOOD (graceful degradation for optional infra):**
```rust
fn load_tls_seeds() -> Option<TlsSeeds> {
    let contents = match std::fs::read_to_string("/var/facebook/x509_identities/server.pem.seeds") {
        Ok(c) if !c.is_empty() => c,
        Ok(_) => { warn!("TLS seeds file is empty, skipping session resumption"); return None; }
        Err(e) => { warn!("Could not read TLS seeds: {}", e); return None; }
    };
    match serde_json::from_str(&contents) {
        Ok(seeds) => Some(seeds),
        Err(e) => { error!("Failed to parse TLS seeds: {}", e); None }
    }
}
```

**BAD (opaque error):**
```rust
let cs_id = bonsai.get_changeset(ctx, cs).await?;
```

**GOOD (actionable context):**
```rust
let cs_id = bonsai
    .get_changeset(ctx, cs)
    .await
    .with_context(|| format!("Changeset {} not found in {}", cs, repo_name))?;
```

## Recommendation

Replace `.unwrap()` / `.expect()` with `?` and add `.context()` or `.with_context()` from the `anyhow` crate. Error messages should include the variable values that led to the failure so on-call can diagnose from logs alone. For startup code, classify each file/resource as **required** vs **optional** — optional resources (TLS session caches, performance hints, seed files) must degrade gracefully, not crash.

## Evidence

- **S527246**: Mononoke crashlooped across all tasks because `server.pem.seeds` (a TLS session resumption optimization) was momentarily empty after an agent update. The file was optional for correctness but parsed with `.expect()`.
- **S596057**: EdenFS restart failures because `hg` wasn't in PATH during chef-initiated restarts — a hard dependency on an optional binary in the startup path.
