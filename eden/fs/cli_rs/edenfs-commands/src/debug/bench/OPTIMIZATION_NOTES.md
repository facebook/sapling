# Benchmark Traversal Optimization: --include-dir-stats Flag

## Problem Statement

The `--detailed-read-stats` flag in the benchmark traversal command was causing a **~37% throughput regression** (16,902 → 10,713 files/s). Initial investigation suggested mutex lock contention as the culprit.

## Investigation Summary

### Initial Theory (Wrong)
The original hypothesis was that `Mutex<HashMap>` was causing lock contention. We attempted to replace it with `DashMap` for lock-free concurrent access.

**Result**: DashMap made things worse because file reading is **single-threaded** - there's no lock contention to eliminate.

### Root Cause: CPU Cache Pollution

Profiling revealed that `record_file()` only takes **~1.24 µs/file**, but the total overhead was **~19 µs/file**. The missing ~17 µs was caused by **CPU cache pollution**:

1. `record_file()` touches many memory locations:
   - String allocation (`to_string_lossy().to_string()`) - touches allocator metadata
   - HashMap access (~29,000 directories = ~4-8 MB of data)
   - Depth calculation - iterates path components

2. This memory access pattern **evicts file-I/O-related data from CPU cache**:
   - Kernel page cache metadata
   - File descriptor tables
   - VFS inode cache
   - EdenFS FUSE buffers

3. When the NEXT file's `File::open()` runs, it suffers cache misses, taking ~19 µs longer.

### Evidence

| Metric | Without Flag | With Flag | Delta |
|--------|-------------|-----------|-------|
| Throughput | 13,522 files/s | 10,776 files/s | -20% |
| Time/file | 73.9 µs | 92.8 µs | +18.9 µs |
| open() latency | 22.3 µs | 41.5 µs | +19.2 µs |
| record_file() | 0 | 1.24 µs | +1.24 µs |
| **Unexplained** | - | - | **~17.7 µs** |

The key insight: `open()` latency nearly doubled, and this matches the unexplained overhead exactly.

## Solution: Optional --include-dir-stats Flag

Rather than eliminate the feature, we made the expensive parts optional:

### Changes Made

1. **Added `--include-dir-stats` CLI flag** (`cmd.rs`)
   - Users must explicitly opt-in to the slow per-directory stats

2. **Added `collect_dir_stats` field to `AdvancedStats`** (`traversal.rs`)
   - Controls whether expensive operations run

3. **Made `record_file()` conditional** (`traversal.rs`)
   - Fast path (always runs): histogram + category stats (atomic operations, minimal cache impact)
   - Slow path (optional): dir_stats HashMap + depth calculation

4. **Updated `print_detailed_read_statistics()`** (`traversal.rs`)
   - Shows message when dir_stats disabled
   - Conditionally displays directory-related sections

5. **Removed profiling instrumentation**
   - Removed 6 profiling counter fields
   - Removed all timing code from `record_file()`

### Expected Performance

| Mode | Throughput | Overhead |
|------|------------|----------|
| No flag | ~13,500 files/s | 0% (baseline) |
| `--detailed-read-stats` | ~12,500+ files/s | ~8% |
| `--detailed-read-stats --include-dir-stats` | ~10,800 files/s | ~20% |

### Usage

```bash
# Fast detailed stats (histogram + category performance only)
edenfsctl debug bench traversal --dir=/path --detailed-read-stats

# Full detailed stats including per-directory breakdown (slower)
edenfsctl debug bench traversal --dir=/path --detailed-read-stats --include-dir-stats
```

## Key Learnings

1. **CPU cache effects can dominate performance** - Even 1 µs of code can cause 17 µs of cache misses on subsequent operations

2. **Single-threaded code doesn't benefit from lock-free data structures** - DashMap adds overhead compared to uncontended Mutex

3. **HashMap size matters for cache** - ~29,000 directory entries (~4-8 MB) pollutes L2/L3 cache

4. **String allocations touch allocator metadata** - Even simple `to_string()` can evict cached data

5. **Measure the RIGHT thing** - We were measuring `record_file()` but the impact was on `File::open()`

## Files Modified

- `eden/fs/cli_rs/edenfs-commands/src/debug/bench/cmd.rs`
- `eden/fs/cli_rs/edenfs-commands/src/debug/bench/traversal.rs`
