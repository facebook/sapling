# Eden Stats

**`eden stats --json` for diagnosing inode pressure, memory usage, and cache performance**

## Basic usage

```bash
# Quick lightweight check — inode counts and memory only
eden stats --json 2>/dev/null | python3 -m json.tool

# Full stats including all counters (thousands of entries)
eden stats --json 2>/dev/null | python3 -m json.tool
```

**Note:** `--json` only works on the top-level `eden stats` command, NOT on subcommands. `eden stats fuse --json` will fail — use `eden stats --json` instead.

**Note:** `eden stats` outputs status messages to stderr. Redirect stderr when capturing JSON: `eden stats --json 2>/dev/null`.

## Top-level keys

The JSON output contains these top-level keys:

### `mountPointInfo`
Per-mount inode statistics:
```json
{
  "/data/users/foo/fbsource": {
    "unloadedInodeCount": 25362,
    "loadedFileCount": 775,
    "loadedTreeCount": 45438
  }
}
```
- **`loadedTreeCount`** — number of directory inodes in memory. High values (>100K) indicate heavy directory traversal (build systems, IDE indexing).
- **`loadedFileCount`** — number of file inodes in memory. Usually much lower than tree count.
- **`unloadedInodeCount`** — inodes that were loaded but have been unloaded from memory. These are tracked but not consuming memory.
- **Total loaded inodes** = `loadedFileCount + loadedTreeCount`. If very high, consider `eden gc`.

### `vmRSSBytes`
EdenFS daemon resident memory in bytes. Divide by 1024^3 for GB.
- Normal range: 500MB–2GB
- If >4GB, investigate loaded inode count and cache sizes

### `mountPointJournalInfo`
Per-mount journal statistics:
```json
{
  "/data/users/foo/fbsource": {
    "entryCount": 318,
    "memoryUsage": 62336,
    "durationSeconds": 436886
  }
}
```
- **`entryCount`** — number of journal entries (file change events tracked for watchman)
- **`durationSeconds`** — time span covered by the journal
- High entry counts may correlate with slow `sl status` (watchman processes these)

### `counters` (full mode only)
Thousands of counters covering every subsystem. Key counter families for diagnosis:

**Store latency (fetch pipeline):**
- `store.sapling.get_tree.local_store.*` — local cache lookup times
- `store.sapling.get_tree.backing_store.*` — backing store (Rust/network) lookup times
- `store.sapling.get_blob.local_store.*` — blob fetch from local cache
- `store.sapling.get_blob.backing_store.*` — blob fetch from backing store

**Cache hit rates:**
- `object_store.get_tree.memory_cache.hit_count` / `miss_count`
- `object_store.get_blob.memory_cache.hit_count` / `miss_count`

**FUSE/NFS operation latency:**
- `fuse.lookup_us.*` — file lookup latency percentiles
- `fuse.read_us.*` — file read latency percentiles
- `fuse.readdir_us.*` — directory read latency percentiles

### `blobCacheStats` / `treeCacheStats`
In-memory cache statistics:
```json
{
  "blobCacheStats": {
    "entryCount": 100,
    "totalSizeInBytes": 5242880,
    "hitCount": 1500,
    "missCount": 200
  }
}
```
- **Hit rate** = `hitCount / (hitCount + missCount)`. Low hit rates indicate cache thrashing.
- **`entryCount`** — number of items in cache
- **`totalSizeInBytes`** — cache memory consumption

### `smaps` / `privateBytes`
Detailed memory breakdown (Linux). `privateBytes` shows non-shared memory.

## Diagnostic patterns

**High inode count — the cascade:**

When loaded inodes are very high (>500K), trace the causal chain:

1. **Was there a large checkout?** Grep eden log for checkout events:
   ```bash
   grep "checkout for\|semifuture_checkOutRevision" "$(eden debug log --path)" | tail -20
   ```
   A checkout touching >10K trees loads that many directory inodes into EdenFS.

2. **What processes amplified it?** Grep for heavy fetches:
   ```bash
   grep "Heavy fetches" "$(eden debug log --path)" | tail -20
   ```
   Shows which processes (buck2d, watchman, hg status) drove the most object fetches, materializing file inodes within the loaded trees.

3. **Is GC keeping up?** See the GC section below.

4. **Did watchman trigger a fresh instance?** Get the watchman log:
   ```bash
   watchman get-log
   ```
   Grep for `"Change amount exceeded threshold"` — this signals watchman detected a checkout with >10K changes. With `empty_on_fresh_instance=true` (the default), this returns empty results to subscribers, not a full crawl. It's a timestamp marker for when the large checkout happened, not a cause.

5. **Are hg status calls slow?** Grep for status call duration:
   ```bash
   grep "semifuture_getScmStatusV2" "$(eden debug log --path)" | tail -20
   ```
   A 30+ second status call means it walked the entire tree and materialized file inodes.

**GC (garbage collection):**

EdenFS periodically unloads unused inodes from memory to reduce memory pressure.

- **Frequency** — runs as a periodic background task, default every 6 hours on Linux
- **Normal GC cutoff** — inodes unused for >24 hours get unloaded
- **Aggressive GC cutoff** — 1 hour, but `aggressiveGcThreshold` is 0 (disabled by default)
- **Manual trigger** — `eden gc` forces a GC cycle

**Check GC in eden log:**
```bash
grep "Starting GC for\|GC for:.*completed" "$(eden debug log --path)" | tail -20
```

This shows:
- `Starting GC for: <mount> total number of inodes N` — inode count before GC
- `GC for: <mount>, completed in: Ns total number of inodes after GC: N` — inode count after, duration

**Interpreting GC:**
- If inodes are high **before** GC and drop significantly **after** → GC is working, inodes accumulated between cycles. May need more frequent GC or `eden gc` manually.
- If inodes are still high **after** GC → something is actively holding inodes loaded (processes with open file handles, running builds, IDE indexing). Check heavy fetches to find the culprit.
- If GC hasn't run recently → inodes accumulated without cleanup. Run `eden gc` manually.

**High memory usage:**
1. Check `vmRSSBytes` — is it abnormally high?
2. Check `mountPointInfo` — which mount has the most loaded inodes?
3. Check `blobCacheStats` / `treeCacheStats` — are caches consuming significant memory?
4. Fix: `eden gc` to unload inodes and free caches

**Slow file operations:**
1. Check store latency counters — are backing_store fetches slow?
2. Check cache hit/miss ratios — are we hitting cache or going to network?
3. Check FUSE latency percentiles — which operations are slow?

**Investigating a specific mount:**
```bash
# Get stats and extract info for a specific mount
eden stats --json 2>/dev/null | python3 -c "
import sys, json
data = json.load(sys.stdin)
mount = '/data/users/$USER/fbsource'
print('Inodes:', json.dumps(data.get('mountPointInfo', {}).get(mount, {}), indent=2))
print('Journal:', json.dumps(data.get('mountPointJournalInfo', {}).get(mount, {}), indent=2))
print('RSS:', data.get('vmRSSBytes', 'N/A'), 'bytes')
"
```

## Known issues

- `eden stats fuse --json` has a bug (KeyError on 'p95' percentile key). Use `eden stats --json` instead and look at FUSE counters in the `counters` object.
- The `--json` flag must come before any subcommand. `eden stats --json` works; `eden stats fuse --json` does not.
