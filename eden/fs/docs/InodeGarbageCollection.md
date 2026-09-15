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

For pressure-based FUSE GC, a zero-reference tree remains loaded when unloading
it would immediately recreate an `unloadedInodes_` record because it still
anchors a remembered child. This avoids repeatedly loading and unloading the
same directory ancestry between GC cycles.

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

**Behavior:** NFSv3 has no message from the server to the client. The client
never tells EdenFS when it stops using a handle (there is no `FUSE_FORGET`),
and EdenFS cannot tell the client to drop one. Every name and handle the
client has resolved stays in its caches until it decides otherwise, so an
inode's FS reference count on NFS is a sticky flag: set by the first LOOKUP
that hands the handle out, cleared only by GC.

**What the macOS client does, as measured** (`eden trace fs` shows the
requests with `cred_uid`/`cred_gid`; sources in the `apple-oss-distributions/NFS`
kext):

- A directory's cached names are dropped only when the client compares the
  directory's mtime against the one it recorded for its name cache, which
  happens lazily at its next `getattr` of that directory, or immediately when
  any request on the directory is answered `NFS3ERR_STALE`. The stale reply
  purges the directory's own name in its parent and all of its children's
  names at once. A chmod that changes nothing does not change the mtime and
  so does nothing on the client by itself.
- A stale **file** handle heals only while nothing has the file open:
  `nfs_refresh_fh` looks the name up again in the parent, EdenFS reloads the
  inode under its persisted number (child inode numbers are stored in the
  overlay), and the request retries. The same function refuses a file with
  open state, which every open fd and every mapping is, and refuses anything
  but files and symlinks. A process reading an open file, or faulting in a
  mapped one, whose inode GC forgot therefore sees ESTALE, and so does every
  path resolved through a forgotten **directory**. Forgetting a directory the
  client still holds is the one thing NFS GC must never do; forgetting an open
  file is what the fd and mapping pins prevent.
- To resolve and authorize a chmod, the kernel sends its own requests, with
  the caller's uid: a GETATTR of the directory whenever the client's attribute
  cache for it has expired (30 to 60 seconds idle), an ACCESS of the parent,
  and a LOOKUP of the AppleDouble name `._<dir>` in the parent.
- A *successful* chmod emits a file system event, and user-level daemons
  react by re-resolving the changed path within tens of milliseconds:
  `LOOKUP(parent, dir)` twice, `GETATTR(dir)`, `GETATTR(parent)`. A failed
  chmod emits none.
- A process's working directory, open fds and mmaps hold vnodes the client
  will keep using; only the privhelper's pin scan can see them.

**GC Strategy** (`invalidateChildrenNotMaterializedNFS`):

1. Walk loaded directories bottom-up, materialized or not; each directory
   waits for its children before deciding. A directory is stale if neither it
   nor its loaded children were named by a request since the cutoff. A
   materialized directory's state is in the overlay, so an unloaded child
   reloads from there, as on FUSE. The root's `.eden` directory is left alone
   and kept referenced: tools resolve its entries constantly.
2. Skip the chmod when nothing under the directory could be cleared (no child
   is loaded or remembered), but still report the directory as invalidated so
   its parent can clear it.
3. Queue a chmod of the directory to its current mode through the channel's
   bounded `InvalidationQueue` (`nfs:max-queued-gc-invalidations`, cancellable
   so checkout is not held up), on the mount's GC invalidation executor.
   `Nfsd3` records the directory and its ancestors in `InvalidatingInodes`
   while the chmod runs.
4. When the chmod reaches EdenFS as a SETATTR (a no-op SETATTR of a
   directory being invalidated), `Nfsd3` first runs GC's forget callback and
   then answers `NFS3ERR_STALE`. The stale reply makes the client drop the
   directory's names at once, and the forget is EdenFS's counterpart to the
   FORGET FUSE gets from the kernel: it clears the FS references of the
   directory's children, files always, directories only when a pin set is
   known, and never a pinned inode or a directory whose subtree contains one.
   A LOOKUP the client sends afterwards references a child anew, as a FUSE
   lookup would, so nothing that a process resolves after the purge is
   forgotten. The chmod fails with `ESTALE`, which counts as its success
   (`nfs.invalidation.gc.stale_reply`), and emits no file system event.
   Only the first such SETATTR takes the forget and is answered stale; a
   second no-op SETATTR of the directory while its chmod runs is answered
   normally and changes nothing. Nothing tells GC's chmod apart from a
   client's own same-mode chmod of that directory in that window: the
   client's takes the stale reply and fails once, and GC's chmod is then the
   second. With `experimental:nfs-gc-stale-reply` off,
   the fallback, the SETATTR is answered normally and the children are
   forgotten once the chmod has succeeded, as GC did before; the client then
   keeps its names until a stale file handle sends it back to EdenFS.
   Checkout's invalidations are answered normally; their directories change,
   so their mtime updates purge the client.
5. While the chmod runs, the kernel's own requests on the directory and its
   ancestors that hand out no entries (GETATTR, ACCESS, SETATTR, negative
   LOOKUP) do not refresh request times; requests that resolve entries always
   do. A directory whose chmod never reached EdenFS as a SETATTR (ENOENT,
   EACCES, EPERM) clears nothing and does not count as invalidated, so its
   ancestors are left alone.
6. The one race this leaves is a LOOKUP processed just before the SETATTR
   whose reply reaches the client just after the stale reply: the client then
   caches a handle that was cleared. For a file it heals; for a directory it
   would be a stale handle. The window is the interval between two handlers
   writing their replies to the same socket.
7. Each directory waits on its own chmod's completion future, not on a queue
   flush, so with `nfs:num-invalidation-threads` above one the chmods of
   unrelated directories overlap. The future carries the number of references
   the forget cleared, or nothing if no SETATTR arrived. The parent then
   decides. A cancelled walk still waits for the chmods it queued and joins
   the child walks it started; cancellation only stops new work.
