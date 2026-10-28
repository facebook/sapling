---
name: scs-api-layering
metadata:
  oncalls: ['source_control']
  strict: true
  apply_to_path: 'eden/mononoke/servers/scs/.*\.rs$'
  apply_to_content: 'use |facet|restricted_paths|commit_graph|blobstore|repo_derived_data|repo_blobstore|repo_identity|phases|bookmarks|pushrebase'
---

# SCS Must Use mononoke_api, Not Facets Directly

**Severity: HIGH**

## What to Look For

- SCS server code (`scs_methods`, `scs_server`) importing and using Mononoke facet crates directly instead of going through `mononoke_api` primitives (`RepoContext`, `ChangesetContext`, etc.)
- Direct access to repo facets like `repo.repo_blobstore()`, `repo.commit_graph()`, `repo.repo_derived_data()` from within SCS handler code
- SCS code that re-implements logic already available (or that should be available) as a `mononoke_api` method

## When to Flag

- `use` statements in `scs_methods` or `scs_server` importing facet crates (e.g., `restricted_paths`, `repo_derived_data`, `commit_graph`, `repo_blobstore`, `phases`, `bookmarks`, `pushrebase`)
- SCS handler code calling facet methods directly on the repo object instead of using `RepoContext`/`ChangesetContext` methods
- SCS code that duplicates logic from `mononoke_api` -- e.g., manually querying derived data when `ChangesetContext` already exposes a method for it
- New SCS endpoints that bypass `mononoke_api` for operations that other consumers (Git server, SLAPI server) would also need

## Do NOT Flag

- `mononoke_api` code itself importing facets -- that's the correct layer to do so
- SCS code using `RepoContext`, `ChangesetContext`, `ChangesetPathContext` and their methods -- that IS the correct API
- Test code or test fixtures
- SCS Thrift type definitions and conversion code (these are SCS-specific by nature)
- Imports of `mononoke_api` types like `MononokeError`, `RepoContext`, `ChangesetContext`

## Why This Matters

Mononoke's architecture has a clear layering:

```
SCS Server / Git Server / SLAPI Server  (consumers)
         |
    mononoke_api  (shared primitives: RepoContext, ChangesetContext, etc.)
         |
    facets  (repo_blobstore, commit_graph, derived_data, etc.)
```

When SCS bypasses `mononoke_api` and talks to facets directly:
1. Other servers (Git, SLAPI) can't reuse the logic -- it gets duplicated
2. Cross-cutting concerns (access control, logging, rate limiting) applied in `mononoke_api` get skipped
3. Refactoring facets becomes harder because SCS is coupled to their internal APIs

## Examples

**BAD (SCS handler using facet directly):**
```rust
// In scs_methods/src/commit.rs
use repo_derived_data::RepoDerivedData;

async fn commit_info(repo: &impl RepoDerivedData, cs_id: ChangesetId) -> Result<Info> {
    let derived = repo.repo_derived_data().derive::<SomeType>(ctx, cs_id).await?;
    // ...
}
```

**GOOD (SCS handler using mononoke_api):**
```rust
// In scs_methods/src/commit.rs
use mononoke_api::ChangesetContext;

async fn commit_info(changeset: &ChangesetContext) -> Result<Info> {
    let info = changeset.some_method().await?;
    // The ChangesetContext internally uses the facets, applying
    // access control and logging as needed.
}
```

**BAD (SCS re-implementing mononoke_api logic):**
```rust
// In scs_methods -- manually checking restricted paths via facet
let restricted = repo.restricted_paths_facet().check_paths(ctx, &paths).await?;
```

**GOOD (using mononoke_api primitive):**
```rust
// mononoke_api exposes this as a ChangesetContext method
let restricted = changeset.paths_restriction_info(paths).await?;
```

## Recommendation

When adding new SCS functionality, first check if `mononoke_api` already exposes a suitable method on `RepoContext` or `ChangesetContext`. If not, add the primitive to `mononoke_api` first, then call it from SCS. This keeps the shared layer complete and ensures all servers benefit from the same logic. The `scs_methods` crate should ideally only depend on `mononoke_api`, not on individual facet crates.

## Evidence

- D93601164: Review comment -- "We don't want SCS (or Mononoke SLAPI server) to interface directly with the facets if we can avoid. That's one of the points of mononoke_api, is to abstract higher level operations into reusable primitives."
- Mononoke design guidelines document the layered architecture with mononoke_api as the shared primitive layer.
