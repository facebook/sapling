# EdenFS reference

Oncall: `scm_client_infra` · Language: C++

Use this reference to locate EdenFS components and follow filesystem requests.

## Start here

| Need | Path | Primary symbols |
|------|------|-----------------|
| Process lifecycle | `fbcode/eden/fs/service/` | `EdenServer` |
| Mount lifecycle | `fbcode/eden/fs/inodes/` | `EdenMount` |
| Thrift API and handlers | `fbcode/eden/fs/service/` | `eden.thrift`, `streamingeden.thrift`, `EdenServiceHandler` |
| Inodes and checkout actions | `fbcode/eden/fs/inodes/` | `TreeInode`, `FileInode`, `InodeMap`, `CheckoutAction` |
| Materialized working-copy data | `fbcode/eden/fs/inodes/` | `Overlay`, `InodeCatalog`, `FileContentStore` |
| Object and cache lookup | `fbcode/eden/fs/store/` | `BlobAccess`, `ObjectStore`, `BlobCache`, `TreeCache`, `BackingStore` |
| Sapling and Mononoke fetching | `fbcode/eden/fs/store/sl/` | `SaplingBackingStore` |
| Linux kernel interface | `fbcode/eden/fs/fuse/` | `FuseChannel`, `FuseDispatcher` |
| macOS kernel interface | `fbcode/eden/fs/nfs/` | `Nfsd3`, `NfsDispatcher` |
| Windows kernel interface | `fbcode/eden/fs/prjfs/` | `PrjfsChannel`, `PrjfsDispatcher` |
| Graceful restart | `fbcode/eden/fs/takeover/` | `TakeoverClient`, `TakeoverServer` |
| Watchman change tracking | `fbcode/eden/fs/journal/` | `Journal`, `JournalDelta` |
| Unit-test fixtures | `fbcode/eden/fs/testharness/` | `TestMount`, `FakeBackingStore`, `FakeTreeBuilder` |

## Request model

```text
Kernel VFS
  -> FUSE, NFS, or PrjFS channel
  -> inode lookup
  -> materialized data: overlay
  -> source-controlled data: BlobAccess -> ObjectStore -> BackingStore
```

An inode can be loaded or unloaded independently of whether it is materialized.
`VirtualInode` supports read-only operations without loading the full inode.

For locking, async C++ patterns, Thrift conventions, and test selection, read
`fbcode/eden/fs/.claude/CLAUDE.md` when those details affect the task.
