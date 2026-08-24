---
name: flavour-support-completeness
metadata:
  oncalls: ['source_control']
  strict: true
  apply_to_path: 'eden/mononoke/servers/slapi/.*\.rs$'
  apply_to_content: 'SUPPORTED_FLAVOURS|SlapiCommitIdentityScheme|slapi_flavour'
---

# Flavour (Hg/Git) Support Completeness

**Severity: HIGH**

## What to Look For

Handlers declare which commit identity schemes they support via `SUPPORTED_FLAVOURS`. Adding `Git` to the list requires the handler body to actually handle both code paths.

## When to Flag

- A handler with `SUPPORTED_FLAVOURS` containing `Git` but no `match ectx.slapi_flavour()` or equivalent branching in the handler body
- A handler that hardcodes Hg-specific types (`HgChangesetId`, `HgNodeHash`) while declaring Git support

## Do NOT Flag

- Handlers that only support `[Hg]` (the default)
- Handlers where the logic is identity-scheme-agnostic (e.g., capabilities, repos metadata)

## Examples

**BAD (declares Git support but uses Hg types unconditionally):**
```rust
impl SaplingRemoteApiHandler for MyHandler {
    const SUPPORTED_FLAVOURS: &'static [SlapiCommitIdentityScheme] = &[Hg, Git];

    async fn handler(ctx, req) -> HandlerResult<..> {
        // Always uses HgChangesetId -- Git requests will fail or produce wrong results
        let cs = HgChangesetId::new(HgNodeHash::from(req.id));
        // ...
    }
}
```

**GOOD (branches on flavour):**
```rust
async fn handler(ctx, req) -> HandlerResult<..> {
    match ctx.slapi_flavour() {
        SlapiCommitIdentityScheme::Hg => {
            let cs = HgChangesetId::new(HgNodeHash::from(req.id));
            // ...
        }.left_future(),
        SlapiCommitIdentityScheme::Git => {
            let cs = GitSha1::from(req.id);
            // ...
        }.right_future(),
    }.await
}
```

## Evidence

- **D94865396**, **D94861939**: Code extending handlers that needed both Hg and Git support, using `match ectx.slapi_flavour()` with `.left_future()`/`.right_future()` for type unification.
