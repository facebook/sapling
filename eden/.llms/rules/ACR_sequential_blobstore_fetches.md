---
oncalls: ['source_control']
apply_to_regex: 'eden/(mononoke|scm)/.*\.rs$'
apply_to_content: '\.load\(|\.get\(|blobstore|derive|manifest|Loadable|get_hg_from_bonsai|get_git_sha1_from_bonsai|get_bonsai_from|HgMapping|GitMapping|GlobalrevMapping|SvnrevMapping'
---

# Sequential Blobstore Fetches

**Severity: HIGH**

## What to Look For

- Sequential `.load()` calls in a loop instead of concurrent loading via `buffer_unordered`
- Sequential `blobstore.get()` calls in a loop instead of concurrent fetching via `buffer_unordered`
- Sequential `derive()` calls when `derive_batch` / `derive_exactly_batch` / `fetch_batch` exists
- Sequential `fetch_many_edges` calls one at a time instead of batching changeset IDs
- Sequential single-ID mapping lookups (e.g., `get_hg_from_bonsai`, `get_git_sha1_from_bonsai`) in a loop instead of using the bulk `.get()` method with all IDs at once

Note: The `Blobstore` and `Loadable` traits do NOT have bulk get/put methods. The only way to batch raw blobstore and loadable operations is `stream::iter(...).buffer_unordered(N)`. However, the mapping traits (`BonsaiHgMapping`, `BonsaiGitMapping`, `BonsaiGlobalrevMapping`, `BonsaiSvnrevMapping`) DO have bulk APIs -- their `.get()` method accepts multiple IDs, and they have `bulk_add` / `bulk_import` for writes.

## When to Flag

- `for id in ids { id.load(ctx, blobstore).await? }` -- sequential load in a loop
- `for key in keys { blobstore.get(ctx, &key).await? }` -- sequential blobstore get in a loop (no bulk API exists; use `buffer_unordered`)
- `for cs_id in cs_ids { derive::<T>(ctx, cs_id).await? }` -- sequential derivation when batch API exists
- `for cs_id in cs_ids { commit_graph.fetch_many_edges(ctx, &[cs_id]).await? }` -- single-element batch calls in a loop
- `for cs_id in cs_ids { mapping.get_hg_from_bonsai(ctx, cs_id).await? }` -- sequential single-ID mapping lookup when the bulk `.get()` accepts all IDs at once
- Any `for` loop or `map` + `collect` that awaits a blobstore/derivation/mapping future on each iteration

## Do NOT Flag

- `stream::iter(...).map(...).buffer_unordered(N)` -- this IS the correct pattern
- `derive_batch` / `derive_exactly_batch` / `fetch_batch` calls -- these ARE bulk APIs
- Sequential loads where there is a true data dependency (result of one load determines the next)
- Loops over a small, fixed number of items (2-3)
- Test code

## Examples

**BAD (sequential load in loop):**
```rust
let mut changesets = Vec::new();
for cs_id in &changeset_ids {
    let bonsai = cs_id.load(ctx, repo.repo_blobstore()).await?;
    changesets.push(bonsai);
}
```

**GOOD (buffer_unordered):**
```rust
let changesets: Vec<_> = stream::iter(changeset_ids)
    .map(|cs_id| async move {
        cs_id.load(ctx, repo.repo_blobstore()).await
    })
    .buffer_unordered(100)
    .try_collect()
    .await?;
```

**BAD (sequential blobstore get):**
```rust
let mut blobs = Vec::new();
for key in &keys {
    let blob = blobstore.get(ctx, key).await?;
    blobs.push(blob);
}
```

**GOOD (buffer_unordered):**
```rust
let blobs: Vec<_> = stream::iter(keys)
    .map(|key| async move {
        blobstore.get(ctx, &key).await
    })
    .buffer_unordered(100)
    .try_collect()
    .await?;
```

