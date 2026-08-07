---
name: ACR-inode-refcount
metadata:
  oncalls: ['scm_client_infra']
  strict: true
  apply_to_path: 'eden/fs/.*\.(cpp|h)$'
  apply_to_content: 'incFsRefcount|decFsRefcount|lookupInode|FUSE_FORGET|fuse_entry_out|NfsDispatcher'
---

# Inode FS Refcount Management

**Severity: CRITICAL**

## What to Look For

- FUSE `lookup`, `create`, `mkdir`, `mknod`, `symlink` handlers that return an inode to the kernel
- NFS handlers that return file handles to the NFS client
- Any code path that returns an inode number to the kernel without incrementing its FS refcount
- Mismatched `incFsRefcount` / `decFsRefcount` calls

## When to Flag

- A FUSE lookup-like operation that returns an inode to the kernel without calling `incFsRefcount()` — the kernel will later send `FUSE_FORGET` to decrement, causing underflow
- A new FUSE handler that creates an inode entry visible to the kernel (populates `fuse_entry_out`) without `incFsRefcount()`
- `decFsRefcount()` called without a matching `incFsRefcount()` or `FUSE_FORGET` path
- Inode returned via NFS `LOOKUP` or `CREATE` without tracking the file handle reference
- Error paths after `incFsRefcount()` that don't call `decFsRefcount()` when the FUSE reply to the kernel fails — the kernel never received the entry and will never send `FUSE_FORGET`, so the refcount is leaked
- `FUSE_BATCH_FORGET` handler that skips entries or miscounts — Linux kernels (5.x+) can batch multiple forget requests

## Do NOT Flag

- `FuseDispatcherImpl::lookup()` and existing create/mkdir/mknod/symlink handlers — these already call `incFsRefcount()` correctly
- `FUSE_FORGET` handler calling `decFsRefcount()` — this is the kernel releasing its reference
- Code that only reads inode attributes without returning the inode to the kernel (e.g., `getattr`, `readdir` without `READDIRPLUS`) — note: `READDIRPLUS` does return inode information and requires refcount increments
- Test code using `TestMount` where kernel refcounting doesn't apply

## Examples

**BAD (lookup returns inode without refcount increment):**
```cpp
ImmediateFuture<fuse_entry_out> MyDispatcher::lookup(
    InodeNumber parent, PathComponentPiece name, const ObjectFetchContextPtr& ctx) {
  return inodeMap_->lookupTreeInode(parent)
      .thenValue([name, ctx = ctx.copy()](auto&& tree) {
        return tree->getOrLoadChild(name, ctx);
      })
      .thenValue([](auto&& inode) {
        auto entry = inode->stat();
        // MISSING: inode->incFsRefcount()
        // Kernel will send FUSE_FORGET later, decrementing below zero
        return entry;
      });
}
```

**GOOD (increment refcount before returning to kernel):**
```cpp
.thenValue([](auto&& inode) {
    auto entry = inode->stat();
    inode->incFsRefcount();
    return entry;
});
```

## Evidence

- Missing `incFsRefcount()` causes refcount underflow when the kernel sends `FUSE_FORGET`, leading to premature inode unloading and use-after-free.
- Error-path refcount leaks cause inodes to never be unloaded, growing memory monotonically.
