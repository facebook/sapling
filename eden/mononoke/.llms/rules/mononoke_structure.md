---
oncalls: ['scm_server_infra']
apply_to_regex: 'eden/mononoke/.*(\.rs|BUCK)$'
---

# Mononoke Architecture & Filesystem Layout

**Severity: HIGH**

## What to Look For

- Code that violates the layered architecture (lower layers importing higher layers)
- Crates placed at the wrong directory depth or missing their own BUCK file
- Features that hold mutable state instead of delegating to repo attributes
- Server binaries with business logic instead of delegating to `mononoke_api`
- Overly large `lib.rs` files that should be split into modules
- Use of `foo/mod.rs` instead of `foo.rs` for module declarations
- Vague or non-unique crate names
- Facebook-only code placed outside the `facebook/` mirror hierarchy

## When to Flag

- A crate under `elements/` imports from `features/`, `servers/`, or `tools/`
- A crate under `repo_attributes/` imports from `features/`, `servers/`, or `tools/`
- A crate under `features/` imports from `servers/` or `tools/`
- A feature crate stores mutable state (e.g., owns a `Mutex`, `RwLock`, or mutable singleton) instead of reading/writing state through a repo attribute
- A server binary (under `servers/`) contains domain logic that should live in `mononoke_api` or a feature crate
- A new crate is created at the top level of `eden/mononoke/` instead of nested under the appropriate layer directory
- A new crate lacks its own BUCK file or does not keep source files in `src/`
- A crate name is ambiguous (e.g., `fsnodes` when `fsnodes_derivation` is more precise)
- A module uses `foo/mod.rs` instead of `foo.rs`
- `lib.rs` contains substantial implementation logic instead of re-exports and glue

## Do NOT Flag

- Existing code that predates these conventions (only flag new or modified code)
- Cross-layer imports within test code or test utilities
- Crates that legitimately need to be at the top level (e.g., `mononoke_api`, shared types)
- Small `lib.rs` files that naturally contain all the crate logic
- `mod.rs` files that already exist and are not being modified

## Examples

**BAD (element importing a feature):**
```rust
// In eden/mononoke/elements/mercurial_types/src/lib.rs
use repo_cross_repo::RepoCrossRepo;  // elements must not depend on features
```

**GOOD (feature importing an element):**
```rust
// In eden/mononoke/features/repo_cross_repo/src/lib.rs
use mercurial_types::HgChangesetId;  // features may depend on elements
```

**BAD (server with business logic):**
```rust
// In eden/mononoke/servers/scs/scs_server/src/methods.rs
fn resolve_bookmark(&self, ctx: &CoreContext, repo: &BlobRepo, bookmark: &str) -> Result<HgChangesetId> {
    // Directly queries blobstore, applies derivation, etc.
}
```

**GOOD (server delegating to mononoke_api):**
```rust
// In eden/mononoke/servers/scs/scs_server/src/methods.rs
fn resolve_bookmark(&self, ctx: &CoreContext, repo: &RepoContext, bookmark: &str) -> Result<ChangesetId> {
    repo.resolve_bookmark(bookmark).await
}
```

**BAD (vague crate name):**
```python
# BUCK
rust_library(
    name = "fsnodes",  # too ambiguous -- is this types? derivation? validation?
)
```

**GOOD (precise crate name):**
```python
# BUCK
rust_library(
    name = "fsnodes_derivation",  # clearly scoped purpose
)
```

**BAD (mod.rs pattern):**
```
src/
  commits/
    mod.rs      # prefer commits.rs at the parent level
    helpers.rs
```

**GOOD (flat module file):**
```
src/
  commits.rs    # module declaration as a file
  commits/
    helpers.rs
```

## Recommendation

Mononoke follows a strict layered architecture. From bottom to top: **Elements** (pure data types) -> **Repo Attributes** (per-repo state/storage) -> **Features** (stateless business logic) -> **API** (`mononoke_api`, the unified interface) -> **Tools/Servers/Jobs** (thin entry points). Each layer may only depend on layers below it. Features must be stateless -- all mutable state belongs in repo attributes. Server binaries must be thin wrappers that delegate to `mononoke_api`. Every crate lives 2+ directory levels deep, has its own BUCK file, keeps source in `src/`, and uses a globally unique, precise name. Prefer `foo.rs` over `foo/mod.rs` for module files, and keep `lib.rs` small by splitting implementation into dedicated modules. Facebook-only code goes in a `facebook/` subdirectory mirroring the open-source hierarchy.
