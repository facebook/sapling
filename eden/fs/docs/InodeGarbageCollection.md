# Inode Garbage Collection in EdenFS

## Overview

EdenFS implements a garbage collection (GC) mechanism to manage memory usage by
deleting inodes that are no longer actively used. This document explains the GC
process, its entry points, and how it differs across the three filesystem
interfaces: **FUSE**, **NFS**, and **PrjFS**.

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              EdenServer                                     │
│                                                                             │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                       garbageCollectInodes()                         │   │
│  │                                                                      │   │
│  │  1. Acquire GC lease (prevents concurrent GC on same mount)          │   │
│  │  2. Call handleChildrenNotAccessedRecently() on root TreeInode       │   │
│  │  3. Call unloadChildrenUnreferencedByFs() to clean up                │   │
│  │  4. Log metrics and release lease                                    │   │
│  └───────────────────────────────┬──────────────────────────────────────┘   │
│                                  │                                          │
│                                  ▼                                          │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                  handleChildrenNotAccessedRecently()                 │   │
│  │                          (TreeInode)                                 │   │
│  │                                                                      │   │
│  │  Platform-specific dispatch:                                         │   │
│  │                                                                      │   │
│  │  ┌─────────────┐  ┌─────────────────────┐  ┌──────────────────────┐  │   │
│  │  │    FUSE     │  │        NFS          │  │       PrjFS          │  │   │
│  │  └─────────────┘  └─────────────────────┘  └──────────────────────┘  │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│                                  │                                          │
│                                  ▼                                          │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                    unloadChildrenUnreferencedByFs()                  │   │
│  │                                                                      │   │
│  │  Final cleanup: Unload inodes with zero FS reference count           │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Entry Point: `garbageCollectInodes`

**File:** `eden/fs/service/EdenServer.cpp`

The GC process is initiated by `EdenServer::garbageCollectInodes()`. This
function:

1. **Acquires a GC lease** - Prevents concurrent GC operations on the same mount
   using `mount.tryStartInodeGC()`
2. **Calls the first phase** - `handleChildrenNotAccessedRecently()` on the root
   TreeInode
3. **Calls the second phase** - `unloadChildrenUnreferencedByFs()` for final
   cleanup
4. **Logs metrics** - Records the GC duration and number of inodes
   invalidated/unloaded

## Phase 1: `handleChildrenNotAccessedRecently`

**File:** `eden/fs/inodes/TreeInode.cpp`

This function dispatches to platform-specific implementations based on the
filesystem channel type. Pressure-based FUSE GC actively invalidates stale
entries. Legacy FUSE GC only unloads stale inode objects whose references have
already been released by the kernel.

## Phase 2: `unloadChildrenUnreferencedByFs`

**File:** `eden/fs/inodes/TreeInode.cpp`

The final cleanup phase that removes inodes no longer referenced by the
filesystem. This function:

1. Recursively processes all tree children
2. Unloads any inode whose filesystem reference count is zero
3. Removes the inode from memory and the InodeMap

---

## Platform-Specific GC Behavior

### FUSE (Currently only supported on Linux)

**Behavior:** FUSE sends `FUSE_FORGET` after dropping a cached entry. Under inode
pressure, EdenFS invalidates stale entries to prompt those FORGET messages
rather than waiting for normal kernel cache eviction.

**GC Strategy:**

1. Load remembered `TreeInode` objects to walk their directory contents
2. Copy unloaded file candidates in bounded directory batches and retrieve
   their timestamps and filesystem reference counts with one `InodeMap` lookup
   per batch
3. Invalidate stale loaded and unloaded entries bottom-up through the bounded
   FUSE invalidation queue
4. Wait for invalidations to drain, then unload loaded inodes whose FORGET has
   reduced their filesystem reference count to zero

GC reconstructs remembered trees because their contents are needed to discover
descendants. It does not reconstruct unloaded `FileInode` objects before
invalidating their parent/name entries.

---

### NFS (Currently only supported on macOS)

**Behavior:** NFS (NFSv3) does not notify EdenFS when file handles are closed.
This means EdenFS cannot rely on the kernel to decrement FS refcounts
automatically.

**GC Strategy:**

1. Recursively traverse the inode tree using `processTreeChildren()`
2. For each non-materialized directory older than the cutoff:
   - Invalidate the directory in the NFS cache via
     `nfsInvalidateCacheEntryForGC()`
   - The invalidation callback decrements the FS refcount for all children
3. Return the count of invalidated inodes

**Key Considerations:**

- Only non-materialized directories are invalidated (user modifications are
  preserved)
- Uses `lastUsedTime` to determine if an inode is stale
- Bottom-up invalidation: children are invalidated before parents
- Invalidation is asynchronous; `completeInvalidations()` waits for all to
  finish

---

### PrjFS (Currently only supported on Windows)

**Behavior:** PrjFS manages placeholders on disk. Unlike FUSE, it doesn't
automatically notify EdenFS when files are closed.

