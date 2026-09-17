# EdenFS Architecture

**Oncall**: `scm_client_infra` · **Language**: C++

## Component Overview

| Component | Path | Key Types |
|-----------|------|-----------|
| Inodes | `inodes/` | TreeInode, FileInode, InodeMap, Overlay, CheckoutAction |
| Object Store | `store/` | ObjectStore, BlobAccess, BlobCache, TreeCache, BackingStore |
| Sapling Backend | `store/sl/` | SaplingBackingStore — Rust FFI bridge to Mononoke |
| FUSE | `fuse/` | FuseChannel, FuseDispatcher, FuseDispatcherImpl (in `inodes/`) — Linux |
| NFS | `nfs/` | Nfsd3, NfsDispatcher, NfsDispatcherImpl (in `inodes/`) — macOS |
| PrjFS | `prjfs/` | PrjfsChannel, PrjfsDispatcher, PrjfsDispatcherImpl (in `inodes/`) — Windows |
| Thrift Service | `service/` | EdenServiceHandler, EdenServer, EdenMain |
| Thrift Defs | `service/` | eden.thrift, streamingeden.thrift |
| Takeover | `takeover/` | TakeoverClient, TakeoverServer — graceful restart |
| PrivHelper | `privhelper/` | PrivHelper, PrivHelperServer — privileged ops |
| Journal | `journal/` | Journal, JournalDelta — change tracking for Watchman |
| Config | `config/` | EdenConfig, CheckoutConfig |
| Model | `model/` | Blob, Tree, ObjectId, Hash20 |
| CLI (Rust) | `cli_rs/` | edenfsctl — replacing Python CLI |
| Test Harness | `testharness/` | TestMount, FakeBackingStore, FakeTreeBuilder |

## Architecture

```
EdenServer (one per process)
  ├─ EdenServiceHandler (Thrift API)
  ├─ BlobCache, TreeCache, BackingStore (shared across mounts)
  └─ EdenMount (per checkout)
       ├─ InodeMap: inode# → loaded InodeBase* or unloaded (parent, name)
       ├─ BlobAccess → ObjectStore → BackingStore chain
       ├─ Overlay: InodeCatalog (dir metadata) + FileContentStore (file data)
       ├─ FsChannel: FUSE / NFS / PrjFS kernel interface
       └─ Journal: file change events for Watchman
```

## Key Concepts

- **Loaded/Unloaded**: Whether an inode exists in memory. InodeMap tracks both.
- **Materialized/Non-materialized**: Whether a file differs from source control. Materialized = stored in Overlay.
- **VirtualInode**: Avoids full inode loads for read-only ops like glob.

## Quick Reference

| Task | Where to Start |
|------|---------------|
| Debug file read | `fuse/FuseChannel.cpp` → `inodes/FileInode.cpp` → `store/ObjectStore.cpp` |
| Debug checkout | `service/EdenServiceHandler.cpp` → `inodes/CheckoutAction.cpp` |
| Debug materialization | `inodes/FileInode.cpp` → `inodes/Overlay.cpp` |
| Add Thrift endpoint | `service/EdenServiceHandler.cpp` + `service/eden.thrift` |
| Add inode operation | `inodes/TreeInode.cpp` or `inodes/FileInode.cpp` |
| Add store operation | `store/ObjectStore.cpp` + `store/BackingStore.h` |

For C++ patterns, testing infrastructure, locking discipline, and build commands, see `eden/fs/.claude/CLAUDE.md`.