**BAD (sequential derive):**
```rust
let mut derived = Vec::new();
for cs_id in &cs_ids {
    let d = repo.derive::<SkeletonManifestId>(ctx, *cs_id).await?;
    derived.push(d);
}
```

**GOOD (batch derive):**
```rust
let derived = repo
    .derive_batch::<SkeletonManifestId>(ctx, cs_ids.clone())
    .await?;
```

**BAD (single-element fetch_many_edges in a loop):**
```rust
for cs_id in &cs_ids {
    let edges = commit_graph.fetch_many_edges(ctx, &[*cs_id]).await?;
    process(edges)?;
}
```

**GOOD (batch all IDs at once):**
```rust
let all_edges = commit_graph.fetch_many_edges(ctx, &cs_ids).await?;
for (cs_id, edges) in all_edges {
    process(edges)?;
}
```

**BAD (sequential single-ID mapping lookup):**
```rust
let mut hg_ids = Vec::new();
for cs_id in &changeset_ids {
    let hg_id = bonsai_hg_mapping.get_hg_from_bonsai(ctx, *cs_id).await?;
    hg_ids.push(hg_id);
}
```

**GOOD (bulk mapping lookup):**
```rust
let entries = bonsai_hg_mapping
    .get(ctx, changeset_ids.clone().into())
    .await?;
let hg_ids: HashMap<_, _> = entries
    .into_iter()
    .map(|entry| (entry.bcs_id, entry.hg_cs_id))
    .collect();
```

The same pattern applies to `BonsaiGitMapping::get(ctx, BonsaisOrGitShas::Bonsai(ids))`, `BonsaiGlobalrevMapping::get(ctx, BonsaisOrGlobalrevs::Bonsai(ids))`, and `BonsaiSvnrevMapping::get(ctx, BonsaisOrSvnrevs::Bonsai(ids))` -- all accept multiple IDs. Avoid calling the single-ID convenience methods (`get_hg_from_bonsai`, `get_git_sha1_from_bonsai`, `get_bonsai_from_globalrev`, etc.) in a loop.

## Recommendation

For raw blobstore operations (`Blobstore::get`, `Blobstore::put`, `Loadable::load`, `Storable::store`), there are no bulk/batch methods on these traits -- the only way to batch is `stream::iter(...).map(...).buffer_unordered(N)`. Choose N based on downstream capacity: 64-100 for blobstore operations, 10-50 for SQL-backed stores.

For higher-level operations, prefer dedicated batch APIs when available:

- `BonsaiDerivable::derive_batch` / `fetch_batch` / `store_mapping_batch` -- batch derived data operations (note: `fetch_batch`'s default impl itself uses `buffer_unordered` internally)
- `CommitGraphStorage::fetch_many_edges` -- batch commit graph edge fetching
- `derive_manifests_for_simple_stack_of_commits` -- batch manifest derivation for linear stacks
- `find_entries` -- batch path lookup in a manifest tree

For ID mapping lookups, use the bulk `.get()` method instead of single-ID convenience methods in a loop:

- `BonsaiHgMapping::get(ctx, BonsaiOrHgChangesetIds)` + `bulk_add` -- bulk read/write of bonsai-to-hg mappings
- `BonsaiGitMapping::get(ctx, BonsaisOrGitShas)` + `bulk_add` -- bulk read/write of bonsai-to-git mappings
- `BonsaiGlobalrevMapping::get(ctx, BonsaisOrGlobalrevs)` + `bulk_import` -- bulk read/write of bonsai-to-globalrev mappings
- `BonsaiSvnrevMapping::get(ctx, BonsaisOrSvnrevs)` + `bulk_import` -- bulk read/write of bonsai-to-svnrev mappings

These mapping traits' `.get()` methods accept multiple IDs in a single SQL query. The convenience methods (`get_hg_from_bonsai`, `get_git_sha1_from_bonsai`, etc.) are wrappers that pass a single ID -- calling them in a loop issues N separate SQL queries instead of one.