8. A run reports the number of FS references it cleared; the sweep then
   forgets inodes with a zero count. Pressure GC judges progress by what the
   sweep unloaded plus the remembered (unloaded) inodes that were forgotten
   outright when their reference was cleared, which the sweep never sees.

**Pins:** when `mount:pressure-gc-scan-pins` is on, pressure-based GC asks the
privhelper (`--scan-pins`, via libproc on macOS) for the inodes that processes
hold as process or per-thread cwd, open fd or mapping, and keeps those and
their ancestors referenced. The scan is a snapshot: a process that acquires a
pin after it is not protected by it. Without a pin set, whether because the
knob is off or because the scan failed, GC keeps every directory referenced
and reclaims files only, which is what the periodic GC does. Open and mapped
files are unprotected in such a run and see ESTALE if GC forgets them, so a
failing scan must be fixed rather than lived with: each failure logs a
`pin_scan_failure` edenfs_events event with the reason, exit status or errno,
duration and the start of the helper's stdout and stderr, next to a
rate-limited warning in the log. `eden debug gc-inodes` does whatever the
periodic GC would do: with pressure GC off it has no pin set and reclaims
files only, with it on it scans for pins like the tick.

**Key Considerations:**

- Materialized directories are invalidated too; their state, and that of
  materialized files, lives in the overlay and survives unloading
- Bottom-up: children are invalidated before parents
- Invalidation is asynchronous; a directory waits for its own chmod, and
  checkout uses `completeInvalidations()`, the queue's flush barrier
- The tests in `NfsGcTest.cpp` hold every chmod at the `nfsInvalidation`
  fault and play the kernel themselves, sending the SETATTR before letting
  the chmod go
- Raising `nfs:num-invalidation-threads` is safe: invalidations of different
  directories share no ordering assumption, and `InvalidatingInodes` counts
  concurrent lineages

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
| **Refcount Management**   | Automatic by kernel                  | Sticky flag, cleared by GC               | Manual via GC                                                  |
| **Invalidation Required** | Under pressure                       | Yes                                      | Yes                                                            |
| **Time Tracking**         | Last filesystem request             | Last filesystem request                  | On-disk atime                                                  |
| **First GC Phase**        | `invalidateChildrenNotAccessedRecentlyFuse()` | `invalidateChildrenNotMaterializedNFS()` | `invalidateChildrenNotMaterializedPrjFS()`                     |
| **Invalidation Method**   | Bounded FUSE entry invalidation      | chmod answered `NFS3ERR_STALE`, then clear children's references | `invalidateChannelEntryCache()`                    |
| **Invalidation Scope**    | Stale loaded and unloaded entries    | Stale directories, materialized or not   | Non-materialized files/directories                             |
| **Data Safety**           | Skip configured and mounted barriers | Pins keep held inodes; overlay keeps materialized state | Skip materialized inodes; relies on failure for non-empty dirs |

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

| Config Key                                | Description                                                        |
| ----------------------------------------- | ------------------------------------------------------------------ |
| `experimental:enable-garbage-collection`  | Enable periodic garbage collection                                 |
| `mount:garbage-collection-period`         | Interval between periodic GC runs                                  |
| `mount:garbage-collection-cutoff`         | Time threshold for considering inodes stale                        |
| `experimental:enable-pressure-based-gc`   | Run GC when the inode count exceeds a threshold, with an adaptive cutoff |
| `mount:gc-pressure-min-inodes`, `mount:gc-pressure-max-inodes` | Inode counts between which pressure GC ramps up        |
| `mount:gc-cutoff-min-seconds`, `mount:gc-cutoff-max-seconds` | Cutoff range pressure GC interpolates over                |
| `mount:pressure-gc-scan-pins`             | Ask the privhelper which inodes processes hold before reclaiming directories |
| `nfs:num-invalidation-threads`            | Threads sending invalidation chmods per NFS mount (at least one)   |
| `nfs:max-queued-gc-invalidations`         | How many GC chmods may wait in the NFS invalidation queue          |
| `experimental:nfs-gc-stale-reply`         | Answer GC's chmod with a stale handle error (default); off falls back to forgetting after the chmod |

---

## Cancellation and Safety

The GC process supports cancellation at multiple points:

1. **Mount shutdown** - GC exits if the mount is shutting down
2. **External cancellation** - Via `gcCancelSource_.requestCancellation()`
3. **Graceful restart** - `stopAllGarbageCollections()` is called before
   takeover

Cancellation stops new work only. Child walks already started and NFS chmods
already queued are joined, so their inode references are released and the GC
lease is held until they finish, before takeover stops serving NFS requests.

---

## Related Files

- `eden/fs/service/EdenServer.cpp` - GC entry point and scheduling
- `eden/fs/inodes/TreeInode.cpp` - Core GC logic and platform dispatch
- `eden/fs/inodes/TreeInode.h` - TreeInode class declaration
- `eden/fs/inodes/InodeMap.cpp` - Inode tracking and refcount management
- `eden/fs/nfs/Nfsd3.cpp` - NFS invalidation implementation, `InvalidatingInodes`, the stale reply to GC's SETATTR
- `eden/fs/utils/InvalidationQueue.h` - Bounded multi-threaded invalidation queue shared by FUSE and NFS
- `eden/fs/privhelper/PinScan.cpp` - Pin scan of process working directories, fds and mappings
- `eden/fs/inodes/test/NfsGcTest.cpp` - NFS GC tests over a socketpair-attached `Nfsd3`
- `eden/fs/prjfs/PrjfsChannel.cpp` - PrjFS invalidation implementation
