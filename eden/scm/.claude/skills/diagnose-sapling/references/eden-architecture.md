# EdenFS Storage Architecture

**For understanding where data lives and why things break.**

## Fetch pipeline (lookup order)

When EdenFS needs a file or tree, it checks these layers top-to-bottom. A miss at one layer falls through to the next:

1. **ObjectStore in-memory cache** (C++ LRU caches, nanoseconds) — per-mount TreeCache for trees, AuxData caches for metadata. **No in-memory blob cache** — blobs are fetched from BackingStore every time they're not in the kernel page cache.
2. **BackingStore / SaplingBackingStore** (Rust FFI, local-first) — tries local IndexedLog on disk first (`LocalOnly`), then falls back to network (`AllowRemote` via EdenAPI to Mononoke).
3. **Mononoke** (network, EdenAPI over HTTPS) — source of truth, 100ms+. Fetches populate all cache layers on the way back.

**Key code locations:**
- `eden/fs/store/ObjectStore.h/.cpp` — caching coordinator, owns TreeCache and AuxData caches
- `eden/fs/store/BackingStore.h` — abstract interface with private `getTree`/`getBlob` methods (friend class ObjectStore)
- `eden/fs/store/sl/SaplingBackingStore.h/.cpp` — production implementation using Rust FFI

## ObjectStore — the caching coordinator

ObjectStore sits between EdenFS inode code and BackingStore. It:
- Maintains in-memory LRU caches (TreeCache, blobAuxDataCache, treeAuxDataCache)
- Deduplicates concurrent requests for the same object
- Calls BackingStore private methods (it's a friend class)
- Records telemetry for cache hits/misses

**Important:** ObjectStore does NOT cache blobs in memory. It only caches trees and auxiliary data (size, SHA-1, Blake3 hashes). Blob data flows through without caching — the kernel page cache serves as the de facto blob cache.

## BackingStore — abstract interface

BackingStore provides private fetch methods that only ObjectStore can call:
- `getTree()` — fetch a directory listing
- `getBlob()` — fetch file contents
- `getBlobMetadata()` — fetch blob size/hashes without the full content
- `getGlobFiles()` — batch glob matching

Implementations:
- **SaplingBackingStore** — production (Sapling/Mercurial repos)
- **GitBackingStore** — Git repos (conditional compilation)
- **FilteredBackingStore** — filtered repo views
- **RecasBackingStore** — Remote Execution CAS

## SaplingBackingStore — the production path

Uses Rust FFI to call into Sapling's `scmstore` crate. The fetch pattern is **local-first**:

1. **`getTreeLocal()`** — check IndexedLog on disk (`ImportPriority::LocalOnly`)
2. If local miss → **enqueue to worker thread pool** → `getTree()` with `ImportPriority::AllowRemote`
3. Worker thread calls Rust `scmstore` → IndexedLog (local disk) → EdenAPI (network) → Mononoke

**Local storage:** IndexedLog files in the backing repo's `.hg/store/`:
- `indexedlogdatastore/` — file content blobs (zstd compressed, append-only)
- `manifests/` — tree objects (directory listings)
- `hgcommits/` — commit metadata and DAG segments
- `metalog/` — bookmarks, remote bookmarks, visibility
- `mutation/` — commit mutation tracking (amend, rebase, fold history)

## Diagnostic relevance

- **Slow file operations** — could be a miss at every layer, ending in a network fetch. Check `eden trace sl --retroactive` to see fetch activity. Check `eden stats --json` counters for `store.sapling.get_tree.backing_store` latency.
- **High disk usage** — `eden du --fast` shows backing repo sizes. Large backing repos can be trimmed with `eden gc`.
- **Corrupt store** — `sl doctor` repairs IndexedLog (segments, metalog, mutation). `eden fsck` repairs overlay.
- **EdenFS memory** — loaded inodes stay in memory. `eden stats` shows counts, `eden gc` frees them. TreeCache and AuxData caches also consume memory.
- **Cache performance** — `eden stats --json` shows `blobCacheStats` and `treeCacheStats` with hit/miss counts. Low hit rates indicate cache thrashing or undersized caches.
- **Code to investigate**: `eden/fs/store/` (ObjectStore, BackingStore), `eden/scm/lib/revisionstore/` (Sapling stores), `eden/scm/lib/edenapi/` (network protocol)