**GC Strategy:**

1. Recursively traverse the inode tree using `processTreeChildren()`
2. For each non-materialized entry:
   - Check the file's access time on disk via `_wstat64()`
   - If the access time is older than the cutoff, invalidate via
     `invalidateChannelEntryCache()`
3. Return the count of invalidated inodes

**Key Considerations:**

- Uses on-disk access time instead of in-memory tracking
- Only invalidates non-materialized entries
- Relies on `invalidateChannelEntryCache()` failing for non-empty directories to
  prevent data loss
- Note: A race condition exists where a file could become materialized between
  the check and invalidation

---

## Helper Functions

### `processTreeChildren`

A template function that recursively processes tree children with cancellation
support.

### `getLoadedOrRememberedTreeChildren`

A helper function that is called from `processTreeChildren` to get the list of
tree's children (both loaded and unloaded). Pressure-based FUSE GC uses this to
reconstruct remembered trees before scanning their contents.

### `shouldCancelGC`

Checks for early termination conditions.

---

## Comparison Table

| Aspect                    | FUSE (Linux)                         | NFS (macOS)                              | PrjFS (Windows)                                                |
| ------------------------- | ------------------------------------ | ---------------------------------------- | -------------------------------------------------------------- |
| **Kernel Notification**   | Yes (`FUSE_FORGET`)                  | No                                       | No                                                             |
| **Refcount Management**   | Automatic by kernel                  | Manual via GC                            | Manual via GC                                                  |
| **Invalidation Required** | Under pressure                       | Yes                                      | Yes                                                            |
| **Time Tracking**         | Last filesystem request             | Last filesystem request                  | On-disk atime                                                  |
| **First GC Phase**        | `invalidateChildrenNotAccessedRecentlyFuse()` | `invalidateChildrenNotMaterializedNFS()` | `invalidateChildrenNotMaterializedPrjFS()`                     |
| **Invalidation Method**   | Bounded FUSE entry invalidation      | `nfsInvalidateCacheEntryForGC()`         | `invalidateChannelEntryCache()`                                |
| **Invalidation Scope**    | Stale loaded and unloaded entries    | Non-materialized directories             | Non-materialized files/directories                             |
| **Data Safety**           | Skip configured and mounted barriers | Skip materialized inodes                 | Skip materialized inodes; relies on failure for non-empty dirs |

---

## Sequence Diagram

```
┌─────────────┐     ┌──────────────┐     ┌──────────────┐     ┌──────────┐
│ EdenServer  │     │  TreeInode   │     │   InodeMap   │     │ FsChannel│
│             │     │   (Root)     │     │              │     │          │
└──────┬──────┘     └──────┬───────┘     └──────┬───────┘     └────┬─────┘
       │                   │                    │                  │
       │ garbageCollectInodes()                 │                  │
       │──────────────────>│                    │                  │
       │                   │                    │                  │
       │                   │ handleChildrenNotAccessedRecently()   │
       │                   │───────────────────────────────────────>
       │                   │                    │                  │
       │                   │    [Platform-specific invalidation]   │
       │                   │<──────────────────────────────────────│
       │                   │                    │                  │
       │                   │ invalidate/unload  │                  │
       │                   │ children           │                  │
       │                   │───────────────────>│                  │
       │                   │                    │                  │
       │                   │ unloadChildrenUnreferencedByFs()      │
       │                   │───────────────────>│                  │
       │                   │                    │                  │
       │                   │ [Unload inodes with refcount=0]       │
       │                   │<───────────────────│                  │
       │                   │                    │                  │
       │<──────────────────│                    │                  │
       │ (return count)    │                    │                  │
       │                   │                    │                  │
```

---

## Configuration Options

The GC behavior can be configured via `EdenConfig`:

| Config Key | Description                                 | Default                 |
| ---------- | ------------------------------------------- | ----------------------- |
| `enableGc` | Enable periodic garbage collection          | Platform-dependent      |
| `gcPeriod` | Interval between GC runs                    | Configured per platform |
| `gcCutoff` | Time threshold for considering inodes stale | 6 hours                 |

---

## Cancellation and Safety

The GC process supports cancellation at multiple points:

1. **Mount shutdown** - GC exits if the mount is shutting down
2. **External cancellation** - Via `gcCancelSource_.requestCancellation()`
3. **Graceful restart** - `stopAllGarbageCollections()` is called before
   takeover

---

## Related Files

- `eden/fs/service/EdenServer.cpp` - GC entry point and scheduling
- `eden/fs/inodes/TreeInode.cpp` - Core GC logic and platform dispatch
- `eden/fs/inodes/TreeInode.h` - TreeInode class declaration
- `eden/fs/inodes/InodeMap.cpp` - Inode tracking and refcount management
- `eden/fs/nfs/Nfsd3.cpp` - NFS invalidation implementation
- `eden/fs/prjfs/PrjfsChannel.cpp` - PrjFS invalidation implementation
